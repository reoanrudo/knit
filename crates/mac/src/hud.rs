//! カーソル近傍に「掴みを断った理由」を出す(I3)。
//!
//! AppKit は**メインスレッドからしか触れない**。呼び出し元は入力経路(CGEventTap)
//! なので、ここでは文言を置いて `gui::request_hud()` でメインスレッドへ投げるだけにし、
//! 生成・表示は `show()`(メインスレッド)で行う。
//!
//! 通知(osascript)は併用して残す: フルスクリーンの別 Space ではパネルが出ない
//! 可能性があるため、**HUD が出ない環境でも無言にしない**(設計 §4 I3 の注意点)
use crate::*;
use std::sync::atomic::{AtomicU64, Ordering};

/// 表示待ちの内容と種類。**断りの説明は進捗より優先する**
/// (同じスロットを取り合うため、種類を持たないと進捗の毎秒更新が断りを消してしまう)
#[derive(Clone, Copy, PartialEq, Eq)]
enum HudKind {
    /// 掴みを断った理由(利用者の操作に直接答える説明)
    Refusal,
    /// 受け取り待ちの進捗(毎秒更新される)
    Progress,
}

/// 表示する文言(tap スレッドが書き、メインスレッドが取り出す)
static PENDING: Mutex<Option<(HudKind, String, u64)>> = Mutex::new(None);

/// 断りの説明を優先する時間。この間は進捗で上書きしない。
/// `PENDING` は表示時に空になるため、**表示後も効く**よう別の時刻でも持つ
const REFUSAL_PRIORITY_MS: u64 = 2_000;
/// 直近に断りを出した時刻(表示して PENDING が空になっても残る)
static LAST_REFUSAL_MS: AtomicU64 = AtomicU64::new(0);
/// パネルとラベル(初回だけ作る)
static PANEL: AtomicU64 = AtomicU64::new(0);
static LABEL: AtomicU64 = AtomicU64::new(0);
/// 直近に表示した時刻(連続で断られたとき、古い予約が新しい表示を消さないようにする)
static SHOWN_AT_MS: AtomicU64 = AtomicU64::new(0);
/// これより短い間隔で来た非表示要求は無視する(表示時間より少し短く)
const HIDE_GRACE_MS: u64 = 1400;

/// パネルの大きさ(1行の理由 + 一言の2行が入る)
const HUD_W: f64 = 360.0;
const HUD_H: f64 = 56.0;

fn sel(name: &std::ffi::CStr) -> SEL {
    unsafe { sel_registerName(name.as_ptr()) }
}

/// 「掴みを断った理由」をカーソル近傍に出す(実際の表示はメインスレッド)
pub(crate) fn note_refusal(reason: &knit_common::drag::Refusal) {
    LAST_REFUSAL_MS.store(now_ms(), Ordering::Relaxed);
    *PENDING.lock().unwrap_or_else(|e| e.into_inner()) = Some((
        HudKind::Refusal,
        format!("{}\n{}", reason.hud_line(), knit_common::hud::HUD_HINT),
        now_ms(),
    ));
    gui::request_hud();
}

/// 受け取り待ちの進捗をカーソル近傍に出す(Win→Mac の「押したまま待つ」に手元の表示)。
/// **この向きでは離すと保存のみになる**ため、断りとは別の一言を添える
pub(crate) fn note_progress(line: &str) {
    // 出したばかりの断りの説明を、毎秒の進捗更新で消さない。
    // 表示済み(PENDING が空)でも効くよう、直近の断り時刻で判定する
    if refusal_has_priority(LAST_REFUSAL_MS.load(Ordering::Relaxed), now_ms()) {
        return;
    }
    let mut g = PENDING.lock().unwrap_or_else(|e| e.into_inner());
    *g = Some((
        HudKind::Progress,
        format!("{line}\n{}", knit_common::hud::PROGRESS_HINT),
        now_ms(),
    ));
    drop(g);
    gui::request_hud();
}

/// メインスレッドのセレクタから呼ぶ: 表示する文言を取り出す
pub(crate) fn take_pending() -> Option<String> {
    PENDING
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take()
        .map(|(_, text, _)| text)
}

/// 断りの説明を優先すべきか(純関数)。出した直後の窓では進捗で上書きしない
fn refusal_has_priority(last_refusal_ms: u64, now: u64) -> bool {
    last_refusal_ms != 0 && now.saturating_sub(last_refusal_ms) < REFUSAL_PRIORITY_MS
}

/// 非表示にしてよいか(純関数)。表示から grace 未満なら「新しい表示を守る」ため隠さない
fn hide_due(shown_ms: u64, now: u64, grace_ms: u64) -> bool {
    shown_ms == 0 || now.saturating_sub(shown_ms) >= grace_ms
}

/// メインスレッドで呼ぶ: 文言をカーソル近傍に出す(**クリック透過**)
pub(crate) unsafe fn show(line: &str) {
    let panel = ensure_panel(line);
    if panel.is_null() {
        return;
    }
    position(panel);
    call0(panel, c"orderFrontRegardless");
    SHOWN_AT_MS.store(now_ms(), Ordering::Relaxed);
}

/// メインスレッドで呼ぶ: 隠す
pub(crate) unsafe fn hide() {
    // 直前に出したばかりなら隠さない(連続で断られたとき、古い予約で新しい案内を消さない)
    if !hide_due(SHOWN_AT_MS.load(Ordering::Relaxed), now_ms(), HIDE_GRACE_MS) {
        return;
    }
    let panel = PANEL.load(Ordering::Relaxed) as ID;
    if !panel.is_null() {
        call1_id(panel, c"orderOut:", std::ptr::null_mut());
    }
}

/// パネルを初回だけ作る(枠なし・非アクティブ・クリック透過・最前面)
unsafe fn ensure_panel(line: &str) -> ID {
    let existing = PANEL.load(Ordering::Relaxed) as ID;
    if !existing.is_null() {
        let label = LABEL.load(Ordering::Relaxed) as ID;
        if !label.is_null() {
            msg1_id(label, sel(c"setStringValue:"), nsstring(line));
        }
        return existing;
    }
    let init: unsafe extern "C" fn(ID, SEL, CGRect, usize, usize, u8) -> ID =
        std::mem::transmute(objc_msgSend as *const ());
    let panel = init(
        msg0(objc_getClass(c"NSPanel".as_ptr()), sel(c"alloc")),
        sel(c"initWithContentRect:styleMask:backing:defer:"),
        CGRect {
            origin: CGPoint { x: 0.0, y: 0.0 },
            size: CGSize { w: HUD_W, h: HUD_H },
        },
        128, // NSWindowStyleMaskNonactivatingPanel(アクティブにしない)
        2,   // NSBackingStoreBuffered
        0,
    );
    if panel.is_null() {
        eprintln!("[hud] NSPanel を作れません");
        return std::ptr::null_mut();
    }
    call1_u8(panel, c"setReleasedWhenClosed:", 0);
    call1_u8(panel, c"setOpaque:", 0);
    // クリックを奪わない(掴んだまま出るため必須。奪うと 1.6 秒間ドロップできない)
    call1_u8(panel, c"setIgnoresMouseEvents:", 1);
    call1_u8(panel, c"setHasShadow:", 1);
    msg1_id(
        panel,
        sel(c"setBackgroundColor:"),
        call1_f64_id(
            msg0(objc_getClass(c"NSColor".as_ptr()), sel(c"blackColor")),
            c"colorWithAlphaComponent:",
            0.78,
        ),
    );
    call1_isize(panel, c"setLevel:", 3); // NSFloatingWindowLevel
    let content = msg0(panel, sel(c"contentView"));
    let alloc = msg0(objc_getClass(c"NSTextField".as_ptr()), sel(c"alloc"));
    let init_with_frame: unsafe extern "C" fn(ID, SEL, CGRect) -> ID =
        std::mem::transmute(objc_msgSend as *const ());
    let label = init_with_frame(
        alloc,
        sel(c"initWithFrame:"),
        CGRect {
            origin: CGPoint { x: 14.0, y: 8.0 },
            size: CGSize {
                w: HUD_W - 28.0,
                h: HUD_H - 16.0,
            },
        },
    );
    if !label.is_null() {
        msg1_id(label, sel(c"setStringValue:"), nsstring(line));
        call1_u8(label, c"setBezeled:", 0);
        call1_u8(label, c"setEditable:", 0);
        call1_u8(label, c"setSelectable:", 0);
        call1_u8(label, c"setDrawsBackground:", 0);
        // **NSTextField に setWraps: は無い**(呼ぶと NSInvalidArgumentException で
        // 例外になる。敵対的検証役が clang -framework AppKit の実機プローブで確定)。
        // 折り返しは cell に対して設定する。1行に収める設定も併せて外す
        call1_u8(label, c"setUsesSingleLineMode:", 0);
        let cell = msg0(label, sel(c"cell"));
        if !cell.is_null() {
            call1_u8(cell, c"setWraps:", 1); // 2行に折り返す
        }
        msg1_id(
            label,
            sel(c"setTextColor:"),
            msg0(objc_getClass(c"NSColor".as_ptr()), sel(c"whiteColor")),
        );
        let font: unsafe extern "C" fn(ID, SEL, f64) -> ID =
            std::mem::transmute(objc_msgSend as *const ());
        msg1_id(
            label,
            sel(c"setFont:"),
            font(objc_getClass(c"NSFont".as_ptr()), sel(c"systemFontOfSize:"), 12.0),
        );
        msg1_id(content, sel(c"addSubview:"), label);
        LABEL.store(label as u64, Ordering::Relaxed);
    }
    PANEL.store(panel as u64, Ordering::Relaxed);
    panel
}

/// カーソルを含む画面の frame を返す。`[NSScreen mainScreen]` はキーウィンドウのある
/// 画面(通常は主画面)なので、**2台目のディスプレイにカーソルがあると HUD が主画面へ
/// 寄せられてしまう**(敵対的検証役の指摘)。見つからなければ主画面へ落とす
unsafe fn cursor_screen(cursor: CGPoint) -> CGRect {
    let frame: unsafe extern "C" fn(ID, SEL) -> CGRect =
        std::mem::transmute(objc_msgSend as *const ());
    let screens = msg0(objc_getClass(c"NSScreen".as_ptr()), sel(c"screens"));
    if !screens.is_null() {
        let count: unsafe extern "C" fn(ID, SEL) -> usize =
            std::mem::transmute(objc_msgSend as *const ());
        let object_at: unsafe extern "C" fn(ID, SEL, usize) -> ID =
            std::mem::transmute(objc_msgSend as *const ());
        for i in 0..count(screens, sel(c"count")) {
            let s = object_at(screens, sel(c"objectAtIndex:"), i);
            if s.is_null() {
                continue;
            }
            let f = frame(s, sel(c"frame"));
            if cursor.x >= f.origin.x
                && cursor.x < f.origin.x + f.size.w
                && cursor.y >= f.origin.y
                && cursor.y < f.origin.y + f.size.h
            {
                return f;
            }
        }
    }
    frame(
        msg0(objc_getClass(c"NSScreen".as_ptr()), sel(c"mainScreen")),
        sel(c"frame"),
    )
}

/// カーソルの近くへ置く(画面外へはみ出さない)。座標系は AppKit と同じ左下原点
unsafe fn position(panel: ID) {
    let point: unsafe extern "C" fn(ID, SEL) -> CGPoint =
        std::mem::transmute(objc_msgSend as *const ());
    let cursor = point(objc_getClass(c"NSEvent".as_ptr()), sel(c"mouseLocation"));
    let f = cursor_screen(cursor);
    let (x, y) = knit_common::hud::place(
        (cursor.x, cursor.y),
        (HUD_W, HUD_H),
        (f.origin.x, f.origin.y, f.size.w, f.size.h),
    );
    let set_frame: unsafe extern "C" fn(ID, SEL, CGRect, u8) =
        std::mem::transmute(objc_msgSend as *const ());
    set_frame(
        panel,
        sel(c"setFrame:display:"),
        CGRect {
            origin: CGPoint { x, y },
            size: CGSize { w: HUD_W, h: HUD_H },
        },
        1,
    );
}

// ---- objc_msgSend の型付き呼び出し(可変引数のまま呼ぶと壊れるため個別に定義) ----
unsafe fn call0(obj: ID, name: &std::ffi::CStr) {
    let f: unsafe extern "C" fn(ID, SEL) = std::mem::transmute(objc_msgSend as *const ());
    f(obj, sel(name));
}

unsafe fn call1_id(obj: ID, name: &std::ffi::CStr, value: ID) {
    let f: unsafe extern "C" fn(ID, SEL, ID) = std::mem::transmute(objc_msgSend as *const ());
    f(obj, sel(name), value);
}

unsafe fn call1_u8(obj: ID, name: &std::ffi::CStr, value: u8) {
    let f: unsafe extern "C" fn(ID, SEL, u8) = std::mem::transmute(objc_msgSend as *const ());
    f(obj, sel(name), value);
}

unsafe fn call1_isize(obj: ID, name: &std::ffi::CStr, value: isize) {
    let f: unsafe extern "C" fn(ID, SEL, isize) = std::mem::transmute(objc_msgSend as *const ());
    f(obj, sel(name), value);
}

unsafe fn call1_f64_id(obj: ID, name: &std::ffi::CStr, value: f64) -> ID {
    let f: unsafe extern "C" fn(ID, SEL, f64) -> ID =
        std::mem::transmute(objc_msgSend as *const ());
    f(obj, sel(name), value)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 表示待ちはプロセス共有の static。テストは直列に走らせる
    /// (並列だと他のテストの `take_pending()` が文言を消費してフレークする: 検証役が実測)
    static HUD_TEST_LOCK: Mutex<()> = Mutex::new(());
    fn lock() -> std::sync::MutexGuard<'static, ()> {
        HUD_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// 連続で断られたときは**最後の1件**だけを出す(古い文言を残さない)
    #[test]
    fn only_the_latest_refusal_is_shown() {
        let _g = lock();
        note_refusal(&knit_common::drag::Refusal::TooMany);
        note_refusal(&knit_common::drag::Refusal::Busy); // 後から来たものが勝つ
        let line = take_pending().expect("要求がある");
        assert!(line.contains("転送中です"), "最後の断り: {line}");
        assert!(!line.contains("掴みすぎ"), "前の断りは残さない: {line}");
        assert!(take_pending().is_none(), "取り出したら消える");
    }

    /// 断りの本文には「そのまま離しても壊れない」、進捗の本文には「離すと保存のみ」を
    /// 示す別の一言が入る(取り違えると逆の案内になる)
    #[test]
    fn refusal_and_progress_use_different_reassurance() {
        let _g = lock();
        note_refusal(&knit_common::drag::Refusal::Busy);
        let refusal = take_pending().expect("断りの要求がある");
        assert!(refusal.contains("転送中です"), "{refusal}");
        assert!(refusal.contains(knit_common::hud::HUD_HINT), "{refusal}");
        // 断りの優先窓を外してから進捗を出す(優先度そのものは別テストで固定)
        LAST_REFUSAL_MS.store(0, Ordering::Relaxed);
        note_progress("受け取り中 42%");
        let progress = take_pending().expect("進捗の要求がある");
        assert!(progress.contains("受け取り中 42%"), "{progress}");
        assert!(progress.contains(knit_common::hud::PROGRESS_HINT), "{progress}");
        assert!(
            !progress.contains(knit_common::hud::HUD_HINT),
            "受け取り待ちで「そのまま離してよい」と言わない: {progress}"
        );
    }

    /// **毎秒の進捗更新が、出したばかりの断りの説明を消さない**(優先度の固定)
    #[test]
    fn progress_does_not_erase_a_fresh_refusal() {
        let _g = lock();
        note_refusal(&knit_common::drag::Refusal::EmptyGrab);
        note_progress("受け取り中 42%");
        let shown = take_pending().expect("断りの説明が出る");
        assert!(shown.contains("掴んだ項目がありません"), "{shown}");
        assert!(!shown.contains("受け取り中"), "{shown}");
        // **表示後も優先**: 断りを消費(PENDING が空)した直後の進捗は出さない
        note_progress("受け取り中 42%");
        assert!(
            take_pending().is_none(),
            "断りの直後の進捗で上書きしない(表示済みでも優先する)"
        );
        // 十分に時間が経てば出てよい(判定は純関数で固定)
        assert!(refusal_has_priority(1_000, 1_500));
        assert!(!refusal_has_priority(1_000, 1_000 + REFUSAL_PRIORITY_MS));
        assert!(!refusal_has_priority(0, 99_999), "断りが無ければ優先しない");
    }

    /// 古い非表示予約が新しい表示を消さない(表示から grace 未満では隠さない)
    #[test]
    fn a_stale_hide_request_does_not_close_a_newer_hud() {
        let _g = lock();
        assert!(!hide_due(1_000, 1_100, HIDE_GRACE_MS), "出した直後は隠さない");
        assert!(hide_due(1_000, 1_000 + HIDE_GRACE_MS, HIDE_GRACE_MS), "時間が来たら隠す");
        assert!(hide_due(0, 99_999, HIDE_GRACE_MS), "未表示なら隠してよい");
    }
}

// メニューバー常駐 GUI(NSStatusItem + NSMenu)。
// objc_msgSend 固定シグネチャ方式(実績パターン#1。依存追加ゼロ)。
// objc2-app-kit への移行はモジュール分割(Wave3 Step6)時に検討する。
// 呼び出し規約: このモジュールの全関数はメインスレッドから呼ぶこと
// (start() は main() の末尾、IMP は AppKit のイベント配信=メインRunLoop)。

use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Mutex;

use crate::{msg0, msg0_cstr, msg0_isize, nsstring, objc_getClass, CGPoint, CGRect, CGSize};

type ID = *mut core::ffi::c_void;
type SEL = *mut core::ffi::c_void;
type CLS = *mut core::ffi::c_void;

mod preferences;
mod prefs;
pub mod setup;
pub static UI_PREVIEW: AtomicBool = AtomicBool::new(false);
pub fn restore_preferences() {
    preferences::restore();
}

/// セレクタ登録の薄いラッパ(extern fn はそのまま import できないため)
unsafe fn sel(name: &std::ffi::CStr) -> SEL {
    crate::sel_registerName(name.as_ptr())
}

#[link(name = "AppKit", kind = "framework")]
#[link(name = "CoreFoundation", kind = "framework")]
#[link(name = "CoreGraphics", kind = "framework")]
#[link(name = "objc", kind = "dylib")]
unsafe extern "C" {
    static kCFRunLoopCommonModes: *mut core::ffi::c_void;
    fn objc_allocateClassPair(
        superclass: CLS,
        name: *const core::ffi::c_char,
        extra_bytes: usize,
    ) -> CLS;
    fn class_addMethod(cls: CLS, name: SEL, imp: usize, types: *const core::ffi::c_char) -> u8;
    fn objc_registerClassPair(cls: CLS);
    // メニューバーアイコンを CoreGraphics で描くための最小セット
    fn CGColorSpaceCreateDeviceRGB() -> *mut core::ffi::c_void;
    fn CGBitmapContextCreate(
        data: *mut u8,
        width: usize,
        height: usize,
        bits_per_component: usize,
        bytes_per_row: usize,
        space: *mut core::ffi::c_void,
        bitmap_info: u32,
    ) -> *mut core::ffi::c_void;
    fn CGBitmapContextCreateImage(ctx: *mut core::ffi::c_void) -> *mut core::ffi::c_void;
    fn CGContextRelease(ctx: *mut core::ffi::c_void);
    fn CGImageRelease(img: *mut core::ffi::c_void);
    fn CGContextSetRGBStrokeColor(ctx: *mut core::ffi::c_void, r: f64, g: f64, b: f64, a: f64);
    fn CGContextSetLineWidth(ctx: *mut core::ffi::c_void, w: f64);
    fn CGContextSetLineCap(ctx: *mut core::ffi::c_void, cap: u32);
    fn CGContextBeginPath(ctx: *mut core::ffi::c_void);
    fn CGContextMoveToPoint(ctx: *mut core::ffi::c_void, x: f64, y: f64);
    fn CGContextStrokePath(ctx: *mut core::ffi::c_void);
    fn CFRelease(cf: *mut core::ffi::c_void);
}

// ---------- 固定シグネチャ呼び出しヘルパ(この画面で必要なものだけ) ----------

unsafe fn msg0_void(target: ID, cmd: SEL) {
    let f: unsafe extern "C" fn(ID, SEL) =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    f(target, cmd)
}
unsafe fn msg1_void_id(target: ID, cmd: SEL, a: ID) {
    let f: unsafe extern "C" fn(ID, SEL, ID) =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    f(target, cmd, a)
}
unsafe fn msg1_void_sel(target: ID, cmd: SEL, a: SEL) {
    let f: unsafe extern "C" fn(ID, SEL, SEL) =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    f(target, cmd, a)
}
unsafe fn msg1_void_u8(target: ID, cmd: SEL, a: u8) {
    let f: unsafe extern "C" fn(ID, SEL, u8) =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    f(target, cmd, a)
}
unsafe fn msg1_void_i64(target: ID, cmd: SEL, a: i64) {
    let f: unsafe extern "C" fn(ID, SEL, i64) =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    f(target, cmd, a)
}
unsafe fn msg2_void_id_id(target: ID, cmd: SEL, a: ID, b: ID) {
    let f: unsafe extern "C" fn(ID, SEL, ID, ID) =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    f(target, cmd, a, b)
}
unsafe fn msg1_id_f64(target: ID, cmd: SEL, a: f64) -> ID {
    let f: unsafe extern "C" fn(ID, SEL, f64) -> ID =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    f(target, cmd, a)
}
unsafe fn msg3_id(target: ID, cmd: SEL, a: ID, b: SEL, c: ID) -> ID {
    let f: unsafe extern "C" fn(ID, SEL, ID, SEL, ID) -> ID =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    f(target, cmd, a, b, c)
}
unsafe fn msg5_timer(target: ID, cmd: SEL, t: f64, a: ID, b: SEL, c: ID, r: u8) -> ID {
    let f: unsafe extern "C" fn(ID, SEL, f64, ID, SEL, ID, u8) -> ID =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    f(target, cmd, t, a, b, c, r)
}

// ---------- GUI 部品への参照(AtomicUsize で生ポインタを保持) ----------

static GUI_TARGET: AtomicUsize = AtomicUsize::new(0);
static GUI_BUTTON: AtomicUsize = AtomicUsize::new(0);
static GUI_STATE_ITEM: AtomicUsize = AtomicUsize::new(0);
static GUI_MODE_ITEM: AtomicUsize = AtomicUsize::new(0);
static GUI_TAPS_ITEM: AtomicUsize = AtomicUsize::new(0);
static GUI_AUDIO_ITEM: AtomicUsize = AtomicUsize::new(0);
static GUI_CMD_ITEM: AtomicUsize = AtomicUsize::new(0);
static GUI_SCROLL_ITEM: AtomicUsize = AtomicUsize::new(0);
static GUI_SPK_ITEM: AtomicUsize = AtomicUsize::new(0);
/// クリップボード履歴のサブメニュー(項目は refresh_status が変化時だけ作り直す)
static GUI_HISTORY_MENU: AtomicUsize = AtomicUsize::new(0);
static GUI_PEERS_MENU: AtomicUsize = AtomicUsize::new(0);
static GUI_PEERS_ITEM: AtomicUsize = AtomicUsize::new(0);
static GUI_PEERS_SIG: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());
/// 変化検知の初期値は最大値にして、起動直後の 1 回目で必ず作り直させる
static GUI_HISTORY_SEEN: AtomicU64 = AtomicU64::new(u64::MAX);
/// 履歴の見出し項目(件数表示を setTitle で更新する)
static GUI_HISTORY_ITEM: AtomicUsize = AtomicUsize::new(0);

// ---------- Search My Desk(ビジョン§14) ----------
static SEARCH_WINDOW: AtomicUsize = AtomicUsize::new(0);
static SEARCH_FIELD: AtomicUsize = AtomicUsize::new(0);
static SEARCH_BUTTONS: [AtomicUsize; 8] = [const { AtomicUsize::new(0) }; 8];
/// 現在の候補(kind, 表示タイトル, 本文)。タイトルは ↑↓ の選択表示の
/// 書き換えに使うため本文と分けて持つ
static SEARCH_HITS: Mutex<Vec<(u8, String, String)>> = Mutex::new(Vec::new());
/// ↑↓ で動く選択位置。クエリが変わったら先頭へ戻す。Enter はこの位置を実行
static SEARCH_SEL: AtomicUsize = AtomicUsize::new(0);
/// 検索窓が開いている間 true。tap が文字キーを横取りする判定に使う(AppKit に
/// 觸れない tap スレッドからも読めるようフラグで管理)
pub static SEARCH_OPEN: AtomicBool = AtomicBool::new(false);

/// 選択位置を 1 つ動かす(端では反対側へ折り返す。Spotlight と同じ挙動)。
/// 候補が無いときは動かさない
pub(crate) fn next_sel(cur: usize, len: usize, down: bool) -> usize {
    if len == 0 {
        return 0;
    }
    if down {
        (cur + 1) % len
    } else {
        cur.checked_sub(1).unwrap_or(len - 1)
    }
}

/// 候補ボタンのタイトルを選択位置に合わせて書き換える(► 前置)。
/// クエリ再計算を伴わないため ↑↓ の反映は軽い
unsafe fn search_highlight_buttons() {
    let hits = SEARCH_HITS.lock().unwrap_or_else(|e| e.into_inner());
    let sel_idx = SEARCH_SEL.load(Ordering::Relaxed);
    for (i, btn_slot) in SEARCH_BUTTONS.iter().enumerate() {
        let btn = btn_slot.load(Ordering::Relaxed) as ID;
        if btn.is_null() {
            continue;
        }
        if let Some((_, title, _)) = hits.get(i) {
            let shown = if i == sel_idx {
                format!("► {title}")
            } else {
                format!("  {title}")
            };
            msg1_void_id(btn, sel(c"setTitle:"), nsstring(&shown));
        }
    }
}

/// ↑↓ キー 1 回分(メインスレッドで実行される)
unsafe fn imp_search_arrow(down: bool) {
    let len = SEARCH_HITS.lock().unwrap_or_else(|e| e.into_inner()).len();
    let cur = SEARCH_SEL.load(Ordering::Relaxed);
    let next = next_sel(cur, len, down);
    SEARCH_SEL.store(next, Ordering::Relaxed);
    search_highlight_buttons();
    let titles = SEARCH_HITS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(next)
        .map(|(_, t, _)| t.clone())
        .unwrap_or_default();
    eprintln!("[search] 選択 #{next}: {titles}");
}

/// tap スレッドから: 検索窓が開いているか
pub fn search_open() -> bool {
    SEARCH_OPEN.load(Ordering::Relaxed)
}

pub fn target_id() -> ID {
    GUI_TARGET.load(Ordering::Relaxed) as ID
}

/// Search My Desk の実機検証(--probe-search)。
/// AppKit の生成・空クエリの候補・絞り込みを 1 回確認して閉じる。
/// 対話セッション(GUI)でのみ成功する(SSH では AppKit が使えない)
pub fn probe_search() -> bool {
    // AppKit のオブジェクトは autorelease pool の内側で作る(無いと
    // 終了時の dealloc で例外が出る=実測)
    crate::with_pool(|| unsafe { probe_search_inner() })
}

unsafe fn probe_search_inner() -> bool {
    // start() が既に target を登録している。同じ名前のクラスは
    // 2 度は作れないため、既存の GUI_TARGET を優先して使う
    let target = match target_id().is_null() {
        false => target_id(),
        true => {
            let t = make_target();
            if t.is_null() {
                eprintln!("[probe-search] target クラスを生成できません");
                return false;
            }
            let _ = GUI_TARGET.store(t as usize, Ordering::Relaxed);
            t
        }
    };
    let _ = target;
    if !build_search_window() {
        eprintln!("[probe-search] 検索窓(NSPanel/入力/8 ボタン)の生成に失敗");
        return false;
    }
    let field = SEARCH_FIELD.load(Ordering::Relaxed) as ID;
    let window = SEARCH_WINDOW.load(Ordering::Relaxed) as ID;
    let buttons = SEARCH_BUTTONS
        .iter()
        .filter(|b| b.load(Ordering::Relaxed) != 0)
        .count();
    refresh_search_results("");
    let initial = SEARCH_HITS.lock().map(|h| h.len()).unwrap_or(0);
    let ok =
        !field.is_null() && !window.is_null() && buttons == SEARCH_BUTTONS.len() && initial > 0;
    if ok {
        refresh_search_results("term");
        let narrowed = SEARCH_HITS.lock().map(|h| h.len()).unwrap_or(0);
        eprintln!(
            "[probe-search] 窓/入力/8 ボタン OK、初期候補 {initial} 件、絞り込み後 {narrowed} 件"
        );
    }
    // orderOut は表示中の窓だけに限る(非表示窓への orderOut は
    // WindowServer 未接続の環境で例外になった=実測)
    if !window.is_null() && msg0_isize(window, sel(c"isVisible")) != 0 {
        msg1_void_id(window, sel(c"orderOut:"), std::ptr::null_mut());
    }
    ok
}

// ---------- 設定ウィンドウ(メニュー「設定…」で開く) ----------
static PREFS_WIN: AtomicUsize = AtomicUsize::new(0);
static PREFS_CHK_MODE: AtomicUsize = AtomicUsize::new(0);
static PREFS_CHK_TAPS: AtomicUsize = AtomicUsize::new(0);
static PREFS_CHK_AUDIO: AtomicUsize = AtomicUsize::new(0);
static PREFS_CHK_CMD: AtomicUsize = AtomicUsize::new(0);
static PREFS_CHK_SCROLL: AtomicUsize = AtomicUsize::new(0);
static PREFS_CHK_SPK: AtomicUsize = AtomicUsize::new(0);
static PREFS_SLIDER: AtomicUsize = AtomicUsize::new(0);
static PREFS_GAIN_LABEL: AtomicUsize = AtomicUsize::new(0);
static PREFS_SIDE_POP: AtomicUsize = AtomicUsize::new(0);
static PREFS_DELAY_SLIDER: AtomicUsize = AtomicUsize::new(0);
static PREFS_DELAY_LBL: AtomicUsize = AtomicUsize::new(0);
static PREFS_DBL_SLIDER: AtomicUsize = AtomicUsize::new(0);
static PREFS_DBL_LBL: AtomicUsize = AtomicUsize::new(0);
static PREFS_MSCALE_SLIDER: AtomicUsize = AtomicUsize::new(0);
static PREFS_MSCALE_LBL: AtomicUsize = AtomicUsize::new(0);
static PREFS_EDGE_LBL: AtomicUsize = AtomicUsize::new(0);
static PREFS_CHK_CLIP: AtomicUsize = AtomicUsize::new(0);
static PREFS_CHK_SCOMPAT: AtomicUsize = AtomicUsize::new(0);
static GUI_SIDE_ITEM: AtomicUsize = AtomicUsize::new(0);
static PREFS_STATE: AtomicUsize = AtomicUsize::new(0);
/// 起動直後に設定ウィンドウを開く(--show-prefs。1 秒タイマーの初回で処理)
pub static SHOW_AT_START: AtomicBool = AtomicBool::new(false);

/// NSRect(f64 x4)。戻り値受け取りにのみ使う(渡しは rect_args を使う)
#[repr(C)]
#[derive(Clone, Copy)]
struct NSRect {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

// ---------- メニュー項目のアクション(Objective-C クラスの IMP) ----------

/// スクロール速度スライダー(値=除数。小さいほど速い)。ドラッグ中も連続で飛ぶ
unsafe extern "C" fn imp_scroll_gain(_s: ID, _c: SEL, sender: ID) {
    unsafe {
        let get: unsafe extern "C" fn(ID, SEL) -> f64 =
            std::mem::transmute(crate::objc_msgSend as *const () as usize);
        let v = get(sender, sel(c"doubleValue"));
        crate::set_scroll_div(v);
        let lbl = PREFS_GAIN_LABEL.load(Ordering::Relaxed) as ID;
        if !lbl.is_null() {
            let speed = if v <= 40.0 {
                "速い"
            } else if v >= 140.0 {
                "遅い"
            } else {
                "標準"
            };
            msg1_void_id(
                lbl,
                sel(c"setStringValue:"),
                nsstring(&format!("スクロール速度: {speed}({v:.0})")),
            );
        }
    }
    preferences::save();
}

unsafe extern "C" fn imp_switch_mode(_s: ID, _c: SEL, _n: ID) {
    let next = !crate::HOTKEY_ONLY.load(Ordering::Relaxed);
    crate::HOTKEY_ONLY.store(next, Ordering::Relaxed);
    eprintln!(
        "[mode] switch_mode -> {}",
        if next { "hotkey(ロック)" } else { "edge" }
    );
    refresh_status();
    preferences::save();
}
unsafe extern "C" fn imp_edge_taps(_s: ID, _c: SEL, _n: ID) {
    let next = if crate::EDGE_TAPS.load(Ordering::Relaxed) >= 2 {
        1
    } else {
        2
    };
    crate::EDGE_TAPS.store(next, Ordering::Relaxed);
    eprintln!("[mode] edge_taps -> {next}");
    refresh_status();
    preferences::save();
}
unsafe extern "C" fn imp_open_log(_s: ID, _c: SEL, _n: ID) {
    let _ = std::process::Command::new("open")
        .args(["-a", "Console", "/tmp/tsunagu-mac.log"])
        .spawn();
}

/// 履歴メニューの「消す」クリック。本文は不要(メニュー操作で即反映)
unsafe extern "C" fn imp_history_clear(_s: ID, _c: SEL, _sender: ID) {
    crate::history_clear();
    crate::HISTORY_LAST_ID.store(0, Ordering::Relaxed);
}

// ---------- Search My Desk(ビジョン§14) ----------

/// 検索窓を(無ければ作って)開く。表示中なら閉じる(トグル)。
/// メインスレッドからのみ呼ぶ(performSelectorOnMainThread 経由)
unsafe extern "C" fn imp_show_search(_s: ID, _c: SEL, _n: ID) {
    let window = SEARCH_WINDOW.load(Ordering::Relaxed) as ID;
    if window.is_null() && !build_search_window() {
        eprintln!("[search] 検索窓を生成できませんでした");
        return;
    }
    let window = SEARCH_WINDOW.load(Ordering::Relaxed) as ID;
    let field = SEARCH_FIELD.load(Ordering::Relaxed) as ID;
    if msg0_isize(window, sel(c"isVisible")) != 0 {
        eprintln!("[search] 検索窓を閉じます(トグル)");
        msg1_void_id(window, sel(c"orderOut:"), std::ptr::null_mut());
        SEARCH_OPEN.store(false, Ordering::Relaxed);
        return;
    }
    // 開くたびに候補を初期化し、Windows 側のアプリ一覧も問い合わせる
    //(応答は非同期。届いたら sdSearchRefresh: で絞り込みをやり直す)
    msg1_void_id(field, sel(c"setStringValue:"), nsstring(""));
    refresh_search_results("");
    crate::send_msg(&crate::Msg::AppsQuery);
    // デスクのファイル/フォルダ索引を裏で更新する(10 分キャッシュ。初回は
    // 空のまま出て、索引が揃った次回の絞り込みから候補に混ざる)
    std::thread::spawn(|| crate::refresh_desk_files());
    // 開いている間は tap が文字キーを横取りして直接 field へ積む(IME を
    // 通さない=Spotlight 型。inputContext は get-only で無効化できないため)
    SEARCH_OPEN.store(true, Ordering::Relaxed);
    // 前回 closed 状態で握り損ねた up の残りを消す(詳細は main.rs の宣言コメント)
    crate::SEARCH_ENTER_DOWN.store(false, Ordering::Relaxed);
    crate::SEARCH_ESC_DOWN.store(false, Ordering::Relaxed);
    // カーソルがある画面の中央へ(Spotlight の体感。多画面で見えない場所に
    // 出ないようにする)
    let (x, y) = crate::cursor_screen_center_appkit(520.0, 344.0);
    let set_origin: unsafe extern "C" fn(ID, SEL, f64, f64) =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    set_origin(window, sel(c"setFrameOrigin:"), x, y);
    let app = msg0(
        objc_getClass(c"NSApplication".as_ptr()),
        sel(c"sharedApplication"),
    );
    msg1_void_u8(app, sel(c"activateIgnoringOtherApps:"), 1);
    // macOS 14+ では旧 API(activateIgnoringOtherApps:)が効かないことがあるため
    // モダンな activate() も併用する(key になれないと入力欄に打てない)
    msg0_void(app, sel(c"activate"));
    msg1_void_id(window, sel(c"makeKeyAndOrderFront:"), std::ptr::null_mut());
    msg0_void(window, sel(c"makeKeyWindow")); // 引数なし(makeKeyWindow: は実在しない)
    msg1_void_id(window, sel(c"makeFirstResponder:"), field);
    // key 化の成否を記録する(accessory 常駐アプリは activation が拒否されて
    // 入力欄に打てないことがあるため、実機診断の鍵になる)
    let keywin = msg0(app, sel(c"keyWindow"));
    eprintln!(
        "[search] 検索窓を開きました(visible={}) keyWindow一致={}",
        msg0_isize(window, sel(c"isVisible")),
        keywin == window
    );
}

/// Windows 側アプリ一覧が届いたときの再絞り込み(メインスレッドから呼ばれる)
unsafe extern "C" fn imp_search_refresh(_s: ID, _c: SEL, _n: ID) {
    let window = SEARCH_WINDOW.load(Ordering::Relaxed) as ID;
    let field = SEARCH_FIELD.load(Ordering::Relaxed) as ID;
    if window.is_null() || field.is_null() {
        return;
    }
    if msg0_isize(window, sel(c"isVisible")) == 0 {
        return;
    }
    let value = msg0(field, sel(c"objectValue"));
    let utf8 = if value.is_null() {
        std::ptr::null()
    } else {
        msg0_cstr(value, sel(c"UTF8String"))
    };
    let query = if utf8.is_null() {
        String::new()
    } else {
        std::ffi::CStr::from_ptr(utf8)
            .to_string_lossy()
            .into_owned()
    };
    refresh_search_results(&query);
}

/// tap から渡された文字を入力欄へ積む(IMO を通さない直接入力)。
/// tap 側で kc→文字に変換済みの文字列を受け取る
unsafe extern "C" fn imp_search_char(_s: ID, _c: SEL, text: ID) {
    let field = SEARCH_FIELD.load(Ordering::Relaxed) as ID;
    if field.is_null() || text.is_null() {
        return;
    }
    let cur = msg0(field, sel(c"stringValue"));
    let joined = if cur.is_null() {
        text
    } else {
        crate::msg1_id(cur, sel(c"stringByAppendingString:"), text)
    };
    msg1_void_id(field, sel(c"setStringValue:"), joined);
    let utf8 = msg0_cstr(joined, sel(c"UTF8String"));
    let query = if utf8.is_null() {
        String::new()
    } else {
        std::ffi::CStr::from_ptr(utf8)
            .to_string_lossy()
            .into_owned()
    };
    eprintln!("[search] 直接入力「{query}」で絞り込み(tap 経由)");
    refresh_search_results(&query);
}

/// tap から: 入力欄の末尾 1 文字を削除(Backspace)
unsafe extern "C" fn imp_search_backspace(_s: ID, _c: SEL, _n: ID) {
    let field = SEARCH_FIELD.load(Ordering::Relaxed) as ID;
    if field.is_null() {
        return;
    }
    let cur = msg0(field, sel(c"stringValue"));
    let utf8 = if cur.is_null() {
        std::ptr::null()
    } else {
        msg0_cstr(cur, sel(c"UTF8String"))
    };
    if utf8.is_null() {
        return;
    }
    let mut query = std::ffi::CStr::from_ptr(utf8)
        .to_string_lossy()
        .into_owned();
    if query.pop().is_none() {
        return;
    }
    msg1_void_id(field, sel(c"setStringValue:"), nsstring(&query));
    refresh_search_results(&query);
}

/// tap スレッドから: 1 文字をメインスレッドで field へ積むよう依頼する
pub fn dispatch_search_text(text: &str) {
    unsafe {
        let target = target_id();
        if target.is_null() {
            return;
        }
        let f: unsafe extern "C" fn(ID, SEL, SEL, ID, u8) =
            std::mem::transmute(crate::objc_msgSend as *const () as usize);
        f(
            target,
            crate::sel_registerName(
                c"performSelectorOnMainThread:withObject:waitUntilDone:".as_ptr(),
            ),
            crate::sel_registerName(c"sdSearchChar:".as_ptr()),
            nsstring(text),
            0,
        );
    }
}

/// tap スレッドから: Backspace(1 文字削除)を依頼する
pub fn dispatch_search_backspace() {
    unsafe {
        let target = target_id();
        if target.is_null() {
            return;
        }
        let f: unsafe extern "C" fn(ID, SEL, SEL, ID, u8) =
            std::mem::transmute(crate::objc_msgSend as *const () as usize);
        f(
            target,
            crate::sel_registerName(
                c"performSelectorOnMainThread:withObject:waitUntilDone:".as_ptr(),
            ),
            crate::sel_registerName(c"sdSearchBackspace:".as_ptr()),
            std::ptr::null_mut(),
            0,
        );
    }
}

/// tap スレッドから: Enter(選択候補を実行)を依頼する
pub fn dispatch_search_enter() {
    unsafe {
        let target = target_id();
        if target.is_null() {
            return;
        }
        let f: unsafe extern "C" fn(ID, SEL, SEL, ID, u8) =
            std::mem::transmute(crate::objc_msgSend as *const () as usize);
        f(
            target,
            crate::sel_registerName(
                c"performSelectorOnMainThread:withObject:waitUntilDone:".as_ptr(),
            ),
            crate::sel_registerName(c"sdSearchGo:".as_ptr()),
            std::ptr::null_mut(),
            0,
        );
    }
}

/// tap スレッドから: ↑↓(選択を 1 つ動かす)を依頼する
pub fn dispatch_search_arrow(down: bool) {
    let name = if down {
        c"sdSearchArrowDown:"
    } else {
        c"sdSearchArrowUp:"
    };
    unsafe {
        let target = target_id();
        if target.is_null() {
            return;
        }
        let f: unsafe extern "C" fn(ID, SEL, SEL, ID, u8) =
            std::mem::transmute(crate::objc_msgSend as *const () as usize);
        f(
            target,
            crate::sel_registerName(
                c"performSelectorOnMainThread:withObject:waitUntilDone:".as_ptr(),
            ),
            crate::sel_registerName(name.as_ptr()),
            std::ptr::null_mut(),
            0,
        );
    }
}

/// sdSearchArrowDown: / sdSearchArrowUp: の受け口
unsafe extern "C" fn imp_search_arrow_down(_s: ID, _c: SEL, _n: ID) {
    imp_search_arrow(true);
}
unsafe extern "C" fn imp_search_arrow_up(_s: ID, _c: SEL, _n: ID) {
    imp_search_arrow(false);
}

/// 入力のたびに候補を絞り直す(NSTextField の delegate)
unsafe extern "C" fn imp_control_text_did_change(_s: ID, _c: SEL, _note: ID) {
    let field = SEARCH_FIELD.load(Ordering::Relaxed) as ID;
    if field.is_null() {
        return;
    }
    let value = msg0(field, sel(c"objectValue"));
    let utf8 = if value.is_null() {
        std::ptr::null()
    } else {
        msg0_cstr(value, sel(c"UTF8String"))
    };
    let query = if utf8.is_null() {
        String::new()
    } else {
        std::ffi::CStr::from_ptr(utf8)
            .to_string_lossy()
            .into_owned()
    };
    eprintln!("[search] 入力「{query}」で絞り込み");
    refresh_search_results(&query);
}

/// 候補のクリック(タグ=index+1)
unsafe extern "C" fn imp_search_pick(_s: ID, _c: SEL, sender: ID) {
    if sender.is_null() {
        return;
    }
    let tag = msg0_isize(sender, sel(c"tag"));
    run_search_hit(tag.max(1) as usize - 1, false);
}

/// Enter(入力欄の action)= 選択位置の候補を実行。⌥Enter は候補を Windows へ投げる
unsafe extern "C" fn imp_search_go(_s: ID, _c: SEL, _sender: ID) {
    let throw = crate::SEARCH_ENTER_OPT.swap(false, Ordering::Relaxed);
    let sel = SEARCH_SEL.load(Ordering::Relaxed);
    eprintln!(
        "[search] Enter 受付: 選択候補 #{sel} を実行します{}",
        if throw {
            "(⌥: Windows へ投げます)"
        } else {
            ""
        }
    );
    run_search_hit(sel, throw);
}

/// kind: 0=アプリ/1=履歴/2=URL/3=Windows アプリ/4=ファイル/5=コマンド。
/// throw は ⌥Enter(候補を Windows へ投げる=ビジョン§13 のファイル版)
unsafe fn run_search_hit(index: usize, throw: bool) {
    let hit = SEARCH_HITS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(index)
        .map(|(k, _, t)| (*k, t.clone()));
    let Some((kind, text)) = hit else {
        eprintln!("[search] 候補が空のため実行を中止(#{index})");
        return;
    };
    match kind {
        0 | 4 => {
            if kind == 4 && throw {
                // ファイル/フォルダ候補を Windows へ投げる(Throw)。送信経路は
                // Finder の ⌘C 同期と同じ(FileBegin→FileEnd、Ctrl+V で貼り付け可)
                eprintln!("[search] ファイルを Windows へ投げます: {text}");
                crate::send_files_to_win(vec![std::path::PathBuf::from(&text)], false);
            } else {
                // 既定アプリで開く(.app は NSWorkspace が起動、フォルダは Finder)
                let url = crate::msg1_id(
                    objc_getClass(c"NSURL".as_ptr()),
                    crate::sel_registerName(c"fileURLWithPath:".as_ptr()),
                    nsstring(&text),
                );
                let ws = msg0(
                    objc_getClass(c"NSWorkspace".as_ptr()),
                    sel(c"sharedWorkspace"),
                );
                let open: unsafe extern "C" fn(ID, crate::SEL, ID) -> u8 =
                    std::mem::transmute(crate::objc_msgSend as *const () as usize);
                let _ = open(ws, sel(c"openURL:"), url);
                eprintln!(
                    "[search] {}: {text}",
                    if kind == 4 {
                        "ファイル/フォルダを開く"
                    } else {
                        "アプリを起動"
                    }
                );
            }
        }
        1 => crate::history_restore(text),
        2 => {
            // URL は相手 PC(Windows)の既定ブラウザで開く(Continue Here と同じ経路)
            if crate::send_msg_reported(&crate::Msg::OpenUrl { url: text.clone() }) {
                eprintln!("[search] Windows で開くよう送信: {text}");
            } else {
                crate::notify("Tsunagu", "未接続のため Windows で開けませんでした");
            }
        }
        3 => {
            // Windows アプリの起動。受け側は列挙済みパスと完全一致だけ実行する
            if crate::send_msg_reported(&crate::Msg::RunApp { path: text.clone() }) {
                eprintln!("[search] Windows へ起動指示: {text}");
            } else {
                crate::notify("Tsunagu", "未接続のため Windows で起動できませんでした");
            }
        }
        5 => crate::run_desk_command(&text),
        _ => {}
    }
    let window = SEARCH_WINDOW.load(Ordering::Relaxed) as ID;
    if !window.is_null() {
        msg1_void_id(window, sel(c"orderOut:"), std::ptr::null_mut());
    }
    SEARCH_OPEN.store(false, Ordering::Relaxed);
}

unsafe fn refresh_search_results(query: &str) {
    // 候補 1 行あたりの内部表現(kind: 0=アプリ/1=履歴/2=URL/3=Windows アプリ/
    // 4=ファイル/5=コマンド)
    struct Row {
        kind: u8,
        title: String,
        text: String,
    }
    let apps = crate::list_apps();
    let files = crate::DESK_FILES
        .lock()
        .map(|f| f.clone())
        .unwrap_or_default();
    let commands = crate::desk_commands();
    let entries: Vec<(u64, String)> = crate::HISTORY
        .lock()
        .map(|h| h.entries().iter().map(|e| (e.id, e.text.clone())).collect())
        .unwrap_or_default();
    let mut rows: Vec<Row> = tsunagu_common::desksearch::search(
        query,
        &apps,
        &files,
        &commands,
        &entries,
        SEARCH_BUTTONS.len(),
    )
    .into_iter()
    .map(|h| {
        let kind = match h.kind {
            tsunagu_common::desksearch::Kind::App => 0u8,
            tsunagu_common::desksearch::Kind::History => 1,
            tsunagu_common::desksearch::Kind::Url => 2,
            tsunagu_common::desksearch::Kind::File => 4,
            tsunagu_common::desksearch::Kind::Cmd => 5,
        };
        Row {
            kind,
            title: h.title,
            text: h.text,
        }
    })
    .collect();
    // Windows 側のアプリを同じ枠へ混ぜる(どちらの PC かはタイトル前置で分かる)。
    // 残り枠だけ使うため、手元の候補が優先される。
    // App の Hit だけを混ぜる: search は URL クエリで kind=Url の行も返すため
    // そのまま足すと Mac 側で出した URL 行と重複する(実測)
    if rows.len() < SEARCH_BUTTONS.len() {
        let win_apps = crate::WIN_APPS
            .lock()
            .map(|a| a.clone())
            .unwrap_or_default();
        if !win_apps.is_empty() {
            let remain = SEARCH_BUTTONS.len() - rows.len();
            for h in tsunagu_common::desksearch::search(query, &win_apps, &[], &[], &[], remain) {
                if !matches!(h.kind, tsunagu_common::desksearch::Kind::App) {
                    continue;
                }
                rows.push(Row {
                    kind: 3,
                    title: format!("Windows・{}", h.title),
                    text: h.text,
                });
            }
        }
    }
    rows.truncate(SEARCH_BUTTONS.len());
    *SEARCH_HITS.lock().unwrap_or_else(|e| e.into_inner()) = rows
        .iter()
        .map(|r| (r.kind, r.title.clone(), r.text.clone()))
        .collect();
    // クエリが変わったので選択は先頭へ戻す(以前の選択位置が新しい候補数を
    // 超えている事故も防ぐ)。タイトルの描画は search_highlight_buttons が
    // 選択位置込みで行うため、ここでは表示/非表示だけ切り替える
    SEARCH_SEL.store(0, Ordering::Relaxed);
    for (i, btn_slot) in SEARCH_BUTTONS.iter().enumerate() {
        let btn = btn_slot.load(Ordering::Relaxed) as ID;
        if btn.is_null() {
            continue;
        }
        match rows.get(i) {
            Some(_) => msg1_void_u8(btn, sel(c"setHidden:"), 0),
            None => msg1_void_u8(btn, sel(c"setHidden:"), 1),
        }
    }
    search_highlight_buttons();
    // 候補の実機検証用ログ(上位 3 件。0=Mac アプリ/1=履歴/2=URL/3=Win アプリ/
    // 4=ファイル/5=コマンド)
    if !rows.is_empty() {
        let top: Vec<String> = rows
            .iter()
            .take(3)
            .map(|r| format!("[{}]", r.title))
            .collect();
        eprintln!("[search] 候補{}件: {}", rows.len(), top.join(" "));
    }
}

/// 検索窓を 1 回だけ組み立てる(以後は再利用)。
/// Cocoa 座標は左下原点: 上に入力、下に候補ボタン 8 件を並べる
unsafe fn build_search_window() -> bool {
    let frame = CGRect {
        origin: CGPoint { x: 0.0, y: 0.0 },
        size: CGSize { w: 520.0, h: 344.0 },
    };
    let init: unsafe extern "C" fn(ID, SEL, CGRect, usize, usize, u8) -> ID =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    let panel = init(
        msg0(objc_getClass(c"NSPanel".as_ptr()), sel(c"alloc")),
        sel(c"initWithContentRect:styleMask:backing:defer:"),
        frame,
        // Titled | Closable | UtilityWindow | NonactivatingPanel(0x80)。
        // NonactivatingPanel は常駐(accessory)アプリの窓をアプリのアクティブ化
        // なしで key にできる(Spotlight 型ランチャーの定石)。無いと
        // 他アプリが前面の間に入力欄へキーが届かない(実測)
        1 | 2 | 16 | (1 << 7),
        2,
        0,
    );
    if panel.is_null() {
        return false;
    }
    msg1_void_u8(panel, sel(c"setReleasedWhenClosed:"), 0);
    // NSPanel は既定で hidesOnDeactivate=YES のため、別アプリが前面に来た瞬間
    // 窓が勝手に隠れて isVisible==0 になり、トグル閉鎖と再オープンが壊れる(実測)。
    // ランチャー窓は他アプリの上に開いたまま残るべきなので無効化する
    msg1_void_u8(panel, sel(c"setHidesOnDeactivate:"), 0);
    msg1_void_id(panel, sel(c"setTitle:"), nsstring("Search My Desk"));
    // 位置は開くたびに imp_show_search がカーソル画面の中央へ置く(center は
    // main 以外の画面を見ているときに窓が見えない場所へ出ることがある=実測)
    let content = msg0(panel, sel(c"contentView"));
    if content.is_null() {
        return false;
    }
    let target = target_id();
    if target.is_null() {
        return false;
    }
    let subview: unsafe extern "C" fn(ID, SEL, CGRect) -> ID =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    // 入力欄
    let field = subview(
        msg0(objc_getClass(c"NSTextField".as_ptr()), sel(c"alloc")),
        sel(c"initWithFrame:"),
        CGRect {
            origin: CGPoint { x: 16.0, y: 296.0 },
            size: CGSize { w: 488.0, h: 30.0 },
        },
    );
    if field.is_null() {
        return false;
    }
    msg1_void_id(content, sel(c"addSubview:"), field);
    msg1_void_id(field, sel(c"setTarget:"), target);
    msg1_void_sel(field, sel(c"setAction:"), sel(c"sdSearchGo:"));
    msg1_void_id(field, sel(c"setDelegate:"), target);
    msg1_void_id(
        field,
        sel(c"setPlaceholderString:"),
        nsstring("アプリ・履歴・URL を検索(Enter で先頭候補を実行)"),
    );
    // 候補ボタン 8 件
    for i in 0..SEARCH_BUTTONS.len() {
        let y = 256.0 - i as f64 * 34.0;
        let btn = subview(
            msg0(objc_getClass(c"NSButton".as_ptr()), sel(c"alloc")),
            sel(c"initWithFrame:"),
            CGRect {
                origin: CGPoint { x: 16.0, y },
                size: CGSize { w: 488.0, h: 30.0 },
            },
        );
        if btn.is_null() {
            continue;
        }
        msg1_void_id(content, sel(c"addSubview:"), btn);
        msg1_void_id(btn, sel(c"setTarget:"), target);
        msg1_void_sel(btn, sel(c"setAction:"), sel(c"sdSearchPick:"));
        msg1_void_i64(btn, sel(c"setTag:"), i as i64 + 1);
        msg1_void_id(btn, sel(c"setTitle:"), nsstring(""));
        msg1_void_u8(btn, sel(c"setHidden:"), 1);
        SEARCH_BUTTONS[i].store(btn as usize, Ordering::Relaxed);
    }
    SEARCH_FIELD.store(field as usize, Ordering::Relaxed);
    SEARCH_WINDOW.store(panel as usize, Ordering::Relaxed);
    true
}

/// クリップボード履歴メニューの項目クリック。representedObject(NSString)から
/// 本文を取り出して Mac のクリップボードへ復元する
unsafe extern "C" fn imp_history_restore(_s: ID, _c: SEL, sender: ID) {
    if sender.is_null() {
        return;
    }
    let obj = msg0(sender, sel(c"representedObject"));
    if obj.is_null() {
        return;
    }
    let utf8 = crate::msg0_cstr(obj, sel(c"UTF8String"));
    if utf8.is_null() {
        return;
    }
    let text = std::ffi::CStr::from_ptr(utf8)
        .to_string_lossy()
        .into_owned();
    if !text.is_empty() {
        crate::history_restore(text);
    }
}
unsafe extern "C" fn imp_restart(_s: ID, _c: SEL, _n: ID) {
    if UI_PREVIEW.load(Ordering::Relaxed) {
        return;
    }
    // 終了と同じ理由で、再起動(スクリプトが自分を kill する)の前にも
    // 正規の leave を経由させる
    if crate::WIN_MODE.swap(false, Ordering::Relaxed) {
        crate::leave_win_mode_cursor_unlock(None);
    }
    match restart_script() {
        Some(script) => {
            eprintln!("[gui] restart-mac.sh を起動します");
            let _ = std::process::Command::new("bash")
                .arg(script)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn();
        }
        None => {
            eprintln!("[gui] 再起動スクリプトが見つかりません(.app 配布時は終了後に LaunchAgent が再起動します)");
            crate::notify("tsunagu", "再起動スクリプトが見つかりません");
        }
    }
}
unsafe extern "C" fn imp_audio_toggle(_s: ID, _c: SEL, _n: ID) {
    let next = !crate::audio::MUTED.load(Ordering::Relaxed);
    crate::audio::MUTED.store(next, Ordering::Relaxed);
    eprintln!("[audio] mute -> {next}");
    refresh_status();
    preferences::save();
}
unsafe extern "C" fn imp_cmd_map(_s: ID, _c: SEL, _n: ID) {
    let next = !crate::CMD_ALT.load(Ordering::Relaxed);
    crate::CMD_ALT.store(next, Ordering::Relaxed);
    eprintln!("[cfg] ⌘キー -> {}", if next { "Alt" } else { "Ctrl" });
    crate::send_cfg();
    refresh_status();
    preferences::save();
}
unsafe extern "C" fn imp_spk_mute(_s: ID, _c: SEL, _n: ID) {
    let next = !crate::SPK_MUTE.load(Ordering::Relaxed);
    crate::SPK_MUTE.store(next, Ordering::Relaxed);
    eprintln!(
        "[cfg] 接続中スピーカーミュート -> {}",
        if next { "ON" } else { "OFF" }
    );
    crate::send_cfg();
    refresh_status();
    preferences::save();
}
/// 「Windows の位置」ポップアップ(0=右/1=左/2=上/3=下)。Deskflow links 相当
unsafe extern "C" fn imp_side(_s: ID, _c: SEL, sender: ID) {
    unsafe {
        let get: unsafe extern "C" fn(ID, SEL) -> isize =
            std::mem::transmute(crate::objc_msgSend as *const () as usize);
        let idx = get(sender, sel(c"indexOfSelectedItem"));
        crate::set_side(idx.clamp(0, 7) as u8);
        *LAY_WIN.lock().unwrap_or_else(|e| e.into_inner()) = (0.0, 0.0);
    }
    preferences::save();
}

/// switchDelay スライダ(0=無効。端に N ms 滞ってから切替)
unsafe extern "C" fn imp_switch_delay(_s: ID, _c: SEL, sender: ID) {
    unsafe {
        let get: unsafe extern "C" fn(ID, SEL) -> f64 =
            std::mem::transmute(crate::objc_msgSend as *const () as usize);
        let v = get(sender, sel(c"doubleValue"));
        crate::SWITCH_DELAY_MS.store(v as u64, Ordering::Relaxed);
        let lbl = PREFS_DELAY_LBL.load(Ordering::Relaxed) as ID;
        if !lbl.is_null() {
            let t = if v < 1.0 {
                "無効(即時/ダブルタップ)"
            } else {
                &format!("{v:.0}ms 滞って切替")
            };
            msg1_void_id(lbl, sel(c"setStringValue:"), nsstring(t));
        }
    }
    preferences::save();
}

/// switchDoubleTap スライダ(ダブルタップ判定窓 ms)
unsafe extern "C" fn imp_dbl_tap(_s: ID, _c: SEL, sender: ID) {
    unsafe {
        let get: unsafe extern "C" fn(ID, SEL) -> f64 =
            std::mem::transmute(crate::objc_msgSend as *const () as usize);
        let v = get(sender, sel(c"doubleValue"));
        crate::DOUBLE_TAP_MS.store(v.max(100.0) as u64, Ordering::Relaxed);
        let lbl = PREFS_DBL_LBL.load(Ordering::Relaxed) as ID;
        if !lbl.is_null() {
            msg1_void_id(
                lbl,
                sel(c"setStringValue:"),
                nsstring(&format!("{v:.0}ms 以内の2回")),
            );
        }
    }
    preferences::save();
}

/// カーソル速度スライダ(0.2..3.0。倍率=Windows 上の移動量)
unsafe extern "C" fn imp_mouse_scale(_s: ID, _c: SEL, sender: ID) {
    unsafe {
        let get: unsafe extern "C" fn(ID, SEL) -> f64 =
            std::mem::transmute(crate::objc_msgSend as *const () as usize);
        let v = get(sender, sel(c"doubleValue"));
        crate::set_mouse_scale(v);
        let lbl = PREFS_MSCALE_LBL.load(Ordering::Relaxed) as ID;
        if !lbl.is_null() {
            msg1_void_id(
                lbl,
                sel(c"setStringValue:"),
                nsstring(&format!("速度 x{v:.1}")),
            );
        }
    }
    preferences::save();
}

/// 境界の敏感さスライダ(0..30px。大きいほど境界に届きやすい)
unsafe extern "C" fn imp_edge_px(_s: ID, _c: SEL, sender: ID) {
    unsafe {
        let get: unsafe extern "C" fn(ID, SEL) -> f64 =
            std::mem::transmute(crate::objc_msgSend as *const () as usize);
        let v = get(sender, sel(c"doubleValue"));
        crate::set_edge_px(v);
        let lbl = PREFS_EDGE_LBL.load(Ordering::Relaxed) as ID;
        if !lbl.is_null() {
            msg1_void_id(
                lbl,
                sel(c"setStringValue:"),
                nsstring(&format!("敏感さ {v:.0}px")),
            );
        }
    }
    preferences::save();
}

/// スクロール互換モードのトグル(120 未満を無視する古いアプリ向け)
unsafe extern "C" fn imp_scroll_compat(_s: ID, _c: SEL, _n: ID) {
    let next = !crate::SCROLL_COMPAT.load(Ordering::Relaxed);
    crate::SCROLL_COMPAT.store(next, Ordering::Relaxed);
    eprintln!("[cfg] スクロール互換モード -> {next}");
    preferences::save();
}

/// clipboardSharing トグル(Deskflow 標準オプション)
unsafe extern "C" fn imp_clip_share(_s: ID, _c: SEL, _n: ID) {
    let next = !crate::CLIP_SHARE.load(Ordering::Relaxed);
    crate::CLIP_SHARE.store(next, Ordering::Relaxed);
    eprintln!("[cfg] クリップボード共有 -> {next}");
    crate::send_cfg();
    preferences::save();
}

/// メニュー「Windows の位置」: 右→左→上→下→右 のローテート
unsafe extern "C" fn imp_rotate_side(_s: ID, _c: SEL, _n: ID) {
    let next = (crate::SIDE.load(Ordering::Relaxed) + 1) % 4;
    crate::set_side(next);
    refresh_status();
    preferences::save();
}

/// 配置エディタ(独立ウィンドウ)を開く
/// 配置エディタ(独立ウィンドウ)を開く(メニュー IMP と起動直後の両方から呼ぶ)
pub fn show_layout() {
    show_prefs();
    unsafe {
        prefs::select_page(1);
    }
}

unsafe extern "C" fn imp_show_layout(_s: ID, _c: SEL, _n: ID) {
    show_layout();
}

unsafe extern "C" fn imp_scroll_flip(_s: ID, _c: SEL, _n: ID) {
    let next = !crate::SCROLL_FLIP.load(Ordering::Relaxed);
    crate::SCROLL_FLIP.store(next, Ordering::Relaxed);
    eprintln!(
        "[cfg] スクロール方向 -> {}",
        if next {
            "反転(Mac準拠)"
        } else {
            "標準(Windows準拠)"
        }
    );
    refresh_status();
    preferences::save();
}
unsafe extern "C" fn imp_vol(_s: ID, _c: SEL, sender: ID) {
    // 3 つのメニュー項目(▲/▼/ミュート)から送信元 tag で判別する
    let tag: isize = {
        let f: unsafe extern "C" fn(ID, SEL) -> isize =
            std::mem::transmute(crate::objc_msgSend as *const () as usize);
        f(sender, sel(c"tag"))
    };
    let op = match tag {
        1 => 0u8, // up
        2 => 1,   // down
        _ => 2,   // mute
    };
    crate::send_msg(&crate::Msg::Vol { op });
}

unsafe extern "C" fn imp_send_file(_s: ID, _c: SEL, _n: ID) {
    // accessory アプリでも明示アクティベートすればモーダルパネルは出せる
    let app = msg0(
        crate::objc_getClass(c"NSApplication".as_ptr()),
        sel(c"sharedApplication"),
    );
    msg1_void_u8(app, sel(c"activateIgnoringOtherApps:"), 1);
    let panel = msg0(
        crate::objc_getClass(c"NSOpenPanel".as_ptr()),
        sel(c"openPanel"),
    );
    if panel.is_null() {
        eprintln!("[gui] NSOpenPanel を生成できません");
        return;
    }
    msg1_void_u8(panel, sel(c"setCanChooseFiles:"), 1);
    msg1_void_u8(panel, sel(c"setCanChooseDirectories:"), 0);
    msg1_void_u8(panel, sel(c"setAllowsMultipleSelection:"), 1);
    // 送信の起点をデスクトップへ合わせる(手元で作った一時ファイルの定位置)
    let fm = msg0(
        objc_getClass(c"NSFileManager".as_ptr()),
        sel(c"defaultManager"),
    );
    if !fm.is_null() {
        let at: unsafe extern "C" fn(ID, SEL, usize, usize) -> ID =
            std::mem::transmute(crate::objc_msgSend as *const () as usize);
        let desktop = at(
            fm,
            sel(c"URLsForDirectory:inDomains:"),
            12, /*NSDesktopDirectory*/
            1,  /*NSUserDomainMask*/
        );
        if !desktop.is_null() {
            msg1_void_id(panel, sel(c"setDirectoryURL:"), desktop);
        }
    }
    msg1_void_id(
        panel,
        sel(c"setMessage:"),
        crate::nsstring(&format!(
            "Windows へ送信します(1回の合計 {} まで)",
            tsunagu_common::bulk::file_limit_label()
        )),
    );
    // runModal は選択が確定するまで戻らない(メイン RunLoop を内回りする)
    let resp = crate::msg0_isize(panel, sel(c"runModal"));
    if resp != 1 {
        return; // NSModalResponseOK 以外 = キャンセル
    }
    let urls = msg0(panel, sel(c"URLs"));
    if urls.is_null() {
        return;
    }
    let n = crate::msg0_isize(urls, sel(c"count")).max(0);
    let at: unsafe extern "C" fn(ID, SEL, usize) -> ID =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    let mut paths = Vec::new();
    for i in 0..n.min(64) {
        let url = at(urls, sel(c"objectAtIndex:"), i as usize);
        if url.is_null() {
            continue;
        }
        let path = msg0(url, sel(c"path")); // NSURL.path -> NSString
        if path.is_null() {
            continue;
        }
        let utf8 = crate::msg0_cstr(path, sel(c"UTF8String"));
        if utf8.is_null() {
            continue;
        }
        let s = std::ffi::CStr::from_ptr(utf8)
            .to_string_lossy()
            .into_owned();
        if !s.is_empty() {
            paths.push(s);
        }
    }
    if !paths.is_empty() {
        let pb: Vec<std::path::PathBuf> = paths.into_iter().map(std::path::PathBuf::from).collect();
        eprintln!("[gui] ファイル送信: {} 件", pb.len());
        crate::send_files_to_win(pb, false);
    }
}
/// 設定ウィンドウを開く(初回のみ生成。以降は同一ウィンドウを前面化)。
/// メニューの IMP と起動直後(--show-prefs)の両方から呼ぶ
/// NSException の内容を abort 前にログへ(objc が unwind 前に呼ぶ)
unsafe extern "C" fn uncaught_exc_handler(exc: ID) {
    unsafe {
        let name = msg0_cstr(msg0(exc, sel(c"name")), sel(c"UTF8String"));
        let reason = msg0_cstr(msg0(exc, sel(c"reason")), sel(c"UTF8String"));
        eprintln!(
            "[prefs] NSException name={:?} reason={:?}",
            if name.is_null() {
                None
            } else {
                std::ffi::CStr::from_ptr(name).to_str().ok()
            },
            if reason.is_null() {
                None
            } else {
                std::ffi::CStr::from_ptr(reason).to_str().ok()
            },
        );
    }
}

pub fn show_prefs() {
    eprintln!("[prefs] enter");
    unsafe {
        let app = msg0(
            objc_getClass(c"NSApplication".as_ptr()),
            sel(c"sharedApplication"),
        );
        if !app.is_null() {
            // メニューバー常駐型は非アクティブなので明示的に前面化する
            msg1_void_u8(app, sel(c"activateIgnoringOtherApps:"), 1);
        }
        extern "C" {
            fn NSSetUncaughtExceptionHandler(h: Option<unsafe extern "C" fn(ID)>);
        }
        NSSetUncaughtExceptionHandler(Some(uncaught_exc_handler));
        eprintln!("[prefs] activated");
        let existing = PREFS_WIN.load(Ordering::Relaxed) as ID;
        if existing.is_null() {
            let target = GUI_TARGET.load(Ordering::Relaxed) as ID;
            let win = make_prefs_window(target);
            if win.is_null() {
                eprintln!("[gui] 設定ウィンドウの生成に失敗");
                return;
            }
            PREFS_WIN.store(win as usize, Ordering::Relaxed);
        }
        let win = PREFS_WIN.load(Ordering::Relaxed) as ID;
        msg1_void_id(win, sel(c"makeKeyAndOrderFront:"), std::ptr::null_mut());
        sync_prefs_state();
    }
}

unsafe extern "C" fn imp_show_prefs(_s: ID, _c: SEL, _n: ID) {
    show_prefs();
}

/// チェックボックスの見た目を本体の状態(static)へ同期する(1秒タイマーから)
fn sync_prefs_state() {
    unsafe {
        prefs::sync();
        // Windows の位置ポップアップ(メニューのローテート反映。閉じた状態への
        // selectItemAtIndex はユーザー操作と競合しない)
        let pop = PREFS_SIDE_POP.load(Ordering::Relaxed) as ID;
        if !pop.is_null() {
            let select: unsafe extern "C" fn(ID, SEL, isize) =
                std::mem::transmute(crate::objc_msgSend as *const () as usize);
            select(
                pop,
                sel(c"selectItemAtIndex:"),
                crate::SIDE.load(Ordering::Relaxed) as isize,
            );
        }
        // 状態行(接続・操作中・遅延)。操作中の表示はモード名なしで統一
        let st = PREFS_STATE.load(Ordering::Relaxed) as ID;
        if !st.is_null() {
            let connected = crate::CONNECTED.load(Ordering::Relaxed);
            let rtt = crate::RTT_MS.load(Ordering::Relaxed);
            let conn = if connected {
                if rtt > 0 {
                    format!("接続済(遅延 {rtt}ms)")
                } else {
                    "接続済".into()
                }
            } else {
                "切断(再接続待機中)".to_string()
            };
            let mode = "操作中";
            msg1_void_id(
                st,
                sel(c"setStringValue:"),
                nsstring(&format!("状態: {conn} ・ {mode}")),
            );
            // 切断時は赤で強調(接続時は標準ラベル色)
            let color_cls = objc_getClass(c"NSColor".as_ptr());
            if !color_cls.is_null() {
                let get_color: unsafe extern "C" fn(ID, SEL) -> ID =
                    std::mem::transmute(crate::objc_msgSend as *const () as usize);
                let color = get_color(
                    color_cls,
                    sel(if connected {
                        c"labelColor"
                    } else {
                        c"systemRedColor"
                    }),
                );
                if !color.is_null() {
                    msg1_void_id(st, sel(c"setTextColor:"), color);
                }
            }
        }
        let set = |slot: &AtomicUsize, on: bool| {
            let b = slot.load(Ordering::Relaxed) as ID;
            if !b.is_null() {
                msg1_void_u8(b, sel(c"setState:"), on as u8);
            }
        };
        set(&PREFS_CHK_MODE, !crate::HOTKEY_ONLY.load(Ordering::Relaxed));
        set(
            &PREFS_CHK_TAPS,
            crate::EDGE_TAPS.load(Ordering::Relaxed) >= 2,
        );
        set(
            &PREFS_CHK_AUDIO,
            !crate::audio::MUTED.load(Ordering::Relaxed),
        );
        set(&PREFS_CHK_CMD, crate::CMD_ALT.load(Ordering::Relaxed));
        set(
            &PREFS_CHK_SCROLL,
            !crate::SCROLL_FLIP.load(Ordering::Relaxed),
        );
        set(&PREFS_CHK_SPK, crate::SPK_MUTE.load(Ordering::Relaxed));
        set(&PREFS_CHK_CLIP, crate::CLIP_SHARE.load(Ordering::Relaxed));
        set(
            &PREFS_CHK_SCOMPAT,
            crate::SCROLL_COMPAT.load(Ordering::Relaxed),
        );
    }
}

// ---------- モニター配置エディタ(Mac の「ディスプレイ配置」相当) ----------
// 灰色=Mac、青=Windows の矩形を描き、Windows 側をドラッグして物理配置を再現する。
// ドロップ時に「接する辺+辺に沿った接続範囲」を算出して SIDE/LAY_RANGE へ反映
const LAY_VW: f64 = 560.0;
const LAY_VH: f64 = 320.0;

/// Mac/Win 両画面の実ピクセルサイズ(hello 受信値。未接続時は一般値)
fn lay_px() -> ((f64, f64), (f64, f64)) {
    let mac = {
        let g = crate::geo();
        (g.main_w, g.main_h)
    };
    let win = *crate::WIN_SCREEN.lock().unwrap_or_else(|e| e.into_inner());
    (mac, win)
}

/// 共通縮尺: 両モニターを横並び + 縦に収める(重ならず両方必ず見える)
fn lay_scale() -> f64 {
    let ((mw, mh), (ww, wh)) = lay_px();
    let by_w = (LAY_VW - 80.0) / (mw + 2.0 * ww).max(1.0);
    let by_h = (LAY_VH - 40.0) / (mh + 2.0 * wh).max(1.0);
    by_w.min(by_h)
}

/// Windows 矩形のサイズ(実際の大きさ比・共通縮尺)
fn lay_win_size() -> (f64, f64) {
    let (_, (ww, wh)) = lay_px();
    let sc = lay_scale();
    (ww * sc, wh * sc)
}
static LAY_WIN: std::sync::Mutex<(f64, f64)> = std::sync::Mutex::new((0.0, 0.0)); // Win 矩形中心(0=既定=Macの右隣)

/// Win 矩形の中心(未設定なら Mac 右隣の既定位置)
fn lay_win_center() -> (f64, f64) {
    let c = *LAY_WIN.lock().unwrap_or_else(|e| e.into_inner());
    if c.0 <= 0.0 {
        let m = lay_mac_rect();
        let (ww, wh) = lay_win_size();
        let (cx, cy) = (m.x + m.w / 2.0, m.y + m.h / 2.0);
        return match crate::SIDE.load(Ordering::Relaxed) {
            1 => (m.x - 8.0 - ww / 2.0, cy),
            2 => (cx, m.y + m.h + 8.0 + wh / 2.0),
            3 => (cx, m.y - 8.0 - wh / 2.0),
            4 => (m.x + m.w + 8.0 + ww / 2.0, cy + m.h / 3.0),
            5 => (m.x + m.w + 8.0 + ww / 2.0, cy - m.h / 3.0),
            6 => (m.x - 8.0 - ww / 2.0, cy + m.h / 3.0),
            7 => (m.x - 8.0 - ww / 2.0, cy - m.h / 3.0),
            _ => (m.x + m.w + 8.0 + ww / 2.0, cy),
        };
    }
    c
}
static LAY_GRAB: std::sync::Mutex<(f64, f64)> = std::sync::Mutex::new((0.0, 0.0));
static LAY_DRAG: AtomicBool = AtomicBool::new(false);

fn lay_mac_rect() -> NSRect {
    let ((mw, mh), _) = lay_px();
    let sc = lay_scale();
    let h = mh * sc;
    NSRect {
        x: (LAY_VW - mw * sc) / 2.0,
        y: (LAY_VH - h) / 2.0,
        w: mw * sc,
        h,
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
struct CGPoint2 {
    x: f64,
    y: f64,
}

unsafe fn lay_point_in_view(_self: ID, ev: ID) -> CGPoint2 {
    unsafe {
        let loc: unsafe extern "C" fn(ID, SEL) -> CGPoint2 =
            std::mem::transmute(crate::objc_msgSend as *const () as usize);
        let conv: unsafe extern "C" fn(ID, SEL, CGPoint2, ID) -> CGPoint2 =
            std::mem::transmute(crate::objc_msgSend as *const () as usize);
        let p = loc(ev, sel(c"locationInWindow"));
        conv(
            _self,
            sel(c"convertPoint:fromView:"),
            p,
            std::ptr::null_mut(),
        )
    }
}

/// 描画: CoreGraphics で直接塗る(graphicsPort 経由)
unsafe extern "C" fn lay_draw(_self: ID, _cmd: SEL, _r: NSRect) {
    unsafe {
        let ctx_cls = objc_getClass(c"NSGraphicsContext".as_ptr());
        let cur = msg0(ctx_cls, sel(c"currentContext"));
        if cur.is_null() {
            return;
        }
        let port = msg0(cur, sel(c"graphicsPort"));
        if port.is_null() {
            return;
        }
        extern "C" {
            fn CGContextSetRGBFillColor(c: *mut core::ffi::c_void, r: f64, g: f64, b: f64, a: f64);
            fn CGContextFillRect(c: *mut core::ffi::c_void, r: NSRect);
            fn CGContextSetRGBStrokeColor(
                c: *mut core::ffi::c_void,
                r: f64,
                g: f64,
                b: f64,
                a: f64,
            );
            fn CGContextSetLineWidth(c: *mut core::ffi::c_void, w: f64);
            fn CGContextStrokeRect(c: *mut core::ffi::c_void, r: NSRect);
        }
        let ctx = port as *mut core::ffi::c_void;
        // 背景
        CGContextSetRGBFillColor(ctx, 0.94, 0.95, 0.98, 1.0);
        CGContextFillRect(
            ctx,
            NSRect {
                x: 0.0,
                y: 0.0,
                w: LAY_VW,
                h: LAY_VH,
            },
        );
        // Mac(灰+白枠: 青が重なっても輪郭が見える)
        let mr = lay_mac_rect();
        CGContextSetRGBFillColor(ctx, 0.52, 0.56, 0.65, 1.0);
        CGContextFillRect(ctx, mr);
        CGContextSetRGBStrokeColor(ctx, 1.0, 1.0, 1.0, 0.9);
        CGContextSetLineWidth(ctx, 1.5);
        CGContextStrokeRect(ctx, mr);
        // Windows(青=標準アクセント)
        let wc = lay_win_center();
        let (ww, wh) = lay_win_size();
        CGContextSetRGBFillColor(ctx, 0.32, 0.38, 0.82, 1.0);
        CGContextFillRect(
            ctx,
            NSRect {
                x: wc.0 - ww / 2.0,
                y: wc.1 - wh / 2.0,
                w: ww,
                h: wh,
            },
        );
    }
}

unsafe extern "C" fn lay_down(_self: ID, _cmd: SEL, ev: ID) {
    unsafe {
        let p = lay_point_in_view(_self, ev);
        let wc = lay_win_center();
        let (ww, wh) = lay_win_size();
        let inside = p.x >= wc.0 - ww / 2.0 - 4.0
            && p.x <= wc.0 + ww / 2.0 + 4.0
            && p.y >= wc.1 - wh / 2.0 - 4.0
            && p.y <= wc.1 + wh / 2.0 + 4.0;
        if inside {
            LAY_DRAG.store(true, Ordering::Relaxed);
            *LAY_GRAB.lock().unwrap_or_else(|e| e.into_inner()) = (p.x - wc.0, p.y - wc.1);
        }
    }
}

unsafe extern "C" fn lay_dragged(_self: ID, _cmd: SEL, ev: ID) {
    unsafe {
        if !LAY_DRAG.load(Ordering::Relaxed) {
            return;
        }
        let p = lay_point_in_view(_self, ev);
        let g = *LAY_GRAB.lock().unwrap_or_else(|e| e.into_inner());
        let (ww, wh) = lay_win_size();
        let nx = (p.x - g.0).clamp(ww / 2.0 + 2.0, LAY_VW - ww / 2.0 - 2.0);
        let ny = (p.y - g.1).clamp(wh / 2.0 + 2.0, LAY_VH - wh / 2.0 - 2.0);
        *LAY_WIN.lock().unwrap_or_else(|e| e.into_inner()) = (nx, ny);
        let snd: unsafe extern "C" fn(ID, SEL, u8) =
            std::mem::transmute(crate::objc_msgSend as *const () as usize);
        snd(_self, sel(c"setNeedsDisplay:"), 1);
    }
}

/// ドロップ: 接する辺と接続範囲(LAY_RANGE)を算出して SIDE へ反映し、
/// 矩形をきれいな位置(辺にスナップ)へ揃える
unsafe extern "C" fn lay_up(_self: ID, _cmd: SEL, _ev: ID) {
    unsafe {
        if !LAY_DRAG.swap(false, Ordering::Relaxed) {
            return;
        }
        let m = lay_mac_rect();
        let mc = (m.x + m.w / 2.0, m.y + m.h / 2.0);
        let wc0 = lay_win_center();
        let (ww, wh) = lay_win_size();
        let (dx, dy) = (wc0.0 - mc.0, wc0.1 - mc.1);
        let (edge, f0, f1) = if dx.abs() >= dy.abs() {
            // 左右いずれかの辺に接続。Win の縦範囲が Mac の縦範囲のどこに来るか
            let f0 = ((wc0.1 - wh / 2.0) - m.y) / m.h;
            let f1 = ((wc0.1 + wh / 2.0) - m.y) / m.h;
            (
                if dx >= 0.0 { 0u8 } else { 1u8 },
                f0.clamp(0.0, 1.0),
                f1.clamp(0.0, 1.0),
            )
        } else {
            let f0 = ((wc0.0 - ww / 2.0) - m.x) / m.w;
            let f1 = ((wc0.0 + ww / 2.0) - m.x) / m.w;
            (
                if dy >= 0.0 { 2u8 } else { 3u8 },
                f0.clamp(0.0, 1.0),
                f1.clamp(0.0, 1.0),
            )
        };
        // 斜め(4-7)表現: 水平辺で接続範囲が半分未満なら上下の半分側へ
        let mut side = edge;
        if edge <= 1 && (f1 - f0) < 0.6 {
            side = match (edge, (f0 + f1) / 2.0 >= 0.5) {
                (0, true) => 4,
                (0, false) => 5,
                (1, true) => 6,
                _ => 7,
            };
        }
        crate::set_side(side);
        // 細かい範囲で上書き(set_side は半分単位で設定するため)
        let (start, end) = if edge <= 1 {
            (1.0 - f1, 1.0 - f0)
        } else {
            (f0, f1)
        };
        let start = start.clamp(0.0, 0.95);
        let range = (start, end.max(start + 0.05).min(1.0));
        *crate::LAY_RANGE.lock().unwrap_or_else(|e| e.into_inner()) = range;
        let snd: unsafe extern "C" fn(ID, SEL, u8) =
            std::mem::transmute(crate::objc_msgSend as *const () as usize);
        snd(_self, sel(c"setNeedsDisplay:"), 1);
        eprintln!(
            "[lay] 配置を更新: {}(接続範囲 {:.2}〜{:.2})",
            crate::side_name(),
            range.0,
            range.1
        );
    }
    preferences::save();
}

/// 配置エディタの NSView サブクラスを登録して生成(初回のみ)
unsafe fn make_layout_view(target_frame_host: ID) -> ID {
    unsafe {
        let _ = target_frame_host;
        static CLS: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
        let cls = *CLS.get_or_init(|| {
            let super_cls = objc_getClass(c"NSView".as_ptr());
            if super_cls.is_null() {
                return 0usize;
            }
            let cls = objc_allocateClassPair(super_cls, c"TSLayView".as_ptr(), 0);
            if cls.is_null() {
                return 0usize;
            }
            let ok1 = class_addMethod(
                cls,
                sel(c"drawRect:"),
                lay_draw as *const () as usize,
                c"v@:{CGRect={CGPoint=dd}{CGSize=dd}}".as_ptr(),
            );
            let ok2 = class_addMethod(
                cls,
                sel(c"mouseDown:"),
                lay_down as *const () as usize,
                c"v@:@".as_ptr(),
            );
            let ok3 = class_addMethod(
                cls,
                sel(c"mouseDragged:"),
                lay_dragged as *const () as usize,
                c"v@:@".as_ptr(),
            );
            let ok4 = class_addMethod(
                cls,
                sel(c"mouseUp:"),
                lay_up as *const () as usize,
                c"v@:@".as_ptr(),
            );
            if ok1 == 0 || ok2 == 0 || ok3 == 0 || ok4 == 0 {
                return 0usize;
            }
            objc_registerClassPair(cls);
            cls as usize
        });
        if cls == 0 {
            return std::ptr::null_mut();
        }
        let init: unsafe extern "C" fn(ID, SEL, NSRect) -> ID =
            std::mem::transmute(crate::objc_msgSend as *const () as usize);
        init(
            msg0(cls as ID, sel(c"alloc")),
            sel(c"initWithFrame:"),
            NSRect {
                x: 0.0,
                y: 0.0,
                w: LAY_VW,
                h: LAY_VH,
            },
        )
    }
}

/// OSネイティブの4分類設定画面を構築する。
unsafe fn make_prefs_window(target: ID) -> ID {
    prefs::build(target)
}

unsafe extern "C" fn imp_quit(_s: ID, _c: SEL, _n: ID) {
    eprintln!("[gui] メニューから終了しました");
    // Windows 画面を操作したまま終了すると、カーソルの関連切断・非表示が
    // システムへ残る。正規の leave(カーソル復帰+Windows 側の後片付け)を
    // 経由してから終了する
    if crate::WIN_MODE.swap(false, Ordering::Relaxed) {
        crate::leave_win_mode_cursor_unlock(None);
    }
    std::process::exit(0);
}
unsafe extern "C" fn imp_update(_s: ID, _c: SEL, _n: ID) {
    // --show-prefs: NSApp.run 開始後のタイマーコンテキストで開く
    // (run 前のウィンドウ操作は NSException で abort するため遅延させる)
    if SHOW_AT_START.swap(false, Ordering::Relaxed) {
        show_prefs();
        // 検証用: TSUNAGU_SHOW_LAYOUT=1 で配置ウィンドウも同時オープン
        if !UI_PREVIEW.load(Ordering::Relaxed)
            && crate::envutil::get("TSUNAGU_SHOW_LAYOUT").as_deref() == Some("1")
        {
            show_layout();
        }
    }
    refresh_status();
}

/// 開発環境のリポジトリを探す(<repo>/target/release/tsunagu-mac から3階層上)
fn restart_script() -> Option<std::path::PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let mut p = exe.clone();
    for _ in 0..3 {
        p.pop();
    }
    let s = p.join("scripts/restart-mac.sh");
    s.exists().then_some(s)
}

/// 状態表示の更新(メニューバーのボタンタイトル + メニュー内の動的項目)。
/// NSTimer から毎秒呼ばれる。メニュー開閉中も止まらないよう common modes で登録
fn refresh_status() {
    unsafe {
        let button = GUI_BUTTON.load(Ordering::Relaxed) as ID;
        if button.is_null() {
            return;
        }
        let connected = crate::CONNECTED.load(Ordering::Relaxed);
        // 未接続の時だけ文字を出し、接続中は常時アイコンのみ
        //(どちらの画面を見ているかは操作の結果で分かる。ユーザー指示 459/494)
        let title = if !connected { "未接続" } else { "" };
        msg1_void_id(button, sel(c"setTitle:"), nsstring(title));

        let state = GUI_STATE_ITEM.load(Ordering::Relaxed) as ID;
        if !state.is_null() {
            let conn = if connected {
                "接続済"
            } else {
                "切断(自動再接続中・Windows アプリの起動を確認)"
            };
            // 操作中の表示はモード名を挟まず「操作中」に統一する
            let mode = "操作中";
            let rtt = crate::RTT_MS.load(Ordering::Relaxed);
            let rtt_s = if connected && rtt > 0 {
                format!("・遅延 {rtt}ms")
            } else {
                String::new()
            };
            // 経路(LAN 直 / Tailscale)も出す: 中継へ落ちていないかの常時確認用
            let route = crate::route_label();
            let route_s = if connected && !route.is_empty() {
                format!("・{route}")
            } else {
                String::new()
            };
            let history = crate::HISTORY
                .lock()
                .map(|h| h.entries().len())
                .unwrap_or(0);
            let history_s = if history > 0 {
                format!("・履歴{history}件")
            } else {
                String::new()
            };
            let text = format!("{conn} ・ {mode}{rtt_s}{route_s}{history_s}");
            msg1_void_id(state, sel(c"setTitle:"), nsstring(&text));
        }
        let mode_item = GUI_MODE_ITEM.load(Ordering::Relaxed) as ID;
        if !mode_item.is_null() {
            let hotkey = crate::HOTKEY_ONLY.load(Ordering::Relaxed);
            let t = if hotkey {
                "切替方式: ホットキーロック"
            } else {
                "切替方式: 境界+ダブルタップ"
            };
            msg1_void_id(mode_item, sel(c"setTitle:"), nsstring(t));
        }
        let taps_item = GUI_TAPS_ITEM.load(Ordering::Relaxed) as ID;
        if !taps_item.is_null() {
            let t = if crate::EDGE_TAPS.load(Ordering::Relaxed) >= 2 {
                "境界到達: ダブルタップ"
            } else {
                "境界到達: 1回"
            };
            msg1_void_id(taps_item, sel(c"setTitle:"), nsstring(t));
        }
        let side_item = GUI_SIDE_ITEM.load(Ordering::Relaxed) as ID;
        if !side_item.is_null() {
            msg1_void_id(
                side_item,
                sel(c"setTitle:"),
                nsstring(&format!("Windows の位置: {}", crate::side_name())),
            );
        }
        let audio_item = GUI_AUDIO_ITEM.load(Ordering::Relaxed) as ID;
        if !audio_item.is_null() {
            let t = if crate::audio::MUTED.load(Ordering::Relaxed) {
                "音声転送: OFF(ミュート)"
            } else {
                "音声転送: ON"
            };
            msg1_void_id(audio_item, sel(c"setTitle:"), nsstring(t));
        }
        let cmd_item = GUI_CMD_ITEM.load(Ordering::Relaxed) as ID;
        if !cmd_item.is_null() {
            let t = if crate::CMD_ALT.load(Ordering::Relaxed) {
                "⌘キー: Alt"
            } else {
                "⌘キー: Ctrl"
            };
            msg1_void_id(cmd_item, sel(c"setTitle:"), nsstring(t));
        }
        let scroll_item = GUI_SCROLL_ITEM.load(Ordering::Relaxed) as ID;
        if !scroll_item.is_null() {
            let t = if crate::SCROLL_FLIP.load(Ordering::Relaxed) {
                "スクロール方向: Windows 標準"
            } else {
                "スクロール方向: Mac に合わせる"
            };
            msg1_void_id(scroll_item, sel(c"setTitle:"), nsstring(t));
        }
        let spk_item = GUI_SPK_ITEM.load(Ordering::Relaxed) as ID;
        if !spk_item.is_null() {
            let t = if crate::SPK_MUTE.load(Ordering::Relaxed) {
                "Windowsスピーカー: 接続中ミュート(Macのみ発音)"
            } else {
                "Windowsスピーカー: 常時鳴らす"
            };
            msg1_void_id(spk_item, sel(c"setTitle:"), nsstring(t));
        }
        // 設定ウィンドウが開いていればチェック状態も保ち直す
        sync_prefs_state();

        // クリップボード履歴メニュー(push があったときだけ作り直す。
        // 開いているメニューのちらつきを避けるため変化検知する)
        let history_menu = GUI_HISTORY_MENU.load(Ordering::Relaxed) as ID;
        if !history_menu.is_null() {
            let last = crate::HISTORY_LAST_ID.load(Ordering::Relaxed);
            if last != GUI_HISTORY_SEEN.load(Ordering::Relaxed) {
                GUI_HISTORY_SEEN.store(last, Ordering::Relaxed);
                rebuild_history_menu(history_menu);
            }
        }

        // 接続先メニュー(複数台保持。接続の一覧・切替)。履歴と同じく変化時だけ作り直す
        let peers_menu = GUI_PEERS_MENU.load(Ordering::Relaxed) as ID;
        if !peers_menu.is_null() {
            let sig = {
                let peers = crate::PEERS.lock().unwrap_or_else(|e| e.into_inner());
                let act = *crate::ACTIVE_PEER.lock().unwrap_or_else(|e| e.into_inner());
                peers
                    .iter()
                    .enumerate()
                    .map(|(i, p)| format!("{}|{}", i == act, p.name))
                    .collect::<Vec<_>>()
                    .join("\n")
            };
            let mut seen = GUI_PEERS_SIG.lock().unwrap_or_else(|e| e.into_inner());
            if *seen != sig {
                *seen = sig;
                rebuild_peers_menu(peers_menu);
            }
        }
    }
}

/// 接続先サブメニューの項目を作り直す。アクティブな相手はチェックを付け、
/// 選択で representedObject の id を sdPeerActivate: へ渡す
unsafe fn rebuild_peers_menu(menu: ID) {
    msg0(menu, sel(c"removeAllItems"));
    let target = GUI_TARGET.load(Ordering::Relaxed) as ID;
    let entries: Vec<(bool, String, String)> = {
        let peers = crate::PEERS.lock().unwrap_or_else(|e| e.into_inner());
        let act = *crate::ACTIVE_PEER.lock().unwrap_or_else(|e| e.into_inner());
        peers
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let mons = tsunagu_common::proto::Monitor::summary(&p.monitors);
                let title = if p.monitors.len() > 1 {
                    format!("{} ・{}面", p.name, p.monitors.len())
                } else if p.monitors.len() == 1 {
                    format!("{} ・{}", p.name, mons)
                } else {
                    p.name.clone()
                };
                (i == act, title, p.id.clone())
            })
            .collect()
    };
    if entries.is_empty() {
        let item = menu_item("接続はまだありません", None, "");
        msg1_void_u8(item, sel(c"setEnabled:"), 0);
        add_item(menu, item);
        return;
    }
    for (active, title, id) in entries {
        let item = menu_item(&title, Some(c"sdPeerActivate:"), "");
        if item.is_null() {
            continue;
        }
        msg1_void_id(item, sel(c"setTarget:"), target);
        if active {
            // アクティブな相手にはチェックを付ける(NSOnState)
            msg1_void_i64(item, sel(c"setState:"), 1);
        }
        msg1_void_id(item, sel(c"setRepresentedObject:"), crate::nsstring(&id));
        add_item(menu, item);
    }
}

/// 接続先メニューの選択: representedObject の端末 id でピアを探してアクティブへ
unsafe extern "C" fn imp_peer_activate(_s: ID, _c: SEL, sender: ID) {
    if sender.is_null() {
        return;
    }
    let obj = msg0(sender, sel(c"representedObject"));
    if obj.is_null() {
        return;
    }
    let utf8 = crate::msg0_cstr(obj, sel(c"UTF8String"));
    if utf8.is_null() {
        return;
    }
    let id = std::ffi::CStr::from_ptr(utf8)
        .to_string_lossy()
        .into_owned();
    let idx = crate::PEERS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .position(|p| p.id == id);
    if let Some(i) = idx {
        crate::activate_peer(i, "接続先メニュー");
    }
}

/// 履歴サブメニューの項目を作り直す(新しい順 10 件+「消す」)。
/// 本文は representedObject に載せ、クリックで sdHistoryRestore: へ渡す
unsafe fn rebuild_history_menu(menu: ID) {
    msg0(menu, sel(c"removeAllItems"));
    let target = GUI_TARGET.load(Ordering::Relaxed) as ID;
    let count = crate::HISTORY
        .lock()
        .map(|h| h.entries().len())
        .unwrap_or(0);
    // 見出しに件数を出す(メニューバー本体の項目タイトルも更新)
    let holder = GUI_HISTORY_ITEM.load(Ordering::Relaxed) as ID;
    if !holder.is_null() {
        let t = if count > 0 {
            format!("クリップボード履歴({count}件)")
        } else {
            "クリップボード履歴".to_string()
        };
        msg1_void_id(holder, sel(c"setTitle:"), nsstring(&t));
    }
    let items = crate::HISTORY.lock().ok().map(|h| {
        let now = tsunagu_common::history::now_epoch_ms();
        h.recent(tsunagu_common::history::MENU_ITEMS)
            .into_iter()
            .map(|e| (tsunagu_common::history::label(e, now, 34), e.text.clone()))
            .collect::<Vec<_>>()
    });
    let items = items.unwrap_or_default();
    if items.is_empty() {
        let item = menu_item("履歴はまだありません(画面を越えると記録されます)", None, "");
        msg1_void_u8(item, sel(c"setEnabled:"), 0);
        add_item(menu, item);
        return;
    }
    for (title, text) in items {
        let item = menu_item(&title, Some(c"sdHistoryRestore:"), "");
        if item.is_null() {
            continue;
        }
        msg1_void_id(item, sel(c"setTarget:"), target);
        msg1_void_id(item, sel(c"setRepresentedObject:"), crate::nsstring(&text));
        add_item(menu, item);
    }
    let sep = msg0(objc_getClass(c"NSMenuItem".as_ptr()), sel(c"separatorItem"));
    add_item(menu, sep);
    let clear = menu_item("履歴を消す", Some(c"sdHistoryClear:"), "");
    if !clear.is_null() {
        msg1_void_id(clear, sel(c"setTarget:"), target);
        add_item(menu, clear);
    }
}

/// メニューバー用テンプレートアイコン(アプリアイコンと同じリボン)。
/// CoreGraphics で 44px ビットマップに描き NSImage(template) 化する。
/// template なのでメニューバーの明暗に自動追従する(色ではなくアルファで描画)
unsafe fn make_menu_icon() -> ID {
    const C: usize = 44;
    let mut data = vec![0u8; C * 4 * C];
    let space = CGColorSpaceCreateDeviceRGB();
    let ctx = CGBitmapContextCreate(
        data.as_mut_ptr(),
        C,
        C,
        8,
        C * 4,
        space,
        2 | (2 << 12), // BGRA
    );
    if ctx.is_null() {
        return std::ptr::null_mut();
    }
    extern "C" {
        fn CGContextAddCurveToPoint(c: ID, x1: f64, y1: f64, x2: f64, y2: f64, x: f64, y: f64);
    }
    CGContextSetRGBStrokeColor(ctx, 0.0, 0.0, 0.0, 1.0);
    CGContextSetLineWidth(ctx, 4.0);
    CGContextSetLineCap(ctx, 1);
    CGContextBeginPath(ctx);
    CGContextMoveToPoint(ctx, 20.0, 19.0);
    CGContextAddCurveToPoint(ctx, 11.0, 8.0, 4.0, 13.0, 8.0, 21.0);
    CGContextAddCurveToPoint(ctx, 10.0, 25.0, 14.0, 28.0, 19.0, 29.0);
    CGContextStrokePath(ctx);
    CGContextBeginPath(ctx);
    CGContextMoveToPoint(ctx, 16.0, 14.0);
    CGContextAddCurveToPoint(ctx, 23.0, 22.0, 27.0, 36.0, 34.0, 29.0);
    CGContextAddCurveToPoint(ctx, 40.0, 22.0, 29.0, 16.0, 24.0, 15.0);
    CGContextStrokePath(ctx);

    let img = CGBitmapContextCreateImage(ctx);
    CGContextRelease(ctx);
    CFRelease(space);
    if img.is_null() {
        return std::ptr::null_mut();
    }
    // NSImage initWithCGImage:size: (NSSize は arm64 で d0/d1 レジスタ渡し)
    let f: unsafe extern "C" fn(ID, SEL, ID, f64, f64) -> ID =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    let nsimg = f(
        msg0(objc_getClass(c"NSImage".as_ptr()), sel(c"alloc")),
        sel(c"initWithCGImage:size:"),
        img,
        18.0,
        18.0,
    );
    CGImageRelease(img);
    if nsimg.is_null() {
        return std::ptr::null_mut();
    }
    msg1_void_u8(nsimg, sel(c"setTemplate:"), 1);
    nsimg
}

unsafe fn make_target() -> ID {
    let super_cls = objc_getClass(c"NSObject".as_ptr());
    if super_cls.is_null() {
        return std::ptr::null_mut();
    }
    let cls = objc_allocateClassPair(super_cls, c"SDMenuTarget".as_ptr(), 0);
    if cls.is_null() {
        return std::ptr::null_mut();
    }
    //IMP は (self, _cmd, sender) の v@:@ 型
    let types = c"v@:@".as_ptr();
    let methods: &[(&std::ffi::CStr, usize)] = &[
        (c"sdHotkey:", prefs::hotkey as *const () as usize),
        (c"sdNavigate:", prefs::navigate as *const () as usize),
        (
            c"sdSwitchMethod:",
            prefs::switch_method as *const () as usize,
        ),
        (c"sdReturnMac:", prefs::return_mac as *const () as usize),
        (
            c"sdRegistration:",
            setup::show_registration as *const () as usize,
        ),
        (c"sdScrollSpeed:", prefs::scroll_speed as *const () as usize),
        (c"sdSwitchMode:", imp_switch_mode as *const () as usize),
        (c"sdEdgeTaps:", imp_edge_taps as *const () as usize),
        (c"sdOpenLog:", imp_open_log as *const () as usize),
        (
            c"sdHistoryRestore:",
            imp_history_restore as *const () as usize,
        ),
        (c"sdHistoryClear:", imp_history_clear as *const () as usize),
        (c"sdPeerActivate:", imp_peer_activate as *const () as usize),
        (c"sdShowSearch:", imp_show_search as *const () as usize),
        (c"sdSearchPick:", imp_search_pick as *const () as usize),
        (c"sdSearchGo:", imp_search_go as *const () as usize),
        (
            c"sdSearchRefresh:",
            imp_search_refresh as *const () as usize,
        ),
        (c"sdSearchChar:", imp_search_char as *const () as usize),
        (
            c"sdSearchBackspace:",
            imp_search_backspace as *const () as usize,
        ),
        (
            c"sdSearchArrowDown:",
            imp_search_arrow_down as *const () as usize,
        ),
        (
            c"sdSearchArrowUp:",
            imp_search_arrow_up as *const () as usize,
        ),
        (
            c"controlTextDidChange:",
            imp_control_text_did_change as *const () as usize,
        ),
        (c"sdRestart:", imp_restart as *const () as usize),
        (c"sdAudio:", imp_audio_toggle as *const () as usize),
        (c"sdCmdMap:", imp_cmd_map as *const () as usize),
        (c"sdScroll:", imp_scroll_flip as *const () as usize),
        (c"sdSpkMute:", imp_spk_mute as *const () as usize),
        (c"sdVol:", imp_vol as *const () as usize),
        (c"sdSendFile:", imp_send_file as *const () as usize),
        (c"sdShowPrefs:", imp_show_prefs as *const () as usize),
        (c"sdScrollGain:", imp_scroll_gain as *const () as usize),
        (c"sdSide:", imp_side as *const () as usize),
        (c"sdDelay:", imp_switch_delay as *const () as usize),
        (c"sdDblTap:", imp_dbl_tap as *const () as usize),
        (c"sdClipShare:", imp_clip_share as *const () as usize),
        (c"sdScrollCompat:", imp_scroll_compat as *const () as usize),
        (c"sdRotateSide:", imp_rotate_side as *const () as usize),
        (c"sdLayout:", imp_show_layout as *const () as usize),
        (c"sdMouseScale:", imp_mouse_scale as *const () as usize),
        (c"sdEdgePx:", imp_edge_px as *const () as usize),
        (c"sdQuit:", imp_quit as *const () as usize),
        (c"updateStatus:", imp_update as *const () as usize),
        (
            c"pollIncomingDrag:",
            crate::incoming_drag::poll as *const () as usize,
        ),
    ];
    for (name, imp) in methods {
        if class_addMethod(cls, sel(name), *imp, types) == 0 {
            return std::ptr::null_mut();
        }
    }
    objc_registerClassPair(cls);
    msg0(cls, sel(c"new"))
}

unsafe fn menu_item(title: &str, action: Option<&std::ffi::CStr>, key: &str) -> ID {
    let t = nsstring(title);
    let k = nsstring(key);
    let a = action.map(|x| sel(x)).unwrap_or(std::ptr::null_mut());
    // alloc + initWithTitle:action:keyEquivalent:(インスタンス初期化子)。
    // NSMenuItem には itemWithTitle:/menuItemWithTitle: クラスファクトリが無い
    // (未認識セレクタで NS例外→abort になるため使わない)
    let alloc = msg0(objc_getClass(c"NSMenuItem".as_ptr()), sel(c"alloc"));
    if alloc.is_null() {
        return std::ptr::null_mut();
    }
    msg3_id(alloc, sel(c"initWithTitle:action:keyEquivalent:"), t, a, k)
}

unsafe fn add_item(menu: ID, item: ID) {
    msg1_void_id(menu, sel(c"addItem:"), item);
}

/// メニューバー常駐を構築する。失敗(ssh 由来等 AppKit 不可)は false を返し、
/// 呼び出し側は CUI(CFRunLoop)へフォールバックする
pub fn start() -> bool {
    unsafe {
        // NSApplication は最初に取得しておく(AppKit の初期化順序の慣例)
        let app = msg0(
            objc_getClass(c"NSApplication".as_ptr()),
            sel(c"sharedApplication"),
        );
        if app.is_null() {
            eprintln!("[gui] NSApplication を取得できません(CUI モードで継続)");
            return false;
        }
        let target = make_target();
        if target.is_null() {
            eprintln!("[gui] メニューターゲットクラスの登録に失敗(CUI モードで継続)");
            return false;
        }
        let _ = GUI_TARGET.store(target as usize, Ordering::Relaxed);

        let menu = msg0(objc_getClass(c"NSMenu".as_ptr()), sel(c"new"));
        if menu.is_null() {
            eprintln!("[gui] NSMenu 生成に失敗(CUI モードで継続)");
            return false;
        }
        // 自動有効化を切る(action 無しの状態行をクリック可能に見せないため)
        msg1_void_u8(menu, sel(c"setAutoenablesItems:"), 0);

        // 状態行(選択不可・refresh_status で毎秒更新)
        let state = menu_item("状態: …", None, "");
        if state.is_null() {
            return false;
        }
        msg1_void_u8(state, sel(c"setEnabled:"), 0);
        let _ = GUI_STATE_ITEM.store(state as usize, Ordering::Relaxed);
        add_item(menu, state);

        add_item(
            menu,
            msg0(objc_getClass(c"NSMenuItem".as_ptr()), sel(c"separatorItem")),
        );

        // Search My Desk(⌥⌘S でも開ける)。アクセサリアプリのキー等価は
        // 自身がアクティブな時しか効かないため、メニューからの導線を主にする
        let search = menu_item("Search My Desk…(⌥⌘S)", Some(c"sdShowSearch:"), "");
        if !search.is_null() {
            msg1_void_id(search, sel(c"setTarget:"), target);
            msg1_void_sel(search, sel(c"setAction:"), sel(c"sdShowSearch:"));
            add_item(menu, search);
        }

        let prefs = menu_item("設定…", Some(c"sdShowPrefs:"), ",");
        if prefs.is_null() {
            return false;
        }
        msg1_void_id(prefs, sel(c"setTarget:"), target);
        msg1_void_sel(prefs, sel(c"setAction:"), sel(c"sdShowPrefs:"));
        add_item(menu, prefs);

        // クリップボード履歴(送信・受信したテキストから選んで復元)。
        // 項目は refresh_status が履歴の変化だけ検知して作り直す
        let history_menu = msg0(objc_getClass(c"NSMenu".as_ptr()), sel(c"new"));
        if !history_menu.is_null() {
            msg1_void_u8(history_menu, sel(c"setAutoenablesItems:"), 0);
            let holder = menu_item("クリップボード履歴", None, "");
            if !holder.is_null() {
                msg1_void_id(holder, sel(c"setSubmenu:"), history_menu);
                add_item(menu, holder);
                let _ = GUI_HISTORY_ITEM.store(holder as usize, Ordering::Relaxed);
                let _ = GUI_HISTORY_MENU.store(history_menu as usize, Ordering::Relaxed);
            }
        }

        // 接続先(複数台の Windows を同時保持し、ここから切替える)。
        // 項目は refresh_status が接続一覧の変化だけ検知して作り直す
        let peers_menu = msg0(objc_getClass(c"NSMenu".as_ptr()), sel(c"new"));
        if !peers_menu.is_null() {
            msg1_void_u8(peers_menu, sel(c"setAutoenablesItems:"), 0);
            let holder = menu_item("接続先", None, "");
            if !holder.is_null() {
                msg1_void_id(holder, sel(c"setSubmenu:"), peers_menu);
                add_item(menu, holder);
                let _ = GUI_PEERS_ITEM.store(holder as usize, Ordering::Relaxed);
                let _ = GUI_PEERS_MENU.store(peers_menu as usize, Ordering::Relaxed);
            }
        }

        add_item(
            menu,
            msg0(objc_getClass(c"NSMenuItem".as_ptr()), sel(c"separatorItem")),
        );
        let quit = menu_item("tsunagu を終了", Some(c"sdQuit:"), "q");
        if quit.is_null() {
            return false;
        }
        msg1_void_id(quit, sel(c"setTarget:"), target);
        msg1_void_sel(quit, sel(c"setAction:"), sel(c"sdQuit:"));
        add_item(menu, quit);

        // ステータスバー項目(可変幅)。button のタイトルで状態を常時表示する
        let sb = msg0(
            objc_getClass(c"NSStatusBar".as_ptr()),
            sel(c"systemStatusBar"),
        );
        if sb.is_null() {
            return false;
        }
        let item = msg1_id_f64(sb, sel(c"statusItemWithLength:"), -1.0); // NSVariableStatusItemLength
        if item.is_null() {
            return false;
        }
        let button = msg0(item, sel(c"button"));
        if button.is_null() {
            return false;
        }
        let _ = GUI_BUTTON.store(button as usize, Ordering::Relaxed);
        // アイコン(テンプレート)+状態テキストの併記。失敗時はテキストのみで継続
        let icon = make_menu_icon();
        if !icon.is_null() {
            msg1_void_id(button, sel(c"setImage:"), icon);
        }
        msg1_void_id(item, sel(c"setMenu:"), menu);

        // 毎秒の状態反映。メニュー追跡中も止まらないよう common modes へ登録する
        let timer = msg5_timer(
            objc_getClass(c"NSTimer".as_ptr()),
            sel(c"scheduledTimerWithTimeInterval:target:selector:userInfo:repeats:"),
            1.0,
            target,
            sel(c"updateStatus:"),
            std::ptr::null_mut(),
            1,
        );
        if !timer.is_null() {
            let rl = msg0(objc_getClass(c"NSRunLoop".as_ptr()), sel(c"mainRunLoop"));
            // メニュー追跡(NSEventTrackingRunLoopMode)中も更新が続くよう common modes へ
            msg2_void_id_id(
                rl,
                sel(c"addTimer:forMode:"),
                timer,
                kCFRunLoopCommonModes as ID,
            );
        }
        msg5_timer(
            objc_getClass(c"NSTimer".as_ptr()),
            sel(c"scheduledTimerWithTimeInterval:target:selector:userInfo:repeats:"),
            0.016,
            target,
            sel(c"pollIncomingDrag:"),
            std::ptr::null_mut(),
            1,
        );
        refresh_status();
        true
    }
}

/// アプリケーション実行(NSApp.run)。戻らない。終了はメニューの「終了」
pub unsafe fn run_app() {
    let app = msg0(
        objc_getClass(c"NSApplication".as_ptr()),
        sel(c"sharedApplication"),
    );
    if app.is_null() {
        return;
    }
    // Accessory ポリシー = Dock アイコン非表示(メニューバー常駐型の標準)
    msg1_void_i64(app, sel(c"setActivationPolicy:"), 1);
    msg0_void(app, sel(c"run"));
}

#[cfg(test)]
mod search_sel_tests {
    use super::next_sel;

    #[test]
    fn arrow_moves_and_wraps_like_spotlight() {
        // ↓で進み、末尾で先頭へ折り返す
        assert_eq!(next_sel(0, 3, true), 1);
        assert_eq!(next_sel(2, 3, true), 0, "末尾の↓は先頭へ折り返す");
        // ↑で戻り、先頭で末尾へ折り返す
        assert_eq!(next_sel(2, 3, false), 1);
        assert_eq!(next_sel(0, 3, false), 2, "先頭の↑は末尾へ折り返す");
        // 候補が無いときは動かさない(範囲外を選択させない)
        assert_eq!(next_sel(0, 0, true), 0);
        assert_eq!(next_sel(0, 0, false), 0);
    }
}

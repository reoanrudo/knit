//! 設定の分類と画面構築。入力・通信の処理は既存アクションへ委譲する。
use super::*;
use std::sync::Mutex;
use super::localization::{self, Language};
pub(super) mod refined;
mod refined_layout;
mod refined_home;

struct CopyBinding {
    view: usize,
    property: &'static std::ffi::CStr,
    japanese: String,
    english: String,
}
static COPY_BINDINGS: Mutex<Vec<CopyBinding>> = Mutex::new(Vec::new());
static DYNAMIC_COPY: AtomicBool = AtomicBool::new(false);

fn dynamic_copy_scope<T>(f: impl FnOnce() -> T) -> T {
    struct Restore(bool);
    impl Drop for Restore { fn drop(&mut self){ DYNAMIC_COPY.store(self.0,Ordering::Relaxed); } }
    let _restore=Restore(DYNAMIC_COPY.swap(true,Ordering::Relaxed));
    f()
}
static LANGUAGE_POP: AtomicUsize = AtomicUsize::new(0);
static HOST_HINT: AtomicUsize = AtomicUsize::new(0);
static LAST_SAVE_STATUS: Mutex<String> = Mutex::new(String::new());
#[cfg(debug_assertions)]
static PREVIEW_CHECKED: AtomicBool = AtomicBool::new(false);

// 部品を作り直さずにコピーだけを替える。未保存の入力や first responder を失わない。
unsafe fn bind_copy(view: ID, property: &'static std::ffi::CStr, japanese: &str) {
    let english = localization::copy_for(Language::English, japanese);
    if english != japanese {
        msg1_void_id(view, sel(property), nsstring(localization::language().pick(japanese, &english)));
        if !DYNAMIC_COPY.load(Ordering::Relaxed) {
            COPY_BINDINGS.lock().unwrap_or_else(|e| e.into_inner()).push(CopyBinding {
                view: view as usize, property, japanese: japanese.into(), english,
            });
        }
    }
}

unsafe fn tooltip(view: ID, japanese: &str) {
    msg1_void_id(view, sel(c"setToolTip:"), nsstring(&localization::copy(japanese)));
    bind_copy(view, c"setToolTip:", japanese);
}

pub(super) unsafe extern "C" fn change_language(_s: ID, _c: SEL, sender: ID) {
    let index = crate::msg0_isize(sender, sel(c"indexOfSelectedItem"));
    localization::set_language(if index == 1 { Language::English } else { Language::Japanese });
    for binding in COPY_BINDINGS.lock().unwrap_or_else(|e| e.into_inner()).iter() {
        msg1_void_id(binding.view as ID, sel(binding.property), nsstring(localization::language().pick(&binding.japanese, &binding.english)));
    }
    sync();
    let fixed: Vec<_> = OPERATION_ENV_KEYS.iter().copied().filter(|k| crate::envutil::get(k).is_some()).collect();
    set_label(&ENV_NOTE_LABEL, &operation_env_note(&fixed));
    preferences::save();
}
/// ページ構成(Deskflow の設定ダイアログと同じ上部タブ): 0=接続(役割・接続先・
/// 状態・登録)、1=画面配置、2=切替、3=共有、4=その他(再起動・初期化)。
/// 全ページをウィンドウ上部のタブバーで直接切り替える(メイン⇔詳細の 2 階層
/// 往復をなくす。ウィンドウサイズは全ページ共通)
static PAGES: [AtomicUsize; 5] = [const { AtomicUsize::new(0) }; 5];
/// 上部タブバーのボタン(5 タブ)。select_page が選択表示(アクセント背景+tint)を
/// 揃える
static TABS: [AtomicUsize; 5] = [const { AtomicUsize::new(0) }; 5];
static SAVE_LABEL: AtomicUsize = AtomicUsize::new(0);
static HOTKEY_POP: AtomicUsize = AtomicUsize::new(0);
/// 切替キーポップアップの NSMenu(delegate の sender 比較用)。毎秒の
/// sync() が selectItemAtIndex: を打つため、開示中保護を付ける
/// (開示中の項目書き換えは未定義動作になるため)
static HOTKEY_MENU: AtomicUsize = AtomicUsize::new(0);
static HOTKEY_MENU_OPEN: AtomicBool = AtomicBool::new(false);
static AUDIO_LABEL: AtomicUsize = AtomicUsize::new(0);
/// 音声の再生音量スライダ(Windows から届く音をこの Mac で鳴らす大きさ)
static AUDIO_GAIN_SLIDER: AtomicUsize = AtomicUsize::new(0);
static AUDIO_GAIN_LABEL: AtomicUsize = AtomicUsize::new(0);
/// 再生音量スライダの一式(区切り線・caption・スライダ・値表示)。音声転送が
/// オン・env で audio が許可・Androidアプリ版相手でない間だけ表示する
/// (SWITCH_DELAY_VIEWS と同じパターンで sync() が切り替える)
static AUDIO_GAIN_VIEWS: Mutex<Vec<usize>> = Mutex::new(Vec::new());
static SHARE_HINT: AtomicUsize = AtomicUsize::new(0);
/// 接続ページの Android タブレットの状態行(端末が無ければ空)
static ANDROID_STATE: AtomicUsize = AtomicUsize::new(0);
/// 「端で少し待つ」の滞在時間スライダ(方式で選んだときだけ見せる)
static SWITCH_DELAY_SLIDER: AtomicUsize = AtomicUsize::new(0);
static SWITCH_DELAY_LABEL: AtomicUsize = AtomicUsize::new(0);
/// 滞在時間スライダの一式(区切り線・caption・スライダ・値表示。方式が
/// 「端で少し待つ」のときだけ表示する。AUDIO_GAIN_VIEWS と同じパターン)
static SWITCH_DELAY_VIEWS: Mutex<Vec<usize>> = Mutex::new(Vec::new());
static LAYOUT_CANVAS: AtomicUsize = AtomicUsize::new(0);
/// 「画面配置」ページの現在値行(各端末のモニター名+辺)。ドロップ直後の
/// redraw_layout と毎秒の sync() で書き換わる
static LAYOUT_VALUES: AtomicUsize = AtomicUsize::new(0);
/// 接続ページの「接続先のWindows」入力欄(この Mac が接続しに行く側のときだけ有効)
static HOST_FIELD: AtomicUsize = AtomicUsize::new(0);
/// 接続先の保存ボタン(入力欄と同じ条件で有効/無効が変わる)
static HOST_SAVE_BUTTON: AtomicUsize = AtomicUsize::new(0);
/// 保存系エラーの出元(設定「接続」のどの欄で起きたか)。Host 由来は保存状態行
/// (SAVE_LABEL・全ページ共通の最下部)へ出す(欄の下に注記行を新設しない=
/// 全ページ共通の最下部に出す方が「どこを見れば良いか」一本化できる)
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum SaveErrorSource {
    Host,
    /// preferences.json 自体が壊れていた(起動時に退避して初期設定で起動した)。
    /// 次の保存成功で外れる(preferences::save が解除する)
    PrefsFile,
    /// peer-sides.json(端末の配置と名前)が壊れて退避された(tap が検知して
    /// gui::report_peer_sides_error から設定する)。tap::save_peer_sides の
    /// 成功で外れる(clear_peer_sides_error)
    PeerSidesFile,
}
/// 設定「接続」の保存系(接続先)の直近エラー。設定ウィンドウを開いているのに
/// 通知センターへ逃げて数秒で消える問題への対策で、次の保存成功まで保存状態行
/// (SAVE_LABEL)へ出し続ける。sync() が毎秒書き直すため、preferences::save() が
/// 既定文言で上書きしても 1 秒でエラー文言へ戻る=入力を直すべき理由が画面から
/// 消えない
// 出元ごとに未解決エラーを保持する。同じ出元の更新だけを置換する。
static SAVE_ERROR_NOTE: Mutex<Vec<(SaveErrorSource, String)>> = Mutex::new(Vec::new());
/// 役割カードの注記行。通常は再接続の案内、KNIT_ROLE 固定中はその注記、
/// 切替の進行中は確認状況へ文言が変わる(進行は ROLE_NOTE_KIND で伝える)
static ROLE_NOTE_LABEL: AtomicUsize = AtomicUsize::new(0);
/// 役割カードの注記行のモード(0=通常 1=Env固定 2=確認待ち 3=確認済み 4=タイムアウト)。
/// 切替ハンドラと Ack 待ちスレッドが書き、毎秒の sync() が文言へ反映する
pub(super) static ROLE_NOTE_KIND: AtomicUsize = AtomicUsize::new(0);
/// 「切替」ページの環境変数固定の注記行(KNIT_SWITCH_MODE 等で固定中の見える化)。
/// 固定が無いときは空文字=見えない(ANDROID_STATE と同じ扱い)
static ENV_NOTE_LABEL: AtomicUsize = AtomicUsize::new(0);
/// 「その他」ページの先頭ボタン(Knitを再起動)。ページの初期フォーカス用
static OTHER_RESTART: AtomicUsize = AtomicUsize::new(0);

/// 「切替」ページの項目を固定できる環境変数(注記行の対象。接続の KNIT_ROLE は
/// 役割カードの注記が、共有の KNIT_SHARE は共有ページの注記が既に出すため含めない)
const OPERATION_ENV_KEYS: [&str; 9] = [
    "KNIT_SWITCH_MODE",
    "KNIT_HOTKEY_KC",
    "KNIT_SWITCH_DELAY",
    "KNIT_EDGE_TAPS",
    "KNIT_DOUBLE_TAP_MS",
    "KNIT_SCROLL_FLIP",
    "KNIT_SCROLL_DIV",
    "KNIT_MOUSE_SCALE",
    "KNIT_SCROLL_COMPAT",
];

/// 切替ページの注記行の本文(固定されているキーが無いときは空=非表示)。
/// キーは save_status_saved と同じ「先頭3件+ほか{N}件」へ丸める(9 キー全部が
/// 固定でも 1 行=ラベル幅 565px に収まるように)。初期化の導線
/// (「(初期化は左下の「その他」)」)は全ページ共通の保存状態行(SAVE_LABEL)が
/// 出すため、ここでは繰り返さない(上書きの意味はツールチップが担う)
fn operation_env_note(keys: &[&str]) -> String {
    if keys.is_empty() {
        String::new()
    } else {
        let names = match keys.len() {
            n if n <= 3 => keys.join("・"),
            n => format!("{} ほか{}件", keys[..3].join("・"), n - 3),
        };
        format!("{names} で固定中のため優先")
    }
}

/// 切替ページの注記行のツールチップ(1 行に収まらない対処案の完全版)。本文が
/// 短縮された分、優先の意味(起動時に上書き)と初期化の所在(設定画面の左下の
/// 「その他」)をここで伝える
fn operation_env_note_tooltip() -> &'static str {
    "起動時は環境変数の値でこの画面の設定を上書きします。初期化は設定画面の左下の「その他」からできます"
}

/// 保存状態行へ並べる環境変数キー列(純粋関数・単体テストで守る)。基本は先頭2件+
/// 「ほかN件」だが、キー名の累積幅(ASCII 6px/全角 11px の概算)が広い組合せでは
/// 1件へ縮退する: 行の幅 590px(11pt)に対処導線「(初期化は…)」まで必ず収まる
/// ようにするため(先頭3件を出していた頃は2件設定だけで導線ごと切れていた)
fn env_keys_summary(keys: &[&str]) -> String {
    let px = |s: &str| {
        s.chars()
            .map(|c| if c.is_ascii() { 6 } else { 11 })
            .sum::<usize>()
    };
    if keys.len() <= 2 {
        keys.join("・")
    } else if px(&keys[..2].join("・")) <= 200 {
        format!("{} ほか{}件", keys[..2].join("・"), keys.len() - 2)
    } else {
        format!("{} ほか{}件", keys[0], keys.len() - 1)
    }
}

/// 保存状態ラベルの本文(環境変数・envファイルで固定されているキーが分かるように。
/// 対処案(設定画面の左下の「その他」から初期化)は件数に関係なく同じ形で出す。
/// 固定なしのときは自動保存の対象だけを言う(入力欄がボタン保存であることは欄の
/// 横のボタン名「保存」「保存して再接続」が伝えるため、行では繰り返さない)
fn save_status_initial(keys: &[&str]) -> String {
    match keys.len() {
        0 => "スイッチ・スライダーは自動保存".to_string(),
        _ => format!(
            "自動保存・起動時は環境変数 {} が優先(初期化は左下の「その他」)",
            env_keys_summary(keys)
        ),
    }
}

/// 保存を明示的に行った直後のステータス文言(初期表示と同じ形式で固定キーを伝える)
pub(super) fn save_status_saved(keys: &[&str]) -> String {
    match keys.len() {
        0 => "変更を保存しました".to_string(),
        _ => format!(
            "保存済み・起動時は環境変数 {} が優先(初期化は左下の「その他」)",
            env_keys_summary(keys)
        ),
    }
}

unsafe fn frame(v: ID, r: NSRect) {
    let f: unsafe extern "C" fn(ID, SEL, NSRect) =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    f(v, sel(c"setFrame:"), r);
}
unsafe fn view(parent: ID, r: NSRect) -> ID {
    let f: unsafe extern "C" fn(ID, SEL, NSRect) -> ID =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    let v = f(
        msg0(objc_getClass(c"NSView".as_ptr()), sel(c"alloc")),
        sel(c"initWithFrame:"),
        r,
    );
    msg1_void_id(parent, sel(c"addSubview:"), v);
    v
}
unsafe fn group(parent: ID, y: f64, h: f64) -> ID {
    // NSColorをCGColorへ焼き付けない。NSBoxが窓のappearanceで色を解決する。
    let v = msg0(objc_getClass(c"NSBox".as_ptr()), sel(c"new"));
    frame(v, NSRect { x: 24.0, y, w: 572.0, h });
    msg1_void_i64(v, sel(c"setBoxType:"), 4);
    msg1_void_i64(v, sel(c"setBorderType:"), 1);
    msg1_void_i64(v, sel(c"setTitlePosition:"), 0);
    msg1_void_id(v, sel(c"setFillColor:"), msg0(objc_getClass(c"NSColor".as_ptr()), sel(c"controlBackgroundColor")));
    let f: unsafe extern "C" fn(ID, SEL, f64) =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    f(v, sel(c"setCornerRadius:"), 12.0);
    f(v, sel(c"setBorderWidth:"), 0.5);
    let color = msg0(objc_getClass(c"NSColor".as_ptr()), sel(c"separatorColor"));
    msg1_void_id(v, sel(c"setBorderColor:"), color);
    msg1_void_id(parent, sel(c"addSubview:"), v);
    v
}
unsafe fn divider(parent: ID, y: f64) -> ID {
    let v = msg0(objc_getClass(c"NSBox".as_ptr()), sel(c"new"));
    frame(v, NSRect { x: 40.0, y, w: 540.0, h: 1.0 });
    msg1_void_i64(v, sel(c"setBoxType:"), 2);
    msg1_void_id(parent, sel(c"addSubview:"), v);
    v
}
unsafe fn label(parent: ID, text: &str, x: f64, y: f64, w: f64, size: f64, muted: bool) -> ID {
    let f: unsafe extern "C" fn(ID, SEL, ID) -> ID =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    let v = f(
        objc_getClass(c"NSTextField".as_ptr()),
        sel(c"labelWithString:"),
        nsstring(&localization::copy(text)),
    );
    frame(
        v,
        NSRect {
            x,
            y,
            w,
            h: size + 10.0,
        },
    );
    let font = msg1_id_f64(
        objc_getClass(c"NSFont".as_ptr()),
        sel(if size >= 20.0 {
            c"boldSystemFontOfSize:"
        } else {
            c"systemFontOfSize:"
        }),
        size,
    );
    msg1_void_id(v, sel(c"setFont:"), font);
    let color = msg0(
        objc_getClass(c"NSColor".as_ptr()),
        sel(if muted {
            c"secondaryLabelColor"
        } else {
            c"labelColor"
        }),
    );
    msg1_void_id(v, sel(c"setTextColor:"), color);
    msg1_void_i64(v, sel(c"setLineBreakMode:"), 4);
    tooltip(v, text);
    bind_copy(v, c"setStringValue:", text);
    msg1_void_id(parent, sel(c"addSubview:"), v);
    v
}
unsafe fn button(parent: ID, target: ID, title: &str, action: &std::ffi::CStr, r: NSRect) -> ID {
    let f: unsafe extern "C" fn(ID, SEL, ID, ID, SEL) -> ID =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    let b = f(
        objc_getClass(c"NSButton".as_ptr()),
        sel(c"buttonWithTitle:target:action:"),
        nsstring(&localization::copy(title)),
        target,
        sel(action),
    );
    frame(b, r);
    bind_copy(b, c"setTitle:", title);
    bind_copy(b, c"setAccessibilityLabel:", title);
    msg1_void_id(parent, sel(c"addSubview:"), b);
    b
}

/// 編集できるテキスト入力欄(接続先・名前の指定用)。値は保存ボタンと同じアクション
/// で読み取る(ハンドラは欄の値を読むため sender がボタンでも入力欄でも同じ動き)。
/// action は欄自身へ付けるかどうか選べる: Some なら単行 NSTextField の Return が
/// action 発火=Enter で保存になる(名前・エイリアス欄。保存は即座に効くだけのため
/// 無害)。None ならボタン押下でのみ保存が走る(接続先・トークン欄。Enter や初期
/// フォーカスのままの Return が「保存して再起動」に直結するのを防ぐ。Tab は次の
/// コントロールへ移動するだけで保存は走らない。IME 確定前の Return は入力メソッド
/// が消費するため誤保存も起きない)
unsafe fn text_field(
    parent: ID,
    target: ID,
    action: Option<&std::ffi::CStr>,
    accessibility: &str,
    placeholder: &str,
    value: &str,
    r: NSRect,
) -> ID {
    let f: unsafe extern "C" fn(ID, SEL, NSRect) -> ID =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    let v = f(
        msg0(objc_getClass(c"NSTextField".as_ptr()), sel(c"alloc")),
        sel(c"initWithFrame:"),
        r,
    );
    msg1_void_u8(v, sel(c"setBezeled:"), 1);
    msg1_void_u8(v, sel(c"setEditable:"), 1);
    msg1_void_id(v, sel(c"setPlaceholderString:"), nsstring(&localization::copy(placeholder)));
    bind_copy(v, c"setPlaceholderString:", placeholder);
    msg1_void_id(v, sel(c"setStringValue:"), nsstring(value));
    let font = msg1_id_f64(
        objc_getClass(c"NSFont".as_ptr()),
        sel(c"systemFontOfSize:"),
        13.0,
    );
    msg1_void_id(v, sel(c"setFont:"), font);
    msg1_void_id(v, sel(c"setAccessibilityLabel:"), nsstring(&localization::copy(accessibility)));
    bind_copy(v, c"setAccessibilityLabel:", accessibility);
    if let Some(action) = action {
        msg1_void_id(v, sel(c"setTarget:"), target);
        msg1_void_sel(v, sel(c"setAction:"), sel(action));
    }
    msg1_void_id(parent, sel(c"addSubview:"), v);
    v
}
pub(super) static ROLE_RADIO: [AtomicUsize; 2] = [AtomicUsize::new(0), AtomicUsize::new(0)];

/// 切替キーの候補(GUI で選べるキー)。右⌘(54)は MacBook 内蔵キーボードなど
/// F13 が無い機種での代替(tap 側でホットキー分岐が先にイベントを掴むため、
/// 右⌘→右Ctrl 翻訳(KNIT_RCMD_CTRL)とは競合しない)。hotkey()/sync()/build() の
/// 3箇所で同じ並びが必要なため定数へ切り出した
const HOTKEY_CHOICES: [i64; 4] = [97, 100, 105, 54];

unsafe fn radio(parent: ID, target: ID, title: &str, action: &std::ffi::CStr, tag: i64, r: NSRect) -> ID {
    let b = msg0(objc_getClass(c"NSButton".as_ptr()), sel(c"new"));
    frame(b, r);
    msg1_void_id(b, sel(c"setTitle:"), nsstring(&localization::copy(title)));
    bind_copy(b, c"setTitle:", title);
    msg1_void_i64(b, sel(c"setButtonType:"), 4); // NSButtonTypeRadio
    msg1_void_i64(b, sel(c"setTag:"), tag);
    msg1_void_id(b, sel(c"setTarget:"), target);
    let action_fn: unsafe extern "C" fn(ID, SEL, SEL) =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    action_fn(b, sel(c"setAction:"), sel(action));
    msg1_void_id(b, sel(c"setAccessibilityLabel:"), nsstring(&localization::copy(title)));
    bind_copy(b, c"setAccessibilityLabel:", title);
    msg1_void_id(parent, sel(c"addSubview:"), b);
    b
}

/// 選択表示を現在の役割に合わせる(0=この Mac がホスト / 1=Windows がホスト)。
/// 実役割は環境変数 KNIT_ROLE=client が GUI 設定に優先するため表示も env を反映し、
/// 固定中は GUI から切り替えられないためラジオを無効化する(preview は除く)
pub(super) unsafe fn sync_role() {
    let client = crate::effective_client_role();
    let env_fixed = crate::role_env_fixed();
    for (i, slot) in ROLE_RADIO.iter().enumerate() {
        let b = slot.load(Ordering::Relaxed) as ID;
        if !b.is_null() {
            msg1_void_i64(b, sel(c"setState:"), ((i == 1) == client) as i64);
            msg1_void_u8(
                b,
                sel(c"setEnabled:"),
                (!env_fixed || UI_PREVIEW.load(Ordering::Relaxed)) as u8,
            );
        }
    }
}

unsafe fn check(
    parent: ID,
    target: ID,
    title: &str,
    action: &std::ffi::CStr,
    slot: &AtomicUsize,
    y: f64,
) -> ID {
    let caption = label(parent, title, 40.0, y + 1.0, 460.0, 13.0, false);
    let b = msg0(objc_getClass(c"NSSwitch".as_ptr()), sel(c"new"));
    frame(
        b,
        NSRect {
            x: 532.0,
            y,
            w: 44.0,
            h: 28.0,
        },
    );
    msg1_void_id(b, sel(c"setTarget:"), target);
    let action_fn: unsafe extern "C" fn(ID, SEL, SEL) =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    action_fn(b, sel(c"setAction:"), sel(action));
    msg1_void_id(b, sel(c"setAccessibilityLabel:"), nsstring(&localization::copy(title)));
    bind_copy(b, c"setAccessibilityLabel:", title);
    msg1_void_id(parent, sel(c"addSubview:"), b);
    slot.store(b as usize, Ordering::Relaxed);
    caption
}
unsafe fn popup(parent: ID, target: ID, titles: &[&str], action: &std::ffi::CStr, r: NSRect) -> ID {
    let f: unsafe extern "C" fn(ID, SEL, NSRect, u8) -> ID =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    let p = f(
        msg0(objc_getClass(c"NSPopUpButton".as_ptr()), sel(c"alloc")),
        sel(c"initWithFrame:pullsDown:"),
        r,
        0,
    );
    for (index, t) in titles.iter().enumerate() {
        msg1_void_id(p, sel(c"addItemWithTitle:"), nsstring(&localization::copy(t)));
        let get: unsafe extern "C" fn(ID, SEL, isize) -> ID = std::mem::transmute(crate::objc_msgSend as *const () as usize);
        bind_copy(get(p, sel(c"itemAtIndex:"), index as isize), c"setTitle:", t);
    }
    msg1_void_id(p, sel(c"setTarget:"), target);
    msg1_void_sel(p, sel(c"setAction:"), sel(action));
    msg1_void_id(parent, sel(c"addSubview:"), p);
    p
}
#[allow(clippy::too_many_arguments)] // UI 部品の生成は引数が本質的に多い
unsafe fn slider(
    parent: ID,
    target: ID,
    title: &str,
    value: f64,
    min: f64,
    max: f64,
    action: &std::ffi::CStr,
    slot: &AtomicUsize,
    value_slot: &AtomicUsize,
    y: f64,
) -> ID {
    let caption = label(parent, title, 40.0, y, 240.0, 13.0, false);
    let f: unsafe extern "C" fn(ID, SEL, f64, f64, f64, ID, SEL) -> ID =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    let s = f(
        objc_getClass(c"NSSlider".as_ptr()),
        sel(c"sliderWithValue:minValue:maxValue:target:action:"),
        value,
        min,
        max,
        target,
        sel(action),
    );
    frame(
        s,
        NSRect {
            x: 282.0,
            y,
            w: 204.0,
            h: 24.0,
        },
    );
    msg1_void_u8(s, sel(c"setContinuous:"), 0);
    msg1_void_id(s, sel(c"setAccessibilityLabel:"), nsstring(&localization::copy(title)));
    bind_copy(s, c"setAccessibilityLabel:", title);
    msg1_void_id(parent, sel(c"addSubview:"), s);
    slot.store(s as usize, Ordering::Relaxed);
    let value = label(parent, "", 490.0, y, 90.0, 11.0, true);
    msg1_void_i64(value, sel(c"setAlignment:"), 2);
    value_slot.store(value as usize, Ordering::Relaxed);
    caption
}

pub(super) unsafe fn select_page(index: usize) {
    if refined::is_built() {
        refined::select_page(index);
        return;
    }
    let index = index.min(PAGES.len() - 1);
    for i in 0..PAGES.len() {
        let p = PAGES[i].load(Ordering::Relaxed) as ID;
        if !p.is_null() {
            msg1_void_u8(p, sel(c"setHidden:"), (i != index) as u8);
        }
        let b = TABS[i].load(Ordering::Relaxed) as ID;
        if !b.is_null() {
            msg1_void_i64(b, sel(c"setState:"), (i == index) as i64);
            let color_cls = objc_getClass(c"NSColor".as_ptr());
            let accent = msg1_id_f64(
                msg0(color_cls, sel(c"controlAccentColor")),
                sel(c"colorWithAlphaComponent:"),
                if i == index { 0.14 } else { 0.0 },
            );
            msg1_void_u8(b, sel(c"setWantsLayer:"), 1);
            let radius: unsafe extern "C" fn(ID, SEL, f64) =
                std::mem::transmute(crate::objc_msgSend as *const () as usize);
            radius(msg0(b, sel(c"layer")), sel(c"setCornerRadius:"), 7.0);
            msg1_void_id(
                msg0(b, sel(c"layer")),
                sel(c"setBackgroundColor:"),
                msg0(accent, sel(c"CGColor")),
            );
            msg1_void_id(
                b,
                sel(c"setContentTintColor:"),
                msg0(
                    color_cls,
                    sel(if i == index {
                        c"controlAccentColor"
                    } else {
                        c"labelColor"
                    }),
                ),
            );
        }
    }
    // 切替はページの表示とタブの選択表示だけ(ウィンドウサイズは全ページ共通の
    // ため遷移アニメもサイズ変更も不要)
    let page = PAGES[index].load(Ordering::Relaxed) as ID;
    if !page.is_null() {
        let window = msg0(page, sel(c"window"));
        if !window.is_null() {
            msg1_void_u8(
                msg0(window, sel(c"contentView")),
                sel(c"setNeedsDisplay:"),
                1,
            );
            msg0_void(window, sel(c"display"));
        }
    }
    focus_initial_responder(index);
}

/// ページの先頭コントロール(キーボード操作の始点)。接続(0)は接続先欄が
/// 有効な役割(この Mac が接続しに行く側)のとき HOST_FIELD、待ち受け側の間は
/// 役割ラジオの先頭(無効な欄へフォーカスしても入力できないため)。画面配置は
/// 配置キャンバス、切替は切替キー、共有はテキストと画像のスイッチ、その他は
/// 再起動ボタンを当てる
unsafe fn page_first_responder(index: usize) -> ID {
    match index {
        0 => {
            if crate::effective_client_role() || UI_PREVIEW.load(Ordering::Relaxed) {
                HOST_FIELD.load(Ordering::Relaxed) as ID
            } else {
                ROLE_RADIO[0].load(Ordering::Relaxed) as ID
            }
        }
        1 => LAYOUT_CANVAS.load(Ordering::Relaxed) as ID,
        2 => HOTKEY_POP.load(Ordering::Relaxed) as ID,
        3 => super::PREFS_CHK_CLIP.load(Ordering::Relaxed) as ID,
        _ => OTHER_RESTART.load(Ordering::Relaxed) as ID,
    }
}

/// 可視ページの先頭コントロールを初期フォーカスへ指定する。build() 内の
/// select_page(ウィンドウ表示前)では setInitialFirstResponder: だけが効き、
/// makeKeyAndOrderFront の時に初めてフォーカスが乗る。表示後のページ切替は
/// こちらから makeFirstResponder: で即座に追従させる(ページを切り替えるたびに
/// マウスを取り直さなくて済むように)
unsafe fn focus_initial_responder(index: usize) {
    let first = page_first_responder(index);
    if first.is_null() {
        return;
    }
    let window = msg0(first, sel(c"window"));
    if window.is_null() {
        return;
    }
    msg1_void_id(window, sel(c"setInitialFirstResponder:"), first);
    let is_key: unsafe extern "C" fn(ID, SEL) -> u8 =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    if is_key(window, sel(c"isKeyWindow")) != 0 {
        let make: unsafe extern "C" fn(ID, SEL, ID) -> u8 =
            std::mem::transmute(crate::objc_msgSend as *const () as usize);
        make(window, sel(c"makeFirstResponder:"), first);
    }
}
pub(super) unsafe extern "C" fn navigate(_s: ID, _c: SEL, sender: ID) {
    select_page(crate::msg0_isize(sender, sel(c"tag")).clamp(0, PAGES.len() as isize - 1) as usize);
}
pub(super) unsafe extern "C" fn hotkey(_s: ID, _c: SEL, sender: ID) {
    let i = crate::msg0_isize(sender, sel(c"indexOfSelectedItem"));
    if let Some(k) = HOTKEY_CHOICES.get(i.max(0) as usize) {
        crate::HOTKEY_KC.store(*k, Ordering::Relaxed);
        preferences::save();
    }
}

/// 滞在時間スライダの値表示。「端で少し待つ」の既定 300ms のときだけ目印を出す
fn delay_text(ms: u64) -> String {
    if ms == 300 {
        "300ms(既定)".to_string()
    } else {
        format!("{ms}ms")
    }
}

/// 「端で少し待つ」の滞在時間スライダ(50-1000ms)。値は SWITCH_DELAY_MS へ保存し、
/// 遠隔適用(Windows 側設定の delay)・起動時復元も同じ経路を使う
pub(super) unsafe extern "C" fn switch_delay(_s: ID, _c: SEL, sender: ID) {
    let f: unsafe extern "C" fn(ID, SEL) -> f64 =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    let v = f(sender, sel(c"doubleValue"));
    let ms = v.clamp(50.0, 1000.0) as u64;
    crate::SWITCH_DELAY_MS.store(ms, Ordering::Relaxed);
    set_label(&SWITCH_DELAY_LABEL, &delay_text(ms));
    preferences::save();
}
pub(super) unsafe extern "C" fn enter_peer(_s: ID, _c: SEL, _sender: ID) {
    if !UI_PREVIEW.load(Ordering::Relaxed) && !crate::WIN_MODE.load(Ordering::Relaxed) {
        crate::do_toggle("設定画面");
    }
    refresh_status();
}

/// menuWillOpen:/menuDidClose: は NSMenuDelegate の固定メソッド名のため、
/// 履歴サブメニュー(再構築の保留制御)もメニューバー本体(状態行の書き換え保留)も
/// 切替キーもこの IMP を共有する。sender(menu)で区別する
pub(super) unsafe extern "C" fn peer_menu_open(_s: ID, _c: SEL, menu: ID) {
    if super::history_menu_tracking(menu) {
        super::history_menu_opened();
        return;
    }
    if super::registry_menu_tracking(menu) {
        super::registry_menu_opened();
        return;
    }
    if super::main_menu_tracking(menu) {
        super::main_menu_opened();
        return;
    }
    set_menu_open(menu, true);
}

pub(super) unsafe extern "C" fn peer_menu_close(_s: ID, _c: SEL, menu: ID) {
    if super::history_menu_tracking(menu) {
        super::history_menu_closed();
        return;
    }
    if super::registry_menu_tracking(menu) {
        super::registry_menu_closed();
        return;
    }
    if super::main_menu_tracking(menu) {
        super::main_menu_closed();
        return;
    }
    set_menu_open(menu, false);
}

/// ポップアップ系メニューの開示中フラグの切替(sender=NSMenu でどのポップアップかを
/// 区別)。開いている間は sync() の selectItemAtIndex: を控えさせる
/// (menuDidClose: で false へ戻り、次の sync() が最新値を反映する)
unsafe fn set_menu_open(menu: ID, open: bool) {
    if !menu.is_null() && menu == HOTKEY_MENU.load(Ordering::Relaxed) as ID {
        HOTKEY_MENU_OPEN.store(open, Ordering::Relaxed);
    }
}

unsafe fn set_label(slot: &AtomicUsize, text: &str) {
    let label = slot.load(Ordering::Relaxed) as ID;
    if !label.is_null() {
        let text = localization::copy(text);
        msg1_void_id(label, sel(c"setStringValue:"), nsstring(&text));
        msg1_void_id(label, sel(c"setToolTip:"), nsstring(&text));
    }
}

/// 共有ページの最下部の説明(SHARE_HINT・純粋関数・単体テストで守る)。(本文,
/// ツールチップ)を返す。通常時の本文は「切断すると元に戻ります」の 1 文だけに
/// 削り、異常終了時の復旧(相手側は次回起動時に自動で戻す)はツールチップへ退避
/// する(困った時だけ必要な情報のため。Android 系・env 制限中の分岐は本文に
/// 情報が残り切るためツールチップは本文と同じで良い)
fn share_hint_and_tooltip(android_app: bool, android: bool) -> (&'static str, &'static str) {
    if android_app {
        (
            "ファイルはDownload/Knitへ保存します。日本語はタブレット操作中に自動で出る入力欄から送れます。",
            "ファイルはDownload/Knitへ保存します。日本語はタブレット操作中に自動で出る入力欄から送れます。",
        )
    } else if android {
        (
            "ファイルはDownloadへ保存します。タブレットの音はそのまま残ります。",
            "ファイルはDownloadへ保存します。タブレットの音はそのまま残ります。",
        )
    } else if knit_common::share::env_cap() != knit_common::share::Scope::ALL {
        (
            "環境変数 KNIT_SHARE で制限中のため、一部の項目は変更できません。",
            "環境変数 KNIT_SHARE で制限中のため、一部の項目は変更できません。",
        )
    } else {
        (
            "切断すると元に戻ります。",
            "切断すると元に戻ります。異常終了したときも、相手側は次回起動時に自動で戻します。",
        )
    }
}

/// 接続先(host)の検証エラー文言(純粋関数・単体テストで守る)。入力原文(bad)は
/// 長さが決まらないため 30 文字+「…」へ切り詰める。対処文言(192.168.1.23 の
/// 形式で…)を先頭へ置く: 保存状態行(SAVE_LABEL・幅 590px・末尾切り詰め)では
/// 原文がどれだけ長くても切れるのは必ず末尾(理由側)だけで、対処だけは必ず
/// 残る。全文は set_label がツールチップへも載せるためホバーで読める
pub(super) fn host_validation_error(bad: &str) -> String {
    format!(
        "192.168.1.23の形式で入力してください({}はIPアドレスとして読めません)",
        knit_common::history::truncate_chars(bad, 30)
    )
}

/// 役割カードの注記行の文言(ROLE_NOTE_KIND の状態から)。通常は再接続の案内、
/// KNIT_ROLE 固定中・切替の進行中はそれぞれの状況を示す
fn role_note_text() -> String {
    match ROLE_NOTE_KIND.load(Ordering::Relaxed) {
        1 => "環境変数 KNIT_ROLE で役割を固定中のため、ここでは切り替えられません".into(),
        2 => "切り替えを相手へ送りました。相手の適用を確認しています…".into(),
        3 => "相手の適用を確認しました。接続を張り直します…".into(),
        4 => "相手の適用確認が取れないため、時間経過で接続を張り直します…".into(),
        _ => "切り替えると、両方の端末で接続を切り、役割を入れ替えて自動で再接続します".into(),
    }
}

/// 状態行が「自動で再接続します」を出して良いか(純粋関数・単体テストで守る)。
/// PAIRED は接続キーの保存だけでも立つ(招待を発行したまま相手の登録がまだの
/// 状態で再起動した時など)。その間の「自動で再接続します(相手側アプリの起動を
/// 確認)」は永久に繋がらない嘘になるため、接続中 PEERS と peer-sides.json の
/// 保存件数で登録の実体が 1件でもあるときだけ true にする。
/// メニュー(gui.rs update_state_item)と設定画面(sync)の状態行が共用する
pub(super) fn paired_registered(paired: bool, peers_live: usize, saved: usize) -> bool {
    paired && (peers_live > 0 || saved > 0)
}

/// 保存エラーを出元の欄の隣の注記行へ出す(検証エラー・保存失敗の共通導線)。
/// 通知センターの表示は数秒で消えるため、設定ウィンドウを開いているときの
/// 一次情報はこちらが担う(通知との併用も可)。出元(source)で出す行が決まる:
/// 接続先の失敗がトークン欄の下に出るのを防ぐ
pub(super) fn set_save_error(source: SaveErrorSource, note: String) {
    let mut errors = SAVE_ERROR_NOTE.lock().unwrap_or_else(|e| e.into_inner());
    errors.retain(|(existing, _)| *existing != source);
    errors.push((source, note));
}

/// 出元ごとのエラー解除の判定(純粋関数・単体テストで守る)。出元が一致するとき
/// だけ None へ=他方の出元の未解決エラーは残す
fn cleared_save_error(
    source: SaveErrorSource,
    current: Option<(SaveErrorSource, String)>,
) -> Option<(SaveErrorSource, String)> {
    match current {
        Some((s, _)) if s == source => None,
        other => other,
    }
}

/// 保存が成功したら「その出元の」エラーだけを外す(「次の保存成功まで保持」の
/// 解除側)。出元が違うエラーは未解決のまま残す(接続先の保存成功がトークン欄の
/// 未解決エラーまで消すと、直すべき入力の理由が画面から消えるため)
pub(super) fn clear_save_error(source: SaveErrorSource) {
    let mut note = SAVE_ERROR_NOTE.lock().unwrap_or_else(|e| e.into_inner());
    *note = std::mem::take(&mut *note).into_iter()
        .filter_map(|error| cleared_save_error(source, Some(error)))
        .collect();
}

fn save_error_display(language: Language, errors: &[(SaveErrorSource, String)]) -> Option<String> {
    if errors.is_empty() { return None; }
    Some(errors.iter().rev().map(|(_, note)| localization::copy_for(language, note))
        .collect::<Vec<_>>().join(" · "))
}

/// 接続ページの状態行(PREFS_STATE)の文言(純粋関数・単体テストで守る)。
/// rtt は往復遅延の測定値(0=未測定)。manual は手動接続(KNIT_TOKEN)運用中:
/// この待機は「自動で再接続します」ではなく、トークンが合っていればすぐ
/// 繋がる待ち状態のため文言を分ける(通常の未接続と同じ表示だと、相手側の
/// トークン不一致に気づけない)。
/// paired は登録の実体があるか(paired_registered: 接続キーの保存だけが残る
/// 招待だけの状態は false)。未登録(!paired)の未接続は再接続の待ちではなく登録が
/// まだ無いため、「自動で再接続します」を出さず登録への導線を出す(メニューバーの
/// 状態行 gui.rs と同じ文言)
/// retry と missing は未接続が続くときの補足(メニューバーの状態行と同じ
/// next_retry_line/not_found_line の文言を再利用): 設定画面だけ見ている人にも
/// 「再試行まで 3秒」「相手を 3分見つけられません」が読み取れるように括弧内へ連結する
fn connection_state_text(
    preview: bool,
    connected: bool,
    rtt: u64,
    manual: bool,
    paired: bool,
    retry: Option<&str>,
    missing: Option<&str>,
) -> String {
    if preview {
        "設定画面のプレビュー".into()
    } else if connected && rtt > 0 {
        format!("接続済み({}・遅延 {rtt}ms)", crate::route_label())
    } else if connected {
        "接続済み".into()
    } else if !manual && !paired {
        // 一度も登録していない人には「自動で再接続します」は嘘になるため、
        // 登録への導線を出す(再試行の待ちも出ない: 登録が無ければ再試行の
        // 相手が存在しないため)
        "未接続(この画面の「端末を登録…」から相手と登録できます)".into()
    } else {
        // 未接続時の基本文言(手動接続か通常か)に、待ちの状態を括弧内へ「・」で
        // 連結する(例: 未接続(自動で再接続します・再試行まで 3秒))。
        // 補足は 1 行(幅 540px)に収めるため優先度の高い方だけを出す:
        // 「N分見つけられません」(1 分超の断)は次の試行の待ち秒より状況を
        // 説明する情報のため優先し、出ている間は retry を並記しない
        let inner = if manual {
            "直接つなぐで待機中"
        } else {
            "自動で再接続します"
        };
        match missing.or(retry) {
            Some(extra) => format!("未接続({inner}・{extra})"),
            None => format!("未接続({inner})"),
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn connection_state_display(language: Language, preview: bool, connected: bool, rtt: u64, manual: bool, paired: bool, retry: Option<&str>, missing: Option<&str>) -> String {
    if language == Language::Japanese {
        return connection_state_text(preview, connected, rtt, manual, paired, retry, missing);
    }
    if preview { return "Settings preview".into(); }
    if connected {
        return if rtt == 0 { "Connected".into() } else { format!("Connected · {} · {rtt}ms", localization::copy_for(language, crate::route_label())) };
    }
    if !manual && !paired { return "Not connected · choose Pair a Device below".into(); }
    let base = if manual { "Not connected · waiting for direct connection" } else { "Not connected · reconnecting automatically" };
    match missing.or(retry) {
        Some(extra) => format!("{base} · {}", localization::wait_detail_for(language, extra)),
        None => base.into(),
    }
}

/// 接続ページの Android 行(ANDROID_STATE)の文言(純粋関数・単体テストで守る)。
/// android_app は接続済み端末が Android アプリ版のとき: 権限(画面操作・キー
/// ボード)の状態に応じて案内が変わる(許可待ち→許可を促す、画面操作のみ許可→
/// キーボードの選択を促す、両方許可→許可済み)。この行が許可前後を通じて唯一の
/// 案内を担う。
/// それ以外は adb 接続(Android)の状況を出す: 全端末共通の問題(problem)を
/// 優先し、無ければ先頭タブレットの状態(name・summary・台数)。どちらの材料も
/// 無いときは空文字=行が見えない(ANDROID_STATE は状況があった時だけ出る行)
fn android_state_text(
    android_app: bool,
    permissions: Option<(bool, bool)>,
    problem: Option<&str>,
    tablet: Option<(&str, String, usize)>,
) -> String {
    if android_app {
        match permissions {
            Some((true, true)) => "Androidアプリ:画面操作・キーボードを許可済み".into(),
            Some((true, false)) => {
                "Androidアプリ:文字入力にはKnitキーボードを選択してください".into()
            }
            _ => "Androidアプリ:タブレットで画面操作を許可してください".into(),
        }
    } else if let Some(problem) = problem {
        format!("Android: {problem}")
    } else if let Some((name, summary, count)) = tablet {
        format!("Android: {name} — {summary}({count}台)")
    } else {
        String::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn english_connection_states_keep_pairing_and_retry_semantics() {
        let en = Language::English;
        assert_eq!(connection_state_display(en, true, false, 0, false, false, None, None), "Settings preview");
        assert_eq!(connection_state_display(en, false, true, 0, false, false, None, None), "Connected");
        assert_eq!(connection_state_display(en, false, false, 0, false, false, Some("再試行まで 3秒"), None), "Not connected · choose Pair a Device below");
        assert_eq!(connection_state_display(en, false, false, 0, false, true, Some("再試行まで 3秒"), None), "Not connected · reconnecting automatically · Retrying in 3s");
        assert_eq!(connection_state_display(en, false, false, 0, true, false, Some("再試行まで 3秒"), Some("相手を 5分見つけられません")), "Not connected · waiting for direct connection · Device not found for 5 min");
    }

    /// 状態行: 手動接続(トークン)運用中の未接続は「自動で再接続します」ではなく
    /// 待機中であることが読み取れる文言になる(トークン不一致に気づけるように)。
    /// 14pt・幅 540px に収まることも守る
    #[test]
    fn connection_state_text_manual_waiting() {
        assert_eq!(
            connection_state_text(false, false, 0, true, true, None, None),
            "未接続(直接つなぐで待機中)"
        );
        assert_eq!(
            connection_state_text(false, false, 0, false, true, None, None),
            "未接続(自動で再接続します)"
        );
        assert_eq!(
            connection_state_text(false, true, 0, true, false, Some("再試行まで 3秒"), None),
            "接続済み",
            "接続済みなら待ちの補足は無視する"
        );
        assert_eq!(
            connection_state_text(false, true, 12, false, false, None, None),
            format!("接続済み({}・遅延 12ms)", crate::route_label())
        );
        let width = connection_state_text(false, false, 0, true, true, None, None)
            .chars()
            .map(|c| if c.is_ascii() { 8 } else { 14 })
            .sum::<usize>();
        assert!(width <= 540, "状態行(14pt)に収まる: {width}px");
    }

    /// 状態行(未接続): 再試行までの残りと「見つけられない」の継続が括弧内へ
    /// 連結される(メニューバーの状態行と同じ文言・設定画面だけ見ていても
    /// 待ちの見通しが立つ)。両方出ているときは状況を説明する「見つけられない」
    /// を優先し、どの組合せでも 14pt・幅 540px の 1 行に収まることを守る
    #[test]
    fn connection_state_text_appends_retry_and_missing() {
        // 再試行の待ちだけ出ているとき(クライアントモードのバックオフ中)
        assert_eq!(
            connection_state_text(false, false, 0, false, true, Some("再試行まで 3秒"), None),
            "未接続(自動で再接続します・再試行まで 3秒)"
        );
        // 見つけられない継続(1 分超)が出ているときはこちらを優先(1 行に収める)
        assert_eq!(
            connection_state_text(
                false,
                false,
                0,
                false,
                true,
                Some("再試行まで 3秒"),
                Some("相手を 3分見つけられません")
            ),
            "未接続(自動で再接続します・相手を 3分見つけられません)"
        );
        // missing のみ(サーバモードで待ち受け続けている長期断)でも出る
        assert_eq!(
            connection_state_text(false, false, 0, false, true, None, Some("相手を 5分見つけられません")),
            "未接続(自動で再接続します・相手を 5分見つけられません)"
        );
        // 手動接続の待機にも同じ補足が付く(トークン不一致でも待ちの見通しは出る)
        assert_eq!(
            connection_state_text(false, false, 0, true, true, Some("再試行まで 10秒"), None),
            "未接続(直接つなぐで待機中・再試行まで 10秒)"
        );
        // 幅: 実運用で最長になる組合せ(手動+見つけられない)でも 1 行に収まる
        let longest = connection_state_text(
            false,
            false,
            0,
            true,
            true,
            Some("再試行まで 999秒"),
            Some("相手を 999分見つけられません"),
        );
        let width = longest
            .chars()
            .map(|c| if c.is_ascii() { 8 } else { 14 })
            .sum::<usize>();
        assert!(width <= 540, "状態行(14pt)に収まる: {longest} ({width}px)");
    }

    /// 状態行(未接続・未登録): 一度も端末登録が済んでいない(paired=false)ときは
    /// 「自動で再接続します」の嘘を出さず、登録への導線(メニューバーの状態行と
    /// 同じ文言)へ切り替わる。再試行の補足が出ていても導線を優先し、
    /// 手動接続(トークン)運用中は登録よりトークンの待機を優先する。14pt・幅 540px
    /// に収まることも守る
    #[test]
    fn connection_state_text_unpaired_shows_registration_guide() {
        assert_eq!(
            connection_state_text(false, false, 0, false, false, None, None),
            "未接続(この画面の「端末を登録…」から相手と登録できます)"
        );
        // 再試行の補足が出ていても「再接続します」は出ない(登録がまだ無いため)
        assert_eq!(
            connection_state_text(false, false, 0, false, false, Some("再試行まで 3秒"), None),
            "未接続(この画面の「端末を登録…」から相手と登録できます)"
        );
        // 手動接続(トークン)運用中は待機の文言を優先(トークンが合っていれば
        // 繋がる待ちであり、登録の導線はトークン運用の邪魔になるため)
        assert_eq!(
            connection_state_text(false, false, 0, true, false, None, None),
            "未接続(直接つなぐで待機中)"
        );
        let width = connection_state_text(false, false, 0, false, false, None, None)
            .chars()
            .map(|c| if c.is_ascii() { 8 } else { 14 })
            .sum::<usize>();
        assert!(width <= 540, "状態行(14pt)に収まる: {width}px");
    }

    /// Android 行(ANDROID_STATE・幅 540px・11pt): Androidアプリ接続の権限状態に
    /// 応じて案内が切り替わる(許可待ち→許可を促す、画面操作のみ許可→キーボード
    /// の選択を促す、両方許可→許可済み)。許可前後の案内はこの行が唯一担う。
    /// adb 接続では全端末共通の問題を優先し、無ければ先頭タブレットの状態
    /// (name・summary・台数。summary は Phase::summary() の String をそのまま渡す)。
    /// どちらの材料も無いときは空文字=行が見えない(ANDROID_STATE は状況があった
    /// 時だけ出る行)
    #[test]
    fn android_state_text_switches_by_permission() {
        let px = |s: &str| s.chars().map(|c| if c.is_ascii() { 6 } else { 11 }).sum::<usize>();
        // Androidアプリ: 権限の 3 状態(未許可・画面操作のみ・両方許可)
        assert_eq!(
            android_state_text(true, None, None, None),
            "Androidアプリ:タブレットで画面操作を許可してください"
        );
        assert_eq!(
            android_state_text(true, Some((false, false)), None, None),
            "Androidアプリ:タブレットで画面操作を許可してください"
        );
        assert_eq!(
            android_state_text(true, Some((true, false)), None, None),
            "Androidアプリ:文字入力にはKnitキーボードを選択してください"
        );
        assert_eq!(
            android_state_text(true, Some((true, true)), None, None),
            "Androidアプリ:画面操作・キーボードを許可済み"
        );
        // adb 接続: 共通の問題があれば優先、無ければ先頭タブレットの状態(台数つき)
        assert_eq!(
            android_state_text(false, None, Some("adb が見つかりません"), None),
            "Android: adb が見つかりません"
        );
        assert_eq!(
            android_state_text(false, None, None, Some(("Pixel Tablet", "操作できます".into(), 2))),
            "Android: Pixel Tablet — 操作できます(2台)"
        );
        assert_eq!(android_state_text(false, None, None, None), "", "材料なしは空=非表示");
        // どの文言も 1 行(幅 540px・11pt)に収まる
        for text in [
            android_state_text(true, None, None, None),
            android_state_text(true, Some((true, false)), None, None),
            android_state_text(false, None, Some("adb が見つかりません"), None),
            android_state_text(false, None, None, Some(("Pixel Tablet", "接続できません(長い理由)".into(), 12))),
        ] {
            assert!(px(&text) <= 540, "Android 行(540px・11pt)に収まる: {text} ({}px)", px(&text));
        }
    }

    /// 共有ページの最下部の説明: 通常時の本文は「切断すると元に戻ります」の 1 文
    /// だけに削り、異常終了時の復旧(相手側は次回起動時に自動で戻す)はツールチップ
    /// が担う(本文は幅 565px・12pt の 1 行に収まる)
    #[test]
    fn share_hint_keeps_crash_recovery_in_tooltip() {
        let (text, tip) = share_hint_and_tooltip(false, false);
        assert_eq!(text, "切断すると元に戻ります。");
        assert!(!text.contains("異常終了"), "異常終了時の案内は本文へ出さない: {text}");
        assert!(
            tip.contains("異常終了したときも、相手側は次回起動時に自動で戻します"),
            "復旧の完全版をツールチップが担う: {tip}"
        );
        let px12 = |s: &str| {
            s.chars()
                .map(|c| if c.is_ascii() { 6 } else { 12 })
                .sum::<usize>()
        };
        assert!(px12(text) <= 565, "本文(565px・12pt)に収まる: {text} ({}px)", px12(text));
    }

    /// 接続先の検証エラー: 入力原文(bad)は 30 文字+「…」へ切り詰め、対処文言が
    /// 保存状態行(SAVE_LABEL・幅 590px・末尾切り詰め)から押し出されない
    #[test]
    fn host_validation_error_keeps_the_fix_within_save_label() {
        let px = |s: &str| s.chars().map(|c| if c.is_ascii() { 6 } else { 11 }).sum::<usize>();
        let fix = "192.168.1.23の形式で入力してください";
        // ASCII のワーストケース(IPv6 の貼り損じ等・75 文字)でも行全体が 590px 内
        let ascii = host_validation_error(&"9".repeat(75));
        assert!(
            ascii.contains(&format!("{}…", "9".repeat(30))),
            "30 文字+「…」へ切る: {ascii}"
        );
        assert!(px(&ascii) <= 590, "ASCII 原文でも 1 行に収まる: {ascii} ({}px)", px(&ascii));
        // 全角 60 文字を貼ったワーストケース: 行は 590px を超え得るが、対処文言は
        // 先頭にあり必ず表示内に残る(切れるのは末尾の理由側だけ)
        let wide = host_validation_error(&"あ".repeat(60));
        assert!(wide.starts_with(fix), "対処文言が先頭: {wide}");
        assert!(px(fix) <= 590, "対処文言自体は必ず残る長さ: {}px", px(fix));
        // 短い原文は切らない
        assert_eq!(
            host_validation_error("192.168.o.1"),
            "192.168.1.23の形式で入力してください(192.168.o.1はIPアドレスとして読めません)"
        );
    }

    /// 切替ページの注記: 固定キーが無いときは空(=行が見えない)、あるときは
    /// キー名と優先される理由が出る。キーは save_status_saved と同じ先頭3件+
    /// 残件数へ丸め、全キー固定でも 1 行(ラベル幅 565px)に収まる。初期化の導線は
    /// 全ページ共通の保存状態行(SAVE_LABEL)が出すため、この行では繰り返さない
    /// (上書きの意味と手順は operation_env_note_tooltip が担う)
    #[test]
    fn operation_env_note_names_fixed_keys() {
        assert_eq!(operation_env_note(&[]), "", "固定なしは空=非表示");
        let text = operation_env_note(&["KNIT_SWITCH_MODE", "KNIT_SCROLL_DIV"]);
        assert!(text.contains("KNIT_SWITCH_MODE・KNIT_SCROLL_DIV"), "{text}");
        assert!(text.contains("固定中"), "優先される理由も出す: {text}");
        assert!(!text.contains("初期化"), "初期化の導線は保存状態行が担う: {text}");
        // 4 件以上は先頭3件+残件数へ丸める(丸めないと 1 行に収まらないため)
        let many = operation_env_note(&OPERATION_ENV_KEYS);
        assert!(
            many.contains("KNIT_SWITCH_MODE・KNIT_HOTKEY_KC・KNIT_SWITCH_DELAY ほか6件"),
            "先頭3件+残件数へ丸める: {many}"
        );
        assert!(!many.contains("初期化"), "丸めても導線は保存状態行のまま: {many}");
        assert!(!many.contains("KNIT_EDGE_TAPS"), "4件目以降は並べない: {many}");
        // 11pt のラベル幅 565px に収まる(全角 11px / ASCII 6px 換算)
        for text in [text, many] {
            let width = text
                .chars()
                .map(|c| if c.is_ascii() { 6 } else { 11 })
                .sum::<usize>();
            assert!(width <= 565, "1 行に収まる: {text} ({width}px)");
        }
    }

    /// 保存状態ラベル: 固定キーが無い/少ない/多いの3状態。多いときは
    /// 先頭2件+残件数に丸めて 1 行に収める。対処案(初期化の導線)は
    /// 件数に関係なく「環境変数」の語と一緒に出る。固定なしのときは自動保存の
    /// 対象だけを言う(入力欄がボタン保存であることは欄の横のボタン名が伝える)
    #[test]
    fn save_status_initial_lists_keys_and_compacts_many() {
        assert_eq!(save_status_initial(&[]), "スイッチ・スライダーは自動保存");
        assert_eq!(
            save_status_initial(&["KNIT_ROLE"]),
            "自動保存・起動時は環境変数 KNIT_ROLE が優先(初期化は左下の「その他」)"
        );
        assert_eq!(
            save_status_initial(&["KNIT_ROLE", "KNIT_SHARE"]),
            "自動保存・起動時は環境変数 KNIT_ROLE・KNIT_SHARE が優先(初期化は左下の「その他」)"
        );
        let many = save_status_initial(&[
            "KNIT_ROLE", "KNIT_SHARE", "KNIT_CLIP", "KNIT_SIDE", "KNIT_AUDIO",
        ]);
        assert!(
            many.starts_with("自動保存・起動時は環境変数 KNIT_ROLE・KNIT_SHARE ほか3件"),
            "先頭2件+残件数へ丸める: {many}"
        );
        assert!(many.contains("初期化は左下の「その他」"), "丸めても対処導線が消えない: {many}");
        assert!(!many.contains("KNIT_CLIP"), "3件目以降は並べない: {many}");
    }

    /// 保存直後のステータスも初期表示と同じ形式(環境変数・対処案の導線つき)で
    /// 出る: 件数の分岐で対処案が消えないようにする
    #[test]
    fn save_status_saved_keeps_env_wording_and_reset_hint() {
        assert_eq!(save_status_saved(&[]), "変更を保存しました");
        for keys in [
            vec!["KNIT_ROLE"],
            vec!["KNIT_ROLE", "KNIT_SHARE"],
            vec!["KNIT_ROLE", "KNIT_SHARE", "KNIT_CLIP", "KNIT_SIDE"],
        ] {
            let text = save_status_saved(&keys);
            assert!(text.starts_with("保存済み・起動時は環境変数 "), "{text}");
            assert!(text.contains("初期化は左下の「その他」"), "対処案の導線も出る: {text}");
            assert!(!text.contains("画面最下部"), "位置の説明は付けない: {text}");
        }
    }

    /// 保存状態行(SAVE_LABEL・幅 590px・11pt): 環境変数の固定キーが何件あっても
    /// 対処導線「(初期化は…)」まで 1 行に収まる(先頭3件を出していた頃は2件の
    /// 設定で 590px を超え、導線ごと切れていた)。ワーストケースとして、実在の
    /// キーで最長の組(16+18 ASCII)と、幅ガードで 1件へ縮退する極端な組
    /// (16+24 ASCII)の両方で守る
    #[test]
    fn save_status_lines_fit_590px_with_any_key_count() {
        let px11 = |s: &str| {
            s.chars()
                .map(|c| if c.is_ascii() { 6 } else { 11 })
                .sum::<usize>()
        };
        // 1件〜21件(ENV_OVERRIDE_KEYS の全数)まで、現実のキー名の並びで検証
        let realistic = [
            "KNIT_SIDE", "KNIT_SWITCH_MODE", "KNIT_EDGE_TAPS", "KNIT_HOTKEY_KC",
            "KNIT_SWITCH_DELAY", "KNIT_DOUBLE_TAP_MS", "KNIT_SCROLL_DIV",
            "KNIT_MOUSE_SCALE", "KNIT_SCROLL_FLIP", "KNIT_SCROLL_COMPAT",
            "KNIT_CMD_ALT", "KNIT_MUTE_SPK", "KNIT_CLIP", "KNIT_ROLE",
            "KNIT_LOCAL_HISTORY", "KNIT_AUDIO", "KNIT_AUDIO_GAIN", "KNIT_ANDROID",
            "KNIT_ANDROID_ADB", "KNIT_ANDROID_GAIN", "KNIT_ANDROID_SCROLL_FLIP",
        ];
        // 先頭2件が最長になるよう長いキーを先頭へ置いたワースト並び(幅ガードの
        // 縮退が効く組=16+24 ASCII と、効かない組=16+18 ASCII の両方を含む)
        let worst = [
            "KNIT_SWITCH_MODE", "KNIT_ANDROID_SCROLL_FLIP", "KNIT_DOUBLE_TAP_MS",
            "KNIT_LOCAL_HISTORY", "KNIT_AUDIO_GAIN",
        ];
        for keys in [
            realistic.as_slice(),
            worst.as_slice(),
        ] {
            for n in 1..=keys.len() {
                for text in [
                    save_status_initial(&keys[..n]),
                    save_status_saved(&keys[..n]),
                ] {
                    assert!(
                        px11(&text) <= 590,
                        "保存状態行(590px・11pt)に収まる: {text} ({}px)",
                        px11(&text)
                    );
                    assert!(
                        text.contains("初期化は左下の「その他」"),
                        "どの件数でも対処導線が残る: {text}"
                    );
                }
            }
        }
    }

    /// キー列の丸め: 幅ガード(env_keys_summary)。先頭2件の概算幅が 200px 以下の
    /// 並びでは2件+ほかN件、超える並びでは1件+ほかN件へ縮退する(どの組合せでも
    /// 行の幅に収めるための分岐)
    #[test]
    fn env_keys_summary_falls_back_to_one_key_for_wide_pairs() {
        // 通常の組(先頭2件で 161px)は2件のまま
        assert_eq!(
            env_keys_summary(&["KNIT_SIDE", "KNIT_SWITCH_MODE", "KNIT_CLIP"]),
            "KNIT_SIDE・KNIT_SWITCH_MODE ほか1件"
        );
        // 広い組(96+11+144=251px)は1件へ縮退する
        let wide = env_keys_summary(&[
            "KNIT_SWITCH_MODE",
            "KNIT_ANDROID_SCROLL_FLIP",
            "KNIT_CLIP",
            "KNIT_ROLE",
        ]);
        assert!(wide.starts_with("KNIT_SWITCH_MODE ほか3件"), "1件+残件数へ縮退: {wide}");
        assert!(!wide.contains("KNIT_ANDROID_SCROLL_FLIP"), "広い2件目は並べない: {wide}");
        // 2件以下は丸めない(ほかN件を付けない)
        assert_eq!(env_keys_summary(&["KNIT_ROLE", "KNIT_SHARE"]), "KNIT_ROLE・KNIT_SHARE");
        assert_eq!(env_keys_summary(&["KNIT_ROLE"]), "KNIT_ROLE");
    }

    /// 状態行の paired 判定: PAIRED が立っていても、接続中・保存済みのどちらにも
    /// 端末が無ければ「自動で再接続します」は出させない(招待だけの状態)
    #[test]
    fn paired_registered_requires_peers_or_saved() {
        assert!(paired_registered(true, 1, 0), "接続中の端末があれば実体がある");
        assert!(paired_registered(true, 0, 2), "保存済みだけでも実体がある");
        assert!(
            !paired_registered(true, 0, 0),
            "招待を発行したまま相手の登録がまだ(PAIRED だけ立っている)は案内しない"
        );
        assert!(!paired_registered(false, 0, 0), "そもそも未登録");
    }

    /// 再生音量の値表示: 既定の 100% には「(既定)」の目印が付き、他の値には
    /// 付かない(丸め誤差で 100 に届く値も既定として出す=スライダ位置の実感と
    /// 表示がぶれないように round 後の値で判定する)
    #[test]
    fn gain_text_marks_default_volume() {
        assert_eq!(gain_text(1.0), "100%(既定)");
        assert_eq!(gain_text(0.999), "100%(既定)", "丸めて 100 なら既定扱い");
        assert_eq!(gain_text(0.5), "50%");
        assert_eq!(gain_text(1.5), "150%");
        assert_eq!(gain_text(0.0), "0%");
        assert_eq!(gain_text(2.0), "200%");
    }

    /// 滞在時間の値表示: 「端で少し待つ」の既定 300ms には「(既定)」の目印が
    /// 付き、他の値には付かない
    #[test]
    fn delay_text_marks_default_delay() {
        assert_eq!(delay_text(300), "300ms(既定)");
        assert_eq!(delay_text(299), "299ms");
        assert_eq!(delay_text(301), "301ms");
        assert_eq!(delay_text(50), "50ms", "スライダの最小値");
        assert_eq!(delay_text(1000), "1000ms", "スライダの最大値");
    }

    /// エラー解除は出元ごと: 一致する出元のエラーだけが消え、他の出元の
    /// 未解決エラーは残る(ある出元の保存成功が、別の出元の直すべき理由まで
    /// 画面から消すのを防ぐ)
    #[test]
    fn cleared_save_error_clears_only_matching_source() {
        let host_err = (SaveErrorSource::Host, "接続先を保存できませんでした".to_string());
        assert_eq!(cleared_save_error(SaveErrorSource::Host, Some(host_err.clone())), None);
        assert_eq!(
            cleared_save_error(SaveErrorSource::PrefsFile, Some(host_err.clone())),
            Some(host_err),
            "設定ファイル起因の解除は Host のエラーを消さない"
        );
        let file_err = (
            SaveErrorSource::PeerSidesFile,
            "端末の配置の保存が壊れていたため退避しました".to_string(),
        );
        assert_eq!(cleared_save_error(SaveErrorSource::PeerSidesFile, Some(file_err.clone())), None);
        assert_eq!(
            cleared_save_error(SaveErrorSource::Host, Some(file_err.clone())),
            Some(file_err),
            "接続先の保存成功は配置ファイルのエラーを消さない"
        );
        assert_eq!(cleared_save_error(SaveErrorSource::Host, None), None);
        assert_eq!(cleared_save_error(SaveErrorSource::PrefsFile, None), None);
    }

    #[test]
    fn preferences_save_recovery_does_not_erase_unresolved_host_error() {
        let before = SAVE_ERROR_NOTE.lock().unwrap_or_else(|e| e.into_inner()).clone();
        set_save_error(SaveErrorSource::Host, "Host validation fixture".into());
        set_save_error(SaveErrorSource::PrefsFile, "Preferences write fixture".into());
        clear_save_error(SaveErrorSource::PrefsFile);
        let after = format!("{:?}", *SAVE_ERROR_NOTE.lock().unwrap_or_else(|e| e.into_inner()));
        *SAVE_ERROR_NOTE.lock().unwrap_or_else(|e| e.into_inner()) = before;
        assert!(after.contains("Host validation fixture"), "unresolved Host error was lost: {after}");
    }
}

/// 現在の切替方式(env・遠隔適用・preferences 復元で決まる)。方式を選ぶUIは
/// 無いため表示はせず、sync() が滞在時間スライダ(方式 1 だけ意味がある)の
/// 表示判定に使う
fn method_index() -> i64 {
    if crate::HOTKEY_ONLY.load(Ordering::Relaxed) { 2 }
    else if crate::SWITCH_DELAY_MS.load(Ordering::Relaxed) > 0 { 1 }
    else if crate::EDGE_TAPS.load(Ordering::Relaxed) == 1 { 3 }
    else { 0 }
}

pub(super) unsafe extern "C" fn return_mac(_s: ID, _c: SEL, _sender: ID) {
    if UI_PREVIEW.load(Ordering::Relaxed) {
        return;
    }
    if crate::WIN_MODE.swap(false, Ordering::Relaxed) {
        crate::leave_win_mode_cursor_unlock(None);
    }
    refresh_status();
}

/// 音量スライダの値表示(1.0 = 100%)。既定の 100%(audio の初期値)のときだけ
/// 目印を出す(初期値のまま触っていないことが分かるように)
fn gain_text(g: f32) -> String {
    let pct = (g * 100.0).round() as i32;
    if pct == 100 {
        "100%(既定)".to_string()
    } else {
        format!("{pct}%")
    }
}

/// 音声の再生音量スライダ(受信サンプルへのソフトゲイン。Windows 操作中は
/// Mac の音量キーが Windows 側へ転送されるため、Knit 内で完結する調整口)
pub(super) unsafe extern "C" fn audio_gain(_s: ID, _c: SEL, sender: ID) {
    let f: unsafe extern "C" fn(ID, SEL) -> f64 =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    let v = f(sender, sel(c"doubleValue"));
    crate::audio::set_gain(v);
    set_label(&AUDIO_GAIN_LABEL, &gain_text(crate::audio::gain()));
    preferences::save();
}
pub(super) unsafe fn set_save_status(text: &str) {
    *LAST_SAVE_STATUS.lock().unwrap_or_else(|e| e.into_inner()) = text.into();
    let error = SAVE_ERROR_NOTE.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let error = save_error_display(localization::language(), &error);
    set_label(&SAVE_LABEL, error.as_deref().unwrap_or(text));
}
/// 「接続先の端末」入力欄(保存ハンドラが本文を読み取る用)
pub(super) fn host_field() -> usize {
    if refined::is_built() { return refined::host_field(); }
    HOST_FIELD.load(Ordering::Relaxed)
}

pub(super) unsafe fn sync() {
    if refined::is_built() {
        refined::sync();
        return;
    }
    sync_role();
    let preview = UI_PREVIEW.load(Ordering::Relaxed);
    let connected = crate::CONNECTED.load(Ordering::Relaxed);
    let android = !preview && crate::active_peer_is_android();
    let android_app = !preview && crate::active_peer_is_android_app();
    let enable = |slot: &AtomicUsize, enabled: bool| {
        let view = slot.load(Ordering::Relaxed) as ID;
        if !view.is_null() { msg1_void_u8(view, sel(c"setEnabled:"), enabled as u8); }
    };
    let method = method_index();
    // 「端で少し待つ」の滞在時間スライダ(方式 1 のときだけ表示)。
    // 遠隔適用・起動時復元の値もここで画面へ反映する(AUDIO_GAIN_SLIDER と同じ)
    for v in SWITCH_DELAY_VIEWS.lock().unwrap_or_else(|e| e.into_inner()).iter() {
        let v = *v as ID;
        if !v.is_null() { msg1_void_u8(v, sel(c"setHidden:"), (method != 1) as u8); }
    }
    let delay_slider = SWITCH_DELAY_SLIDER.load(Ordering::Relaxed) as ID;
    if !delay_slider.is_null() && method == 1 {
        let get: unsafe extern "C" fn(ID, SEL) -> f64 =
            std::mem::transmute(crate::objc_msgSend as *const () as usize);
        let set: unsafe extern "C" fn(ID, SEL, f64) =
            std::mem::transmute(crate::objc_msgSend as *const () as usize);
        let ms = crate::SWITCH_DELAY_MS.load(Ordering::Relaxed) as f64;
        if get(delay_slider, sel(c"doubleValue")) != ms {
            set(delay_slider, sel(c"setDoubleValue:"), ms);
        }
        set_label(&SWITCH_DELAY_LABEL, &delay_text(crate::SWITCH_DELAY_MS.load(Ordering::Relaxed)));
    }
    // 切替キーポップアップも同じく開示中は書き換えない
    let keys = HOTKEY_POP.load(Ordering::Relaxed) as ID;
    if !keys.is_null() && !HOTKEY_MENU_OPEN.load(Ordering::Relaxed) {
        let current = crate::hotkey_kc();
        // 未知のキー(env で指定)は末尾の「現在のキー(…)」項目を選ぶ
        let index = HOTKEY_CHOICES
            .iter()
            .position(|k| *k == current)
            .unwrap_or(HOTKEY_CHOICES.len());
        msg1_void_i64(keys, sel(c"selectItemAtIndex:"), index as i64);
    }
    // KNIT_SHARE で禁じられた項目は、設定画面から許可できない
    let cap = knit_common::share::env_cap();
    enable(&super::PREFS_CHK_CLIP, cap.clip && !preview);
    enable(&super::PREFS_CHK_FILES, cap.files && !preview);
    enable(&super::PREFS_CHK_HISTORY, cap.clip && !preview);
    // 「切替」ページの環境変数固定の注記(KNIT_SWITCH_MODE 等)は env 起因で実行中
    // 変わらないため、build 時に 1 回だけ設定する(毎秒の sync() では触らない)
    let audio_ok = knit_common::share::env_cap().audio;
    enable(&super::PREFS_CHK_AUDIO, !android_app && !preview && audio_ok);
    let rtt = crate::RTT_MS.load(Ordering::Relaxed);
    // 手動接続(KNIT_TOKEN)運用中か(状態行の文言が参照)
    let manual = knit_common::envutil::get("KNIT_TOKEN").is_some_and(|t| !t.is_empty());
    // 未接続が続くときの補足(再試行までの残り・見つけられない継続)も接続ページの
    // 状態行へ出す: メニューバー(gui.rs refresh_status)と同じ文言の関数から取り、
    // 設定画面だけを見ている人にも待ちの見通しが読み取れるようにする。
    // paired は PAIRED のまま渡さず登録の実体(paired_registered)へ通す: 接続キーの
    // 保存だけが残る状態で「自動で再接続します」を出さない(メニュー状態行と同じ)
    let state = connection_state_display(
        localization::language(),
        preview,
        connected,
        rtt,
        manual,
        paired_registered(
            crate::PAIRED.load(Ordering::Relaxed),
            crate::PEERS.lock().unwrap_or_else(|e| e.into_inner()).len(),
            crate::tap::saved_peer_count(),
        ),
        crate::next_retry_line().as_deref(),
        crate::not_found_line().as_deref(),
    );
    set_label(&PREFS_STATE, &state);
    let state_label = PREFS_STATE.load(Ordering::Relaxed) as ID;
    if !state_label.is_null() {
        let color = msg0(objc_getClass(c"NSColor".as_ptr()), sel(if connected { c"labelColor" } else { c"secondaryLabelColor" }));
        msg1_void_id(state_label, sel(c"setTextColor:"), color);
    }
    // 保存状態行(全ページ共通・最下部)に保存エラーを出す。通常時の文言は
    // preferences::save() と初期構築が管理する=エラー保持中だけここで書き直す。
    // set_label がツールチップにも同じ文言を載せるため、1 行に収まらない長い
    // エラーもホバーで読める
    let save_error = SAVE_ERROR_NOTE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    let save_error = save_error_display(localization::language(), &save_error);
    if let Some(err) = &save_error {
        set_label(&SAVE_LABEL, err);
    } else {
        let status = LAST_SAVE_STATUS.lock().unwrap_or_else(|e| e.into_inner()).clone();
        if !status.is_empty() { set_label(&SAVE_LABEL, &status); }
    }
    // エラー保持中は保存状態行を警告色へ(どのページにもいるため、最下部でも
    // 見落とさないように。エラーが無ければ通常の控えめな色へ戻す)
    let save_label = SAVE_LABEL.load(Ordering::Relaxed) as ID;
    if !save_label.is_null() {
        let color = msg0(
            objc_getClass(c"NSColor".as_ptr()),
            sel(if save_error.is_some() {
                c"systemRedColor"
            } else {
                c"secondaryLabelColor"
            }),
        );
        msg1_void_id(save_label, sel(c"setTextColor:"), color);
    }
    // 役割カードの注記行(通常/Env固定/切替進行の文言)
    set_label(&ROLE_NOTE_LABEL, &role_note_text());
    // 「接続先の端末」の入力と保存は、この Mac が接続しに行く側の間だけ有効。
    // 待ち受け側(ホスト役)では接続先は要らない(Windows 側の入力欄と同じ条件)
    let going_out = crate::effective_client_role();
    enable(&HOST_FIELD, going_out || preview);
    enable(&HOST_SAVE_BUTTON, going_out || preview);
    set_label(&HOST_HINT, localization::text(
        if going_out || preview { "空欄なら自動発見。変更は「保存して再接続」で反映します。" } else { "ホストでは相手側から接続します。接続先の指定は不要です。" },
        if going_out || preview { "Leave blank to discover. Use Save & Reconnect to apply an address." } else { "As the host, this Mac receives connections. No device address is needed." },
    ));
    // Android 行: プレビュー中は材料を見ない(実機が無くても行を出さない)。
    // 文言の切り替え(許可待ち・許可済み・adb 接続の状況)は純粋関数が担う
    let android_hint = if preview {
        String::new()
    } else {
        let (_, status) = crate::android::state::snapshot();
        let tablet = status
            .tablets
            .first()
            .map(|t| (t.name.as_str(), t.phase.summary(), status.tablets.len()));
        android_state_text(
            android_app,
            crate::active_android_app_permissions(),
            status.problem.as_deref(),
            tablet,
        )
    };
    set_label(&ANDROID_STATE, &android_hint);
    set_label(&AUDIO_LABEL, if android_app { "音声共有(Androidアプリ版は未対応)" } else { "接続先の音声をこのPCで再生" });
    // SHARE_HINT: 通常時の本文は「切断すると元に戻ります」の 1 文とし、異常終了時の
    // 復旧はツールチップへ退避する。set_label はツールチップを本文で上書きするため、
    // stringValue と toolTip を別々に設定する(本文とツールチップで文言が違うため)
    let share_hint_label = SHARE_HINT.load(Ordering::Relaxed) as ID;
    if !share_hint_label.is_null() {
        let (text, tip) = share_hint_and_tooltip(android_app, android);
        msg1_void_id(share_hint_label, sel(c"setStringValue:"), nsstring(&localization::copy(text)));
        msg1_void_id(share_hint_label, sel(c"setToolTip:"), nsstring(&localization::copy(tip)));
    }
    // 再生音量スライダ(起動時の設定復元・リモート適用を画面へ反映)。音声転送が
    // オフ・env で audio が禁じられている・Androidアプリ版相手の間は行ごと隠す
    //(触れないつまみを常設しない。SWITCH_DELAY_VIEWS と同じ扱い。preview は常時表示)
    let gain_visible = preview
        || (!crate::audio::MUTED.load(Ordering::Relaxed) && audio_ok && !android_app);
    for v in AUDIO_GAIN_VIEWS.lock().unwrap_or_else(|e| e.into_inner()).iter() {
        let v = *v as ID;
        if !v.is_null() { msg1_void_u8(v, sel(c"setHidden:"), !gain_visible as u8); }
    }
    let gain_slider = AUDIO_GAIN_SLIDER.load(Ordering::Relaxed) as ID;
    if gain_visible && !gain_slider.is_null() {
        let get: unsafe extern "C" fn(ID, SEL) -> f64 =
            std::mem::transmute(crate::objc_msgSend as *const () as usize);
        let set: unsafe extern "C" fn(ID, SEL, f64) =
            std::mem::transmute(crate::objc_msgSend as *const () as usize);
        let g = crate::audio::gain() as f64;
        if get(gain_slider, sel(c"doubleValue")) != g {
            set(gain_slider, sel(c"setDoubleValue:"), g);
        }
        set_label(&AUDIO_GAIN_LABEL, &gain_text(crate::audio::gain()));
    }
    let canvas = LAYOUT_CANVAS.load(Ordering::Relaxed) as ID;
    if !canvas.is_null() && !LAY_DRAG.load(Ordering::Relaxed) { msg1_void_u8(canvas, sel(c"setNeedsDisplay:"), 1); }
    // 配置の現在値(モニター名+辺)も毎秒揃える(接続の増減・設定変更の反映)
    sync_layout_values();
    #[cfg(debug_assertions)]
    if preview && !PREVIEW_CHECKED.load(Ordering::Relaxed) && std::env::args().any(|v| v == "--preview-check-language") {
        PREVIEW_CHECKED.store(true, Ordering::Relaxed);
        check_preview_language();
    }
}

// 実際のAppKit部品で言語変更の安全性を確認する。プレビュー以外からは呼ばない。
#[cfg(debug_assertions)]
unsafe fn check_preview_language() {
    assert!(UI_PREVIEW.load(Ordering::Relaxed));
    assert_eq!(super::GUI_BUTTON.load(Ordering::Relaxed), 0, "preview must not create a status item");
    assert_eq!(super::GUI_MAIN_MENU.load(Ordering::Relaxed), 0, "preview must not create a resident menu");
    select_page(0);
    let field = HOST_FIELD.load(Ordering::Relaxed) as ID;
    let window = msg0(field, sel(c"window"));
    let string = |object: ID, property: &'static std::ffi::CStr| -> String {
        let value = msg0(object, sel(property));
        let utf8 = crate::msg0_cstr(value, sel(c"UTF8String"));
        assert!(!utf8.is_null());
        std::ffi::CStr::from_ptr(utf8).to_string_lossy().into_owned()
    };
    msg1_void_id(field, sel(c"setStringValue:"), nsstring("192.0.2.10"));
    let responder = msg0(window, sel(c"firstResponder"));
    assert!(!responder.is_null());
    let popup = LANGUAGE_POP.load(Ordering::Relaxed) as ID;
    for (index, title) in [(0, "Knit設定"), (1, "Knit Settings")] {
        set_save_error(SaveErrorSource::Host, "ホームが取得できず接続先を保存できませんでした".into());
        set_save_error(SaveErrorSource::PrefsFile, "Preferences save fixture".into());
        clear_save_error(SaveErrorSource::PrefsFile);
        msg1_void_i64(popup, sel(c"selectItemAtIndex:"), index);
        change_language(std::ptr::null_mut(), sel(c"sdLanguage:"), popup);
        assert_eq!(string(window, c"title"), title);
        assert_eq!(string(field, c"stringValue"), "192.0.2.10");
        assert_eq!(msg0(window, sel(c"firstResponder")), responder);
        let keys = HOTKEY_POP.load(Ordering::Relaxed) as ID;
        let item_at: unsafe extern "C" fn(ID, SEL, isize) -> ID =
            std::mem::transmute(crate::objc_msgSend as *const () as usize);
        let item = item_at(keys, sel(c"itemAtIndex:"), HOTKEY_CHOICES.len() as isize);
        let expected_key = if index == 0 { "現在のキー(コード63)" } else { "Current key (Key code 63)" };
        assert_eq!(string(item, c"title"), expected_key);
        assert!(!SAVE_ERROR_NOTE.lock().unwrap_or_else(|e| e.into_inner()).is_empty());
        let expected = localization::copy("ホームが取得できず接続先を保存できませんでした");
        assert_eq!(string(SAVE_LABEL.load(Ordering::Relaxed) as ID, c"stringValue"), expected);
    }
    clear_save_error(SaveErrorSource::Host);
    msg1_void_id(field, sel(c"setStringValue:"), nsstring(""));
    eprintln!("[ui-check] PASS: ja/en live switch, window title, unsaved address, first responder, custom key, source-scoped persistent host error; preview saves disabled");
    if std::env::args().any(|v| v == "--preview-check-close") {
        // 赤い閉じるボタン・⌘W と同じ NSWindow の終了通知を実機で検証する。
        msg1_void_id(window, sel(c"performClose:"), std::ptr::null_mut());
    }
}

/// 配置キャンバスの再描画要求(設定の変更を即座に絵へ反映する)。
/// ドラッグ中は絵が動かなくなるため、終わるまで待つ。
/// 現在値行(モニター名+辺)も同じタイミングで書き換える
pub(super) unsafe fn redraw_layout() {
    let canvas = LAYOUT_CANVAS.load(Ordering::Relaxed) as ID;
    if !canvas.is_null() && !LAY_DRAG.load(Ordering::Relaxed) {
        msg1_void_u8(canvas, sel(c"setNeedsDisplay:"), 1);
    }
    sync_layout_values();
}

/// 「画面配置」ページの現在値行の本文(各端末のモニター名+辺の一覧)。
/// 未接続のときは全体設定(SIDE)の辺を出す(モニター指定の無い既定位置)。
/// モニター名は mac_displays から(取れない時は番号表記。ログと同じ形式)
fn layout_values_text() -> String {
    let displays = crate::mac_displays();
    let monitor_label = |mi: usize| -> String {
        match displays.get(mi) {
            Some(d) if !d.name.is_empty() => d.name.clone(),
            Some(_) if mi == 0 => "メインモニター".to_string(),
            Some(_) => format!("モニター{}", mi + 1),
            None => format!("モニター{}", mi + 1),
        }
    };
    let peers = crate::PEERS.lock().unwrap_or_else(|e| e.into_inner());
    if peers.is_empty() {
        let side = crate::SIDE.load(Ordering::Relaxed).min(7);
        return format!(
            "現在: 未接続(次に繋がる端末は全画面の{})",
            crate::side_label(side)
        );
    }
    let items: Vec<String> = peers
        .iter()
        .map(|p| {
            let target = match p.edge_monitor {
                None => "全画面".to_string(),
                Some(mi) => monitor_label(mi),
            };
            format!("{}: {}の{}", crate::conn::peer_label(p), target, crate::side_label(p.side))
        })
        .collect();
    format!("現在: {}", items.join("・"))
}

/// 現在値行を最新の状態へ書き換える(描画要求と一緒に呼ぶ)
unsafe fn sync_layout_values() {
    if localization::language() == Language::Japanese {
        set_label(&LAYOUT_VALUES, &layout_values_text());
    } else {
        let displays = crate::mac_displays();
        let peers = crate::PEERS.lock().unwrap_or_else(|e| e.into_inner());
        let text = if peers.is_empty() {
            format!("Current: not connected · next device at the {} edge of all displays", localization::side_label(crate::SIDE.load(Ordering::Relaxed)))
        } else {
            let items: Vec<_> = peers.iter().map(|p| {
                let display = match p.edge_monitor {
                    None => "all displays".into(),
                    Some(mi) => displays.get(mi).filter(|d| !d.name.is_empty()).map(|d| d.name.clone()).unwrap_or_else(|| format!("Display {}", mi + 1)),
                };
                format!("{}: {} edge of {display}", crate::conn::peer_label(p), localization::side_label(p.side))
            }).collect();
            format!("Current: {}", items.join(" · "))
        };
        set_label(&LAYOUT_VALUES, &text);
    }
}

pub(super) unsafe fn build(target: ID) -> ID {
    refined::build(target)
}

#[allow(dead_code)]
unsafe fn build_legacy(target: ID) -> ID {
    COPY_BINDINGS.lock().unwrap_or_else(|e| e.into_inner()).clear();
    let args: Vec<String> = std::env::args().collect();
    let preview = UI_PREVIEW.load(Ordering::Relaxed);
    if preview {
        *crate::ACTIVE_PEER.lock().unwrap_or_else(|e| e.into_inner()) = if args.iter().any(|v| v == "--preview-tablet") { 1 } else { 0 };
        if let Some(language) = args.iter().position(|v| v == "--preview-language")
            .and_then(|i| args.get(i + 1)).and_then(|s| Language::parse(s)) {
            localization::set_language(language);
        }
    }
    let narrow = preview && args.iter().any(|v| v == "--preview-narrow");
    #[cfg(debug_assertions)]
    if preview && args.iter().any(|v| v == "--preview-check-language") {
        crate::HOTKEY_KC.store(63, Ordering::Relaxed);
    }
    let (width, height) = if narrow { (660.0, 540.0) } else { (720.0, 700.0) };
    let f: unsafe extern "C" fn(ID, SEL, NSRect, u64, u64, u8) -> ID =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    // ナビと保存状態は固定し、短い画面では本文だけスクロールする。
    let win = f(
        msg0(objc_getClass(c"NSWindow".as_ptr()), sel(c"alloc")),
        sel(c"initWithContentRect:styleMask:backing:defer:"),
        NSRect {
            x: 0.0,
            y: 0.0,
            w: width,
            h: height,
        },
        1 | 2 | 4 | 8,
        2,
        0,
    );
    if win.is_null() {
        return win;
    }
    msg1_void_id(win, sel(c"setTitle:"), nsstring("Knit設定"));
    bind_copy(win, c"setTitle:", "Knit設定");
    msg1_void_u8(win, sel(c"setReleasedWhenClosed:"), 0);
    let size: unsafe extern "C" fn(ID, SEL, crate::CGSize) = std::mem::transmute(crate::objc_msgSend as *const () as usize);
    size(win, sel(c"setContentMinSize:"), crate::CGSize { w: 660.0, h: 540.0 });
    size(win, sel(c"setContentMaxSize:"), crate::CGSize { w: 960.0, h: 700.0 });
    if preview && args.iter().any(|v| v == "--preview-dark" || v == "--preview-light") {
        let name = if args.iter().any(|v| v == "--preview-dark") { "NSAppearanceNameDarkAqua" } else { "NSAppearanceNameAqua" };
        let appearance = crate::msg1_id(objc_getClass(c"NSAppearance".as_ptr()), sel(c"appearanceNamed:"), nsstring(name));
        msg1_void_id(win, sel(c"setAppearance:"), appearance);
    }
    msg0_void(win, sel(c"center"));
    let cv = msg0(win, sel(c"contentView"));
    let margin = (width - 620.0) / 2.0;
    let brand = label(cv, "Knit", margin + 24.0, height - 44.0, 160.0, 18.0, false);
    msg1_void_i64(brand, sel(c"setAutoresizingMask:"), 1 | 4 | 8);
    let context = label(cv, "設定", margin + 420.0, height - 40.0, 176.0, 10.0, true);
    msg1_void_i64(context, sel(c"setAlignment:"), 2);
    msg1_void_i64(context, sel(c"setAutoresizingMask:"), 1 | 4 | 8);
    let pages: Vec<ID> = (0..PAGES.len())
        .map(|i| {
            let scroll = msg0(objc_getClass(c"NSScrollView".as_ptr()), sel(c"new"));
            frame(scroll, NSRect { x: margin, y: 70.0, w: 620.0, h: height - 190.0 });
            msg1_void_i64(scroll, sel(c"setAutoresizingMask:"), 1 | 4 | 16);
            msg1_void_u8(scroll, sel(c"setDrawsBackground:"), 0);
            msg1_void_u8(scroll, sel(c"setHasVerticalScroller:"), 1);
            msg1_void_u8(scroll, sel(c"setAutohidesScrollers:"), 1);
            msg1_void_id(cv, sel(c"addSubview:"), scroll);
            let clip = msg0(scroll, sel(c"contentView"));
            let p = view(
                clip,
                NSRect {
                    x: 0.0,
                    y: 0.0,
                    w: 620.0,
                    h: 500.0,
                },
            );
            msg1_void_id(scroll, sel(c"setDocumentView:"), p);
            let point: unsafe extern "C" fn(ID, SEL, crate::CGPoint) = std::mem::transmute(crate::objc_msgSend as *const () as usize);
            point(clip, sel(c"scrollToPoint:"), crate::CGPoint { x: 0.0, y: (500.0 - (height - 190.0)).max(0.0) });
            msg1_void_id(scroll, sel(c"reflectScrolledClipView:"), clip);
            PAGES[i].store(scroll as usize, Ordering::Relaxed);
            p
        })
        .collect();
    // 全5ページは同じ位置から直接選べる。短い画面でもタブはスクロールしない。
    for (i, title) in ["接続", "画面配置", "切替", "共有", "その他"].iter().enumerate() {
        let b = button(
            cv,
            target,
            title,
            c"sdNavigate:",
            NSRect {
                x: margin + 24.0 + i as f64 * 114.0,
                y: height - 94.0,
                w: 108.0,
                h: 32.0,
            },
        );
        msg1_void_i64(b, sel(c"setAutoresizingMask:"), 1 | 4 | 8);
        msg1_void_u8(b, sel(c"setBordered:"), 0);
        msg1_void_i64(b, sel(c"setButtonType:"), 1); // push-on/push-off(選択表示)
        msg1_void_i64(b, sel(c"setTag:"), i as i64);
        TABS[i].store(b as usize, Ordering::Relaxed);
    }
    // 見出しは編集対象、補足はそのページでできることを短く伝える。
    let subs = [
        "端末どうしをつなぎ、いまの状態を確かめます。",
        "画面の並びを、机の上の配置と揃えます。",
        "画面を越える方法を、自分の操作に合わせます。",
        "境界を越えて渡すものと、受け取るもの。",
        "表示言語と、アプリのメンテナンス。",
    ];
    for ((p, sub), title) in pages.iter().zip(subs).zip(["接続", "画面配置", "切替", "共有", "その他"]) {
        label(*p, title, 40.0, 462.0, 540.0, 26.0, false);
        label(*p, sub, 40.0, 434.0, 540.0, 12.0, true);
    }
    // 方向 → 状態と登録 → アドレス。項目数は増やさず、判断と操作を近づける。
    let p = pages[0];
    label(p, "接続の方向", 40.0, 388.0, 540.0, 13.0, false);
    ROLE_RADIO[0].store(
        radio(p, target, "このPCがホスト(既定) — 相手の端末が接続しに来ます", c"sdRole:", 0, NSRect { x: 40.0, y: 352.0, w: 540.0, h: 22.0 }) as usize,
        Ordering::Relaxed,
    );
    ROLE_RADIO[1].store(
        radio(p, target, "相手の端末がホスト — このPCが接続しに行きます", c"sdRole:", 1, NSRect { x: 40.0, y: 318.0, w: 540.0, h: 22.0 }) as usize,
        Ordering::Relaxed,
    );
    ROLE_NOTE_LABEL.store(
        label(p, &role_note_text(), 40.0, 280.0, 540.0, 11.0, true) as usize,
        Ordering::Relaxed,
    );
    group(p, 142.0, 126.0);
    label(p, "接続状態", 40.0, 238.0, 540.0, 12.0, true);
    PREFS_STATE.store(
        label(p, "接続を確認しています…", 40.0, 210.0, 540.0, 13.0, false) as usize,
        Ordering::Relaxed,
    );
    ANDROID_STATE.store(
        label(p, "", 40.0, 184.0, 540.0, 11.0, true) as usize,
        Ordering::Relaxed,
    );
    button(p, target, "端末を登録…", c"sdRegistration:", NSRect { x: 40.0, y: 150.0, w: 170.0, h: 30.0 });
    // Enter を接続先の保存へ結ばない。明示ボタンだけが保存と再起動を行う。
    label(p, "接続先の端末", 40.0, 104.0, 540.0, 13.0, false);
    let host_now = if preview { String::new() } else { knit_common::envutil::get("KNIT_HOST").unwrap_or_default() };
    HOST_FIELD.store(
        text_field(
            p,
            target,
            None,
            "接続先の端末のアドレス",
            "192.168.1.23(空欄で自動発見)",
            &host_now,
            NSRect { x: 40.0, y: 66.0, w: 336.0, h: 26.0 },
        ) as usize,
        Ordering::Relaxed,
    );
    HOST_SAVE_BUTTON.store(
        button(p, target, "保存して再接続", c"sdSaveHost:", NSRect { x: 392.0, y: 64.0, w: 188.0, h: 30.0 }) as usize,
        Ordering::Relaxed,
    );
    HOST_HINT.store(label(p, "", 40.0, 34.0, 540.0, 11.0, true) as usize, Ordering::Relaxed);
    tooltip(HOST_SAVE_BUTTON.load(Ordering::Relaxed) as ID, "接続先を保存してから、Knitを再起動して反映します。");
    let p = pages[1];
    let lay = make_layout_view(p);
    frame(
        lay,
        NSRect {
            x: 28.0,
            y: 106.0,
            w: LAY_VW,
            h: LAY_VH,
        },
    );
    msg1_void_id(p, sel(c"addSubview:"), lay);
    LAYOUT_CANVAS.store(lay as usize, Ordering::Relaxed);
    label(
        p,
        "グレー：Mac／ブルー：接続先(各モニターに機種名)。ドラッグで配置(斜めも可)",
        28.0,
        78.0,
        560.0,
        12.0,
        true,
    );
    // 操作の説明: クリック選択と「全画面の端」への戻し方の 2 トピックへ絞る
    //(3 文 dotted は幅 560px の第3文がほぼ読めないため)。保存済み配置の優先は
    // ツールチップへ退避し、モニター指定の解除(全画面)は短縮形で明記し続ける
    let layout_hint = label(
        p,
        "クリックで操作する端末を選べます・Macの上へ戻すと全画面の端になります",
        28.0,
        54.0,
        560.0,
        11.0,
        true,
    );
    if !layout_hint.is_null() {
        tooltip(layout_hint, "ブロックのクリックで操作する端末を選べます。Macの上へ戻すと「全画面の端(モニター指定なし)」になります。保存済みの端末は各端末の設定が優先されます");
    }
    // ドロップ結果の現在値(各端末のモニター名+辺)。redraw_layout と sync() が更新する
    LAYOUT_VALUES.store(
        label(p, "", 28.0, 28.0, 560.0, 11.0, true) as usize,
        Ordering::Relaxed,
    );
    sync_layout_values();
    // 初期化ボタンは持たない: ドラッグで好きなセルへ置き直せるため
    //(Windows 側設定の「既定(右)に戻す」は Windows に配置エディタが無い分の代替)
    let p = pages[2];
    // 画面を移る操作(キー・滞在時間)を 1 枚のカードへ並べる。切替方式
    // (タップ回数・待ち・キーのみ)は env(KNIT_SWITCH_MODE)と遠隔適用が担うため
    // 画面から外した。スクロール方向・速度・カーソル速度・互換・タブレットの
    // ジェスチャも同様(env・peer-sides.json・preferences)
    label(p, "切替キー", 40.0, 378.0, 260.0, 13.0, false);
    let current = crate::hotkey_kc();
    // env 固定(KNIT_HOTKEY_KC)のキーも名前で読めるように mac_key_label を通す
    //(未知のコードのときだけ同関数が「コードN」へフォールバックする)
    let custom = format!("現在のキー({})", knit_common::keymap::mac_key_label(current));
    let titles = [
        "F6(必要に応じてfnと併用)",
        "F8(必要に応じてfnと併用)",
        // F13 は起動時の既定(HOTKEY_KC 初期値)のため「(既定)」を付ける
        // (hotkey ハンドラは index で HOTKEY_CHOICES を引くため文言変更の影響無し)
        "F13(既定)",
        "右⌘",
        custom.as_str(),
    ];
    let known = HOTKEY_CHOICES.contains(&current);
    let pop = popup(
        p,
        target,
        // 既知の候補キーなら候補のみ。未知(env で指定)のときは末尾に現在値を出す
        &titles[..if known {
            HOTKEY_CHOICES.len()
        } else {
            HOTKEY_CHOICES.len() + 1
        }],
        c"sdHotkey:",
        NSRect {
            x: 300.0,
            y: 376.0,
            w: 280.0,
            h: 28.0,
        },
    );
    msg1_void_i64(
        pop,
        sel(c"selectItemAtIndex:"),
        HOTKEY_CHOICES
            .iter()
            .position(|k| *k == current)
            .unwrap_or(HOTKEY_CHOICES.len()) as i64,
    );
    // 切替キーも sync() が毎秒選択位置を書き換えるため、同じ delegate 保護を付ける
    let hotkey_menu = msg0(pop, sel(c"menu"));
    if !hotkey_menu.is_null() {
        msg1_void_id(hotkey_menu, sel(c"setDelegate:"), target);
        HOTKEY_MENU.store(hotkey_menu as usize, Ordering::Relaxed);
    }
    HOTKEY_POP.store(pop as usize, Ordering::Relaxed);
    // 「端で少し待つ」の滞在時間スライダ。sync() が方式に応じて一式の表示を
    // 切り替える(値の遠隔適用・復元も sync が反映する)
    {
        let delay_divider = divider(p, 356.0) as usize;
        let caption = slider(
            p,
            target,
            "滞在時間   短い / 長い",
            (crate::SWITCH_DELAY_MS.load(Ordering::Relaxed)).clamp(50, 1000) as f64,
            50.0,
            1000.0,
            c"sdSwitchDelay:",
            &SWITCH_DELAY_SLIDER,
            &SWITCH_DELAY_LABEL,
            310.0,
        );
        *SWITCH_DELAY_VIEWS.lock().unwrap_or_else(|e| e.into_inner()) = vec![
            delay_divider,
            caption as usize,
            SWITCH_DELAY_SLIDER.load(Ordering::Relaxed),
            SWITCH_DELAY_LABEL.load(Ordering::Relaxed),
        ];
        set_label(&SWITCH_DELAY_LABEL, &delay_text(crate::SWITCH_DELAY_MS.load(Ordering::Relaxed)));
    }
    // 環境変数で固定されている項目の注記(KNIT_SWITCH_MODE 等)。固定が無ければ
    // 空文字=見えない。env 起因で実行中変わらないため、build 時に本文と対処案の
    // ツールチップを 1 回だけ設定する(毎秒の sync() では触らない)
    let op_fixed: Vec<&str> = OPERATION_ENV_KEYS
        .iter()
        .filter(|k| crate::envutil::get(k).is_some())
        .copied()
        .collect();
    label(p, "切替キーは、どの方式でも使えます。", 40.0, 170.0, 540.0, 12.0, true);
    let env_note_label = label(p, &operation_env_note(&op_fixed), 40.0, 108.0, 540.0, 11.0, true);
    if !op_fixed.is_empty() {
        tooltip(env_note_label, operation_env_note_tooltip());
    }
    ENV_NOTE_LABEL.store(env_note_label as usize, Ordering::Relaxed);
    let p = pages[3];
    // 渡すもの(テキスト・画像・ファイル)と、音。クリップとファイルの各 check の
    // 下の説明行は caption の言い換えで情報が増えないため削り、説明は check の
    // caption へ setToolTip で出す(clip の check と同じ作法)。スピーカーミュートを
    // 外した分、カードを上へ詰めて配置する(KNIT_MUTE_SPK と遠隔適用で機能は残る)
    label(p, "渡すもの", 40.0, 388.0, 540.0, 13.0, false);
    let clip_caption = check(p, target, "テキストと画像", c"sdClipShare:", &PREFS_CHK_CLIP, 354.0);
    tooltip(clip_caption, "コピーした内容を、もう1台でも貼り付けられます");
    let file_caption = check(p, target, "ファイルの受け渡し", c"sdFileShare:", &super::PREFS_CHK_FILES, 314.0);
    tooltip(file_caption, "コピーしたファイルや、掴んだファイルを渡せます");
    divider(p, 296.0);
    label(p, "音声", 40.0, 264.0, 540.0, 13.0, false);
    AUDIO_LABEL.store(check(
        p,
        target,
        "接続先の音声をこのPCで再生",
        c"sdAudio:",
        &PREFS_CHK_AUDIO,
        230.0,
    ) as usize, Ordering::Relaxed);
    // 再生音量スライダ: 音声転送オン・env で audio が許可・Androidアプリ版相手で
    // ない間だけ sync() が表示する(行の一式をまとめて隠す)
    {
        let gain_caption = slider(
            p,
            target,
            "再生音量   小さい / 大きい",
            crate::audio::gain() as f64,
            0.0,
            2.0,
            c"sdAudioGain:",
            &AUDIO_GAIN_SLIDER,
            &AUDIO_GAIN_LABEL,
            188.0,
        );
        *AUDIO_GAIN_VIEWS.lock().unwrap_or_else(|e| e.into_inner()) = vec![
            gain_caption as usize,
            AUDIO_GAIN_SLIDER.load(Ordering::Relaxed),
            AUDIO_GAIN_LABEL.load(Ordering::Relaxed),
        ];
    }
    divider(p, 162.0);
    label(p, "履歴", 40.0, 132.0, 540.0, 13.0, false);
    check(p, target, "このPCのコピーを履歴に残す", c"sdLocalHistory:", &super::PREFS_CHK_HISTORY, 98.0);
    // 説明は核心(選び直し)の 1 文へ短縮し、機密コピーを残さない注記はツールチップ
    // へ退避する(2 文 dotted は幅 540px で否定「残しません」が切れかけだった)
    let history_hint = label(p, "メニューバーの履歴から選び直せます", 40.0, 68.0, 540.0, 11.0, true);
    if !history_hint.is_null() {
        tooltip(history_hint, "履歴は平文で保存されます。残したくない場合はオフにしてください。");
    }
    // 最下部の説明は「切断すると元に戻ります」の 1 文で作り、異常終了時の復旧は
    // ツールチップへ(毎秒の sync() が share_hint_and_tooltip で両方を揃える)
    let share_hint = label(p, "切断すると元に戻ります。", 40.0, 26.0, 540.0, 11.0, true);
    if !share_hint.is_null() {
        msg1_void_id(
            share_hint,
            sel(c"setToolTip:"),
            nsstring("切断すると元に戻ります。異常終了したときも、相手側は次回起動時に自動で戻します。"),
        );
    }
    SHARE_HINT.store(share_hint as usize, Ordering::Relaxed);
    // 「その他」ページ(ページ 4): サイドバー下部にあった再起動・初期化の 3 ボタン
    // をページへ昇格する(2 階層化でメインが導線だけになったため、破壊的操作は
    // 専用ページへ隔てる)。初期化の 2 ボタンは確認ダイアログ(setup の
    // reset_registration/reset_all_settings)と mark_destructive が守る
    let p = pages[4];
    label(p, "表示言語", 40.0, 378.0, 240.0, 13.0, false);
    let language_pop = popup(p, target, &["日本語", "English"], c"sdLanguage:", NSRect { x: 300.0, y: 376.0, w: 280.0, h: 28.0 });
    msg1_void_i64(language_pop, sel(c"selectItemAtIndex:"), (localization::language() == Language::English) as i64);
    LANGUAGE_POP.store(language_pop as usize, Ordering::Relaxed);
    label(p, "言語を変えても、接続や入力中の内容はそのままです。", 40.0, 334.0, 540.0, 11.0, true);
    divider(p, 310.0);
    label(p, "メンテナンス", 40.0, 276.0, 540.0, 13.0, false);
    label(p, "再起動", 40.0, 232.0, 240.0, 13.0, false);
    OTHER_RESTART.store(
        button(p, target, "Knitを再起動", c"sdRestart:", NSRect { x: 340.0, y: 226.0, w: 240.0, h: 30.0 }) as usize,
        Ordering::Relaxed,
    );
    divider(p, 194.0);
    label(p, "初期化", 40.0, 160.0, 540.0, 13.0, false);
    // 登録の初期化。破壊性(全端末が締め出され再登録が必要)はダイアログの本文と
    // ボタンのツールチップが伝える
    label(p, "端末の登録", 40.0, 116.0, 240.0, 13.0, false);
    let reset_reg_btn = button(
        p,
        target,
        "すべての登録を初期化…",
        c"sdResetRegistration:",
        NSRect { x: 340.0, y: 110.0, w: 240.0, h: 30.0 },
    );
    mark_destructive(reset_reg_btn);
    if !reset_reg_btn.is_null() {
        tooltip(reset_reg_btn, "初期化すると全端末が締め出され、再登録が必要です(取り消せません)。特定の1台だけ外すには、初期化後に必要な端末を再登録します");
    }

    // 破壊ボタン(すべての設定を初期化)は赤文字で見た目を区別する。初期化の確認
    // ダイアログと合わせて二重の抑止とする
    label(p, "すべての設定", 40.0, 64.0, 240.0, 13.0, false);
    mark_destructive(button(
        p,
        target,
        "すべての設定を初期化…",
        c"sdResetSettings:",
        NSRect { x: 340.0, y: 58.0, w: 240.0, h: 30.0 },
    ));
    SAVE_LABEL.store(
        label(
            cv,
            if UI_PREVIEW.load(Ordering::Relaxed) {
                "デザイン確認モード・設定は保存しません".to_string()
            } else {
                save_status_initial(&preferences::override_keys())
            }
            .as_str(),
            margin + 24.0,
            30.0,
            572.0,
            11.0,
            true,
        ) as usize,
        Ordering::Relaxed,
    );
    *LAST_SAVE_STATUS.lock().unwrap_or_else(|e| e.into_inner()) = if preview {
        "デザイン確認モード・設定は保存しません".into()
    } else { save_status_initial(&preferences::override_keys()) };
    // バージョン表示(旧サイドバー下部から移設。保存状態行と同じ最下部の右端へ
    // 小さく置く=問い合わせの際に画面から読み取れる)
    let version = label(
        cv,
        &format!("v{}", crate::VERSION_STR),
        width - margin - 184.0,
        10.0,
        160.0,
        10.0,
        true,
    );
    msg1_void_i64(version, sel(c"setAlignment:"), 2); // 右揃え
    msg1_void_i64(version, sel(c"setAutoresizingMask:"), 1);
    msg1_void_i64(SAVE_LABEL.load(Ordering::Relaxed) as ID, sel(c"setAutoresizingMask:"), 1 | 4);
    let args: Vec<String> = std::env::args().collect();
    let page = if UI_PREVIEW.load(Ordering::Relaxed) {
        args.iter()
            .position(|v| v == "--preview-page")
            .and_then(|i| args.get(i + 1))
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(0)
            .min(PAGES.len()-1)
    } else {
        0
    };
    select_page(page);
    // ⌘W でウィンドウを閉じる経路。メニューバー常駐型はアプリのメインメニューを
    // 持たないため、contentView 直下に keyEquivalent "w"(⌘)のボタンを置く。
    // hidden にすると keyEquivalent ごと無効になるため、サイズ 0 で配置する
    // (見えず・押せず、キーの照合だけに効く)。performClose: は
    // releasedWhenClosed=0 のため閉じても破棄されず、次回の show_prefs で
    // 同じウィンドウが戻る
    let close_btn = msg0(objc_getClass(c"NSButton".as_ptr()), sel(c"new"));
    if !close_btn.is_null() {
        frame(close_btn, NSRect { x: 0.0, y: 0.0, w: 0.0, h: 0.0 });
        msg1_void_id(close_btn, sel(c"setTitle:"), nsstring(&localization::copy("設定を閉じる")));
        bind_copy(close_btn, c"setTitle:", "設定を閉じる");
        msg1_void_id(close_btn, sel(c"setTarget:"), target);
        msg1_void_sel(close_btn, sel(c"setAction:"), sel(c"sdClosePrefs:"));
        msg1_void_id(close_btn, sel(c"setKeyEquivalent:"), nsstring("w"));
        // NSModifierFlagCommand = 1 << 20
        msg1_void_i64(close_btn, sel(c"setKeyEquivalentModifierMask:"), 1 << 20);
        msg1_void_id(cv, sel(c"addSubview:"), close_btn);
    }
    win
}

/// 役割の表示決定(環境変数 KNIT_ROLE のあり/なしで GUI 設定と実役割が食い違う問題の
/// 単体テスト)。main の起動判定と同じ優先順位であることを純粋関数で検証する
#[cfg(test)]
mod role_display_tests {
    use crate::display_client_role;

    /// 環境変数 KNIT_ROLE=client は GUI 設定に優先し、それ以外の値は GUI に従う
    #[test]
    fn env_client_overrides_gui_and_other_values_do_not() {
        // env 無し: GUI 設定のまま
        assert_eq!(display_client_role(None, false), false);
        assert_eq!(display_client_role(None, true), true);
        // client 固定: GUI 設定に優先して接続側になる
        assert_eq!(display_client_role(Some("client"), false), true);
        assert_eq!(display_client_role(Some("client"), true), true);
        // server 等の他の値は Mac では固定にならない(Windows 側の固定値)
        assert_eq!(display_client_role(Some("server"), false), false);
        assert_eq!(display_client_role(Some("server"), true), true);
        assert_eq!(display_client_role(Some(""), true), true);
    }
}

/// 現在値行の本文(5件目の改善: モニター名+辺の一覧)。表示形式を単体テストで守る
#[cfg(test)]
mod layout_values_tests {
    use super::layout_values_text;
    use crate::PeerEntry;
    use std::sync::atomic::Ordering;

    fn entry(name: &str, side: u8, edge_monitor: Option<usize>) -> PeerEntry {
        PeerEntry {
            id: format!("id-{name}"),
            name: name.into(),
            ip: "127.0.0.1".parse().unwrap(),
            screen: (800.0, 600.0),
            monitors: vec![],
            writer: None,
            gen: 1, auth: knit_common::secure::Who::Legacy, shut: None,
            side,
            edge_monitor,
            ver: 13,
            alias: None,
        }
    }

    /// PEERS はプロセス共有の状態。入れ替えて必ず元へ戻す(conn::tests と同じ手法)。
    /// 置換中の区間は conn の PEERS_TEST_SERIAL で直列化する(conn::tests の
    /// with_peers と交錯すると互いに他人の PEERS を覨いて間欠失敗するため)
    fn with_peers(peers: Vec<PeerEntry>, f: impl FnOnce()) {
        let _serial = crate::conn::PEERS_TEST_SERIAL
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let saved = {
            let mut p = crate::PEERS.lock().unwrap_or_else(|e| e.into_inner());
            std::mem::replace(&mut *p, peers)
        };
        f();
        *crate::PEERS.lock().unwrap_or_else(|e| e.into_inner()) = saved;
    }

    /// PEERS は単一の static のため、テストは直列で1本にまとめる(role_ack_tests と同じ)
    #[test]
    fn values_follow_peers_and_fallback_to_global_side() {
        // 未接続: 全体設定(SIDE)の辺を「全画面の…」として出す
        with_peers(Vec::new(), || {
            let side = crate::SIDE.load(Ordering::Relaxed).min(7);
            assert_eq!(
                layout_values_text(),
                format!("現在: 未接続(次に繋がる端末は全画面の{})", crate::side_label(side))
            );
        });
        // 接続中: 各端末の「モニター+辺」を並べる。1 台は全画面指定なし、
        // 1 台はメイン(0 番)モニターの斜め右上。モニター名が取れない実行環境では
        // 番号表記へ落ちるため部分一致で見る
        with_peers(vec![entry("Win", 1, None), entry("Tab", 4, Some(0))], || {
            let text = layout_values_text();
            assert!(text.starts_with("現在: Win: 全画面の左・Tab: "), "{text}");
            assert!(text.ends_with("の右上"), "{text}");
        });
        // エイリアス(利用者が付けた名前)はコンピュータ名より優先される
        let mut aliased = entry("Win", 1, None);
        aliased.alias = Some("事務室のPC".into());
        with_peers(vec![aliased], || {
            let text = layout_values_text();
            assert!(text.contains("事務室のPC: 全画面の左"), "{text}");
            assert!(!text.contains("Win:"), "エイリアス優先: {text}");
        });
    }
}

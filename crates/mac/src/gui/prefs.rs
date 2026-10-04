//! 設定の分類と画面構築。入力・通信の処理は既存アクションへ委譲する。
use super::*;
use std::sync::Mutex;
static PAGES: [AtomicUsize; 4] = [const { AtomicUsize::new(0) }; 4];
static TABS: [AtomicUsize; 4] = [const { AtomicUsize::new(0) }; 4];
/// 「操作」ページのタブレット項目(タブレットが無い時は隠す)
static TABLET_VIEWS: Mutex<Vec<usize>> = Mutex::new(Vec::new());
/// 設定画面のアップデートボタン(状態に応じて文言が変わる)
static UPDATE_BUTTON: AtomicUsize = AtomicUsize::new(0);
static SAVE_LABEL: AtomicUsize = AtomicUsize::new(0);
static HOTKEY_POP: AtomicUsize = AtomicUsize::new(0);
static SWITCH_POP: AtomicUsize = AtomicUsize::new(0);
static PEER_POP: AtomicUsize = AtomicUsize::new(0);
static PEER_MENU_OPEN: AtomicBool = AtomicBool::new(false);
static PEER_CHOICES: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());
static AUDIO_LABEL: AtomicUsize = AtomicUsize::new(0);
static SPEAKER_LABEL: AtomicUsize = AtomicUsize::new(0);
/// 音声の再生音量スライダ(Windows から届く音をこの Mac で鳴らす大きさ)
static AUDIO_GAIN_SLIDER: AtomicUsize = AtomicUsize::new(0);
static AUDIO_GAIN_LABEL: AtomicUsize = AtomicUsize::new(0);
static SHARE_HINT: AtomicUsize = AtomicUsize::new(0);
static CONNECTION_HINT: AtomicUsize = AtomicUsize::new(0);
/// 接続ページの受け入れ範囲の行(KNIT_ALLOW_ANY/TS の緩和を見える化する)
static ACCEPT_LABEL: AtomicUsize = AtomicUsize::new(0);
/// 接続ページの暗号化の行(常時暗号化・相互認証と鍵の略号を常に表示する。
/// 暗号化は常時ONのためトグルは持たない=設定不要であることを読み取らせる)
static SECURE_LABEL: AtomicUsize = AtomicUsize::new(0);
/// 接続ページの「登録の管理」の行(登録済み台数を常に見せる。1対1でも
/// 台数が出るように、multi_peer 判定とは無関係に毎秒の sync() が書き換える)
static REG_MGMT_LABEL: AtomicUsize = AtomicUsize::new(0);
/// 接続ページの待受アドレスの行(KNIT_BIND の指定を見える化する)
static BIND_LABEL: AtomicUsize = AtomicUsize::new(0);
/// 接続ページのポート一覧の行(経路ごとの固定ポートの静的表示)
static PORTS_LABEL: AtomicUsize = AtomicUsize::new(0);
/// 接続ページの相手側の名前(Android 接続中は端末名へ書き換わる)
static PEER_LABEL: AtomicUsize = AtomicUsize::new(0);
/// 接続ページの Android タブレットの状態行(端末が無ければ空)
static ANDROID_STATE: AtomicUsize = AtomicUsize::new(0);
static NAV_SWITCH: AtomicUsize = AtomicUsize::new(0);
static GESTURE_HINT: AtomicUsize = AtomicUsize::new(0);
static PINCH_SWITCH: AtomicUsize = AtomicUsize::new(0);
/// 切替方式ポップアップの下の速度越境(FAST_EDGE)の説明行。誤爆防止が効く
/// 方式(2回触れる/少し待つ)のときだけ sync() が見せる
static FAST_EDGE_HINT: AtomicUsize = AtomicUsize::new(0);
/// 「端で少し待つ」の滞在時間スライダ(方式で選んだときだけ見せる)
static SWITCH_DELAY_SLIDER: AtomicUsize = AtomicUsize::new(0);
static SWITCH_DELAY_LABEL: AtomicUsize = AtomicUsize::new(0);
/// 滞在時間スライダの一式(区切り線・caption・スライダ・値表示。方式が
/// 「端で少し待つ」のときだけ表示する。TABLET_VIEWS と同じパターン)
static SWITCH_DELAY_VIEWS: Mutex<Vec<usize>> = Mutex::new(Vec::new());
static LAYOUT_CANVAS: AtomicUsize = AtomicUsize::new(0);
/// 「画面配置」ページの現在値行(各端末のモニター名+辺)。ドロップ直後の
/// redraw_layout と毎秒の sync() で書き換わる
static LAYOUT_VALUES: AtomicUsize = AtomicUsize::new(0);
/// 接続ページの「操作する端末」の行(登録済み端末が2台以上のときだけ見せる)
static PEER_VIEWS: Mutex<Vec<usize>> = Mutex::new(Vec::new());
/// 接続ページの「接続先のWindows」入力欄(この Mac が接続しに行く側のときだけ有効)
static HOST_FIELD: AtomicUsize = AtomicUsize::new(0);
/// 接続先の保存ボタン(入力欄と同じ条件で有効/無効が変わる)
static HOST_SAVE_BUTTON: AtomicUsize = AtomicUsize::new(0);
/// 接続ページの「選択中の端末の名前」入力欄(アクティブな端末のエイリアス保存用)
static ALIAS_FIELD: AtomicUsize = AtomicUsize::new(0);
/// エイリアスの保存ボタン(入力欄と同じ条件で有効)
static ALIAS_SAVE_BUTTON: AtomicUsize = AtomicUsize::new(0);
/// エイリアス欄に最後に書いた端末 id(端末切替時だけ書き換え、入力中は触らない)
static ALIAS_FIELD_PEER: Mutex<String> = Mutex::new(String::new());
/// 接続ページの「このMacの名前」入力欄(空欄=ホスト名既定。hello の名乗り名)
static OWN_NAME_FIELD: AtomicUsize = AtomicUsize::new(0);
/// このMacの名前の保存ボタン
static OWN_NAME_SAVE_BUTTON: AtomicUsize = AtomicUsize::new(0);
/// 接続ページの「このMacのアドレス」行(複数 NIC は列挙。10 秒キャッシュで更新)
static OWN_IP_LABEL: AtomicUsize = AtomicUsize::new(0);
/// 役割カードの注記行。通常は再起動の案内、KNIT_ROLE 固定中はその注記、
/// 切替の進行中は確認状況へ文言が変わる(進行は ROLE_NOTE_KIND で伝える)
static ROLE_NOTE_LABEL: AtomicUsize = AtomicUsize::new(0);
/// 役割カードの注記行のモード(0=通常 1=Env固定 2=確認待ち 3=確認済み 4=タイムアウト)。
/// 切替ハンドラと Ack 待ちスレッドが書き、毎秒の sync() が文言へ反映する
pub(super) static ROLE_NOTE_KIND: AtomicUsize = AtomicUsize::new(0);
/// 「その他」の詳細記録チェック(1秒ごとの診断行=--diag 相当。お問い合わせ時に)
static DIAG_SWITCH: AtomicUsize = AtomicUsize::new(0);
/// 「操作」ページの環境変数固定の注記行(KNIT_SWITCH_MODE 等で固定中の見える化)。
/// 固定が無いときは空文字=見えない(ANDROID_STATE と同じ扱い)
static ENV_NOTE_LABEL: AtomicUsize = AtomicUsize::new(0);

/// 「操作」ページの項目を固定できる環境変数(注記行の対象。接続の KNIT_ROLE は
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

/// 操作ページの注記行の本文(固定されているキーが無いときは空=非表示)
fn operation_env_note(keys: &[&str]) -> String {
    if keys.is_empty() {
        String::new()
    } else {
        format!(
            "{} で固定中のため、該当項目は起動時にそちらの値で上書きされます(設定「その他」から初期化できます)",
            keys.join("・")
        )
    }
}

/// 保存状態ラベルの本文(環境変数・envファイルで固定されているキーが分かるように。
/// 多いときは先頭3件+件数で読み切れる形にする)
fn save_status_initial(keys: &[&str]) -> String {
    match keys.len() {
        0 => "変更はこのMacに自動保存されます".to_string(),
        n if n <= 3 => format!(
            "自動保存 · 起動時は {} が優先されます",
            keys.join("・")
        ),
        n => format!(
            "自動保存 · 起動時は {} ほか{}件が優先されます(設定「その他」から初期化できます)",
            keys[..3].join("・"),
            n - 3
        ),
    }
}

/// 保存を明示的に行った直後のステータス文言(初期表示と同じ形式で固定キーを伝える)
pub(super) fn save_status_saved(keys: &[&str]) -> String {
    match keys.len() {
        0 => "変更を保存しました".to_string(),
        n if n <= 3 => format!("保存済み · 再起動時は {} が優先されます", keys.join("・")),
        n => format!(
            "保存済み · 再起動時は {} ほか{}件が優先されます",
            keys[..3].join("・"),
            n - 3
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
unsafe fn surface(v: ID, color: &std::ffi::CStr, radius: f64) {
    msg1_void_u8(v, sel(c"setWantsLayer:"), 1);
    let layer = msg0(v, sel(c"layer"));
    let color = msg0(objc_getClass(c"NSColor".as_ptr()), sel(color));
    msg1_void_id(
        layer,
        sel(c"setBackgroundColor:"),
        msg0(color, sel(c"CGColor")),
    );
    let f: unsafe extern "C" fn(ID, SEL, f64) =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    f(layer, sel(c"setCornerRadius:"), radius);
}
unsafe fn group(parent: ID, y: f64, h: f64) -> ID {
    let v = view(
        parent,
        NSRect {
            x: 24.0,
            y,
            w: 572.0,
            h,
        },
    );
    surface(v, c"controlBackgroundColor", 12.0);
    let layer = msg0(v, sel(c"layer"));
    let f: unsafe extern "C" fn(ID, SEL, f64) =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    f(layer, sel(c"setBorderWidth:"), 0.5);
    let color = msg0(objc_getClass(c"NSColor".as_ptr()), sel(c"separatorColor"));
    msg1_void_id(layer, sel(c"setBorderColor:"), msg0(color, sel(c"CGColor")));
    v
}
unsafe fn divider(parent: ID, y: f64) -> ID {
    let v = view(
        parent,
        NSRect {
            x: 40.0,
            y,
            w: 540.0,
            h: 0.5,
        },
    );
    surface(v, c"separatorColor", 0.0);
    v
}
unsafe fn symbol(parent: ID, name: &str, x: f64, y: f64, size: f64) {
    let f: unsafe extern "C" fn(ID, SEL, ID, ID) -> ID =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    let image = f(
        objc_getClass(c"NSImage".as_ptr()),
        sel(c"imageWithSystemSymbolName:accessibilityDescription:"),
        nsstring(name),
        std::ptr::null_mut(),
    );
    let v = msg0(objc_getClass(c"NSImageView".as_ptr()), sel(c"new"));
    frame(
        v,
        NSRect {
            x,
            y,
            w: size,
            h: size,
        },
    );
    msg1_void_id(v, sel(c"setImage:"), image);
    msg1_void_i64(v, sel(c"setImageScaling:"), 3);
    let color = msg0(
        objc_getClass(c"NSColor".as_ptr()),
        sel(c"controlAccentColor"),
    );
    msg1_void_id(v, sel(c"setContentTintColor:"), color);
    msg1_void_id(parent, sel(c"addSubview:"), v);
}
unsafe fn label(parent: ID, text: &str, x: f64, y: f64, w: f64, size: f64, muted: bool) -> ID {
    let f: unsafe extern "C" fn(ID, SEL, ID) -> ID =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    let v = f(
        objc_getClass(c"NSTextField".as_ptr()),
        sel(c"labelWithString:"),
        nsstring(text),
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
    msg1_void_id(v, sel(c"setToolTip:"), nsstring(text));
    msg1_void_id(parent, sel(c"addSubview:"), v);
    v
}
unsafe fn button(parent: ID, target: ID, title: &str, action: &std::ffi::CStr, r: NSRect) -> ID {
    let f: unsafe extern "C" fn(ID, SEL, ID, ID, SEL) -> ID =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    let b = f(
        objc_getClass(c"NSButton".as_ptr()),
        sel(c"buttonWithTitle:target:action:"),
        nsstring(title),
        target,
        sel(action),
    );
    frame(b, r);
    msg1_void_id(parent, sel(c"addSubview:"), b);
    b
}

/// 編集できるテキスト入力欄(接続先・名前の指定用)。値は保存ボタンのアクションで
/// 読み取るため、入力欄自身には action を付けない
unsafe fn text_field(parent: ID, accessibility: &str, placeholder: &str, value: &str, r: NSRect) -> ID {
    let f: unsafe extern "C" fn(ID, SEL, NSRect) -> ID =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    let v = f(
        msg0(objc_getClass(c"NSTextField".as_ptr()), sel(c"alloc")),
        sel(c"initWithFrame:"),
        r,
    );
    msg1_void_u8(v, sel(c"setBezeled:"), 1);
    msg1_void_u8(v, sel(c"setEditable:"), 1);
    msg1_void_id(v, sel(c"setPlaceholderString:"), nsstring(placeholder));
    msg1_void_id(v, sel(c"setStringValue:"), nsstring(value));
    let font = msg1_id_f64(
        objc_getClass(c"NSFont".as_ptr()),
        sel(c"systemFontOfSize:"),
        13.0,
    );
    msg1_void_id(v, sel(c"setFont:"), font);
    msg1_void_id(v, sel(c"setAccessibilityLabel:"), nsstring(accessibility));
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
    msg1_void_id(b, sel(c"setTitle:"), nsstring(title));
    msg1_void_i64(b, sel(c"setButtonType:"), 4); // NSButtonTypeRadio
    msg1_void_i64(b, sel(c"setTag:"), tag);
    msg1_void_id(b, sel(c"setTarget:"), target);
    let action_fn: unsafe extern "C" fn(ID, SEL, SEL) =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    action_fn(b, sel(c"setAction:"), sel(action));
    msg1_void_id(b, sel(c"setAccessibilityLabel:"), nsstring(title));
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
    msg1_void_id(b, sel(c"setAccessibilityLabel:"), nsstring(title));
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
    for t in titles {
        msg1_void_id(p, sel(c"addItemWithTitle:"), nsstring(t));
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
            w: 228.0,
            h: 24.0,
        },
    );
    msg1_void_u8(s, sel(c"setContinuous:"), 0);
    msg1_void_id(s, sel(c"setAccessibilityLabel:"), nsstring(title));
    msg1_void_id(parent, sel(c"addSubview:"), s);
    slot.store(s as usize, Ordering::Relaxed);
    let value = label(parent, "", 518.0, y, 60.0, 11.0, true);
    msg1_void_i64(value, sel(c"setAlignment:"), 2);
    value_slot.store(value as usize, Ordering::Relaxed);
    caption
}

pub(super) unsafe fn select_page(index: usize) {
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
    let page = PAGES[index.min(PAGES.len()-1)].load(Ordering::Relaxed) as ID;
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
}
pub(super) unsafe extern "C" fn navigate(_s: ID, _c: SEL, sender: ID) {
    select_page(crate::msg0_isize(sender, sel(c"tag")).clamp(0, 3) as usize);
}
pub(super) unsafe extern "C" fn hotkey(_s: ID, _c: SEL, sender: ID) {
    let i = crate::msg0_isize(sender, sel(c"indexOfSelectedItem"));
    if let Some(k) = HOTKEY_CHOICES.get(i.max(0) as usize) {
        crate::HOTKEY_KC.store(*k, Ordering::Relaxed);
        preferences::save();
    }
}
pub(super) unsafe extern "C" fn switch_method(_s: ID, _c: SEL, sender: ID) {
    let i = crate::msg0_isize(sender, sel(c"indexOfSelectedItem"));
    crate::HOTKEY_ONLY.store(i == 2, Ordering::Relaxed);
    crate::EDGE_TAPS.store(if i == 3 { 1 } else { 2 }, Ordering::Relaxed);
    crate::SWITCH_DELAY_MS.store(
        delay_for_method(i, crate::SWITCH_DELAY_MS.load(Ordering::Relaxed)),
        Ordering::Relaxed,
    );
    preferences::save();
    refresh_status();
}

/// 「端で少し待つ」を選んだ時の滞在時間(ms)。未設定(0)なら既定 300ms、
/// 設定済みならその値を保つ(スライダで調整した値を方式の行き来で失わない)。
/// 他の方式は滞在を使わないため 0 に戻す(method_index の射影と整合)
fn delay_for_method(i: isize, current_ms: u64) -> u64 {
    if i == 1 {
        if current_ms == 0 { 300 } else { current_ms }
    } else {
        0
    }
}

/// 滞在時間スライダの値表示
fn delay_text(ms: u64) -> String {
    format!("{}ms", ms)
}

/// 「端で少し待つ」の滞在時間スライダ(50-1000ms)。値は SWITCH_DELAY_MS へ保存し、
/// 遠隔適用(Windows 側設定の delay)・起動時復元も同じ経路を使う
pub(super) unsafe extern "C" fn switch_delay(_s: ID, _c: SEL, sender: ID) {
    let f: unsafe extern "C" fn(ID, SEL) -> f64 =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    let v = f(sender, sel(c"doubleValue"));
    crate::SWITCH_DELAY_MS.store(v.clamp(50.0, 1000.0) as u64, Ordering::Relaxed);
    set_label(&SWITCH_DELAY_LABEL, &delay_text(crate::SWITCH_DELAY_MS.load(Ordering::Relaxed)));
    preferences::save();
}
pub(super) unsafe extern "C" fn enter_peer(_s: ID, _c: SEL, _sender: ID) {
    if !UI_PREVIEW.load(Ordering::Relaxed) && !crate::WIN_MODE.load(Ordering::Relaxed) {
        crate::do_toggle("設定画面");
    }
    refresh_status();
}

pub(super) unsafe extern "C" fn select_peer(s: ID, c: SEL, sender: ID) {
    if UI_PREVIEW.load(Ordering::Relaxed) { return; }
    let item = msg0(sender, sel(c"selectedItem"));
    super::imp_peer_activate(s, c, item);
}

/// menuWillOpen:/menuDidClose: は NSMenuDelegate の固定メソッド名のため、
/// 履歴サブメニュー(再構築の保留制御)もこの IMP を共有する。sender で区別する
pub(super) unsafe extern "C" fn peer_menu_open(_s: ID, _c: SEL, menu: ID) {
    if super::history_menu_tracking(menu) {
        super::history_menu_opened();
        return;
    }
    PEER_MENU_OPEN.store(true, Ordering::Relaxed);
}

pub(super) unsafe extern "C" fn peer_menu_close(_s: ID, _c: SEL, menu: ID) {
    if super::history_menu_tracking(menu) {
        super::history_menu_closed();
        return;
    }
    PEER_MENU_OPEN.store(false, Ordering::Relaxed);
}

unsafe fn set_label(slot: &AtomicUsize, text: &str) {
    let label = slot.load(Ordering::Relaxed) as ID;
    if !label.is_null() {
        msg1_void_id(label, sel(c"setStringValue:"), nsstring(text));
        msg1_void_id(label, sel(c"setToolTip:"), nsstring(text));
    }
}

/// 役割カードの注記行の文言(ROLE_NOTE_KIND の状態から)。通常は再起動の案内、
/// KNIT_ROLE 固定中・切替の進行中はそれぞれの状況を示す
fn role_note_text() -> String {
    match ROLE_NOTE_KIND.load(Ordering::Relaxed) {
        1 => "環境変数 KNIT_ROLE で役割を固定中のため、ここでは切り替えられません".into(),
        2 => "切り替えを相手へ送りました。相手の適用を確認しています…".into(),
        3 => "相手の適用を確認しました。再起動します…".into(),
        4 => "相手の適用確認が取れないため、時間経過で再起動します…".into(),
        _ => "切り替えると、両方の端末で Knit が自動で再起動します".into(),
    }
}

/// この Mac の IPv4 一覧の表示文字列(10 秒キャッシュ)。毎秒の sync() で
/// getifaddrs を呼び続けないためのキャッシュで、Wi-Fi 切替等の変化も 10 秒で追従する
fn own_ip_text() -> String {
    static CACHE: Mutex<Option<(std::time::Instant, String)>> = Mutex::new(None);
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((at, text)) = cache.as_ref() {
        if at.elapsed() < std::time::Duration::from_secs(10) {
            return text.clone();
        }
    }
    let ips = knit_common::discover::local_ipv4s();
    let text = if ips.is_empty() {
        "このMacのアドレス: (取得できません)".to_string()
    } else {
        format!(
            "このMacのアドレス: {}",
            ips.iter()
                .map(|ip| ip.to_string())
                .collect::<Vec<_>>()
                .join("、")
        )
    };
    *cache = Some((std::time::Instant::now(), text.clone()));
    text
}

/// この Mac の接続キーのフィンガープリント(表示用・初回のみ取得)。毎秒の
/// sync() がキーチェーンへ繰り返しアクセスしないように一度だけ計算して保つ。
/// 鍵は生で出さない(common 側の fingerprint は SHA-256 の先頭8桁のみ)
fn key_fingerprint() -> Option<String> {
    static CACHE: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    CACHE
        .get_or_init(|| {
            knit_common::envutil::get("KNIT_TOKEN")
                .filter(|t| !t.is_empty())
                .or_else(|| knit_common::credentials::load().ok().flatten())
                .map(|t| knit_common::secure::fingerprint(&t))
        })
        .clone()
}

/// 「登録の管理」行の文言(登録済み台数の常時表示)。PEERS の件数から組み立てる
/// 純粋関数(単体テストで守る)。2台以上でしか「操作する端末」が出ないため、
/// 1対1のときに台数が見えない問題への対策として常時出す
fn registration_text(peers: usize) -> String {
    format!("登録の管理 · 登録済み: {peers}台")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registration_text_shows_count_always() {
        assert_eq!(registration_text(0), "登録の管理 · 登録済み: 0台");
        assert_eq!(registration_text(1), "登録の管理 · 登録済み: 1台");
        assert_eq!(registration_text(3), "登録の管理 · 登録済み: 3台");
    }

    /// 操作ページの注記: 固定キーが無いときは空(=行が見えない)、あるときは
    /// キー名と「上書きされる」理由が出る
    #[test]
    fn operation_env_note_names_fixed_keys() {
        assert_eq!(operation_env_note(&[]), "", "固定なしは空=非表示");
        let text = operation_env_note(&["KNIT_SWITCH_MODE", "KNIT_SCROLL_DIV"]);
        assert!(text.contains("KNIT_SWITCH_MODE・KNIT_SCROLL_DIV"), "{text}");
        assert!(text.contains("上書きされます"), "理由も出す: {text}");
        assert!(text.contains("初期化できます"), "対処の導線も出す: {text}");
    }

    /// 保存状態ラベル: 固定キーが無い/少ない/多いの3状態。多いときは
    /// 先頭3件+残件数に丸めて 1 行に収める
    #[test]
    fn save_status_initial_lists_keys_and_compacts_many() {
        assert_eq!(save_status_initial(&[]), "変更はこのMacに自動保存されます");
        assert_eq!(
            save_status_initial(&["KNIT_ROLE"]),
            "自動保存 · 起動時は KNIT_ROLE が優先されます"
        );
        assert_eq!(
            save_status_initial(&["KNIT_ROLE", "KNIT_SHARE", "KNIT_CLIP"]),
            "自動保存 · 起動時は KNIT_ROLE・KNIT_SHARE・KNIT_CLIP が優先されます"
        );
        let many = save_status_initial(&[
            "KNIT_ROLE", "KNIT_SHARE", "KNIT_CLIP", "KNIT_SIDE", "KNIT_AUDIO",
        ]);
        assert!(many.starts_with("自動保存 · 起動時は KNIT_ROLE・KNIT_SHARE・KNIT_CLIP ほか2件"), "{many}");
        assert!(many.contains("初期化できます"), "{many}");
    }
}

fn method_index() -> i64 {
    if crate::HOTKEY_ONLY.load(Ordering::Relaxed) { 2 }
    else if crate::SWITCH_DELAY_MS.load(Ordering::Relaxed) > 0 { 1 }
    else if crate::EDGE_TAPS.load(Ordering::Relaxed) == 1 { 3 }
    else { 0 }
}

unsafe fn sync_peer_picker() {
    let pop = PEER_POP.load(Ordering::Relaxed) as ID;
    if pop.is_null() || PEER_MENU_OPEN.load(Ordering::Relaxed) { return; }
    let (mut entries, selected) = {
        let peers = crate::PEERS.lock().unwrap_or_else(|e| e.into_inner());
        let active = *crate::ACTIVE_PEER.lock().unwrap_or_else(|e| e.into_inner());
        // 項目名は「表示名 (IP)」: エイリアス優先・同じコンピュータ名の
        // 複数台を IP で区別できる形式(conn::peer_display_label と共通)
        (
            peers
                .iter()
                .map(|p| {
                    (
                        p.id.clone(),
                        crate::conn::peer_display_label(p.alias.as_deref(), &p.name, &p.ip.to_string()),
                    )
                })
                .collect::<Vec<_>>(),
            active,
        )
    };
    let selectable = entries.len() > 1 && !UI_PREVIEW.load(Ordering::Relaxed);
    if entries.is_empty() {
        entries.push((String::new(), if crate::CONNECTED.load(Ordering::Relaxed) {
            crate::active_peer_label()
        } else { "接続先を待っています".into() }));
    }
    let mut seen = PEER_CHOICES.lock().unwrap_or_else(|e| e.into_inner());
    if *seen != entries {
        msg0_void(pop, sel(c"removeAllItems"));
        for (index, (id, name)) in entries.iter().enumerate() {
            msg1_void_id(pop, sel(c"addItemWithTitle:"), nsstring(name));
            let item_at: unsafe extern "C" fn(ID, SEL, isize) -> ID =
                std::mem::transmute(crate::objc_msgSend as *const () as usize);
            let item = item_at(pop, sel(c"itemAtIndex:"), index as isize);
            msg1_void_id(item, sel(c"setRepresentedObject:"), nsstring(id));
        }
        *seen = entries;
    }
    msg1_void_i64(pop, sel(c"selectItemAtIndex:"), selected.min(seen.len() - 1) as i64);
    msg1_void_u8(pop, sel(c"setEnabled:"), selectable as u8);
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
pub(super) unsafe extern "C" fn scroll_speed(_s: ID, _c: SEL, sender: ID) {
    let f: unsafe extern "C" fn(ID, SEL) -> f64 =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    crate::set_scroll_div(260.0 - f(sender, sel(c"doubleValue")));
    preferences::save();
}

/// 音量スライダの値表示(1.0 = 100%)
fn gain_text(g: f32) -> String {
    format!("{}%", (g * 100.0).round() as i32)
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
    let l = SAVE_LABEL.load(Ordering::Relaxed) as ID;
    if !l.is_null() {
        msg1_void_id(l, sel(c"setStringValue:"), nsstring(text));
    }
}
/// 「接続先のWindows」入力欄(保存ハンドラが本文を読み取る用)
pub(super) fn host_field() -> usize {
    HOST_FIELD.load(Ordering::Relaxed)
}
/// 「選択中の端末の名前」入力欄(保存ハンドラが本文を読み取る用)
pub(super) fn alias_field() -> usize {
    ALIAS_FIELD.load(Ordering::Relaxed)
}
/// 「このMacの名前」入力欄(保存ハンドラが本文を読み取る用)
pub(super) fn own_name_field() -> usize {
    OWN_NAME_FIELD.load(Ordering::Relaxed)
}

/// エイリアス欄の現在値をアクティブな端末へ合わせる。端末が切り替わった時だけ
/// 書き換える(利用者の入力中に毎秒の sync() が書き換えるのを防ぐ)
unsafe fn sync_alias_field(preview: bool, connected: bool) {
    let field = ALIAS_FIELD.load(Ordering::Relaxed) as ID;
    if field.is_null() {
        return;
    }
    let (id, alias) = {
        let peers = crate::PEERS.lock().unwrap_or_else(|e| e.into_inner());
        let act = *crate::ACTIVE_PEER.lock().unwrap_or_else(|e| e.into_inner());
        match peers.get(act) {
            Some(p) => (p.id.clone(), p.alias.clone().unwrap_or_default()),
            None => (String::new(), String::new()),
        }
    };
    let mut last = ALIAS_FIELD_PEER.lock().unwrap_or_else(|e| e.into_inner());
    if *last != id {
        msg1_void_id(field, sel(c"setStringValue:"), nsstring(&alias));
        *last = id;
    }
    // 未接続では保存先の端末が決まらないため編集させない(preview は見た目確認用)
    msg1_void_u8(field, sel(c"setEnabled:"), (connected || preview) as u8);
}
pub(super) unsafe extern "C" fn navigation_toggle(_s: ID, _c: SEL, sender: ID) {
    crate::trackpad::set_navigation(crate::msg0_isize(sender,sel(c"state")) != 0);
    preferences::save();
}
pub(super) unsafe extern "C" fn pinch_toggle(_s: ID, _c: SEL, sender: ID) {
    crate::trackpad::set_enabled(crate::msg0_isize(sender,sel(c"state")) != 0);
    preferences::save();
}

/// 「その他」の詳細記録チェック(--diag 相当の runtime 切替)。出力スレッドは
/// 常駐しているため、フラグを書き換えるだけで即座に効く
pub(super) unsafe extern "C" fn diag_log(_s: ID, _c: SEL, sender: ID) {
    crate::DIAG_ENABLED.store(crate::msg0_isize(sender, sel(c"state")) != 0, Ordering::Relaxed);
    eprintln!("[gui] diag_log -> {}", crate::DIAG_ENABLED.load(Ordering::Relaxed));
    preferences::save();
}

pub(super) unsafe fn sync() {
    sync_role();
    let preview = UI_PREVIEW.load(Ordering::Relaxed);
    let connected = crate::CONNECTED.load(Ordering::Relaxed);
    let remote = crate::WIN_MODE.load(Ordering::Relaxed);
    let android = !preview && crate::active_peer_is_android();
    let android_app = !preview && crate::active_peer_is_android_app();
    let peer = crate::active_peer_label();
    let enable = |slot: &AtomicUsize, enabled: bool| {
        let view = slot.load(Ordering::Relaxed) as ID;
        if !view.is_null() { msg1_void_u8(view, sel(c"setEnabled:"), enabled as u8); }
    };
    let method = method_index();
    let pop = SWITCH_POP.load(Ordering::Relaxed) as ID;
    if !pop.is_null() { msg1_void_i64(pop, sel(c"selectItemAtIndex:"), method); }
    // 「ショートカットのみ」の項目名は実際の切替キー名へ(方式とキーの行が離れて
    // いるため、この行だけで「どのキーで切替できるか」が読み取れるようにする)。
    // 項目位置は常に 2(=3番目)。Windows 側の同じ項目も対称に書き換わる
    if !pop.is_null() {
        let title = format!(
            "切替キー（{}）のみ",
            knit_common::keymap::mac_key_label(crate::hotkey_kc())
        );
        let item_at: unsafe extern "C" fn(ID, SEL, isize) -> ID =
            std::mem::transmute(crate::objc_msgSend as *const () as usize);
        let item = item_at(pop, sel(c"itemAtIndex:"), 2);
        if !item.is_null() {
            msg1_void_id(item, sel(c"setTitle:"), nsstring(&title));
        }
    }
    // 速度越境(FAST_EDGE)の説明は、誤爆防止が効く方式(2回触れる/少し待つ)の
    // ときだけ見せる。「1回触れる」は既に1回で切替のため案内の意味が無く、
    // 「ショートカットのみ」は境界切替自体が無い。無効化(KNIT_FAST_EDGE=0)
    // 済みの環境でも案内は要らない
    let fast_hint = FAST_EDGE_HINT.load(Ordering::Relaxed) as ID;
    if !fast_hint.is_null() {
        let show = method <= 1 && crate::FAST_EDGE.load(Ordering::Relaxed);
        msg1_void_u8(fast_hint, sel(c"setHidden:"), !show as u8);
    }
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
    let keys = HOTKEY_POP.load(Ordering::Relaxed) as ID;
    if !keys.is_null() {
        let current = crate::hotkey_kc();
        // 未知のキー(env で指定)は末尾の「現在のキー（コードN）」項目を選ぶ
        let index = HOTKEY_CHOICES
            .iter()
            .position(|k| *k == current)
            .unwrap_or(HOTKEY_CHOICES.len());
        msg1_void_i64(keys, sel(c"selectItemAtIndex:"), index as i64);
    }
    sync_peer_picker();
    // 操作する端末の行は、選べる相手が 2 台以上のときだけ見せる(1対1では行ごと隠す)
    let multi_peer = preview
        || crate::PEERS.lock().unwrap_or_else(|e| e.into_inner()).len() > 1;
    for v in PEER_VIEWS.lock().unwrap_or_else(|e| e.into_inner()).iter() {
        let v = *v as ID;
        if !v.is_null() { msg1_void_u8(v, sel(c"setHidden:"), !multi_peer as u8); }
    }
    let pinch = PINCH_SWITCH.load(Ordering::Relaxed) as ID;
    if !pinch.is_null() { msg1_void_u8(pinch,sel(c"setState:"),crate::trackpad::ENABLED.load(Ordering::Relaxed) as u8); }
    enable(&PINCH_SWITCH, preview || crate::trackpad::AVAILABLE.load(Ordering::Relaxed));
    let nav = NAV_SWITCH.load(Ordering::Relaxed) as ID;
    if !nav.is_null() { msg1_void_i64(nav,sel(c"setState:"),crate::trackpad::NAV_ENABLED.load(Ordering::Relaxed) as i64); }
    enable(&NAV_SWITCH, preview || crate::trackpad::navigation_available());
    // 詳細記録(--diag 相当)は設定ファイルの復元でもここで画面へ反映する。
    // 診断行は自分の端末のログへ出るもののため遠隔適用は無い(常にローカル値)
    let diag = DIAG_SWITCH.load(Ordering::Relaxed) as ID;
    if !diag.is_null() {
        msg1_void_i64(diag, sel(c"setState:"), crate::DIAG_ENABLED.load(Ordering::Relaxed) as i64);
    }
    let hint=GESTURE_HINT.load(Ordering::Relaxed) as ID;
    if !hint.is_null() {
        let text=if preview {"タブレット操作中に有効 · 上で一時停止すると最近のタスクを表示"}
            else if !crate::trackpad::navigation_available() {"指の位置を取得できません。ピンチと通常のスクロールを利用できます"}
            else if !crate::trackpad::NAV_ENABLED.load(Ordering::Relaxed) {"ナビゲーションはオフです。スクロールとピンチは個別に利用できます"}
            else if android && remote {"タブレットを操作中 · 上で一時停止すると最近のタスクを表示"}
            else {"タブレットへ操作を切り替えると有効になります"};
        set_label(&GESTURE_HINT,text);
    }
    // KNIT_SHARE で禁じられた項目は、設定画面から許可できない
    let cap = knit_common::share::env_cap();
    enable(&super::PREFS_CHK_CLIP, cap.clip && !preview);
    enable(&super::PREFS_CHK_FILES, cap.files && !preview);
    enable(&super::PREFS_CHK_HISTORY, cap.clip && !preview);
    // 「操作」ページの環境変数固定の注記(KNIT_SWITCH_MODE 等)。遠隔適用や
    // 設定の読み込みで env が変わることは無いが、毎秒の sync() で迷いなく出す
    let op_fixed: Vec<&str> = OPERATION_ENV_KEYS
        .iter()
        .filter(|k| crate::envutil::get(k).is_some())
        .copied()
        .collect();
    set_label(&ENV_NOTE_LABEL, &operation_env_note(&op_fixed));
    let audio_ok = knit_common::share::env_cap().audio;
    enable(&super::PREFS_CHK_SPK, !android && !preview && audio_ok);
    enable(&super::PREFS_CHK_AUDIO, !android_app && !preview && audio_ok);
    // 相手側ラベルは接続状態によらず「どの端末との組み合わせか」を示し続ける。
    // 状態(接続済み/未接続)は下の PREFS_STATE が担うため二重に書き換えない
    set_label(&PEER_LABEL, &peer);
    let rtt = crate::RTT_MS.load(Ordering::Relaxed);
    let state = if preview { "設定画面のプレビュー".into() }
        else if connected && rtt > 0 { format!("接続済み（{}・遅延 {rtt}ms）", crate::route_label()) }
        else if connected { "接続済み".into() }
        else { "未接続(自動で再接続します)".into() };
    set_label(&PREFS_STATE, &state);
    let state_label = PREFS_STATE.load(Ordering::Relaxed) as ID;
    if !state_label.is_null() {
        let color = msg0(objc_getClass(c"NSColor".as_ptr()), sel(if connected { c"labelColor" } else { c"secondaryLabelColor" }));
        msg1_void_id(state_label, sel(c"setTextColor:"), color);
    }
    // 受け入れ範囲(KNIT_ALLOW_ANY/TS で緩んでいるときは警告色で目立たせる)。
    // 緩和は暗号化を外すものではないため、警告文でも「暗号化は効いている」ことを
    // 読み取らせる(外れるのは接続元を絞る保護の方)
    let scope = knit_common::net::accept_scope();
    let accept_text = if scope.is_wide_open() {
        format!("受け入れ範囲: {}。通信は暗号化されていますが、接続元を絞る保護が外れます。信頼できるネットワークでのみ使ってください", scope.label())
    } else {
        format!("受け入れ範囲: {}", scope.label())
    };
    set_label(&ACCEPT_LABEL, &accept_text);
    let accept_label = ACCEPT_LABEL.load(Ordering::Relaxed) as ID;
    if !accept_label.is_null() {
        let color = msg0(objc_getClass(c"NSColor".as_ptr()), sel(if scope.is_wide_open() { c"systemRedColor" } else { c"secondaryLabelColor" }));
        msg1_void_id(accept_label, sel(c"setTextColor:"), color);
    }
    // 暗号化の状態と方式(Noise・相互認証・鍵の略号)。設定は存在しないため
    // 「設定不要」の案内として常にこの行が出る(鍵は生で出さない)
    set_label(&SECURE_LABEL, &knit_common::secure::encryption_line(key_fingerprint().as_deref()));
    // 登録済みの台数(接続中の PEERS の件数)。1対1のときも台数が読めるように
    let peers_now = crate::PEERS.lock().unwrap_or_else(|e| e.into_inner()).len();
    set_label(&REG_MGMT_LABEL, &registration_text(peers_now));
    // 待受アドレス(KNIT_BIND の現在値)。既定 0.0.0.0 から変えているときの見える化
    let bind = knit_common::envutil::get("KNIT_BIND").unwrap_or_else(|| "0.0.0.0".to_string());
    set_label(&BIND_LABEL, &format!("待受け: {bind}(KNIT_BIND で変更可)"));
    // 役割カードの注記行(通常/Env固定/切替進行の文言)と、この Mac のアドレス表示
    set_label(&ROLE_NOTE_LABEL, &role_note_text());
    set_label(&OWN_IP_LABEL, &own_ip_text());
    // 「接続先のWindows」の入力と保存は、この Mac が接続しに行く側の間だけ有効。
    // 待ち受け側(ホスト役)では接続先は要らない(Windows 側の入力欄と同じ条件)
    let going_out = crate::effective_client_role();
    enable(&HOST_FIELD, going_out || preview);
    enable(&HOST_SAVE_BUTTON, going_out || preview);
    // 「選択中の端末の名前」(エイリアス)欄は端末切替に追従させる。
    // 「このMacの名前」欄は保存ハンドラだけが書き換える(入力中に触らない)
    sync_alias_field(preview, connected);
    enable(&ALIAS_SAVE_BUTTON, connected || preview);
    enable(&OWN_NAME_FIELD, !preview);
    enable(&OWN_NAME_SAVE_BUTTON, !preview);
    let hint = if preview { "入力・通信・音声は動作せず、設定も保存しません。".into() }
        else if !connected { "接続する端末でKnitを開いてください。初めてなら、この画面の「端末を登録…」から始めます。".into() }
        else if let Some((false,_))=crate::active_android_app_permissions() { "接続済みです。タブレットのKnitアプリで画面操作を許可してください。".into() }
        else { format!("画面の端や切替キーで{peer}の画面へ移れます(方法は「操作」で選べます)") };
    set_label(&CONNECTION_HINT, &hint);
    let android_hint = if preview { String::new() }
        else if android_app {
            match crate::active_android_app_permissions() {
                Some((true,true))=>"Androidアプリ：画面操作・キーボードを許可済み".into(),
                Some((true,false))=>"Androidアプリ：文字入力にはKnitキーボードを選択してください".into(),
                _=>"Androidアプリ：タブレットで画面操作を許可してください".into(),
            }
        } else {
        let (_, status) = crate::android::state::snapshot();
        if let Some(problem) = &status.problem { format!("Android: {problem}") }
        else if status.tablets.is_empty() { String::new() }
        else { let tablet = &status.tablets[0]; format!("Android: {} — {}（{}台）", tablet.name, tablet.phase.summary(), status.tablets.len()) }
    };
    set_label(&ANDROID_STATE, &android_hint);
    set_label(&AUDIO_LABEL, if android_app { "音声共有（Androidアプリ版は未対応）" } else { "接続先の音声をこのMacで再生" });
    set_label(&SPEAKER_LABEL, if android { "相手のスピーカーをミュート（PCのみ）" } else { "接続中は相手のスピーカーをミュート（相手が音声転送をオフの間は適用されません）" });
    set_label(&SHARE_HINT, if android_app { "ファイルはDownload/Knitへ保存します。日本語はタブレット操作中に自動で出る入力欄から送れます。" } else if android { "ファイルはDownloadへ保存します。タブレットの音はそのまま残ります。" } else if knit_common::share::env_cap() != knit_common::share::Scope::ALL { "環境変数 KNIT_SHARE で制限中のため、一部の項目は変更できません。" } else { "切断すると元に戻ります。異常終了したときも、Windows 側は次回起動時に自動で戻します。" });
    // 再生音量スライダ(起動時の設定復元・リモート適用を画面へ反映)
    let gain_slider = AUDIO_GAIN_SLIDER.load(Ordering::Relaxed) as ID;
    if !gain_slider.is_null() {
        let get: unsafe extern "C" fn(ID, SEL) -> f64 =
            std::mem::transmute(crate::objc_msgSend as *const () as usize);
        let set: unsafe extern "C" fn(ID, SEL, f64) =
            std::mem::transmute(crate::objc_msgSend as *const () as usize);
        let g = crate::audio::gain() as f64;
        if get(gain_slider, sel(c"doubleValue")) != g {
            set(gain_slider, sel(c"setDoubleValue:"), g);
        }
    }
    set_label(&AUDIO_GAIN_LABEL, &gain_text(crate::audio::gain()));
    let div = crate::scroll_div();
    set_label(&PREFS_GAIN_LABEL, if div <= 40.0 { "速め" } else if div >= 140.0 { "遅め" } else { "標準" });
    // タブレット項目は、タブレットが見つかっている(または接続中の)時だけ見せる
    let tablets = !crate::android::state::snapshot().1.tablets.is_empty();
    let show_tablet = preview || tablets || android || android_app;
    for v in TABLET_VIEWS.lock().unwrap_or_else(|e| e.into_inner()).iter() {
        let v = *v as ID;
        if !v.is_null() { msg1_void_u8(v, sel(c"setHidden:"), !show_tablet as u8); }
    }
    let update = UPDATE_BUTTON.load(Ordering::Relaxed) as ID;
    if !update.is_null() {
        msg1_void_id(update, sel(c"setTitle:"), nsstring(&crate::updater::menu_title()));
        msg1_void_u8(update, sel(c"setEnabled:"), !preview as u8);
    }
    let canvas = LAYOUT_CANVAS.load(Ordering::Relaxed) as ID;
    if !canvas.is_null() && !LAY_DRAG.load(Ordering::Relaxed) { msg1_void_u8(canvas, sel(c"setNeedsDisplay:"), 1); }
    // 配置の現在値(モニター名+辺)も毎秒揃える(接続の増減・設定変更の反映)
    sync_layout_values();
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
    set_label(&LAYOUT_VALUES, &layout_values_text());
}

pub(super) unsafe fn build(target: ID) -> ID {
    let f: unsafe extern "C" fn(ID, SEL, NSRect, u64, u64, u8) -> ID =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    let win = f(
        msg0(objc_getClass(c"NSWindow".as_ptr()), sel(c"alloc")),
        sel(c"initWithContentRect:styleMask:backing:defer:"),
        NSRect {
            x: 0.0,
            y: 0.0,
            w: 820.0,
            h: 872.0,
        },
        1 | 2 | 4,
        2,
        0,
    );
    if win.is_null() {
        return win;
    }
    msg1_void_id(win, sel(c"setTitle:"), nsstring("Knit 設定"));
    msg1_void_u8(win, sel(c"setReleasedWhenClosed:"), 0);
    msg0_void(win, sel(c"center"));
    let cv = msg0(win, sel(c"contentView"));
    let sidebar = view(
        cv,
        NSRect {
            x: 0.0,
            y: 0.0,
            w: 172.0,
            h: 872.0,
        },
    );
    surface(sidebar, c"windowBackgroundColor", 0.0);
    let bytes = include_bytes!("../../../../assets/AppIcon.iconset/icon_128x128.png");
    let data_fn: unsafe extern "C" fn(ID, SEL, *const u8, usize) -> ID =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    let data = data_fn(
        objc_getClass(c"NSData".as_ptr()),
        sel(c"dataWithBytes:length:"),
        bytes.as_ptr(),
        bytes.len(),
    );
    let init_image: unsafe extern "C" fn(ID, SEL, ID) -> ID =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    let icon = init_image(
        msg0(objc_getClass(c"NSImage".as_ptr()), sel(c"alloc")),
        sel(c"initWithData:"),
        data,
    );
    let iv = msg0(objc_getClass(c"NSImageView".as_ptr()), sel(c"new"));
    frame(
        iv,
        NSRect {
            x: 20.0,
            y: 737.0,
            w: 32.0,
            h: 32.0,
        },
    );
    msg1_void_id(iv, sel(c"setImage:"), icon);
    msg1_void_id(sidebar, sel(c"addSubview:"), iv);
    label(sidebar, "Knit", 59.0, 739.0, 110.0, 20.0, false);
    label(
        sidebar,
        "机を、ひとつの手元で。",
        20.0,
        710.0,
        150.0,
        10.0,
        true,
    );
    for (i, title) in ["接続", "画面配置", "操作", "共有"].iter().enumerate() {
        let b = button(
            sidebar,
            target,
            title,
            c"sdNavigate:",
            NSRect {
                x: 16.0,
                y: 618.0 - i as f64 * 48.0,
                w: 140.0,
                h: 36.0,
            },
        );
        msg1_void_u8(b, sel(c"setBordered:"), 0);
        msg1_void_i64(b, sel(c"setAlignment:"), 0);
        let image_fn: unsafe extern "C" fn(ID, SEL, ID, ID) -> ID =
            std::mem::transmute(crate::objc_msgSend as *const () as usize);
        let symbol = image_fn(
            objc_getClass(c"NSImage".as_ptr()),
            sel(c"imageWithSystemSymbolName:accessibilityDescription:"),
            nsstring(["link", "display.2", "keyboard", "square.and.arrow.up"][i]),
            nsstring(title),
        );
        msg1_void_id(b, sel(c"setImage:"), symbol);
        msg1_void_i64(b, sel(c"setImagePosition:"), 2);
        msg1_void_i64(b, sel(c"setTag:"), i as i64);
        msg1_void_i64(b, sel(c"setButtonType:"), 1); // push-on/push-off
        TABS[i].store(b as usize, Ordering::Relaxed);
    }
    label(
        sidebar,
        &format!("バージョン {}", crate::VERSION_STR),
        20.0,
        24.0,
        150.0,
        10.0,
        true,
    );
    let pages: Vec<ID> = (0..PAGES.len())
        .map(|i| {
            let p = view(
                cv,
                NSRect {
                    x: 184.0,
                    y: 48.0,
                    w: 620.0,
                    h: 812.0,
                },
            );
            surface(p, c"controlBackgroundColor", 0.0);
            PAGES[i].store(p as usize, Ordering::Relaxed);
            p
        })
        .collect();
    let titles = [
        ("接続", "この Mac の役割と、つながっている端末の状態です。"),
        ("画面配置", "接続先の位置を、実際の画面配置に合わせます。"),
        ("操作", "画面を移る方法と、スクロールの感触です。"),
        ("共有", "この Mac が渡すものと、受け取るものを選びます。"),
    ];
    for (p, (title, sub)) in pages.iter().zip(titles) {
        label(*p, title, 28.0, 752.0, 565.0, 24.0, false);
        label(*p, sub, 28.0, 723.0, 565.0, 12.0, true);
    }
    let p = pages[0];
    // Deskflow 流の構成: 役割の 2 択と接続先(IP)・このMacの名前を最上部のカードへ
    // 置く(役割と接続先は一对のため。Windows 側設定の「接続先のMac」と対)。
    // 中: 端末の状態と選択中の端末の名前・通信の保護の状況。下: 登録と困った時。
    // 下のカードには初期化の注意2行も常時出す(ボタンを開く前に読めるように)
    group(p, 476.0, 264.0);
    group(p, 242.0, 262.0);
    group(p, 18.0, 258.0);
    // 役割(どちらがホストか)。選ぶと相手にも伝わり、両方が再起動して切り替わる
    label(p, "この Mac の役割", 40.0, 708.0, 300.0, 13.0, false);
    ROLE_RADIO[0].store(
        radio(p, target, "この Mac がホスト(既定) — Windows が接続しに来ます", c"sdRole:", 0, NSRect { x: 40.0, y: 676.0, w: 516.0, h: 22.0 }) as usize,
        Ordering::Relaxed,
    );
    ROLE_RADIO[1].store(
        radio(p, target, "Windows がホスト — この Mac が接続しに行きます", c"sdRole:", 1, NSRect { x: 40.0, y: 650.0, w: 516.0, h: 22.0 }) as usize,
        Ordering::Relaxed,
    );
    // 注記行は状況で文言が変わる(通常の再起動案内・KNIT_ROLE 固定中・切替の進行)
    ROLE_NOTE_LABEL.store(
        label(p, &role_note_text(), 40.0, 626.0, 516.0, 11.0, true) as usize,
        Ordering::Relaxed,
    );
    // 接続先のWindows(この Mac が接続しに行く側のときだけ使う。Windows 側設定の
    // 「接続先のMac」と対)。空欄=LAN からの自動発見。保存で ~/.config/knit/env へ
    label(p, "接続先のWindows", 40.0, 596.0, 300.0, 13.0, false);
    let host_now = knit_common::envutil::get("KNIT_HOST").unwrap_or_default();
    HOST_FIELD.store(
        text_field(
            p,
            "接続先のWindowsのアドレス",
            "192.168.1.23(空欄で自動発見)",
            &host_now,
            NSRect { x: 40.0, y: 562.0, w: 336.0, h: 26.0 },
        ) as usize,
        Ordering::Relaxed,
    );
    HOST_SAVE_BUTTON.store(
        button(p, target, "保存して再接続", c"sdSaveHost:", NSRect { x: 392.0, y: 556.0, w: 164.0, h: 30.0 }) as usize,
        Ordering::Relaxed,
    );
    // この Mac 自身のアドレス(相手側で IP を指定するときの確認用。複数 NIC は列挙)
    OWN_IP_LABEL.store(
        label(p, &own_ip_text(), 40.0, 538.0, 516.0, 11.0, true) as usize,
        Ordering::Relaxed,
    );
    // このMacの名前(相手への名乗り名。hello/hello_ok の name になる。空欄=ホスト名)。
    // 相手側の画面・通知に載るため、複数台をつなぐときの区別に使える
    label(p, "このMacの名前", 40.0, 516.0, 300.0, 13.0, false);
    let own_now = crate::OWN_NAME.lock().unwrap_or_else(|e| e.into_inner()).clone();
    OWN_NAME_FIELD.store(
        text_field(
            p,
            "このMacの名前",
            &format!("空欄でホスト名({})", crate::hostname_label()),
            &own_now,
            NSRect { x: 40.0, y: 484.0, w: 336.0, h: 26.0 },
        ) as usize,
        Ordering::Relaxed,
    );
    OWN_NAME_SAVE_BUTTON.store(
        button(p, target, "保存", c"sdSaveOwnName:", NSRect { x: 392.0, y: 478.0, w: 164.0, h: 30.0 }) as usize,
        Ordering::Relaxed,
    );
    // 端末の状態(この Mac と相手)。状態行の 1 行で読み取れ、続きは必要な時だけ出る
    PREFS_STATE.store(
        label(p, "接続を確認しています…", 40.0, 470.0, 540.0, 14.0, false) as usize,
        Ordering::Relaxed,
    );
    symbol(p, "laptopcomputer", 111.0, 426.0, 40.0);
    symbol(p, "desktopcomputer", 439.0, 426.0, 40.0);
    label(p, "このMac", 107.0, 398.0, 145.0, 14.0, false);
    // 相手側の名前は接続先に応じて書き換わる(Android 接続中は端末名)
    PEER_LABEL.store(
        label(p, "Windows", 427.0, 398.0, 145.0, 14.0, false) as usize,
        Ordering::Relaxed,
    );
    symbol(p, "link", 282.0, 436.0, 26.0);
    // 操作する端末(登録済みが 2 台以上のときだけ行ごと見せる。sync() が切り替える)。
    // 状況行(下 3 行)はこのポップアップより下に置く、重ならないようにする
    let peer_caption = label(p, "操作する端末", 40.0, 371.0, 105.0, 12.0, true);
    let peer_pop = popup(p, target, &[], c"sdSelectPeer:", NSRect { x: 150.0, y: 368.0, w: 428.0, h: 28.0 });
    msg1_void_id(peer_pop, sel(c"setAccessibilityLabel:"), nsstring("操作する接続先"));
    msg1_void_id(msg0(peer_pop, sel(c"menu")), sel(c"setDelegate:"), target);
    PEER_POP.store(peer_pop as usize, Ordering::Relaxed);
    *PEER_VIEWS.lock().unwrap_or_else(|e| e.into_inner()) =
        vec![peer_caption as usize, peer_pop as usize];
    // 選択中の端末の名前(エイリアス)。同じコンピュータ名の端末が複数あるときの
    // 区別や、履歴・通知の名前を好きな呼び名に変えられる。空權でコンピュータ名に戻る
    label(p, "選択中の端末の名前", 40.0, 344.0, 200.0, 12.0, true);
    ALIAS_FIELD.store(
        text_field(
            p,
            "選択中の端末の名前",
            "空欄でコンピュータ名に戻る",
            "",
            NSRect { x: 40.0, y: 310.0, w: 336.0, h: 26.0 },
        ) as usize,
        Ordering::Relaxed,
    );
    ALIAS_SAVE_BUTTON.store(
        button(p, target, "保存", c"sdSaveAlias:", NSRect { x: 392.0, y: 304.0, w: 164.0, h: 30.0 }) as usize,
        Ordering::Relaxed,
    );
    // 状況があった時だけ内容が入る行(空の間は見えない)
    ANDROID_STATE.store(
        label(p, "", 40.0, 322.0, 540.0, 11.0, true) as usize,
        Ordering::Relaxed,
    );
    // 通信の保護(常時暗号化・相互認証)。設定は存在しない(常時ON)のため、「設定不要」と
    // 鍵の略号(フィンガープリント)を常時表示する。sync() が鍵の現在値で書き換える
    SECURE_LABEL.store(
        label(p, "", 40.0, 300.0, 540.0, 11.0, true) as usize,
        Ordering::Relaxed,
    );
    // 接続の受け入れ範囲(net.rs の判定結果)。KNIT_ALLOW_ANY/TS の緩和が
    // 画面から見えない問題への対策。ANY のときは sync() が警告色へ変える
    ACCEPT_LABEL.store(
        label(p, "", 40.0, 278.0, 540.0, 11.0, true) as usize,
        Ordering::Relaxed,
    );
    // 未接続・許可待ちの案内と、接続済みでも移り方の一行
    CONNECTION_HINT.store(
        label(p, "", 40.0, 254.0, 540.0, 11.0, true) as usize,
        Ordering::Relaxed,
    );
    // 待受アドレス(KNIT_BIND の指定)。既定 0.0.0.0(全 NIC)から変えている
    // ときの見える化。sync() が env の現在値で書き換える
    BIND_LABEL.store(
        label(p, "", 40.0, 230.0, 540.0, 11.0, true) as usize,
        Ordering::Relaxed,
    );
    // 経路ごとのポート一覧(固定)。ファイアウォール整理やポート占有の確認用
    PORTS_LABEL.store(
        label(
            p,
            &format!(
                "経路: 本線 {} / 音声 {} / ファイル {} / 発見 {}(UDP) / 登録 {}",
                knit_common::proto::PORT,
                knit_common::proto::PORT + 1,
                knit_common::proto::PORT + knit_common::bulk::PORT_OFFSET,
                knit_common::proto::PORT + knit_common::discover::PORT_OFFSET,
                knit_common::pairing::PORT
            ),
            40.0,
            208.0,
            540.0,
            11.0,
            true,
        ) as usize,
        Ordering::Relaxed,
    );
    label(p, "端末の登録", 40.0, 214.0, 300.0, 13.0, false);
    button(
        p,
        target,
        "端末を登録…",
        c"sdRegistration:",
        NSRect { x: 392.0, y: 208.0, w: 188.0, h: 30.0 },
    );
    // 初期化(=鍵の再生成)の破壊性は、ダイアログを開く前のこの行で読み取れるように
    // 常時出す(改善依頼: ボタンからは読めなかったため)
    label(
        p,
        "初期化すると全端末が締め出され、再登録が必要です(取り消せません)",
        40.0,
        186.0,
        540.0,
        11.0,
        true,
    );
    // 登録の管理行には登録済み台数を常時表示(1対1でも台数が出る)。
    // sync() が PEERS の件数で書き換える
    REG_MGMT_LABEL.store(
        label(p, &registration_text(0), 40.0, 164.0, 336.0, 13.0, false) as usize,
        Ordering::Relaxed,
    );
    button(
        p,
        target,
        "すべての登録を初期化…",
        c"sdResetRegistration:",
        NSRect { x: 392.0, y: 156.0, w: 188.0, h: 30.0 },
    );
    // 1台だけの剥奪は未実装のため、初期化+再登録で対応する案内を直下へ
    label(
        p,
        "特定の1台だけ外すには、初期化後に必要な端末を再登録します",
        40.0,
        134.0,
        540.0,
        11.0,
        true,
    );
    divider(p, 124.0);
    // 詳細記録(--diag 相当)。GUI から切替できるようにした項目で、お問い合わせ時に
    // 担当者が入れ替わることもあるため設定として残る(出力は 1秒ごとの [diag] 行)
    check(
        p,
        target,
        "詳しく記録(1秒ごとの診断行・お問い合わせ時に)",
        c"sdDiagLog:",
        &DIAG_SWITCH,
        92.0,
    );
    // バージョンはサイドバー下部に既にあるため、ここには置かない
    label(p, "その他", 40.0, 60.0, 100.0, 13.0, false);
    UPDATE_BUTTON.store(
        button(
            p,
            target,
            "アップデートを確認…",
            c"sdCheckUpdate:",
            NSRect { x: 146.0, y: 58.0, w: 140.0, h: 30.0 },
        ) as usize,
        Ordering::Relaxed,
    );
    let log_btn = button(
        p,
        target,
        "ログを開く",
        c"sdOpenLog:",
        NSRect { x: 292.0, y: 58.0, w: 100.0, h: 30.0 },
    );
    // 保存場所が分からないと報告材料の切り出し自体ができないため、ボタンと
    // 注記行の両方で場所を示す(ボタンが開けない環境の代替の手がかりにもする)
    if !log_btn.is_null() {
        msg1_void_id(
            log_btn,
            sel(c"setToolTip:"),
            nsstring("/tmp/knit-mac.log を開きます。ボタンで開けない場合は、このファイルを開いてください"),
        );
    }
    button(
        p,
        target,
        "再起動",
        c"sdRestart:",
        NSRect { x: 398.0, y: 58.0, w: 84.0, h: 30.0 },
    );
    // 接続の診断(メニューバー「接続を診断…」と同じ imp_diag へ接続。
    // 設定画面だけを見ていても繋がらない原因を確認できるように)
    button(
        p,
        target,
        "接続を診断…",
        c"sdDiagnose:",
        NSRect { x: 488.0, y: 58.0, w: 108.0, h: 30.0 },
    );
    // 設定まわりの導線(保存先を開く・書き出す・読み込む・初期化)。初期化は
    // 破壊的なため行の端へ置き、注記行で対象と消えないものを読めるようにする
    button(
        p,
        target,
        "設定フォルダを開く",
        c"sdOpenSettingsDir:",
        NSRect { x: 40.0, y: 24.0, w: 132.0, h: 30.0 },
    );
    button(
        p,
        target,
        "設定を書き出す…",
        c"sdExportPrefs:",
        NSRect { x: 176.0, y: 24.0, w: 124.0, h: 30.0 },
    );
    button(
        p,
        target,
        "設定を読み込む…",
        c"sdImportPrefs:",
        NSRect { x: 304.0, y: 24.0, w: 124.0, h: 30.0 },
    );
    button(
        p,
        target,
        "すべての設定を初期化…",
        c"sdResetSettings:",
        NSRect { x: 432.0, y: 24.0, w: 164.0, h: 30.0 },
    );
    // 設定とログの保存場所(初期化の注意行と同じ「ボタンの下に1行」のパターン。
    // 設定フォルダが開けない環境の代替の手がかりにもする)
    label(
        p,
        "設定: ~/.config/knit/preferences.json ほか(「設定フォルダを開く」で開けます)・ログ: /tmp/knit-mac.log(/tmp は Mac の再起動で消えることがあります)",
        40.0,
        2.0,
        556.0,
        10.0,
        true,
    );
    let p = pages[1];
    let lay = make_layout_view(p);
    frame(
        lay,
        NSRect {
            x: 28.0,
            y: 128.0,
            w: LAY_VW,
            h: LAY_VH,
        },
    );
    msg1_void_id(p, sel(c"addSubview:"), lay);
    LAYOUT_CANVAS.store(lay as usize, Ordering::Relaxed);
    label(
        p,
        "グレー：Mac／ブルー：接続先（各モニターに機種名）。青い枠のセルへドラッグして配置（斜めも置けます）",
        28.0,
        98.0,
        560.0,
        12.0,
        true,
    );
    // 操作の説明: クリック選択・「全画面の端」への戻し方・保存済み配置の優先。
    // 初期化ボタンは持たない代わりに、モニター指定の解除(全画面)を明記する
    label(
        p,
        "ブロックのクリックで操作する端末を選べます。Mac の上へ戻すと「全画面の端（モニター指定なし）」になります。保存済みの端末は各端末の設定が優先されます",
        28.0,
        78.0,
        560.0,
        11.0,
        true,
    );
    // ドロップ結果の現在値(各端末のモニター名+辺)。redraw_layout と sync() が更新する
    LAYOUT_VALUES.store(
        label(p, "", 28.0, 56.0, 560.0, 11.0, true) as usize,
        Ordering::Relaxed,
    );
    sync_layout_values();
    // 初期化ボタンは持たない: ドラッグで好きなセルへ置き直せるため
    //(Windows 側設定の「既定(右)に戻す」は Windows に配置エディタが無い分の代替)
    let p = pages[2];
    // 切替(方式・キー・滞在時間)/ スクロール / タブレット(ある時だけ)。
    // 切替グループは説明行と滞在時間スライダの分を高く取り、下の 2 グループを
    // その分(60px)下げている
    group(p, 270.0, 148.0);
    group(p, 176.0, 78.0);
    divider(p, 350.0);
    divider(p, 208.0);
    label(p, "切替方式", 40.0, 386.0, 220.0, 13.0, false);
    let hotkey_title = format!(
        "切替キー（{}）のみ",
        knit_common::keymap::mac_key_label(crate::hotkey_kc())
    );
    SWITCH_POP.store(
        popup(
            p,
            target,
            &[
                "端に2回触れる",
                "端で少し待つ",
                // 項目名は sync() が現在の切替キー名で書き換える(初回も同じ形式で)
                hotkey_title.as_str(),
                "端に1回触れる",
            ],
            c"sdSwitchMethod:",
            NSRect {
                x: 300.0,
                y: 384.0,
                w: 252.0,
                h: 28.0,
            },
        ) as usize,
        Ordering::Relaxed,
    );
    // 速度越境(FAST_EDGE)の説明行。sync() が方式と KNIT_FAST_EDGE に応じて
    // 表示を切り替える(GESTURE_HINT と同じ扱いの 1 行ラベル)
    FAST_EDGE_HINT.store(
        label(
            p,
            "速い移動では1回の到達で切り替わります（誤発火が続く場合は KNIT_FAST_EDGE=0 で無効化）",
            40.0,
            358.0,
            516.0,
            11.0,
            true,
        ) as usize,
        Ordering::Relaxed,
    );
    label(p, "切替キー", 40.0, 318.0, 260.0, 13.0, false);
    let current = crate::hotkey_kc();
    let custom = format!("現在のキー（コード{current}）");
    let titles = [
        "F6（必要に応じてfnと併用）",
        "F8（必要に応じてfnと併用）",
        "F13",
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
            y: 316.0,
            w: 252.0,
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
    HOTKEY_POP.store(pop as usize, Ordering::Relaxed);
    // 「端で少し待つ」の滞在時間スライダ。sync() が方式に応じて一式の表示を
    // 切り替える(値の遠隔適用・復元も sync が反映する)
    {
        let delay_divider = divider(p, 306.0) as usize;
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
            276.0,
        );
        *SWITCH_DELAY_VIEWS.lock().unwrap_or_else(|e| e.into_inner()) = vec![
            delay_divider,
            caption as usize,
            SWITCH_DELAY_SLIDER.load(Ordering::Relaxed),
            SWITCH_DELAY_LABEL.load(Ordering::Relaxed),
        ];
        set_label(&SWITCH_DELAY_LABEL, &delay_text(crate::SWITCH_DELAY_MS.load(Ordering::Relaxed)));
    }
    let scroll_caption = check(
        p,
        target,
        "スクロール方向をMacに合わせる",
        c"sdScroll:",
        &PREFS_CHK_SCROLL,
        218.0,
    );
    // 方向は起動時の macOS 設定で決まり、起動中の変更は設定画面を開き直す時に
    // 反映する(この画面を開くタイミングで再取得している)
    msg1_void_id(
        scroll_caption,
        sel(c"setToolTip:"),
        nsstring("macOS の自然スクロール設定に合わせます。起動中に切り替えた場合は、この設定画面を開き直すと反映します"),
    );
    slider(
        p,
        target,
        "スクロール速度   遅い / 速い",
        260.0 - crate::scroll_div(),
        20.0,
        240.0,
        c"sdScrollSpeed:",
        &PREFS_SLIDER,
        &PREFS_GAIN_LABEL,
        184.0,
    );
    // タブレット(Android)がある時だけ見せる。無い人には関係のない項目なので隠す
    let mut tablet = vec![group(p, 72.0, 84.0) as usize, divider(p, 108.0) as usize];
    tablet.push(check(p, target, "タブレットのナビゲーション", c"sdTabletNav:", &NAV_SWITCH, 118.0) as usize);
    tablet.push(check(p, target, "ピンチで拡大・縮小", c"sdPinch:", &PINCH_SWITCH, 80.0) as usize);
    tablet.push(label(p, "", 28.0, 48.0, 565.0, 11.0, true) as usize);
    GESTURE_HINT.store(*tablet.last().unwrap(), Ordering::Relaxed);
    tablet.push(NAV_SWITCH.load(Ordering::Relaxed));
    tablet.push(PINCH_SWITCH.load(Ordering::Relaxed));
    *TABLET_VIEWS.lock().unwrap_or_else(|e| e.into_inner()) = tablet;
    // 環境変数で固定されている項目の注記(KNIT_SWITCH_MODE 等)。固定が無ければ
    // 空文字=見えない。sync() が env の現在値で書き換える
    ENV_NOTE_LABEL.store(
        label(p, "", 28.0, 22.0, 565.0, 11.0, true) as usize,
        Ordering::Relaxed,
    );
    let p = pages[3];
    // 渡すもの(テキスト・画像・ファイル)と、音
    group(p, 280.0, 138.0);
    group(p, 152.0, 114.0);
    group(p, 22.0, 80.0);
    divider(p, 344.0);
    divider(p, 188.0);
    check(p, target, "テキストと画像", c"sdClipShare:", &PREFS_CHK_CLIP, 384.0);
    label(p, "コピーした内容を、もう1台でも貼り付けられます。", 40.0, 358.0, 540.0, 12.0, true);
    check(p, target, "ファイルの受け渡し", c"sdFileShare:", &super::PREFS_CHK_FILES, 312.0);
    label(p, "コピーしたファイルや、掴んだファイルを渡せます。", 40.0, 288.0, 540.0, 12.0, true);
    AUDIO_LABEL.store(check(
        p,
        target,
        "接続先の音声をこのMacで再生",
        c"sdAudio:",
        &PREFS_CHK_AUDIO,
        228.0,
    ) as usize, Ordering::Relaxed);
    slider(
        p,
        target,
        "再生音量   小さい / 大きい",
        crate::audio::gain() as f64,
        0.0,
        2.0,
        c"sdAudioGain:",
        &AUDIO_GAIN_SLIDER,
        &AUDIO_GAIN_LABEL,
        196.0,
    );
    SPEAKER_LABEL.store(check(
        p,
        target,
        "接続中は相手のスピーカーをミュート",
        c"sdSpkMute:",
        &PREFS_CHK_SPK,
        154.0,
    ) as usize, Ordering::Relaxed);
    check(p, target, "Macのコピーを履歴に残す", c"sdLocalHistory:", &super::PREFS_CHK_HISTORY, 64.0);
    label(p, "メニューバーの「クリップボード履歴」から選び直せます。パスワードなどの機密コピーは残しません。", 40.0, 36.0, 540.0, 12.0, true);
    SHARE_HINT.store(label(
        p,
        "切断すると元に戻ります。異常終了したときも、Windows 側は次回起動時に自動で戻します。",
        28.0,
        116.0,
        565.0,
        12.0,
        true,
    ) as usize, Ordering::Relaxed);
    SAVE_LABEL.store(
        label(
            cv,
            if UI_PREVIEW.load(Ordering::Relaxed) {
                "デザイン確認モード · 設定は保存しません".to_string()
            } else {
                save_status_initial(&preferences::override_keys())
            }
            .as_str(),
            210.0,
            14.0,
            590.0,
            11.0,
            true,
        ) as usize,
        Ordering::Relaxed,
    );
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

/// 切替方式の選択肢と設定値の射影(2件目の改善)。方式の選択が滞在時間・タップ数へ
/// どう落ちるかと、設定値から選択肢へ戻る射影を単体テストで守る
#[cfg(test)]
mod switch_method_tests {
    use super::{delay_for_method, method_index};
    use super::preferences::TEST_LOCK;
    use std::sync::atomic::Ordering;

    /// 設定系 static はプロセス共有のため、preferences のテストと直列化する
    #[test]
    fn method_selection_and_state_project_both_ways() {
        let _serial = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let saved = (
            crate::HOTKEY_ONLY.load(Ordering::Relaxed),
            crate::EDGE_TAPS.load(Ordering::Relaxed),
            crate::SWITCH_DELAY_MS.load(Ordering::Relaxed),
        );
        // 方式の選択 → 設定値: 「端で少し待つ」は未設定(0)なら既定 300ms、
        // 設定済みならその値を保つ(スライダの値を方式の行き来で失わない)
        assert_eq!(delay_for_method(0, 0), 0, "2回触れる: 滞在なし");
        assert_eq!(delay_for_method(1, 0), 300, "少し待つ(未設定): 既定 300ms");
        assert_eq!(delay_for_method(1, 500), 500, "少し待つ(設定済み): 値を保つ");
        assert_eq!(delay_for_method(2, 500), 0, "ショートカットのみ: 滞在なし");
        assert_eq!(delay_for_method(3, 500), 0, "1回触れる: 滞在なし");
        // 設定値 → 方式の選択肢(遠隔適用・起動時復元後の表示)。
        // 優先順位は Mac/Windows 共通: ショートカットのみ > 待ち時間 > 1回 > 2回
        crate::HOTKEY_ONLY.store(false, Ordering::Relaxed);
        crate::EDGE_TAPS.store(2, Ordering::Relaxed);
        crate::SWITCH_DELAY_MS.store(0, Ordering::Relaxed);
        assert_eq!(method_index(), 0, "既定は 2回触れる");
        crate::SWITCH_DELAY_MS.store(300, Ordering::Relaxed);
        assert_eq!(method_index(), 1, "待ち時間があれば 少し待つ");
        crate::SWITCH_DELAY_MS.store(0, Ordering::Relaxed);
        crate::EDGE_TAPS.store(1, Ordering::Relaxed);
        assert_eq!(method_index(), 3, "1回タップは 1回触れる");
        crate::HOTKEY_ONLY.store(true, Ordering::Relaxed);
        assert_eq!(method_index(), 2, "ショートカットのみが最優先");
        crate::HOTKEY_ONLY.store(saved.0, Ordering::Relaxed);
        crate::EDGE_TAPS.store(saved.1, Ordering::Relaxed);
        crate::SWITCH_DELAY_MS.store(saved.2, Ordering::Relaxed);
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
            gen: 1,
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

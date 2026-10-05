//! 設定の分類と画面構築。入力・通信の処理は既存アクションへ委譲する。
use super::*;
use std::sync::Mutex;
static PAGES: [AtomicUsize; 4] = [const { AtomicUsize::new(0) }; 4];
static TABS: [AtomicUsize; 4] = [const { AtomicUsize::new(0) }; 4];
/// 「操作」ページのタブレット項目(タブレットが無い時は隠す)
static TABLET_VIEWS: Mutex<Vec<usize>> = Mutex::new(Vec::new());
static SAVE_LABEL: AtomicUsize = AtomicUsize::new(0);
static HOTKEY_POP: AtomicUsize = AtomicUsize::new(0);
static SWITCH_POP: AtomicUsize = AtomicUsize::new(0);
static PEER_POP: AtomicUsize = AtomicUsize::new(0);
static PEER_MENU_OPEN: AtomicBool = AtomicBool::new(false);
/// 切替方式・切替キーポップアップの NSMenu(delegate の sender 比較用)。毎秒の
/// sync() が selectItemAtIndex:/setTitle: を打つため、PEER_POP と同じ開示中保護を
/// 付ける(開示中の項目書き換えは未定義動作になるため)
static SWITCH_MENU: AtomicUsize = AtomicUsize::new(0);
static SWITCH_MENU_OPEN: AtomicBool = AtomicBool::new(false);
static HOTKEY_MENU: AtomicUsize = AtomicUsize::new(0);
static HOTKEY_MENU_OPEN: AtomicBool = AtomicBool::new(false);
static PEER_CHOICES: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());
static AUDIO_LABEL: AtomicUsize = AtomicUsize::new(0);
static SPEAKER_LABEL: AtomicUsize = AtomicUsize::new(0);
/// 音声の再生音量スライダ(Windows から届く音をこの Mac で鳴らす大きさ)
static AUDIO_GAIN_SLIDER: AtomicUsize = AtomicUsize::new(0);
static AUDIO_GAIN_LABEL: AtomicUsize = AtomicUsize::new(0);
/// 再生音量スライダの一式(区切り線・caption・スライダ・値表示)。音声転送が
/// オン・env で audio が許可・Androidアプリ版相手でない間だけ表示する
/// (SWITCH_DELAY_VIEWS と同じパターンで sync() が切り替える)
static AUDIO_GAIN_VIEWS: Mutex<Vec<usize>> = Mutex::new(Vec::new());
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
/// 接続ページの待受アドレスの行(KNIT_BIND の指定を見える化する。経路ごとの
/// ポート一覧はこの行のツールチップへ載せている)
static BIND_LABEL: AtomicUsize = AtomicUsize::new(0);
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
/// エイリアス行の一式(キャプション・欄・保存ボタン)。未接続の間は保存先の端末が
/// 決まらないため無効のまま常設せず行ごと隠す(PEER_VIEWS と同じパターンで
/// sync() が切り替える。接続済みでは現状どおり表示し Enter 保存も不変)
static ALIAS_VIEWS: Mutex<Vec<usize>> = Mutex::new(Vec::new());
/// 接続ページの「このMacの名前」入力欄(空欄=ホスト名既定。hello の名乗り名)
static OWN_NAME_FIELD: AtomicUsize = AtomicUsize::new(0);
/// このMacの名前の保存ボタン
static OWN_NAME_SAVE_BUTTON: AtomicUsize = AtomicUsize::new(0);
/// 接続ページの「直接つなぐ(手動接続)」のトークン入力欄(KNIT_TOKEN。両側へ
/// 同じ値を設定すると登録操作なしに直接つながる)。Secure 風でなく通常の
/// フィールドにするのは、生成した値を相手側へ読み取って転記する運用のため
/// (保存先の ~/.config/knit/env も平文で持つため画面だけ隠しても意味が薄い)
static TOKEN_FIELD: AtomicUsize = AtomicUsize::new(0);
/// 直接つなぐ(手動接続)の節の開閉ラベル(トークン未設定の間だけ見える1行。
/// クリックで TOKEN_VIEWS を展開する。上級者向けの機能であることを先に読ませ、
/// 未設定の多数派には 4 部品(欄・生成・保存・注記)を見せない)
static TOKEN_TOGGLE: AtomicUsize = AtomicUsize::new(0);
/// 直接つなぐの節の開閉状態(プロセス共有のためウィンドウを閉じて開き直しても
/// 維持される)。トークン設定済み(手動接続中)はこの値に依らず常に展開
static TOKEN_SECTION_OPEN: AtomicBool = AtomicBool::new(false);
/// 直接つなぐの節の部品一式(トークン欄・生成・保存して再接続・注記)。
/// TOKEN_SECTION_OPEN と KNIT_TOKEN の有無で sync_token_section が隠す
/// (TABLET_VIEWS・PEER_VIEWS と同じパターン)
static TOKEN_VIEWS: Mutex<Vec<usize>> = Mutex::new(Vec::new());
/// トークンの保存ボタン(入力欄の隣。「生成」で作ってから保存する導線)
static TOKEN_SAVE_BUTTON: AtomicUsize = AtomicUsize::new(0);
/// 直接つなぐの説明(未設定時は使い方、設定中は運用の注記へ sync が切り替える)
static TOKEN_NOTE_L1: AtomicUsize = AtomicUsize::new(0);
/// 保存系エラーの出元(設定「接続」のどの欄で起きたか)。Token 由来はトークン欄の
/// 直下(TOKEN_NOTE_L1)を差し替える。Host・OwnName・Alias 由来は欄の下に行の空きが
/// 無いため保存状態行(SAVE_LABEL・全ページ共通の最下部)へ出す(このMacのアドレスの
/// 行を差し替えるとエラー中にアドレスが見えなくなるため。名前欄・エイリアス欄の
/// 下にも注記行を新設しない=全ページ共通の最下部に出す方が「どこを見れば良いか」
/// 一本化できる)
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum SaveErrorSource {
    Host,
    Token,
    OwnName,
    Alias,
    /// preferences.json 自体が壊れていた(起動時に退避して初期設定で起動した)。
    /// 次の保存成功で外れる(preferences::save が解除する)
    PrefsFile,
    /// peer-sides.json(端末の配置と名前)が壊れて退避された(tap が検知して
    /// gui::report_peer_sides_error から設定する)。tap::save_peer_sides の
    /// 成功で外れる(clear_peer_sides_error)
    PeerSidesFile,
}
/// 設定「接続」の保存系(接続先・直接つなぐ・トークン生成)の直近エラー。設定
/// ウィンドウを開いているのに通知センターへ逃げて数秒で消える問題への対策で、
/// 次の保存成功まで欄の隣の注記行(出元に対応する TOKEN_NOTE/HOST_NOTE/SAVE_LABEL)
/// へ出し続ける。sync() が毎秒書き直すため、preferences::save() が既定文言で
/// 上書きしても 1 秒でエラー文言へ戻る=入力を直すべき理由が画面から消えない
static SAVE_ERROR_NOTE: Mutex<Option<(SaveErrorSource, String)>> = Mutex::new(None);
/// 接続ページの「このMacのアドレス」行(複数 NIC は列挙。10 秒キャッシュで更新)
static OWN_IP_LABEL: AtomicUsize = AtomicUsize::new(0);
/// OWN_IP_LABEL へ最後に set した文字列。own_ip_text が 10 秒キャッシュのため、
/// 値が変わったとき(=キャッシュ更新時)だけ set するための照合用
static OWN_IP_LAST: Mutex<String> = Mutex::new(String::new());
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

/// 操作ページの注記行の本文(固定されているキーが無いときは空=非表示)。
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

/// 操作ページの注記行のツールチップ(1 行に収まらない対処案の完全版)。本文が
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

/// サイドバー下部の「その他」欄のボタン。ページ選択ボタンと同じ形(縁なし・左寄せ・
/// x=16 幅 140 高さ 36)で、文字だけ 11pt に縮める: 最長の「すべての設定を初期化…」
/// (11 字)が 13pt のままだと約 143px で幅 140 を超えるため(11pt なら約 121px)
unsafe fn sidebar_button(
    parent: ID,
    target: ID,
    title: &str,
    action: &std::ffi::CStr,
    y: f64,
) -> ID {
    let b = button(
        parent,
        target,
        title,
        action,
        NSRect {
            x: 16.0,
            y,
            w: 140.0,
            h: 36.0,
        },
    );
    msg1_void_u8(b, sel(c"setBordered:"), 0);
    msg1_void_i64(b, sel(c"setAlignment:"), 0);
    let font = msg1_id_f64(
        objc_getClass(c"NSFont".as_ptr()),
        sel(c"systemFontOfSize:"),
        11.0,
    );
    msg1_void_id(b, sel(c"setFont:"), font);
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
    msg1_void_id(v, sel(c"setPlaceholderString:"), nsstring(placeholder));
    msg1_void_id(v, sel(c"setStringValue:"), nsstring(value));
    let font = msg1_id_f64(
        objc_getClass(c"NSFont".as_ptr()),
        sel(c"systemFontOfSize:"),
        13.0,
    );
    msg1_void_id(v, sel(c"setFont:"), font);
    msg1_void_id(v, sel(c"setAccessibilityLabel:"), nsstring(accessibility));
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
    check_at(parent, target, title, action, slot, 40.0, 460.0, 532.0, y)
}

/// check() の caption 位置・幅指定版。ページ内の詳細記録(スイッチを行の右端=固定
/// 位置へ置く)と、サイドバー下部の「その他」(スイッチをサイドバー幅へ収める)の
/// 両方で使うため、スイッチの x 位置も指定できる
unsafe fn check_at(
    parent: ID,
    target: ID,
    title: &str,
    action: &std::ffi::CStr,
    slot: &AtomicUsize,
    caption_x: f64,
    caption_w: f64,
    switch_x: f64,
    y: f64,
) -> ID {
    let caption = label(parent, title, caption_x, y + 1.0, caption_w, 13.0, false);
    let b = msg0(objc_getClass(c"NSSwitch".as_ptr()), sel(c"new"));
    frame(
        b,
        NSRect {
            x: switch_x,
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
    focus_initial_responder(index.min(PAGES.len()-1));
}

/// ページの先頭コントロール(キーボード操作の始点)。接続ページは接続先欄が
/// 有効な役割(この Mac が接続しに行く側)のとき HOST_FIELD、待ち受け側の間は
/// 役割ラジオの先頭(無効な欄へフォーカスしても入力できないため)。画面配置は
/// 配置キャンバス、操作は切替方式、共有はテキストと画像のスイッチを当てる
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
        2 => SWITCH_POP.load(Ordering::Relaxed) as ID,
        _ => super::PREFS_CHK_CLIP.load(Ordering::Relaxed) as ID,
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

/// 「端で少し待つ」で最後に選んだ滞在時間(ms)。SWITCH_DELAY_MS は方式の切替で
/// 他の方式へ移ると 0 に戻るため、戻ってきたときに同じ値へ復元するための記憶
/// (remember_dwell が非ゼロ値だけを書く。0 は「滞在を使わない」のため記憶しない)
static LAST_DWELL_MS: AtomicU64 = AtomicU64::new(0);

/// 滞在時間の非ゼロ値を記憶する。スライダの switch_delay と起動時・遠隔適用の
/// preferences 復元が呼ぶ(復元経由で値が入ったときも方式の行き来で失わないように)
pub(super) fn remember_dwell(ms: u64) {
    if ms > 0 {
        LAST_DWELL_MS.store(ms, Ordering::Relaxed);
    }
}

/// 「端で少し待つ」を選んだ時の滞在時間(ms)。設定済みならその値を保つ。
/// 未設定(0)のときは直近に選んだ値(LAST_DWELL_MS)、一度も選んだことがなければ
/// 既定 300ms(スライダで調整した値を方式の行き来で失わない)。他の方式は滞在を
/// 使わないため 0 に戻す(method_index の射影と整合)
fn delay_for_method(i: isize, current_ms: u64) -> u64 {
    if i == 1 {
        if current_ms > 0 {
            current_ms
        } else {
            let last = LAST_DWELL_MS.load(Ordering::Relaxed);
            if last > 0 { last } else { 300 }
        }
    } else {
        0
    }
}

/// 滞在時間スライダの値表示。「端で少し待つ」の既定 300ms(delay_for_method が
/// 方式の切替時に戻す値と同じ)のときだけ目印を出す
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
    // 調整した値を記憶する(方式の行き来で 0 に戻った後、戻ってきたときの復元値)
    remember_dwell(ms);
    set_label(&SWITCH_DELAY_LABEL, &delay_text(ms));
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
/// 履歴サブメニュー(再構築の保留制御)もメニューバー本体(状態行の書き換え保留)も
/// 切替方式・切替キー・接続先ピッカーもこの IMP を共有する。sender(menu)で区別する
pub(super) unsafe extern "C" fn peer_menu_open(_s: ID, _c: SEL, menu: ID) {
    if super::history_menu_tracking(menu) {
        super::history_menu_opened();
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
    if super::main_menu_tracking(menu) {
        super::main_menu_closed();
        return;
    }
    set_menu_open(menu, false);
}

/// ポップアップ系メニューの開示中フラグの切替(sender=NSMenu でどのポップアップかを
/// 区別)。開いている間は sync() の selectItemAtIndex:/setTitle: を控えさせる
/// (menuDidClose: で false へ戻り、次の sync() が最新値を反映する)
unsafe fn set_menu_open(menu: ID, open: bool) {
    if !menu.is_null() && menu == SWITCH_MENU.load(Ordering::Relaxed) as ID {
        SWITCH_MENU_OPEN.store(open, Ordering::Relaxed);
    } else if !menu.is_null() && menu == HOTKEY_MENU.load(Ordering::Relaxed) as ID {
        HOTKEY_MENU_OPEN.store(open, Ordering::Relaxed);
    } else {
        PEER_MENU_OPEN.store(open, Ordering::Relaxed);
    }
}

unsafe fn set_label(slot: &AtomicUsize, text: &str) {
    let label = slot.load(Ordering::Relaxed) as ID;
    if !label.is_null() {
        msg1_void_id(label, sel(c"setStringValue:"), nsstring(text));
        msg1_void_id(label, sel(c"setToolTip:"), nsstring(text));
    }
}

/// 暗号化の行の本文(Mac 側の短縮版・純粋関数・単体テストで守る)。「設定不要」の
/// 案内に 1 行を使い切らないよう、鍵の略号と照合手順は同ラベルのツールチップ
/// (ACCEPT_LABEL と同じ手法)へ出す。ツールチップの文言は Win 側と同じ
/// secure::encryption_line(略号付きの完全版)をそのまま使い、二重管理を避ける
fn secure_line_short() -> &'static str {
    "通信: 常時暗号化・相互認証(設定不要)"
}

/// 受け入れ範囲の行(ACCEPT_LABEL)の文言(純粋関数・単体テストで守る)。
/// KNIT_ALLOW_ANY=1 で緩んでいるときは警告として読める一文を添えるが、行の幅
/// (540px・1 行)に収まる長さで止める(「信頼できるネットワークでのみ使って
/// ください」はツールチップへ移動した)。緩和は暗号化を外すものではないため、
/// 警告文でも「暗号化は効いている」ことが読み取れるようにする(外れるのは
/// 接続元を絞る保護の方)
fn accept_scope_text(scope: knit_common::net::AcceptScope) -> String {
    if scope.is_wide_open() {
        format!(
            "受け入れ範囲: {}。暗号化は効きますが、接続元を絞る保護が外れます",
            scope.label()
        )
    } else {
        format!("受け入れ範囲: {}", scope.label())
    }
}

/// 受け入れ範囲の行に載せるツールチップ(純粋関数・単体テストで守る)。1 行に
/// 収まらなかった注意(信頼できるネットワークでのみ使うこと)をここへ出す
fn accept_scope_tooltip(scope: knit_common::net::AcceptScope) -> String {
    if scope.is_wide_open() {
        format!(
            "{}\n信頼できるネットワークでのみ使ってください",
            accept_scope_text(scope)
        )
    } else {
        accept_scope_text(scope)
    }
}

/// 接続中のスピーカーミュートのキャプション(純粋関数・単体テストで守る)。
/// 「(音声転送オフの間は無効)」の括弧は音声転送がオフの間だけ付ける(オンの
/// 間は状態と無関係な注記を常設しない)。例外の完全版はツールチップが常時担う
fn speaker_caption(android: bool, audio_on: bool) -> &'static str {
    if android {
        "相手のスピーカーをミュート(PCのみ)"
    } else if audio_on {
        "接続中は相手のスピーカーをミュート"
    } else {
        "接続中は相手のスピーカーをミュート(音声転送オフの間は無効)"
    }
}

/// スピーカーミュートのツールチップ(純粋関数・単体テストで守る)。キャプションで
/// 省いた例外の完全版(相手が音声転送をオフにしている間は適用されない)をここへ出す
fn speaker_tooltip(android: bool) -> &'static str {
    if android {
        "相手のスピーカーをミュート(PCのみ)"
    } else {
        "接続中は相手のスピーカーをミュート(相手が音声転送をオフの間は適用されません)"
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

/// 「このMacの名前」欄の placeholder(純粋関数・単体テストで守る)。空欄のとき
/// 採用されるホスト名を括弧内へ出すが、ホスト名は長さが決まらないため 24 文字
/// で切り詰める(入力欄は幅 336px・13pt。長い名前で「…」も見えず切れてしまうと
/// 空欄で何が入るか読めなくなるため、必ず「…」まで見える長さで止める)
fn own_name_placeholder(hostname: &str) -> String {
    format!(
        "空欄でホスト名({})",
        knit_common::history::truncate_chars(hostname, 24)
    )
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

/// 役割カードの注記行の文言(ROLE_NOTE_KIND の状態から)。通常は再起動の案内、
/// KNIT_ROLE 固定中・切替の進行中はそれぞれの状況を示す
fn role_note_text() -> String {
    match ROLE_NOTE_KIND.load(Ordering::Relaxed) {
        1 => "環境変数 KNIT_ROLE で役割を固定中のため、ここでは切り替えられません".into(),
        2 => "切り替えを相手へ送りました。相手の適用を確認しています…".into(),
        3 => "相手の適用を確認しました。再起動します…".into(),
        4 => "相手の適用確認が取れないため、時間経過で再起動します…".into(),
        _ => "切り替えると、両方の端末でKnitが自動で再起動します".into(),
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

/// 登録済み台数の表示値(純粋関数・単体テストで守る)。接続中の PEERS と
/// peer-sides.json に保存済みの端末数の max を取る: PEERS は切断中に空に
/// なるため、保存だけが残る間も台数を出し続ける(「未接続(自動で再接続します)」
/// と並んで「登録が消えた?」と誤読させるのを防ぐ)
fn registration_count(peers_live: usize, saved: usize) -> usize {
    peers_live.max(saved)
}

/// 状態行が「自動で再接続します」を出して良いか(純粋関数・単体テストで守る)。
/// PAIRED は接続キーの保存だけでも立つ(招待を発行したまま相手の登録がまだの
/// 状態で再起動した時など)。その間の「自動で再接続します(相手側アプリの起動を
/// 確認)」は永久に繋がらない嘘になるため、registration_count と同じ素材
/// (接続中 PEERS と peer-sides.json の保存件数)で登録の実体が 1件でもある
/// ときだけ true にする。メニュー(gui.rs update_state_item)と設定画面(sync)
/// の状態行が共用する
pub(super) fn paired_registered(paired: bool, peers_live: usize, saved: usize) -> bool {
    paired && (peers_live > 0 || saved > 0)
}

/// 「登録の管理」行の文言(登録済み台数の常時表示)。台数の候補
/// (registration_count)から組み立てる純粋関数(単体テストで守る)。
/// 2台以上でしか「操作する端末」が出ないため、1対1のときに台数が見えない
/// 問題への対策として常時出す。手動接続(KNIT_TOKEN)の間は初期化が効かない
/// ため「(手動接続中)」を添える(この行は幅 336px・13pt しかなく、旧来の長文
/// 「手動接続中のため初期化できません」は行末が切れて否定だけ消える恐れが
/// あった。「初期化できない」理由と戻し方は、初期化ボタンを押したときの
/// setup の reset_registration が同じ条件で出す案内が担う)
fn registration_text(peers: usize, manual_token: bool) -> String {
    if manual_token {
        format!("登録の管理・登録済み: {peers}台(手動接続中)")
    } else {
        format!("登録の管理・登録済み: {peers}台")
    }
}

/// 「直接つなぐ(手動接続)」の説明(1 行)。未設定の間は上級者向けであることを先に
/// 伝える(「両側へ設定」の語が手順を伝え、詳細は setup の KNIT_TOKEN 案内が担う)、
/// 設定中は通常の登録へ戻す方法に切り替わる。手動接続中である旨は下のカードの
/// 登録の管理行(registration_text)が「(手動接続中)」で常に併記するため、ここでは
/// 出さない(「初期化が効かない」理由は reset_registration の案内が担う)
fn token_note_lines(manual: bool) -> &'static str {
    if manual {
        "トークンを空欄にして保存すると、通常の登録に戻ります"
    } else {
        "(上級者向け)同じトークンを両側へ設定すると、登録なしで直接つなげます"
    }
}

/// 直接つなぐの節の開閉ラベルの題名(純粋関数・単体テストで守る)。未設定の間は
/// 「上級者向け」を先に読ませて ▸(閉)/▾(開)で展開状態を示す。設定済み
/// (manual)は常に展開のため開閉の案内は外し、見出しとしてだけ機能させる
fn token_toggle_title(manual: bool, open: bool) -> &'static str {
    if manual {
        "直接つなぐ(手動接続)"
    } else if open {
        "直接つなぐ(手動接続)・上級者向け ▾"
    } else {
        "直接つなぐ(手動接続)・上級者向け ▸"
    }
}

/// 直接つなぐの節の表示切り替え(開閉ラベルのクリックと毎秒の sync() から呼ぶ)。
/// トークン未設定の間は TOKEN_SECTION_OPEN(プロセス共有=開き直しても維持)に
/// 従い、設定済み(手動接続中)は常に展開する。トークン由来の保存エラーが残る間も
/// 展開する(注記行 TOKEN_NOTE_L1 はこの節の中のため、畳んだままだと「次の保存
/// 成功まで残るエラー」が隠れてしまう=エラーの居場所の保護を畳み込みでも守る)。
/// 開閉ラベルの説明(使い方の1行)はツールチップへ移設しているため、展開後の
/// 注記行(TOKEN_NOTE_L1)だけが本文を担う
pub(super) unsafe fn sync_token_section() {
    let manual = knit_common::envutil::get("KNIT_TOKEN").is_some_and(|t| !t.is_empty());
    let token_error = matches!(
        SAVE_ERROR_NOTE.lock().unwrap_or_else(|e| e.into_inner()).as_ref(),
        Some((SaveErrorSource::Token, _))
    );
    let open = manual || token_error || TOKEN_SECTION_OPEN.load(Ordering::Relaxed);
    for v in TOKEN_VIEWS.lock().unwrap_or_else(|e| e.into_inner()).iter() {
        let v = *v as ID;
        if !v.is_null() { msg1_void_u8(v, sel(c"setHidden:"), !open as u8); }
    }
    let toggle = TOKEN_TOGGLE.load(Ordering::Relaxed) as ID;
    if !toggle.is_null() {
        msg1_void_id(
            toggle,
            sel(c"setTitle:"),
            nsstring(token_toggle_title(manual, open)),
        );
    }
}

/// 開閉ラベルのクリック(トークン未設定の間だけトグル。設定済みは常に展開のため
/// 何も変わらない。クリック直後に即座に画面へ反映する=次の sync() を待たない)
pub(super) unsafe fn toggle_token_section() {
    let manual = knit_common::envutil::get("KNIT_TOKEN").is_some_and(|t| !t.is_empty());
    if !manual {
        TOKEN_SECTION_OPEN.fetch_xor(true, Ordering::Relaxed);
    }
    sync_token_section();
}

/// 直接つなぐの説明 1 行と保存エラーの組立(純粋関数・単体テストで守る)。
/// 保存系ハンドラが弾いた検証/保存エラーがある間はエラー文言で置き換える
/// (次の保存成功まで保持するため、直すべき理由が画面から消えない)
fn token_note_with_error(manual: bool, error: Option<&str>) -> String {
    match error {
        Some(err) => err.to_string(),
        None => token_note_lines(manual).to_string(),
    }
}

/// 保存エラーを出元の欄の隣の注記行へ出す(検証エラー・保存失敗の共通導線)。
/// 通知センターの表示は数秒で消えるため、設定ウィンドウを開いているときの
/// 一次情報はこちらが担う(通知との併用も可)。出元(source)で出す行が決まる:
/// 接続先の失敗がトークン欄の下に出るのを防ぐ
pub(super) fn set_save_error(source: SaveErrorSource, note: String) {
    *SAVE_ERROR_NOTE.lock().unwrap_or_else(|e| e.into_inner()) = Some((source, note));
}

/// 保存エラーを出す欄の振り分け(純粋関数・単体テストで守る)。出元が一致する
/// ときだけその欄の注記行を差し替える文言を返す(他方の欄は既定文言のまま)
fn save_error_text<'a>(
    source: SaveErrorSource,
    error: Option<&'a (SaveErrorSource, String)>,
) -> Option<&'a str> {
    match error {
        Some((s, text)) if *s == source => Some(text.as_str()),
        _ => None,
    }
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
    *note = cleared_save_error(source, note.clone());
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

/// 接続ページの状況行(CONNECTION_HINT)の文言(純粋関数・単体テストで守る)。
/// 未接続の登録への導線は状態行(connection_state_text の「端末を登録…」)が、
/// 接続済み Android アプリの許可案内は Android 行(android_state_text)がそれぞ
/// れ担うため、ここでは出さない(1 つの用件を 2 行で言わない)。手動接続運用中で
/// 繋がらないときは、原因として相手側のトークン不一致が読み取れる文言へ
/// 切り替える(何も示さないと「待ち続ければそのうち繋がる」と誤読させる)。
/// peer は動的な端末名(エイリアス最大 48 文字)のため 12 文字で切り詰めてから
/// 差し込む(この行は幅 540px・1 行のため、名前が余白を食い潰さないように)
fn connection_hint_text(
    preview: bool,
    connected: bool,
    manual: bool,
    android_perm_pending: bool,
    peer: &str,
) -> String {
    if preview {
        "入力・通信・音声は動作せず、設定も保存しません。".into()
    } else if manual && !connected {
        // 待機中である旨は状態行(未接続(直接つなぐで待機中))が担うため、ここは
        // 繋がらないときの確認事由(相手側のトークン不一致)だけを 1 行で伝える
        "繋がらないときは、相手側にも同じトークンが設定されているか確認してください".into()
    } else if !connected {
        // 「初めてなら端末を登録…から」の導線は状態行(未登録のときだけ出る)が
        // 担うため、ここは相手側でアプリを開く待ちだけを伝える
        "接続する端末でKnitを開いてください。".into()
    } else if android_perm_pending {
        // Androidアプリの許可案内は Android 行(ANDROID_STATE)が担うため空文字
        String::new()
    } else {
        format!(
            "画面の端や切替キーで{}の画面へ移れます(方法は「操作」で選べます)",
            knit_common::history::truncate_chars(peer, 12)
        )
    }
}

/// 接続ページの Android 行(ANDROID_STATE)の文言(純粋関数・単体テストで守る)。
/// android_app は接続済み端末が Android アプリ版のとき: 権限(画面操作・キー
/// ボード)の状態に応じて案内が変わる(許可待ち→許可を促す、画面操作のみ許可→
/// キーボードの選択を促す、両方許可→許可済み)。状況行(CONNECTION_HINT)から
/// 許可案内を外したため、この行が許可前後を通じて唯一の案内を担う。
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
    fn registration_text_shows_count_always() {
        assert_eq!(registration_text(0, false), "登録の管理・登録済み: 0台");
        assert_eq!(registration_text(1, false), "登録の管理・登録済み: 1台");
        assert_eq!(registration_text(3, false), "登録の管理・登録済み: 3台");
        // 手動接続(KNIT_TOKEN)の間は初期化が効かないため「(手動接続中)」を添える。
        // 「初期化できません」の詳細は初期化ボタンを押したときの reset_registration
        // の案内が担う(この行の幅 336px に長文は収まらないため)
        assert_eq!(
            registration_text(1, true),
            "登録の管理・登録済み: 1台(手動接続中)"
        );
    }

    /// 登録の管理行の幅(13pt・ラベル幅 336px): 手動接続中の注記つきでも 3桁の
    /// 台数まで 1 行に収まる(旧来の長文は 385px 相当で末尾が切れ、否定だけが
    /// 消えて意味が反転し得たため、文言側を 336px 内へ止める)
    #[test]
    fn registration_text_fits_336px_label() {
        let px13 = |s: &str| {
            s.chars()
                .map(|c| if c.is_ascii() { 7 } else { 13 })
                .sum::<usize>()
        };
        for peers in [0, 1, 12, 999] {
            for manual in [false, true] {
                let text = registration_text(peers, manual);
                assert!(
                    px13(&text) <= 336,
                    "登録の管理行(336px・13pt)に収まる: {text} ({}px)",
                    px13(&text)
                );
            }
        }
    }

    /// 直接つなぐの説明: 未設定時は上級者向けの案内、設定中は戻し方だけの 1 行。
    /// 「手動接続中」「初期化が効かない」の旨は登録の管理行(registration_text)が
    /// 常に併記するためここには載せない(旧来の「手動接続(共通トークン)で運用中・」
    /// は管理行と重複していた)。出す行は 1 行に収まる長さであることも守る
    #[test]
    fn token_note_lines_switch_by_manual_mode() {
        let off = token_note_lines(false);
        assert!(off.contains("上級者向け"), "上級者向けであることを先に伝える: {off}");
        assert!(off.contains("登録なし"), "登録が要らないことが読み取れる: {off}");
        assert!(off.contains("直接つなげます"), "直接つなぐ導線であることも出す: {off}");
        let on = token_note_lines(true);
        assert!(
            !on.contains("手動接続"),
            "手動接続中である旨は登録の管理行が担う(ここでは出さない): {on}"
        );
        assert!(on.contains("通常の登録に戻ります"), "戻し方が出る: {on}");
        assert!(!on.contains("初期化"), "初期化の注記は reset_registration の案内が担う: {on}");
        assert!(
            registration_text(1, true).contains("手動接続中"),
            "設定中は登録の管理行に手動接続中である旨が常時出る(初期化できない理由は初期化ボタンの案内が担う)"
        );
        // 11pt で幅 516px に収まる目安(全角 45 文字相当以下)
        for line in [off, on] {
            let width = line
                .chars()
                .map(|c| if c.is_ascii() { 6 } else { 11 })
                .sum::<usize>();
            assert!(width <= 516, "1 行に収まる: {line} ({width}px)");
        }
    }

    /// 直接つなぐの節の開閉ラベル: 未設定の間は「上級者向け」+開閉記号(▸閉/▾開)で
    /// 節全体を 1 行へ畳めることを読み取らせ、設定済み(手動接続中)は常に展開の
    /// ため開閉の案内を外して見出しとしてだけ機能する。13pt で幅 516px に収まる
    /// ことも守る(見出しの行幅と同じ)
    #[test]
    fn token_toggle_title_signals_advanced_and_open_state() {
        let closed = token_toggle_title(false, false);
        assert!(closed.contains("直接つなぐ(手動接続)"), "節の名前は setup の初期化ガイドと同じ: {closed}");
        assert!(closed.contains("上級者向け"), "畳んでいる間は上級者向けであることを先に伝える: {closed}");
        assert!(closed.contains("▸"), "閉じている記号: {closed}");
        let opened = token_toggle_title(false, true);
        assert!(opened.contains("上級者向け"), "開いても未設定の間は上級者向けが残る: {opened}");
        assert!(opened.contains("▾"), "開いている記号: {opened}");
        let manual = token_toggle_title(true, false);
        assert!(manual.starts_with("直接つなぐ(手動接続)"), "設定済みは節名だけの見出し: {manual}");
        assert!(!manual.contains("上級者向け"), "設定済みに上級者向けの案内は要らない: {manual}");
        assert!(!manual.contains("▸"), "設定済みは常に展開のため閉じ記号を出さない: {manual}");
        // 開閉ラベルの説明(使い方)はツールチップへ移設しているため、ラベルの行から
        // 使い方が消えても token_note_lines(false) が同じ内容を担い続ける
        assert!(token_note_lines(false).contains("直接つなげます"));
        let width = |s: &str| {
            s.chars()
                .map(|c| if c.is_ascii() { 7 } else { 13 })
                .sum::<usize>()
        };
        for title in [closed, opened, manual] {
            assert!(width(title) <= 516, "見出しの行(516px・13pt)に収まる: {title} ({}px)", width(title));
        }
    }

    /// 暗号化の行の本文(Mac 側の短縮版): 「設定不要」の案内に 1 行を使い切らない
    /// よう鍵の略号と照合手順は本文から外し、ツールチップが encryption_line の
    /// 完全版(Win 側と同じ文言)で担うことを守る
    #[test]
    fn secure_line_short_keeps_details_in_tooltip() {
        let line = secure_line_short();
        assert!(line.contains("常時暗号化"), "常時暗号化であることが読み取れる: {line}");
        assert!(line.contains("設定不要"), "設定が存在しない案内が残る: {line}");
        assert!(!line.contains("略号"), "鍵の略号は本文からは外す: {line}");
        // 11pt で幅 540px の 1 行に十分収まる(旧来の完全版は 67 文字=737px 相当で
        // 「設定不要」の案内だけで行を使い切っていた)
        let width = line
            .chars()
            .map(|c| if c.is_ascii() { 6 } else { 11 })
            .sum::<usize>();
        assert!(width <= 540, "1 行に収まる: {line} ({width}px)");
        // ツールチップは共通の encryption_line(略号と照合手順の完全版)と同じ文言
        let fp = knit_common::secure::fingerprint(&"7f".repeat(32));
        let full = knit_common::secure::encryption_line(Some(&fp));
        assert!(full.contains(&fp), "ツールチップ側に鍵の略号が載る: {full}");
        assert!(
            full.contains("対になっています"),
            "照合手順もツールチップ側へ退避する: {full}"
        );
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

    /// 状況行: 手動接続運用中で繋がらないとき、相手側のトークン不一致の確認を促す。
    /// 待機中である旨は状態行(connection_state_text の「直接つなぐで待機中」)が
    /// 担うため状況行からは外す(旧来の「直接つなぐで待機中です。」は状態行と
    /// 完全重複していた)。11pt・幅 540px に収まることも守る
    #[test]
    fn connection_hint_text_manual_token_mismatch_hint() {
        let hint = connection_hint_text(false, false, true, false, "端末");
        assert!(
            !hint.contains("待機中"),
            "待機状態は状態行が担う(ここでは出さない): {hint}"
        );
        assert!(
            hint.contains("相手側にも同じトークン"),
            "トークン不一致の確認を促す: {hint}"
        );
        // 通常の未接続(トークン未運用)は相手側でアプリを開く待ちだけを伝える。
        // 登録への導線(「端末を登録…」)は状態行(connection_state_text)が未登録の
        // ときだけ出すため、状況行では二重に言わない
        let normal = connection_hint_text(false, false, false, false, "端末");
        assert!(normal.contains("接続する端末でKnitを開いてください"), "{normal}");
        assert!(!normal.contains("端末を登録"), "登録導線は状態行が担う: {normal}");
        // 接続済み・Android 許可待ちの間は状況行は空(許可案内は Android 行
        // android_state_text が担う)。peer 名の分岐は従来どおり
        assert_eq!(
            connection_hint_text(false, true, true, true, "端末"),
            "",
            "Android 許可案内は Android 行が担う(状況行は空)"
        );
        assert!(
            connection_hint_text(false, true, false, false, "Windows")
                .contains("Windowsの画面へ移れます")
        );
        let width = hint
            .chars()
            .map(|c| if c.is_ascii() { 6 } else { 11 })
            .sum::<usize>();
        assert!(width <= 540, "状況行(11pt)に収まる: {hint} ({width}px)");
    }

    /// 状況行(接続済み): 動的な peer 名(エイリアス最大 48 文字)は 12 文字+
    /// 「…」へ切り詰められ、どの名前でも 1 行(幅 540px・11pt)に収まる
    #[test]
    fn connection_hint_text_truncates_long_peer_names() {
        let px = |s: &str| s.chars().map(|c| if c.is_ascii() { 6 } else { 11 }).sum::<usize>();
        let longest = "太".repeat(48); // エイリアスの上限(safe_peer_name と同じ)
        let hint = connection_hint_text(false, true, false, false, &longest);
        assert!(
            hint.contains(&format!("{}…の画面へ移れます", "太".repeat(12))),
            "peer は 12 文字+「…」で差し込む: {hint}"
        );
        assert!(px(&hint) <= 540, "1 行に収まる: {hint} ({}px)", px(&hint));
        // 短い名前は従来どおりそのまま(切らない)
        assert!(connection_hint_text(false, true, false, false, "ReoのMac").contains("ReoのMacの画面"));
    }

    /// Android 行(ANDROID_STATE・幅 540px・11pt): Androidアプリ接続の権限状態に
    /// 応じて案内が切り替わる(許可待ち→許可を促す、画面操作のみ許可→キーボード
    /// の選択を促す、両方許可→許可済み)。状況行(CONNECTION_HINT)から許可案内を
    /// 外した分、許可前後の案内はこの行が唯一担う。adb 接続では全端末共通の問題を
    /// 優先し、無ければ先頭タブレットの状態(name・summary・台数。summary は
    /// Phase::summary() の String をそのまま渡す)。どちらの材料も無いときは
    /// 空文字=行が見えない(ANDROID_STATE は状況があった時だけ出る行)
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

    /// 受け入れ範囲の行: どの範囲(LAN/Tailscale/ANY)でも 1 行(幅 540px・11pt)に
    /// 収まる。ANY の警告は結論まで読める長さで、はみ出す注意はツールチップへ移る
    #[test]
    fn accept_scope_text_fits_one_line_and_moves_caution_to_tooltip() {
        use knit_common::net::AcceptScope;
        let px = |s: &str| s.chars().map(|c| if c.is_ascii() { 6 } else { 11 }).sum::<usize>();
        for scope in [AcceptScope::Lan, AcceptScope::Tailscale, AcceptScope::Any] {
            let line = accept_scope_text(scope);
            assert!(px(&line) <= 540, "1 行に収まる: {line} ({}px)", px(&line));
        }
        // ANY: 行の警告は「保護が外れる」まで読める(旧文言は 540px の 1.5 倍で
        // 結論が切れていた)。「信頼できるネットワーク」は行からは消え、ツールチップへ
        let any = accept_scope_text(AcceptScope::Any);
        assert!(any.contains("すべてのアドレス(KNIT_ALLOW_ANY=1)"), "{any}");
        assert!(any.contains("暗号化は効きますが、接続元を絞る保護が外れます"), "{any}");
        assert!(!any.contains("信頼できるネットワーク"), "注意は行に出さない: {any}");
        let tip = accept_scope_tooltip(AcceptScope::Any);
        assert!(tip.contains("信頼できるネットワークでのみ使ってください"), "{tip}");
        // 通常時のツールチップは行と同じ(余計な注意を出さない)
        assert_eq!(accept_scope_tooltip(AcceptScope::Lan), accept_scope_text(AcceptScope::Lan));
    }

    /// スピーカーミュートのキャプション: どの状態でも check の行(幅 460px・13pt)に
    /// 収まる(旧来の長文は 469px 相当で「適用されません」の否定が切れて意味が
    /// 反転し得た)。括弧(音声転送オフの間は無効)は音声転送がオフの間だけ付き、
    /// オンの間は出さない。例外の完全版はツールチップが常時担う
    #[test]
    fn speaker_caption_fits_460px_and_shows_exception_only_while_audio_off() {
        let px13 = |s: &str| {
            s.chars()
                .map(|c| if c.is_ascii() { 7 } else { 13 })
                .sum::<usize>()
        };
        for android in [false, true] {
            for audio_on in [false, true] {
                let cap = speaker_caption(android, audio_on);
                assert!(
                    px13(cap) <= 460,
                    "キャプション(460px・13pt)に収まる: {cap} ({}px)",
                    px13(cap)
                );
                assert!(cap.contains("スピーカーをミュート"), "何をするスイッチか読み取れる: {cap}");
            }
        }
        // 括弧(例外の短縮形)は音声転送オフの間だけ。オンの間は状態と無関係な
        // 注記を常設しない
        assert!(
            !speaker_caption(false, true).contains("音声転送オフ"),
            "音声転送オンでは括弧を出さない: {}",
            speaker_caption(false, true)
        );
        assert!(speaker_caption(false, false).contains("音声転送オフの間は無効"), "例外の短縮形も出す");
        for audio_on in [false, true] {
            assert!(speaker_caption(true, audio_on).contains("PCのみ"), "Android 相手には PC のみ: {}", speaker_caption(true, audio_on));
        }
        // ツールチップは状態によらず例外の完全版(主語が「相手」であることも読み取れる)
        let tip = speaker_tooltip(false);
        assert!(tip.contains("相手が音声転送をオフの間は適用されません"), "完全版を担う: {tip}");
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

    /// 「このMacの名前」の placeholder: ホスト名は 24 文字+「…」へ切り詰め、
    /// 入力欄(幅 336px・13pt)に「…」まで必ず見える(切り詰めなしでは長い名前で
    /// 末尾が切れ、空欄で何が採用されるか読めなくなる)
    #[test]
    fn own_name_placeholder_truncates_long_hostnames() {
        let px13 = |s: &str| {
            s.chars()
                .map(|c| if c.is_ascii() { 7 } else { 13 })
                .sum::<usize>()
        };
        // ASCII の長いホスト名(60 文字)でも 336px 内
        let ph = own_name_placeholder(&"m".repeat(60));
        assert!(
            ph.contains(&format!("空欄でホスト名({}…)", "m".repeat(24))),
            "24 文字+「…」へ切る: {ph}"
        );
        assert!(px13(&ph) <= 336, "入力欄に収まる: {ph} ({}px)", px13(&ph));
        // 短いホスト名はそのまま
        assert_eq!(own_name_placeholder("ReoのMac"), "空欄でホスト名(ReoのMac)");
    }

    /// 保存系の検証/保存エラーは TOKEN_NOTE の注記行を差し替える。
    /// エラーが無いときは既定の 1 行(使い方/運用の注記)へ戻る
    #[test]
    fn token_note_with_error_overrides_until_success() {
        let err = token_note_with_error(
            false,
            Some("トークンが短すぎます。32 文字以上の半角英数字を入れてください"),
        );
        assert_eq!(
            err,
            "トークンが短すぎます。32 文字以上の半角英数字を入れてください"
        );
        // エラー解除後は manual の状態に応じた既定文言へ戻る
        assert_eq!(token_note_with_error(false, None), token_note_lines(false));
        assert_eq!(token_note_with_error(true, None), token_note_lines(true));
    }

    /// 操作ページの注記: 固定キーが無いときは空(=行が見えない)、あるときは
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

    /// 登録済み台数の表示値: 切断中(PEERS 空)でも保存済み台数が出る。
    /// 接続中はライブ件数が保存より多くてもそのまま出る
    #[test]
    fn registration_count_uses_saved_floor_when_disconnected() {
        assert_eq!(registration_count(0, 0), 0);
        assert_eq!(registration_count(0, 2), 2, "切断中は保存済み台数が出る");
        assert_eq!(registration_count(1, 3), 3, "保存より多い接続中は接続数");
        assert_eq!(registration_count(4, 1), 4, "接続が保存を上回れば接続数");
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

    /// 保存エラーの振り分け: 出元と一致する欄だけ差し替え文言を返す。
    /// 接続先(Host)の失敗がトークン欄に出る・その逆を起こさない
    #[test]
    fn save_error_text_routes_to_matching_source_only() {
        let host_err = (SaveErrorSource::Host, "接続先を保存できませんでした".to_string());
        assert_eq!(
            save_error_text(SaveErrorSource::Host, Some(&host_err)),
            Some("接続先を保存できませんでした")
        );
        assert_eq!(
            save_error_text(SaveErrorSource::Token, Some(&host_err)),
            None,
            "Host 由来はトークン欄に出さない"
        );
        let token_err = (SaveErrorSource::Token, "トークンが短すぎます".to_string());
        assert_eq!(
            save_error_text(SaveErrorSource::Token, Some(&token_err)),
            Some("トークンが短すぎます")
        );
        assert_eq!(
            save_error_text(SaveErrorSource::Host, Some(&token_err)),
            None,
            "Token 由来は接続先欄に出さない"
        );
        assert_eq!(save_error_text(SaveErrorSource::Host, None), None);
        assert_eq!(save_error_text(SaveErrorSource::Token, None), None);
    }

    /// スクロール速度の値表示: 速め/標準/遅めの 3 分岐の閾値(40/140)が
    /// sync() とハンドラで共通の関数通りに出る。標準帯の既定値(60)には
    /// 「(既定)」の目印が付き、標準帯の他の値には付かない
    #[test]
    fn scroll_speed_text_three_steps() {
        assert_eq!(scroll_speed_text(20.0), "速め");
        assert_eq!(scroll_speed_text(40.0), "速め", "40 は速め側の境界");
        assert_eq!(scroll_speed_text(41.0), "標準");
        assert_eq!(scroll_speed_text(60.0), "標準(既定)", "60 は起動時の既定");
        assert_eq!(scroll_speed_text(61.0), "標準", "既定以外の標準帯に目印は付けない");
        assert_eq!(scroll_speed_text(100.0), "標準");
        assert_eq!(scroll_speed_text(139.0), "標準");
        assert_eq!(scroll_speed_text(140.0), "遅め", "140 は遅め側の境界");
        assert_eq!(scroll_speed_text(260.0), "遅め");
    }

    /// スクロール速度の値ラベルのツールチップ: 3 分岐の表示に内部値(divisor)を
    /// 併記する。中央帯(41〜139)は表示が同じ「標準」のまま動かないため、ツール
    /// チップの内部値で違いが読めることがこのテストの守り
    #[test]
    fn scroll_speed_tooltip_appends_internal_value() {
        assert_eq!(scroll_speed_tooltip(20.0), "速め(20)");
        assert_eq!(scroll_speed_tooltip(60.0), "標準(既定・60)");
        assert_eq!(scroll_speed_tooltip(41.0), "標準(41)", "中央帯の下端");
        assert_eq!(scroll_speed_tooltip(100.0), "標準(100)");
        assert_eq!(scroll_speed_tooltip(139.0), "標準(139)", "中央帯の上端");
        assert_eq!(scroll_speed_tooltip(140.0), "遅め(140)");
        // スライダは連続値なので、丸めた整数で出す(読みやすさを優先する)
        assert_eq!(scroll_speed_tooltip(100.4), "標準(100)");
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

    /// カーソル速度の値表示: 既定の 1.0(等倍)には「(既定)」の目印が付き、
    /// 他の値には付かない(gain_text・delay_text と同じ付け方)
    #[test]
    fn mouse_scale_text_marks_default_scale() {
        assert_eq!(mouse_scale_text(1.0), "等倍(既定)");
        assert_eq!(mouse_scale_text(0.2), "0.2倍", "スライダの最小値");
        assert_eq!(mouse_scale_text(0.5), "0.5倍");
        assert_eq!(mouse_scale_text(1.4), "1.4倍", "既定以外の目印は付けない");
        assert_eq!(mouse_scale_text(3.0), "3.0倍", "スライダの最大値");
    }

    /// エラー解除は出元ごと: 一致する出元のエラーだけが消え、他方の出元の
    /// 未解決エラーは残る(片方の保存成功が、もう片方の直すべき理由まで
    /// 画面から消すのを防ぐ)
    #[test]
    fn cleared_save_error_clears_only_matching_source() {
        let host_err = (SaveErrorSource::Host, "接続先を保存できませんでした".to_string());
        assert_eq!(cleared_save_error(SaveErrorSource::Host, Some(host_err.clone())), None);
        assert_eq!(
            cleared_save_error(SaveErrorSource::Token, Some(host_err.clone())),
            Some(host_err),
            "トークンの保存成功は Host のエラーを消さない"
        );
        let token_err = (SaveErrorSource::Token, "トークンが短すぎます".to_string());
        assert_eq!(cleared_save_error(SaveErrorSource::Token, Some(token_err.clone())), None);
        assert_eq!(
            cleared_save_error(SaveErrorSource::Host, Some(token_err.clone())),
            Some(token_err),
            "接続先の保存成功は Token のエラーを消さない"
        );
        assert_eq!(cleared_save_error(SaveErrorSource::Host, None), None);
        assert_eq!(cleared_save_error(SaveErrorSource::Token, None), None);
    }

    /// 名前系(このMacの名前・エイリアス)の保存エラーも出元限定 clear と組み合わせて
    /// 動く: どちらも欄の下に注記行を持たず保存状態行(SAVE_LABEL)へ出す導線のため、
    /// 一致する出元の保存成功でだけ消え、他方の出元(Host/Token/もう片方の名前)の
    /// 保存成功では消えない。名前欄の保存成功が接続先の未解決エラーまで消すと、
    /// 直すべき入力の理由が画面から消えてしまう
    #[test]
    fn cleared_save_error_keeps_name_sources_isolated() {
        let own_err = (
            SaveErrorSource::OwnName,
            "名前に使えない文字が含まれています。制御文字などを除いてください".to_string(),
        );
        assert_eq!(cleared_save_error(SaveErrorSource::OwnName, Some(own_err.clone())), None);
        assert_eq!(
            cleared_save_error(SaveErrorSource::Alias, Some(own_err.clone())),
            Some(own_err.clone()),
            "エイリアスの保存成功は このMacの名前 のエラーを消さない"
        );
        assert_eq!(
            cleared_save_error(SaveErrorSource::Host, Some(own_err.clone())),
            Some(own_err.clone()),
            "接続先の保存成功は このMacの名前 のエラーを消さない"
        );
        assert_eq!(
            cleared_save_error(SaveErrorSource::Token, Some(own_err)),
            Some((SaveErrorSource::OwnName, "名前に使えない文字が含まれています。制御文字などを除いてください".to_string())),
            "トークンの保存成功は このMacの名前 のエラーを消さない"
        );
        let alias_err = (
            SaveErrorSource::Alias,
            "接続中の端末がありません。つながってから名前を設定してください".to_string(),
        );
        assert_eq!(cleared_save_error(SaveErrorSource::Alias, Some(alias_err.clone())), None);
        assert_eq!(
            cleared_save_error(SaveErrorSource::OwnName, Some(alias_err.clone())),
            Some(alias_err.clone()),
            "このMacの名前の保存成功は エイリアス のエラーを消さない"
        );
        assert_eq!(
            cleared_save_error(SaveErrorSource::Token, Some(alias_err)),
            Some((SaveErrorSource::Alias, "接続中の端末がありません。つながってから名前を設定してください".to_string())),
            "トークンの保存成功は エイリアス のエラーを消さない"
        );
        assert_eq!(cleared_save_error(SaveErrorSource::OwnName, None), None);
        assert_eq!(cleared_save_error(SaveErrorSource::Alias, None), None);
    }

    /// エイリアス欄の sync 書き換え判定: 空でない端末 id へ変わった時だけ保存値へ
    /// 合わせる。id が空(切断)へ変わった時は欄をクリアしない=入力途中の文言を
    /// 毎秒の sync() が黙って消すのを防ぐ(復帰時は last が空に変わっているため
    /// 必ず保存値へ戻る)。同じ id の間は書き換えない(入力中の保護)
    #[test]
    fn alias_field_sync_rewrite_skips_disconnect_and_keeps_peer() {
        assert!(
            alias_field_sync_rewrite("peer-a", "peer-b"),
            "端末が切り替わった(空でない id へ): 保存値へ合わせる"
        );
        assert!(
            alias_field_sync_rewrite("", "peer-a"),
            "切断から復帰した(last は切断を記録済み): 保存値へ戻す"
        );
        assert!(
            !alias_field_sync_rewrite("peer-a", "peer-a"),
            "同じ端末の間は書き換えない(入力中の保護)"
        );
        assert!(
            !alias_field_sync_rewrite("peer-a", ""),
            "切断へ変わった: 欄を黙って空にしない(無効化だけ行う)"
        );
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
/// スクロール速度スライダの値表示(速め/標準/遅めの3分岐)。sync() とハンドラの
/// 両方で同じ文言を出すため関数へ切り出した(閾値は set_scroll_div の divisor)。
/// 標準帯のうち 60 は起動時の既定(state の初期値・KNIT_SCROLL_DIV 未指定)のため
/// 「(既定)」を付ける(スライダを初期位置から動かしていないことが分かるように)
fn scroll_speed_text(div: f64) -> &'static str {
    if div <= 40.0 {
        "速め"
    } else if div >= 140.0 {
        "遅め"
    } else if div == 60.0 {
        "標準(既定)"
    } else {
        "標準"
    }
}

/// スクロール速度の値ラベルのツールチップ(3 分岐の表示に内部値 divisor を併記)。
/// 中央帯(41〜139・約41%)は表示が同じ「標準」のまま動かないため、内部値で違いが
/// 読めるようにする(60 の「既定」の目印は保つ)
fn scroll_speed_tooltip(div: f64) -> String {
    let value = div.round() as i64;
    if div == 60.0 {
        format!("標準(既定・{value})")
    } else {
        format!("{}({value})", scroll_speed_text(div))
    }
}

/// スクロール速度の値ラベルへ表示(3 分岐)とツールチップ(内部値併記)を設定する。
/// set_label と違いツールチップを表示と別の文字列へ出す(scroll_speed ハンドラと
/// sync の両方から呼ぶ=ドラッグ直後も毎秒の反映も同じ文言になる)
unsafe fn set_scroll_speed_label(div: f64) {
    let l = PREFS_GAIN_LABEL.load(Ordering::Relaxed) as ID;
    if !l.is_null() {
        msg1_void_id(l, sel(c"setStringValue:"), nsstring(scroll_speed_text(div)));
        msg1_void_id(
            l,
            sel(c"setToolTip:"),
            nsstring(&scroll_speed_tooltip(div)),
        );
    }
}

pub(super) unsafe extern "C" fn scroll_speed(_s: ID, _c: SEL, sender: ID) {
    let f: unsafe extern "C" fn(ID, SEL) -> f64 =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    crate::set_scroll_div(260.0 - f(sender, sel(c"doubleValue")));
    // 値表示は毎秒の sync() を待たずに即座に切り替える(最大1秒遅れの解消。
    // スライダ自体は setContinuous(0) のまま=ドラッグ中の保存連発はしない)
    set_scroll_speed_label(crate::scroll_div());
    preferences::save();
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

/// カーソル速度スライダの値表示(倍率)。既定の 1.0(等倍・MOUSE_SCALE 初期値)の
/// ときだけ目印を出す(gain_text と同じ付け方。sync() とハンドラの両方で出す)
pub(super) fn mouse_scale_text(v: f64) -> String {
    if v == 1.0 {
        "等倍(既定)".to_string()
    } else {
        format!("{v:.1}倍")
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
    let l = SAVE_LABEL.load(Ordering::Relaxed) as ID;
    if !l.is_null() {
        msg1_void_id(l, sel(c"setStringValue:"), nsstring(text));
    }
}
/// 「接続先の端末」入力欄(保存ハンドラが本文を読み取る用)
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
/// 「直接つなぐ(手動接続)」のトークン入力欄(保存・生成ハンドラが読み書きする用)
pub(super) fn token_field() -> usize {
    TOKEN_FIELD.load(Ordering::Relaxed)
}

/// エイリアス欄を保存値へ書き換える判定(純粋関数・単体テストで守る)。
/// 端末 id が空(切断)へ変わった時は欄をクリアしない=入力途中の文言を毎秒の
/// sync() が黙って消すのを防ぐ。空でない id へ変わった時だけ保存値へ合わせる
/// (last は切断も記録するため、復帰時は必ず保存値へ戻る)
fn alias_field_sync_rewrite(last: &str, id: &str) -> bool {
    !id.is_empty() && last != id
}

/// エイリアス欄の現在値をアクティブな端末へ合わせる。端末が切り替わった時だけ
/// 書き換える(利用者の入力中に毎秒の sync() が書き換えるのを防ぐ)。
/// 切断(id が空)への変化では書き換えず無効化だけ行う(alias_field_sync_rewrite)
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
    if alias_field_sync_rewrite(&last, &id) {
        msg1_void_id(field, sel(c"setStringValue:"), nsstring(&alias));
    }
    // last は空(切断)への変化も記録する(復帰時に保存値へ戻すための目印)
    *last = id;
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
    // 切替方式ポップアップは開示中(menuWillOpen:)に selectItemAtIndex:/setTitle:
    // を打たない(接続先ピッカーと同じ保護。閉じたら次の sync() が反映する)
    let pop = SWITCH_POP.load(Ordering::Relaxed) as ID;
    if !pop.is_null() && !SWITCH_MENU_OPEN.load(Ordering::Relaxed) {
        msg1_void_i64(pop, sel(c"selectItemAtIndex:"), method);
        // 「ショートカットのみ」の項目名は実際の切替キー名へ(方式とキーの行が離れて
        // いるため、この行だけで「どのキーで切替できるか」が読み取れるようにする)。
        // 項目位置は常に 2(=3番目)。Windows 側の同じ項目も対称に書き換わる
        let title = format!(
            "切替キー({})のみ",
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
        let text=if preview {"タブレット操作中に有効・上で一時停止すると最近のタスクを表示"}
            else if !crate::trackpad::navigation_available() {"指の位置を取得できません。ピンチと通常のスクロールを利用できます"}
            else if !crate::trackpad::NAV_ENABLED.load(Ordering::Relaxed) {"ナビゲーションはオフです。スクロールとピンチは個別に利用できます"}
            else if android && remote {"タブレットを操作中・上で一時停止すると最近のタスクを表示"}
            else {"タブレットへ操作を切り替えると有効になります"};
        set_label(&GESTURE_HINT,text);
    }
    // KNIT_SHARE で禁じられた項目は、設定画面から許可できない
    let cap = knit_common::share::env_cap();
    enable(&super::PREFS_CHK_CLIP, cap.clip && !preview);
    enable(&super::PREFS_CHK_FILES, cap.files && !preview);
    enable(&super::PREFS_CHK_HISTORY, cap.clip && !preview);
    // 「操作」ページの環境変数固定の注記(KNIT_SWITCH_MODE 等)は env 起因で実行中
    // 変わらないため、build 時に 1 回だけ設定する(毎秒の sync() では触らない)
    let audio_ok = knit_common::share::env_cap().audio;
    enable(&super::PREFS_CHK_SPK, !android && !preview && audio_ok);
    enable(&super::PREFS_CHK_AUDIO, !android_app && !preview && audio_ok);
    // 相手側ラベルは接続状態によらず「どの端末との組み合わせか」を示し続ける。
    // 状態(接続済み/未接続)は下の PREFS_STATE が担うため二重に書き換えない
    set_label(&PEER_LABEL, &peer);
    let rtt = crate::RTT_MS.load(Ordering::Relaxed);
    // 手動接続(KNIT_TOKEN)運用中か(状態行・登録の管理・直接つなぐの注記が参照)
    let manual = knit_common::envutil::get("KNIT_TOKEN").is_some_and(|t| !t.is_empty());
    // 未接続が続くときの補足(再試行までの残り・見つけられない継続)も接続ページの
    // 状態行へ出す: メニューバー(gui.rs refresh_status)と同じ文言の関数から取り、
    // 設定画面だけを見ている人にも待ちの見通しが読み取れるようにする。
    // paired は PAIRED のまま渡さず登録の実体(paired_registered)へ通す: 接続キーの
    // 保存だけが残る状態で「自動で再接続します」を出さない(メニュー状態行と同じ)
    let state = connection_state_text(
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
    // 受け入れ範囲(KNIT_ALLOW_ANY/TS)と暗号化の行(本文・ツールチップ・警告色)は
    // すべて env 起因か静的定数のため、build 時に 1 回だけ設定する(毎秒の sync()
    // では触らない。文言の中身は accept_scope_text/secure_line_short の単体テストが守る)
    // 登録済みの台数(接続中の PEERS と peer-sides.json の保存済み件数の max)。
    // 1対1のときも台数が読めるように常時出す。手動接続(KNIT_TOKEN)の間は
    // 初期化が効かないため、同じ行に注意を出す
    let peers_now = registration_count(
        crate::PEERS.lock().unwrap_or_else(|e| e.into_inner()).len(),
        crate::tap::saved_peer_count(),
    );
    set_label(&REG_MGMT_LABEL, &registration_text(peers_now, manual));
    // 直接つなぐの説明(未設定=使い方、設定中=運用の注記へ切り替わる)。
    // トークン由来の検証/保存エラーがある間は注記行をエラー文言で置き換える
    // (次の保存成功まで保持。sync() が毎秒書き直すため preferences::save() の
    // 上書きにも負けない。接続先由来のエラーはここに出さない)
    let save_error = SAVE_ERROR_NOTE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    let token_note = token_note_with_error(
        manual,
        save_error_text(SaveErrorSource::Token, save_error.as_ref()),
    );
    set_label(&TOKEN_NOTE_L1, &token_note);
    // 直接つなぐの節の開閉(未設定の間は1行の開閉ラベルへ畳む。トークン保存で
    // 再起動した後は manual=true で常に展開される)
    sync_token_section();
    // 接続先(host)の保存エラーは下の保存状態行(SAVE_LABEL)へ集約する。ここ
    // (OWN_IP_LABEL)を差し替えると、エラーが出ている間この Mac のアドレスが
    // 見えなくなるため、この行は常時アドレス表示のままにする(相手側で IP を
    // 指定するときの確認用。set は下の表示条件の所で値が変わったときだけ行う)
    // 保存状態行(全ページ共通・最下部)にエラーを出す(接続先・トークン両方の
    // エラーが出る。通常時の文言は preferences::save() と初期構築が管理する=
    // エラー保持中だけここで書き直す。set_label がツールチップにも同じ文言を
    // 載せるため、1 行に収まらない長いエラーもホバーで読める)
    if let Some((_, err)) = &save_error {
        set_label(&SAVE_LABEL, err);
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
    // 待受アドレス(KNIT_BIND の指定)も env 起因のため build 時 1 回で済ませ、
    // 毎秒の sync() では触らない(ポート一覧はメニュー「ログを開く」項目の
    // ツールチップが担うため退避先の新設は不要)
    // 役割カードの注記行(通常/Env固定/切替進行の文言)。この Mac のアドレス表示
    // (OWN_IP_LABEL)は保存エラーの振り分け箇所で一緒に書き換えている
    set_label(&ROLE_NOTE_LABEL, &role_note_text());
    // 「接続先の端末」の入力と保存は、この Mac が接続しに行く側の間だけ有効。
    // 待ち受け側(ホスト役)では接続先は要らない(Windows 側の入力欄と同じ条件)
    let going_out = crate::effective_client_role();
    enable(&HOST_FIELD, going_out || preview);
    enable(&HOST_SAVE_BUTTON, going_out || preview);
    // このMacのアドレス行は「このMacがホスト(既定)」でまだ未接続の間(=相手側で
    // この Mac のアドレスを指定して接続しに来る場面)だけ見せる。それ以外(接続側・
    // 接続済み)で用事が無いため行ごと隠す(TABLET_VIEWS と同じ毎秒の setHidden)。
    // 接続先の保存エラーはこの行でなく SAVE_LABEL へ集約しているため無傷
    let own_ip = OWN_IP_LABEL.load(Ordering::Relaxed) as ID;
    if !own_ip.is_null() {
        let show_own_ip = preview || (!going_out && !connected);
        msg1_void_u8(own_ip, sel(c"setHidden:"), !show_own_ip as u8);
        // アドレス本文は own_ip_text の 10 秒キャッシュに従い、値が変わったとき
        // (=キャッシュ更新時)だけ set する。隠れている間の set も止める(隠れた
        // 間に変わっていても、表示へ戻った最初のここで必ず最新が入る)
        if show_own_ip {
            let text = own_ip_text();
            let mut last = OWN_IP_LAST.lock().unwrap_or_else(|e| e.into_inner());
            if *last != text {
                set_label(&OWN_IP_LABEL, &text);
                *last = text;
            }
        }
    }
    // 「選択中の端末の名前」(エイリアス)欄は端末切替に追従させる。
    // 「このMacの名前」欄は保存ハンドラだけが書き換える(入力中に触らない)
    sync_alias_field(preview, connected);
    // エイリアス行(キャプション・欄・保存)は未接続の間は行ごと隠す: 保存先の
    // 端末が決まらない欄が無効のまま常設していると「何のための欄か」が読めない
    // ため(PEER_VIEWS と同じパターン。接続済みでは現状どおり表示し、Enter 保存・
    // SaveErrorSource::Alias の保存状態行(SAVE_LABEL)へのエラー出力は不変)
    for v in ALIAS_VIEWS.lock().unwrap_or_else(|e| e.into_inner()).iter() {
        let v = *v as ID;
        if !v.is_null() { msg1_void_u8(v, sel(c"setHidden:"), !(connected || preview) as u8); }
    }
    enable(&ALIAS_SAVE_BUTTON, connected || preview);
    enable(&OWN_NAME_FIELD, !preview);
    enable(&OWN_NAME_SAVE_BUTTON, !preview);
    // 状況行: 未接続のとき手動接続(トークン)運用中なら、繋がらない原因として
    // 相手側のトークン不一致が読み取れる文言へ切り替わる(connection_hint_text)
    let android_perm_pending = matches!(
        crate::active_android_app_permissions(),
        Some((false, _))
    );
    let hint = connection_hint_text(preview, connected, manual, android_perm_pending, &peer);
    set_label(&CONNECTION_HINT, &hint);
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
    set_label(&AUDIO_LABEL, if android_app { "音声共有(Androidアプリ版は未対応)" } else { "接続先の音声をこのMacで再生" });
    // スピーカーミュートのキャプションは check の行(幅 460px)に収まる短縮形を
    // set_label と同じ文中に置けないため、キャプションとツールチップを別々に
    // 書き換える(例外の完全版はツールチップが担う)
    let spk_label = SPEAKER_LABEL.load(Ordering::Relaxed) as ID;
    if !spk_label.is_null() {
        // 括弧(音声転送オフの間は無効)は音声転送がオフの間だけ付ける
        msg1_void_id(
            spk_label,
            sel(c"setStringValue:"),
            nsstring(speaker_caption(android, !crate::audio::MUTED.load(Ordering::Relaxed))),
        );
        msg1_void_id(
            spk_label,
            sel(c"setToolTip:"),
            nsstring(speaker_tooltip(android)),
        );
    }
    // SHARE_HINT: 通常時の本文は「切断すると元に戻ります」の 1 文とし、異常終了時の
    // 復旧はツールチップへ退避する。set_label はツールチップを本文で上書きするため、
    // stringValue と toolTip を別々に設定する(上の SPEAKER_LABEL と同じパターン)
    let share_hint_label = SHARE_HINT.load(Ordering::Relaxed) as ID;
    if !share_hint_label.is_null() {
        let (text, tip) = share_hint_and_tooltip(android_app, android);
        msg1_void_id(share_hint_label, sel(c"setStringValue:"), nsstring(text));
        msg1_void_id(share_hint_label, sel(c"setToolTip:"), nsstring(tip));
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
    set_scroll_speed_label(crate::scroll_div());
    // カーソル速度スライダも同じく起動時の設定復元・リモート適用を画面へ反映
    let mouse_slider = PREFS_MOUSE_SLIDER.load(Ordering::Relaxed) as ID;
    if !mouse_slider.is_null() {
        let get: unsafe extern "C" fn(ID, SEL) -> f64 =
            std::mem::transmute(crate::objc_msgSend as *const () as usize);
        let set: unsafe extern "C" fn(ID, SEL, f64) =
            std::mem::transmute(crate::objc_msgSend as *const () as usize);
        let m = crate::mouse_scale();
        if get(mouse_slider, sel(c"doubleValue")) != m {
            set(mouse_slider, sel(c"setDoubleValue:"), m);
        }
    }
    set_label(&PREFS_MOUSE_SCALE_LABEL, &mouse_scale_text(crate::mouse_scale()));
    // スクロール互換(古いアプリ用)も設定ファイルの復元をここで画面へ反映
    let compat = PREFS_CHK_SCROLL_COMPAT.load(Ordering::Relaxed) as ID;
    if !compat.is_null() {
        msg1_void_i64(
            compat,
            sel(c"setState:"),
            crate::SCROLL_COMPAT.load(Ordering::Relaxed) as i64,
        );
    }
    // タブレット項目は、タブレットが見つかっている(または接続中の)時だけ見せる
    let tablets = !crate::android::state::snapshot().1.tablets.is_empty();
    let show_tablet = preview || tablets || android || android_app;
    for v in TABLET_VIEWS.lock().unwrap_or_else(|e| e.into_inner()).iter() {
        let v = *v as ID;
        if !v.is_null() { msg1_void_u8(v, sel(c"setHidden:"), !show_tablet as u8); }
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
            h: 894.0,
        },
        1 | 2 | 4,
        2,
        0,
    );
    if win.is_null() {
        return win;
    }
    msg1_void_id(win, sel(c"setTitle:"), nsstring("Knit設定"));
    msg1_void_u8(win, sel(c"setReleasedWhenClosed:"), 0);
    msg0_void(win, sel(c"center"));
    let cv = msg0(win, sel(c"contentView"));
    let sidebar = view(
        cv,
        NSRect {
            x: 0.0,
            y: 0.0,
            w: 172.0,
            h: 894.0,
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
    // 「その他」(接続と無関係の設定の導線)はページではなくサイドバー下部へ置く:
    // どのページを開いていても同じ位置へ手が届くようにするため(かつては接続
    // ページの最下部にしかなく、ページをまたいで探す必要があった)。ページ選択
    // ボタン群の下端(y=474)とバージョン表示(y=24)の間の空白へ、見出し+6部品を
    // ボタンと同じ 48px 刻みの縦並びで収める(見出しフレーム上限 459・最下ボタンの
    // 下端 176 で、上下の部品とも重ならない)。確認ダイアログ(読み込み・初期化)と
    // 初期化の導線は移動前と同じ形で守る
    label(sidebar, "その他", 20.0, 436.0, 132.0, 13.0, false);
    // 設定とログの保存場所は注記行では出さず、各ボタンのツールチップへ統合した
    //(ボタンが開けない環境の代替の手がかりはツールチップのパス表記が担う)
    let dir_btn = sidebar_button(
        sidebar,
        target,
        "設定フォルダを開く",
        c"sdOpenSettingsDir:",
        384.0,
    );
    if !dir_btn.is_null() {
        msg1_void_id(
            dir_btn,
            sel(c"setToolTip:"),
            nsstring("~/.config/knit を開きます(preferences.json・env などがあります)。ボタンで開けない場合は、このフォルダを開いてください"),
        );
    }
    sidebar_button(sidebar, target, "設定を書き出す…", c"sdExportPrefs:", 336.0);
    sidebar_button(sidebar, target, "設定を読み込む…", c"sdImportPrefs:", 288.0);
    // 詳細記録(--diag 相当)。GUI から切替できるようにした項目で、お問い合わせ時に
    // 担当者が入れ替わることもあるため設定として残る(出力は 1秒ごとの [diag] 行)。
    // 詳細はツールチップで伝える。スイッチはサイドバーの右端へ置く
    let diag_caption = check_at(
        sidebar,
        target,
        "詳しく記録",
        c"sdDiagLog:",
        &DIAG_SWITCH,
        16.0,
        94.0,
        112.0,
        244.0,
    );
    if !diag_caption.is_null() {
        msg1_void_id(
            diag_caption,
            sel(c"setToolTip:"),
            nsstring("1秒ごとの診断行を出します(--diag 相当)。お問い合わせ時にオンにします"),
        );
    }
    sidebar_button(sidebar, target, "再起動", c"sdRestart:", 192.0);
    // 破壊ボタン(すべての設定を初期化)は日常の設定導線(書き出す・読み込む)と
    // 離した位置・赤文字で見た目を区別する。初期化の確認ダイアログと合わせて
    // 三重の抑止とする
    mark_destructive(sidebar_button(
        sidebar,
        target,
        "すべての設定を初期化…",
        c"sdResetSettings:",
        140.0,
    ));
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
                    h: 834.0,
                },
            );
            surface(p, c"controlBackgroundColor", 0.0);
            PAGES[i].store(p as usize, Ordering::Relaxed);
            p
        })
        .collect();
    let titles = [
        ("接続", "このMacの役割と、つながっている端末の状態です。"),
        ("画面配置", "接続先の位置を、実際の画面配置に合わせます。"),
        ("操作", "画面を移る方法と、カーソル・スクロールの感触です。"),
        ("共有", "このMacが渡すものと、受け取るものを選びます。"),
    ];
    for (p, (title, sub)) in pages.iter().zip(titles) {
        label(*p, title, 28.0, 774.0, 565.0, 24.0, false);
        label(*p, sub, 28.0, 745.0, 565.0, 12.0, true);
    }
    let p = pages[0];
    // Deskflow 流の構成: 役割の 2 択と接続先(IP)・このMacの名前を最上部のカードへ
    // 置く(役割と接続の作りは一对のため。Windows 側設定の「接続先のMac」と対)。
    // 中: 端末の状態と選択中の端末の名前・通信の保護の状況。下: 登録と直接つなぎ。
    // 下のカードには初期化の注意行も常時出す(ボタンを開く前に読めるように)。
    // 画面は高いが 894pt(956pt 画面の MacBook Air にも収まる)
    group(p, 487.0, 275.0);
    // 中段カードの上端(226+261=487)は上段カードの下端(487)に一致させる。
    // 中段の最上行は端末の状況(PREFS_STATE y=462・フレーム上限 486)のため、
    // 上端 487 に上げても侵さない。下端は状況 5 行(ANDROID/CONNECTION/BIND/
    // SECURE/ACCEPT・22px 刻み)を収めた受け入れ範囲 y=227 の直下(226)。
    // 直接つなぐの節を下段カードへ移した分、上段が 68px 縮んだ分だけ上へ詰めた
    group(p, 226.0, 261.0);
    // 下段カードは登録と直接つなぎ(かつては状況行・待受行・「その他」節が混在
    // していた)。上端は中段カードの下端(226)と一致させ、中身は見出し〜登録の
    // 初期化ボタンまで+直接つなぐの節(展開時の欄・ボタン・注記の 4 部品の分まで
    // 高さへ入れている。閉じている間は下端に余白が出る。上端の余白は 37px)
    group(p, 18.0, 208.0);
    // 役割(どちらがホストか)。選ぶと相手にも伝わり、両方が再起動して切り替わる
    label(p, "このMacの役割", 40.0, 733.0, 300.0, 13.0, false);
    ROLE_RADIO[0].store(
        radio(p, target, "このMacがホスト(既定) — 相手の端末が接続しに来ます", c"sdRole:", 0, NSRect { x: 40.0, y: 701.0, w: 516.0, h: 22.0 }) as usize,
        Ordering::Relaxed,
    );
    ROLE_RADIO[1].store(
        radio(p, target, "相手の端末がホスト — このMacが接続しに行きます", c"sdRole:", 1, NSRect { x: 40.0, y: 675.0, w: 516.0, h: 22.0 }) as usize,
        Ordering::Relaxed,
    );
    // 注記行は状況で文言が変わる(通常の再起動案内・KNIT_ROLE 固定中・切替の進行)
    ROLE_NOTE_LABEL.store(
        label(p, &role_note_text(), 40.0, 651.0, 516.0, 11.0, true) as usize,
        Ordering::Relaxed,
    );
    // 接続先の端末(この Mac が接続しに行く側のときだけ使う。Windows 側設定の
    // 「接続先のMac」と対)。空欄=LAN からの自動発見。保存(=env へ書き換え+再起動)
    // は欄の action には繋がず「保存して再接続」ボタン押下でのみ走る(Enter や
    // 初期フォーカスの Return が確認なしの再起動に直結するのを防ぐ)
    label(p, "接続先の端末", 40.0, 621.0, 300.0, 13.0, false);
    let host_now = knit_common::envutil::get("KNIT_HOST").unwrap_or_default();
    HOST_FIELD.store(
        text_field(
            p,
            target,
            None,
            "接続先の端末のアドレス",
            "192.168.1.23(空欄で自動発見)",
            &host_now,
            NSRect { x: 40.0, y: 587.0, w: 336.0, h: 26.0 },
        ) as usize,
        Ordering::Relaxed,
    );
    HOST_SAVE_BUTTON.store(
        button(p, target, "保存して再接続", c"sdSaveHost:", NSRect { x: 392.0, y: 581.0, w: 164.0, h: 30.0 }) as usize,
        Ordering::Relaxed,
    );
    // この Mac 自身のアドレス(相手側で IP を指定するときの確認用。複数 NIC は列挙)。
    // 本文は build 時に 1 回入れ、以降は sync() が値の変化時(10 秒キャッシュの
    // 更新時)だけ set するため、最初の値を OWN_IP_LAST へも入れておく
    let own_ip_text_now = own_ip_text();
    OWN_IP_LABEL.store(
        label(p, &own_ip_text_now, 40.0, 563.0, 516.0, 11.0, true) as usize,
        Ordering::Relaxed,
    );
    *OWN_IP_LAST.lock().unwrap_or_else(|e| e.into_inner()) = own_ip_text_now;
    // このMacの名前(相手への名乗り名。hello/hello_ok の name になる。空欄=ホスト名)。
    // 相手側の画面・通知に載るため、複数台をつなぐときの区別に使える。Enter での
    // 保存は即座に効くだけ(再起動を伴わない)のため欄の action を残している
    label(p, "このMacの名前", 40.0, 541.0, 300.0, 13.0, false);
    let own_now = crate::OWN_NAME.lock().unwrap_or_else(|e| e.into_inner()).clone();
    OWN_NAME_FIELD.store(
        text_field(
            p,
            target,
            Some(c"sdSaveOwnName:"),
            "このMacの名前",
            &own_name_placeholder(&crate::hostname_label()),
            &own_now,
            NSRect { x: 40.0, y: 510.0, w: 336.0, h: 26.0 },
        ) as usize,
        Ordering::Relaxed,
    );
    OWN_NAME_SAVE_BUTTON.store(
        button(p, target, "保存", c"sdSaveOwnName:", NSRect { x: 392.0, y: 503.0, w: 164.0, h: 30.0 }) as usize,
        Ordering::Relaxed,
    );
    // 端末の状態(この Mac と相手)。状態行の 1 行で読み取れ、続きは必要な時だけ出る
    PREFS_STATE.store(
        label(p, "接続を確認しています…", 40.0, 462.0, 540.0, 14.0, false) as usize,
        Ordering::Relaxed,
    );
    // 端末の図(このMac ⇄ 相手)。名前をアイコンの横へ置く 1 行にした
    symbol(p, "laptopcomputer", 111.0, 430.0, 28.0);
    label(p, "このMac", 145.0, 432.0, 110.0, 14.0, false);
    symbol(p, "link", 284.0, 434.0, 20.0);
    symbol(p, "desktopcomputer", 439.0, 430.0, 28.0);
    // 相手側の名前は接続先に応じて書き換わる(Android 接続中は端末名)
    PEER_LABEL.store(
        label(p, "端末", 473.0, 432.0, 115.0, 14.0, false) as usize,
        Ordering::Relaxed,
    );
    // 操作する端末(登録済みが 2 台以上のときだけ行ごと見せる。sync() が切り替える)。
    // 状況行(下 3 行)はこのポップアップより下に置く、重ならないようにする
    let peer_caption = label(p, "操作する端末", 40.0, 399.0, 105.0, 12.0, true);
    let peer_pop = popup(p, target, &[], c"sdSelectPeer:", NSRect { x: 150.0, y: 396.0, w: 428.0, h: 28.0 });
    msg1_void_id(peer_pop, sel(c"setAccessibilityLabel:"), nsstring("操作する接続先"));
    msg1_void_id(msg0(peer_pop, sel(c"menu")), sel(c"setDelegate:"), target);
    PEER_POP.store(peer_pop as usize, Ordering::Relaxed);
    *PEER_VIEWS.lock().unwrap_or_else(|e| e.into_inner()) =
        vec![peer_caption as usize, peer_pop as usize];
    // 選択中の端末の名前(エイリアス)。同じコンピュータ名の端末が複数あるときの
    // 区別や、履歴・通知の名前を好きな呼び名に変えられる。空欄でコンピュータ名に戻る。
    // Enter での保存は即座に効くだけ(再起動を伴わない)ため欄の action を残している。
    // 未接続の間は保存先の端末が決まらないため sync() が行ごと隠す(ALIAS_VIEWS)
    let alias_caption = label(p, "選択中の端末の名前", 40.0, 372.0, 200.0, 12.0, true);
    ALIAS_FIELD.store(
        text_field(
            p,
            target,
            Some(c"sdSaveAlias:"),
            "選択中の端末の名前",
            "空欄でコンピュータ名に戻る",
            "",
            NSRect { x: 40.0, y: 338.0, w: 336.0, h: 26.0 },
        ) as usize,
        Ordering::Relaxed,
    );
    ALIAS_SAVE_BUTTON.store(
        button(p, target, "保存", c"sdSaveAlias:", NSRect { x: 392.0, y: 336.0, w: 164.0, h: 30.0 }) as usize,
        Ordering::Relaxed,
    );
    *ALIAS_VIEWS.lock().unwrap_or_else(|e| e.into_inner()) = vec![
        alias_caption as usize,
        ALIAS_FIELD.load(Ordering::Relaxed),
        ALIAS_SAVE_BUTTON.load(Ordering::Relaxed),
    ];
    // 状況があった時だけ内容が入る行(空の間は見えない)。位置はエイリアス入力欄
    //(y=338, h=26 → 338〜364)の直下: ラベルの高さは size+10=21 なので y=315
    //(フレーム 315..336)とすると入力欄・保存ボタン(336..366)とも重ならない
    ANDROID_STATE.store(
        label(p, "", 40.0, 315.0, 540.0, 11.0, true) as usize,
        Ordering::Relaxed,
    );
    // 未接続・許可待ちの案内と、接続済みでも移り方の一行。端末の状況の 1 行として
    // 中段カードへ置く(かつては下段カードにあったが、登録と無関係な行だった)
    CONNECTION_HINT.store(
        label(p, "", 40.0, 293.0, 540.0, 11.0, true) as usize,
        Ordering::Relaxed,
    );
    // 待受アドレス(KNIT_BIND の指定)。env 起因で実行中変わらないため、build 時に
    // 本文(=label が同じ文言のツールチップも設定)と既定の隠しを 1 回だけ済ませる
    //(既定 0.0.0.0=すべてのアドレスの間は行ごと隠す。ポート一覧はメニューの
    // 「ログを開く」項目のツールチップが担う。以降は毎秒の sync() が保たない
    // 代わりに触らない)。KNIT_BIND 指定時のみ表示だが、行の高さは常時確保する
    //(下の行との 22px 刻みを保つため)
    let bind = knit_common::envutil::get("KNIT_BIND").unwrap_or_else(|| "0.0.0.0".to_string());
    let bind_label = label(
        p,
        &format!("待受アドレス: {bind}(~/.config/knit/env の KNIT_BIND)"),
        40.0,
        271.0,
        540.0,
        11.0,
        true,
    );
    if bind == "0.0.0.0" {
        msg1_void_u8(bind_label, sel(c"setHidden:"), 1);
    }
    BIND_LABEL.store(bind_label as usize, Ordering::Relaxed);
    // 通信の保護(常時暗号化・相互認証)。設定は存在しない(常時ON)のため、「設定不要」と
    // 鍵の略号(フィンガープリント)を常時表示する。本文(secure_line_short)は静的
    // 定数・略号(key_fingerprint)も OnceLock のため、build 時に本文とツールチップ
    //(Win 側と同じ encryption_line の完全版)を 1 回だけ設定する(毎秒の sync() では
    // 触らない)。状況行は 22px 刻み・フレーム同士は重ならない
    let secure_label = label(p, secure_line_short(), 40.0, 249.0, 540.0, 11.0, true);
    msg1_void_id(
        secure_label,
        sel(c"setToolTip:"),
        nsstring(&knit_common::secure::encryption_line(key_fingerprint().as_deref())),
    );
    SECURE_LABEL.store(secure_label as usize, Ordering::Relaxed);
    // 接続の受け入れ範囲(net.rs の判定結果)。KNIT_ALLOW_ANY/TS の緩和が画面から
    // 見えない問題への対策。scope は env 起因で実行中変わらないため、build 時に
    // 本文・ツールチップ・警告色(ANY のとき赤)を 1 回だけ設定する
    let scope = knit_common::net::accept_scope();
    let accept_label = label(p, &accept_scope_text(scope), 40.0, 227.0, 540.0, 11.0, true);
    msg1_void_id(
        accept_label,
        sel(c"setToolTip:"),
        nsstring(&accept_scope_tooltip(scope)),
    );
    if scope.is_wide_open() {
        let color = msg0(
            objc_getClass(c"NSColor".as_ptr()),
            sel(c"systemRedColor"),
        );
        msg1_void_id(accept_label, sel(c"setTextColor:"), color);
    }
    ACCEPT_LABEL.store(accept_label as usize, Ordering::Relaxed);
    // 下段カードは登録と直接つなぎ(見出しと中身が一致する。「その他」の節は
    // サイドバー下部へ移動した)。見出し〜初期化ボタンをカード上端(226)からの
    // 余白 37px と同じ形で並べる(見出し 13pt のフレーム 166〜189・ボタン 160〜190
    // は重なるが x が離れる。注記行の実描画とは重ならない)
    label(p, "登録と直接つなぎ", 40.0, 166.0, 300.0, 13.0, false);
    button(
        p,
        target,
        "端末を登録…",
        c"sdRegistration:",
        NSRect { x: 392.0, y: 160.0, w: 188.0, h: 30.0 },
    );
    // 初期化(=鍵の再生成)の破壊性は、ダイアログを開く前のこの行で読み取れるように
    // 常時出す(改善依頼: ボタンからは読めなかったため)
    label(
        p,
        "初期化すると全端末が締め出され、再登録が必要です(取り消せません)",
        40.0,
        147.0,
        540.0,
        11.0,
        true,
    );
    // 登録の管理行には登録済み台数を常時表示(1対1でも台数が出る)。
    // sync() が台数(PEERS と peer-sides.json の max)と KNIT_TOKEN の有無で書き換える
    REG_MGMT_LABEL.store(
        label(
            p,
            &registration_text(
                registration_count(
                    crate::PEERS.lock().unwrap_or_else(|e| e.into_inner()).len(),
                    crate::tap::saved_peer_count(),
                ),
                knit_common::envutil::get("KNIT_TOKEN").is_some_and(|t| !t.is_empty()),
            ),
            40.0,
            125.0,
            336.0,
            13.0,
            false,
        ) as usize,
        Ordering::Relaxed,
    );
    // 破壊ボタンは「端末を登録…」の直下に密着させない(誤操作の抑止。注記行との
    // 間に間隔を空ける)。1台だけの剥奪は未実装のため、ツールチップで案内する
    let reset_btn = button(
        p,
        target,
        "すべての登録を初期化…",
        c"sdResetRegistration:",
        NSRect { x: 392.0, y: 109.0, w: 188.0, h: 30.0 },
    );
    mark_destructive(reset_btn);
    if !reset_btn.is_null() {
        msg1_void_id(
            reset_btn,
            sel(c"setToolTip:"),
            nsstring("特定の1台だけ外すには、初期化後に必要な端末を再登録します"),
        );
    }
    // 直接つなぐ(手動接続)。両側へ同じトークン(KNIT_TOKEN)を設定すると登録操作
    // なしに直接つながる(相手は Windows・Mac・タブレットを問わない)。「生成」は
    // 32 文字の共有トークンを作って入れるだけで、保存はしない(保存して初めて
    // env へ書き出され、再起動で効く)。保存は「保存して再接続」ボタン押下でのみ
    // 走る(接続先欄と同じ理由で Enter が再起動に直結しないようにする)。
    // 登録の導線(多数派)の下へ置く: トークン未設定の間は上級者向けの機能を
    // 全員に見せる意味が無いため、節全体を 1 行の開閉ラベル(トグルボタン)へ畳む
    //(使い方の説明はラベルのツールチップへ移設)。クリックで展開し、開閉状態は
    // プロセス共有の static ため開き直しても維持される。トークン設定済みなら常に
    // 展開(sync_token_section)。題名は他欄と同じ 13pt で節の見出しを兼ねる(setup
    // の初期化ガイド「設定「接続」の「直接つなぐ(手動接続)」」が指す先はこの題名。
    // 初期化が効かない旨は上の登録の管理行が「(手動接続中)」で出す)
    let token_toggle = button(
        p,
        target,
        "直接つなぐ(手動接続)・上級者向け ▸",
        c"sdToggleTokenSection:",
        NSRect { x: 40.0, y: 81.0, w: 516.0, h: 24.0 },
    );
    msg1_void_u8(token_toggle, sel(c"setBordered:"), 0);
    let heading_font = msg1_id_f64(
        objc_getClass(c"NSFont".as_ptr()),
        sel(c"systemFontOfSize:"),
        13.0,
    );
    msg1_void_id(token_toggle, sel(c"setFont:"), heading_font);
    msg1_void_id(
        token_toggle,
        sel(c"setToolTip:"),
        nsstring(token_note_lines(false)),
    );
    msg1_void_id(
        token_toggle,
        sel(c"setAccessibilityLabel:"),
        nsstring("直接つなぐ(手動接続)の開閉"),
    );
    TOKEN_TOGGLE.store(token_toggle as usize, Ordering::Relaxed);
    let token_now = knit_common::envutil::get("KNIT_TOKEN")
        .filter(|t| !t.is_empty())
        .unwrap_or_default();
    let token_field = text_field(
        p,
        target,
        None,
        "直接つなぐ(手動接続)のトークン",
        "32文字以上の半角英数字(空欄で通常の登録)",
        &token_now,
        NSRect { x: 40.0, y: 49.0, w: 280.0, h: 26.0 },
    );
    TOKEN_FIELD.store(token_field as usize, Ordering::Relaxed);
    // トークン行のボタンは他の保存行と同じ位置・幅へ揃える(かつては欄が 320px で
    // 「保存して再接続」が x=432・幅 124 の行だけずれ、継ぎ接ぎに見えていた)。
    // 権 40..320・生成 328..384・保存 392..556 とすべて 8px 間隔で重ならない
    let gen_btn = button(
        p,
        target,
        "生成",
        c"sdGenToken:",
        NSRect { x: 328.0, y: 39.0, w: 56.0, h: 30.0 },
    );
    TOKEN_SAVE_BUTTON.store(
        button(p, target, "保存して再接続", c"sdSaveToken:", NSRect { x: 392.0, y: 39.0, w: 164.0, h: 30.0 }) as usize,
        Ordering::Relaxed,
    );
    TOKEN_NOTE_L1.store(
        label(p, token_note_lines(!token_now.is_empty()), 40.0, 19.0, 516.0, 11.0, true) as usize,
        Ordering::Relaxed,
    );
    *TOKEN_VIEWS.lock().unwrap_or_else(|e| e.into_inner()) = vec![
        token_field as usize,
        gen_btn as usize,
        TOKEN_SAVE_BUTTON.load(Ordering::Relaxed),
        TOKEN_NOTE_L1.load(Ordering::Relaxed),
    ];
    // ウィンドウを出す前に開閉の初期状態を整える(未設定+閉の間は一瞬でも 4 部品が
    // 見えたままにならないように。以降は毎秒の sync() が保つ)
    sync_token_section();
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
        "グレー：Mac／ブルー：接続先(各モニターに機種名)。ドラッグで配置(斜めも可)",
        28.0,
        98.0,
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
        78.0,
        560.0,
        11.0,
        true,
    );
    if !layout_hint.is_null() {
        msg1_void_id(
            layout_hint,
            sel(c"setToolTip:"),
            nsstring("ブロックのクリックで操作する端末を選べます。Macの上へ戻すと「全画面の端(モニター指定なし)」になります。保存済みの端末は各端末の設定が優先されます"),
        );
    }
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
    // 切替グループは説明行と滞在時間スライダの分を高く取り、スクロールグループは
    // 方向・速度・カーソル速度・互換の 4 行。カーソル速度と互換を足した分
    //(68px)のうち 46px は切替グループを上へ、22px はタブレット以下を下へずらす
    group(p, 316.0, 148.0);
    group(p, 154.0, 146.0);
    divider(p, 396.0);
    divider(p, 254.0);
    label(p, "切替方式", 40.0, 432.0, 220.0, 13.0, false);
    let hotkey_title = format!(
        "切替キー({})のみ",
        knit_common::keymap::mac_key_label(crate::hotkey_kc())
    );
    let switch_pop = popup(
        p,
        target,
        &[
            // 第1項は起動時の既定(EDGE_TAPS 初期値 2)のため「(既定)」を付ける
            // (delay_text・scroll_speed_text と同じ目印の付け方。項目の並びは
            // switch_method が index で読むため、文言を変えても位置は不変)
            "端に2回触れる(既定)",
            "端で少し待つ",
            // 項目名は sync() が現在の切替キー名で書き換える(初回も同じ形式で)
            hotkey_title.as_str(),
            "端に1回触れる",
        ],
        c"sdSwitchMethod:",
        NSRect {
            x: 300.0,
            y: 430.0,
            w: 252.0,
            h: 28.0,
        },
    );
    // 毎秒の sync() が selectItemAtIndex:/setTitle: を打つため、接続先ピッカー
    // (PEER_POP)と同じ menuWillOpen:/menuDidClose: の delegate を付けて開示中の
    // 書き換えを控える(IMP は sender=menu で区別して共用)
    let switch_menu = msg0(switch_pop, sel(c"menu"));
    if !switch_menu.is_null() {
        msg1_void_id(switch_menu, sel(c"setDelegate:"), target);
        SWITCH_MENU.store(switch_menu as usize, Ordering::Relaxed);
    }
    SWITCH_POP.store(switch_pop as usize, Ordering::Relaxed);
    // 速度越境(FAST_EDGE)の説明行。sync() が方式と KNIT_FAST_EDGE に応じて
    // 表示を切り替える(GESTURE_HINT と同じ扱いの 1 行ラベル)。本文は効果の
    // 1 文だけとし、誤発火が続くときの対処(KNIT_FAST_EDGE=0 で無効化)は困った
    // 時だけ必要な情報のためツールチップへ退避する(label() が本文と同じ
    // ツールチップを設定するため上書きする)
    let fast_hint = label(
        p,
        "速い移動では1回の到達で切り替わります",
        40.0,
        404.0,
        516.0,
        11.0,
        true,
    );
    if !fast_hint.is_null() {
        msg1_void_id(
            fast_hint,
            sel(c"setToolTip:"),
            nsstring("速い移動では1回の到達で切り替わります。誤発火が続く場合は KNIT_FAST_EDGE=0 で無効化できます"),
        );
    }
    FAST_EDGE_HINT.store(fast_hint as usize, Ordering::Relaxed);
    label(p, "切替キー", 40.0, 364.0, 260.0, 13.0, false);
    let current = crate::hotkey_kc();
    // env 固定(KNIT_HOTKEY_KC)のキーも名前で読めるように mac_key_label を通す
    //(未知のコードのときだけ同関数が「コードN」へフォールバックする)
    let custom = format!(
        "現在のキー({})",
        knit_common::keymap::mac_key_label(current)
    );
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
            y: 362.0,
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
        let delay_divider = divider(p, 352.0) as usize;
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
            322.0,
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
        264.0,
    );
    // 方向は起動時の macOS 設定で決まり、起動中の変更は設定画面を開き直す時に
    // 反映する(この画面を開くタイミングで再取得している)
    msg1_void_id(
        scroll_caption,
        sel(c"setToolTip:"),
        nsstring("macOSの自然スクロール設定に合わせます。起動中に切り替えた場合は、この設定画面を開き直すと反映します"),
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
        230.0,
    );
    // カーソル速度(0.2..3.0 の倍率)。Mac の加速済み delta に Windows 側の加速が
    // 重なって速すぎ/遅すぎに感じるときの調整口(KNIT_MOUSE_SCALE は初期値)。
    // 値表示・復元反映はスクロール速度と同じ作法(imp_mouse_scale / sync)。
    // 「N倍」の基準(何に対する倍か)は caption の標準ラベルには書けないため、
    // ツールチップで補う(値表示 mouse_scale_text と同じ 1.0=等倍の言い方)
    let mouse_caption = slider(
        p,
        target,
        "カーソル速度   遅い / 速い",
        crate::mouse_scale(),
        0.2,
        3.0,
        c"sdMouseScale:",
        &PREFS_MOUSE_SLIDER,
        &PREFS_MOUSE_SCALE_LABEL,
        198.0,
    );
    msg1_void_id(
        mouse_caption,
        sel(c"setToolTip:"),
        nsstring("相手の画面でのカーソル移動量の倍率(1.0=等倍)"),
    );
    // スクロール互換(古いアプリ用): 120 未満のホイール量を無視する古い設計の
    // アプリ向けに 1 ノッチ(120)単位へ切り替える(KNIT_SCROLL_COMPAT=1 と同じ効果)
    check(
        p,
        target,
        "スクロール互換(古いアプリ用)",
        c"sdScrollCompat:",
        &PREFS_CHK_SCROLL_COMPAT,
        162.0,
    );
    // タブレット(Android)がある時だけ見せる。無い人には関係のない項目なので隠す
    let mut tablet = vec![group(p, 50.0, 84.0) as usize, divider(p, 86.0) as usize];
    tablet.push(check(p, target, "タブレットのナビゲーション", c"sdTabletNav:", &NAV_SWITCH, 96.0) as usize);
    tablet.push(check(p, target, "ピンチで拡大・縮小", c"sdPinch:", &PINCH_SWITCH, 58.0) as usize);
    tablet.push(label(p, "", 28.0, 26.0, 565.0, 11.0, true) as usize);
    GESTURE_HINT.store(*tablet.last().unwrap(), Ordering::Relaxed);
    tablet.push(NAV_SWITCH.load(Ordering::Relaxed));
    tablet.push(PINCH_SWITCH.load(Ordering::Relaxed));
    *TABLET_VIEWS.lock().unwrap_or_else(|e| e.into_inner()) = tablet;
    // 環境変数で固定されている項目の注記(KNIT_SWITCH_MODE 等)。固定が無ければ
    // 空文字=見えない。env 起因で実行中変わらないため、build 時に本文と対処案の
    // ツールチップを 1 回だけ設定する(毎秒の sync() では触らない)
    let op_fixed: Vec<&str> = OPERATION_ENV_KEYS
        .iter()
        .filter(|k| crate::envutil::get(k).is_some())
        .copied()
        .collect();
    let env_note_label = label(p, &operation_env_note(&op_fixed), 28.0, 0.0, 565.0, 11.0, true);
    if !op_fixed.is_empty() {
        msg1_void_id(
            env_note_label,
            sel(c"setToolTip:"),
            nsstring(operation_env_note_tooltip()),
        );
    }
    ENV_NOTE_LABEL.store(env_note_label as usize, Ordering::Relaxed);
    let p = pages[3];
    // 渡すもの(テキスト・画像・ファイル)と、音。クリップとファイルの各 check の
    // 下の説明行は caption の言い換えで情報が増えないため削り、説明は check の
    // caption へ setToolTip で出す(scroll_caption と同じ作法)。削った分だけ
    // 上の group の高さも詰める
    group(p, 280.0, 114.0);
    group(p, 152.0, 114.0);
    group(p, 22.0, 80.0);
    divider(p, 344.0);
    let clip_caption = check(p, target, "テキストと画像", c"sdClipShare:", &PREFS_CHK_CLIP, 384.0);
    msg1_void_id(
        clip_caption,
        sel(c"setToolTip:"),
        nsstring("コピーした内容を、もう1台でも貼り付けられます"),
    );
    let file_caption = check(p, target, "ファイルの受け渡し", c"sdFileShare:", &super::PREFS_CHK_FILES, 312.0);
    msg1_void_id(
        file_caption,
        sel(c"setToolTip:"),
        nsstring("コピーしたファイルや、掴んだファイルを渡せます"),
    );
    AUDIO_LABEL.store(check(
        p,
        target,
        "接続先の音声をこのMacで再生",
        c"sdAudio:",
        &PREFS_CHK_AUDIO,
        228.0,
    ) as usize, Ordering::Relaxed);
    // 再生音量スライダ: 音声転送オン・env で audio が許可・Androidアプリ版相手で
    // ない間だけ sync() が表示する(区切りの線も行の一部として一緒に隠す)
    {
        let gain_divider = divider(p, 188.0) as usize;
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
            196.0,
        );
        *AUDIO_GAIN_VIEWS.lock().unwrap_or_else(|e| e.into_inner()) = vec![
            gain_divider,
            gain_caption as usize,
            AUDIO_GAIN_SLIDER.load(Ordering::Relaxed),
            AUDIO_GAIN_LABEL.load(Ordering::Relaxed),
        ];
    }
    SPEAKER_LABEL.store(check(
        p,
        target,
        // 括弧は音声転送がオフの間だけ(sync() が毎秒揃える。初期値は現在値)
        speaker_caption(false, !crate::audio::MUTED.load(Ordering::Relaxed)),
        c"sdSpkMute:",
        &PREFS_CHK_SPK,
        154.0,
    ) as usize, Ordering::Relaxed);
    check(p, target, "Macのコピーを履歴に残す", c"sdLocalHistory:", &super::PREFS_CHK_HISTORY, 64.0);
    // 説明は核心(選び直し)の 1 文へ短縮し、機密コピーを残さない注記はツールチップ
    // へ退避する(2 文 dotted は幅 540px で否定「残しません」が切れかけだった)
    let history_hint = label(p, "メニューバーの履歴から選び直せます", 40.0, 36.0, 540.0, 12.0, true);
    if !history_hint.is_null() {
        msg1_void_id(
            history_hint,
            sel(c"setToolTip:"),
            nsstring("メニューバーの「クリップボード履歴」から選び直せます。パスワードなどの機密コピーは残しません。"),
        );
    }
    // 最下部の説明は「切断すると元に戻ります」の 1 文で作り、異常終了時の復旧は
    // ツールチップへ(毎秒の sync() が share_hint_and_tooltip で両方を揃える)
    let share_hint = label(p, "切断すると元に戻ります。", 28.0, 116.0, 565.0, 12.0, true);
    if !share_hint.is_null() {
        msg1_void_id(
            share_hint,
            sel(c"setToolTip:"),
            nsstring("切断すると元に戻ります。異常終了したときも、相手側は次回起動時に自動で戻します。"),
        );
    }
    SHARE_HINT.store(share_hint as usize, Ordering::Relaxed);
    SAVE_LABEL.store(
        label(
            cv,
            if UI_PREVIEW.load(Ordering::Relaxed) {
                "デザイン確認モード・設定は保存しません".to_string()
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
    // ⌘W でウィンドウを閉じる経路。メニューバー常駐型はアプリのメインメニューを
    // 持たないため、contentView 直下に keyEquivalent "w"(⌘)のボタンを置く。
    // hidden にすると keyEquivalent ごと無効になるため、サイズ 0 で配置する
    // (見えず・押せず、キーの照合だけに効く)。performClose: は
    // releasedWhenClosed=0 のため閉じても破棄されず、次回の show_prefs で
    // 同じウィンドウが戻る
    let close_btn = msg0(objc_getClass(c"NSButton".as_ptr()), sel(c"new"));
    if !close_btn.is_null() {
        frame(close_btn, NSRect { x: 0.0, y: 0.0, w: 0.0, h: 0.0 });
        msg1_void_id(close_btn, sel(c"setTitle:"), nsstring("設定を閉じる"));
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

/// 切替方式の選択肢と設定値の射影(2件目の改善)。方式の選択が滞在時間・タップ数へ
/// どう落ちるかと、設定値から選択肢へ戻る射影を単体テストで守る
#[cfg(test)]
mod switch_method_tests {
    use super::{delay_for_method, method_index, remember_dwell, LAST_DWELL_MS};
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
            LAST_DWELL_MS.load(Ordering::Relaxed),
        );
        // LAST_DWELL_MS もプロセス共有のため、「未設定なら既定 300ms」の期待の前に
        // 掃いておく(他のテストが記憶した値に期待を壊されないように)
        LAST_DWELL_MS.store(0, Ordering::Relaxed);
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
        LAST_DWELL_MS.store(saved.3, Ordering::Relaxed);
    }

    /// 滞在時間は方式の行き来で失わない: スライダで 700ms へ調整した後、他の方式へ
    /// 移って SWITCH_DELAY_MS が 0 に戻っても、「端で少し待つ」へ戻れば 700ms が
    /// 復元される(黙って既定 300ms へは戻さない)
    #[test]
    fn dwell_survives_method_round_trip() {
        let _serial = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let saved = (
            crate::SWITCH_DELAY_MS.load(Ordering::Relaxed),
            LAST_DWELL_MS.load(Ordering::Relaxed),
        );
        LAST_DWELL_MS.store(0, Ordering::Relaxed);
        // 一度も調整したことがなければ既定 300ms
        assert_eq!(delay_for_method(1, 0), 300, "未調整: 既定 300ms");
        // スライダ(switch_delay)で 700ms へ調整したのと同じ状態(非ゼロ値を記憶)
        remember_dwell(700);
        // 700ms のまま他の方式へ移る → 滞在は 0 に戻る
        assert_eq!(delay_for_method(1, 700), 700, "設定済み: 値を保つ");
        assert_eq!(delay_for_method(2, 700), 0, "ショートカットのみ: 滞在なし");
        // 戻ってきたら 700ms が復元される(300ms へ黙って戻さない)
        assert_eq!(delay_for_method(1, 0), 700, "往復後: 調整済みの 700ms を復元");
        crate::SWITCH_DELAY_MS.store(saved.0, Ordering::Relaxed);
        LAST_DWELL_MS.store(saved.1, Ordering::Relaxed);
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

//! 初回導入は通常の通信・入力フックを起動する前に完了させる。
use super::*;
use knit_common::credentials;

unsafe fn alert(title: &str, body: &str, buttons: &[&str], key: Option<&str>) -> isize {
    let app = msg0(
        objc_getClass(c"NSApplication".as_ptr()),
        sel(c"sharedApplication"),
    );
    msg1_void_i64(app, sel(c"setActivationPolicy:"), 0);
    msg0_void(app, sel(c"finishLaunching"));
    msg1_void_u8(app, sel(c"activateIgnoringOtherApps:"), 1);
    let a = msg0(objc_getClass(c"NSAlert".as_ptr()), sel(c"new"));
    msg1_void_id(a, sel(c"setMessageText:"), nsstring(title));
    msg1_void_id(a, sel(c"setInformativeText:"), nsstring(body));
    for button in buttons {
        let _: ID = crate::msg1_id(a, sel(c"addButtonWithTitle:"), nsstring(button));
    }
    if let Some(key) = key {
        let f: unsafe extern "C" fn(ID, SEL, NSRect) -> ID =
            std::mem::transmute(crate::objc_msgSend as *const () as usize);
        let field = f(
            msg0(objc_getClass(c"NSTextField".as_ptr()), sel(c"alloc")),
            sel(c"initWithFrame:"),
            NSRect {
                x: 0.0,
                y: 0.0,
                w: 440.0,
                h: 60.0,
            },
        );
        msg1_void_id(field, sel(c"setStringValue:"), nsstring(key));
        msg1_void_u8(field, sel(c"setEditable:"), 0);
        msg1_void_u8(field, sel(c"setSelectable:"), 1);
        msg1_void_id(
            field,
            sel(c"setAccessibilityLabel:"),
            nsstring("確認番号またはコード"),
        );
        let font_fn: unsafe extern "C" fn(ID, SEL, f64) -> ID =
            std::mem::transmute(crate::objc_msgSend as *const () as usize);
        let font = font_fn(
            objc_getClass(c"NSFont".as_ptr()),
            sel(c"systemFontOfSize:"),
            36.0,
        );
        msg1_void_id(field, sel(c"setFont:"), font);
        msg1_void_i64(field, sel(c"setAlignment:"), 1);
        msg1_void_u8(field, sel(c"setBezeled:"), 0);
        msg1_void_u8(field, sel(c"setDrawsBackground:"), 0);
        msg1_void_id(a, sel(c"setAccessoryView:"), field);
        msg0_void(field, sel(c"release"));
    }
    PAIR_ALERT.store(a as usize, Ordering::Relaxed);
    let result = crate::msg0_isize(a, sel(c"runModal"));
    PAIR_ALERT.store(0, Ordering::Relaxed);
    msg0_void(a, sel(c"release"));
    msg1_void_i64(app, sel(c"setActivationPolicy:"), 1);
    result
}
pub fn error(message: &str) {
    unsafe {
        alert("接続の準備を完了できません", message, &["閉じる"], None);
    }
}
/// 保存した接続キーが読み取れない時(破損・キーチェーン不通)。読めない登録が
/// 残っている限り再起動しても同じ場所で止まり、復旧UI(設定の初期化)は起動後で
/// しか選べないため、起動を諦める前に初期化と再登録を提案する。
/// 戻り値: true=「初期化して登録し直す」が選ばれた
pub fn confirm_broken_registration_reset() -> bool {
    unsafe {
        alert(
            "保存した登録を読み取れません",
            "保存した接続キーが読み取れないため、Knitを起動できません。\n登録を初期化してもう一度登録し直しますか?\n\n初期化すると、いま登録済みの端末はすべて接続できなくなり、再登録が必要です。\nキーチェーンのアクセス許可が原因の場合は「キャンセル」を選び、許可を確認してから起動し直してください。",
            &["キャンセル", "初期化して登録し直す"],
            None,
        ) == 1001
    }
}
/// 直近の招待画面で相手の登録が完了したか。未完了のまま閉じた時は main が
/// 「再接続を待つ」案内ではなく「端末を登録…」への案内に切り替える
static INVITE_REGISTERED: AtomicBool = AtomicBool::new(false);
pub fn first_run(preview: bool) -> Option<(String, bool)> {
    // 説明だけの画面は挟まず、すぐにコードを見せる。閉じても終了せず、あとで設定「接続」の「端末を登録…」から出せる
    let token = match credentials::generate() {
        Ok(t) => t,
        Err(_) => {
            error("登録の準備ができませんでした。もう一度起動してください。");
            return None;
        }
    };
    if !preview && credentials::save(&token).is_err() {
        error("キーチェーンに保存できませんでした。アクセス許可を確認してください。");
        return None;
    }
    show_invitation(&token, preview, true);
    Some((token, INVITE_REGISTERED.load(Ordering::Relaxed)))
}

struct PairingUi {
    invitation: knit_common::pairing::Invitation,
    /// 招待の有効期限(壁時計)。Invitation 本体と同じ時刻源で、スリープ中も
    /// 延命されない(表示と実体がずれない)
    until: std::time::SystemTime,
    message: String,
    /// 手入力する場合のコード(承認方式が使えない相手・古い版のための代わり)
    code: String,
    /// 初回の登録か。完了文言の2行目(別の端末の登録方法)は初回にだけ出す
    first: bool,
}
/// 相手から届いた「つなぎたい」の要求。待ち受けの画面を閉じて、確認の画面へ渡す
static PENDING: std::sync::Mutex<Option<knit_common::pairing::Approval>> = std::sync::Mutex::new(None);
static PAIRING: std::sync::Mutex<Option<PairingUi>> = std::sync::Mutex::new(None);
static PAIR_ALERT: AtomicUsize = AtomicUsize::new(0);
/// 招待が期限切れ・試行3回で終わった時、poll(タイマー)から show_invitation へ
/// 「新しいコードを作る」提案の文面を渡す。None=終端でない(完了・利用者のクローズ)
static TERMINATED: std::sync::Mutex<Option<(String, String)>> = std::sync::Mutex::new(None);

/// 招待の終端状態。Registered は一定時間だけ結果を読めるように残し、
/// Expired/Locked は待ち受けの画面を閉じて「新しいコードを作る」提案へ移る
#[derive(Debug, PartialEq)]
enum Terminal {
    Registered,
    Expired,
    Locked,
}
impl Terminal {
    /// 終端後に待ち受けの画面へ残す一文(コードは消した上で読める)。
    /// first=初回の登録の時だけ、2台目の登録方法(2行目)を添える
    fn line(&self, first: bool) -> &'static str {
        match self {
            Self::Registered => {
                if first {
                    "登録が完了しました。登録した端末は自動でつながります(AndroidはKnitアプリの「接続を開始」を押してください)。\n別の端末も登録するには、メニューや設定「接続」の「端末を登録…」を再度開きます。"
                } else {
                    "登録が完了しました。登録した端末は自動でつながります(AndroidはKnitアプリの「接続を開始」を押してください)。"
                }
            }
            Self::Expired => "コードの有効期限が切れました。",
            Self::Locked => "3回の接続試行に達しました。",
        }
    }
    /// 再開の提案(2ボタン)を出すか。完了表示は提案しない
    fn reopenable(&self) -> bool {
        matches!(self, Self::Expired | Self::Locked)
    }
    /// 「新しいコードを作る」提案画面のタイトルと本文
    fn offer(&self) -> Option<(&'static str, &'static str)> {
        match self {
            Self::Expired => Some((
                "コードの有効期限が切れました",
                "5分の有効期限が切れたため、このコードでは登録できません。\n新しいコードを作ると、もう一度登録を受け付けます。",
            )),
            Self::Locked => Some((
                "登録の試行回数に達しました",
                "3回の接続試行に達したため、このコードでは登録できません。\n新しいコードを作ると、もう一度登録を受け付けます。",
            )),
            Self::Registered => None,
        }
    }
}
/// 確認の画面(名前・確認番号・許可/拒否)を出している間。待ち受けの更新でその文面を書き換えない
static CONFIRMING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// 登録が完了した時、結果を読める間だけ見せてから自動で閉じる時刻(0=なし)
static CLOSE_AT_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
/// 完了文言を表示する長さ。追加登録(1行のみ)は短く、初回(2台目の案内を
/// 含む)は長めに読ませる
const RESULT_SHOWN_MS: u64 = 3000;
const RESULT_SHOWN_FIRST_MS: u64 = 6000;
unsafe extern "C" fn poll_pairing(_s: ID, _c: SEL, _timer: ID) {
    use knit_common::pairing::Event;
    let close_at = CLOSE_AT_MS.load(Ordering::Relaxed);
    if close_at != 0 && crate::now_ms() >= close_at {
        CLOSE_AT_MS.store(0, Ordering::Relaxed);
        let app = msg0(objc_getClass(c"NSApplication".as_ptr()), sel(c"sharedApplication"));
        msg0_void(app, sel(c"abortModal"));
        return;
    }
    let mut state = PAIRING.lock().unwrap_or_else(|e| e.into_inner());
    let Some(ui) = state.as_mut() else {
        return;
    };
    let a = PAIR_ALERT.load(Ordering::Relaxed) as ID;
    if a.is_null() {
        return;
    }
    let mut terminal = None;
    let confirming = CONFIRMING.load(Ordering::Relaxed);
    while let Ok(event) = ui.invitation.events.try_recv() {
        match event {
            // 別の要求は、確認中は断る(1度に1件だけ確認する)
            Event::Approval(request) if confirming => request.deny(),
            // 依頼した側が去った: いま出している確認の画面を取り下げる。
            // 取り下げられた理由を、戻る待ち受けの画面で読めるようにする
            // (AttemptFailed と同じ導線: ui.message が待ち受け本文へ連結される)
            Event::Withdrawn => {
                if confirming {
                    ui.message = "相手が登録の要求を取り消しました。".into();
                    let app = msg0(objc_getClass(c"NSApplication".as_ptr()), sel(c"sharedApplication"));
                    msg0_void(app, sel(c"abortModal"));
                }
            }
            Event::Registered(_) => {
                INVITE_REGISTERED.store(true, Ordering::Relaxed);
                terminal = Some(Terminal::Registered);
                // 完了文言が2行(初回)か1行(追加)かで、読ませる長さを変える
                let shown_ms = if ui.first {
                    RESULT_SHOWN_FIRST_MS
                } else {
                    RESULT_SHOWN_MS
                };
                CLOSE_AT_MS.store(crate::now_ms() + shown_ms, Ordering::Relaxed);
            }
            Event::Approval(request) => {
                // 待ち受けの画面を閉じ、確認の画面(名前と確認番号)へ移る。
                // 先に CLOSE_AT_MS を 0 へ戻す: 登録完了の自動閉じが予約されたまま
                // 確認へ移ると、タイマーが後続の確認ダイアログごと abortModal して
                // 「拒否」扱いにしてしまう(2台目の登録で発生する)
                CLOSE_AT_MS.store(0, Ordering::Relaxed);
                *PENDING.lock().unwrap_or_else(|e| e.into_inner()) = Some(request);
                let app = msg0(objc_getClass(c"NSApplication".as_ptr()), sel(c"sharedApplication"));
                msg0_void(app, sel(c"abortModal"));
            }
            Event::Expired => terminal = Some(Terminal::Expired),
            Event::Locked => terminal = Some(Terminal::Locked),
            Event::AttemptFailed(left) => ui.message = format!("接続を確認できませんでした。コードを確認してください(残り{left}回)。"),
        }
    }
    // 確認の画面が出ている間は、その文面に触れない(番号と手入力コードが混ざる・許可だけが残るのを防ぐ)
    if confirming {
        return;
    }
    if let Some(t) = terminal {
        msg1_void_id(a, sel(c"setInformativeText:"), nsstring(t.line(ui.first)));
        // Remove the expired/used code immediately; leave the result readable.
        let field = msg0(a, sel(c"accessoryView"));
        if !field.is_null() {
            msg1_void_u8(field, sel(c"setHidden:"), 1);
        }
        msg1_void_id(a, sel(c"setAccessoryView:"), std::ptr::null_mut());
        msg0_void(a, sel(c"layout"));
        *state = None;
        // 期限切れ・試行3回: 待ち受けの画面を閉じて、「新しいコードを作る」提案へ渡す
        if t.reopenable() {
            if let Some((title, body)) = t.offer() {
                *TERMINATED.lock().unwrap_or_else(|e| e.into_inner()) =
                    Some((title.into(), body.into()));
            }
            let app = msg0(objc_getClass(c"NSApplication".as_ptr()), sel(c"sharedApplication"));
            msg0_void(app, sel(c"abortModal"));
        }
        return;
    }
    // 残り時間は招待本体と同じ壁時計の期限から(同じ時刻源を参照する)
    let seconds =
        knit_common::pairing::remaining(ui.until, std::time::SystemTime::now()).as_secs();
    let body = format!("相手の端末でKnitを開くと、このMacを自動で見つけます。\n相手から要求が届いたら、番号と相手のアドレスを確かめて許可してください。\n\n自動で見つからない時の手入力コード：{}\n有効期限：{}分{:02}秒\n{}", ui.code, seconds / 60, seconds % 60, ui.message);
    msg1_void_id(a, sel(c"setInformativeText:"), nsstring(&body));
    msg0_void(a, sel(c"layout"));
}
/// 「〈名前〉とつなぎますか?」を、確認番号つきで尋ねる。既定(Return)は拒否。
/// 番号が相手の画面と同じ時だけ「許可」を押す。心当たりが無い要求は許可しない。
fn confirm_request(request: &knit_common::pairing::Approval) {
    CONFIRMING.store(true, Ordering::Relaxed);
    let allowed = ask_confirmation(&request.name, &request.peer.ip().to_string(), &request.sas);
    CONFIRMING.store(false, Ordering::Relaxed);
    if allowed {
        request.approve();
    } else {
        request.deny();
    }
}
/// 名前は相手が名乗ったもので、確かめられていない。アドレスと番号で判断してもらう。
/// 番号は、相手の画面で「この番号を選ぶ」ために使われる。相手の画面に選択肢として出るのを確かめて許可する。
fn ask_confirmation(name: &str, address: &str, sas: &str) -> bool {
    unsafe {
        alert(
            &format!("「{}」とつなぎますか?", name),
            &format!("相手が名乗った名前です(アドレス: {address})。\n心当たりのある端末なら、相手の画面で下の番号を選んでもらい、「許可」を押してください。"),
            &["拒否", "許可"],
            Some(sas),
        ) == 1001
    }
}
fn show_invitation(token: &str, preview: bool, first: bool) {
    use knit_common::pairing::{Invitation, LIFETIME};
    if preview {
        unsafe {
            // 確認画面だけを見たい時(KNIT_PREVIEW_CONFIRM=1)は、待ち受けの画面を飛ばす
            if std::env::var_os("KNIT_PREVIEW_CONFIRM").is_none() {
            alert("相手の端末とつなぐ", "相手の端末でKnitを開くと、このMacを自動で見つけます。\n相手から要求が届いたら、番号と相手のアドレスを確かめて許可してください。\n\n自動で見つからない時の手入力コード：123 456\nこれは画面確認用です。通信・保存は行いません。", &["閉じる"], None);
            }
            // 相手から要求が届いた時の確認画面(見本)
            let _ = ask_confirmation("もう1台のPC", "192.168.1.20", "4821");
        }
        return;
    }
    let port = std::env::args()
        .collect::<Vec<_>>()
        .windows(2)
        .find(|a| a[0] == "--port")
        .and_then(|a| a[1].parse().ok())
        .unwrap_or(24900);
    unsafe {
        let class_name = c"KnitPairingTarget";
        let mut cls = objc_getClass(class_name.as_ptr());
        if cls.is_null() {
            cls =
                objc_allocateClassPair(objc_getClass(c"NSObject".as_ptr()), class_name.as_ptr(), 0);
            class_addMethod(
                cls,
                sel(c"pollPairing:"),
                poll_pairing as *const () as usize,
                c"v@:@".as_ptr(),
            );
            objc_registerClassPair(cls);
        }
        let target = msg0(cls, sel(c"new"));
        let timer = msg5_timer(
            objc_getClass(c"NSTimer".as_ptr()),
            sel(c"scheduledTimerWithTimeInterval:target:selector:userInfo:repeats:"),
            0.25,
            target,
            sel(c"pollPairing:"),
            std::ptr::null_mut(),
            1,
        );
        let runloop = msg0(objc_getClass(c"NSRunLoop".as_ptr()), sel(c"mainRunLoop"));
        msg2_void_id_id(
            runloop,
            sel(c"addTimer:forMode:"),
            timer,
            nsstring("NSModalPanelRunLoopMode"),
        );
        // alert() publishes the active alert for this timer on the main thread.
        // 期限切れ・試行3回で招待が終わった時は「新しいコードを作る」で作り直す
        loop {
            // 招待を開くたびに「登録完了」はやり直し(前回の結果を持ち越さない)
            INVITE_REGISTERED.store(false, Ordering::Relaxed);
            *TERMINATED.lock().unwrap_or_else(|e| e.into_inner()) = None;
            let invitation = match Invitation::open(token.into(), port) {
                Ok(i) => i,
                Err(_) => {
                    error(
                        "登録の待受を開始できませんでした。別の登録画面が開いていないか確認してください。",
                    );
                    break;
                }
            };
            let code = format!("{} {}", &invitation.code[..3], &invitation.code[3..]);
            *PAIRING.lock().unwrap_or_else(|e| e.into_inner()) = Some(PairingUi {
                invitation,
                until: std::time::SystemTime::now() + LIFETIME,
                message: "この画面を閉じると登録の受付を終了します。".into(),
                code,
                first,
            });
            // 要求が届くと待ち受けの画面が閉じ、確認の画面を出してから、また待ち受けに戻る。
            let mut reopen = false;
            loop {
                alert(
                    "相手の端末とつなぐ",
                    "相手の端末でKnitを開くと、このMacを自動で見つけます。\n相手から要求が届いたら、番号と相手のアドレスを確かめて許可してください。",
                    &["閉じる"],
                    None,
                );
                // 期限切れ・試行3回で招待が終わった時: 設定を開き直さず、この場で作り直せる
                if let Some((title, body)) =
                    TERMINATED.lock().unwrap_or_else(|e| e.into_inner()).take()
                {
                    reopen =
                        alert(&title, &body, &["閉じる", "新しいコードを作る"], None) == 1001;
                    break;
                }
                let request = PENDING.lock().unwrap_or_else(|e| e.into_inner()).take();
                let Some(request) = request else { break };
                confirm_request(&request);
            }
            // 招待を閉じる(利用者のクローズで残っている場合。drop で待受ソケットも閉じ、
            // 次の招待が同じポートを開ける)
            PAIRING.lock().unwrap_or_else(|e| e.into_inner()).take();
            if !reopen {
                break;
            }
        }
        msg0_void(timer, sel(c"invalidate"));
        msg0_void(target, sel(c"release"));
    }
}
pub(super) unsafe extern "C" fn show_registration(_s: ID, _c: SEL, _sender: ID) {
    if UI_PREVIEW.load(Ordering::Relaxed) {
        let _ = first_run(true);
        return;
    }
    let token = crate::envutil::get("KNIT_TOKEN")
        .filter(|t| !t.is_empty())
        .or_else(|| credentials::load().ok().flatten());
    match token {
        Some(token) if credentials::validate_transport_key(&token).is_ok() => show_invitation(&token, false, false),
        // 行き止まりにしない: 登録のやり直し導線は設定「接続」の下にあることを案内
        // する(この登録画面はメニューの「端末を登録…」からも開くため、指す先を
        // 画面名で特定する。初期化は全端末の再登録を要するため、その旨も伝える)
        _ => error("この接続キーでは追加登録を開始できません。既存の接続は保持されています。\n登録をやり直すには、設定「接続」の下の「すべての登録を初期化…」からやり直せます(全端末の再登録が必要になります)。"),
    }
}

/// 設定「接続」の「すべての登録を初期化…」。確認のうえでキーチェーンの接続キーを
/// 削除して作り直す。接続キーは全端末で共通(共有鍵)のため、旧いキーを持つ相手は
/// 全員つながらなくなり、再登録まで戻れない(=締め出し)。取り消しはできないため、
/// 既定(Return)は「キャンセル」にして、失敗経路では必ず結果を案内する
pub(super) unsafe extern "C" fn reset_registration(_s: ID, _c: SEL, _sender: ID) {
    if UI_PREVIEW.load(Ordering::Relaxed) {
        return;
    }
    // env の KNIT_TOKEN(直接つなぐ)はキーチェーンより優先され、この初期化では
    // 消せない。誤って「初期化したのに締め出せない」状態を作らないため、ここで
    // 止めて、通常の登録へ戻す導線(設定「接続」のトークンを空欄にして保存)へ案内する
    if crate::envutil::get("KNIT_TOKEN").is_some_and(|t| !t.is_empty()) {
        error("手動接続(共通トークン)で運用中のため、画面からは初期化できません。\n設定「接続」の「直接つなぐ(手動接続)」のトークンを空欄にして「保存して再接続」を押すと、通常の登録へ戻せます。\n(起動時の環境変数 KNIT_TOKEN として設定している場合は、設定元のスクリプトなどで外してください。「設定フォルダを開く」で ~/.config/knit を開けます)");
        return;
    }
    let confirmed = alert(
        "すべての登録を初期化しますか?",
        "このMacの接続キーを削除して、新しいキーを作り直します。\nいま登録済みの端末はすべて接続できなくなり、使うには再登録が必要です(接続中の相手も切断されます)。\n端末の配置と名前の保存(peer-sides.json)も消えます。\n\nこの操作は取り消せません。",
        &["キャンセル", "初期化"],
        None,
    );
    if confirmed != 1001 {
        return;
    }
    // peer-sides.json(端末の配置・名前・登録済み台数表示の保存元)も消す: キーだけ
    // 作り直しても残っていると、初期化後の画面に「登録済み: N台」が旧いまま出て
    // 初期化が済んでいないように見える。履歴は対象外(「履歴を消す」が担う)。
    // 無いファイルは素通り(reset_all_settings と同じ冪等パターン)。先に消すのは、
    // キー削除より後で失敗すると「登録は変更していません」の案内が嘘になるため
    if let Some(dir) = knit_common::envutil::config_dir() {
        let target = dir.join("peer-sides.json");
        if target.exists() && std::fs::remove_file(&target).is_err() {
            error("端末の配置の保存(peer-sides.json)を削除できませんでした。\n登録は変更していません。\n設定フォルダ(~/.config/knit)の権限を確認してください");
            return;
        }
    }
    if let Err(e) = credentials::delete() {
        error(&format!("キーチェーンから接続キーを削除できませんでした。\n登録は変更していません。\n{e}"));
        return;
    }
    let Ok(token) = credentials::generate() else {
        error("接続キーを削除しましたが、新しいキーを作成できませんでした。\nKnitを再起動すると、初回登録の画面からやり直せます。");
        return;
    };
    if credentials::save(&token).is_err() {
        error("新しい接続キーをキーチェーンに保存できませんでした。\nKnitを再起動すると、初回登録の画面からやり直せます。");
        return;
    }
    eprintln!("[setup] すべての登録を初期化しました(接続キーを作り直し)");
    alert(
        "登録をすべて初期化しました",
        "Knitを再起動すると、新しい接続キーで動き始めます。旧いキーを持つ端末は接続できません。\n使いたい端末は、設定「接続」の「端末を登録…」から再度登録してください。",
        &["再起動する"],
        None,
    );
    restart_now();
}

/// 「すべての設定を初期化…」の確認本文(純粋関数・単体テストで守る)。
/// manual_token=直接つなぐ(env の KNIT_TOKEN)運用中: この初期化で env の
/// KNIT_TOKEN 行も消えるため、黙って通常の登録へ戻らないことを本文へ 1 行足す
fn reset_all_settings_body(manual_token: bool) -> String {
    let mut body = "このMacの設定をすべて消して、最初の状態に戻します。\n対象: 設定(preferences.json)・端末の配置と名前(peer-sides.json)・envファイルの KNIT_* 行\n履歴と端末の登録は消えません(それぞれ専用の操作があります)。".to_string();
    if manual_token {
        body.push_str("\n直接つなぐ(手動接続)の設定も解除され、通常の登録が必要になります。");
    }
    body.push_str(
        "\n\n環境変数として設定されている KNIT_* は消えません(設定元のスクリプト側で外してください)。\nこの操作は取り消せません。",
    );
    body
}

/// 設定画面の左下「その他」の「すべての設定を初期化…」。preferences.json・peer-sides.json・
/// env ファイルの KNIT_* 行を消して、出荷状態へ戻す。**履歴と端末の登録は消さない**
/// (履歴は「履歴を消す」、登録は「すべての登録を初期化…」と操作を分離している)。
/// 環境変数として設定された KNIT_* は起動スクリプト由来のためここからは消えない
/// (ダイアログに注記する)。取り消せないため既定(Return)は「キャンセル」
pub(super) unsafe extern "C" fn reset_all_settings(_s: ID, _c: SEL, _sender: ID) {
    if UI_PREVIEW.load(Ordering::Relaxed) {
        return;
    }
    // 直接つなぐ(KNIT_TOKEN)運用中は、この初期化で env のトークン行が消えて
    // 通常の登録へ戻る。確認本文にその行を足して知らせる(黙って解除されない)
    let manual_token = crate::envutil::get("KNIT_TOKEN").is_some_and(|t| !t.is_empty());
    let confirmed = alert(
        "すべての設定を初期化しますか?",
        &reset_all_settings_body(manual_token),
        &["キャンセル", "初期化"],
        None,
    );
    if confirmed != 1001 {
        return;
    }
    let Some(dir) = knit_common::envutil::config_dir() else {
        error("ホームが取得できないため、設定を初期化できませんでした");
        return;
    };
    let mut failures: Vec<String> = Vec::new();
    for name in ["preferences.json", "peer-sides.json"] {
        let target = dir.join(name);
        // 無いファイルは素通り(初期化の冪等性。部分的に済んでいる状態からの再試行も通る)
        if target.exists() {
            if let Err(e) = std::fs::remove_file(&target) {
                failures.push(format!("{name}: {e}"));
            }
        }
    }
    let env_path = dir.join("env");
    if let Err(e) = knit_common::envutil::clear_knit_lines(&env_path) {
        failures.push(format!("env: {e}"));
    }
    if !failures.is_empty() {
        error(&format!(
            "一部の設定を初期化できませんでした。\n{}\nログを確認してください",
            failures.join("\n")
        ));
        return;
    }
    // ここから再起動までは設定を保存させない: メモリには旧設定が残っているため、
    // この間の保存(毎秒の sync()・相手からの遠隔適用)が消した preferences.json を
    // 書き戻し、初期化を黙って取り消す。再起動に失敗した時は restart_now が戻す
    RESTARTING.store(true, Ordering::Relaxed);
    eprintln!("[setup] すべての設定を初期化しました(設定ファイルと env の KNIT_* 行を削除)");
    alert(
        "設定を初期化しました",
        "Knitを再起動すると、最初の状態で起動します。\n履歴と端末の登録はそのまま残っています。",
        &["再起動する"],
        None,
    );
    restart_now();
}

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXIsProcessTrusted() -> bool;
}
unsafe extern "C" fn poll_trust(_s: ID, _c: SEL, _timer: ID) {
    if AXIsProcessTrusted() {
        let app = msg0(objc_getClass(c"NSApplication".as_ptr()), sel(c"sharedApplication"));
        msg0_void(app, sel(c"abortModal"));
    }
}
/// `f`(モーダルの画面)を表示している間、許可されたかを見張り、許可されたら画面を自動で閉じる
unsafe fn watch_trust<R>(f: impl FnOnce() -> R) -> R {
    let class_name = c"KnitTrustTarget";
    let mut cls = objc_getClass(class_name.as_ptr());
    if cls.is_null() {
        cls = objc_allocateClassPair(objc_getClass(c"NSObject".as_ptr()), class_name.as_ptr(), 0);
        class_addMethod(cls, sel(c"pollTrust:"), poll_trust as *const () as usize, c"v@:@".as_ptr());
        objc_registerClassPair(cls);
    }
    let target = msg0(cls, sel(c"new"));
    let timer = msg5_timer(
        objc_getClass(c"NSTimer".as_ptr()),
        sel(c"scheduledTimerWithTimeInterval:target:selector:userInfo:repeats:"),
        0.5,
        target,
        sel(c"pollTrust:"),
        std::ptr::null_mut(),
        1,
    );
    let runloop = msg0(objc_getClass(c"NSRunLoop".as_ptr()), sel(c"mainRunLoop"));
    msg2_void_id_id(runloop, sel(c"addTimer:forMode:"), timer, nsstring("NSModalPanelRunLoopMode"));
    let result = f();
    msg0_void(timer, sel(c"invalidate"));
    msg0_void(target, sel(c"release"));
    result
}
/// 権限確認で「あとで」が選ばれて起動を中断する時: 何も言わずに終了せず、
/// 再開方法を案内してから終了する(接続キー・設定は保存済み)
pub fn permission_postponed() {
    unsafe {
        alert(
            "アクセシビリティ権限がまだ許可されていません",
            "Knitを終了します。接続キーと設定は保存されています。\nもう一度Knit.appを開くと、ここから再開できます。",
            &["閉じる"],
            None,
        );
    }
}
pub fn ensure_permission() -> bool {
    unsafe {
        if AXIsProcessTrusted() {
            return true;
        }
        let choice = watch_trust(|| alert(
            "このMacから操作する準備",
            "Knitで相手の端末を操作するには、Macのアクセシビリティ権限が必要です。\n許可すると、自動で次へ進みます。",
            &["システム設定を開く", "あとで"],
            None,
        ));
        if AXIsProcessTrusted() {
            return true;
        }
        if choice != 1000 {
            return false;
        }
        let url = crate::msg1_id(objc_getClass(c"NSURL".as_ptr()), sel(c"URLWithString:"), nsstring("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility"));
        let workspace = msg0(objc_getClass(c"NSWorkspace".as_ptr()), sel(c"sharedWorkspace"));
        let open: unsafe extern "C" fn(ID, SEL, ID) -> u8 = std::mem::transmute(crate::objc_msgSend as *const () as usize);
        open(workspace, sel(c"openURL:"), url);
        // 許可を待つ。オンにすると、この画面は自動で閉じる
        watch_trust(|| alert(
            "Knitをオンにしてください",
            "Knitをオンにしてください。\nオンにすると、この画面は自動で閉じて、つながります。",
            &["あとで"],
            None,
        ));
        AXIsProcessTrusted()
    }
}

/// Debug-only end-to-end UI probe: ephemeral credential, no OS-store access.
#[cfg(debug_assertions)]
pub fn probe_invitation() {
    if let Ok(token) = credentials::generate() {
        show_invitation(&token, false, true);
    }
}

#[cfg(test)]
mod termination_tests {
    use super::Terminal;

    /// 期限切れ・試行3回のときだけ「新しいコードを作る」提案を出す。
    /// 完了(Registered)は提案しない: 再開の必要がないため
    #[test]
    fn only_expired_and_locked_offer_reopening() {
        assert!(Terminal::Expired.reopenable(), "期限切れは作り直せる");
        assert!(Terminal::Locked.reopenable(), "試行3回も作り直せる");
        assert!(!Terminal::Registered.reopenable(), "完了後に提案は出さない");
        assert!(Terminal::Registered.offer().is_none());
    }

    /// 提案の文案はどちらも、理由と「新しいコードを作ると再開できる」を含む。
    /// 旧文言の「設定を開き直してください」は案内が長く、この場で再開できない
    #[test]
    fn offers_explain_the_reason_and_the_way_to_resume() {
        for (terminal, word) in [
            (&Terminal::Expired, "有効期限"),
            (&Terminal::Locked, "試行回数"),
        ] {
            let (title, body) = terminal.offer().unwrap();
            assert!(title.contains(word), "タイトルに理由({word})が出る: {title}");
            assert!(
                body.contains("このコードでは登録できません"),
                "このコードの終わりを明示する: {body}"
            );
            assert!(
                body.contains("新しいコードを作ると"),
                "再開方法を示す: {body}"
            );
        }
    }

    /// 完了文言の2行目(別の端末の登録方法)は初回のみ。追加登録では1行目だけを
    /// 出す(すでに登録の仕方を知っている人に2台目の勧誘は要らない)
    #[test]
    fn registered_line_guides_adding_another_device() {
        let line = Terminal::Registered.line(true);
        assert!(line.contains("登録が完了しました"), "完了を伝える: {line}");
        assert!(
            line.contains("端末を登録…"),
            "初回は2台目を「端末を登録…」から案内する: {line}"
        );
        let again = Terminal::Registered.line(false);
        assert!(
            again.contains("登録が完了しました"),
            "追加登録でも完了は伝える: {again}"
        );
        assert!(
            !again.contains("端末を登録…"),
            "追加登録では2台目の案内は出さない: {again}"
        );
    }
}

#[cfg(test)]
mod reset_all_settings_tests {
    use super::reset_all_settings_body;

    /// 直接つなぐ(KNIT_TOKEN)運用中の確認本文: トークン行も消えて通常の登録が
    /// 必要になることを 1 行で伝える(黙って解除されるのを防ぐ)
    #[test]
    fn manual_token_operation_is_called_out_in_body() {
        let body = reset_all_settings_body(true);
        assert!(
            body.contains("直接つなぐ(手動接続)の設定も解除され、通常の登録が必要になります"),
            "トークン運用中の解除と再登録を本文へ出す: {body}"
        );
    }

    /// 通常運用の確認本文にトークンの行は出ない(無関係の注意で読ませない)。
    /// 対象・消えないもの・環境変数の注記は条件に関係なく残る
    #[test]
    fn body_without_token_keeps_common_wording_only() {
        let body = reset_all_settings_body(false);
        assert!(
            !body.contains("直接つなぐ"),
            "トークン未運用では解除の案内は出ない: {body}"
        );
        for common in [
            "preferences.json",
            "peer-sides.json",
            "履歴と端末の登録は消えません",
            "環境変数として設定されている KNIT_* は消えません",
            "この操作は取り消せません",
        ] {
            assert!(body.contains(common), "共通の本文が欠けている: {common}\n{body}");
        }
    }
}

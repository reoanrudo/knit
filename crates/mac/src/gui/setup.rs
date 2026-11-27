//! 初回導入は通常の通信・入力フックを起動する前に完了させる。
use super::*;
use knit_common::credentials;

pub(super) unsafe fn alert(title: &str, body: &str, buttons: &[&str], key: Option<&str>) -> isize {
    let app = msg0(
        objc_getClass(c"NSApplication".as_ptr()),
        sel(c"sharedApplication"),
    );
    msg1_void_i64(app, sel(c"setActivationPolicy:"), 0);
    msg0_void(app, sel(c"finishLaunching"));
    msg1_void_u8(app, sel(c"activateIgnoringOtherApps:"), 1);
    let a = msg0(objc_getClass(c"NSAlert".as_ptr()), sel(c"new"));
    msg1_void_id(a, sel(c"setMessageText:"), nsstring(&localization::copy(title)));
    msg1_void_id(a, sel(c"setInformativeText:"), nsstring(&localization::copy(body)));
    for button in buttons {
        let _: ID = crate::msg1_id(a, sel(c"addButtonWithTitle:"), nsstring(&localization::copy(button)));
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
            nsstring(&localization::copy("確認番号またはコード")),
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
    let result = crate::msg0_isize(a, sel(c"runModal"));
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
    // Create and store the connection key, then walk through setup in the setup window.
    // Closing the window does not quit; pairing can be reopened from Settings > Connection.
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
    let (_, registered) = websetup::run(websetup::Mode::First, Some(token.clone()), preview);
    INVITE_REGISTERED.store(registered, Ordering::Relaxed);
    Some((token, registered))
}

pub(super) unsafe extern "C" fn show_registration(_s: ID, _c: SEL, _sender: ID) {
    if UI_PREVIEW.load(Ordering::Relaxed) {
        let _ = websetup::run(websetup::Mode::Pair, None, true);
        return;
    }
    let token = crate::envutil::get("KNIT_TOKEN")
        .filter(|t| !t.is_empty())
        .or_else(|| credentials::load().ok().flatten());
    match token {
        Some(token) if credentials::validate_transport_key(&token).is_ok() => {
            let _ = websetup::run(websetup::Mode::Pair, Some(token), false);
        }
        // Do not dead-end: point to the reset in Settings > Connection (re-pairing every device)
        _ => error("この接続キーでは追加登録を開始できません。既存の接続は保持されています。\n登録をやり直すには、Settings の Connection にある「すべての登録を初期化…」を使います(全端末の再登録が必要になります)。"),
    }
}

/// 設定サイドバー「その他」の「すべての登録を初期化…」。確認のうえでキーチェーンの
/// 接続キーを削除して作り直す。接続キーは全端末で共通(共有鍵)のため、旧いキーを
/// 持つ相手は全員つながらなくなり、再登録まで戻れない(=締め出し)。取り消しは
/// できないため、既定(Return)は「キャンセル」にして、失敗経路では必ず結果を案内する
pub(super) unsafe extern "C" fn reset_registration(_s: ID, _c: SEL, _sender: ID) {
    if UI_PREVIEW.load(Ordering::Relaxed) {
        return;
    }
    // env の KNIT_TOKEN(直接つなぐ)はキーチェーンより優先され、この初期化では
    // 消せない。誤って「初期化したのに締め出せない」状態を作らないため、ここで
    // 止めて、通常の登録へ戻す導線(env の KNIT_TOKEN 行を消す)へ案内する
    if crate::envutil::get("KNIT_TOKEN").is_some_and(|t| !t.is_empty()) {
        error("手動接続(共通トークン)で運用中のため、画面からは初期化できません。\n~/.config/knit/env の KNIT_TOKEN 行を消して Knitを再起動すると、通常の登録へ戻せます。\n(起動時の環境変数 KNIT_TOKEN として設定している場合は、設定元のスクリプトなどで外してください)");
        return;
    }
    let confirmed = alert(
        "すべての登録を初期化しますか?",
        "このPCの接続キーを削除して、新しいキーを作り直します。\nいま登録済みの端末はすべて接続できなくなり、使うには再登録が必要です(接続中の相手も切断されます)。\n端末の配置と名前の保存(peer-sides.json)も消えます。\n\nこの操作は取り消せません。",
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
    // 登録台帳も空にする(消し忘れると、新しい master で二度と繋がらない
    // ゴーストエントリが「登録済み N台」として残り、台帳の上限を食う)
    if let Some(reg) = crate::registry() {
        if let Err(e) = reg.clear() {
            eprintln!("[setup] 登録台帳の初期化に失敗しました: {e}(再起動後にやり直します)");
        }
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
    let mut body = "このPCの設定をすべて消して、最初の状態に戻します。\n対象: 設定(preferences.json / desktop-settings.json)・端末の配置と名前(peer-sides.json)・感度と境界範囲(screen-settings.json)・envファイルの KNIT_* 行\n履歴と端末の登録は消えません(それぞれ専用の操作があります)。".to_string();
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
    for name in ["preferences.json", "peer-sides.json", "screen-settings.json", "desktop-settings.json"] {
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
#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXIsProcessTrusted() -> bool;
}

/// Accessibility is required before the input tap starts. When it is missing, show the
/// permission step of the setup window; it advances by itself once Knit is switched on.
pub fn ensure_permission() -> bool {
    unsafe {
        if AXIsProcessTrusted() {
            return true;
        }
        let _ = websetup::run(websetup::Mode::Permission, None, false);
        AXIsProcessTrusted()
    }
}

/// Debug-only end-to-end UI probe: ephemeral credential, no OS-store access.
#[cfg(debug_assertions)]
pub fn probe_invitation() {
    if let Ok(token) = credentials::generate() {
        let _ = websetup::run(websetup::Mode::Pair, Some(token), false);
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

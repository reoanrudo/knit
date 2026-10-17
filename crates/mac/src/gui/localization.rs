//! 表示言語はこの Mac だけの設定。通信・保存キーや利用者の端末名は翻訳しない。
use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Language {
    Japanese,
    English,
}

impl Language {
    pub(super) fn parse(code: &str) -> Option<Self> {
        match code {
            "ja" => Some(Self::Japanese),
            "en" => Some(Self::English),
            _ => None,
        }
    }

    pub(super) fn code(self) -> &'static str {
        match self {
            Self::Japanese => "ja",
            Self::English => "en",
        }
    }

    pub(super) fn pick<'a>(self, ja: &'a str, en: &'a str) -> &'a str {
        match self {
            Self::Japanese => ja,
            Self::English => en,
        }
    }
}

static LANGUAGE: AtomicU8 = AtomicU8::new(0);

pub(super) fn language() -> Language {
    if LANGUAGE.load(Ordering::Relaxed) == 1 {
        Language::English
    } else {
        Language::Japanese
    }
}

pub(super) fn set_language(value: Language) {
    LANGUAGE.store((value == Language::English) as u8, Ordering::Relaxed);
}

pub(super) fn text<'a>(ja: &'a str, en: &'a str) -> &'a str {
    language().pick(ja, en)
}

// 静的コピーの対を一か所に置く。UI は元の日本語を保持して言語変更時に再適用する。
const COPY: &[(&str, &str)] = &[
    ("元に戻す", "Revert"),
    ("適用", "Apply"),
    ("診断", "Diagnostics"),
    ("端末を追加…", "Add Device…"),
    ("手動接続", "Manual Connection"),
    ("このMacで待ち受ける", "Listen on this Mac"),
    ("相手へ接続する", "Connect to another device"),
    ("画面を識別", "Identify Displays"),
    ("数値で指定", "Edit Values"),
    ("ポインター感度", "Pointer Sensitivity"),
    ("標準に戻す", "Reset"),
    ("Windows音声", "Windows Audio"),
    ("コピーとファイル", "Clipboard & Files"),
    ("Macで再生", "Play on Mac"),
    ("音量", "Volume"),
    ("出力先", "Output"),
    ("システム既定", "System Default"),
    ("未接続の出力先", "Unavailable Output"),
    ("Windowsスピーカーを消音", "Mute Windows Speakers"),
    ("出力を確認…", "Test Output…"),
    ("コピー", "Clipboard"),
    ("ファイル", "Files"),
    ("受信フォルダ", "Receive Folder"),
    ("変更…", "Change…"),
    ("開く", "Open"),
    ("コピーを保存", "Save Copies"),
    ("受信後にフォルダを開く", "Open the Folder After Receiving"),
    ("保存件数", "History Limit"),
    ("診断中…", "Running…"),
    ("履歴を消去…", "Clear History…"),
    ("10件", "10"),
    ("50件", "50"),
    ("100件", "100"),
    ("200件", "200"),
    ("一般", "General"),
    ("言語", "Language"),
    ("ログイン時に起動", "Launch at Login"),
    ("通知", "Notifications"),
    ("接続・切断", "Connections"),
    ("転送完了", "Completed Transfers"),
    ("書き出す…", "Export…"),
    ("読み込む…", "Import…"),
    ("ログ", "Logs"),
    ("保存…", "Save…"),
    ("再起動…", "Restart…"),
    ("初期化…", "Reset…"),
    ("再接続", "Reconnect"),
    ("Knit設定", "Knit Settings"),
    ("設定", "SETTINGS"),
    ("接続", "Connection"),
    ("画面配置", "Displays"),
    ("切替", "Switching"),
    ("共有", "Sharing"),
    ("その他", "General"),
    ("端末どうしをつなぎ、いまの状態を確かめます。", "Connect your devices. Know where you stand."),
    ("画面の並びを、机の上の配置と揃えます。", "Arrange your displays as they sit on your desk."),
    ("画面を越える方法を、自分の操作に合わせます。", "Choose how you move between devices."),
    ("境界を越えて渡すものと、受け取るもの。", "Choose what travels between your devices."),
    ("表示言語と、アプリのメンテナンス。", "Language and app maintenance."),
    ("接続の方向", "Connection direction"),
    ("このPCがホスト(既定) — 相手の端末が接続しに来ます", "This Mac is the host (default) — other devices connect here"),
    ("相手の端末がホスト — このPCが接続しに行きます", "The other device is the host — this Mac connects to it"),
    ("接続状態", "Connection status"),
    ("接続を確認しています…", "Checking the connection…"),
    ("端末を登録…", "Pair a Device…"),
    ("接続先の端末", "Device address"),
    ("接続先の端末のアドレス", "Other device’s IP address"),
    ("192.168.1.23(空欄で自動発見)", "192.168.1.23 (leave blank to discover)"),
    ("保存して再接続", "Save & Reconnect"),
    ("接続先を保存してから、Knitを再起動して反映します。", "Saves the address, then restarts Knit to reconnect."),
    ("切替キー", "Switching key"),
    ("F6(必要に応じてfnと併用)", "F6 (with fn if needed)"),
    ("F8(必要に応じてfnと併用)", "F8 (with fn if needed)"),
    ("F13(既定)", "F13 (default)"),
    ("右⌘", "Right ⌘"),
    ("ほかのキー…", "Other Key…"),
    ("切替キーのキーコード(HID)", "Switching-key code (HID)"),
    ("例: 97", "e.g. 97"),
    ("キーコード(HID)を1〜127の数値で入力してください。主なキー: F6=97・F8=100・F13=105(既定)・F14=107・右⌘=54", "Enter a HID key code from 1 to 127. Common keys: F6=97, F8=100, F13=105 (default), F14=107, Right ⌘=54."),
    ("登録", "Register"),
    ("キーコードは1〜127の数値で指定してください", "Enter a key code between 1 and 127."),
    ("環境変数 KNIT_HOTKEY_KC で固定中のため変更できません", "Fixed by KNIT_HOTKEY_KC. Remove the override and restart to change it here."),
    ("キー変換", "Key Translation"),
    ("Windows 操作中の Mac 流キー操作の対応です。⌘の対応先は環境変数 KNIT_CMD_ALT と設定(cmd_alt)で Ctrl/Alt に切り替わります。右⌘は常に右Ctrlです", "How Mac-style keys map while controlling Windows. KNIT_CMD_ALT and the stored cmd_alt setting switch ⌘ between Ctrl and Alt. Right ⌘ is always Right Ctrl."),
    ("更新を自動で確認", "Check for Updates Automatically"),
    ("名前…", "Name…"),
    ("端末の名前", "Device Name"),
    ("例: 書斎のWindows", "e.g. Study PC"),
    ("空欄で確定すると名前を削除し、コンピュータ名へ戻ります", "Leave the field blank to remove the name and use the computer’s name."),
    ("変更", "Change"),
    ("プレビューでは名前を変更しません", "Device names are not changed in preview"),
    ("滞在時間   短い / 長い", "Pause duration"),
    ("切替キーは、どの方式でも使えます。", "The switching key works with every method."),
    ("渡すもの", "What you share"),
    ("テキストと画像", "Text and images"),
    ("ファイルの受け渡し", "Files"),
    ("コピーした内容を、もう1台でも貼り付けられます", "Paste copied content on your other device."),
    ("コピーしたファイルや、掴んだファイルを渡せます", "Transfer copied files, or drag them across the boundary."),
    ("音声", "Audio"),
    ("接続先の音声をこのPCで再生", "Play the other device’s audio on this Mac"),
    ("音声共有(Androidアプリ版は未対応)", "Audio sharing (unavailable in the Android app)"),
    ("再生音量   小さい / 大きい", "Playback volume"),
    ("履歴", "Clipboard history"),
    ("このPCのコピーを履歴に残す", "Keep this Mac’s copies in history"),
    ("メニューバーの履歴から選び直せます", "Retrieve earlier copies from the menu bar."),
    ("履歴は平文で保存されます。残したくない場合はオフにしてください。", "History is stored unencrypted. Turn it off if you prefer not to retain copies."),
    ("表示言語", "Interface language"),
    ("言語を変えても、接続や入力中の内容はそのままです。", "Changing language keeps your connection and unsaved input intact."),
    ("メンテナンス", "Maintenance"),
    ("再起動", "Restart"),
    ("Knitを再起動", "Restart Knit"),
    ("初期化", "Reset"),
    ("端末の登録", "Device pairing"),
    ("すべての登録を初期化…", "Reset All Pairings…"),
    ("すべての設定", "All settings"),
    ("すべての設定を初期化…", "Reset All Settings…"),
    ("初期化すると全端末が締め出され、再登録が必要です(取り消せません)。特定の1台だけ外すには、初期化後に必要な端末を再登録します", "Revokes access for every paired device. Pair the devices you want to keep again. This cannot be undone."),
    ("設定を閉じる", "Close Settings"),
    ("設定画面のプレビュー", "Settings preview"),
    ("デザイン確認モード・設定は保存しません", "Design preview — changes are not saved"),
    ("スイッチ・スライダーは自動保存", "Switches and sliders save automatically"),
    ("変更を保存しました", "Changes saved"),
    ("設定を保存できません。ログを確認してください", "Could not save settings. Check the log."),
    ("ホームが取得できず接続先を保存できませんでした", "Could not save the address: the home directory is unavailable."),
    ("接続先の指定を外しました(自動発見へ戻します)。Knitを再起動します", "Address cleared. Restarting Knit with automatic discovery."),
    ("環境変数 KNIT_ROLE で役割を固定しています。切り替えるには KNIT_ROLE の指定を外して再起動してください", "KNIT_ROLE fixes the connection direction. Remove that override and restart Knit to change it."),
    ("接続の方向を切り替えますか?", "Change the connection direction?"),
    ("両方の端末で接続を切り、役割を入れ替えて自動で再接続します。進行中のファイル転送は中止されます。", "Both devices will disconnect, exchange roles, and reconnect automatically. Any file transfer in progress will be cancelled."),
    ("キャンセル", "Cancel"),
    ("ファイル転送中です", "Transfer in Progress"),
    ("中止して終了", "Cancel Transfer and Quit"),
    ("切り替える", "Change Direction"),
    ("グレー：Mac／ブルー：接続先(各モニターに機種名)。ドラッグで配置(斜めも可)", "Mac in gray. Other devices in blue. Drag to arrange, including diagonally."),
    ("クリックで操作する端末を選べます・Macの上へ戻すと全画面の端になります", "Click to select a device. Drop onto the Mac to use the edge of all displays."),
    ("ブロックのクリックで操作する端末を選べます。Macの上へ戻すと「全画面の端(モニター指定なし)」になります。保存済みの端末は各端末の設定が優先されます", "Click a block to select the device to control. Drop it onto the Mac to use the edge of all displays. Saved per-device arrangements take priority."),
    ("環境変数 KNIT_ROLE で役割を固定中のため、ここでは切り替えられません", "KNIT_ROLE fixes the connection direction. Remove the override to change it."),
    ("切り替えを相手へ送りました。相手の適用を確認しています…", "Change sent. Waiting for the other device to confirm…"),
    ("相手の適用を確認しました。接続を張り直します…", "Change confirmed. Reconnecting…"),
    ("相手の適用確認が取れないため、時間経過で接続を張り直します…", "No confirmation received. Reconnecting after the timeout…"),
    ("切り替えると、両方の端末で接続を切り、役割を入れ替えて自動で再接続します", "Changing direction disconnects both devices, swaps roles, and reconnects."),
    ("接続済み", "Connected"),
    ("100%(既定)", "100% (default)"),
    ("300ms(既定)", "300ms (default)"),
    ("切断すると元に戻ります。", "Settings are restored when you disconnect."),
    ("切断すると元に戻ります。異常終了したときも、相手側は次回起動時に自動で戻します。", "Settings are restored when you disconnect. After an unexpected exit, the other device restores them on its next launch."),
    ("ファイルはDownload/Knitへ保存します。日本語はタブレット操作中に自動で出る入力欄から送れます。", "Files go to Download/Knit. Use the automatic text field for Japanese input."),
    ("ファイルはDownloadへ保存します。タブレットの音はそのまま残ります。", "Files go to Download. Audio still plays on the tablet."),
    ("環境変数 KNIT_SHARE で制限中のため、一部の項目は変更できません。", "KNIT_SHARE limits sharing. Some options cannot be changed here."),
    ("起動時は環境変数の値でこの画面の設定を上書きします。初期化は設定画面の左下の「その他」からできます", "Environment overrides take priority at startup. Reset options are in General."),
    ("Androidアプリ:画面操作・キーボードを許可済み", "Android app: screen control and keyboard permitted"),
    ("Androidアプリ:文字入力にはKnitキーボードを選択してください", "Android app: select the Knit keyboard to type"),
    ("Androidアプリ:タブレットで画面操作を許可してください", "Android app: allow screen control on the tablet"),
    ("メインモニター", "Main display"),
    ("全画面", "All displays"),
    ("全画面の端へ", "Use all display edges"),
    ("未接続", "Not connected"),
    ("操作中", "Active"),
    ("端末", "Device"),
    ("確認番号またはコード", "Verification number or code"),
    ("閉じる", "Close"),
    ("許可", "Allow"),
    ("拒否", "Decline"),
    ("新しいコードを作る", "Create a New Code"),
    ("相手の端末とつなぐ", "Pair with Another Device"),
    ("LAN 直", "Direct LAN"),
    ("タブレット", "Tablet"),
    ("接続の準備を完了できません", "Could Not Prepare the Connection"),
    ("すべての登録を初期化しますか?", "Reset All Device Pairings?"),
    ("このPCの接続キーを削除して、新しいキーを作り直します。\nいま登録済みの端末はすべて接続できなくなり、使うには再登録が必要です(接続中の相手も切断されます)。\n端末の配置と名前の保存(peer-sides.json)も消えます。\n\nこの操作は取り消せません。", "Replaces this Mac’s connection key.\nEvery paired device will lose access and must be paired again. Connected devices will disconnect.\nSaved device arrangements and names (peer-sides.json) are also removed.\n\nThis cannot be undone."),
    ("すべての設定を初期化しますか?", "Reset All Settings?"),
    ("登録をすべて初期化しました", "All Pairings Reset"),
    ("Knitを再起動すると、新しい接続キーで動き始めます。旧いキーを持つ端末は接続できません。\n使いたい端末は、設定「接続」の「端末を登録…」から再度登録してください。", "Restart Knit to use the new connection key. Devices with the old key can no longer connect.\nPair the devices you want to use from Connection → Pair a Device."),
    ("設定を初期化しました", "Settings Reset"),
    ("Knitを再起動すると、最初の状態で起動します。\n履歴と端末の登録はそのまま残っています。", "Restart Knit to use the default settings.\nClipboard history and device pairings have been retained."),
    ("再起動する", "Restart"),
    ("この画面を閉じると登録の受付を終了します。", "Closing this window stops accepting pairing requests."),
    ("相手が登録の要求を取り消しました。", "The other device withdrew its pairing request."),
    ("相手の端末でKnitを開くと、このPCを自動で見つけます。\n相手から要求が届いたら、番号と相手のアドレスを確かめて許可してください。", "Open Knit on your other device to discover this Mac.\nWhen a request arrives, check the number and device address before allowing it."),
    ("登録が完了しました。登録した端末は自動でつながります(AndroidはKnitアプリの「接続を開始」を押してください)。", "Pairing complete. Your device will connect automatically. On Android, choose Start Connection in the Knit app."),
    ("登録が完了しました。登録した端末は自動でつながります(AndroidはKnitアプリの「接続を開始」を押してください)。\n別の端末も登録するには、メニューや設定「接続」の「端末を登録…」を再度開きます。", "Pairing complete. Your device will connect automatically. On Android, choose Start Connection in the Knit app.\nTo pair another device, open Pair a Device again from the menu or Connection settings."),
    ("コードの有効期限が切れました。", "The code has expired."),
    ("コードの有効期限が切れました", "Code Expired"),
    ("3回の接続試行に達しました。", "The three-attempt limit has been reached."),
    ("登録の試行回数に達しました", "Pairing Attempt Limit Reached"),
    ("5分の有効期限が切れたため、このコードでは登録できません。\n新しいコードを作ると、もう一度登録を受け付けます。", "This code has passed its five-minute expiry.\nCreate a new code to accept pairing requests again."),
    ("3回の接続試行に達したため、このコードでは登録できません。\n新しいコードを作ると、もう一度登録を受け付けます。", "This code has reached its three-attempt limit.\nCreate a new code to accept pairing requests again."),
    ("登録の待受を開始できませんでした。別の登録画面が開いていないか確認してください。", "Could not accept pairing requests. Check whether another pairing window is already open."),
    ("登録の準備ができませんでした。もう一度起動してください。", "Could not prepare pairing. Restart Knit and try again."),
    ("キーチェーンに保存できませんでした。アクセス許可を確認してください。", "Could not save to Keychain. Check its access permissions."),
    ("設定ファイルが壊れていたため初期設定で起動しました(退避: preferences.json.corrupt)", "Started with defaults because settings were corrupt (backup: preferences.json.corrupt)."),
    ("設定ファイルが壊れていたため初期設定で起動しました(退避できませんでした。ログを確認してください)", "Started with defaults because settings were corrupt. Backup failed; check the log."),
];

pub(super) fn english_copy(ja: &str) -> Option<&'static str> {
    COPY.iter()
        .find_map(|&(source, en)| (source == ja).then_some(en))
}

pub(super) fn copy(ja: &str) -> String {
    copy_for(language(), ja)
}

pub(super) fn copy_for(language: Language, ja: &str) -> String {
    if language == Language::Japanese {
        if ja.starts_with("自動保存・起動時は環境変数 ")
            || ja.starts_with("保存済み・起動時は環境変数 ")
        {
            return ja.replace("左下の「その他」", "「その他」");
        }
        return ja.into();
    }
    if let Some(en) = english_copy(ja) {
        return en.into();
    }
    // アプリが生成する既知の書式だけを扱う。端末名や OS のエラーには置換を掛けない。
    for (prefix, suffix, lead, tail) in [
        ("現在のキー(", ")", "Current key (", ")"),
        ("ほかのキー…(", ")", "Other Key… (", ")"),
        (
            "192.168.1.23の形式で入力してください(",
            "はIPアドレスとして読めません)",
            "Enter an IP address such as 192.168.1.23. Invalid: ",
            "",
        ),
        (
            "接続先を保存できませんでした(",
            ")",
            "Could not save the address (",
            ")",
        ),
        (
            "接続先を ",
            " に保存しました。Knitを再起動します",
            "Address saved: ",
            ". Restarting Knit.",
        ),
    ] {
        if let Some(value) = ja.strip_prefix(prefix).and_then(|v| v.strip_suffix(suffix)) {
            let value = if prefix.starts_with("現在のキー") || prefix.starts_with("ほかのキー…")
            {
                if let Some(code) = value.strip_prefix("コード") {
                    return format!("{lead}Key code {code}{tail}");
                }
                english_copy(value).unwrap_or(value)
            } else {
                value
            };
            return format!("{lead}{value}{tail}");
        }
    }
    for (prefix, lead) in [
        ("自動保存・起動時は環境変数 ", "Auto-save"),
        ("保存済み・起動時は環境変数 ", "Saved"),
    ] {
        if let Some(keys) = ja
            .strip_prefix(prefix)
            .and_then(|v| v.strip_suffix(" が優先(初期化は左下の「その他」)"))
        {
            return format!(
                "{lead} · startup overrides: {} · reset in General",
                keys_english(keys)
            );
        }
    }
    if let Some(keys) = ja.strip_suffix(" で固定中のため優先") {
        return format!("Fixed by {}", keys_english(keys));
    }
    if ja.starts_with("このPCの設定をすべて消して、最初の状態に戻します。")
    {
        let manual = if ja.contains("直接つなぐ(手動接続)の設定も解除され") {
            "\nDirect-connection settings are also removed. Normal pairing will be required."
        } else {
            ""
        };
        return format!("Restores this Mac’s settings to their defaults.\nRemoves preferences.json, saved device arrangements and names (peer-sides.json), and KNIT_* entries from the env file.\nClipboard history and device pairings are retained.{manual}\n\nKNIT_* variables set in your environment are not removed. Remove those at their source.\nThis cannot be undone.");
    }
    if let Some(left) = ja
        .strip_prefix("接続を確認できませんでした。コードを確認してください(残り")
        .and_then(|v| v.strip_suffix("回)。"))
    {
        return format!(
            "Could not verify the connection. Check the code ({left} attempts remaining)."
        );
    }
    ja.into()
}

fn keys_english(keys: &str) -> String {
    let (names, rest) = keys.split_once(" ほか").unwrap_or((keys, ""));
    let names = names.replace('・', ", ");
    if let Some(n) = rest.strip_suffix('件') {
        format!("{names} +{n} more")
    } else {
        names
    }
}

pub(super) fn side_label(side: u8) -> &'static str {
    const EN: [&str; 8] = [
        "right",
        "left",
        "top",
        "bottom",
        "upper right",
        "lower right",
        "upper left",
        "lower left",
    ];
    language().pick(crate::side_label(side), EN[side.min(7) as usize])
}

// 接続の待ち表示は共通層からの既知の書式だけを翻訳する。端末名などへ置換を掛けない。
pub(super) fn wait_detail_for(language: Language, ja: &str) -> String {
    if language == Language::Japanese {
        return ja.into();
    }
    if let Some(n) = ja
        .strip_prefix("再試行まで ")
        .and_then(|v| v.strip_suffix("秒"))
    {
        return format!("Retrying in {n}s");
    }
    if let Some(n) = ja
        .strip_prefix("相手を ")
        .and_then(|v| v.strip_suffix("分見つけられません"))
    {
        return format!("Device not found for {n} min");
    }
    ja.into()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn english_templates_preserve_user_values_and_override_keys() {
        assert_eq!(copy_for(Language::English, "自動保存・起動時は環境変数 KNIT_ROLE・KNIT_SHARE ほか3件 が優先(初期化は左下の「その他」)"), "Auto-save · startup overrides: KNIT_ROLE, KNIT_SHARE +3 more · reset in General");
        assert_eq!(
            copy_for(
                Language::English,
                "192.168.1.23の形式で入力してください(田口さんはIPアドレスとして読めません)"
            ),
            "Enter an IP address such as 192.168.1.23. Invalid: 田口さん"
        );
        assert_eq!(copy_for(Language::English, "田口さんのPC"), "田口さんのPC");
    }

    #[test]
    fn supported_languages_are_explicit_and_unknown_values_are_rejected() {
        assert_eq!(Language::parse("ja"), Some(Language::Japanese));
        assert_eq!(Language::parse("en"), Some(Language::English));
        for unsupported in ["", "EN", "fr", "../../en"] {
            assert_eq!(Language::parse(unsupported), None);
        }
        for lang in [Language::Japanese, Language::English] {
            assert_eq!(Language::parse(lang.code()), Some(lang));
            assert_eq!(
                lang.pick("日本語", "English"),
                if lang == Language::English {
                    "English"
                } else {
                    "日本語"
                }
            );
        }
    }

    #[test]
    fn static_copy_is_unique_complete_and_preserves_identifiers() {
        let mut seen = std::collections::HashSet::new();
        for &(ja, en) in COPY {
            assert!(seen.insert(ja), "duplicate localization key: {ja}");
            assert!(!en.is_empty());
            assert!(
                !en.chars().any(|c| ('\u{3040}'..='\u{9fff}').contains(&c)),
                "untranslated copy: {en}"
            );
            if ja.contains("KNIT_ROLE") {
                assert!(en.contains("KNIT_ROLE"));
            }
        }
        for key in [
            "接続",
            "画面配置",
            "切替",
            "共有",
            "その他",
            "保存して再接続",
            "すべての登録を初期化…",
        ] {
            assert!(english_copy(key).is_some(), "missing critical copy: {key}");
        }
        assert_eq!(
            english_copy("田口さんのPC"),
            None,
            "user names are not localization keys"
        );
    }
}

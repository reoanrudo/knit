//! GUI設定だけを保存し、認証情報や接続先の設定と分離する。
use super::*;
use serde_json::{json, Value};

fn path() -> Option<std::path::PathBuf> {
    knit_common::envutil::config_dir().map(|dir| dir.join("preferences.json"))
}

/// GUI 設定を上書きできる環境変数・envファイルのキー(新しい順に確認する)
const ENV_OVERRIDE_KEYS: [&str; 21] = [
    "KNIT_SIDE",
    "KNIT_SWITCH_MODE",
    "KNIT_EDGE_TAPS",
    "KNIT_HOTKEY_KC",
    "KNIT_SWITCH_DELAY",
    "KNIT_DOUBLE_TAP_MS",
    "KNIT_SCROLL_DIV",
    "KNIT_MOUSE_SCALE",
    "KNIT_SCROLL_FLIP",
    "KNIT_SCROLL_COMPAT",
    "KNIT_CMD_ALT",
    "KNIT_MUTE_SPK",
    "KNIT_CLIP",
    "KNIT_ROLE",
    "KNIT_LOCAL_HISTORY",
    "KNIT_AUDIO",
    "KNIT_AUDIO_GAIN",
    "KNIT_ANDROID",
    "KNIT_ANDROID_ADB",
    "KNIT_ANDROID_GAIN",
    "KNIT_ANDROID_SCROLL_FLIP",
];

/// 環境変数・envファイルで指定されている(=GUI 設定より優先される)キーの一覧。
/// 「どの項目が固定されているか」を見える化するため bool から拡張した
pub(super) fn override_keys() -> Vec<&'static str> {
    override_keys_with(|key| crate::envutil::get(key))
}

/// キー一覧の生成(参照の解決を注入して単体テストできるようにした純粋関数)
fn override_keys_with(get: impl Fn(&str) -> Option<String>) -> Vec<&'static str> {
    ENV_OVERRIDE_KEYS
        .iter()
        .filter(|key| get(key).is_some())
        .copied()
        .collect()
}

pub(super) fn snapshot() -> Value {
    json!({
        "version": 1, "hotkey": crate::hotkey_kc(), "side": crate::SIDE.load(Ordering::Relaxed),
        "client_role": crate::CLIENT_ROLE.load(Ordering::Relaxed),
        "hotkey_only": crate::HOTKEY_ONLY.load(Ordering::Relaxed),
        "edge_taps": crate::EDGE_TAPS.load(Ordering::Relaxed),
        "delay": crate::SWITCH_DELAY_MS.load(Ordering::Relaxed),
        "double_tap": crate::DOUBLE_TAP_MS.load(Ordering::Relaxed),
        "scroll_div": crate::scroll_div(), "mouse_scale": crate::mouse_scale(),
        "scroll_flip": crate::SCROLL_FLIP.load(Ordering::Relaxed),
        "scroll_compat": crate::SCROLL_COMPAT.load(Ordering::Relaxed),
        "cmd_alt": crate::CMD_ALT.load(Ordering::Relaxed),
        "spk_mute": crate::SPK_MUTE.load(Ordering::Relaxed),
        "clip_share": crate::CLIP_SHARE.load(Ordering::Relaxed),
        "share_files": knit_common::share::user_files(),
        "local_history": crate::LOCAL_HISTORY.load(Ordering::Relaxed),
        "audio_muted": crate::audio::MUTED.load(Ordering::Relaxed),
        "audio_gain": crate::audio::gain() as f64,
        "android_pinch": crate::trackpad::ENABLED.load(Ordering::Relaxed),
        "android_navigation": crate::trackpad::NAV_ENABLED.load(Ordering::Relaxed),
        "layout_range": *crate::LAY_RANGE.lock().unwrap_or_else(|e| e.into_inner()),
        // このMacの名前(空=ホスト名既定)。hello の name と同じ規則で保存する
        "own_name": crate::OWN_NAME.lock().unwrap_or_else(|e| e.into_inner()).clone(),
        // 詳細記録(--diag 相当。1秒ごとの診断行)。起動時の --diag 引数は
        // これより優先する(起動時に true へ上書きされるため矛盾しない)
        "diag_log": crate::DIAG_ENABLED.load(Ordering::Relaxed),
        // 環境変数・envファイルで固定されているキー(設定が効かない原因の見える化)。
        // apply は知らないキーを無視するため、読み込み側でそのまま戻しても無害
        "env_overrides": override_keys()
    })
}

/// 画面に触れない保存(別スレッドから呼ぶ用)
pub(super) fn save_quiet() {
    if UI_PREVIEW.load(Ordering::Relaxed) || RESTARTING.load(Ordering::Relaxed) {
        return;
    }
    if let Some(path) = path() {
        if let Err(e) = write_to(&path, &snapshot()) {
            eprintln!("[prefs] save failed: {e}");
        }
    }
}

pub(super) fn save() {
    // RESTARTING=再起動待ちの間はメモリの設定が旧いままのため、保存すると
    // 「すべての設定を初期化」が消したファイルを書き戻してしまう
    if UI_PREVIEW.load(Ordering::Relaxed) || RESTARTING.load(Ordering::Relaxed) {
        return;
    }
    let result = (|| -> std::io::Result<()> {
        let path = path().ok_or_else(|| std::io::Error::other("HOMEがありません"))?;
        write_to(&path, &snapshot())
    })();
    let status = if result.is_ok() {
        // 保存が成功した=壊れたファイル(退避済み)に代わる新しい設定が書けたため、
        // 起動時の破損の案内(PrefsFile)だけを外す(他の出元の未解決エラーは残す)
        prefs::clear_save_error(prefs::SaveErrorSource::PrefsFile);
        prefs::save_status_saved(&override_keys())
    } else {
        "設定を保存できません。ログを確認してください".to_string()
    };
    unsafe {
        prefs::set_save_status(&status);
    }
    if let Err(e) = result {
        eprintln!("[prefs] save failed: {e}");
    }
}

/// 設定一覧を JSON 文字列で返す(Windows の設定画面への応答)
pub(super) fn snapshot_json() -> String {
    snapshot().to_string()
}

/// 現在の設定を JSON ファイルへ書き出す(設定画面の左下「その他」の「設定を書き出す…»)。
/// 中身は snapshot と同じ形式(version 1)。引っ越しやバックアップに使う。
/// 書き込みは通常の保存と同じ一時ファイル+リネームのため、失敗しても
/// 既存ファイルは壊れない
pub(super) fn export_to(path: &std::path::Path) -> std::io::Result<()> {
    write_to(path, &snapshot())
}

/// 書き出した JSON ファイルから設定を読み込む(「設定を読み込む…」)。
/// version 1 の形式だけを受け付ける。apply は知らないキーを無視し範囲検査も
/// 通るため、外部で編集されたファイルでも安全。読み込んだら保存まで行う
pub(super) fn import_from(source: &std::path::Path) -> Result<(), String> {
    let bytes = std::fs::read(source).map_err(|e| format!("ファイルを読めません: {e}"))?;
    let v: Value = serde_json::from_slice(&bytes)
        .map_err(|e| format!("設定ファイルとして読めません: {e}"))?;
    if v["version"].as_u64() != Some(1) {
        return Err("このファイルはKnitの設定ファイルではありません".into());
    }
    apply(&v);
    if let Some(dest) = path() {
        if let Err(e) = write_to(&dest, &snapshot()) {
            return Err(format!("設定を保存できませんでした: {e}"));
        }
    }
    Ok(())
}

/// Windows から届いた設定変更を適用する。許可したキーだけを範囲検査つきで反映し、
/// 画面には触れず(別スレッドから呼ばれる)、ファイルへ保存する
pub(super) fn apply_remote(json: &str) {
    const ALLOWED: [&str; 21] = [
        "hotkey", "side", "hotkey_only", "edge_taps", "delay", "double_tap", "scroll_div",
        "mouse_scale", "scroll_flip", "scroll_compat", "cmd_alt", "spk_mute", "clip_share",
        "share_files", "local_history", "audio_muted", "audio_gain", "layout_range",
        "android_pinch", "android_navigation", "peer_layout_reset",
    ];
    // RESTARTING も同じ理由で止める(遠隔適用は受信スレッドから呼ばれるため、
    // 初期化後の再起動待ちでも届き得る)
    if json.len() > 4096
        || UI_PREVIEW.load(Ordering::Relaxed)
        || RESTARTING.load(Ordering::Relaxed)
    {
        return;
    }
    let Ok(Value::Object(map)) = serde_json::from_str::<Value>(json) else {
        return;
    };
    let mut filtered = serde_json::Map::new();
    for (k, v) in map {
        if ALLOWED.contains(&k.as_str()) {
            filtered.insert(k, v);
        }
    }
    // Android アプリは音声非対応。その接続中に音声設定を書き換えない
    if crate::active_peer_is_android_app() {
        filtered.remove("audio_muted");
    }
    // 配置の完全リセット(辺とモニター指定の両方を既定へ戻す。
    // Mac 設定画面の「配置を初期化」と同じ効果)
    let layout_reset = filtered.remove("peer_layout_reset").is_some_and(|v| v == json!(true));
    let v = Value::Object(filtered);
    if layout_reset {
        let act = *crate::ACTIVE_PEER.lock().unwrap_or_else(|e| e.into_inner());
        if act != usize::MAX {
            crate::set_peer_side(act, 0, None);
        }
    }
    apply(&v);
    // 画面の部品を介さない設定(Android のジェスチャ)は専用の入口で反映する
    if let Some(on) = v["android_pinch"].as_bool() {
        crate::trackpad::set_enabled(on);
    }
    if let Some(on) = v["android_navigation"].as_bool() {
        crate::trackpad::set_navigation(on);
    }
    // 試験が利用者の設定ファイルを上書きしないよう、試験では保存しない
    if cfg!(test) {
        return;
    }
    if let Some(path) = path() {
        if let Err(e) = write_to(&path, &snapshot()) {
            eprintln!("[prefs] remote save failed: {e}");
        }
    }
}

fn write_to(path: &std::path::Path, value: &Value) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let parent = path
        .parent()
        .ok_or_else(|| std::io::Error::other("保存先がありません"))?;
    std::fs::create_dir_all(parent)?;
    knit_common::history::restrict_dir(parent);
    let tmp = path.with_extension(format!("{}.tmp", std::process::id()));
    // 履歴と同じく 0600 で作成する(作成後に chmod する方式だと共有 Mac で
    // 僅かな時間他ユーザーに読まれる窓が残る)
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&tmp)?;
    f.write_all(serde_json::to_string_pretty(value)?.as_bytes())?;
    f.sync_all()?;
    drop(f);
    std::fs::rename(tmp, path)
}

/// 壊れた preferences.json で起動した時の保存状態行の文言(純粋関数・単体テストで
/// 守る)。quarantined=退避に成功したか(persist は rename で退避するため、読み込み
/// 後に元のパスが消えていたかで判る)
fn corrupt_restore_note(quarantined: bool) -> &'static str {
    if quarantined {
        "設定ファイルが壊れていたため初期設定で起動しました(退避: preferences.json.corrupt)"
    } else {
        "設定ファイルが壊れていたため初期設定で起動しました(退避できませんでした。ログを確認してください)"
    }
}

pub(super) fn restore() {
    let Some(path) = path() else { return };
    // 壊れた preferences.json を黙って無視すると、次の保存で上書きされて全設定が
    // 救出不能になる。persist の共通導線(peer-sides.json 等と同じ)で .corrupt へ
    // 退避してから既定値で起動し、退避したことを保存状態行へ出す(黙って設定が
    // 消えたように見せない)
    let existed = path.exists();
    match knit_common::persist::read_json_or_quarantine(&path) {
        Some(v) => {
            // 旧版が 644 で作ったファイルを読めたら 600 へ是正する
            knit_common::history::restrict(&path);
            if v["version"].as_u64() != Some(1) {
                return;
            }
            apply(&v);
        }
        None => {
            // 読み込み前にファイルが有ったのに読めない=壊れて退避した(または退避に
            // 失敗した)。初回起動(ファイルが無い)では出さない
            if existed {
                // rename で退避されていれば元のパスは消えている
                let note = corrupt_restore_note(!path.exists());
                eprintln!("[prefs] {note}");
                prefs::set_save_error(prefs::SaveErrorSource::PrefsFile, note.to_string());
            }
        }
    }
}

fn apply(v: &Value) {
    let boolean = |key: &str, atom: &AtomicBool| {
        if let Some(b) = v[key].as_bool() {
            atom.store(b, Ordering::Relaxed);
        }
    };
    if let Some(n) = v["hotkey"].as_i64().filter(|n| (1..=127).contains(n)) {
        crate::HOTKEY_KC.store(n, Ordering::Relaxed);
    }
    boolean("client_role", &crate::CLIENT_ROLE);
    boolean("hotkey_only", &crate::HOTKEY_ONLY);
    boolean("scroll_flip", &crate::SCROLL_FLIP);
    boolean("scroll_compat", &crate::SCROLL_COMPAT);
    boolean("cmd_alt", &crate::CMD_ALT);
    boolean("spk_mute", &crate::SPK_MUTE);
    boolean("clip_share", &crate::CLIP_SHARE);
    boolean("local_history", &crate::LOCAL_HISTORY);
    if let Some(on) = v["share_files"].as_bool() {
        knit_common::share::set_user_files(on);
    }
    boolean("audio_muted", &crate::audio::MUTED);
    if let Some(n) = v["audio_gain"].as_f64().filter(|n| n.is_finite()) {
        crate::audio::set_gain(n);
    }
    boolean("android_pinch", &crate::trackpad::ENABLED);
    boolean("android_navigation", &crate::trackpad::NAV_ENABLED);
    // 詳細記録の復元(--diag 起動時は main が後から true へ上書きするため優先の
    // 順序問題は無い。遠隔適用の許可リストには入れない: 診断行は自分の端末のもの)
    boolean("diag_log", &crate::DIAG_ENABLED);
    if let Some(n) = v["side"].as_u64().filter(|n| *n <= 7) {
        crate::set_side(n as u8);
    }
    if let Some(n) = v["edge_taps"].as_u64() {
        crate::EDGE_TAPS.store(n.clamp(1, 3) as u32, Ordering::Relaxed);
    }
    if let Some(n) = v["delay"].as_u64() {
        // 0=「端で少し待つ」以外の方式(滞在を使わない)のため素通し。それ以外は
        // GUI スライダー(50-1000ms)と同じ範囲へ収める: 範囲外の値で復元すると
        // スライダーに触れた瞬間に黙って値が潰れるため、受け口の段階で揃える
        let ms = if n == 0 { 0 } else { n.clamp(50, 1000) };
        crate::SWITCH_DELAY_MS.store(ms, Ordering::Relaxed);
        // 復元経由でも滞在値を記憶する: 方式を切り替えて 0 に戻った後、
        // 「端で少し待つ」へ戻ったときにこの値へ復元する(スライダと同じ導線)
        super::prefs::remember_dwell(ms);
    }
    if let Some(n) = v["double_tap"].as_u64() {
        crate::DOUBLE_TAP_MS.store(n.clamp(100, 3000), Ordering::Relaxed);
    }
    if let Some(n) = v["scroll_div"].as_f64().filter(|n| n.is_finite()) {
        crate::set_scroll_div(n);
    }
    if let Some(n) = v["mouse_scale"].as_f64().filter(|n| n.is_finite()) {
        crate::set_mouse_scale(n);
    }
    if let (Some(a), Some(b)) = (v["layout_range"][0].as_f64(), v["layout_range"][1].as_f64()) {
        if a.is_finite() && b.is_finite() && a >= 0.0 && b <= 1.0 && a < b {
            *crate::LAY_RANGE.lock().unwrap_or_else(|e| e.into_inner()) = (a, b);
        }
    }
    // このMacの名前。手で書き換えられたファイルから不正文字を表示・通信へ
    // 流さないため safe_peer_name(hello の name と同じ規則)で検査する。
    // 空文字=ホスト名(kern.hostname)既定
    if let Some(n) = v["own_name"].as_str() {
        *crate::OWN_NAME.lock().unwrap_or_else(|e| e.into_inner()) =
            knit_common::proto::safe_peer_name(n.trim());
    }
}

/// この画面の設定は static そのもののため、テスト同士の干渉を直列化で防ぐ。
/// prefs.rs の切替方式テストも同じ static 群(HOTKEY_ONLY 等)を触るため共用する
#[cfg(test)]
pub(super) static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;

    /// 書き出し→読み込みのラウンドトリップ。読み込みは version 1 の形式だけを
    /// 受け付け、apply を通すため外部で編集された内容でも範囲検査が効く
    #[test]
    fn export_and_import_round_trip() {
        let _serial = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let before = snapshot();
        let dir = std::env::temp_dir().join(format!(
            "knit-prefs-export-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let file = dir.join("knit-settings.json");
        export_to(&file).unwrap();
        // 値を変えてから読み込むと書き出し時点へ戻る
        apply(&json!({"scroll_div": 140.0, "edge_taps": 1}));
        assert_eq!(crate::scroll_div(), 140.0);
        import_from(&file).unwrap();
        assert_eq!(crate::scroll_div(), before["scroll_div"].as_f64().unwrap());
        assert_eq!(
            crate::EDGE_TAPS.load(Ordering::Relaxed) as u64,
            before["edge_taps"].as_u64().unwrap()
        );
        // version が違うファイル・壊れたファイルは拒否される
        std::fs::write(&file, "{\"version\": 9}").unwrap();
        assert!(import_from(&file).is_err(), "version 不一致は拒否");
        std::fs::write(&file, "not json").unwrap();
        assert!(import_from(&file).is_err(), "JSON 不一致は拒否");
        std::fs::remove_dir_all(dir).unwrap();
        apply(&before);
    }

    /// 滞在時間(delay)の復元: 0(=「端で少し待つ」以外の方式・滞在なし)はそのまま
    /// 通し、それ以外は GUI スライダーと同じ 50-1000ms へ収める。復元値が GUI の
    /// 操作範囲を超えていると、スライダーに触れた瞬間に値が黙って潰れるため
    #[test]
    fn restored_delay_is_clamped_to_slider_range_but_zero_passes() {
        let _serial = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let before = snapshot();
        apply(&json!({"delay": 0}));
        assert_eq!(
            crate::SWITCH_DELAY_MS.load(Ordering::Relaxed),
            0,
            "0=滞在なしは方式の情報のため素通り"
        );
        apply(&json!({"delay": 10}));
        assert_eq!(crate::SWITCH_DELAY_MS.load(Ordering::Relaxed), 50, "下限は 50ms");
        apply(&json!({"delay": 4000}));
        assert_eq!(crate::SWITCH_DELAY_MS.load(Ordering::Relaxed), 1000, "上限は 1000ms");
        apply(&json!({"delay": 300}));
        assert_eq!(crate::SWITCH_DELAY_MS.load(Ordering::Relaxed), 300, "範囲内はそのまま");
        apply(&before);
    }

    /// 壊れた preferences.json で起動した時の案内: 退避の成否で文言が変わる。
    /// どちらも「初期設定で起動した」ことと救出材料の手がかりを出す
    #[test]
    fn corrupt_restore_note_mentions_quarantine() {
        let saved = corrupt_restore_note(true);
        assert!(saved.contains("初期設定で起動しました"), "{saved}");
        assert!(
            saved.contains("preferences.json.corrupt"),
            "退避先を手がかりとして出す: {saved}"
        );
        let failed = corrupt_restore_note(false);
        assert!(failed.contains("初期設定で起動しました"), "{failed}");
        assert!(failed.contains("退避できませんでした"), "{failed}");
    }

    #[test]
    fn override_keys_list_only_present_entries() {
        // 参照解決を注入した純粋関数で検証: 指定のあるキーだけが一覧に入る
        assert!(override_keys_with(|_| None).is_empty(), "指定が無ければ空");
        let keys = override_keys_with(|k| (k == "KNIT_ROLE" || k == "KNIT_SCROLL_DIV").then(|| "1".into()));
        assert_eq!(keys, ["KNIT_SCROLL_DIV", "KNIT_ROLE"], "定義順に並ぶ");
    }

    /// snapshot の env_overrides: 固定キーの配列として出る(apply は未知キーを
    /// 無視するため、このキーを含む JSON を読み戻しても無害)
    #[test]
    fn snapshot_carries_env_overrides_without_affecting_apply() {
        let _serial = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let snap = snapshot();
        assert!(
            snap["env_overrides"].is_array(),
            "env_overrides は配列: {}",
            snap["env_overrides"]
        );
        let before = snapshot();
        apply(&snap);
        assert_eq!(snapshot()["hotkey"], before["hotkey"], "読み戻しても値は変わらない");
        apply(&before);
    }

    #[test]
    fn remote_apply_ignores_unlisted_keys_and_oversized_input() {
        let _serial = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let before = snapshot();
        let role = crate::CLIENT_ROLE.load(Ordering::Relaxed);
        apply_remote(r#"{"client_role": true, "version": 9, "scroll_div": 140.0}"#);
        assert_eq!(crate::CLIENT_ROLE.load(Ordering::Relaxed), role);
        assert_eq!(crate::scroll_div(), 140.0);
        apply_remote(&format!(r#"{{"scroll_div": 40.0, "pad": "{}"}}"#, "x".repeat(5000)));
        assert_eq!(crate::scroll_div(), 140.0);
        apply(&before);
    }

    /// このMacの名前(own_name): trim 済み・safe_peer_name で検査され、
    /// 空・空白のみはホスト名既定(空文字)へ戻る。リモート(PrefsSet)からは
    /// 変更できない(自分の名乗り名を相手側から書き換えさせない)
    #[test]
    fn own_name_is_sanitized_and_not_remotely_writable() {
        let _serial = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let before = snapshot();
        apply(&json!({"own_name": "  書斎のMac  "}));
        assert_eq!(
            *crate::OWN_NAME.lock().unwrap_or_else(|e| e.into_inner()),
            "書斎のMac",
            "前後の空白は落として保存される"
        );
        assert_eq!(snapshot()["own_name"], json!("書斎のMac"));
        // 偽装文字(Bidi オーバーライド)は hello の name と同じ規則で除去
        apply(&json!({"own_name": "A\u{202e}B"}));
        assert_eq!(*crate::OWN_NAME.lock().unwrap_or_else(|e| e.into_inner()), "AB");
        // 空・空白のみはホスト名既定へ戻る
        apply(&json!({"own_name": "   "}));
        assert_eq!(
            *crate::OWN_NAME.lock().unwrap_or_else(|e| e.into_inner()),
            "",
            "空白のみは空=ホスト名既定"
        );
        // リモート適用の許可リストに無いため変わらない
        apply_remote(r#"{"own_name": "遠隔のMac"}"#);
        assert_eq!(*crate::OWN_NAME.lock().unwrap_or_else(|e| e.into_inner()), "");
        apply(&before);
    }

    #[test]
    fn restored_values_are_bounded_and_invalid_layout_is_ignored() {
        let _serial = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let before = snapshot();
        *crate::LAY_RANGE.lock().unwrap_or_else(|e| e.into_inner()) = (0.2, 0.8);
        let side_before = crate::SIDE.load(Ordering::Relaxed);
        apply(
            &json!({"scroll_div": -20.0, "mouse_scale": 99.0, "edge_taps": 0,
            "delay": 9000, "side": 999, "layout_range": [0.9, 0.1]}),
        );
        assert_eq!(crate::scroll_div(), 20.0);
        assert_eq!(crate::mouse_scale(), 3.0);
        assert_eq!(crate::EDGE_TAPS.load(Ordering::Relaxed), 1);
        // 滞在時間は GUI スライダー(50-1000ms)と同じ上限へ潰される
        assert_eq!(crate::SWITCH_DELAY_MS.load(Ordering::Relaxed), 1000);
        assert_eq!(
            crate::SIDE.load(Ordering::Relaxed),
            side_before,
            "side:999 は受理されない(apply の n <= 7 フィルタ)"
        );
        assert_eq!(*crate::LAY_RANGE.lock().unwrap_or_else(|e| e.into_inner()), (0.2, 0.8));
        // 上限そのもの(7)は受理・直上(8)は拒否
        apply(&json!({"side": 8}));
        assert_eq!(crate::SIDE.load(Ordering::Relaxed), side_before, "side:8 は拒否");
        apply(&json!({"side": 7}));
        assert_eq!(crate::SIDE.load(Ordering::Relaxed), 7, "side:7 は受理");
        apply(&before);
        let dir = std::env::temp_dir().join(format!(
            "knit-prefs-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let path = dir.join("preferences.json");
        write_to(&path, &before).unwrap();
        let mut changed = before.clone();
        changed["hotkey"] = json!(97);
        changed["side"] = json!(6);
        changed["scroll_div"] = json!(140.0);
        write_to(&path, &changed).unwrap();
        let loaded: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        apply(&loaded);
        assert_eq!(crate::hotkey_kc(), 97);
        assert_eq!(crate::SIDE.load(Ordering::Relaxed), 6);
        assert_eq!(crate::scroll_div(), 140.0);
        // ファイルの受け渡しの選択も保存・復元される(直後に元へ戻す)
        let mut share_off = before.clone();
        share_off["share_files"] = json!(false);
        apply(&share_off);
        assert!(!knit_common::share::user_files());
        assert_eq!(snapshot()["share_files"], json!(false));
        apply(&before);
        assert!(knit_common::share::user_files());
        assert!(write_to(&path.join("invalid.json"), &before).is_err());
        assert_eq!(
            serde_json::from_slice::<Value>(&std::fs::read(&path).unwrap()).unwrap(),
            changed
        );
        std::fs::remove_dir_all(dir).unwrap();
        apply(&before);
    }
}

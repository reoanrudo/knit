//! この Mac だけで使う通知・受信先・ログイン時起動の設定。
//! 受信先は最初の receive_dir() で固定する。apply は保存だけを行い、実際の
//! Endpoint は次回起動から変更する（役割切替での再接続でも変更しない）。
use serde_json::{json, Value};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    pub notify_connection: bool,
    pub notify_transfer: bool,
    /// 空文字列は ~/Downloads/Knit。指定する場合は既存の絶対パス。
    pub receive_folder: String,
    /// ファイル受信完了後に受信フォルダをFinderで開くか。
    pub open_receive_folder: bool,
    /// クリップボード履歴の保存件数上限。許容値は 10/50/100/200。
    pub history_limit: usize,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            notify_connection: true,
            notify_transfer: true,
            receive_folder: String::new(),
            open_receive_folder: false,
            history_limit: 50,
        }
    }
}

fn sandboxed() -> bool {
    cfg!(test) || crate::gui::UI_PREVIEW.load(Ordering::Relaxed)
}

fn settings_path() -> Result<PathBuf, String> {
    knit_common::envutil::config_dir()
        .map(|dir| dir.join("desktop-settings.json"))
        .ok_or_else(|| "設定フォルダを取得できません".into())
}

fn validate_history_limit(limit: usize) -> Result<usize, String> {
    if [10, 50, 100, 200].contains(&limit) {
        Ok(limit)
    } else {
        Err("history_limit は 10/50/100/200 のいずれかで指定してください".into())
    }
}

fn decode_settings(bytes: &[u8]) -> Result<Settings, String> {
    let v: Value = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    let map = v
        .as_object()
        .ok_or("設定はJSONオブジェクトである必要があります")?;
    if map.get("version").is_some_and(|v| v.as_u64() != Some(1)) {
        return Err("対応していない設定ファイルの版です".into());
    }
    let mut settings = Settings::default();
    for (key, dest) in [
        ("notify_connection", &mut settings.notify_connection),
        ("notify_transfer", &mut settings.notify_transfer),
    ] {
        if let Some(value) = map.get(key) {
            *dest = value
                .as_bool()
                .ok_or_else(|| format!("{key} は真偽値で指定してください"))?;
        }
    }
    if let Some(value) = map.get("receive_folder") {
        settings.receive_folder = value
            .as_str()
            .ok_or("receive_folder は文字列で指定してください")?
            .into();
    }
    if let Some(value) = map.get("open_receive_folder") {
        settings.open_receive_folder = value
            .as_bool()
            .ok_or("open_receive_folder は真偽値で指定してください")?;
    }
    if let Some(value) = map.get("history_limit") {
        let limit = value
            .as_u64()
            .ok_or("history_limit は 10/50/100/200 のいずれかで指定してください")?
            as usize;
        settings.history_limit = validate_history_limit(limit)?;
    }
    validate_path_syntax(&settings.receive_folder)?;
    Ok(settings)
}

fn load_settings() -> Settings {
    let result = (|| {
        let path = settings_path()?;
        match std::fs::read(path) {
            Ok(bytes) => decode_settings(&bytes),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Settings::default()),
            Err(e) => Err(e.to_string()),
        }
    })();
    result.unwrap_or_else(|e| {
        eprintln!("[desktop-settings] 読み込めません。既定値で続けます: {e}");
        Settings::default()
    })
}

fn state() -> &'static Mutex<Settings> {
    // Preview の値は本番用 OnceLock と分ける。Preview から config_dir() を呼ぶと
    // 旧設定の移行まで起こり得るため、パスの取得より前に必ず分岐する。
    static LIVE: OnceLock<Mutex<Settings>> = OnceLock::new();
    static PREVIEW: OnceLock<Mutex<Settings>> = OnceLock::new();
    if sandboxed() {
        PREVIEW.get_or_init(|| Mutex::new(Settings::default()))
    } else {
        LIVE.get_or_init(|| Mutex::new(load_settings()))
    }
}

pub fn snapshot() -> Settings {
    state().lock().unwrap_or_else(|e| e.into_inner()).clone()
}

fn default_receive_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .map(|home| home.join("Downloads/Knit"))
        .unwrap_or_else(|| std::env::temp_dir().join("knit-downloads"))
}

fn configured_receive_dir(settings: &Settings) -> PathBuf {
    if settings.receive_folder.is_empty() {
        default_receive_dir()
    } else {
        PathBuf::from(&settings.receive_folder)
    }
}

/// 現在のプロセスの実際の受信先。設定を保存しても次回起動まで変わらない。
/// 起動時の作成・残骸掃除・Endpoint・通知の表示場所はすべてこの値を使う。
pub fn receive_dir() -> PathBuf {
    static LIVE: OnceLock<PathBuf> = OnceLock::new();
    static PREVIEW: OnceLock<PathBuf> = OnceLock::new();
    let dir = if sandboxed() { &PREVIEW } else { &LIVE };
    dir.get_or_init(|| configured_receive_dir(&snapshot()))
        .clone()
}

/// Apply 前の確認用。true なら保存後に再起動が必要で、現在の転送先は変わらない。
pub fn receive_dir_change_requires_restart(next: &Settings) -> bool {
    configured_receive_dir(next) != receive_dir()
}

/// 保存済みの受信先変更が、次回起動待ちか。
pub fn receive_dir_restart_required() -> bool {
    receive_dir_change_requires_restart(&snapshot())
}

fn validate_path_syntax(folder: &str) -> Result<(), String> {
    if !folder.is_empty() && (!Path::new(folder).is_absolute() || folder.contains('\0')) {
        return Err(
            "受信フォルダは既存の絶対パスで指定してください（空欄はDownloads/Knit）".into(),
        );
    }
    Ok(())
}

fn validate_receive_dir(settings: &Settings) -> Result<(), String> {
    validate_path_syntax(&settings.receive_folder)?;
    let path = configured_receive_dir(settings);
    let metadata = std::fs::metadata(&path)
        .map_err(|e| format!("受信フォルダを確認できません（{}）: {e}", path.display()))?;
    if !metadata.is_dir() {
        return Err("受信先にはファイルではなくフォルダを指定してください".into());
    }
    // 試験ファイルを作成せず、ACLを含む実行ユーザーの書込・検索権限を確認する。
    // 容量切れ・後からの権限変更等は転送時にもエラーとして通知される。
    use std::os::unix::ffi::OsStrExt;
    let path_c = std::ffi::CString::new(path.as_os_str().as_bytes())
        .map_err(|_| "受信フォルダに無効な文字があります".to_string())?;
    unsafe extern "C" {
        fn access(path: *const std::ffi::c_char, mode: i32) -> i32;
    }
    const W_OK_X_OK: i32 = 2 | 1;
    if unsafe { access(path_c.as_ptr(), W_OK_X_OK) } != 0 {
        return Err(format!("受信フォルダに書き込めません: {}", path.display()));
    }
    Ok(())
}

fn transfer_active() -> bool {
    crate::FILE_TX_BUSY.load(Ordering::Relaxed)
        || knit_common::bulk::rx_active()
        || crate::android::PUSH_REMAINING.load(Ordering::Relaxed) > 0
}

/// 設定を検証し、原子的な保存が成功した後にだけメモリへ反映する。
/// 起動時設定は別APIで明示適用する。転送中は保存を拒否する。
/// Test/Preview は独立したメモリだけを変更し、設定読込・保存・実フォルダ検証はしない。
pub fn apply(next: Settings) -> Result<(), String> {
    validate_path_syntax(&next.receive_folder)?;
    validate_history_limit(next.history_limit)?;
    // 最初の apply より前に実際の受信先を固定し、未初期化の Endpoint が
    // 保存直後だけ新フォルダを使ってしまう状態を防ぐ。
    let _ = receive_dir();
    let mut current = state().lock().unwrap_or_else(|e| e.into_inner());
    if sandboxed() {
        *current = next;
        return Ok(());
    }
    if transfer_active() {
        return Err(
            "ファイル転送中は設定を保存できません。転送の完了後にもう一度適用してください".into(),
        );
    }
    validate_receive_dir(&next)?;
    let bytes = serde_json::to_vec_pretty(&json!({
        "version": 1,
        "notify_connection": next.notify_connection,
        "notify_transfer": next.notify_transfer,
        "receive_folder": next.receive_folder,
        "open_receive_folder": next.open_receive_folder,
        "history_limit": next.history_limit,
    }))
    .map_err(|e| e.to_string())?;
    let path = settings_path()?;
    let parent = path.parent().ok_or("設定フォルダを取得できません")?;
    std::fs::create_dir_all(parent).map_err(|e| format!("設定フォルダを用意できません: {e}"))?;
    // 検証中に転送が始まった場合も保存しない。受信先はそもそも起動中不変で、
    // この確認直後に新たな転送が始まっても経路を切断・変更しない。
    if transfer_active() {
        return Err(
            "ファイル転送が始まったため保存を中止しました。完了後にもう一度適用してください".into(),
        );
    }
    atomic_write(&path, &bytes).map_err(|e| format!("設定を保存できません: {e}"))?;
    *current = next;
    Ok(())
}

/// 同じディレクトリの排他的な一時ファイル。失敗時は自分が作成した物だけ片付ける。
struct StagedFile {
    path: PathBuf,
    file: std::fs::File,
}
impl StagedFile {
    fn new(target: &Path, bytes: &[u8]) -> std::io::Result<Self> {
        use std::os::unix::fs::OpenOptionsExt;
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let parent = target
            .parent()
            .ok_or_else(|| std::io::Error::other("保存先の親がありません"))?;
        let name = target
            .file_name()
            .ok_or_else(|| std::io::Error::other("保存先名がありません"))?;
        let mut temp_name = std::ffi::OsString::from(".");
        temp_name.push(name);
        temp_name.push(format!(
            ".{}.{}.tmp",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let path = parent.join(temp_name);
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)?;
        let mut staged = Self { path, file };
        staged.file.write_all(bytes)?;
        Ok(staged)
    }
    fn commit(&self, target: &Path) -> std::io::Result<()> {
        self.file.sync_all()?;
        std::fs::rename(&self.path, target)?;
        // ディレクトリの同期はrename成功後の補強。失敗しても「保存失敗」として
        // メモリだけ旧値に残すことはしない（ディスクとUIの不一致を防ぐ）。
        if let Some(parent) = target.parent() {
            if let Ok(dir) = std::fs::File::open(parent) {
                let _ = dir.sync_all();
            }
        }
        Ok(())
    }
}
impl Drop for StagedFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn atomic_write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    StagedFile::new(path, bytes)?.commit(path)
}

fn startup_path() -> Result<PathBuf, String> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .map(|home| home.join("Library/LaunchAgents/local.knit.plist"))
        .ok_or_else(|| "ログイン時起動の設定先を取得できません".into())
}

fn read_plist(path: &Path) -> Result<Value, String> {
    let output = std::process::Command::new("/usr/bin/plutil")
        .args(["-convert", "json", "-o", "-"])
        .arg(path)
        .output()
        .map_err(|e| format!("ログイン時起動の設定を読めません: {e}"))?;
    if !output.status.success() {
        return Err(
            "既存のログイン時起動設定を安全に読み取れません。手動設定は変更しません".into(),
        );
    }
    serde_json::from_slice(&output.stdout)
        .map_err(|e| format!("起動設定の形式を確認できません: {e}"))
}

fn validate_plist(value: &Value) -> Result<(), String> {
    let map = value
        .as_object()
        .ok_or("既存の起動設定の形式が不明です。変更しません")?;
    if map.get("Label").and_then(Value::as_str) != Some("local.knit") {
        return Err(
            "既存の起動設定のLabelがlocal.knitではありません。手動設定は変更しません".into(),
        );
    }
    let program = map.get("Program").and_then(Value::as_str).or_else(|| {
        map.get("ProgramArguments")
            .and_then(Value::as_array)
            .and_then(|args| args.first())
            .and_then(Value::as_str)
    });
    if !program.is_some_and(|p| Path::new(p).is_absolute()) {
        return Err("既存の起動設定の実行先を確認できません。手動設定は変更しません".into());
    }
    if map.get("RunAtLoad").is_some_and(|v| !v.is_boolean()) {
        return Err("既存のRunAtLoadが真偽値ではありません。手動設定は変更しません".into());
    }
    // これらがあるとRunAtLoad=falseだけでは自動起動を止められない。
    // 自動再起動や手動のジョブ条件を勝手に削除・無効化せず、明示エラーにする。
    if map
        .get("KeepAlive")
        .is_some_and(|v| v != &Value::Bool(false))
        || map
            .get("Disabled")
            .is_some_and(|v| v != &Value::Bool(false))
        || [
            "StartInterval",
            "StartCalendarInterval",
            "WatchPaths",
            "QueueDirectories",
            "StartOnMount",
            "Sockets",
            "MachServices",
            "LaunchEvents",
        ]
        .iter()
        .any(|key| map.contains_key(*key))
    {
        return Err("既存の起動設定に手動の自動起動条件があります。安全のため変更しません".into());
    }
    Ok(())
}

static PREVIEW_STARTUP: AtomicBool = AtomicBool::new(false);
static STARTUP_WRITE: Mutex<()> = Mutex::new(());

/// 次回ログイン時のRunAtLoadを返す。登録の読取失敗・別Labelのplistはfalse。
/// 特殊な手動条件があって編集不可でも、明示されたRunAtLoadの値はそのまま表示する。
/// 読み取りだけを行い、launchctl・登録作成・ジョブの起動は行わない。
pub fn startup_enabled() -> bool {
    if sandboxed() {
        return PREVIEW_STARTUP.load(Ordering::Relaxed);
    }
    let _guard = STARTUP_WRITE.lock().unwrap_or_else(|e| e.into_inner());
    let result = (|| {
        let path = startup_path()?;
        if !path.try_exists().map_err(|e| e.to_string())? {
            return Ok(false);
        }
        let value = read_plist(&path)?;
        if value["Label"].as_str() != Some("local.knit") {
            return Err("既存の起動設定のLabelがlocal.knitではありません".into());
        }
        Ok::<bool, String>(value["RunAtLoad"].as_bool().unwrap_or(false))
    })();
    result.unwrap_or_else(|e| {
        eprintln!("[desktop-settings] {e}");
        false
    })
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
        // XMLの改行正規化でパス中のCRが別の文字へ変わらないようにする。
        .replace('\r', "&#13;")
}

fn new_startup_plist(exe: &str) -> Result<String, String> {
    if !Path::new(exe).is_absolute()
        || exe.chars().any(|c| !matches!(c, '\u{9}' | '\u{a}' | '\u{d}' | '\u{20}'..='\u{d7ff}' | '\u{e000}'..='\u{fffd}' | '\u{10000}'..='\u{10ffff}'))
    {
        return Err("実行ファイルのパスを起動設定へ安全に保存できません".into());
    }
    Ok(format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>Label</key><string>local.knit</string>
<key>ProgramArguments</key><array><string>{}</string></array>
<key>RunAtLoad</key><true/>
<key>StandardOutPath</key><string>/tmp/knit-mac.log</string>
<key>StandardErrorPath</key><string>/tmp/knit-mac.log</string>
</dict></plist>
"#,
        xml_escape(exe)
    ))
}

/// 次回ログインからの自動起動を明示適用する。現在のKnitは停止しない。
/// 既存plistの実行先・引数・環境変数を保持し、RunAtLoadだけ変更する。
/// launchctl load/unload/bootout/kickstartは一切呼ばない。特殊な手動設定は拒否する。
pub fn set_startup_enabled(enabled: bool) -> Result<(), String> {
    if sandboxed() {
        PREVIEW_STARTUP.store(enabled, Ordering::Relaxed);
        return Ok(());
    }
    let _guard = STARTUP_WRITE.lock().unwrap_or_else(|e| e.into_inner());
    let path = startup_path()?;
    let existing = match std::fs::symlink_metadata(&path) {
        Ok(metadata) => {
            if !metadata.is_file() || metadata.file_type().is_symlink() {
                return Err("既存の起動設定が通常ファイルではありません。変更しません".into());
            }
            Some(metadata)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(format!("起動設定を確認できません: {e}")),
    };
    if let Some(metadata) = existing {
        let original =
            std::fs::read(&path).map_err(|e| format!("既存の起動設定を読めません: {e}"))?;
        let mut expected = read_plist(&path)?;
        validate_plist(&expected)?;
        if expected["RunAtLoad"].as_bool() == Some(enabled) {
            return Ok(());
        }
        let staged = StagedFile::new(&path, &original)
            .map_err(|e| format!("起動設定を準備できません: {e}"))?;
        // PlistBuddyには一時コピーだけを渡し、未対応形式・編集失敗で元を壊さない。
        let action = if expected.get("RunAtLoad").is_some() {
            "Set"
        } else {
            "Add"
        };
        let command = if action == "Set" {
            format!("Set :RunAtLoad {enabled}")
        } else {
            format!("Add :RunAtLoad bool {enabled}")
        };
        let output = std::process::Command::new("/usr/libexec/PlistBuddy")
            .args(["-c", &command])
            .arg(&staged.path)
            .output()
            .map_err(|e| format!("起動設定を更新できません: {e}"))?;
        if !output.status.success() {
            return Err("RunAtLoadを更新できません。既存の起動設定は変更していません".into());
        }
        expected["RunAtLoad"] = Value::Bool(enabled);
        if read_plist(&staged.path)? != expected {
            return Err("RunAtLoad以外の変更を検出したため、既存の起動設定は変更しません".into());
        }
        // 外部のインストーラ等が同時に更新した場合も上書きしない。
        if std::fs::read(&path).map_err(|e| e.to_string())? != original {
            return Err("起動設定が他の操作で変更されました。もう一度適用してください".into());
        }
        std::fs::set_permissions(&staged.path, metadata.permissions())
            .map_err(|e| e.to_string())?;
        // PlistBuddyはファイルを置換する場合があるため、元のFDではなく編集後を同期する。
        std::fs::File::open(&staged.path)
            .and_then(|f| f.sync_all())
            .map_err(|e| e.to_string())?;
        staged
            .commit(&path)
            .map_err(|e| format!("起動設定を保存できません: {e}"))?;
    } else if enabled {
        let exe =
            std::env::current_exe().map_err(|e| format!("実行ファイルを確認できません: {e}"))?;
        let exe = exe
            .to_str()
            .ok_or("実行ファイルのパスがUTF-8ではありません")?;
        let plist = new_startup_plist(exe)?;
        let parent = path.parent().ok_or("起動設定の保存先を取得できません")?;
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("起動設定フォルダを作れません: {e}"))?;
        let staged = StagedFile::new(&path, plist.as_bytes()).map_err(|e| e.to_string())?;
        // 新規作成時に手動plistが現れても置換しない（hard_linkは既存先なら失敗）。
        staged.file.sync_all().map_err(|e| e.to_string())?;
        std::fs::hard_link(&staged.path, &path)
            .map_err(|e| format!("起動設定を新規保存できません: {e}"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_defaults_and_json_validation() {
        assert_eq!(decode_settings(b"{}").unwrap(), Settings::default());
        let next = decode_settings(br#"{"version":1,"notify_connection":false,"notify_transfer":false,"receive_folder":"/Volumes/Inbox","open_receive_folder":true,"history_limit":200}"#).unwrap();
        assert!(!next.notify_connection && !next.notify_transfer);
        assert_eq!(next.receive_folder, "/Volumes/Inbox");
        assert!(next.open_receive_folder);
        assert_eq!(next.history_limit, 200);
        for invalid in [
            "[]",
            "{\"version\":2}",
            "{\"notify_transfer\":\"false\"}",
            "{\"receive_folder\":\"relative\"}",
            "{\"open_receive_folder\":\"true\"}",
            "{\"history_limit\":30}",
            "{\"history_limit\":-10}",
            "{\"history_limit\":\"50\"}",
        ] {
            assert!(decode_settings(invalid.as_bytes()).is_err());
        }
        for valid_limit in [10, 50, 100, 200] {
            let next = decode_settings(format!(r#"{{"history_limit":{valid_limit}}}"#).as_bytes())
                .unwrap();
            assert_eq!(next.history_limit, valid_limit);
        }
        assert!(validate_path_syntax("/tmp/a\0b").is_err());
    }

    #[test]
    fn startup_preserves_manual_conditions_by_refusing_them() {
        let valid = json!({"Label":"local.knit", "ProgramArguments":["/custom/Knit", "--custom"],
            "RunAtLoad":true, "EnvironmentVariables":{"KNIT_BIND":"127.0.0.1"}});
        assert!(validate_plist(&valid).is_ok());
        for (key, value) in [
            ("Label", json!("another.job")),
            ("KeepAlive", json!(true)),
            ("StartInterval", json!(60)),
            ("Disabled", json!(true)),
            ("RunAtLoad", json!("true")),
        ] {
            let mut invalid = valid.clone();
            invalid[key] = value;
            assert!(validate_plist(&invalid).is_err(), "{key}");
        }
    }

    #[test]
    fn generated_plist_escapes_paths_and_never_keeps_alive() {
        let plist = new_startup_plist("/Applications/Knit & <dev> \"' アプリ/Knit").unwrap();
        assert!(plist.contains("Knit &amp; &lt;dev&gt; &quot;&apos; アプリ/Knit"));
        assert!(!plist.contains("KeepAlive"));
        assert!(new_startup_plist("relative/Knit").is_err());
        assert!(new_startup_plist("/tmp/Knit\0").is_err());
    }

    #[test]
    fn test_apis_use_only_isolated_memory_and_keep_receive_dir_fixed() {
        // cfg(test)では実設定・launchctlに触れない。存在しないパスも試験用メモリのみ。
        assert!(sandboxed());
        let old = snapshot();
        let dir = receive_dir();
        let next = Settings {
            notify_connection: false,
            notify_transfer: false,
            receive_folder: "/knit-test-nonexistent/receive".into(),
            open_receive_folder: true,
            history_limit: 100,
        };
        assert!(receive_dir_change_requires_restart(&next));
        apply(next.clone()).unwrap();
        assert_eq!(snapshot(), next);
        assert_eq!(receive_dir(), dir);
        assert!(receive_dir_restart_required());
        assert!(apply(Settings {
            receive_folder: "relative".into(),
            ..next.clone()
        })
        .is_err());
        assert!(apply(Settings {
            history_limit: 30,
            ..next.clone()
        })
        .is_err());
        assert_eq!(snapshot(), next);
        let startup = startup_enabled();
        set_startup_enabled(!startup).unwrap();
        assert_eq!(startup_enabled(), !startup);
        set_startup_enabled(startup).unwrap();
        apply(old).unwrap();
    }
}

//! アプリ自体の更新(Windows)。署名付き更新情報を検証し、新版を展開・検査してから、
//! 終了後に新版の複製が入れ替えを行う。新版が起動し続けなければ旧版へ戻す。
//! 設計は docs/update-design.md。
use knit_common::update::{self, Available, Launch};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::Duration;

const CURRENT: &str = env!("CARGO_PKG_VERSION");
const CHANNEL: &str = "stable";
const PLATFORM: &str = "windows-x64";
const FIRST_CHECK_DELAY: Duration = Duration::from_secs(120);
const CHECK_INTERVAL: Duration = Duration::from_secs(6 * 3600);
/// 入れ替え後、新版がこの間生き続ければ成功とみなす
const HEALTH: Duration = Duration::from_secs(10);
/// 更新対象のファイル(利用者の設定・ログは対象外)。先頭が実行ファイル
const KEEP: [&str; 4] = ["knit-win.exe", "run_knit.bat", "run_knit.vbs", "release-manifest.json"];
const LOCK_FILE: &str = "update.lock";
const RESULT_FILE: &str = "update-result.txt";
/// 更新の入れ替え中は、毎分の自動復帰タスクが旧版を起こさないよう起動を見送る
const LOCK_MAX_AGE: Duration = Duration::from_secs(180);
const DETACHED_PROCESS: u32 = 0x0000_0008;
const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Clone)]
enum State {
    Idle,
    Checking,
    Available(String, update::Artifact),
    Installing,
}

static STATE: Mutex<State> = Mutex::new(State::Idle);
static NOTIFIED: Mutex<String> = Mutex::new(String::new());

fn state() -> State {
    STATE.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

fn set_state(s: State) {
    *STATE.lock().unwrap_or_else(|e| e.into_inner()) = s;
}

/// トレイメニューの表示。状態に応じて変わる
pub fn menu_title() -> String {
    match state() {
        State::Idle => "アップデートを確認…".into(),
        State::Checking => "アップデートを確認中…".into(),
        State::Available(v, _) => format!("Knit {v} に更新…"),
        State::Installing => "更新を準備中…".into(),
    }
}

/// 更新が見つかった時の通知文言。署名検証(ed25519)を通った更新だけが
/// ここへ来るため、成功時に検証が見えるように検証済みであることを書く
/// (Mac 側 updater::available_text と同じ文言)
fn available_text(version: &str) -> String {
    format!("Knit {version} が利用できます(署名を確認した更新です・ed25519 検証済み)。トレイのメニューから更新できます")
}

fn system_tool(name: &str) -> String {
    let root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
    format!(r"{root}\System32\{name}")
}

fn manifest_url() -> String {
    if cfg!(debug_assertions) {
        if let Some(u) = knit_common::envutil::get("KNIT_UPDATE_URL") {
            return u;
        }
    }
    update::MANIFEST_URL.to_string()
}

fn trusted_keys() -> Vec<[u8; 32]> {
    let mut keys = update::trusted_keys();
    if cfg!(debug_assertions) {
        if let Some(k) = knit_common::envutil::get("KNIT_UPDATE_PUBKEY")
            .and_then(|h| update::from_hex(h.trim()))
            .and_then(|b| b.try_into().ok())
        {
            keys.push(k);
        }
    }
    keys
}

fn curl(url: &str, dest: Option<&Path>, max: u64) -> Result<Vec<u8>, String> {
    update::curl(&system_tool("curl.exe"), url, dest, max, &format!("Knit/{CURRENT}"))
}

fn check() -> Result<Option<Available>, String> {
    let url = manifest_url();
    let keys = trusted_keys();
    update::check_with(
        &update::CheckConfig {
            manifest_url: &url,
            keys: &keys,
            channel: CHANNEL,
            current: CURRENT,
            platform: PLATFORM,
        },
        &|u, max| curl(u, None, max),
    )
}

fn install_dir() -> Result<PathBuf, String> {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .ok_or_else(|| "インストール先を特定できません".to_string())
}

/// 取得・検証・展開まで行い、入れ替えを新版の複製へ渡す。成功したら呼び出し側が終了する
fn install(a: &Available) -> Result<(), String> {
    let target = install_dir()?;
    let stage = target.join(format!(".knit-update-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&stage);
    std::fs::create_dir(&stage)
        .map_err(|_| "インストール先に書き込めません。更新は行いません".to_string())?;
    let result = (|| {
        let zip = stage.join("Knit.zip");
        curl(&a.artifact.url, Some(&zip), a.artifact.size)?;
        let file = std::fs::File::open(&zip).map_err(|_| update::UpdateError::Io.message().to_string())?;
        update::verify_artifact(file, &a.artifact).map_err(|e| e.message().to_string())?;
        let out = stage.join("x");
        std::fs::create_dir(&out).map_err(|_| "更新の準備に失敗しました".to_string())?;
        let ok = Command::new(system_tool("tar.exe"))
            .arg("-xf")
            .arg(&zip)
            .arg("-C")
            .arg(&out)
            .creation_flags(CREATE_NO_WINDOW)
            .stdin(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if !ok {
            return Err("更新ファイルを展開できません".into());
        }
        let pkg = update::prepare_package(&out, &a.version, PLATFORM, &KEEP)?;
        // 旧プロセスが終了する前にロックを置く(自動復帰タスクが旧版を起こす隙間を作らない)。
        // ロックが置けないまま進むと、入れ替え中に旧版が起こされて競合するため中止する
        if let Err(e) = std::fs::write(target.join(LOCK_FILE), b"updating") {
            return Err(format!("更新ロックを作成できません({e})。更新を中止します"));
        }
        // 入れ替え中は配置先の exe が使われるため、新版の複製から入れ替えを実行する
        let helper = std::env::temp_dir().join(format!("knit-update-{}.exe", std::process::id()));
        std::fs::copy(pkg.join("knit-win.exe"), &helper)
            .map_err(|_| "更新の準備に失敗しました".to_string())?;
        Command::new(&helper)
            .arg("--apply-update")
            .arg(std::process::id().to_string())
            .arg(&pkg)
            .arg(&target)
            .creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map(|_| ())
            .map_err(|_| "更新の適用を開始できません".to_string())
    })();
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&stage);
        let _ = std::fs::remove_file(target.join(LOCK_FILE));
    }
    result
}

/// トレイメニューの項目が押された時。確認 → (見つかっていれば)更新 の二段階
pub fn on_click() {
    match state() {
        State::Idle => {
            set_state(State::Checking);
            std::thread::spawn(|| match check() {
                Ok(Some(a)) => {
                    crate::tray::notify("Knit", &available_text(&a.version));
                    set_state(State::Available(a.version, a.artifact));
                }
                Ok(None) => {
                    crate::tray::notify("Knit", &format!("最新の版です(Knit {CURRENT})"));
                    set_state(State::Idle);
                }
                Err(e) => {
                    crate::tray::notify("Knit", &e);
                    set_state(State::Idle);
                }
            });
        }
        State::Available(version, artifact) => {
            set_state(State::Installing);
            std::thread::spawn(move || match install(&Available { version, artifact }) {
                Ok(()) => {
                    crate::tray::notify("Knit", "更新を適用します。Knitは自動で再起動します");
                    std::thread::sleep(Duration::from_millis(1500));
                    crate::audio::speaker_disconnect();
                    crate::release_all_input();
                    std::process::exit(0);
                }
                Err(e) => {
                    crate::tray::notify("Knit", &e);
                    set_state(State::Idle);
                }
            });
        }
        State::Checking | State::Installing => {}
    }
}

/// 起動後に静かに確認する。署名鍵が未登録の間、および KNIT_AUTO_UPDATE=0 の時は何もしない
pub fn start_background() {
    if knit_common::envutil::get("KNIT_AUTO_UPDATE").as_deref() == Some("0") || trusted_keys().is_empty() {
        return;
    }
    std::thread::spawn(|| {
        std::thread::sleep(FIRST_CHECK_DELAY);
        loop {
            if matches!(state(), State::Idle) {
                if let Ok(Some(a)) = check() {
                    let mut n = NOTIFIED.lock().unwrap_or_else(|e| e.into_inner());
                    if *n != a.version {
                        *n = a.version.clone();
                        crate::tray::notify("Knit", &available_text(&a.version));
                    }
                    if matches!(state(), State::Idle) {
                        set_state(State::Available(a.version, a.artifact));
                    }
                }
            }
            std::thread::sleep(CHECK_INTERVAL);
        }
    });
}

/// 入れ替え作業中(新版の起動確認まで)は true。自動復帰タスクからの起動を見送るために使う。
/// 入れ替えの当事者(`--apply-update` と、そこから起動される `--after-update`)は対象外
pub fn update_in_progress() -> bool {
    if std::env::args().any(|a| a == "--apply-update" || a == "--after-update") {
        return false;
    }
    install_dir()
        .ok()
        .and_then(|d| std::fs::metadata(d.join(LOCK_FILE)).ok())
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.elapsed().ok())
        .is_some_and(|age| age < LOCK_MAX_AGE)
}

/// 前回の更新結果を1度だけ通知する(入れ替えは別プロセスのため、次の起動で知らせる)
pub fn report_last_result() {
    let Ok(dir) = install_dir() else { return };
    let path = dir.join(RESULT_FILE);
    if let Ok(text) = std::fs::read_to_string(&path) {
        let _ = std::fs::remove_file(&path);
        crate::tray::notify("Knit", text.trim());
    }
}

fn wait_for_exit(pid: u32) {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{OpenProcess, WaitForSingleObject};
    const SYNCHRONIZE: u32 = 0x0010_0000;
    unsafe {
        let h = OpenProcess(SYNCHRONIZE, 0, pid);
        if !h.is_null() {
            WaitForSingleObject(h, 60_000);
            CloseHandle(h);
        }
    }
}

/// `knit-win.exe --apply-update <旧プロセスの PID> <新しい配布物のフォルダ> <配置先>`
/// 新版の複製として動く。旧プロセスの終了を待ち、ファイルを入れ替えて新版を起動する。
pub fn run_apply(args: &[String]) -> i32 {
    let [pid, pkg, target] = args else { return 2 };
    let (Ok(pid), pkg, target) = (pid.parse::<u32>(), PathBuf::from(pkg), PathBuf::from(target)) else {
        return 2;
    };
    // pkg は <stage>/x/<配布物>。作業フォルダは名前を確かめてから消す
    let stage = pkg.ancestors().nth(2).map(Path::to_path_buf);
    let lock = target.join(LOCK_FILE);
    let _ = std::fs::write(&lock, b"updating");
    wait_for_exit(pid);
    let exe = target.join("knit-win.exe");
    let log = target.join("knit-win.log");
    let result_path = target.join(RESULT_FILE);
    // 結果は、起動する側のプロセスが読める「起動の前」に書く(新版は起動直後に読むため)
    let start = |launch: Launch| {
        let message = match launch {
            Launch::New => "Knitを更新しました",
            Launch::Restore { restored: true } => "更新できませんでした。元の版へ戻しました",
            Launch::Restore { restored: false } => "更新できませんでした。元の版へ完全には戻せませんでした。インストール先の「.knit-old」が付いたファイルを元の名前に戻してください",
        };
        let _ = std::fs::write(&result_path, message);
        let out = std::fs::OpenOptions::new().create(true).append(true).open(&log)?;
        Command::new(&exe)
            .args(["--background", "--after-update"])
            .current_dir(&target)
            .stdin(Stdio::null())
            .stdout(out.try_clone()?)
            .stderr(out)
            .creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW)
            .spawn()
    };
    // 入れ替えと起動確認(HEALTH)の間はシステムを眠らせない(第一の防御は共通側の
    // 「止まっていた時間ぶん期限を延ばす」方式。これはその補助で、監視自体を
    // スリープで止めないようにする)
    use windows_sys::Win32::System::Power::{
        SetThreadExecutionState, ES_CONTINUOUS, ES_SYSTEM_REQUIRED,
    };
    unsafe { SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED) };
    let result = update::swap_and_start(&pkg, &target, &start, HEALTH);
    unsafe { SetThreadExecutionState(ES_CONTINUOUS) };
    let _ = std::fs::remove_file(&lock);
    if let Some(s) = stage {
        if s.file_name().is_some_and(|n| n.to_string_lossy().starts_with(".knit-update-")) {
            let _ = std::fs::remove_dir_all(s);
        }
    }
    i32::from(result.is_err())
}

/// 更新で残った一時ファイルを片付ける(実行中の自分は消せないため、次の起動時に行う)
pub fn cleanup_leftovers() {
    std::thread::spawn(|| {
        let me = std::env::current_exe().ok();
        if let Ok(rd) = std::fs::read_dir(std::env::temp_dir()) {
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().to_lowercase();
                if name.starts_with("knit-update-") && name.ends_with(".exe") && Some(e.path()) != me {
                    let _ = std::fs::remove_file(e.path());
                }
            }
        }
        let Ok(dir) = install_dir() else { return };
        if update_in_progress() {
            return;
        }
        if let Ok(rd) = std::fs::read_dir(&dir) {
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                if name.starts_with(".knit-update-") && e.path().is_dir() {
                    let _ = std::fs::remove_dir_all(e.path());
                }
            }
        }
    });
}

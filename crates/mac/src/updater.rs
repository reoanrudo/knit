//! アプリ自体の更新。署名付き更新情報を取得・検証し、新しい .app を検証してから、
//! 終了後に別プロセスで入れ替える。新版が起動しなければ旧版へ戻す。設計は docs/update-design.md。
use knit_common::update::{self, Artifact};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::Duration;

const CURRENT: &str = env!("CARGO_PKG_VERSION");
const CHANNEL: &str = "stable";
const BUNDLE_ID: &str = "local.knit";
/// 自動確認の間隔。起動直後は待つ(接続の確立を邪魔しない)
const FIRST_CHECK_DELAY: Duration = Duration::from_secs(90);
const CHECK_INTERVAL: Duration = Duration::from_secs(6 * 3600);

#[derive(Clone)]
enum State {
    Idle,
    Checking,
    Available { version: String, artifact: Artifact },
    Installing,
    /// 確認(or 適用)に失敗した。通知は数秒で消えるため、失敗が分かる表示を
    /// 次のクリックまで残す(クリックで Idle へ戻して最初からやり直す)
    Failed(String),
}

static STATE: Mutex<State> = Mutex::new(State::Idle);
/// 自動確認で通知済みの版(同じ版を何度も通知しない)
static NOTIFIED: Mutex<String> = Mutex::new(String::new());

fn state() -> State {
    STATE.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

fn set_state(s: State) {
    *STATE.lock().unwrap_or_else(|e| e.into_inner()) = s;
}

/// メニュー項目の表示。状態に応じて変わる
pub fn menu_title() -> String {
    menu_title_with_reason().0
}

/// メニュー項目の表示と、失敗時の理由(メニュー項目のツールチップ用)。Failed の
/// 理由は通知だと数秒で消えるため、メニューバーの項目へはこちらで理由も載せる
/// (設定画面のアップデートボタンは廃止したため、退避先はメニュー項目のみ)
pub fn menu_title_with_reason() -> (String, Option<String>) {
    match state() {
        State::Idle => ("アップデートを確認…".into(), None),
        State::Checking => ("アップデートを確認中…".into(), None),
        State::Available { version, .. } => (format!("Knit {version} に更新…"), None),
        State::Installing => ("更新を準備中…".into(), None),
        State::Failed(reason) => ("更新を確認できません".into(), Some(reason)),
    }
}

fn platform() -> &'static str {
    if cfg!(target_arch = "aarch64") {
        "macos-arm64"
    } else {
        "macos-x64"
    }
}

/// 更新が見つかった時の通知文言。署名検証(ed25519)を通った更新だけが
/// ここへ来るため、成功時に検証が見えるように検証済みであることを書く
fn available_text(version: &str) -> String {
    format!("Knit {version} が利用できます(署名を確認した更新です・ed25519 検証済み)。メニューから更新できます")
}

/// 試験用の差し替えはデバッグビルドに限る(配布版では常に組み込みの置き場と鍵だけを使う)
fn manifest_url() -> String {
    if cfg!(debug_assertions) {
        if let Some(u) = crate::envutil::get("KNIT_UPDATE_URL") {
            return u;
        }
    }
    update::MANIFEST_URL.to_string()
}

fn trusted_keys() -> Vec<[u8; 32]> {
    let mut keys = update::trusted_keys();
    if cfg!(debug_assertions) {
        if let Some(k) = crate::envutil::get("KNIT_UPDATE_PUBKEY")
            .and_then(|h| update::from_hex(h.trim()))
            .and_then(|b| b.try_into().ok())
        {
            keys.push(k);
        }
    }
    keys
}

fn curl(url: &str, dest: Option<&Path>, max: u64) -> Result<Vec<u8>, String> {
    update::curl("/usr/bin/curl", url, dest, max, &format!("Knit/{CURRENT}"))
}

type Available = update::Available;

/// `Ok(None)` は最新。署名の検証に成功した更新情報だけを信用する
fn check() -> Result<Option<Available>, String> {
    let url = manifest_url();
    let keys = trusted_keys();
    update::check_with(
        &update::CheckConfig {
            manifest_url: &url,
            keys: &keys,
            channel: CHANNEL,
            current: CURRENT,
            platform: platform(),
        },
        &|u, max| curl(u, None, max),
    )
}

/// 実行中の .app(Contents/MacOS/Knit の3階層上)。.app 以外(開発ビルド)では None
fn running_bundle() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?.canonicalize().ok()?;
    let app = exe.ancestors().nth(3)?.to_path_buf();
    (app.extension()? == "app").then_some(app)
}

fn run_out(cmd: &str, args: &[&std::ffi::OsStr]) -> Option<(bool, String)> {
    let o = Command::new(cmd).args(args).stdin(Stdio::null()).output().ok()?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
    Some((o.status.success(), text))
}

fn team_id(app: &Path) -> Option<String> {
    let (_, text) = run_out("/usr/bin/codesign", &["-dv".as_ref(), app.as_os_str()])?;
    text.lines()
        .find_map(|l| l.strip_prefix("TeamIdentifier="))
        .filter(|t| *t != "not set")
        .map(str::to_string)
}

fn plist_string(app: &Path, key: &str) -> Option<String> {
    let plist = app.join("Contents/Info.plist");
    let (ok, text) = run_out(
        "/usr/bin/plutil",
        &["-extract".as_ref(), key.as_ref(), "raw".as_ref(), "-o".as_ref(), "-".as_ref(), plist.as_os_str()],
    )?;
    ok.then(|| text.trim().to_string())
}

/// 展開した新しい .app が、宣言どおりの Knit か・署名が壊れていないか・署名者が変わっていないかを確認する
fn verify_bundle(new: &Path, current: &Path, version: &str) -> Result<(), String> {
    if plist_string(new, "CFBundleIdentifier").as_deref() != Some(BUNDLE_ID) {
        return Err("更新ファイルがKnitではありません".into());
    }
    if plist_string(new, "CFBundleShortVersionString").as_deref() != Some(version) {
        return Err("更新ファイルの版が更新情報と一致しません".into());
    }
    let (ok, _) = run_out(
        "/usr/bin/codesign",
        &["--verify".as_ref(), "--deep".as_ref(), "--strict".as_ref(), new.as_os_str()],
    )
    .ok_or("署名を確認できません")?;
    if !ok {
        return Err("更新ファイルの署名が不正です".into());
    }
    // 署名者が変わると macOS はアクセシビリティ許可を引き継がない。
    // 現行版が正式署名なら、新版も同じ署名者であることを求める
    if let Some(cur) = team_id(current) {
        if team_id(new).as_deref() != Some(cur.as_str()) {
            return Err("更新ファイルの署名者が現在と異なるため中止しました".into());
        }
    }
    Ok(())
}

/// 終了後に入れ替えを行う。新版が起動しなければ旧版へ戻す。
/// 引数: <終了を待つ PID> <新しい .app> <置き換え先の .app> <起動方法> <起動確認の待ち秒数>
const APPLY_SCRIPT: &str = r#"#!/bin/bash
PID="$1"; NEW="$2"; TARGET="$3"; MODE="$4"; WAIT="$5"
OLD="$TARGET.knit-old"
STAGE="$(dirname "$NEW")"
BIN="$TARGET/Contents/MacOS/Knit"
notify() { [ "$MODE" = exec ] || osascript -e "display notification "$1" with title "Knit"" >/dev/null 2>&1; }
start() {
  case "$MODE" in
    launchd) launchctl kickstart -k "gui/$(id -u)/local.knit" >/dev/null 2>&1 || open "$TARGET" ;;
    open) open "$TARGET" ;;
    exec) "$BIN" >/dev/null 2>&1 & ;;
  esac
}
# パスに ( や + が含まれても正しく判定できるよう、正規表現ではなく固定文字列で照合する
pids() { ps -axo pid=,command= | while read -r p c; do case "$c" in *"$BIN"*) echo "$p" ;; esac; done; }
alive() { [ -n "$(pids)" ]; }
cleanup() { case "$(basename "$STAGE")" in .knit-update-*) rm -rf "$STAGE" ;; esac; }
i=0
while kill -0 "$PID" 2>/dev/null; do
  i=$((i+1)); [ "$i" -gt 150 ] && { notify "更新できませんでした。Knitを終了できなかったため、現在の版のままです"; cleanup; exit 1; }; sleep 0.1
done
rm -rf "$OLD"
mv "$TARGET" "$OLD" || { notify "更新できませんでした。現在の版のままです"; start; cleanup; exit 1; }
if ! mv "$NEW" "$TARGET"; then mv "$OLD" "$TARGET"; notify "更新できませんでした。元の版へ戻しました"; start; cleanup; exit 1; fi
start
sleep "$WAIT"
if alive; then rm -rf "$OLD"; notify "Knitを更新しました"; cleanup; exit 0; fi
for p in $(pids); do kill "$p" 2>/dev/null; done
rm -rf "$TARGET"; mv "$OLD" "$TARGET"; notify "新しい版が起動しなかったため、元の版へ戻しました"; start; cleanup; exit 1
"#;

fn start_mode(target: &Path) -> &'static str {
    // 試験用(scripts/tests/update-e2e.sh)。起動中の本物の Knit を巻き込まないよう直接起動する
    if cfg!(debug_assertions) && crate::envutil::get("KNIT_UPDATE_START_MODE").as_deref() == Some("exec") {
        return "exec";
    }
    let plist = std::env::var_os("HOME")
        .map(|h| PathBuf::from(h).join("Library/LaunchAgents/local.knit.plist"))
        .and_then(|p| std::fs::read_to_string(p).ok())
        .unwrap_or_default();
    let bin = target.join("Contents/MacOS/Knit");
    if plist.contains(bin.to_string_lossy().as_ref()) {
        "launchd"
    } else {
        "open"
    }
}

fn spawn_apply(script: &Path, args: &[&std::ffi::OsStr]) -> std::io::Result<()> {
    use std::os::unix::process::CommandExt;
    // 別プロセスグループにして、LaunchAgent の終了時に一緒に止められないようにする
    Command::new("/bin/bash")
        .arg(script)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .map(|_| ())
}

/// 取得・検証・展開まで行い、入れ替えを別プロセスへ渡す。成功したら呼び出し側がアプリを終了する
fn install(a: &Available) -> Result<(), String> {
    let target = running_bundle().ok_or("アプリ版でのみ更新できます")?;
    let parent = target.parent().ok_or("インストール先を特定できません")?;
    // 入れ替えを原子的にするため、置き換え先と同じ場所へ展開する(書き込み権限の確認も兼ねる)
    let stage = parent.join(format!(".knit-update-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&stage);
    std::fs::create_dir(&stage).map_err(|_| "インストール先に書き込めません。更新は行いません".to_string())?;
    let result = (|| {
        let zip = stage.join("Knit.zip");
        curl(&a.artifact.url, Some(&zip), a.artifact.size)?;
        let file = std::fs::File::open(&zip).map_err(|_| update::UpdateError::Io.message().to_string())?;
        update::verify_artifact(file, &a.artifact).map_err(|e| e.message().to_string())?;
        let out = stage.join("x");
        let (ok, _) = run_out(
            "/usr/bin/ditto",
            &["-x".as_ref(), "-k".as_ref(), zip.as_os_str(), out.as_os_str()],
        )
        .ok_or("展開を開始できません")?;
        if !ok {
            return Err("更新ファイルを展開できません".to_string());
        }
        // 配布 zip は「版付きフォルダ/Knit.app + release-manifest.json」の構成
        let pkg = update::find_package_dir(&out)?;
        update::check_release_manifest(&pkg, &a.version, platform())?;
        let new = pkg.join("Knit.app");
        // シンボリックリンクではなく実体のフォルダだけを受け付ける
        if !std::fs::symlink_metadata(&new).map(|m| m.is_dir()).unwrap_or(false) {
            return Err("更新ファイルにKnit.appがありません".to_string());
        }
        verify_bundle(&new, &target, &a.version)?;
        let staged = stage.join("Knit.app");
        std::fs::rename(&new, &staged).map_err(|_| "更新ファイルを配置できません".to_string())?;
        let script = stage.join("apply.sh");
        std::fs::write(&script, APPLY_SCRIPT).map_err(|_| "更新の準備に失敗しました".to_string())?;
        let pid = std::process::id().to_string();
        spawn_apply(
            &script,
            &[
                pid.as_ref(),
                staged.as_os_str(),
                target.as_os_str(),
                start_mode(&target).as_ref(),
                "6".as_ref(),
            ],
        )
        .map_err(|_| "更新の適用を開始できません".to_string())
    })();
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&stage);
    }
    result
}

/// メニュー項目が押された時。確認 → (見つかっていれば)更新 の二段階
pub fn on_click() {
    match state() {
        State::Idle => {
            set_state(State::Checking);
            std::thread::spawn(|| {
                match check() {
                    Ok(Some(a)) => {
                        crate::notify("Knit", &available_text(&a.version));
                        set_state(State::Available { version: a.version, artifact: a.artifact });
                    }
                    Ok(None) => {
                        crate::notify("Knit", &format!("最新の版です(Knit {CURRENT})"));
                        set_state(State::Idle);
                    }
                    Err(e) => {
                        crate::notify("Knit", &e);
                        // Idle へ戻すと通知だけで失敗が消え、ボタンが何事も無かった
                        // かのように戻るため、失敗の表示を次のクリックまで残す
                        set_state(State::Failed(e));
                    }
                }
            });
        }
        // 失敗表示のまま押されたら最初からやり直す(1 段の再帰で必ず Idle へ着く)。
        // 通知は数秒で消えるため、前回の失敗理由はログへ残す(問い合わせの確認用)
        State::Failed(reason) => {
            eprintln!("[update] 確認を再試行します(前回の失敗理由: {reason})");
            set_state(State::Idle);
            on_click();
        }
        State::Available { version, artifact } => {
            set_state(State::Installing);
            std::thread::spawn(move || match install(&Available { version, artifact }) {
                Ok(()) => {
                    // 署名者(Team)が無い版同士の更新では、macOS がアクセシビリティ許可を引き継がない
                    let note = if running_bundle().is_some_and(|b| team_id(&b).is_none()) {
                        "更新を適用します。Knitは自動で再起動します。許可が外れた場合は、システム設定でアクセシビリティを許可し直してください"
                    } else {
                        "更新を適用します。Knitは自動で再起動します"
                    };
                    crate::notify("Knit", note);
                    crate::request_quit_for_update();
                }
                Err(e) => {
                    crate::notify("Knit", &e);
                    // 確認時と同じく、失敗が通知だけで消えないように表示を残す
                    set_state(State::Failed(e));
                }
            });
        }
        State::Checking | State::Installing => {}
    }
}

/// 同じ .app から起動した Knit が、この CLI 以外にも動いているか
fn other_instance_running(app: &Path) -> bool {
    let bin = app.join("Contents/MacOS/Knit").to_string_lossy().to_string();
    let me = std::process::id().to_string();
    run_out("/bin/ps", &["-axo".as_ref(), "pid=,command=".as_ref()])
        .map(|(_, text)| {
            text.lines().any(|l| {
                let l = l.trim_start();
                let (pid, cmd) = l.split_once(' ').unwrap_or((l, ""));
                pid != me && (cmd.trim_start() == bin || cmd.trim_start().starts_with(&format!("{bin} ")))
            })
        })
        .unwrap_or(false)
}

/// `knit-mac --update`: 端末から確認して更新する(メニューと同じ処理)。終了コード 0=最新/適用開始
pub fn run_cli() -> i32 {
    // 起動中の Knit は、入れ替えの前に終了しない(別プロセスが待つのはこの CLI だけ)。
    // 旧版が動いたまま新版を置くことになるため、先に終了してもらう
    if let Some(app) = running_bundle() {
        if other_instance_running(&app) {
            eprintln!("Knit が起動中です。メニューバーの「アップデートを確認…」から更新するか、Knit を終了してから実行してください");
            return 1;
        }
    }
    match check() {
        Ok(None) => {
            println!("最新の版です(Knit {CURRENT})");
            0
        }
        Ok(Some(a)) => {
            println!("Knit {} に更新します", a.version);
            match install(&a) {
                Ok(()) => {
                    println!("更新を適用します。終了後に自動で入れ替わります");
                    0
                }
                Err(e) => {
                    eprintln!("{e}");
                    1
                }
            }
        }
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}

/// 起動後に静かに確認する。新しい版があれば1度だけ通知し、メニューを更新項目にする。
/// 署名鍵が未登録の間、および KNIT_AUTO_UPDATE=0 の時は何もしない
/// 前回の更新適用中に強制終了した際のステージ残留(.knit-update-*)を掃除する。
/// 適用は数分で終わるため、60分より古いものは失敗した残留とみなす
fn cleanup_leftovers() {
    let Some(self_path) = std::env::current_exe().ok().map(std::path::PathBuf::from) else {
        return;
    };
    let Some(app_dir) = self_path.parent().and_then(std::path::Path::parent) else {
        return;
    };
    let Ok(entries) = std::fs::read_dir(app_dir) else {
        return;
    };
    for e in entries.flatten() {
        let name = e.file_name();
        let name = name.to_string_lossy();
        if name.starts_with(".knit-update-") {
            let stale = e
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.elapsed().ok())
                .is_some_and(|d| d.as_secs() > 60 * 60);
            if stale {
                eprintln!("[update] 残留ステージを削除: {}", e.path().display());
                let _ = std::fs::remove_dir_all(e.path());
            }
        }
    }
}

pub fn start_background() {
    cleanup_leftovers();
    if crate::envutil::get("KNIT_AUTO_UPDATE").as_deref() == Some("0") || trusted_keys().is_empty() {
        return;
    }
    std::thread::spawn(|| {
        std::thread::sleep(FIRST_CHECK_DELAY);
        loop {
            // Failed 中も自動確認は回す: 失敗表示の解除をクリック待ちにすると、
            // 6 時間毎の確認が一度の失敗で止まってしまうため(新しい版が見つかれば
            // Available が Failed を上書きする。エラー時は通知を増やさず何もしない)
            if matches!(state(), State::Idle | State::Failed(_)) {
                if let Ok(Some(a)) = check() {
                    let mut n = NOTIFIED.lock().unwrap_or_else(|e| e.into_inner());
                    if *n != a.version {
                        *n = a.version.clone();
                        crate::notify("Knit", &available_text(&a.version));
                    }
                    if matches!(state(), State::Idle | State::Failed(_)) {
                        set_state(State::Available { version: a.version, artifact: a.artifact });
                    }
                }
            }
            std::thread::sleep(CHECK_INTERVAL);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 偽の .app(Contents/MacOS/Knit が run の内容)を作る
    fn fake_app(dir: &Path, name: &str, body: &str) -> PathBuf {
        let app = dir.join(name);
        let bin = app.join("Contents/MacOS");
        std::fs::create_dir_all(&bin).unwrap();
        let exe = bin.join("Knit");
        std::fs::write(&exe, format!("#!/bin/bash\n{body}\n")).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
        app
    }

    fn apply(tag: &str, new_body: &str) -> (PathBuf, bool) {
        // パスに ( ) + [ ] を含めても、起動確認が正しく動くことも確かめる
        let root = std::env::temp_dir().join(format!("knit-apply-({tag})+[x]-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let stage = root.join(".knit-update-1");
        std::fs::create_dir_all(&stage).unwrap();
        let target = fake_app(&root, "Knit.app", "echo old-version; sleep 30");
        let new = fake_app(&stage, "Knit.app", new_body);
        let script = stage.join("apply.sh");
        std::fs::write(&script, APPLY_SCRIPT).unwrap();
        // 存在しない PID = すでに終了済みの旧プロセス
        let ok = Command::new("/bin/bash")
            .arg(&script)
            .args(["999999999", new.to_str().unwrap(), target.to_str().unwrap(), "exec", "1"])
            .status()
            .unwrap()
            .success();
        (root, ok)
    }

    fn version_of(root: &Path) -> String {
        std::fs::read_to_string(root.join("Knit.app/Contents/MacOS/Knit")).unwrap()
    }

    fn cleanup(root: &Path) {
        let _ = Command::new("/usr/bin/pkill")
            .args(["-f", "--", &root.to_string_lossy()])
            .status();
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn apply_swaps_in_the_new_version_and_removes_leftovers() {
        let (root, ok) = apply("ok", "sleep 30 # new-version");
        assert!(ok);
        assert!(version_of(&root).contains("new-version"));
        assert!(!root.join("Knit.app.knit-old").exists());
        assert!(!root.join(".knit-update-1").exists());
        cleanup(&root);
    }

    #[test]
    fn apply_rolls_back_when_the_new_version_does_not_stay_running() {
        let (root, ok) = apply("rollback", "exit 1 # new-version");
        assert!(!ok);
        assert!(version_of(&root).contains("old-version"));
        assert!(!root.join("Knit.app.knit-old").exists());
        cleanup(&root);
    }

    #[test]
    fn menu_title_follows_state() {
        set_state(State::Idle);
        assert_eq!(menu_title(), "アップデートを確認…");
        set_state(State::Available {
            version: "9.9.9".into(),
            artifact: Artifact { platform: "p".into(), url: "u".into(), size: 1, sha256: "0".into() },
        });
        assert_eq!(menu_title(), "Knit 9.9.9 に更新…");
        set_state(State::Idle);
    }

    /// 確認の失敗は通知だけで消えず、項目名が「更新を確認できません」へ切り替わる
    /// (理由の詳細は通知で既に出ているため、項目は状態だけ示す)。
    /// STATE は共有 static のため、このテストは状態遷移を 1 本にまとめる
    #[test]
    fn failed_state_keeps_title_until_next_click() {
        set_state(State::Idle);
        set_state(State::Failed("確認できませんでした".into()));
        assert_eq!(menu_title(), "更新を確認できません");
        // 次のクリックで Idle へ戻る(=再試行の導線が復活する)
        if let State::Failed(_) = state() {
            set_state(State::Idle);
        }
        assert_eq!(menu_title(), "アップデートを確認…");
        set_state(State::Idle);
    }
}

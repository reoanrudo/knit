//! Android タブレットを接続先の 1 台として扱う中継。
//!
//! Android へは adb(ワイヤレスデバッグ)で scrcpy のサーバー部品を送り込み、端末側に
//! 仮想のキーボードとマウス(UHID)を作って入力を流す。専用アプリの導入は要らない。
//! 中継自身は 127.0.0.1 から Knit の本線へ Windows と同じ手順(暗号化ハンドシェイク→hello)で
//! 接続するため、画面端での切替・接続先メニュー・複数台の切替は既存の仕組みがそのまま効く。
//!
//! 見つけたタブレットは黙って操作対象にしない。有線(USB)・ワイヤレスとも、利用者が
//! メニューで端末ごとに「操作する」を選んだものだけへ繋ぎ、選び直せば取り消せる(state.rs)。
//!
//! 前提: `adb`(android-platform-tools)と scrcpy のサーバー部品(scrcpy に同梱)が Mac にあり、
//! タブレットで USB デバッグ(有線)またはワイヤレスデバッグを許可していること。KNIT_ANDROID=0 で無効

mod hid;
pub(crate) mod display;
pub(crate) mod app;
mod gestures;
mod scrcpy;
pub mod state;

use state::{Choice, Link, Phase, Tablet};

use knit_common::envutil;
use knit_common::proto::{decode, encode, Monitor, Msg, VERSION};
use knit_common::secure;
use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, BufWriter, Read, Write};
use std::net::{Shutdown, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

/// 端末へ送り込むサーバー部品の置き場所(scrcpy 本体と重ならない名前にする)
const DEVICE_JAR: &str = "/data/local/tmp/knit-scrcpy-server.jar";

/// クリップボード画像設定ヘルパー(app_process で動く dex。apk インストール不要)。
/// スクリプト scripts/build-clip-helper.sh で生成する
const CLIP_JAR_PATH: &str = "/data/local/tmp/knit-clip.jar";
const CLIP_JAR: &[u8] = include_bytes!("android/clip-setter.jar");

type Stopper = Arc<dyn Fn() + Send + Sync>;

/// 端末ごとの接続の進み具合(監視スレッドと接続スレッドで共有する)
struct Live {
    phase: Phase,
    running: bool,
    stop: Option<Stopper>,
    failures: u32,
    next_try: Instant,
}

static LIVE: Mutex<Option<HashMap<String, Live>>> = Mutex::new(None);

fn with_live<T>(f: impl FnOnce(&mut HashMap<String, Live>) -> T) -> T {
    let mut g = LIVE.lock().unwrap_or_else(|e| e.into_inner());
    f(g.get_or_insert_with(HashMap::new))
}

fn set_phase(key: &str, phase: Phase) {
    with_live(|m| {
        if let Some(l) = m.get_mut(key) {
            l.phase = phase;
        }
    });
    state::bump();
}

/// 同じ知らせを 1 起動につき 1 回だけ出す
fn notify_once(key: &str, kind: &str, body: &str) {
    static SEEN: Mutex<Option<HashSet<String>>> = Mutex::new(None);
    let mut g = SEEN.lock().unwrap_or_else(|e| e.into_inner());
    if g.get_or_insert_with(HashSet::new)
        .insert(format!("{kind}\t{key}"))
    {
        crate::notify("Knit", body);
    }
}

/// adb から見えた 1 台
#[derive(Debug, PartialEq)]
struct Seen {
    serial: String,
    state: String,
    model: Option<String>,
}

/// 端末を識別する情報(接続中は adb で読み直さない)
#[derive(Clone)]
struct Ident {
    key: String,
    name: String,
}

pub fn spawn(port: u16, token: String) {
    let mode = envutil::get("KNIT_ANDROID").unwrap_or_default();
    if mode == "0" || mode.eq_ignore_ascii_case("off") {
        state::publish(state::Status {
            problem: Some("Android 接続は無効です(KNIT_ANDROID=0)".into()),
            tablets: Vec::new(),
        });
        return;
    }
    std::thread::spawn(move || {
        let mut idents: HashMap<String, Ident> = HashMap::new();
        let mut last_mdns = Instant::now() - Duration::from_secs(60);
        let mut logged = false;
        loop {
            // 導入が後からでも拾えるよう、見つかるまで毎回探す
            let tools = find_adb().zip(find_server_jar());
            let Some((adb, jar)) = tools else {
                state::publish(state::Status {
                    problem: Some(if find_adb().is_none() {
                        "adb がありません(brew install android-platform-tools)".into()
                    } else {
                        "scrcpy がありません(brew install scrcpy)".into()
                    }),
                    tablets: Vec::new(),
                });
                std::thread::sleep(Duration::from_secs(10));
                continue;
            };
            if !logged {
                logged = true;
                eprintln!(
                    "[android] 待機中(adb={} server={})",
                    adb.display(),
                    jar.display()
                );
            }
            let seen = list_devices(&adb);
            idents.retain(|serial, _| seen.iter().any(|d| &d.serial == serial));
            let mut tablets = Vec::new();
            for d in &seen {
                let link = Link::of(&d.serial);
                let fallback_name = d.model.clone().unwrap_or_else(|| "Android 端末".into());
                if d.state != "device" {
                    let phase = if d.state == "unauthorized" {
                        notify_once(
                            &d.serial,
                            "unauthorized",
                            "タブレットの画面で「USB デバッグを許可」を選んでください",
                        );
                        Phase::Unauthorized
                    } else {
                        Phase::Offline
                    };
                    tablets.push(Tablet {
                        key: d.serial.clone(),
                        name: fallback_name,
                        link,
                        phase,
                    });
                    continue;
                }
                let id = idents
                    .entry(d.serial.clone())
                    .or_insert_with(|| identify(&adb, &d.serial, &fallback_name))
                    .clone();
                let phase = match state::choice(&id.key) {
                    None => {
                        notify_once(
                            &id.key,
                            "ask",
                            &format!("{} が見つかりました。タブレットの Knit アプリから接続してください(adb で操作する場合は KNIT_ANDROID_ADB=1)", id.name),
                        );
                        Phase::AskUser
                    }
                    Some(Choice::Deny) => Phase::Declined,
                    Some(Choice::Allow) => {
                        let start = with_live(|m| {
                            let l = m.entry(id.key.clone()).or_insert_with(|| Live {
                                phase: Phase::Connecting,
                                running: false,
                                stop: None,
                                failures: 0,
                                next_try: Instant::now(),
                            });
                            let start = !l.running && Instant::now() >= l.next_try;
                            if start {
                                l.running = true;
                                l.phase = Phase::Connecting;
                            }
                            start
                        });
                        if start {
                            let (adb, jar, token, serial, id) = (
                                adb.clone(),
                                jar.clone(),
                                token.clone(),
                                d.serial.clone(),
                                id.clone(),
                            );
                            std::thread::spawn(move || {
                                session_thread(&adb, &serial, &jar, port, &token, &id)
                            });
                        }
                        with_live(|m| m.get(&id.key).map(|l| l.phase.clone()))
                            .unwrap_or(Phase::Connecting)
                    }
                };
                tablets.push(Tablet {
                    key: id.key,
                    name: id.name,
                    link,
                    phase,
                });
            }
            state::publish(state::Status {
                problem: None,
                tablets,
            });
            // ペア済みの端末はワイヤレスデバッグのたびにポートが変わる。mDNS で探して繋ぐ
            if !seen.iter().any(|d| d.state == "device")
                && last_mdns.elapsed() >= Duration::from_secs(10)
            {
                last_mdns = Instant::now();
                mdns_connect(&adb);
            }
            std::thread::sleep(Duration::from_secs(3));
        }
    });
}

fn identify(adb: &Path, serial: &str, fallback: &str) -> Ident {
    let name = getprop(adb, serial, "ro.product.marketname")
        .or_else(|| getprop(adb, serial, "ro.product.model"))
        .unwrap_or_else(|| fallback.to_string());
    Ident {
        key: device_key(adb, serial),
        name: knit_common::proto::safe_peer_name(&name),
    }
}

fn session_thread(adb: &Path, serial: &str, jar: &Path, port: u16, token: &str, id: &Ident) {
    // 二重チェック: キャッシュ参照だけをロック内で行い、未取得時の adb 実行
    //(最大15秒)はロック外で行う。保持したまま実行すると、複数タブレットの
    // 同時初回接続が直列に詰まる
    static VERSION: Mutex<Option<String>> = Mutex::new(None);
    let ver = {
        let v = VERSION.lock().unwrap_or_else(|e| e.into_inner()).clone();
        match v {
            Some(v) => Some(v),
            None => {
                let got = server_version(adb, serial, jar);
                if got.is_some() {
                    *VERSION.lock().unwrap_or_else(|e| e.into_inner()) = got.clone();
                }
                got
            }
        }
    };
    let started = Instant::now();
    let res = match ver {
        Some(ver) => run_session(adb, serial, jar, &ver, port, token, id),
        None => Err("scrcpy の版を特定できません(KNIT_SCRCPY_VERSION で指定できます)".into()),
    };
    // クリップボード共有の送り先登録を解除(別セッションが上書きしていたら触らない)
    unregister_push_target(serial);
    match &res {
        Ok(()) => eprintln!("[android] {} との接続が終わりました", id.name),
        Err(e) => eprintln!("[android] {}: {e}", id.name),
    }
    with_live(|m| {
        if let Some(l) = m.get_mut(&id.key) {
            l.running = false;
            l.stop = None;
            // すぐ切れる失敗が続く時は間隔を広げる(送り込みとログを繰り返さない)
            l.failures = if res.is_err() && started.elapsed() < Duration::from_secs(30) {
                (l.failures + 1).min(5)
            } else {
                0
            };
            l.next_try = Instant::now()
                + Duration::from_secs(if l.failures == 0 { 1 } else { 3 << l.failures });
            l.phase = match res {
                Ok(()) => Phase::Connecting,
                Err(e) => Phase::Failed(e.chars().take(60).collect()),
            };
        }
    });
    state::bump();
}

fn find_adb() -> Option<PathBuf> {
    if let Some(p) = envutil::get("KNIT_ADB") {
        return Some(PathBuf::from(p));
    }
    // LaunchAgent から起動すると PATH が最小限になるため、既定の置き場所も直接見る
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    let mut cands: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).map(|d| d.join("adb")).collect())
        .unwrap_or_default();
    cands.extend([
        PathBuf::from("/opt/homebrew/bin/adb"),
        PathBuf::from("/usr/local/bin/adb"),
        home.join("Library/Android/sdk/platform-tools/adb"),
    ]);
    cands.into_iter().find(|p| p.is_file())
}

fn find_server_jar() -> Option<PathBuf> {
    if let Some(p) = envutil::get("KNIT_SCRCPY_SERVER") {
        return Some(PathBuf::from(p));
    }
    [
        "/opt/homebrew/share/scrcpy/scrcpy-server",
        "/usr/local/share/scrcpy/scrcpy-server",
    ]
    .iter()
    .map(PathBuf::from)
    .find(|p| p.is_file())
}

/// サーバー部品の版。Homebrew の実体パス(Cellar/scrcpy/<版>/…)から読み、
/// 読めなければ版の不一致エラーの文面から得る(サーバーは版が一致しないと動かない)
fn server_version(adb: &Path, serial: &str, jar: &Path) -> Option<String> {
    if let Some(v) = envutil::get("KNIT_SCRCPY_VERSION") {
        return Some(v);
    }
    if let Some(v) = std::fs::canonicalize(jar)
        .ok()
        .and_then(|p| version_from_cellar_path(&p))
    {
        return Some(v);
    }
    push_server(adb, serial, jar).ok()?;
    let out = adb_out(
        adb,
        serial,
        &[
            "shell",
            &format!("CLASSPATH={DEVICE_JAR}"),
            "app_process",
            "/",
            "com.genymobile.scrcpy.Server",
            "0",
        ],
        Duration::from_secs(15),
    )?;
    version_from_mismatch(&out)
}

fn version_from_cellar_path(p: &Path) -> Option<String> {
    let parts: Vec<_> = p.iter().map(|c| c.to_string_lossy().into_owned()).collect();
    let i = parts.iter().position(|c| c == "Cellar")?;
    let dir = (parts.get(i + 1)? == "scrcpy").then(|| parts.get(i + 2))??;
    // Homebrew は同じ版の作り直しに "_1" 等を付ける(4.1_1)。サーバーの版は "4.1"
    Some(match dir.rsplit_once('_') {
        Some((v, rev)) if !rev.is_empty() && rev.bytes().all(|b| b.is_ascii_digit()) => {
            v.to_string()
        }
        _ => dir.clone(),
    })
}

fn version_from_mismatch(out: &str) -> Option<String> {
    let rest = out.split("server version (").nth(1)?;
    Some(rest.split(')').next()?.to_string())
}

/// adb を期限付きで実行し、標準出力と標準エラーをまとめて返す(応答しない端末で止まらないため)
fn adb_run(adb: &Path, args: &[&str], timeout: Duration) -> Option<(bool, String)> {
    let mut child = Command::new(adb)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .ok()?;
    let mut out = child.stdout.take()?;
    let mut err = child.stderr.take()?;
    let t_out = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = out.read_to_string(&mut s);
        s
    });
    let t_err = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = err.read_to_string(&mut s);
        s
    });
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(st)) => break Some(st),
            Ok(None) if start.elapsed() < timeout => std::thread::sleep(Duration::from_millis(50)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    let text = t_out.join().unwrap_or_default() + &t_err.join().unwrap_or_default();
    Some((status.is_some_and(|s| s.success()), text))
}

fn adb_out(adb: &Path, serial: &str, args: &[&str], timeout: Duration) -> Option<String> {
    let mut all = vec!["-s", serial];
    all.extend_from_slice(args);
    adb_run(adb, &all, timeout).map(|(_, s)| s)
}

fn list_devices(adb: &Path) -> Vec<Seen> {
    adb_run(adb, &["devices", "-l"], Duration::from_secs(10))
        .map(|(_, out)| parse_devices(&out))
        .unwrap_or_default()
}

/// `adb devices -l` の各行: シリアル 状態 [usb:… product:… model:… …]
fn parse_devices(out: &str) -> Vec<Seen> {
    out.lines()
        .skip_while(|l| !l.starts_with("List of devices"))
        .skip(1)
        .filter_map(|l| {
            let mut it = l.split_whitespace();
            let serial = it.next()?.to_string();
            let state = it.next()?.to_string();
            let model = it
                .find_map(|t| t.strip_prefix("model:"))
                .map(|m| m.replace('_', " "));
            Some(Seen {
                serial,
                state,
                model,
            })
        })
        .collect()
}

fn mdns_connect(adb: &Path) {
    let Some((_, out)) = adb_run(adb, &["mdns", "services"], Duration::from_secs(10)) else {
        return;
    };
    for addr in parse_mdns(&out) {
        if let Some((_, r)) = adb_run(adb, &["connect", &addr], Duration::from_secs(10)) {
            eprintln!("[android] adb connect {addr}: {}", r.trim());
        }
    }
}

fn parse_mdns(out: &str) -> Vec<String> {
    out.lines()
        .filter(|l| l.contains("_adb-tls-connect._tcp"))
        .filter_map(|l| l.split_whitespace().last())
        .filter(|a| a.contains(':'))
        .map(str::to_string)
        .collect()
}

/// 現在の向きでの画面サイズ。dumpsys の cur= は回転後の値、wm size は回転前の値
fn screen_size(adb: &Path, serial: &str) -> Option<(i32, i32)> {
    let t = Duration::from_secs(10);
    if let Some(s) = adb_out(adb, serial, &["shell", "dumpsys", "window", "displays"], t)
        .and_then(|o| parse_cur_size(&o))
    {
        return Some(s);
    }
    adb_out(adb, serial, &["shell", "wm", "size"], t).and_then(|o| parse_wm_size(&o))
}

fn parse_wxh(s: &str) -> Option<(i32, i32)> {
    let (w, h) = s.split_once('x')?;
    let h: String = h.chars().take_while(|c| c.is_ascii_digit()).collect();
    Some((w.parse().ok()?, h.parse().ok()?))
}

fn parse_cur_size(out: &str) -> Option<(i32, i32)> {
    out.split_whitespace()
        .find_map(|t| t.strip_prefix("cur=").and_then(parse_wxh))
}

fn parse_wm_size(out: &str) -> Option<(i32, i32)> {
    let pick = |key: &str| {
        out.lines()
            .find_map(|l| l.trim().strip_prefix(key).map(|r| r.trim().to_string()))
            .and_then(|v| parse_wxh(&v))
    };
    pick("Override size:").or_else(|| pick("Physical size:"))
}

fn parse_density(out: &str) -> Option<u32> {
    let pick = |key: &str| {
        out.lines()
            .find_map(|l| l.trim().strip_prefix(key).map(|r| r.trim().to_string()))
            .and_then(|v| v.parse::<u32>().ok())
            .filter(|d| *d > 0)
    };
    pick("Override density:").or_else(|| pick("Physical density:"))
}

fn getprop(adb: &Path, serial: &str, key: &str) -> Option<String> {
    let v = adb_out(
        adb,
        serial,
        &["shell", "getprop", key],
        Duration::from_secs(10),
    )?;
    let v = v.trim();
    (!v.is_empty()).then(|| v.to_string())
}

fn push_server(adb: &Path, serial: &str, jar: &Path) -> Result<(), String> {
    let jar_s = jar.to_string_lossy();
    match adb_run(
        adb,
        &["-s", serial, "push", &jar_s, DEVICE_JAR],
        Duration::from_secs(30),
    ) {
        Some((true, _)) => Ok(()),
        Some((false, out)) => Err(format!("サーバー部品を送れません: {}", out.trim())),
        None => Err("adb を実行できません".into()),
    }
}

/// 端末ごとの安定した識別子(接続先メニューで再接続を同じ 1 台として扱うため)
fn device_key(adb: &Path, serial: &str) -> String {
    let base = getprop(adb, serial, "ro.serialno").unwrap_or_else(|| serial.to_string());
    let mut h: u64 = 0xcbf29ce484222325;
    for b in base.bytes() {
        h = (h ^ b as u64).wrapping_mul(0x100000001b3);
    }
    format!("android-{h:016x}")
}

/// セッション終了時に端末側のサーバーと adb の転送設定を片付ける
struct Cleanup<'a> {
    adb: &'a Path,
    serial: &'a str,
    local_ports: Vec<u16>,
    child: Option<Child>,
}

impl Drop for Cleanup<'_> {
    fn drop(&mut self) {
        if let Some(mut c) = self.child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
        for p in &self.local_ports {
            let _ = adb_run(
                self.adb,
                &[
                    "-s",
                    self.serial,
                    "forward",
                    "--remove",
                    &format!("tcp:{p}"),
                ],
                Duration::from_secs(5),
            );
        }
    }
}

fn run_session(
    adb: &Path,
    serial: &str,
    jar: &Path,
    version: &str,
    port: u16,
    token: &str,
    ident: &Ident,
) -> Result<(), String> {
    let (w, h) = screen_size(adb, serial).ok_or("画面サイズを取得できません")?;
    let name = ident.name.clone();
    let id = ident.key.clone();
    display::remember(&id, adb_out(adb, serial, &["shell", "dumpsys", "display"], Duration::from_secs(10)).and_then(|out| display::parse(&out)));
    // Android はマウスの移動量に画面密度(160dpi を 1 とする倍率)を掛けるため、
    // その分を割り引いて送る(エミュレーター 420dpi で実測: 補正なし 2.9 倍 → 補正後 0.97 倍)
    let gain = envutil::get("KNIT_ANDROID_GAIN")
        .and_then(|v| v.parse::<f64>().ok())
        .filter(|g| *g > 0.0)
        .or_else(|| {
            adb_out(
                adb,
                serial,
                &["shell", "wm", "density"],
                Duration::from_secs(10),
            )
            .and_then(|o| parse_density(&o))
            .map(|d| 160.0 / d as f64)
        })
        .unwrap_or(1.0);
    push_server(adb, serial, jar)?;
    // クリップボード画像のヘルパー dex も同じ場所へ(失敗しても操作には影響しない)
    {
        let tmp = std::env::temp_dir().join("knit-clip.jar");
        if std::fs::write(&tmp, CLIP_JAR).is_ok() {
            if let Some((false, out)) = adb_run(
                adb,
                &["-s", serial, "push", &tmp.to_string_lossy(), CLIP_JAR_PATH],
                Duration::from_secs(30),
            ) {
                eprintln!("[android] クリップボードヘルパーを送れません: {}", out.trim());
            }
        }
    }

    let scid = (std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos() ^ d.as_secs() as u32)
        .unwrap_or(1))
        & 0x7fff_ffff;
    let socket = format!("localabstract:scrcpy_{scid:08x}");
    let fwd = |n: usize| -> Result<u16, String> {
        match adb_run(
            adb,
            &["-s", serial, "forward", "tcp:0", &socket],
            Duration::from_secs(10),
        ) {
            Some((true, out)) => out
                .trim()
                .parse()
                .map_err(|_| format!("adb forward({n}): {out}")),
            Some((false, out)) => Err(format!("adb forward({n}): {}", out.trim())),
            None => Err("adb forward を実行できません".into()),
        }
    };
    // 音声と操作で 2 本開く(サーバーは video→audio→control の順に accept するため、
    // audio=true では audio への接続が必ず先。dummy byte も最初の accept へ送られる)
    let audio_port = fwd(1)?;
    let control_port = fwd(2)?;
    let mut cleanup = Cleanup {
        adb,
        serial,
        local_ports: vec![audio_port, control_port],
        child: None,
    };
    let mut child = Command::new(adb)
        .args(["-s", serial, "shell"])
        .arg(format!("CLASSPATH={DEVICE_JAR}"))
        .args(["app_process", "/", "com.genymobile.scrcpy.Server", version])
        .arg(format!("scid={scid:08x}"))
        .args([
            "log_level=warn",
            "tunnel_forward=true",
            "video=false",
            "audio=true",
            "audio_codec=raw",
            "control=true",
            "send_device_meta=false",
            "send_dummy_byte=true",
            "clipboard_autosync=true",
            "cleanup=true",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("サーバー部品を起動できません: {e}"))?;
    for pipe in [
        child
            .stdout
            .take()
            .map(|p| Box::new(p) as Box<dyn Read + Send>),
        child
            .stderr
            .take()
            .map(|p| Box::new(p) as Box<dyn Read + Send>),
    ]
    .into_iter()
    .flatten()
    {
        std::thread::spawn(move || {
            for line in BufReader::new(pipe).lines().map_while(Result::ok) {
                eprintln!("[android/scrcpy] {line}");
            }
        });
    }
    cleanup.child = Some(child);

    // audio ソケット(accept の先頭。dummy byte がここへ来る)
    let audio_sock = match connect_ready(audio_port) {
        Ok(s) => Some(s),
        Err(e) => {
            eprintln!("[android] 音声ソケット: {e}(音声なしで続行)");
            None
        }
    };
    // control ソケット(2 本目。dummy byte は audio 側へ行ったため読んで待たない)
    let control = connect_plain(control_port)?;
    control.set_nodelay(true).ok();
    control.set_read_timeout(None).ok();
    if let Some(sock) = audio_sock {
        let name = ident.name.clone();
        std::thread::spawn(move || audio_thread(sock, &name));
    }
    let mut ctl = BufWriter::new(control.try_clone().map_err(|e| e.to_string())?);
    let kb_name = format!("Knit keyboard ({})", hostname());
    let mouse_name = format!("Knit mouse ({})", hostname());
    ctl.write_all(&scrcpy::uhid_create(
        hid::KEYBOARD_ID,
        &kb_name,
        hid::KEYBOARD_DESC,
    ))
    .and_then(|_| {
        ctl.write_all(&scrcpy::uhid_create(
            hid::MOUSE_ID,
            &mouse_name,
            hid::MOUSE_DESC,
        ))
    })
    .and_then(|_| ctl.flush())
    .map_err(|e| format!("仮想キーボード・マウスを作れません: {e}"))?;

    // 準備の間に「操作しない」設定へ変わっていたら繋がない
    if state::choice(&id) != Some(Choice::Allow) {
        return Ok(());
    }
    // Knit の本線へ 1 台の接続先として入る
    let knit = TcpStream::connect(("127.0.0.1", port))
        .map_err(|e| format!("Knit へ接続できません: {e}"))?;
    knit.set_nodelay(true).ok();
    knit.set_write_timeout(Some(Duration::from_secs(5))).ok();
    knit.set_read_timeout(Some(Duration::from_secs(5))).ok();
    let knit_raw = knit.try_clone().map_err(|e| e.to_string())?;
    let (r, mut writer) = secure::connect(knit, token, b"knit-main")
        .map_err(|e| format!("Knit との暗号化ハンドシェイク失敗: {e}"))?;
    let hello = encode(&Msg::Hello {
        ver: VERSION,
        name: name.clone(),
        token: String::new(),
        w,
        h,
        id: id.clone(),
        monitors: vec![Monitor { x: 0, y: 0, w, h, name: String::new() }],
    });
    writer
        .write_all(hello.as_bytes())
        .and_then(|_| writer.flush())
        .map_err(|e| format!("hello を送れません: {e}"))?;
    let mut reader = BufReader::new(r);
    let mut line = String::new();
    reader
        .read_line(&mut line)
        .map_err(|e| format!("hello_ok を受け取れません: {e}"))?;
    if !matches!(decode(line.trim()), Some(Msg::HelloOk { .. })) {
        return Err("Knit から hello_ok が返りません".into());
    }
    // Knit は 3 秒毎に pong/ping を返すため、9 秒の無通信は経路断とみなす
    reader
        .get_ref()
        .set_read_timeout(Some(Duration::from_secs(9)));
    eprintln!("[android] {name} を接続先に加えました(画面 {w}x{h}、移動の補正 {gain:.3})");
    crate::notify("Knit", &format!("{name} を接続しました"));
    // クリップボードのファイル・画像共有の送り先として登録(セッション終了で外れる)
    register_push_target(adb, serial, &name);

    let done = Arc::new(AtomicBool::new(false));
    let stop = {
        let (a, b) = (
            knit_raw.try_clone().map_err(|e| e.to_string())?,
            control.try_clone().map_err(|e| e.to_string())?,
        );
        let done = done.clone();
        Arc::new(move || {
            done.store(true, Ordering::Relaxed);
            let _ = a.shutdown(Shutdown::Both);
            let _ = b.shutdown(Shutdown::Both);
        })
    };
    // メニューの「操作しない」でこの接続を切れるようにする
    {
        let stop: Stopper = stop.clone();
        with_live(|m| {
            if let Some(l) = m.get_mut(&id) {
                l.stop = Some(stop);
            }
        });
    }
    set_phase(&id, Phase::Connected);

    // Knit への送信は 1 本のスレッドへ集約する(並行 write で行が混ざらないように)
    let (tx, rx) = mpsc::channel::<String>();
    {
        let stop = stop.clone();
        std::thread::spawn(move || {
            while let Ok(line) = rx.recv() {
                if writer
                    .write_all(line.as_bytes())
                    .and_then(|_| writer.flush())
                    .is_err()
                {
                    break;
                }
            }
            stop();
        });
    }
    {
        let tx = tx.clone();
        let done = done.clone();
        std::thread::spawn(move || {
            while !done.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_secs(3));
                if tx.send(encode(&Msg::Ping { ts: 0 })).is_err() {
                    break;
                }
            }
        });
    }
    // 端末のクリップボード変化を Knit へ渡す(Knit から設定した直後の自分の値は返さない)
    let last_clip: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let clip_share = Arc::new(AtomicBool::new(true));
    {
        let mut rd = BufReader::new(control.try_clone().map_err(|e| e.to_string())?);
        let (tx, stop, last_clip, clip_share) = (
            tx.clone(),
            stop.clone(),
            last_clip.clone(),
            clip_share.clone(),
        );
        std::thread::spawn(move || {
            while let Ok(m) = scrcpy::read_device_msg(&mut rd) {
                if let scrcpy::DeviceMsg::Clipboard(text) = m {
                    let mut last = last_clip.lock().unwrap_or_else(|e| e.into_inner());
                    if last.as_deref() == Some(text.as_str()) || !clip_share.load(Ordering::Relaxed)
                    {
                        continue;
                    }
                    *last = Some(text.clone());
                    let _ = tx.send(encode(&Msg::Clip { text }));
                }
            }
            stop();
        });
    }
    // 画面の回転を見張る。向きが変わったら接続し直して新しい画面サイズを Knit へ伝える
    {
        let (adb, serial, stop, done) = (
            adb.to_path_buf(),
            serial.to_string(),
            stop.clone(),
            done.clone(),
        );
        std::thread::spawn(move || {
            while !done.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_secs(5));
                if done.load(Ordering::Relaxed) {
                    break;
                }
                match screen_size(&adb, &serial) {
                    Some(s) if s != (w, h) => {
                        eprintln!(
                            "[android] 画面の向きが変わりました({}x{})。接続し直します",
                            s.0, s.1
                        );
                        stop();
                        break;
                    }
                    Some(_) => {}
                    None => {
                        eprintln!("[android] 端末から応答がありません");
                        stop();
                        break;
                    }
                }
            }
        });
    }

    let mut kb = hid::Keyboard::default();
    let mut ptr = hid::Pointer::new(w as f64, h as f64, gain);
    let result = input_loop(
        &mut reader,
        &mut ctl,
        &tx,
        &mut kb,
        &mut ptr,
        &last_clip,
        &clip_share,
    );
    // 操作しない設定に変わって切れた時は失敗として扱わない
    let result = if state::choice(&id) == Some(Choice::Allow) {
        result
    } else {
        Ok(())
    };
    // 押しっぱなしを残さない
    let _ = ctl.write_all(&scrcpy::uhid_input(hid::KEYBOARD_ID, &kb.release_all()));
    let _ = ctl.write_all(&scrcpy::uhid_input(hid::MOUSE_ID, &ptr.release_all()));
    let _ = ctl.write_all(&scrcpy::uhid_destroy(hid::KEYBOARD_ID));
    let _ = ctl.write_all(&scrcpy::uhid_destroy(hid::MOUSE_ID));
    let _ = ctl.flush();
    stop();
    drop(cleanup);
    result
}

/// accept の先頭ソケット(audio)。サーバー部品の起動には 1 秒前後かかるため、
/// 最初の 1 バイト(dummy byte = 起動確認用)が読めるまで繋ぎ直す
fn connect_ready(port: u16) -> Result<TcpStream, String> {
    for _ in 0..100 {
        if let Ok(mut s) = TcpStream::connect(("127.0.0.1", port)) {
            s.set_read_timeout(Some(Duration::from_secs(2))).ok();
            let mut b = [0u8; 1];
            if matches!(s.read(&mut b), Ok(1)) {
                s.set_read_timeout(None).ok();
                return Ok(s);
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Err("端末側のサーバー部品に接続できません(ログの [android/scrcpy] を確認してください)".into())
}

/// accept の 2 本目以降(control)。dummy byte は先頭ソケットへ送られるため、
/// 起動確認は先頭ソケットの接続に任せ、ここでは接続の成立だけを待つ
/// (サーバーの accept 前でも adb の転送口に接続は成功し、書き込みは accept 後に届く)
fn connect_plain(port: u16) -> Result<TcpStream, String> {
    for _ in 0..100 {
        if let Ok(s) = TcpStream::connect(("127.0.0.1", port)) {
            return Ok(s);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Err("操作チャネルに接続できません(ログの [android/scrcpy] を確認してください)".into())
}

/// scrcpy の raw 音声(48000Hz/s16le/stereo)を Mac の再生へ直接流す。
/// scrcpy の形式: codec ヘッダ(4 バイト BE。0 はキャプチャ不可)の後、
/// [pts+フラグ(8 バイト BE) + サイズ(4 バイト BE)] + PCM が続く。フラグ bit62 は
/// 設定パケット(読み捨て)。再生はネットワーク経路と共通の ring へ(audio::feed_s16)。
/// ポート待受(24901)は accept が直列なため、外部の接続失敗が続くと接続まで
/// タイムアウトする実績があり、同一プロセス内なので直接渡す
fn audio_thread(sock: TcpStream, name: &str) {
    use std::io::{BufReader, Read};
    let mut r = BufReader::new(sock);
    let mut head = [0u8; 4];
    if r.read_exact(&mut head).is_err() {
        return;
    }
    match u32::from_be_bytes(head) {
        0x0072_6177 => {}
        0 => {
            eprintln!("[android] {name}: 端末が音声のキャプチャを許可しません(音声なしで続行)");
            return;
        }
        other => {
            eprintln!("[android] {name}: 未対応の音声形式(0x{other:08x})。音声なしで続行");
            return;
        }
    }
    eprintln!("[android] {name} の音声を Mac へ流します(48000Hz s16/stereo)");
    let mut hdr = [0u8; 12];
    // 診断: 最初のパケットが届くか(届かなければ端末がキャプチャを出していない)
    static FIRST_PACKET: std::sync::atomic::AtomicBool =
        std::sync::atomic::AtomicBool::new(false);
    // 診断: 届いた音の振幅(10 秒毎)。DRM 保護等のキャプチャ不可な音は無音で届く
    let mut peak_max: i16 = 0;
    let mut peak_at = std::time::Instant::now();
    loop {
        if r.read_exact(&mut hdr).is_err() {
            break;
        }
        let pts_flags = u64::from_be_bytes(hdr[..8].try_into().unwrap());
        let size = u32::from_be_bytes(hdr[8..].try_into().unwrap()) as usize;
        if !FIRST_PACKET.swap(true, Ordering::Relaxed) {
            eprintln!("[android] 音声パケット受信を確認(size={size})");
        }
        // bit62 は設定パケット(コーデック初期化データ。raw では通常来ない)
        if pts_flags & (1 << 62) != 0 {
            let mut dump = vec![0u8; size];
            if r.read_exact(&mut dump).is_err() {
                break;
            }
            continue;
        }
        if size == 0 || size > 128 * 1024 {
            continue;
        }
        let mut pcm = vec![0u8; size];
        if r.read_exact(&mut pcm).is_err() {
            break;
        }
        if let Some(p) = pcm
            .as_chunks::<2>().0.iter()
            .map(|b| i16::from_le_bytes([b[0], b[1]]).abs())
            .max()
        {
            peak_max = peak_max.max(p);
        }
        if peak_at.elapsed() >= Duration::from_secs(10) {
            eprintln!(
                "[android] 音声中継の状態: 振幅の最大={peak_max}{}",
                if peak_max < 100 { "(無音: 端末がキャプチャを許可しない音源の可能性)" } else { "" }
            );
            peak_max = 0;
            peak_at = std::time::Instant::now();
        }
        // タブレットの音は Windows 経路と加算ミックスして鳴らす(全部の音を重ねる)
        crate::audio::feed_s16(48000, &pcm);
    }
    // セッション終了: 第2レーンの在庫を捨てる(残りを鳴らさない)
    crate::audio::clear_lane_b();
    eprintln!("[android] {name} の音声を終了しました");
}

// ---------- ファイル送信(明示操作のみ): 設定画面のボタンから Download へ置く ----------
// クリップボードの画像・ファイル共有は Android 未対応(端末のクリップボードへ入れる
// 公開手段が無いため)。ここは利用者が明示的に選んだファイルを渡す経路のみ持つ

/// 送り先(操作中のセッションが登録する): adb の場所、シリアル、端末の表示名
static PUSH_TARGET: Mutex<Option<(PathBuf, String, String)>> = Mutex::new(None);

fn register_push_target(adb: &Path, serial: &str, name: &str) {
    *PUSH_TARGET.lock().unwrap_or_else(|e| e.into_inner()) =
        Some((adb.to_path_buf(), serial.to_string(), name.to_string()));
}

fn unregister_push_target(serial: &str) {
    let mut t = PUSH_TARGET.lock().unwrap_or_else(|e| e.into_inner());
    if t.as_ref().is_some_and(|(_, s, _)| s == serial) {
        *t = None;
    }
}

/// 選択されたファイルを端末の Download へ置く。成功した件数(未接続は None)
pub fn push_files(paths: &[std::path::PathBuf]) -> Option<usize> {
    let (adb, serial, name) = PUSH_TARGET.lock().unwrap_or_else(|e| e.into_inner()).clone()?;
    let mut ok = 0;
    for p in paths {
        let Some(file) = p.file_name() else { continue };
        let dest = format!("/sdcard/Download/{}", file.to_string_lossy());
        match adb_run(
            &adb,
            &["-s", &serial, "push", &p.to_string_lossy(), &dest],
            Duration::from_secs(120),
        ) {
            Some((true, _)) => ok += 1,
            _ => eprintln!(
                "[android] {name} へ {} を送れませんでした",
                file.to_string_lossy()
            ),
        }
    }
    (ok > 0).then_some(ok)
}

/// 端末のクリップボードへ画像を載せる(scrcpy のサーバー部品と同じ方式で
/// app_process から動かすヘルパー経由)。手順: 画像を /sdcard/Pictures/Knit へ
/// adb push すると MediaStore に自動登録されるので、その _id を content query で
/// 引き、content URI をクリップボードへ載せる=端末のアプリでそのまま貼り付けられる
pub fn set_clipboard_image(mime: &str, name: &str, data: &[u8]) -> bool {
    let Some((adb, serial, _)) = PUSH_TARGET.lock().unwrap_or_else(|e| e.into_inner()).clone()
    else {
        return false;
    };
    let local = std::env::temp_dir().join(name);
    if std::fs::write(&local, data).is_err() {
        return false;
    }
    let dev = format!("/sdcard/Pictures/Knit/{name}");
    // フォルダを用意して画像を置く(MediaStore への自動登録が走る)
    let _ = adb_run(
        &adb,
        &["-s", &serial, "shell", "mkdir", "-p", "/sdcard/Pictures/Knit"],
        Duration::from_secs(10),
    );
    let pushed = adb_run(
        &adb,
        &["-s", &serial, "push", &local.to_string_lossy(), &dev],
        Duration::from_secs(60),
    );
    let _ = std::fs::remove_file(&local);
    if !pushed.is_some_and(|(ok, _)| ok) {
        eprintln!("[android] クリップボード画像を端末へ送れませんでした");
        return false;
    }
    // MediaStore のスキャン反映を待って _id を引く(where は端末の shell で
    // 二重引用符に囲まれる形で渡す。無いと SQL のトークンとして壊れる)
    let mut id = None;
    let cond = format!("\"_display_name='{name}'\"");
    for _ in 0..12 {
        std::thread::sleep(Duration::from_millis(500));
        if let Some((true, out)) = adb_run(
            &adb,
            &[
                "-s",
                &serial,
                "shell",
                "content",
                "query",
                "--uri",
                "content://media/external/images/media",
                "--projection",
                "_id",
                "--where",
                &cond,
            ],
            Duration::from_secs(10),
        ) {
            if let Some(v) = out.split("_id=").nth(1) {
                let n: String = v.chars().take_while(|c| c.is_ascii_digit()).collect();
                if !n.is_empty() {
                    id = Some(n);
                    break;
                }
            }
        }
    }
    let Some(id) = id else {
        eprintln!("[android] 画像の MediaStore 登録を確認できませんでした");
        return false;
    };
    match adb_run(
        &adb,
        &[
            "-s",
            &serial,
            "shell",
            &format!("CLASSPATH={CLIP_JAR_PATH}"),
            "app_process",
            "/",
            "ClipSetter",
            &format!("content://media/external/images/media/{id}"),
            mime,
        ],
        Duration::from_secs(60),
    ) {
        Some((true, out)) if out.starts_with("ok") => true,
        Some((_, out)) => {
            eprintln!("[android] クリップボード画像の設定: {}", out.trim());
            false
        }
        None => false,
    }
}

#[allow(clippy::too_many_arguments)]
fn input_loop(
    reader: &mut BufReader<secure::Reader>,
    ctl: &mut BufWriter<TcpStream>,
    tx: &mpsc::Sender<String>,
    kb: &mut hid::Keyboard,
    ptr: &mut hid::Pointer,
    last_clip: &Mutex<Option<String>>,
    clip_share: &AtomicBool,
) -> Result<(), String> {
    let mut pinch = gestures::Pinch::default();
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => {
                let _ = gestures::write_to(ctl,pinch.finish(),ptr.size()).and_then(|_|ctl.flush());
                return Ok(());
            }
            Err(e) => {
                let _ = gestures::write_to(ctl,pinch.finish(),ptr.size()).and_then(|_|ctl.flush());
                return Err(format!("Knit との通信が切れました: {e}"));
            }
            Ok(_) => {}
        }
        let Some(msg) = decode(&line) else { continue };
        let mouse = |ctl: &mut BufWriter<TcpStream>, reps: Vec<[u8; 5]>| -> std::io::Result<()> {
            for r in reps {
                ctl.write_all(&scrcpy::uhid_input(hid::MOUSE_ID, &r))?;
            }
            Ok(())
        };
        let res = match msg {
            Msg::Pinch { delta, phase } => gestures::write_to(ctl,pinch.update(phase,delta,ptr.pos(),ptr.size(),crate::now_ms()),ptr.size()),
            Msg::TabletGesture { action } => gestures::write_to(ctl,pinch.finish(),ptr.size())
                .and_then(|_| ctl.write_all(&gestures::navigation_keys(action))),
            Msg::MouseAbs { nx, ny } => mouse(ctl, ptr.move_abs(nx, ny)),
            Msg::MouseMove { dx, dy } => mouse(ctl, ptr.move_rel(dx, dy)),
            Msg::Warp { nx, ny } => mouse(ctl, ptr.warp(nx, ny)),
            Msg::MouseButton { btn, down } => {
                mouse(ctl, ptr.button(btn, down).into_iter().collect())
            }
            Msg::Scroll { dx, dy } => {
                // UHID ホイールは整数ノッチのため Mac の 0.05 ノッチ刻みが切り捨てられ、
                // ゆっくりしたスクロールが粗くなる(1 ノッチ溜まるまで無反応)。
                // 浮動小数で送れるスクロール注入を使う。位置は推定位置を端末の生座標として
                // 渡す(video=false ではサーバーがそのまま使う)。
                // 縦の符号: 実機報告が逆向きと両方出たため、端末側(HyperOS)のスクロール
                // 方向設定の影響が疑われる。既定 -dy のまま KNIT_ANDROID_SCROLL_FLIP=1
                // で反転できる
                let v = if envutil::get("KNIT_ANDROID_SCROLL_FLIP").as_deref() == Some("1") {
                    dy
                } else {
                    -dy
                };
                let (x, y) = ptr.pos();
                let (sw, sh) = ptr.size();
                // 実機での届きの切り分け用に、セッションで最初の 1 回だけ記録する
                static SCROLL_SEEN: std::sync::atomic::AtomicBool =
                    std::sync::atomic::AtomicBool::new(false);
                if !SCROLL_SEEN.swap(true, Ordering::Relaxed) {
                    eprintln!("[android] スクロール注入を開始(位置 {x},{y} 画面 {sw}x{sh})");
                }
                ctl.write_all(&scrcpy::inject_scroll(x, y, sw, sh, dx, v))
            }
            Msg::Key {
                kc,
                down,
                ctrl,
                opt,
                cmd,
                shift,
                rcmd,
                ..
            } => {
                let m = hid::Mods {
                    ctrl,
                    opt,
                    cmd,
                    shift,
                    rcmd,
                };
                match kb.key(kc, down, m) {
                    Some(r) => ctl.write_all(&scrcpy::uhid_input(hid::KEYBOARD_ID, &r)),
                    None => Ok(()),
                }
            }
            Msg::Ime { kana } => {
                // Mac のかな/英数の状態を、画面を移った時に Android の入力モードへ反映する
                let kc = if kana { 104 } else { 102 };
                let mut out = Vec::new();
                for down in [true, false] {
                    if let Some(r) = kb.key(kc, down, hid::Mods::default()) {
                        out.extend(scrcpy::uhid_input(hid::KEYBOARD_ID, &r));
                    }
                }
                ctl.write_all(&out)
            }
            Msg::Leave => gestures::write_to(ctl,pinch.finish(),ptr.size()).and_then(|_| ctl
                .write_all(&scrcpy::uhid_input(hid::KEYBOARD_ID, &kb.release_all()))
                .and_then(|_| {
                    ctl.write_all(&scrcpy::uhid_input(hid::MOUSE_ID, &ptr.release_all()))
                })),
            Msg::Clip { text } => {
                if clip_share.load(Ordering::Relaxed) {
                    *last_clip.lock().unwrap_or_else(|e| e.into_inner()) = Some(text.clone());
                    ctl.write_all(&scrcpy::set_clipboard(&text))
                } else {
                    Ok(())
                }
            }
            Msg::Cfg { cmd_alt, clip, .. } => {
                kb.cmd_alt = cmd_alt;
                clip_share.store(clip, Ordering::Relaxed);
                Ok(())
            }
            Msg::Vol { op } => {
                let code = match op {
                    0 => Some(scrcpy::KEYCODE_VOLUME_UP),
                    1 => Some(scrcpy::KEYCODE_VOLUME_DOWN),
                    2 => Some(scrcpy::KEYCODE_VOLUME_MUTE),
                    _ => None,
                };
                match code {
                    Some(c) => ctl
                        .write_all(&scrcpy::inject_keycode(true, c))
                        .and_then(|_| ctl.write_all(&scrcpy::inject_keycode(false, c))),
                    None => Ok(()),
                }
            }
            Msg::Ping { ts } => {
                gestures::write_to(ctl,pinch.expire(crate::now_ms()),ptr.size()).map_err(|e|e.to_string())?;
                let _ = tx.send(encode(&Msg::Pong { ts }));
                Ok(())
            }
            Msg::Bye => {
                let _ = gestures::write_to(ctl,pinch.finish(),ptr.size()).and_then(|_|ctl.flush());
                return Ok(());
            }
            _ => Ok(()),
        };
        // 続きの行が届いていればまとめて送る(移動イベントごとの小さな書き込みを減らす)
        let res = res.and_then(|_| {
            if reader.buffer().is_empty() {
                ctl.flush()
            } else {
                Ok(())
            }
        });
        if let Err(e) = res {
            return Err(format!("端末との通信が切れました: {e}"));
        }
    }
}

fn hostname() -> String {
    Command::new("scutil")
        .args(["--get", "ComputerName"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "Mac".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypted_gestures_release_fingers_and_navigation_keys() {
        for leave in [true,false] {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let address = listener.local_addr().unwrap();
            let sender = std::thread::spawn(move || {
                let token = "test-pinch-".repeat(4);
                let (_reader,mut writer) = secure::connect(TcpStream::connect(address).unwrap(),&token,b"knit-main").unwrap();
                for msg in [Msg::Pinch {delta:0.0,phase:0},Msg::Pinch {delta:0.2,phase:1}] {
                    writer.write_all(encode(&msg).as_bytes()).unwrap();
                }
                for action in [knit_common::proto::TabletAction::Back,
                    knit_common::proto::TabletAction::Home,knit_common::proto::TabletAction::Recents,
                    knit_common::proto::TabletAction::Screenshot,knit_common::proto::TabletAction::PreviousApp,
                    knit_common::proto::TabletAction::NextApp] {
                    writer.write_all(encode(&Msg::TabletGesture {action}).as_bytes()).unwrap();
                }
                if leave {
                    writer.write_all(encode(&Msg::Leave).as_bytes()).unwrap();
                    writer.write_all(encode(&Msg::Bye).as_bytes()).unwrap();
                }
                writer.flush().unwrap();
            });
            let (socket,_) = listener.accept().unwrap();
            socket.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
            let (reader,_writer) = secure::accept(socket,&"test-pinch-".repeat(4),b"knit-main").unwrap();
            let control = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let mut ctl = BufWriter::new(TcpStream::connect(control.local_addr().unwrap()).unwrap());
            let (mut target,_) = control.accept().unwrap();
            target.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
            let (tx,_rx) = mpsc::channel();
            let result = input_loop(&mut BufReader::new(reader),&mut ctl,&tx,&mut hid::Keyboard::default(),&mut hid::Pointer::new(1000.0,600.0,1.0),&Mutex::new(None),&AtomicBool::new(false));
            assert_eq!(result.is_ok(),leave); // 突然の切断はエラーで再接続へ進む。
            drop(ctl);
            let mut wire = Vec::new();
            target.read_to_end(&mut wire).unwrap();
            sender.join().unwrap();
            let mut actions = Vec::new();
            let mut keys = Vec::new();
            let mut offset = 0;
            while offset < wire.len() {
                match wire[offset] {
                    0 => { keys.push((wire[offset+1],u32::from_be_bytes(wire[offset+2..offset+6].try_into().unwrap()))); offset+=14; },
                    2 => { actions.push(wire[offset+1]); offset += 32; },
                    13 => { let len = u16::from_be_bytes([wire[offset+3],wire[offset+4]]) as usize; offset += 5+len; },
                    other => panic!("unexpected control packet {other}"),
                }
            }
            assert_eq!(actions,vec![0,0,2,2,1,1]);
            assert_eq!(&keys[..8],&[(0,4),(1,4),(0,3),(1,3),(0,187),(1,187),(0,120),(1,120)]);
            let mut held=std::collections::BTreeSet::new();
            for (action,code) in keys { if action==0 {assert!(held.insert(code));} else {assert!(held.remove(&code));} }
            assert!(held.is_empty());
        }
    }

    #[test]
    fn device_list_keeps_state_and_model() {
        let out = "* daemon started successfully\nList of devices attached\n\
                   e569c535               device usb:1-1 product:sheng_global model:24018RPACG device:sheng transport_id:3\n\
                   R52T123                unauthorized usb:1-2 transport_id:4\n\
                   192.168.1.6:40001      device product:x model:Pixel_Tablet device:y transport_id:5\n";
        let d = parse_devices(out);
        assert_eq!(d.len(), 3);
        assert_eq!(
            d[0],
            Seen {
                serial: "e569c535".into(),
                state: "device".into(),
                model: Some("24018RPACG".into()),
            }
        );
        assert_eq!(d[1].state, "unauthorized");
        assert_eq!(d[1].model, None);
        assert_eq!(d[2].model.as_deref(), Some("Pixel Tablet"));
    }

    #[test]
    fn mdns_lists_connectable_addresses() {
        let out = "List of discovered mdns services\n\
                   adb-R52T123-AbCd\t_adb-tls-connect._tcp\t192.168.1.23:41235\n\
                   adb-R52T123-AbCd\t_adb-tls-pairing._tcp\t192.168.1.23:37000\n";
        assert_eq!(parse_mdns(out), vec!["192.168.1.23:41235"]);
    }

    #[test]
    fn density_prefers_the_override() {
        assert_eq!(parse_density("Physical density: 420\n"), Some(420));
        assert_eq!(
            parse_density("Physical density: 320\nOverride density: 280\n"),
            Some(280)
        );
        assert_eq!(parse_density("error"), None);
    }

    #[test]
    fn screen_size_follows_rotation() {
        let d = "  Display: mDisplayId=0\n    init=1600x2560 320dpi base=1600x2560 cur=2560x1600 app=2560x1504 rng=1600x1504-2560x2464\n";
        assert_eq!(parse_cur_size(d), Some((2560, 1600)));
        let wm = "Physical size: 1600x2560\nOverride size: 1200x1920\n";
        assert_eq!(parse_wm_size(wm), Some((1200, 1920)));
        assert_eq!(
            parse_wm_size("Physical size: 1080x2400\n"),
            Some((1080, 2400))
        );
    }

    #[test]
    fn server_version_is_read_from_brew_path_or_error_text() {
        let p = Path::new("/opt/homebrew/Cellar/scrcpy/4.1/share/scrcpy/scrcpy-server");
        assert_eq!(version_from_cellar_path(p).as_deref(), Some("4.1"));
        let rebuilt = Path::new("/opt/homebrew/Cellar/scrcpy/4.1_1/share/scrcpy/scrcpy-server");
        assert_eq!(version_from_cellar_path(rebuilt).as_deref(), Some("4.1"));
        assert_eq!(
            version_from_cellar_path(Path::new("/usr/share/scrcpy/scrcpy-server")),
            None
        );
        let err = "java.lang.IllegalArgumentException: The server version (4.1) does not match the client (0)";
        assert_eq!(version_from_mismatch(err).as_deref(), Some("4.1"));
    }

    /// 実機・エミュレーター試験用の Knit の代役。中継を 1 本繋いで hello を交わす
    struct Harness {
        adb: PathBuf,
        serial: String,
        w: secure::Writer,
        rd: BufReader<secure::Reader>,
        bridge: std::thread::JoinHandle<Result<(), String>>,
        size: (i32, i32),
    }

    impl Harness {
        fn open() -> Self {
            let serial = std::env::var("KNIT_E2E_SERIAL").expect("KNIT_E2E_SERIAL");
            let adb = find_adb().expect("adb");
            let jar = find_server_jar().expect("scrcpy-server");
            let ver = server_version(&adb, &serial, &jar).expect("server version");
            let token = "e2e-token-".repeat(4);
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let port = listener.local_addr().unwrap().port();
            let bridge = {
                let (adb, serial, token) = (adb.clone(), serial.clone(), token.clone());
                std::thread::spawn(move || {
                    let id = identify(&adb, &serial, "Android");
                    run_session(&adb, &serial, &jar, &ver, port, &token, &id)
                })
            };
            // 中継が Knit へ繋ぐ前に失敗した時に待ち続けないよう、期限付きで受ける
            listener.set_nonblocking(true).unwrap();
            let start = Instant::now();
            let s = loop {
                match listener.accept() {
                    Ok((s, _)) => break s,
                    Err(_) if bridge.is_finished() => {
                        panic!("中継が接続前に終わりました: {:?}", bridge.join().unwrap())
                    }
                    Err(_) if start.elapsed() > Duration::from_secs(90) => {
                        panic!("中継から接続がありません")
                    }
                    Err(_) => std::thread::sleep(Duration::from_millis(100)),
                }
            };
            s.set_nonblocking(false).unwrap();
            s.set_read_timeout(Some(Duration::from_secs(30))).ok();
            let (r, w) = secure::accept(s, &token, b"knit-main").unwrap();
            let mut rd = BufReader::new(r);
            let mut line = String::new();
            rd.read_line(&mut line).unwrap();
            let Some(Msg::Hello {
                w: sw, h: sh, name, ..
            }) = decode(&line)
            else {
                panic!("hello ではありません: {line}");
            };
            eprintln!("hello: {name} {sw}x{sh}");
            let mut h = Harness {
                adb,
                serial,
                w,
                rd,
                bridge,
                size: (sw, sh),
            };
            h.send(&Msg::HelloOk {
                name: "e2e".into(),
                w: 1,
                h: 1,
                ver: VERSION,
                id: "e2e".into(),
                monitors: vec![],
            });
            h
        }

        fn send(&mut self, m: &Msg) {
            self.w.write_all(encode(m).as_bytes()).unwrap();
            self.w.flush().unwrap();
        }

        fn adb(&self, args: &[&str]) -> String {
            adb_out(&self.adb, &self.serial, args, Duration::from_secs(15)).unwrap_or_default()
        }

        /// Android が最後に配ったマウスのカーソル位置(直近イベントの xCursorPosition)。
        /// HyperOS はイベント履歴を出さないため読めない(None)
        fn cursor(&self) -> Option<(f64, f64)> {
            let d = self.adb(&["shell", "dumpsys", "input"]);
            let num = |l: &str, key: &str| -> Option<f64> {
                let r = l.split(key).nth(1)?;
                r.split([',', ')']).next()?.trim().parse().ok()
            };
            d.lines()
                .filter(|l| l.contains("source=MOUSE") && l.contains("xCursorPosition="))
                .filter_map(|l| {
                    Some((
                        num(l, "eventTime=")?,
                        num(l, "xCursorPosition=")?,
                        num(l, "yCursorPosition=")?,
                    ))
                })
                .max_by(|a, b| a.0.total_cmp(&b.0))
                .map(|(_, x, y)| (x, y))
        }

        fn close(mut self) -> Result<(), String> {
            self.send(&Msg::Bye);
            self.bridge.join().unwrap()
        }
    }

    /// 実機・エミュレーターでの通し確認(Knit の代役を立てて中継を繋ぐ)。
    /// KNIT_E2E_SERIAL=emulator-5554 cargo test -p knit-mac e2e_device -- --ignored --nocapture
    #[test]
    #[ignore]
    fn e2e_device() {
        let mut h = Harness::open();
        // 端末の入力層を観測する(shell は input グループなので getevent が読める)
        let ev = {
            let (adb, serial) = (h.adb.clone(), h.serial.clone());
            std::thread::spawn(move || {
                adb_out(
                    &adb,
                    &serial,
                    &["shell", "timeout", "6", "getevent", "-lq"],
                    Duration::from_secs(15),
                )
                .unwrap_or_default()
            })
        };
        std::thread::sleep(Duration::from_secs(2));
        h.send(&Msg::Warp { nx: 0.0, ny: 0.5 });
        for i in 1..=10 {
            h.send(&Msg::MouseAbs {
                nx: i as f64 * 0.03,
                ny: 0.5,
            });
            std::thread::sleep(Duration::from_millis(16));
        }
        let key = |down| Msg::Key {
            kc: 0,
            down,
            ctrl: false,
            opt: false,
            cmd: false,
            shift: false,
            tr: false,
            rcmd: false,
        };
        h.send(&key(true));
        h.send(&key(false));
        h.send(&Msg::Clip {
            text: "knit-e2e".into(),
        });
        h.send(&Msg::Ping { ts: 42 });
        let mut got_pong = false;
        let mut line = String::new();
        for _ in 0..10 {
            line.clear();
            if h.rd.read_line(&mut line).is_err() {
                break;
            }
            if matches!(decode(&line), Some(Msg::Pong { ts: 42 })) {
                got_pong = true;
                break;
            }
        }
        let devices = h.adb(&["shell", "dumpsys", "input"]);
        let events = ev.join().unwrap();
        let res = h.close();
        eprintln!("bridge: {res:?}");
        assert!(got_pong, "ping に pong が返りません");
        assert!(
            devices.contains("Knit keyboard"),
            "仮想キーボードが登録されていません"
        );
        assert!(
            devices.contains("Knit mouse"),
            "仮想マウスが登録されていません"
        );
        assert!(
            events.contains("KEY_A"),
            "キー入力が届いていません:\n{events}"
        );
        assert!(
            events.contains("REL_X"),
            "マウス移動が届いていません:\n{events}"
        );
        assert!(res.is_ok());
    }

    /// Android 側の加速で、推定位置と実際のポインタがどれだけずれるかを測る(調整用)。
    /// KNIT_E2E_SERIAL=emulator-5554 cargo test -p knit-mac e2e_pointer_gain -- --ignored --nocapture
    #[test]
    #[ignore]
    fn e2e_pointer_gain() {
        let mut h = Harness::open();
        let (w, _) = h.size;
        std::thread::sleep(Duration::from_secs(1));
        let dist: f64 = 300.0;
        for (step, interval) in [(2.0f64, 8u64), (5.0, 8), (10.0, 8), (20.0, 16), (40.0, 16)] {
            h.send(&Msg::Warp { nx: 0.0, ny: 0.5 });
            std::thread::sleep(Duration::from_millis(400));
            let start = h.cursor();
            let mut x: f64 = 0.0;
            while x < dist {
                x = (x + step).min(dist);
                h.send(&Msg::MouseAbs {
                    nx: x / w as f64,
                    ny: 0.5,
                });
                std::thread::sleep(Duration::from_millis(interval));
            }
            std::thread::sleep(Duration::from_millis(400));
            let end = h.cursor();
            let speed = step * 1000.0 / interval as f64;
            match (start, end) {
                (Some(s), Some(e)) => eprintln!(
                    "{step:>4}px/{interval}ms({speed:>5.0}px/s): 推定 {dist} → 実際 {:.0}(比 {:.2}、x {:.0}→{:.0})",
                    e.0 - s.0,
                    (e.0 - s.0) / dist,
                    s.0,
                    e.0
                ),
                _ => eprintln!("{step}px/{interval}ms: カーソル位置を読めません"),
            }
        }
        let res = h.close();
        assert!(res.is_ok(), "{res:?}");
    }
}

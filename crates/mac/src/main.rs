// knit-mac: Mac 側クライアント。CGEventTap で入力を横流しし、Windows へ送信する。
// 画面右端でカーソルが Mac→Windows 切替、Windows カーソル左端(または F13)で復帰。
#![allow(non_camel_case_types)]
// 複数フレームワークの #[link] を 1 つの extern ブロックに並記している(実害なし)
#![allow(clippy::duplicated_attributes)]
// SEL / CLS は Objective-C ランタイムの慣用名
#![allow(clippy::upper_case_acronyms)]

mod android;
mod audio;
mod cg;
mod clip;
mod conn;
mod diag;
mod doctor;
mod file_drag;
mod files;
mod gui;
mod incoming_drag;
mod objc;
mod outgoing;
mod peers;
mod session;
mod state;
mod tap;
mod trackpad;
mod updater;

// 分割したモジュールの item を crate ルートへ再エクスポートする。
// 他モジュールの crate:: 参照・use crate::*・テストの use super::* の解決を保つため
pub(crate) use cg::*;
pub(crate) use clip::*;
pub(crate) use conn::*;
pub(crate) use files::*;
pub(crate) use objc::*;
pub(crate) use session::*;
pub(crate) use state::*;
pub(crate) use tap::*;

use knit_common::proto::{encode, Msg, PORT};
use knit_common::{bulk, envutil, secure};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// 更新の適用のために終了する。Windows 操作中ならカーソルを戻してから終える
/// (入れ替えは終了を待つ別プロセスが行う)。通知が出る時間だけ待つ
fn request_quit_for_update() {
    std::thread::sleep(std::time::Duration::from_millis(1500));
    if WIN_MODE.swap(false, Ordering::Relaxed) {
        leave_win_mode_cursor_unlock(None);
    }
    std::process::exit(0);
}

/// macOS の通知センターへ表示(接続/切断のユーザー可視化)。
/// osascript 経由で追加権限なしで出せる。失敗しても本体には影響しない。
/// osascript 起動に数百msかかるため別スレッドで発火し、accept/受信スレッドを
/// ブロックしない(レビュー Wave1 A-M9/D-F3)
fn notify(title: &str, body: &str) {
    let title = title.to_string();
    let body = body.to_string();
    // osascript の文字列リテラルで特別な意味を持つ文字を先に無効化する(流用時に
    // ファイル名等が入っても構文エラーで通知だけ落ちる、という事故を防ぐ)
    let esc = |s: &str| {
        s.replace('\\', "\\\\")
            .replace('"', "'")
            .replace(['\n', '\r', '\t'], " ")
    };
    std::thread::spawn(move || {
        let out = std::process::Command::new("osascript")
            .args([
                "-e",
                &format!(
                    "display notification \"{}\" with title \"{}\"",
                    esc(&body),
                    esc(&title)
                ),
            ])
            .output();
        let _ = out;
    });
}
/// バイト数を通知・表示用に整形する(2048→"2 KB"、3_500_000→"3.3 MB")
pub fn human_bytes(n: u64) -> String {
    if n >= 1024 * 1024 * 1024 {
        format!("{:.1} GB", n as f64 / (1024 * 1024 * 1024) as f64)
    } else if n >= 1024 * 1024 {
        format!("{:.1} MB", n as f64 / (1024 * 1024) as f64)
    } else if n >= 1024 {
        format!("{} KB", n.div_ceil(1024))
    } else {
        format!("{n} B")
    }
}

/// この Mac の名前(設定「接続」で上書きした場合のみ。空=ホスト名既定)。
/// hello/hello_ok の name として相手へ送り、設定画面・配置エディタの表示にも
/// 使う(hostname_label 経由)。保存時に safe_peer_name 済み
pub static OWN_NAME: Mutex<String> = Mutex::new(String::new());

/// この Mac のホスト名(kern.hostname、ドメイン部は除く)。hello で相手へ出す。
/// 設定で名前を上書きしている場合はそちらを優先する
pub fn hostname_label() -> String {
    // 上書き名は保存時に safe_peer_name 済みのためそのまま出す
    let own = OWN_NAME.lock().unwrap_or_else(|e| e.into_inner());
    if !own.is_empty() {
        return own.clone();
    }
    unsafe extern "C" {
        fn sysctlbyname(
            name: *const std::ffi::c_char,
            oldp: *mut core::ffi::c_void,
            oldlenp: *mut usize,
            newp: *mut core::ffi::c_void,
            newlen: usize,
        ) -> i32;
    }
    unsafe {
        let name = c"kern.hostname";
        let mut size = 0usize;
        if sysctlbyname(
            name.as_ptr(),
            std::ptr::null_mut(),
            &mut size,
            std::ptr::null_mut(),
            0,
        ) != 0
        {
            return "Mac".into();
        }
        let mut buf = vec![0u8; size];
        if sysctlbyname(
            name.as_ptr(),
            buf.as_mut_ptr() as *mut core::ffi::c_void,
            &mut size,
            std::ptr::null_mut(),
            0,
        ) != 0
        {
            return "Mac".into();
        }
        let s = std::ffi::CStr::from_bytes_until_nul(&buf)
            .map(|c| c.to_string_lossy().into_owned())
            .unwrap_or_default();
        let short = s.split('.').next().unwrap_or("").trim().to_string();
        if short.is_empty() {
            "Mac".into()
        } else {
            short.chars().take(40).collect()
        }
    }
}
/// Windows と共有する設定を一括送信する(接続確立時と各設定の変更時)
pub fn send_cfg() {
    send_msg(&Msg::Cfg {
        cmd_alt: CMD_ALT.load(Ordering::Relaxed),
        spk_mute: SPK_MUTE.load(Ordering::Relaxed) && knit_common::share::allow_audio(),
        side: side_dir(),
        clip: CLIP_SHARE.load(Ordering::Relaxed) && knit_common::share::allow_clip(),
        files: knit_common::share::allow_files(),
        // この Mac の再生ミュート中は相手の音声ストリームを止めてもよい(版 15 以降)
        listen: !crate::audio::MUTED.load(Ordering::Relaxed) && knit_common::share::allow_audio(),
    });
}
/// 単調時計の ms。壁時計(SystemTime)は NTP 補正やスリープ復帰で飛び、
/// ダブルタップ判定・復帰ガード・pong 監視を誤動作させるため使わない。
/// 実体は knit_common::clock(mac/win で同じ値域・テスト済み)
fn now_ms() -> u64 {
    knit_common::clock::mono_now_ms()
}

/// スリープ検知の判定(純関数): 壁時計と単調時計の前回からの進みから、
/// 眠っていた時間を返す。単調の進みがほぼ無い(<100ms)時だけスリープとみなす。
/// NTP の前方ステップ(壁だけが飛ぶ)では単調も普段どおり進むため弾け、
/// 壁の巻き戻り(進み 0)も 0 になるため誤検知しない
fn slept_duration(wall_adv: Duration, mono_adv: Duration) -> Duration {
    const MONO_ADV_MIN: Duration = Duration::from_millis(100);
    if mono_adv < MONO_ADV_MIN {
        wall_adv.saturating_sub(mono_adv)
    } else {
        Duration::ZERO
    }
}
pub(crate) fn send_msg(msg: &Msg) {
    let _ = send_msg_reported(msg);
}

/// 設定(GUI)で選んだ「Windows をホストにする」。KNIT_ROLE=client の GUI 版。
/// 接続方向は起動時に決まるため、再起動後に反映される
pub(crate) static CLIENT_ROLE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// 役割の表示決定(純粋関数・単体テスト対象)。環境変数 KNIT_ROLE=client が
/// GUI 設定に優先する(mac が env で固定できるのは client のみ。
/// server 等の他の値は Windows 側の固定で、Mac では GUI 設定に従う)
pub(crate) fn display_client_role(env: Option<&str>, gui_client: bool) -> bool {
    env == Some("client") || gui_client
}

/// 画面・診断で示す実効的な役割(この Mac が接続しに行く側か)。
/// 環境変数 KNIT_ROLE=client が GUI 設定に優先する(main の起動判定・diag の診断と
/// 同じ条件を一元化し、表示と実役割が食い違わないようにする)
pub(crate) fn effective_client_role() -> bool {
    display_client_role(
        envutil::get("KNIT_ROLE").as_deref(),
        CLIENT_ROLE.load(Ordering::Relaxed),
    )
}

/// 環境変数 KNIT_ROLE=client で役割が固定されているか(固定中は GUI から切り替えられない)
pub(crate) fn role_env_fixed() -> bool {
    envutil::get("KNIT_ROLE").as_deref() == Some("client")
}

/// Mac 自身をロックする(Win+L に相当する ⌘Ctrl+Q を System Events 経由で発生させる。
/// Knit はアクセシビリティ権限を持つためこの経路が使える)
pub(crate) fn lock_this_mac() {
    let ok = std::process::Command::new("/usr/bin/osascript")
        .args([
            "-e",
            "tell application \"System Events\" to keystroke \"q\" using {command down, control down}",
        ])
        .status()
        .is_ok_and(|s| s.success());
    if !ok {
        eprintln!("[lock] Mac のロックに失敗しました(osascript が失敗。権限を確認)");
    }
}

/// 送信の成否を返す版(未接続時に操作を通知したい UI から使う)
pub fn send_msg_reported(msg: &Msg) -> bool {
    let generation = OUTBOUND_GENERATION.load(Ordering::SeqCst);
    if !CONNECTED.load(Ordering::Relaxed) { return false; }
    DIAG_SEND_COUNT.fetch_add(1, Ordering::Relaxed);
    match TX.get() {
        Some(tx) => tx.send(outgoing::Queued { generation, line: encode(msg) }).is_ok(),
        None => false,
    }
}
/// 表示用のリリースバージョン(設定ウィンドウ等)
pub const VERSION_STR: &str = env!("CARGO_PKG_VERSION");
const BUILD_ID: &str = "build-20261005-153737-c8ea7a3";

/// 起動中はロックファイルを保持する(プロセスの終了で自動的に解放される)
fn acquire_instance_lock() -> bool {
    use std::os::unix::io::AsRawFd;
    extern "C" {
        fn flock(fd: i32, op: i32) -> i32;
    }
    let Some(dir) = knit_common::envutil::config_dir() else {
        return true;
    };
    let _ = std::fs::create_dir_all(&dir);
    let Ok(f) = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .open(dir.join("knit-mac.lock"))
    else {
        return true;
    };
    const LOCK_EX_NB: i32 = 2 | 4;
    if unsafe { flock(f.as_raw_fd(), LOCK_EX_NB) } != 0 {
        return false;
    }
    std::mem::forget(f);
    true
}

/// /tmp/knit-mac.log のローテーション(起動時+実行中の定期チェック)。
/// LaunchAgent はこのファイルを追記(O_APPEND)で開いたままのため、削除
/// (unlink)ではリンク切れの inode へ書き続けて容量が解放されない。切り詰めなら
/// 追記の書き込みオフセットが毎回ファイル末尾へ置き直されるため、0 から追記が
/// 始まる(win 側 helper.log の「256KB 超えたら作り直す」と同じ発想の Mac 版。
/// 常駐運用では起動時だけの切り詰めでは頭打ちにならないため、ping 送信ループが
/// 1 分に 1 回この関数を呼ぶ。チェックは metadata の stat だけで軽い)。
/// 切り詰めの前には 1 世代(.old)へ退避する(Windows の run_knit.bat と同じ
/// 「1 世代残す」運用。障害直前のログが 5MB 到達で消えるのを防ぐ)
fn rotate_tmp_log() {
    /// 5MB。diag 行が約 1MB/日のため、数日分でも次のチェックで収まるサイズ
    const MAX_LOG_BYTES: u64 = 5 * 1024 * 1024;
    rotate_log_at(
        std::path::Path::new("/tmp/knit-mac.log"),
        std::path::Path::new("/tmp/knit-mac.log.old"),
        MAX_LOG_BYTES,
    );
}

/// ログが `max_bytes` を超えていたら、`backup` へ退避してから切り詰める。
/// 退避は fs::copy(新しいファイルを作る)で行うため、元のファイルの inode は
/// 変わらず O_APPEND で開いたままの fd(LaunchAgent 由来)と共存できる。
/// 戻り値は切り詰めたかどうか(テストと、失敗時の静かな継続に使う)
fn rotate_log_at(path: &std::path::Path, backup: &std::path::Path, max_bytes: u64) -> bool {
    let Ok(m) = std::fs::metadata(path) else {
        return false;
    };
    if m.len() <= max_bytes {
        return false;
    }
    // 退避が失敗しても切り詰めは実行する(容量の頭打ちは放置できず、退避は
    // 補助的なため。その場合 .old は前回の内容のまま残る)
    if std::fs::copy(path, backup).is_err() {
        eprintln!("[log] ローテーションの退避に失敗しました({})", backup.display());
    }
    std::fs::write(path, b"").is_ok()
}

fn main() {
    rotate_tmp_log();
    eprintln!("[info] knit-mac {BUILD_ID}");
    crate::state::init_boot_wall();
    if let Some(note) = knit_common::share::startup_note() {
        eprintln!("{note}");
    }
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--update") {
        std::process::exit(updater::run_cli());
    }
    // 二重起動の防止: 役割の切替などで再起動が重なると、同じ Mac が複数動いて
    // 待ち受けや接続を取り合い、つながるまで数分かかる(実測)。ロックを握れなければ終了する
    if !args.iter().any(|a| a == "--allow-multiple") && !acquire_instance_lock() {
        eprintln!("[info] すでに起動中のため終了します");
        return;
    }
    #[cfg(debug_assertions)]
    if args.iter().any(|a| a == "--probe-direct-ime") {
        unsafe { gui::direct_input::probe(); }
        return;
    }
    #[cfg(debug_assertions)]
    if args.iter().any(|a| a == "--probe-trackpad") {
        unsafe { trackpad::probe(); }
        return;
    }
    // 検証用: 履歴の保存先(HOME)を隔離して起動し、Mac のコピーが履歴に入るかを見る
    #[cfg(debug_assertions)]
    if args.iter().any(|a| a == "--probe-local-history") {
        history_load();
        start_local_history();
        eprintln!("[probe] 待機中。この間にコピーしてください");
        std::thread::sleep(Duration::from_secs(16));
        if let Ok(h) = HISTORY.lock() {
            for e in h.recent(10) {
                println!("[probe] {:?} device={} text={}", e.kind, e.device, e.text.lines().next().unwrap_or(""));
            }
        }
        return;
    }
    #[cfg(debug_assertions)]
    if args.iter().any(|a| a == "--probe-incoming-drag") {
        incoming_drag::probe();
        return;
    }
    #[cfg(debug_assertions)]
    if args.iter().any(|a| a == "--probe-setup") {
        gui::setup::probe_invitation();
        return;
    }
    if args.iter().any(|a| a == "--preview-setup") {
        let _ = gui::setup::first_run(true);
        return;
    }
    if args.iter().any(|a| a == "--preview-ui") {
        gui::UI_PREVIEW.store(true, Ordering::Relaxed);
        // プレビューでも「操作する端末」が複数台のときの見た目を確認できるように、
        // ダミーの端末を必ず1台登録しておく(--preview-tablet で更にタブレットが増える)
        PEERS.lock().unwrap_or_else(|e|e.into_inner()).push(PeerEntry {
            id: "pc-preview".into(), name: "PC(プレビュー)".into(),
            ip: "127.0.0.2".parse().unwrap(), screen: (2560.0,1440.0), monitors: Vec::new(),
            writer: None, gen: 0, side: 0, edge_monitor: Some(0), ver: knit_common::proto::VERSION,
            alias: None,
        });
        if args.iter().any(|a| a == "--preview-tablet") {
            android::display::remember("android-preview", Some(android::display::PhysicalSize { width_mm: 175.4, height_mm: 263.2 }));
            PEERS.lock().unwrap_or_else(|e|e.into_inner()).push(PeerEntry {
                id: "android-preview".into(), name: "タブレット(プレビュー)".into(),
                ip: "127.0.0.1".parse().unwrap(), screen: (3048.0,2032.0), monitors: Vec::new(),
                writer: None, gen: 0, side: 0, edge_monitor: Some(0), ver: knit_common::proto::VERSION,
                alias: None,
            });
            *ACTIVE_PEER.lock().unwrap_or_else(|e|e.into_inner()) = 1; // タブレットを見ている想定
        }
        if gui::start() {
            gui::SHOW_AT_START.store(true, Ordering::Relaxed);
            unsafe { gui::run_app() };
        }
        return;
    }
    gui::restore_preferences();

    let port: u16 = args
        .iter()
        .position(|a| a == "--port")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok())
        .unwrap_or(PORT);
    // 既存のenvを優先。新規利用者だけOS保護の接続キーと初回導入を使用する。
    let no_gui = args.iter().any(|a| a == "--no-gui")
        || envutil::get("KNIT_NO_GUI").is_some_and(|v| v == "1");
    let mut registered_now = false;
    // トークンは双方向の共有鍵。ハンドシェイクの成否が oracle になるため短い
    // トークンは LAN 内の総当たりで破られる。128bit 相当(32 文字)を下限に
    let token = if let Some(t) = envutil::get("KNIT_TOKEN").filter(|t| t.len() >= 32) {
        PAIRED.store(true, Ordering::Relaxed);
        t
    } else if envutil::get("KNIT_TOKEN").is_some_and(|t| !t.is_empty()) {
        eprintln!(
            "[fatal] KNIT_TOKEN が短すぎます(32 文字未満)。設定の「直接つなぐ」の「生成」で作り直してください"
        );
        // GUI 利用者には stderr が見えず、アプリが黙って死んだようにしか見えない。
        // 通知センターへも出す(session の listen 失敗と同じ導線)。通知は非同期
        // 発火のため、表示される時間だけ待ってから終える
        if !no_gui {
            crate::notify(
                "Knit",
                "接続トークン(KNIT_TOKEN)が短すぎるため起動できません。設定の「直接つなぐ」で「生成」から作り直してください",
            );
            std::thread::sleep(Duration::from_millis(1500));
        }
        std::process::exit(1);
    } else {
        match knit_common::credentials::load() {
            Ok(Some(t)) => {
                PAIRED.store(true, Ordering::Relaxed);
                t
            }
            Ok(None) if !no_gui => match gui::setup::first_run(false) {
                Some((t, registered)) => {
                    registered_now = true;
                    // 招待を発行しただけ(相手の登録がまだ)なら「再接続を待つ」案内に
                    // しない。状態行が「端末を登録…」へ導く(再起動後は接続キーが
                    // あるため従来どおり再接続待ちになる)
                    if registered {
                        PAIRED.store(true, Ordering::Relaxed);
                    }
                    t
                }
                None => return,
            },
            Ok(None) => {
                eprintln!("[setup] 接続キー未設定。GUIで初回登録を完了してください。");
                return;
            }
            Err(_) => {
                // 読み取れない登録が残っている限り再起動しても同じ場所で止まる。
                // 確認のうえ初期化して初回登録へ導く(キャンセル時は従来どおり終了)
                if no_gui || !gui::setup::confirm_broken_registration_reset() {
                    if !no_gui {
                        gui::setup::error("保存した接続キーを読み取れません。キーチェーンのアクセス許可を確認してください。");
                    }
                    eprintln!("[setup] credential store unavailable");
                    return;
                }
                if let Err(e) = knit_common::credentials::delete() {
                    gui::setup::error(&format!(
                        "保存した接続キーを削除できませんでした。\n{e}"
                    ));
                    eprintln!("[setup] credential delete failed: {e}");
                    return;
                }
                eprintln!("[setup] 読み取れない登録を初期化しました。初回登録をやり直します");
                match gui::setup::first_run(false) {
                    Some((t, registered)) => {
                        registered_now = true;
                        if registered {
                            PAIRED.store(true, Ordering::Relaxed);
                        }
                        t
                    }
                    None => return,
                }
            }
        }
    };

    if !no_gui && !gui::setup::ensure_permission() {
        // 「あとで」で権限確認を中断した時も、再開方法を案内してから終了する
        // (登録と接続キーは保存済み。次回起動でここから再開できる)
        gui::setup::permission_postponed();
        return;
    }
    refresh_geo();
    let (screen_w, screen_h) = {
        let g = geo();
        (g.main_w, g.main_h)
    };
    unsafe { CGDisplayRegisterReconfigurationCallback(display_reconfigured, std::ptr::null_mut()) };
    unsafe {
        if let Some(loc) = live_cursor() {
            *CUR_POS.lock().unwrap_or_else(|e| e.into_inner()) = (loc.x, loc.y);
        }
    }
    if let Some(d) = envutil::get("KNIT_SCROLL_DIV").and_then(|v| v.parse::<f64>().ok()) {
        if d > 0.0 {
            set_scroll_div(d);
        }
    }
    if let Some(m) = envutil::get("KNIT_MOUSE_SCALE").and_then(|v| v.parse::<f64>().ok()) {
        if m > 0.0 {
            set_mouse_scale(m);
        }
    }
    if let Some(m) = envutil::get("KNIT_MOUSE_MODE") {
        if m.eq_ignore_ascii_case("rel") {
            MOUSE_ABS_MODE.store(false, Ordering::Relaxed);
        }
    }
    if let Some(m) = envutil::get("KNIT_SWITCH_MODE") {
        if m.eq_ignore_ascii_case("hotkey") {
            HOTKEY_ONLY.store(true, Ordering::Relaxed);
        }
    }
    if let Some(t) = envutil::get("KNIT_EDGE_TAPS").and_then(|v| v.parse::<u32>().ok()) {
        if (1..=3).contains(&t) {
            EDGE_TAPS.store(t, Ordering::Relaxed);
        }
    }
    if let Some(k) = envutil::get("KNIT_HOTKEY_KC").and_then(|v| v.parse::<i64>().ok()) {
        if (1..=127).contains(&k) {
            HOTKEY_KC.store(k, Ordering::Relaxed);
        }
    }
    // メニューで切替可能な設定の初期値(.env 経由でも指定できる)
    NATURAL_SCROLL.store(detect_natural_scroll(), Ordering::Relaxed);
    eprintln!(
        "[info] macOS scroll: {} / knit 方向: {}",
        if NATURAL_SCROLL.load(Ordering::Relaxed) {
            "自然スクロール"
        } else {
            "標準(非自然)"
        },
        if envutil::get("KNIT_SCROLL_FLIP").as_deref() == Some("1") {
            "Windows 標準(手動上書き)"
        } else {
            "Mac に合わせる"
        },
    );
    if envutil::get("KNIT_SCROLL_FLIP").as_deref() == Some("1") {
        SCROLL_FLIP.store(true, Ordering::Relaxed);
    }
    // Deskflow 標準オプション(画面位置/切替/隅/クリップボード)
    match envutil::get("KNIT_SIDE").as_deref() {
        Some("left") => SIDE.store(1, Ordering::Relaxed),
        Some("up") => SIDE.store(2, Ordering::Relaxed),
        Some("down") => SIDE.store(3, Ordering::Relaxed),
        Some("upright") => SIDE.store(4, Ordering::Relaxed),
        Some("lowright") | Some("downright") => SIDE.store(5, Ordering::Relaxed),
        Some("upleft") => SIDE.store(6, Ordering::Relaxed),
        Some("lowleft") | Some("downleft") => SIDE.store(7, Ordering::Relaxed),
        _ => {}
    }
    if let Some(v) = envutil::get("KNIT_SWITCH_DELAY").and_then(|v| v.parse::<u64>().ok()) {
        // 0=滞在なし(無効)はそのまま。それ以外は GUI スライダーと同じ 50-1000ms へ
        // 収める(設定画面と起動時の受け口で範囲が食い違うと、スライダーに触れた
        // 瞬間に値が黙って潰れるため)
        SWITCH_DELAY_MS.store(
            if v == 0 { 0 } else { v.clamp(50, 1000) },
            Ordering::Relaxed,
        );
    }
    if let Some(v) = envutil::get("KNIT_DOUBLE_TAP_MS").and_then(|v| v.parse::<u64>().ok()) {
        DOUBLE_TAP_MS.store(v.clamp(100, 3000), Ordering::Relaxed);
    }
    if let Some(v) = envutil::get("KNIT_CORNER_PX").and_then(|v| v.parse::<u64>().ok()) {
        CORNER_PX.store(v.min(500), Ordering::Relaxed);
    }
    if envutil::get("KNIT_CLIP").as_deref() == Some("0") {
        CLIP_SHARE.store(false, Ordering::Relaxed);
    }
    if envutil::get("KNIT_DRAG_SWITCH").as_deref() == Some("1") {
        DRAG_SWITCH.store(true, Ordering::Relaxed);
    }
    if envutil::get("KNIT_FAST_EDGE").as_deref() == Some("0") {
        FAST_EDGE.store(false, Ordering::Relaxed);
    }
    if envutil::get("KNIT_APP_HANDOFF").as_deref() == Some("1") {
        APP_HANDOFF.store(true, Ordering::Relaxed);
        eprintln!("[handoff] 越境 App Handoff を有効化しました(KNIT_APP_HANDOFF=1)");
    }
    if envutil::get("KNIT_MAC_KEYS").as_deref() == Some("0") {
        MAC_KEYS.store(false, Ordering::Relaxed);
    }
    if envutil::get("KNIT_SWIPE_NAV").as_deref() == Some("0") {
        SWIPE_NAV.store(false, Ordering::Relaxed);
    }
    if envutil::get("KNIT_LOCK_SYNC").as_deref() == Some("0") {
        LOCK_SYNC.store(false, Ordering::Relaxed);
    }
    if envutil::get("KNIT_IME_SYNC").as_deref() == Some("0") {
        IME_SYNC.store(false, Ordering::Relaxed);
    }
    if envutil::get("KNIT_CONTINUE_HERE").as_deref() == Some("0") {
        CONTINUE_HERE.store(false, Ordering::Relaxed);
    }
    if envutil::get("KNIT_RCMD_CTRL").as_deref() == Some("0") {
        RCMD_CTRL.store(false, Ordering::Relaxed);
    }
    if envutil::get("KNIT_SCROLL_COMPAT").as_deref() == Some("1") {
        SCROLL_COMPAT.store(true, Ordering::Relaxed);
    }
    if envutil::get("KNIT_CMD_ALT").as_deref() == Some("1") {
        CMD_ALT.store(true, Ordering::Relaxed);
    }
    if envutil::get("KNIT_MUTE_SPK").as_deref() == Some("0") {
        SPK_MUTE.store(false, Ordering::Relaxed);
    }
    eprintln!(
        "[info] screen {screen_w}x{screen_h}. listening on :{port} (server mode). scroll_div={} mouse_scale={} clip_max={}KB mouse_mode={} switch_mode={} hotkey_kc={} edge_taps={}",
        scroll_div(),
        mouse_scale(),
        CLIP_MAX_BYTES / 1024,
        if MOUSE_ABS_MODE.load(Ordering::Relaxed) { "abs" } else { "rel" },
        if HOTKEY_ONLY.load(Ordering::Relaxed) { "hotkey(ロック)" } else { "edge" },
        hotkey_kc(),
        EDGE_TAPS.load(Ordering::Relaxed)
    );
    // 起動時に受信フォルダと履歴を用意する(通知のパスが必ず有効になる)
    let _ = std::fs::create_dir_all(
        std::env::var_os("HOME")
            .map(|h| std::path::Path::new(&h).join("Downloads/Knit"))
            .unwrap_or_default(),
    );
    history_load();
    // 画像履歴の実体を件数(60)と総量(512MiB)の両上限へ刈り込む(起動時に 1 回)。
    // 保存時の刈り込みは画像が届いた時しか走らないため、ここで残った超過を掃く
    prune_image_store_now();
    start_local_history();
    eprintln!("[info] 操作ガイド: カーソルを画面端へ動かすと Windows へ移ります。メニューバーにクリップボード履歴があります");

    // 送信チャネル + 書き込みストリームスロット(接続が変わるたび差し替え)
    let (tx, rx) = std::sync::mpsc::channel::<outgoing::Queued>();
    let _ = TX.set(tx);
    let slot: Arc<Mutex<Option<secure::Writer>>> = Arc::new(Mutex::new(None));
    let _ = STREAM_SLOT.set(slot.clone());

    // 単一の送信スレッド(チャネル→ストリーム差し替え方式)
    std::thread::spawn(move || {
        use std::io::Write;
        let mut ping_at = std::time::Instant::now();
        // /tmp/knit-mac.log のサイズチェックは 1 分に 1 回で十分(stat は軽量)。
        // 起動時のみの切り詰めだと常駐運用で際限なく成長するため、ここで定期化する
        let mut log_check_at = std::time::Instant::now();
        let mut pending = None;
        // スリープ復帰の検知: 単調時計はスリープ中に進まないため、壁時計との差が
        // 開いたら眠っていたと分かる。眠っている間に相手側の接続は切れているのが
        // 普通で、単調時計基準の pong 監視では気づけないため、即座に張り直す
        let mut wall = std::time::SystemTime::now();
        let mut mono = std::time::Instant::now();
        loop {
            let (wall_now, mono_now) = (std::time::SystemTime::now(), std::time::Instant::now());
            let slept = slept_duration(
                wall_now.duration_since(wall).unwrap_or_default(),
                mono_now.duration_since(mono),
            );
            (wall, mono) = (wall_now, mono_now);
            if slept > Duration::from_secs(3) {
                // stability-report.sh が「復帰検知→再接続確立」の所要時間を集計できるよう
                // 計測行を残す(スリープ検知のログ行自体には時刻の数値が無いため)
                eprintln!(
                    "[conn-metric] wake unix_ms={} slept_s={}",
                    crate::state::wall_ms(),
                    slept.as_secs()
                );
                drop_stream(&format!("スリープ復帰を検知({}秒)", slept.as_secs()));
                BULK_LINK.clear();
                // クライアントモードの再接続待機を飛ばして、すぐ張り直させる
                WAKE.notify();
            }
            match pending.take().map(Ok).unwrap_or_else(|| rx.recv_timeout(Duration::from_millis(200))) {
                Ok(line) => {
                    // 同じ接続世代だけ束ねる。古い入力を次の端末へ再生しない。
                    let queued = outgoing::batch(line, &rx, &mut pending);
                    let mut guard = STREAM_SLOT
                        .get()
                        .unwrap()
                        .lock()
                        .unwrap_or_else(|e| e.into_inner());
                    let current_line = queued.for_generation(OUTBOUND_GENERATION.load(Ordering::SeqCst));
                    if let Some(s) = guard.as_mut().filter(|_| current_line.is_some()) {
                        // encode() が行末 \n を持つため writeln! だと二重改行で
                        // ワイヤが \n\n になる(受信側の空行パースが倍増する)。write_all で送る
                        if s.write_all(current_line.as_ref().unwrap().as_bytes()).and_then(|_| s.flush()).is_err() {
                            OUTBOUND_GENERATION.fetch_add(1, Ordering::SeqCst);
                            if let Some(s) = guard.take() {
                                s.shutdown();
                            }
                        }
                    }
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(_) => break,
            }
            // 常駐運用のログ頭打ち(5MB 超えたら切り詰め)。受信タイムアウト
            //(200ms)の合間で回すため、チェック周期だけ 1 分に緩める
            if log_check_at.elapsed() >= Duration::from_secs(60) {
                log_check_at = std::time::Instant::now();
                rotate_tmp_log();
            }
            if ping_at.elapsed() >= Duration::from_secs(1) {
                ping_at = std::time::Instant::now();
                // 15 秒 pong が無ければ実質切断扱いでストリームを外す
                // (TCP が生きていても相手プロセスが固まった場合を拾う。相手の
                // 9 秒無通信判定より十分遅くして、ping の間隔揺れで切らないようにする)
                if now_ms().saturating_sub(LAST_PONG_MS.load(Ordering::Relaxed)) > 15_000 {
                    drop_stream("pong が 15 秒途絶");
                    continue;
                }
                let mut guard = STREAM_SLOT
                    .get()
                    .unwrap()
                    .lock()
                    .unwrap_or_else(|e| e.into_inner());
                if let Some(s) = guard.as_mut() {
                    if s.write_all(encode(&Msg::Ping { ts: now_ms() }).as_bytes())
                        .and_then(|_| s.flush())
                        .is_err()
                    {
                        OUTBOUND_GENERATION.fetch_add(1, Ordering::SeqCst);
                        if let Some(s) = guard.take() {
                            s.shutdown();
                        }
                    }
                }
            }
        }
    });

    // 画面ロックの連動(1 秒毎)。Mac がロックされたら Windows 操作中でも制御を
    // Mac へ戻し(ロック中の入力を Windows へ流さない)、Windows もロックする
    std::thread::spawn(|| {
        let mut was = false;
        loop {
            std::thread::sleep(Duration::from_secs(1));
            let now = screen_locked();
            if now && !was {
                eprintln!("[lock] Mac がロックされました");
                if WIN_MODE.swap(false, Ordering::Relaxed) {
                    leave_win_mode_cursor_unlock(None);
                }
                if LOCK_SYNC.load(Ordering::Relaxed) && CONNECTED.load(Ordering::Relaxed) {
                    if !send_msg_reported(&Msg::Lock) {
                        eprintln!("[lock] Windows へのロック指示を送れませんでした(接続経路が死んでいる可能性)");
                    }
                }
            }
            was = now;
        }
    });

    // ネットワーク構成の監視(1 秒毎)。既定経路のローカル IP が変わったら
    //(Wi-Fi 切替・DHCP 更新・LAN ケーブル抜去など)古い接続を捨てて再接続を
    // すぐ起こす。ポーリングは UDP connect(パケットを出さない)だけで軽い
    std::thread::spawn(|| {
        fn route_ip() -> Option<std::net::IpAddr> {
            let s = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
            s.connect("8.8.8.8:80").ok()?;
            Some(s.local_addr().ok()?.ip())
        }
        let mut last = route_ip();
        loop {
            std::thread::sleep(Duration::from_millis(1000));
            let now = route_ip();
            if now == last {
                continue;
            }
            eprintln!(
                "[net] 既定経路のローカル IP が変わりました: {:?} -> {:?}",
                last, now
            );
            last = now;
            // クライアントモードの再接続待機を飛ばす
            WAKE.notify();
            // 接続中でも IP が変わった経路は実質死んでいる。pong タイムアウト
            //(最長 15 秒)を待たずに自分から張り直す
            if CONNECTED.load(Ordering::Relaxed) {
                drop_stream("ネットワーク構成の変化を検知");
                BULK_LINK.clear();
            }
        }
    });

    doctor::start();

    // Tailscale 経路の診断(30 秒毎)。直結から中継(DERP)へ落ちると遅延が数倍になり
    // 「カクつき」の原因になるため、切り替わった時だけ通知する
    std::thread::spawn(|| loop {
        std::thread::sleep(Duration::from_secs(30));
        let peer = *PEER_IP.lock().unwrap_or_else(|e| e.into_inner());
        let Some(peer) = peer
            .filter(|p| CONNECTED.load(Ordering::Relaxed) && knit_common::net::is_tailscale(*p))
        else {
            continue;
        };
        let Some(now) = tailscale_path(peer) else {
            continue;
        };
        let before = TS_PATH.swap(now, Ordering::Relaxed);
        if before != now {
            eprintln!(
                "[net] Tailscale 経路: {}",
                if now == 1 { "直結" } else { "中継(DERP)" }
            );
            if now == 2 {
                notify(
                    "Knit",
                    &format!(
                        "{} との通信が中継経路になりました(遅延が増えます)。同じネットワークか有線直結を推奨します",
                        crate::active_peer_label()
                    ),
                );
            }
        }
    });

    // 音声受信・再生(Windows→Mac。独立ポート 24901。KNIT_AUDIO=0 で無効)
    if envutil::get("KNIT_AUDIO").as_deref() != Some("0") && knit_common::share::allow_audio() {
        // 音声は本線ポートからの差分 +1(24900→24901)
        audio::start(token.clone(), port + 1);
    }

    // 接続方向: 既定は Mac=サーバ(本環境のAP隔離対策)。KNIT_ROLE=client +
    // KNIT_HOST(または --host)で Mac=クライアント(通常ネットワークの配布先向け。
    // その場合は Windows 側を KNIT_ROLE=server で待ち受ける)
    // GUI 設定(Windows をホストにする)か環境変数 KNIT_ROLE=client で接続側になる。
    // 環境変数が優先(GUI は配布時の既定、env は開発者の上書き)
    let client_role = crate::effective_client_role();
    let bulk_ep: &'static bulk::Endpoint = BULK.get_or_init(|| bulk::Endpoint {
        link: &BULK_LINK,
        token: token.clone(),
        dir: std::env::var_os("HOME")
            .map(std::path::PathBuf::from)
            .map(|h| h.join("Downloads/Knit"))
            .unwrap_or_else(|| {
                // HOME 未設定(変則起動)で空 PathBuf のまま join すると相対パスに
                // 落ち、起動 cwd 次第で意図しない場所へ保存されるため固定する
                eprintln!("[bulk] HOME が未設定のため受信先を一時領域にします");
                std::env::temp_dir().join("knit-downloads")
            }),
        on_event: mac_on_bulk,
        log: |s| eprintln!("{s}"),
        on_rx_bytes: |_| {},
        // 受信バッチの開始時点のアクティブ端末を履歴ラベルとして固定する
        //(通知時点で読むと、受信中の端末切替でラベルがすり替わる)
        on_batch_begin: note_bulk_rx_device,
        // 待受 bind の初回失敗だけ通知センターへも出す(接続は生きているのに
        // ファイル・画像だけが届かない状態をログだけで終わらせない)
        on_bind_error: |s: &str| notify("Knit", s),
    });
    // 前回終了時に残った受信中ファイルの一時実体(.knit-*.part)を掃除する。
    // exit(0) は Receiver の Drop を飛ばすため、再起動・初期化経路の保険として
    // 起動時(まだ受信が始まっていない=安全な時期)に消す
    let swept = knit_common::files::sweep_temp_files(&bulk_ep.dir);
    if swept > 0 {
        eprintln!("[bulk] 前回の受信中ファイルの残骸を {swept} 件掃除しました");
    }
    if client_role {
        let host = args
            .iter()
            .position(|a| a == "--host")
            .and_then(|i| args.get(i + 1))
            .cloned()
            .or_else(|| envutil::get("KNIT_HOST"));
        eprintln!(
            "[info] client mode: connecting to {}",
            host.as_deref().unwrap_or("LAN から自動検出")
        );
        std::thread::spawn(move || {
            bulk::connect_loop(
                bulk_ep,
                || {
                    let ip = *PEER_IP.lock().unwrap_or_else(|e| e.into_inner());
                    ip.map(|ip| std::net::SocketAddr::new(ip, port + bulk::PORT_OFFSET))
                },
                || CONNECTED.load(Ordering::Relaxed),
            )
        });
        std::thread::spawn(move || client_thread(host, port, token, screen_w, screen_h));
    } else {
        let bind = envutil::get("KNIT_BIND").unwrap_or_else(|| "0.0.0.0".to_string());
        // LAN 自動発見への応答(ブロードキャストを受けるため常に 0.0.0.0 で待つ)
        let tk = token.clone();
        std::thread::spawn(move || {
            if let Err(e) = knit_common::discover::respond(
                "0.0.0.0",
                port + knit_common::discover::PORT_OFFSET,
                &tk,
                knit_common::net::is_allowed,
            ) {
                eprintln!("[disc] 発見応答の待受に失敗: {e}(自動発見が使えません)");
            }
        });
        std::thread::spawn(move || {
            bulk::serve(
                bulk_ep,
                &bind,
                port + bulk::PORT_OFFSET,
                allow_bulk_peer,
            )
        });
        // Android タブレットの中継(adb とペア済みの端末がある時だけ働く)。
        // 127.0.0.1 から本線へ 1 台の接続先として入るため、待受モードでのみ起動する
        android::spawn(port, token.clone());
        std::thread::spawn(move || server_thread(port, token, screen_w, screen_h));
    }

    // 診断モード: 1秒ごとにモード/受信・送信カウント/実カーソル位置を記録。
    // --diag 起動時の有効化に加え、設定画面の左下「その他」の「詳しく記録」でも runtime で
    // 切り替えられる(出力スレッドは常駐させて、DIAG_ENABLED の間だけ書き出す)
    if args.iter().any(|a| a == "--diag") {
        DIAG_ENABLED.store(true, Ordering::Relaxed);
        // 検証用の切替指示(--diag 起動時のみ): 一時ディレクトリへ "toggle" と書くと
        // 画面を切り替える。クリップボードは画面を移る時にだけ同期するため、自動検証で
        // 「移る」操作を起こす手段が要る(verify.sh が使う)。/tmp 共有領域だとローカルの
        // 他ユーザーから切替できるため temp_dir()=$TMPDIR(ユーザー固有)へ置く
        std::thread::spawn(|| loop {
            std::thread::sleep(Duration::from_millis(200));
            let p = std::env::temp_dir().join("knit-cmd");
            if let Ok(cmd) = std::fs::read_to_string(&p) {
                let _ = std::fs::remove_file(&p);
                if cmd.trim() == "toggle" {
                    do_toggle("verify");
                }
            }
        });
    }
    std::thread::spawn(|| {
        let mut last_cursor = (0.0f64, 0.0f64);
        loop {
            std::thread::sleep(Duration::from_secs(1));
            // 設定での切替は即時に効かせる(1 秒の確認は軽いため常駐で問題ない)
            if !DIAG_ENABLED.load(Ordering::Relaxed) {
                continue;
            }
            let (mode, mv, kd, sd, wp, mc, sc, ab, heal) = (
                WIN_MODE.load(Ordering::Relaxed),
                DIAG_MOVE_COUNT.load(Ordering::Relaxed),
                DIAG_KEY_COUNT.load(Ordering::Relaxed),
                DIAG_SEND_COUNT.load(Ordering::Relaxed),
                DIAG_WARP_COUNT.load(Ordering::Relaxed),
                DIAG_MODE_COUNT.load(Ordering::Relaxed),
                DIAG_SCROLL_COUNT.load(Ordering::Relaxed),
                DIAG_ABS_COUNT.load(Ordering::Relaxed),
                DIAG_SELF_HEAL.load(Ordering::Relaxed),
            );
            unsafe {
                let ev = CGEventCreate(std::ptr::null_mut());
                let p = if ev.is_null() {
                    CGPoint { x: 0.0, y: 0.0 }
                } else {
                    CGEventGetLocation(ev)
                };
                let moved = (p.x - last_cursor.0).abs() + (p.y - last_cursor.1).abs() > 1.0;
                eprintln!(
                    "[diag] mode={} moves={mv} keys={kd} sent={sd} scrolls={sc} abs={ab} warp_fixed={wp} switches={mc} self_heal={heal} cursor=({:.0},{:.0}) moving={}",
                    if mode { "WIN" } else { "MAC" }, p.x, p.y, moved
                );
                last_cursor = (p.x, p.y);
            }
        }
    });

    // 起動時点のクリップボードは送らない(以後の変化だけを切替時に同期する)
    LAST_SYNC_COUNT.store(clipboard_change_count(), Ordering::Relaxed);

    // 基準値はMouseDownで取得済み。重いURL読み出しはタップの外で行う。
    // 取得中に離す・押し直す・越境する場合は、世代の異なる結果を捨てる。
    std::thread::spawn(|| loop {
        std::thread::sleep(Duration::from_millis(16));
        let probe = FILE_DRAG.lock().unwrap_or_else(|e| e.into_inner()).probe();
        let Some(probe) = probe else { continue };
        with_pool(|| unsafe {
            let pb = drag_pasteboard();
            if pb.is_null() {
                return;
            }
            let cnt = msg0_isize(pb, sel_registerName(c"changeCount".as_ptr()));
            if cnt == probe.baseline {
                return;
            }
            if let Some(files) = pb_files(pb) {
                if msg0_isize(pb, sel_registerName(c"changeCount".as_ptr())) != cnt {
                    return;
                }
                let n = files.len();
                let precompute = files.clone();
                if FILE_DRAG
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .complete(probe, cnt, files)
                {
                    eprintln!("[file] ファイル掴み検出: {n} 件");
                    // 越境までの間に、越境時(tap スレッド)で必要になる集計を先に済ませる(M6)
                    precompute_drag_entries(precompute);
                }
            }
        });
    });

    // WIN モード中のカーソル固定監視(改善ループ4):
    // イベントタップ経由の巻き戻しは移動イベントが来た時しか働かない。
    // 慣性や関連切断の効き遅れでカーソルが動いたままになる場合に備え、
    // 常時 200ms ごとに固定位置へ巻き戻す(境界の同時移動抑止の最終防衛)
    std::thread::spawn(|| {
        let mut fixes: u64 = 0;
        loop {
            std::thread::sleep(Duration::from_millis(150));
            // 自己修復: WIN モードでないのにカーソルが隠れたままの異常状態
            // (将来の同種バグや予期しない経路)を検知し、表示を復元する
            if !WIN_MODE.load(Ordering::Relaxed) && CURSOR_HIDDEN.load(Ordering::Relaxed) {
                unsafe {
                    let d = CGMainDisplayID();
                    for _ in 0..3 {
                        CGDisplayShowCursor(d);
                    }
                    CGAssociateMouseAndMouseCursorPosition(true);
                }
                CURSOR_HIDDEN.store(false, Ordering::Relaxed);
                DIAG_SELF_HEAL.fetch_add(1, Ordering::Relaxed);
                eprintln!("[cursor] self-heal: 復帰漏れを修復しました");
                knit_common::doctor::note("隠れたままのカーソルを復元しました");
            }
            // タップ健全性: システムがタイムアウトでタップを無効化した際、
            // 無効化通知を取り逃しても 1 秒毎の冪等な再 enable で必ず復帰させる
            if let Some(&tap) = TAP_PORT.get() {
                if TAP_REARM_N.fetch_add(1, Ordering::Relaxed).is_multiple_of(7) {
                    // 150ms×7 ≒ 1秒毎
                    unsafe { CGEventTapEnable(tap as CFMachPortRef, true) };
                }
            }
            // ウォッチドッグ: WIN モード中にユーザーがマウスを動かしているのに
            // (直近2秒以内にタップ受信) Windows への転送が5秒止まっている状態は
            // 異常。強制的に Mac へ復帰させ、操作不能な状態に陥らないようにする。
            // LAST_ABS_MS は絶対位置モードしか更新しないため、条件にモードを含めないと
            // 相対モード(rel)で必ず誤発火する(rel が5秒で強制復帰されていた実績バグ)
            if WIN_MODE.load(Ordering::Relaxed)
                && MOUSE_ABS_MODE.load(Ordering::Relaxed)
                && !GAME_REL.load(Ordering::Relaxed)
            {
                let now = now_ms();
                let last_ev = LAST_EVENT_MS.load(Ordering::Relaxed);
                let last_abs = LAST_ABS_MS.load(Ordering::Relaxed);
                if last_ev > 0
                    && now.saturating_sub(last_ev) < 2_000
                    && now.saturating_sub(last_abs) > 5_000
                {
                    WIN_MODE.store(false, Ordering::Relaxed);
                    eprintln!(
                        "[watchdog] WIN中に転送停止を検知。強制復帰します(最終イベント{}ms前・最終abs送信{}ms前・abs送信数{})",
                        now.saturating_sub(last_ev),
                        now.saturating_sub(last_abs),
                        DIAG_ABS_COUNT.load(Ordering::Relaxed)
                    );
                    knit_common::doctor::note("相手の端末への転送が止まったためMacに戻しました");
                    leave_win_mode_cursor_unlock(None);
                    continue;
                }
            }
            // 権限喪失の保険: タップ停止の通知が届かない場合でも、1 秒毎に権限を
            // 確認し、剥奪されていたら入力を解放して終了する(フリーズ防止)
            {
                let now = now_ms();
                if now.saturating_sub(PERM_CHECK_MS.swap(now, Ordering::Relaxed)) >= 1_000
                    && !ax_trusted()
                {
                    surrender_on_permission_loss();
                }
            }
            if !WIN_MODE.load(Ordering::Relaxed) {
                continue;
            }
            let Some((lx, ly)) = *LOCK_POS.lock().unwrap_or_else(|e| e.into_inner()) else {
                continue;
            };
            unsafe {
                let Some(loc) = live_cursor() else { continue };
                if (loc.x - lx).abs() > 1.0 || (loc.y - ly).abs() > 1.0 {
                    CGWarpMouseCursorPosition(CGPoint { x: lx, y: ly });
                    fixes += 1;
                    DIAG_WARP_COUNT.store(fixes, Ordering::Relaxed);
                }
            }
        }
    });

    // イベントタップ(メインスレッドで RunLoop)
    let mask: CGEventMask = (1 << EVT_LEFT_DOWN)
        | (1 << EVT_LEFT_UP)
        | (1 << EVT_RIGHT_DOWN)
        | (1 << EVT_RIGHT_UP)
        | (1 << EVT_MOUSE_MOVED)
        | (1 << EVT_LEFT_DRAGGED)
        | (1 << EVT_RIGHT_DRAGGED)
        | (1 << EVT_OTHER_DRAGGED)
        | (1 << EVT_KEY_DOWN)
        | (1 << EVT_KEY_UP)
        | (1 << EVT_FLAGS_CHANGED)
        // F 行のメディア(輝度・照明)は NSSystemDefined で届くため、
        // マスクに入れないとタップ自体が受け取らない(F5 不反応の実績)
        | (1 << EVT_SYSTEM_DEFINED)
        | (1 << EVT_SCROLL_WHEEL)
        | (1 << EVT_OTHER_DOWN)
        | (1 << EVT_OTHER_UP);

    let tap = unsafe {
        CGEventTapCreate(
            0, // kCGHIDEventTap(ヘッダ実測: 0=HID, 1=Session, 2=Annotated。Deskflow は HID)
            0, // kCGHeadInsertEventTap
            0, // kCGEventTapOptionDefault = 0(抑制可/フィルタ)
            mask,
            tap_callback,
            std::ptr::null_mut(),
        )
    };
    if tap.is_null() {
        eprintln!("[fatal] CGEventTapCreate failed(アクセシビリティ権限を確認)");
        std::process::exit(1);
    }
    let _ = TAP_PORT.set(tap as usize);
    unsafe {
        let src = CFMachPortCreateRunLoopSource(std::ptr::null_mut(), tap, 0);
        let rl = CFRunLoopGetMain();
        CFRunLoopAddSource(rl, src, kCFRunLoopCommonModes);
        CGEventTapEnable(tap, true);
    }
    unsafe { trackpad::start(); }
    eprintln!(
        "[info] tap active. カーソルを画面右端へ動かすと Windows モード / F13・メニューでトグル"
    );
    // メニューバー GUI(既定ON。--no-gui / KNIT_NO_GUI=1 で CUI のみ)。
    // AppKit が使えない環境(ssh 由来のセッション等)では start() が失敗し、
    // 従来どおり CFRunLoop で継続する(タップはメインRunLoop共通モードのため共存可)
    let no_gui = args.iter().any(|a| a == "--no-gui")
        || envutil::get("KNIT_NO_GUI").is_some_and(|v| v == "1");
    if !no_gui && gui::start() {
        eprintln!("[gui] メニューバー常駐を開始しました");
        updater::start_background();
        // --show-prefs: 起動直後に設定ウィンドウを開く(スクリーンショット検証用)。
        // 実際の生成は NSApp.run 後のタイマー初回で行う。
        // 招待を発行した直後(相手の登録が済んでいなくても)も開く:
        // 未登録なら「端末を登録…」ボタンへの導線として機能する
        if registered_now || args.iter().any(|a| a == "--show-prefs") {
            gui::SHOW_AT_START.store(true, Ordering::Relaxed);
        }
        unsafe { gui::run_app() }; // NSApp.run(戻らない。終了はメニューから)
    } else {
        unsafe { CFRunLoopRun() };
    }
}
#[cfg(test)]
mod file_tx_tests {
    use super::*;

    /// 他の転送中(掴みドラッグ等で FILE_TX_BUSY が立っている)に来た ⌘C 同期は、
    /// 黙って捨てるのではなく「開始できなかった」ことを呼び出し元へ返す。
    /// 呼び出し元はこれを見て同期済みの印を戻し、次回の切替で再試行する(M4 の回帰)
    #[test]
    fn send_files_to_win_reports_busy_and_keeps_the_flag() {
        FILE_TX_BUSY.store(true, Ordering::Relaxed);
        let outcome = send_files_to_win(vec![std::path::PathBuf::from("/tmp/knit-busy-test")]);
        assert!(
            matches!(outcome, SendFilesOutcome::Busy),
            "転送中は Busy を返す"
        );
        assert!(
            FILE_TX_BUSY.load(Ordering::Relaxed),
            "実行中の転送のフラグを奪ってはいけない"
        );
        FILE_TX_BUSY.store(false, Ordering::Relaxed);
    }

    /// 履歴は転送の完了(Ok)時にだけ載る。開始時に載せると失敗・中止した転送まで
    /// 「送った」記録が残り、履歴からの復元が実在しないパスを指す
    #[test]
    fn file_history_is_pushed_only_after_a_completed_transfer() {
        // history_save() が実環境の ~/.config/knit/history.json を書き換えないよう、
        // 保存先だけをテンポラリの HOME へ向ける(HISTORY を触るのはこのテストだけ)
        struct RestoreHome(Option<std::ffi::OsString>, std::path::PathBuf);
        impl Drop for RestoreHome {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.1);
                if let Some(home) = self.0.as_ref() {
                    std::env::set_var("HOME", home);
                }
            }
        }
        let home = std::env::var_os("HOME");
        let tmp = std::env::temp_dir().join("knit-file-history-test");
        let _ = std::fs::create_dir_all(&tmp);
        let _guard = RestoreHome(home, tmp.clone());
        std::env::set_var("HOME", &tmp);

        let sent = vec![std::path::PathBuf::from("/tmp/knit-history-sent.txt")];
        let aborted = vec![std::path::PathBuf::from("/tmp/knit-history-aborted.txt")];
        let count = || {
            HISTORY
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .entries()
                .len()
        };
        let before = count();
        push_files_history_on_result(&Ok(3usize), &sent);
        assert_eq!(count(), before + 1, "完了した転送は履歴に載る");
        let failed: Result<usize, std::io::Error> =
            Err(std::io::Error::new(std::io::ErrorKind::Interrupted, "中止"));
        push_files_history_on_result(&failed, &aborted);
        assert_eq!(count(), before + 1, "失敗・中止した転送は履歴に載せない");
    }
}

#[cfg(test)]
mod log_rotation_tests {
    use super::*;

    /// 退避→切り詰めの一連(5MB 超えで .old へ残して空になる)。O_APPEND の fd との
    /// 共存は fs::copy(inode を変えない)による設計のため、ここでは退避の中身と
    /// 切り詰め結果・しきい未満では何もしないことを確認する
    #[test]
    fn rotate_backs_up_then_truncates_only_over_the_limit() {
        let dir = std::env::temp_dir().join(format!(
            "knit-rotate-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let log = dir.join("test.log");
        let backup = dir.join("test.log.old");

        // しきい未満: 触らない(前回の .old も上書きしない)
        std::fs::write(&log, "small").unwrap();
        std::fs::write(&backup, "previous backup").unwrap();
        assert!(!rotate_log_at(&log, &backup, 100));
        assert_eq!(std::fs::read_to_string(&log).unwrap(), "small");
        assert_eq!(
            std::fs::read_to_string(&backup).unwrap(),
            "previous backup",
            "しきい未満では退避先を書き換えない"
        );

        // しきい超え: 退避して切り詰める
        let big = "x".repeat(101);
        std::fs::write(&log, &big).unwrap();
        assert!(rotate_log_at(&log, &backup, 100));
        assert_eq!(std::fs::read_to_string(&log).unwrap(), "", "切り詰められる");
        assert_eq!(
            std::fs::read_to_string(&backup).unwrap(),
            big,
            "退避先に切り詰め前の中身が残る"
        );

        // 2 回目の超え: 退避先は新しい中身で上書きされる(1 世代のみ)
        let big2 = "y".repeat(150);
        std::fs::write(&log, &big2).unwrap();
        assert!(rotate_log_at(&log, &backup, 100));
        assert_eq!(std::fs::read_to_string(&backup).unwrap(), big2, "1 世代だけ残す");

        // ログが無い初回起動: false のまま落ちない
        let missing = dir.join("missing.log");
        assert!(!rotate_log_at(&missing, &dir.join("missing.log.old"), 100));

        std::fs::remove_dir_all(&dir).unwrap();
    }
}

#[cfg(test)]
mod drag_end_tests {
    use super::*;

    unsafe extern "C" {
        fn CGEventGetType(event: CGEventRef) -> u32;
    }

    #[test]
    fn handoff_creates_a_tagged_left_release() {
        unsafe {
            // 作成だけを検証する。実際のポインタやボタン状態は操作しない。
            let event = make_drag_end_event(CGPoint { x: 100.0, y: 100.0 });
            assert!(!event.is_null());
            let event_type = CGEventGetType(event);
            let tag = CGEventGetIntegerValueField(event, FIELD_EVENT_SOURCE_USER_DATA);
            assert_eq!(
                CGEventGetLocation(event),
                CGPoint { x: 100.0, y: 100.0 },
                "渡した位置がイベントから読み戻せること"
            );
            CFRelease(event);
            assert_eq!(
                event_type, EVT_LEFT_UP,
                "元の左ドラッグを終えるイベントであること"
            );
            assert_eq!(tag, SYNTH_UP_MAGIC, "物理ボタンの解放と区別できること");
        }
    }

    #[test]
    fn prefers_the_press_position_over_the_live_cursor() {
        assert_eq!(
            drag_end_position(
                Some(CGPoint { x: 8.0, y: 9.0 }),
                Some(CGPoint { x: 900.0, y: 9.0 })
            ),
            Some(CGPoint { x: 8.0, y: 9.0 }),
            "掴み開始位置が分かるなら画面端ではなくそこで離す"
        );
    }

    #[test]
    fn falls_back_to_the_live_cursor_without_a_press_position() {
        assert_eq!(
            drag_end_position(None, Some(CGPoint { x: 900.0, y: 9.0 })),
            Some(CGPoint { x: 900.0, y: 9.0 }),
            "押下位置を取り損ねた場合は現行どおりライブ位置を使う"
        );
    }

    #[test]
    fn yields_none_when_both_positions_are_unknown() {
        assert_eq!(
            drag_end_position(None, None),
            None,
            "両方不明なら None(投稿側は従来どおり (0,0) へフォールバック)"
        );
    }
}

#[cfg(test)]
mod edge_carry_tests {
    use super::edge_button_carry;

    /// 競合窓の回帰: 監視スレッドの complete が ready の読み取りと take の間に
    /// 入ると、スナップショットは false のまま handoff だけが成立する。この
    /// 組み合わせでも MOVE 由来の切替で押下を持ち込む(持ち込まないと Windows 側は
    /// 押下を待って保存へフォールバックし、掴みがどこにも渡らない)
    #[test]
    fn carries_the_press_when_handoff_lands_after_the_ready_snapshot() {
        assert_eq!(
            edge_button_carry(true, false, false, true),
            Some(true),
            "take が成立したら MOVED 由来でも押下を持ち込む"
        );
    }

    #[test]
    fn plain_move_crossing_sends_no_buttons() {
        assert_eq!(
            edge_button_carry(true, false, false, false),
            None,
            "通常の MOVED 切替ではボタンを一切送らない(誤ドラッグ防止)"
        );
    }

    #[test]
    fn keeps_the_carry_rules_for_non_raced_crossings() {
        assert_eq!(
            edge_button_carry(true, true, true, false),
            Some(true),
            "掴み検出済みの MOVED 由来は押下を持ち込む"
        );
        assert_eq!(
            edge_button_carry(false, false, false, false),
            Some(false),
            "DRAGGED 由来の既定は全ボタンを離す"
        );
        assert_eq!(
            edge_button_carry(false, false, true, false),
            Some(true),
            "KNIT_DRAG_SWITCH 有効時は押下を持ち込む"
        );
        assert_eq!(
            edge_button_carry(true, false, true, false),
            None,
            "DRAG_SWITCH 有効でも ready が無い MOVED 由来は送らない"
        );
    }
}

#[cfg(test)]
mod boundary_tests {
    use super::{
        abs_edge_reached, along_ratio_on, boundary_crossed, boundary_of, peer_gap, Geo,
        MacDisplay, PeerEntry,
    };

    fn peer(side: u8, edge_monitor: Option<usize>) -> PeerEntry {
        PeerEntry {
            id: "test".into(),
            name: "test".into(),
            ip: "127.0.0.1".parse().unwrap(),
            screen: (1920.0, 1080.0),
            monitors: Vec::new(),
            writer: None,
            gen: 1,
            ver: knit_common::proto::VERSION,
            side,
            edge_monitor,
            alias: None,
        }
    }

    /// 実機と同じ 3 画面構成: MacBook(メイン 0,0,2056,1329)+ 上段モニター
    /// (-249,-1080, 2560x1080)。全体領域は x=-249..2311 / y=-1080..1329
    fn geo_three_displays() -> Geo {
        Geo {
            main_w: 2056.0,
            main_h: 1329.0,
            min_x: -249.0,
            max_x: 2311.0,
            min_y: -1080.0,
            max_y: 1329.0,
            exit: [
                (-1080.0, 1329.0),
                (-1080.0, 1329.0),
                (-249.0, 2311.0),
                (-249.0, 2311.0),
            ],
            displays: vec![
                MacDisplay {
                    x: 0.0,
                    y: 0.0,
                    w: 2056.0,
                    h: 1329.0,
                    main: true,
                    name: String::new(),
                },
                MacDisplay {
                    x: -249.0,
                    y: -1080.0,
                    w: 2560.0,
                    h: 1080.0,
                    main: false,
                    name: String::new(),
                },
            ],
        }
    }

    /// 境界を境に越える回帰: 境界手前の帯や、積算だけが先行している状態では
    /// 発火しない(旧実装は敏感さ EDGE_PX の帯と実カーソル 40px 手前を許容していた)
    #[test]
    fn crosses_only_when_both_positions_reach_the_boundary_itself() {
        assert!(
            boundary_crossed(0.0, 0.0),
            "積算・実カーソルとも境界に達した=越える"
        );
        assert!(
            boundary_crossed(-30.0, 0.2),
            "積算が境界を跨いでいても、実カーソルが境界に触れて初めて越える"
        );
        assert!(
            !boundary_crossed(-30.0, 36.0),
            "実カーソルが境界手前(旧許容の40px内)では越えない"
        );
        assert!(
            !boundary_crossed(2.0, 2.0),
            "境界手前2px(旧・敏感さの既定値)では越えない"
        );
        assert!(
            !boundary_crossed(12.0, 0.0),
            "積算がまだ境界に届いていなければ越えない"
        );
    }

    /// 距離は「割り当てられた端末の境界(モニター指定込み)」で測る回帰。
    /// 旧実装は全体領域の端で測っていたため、下端 (812,1329) のような接続の
    /// ない辺でも積算のずれが距離に現れず、勝手に越えてカーソルが消えた
    ///(実機ログ: [mode] WINDOWS (edge) at (812,1329) など)
    #[test]
    fn gaps_are_measured_on_the_assigned_monitor_edge() {
        let g = geo_three_displays();
        // Windows=メインモニターの左、タブレット=メインモニターの右
        let (_win_side, win_mon) = (1u8, Some(0usize));
        // メインの右端・左端は距離 0(越えられる境界)
        assert_eq!(peer_gap(&g, win_mon, 0, 2056.0, 740.0), 0.0);
        assert_eq!(peer_gap(&g, win_mon, 1, 0.0, 600.0), 0.0);
        // 下端に触れても、左境界までの距離は x のまま(0 にならない=壁)
        assert_eq!(peer_gap(&g, win_mon, 1, 812.0, 1329.0), 812.0);
        // 上段モニター(y がメインの範囲外)には境界が無い=壁
        assert_eq!(peer_gap(&g, win_mon, 0, 2311.0, -500.0), f64::MAX);
        assert_eq!(peer_gap(&g, win_mon, 1, -249.0, -500.0), f64::MAX);
        // モニター指定なし(全画面)は従来どおり全体領域の端で測る
        assert_eq!(peer_gap(&g, None, 0, 2056.0, 740.0), 255.0);
        // 実カーソルが下端にいる状態では、左境界の距離は 812 のまま=発火しない
        assert!(!boundary_crossed(-1.0, peer_gap(&g, win_mon, 1, 812.0, 1329.0)));
    }

    /// 越え先の境界確定: 端末がいるのにどの端末の辺にも当てはまらない辺は壁。
    /// クライアントモード(端末一覧が空)は全体設定の辺だけが境界
    #[test]
    fn unresolved_edges_are_walls() {
        let peers = vec![peer(1, Some(0)), peer(0, Some(0))];
        // find_enter_edge が端末を返した場所はその端末の境界で越える
        assert_eq!(
            boundary_of(&peers, 0, Some(1)),
            Some((0, Some(0))),
            "タブレットの辺(メイン右)で越える"
        );
        assert_eq!(boundary_of(&peers, 0, Some(0)), Some((1, Some(0))));
        // 端末がいる世界で、どの端末の辺にも当てはまらない(上段モニターの端など)
        // 場所は壁(旧実装は全体設定の辺へ落ちて接続のない辺で切替っていた)
        assert_eq!(
            boundary_of(&peers, 0, None),
            None,
            "解決しない辺では切替しない"
        );
        // クライアントモード(端末なし・右配置): 全体設定の辺だけが境界
        assert_eq!(boundary_of(&[], 0, None), Some((0, None)));
        // 端末の割当が消えた直後(切断との競合)も壁として扱う
        assert_eq!(boundary_of(&peers, 0, Some(9)), None);
    }

    /// モニター指定の境界に沿った比率は、そのモニターの辺で測る
    ///(全体で測ると上段モニターの y 範囲に正規化され、入り位置が常に端になる)
    #[test]
    fn along_ratio_uses_the_boundary_monitor_span() {
        let g = geo_three_displays();
        // メイン右端の y=740 はメインの縦幅の約 56%
        let r = along_ratio_on(&g, 0, Some(0), 2056.0, 740.0);
        assert!((r - 740.0 / 1329.0).abs() < 1e-9, "got {r}");
        // モニター指定なしは従来どおり全体の出口範囲で測る(既存挙動の固定)
        let whole = along_ratio_on(&g, 0, None, 2056.0, 740.0);
        assert_eq!(whole, g.along_ratio(0, 2056.0, 740.0));
    }

    /// abs モードの戻りも境界そのもの(clamp の止まり値)でのみ判定する
    #[test]
    fn abs_mode_returns_only_at_the_clamp_stop() {
        let (w, h) = (1920.0, 1080.0);
        // 既定(右配置): Windows の左端
        assert!(abs_edge_reached(0, 0.0, 400.0, w, h), "clamp 端で戻る");
        assert!(!abs_edge_reached(0, 0.4, 400.0, w, h), "境界手前では戻らない");
        assert!(!abs_edge_reached(0, 2.0, 400.0, w, h), "旧・手前2px帯では戻らない");
        // 左配置: Windows の右端(clamp は幅-2)
        assert!(abs_edge_reached(1, w - 2.0, 400.0, w, h));
        assert!(!abs_edge_reached(1, w - 2.5, 400.0, w, h));
        // 上配置: Windows の下端
        assert!(abs_edge_reached(2, 400.0, h - 2.0, w, h));
        assert!(!abs_edge_reached(2, 400.0, h - 3.0, w, h));
        // 下配置: Windows の上端
        assert!(abs_edge_reached(3, 400.0, 0.0, w, h));
        assert!(!abs_edge_reached(3, 400.0, 1.9, w, h));
    }
}

#[cfg(test)]
mod shortcut_tests {
    use super::mac_shortcut_translation as tr;

    /// 翻訳対応の固定(タップ実装と表の乖離を防ぐ)。kc は Mac keycode
    #[test]
    fn shortcut_table_matches_spec() {
        // ⌘← = Home(Shift 透過: ⌘⇧← は Shift+Home)
        assert_eq!(
            tr(123, false, false, true, false),
            Some((115, false, false, false, false))
        );
        assert_eq!(
            tr(123, false, false, true, true),
            Some((115, false, false, false, true))
        );
        // ⌘→ = End、⌘↑ = Ctrl+Home、⌘↓ = Ctrl+End
        assert_eq!(
            tr(124, false, false, true, false),
            Some((119, false, false, false, false))
        );
        assert_eq!(
            tr(126, false, false, true, false),
            Some((115, false, false, true, false))
        );
        assert_eq!(
            tr(125, false, false, true, false),
            Some((119, false, false, true, false))
        );
        // ⌘M / ⌘H = Win+Down(最小化)
        assert_eq!(
            tr(43, false, false, true, false),
            Some((125, true, false, false, false))
        );
        assert_eq!(
            tr(4, false, false, true, false),
            Some((125, true, false, false, false))
        );
        // ⌘] = Ctrl+Tab / ⌘[ = Ctrl+Shift+Tab(⌘⇧[ も前タブ)
        assert_eq!(
            tr(30, false, false, true, false),
            Some((48, false, false, true, false))
        );
        assert_eq!(
            tr(33, false, false, true, false),
            Some((48, false, false, true, true))
        );
        assert_eq!(
            tr(33, false, false, true, true),
            Some((48, false, false, true, true))
        );
        // ⌘⇧4 / ⌘⇧3 = Win+Shift+S。⇧無しの ⌘4 は素の F4 相当へ翻訳しない
        assert_eq!(
            tr(21, false, false, true, true),
            Some((1, true, false, false, true))
        );
        assert_eq!(
            tr(18, false, false, true, true),
            Some((1, true, false, false, true))
        );
        assert_eq!(tr(21, false, false, true, false), None);
        // ⌘⇧5 = Win+Alt+R
        assert_eq!(
            tr(23, false, false, true, true),
            Some((15, false, true, false, false))
        );
        // ⌘Q = Alt+F4
        assert_eq!(
            tr(12, false, false, true, false),
            Some((118, false, true, false, false))
        );
        // ⌘G = F3 / ⌘⇧G = Shift+F3
        assert_eq!(
            tr(32, false, false, true, false),
            Some((99, false, false, false, false))
        );
        assert_eq!(
            tr(32, false, false, true, true),
            Some((99, false, false, false, true))
        );
        // ⌘. = Esc
        assert_eq!(
            tr(47, false, false, true, false),
            Some((53, false, false, false, false))
        );
        // ⌘Space = Win+Space。翻訳先修飾は既定マップ(cmd→Ctrl / ctrl→Win)の意味で
        // 並ぶため、ctrl フラグ=true が Windows キーを表す(Space+Ctrl ではない)
        assert_eq!(
            tr(49, false, false, true, false),
            Some((49, true, false, false, false))
        );
        // ⌘⌥Esc = Ctrl+Shift+Esc
        assert_eq!(
            tr(53, false, true, true, false),
            Some((53, false, false, true, true))
        );
        // ⌘Ctrl+Q = Win+L(⌘Q より優先)
        assert_eq!(
            tr(12, true, false, true, false),
            Some((37, true, false, false, false))
        );
        // ⌥← = Ctrl+←(単語移動)
        assert_eq!(
            tr(123, false, true, false, false),
            Some((123, false, false, true, false))
        );
        // 翻訳対象外: 素の A、⌘A(そのまま渡る)、⌥A
        assert_eq!(tr(0, false, false, false, false), None);
        assert_eq!(tr(0, false, false, true, false), None);
        assert_eq!(tr(0, false, true, false, false), None);
    }
}

#[cfg(test)]
mod geo_tests {
    use super::Geo;

    /// MacBook(0..2056) の左に 1920 幅、右に 2560 幅のモニターがある構成
    fn three_screens() -> Geo {
        Geo {
            main_w: 2056.0,
            main_h: 1329.0,
            min_x: -1920.0,
            max_x: 4616.0,
            min_y: -200.0,
            max_y: 1329.0,
            exit: [
                (-200.0, 1240.0),
                (0.0, 1080.0),
                (-1920.0, 0.0),
                (0.0, 2056.0),
            ],
            // 配置指定(gap)のテストでは未使用。端末ごとの辺のテストでは実配列を入れる
            displays: Vec::new(),
        }
    }

    #[test]
    fn edges_are_measured_on_the_whole_desktop() {
        let g = three_screens();
        // MacBook の左端(x=0)は左モニターへの通り道なので、左配置でも切替境界ではない
        assert!(g.gap(1, 0.0, 500.0) > 1000.0);
        assert_eq!(g.gap(1, -1920.0, 500.0), 0.0);
        // MacBook の右端も右モニターへの通り道
        assert!(g.gap(0, 2056.0, 500.0) > 1000.0);
        assert_eq!(g.gap(0, 4616.0, 500.0), 0.0);
    }

    #[test]
    fn return_point_is_inside_the_exit_display() {
        let g = three_screens();
        let (x, y) = g.inside_point(1, 60.0, Some(0.5));
        assert_eq!(x, -1860.0);
        assert_eq!(y, 540.0);
        let (x, y) = g.inside_point(0, 60.0, Some(0.0));
        assert_eq!(x, 4556.0);
        assert_eq!(y, -180.0); // 端から 20px は避ける
        assert_eq!(g.along_ratio(0, 4616.0, 520.0), 0.5);
        // 上下の辺は横位置で測る
        assert_eq!(g.along_ratio(3, 1028.0, 1329.0), 0.5);
    }
}

#[cfg(test)]
mod win_cur_tests {
    use super::rescale_win_cur as rs;

    #[test]
    fn rescale_keeps_ratio_on_shrink_and_grow() {
        // 2560x1440 → 1920x1080: 画面内の同じ比率位置へ写す(張り付かせない)
        assert_eq!(
            rs((2000.0, 1000.0), (2560.0, 1440.0), (1920.0, 1080.0)),
            (1500.0, 750.0)
        );
        // 拡大時も比率維持(位置が飛ばない)
        assert_eq!(
            rs((960.0, 540.0), (1920.0, 1080.0), (2560.0, 1440.0)),
            (1280.0, 720.0)
        );
        // 同一サイズなら不変
        assert_eq!(
            rs((123.0, 456.0), (1920.0, 1080.0), (1920.0, 1080.0)),
            (123.0, 456.0)
        );
    }

    #[test]
    fn rescale_ignores_invalid_sizes() {
        // 初期値(0x0)や不正値はそのまま(0 除算・暴発写像の防止)
        assert_eq!(rs((10.0, 20.0), (0.0, 0.0), (1920.0, 1080.0)), (10.0, 20.0));
        assert_eq!(
            rs((10.0, 20.0), (1920.0, 1080.0), (0.0, 1080.0)),
            (10.0, 20.0)
        );
    }

    #[test]
    fn px_per_sec_converts_window_to_seconds() {
        use super::px_per_sec as v;
        assert_eq!(v(120.0, 100), 1200.0);
        assert_eq!(v(3.0, 30), 100.0);
        // 窓が 0ms(初回イベント等)は速度不定ではなく 0 扱い(0 除算回避)
        assert_eq!(v(50.0, 0), 0.0);
    }
}

#[cfg(test)]
mod ime_tests {
    use super::ime_mode_state as st;

    #[test]
    fn japanese_modes_map_to_ime_open_state() {
        // ひらがな/カタカナ/半角カナ/全角英数は ON
        assert_eq!(st("com.apple.inputmethod.Japanese.Hiragana"), Some(true));
        assert_eq!(st("com.apple.inputmethod.Japanese.Katakana"), Some(true));
        assert_eq!(
            st("com.apple.inputmethod.Japanese.HalfWidthKana"),
            Some(true)
        );
        assert_eq!(
            st("com.apple.inputmethod.Japanese.FullWidthRoman"),
            Some(true)
        );
        // 日本語入力の英数モードは OFF
        assert_eq!(st("com.apple.inputmethod.Japanese.Roman"), Some(false));
        // Apple 純正のキーボードレイアウト(英字配列等)には IME が乗っていない
        // ため、日本語入力の英数モードと同じ扱い(OFF へ合わせる)。
        // JIS 配列レイアウトも IME では無いため同様
        assert_eq!(st("com.apple.keylayout.ABC"), Some(false));
        assert_eq!(st("com.apple.keylayout.US"), Some(false));
        assert_eq!(st("com.apple.keylayout.Japanese"), Some(false));
        // サードパーティ IME は入力モードを反映しない ID を返すことがあるため
        // 対象外(誤 ON/OFF を送らない。同期しない旨は Mac 側ログへ出る)
        assert_eq!(st("com.google.inputmethod.Japanese.base"), None);
        assert_eq!(st("com.google.inputmethod.Japanese.base.Roman"), None);
        // 他言語の入力メソッド(中国語等)も同期しない(勝手に閉じない)
        assert_eq!(st("com.apple.inputmethod.SCIM.ITABC"), None);
    }
}

#[cfg(test)]
mod dib_tests {
    use super::dib_to_bmp;

    /// biSize=40 の BITMAPINFOHEADER を組み立てる。comp は biCompression
    fn info_header(w: i32, h: i32, bpp: u16, comp: u32) -> Vec<u8> {
        let mut b = vec![0u8; 40];
        b[0..4].copy_from_slice(&40u32.to_le_bytes());
        b[4..8].copy_from_slice(&w.to_le_bytes());
        b[8..12].copy_from_slice(&h.to_le_bytes());
        b[12..14].copy_from_slice(&1u16.to_le_bytes());
        b[14..16].copy_from_slice(&bpp.to_le_bytes());
        b[16..20].copy_from_slice(&comp.to_le_bytes());
        b
    }

    #[test]
    fn bitfields_masks_after_info_header_are_skipped() {
        // Windows のクリップボードは biSize=40 + BI_BITFIELDS で、ヘッダ直後に
        // 12 バイトのカラーマスクを付ける。offbits はマスクの後でなければ
        // 画像全体が 3px ずれる(実機で緑が赤に化けた実績)
        let mut dib = info_header(8, 8, 32, 3);
        dib.extend_from_slice(&[0, 0, 255, 0, 0, 255, 0, 0, 255, 0, 0, 0]); // RGB マスク
        dib.extend_from_slice(&[0xAA; 8 * 8 * 4]);
        let bmp = dib_to_bmp(&dib);
        let off = u32::from_le_bytes([bmp[10], bmp[11], bmp[12], bmp[13]]) as usize;
        assert_eq!(off, 14 + 40 + 12, "ピクセル開始はマスクの直後");
        assert_eq!(bmp[off], 0xAA, "マスク列をピクセルとして読まない");
    }

    #[test]
    fn plain_header_and_v5_header_offsets_are_unchanged() {
        let dib = [info_header(4, 4, 32, 0), vec![0x11; 4 * 4 * 4]].concat();
        let bmp = dib_to_bmp(&dib);
        let off = u32::from_le_bytes([bmp[10], bmp[11], bmp[12], bmp[13]]) as usize;
        assert_eq!(off, 14 + 40, "BI_RGB はマスクなし");

        // biSize>=52(V4/V5)はマスクがヘッダサイズに含まれるため加算しない
        let mut v5 = info_header(4, 4, 32, 3);
        v5[0..4].copy_from_slice(&124u32.to_le_bytes());
        v5.resize(124, 0);
        v5.extend_from_slice(&[0x22; 4 * 4 * 4]);
        let bmp = dib_to_bmp(&v5);
        let off = u32::from_le_bytes([bmp[10], bmp[11], bmp[12], bmp[13]]) as usize;
        assert_eq!(off, 14 + 124, "V5 ヘッダは二重に足さない");
    }
}

#[cfg(test)]
mod sleep_detect_tests {
    use super::slept_duration;
    use std::time::Duration;

    const S: fn(u64) -> Duration = Duration::from_secs;

    #[test]
    fn detects_an_actual_sleep_where_only_the_wall_clock_moves() {
        // 実スリープ: 壁は 30 秒進み、単調(200ms 周期の監視ループ)はほぼ進まない
        let slept = slept_duration(S(30) + Duration::from_millis(20), Duration::from_millis(20));
        assert!(
            slept >= S(29) && slept <= S(30),
            "眠っていた時間を概ね返す: {slept:?}"
        );
    }

    #[test]
    fn ntp_forward_step_is_not_a_sleep() {
        // NTP の前方ステップ: 壁だけ 5 秒飛ぶ。単調は普段どおり進むためスリープではない
        assert_eq!(
            slept_duration(S(5) + Duration::from_millis(200), Duration::from_millis(200)),
            Duration::ZERO
        );
        // 差が 3 秒を超えていても(旧判定なら誤検知した量でも)単調が進んでいれば切らない
        assert_eq!(slept_duration(S(10), Duration::from_millis(150)), Duration::ZERO);
    }

    #[test]
    fn wall_clock_going_backwards_is_not_a_sleep() {
        // 壁の巻き戻り(NTP 補正の負方向)は進み 0 → slept も 0(切断しない)
        assert_eq!(
            slept_duration(Duration::ZERO, Duration::from_millis(50)),
            Duration::ZERO
        );
    }
}

#[cfg(test)]
mod handoff_tests {
    use super::app_handoff_match;

    fn apps() -> Vec<(String, String)> {
        vec![
            ("Visual Studio Code".to_string(), "C:/code.exe".to_string()),
            ("Windows Terminal".to_string(), "C:/wt.exe".to_string()),
            ("Blender".to_string(), "C:/blender.exe".to_string()),
        ]
    }

    #[test]
    fn matches_exact_name_case_insensitively() {
        let a = apps();
        let hit = app_handoff_match("Visual Studio CODE", &a).unwrap();
        assert_eq!(hit.1, "C:/code.exe", "大小違いの完全一致にヒット");
        assert!(
            app_handoff_match("  Blender  ", &a).is_some(),
            "前後空白は無視"
        );
    }

    #[test]
    fn matches_by_containment_with_a_long_enough_name() {
        let a = apps();
        // Mac の「Terminal」は Windows の「Windows Terminal」の部分文字列
        let hit = app_handoff_match("Terminal", &a).unwrap();
        assert_eq!(hit.1, "C:/wt.exe");
        // 4 文字未満の部分一致は誤爆のもとなので拾わない
        assert!(
            app_handoff_match("Ble", &a).is_none(),
            "4 文字未満の contains は不可"
        );
        assert!(
            app_handoff_match("Safari", &a).is_none(),
            "存在しないアプリは不一致"
        );
        assert!(app_handoff_match("", &a).is_none(), "空の名前は不一致");
    }
}

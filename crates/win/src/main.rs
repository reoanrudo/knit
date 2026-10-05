// knit-win: Windows 側サーバ。TCP で受けた入力イベントを SendInput で注入する。
// 必須: 対話セッション起動 + OpenInputDesktop(フル権限) + SetThreadDesktop
// v0.5: GUI サブシステム化(コンソール非依存)+タスクトレイ常駐+待受モード追加
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
// HRESULT 等は Windows API の慣用名
#![allow(clippy::upper_case_acronyms)]
#![windows_subsystem = "windows"]

mod audio;
mod clipboard;
mod conn;
mod diag;
mod doctor;
mod dragdrop;
mod helper;
mod input;
mod inputdesk;
mod session;
mod state;
mod tray;
mod updater;
mod xfer;

use knit_common::bulk;
use knit_common::proto::PORT;
use std::process::exit;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use windows_sys::Win32::System::Threading::{PROCESS_INFORMATION, STARTUPINFOW};

pub(crate) use clipboard::{
    clipboard_seq, clipboard_write_files, history_clear, history_load, history_push_files,
    history_restore_by_id, HISTORY, LAST_SYNC_SEQ, make_hdrop_global,
};
pub(crate) use conn::{client_loop, last_connected_line, next_retry_line, PEER, peer_ip, server_loop};
pub(crate) use input::{
    BTN_W, CMD_ALT, inject_mouse_btn, refresh_vscreen, release_all_input, vscreen,
};
#[cfg(test)]
pub(crate) use input::{remote_mouse_button, remote_mouse_move_rel};
pub(crate) use state::{
    CONNECTED, DEBUG_KEYS, on_power_event, proto_return, RTT_MS, SIDE_W, SPK_MUTE_MODE, WTX,
};
pub(crate) use xfer::{
    begin_tx, BULK, BULK_LINK, end_tx, FILES_RX, human_bytes, RX_BYTES, spawn_esc_cancel_watcher,
    update_tx, win_on_bulk, xfer_line,
};

// ---------- Win32 直宣言(desktop 接続) ----------
#[link(name = "user32")]
unsafe extern "system" {
    fn OpenInputDesktop(
        dwFlags: u32,
        fInherit: bool,
        dwDesiredAccess: u32,
    ) -> *mut core::ffi::c_void;
    fn SetThreadDesktop(hdesktop: *mut core::ffi::c_void) -> i32;
    fn CloseDesktop(hdesktop: *mut core::ffi::c_void) -> i32;
}

// CF_HDROP からファイルパス群を列挙する(shell32)
#[link(name = "shell32")]
unsafe extern "system" {
    /// ifile=0xFFFFFFFF でファイル個数、それ以外はパス長(文字数・NUL 除外)
    fn DragQueryFileW(
        hdrop: *mut core::ffi::c_void,
        ifile: u32,
        lpszfile: *mut u16,
        cch: u32,
    ) -> u32;
}

// ---------- Win32 直宣言(コンソール離脱) ----------
#[link(name = "user32")]
unsafe extern "system" {
    fn GetConsoleWindow() -> *mut core::ffi::c_void;
}
#[link(name = "kernel32")]
unsafe extern "system" {
    /// windows-sys 0.59 の Threading モジュールに無いため直宣言(ABI は安定)
    fn CreateProcessW(
        application_name: *const u16,
        command_line: *mut u16,
        process_attributes: *const core::ffi::c_void,
        thread_attributes: *const core::ffi::c_void,
        inherit_handles: i32,
        creation_flags: u32,
        environment: *const core::ffi::c_void,
        current_directory: *const u16,
        startup_info: *mut STARTUPINFOW,
        process_information: *mut PROCESS_INFORMATION,
    ) -> i32;
}

// ---------- Win32 直宣言(コンソール無し運用/二重起動防止) ----------
#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetStdHandle(n_std_handle: u32) -> *mut core::ffi::c_void;
    fn SetStdHandle(n_std_handle: u32, handle: *mut core::ffi::c_void) -> i32;
    fn CreateMutexW(
        attrs: *mut core::ffi::c_void,
        initial_owner: i32,
        name: *const u16,
    ) -> *mut core::ffi::c_void;
}

/// GUI サブシステムでは stdout が無効な場合があり、そのままだと println! が
/// パニックするため NUL デバイスへ繋ぎ替える(リダイレクト起動時は何もしない)
fn ensure_stdout() {
    unsafe {
        const STD_OUTPUT_HANDLE: u32 = 0xFFFF_FFF5; // (u32)-11
        const STD_ERROR_HANDLE: u32 = 0xFFFF_FFF2; // (u32)-12
        let out = GetStdHandle(STD_OUTPUT_HANDLE);
        if !out.is_null() && out as isize != -1 {
            return; // リダイレクト起動などで有効
        }
        if let Ok(nul) = std::fs::OpenOptions::new().write(true).open("NUL") {
            use std::os::windows::io::AsRawHandle;
            let h = nul.as_raw_handle();
            SetStdHandle(STD_OUTPUT_HANDLE, h);
            SetStdHandle(STD_ERROR_HANDLE, h);
            std::mem::forget(nul); // ハンドルはプロセス終了まで保持
        }
    }
}

/// 二重起動防止(5分毎の自動復帰タスクが既存インスタンスと並走しないように)
fn acquire_single_instance() -> bool {
    unsafe {
        let mut name: Vec<u16> = "Local\\Knit-Instance".encode_utf16().collect();
        name.push(0);
        let h = CreateMutexW(std::ptr::null_mut(), 0, name.as_ptr());
        if windows_sys::Win32::Foundation::GetLastError() == 183 {
            // ERROR_ALREADY_EXISTS = 既に起動している(自動復帰タスクからの起動等)
            if !h.is_null() {
                windows_sys::Win32::Foundation::CloseHandle(h);
            }
            return false;
        }
        !h.is_null() // ミューテックスはプロセス終了まで保持(明示解放しない)
    }
}

/// コンソール付き起動(手動実行/SSH/ターミナル)を検出したら、DETACHED_PROCESS
/// な自分を再起動して即終了する。GUI サブシステムでもコンソールから起動すると
/// そのコンソールに所属し、「ターミナルを閉じたら接続が切れる」原因になる。
/// 起動経路がどうであれコンソールの生死に左右されない本体へ置き換える
/// (ミューテックス取得の前に行うため、再起動先との二重起動競合も起きない)
fn detach_if_console() {
    unsafe {
        if GetConsoleWindow().is_null() {
            return; // コンソール無し起動(schtasks/vbs)= そのまま本体として続行
        }
        let exe = match std::env::current_exe() {
            Ok(p) => p,
            Err(_) => return,
        };
        // コマンドラインを "exeパス" 引数… の形で組み立てる。
        // 空白入りの引数が割れないよう、含む時だけクォートする(" は \" へ)
        let mut cmd = String::new();
        cmd.push('"');
        cmd.push_str(&exe.to_string_lossy());
        cmd.push('"');
        for a in std::env::args().skip(1) {
            cmd.push(' ');
            if a.is_empty() || a.chars().any(char::is_whitespace) {
                cmd.push('"');
                cmd.push_str(&a.replace('"', "\\\""));
                cmd.push('"');
            } else {
                cmd.push_str(&a);
            }
        }
        let mut cmdw: Vec<u16> = cmd.encode_utf16().collect();
        cmdw.push(0);
        let mut si: STARTUPINFOW = std::mem::zeroed();
        si.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
        let mut pi: PROCESS_INFORMATION = std::mem::zeroed();
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        let ok = CreateProcessW(
            std::ptr::null(),
            cmdw.as_mut_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            0,
            DETACHED_PROCESS,
            std::ptr::null(),
            std::ptr::null(),
            &mut si,
            &mut pi,
        );
        if ok != 0 {
            // 子プロセスは自分のミューテックスを取得して常駐を引き継ぐ
            windows_sys::Win32::Foundation::CloseHandle(pi.hProcess);
            windows_sys::Win32::Foundation::CloseHandle(pi.hThread);
            println!("[info] コンソールから独立したプロセスへ引き継ぎました");
            exit(0);
        }
        // CreateProcess 失敗時はそのまま続行(コンソール依存は受容するが機能は継続)
        eprintln!("[warn] デタッチ再起動に失敗。コンソール付きで継続します");
    }
}

/// 表示用のリリースバージョン(ステータス窓等)
pub const VERSION_STR: &str = env!("CARGO_PKG_VERSION");
const BUILD_ID: &str = "win-20261002-190730-8e80c6b";

static JUST_REGISTERED: AtomicBool = AtomicBool::new(false);
fn registration_authenticated(_token: &str) {
    if JUST_REGISTERED.swap(false, Ordering::Relaxed) {
        tray::notify(
            "接続を確認しました",
            "登録した端末へ、次回から自動で接続します。",
        );
    }
}
fn main() {
    crate::state::init_boot_wall();
    if std::env::args().any(|a| a == "--probe-diag") {
        // 診断を stdout へ出して終わる(ssh からの実機検証・改善ループの自動化用)。
        // 対話セッション無しで動くため、schtasks でも同じ結果が取れる
        println!("{}", crate::diag::run());
        return;
    }
    if let Some(note) = knit_common::share::startup_note() {
        println!("{note}");
    }
    #[cfg(debug_assertions)]
    if std::env::args().any(|a| a == "--probe-setup") {
        // The test runner supplies an isolated LOCALAPPDATA directory.
        if let Some(root) = std::env::var_os("KNIT_PROBE_DATA") {
            std::env::set_var("LOCALAPPDATA", root);
            let _ = tray::setup::first_run(false);
        }
        return;
    }
    if std::env::args().any(|a| a == "--preview-setup") {
        let _ = tray::setup::first_run(true);
        return;
    }
    if std::env::args().any(|a| a == "--preview-ui") {
        tray::preview();
        return;
    }
    if let Some(pos) = std::env::args().position(|a| a == "--probe-history-image") {
        // トレイと同じ経路での履歴復元を実機検証する(画像を含む)。
        // 対話セッションで実行しないとクリップボードへ書けないため
        // schtasks 経由を想定。id 無しなら最近の履歴を一覧して終わる
        let id: u64 = std::env::args()
            .nth(pos + 1)
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        history_load();
        if id == 0 {
            println!("[probe] 使い方: --probe-history-image <id>");
            if let Ok(h) = HISTORY.lock() {
                for e in h.recent(5) {
                    println!(
                        "[probe] id={} kind={:?} device={} {}",
                        e.id,
                        e.kind,
                        e.device,
                        e.text.split('\t').next().unwrap_or("")
                    );
                }
            }
            return;
        }
        history_restore_by_id(id);
        println!("[probe] 履歴 id={id} の復元を試みました(結果は直前の [clip] 行)");
        return;
    }
    // 操作補助(UAC・管理者権限のアプリへの入力)。導入/撤去は管理者権限で実行する
    for (flag, run) in [
        ("--install-input-helper", helper::install as fn() -> i32),
        ("--uninstall-input-helper", helper::uninstall),
        ("--input-supervisor", helper::run_supervisor),
        ("--input-helper", helper::run_helper),
    ] {
        if std::env::args().any(|a| a == flag) {
            ensure_stdout();
            std::process::exit(run());
        }
    }
    if let Some(pos) = std::env::args().position(|a| a == "--apply-update") {
        let rest: Vec<String> = std::env::args().skip(pos + 1).collect();
        std::process::exit(updater::run_apply(&rest));
    }
    // 更新の入れ替え中は起動しない(毎分の自動復帰タスクが旧版を起こす競合を防ぐ)
    if updater::update_in_progress() {
        return;
    }
    ensure_stdout();
    // コンソール付き起動なら DETACHED な自分へ置き換わって終了(常駐性の根保証)
    detach_if_console();
    let retry_setup = std::env::args().any(|a| a == "--retry-setup");
    let mut acquired = acquire_single_instance();
    if !acquired && retry_setup {
        for _ in 0..20 {
            std::thread::sleep(Duration::from_millis(100));
            acquired = acquire_single_instance();
            if acquired {
                break;
            }
        }
    }
    if !acquired {
        return;
    }

    println!("[info] knit-win {BUILD_ID}");
    // 起動時に受信フォルダと履歴を用意する(通知のパスが必ず有効になる)
    let recv_dir = std::env::var_os("USERPROFILE")
        .map(std::path::PathBuf::from)
        .unwrap_or_default()
        .join("Downloads")
        .join("Knit");
    let _ = std::fs::create_dir_all(&recv_dir);
    // 前回終了時に残った受信中ファイルの一時実体(.knit-*.part)を掃除する
    //(exit は Receiver の Drop を飛ばすための保険。Mac 側と同じ起動時掃除)
    let swept = knit_common::files::sweep_temp_files(&recv_dir);
    if swept > 0 {
        println!("[file] 前回の受信中ファイルの残骸を {swept} 件掃除しました");
    }
    history_load();
    // 画像履歴の実体を件数(60)と総量(512MiB)の両上限へ刈り込む(起動時に 1 回)。
    // 保存時の刈り込みは画像が届いた時しか走らないため、ここで残った超過を掃く
    clipboard::prune_image_store_now();
    // 前回異常終了した際のスピーカーミュート残留を、接続を始める前に復元する
    // (退避記録が無ければ何もしない。ここを接続処理より後に置くと、確立時の
    // ミュート適用と競合して復元の意味が無くなる)
    audio::speaker_restore_leftover();
    println!("[info] 操作ガイド: Mac から来るカーソルはそのまま操作できます。トレイ右クリックにクリップボード履歴があります");
    let args: Vec<String> = std::env::args().collect();
    // 対話デスクトップへ接続(SSH 起動では失敗する。schtasks/スタートアップ起動を使う)
    unsafe {
        // UAC・ロック画面の表示中は保護デスクトップが入力デスクトップで開けない。
        // 対話セッション外の起動と区別できないため、少し待って取れなければ終了する
        // (毎分の自動復帰タスクが起こし直す)
        let mut desk = OpenInputDesktop(0, false, 0x01FF);
        for _ in 0..60 {
            if !desk.is_null() {
                break;
            }
            std::thread::sleep(Duration::from_secs(1));
            desk = OpenInputDesktop(0, false, 0x01FF);
        }
        if desk.is_null() {
            eprintln!("[fatal] OpenInputDesktop failed. 対話セッションで起動してください");
            exit(1);
        }
        if SetThreadDesktop(desk) == 0 {
            eprintln!("[fatal] SetThreadDesktop failed");
            CloseDesktop(desk);
            exit(1);
        }
    }

    // トークンは双方向の共有鍵。ハンドシェイクの成否が oracle になるため短い
    // トークンは LAN 内の総当たりで破られる。128bit 相当(32 文字)を下限に
    let token = if let Some(t) = knit_common::envutil::get("KNIT_TOKEN").filter(|t| t.len() >= 32) {
        t
    } else if knit_common::envutil::get("KNIT_TOKEN").is_some_and(|t| !t.is_empty()) {
        eprintln!(
            "[fatal] KNIT_TOKEN が短すぎます(32 文字未満)。scripts/gen-token.sh で生成してください"
        );
        exit(1);
    } else {
        match knit_common::credentials::load() {
            Ok(Some(t)) => t,
            Ok(None) if args.iter().any(|a| a == "--background") => return,
            Ok(None) => match tray::setup::first_run(false) {
                Some(t) => {
                    JUST_REGISTERED.store(true, Ordering::Relaxed);
                    t
                }
                None => return,
            },
            Err(_) => {
                let background = args.iter().any(|a| a == "--background");
                // 読み取れない登録が残っている限り再起動しても同じ場所で止まる。
                // 確認のうえ初期化して初回登録へ導く(キャンセル時は従来どおり終了)
                if background || !tray::setup::confirm_broken_registration_reset() {
                    if !background {
                        tray::setup::error("保存した接続キーを読み取れません。Windowsのユーザーと保存先を確認してください。");
                    }
                    eprintln!("[setup] credential store unavailable");
                    return;
                }
                if let Err(e) = knit_common::credentials::delete() {
                    tray::setup::error(&format!(
                        "保存した接続キーを削除できませんでした。\n{e}"
                    ));
                    eprintln!("[setup] credential delete failed: {e}");
                    return;
                }
                eprintln!("[setup] 読み取れない登録を初期化しました。初回登録をやり直します");
                match tray::setup::first_run(false) {
                    Some(t) => {
                        JUST_REGISTERED.store(true, Ordering::Relaxed);
                        t
                    }
                    None => return,
                }
            }
        }
    };
    let port: u16 = args
        .iter()
        .position(|a| a == "--port")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok())
        .unwrap_or(PORT);

    let (_, _, w, h) = refresh_vscreen();
    println!("[info] desktop attached. screen {w}x{h}. listening on :{port}");

    inputdesk::start();
    doctor::start();
    helper::start_client();

    if args.iter().any(|a| a == "--debug-keys") {
        DEBUG_KEYS.store(true, Ordering::Relaxed);
    }
    // 接続先は優先度順に: --host 引数 > KNIT_HOST(.env 可)> Tailscale の既定
    // (有線直結 Thunderbolt ブリッジ / USB-LAN 直結の際は .env で指定する)
    let host = args
        .iter()
        .position(|a| a == "--host")
        .and_then(|i| args.get(i + 1))
        .cloned()
        .or_else(|| knit_common::envutil::get("KNIT_HOST"));
    // 接続先の既定値(開発者の環境の固定 IP)は持たない。未指定なら LAN で自動発見する
    let host_label = host.clone().unwrap_or_else(|| "LAN から自動検出".into());
    println!("[info] connecting to {host_label}");
    *crate::tray::HOST_NOW
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = host_label;

    // 接続方向: 既定は Win=クライアント(本環境のAP隔離対策)。
    // KNIT_ROLE=server(--listen)で Win=サーバ(Mac=クライアント)に反転できる
    // (通常ネットワークの配布先向け)
    // 設定画面の「ホストとして待ち受ける」(preferences.json)も受け付ける。
    // 環境変数・起動フラグが優先(GUI は配布時の既定、env は開発者の上書き)
    let role_server = args.iter().any(|a| a == "--listen")
        || knit_common::envutil::get("KNIT_ROLE").as_deref() == Some("server")
        || crate::tray::host_mode_pref();

    // 接続中スピーカーミュートの初期値(既定 ON=Mac のみ発音)
    if knit_common::envutil::get("KNIT_MUTE_SPK").as_deref() == Some("0") {
        SPK_MUTE_MODE.store(false, Ordering::Relaxed);
    }

    // タスクトレイ常駐(状態表示・バルーン通知・終了)。失敗しても本体は継続
    tray::start();
    updater::report_last_result();
    updater::cleanup_leftovers();
    updater::start_background();
    dragdrop::edge::start();

    // 音声転送(Windows→Mac)。クライアントモードの接続先へ送る
    // (サーバモードは KNIT_AUDIO_HOST で明示指定した時のみ)
    if knit_common::envutil::get("KNIT_AUDIO").as_deref() != Some("0") && knit_common::share::allow_audio() {
        // 既定は本線の接続先(複数経路のうち繋がったもの)へ追従する
        match (knit_common::envutil::get("KNIT_AUDIO_HOST"), role_server) {
            // 音声は本線ポートからの差分 +1(24900→24901)
            (Some(h), _) => audio::start(Some(h), token.clone(), port + 1),
            (None, false) => audio::start(None, token.clone(), port + 1),
            (None, true) => {
                println!("[audio] サーバモードで音声先未指定のため無効(KNIT_AUDIO_HOST で指定可)")
            }
        }
    }

    let bulk_ep: &'static bulk::Endpoint = BULK.get_or_init(|| bulk::Endpoint {
        link: &BULK_LINK,
        token: token.clone(),
        dir: std::env::var_os("USERPROFILE")
            .map(std::path::PathBuf::from)
            .unwrap_or_default()
            .join("Downloads")
            .join("Knit"),
        on_event: win_on_bulk,
        log: |s| println!("{s}"),
        on_rx_bytes: |n| RX_BYTES.store(n, Ordering::Relaxed),
        // 受信バッチの開始通知。Windows 側は履歴ラベルが接続先(Mac)固定のため
        // 特に記録しない(共通 Endpoint のフィールドとして必須の no-op)
        on_batch_begin: || {},
        // 待受 bind の初回失敗だけトレイのバルーンにも出す(接続は生きて
        // いるのにファイル・画像だけが届かない状態をログだけで終わらせない)
        on_bind_error: |s: &str| crate::tray::notify("Knit", s),
    });
    // Esc での転送中止の監視(物理・注入どちらの Esc も拾う)
    spawn_esc_cancel_watcher();

    if role_server {
        println!("[info] server mode. screen {w}x{h}");
        let bind = knit_common::envutil::get("KNIT_BIND").unwrap_or_else(|| "0.0.0.0".to_string());
        let ep = bulk_ep;
        std::thread::spawn(move || {
            bulk::serve(
                ep,
                &bind,
                port + bulk::PORT_OFFSET,
                knit_common::net::is_allowed,
            )
        });
        let tk = token.clone();
        std::thread::spawn(move || {
            if let Err(e) = knit_common::discover::respond(
                "0.0.0.0",
                port + knit_common::discover::PORT_OFFSET,
                &tk,
                knit_common::net::is_allowed,
            ) {
                println!("[disc] 発見応答の待受に失敗: {e}(自動発見が使えません)");
            }
        });
        server_loop(&token, port, w, h);
        return;
    }

    println!("[info] client mode");
    std::thread::spawn(move || {
        bulk::connect_loop(
            bulk_ep,
            || peer_ip().map(|ip| std::net::SocketAddr::new(ip, port + bulk::PORT_OFFSET)),
            || CONNECTED.load(Ordering::Relaxed),
        )
    });
    client_loop(host, port, &token, w, h);
}

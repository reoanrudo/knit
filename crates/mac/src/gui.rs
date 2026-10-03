// メニューバー常駐 GUI(NSStatusItem + NSMenu)。
// objc_msgSend 固定シグネチャ方式(実績パターン#1。依存追加ゼロ)。
// objc2-app-kit への移行はモジュール分割(Wave3 Step6)時に検討する。
// 呼び出し規約: このモジュールの全関数はメインスレッドから呼ぶこと
// (start() は main() の末尾、IMP は AppKit のイベント配信=メインRunLoop)。

use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Mutex;

use crate::{msg0, msg0_cstr, nsstring, objc_getClass};

type ID = *mut core::ffi::c_void;
type SEL = *mut core::ffi::c_void;
type CLS = *mut core::ffi::c_void;

mod text_input;
pub(crate) mod direct_input;
mod preferences;

/// Windows の設定画面向け: 設定一覧と、Windows からの変更の適用
pub fn prefs_snapshot_json() -> String {
    preferences::snapshot_json()
}
pub fn apply_remote_prefs(json: &str) {
    preferences::apply_remote(json);
}
mod prefs;
pub mod setup;
pub static UI_PREVIEW: AtomicBool = AtomicBool::new(false);
pub fn restore_preferences() {
    preferences::restore();
}

/// セレクタ登録の薄いラッパ(extern fn はそのまま import できないため)
unsafe fn sel(name: &std::ffi::CStr) -> SEL {
    crate::sel_registerName(name.as_ptr())
}

#[link(name = "AppKit", kind = "framework")]
#[link(name = "CoreFoundation", kind = "framework")]
#[link(name = "CoreGraphics", kind = "framework")]
#[link(name = "objc", kind = "dylib")]
unsafe extern "C" {
    static kCFRunLoopCommonModes: *mut core::ffi::c_void;
    fn objc_allocateClassPair(
        superclass: CLS,
        name: *const core::ffi::c_char,
        extra_bytes: usize,
    ) -> CLS;
    fn class_addMethod(cls: CLS, name: SEL, imp: usize, types: *const core::ffi::c_char) -> u8;
    fn objc_registerClassPair(cls: CLS);
    // メニューバーアイコンを CoreGraphics で描くための最小セット
    fn CGColorSpaceCreateDeviceRGB() -> *mut core::ffi::c_void;
    fn CGBitmapContextCreate(
        data: *mut u8,
        width: usize,
        height: usize,
        bits_per_component: usize,
        bytes_per_row: usize,
        space: *mut core::ffi::c_void,
        bitmap_info: u32,
    ) -> *mut core::ffi::c_void;
    fn CGBitmapContextCreateImage(ctx: *mut core::ffi::c_void) -> *mut core::ffi::c_void;
    fn CGContextRelease(ctx: *mut core::ffi::c_void);
    fn CGImageRelease(img: *mut core::ffi::c_void);
    fn CGContextSetRGBStrokeColor(ctx: *mut core::ffi::c_void, r: f64, g: f64, b: f64, a: f64);
    fn CGContextSetLineWidth(ctx: *mut core::ffi::c_void, w: f64);
    fn CGContextSetLineCap(ctx: *mut core::ffi::c_void, cap: u32);
    fn CGContextBeginPath(ctx: *mut core::ffi::c_void);
    fn CGContextMoveToPoint(ctx: *mut core::ffi::c_void, x: f64, y: f64);
    fn CGContextStrokePath(ctx: *mut core::ffi::c_void);
    fn CFRelease(cf: *mut core::ffi::c_void);
}

// ---------- 固定シグネチャ呼び出しヘルパ(この画面で必要なものだけ) ----------

unsafe fn msg0_void(target: ID, cmd: SEL) {
    let f: unsafe extern "C" fn(ID, SEL) =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    f(target, cmd)
}
unsafe fn msg1_void_id(target: ID, cmd: SEL, a: ID) {
    let f: unsafe extern "C" fn(ID, SEL, ID) =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    f(target, cmd, a)
}
unsafe fn msg1_void_sel(target: ID, cmd: SEL, a: SEL) {
    let f: unsafe extern "C" fn(ID, SEL, SEL) =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    f(target, cmd, a)
}
unsafe fn msg1_void_u8(target: ID, cmd: SEL, a: u8) {
    let f: unsafe extern "C" fn(ID, SEL, u8) =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    f(target, cmd, a)
}
unsafe fn msg1_void_i64(target: ID, cmd: SEL, a: i64) {
    let f: unsafe extern "C" fn(ID, SEL, i64) =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    f(target, cmd, a)
}
unsafe fn msg2_void_id_id(target: ID, cmd: SEL, a: ID, b: ID) {
    let f: unsafe extern "C" fn(ID, SEL, ID, ID) =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    f(target, cmd, a, b)
}
unsafe fn msg1_id_f64(target: ID, cmd: SEL, a: f64) -> ID {
    let f: unsafe extern "C" fn(ID, SEL, f64) -> ID =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    f(target, cmd, a)
}
unsafe fn msg3_id(target: ID, cmd: SEL, a: ID, b: SEL, c: ID) -> ID {
    let f: unsafe extern "C" fn(ID, SEL, ID, SEL, ID) -> ID =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    f(target, cmd, a, b, c)
}
unsafe fn msg5_timer(target: ID, cmd: SEL, t: f64, a: ID, b: SEL, c: ID, r: u8) -> ID {
    let f: unsafe extern "C" fn(ID, SEL, f64, ID, SEL, ID, u8) -> ID =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    f(target, cmd, t, a, b, c, r)
}

// ---------- GUI 部品への参照(AtomicUsize で生ポインタを保持) ----------

static GUI_TARGET: AtomicUsize = AtomicUsize::new(0);
static GUI_BUTTON: AtomicUsize = AtomicUsize::new(0);
static GUI_STATE_ITEM: AtomicUsize = AtomicUsize::new(0);
/// ファイル転送の中止項目(進行中だけ有効。Esc と同じ働き)
static GUI_XFER_ITEM: AtomicUsize = AtomicUsize::new(0);
/// クリップボード履歴のサブメニュー(項目は refresh_status が変化時だけ作り直す)
static GUI_HISTORY_MENU: AtomicUsize = AtomicUsize::new(0);
static GUI_UPDATE_ITEM: AtomicUsize = AtomicUsize::new(0);
static GUI_ROLE_ITEM: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
/// Android タブレットのサブメニュー(状態・操作する/しないの切替)。世代が変わった時だけ作り直す
/// 変化検知の初期値は最大値にして、起動直後の 1 回目で必ず作り直させる
static GUI_HISTORY_SEEN: AtomicU64 = AtomicU64::new(u64::MAX);
/// 履歴の見出し項目(件数表示を setTitle で更新する)
static GUI_HISTORY_ITEM: AtomicUsize = AtomicUsize::new(0);

// ---------- 設定ウィンドウ(メニュー「設定…」で開く) ----------
static PREFS_WIN: AtomicUsize = AtomicUsize::new(0);
static PREFS_CHK_AUDIO: AtomicUsize = AtomicUsize::new(0);
static PREFS_CHK_CMD: AtomicUsize = AtomicUsize::new(0);
static PREFS_CHK_SCROLL: AtomicUsize = AtomicUsize::new(0);
static PREFS_CHK_SPK: AtomicUsize = AtomicUsize::new(0);
static PREFS_SLIDER: AtomicUsize = AtomicUsize::new(0);
static PREFS_GAIN_LABEL: AtomicUsize = AtomicUsize::new(0);
static PREFS_CHK_CLIP: AtomicUsize = AtomicUsize::new(0);
static PREFS_CHK_FILES: AtomicUsize = AtomicUsize::new(0);
static PREFS_CHK_HISTORY: AtomicUsize = AtomicUsize::new(0);
static PREFS_STATE: AtomicUsize = AtomicUsize::new(0);
/// 起動直後に設定ウィンドウを開く(--show-prefs。1 秒タイマーの初回で処理)
pub static SHOW_AT_START: AtomicBool = AtomicBool::new(false);

/// NSRect(f64 x4)。戻り値受け取りにのみ使う(渡しは rect_args を使う)
#[repr(C)]
#[derive(Clone, Copy)]
struct NSRect {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

// ---------- メニュー項目のアクション(Objective-C クラスの IMP) ----------

/// スクロール速度スライダー(値=除数。小さいほど速い)。ドラッグ中も連続で飛ぶ
unsafe extern "C" fn imp_scroll_gain(_s: ID, _c: SEL, sender: ID) {
    unsafe {
        let get: unsafe extern "C" fn(ID, SEL) -> f64 =
            std::mem::transmute(crate::objc_msgSend as *const () as usize);
        let v = get(sender, sel(c"doubleValue"));
        crate::set_scroll_div(v);
        let lbl = PREFS_GAIN_LABEL.load(Ordering::Relaxed) as ID;
        if !lbl.is_null() {
            let speed = if v <= 40.0 {
                "速い"
            } else if v >= 140.0 {
                "遅い"
            } else {
                "標準"
            };
            msg1_void_id(
                lbl,
                sel(c"setStringValue:"),
                nsstring(&format!("スクロール速度: {speed}({v:.0})")),
            );
        }
    }
    preferences::save();
}

unsafe extern "C" fn imp_switch_mode(_s: ID, _c: SEL, _n: ID) {
    let next = !crate::HOTKEY_ONLY.load(Ordering::Relaxed);
    crate::HOTKEY_ONLY.store(next, Ordering::Relaxed);
    eprintln!(
        "[mode] switch_mode -> {}",
        if next { "hotkey(ロック)" } else { "edge" }
    );
    refresh_status();
    preferences::save();
}
unsafe extern "C" fn imp_edge_taps(_s: ID, _c: SEL, _n: ID) {
    let next = if crate::EDGE_TAPS.load(Ordering::Relaxed) >= 2 {
        1
    } else {
        2
    };
    crate::EDGE_TAPS.store(next, Ordering::Relaxed);
    eprintln!("[mode] edge_taps -> {next}");
    refresh_status();
    preferences::save();
}
unsafe extern "C" fn imp_open_log(_s: ID, _c: SEL, _n: ID) {
    let _ = std::process::Command::new("open")
        .args(["-a", "Console", "/tmp/knit-mac.log"])
        .spawn();
}

/// 履歴メニューの「消す」クリック。本文は不要(メニュー操作で即反映)
unsafe extern "C" fn imp_history_clear(_s: ID, _c: SEL, _n: ID) {
    crate::history_clear();
    crate::HISTORY_LAST_ID.store(0, Ordering::Relaxed);
}

/// 進行中のファイル転送の中止(Esc と同じ働き)
unsafe extern "C" fn imp_cancel_xfer(_s: ID, _c: SEL, _n: ID) {
    if crate::cancel_active_xfer() {
        eprintln!("[file] メニューから転送の中止を要求しました");
    }
}

/// クリップボード履歴メニューの項目クリック。representedObject(NSString)から
/// 本文を取り出して Mac のクリップボードへ復元する
unsafe extern "C" fn imp_history_restore(_s: ID, _c: SEL, sender: ID) {
    if sender.is_null() {
        return;
    }
    let obj = msg0(sender, sel(c"representedObject"));
    if obj.is_null() {
        return;
    }
    let utf8 = crate::msg0_cstr(obj, sel(c"UTF8String"));
    if utf8.is_null() {
        return;
    }
    let text = std::ffi::CStr::from_ptr(utf8)
        .to_string_lossy()
        .into_owned();
    if !text.is_empty() {
        crate::history_restore(text);
    }
}
unsafe extern "C" fn imp_restart(_s: ID, _c: SEL, _n: ID) {
    if UI_PREVIEW.load(Ordering::Relaxed) {
        return;
    }
    eprintln!("[gui] 再起動します");
    restart_now();
}

/// 再起動のためのシェル文字列。1 秒待って(古いプロセスが終わり二重起動のロックが
/// 空くのを待って)、同じ実行ファイル・同じ引数で起こし直す。.app の中なら .app ごと開く
fn restart_command(exe: &std::path::Path, args: &[String]) -> String {
    fn q(s: &str) -> String {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
    let exe_s = exe.to_string_lossy();
    if let Some(i) = exe_s.find(".app/Contents/MacOS/") {
        let bundle = &exe_s[..i + 4];
        return format!("sleep 1; /usr/bin/open -n {}", q(bundle));
    }
    let mut cmd = format!("sleep 1; exec {}", q(&exe_s));
    for a in args {
        cmd.push(' ');
        cmd.push_str(&q(a));
    }
    cmd
}

/// 自分を終了して、すぐ起こし直す(スクリプトや LaunchAgent に頼らない)。
/// Windows へ操作中なら先に Mac へ戻し、環境変数と標準出力はそのまま引き継ぐ。
/// Role 受信の適用スレッドと自発的切替の待ちスレッドの両方から呼ばれるため、
/// 二重進入で .app を二重起動しないよう先頭で1回だけ通す
fn restart_now() {
    use std::os::unix::process::CommandExt;
    static RESTARTING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if RESTARTING.swap(true, Ordering::Relaxed) {
        return;
    }
    if crate::WIN_MODE.swap(false, Ordering::Relaxed) {
        crate::leave_win_mode_cursor_unlock(None);
    }
    let Ok(exe) = std::env::current_exe() else {
        crate::notify("Knit", "実行ファイルの場所が分からず再起動できません");
        return;
    };
    let args: Vec<String> = std::env::args().skip(1).collect();
    match std::process::Command::new("/bin/sh")
        .arg("-c")
        .arg(restart_command(&exe, &args))
        .process_group(0)
        .spawn()
    {
        Ok(_) => std::process::exit(0),
        Err(e) => {
            eprintln!("[gui] 再起動を始められません: {e}");
            crate::notify("Knit", "再起動できませんでした。ログを確認してください");
        }
    }
}

/// 相手(Windows)から接続の方向の切替を知らされたときの対応(別スレッドから呼ばれる)。
/// 相手がホストになるならこの Mac は接続側へ、相手が接続側へ戻るなら待ち受けへ
pub fn apply_peer_role(peer_is_host: bool) {
    if crate::CLIENT_ROLE.load(Ordering::Relaxed) == peer_is_host {
        return;
    }
    let peer = crate::active_peer_label();
    crate::CLIENT_ROLE.store(peer_is_host, Ordering::Relaxed);
    preferences::save_quiet();
    eprintln!("[role] 相手の切替に合わせて {} へ変更し再起動します", if peer_is_host { "接続側" } else { "待ち受け" });
    let message = if peer_is_host {
        format!("{peer} がホストになったため、この Mac を接続側に切り替えて再起動します")
    } else {
        format!("{peer} が接続側へ戻ったため、この Mac を待ち受けに切り替えて再起動します")
    };
    crate::notify("Knit", &message);
    std::thread::sleep(std::time::Duration::from_millis(600));
    restart_now();
}

/// 相手(Windows)の役割切替の適用完了(RoleAck)。再起動を待っていた set_role が参照する
static ROLE_ACK: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// 本線受信ループから呼ぶ: 相手の適用が済んだ合図を立てる
pub fn note_role_ack() {
    ROLE_ACK.store(true, Ordering::Relaxed);
}
/// 相手の適用完了を待つ(旧版相手は返さないためタイムアウトで諦め、時間経過で再起動)。
/// 確認できた時点で true を戻して消費する(前回分が次回の待ちに響かないように)
fn wait_role_ack(timeout: std::time::Duration) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if ROLE_ACK.swap(false, Ordering::Relaxed) {
            return true;
        }
        if std::time::Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

/// 接続の方向メニュー項目の表示文言(接続先が Android でも成り立つ中性の言葉)
fn role_menu_title() -> String {
    let peer = crate::active_peer_label();
    if crate::CLIENT_ROLE.load(Ordering::Relaxed) {
        format!("{peer} をホストにする(解除してこの Mac が待ち受ける)")
    } else {
        format!("{peer} をホストにする")
    }
}

/// 「{相手} をホストにする」: この Mac を接続側へ切り替える(起動時に決まるため再起動で反映)
unsafe extern "C" fn imp_host_role(_s: ID, _c: SEL, _n: ID) {
    set_role(!crate::CLIENT_ROLE.load(Ordering::Relaxed));
}

/// 設定画面の「接続の方向」(ラジオ)。tag 0=この Mac がホスト / 1=Windows がホスト
unsafe extern "C" fn imp_role(_s: ID, _c: SEL, sender: ID) {
    let want_client = crate::msg0_isize(sender, sel(c"tag")) == 1;
    set_role(want_client);
}

/// 役割を決める。すでにその役割なら選択表示だけ合わせ直す
unsafe fn set_role(next: bool) {
    if UI_PREVIEW.load(Ordering::Relaxed) {
        return;
    }
    if crate::CLIENT_ROLE.load(Ordering::Relaxed) == next {
        prefs::sync_role();
        return;
    }
    crate::CLIENT_ROLE.store(next, Ordering::Relaxed);
    let peer = crate::active_peer_label();
    eprintln!("[role] {peer} をホストにする -> {next}");
    preferences::save();
    let item = GUI_ROLE_ITEM.load(Ordering::Relaxed) as ID;
    if !item.is_null() {
        msg1_void_id(item, sel(c"setTitle:"), nsstring(&role_menu_title()));
    }
    // 相手にも反対の役割へ合わせさせる(双方が待ち受けになってつながらないのを防ぐ)。
    // 版 15 以降の相手は適用済みの RoleAck を返すので、それを確認してから
    // 再起動する(行き損ねで双方が同役割のまま沈黙するのを防ぐ)。旧版相手は
    // 応答しないため、従来どおり時間経過で再起動する
    ROLE_ACK.store(false, Ordering::Relaxed);
    crate::send_msg(&knit_common::proto::Msg::Role { host: !next });
    let message = if next {
        format!("この Mac を {peer}(ホスト)へ接続する側に切り替えました。Knit を再起動します")
    } else {
        "この Mac を接続を待ち受ける側に戻しました。Knit を再起動します".to_string()
    };
    crate::notify("Knit", &message);
    std::thread::spawn(|| {
        if wait_role_ack(std::time::Duration::from_millis(2000)) {
            eprintln!("[role] 相手の適用を確認しました");
        } else {
            eprintln!("[role] 相手の適用確認が取れないため時間経過で再起動します");
        }
        restart_now();
    });
}

unsafe extern "C" fn imp_audio_toggle(_s: ID, _c: SEL, _n: ID) {
    let next = !crate::audio::MUTED.load(Ordering::Relaxed);
    crate::audio::MUTED.store(next, Ordering::Relaxed);
    eprintln!("[audio] mute -> {next}");
    // 再生ミュートは相手の音声ストリームにも影響する(Cfg の listen)ため伝える
    crate::send_cfg();
    refresh_status();
    preferences::save();
}
unsafe extern "C" fn imp_cmd_map(_s: ID, _c: SEL, _n: ID) {
    let next = !crate::CMD_ALT.load(Ordering::Relaxed);
    crate::CMD_ALT.store(next, Ordering::Relaxed);
    eprintln!("[cfg] ⌘キー -> {}", if next { "Alt" } else { "Ctrl" });
    crate::send_cfg();
    refresh_status();
    preferences::save();
}
unsafe extern "C" fn imp_spk_mute(_s: ID, _c: SEL, _n: ID) {
    let next = !crate::SPK_MUTE.load(Ordering::Relaxed);
    crate::SPK_MUTE.store(next, Ordering::Relaxed);
    eprintln!(
        "[cfg] 接続中スピーカーミュート -> {}",
        if next { "ON" } else { "OFF" }
    );
    crate::send_cfg();
    refresh_status();
    preferences::save();
}
/// 「Windows の位置」ポップアップ(0=右/1=左/2=上/3=下)。Deskflow links 相当
unsafe extern "C" fn imp_side(_s: ID, _c: SEL, sender: ID) {
    unsafe {
        let get: unsafe extern "C" fn(ID, SEL) -> isize =
            std::mem::transmute(crate::objc_msgSend as *const () as usize);
        let idx = get(sender, sel(c"indexOfSelectedItem"));
        crate::set_side(idx.clamp(0, 7) as u8);
        *LAY_WIN.lock().unwrap_or_else(|e| e.into_inner()) = (0.0, 0.0);
    }
    preferences::save();
}


/// カーソル速度スライダ(0.2..3.0。倍率=Windows 上の移動量)
unsafe extern "C" fn imp_mouse_scale(_s: ID, _c: SEL, sender: ID) {
    unsafe {
        let get: unsafe extern "C" fn(ID, SEL) -> f64 =
            std::mem::transmute(crate::objc_msgSend as *const () as usize);
        let v = get(sender, sel(c"doubleValue"));
        crate::set_mouse_scale(v);
    }
    preferences::save();
}

/// スクロール互換モードのトグル(120 未満を無視する古いアプリ向け)
unsafe extern "C" fn imp_scroll_compat(_s: ID, _c: SEL, _n: ID) {
    let next = !crate::SCROLL_COMPAT.load(Ordering::Relaxed);
    crate::SCROLL_COMPAT.store(next, Ordering::Relaxed);
    eprintln!("[cfg] スクロール互換モード -> {next}");
    preferences::save();
}

/// clipboardSharing トグル(Deskflow 標準オプション)
unsafe extern "C" fn imp_clip_share(_s: ID, _c: SEL, _n: ID) {
    let next = !crate::CLIP_SHARE.load(Ordering::Relaxed);
    crate::CLIP_SHARE.store(next, Ordering::Relaxed);
    eprintln!("[cfg] クリップボード共有 -> {next}");
    crate::send_cfg();
    preferences::save();
}

/// Mac のコピーを履歴に残す(画面を越えなくても、メニューバーの履歴から選び直せる)
unsafe extern "C" fn imp_local_history(_s: ID, _c: SEL, _n: ID) {
    let next = !crate::LOCAL_HISTORY.load(Ordering::Relaxed);
    crate::LOCAL_HISTORY.store(next, Ordering::Relaxed);
    eprintln!("[cfg] Macのコピーを履歴に残す -> {next}");
    preferences::save();
}

/// ファイルの受け渡し(この Mac が共有するファイル・ドラッグの許可。環境変数 KNIT_SHARE の内側でだけ効く)
unsafe extern "C" fn imp_file_share(_s: ID, _c: SEL, _n: ID) {
    let next = !knit_common::share::user_files();
    knit_common::share::set_user_files(next);
    eprintln!("[cfg] ファイルの受け渡し -> {next}");
    preferences::save();
}

/// メニュー「Windows の位置」: 右→左→上→下→右 のローテート
unsafe extern "C" fn imp_rotate_side(_s: ID, _c: SEL, _n: ID) {
    let next = (crate::SIDE.load(Ordering::Relaxed) + 1) % 4;
    crate::set_side(next);
    refresh_status();
    preferences::save();
}

/// 配置エディタ(独立ウィンドウ)を開く
/// 配置エディタ(独立ウィンドウ)を開く(メニュー IMP と起動直後の両方から呼ぶ)
pub fn show_layout() {
    show_prefs();
    unsafe {
        prefs::select_page(1);
    }
}

unsafe extern "C" fn imp_show_layout(_s: ID, _c: SEL, _n: ID) {
    show_layout();
}

unsafe extern "C" fn imp_scroll_flip(_s: ID, _c: SEL, _n: ID) {
    let next = !crate::SCROLL_FLIP.load(Ordering::Relaxed);
    crate::SCROLL_FLIP.store(next, Ordering::Relaxed);
    // SCROLL_FLIP=true は「Windows 標準へ固定」。false(既定)は Mac の設定に合わせる
    eprintln!(
        "[cfg] スクロール方向 -> {}",
        if next {
            "Windows 標準に固定"
        } else {
            "Mac の設定に合わせる"
        }
    );
    refresh_status();
    preferences::save();
}
unsafe extern "C" fn imp_vol(_s: ID, _c: SEL, sender: ID) {
    // 3 つのメニュー項目(▲/▼/ミュート)から送信元 tag で判別する
    let tag: isize = {
        let f: unsafe extern "C" fn(ID, SEL) -> isize =
            std::mem::transmute(crate::objc_msgSend as *const () as usize);
        f(sender, sel(c"tag"))
    };
    let op = match tag {
        1 => 0u8, // up
        2 => 1,   // down
        _ => 2,   // mute
    };
    crate::send_msg(&crate::Msg::Vol { op });
}

unsafe extern "C" fn imp_send_file(_s: ID, _c: SEL, _n: ID) {
    // accessory アプリでも明示アクティベートすればモーダルパネルは出せる
    let app = msg0(
        crate::objc_getClass(c"NSApplication".as_ptr()),
        sel(c"sharedApplication"),
    );
    msg1_void_u8(app, sel(c"activateIgnoringOtherApps:"), 1);
    let panel = msg0(
        crate::objc_getClass(c"NSOpenPanel".as_ptr()),
        sel(c"openPanel"),
    );
    if panel.is_null() {
        eprintln!("[gui] NSOpenPanel を生成できません");
        return;
    }
    msg1_void_u8(panel, sel(c"setCanChooseFiles:"), 1);
    msg1_void_u8(panel, sel(c"setCanChooseDirectories:"), 0);
    msg1_void_u8(panel, sel(c"setAllowsMultipleSelection:"), 1);
    // 送信の起点をデスクトップへ合わせる(手元で作った一時ファイルの定位置)
    let fm = msg0(
        objc_getClass(c"NSFileManager".as_ptr()),
        sel(c"defaultManager"),
    );
    if !fm.is_null() {
        let at: unsafe extern "C" fn(ID, SEL, usize, usize) -> ID =
            std::mem::transmute(crate::objc_msgSend as *const () as usize);
        let desktop = at(
            fm,
            sel(c"URLsForDirectory:inDomains:"),
            12, /*NSDesktopDirectory*/
            1,  /*NSUserDomainMask*/
        );
        if !desktop.is_null() {
            msg1_void_id(panel, sel(c"setDirectoryURL:"), desktop);
        }
    }
    msg1_void_id(
        panel,
        sel(c"setMessage:"),
        crate::nsstring(&format!(
            "{} へ送信します(1回の合計 {} まで)",
            crate::active_peer_label(),
            knit_common::bulk::file_limit_label()
        )),
    );
    // runModal は選択が確定するまで戻らない(メイン RunLoop を内回りする)
    let resp = crate::msg0_isize(panel, sel(c"runModal"));
    if resp != 1 {
        return; // NSModalResponseOK 以外 = キャンセル
    }
    let urls = msg0(panel, sel(c"URLs"));
    if urls.is_null() {
        return;
    }
    let n = crate::msg0_isize(urls, sel(c"count")).max(0);
    let at: unsafe extern "C" fn(ID, SEL, usize) -> ID =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    let mut paths = Vec::new();
    // 切り詰めず本当の件数を送る(上限は send_files_to_win 側の審査に任せる)
    for i in 0..n {
        let url = at(urls, sel(c"objectAtIndex:"), i as usize);
        if url.is_null() {
            continue;
        }
        let path = msg0(url, sel(c"path")); // NSURL.path -> NSString
        if path.is_null() {
            continue;
        }
        let utf8 = crate::msg0_cstr(path, sel(c"UTF8String"));
        if utf8.is_null() {
            continue;
        }
        let s = std::ffi::CStr::from_ptr(utf8)
            .to_string_lossy()
            .into_owned();
        if !s.is_empty() {
            paths.push(s);
        }
    }
    if !paths.is_empty() {
        let pb: Vec<std::path::PathBuf> = paths.into_iter().map(std::path::PathBuf::from).collect();
        eprintln!("[gui] ファイル送信: {} 件", pb.len());
        // 共有範囲の審査は Windows 経路と ADB 経路のどちらでも必ず通す
        //(UI の無効化だけでは抜け穴になるため)
        if !knit_common::share::allow_files() {
            crate::notify("Knit", "この Mac ではファイルの共有が許可されていません(KNIT_SHARE)");
        } else if crate::active_peer_is_android() && !crate::active_peer_is_android_app() {
            // ADB中継は大容量経路を持たないため Download へ置く。adb push は
            // ファイルごとに最大120秒待つため、メニュー操作(UI スレッド)を
            // 固めないよう別スレッドで実行する
            std::thread::spawn(move || {
                match crate::android::push_files(&pb) {
                    Some(n) => crate::notify(
                        "Knit",
                        &format!("タブレットの Download へ {n} 件を保存しました"),
                    ),
                    None => crate::notify("Knit", "タブレットが接続されていないため送れませんでした"),
                }
            });
        } else {
            if let crate::SendFilesOutcome::Busy = crate::send_files_to_win(pb) {
                crate::notify(
                    "Knit",
                    "前のファイルを転送中のため開始できませんでした。完了後にもう一度お試しください",
                );
            }
        }
    }
}
/// 設定ウィンドウを開く(初回のみ生成。以降は同一ウィンドウを前面化)。
/// メニューの IMP と起動直後(--show-prefs)の両方から呼ぶ
/// NSException の内容を abort 前にログへ(objc が unwind 前に呼ぶ)
unsafe extern "C" fn uncaught_exc_handler(exc: ID) {
    unsafe {
        let name = msg0_cstr(msg0(exc, sel(c"name")), sel(c"UTF8String"));
        let reason = msg0_cstr(msg0(exc, sel(c"reason")), sel(c"UTF8String"));
        eprintln!(
            "[prefs] NSException name={:?} reason={:?}",
            if name.is_null() {
                None
            } else {
                std::ffi::CStr::from_ptr(name).to_str().ok()
            },
            if reason.is_null() {
                None
            } else {
                std::ffi::CStr::from_ptr(reason).to_str().ok()
            },
        );
    }
}

pub fn show_prefs() {
    eprintln!("[prefs] enter");
    // スクロール方向は起動時の macOS 設定(自然スクロール)のスナップショットで
    // 決まる。起動中にシステム設定で切り替えた場合に反映させるため、設定画面を
    // 開くタイミングで再取得する(defaults read はサブプロセス呼び出しなので
    // 設定画面を開く時に限る。手動上書き(KNIT_SCROLL_FLIP=1)中は触らない)
    if !crate::SCROLL_FLIP.load(Ordering::Relaxed) {
        let fresh = crate::state::detect_natural_scroll();
        if fresh != crate::NATURAL_SCROLL.swap(fresh, Ordering::Relaxed) {
            eprintln!(
                "[prefs] macOS のスクロール方向設定の変化を反映(自然スクロール: {})",
                fresh
            );
        }
    }
    unsafe {
        let app = msg0(
            objc_getClass(c"NSApplication".as_ptr()),
            sel(c"sharedApplication"),
        );
        if !app.is_null() {
            // メニューバー常駐型は非アクティブなので明示的に前面化する
            msg1_void_u8(app, sel(c"activateIgnoringOtherApps:"), 1);
        }
        extern "C" {
            fn NSSetUncaughtExceptionHandler(h: Option<unsafe extern "C" fn(ID)>);
        }
        NSSetUncaughtExceptionHandler(Some(uncaught_exc_handler));
        eprintln!("[prefs] activated");
        let existing = PREFS_WIN.load(Ordering::Relaxed) as ID;
        if existing.is_null() {
            let target = GUI_TARGET.load(Ordering::Relaxed) as ID;
            let win = make_prefs_window(target);
            if win.is_null() {
                eprintln!("[gui] 設定ウィンドウの生成に失敗");
                return;
            }
            PREFS_WIN.store(win as usize, Ordering::Relaxed);
        }
        let win = PREFS_WIN.load(Ordering::Relaxed) as ID;
        // 最小化中は makeKeyAndOrderFront だけでは戻らない(未最小化なら無害)
        msg1_void_id(win, sel(c"deminiaturize:"), std::ptr::null_mut());
        msg1_void_id(win, sel(c"makeKeyAndOrderFront:"), std::ptr::null_mut());
        msg0_void(win, sel(c"orderFrontRegardless"));
        // macOS 14 以降は activateIgnoringOtherApps: だけだと前面化されないことがある。
        // ウィンドウを出した後に新 API(activate)でもう一度要求する
        if !app.is_null() {
            let responds: unsafe extern "C" fn(ID, SEL, SEL) -> u8 =
                std::mem::transmute(crate::objc_msgSend as *const () as usize);
            if responds(app, sel(c"respondsToSelector:"), sel(c"activate")) != 0 {
                msg0_void(app, sel(c"activate"));
            }
        }
        sync_prefs_state();
    }
}

unsafe extern "C" fn imp_show_prefs(_s: ID, _c: SEL, _n: ID) {
    show_prefs();
}

/// チェックボックスの見た目を本体の状態(static)へ同期する(1秒タイマーから)
fn sync_prefs_state() {
    unsafe {
        prefs::sync();
        let set = |slot: &AtomicUsize, on: bool| {
            let b = slot.load(Ordering::Relaxed) as ID;
            if !b.is_null() {
                msg1_void_u8(b, sel(c"setState:"), on as u8);
            }
        };
        set(
            &PREFS_CHK_AUDIO,
            !crate::audio::MUTED.load(Ordering::Relaxed) && knit_common::share::env_cap().audio,
        );
        set(&PREFS_CHK_CMD, crate::CMD_ALT.load(Ordering::Relaxed));
        set(
            &PREFS_CHK_SCROLL,
            !crate::SCROLL_FLIP.load(Ordering::Relaxed),
        );
        set(
            &PREFS_CHK_SPK,
            crate::SPK_MUTE.load(Ordering::Relaxed) && knit_common::share::env_cap().audio,
        );
        set(
            &PREFS_CHK_CLIP,
            crate::CLIP_SHARE.load(Ordering::Relaxed) && knit_common::share::env_cap().clip,
        );
        set(&PREFS_CHK_FILES, knit_common::share::allow_files());
        set(
            &PREFS_CHK_HISTORY,
            crate::LOCAL_HISTORY.load(Ordering::Relaxed) && knit_common::share::env_cap().clip,
        );
    }
}

// ---------- モニター配置エディタ(Mac の「ディスプレイ配置」相当) ----------
// 灰色=Mac、青=Windows の矩形を描き、Windows 側をドラッグして物理配置を再現する。
// ドロップ時に「接する辺+辺に沿った接続範囲」を算出して SIDE/LAY_RANGE へ反映
const LAY_VW: f64 = 560.0;
const LAY_VH: f64 = 320.0;

/// Mac/Win 両グループの実ピクセルサイズ(外接。hello の monitors から算出)
fn lay_px() -> ((f64, f64), (f64, f64)) {
    let ((_, msz), (_, wsz)) = (mac_group(), win_group());
    (msz, wsz)
}

/// モニター群を正規化する: 原点を (0,0) に寄せ、y を上向き(View 座標)へ反転。
/// 矩形 (x, y, w, h)
type Rect = (f64, f64, f64, f64);
/// 矩形リストと外接サイズ
type RectGroup = (Vec<Rect>, (f64, f64));

/// 戻り値は (矩形リスト, 外接サイズ)
fn normalize_monitors(list: &[Rect]) -> RectGroup {
    if list.is_empty() {
        return (Vec::new(), (1.0, 1.0));
    }
    let min_x = list.iter().map(|m| m.0).fold(f64::MAX, f64::min);
    let max_x = list.iter().map(|m| m.0 + m.2).fold(f64::MIN, f64::max);
    let min_y = list.iter().map(|m| m.1).fold(f64::MAX, f64::min);
    let max_y = list.iter().map(|m| m.1 + m.3).fold(f64::MIN, f64::max);
    let w = (max_x - min_x).max(1.0);
    let h = (max_y - min_y).max(1.0);
    let rects = list
        .iter()
        .map(|m| (m.0 - min_x, max_y - (m.1 + m.3), m.2.max(1.0), m.3.max(1.0)))
        .collect();
    (rects, (w, h))
}

/// Mac 側の全モニター(内蔵+外部。実座標の相対配置を保つ)。
/// 並びは mac_displays() と同じ(メイン先頭)なので、番号で矩形を引ける
fn mac_group() -> RectGroup {
    let list: Vec<Rect> = crate::mac_displays()
        .iter()
        .map(|d| (d.x, d.y, d.w, d.h))
        .collect();
    normalize_monitors(&list)
}

// 描画・ドラッグ判定で同じ寸法を使う。通信のscreen/monitorsは書き換えない。
fn peer_group(p: &crate::PeerEntry) -> RectGroup {
    let list: Vec<Rect> = p.monitors.iter().map(|m| (m.x as f64,m.y as f64,m.w as f64,m.h as f64)).collect();
    let (mut rects, mut size) = if list.is_empty() { normalize_monitors(&[(0.0,0.0,p.screen.0,p.screen.1)]) } else { normalize_monitors(&list) };
    if p.id.starts_with("android-") {
        let d = unsafe { crate::CGMainDisplayID() };
        let mac = unsafe { crate::CGDisplayBounds(d) };
        let mm = unsafe { crate::CGDisplayScreenSize(d) };
        let display = crate::android::display::layout_size(size, crate::android::display::get(&p.id), (mac.size.w,mac.size.h),(mm.w,mm.h));
        let factor = display.0 / size.0;
        for r in &mut rects { *r = (r.0*factor,r.1*factor,r.2*factor,r.3*factor); }
        size = display;
    }
    (rects,size)
}

fn win_group() -> RectGroup {
    let peers = crate::PEERS.lock().unwrap_or_else(|e| e.into_inner());
    let act = *crate::ACTIVE_PEER.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(p) = peers.get(act) { return peer_group(p); }
    let (w,h) = *crate::WIN_SCREEN.lock().unwrap_or_else(|e| e.into_inner());
    normalize_monitors(&[(0.0,0.0,w,h)])
}

/// 共通縮尺: 両モニターを横並び + 縦に収める(重ならず両方必ず見える)
fn lay_scale() -> f64 {
    let ((mw, mh), (ww, wh)) = lay_px();
    let by_w = (LAY_VW - 80.0) / (mw + 2.0 * ww).max(1.0);
    let by_h = (LAY_VH - 40.0) / (mh + 2.0 * wh).max(1.0);
    by_w.min(by_h)
}

/// Windows 矩形のサイズ(実際の大きさ比・共通縮尺)
fn lay_win_size() -> (f64, f64) {
    let (_, (ww, wh)) = lay_px();
    let sc = lay_scale();
    (ww * sc, wh * sc)
}
static LAY_WIN: std::sync::Mutex<(f64, f64)> = std::sync::Mutex::new((0.0, 0.0)); // Win 矩形中心(0=既定=Macの右隣)

/// side(0-7)に対応する接続先ブロックの中心位置。Mac 側矩形 base の外側に置く。
/// 斜め(4-7)は基の辺(右/左)の上下半分の中心(接続範囲 LAY_RANGE の上半分/
/// 下半分と一致させる)。ここが 4 方向しか対応していないと、斜めへドロップ
/// しても表示が常に右へ戻り「移動できない」ように見える
fn lay_side_center(side: u8, base: &NSRect, w: f64, h: f64) -> (f64, f64) {
    let cy = base.y + base.h / 2.0;
    let cx = base.x + base.w / 2.0;
    let right_x = base.x + base.w + 8.0 + w / 2.0;
    let left_x = base.x - 8.0 - w / 2.0;
    let upper_y = base.y + base.h * 0.75; // 斜めの上半分の中心
    let lower_y = base.y + base.h * 0.25; // 斜めの下半分の中心
    match side {
        1 => (left_x, cy),
        2 => (cx, base.y + base.h + 8.0 + h / 2.0),
        3 => (cx, base.y - 8.0 - h / 2.0),
        4 => (right_x, upper_y), // 右上
        5 => (right_x, lower_y), // 右下
        6 => (left_x, upper_y),  // 左上
        7 => (left_x, lower_y),  // 左下
        _ => (right_x, cy),
    }
}

/// 相手矩形の中心(未設定ならアクティブ端末の side/モニターから既定位置を計算)
fn lay_win_center() -> (f64, f64) {
    let c = *LAY_WIN.lock().unwrap_or_else(|e| e.into_inner());
    if c.0 <= 0.0 {
        // アクティブ端末の設定(存在すれば)を使う。モニター指定ならその矩形の外側
        let (side, mi) = {
            let peers = crate::PEERS.lock().unwrap_or_else(|e| e.into_inner());
            let act = *crate::ACTIVE_PEER.lock().unwrap_or_else(|e| e.into_inner());
            match peers.get(act) {
                Some(p) => (p.side, p.edge_monitor),
                // 未接続時の全体設定。斜め(4-7)もそのまま使う
                None => (crate::SIDE.load(Ordering::Relaxed).min(7), None),
            }
        };
        let sc = lay_scale();
        let (rects, _) = mac_group();
        let (mox, moy) = lay_mac_origin();
        let base = match mi.and_then(|i| rects.get(i)) {
            Some((x, y, w, h)) => NSRect {
                x: mox + x * sc,
                y: moy + y * sc,
                w: w * sc,
                h: h * sc,
            },
            None => lay_mac_rect(),
        };
        let (ww, wh) = lay_win_size();
        return lay_side_center(side, &base, ww, wh);
    }
    c
}
static LAY_GRAB: std::sync::Mutex<(f64, f64)> = std::sync::Mutex::new((0.0, 0.0));
static LAY_DRAG: AtomicBool = AtomicBool::new(false);
/// ドラッグ中の端末(PEERS の添字。usize::MAX = なし)
static LAY_DRAG_IDX: AtomicUsize = AtomicUsize::new(usize::MAX);
/// 非アクティブ端末をドラッグ中の中心(アクティブ用の LAY_WIN とは別に持つ)
static LAY_DRAG_CENTER: std::sync::Mutex<(f64, f64)> = std::sync::Mutex::new((0.0, 0.0));

fn lay_mac_rect() -> NSRect {
    let ((_, (mw, mh)), _) = (mac_group(), win_group());
    let sc = lay_scale();
    let (ox, oy) = lay_mac_origin();
    NSRect {
        x: ox,
        y: oy,
        w: mw * sc,
        h: mh * sc,
    }
}

/// side(0=右/1=左/2=上/3=下/4=右上/5=右下/6=左上/7=左下)の配置セルの矩形。
/// Mac の外接矩形を 1 セルとして、その周囲 8 マスに置く(Deskflow の
/// 「セルにドラッグして置く」にあたる見せ方。斜めも置ける)
unsafe fn lay_cell_rect(mac: &NSRect, side: u8) -> NSRect {
    let (dx, dy) = match side {
        1 => (-1.0, 0.0),
        2 => (0.0, 1.0),
        3 => (0.0, -1.0),
        4 => (1.0, 1.0),
        5 => (1.0, -1.0),
        6 => (-1.0, 1.0),
        7 => (-1.0, -1.0),
        _ => (1.0, 0.0),
    };
    NSRect {
        x: mac.x + dx * mac.w,
        y: mac.y + dy * mac.h,
        w: mac.w,
        h: mac.h,
    }
}

/// 点から最も近い配置セルの side を返す。Mac 自身の上は None(置けない)
unsafe fn lay_cell_for_point(mac: &NSRect, p: (f64, f64)) -> Option<u8> {
    let mcx = mac.x + mac.w / 2.0;
    let mcy = mac.y + mac.h / 2.0;
    let dx = p.0 - mcx;
    let dy = p.1 - mcy;
    if dx.abs() < mac.w / 2.0 && dy.abs() < mac.h / 2.0 {
        return None;
    }
    let a = (dy).atan2(dx).to_degrees();
    let side = if a >= -22.5 && a < 22.5 {
        0 // 右
    } else if a >= 22.5 && a < 67.5 {
        4 // 右上
    } else if a >= 67.5 && a < 112.5 {
        2 // 上
    } else if a >= 112.5 && a < 157.5 {
        6 // 左上
    } else if a >= -67.5 && a < -22.5 {
        5 // 右下
    } else if a >= -112.5 && a < -67.5 {
        3 // 下
    } else if a >= -157.5 && a < -112.5 {
        7 // 左下
    } else {
        1 // 左
    };
    Some(side)
}

/// Mac グループ(外接)の描画原点(中央寄せ)
fn lay_mac_origin() -> (f64, f64) {
    let ((_, (mw, mh)), _) = (mac_group(), win_group());
    let sc = lay_scale();
    ((LAY_VW - mw * sc) / 2.0, (LAY_VH - mh * sc) / 2.0)
}

#[repr(C)]
#[derive(Clone, Copy)]
struct CGPoint2 {
    x: f64,
    y: f64,
}

unsafe fn lay_point_in_view(_self: ID, ev: ID) -> CGPoint2 {
    unsafe {
        let loc: unsafe extern "C" fn(ID, SEL) -> CGPoint2 =
            std::mem::transmute(crate::objc_msgSend as *const () as usize);
        let conv: unsafe extern "C" fn(ID, SEL, CGPoint2, ID) -> CGPoint2 =
            std::mem::transmute(crate::objc_msgSend as *const () as usize);
        let p = loc(ev, sel(c"locationInWindow"));
        conv(
            _self,
            sel(c"convertPoint:fromView:"),
            p,
            std::ptr::null_mut(),
        )
    }
}

/// 描画: CoreGraphics で直接塗る(graphicsPort 経由)
/// 配置エディタに表示する 1 端末分の情報(描画とドラッグ判定の共通ソース)
struct LayPeer {
    idx: usize,
    name: String,
    rects: Vec<Rect>,
    size: (f64, f64),
    /// rects と同じ並びのモニター名(取れない時は空)
    names: Vec<String>,
    active: bool,
    dragging: bool,
    center: (f64, f64),
}

/// sizeWithAttributes: の戻り値(NSSize と同じ並び)
#[repr(C)]
struct CGSize2 {
    w: f64,
    h: f64,
}

/// 全端末の表示位置を計算する。ドラッグ中の端末はその中心、アクティブ端末は
/// 自由位置(LAY_WIN)、それ以外は割り当てた辺の外側に置く
fn lay_peers() -> Vec<LayPeer> {
    let sc = lay_scale();
    let (mrects, _) = mac_group();
    let (mox, moy) = lay_mac_origin();
    let m = lay_mac_rect();
    let act = *crate::ACTIVE_PEER.lock().unwrap_or_else(|e| e.into_inner());
    let drag_idx = LAY_DRAG_IDX.load(Ordering::Relaxed);
    let drag_center = *LAY_DRAG_CENTER.lock().unwrap_or_else(|e| e.into_inner());
    let peers: Vec<(String, u8, Option<usize>, (Vec<Rect>, (f64, f64)), Vec<String>)> = {
        let peers = crate::PEERS.lock().unwrap_or_else(|e| e.into_inner());
        peers
            .iter()
            .map(|p| {
                (
                    p.name.clone(),
                    p.side,
                    p.edge_monitor,
                    peer_group(p),
                    p.monitors.iter().map(|m| m.name.clone()).collect(),
                )
            })
            .collect()
    };
    let mut out = Vec::new();
    for (i, (name, side, edge_monitor, (rects, size), names)) in peers.into_iter().enumerate() {
        let active = i == act;
        let dragging = i == drag_idx;
        let (w, h) = (size.0 * sc, size.1 * sc);
        let (cx, cy) = if dragging {
            if active {
                *LAY_WIN.lock().unwrap_or_else(|e| e.into_inner())
            } else {
                drag_center
            }
        } else if active {
            lay_win_center()
        } else {
            // 割り当てた辺の外側(モニター指定があればそのモニターの外側)。
            // 斜め(4-7)も含めて lay_side_center が 8 方向の位置を出す
            let base = match edge_monitor.and_then(|mi| mrects.get(mi)) {
                Some((x, y, bw, bh)) => NSRect {
                    x: mox + x * sc,
                    y: moy + y * sc,
                    w: bw * sc,
                    h: bh * sc,
                },
                None => m,
            };
            lay_side_center(side, &base, w, h)
        };
        out.push(LayPeer {
            idx: i,
            name,
            rects,
            size,
            names,
            active,
            dragging,
            center: (cx, cy),
        });
    }
    if out.is_empty() {
        // 未接続でも操作対象のプレースホルダ 1 台を表示する(従来と同じ)
        let (rects, size) = win_group();
        out.push(LayPeer {
            idx: usize::MAX,
            name: "接続先".to_string(),
            rects,
            size,
            names: Vec::new(),
            active: true,
            dragging: false,
            center: lay_win_center(),
        });
    }
    out
}

/// キャンバスへ文字を描く(現在の CoreGraphics コンテキストに NSString で載せる)
/// モニター名を矩形の幅に収まる文字数へ切り詰める(全角は約 1 文字=size 幅)
fn lay_fit(name: &str, width: f64, size: f64) -> String {
    let max = ((width - 6.0) / (size * 0.62)).floor().max(1.0) as usize;
    if name.chars().count() <= max {
        return name.to_string();
    }
    let mut t: String = name.chars().take(max.saturating_sub(1)).collect();
    t.push('…');
    t
}

unsafe fn lay_text(s: &str, cx: f64, y: f64, size: f64, light: bool) {
    unsafe {
        let obj = crate::nsstring(s);
        if obj.is_null() {
            return;
        }
        let font = msg1_id_f64(
            objc_getClass(c"NSFont".as_ptr()),
            sel(c"systemFontOfSize:"),
            size,
        );
        let color: ID = {
            let f: unsafe extern "C" fn(ID, SEL, f64, f64, f64, f64) -> ID =
                std::mem::transmute(crate::objc_msgSend as *const () as usize);
            f(
                objc_getClass(c"NSColor".as_ptr()),
                sel(c"colorWithCalibratedRed:green:blue:alpha:"),
                if light { 1.0 } else { 0.25 },
                if light { 1.0 } else { 0.27 },
                if light { 1.0 } else { 0.33 },
                1.0,
            )
        };
        let dict = msg0(
            objc_getClass(c"NSMutableDictionary".as_ptr()),
            sel(c"dictionary"),
        );
        msg2_void_id_id(dict, sel(c"setObject:forKey:"), font, crate::nsstring("NSFont"));
        msg2_void_id_id(dict, sel(c"setObject:forKey:"), color, crate::nsstring("NSColor"));
        let sz: CGSize2 = {
            let f: unsafe extern "C" fn(ID, SEL, ID) -> CGSize2 =
                std::mem::transmute(crate::objc_msgSend as *const () as usize);
            f(obj, sel(c"sizeWithAttributes:"), dict)
        };
        let f: unsafe extern "C" fn(ID, SEL, CGPoint2, ID) =
            std::mem::transmute(crate::objc_msgSend as *const () as usize);
        f(
            obj,
            sel(c"drawAtPoint:withAttributes:"),
            CGPoint2 {
                x: cx - sz.w / 2.0,
                y,
            },
            dict,
        );
    }
}

unsafe extern "C" fn lay_draw(_self: ID, _cmd: SEL, _r: NSRect) {
    unsafe {
        let ctx_cls = objc_getClass(c"NSGraphicsContext".as_ptr());
        let cur = msg0(ctx_cls, sel(c"currentContext"));
        if cur.is_null() {
            return;
        }
        let port = msg0(cur, sel(c"graphicsPort"));
        if port.is_null() {
            return;
        }
        extern "C" {
            fn CGContextSetRGBFillColor(c: *mut core::ffi::c_void, r: f64, g: f64, b: f64, a: f64);
            fn CGContextFillRect(c: *mut core::ffi::c_void, r: NSRect);
            fn CGContextSetRGBStrokeColor(
                c: *mut core::ffi::c_void,
                r: f64,
                g: f64,
                b: f64,
                a: f64,
            );
            fn CGContextSetLineWidth(c: *mut core::ffi::c_void, w: f64);
            fn CGContextStrokeRect(c: *mut core::ffi::c_void, r: NSRect);
        }
        let ctx = port;
        // 背景
        CGContextSetRGBFillColor(ctx, 0.94, 0.95, 0.98, 1.0);
        CGContextFillRect(
            ctx,
            NSRect {
                x: 0.0,
                y: 0.0,
                w: LAY_VW,
                h: LAY_VH,
            },
        );
        // グリッド(Deskflow 風の薄い方眼。1px 矩形で描く)
        CGContextSetRGBFillColor(ctx, 0.88, 0.89, 0.93, 1.0);
        let mut gx = 40.0;
        while gx < LAY_VW {
            CGContextFillRect(
                ctx,
                NSRect {
                    x: gx,
                    y: 0.0,
                    w: 1.0,
                    h: LAY_VH,
                },
            );
            gx += 40.0;
        }
        let mut gy = 40.0;
        while gy < LAY_VH {
            CGContextFillRect(
                ctx,
                NSRect {
                    x: 0.0,
                    y: gy,
                    w: LAY_VW,
                    h: 1.0,
                },
            );
            gy += 40.0;
        }
        // 配置セル(Deskflow 式): Mac を中央に周囲 8 マス。ドラッグ中は
        // ドロップ候補のセルを薄く塗って、置き場所が一目で分かるようにする
        {
            let m = lay_mac_rect();
            let drag = LAY_DRAG.load(Ordering::Relaxed);
            let hover = if drag {
                let idx = LAY_DRAG_IDX.load(Ordering::Relaxed);
                let act = *crate::ACTIVE_PEER.lock().unwrap_or_else(|e| e.into_inner());
                let p = if idx == usize::MAX || idx == act {
                    lay_win_center()
                } else {
                    *LAY_DRAG_CENTER.lock().unwrap_or_else(|e| e.into_inner())
                };
                lay_cell_for_point(&m, p)
            } else {
                None
            };
            for s in 0u8..8 {
                let r = lay_cell_rect(&m, s);
                if Some(s) == hover {
                    CGContextSetRGBFillColor(ctx, 0.36, 0.47, 0.88, 0.14);
                    CGContextFillRect(ctx, r);
                }
                CGContextSetRGBStrokeColor(ctx, 0.80, 0.82, 0.90, 1.0);
                CGContextSetLineWidth(ctx, 1.0);
                CGContextStrokeRect(ctx, r);
            }
        }
        // Mac(灰+白枠): 全モニターを実配置のまま描く。内蔵(メイン)は濃い灰で区別
        let sc = lay_scale();
        let (macs, _) = mac_group();
        let (mox, moy) = lay_mac_origin();
        let (main_w, main_h) = {
            let g = crate::geo();
            (g.main_w, g.main_h)
        };
        let mac_names: Vec<String> = crate::mac_displays().into_iter().map(|d| d.name).collect();
        for (i, (x, y, w, h)) in macs.iter().enumerate() {
            let r = NSRect {
                x: mox + x * sc,
                y: moy + y * sc,
                w: w * sc,
                h: h * sc,
            };
            let is_main = (w - main_w).abs() < 1.0 && (h - main_h).abs() < 1.0;
            if is_main {
                CGContextSetRGBFillColor(ctx, 0.42, 0.46, 0.56, 1.0);
            } else {
                CGContextSetRGBFillColor(ctx, 0.62, 0.66, 0.74, 1.0);
            }
            CGContextFillRect(ctx, r);
            CGContextSetRGBStrokeColor(ctx, 1.0, 1.0, 1.0, 0.9);
            CGContextSetLineWidth(ctx, 1.5);
            CGContextStrokeRect(ctx, r);
            let label = match mac_names.get(i).filter(|n| !n.is_empty()) {
                Some(n) => n.clone(),
                None => format!("モニター{}", i + 1),
            };
            lay_text(&lay_fit(&label, r.w, 10.0), r.x + r.w / 2.0, r.y + r.h / 2.0 - 6.0, 10.0, is_main);
        }
        // 接続先: 全端末のモニター群を実配置のまま描く。アクティブな端末は青、
        // それ以外は薄い青。どの端末もドラッグで配置を変えられ、名前を下に表示する
        let peers = lay_peers();
        // ドラッグ中の端末は最前面に描く
        let mut order: Vec<&LayPeer> = peers.iter().filter(|p| !p.dragging).collect();
        order.extend(peers.iter().filter(|p| p.dragging));
        for lp in order {
            let (cx, cy) = lp.center;
            let (sw, sh) = (lp.size.0 * sc, lp.size.1 * sc);
            let (ox, oy) = (cx - sw / 2.0, cy - sh / 2.0);
            if lp.active || lp.dragging {
                CGContextSetRGBFillColor(ctx, 0.32, 0.38, 0.82, 1.0);
            } else {
                CGContextSetRGBFillColor(ctx, 0.62, 0.70, 0.90, 1.0);
            }
            for (i, (x, y, w, h)) in lp.rects.iter().enumerate() {
                let r = NSRect {
                    x: ox + x * sc,
                    y: oy + y * sc,
                    w: w * sc,
                    h: h * sc,
                };
                CGContextFillRect(ctx, r);
                let label = match lp.names.get(i).filter(|n| !n.is_empty()) {
                    Some(n) => n.clone(),
                    None => format!("モニター{}", i + 1),
                };
                lay_text(
                    &lay_fit(&label, r.w, 10.0),
                    r.x + r.w / 2.0,
                    r.y + r.h / 2.0 - 6.0,
                    10.0,
                    lp.active || lp.dragging,
                );
            }
            // 端末名(Deskflow 風に矩形の下へ)
            lay_text(&lp.name, cx, (oy - 15.0).max(2.0), 11.0, false);
        }
        // Mac 側の名前(実機のホスト名。配置は固定)
        {
            let m = lay_mac_rect();
            lay_text(
                &crate::hostname_label(),
                m.x + m.w / 2.0,
                (m.y - 15.0).max(2.0),
                11.0,
                false,
            );
        }
    }
}

unsafe extern "C" fn lay_down(_self: ID, _cmd: SEL, ev: ID) {
    unsafe {
        let p = lay_point_in_view(_self, ev);
        // 全端末を対象に、押した位置にある矩形を探してドラッグを開始する
        let sc = lay_scale();
        let blocks: Vec<LayPeer> = lay_peers();
        let mut hit: Option<(usize, (f64, f64))> = None;
        for lp in &blocks {
            let (cx, cy) = lp.center;
            let (sw, sh) = (lp.size.0 * sc, lp.size.1 * sc);
            // 当たり判定は描画矩形より少し広く取る(縮尺後のブロックは小さいため)
            let inside = p.x >= cx - sw / 2.0 - 12.0
                && p.x <= cx + sw / 2.0 + 12.0
                && p.y >= cy - sh / 2.0 - 12.0
                && p.y <= cy + sh / 2.0 + 12.0;
            if inside {
                hit = Some((lp.idx, (cx, cy)));
                break;
            }
        }
        eprintln!(
            "[lay] down ({:.0},{:.0}) hit={:?} blocks={}",
            p.x,
            p.y,
            hit.map(|(i, _)| i),
            blocks
                .iter()
                .map(|lp| format!(
                    "{}@({:.0},{:.0})",
                    lp.name, lp.center.0, lp.center.1
                ))
                .collect::<Vec<_>>()
                .join(" ")
        );
        if let Some((idx, (cx, cy))) = hit {
            let act = *crate::ACTIVE_PEER.lock().unwrap_or_else(|e| e.into_inner());
            if idx == usize::MAX || idx == act {
                // アクティブ端末(と未接続プレースホルダ)は LAY_WIN を自由位置に使う
                *LAY_WIN.lock().unwrap_or_else(|e| e.into_inner()) = (cx, cy);
            } else {
                *LAY_DRAG_CENTER.lock().unwrap_or_else(|e| e.into_inner()) = (cx, cy);
            }
            LAY_DRAG_IDX.store(idx, Ordering::Relaxed);
            LAY_DRAG.store(true, Ordering::Relaxed);
            *LAY_GRAB.lock().unwrap_or_else(|e| e.into_inner()) = (p.x - cx, p.y - cy);
        }
    }
}

unsafe extern "C" fn lay_dragged(_self: ID, _cmd: SEL, ev: ID) {
    unsafe {
        if !LAY_DRAG.load(Ordering::Relaxed) {
            return;
        }
        let p = lay_point_in_view(_self, ev);
        let g = *LAY_GRAB.lock().unwrap_or_else(|e| e.into_inner());
        let idx = LAY_DRAG_IDX.load(Ordering::Relaxed);
        let act = *crate::ACTIVE_PEER.lock().unwrap_or_else(|e| e.into_inner());
        let sc = lay_scale();
        let (ww, wh) = lay_peers()
            .iter()
            .find(|lp| lp.idx == idx)
            .map(|lp| (lp.size.0 * sc, lp.size.1 * sc))
            .unwrap_or_else(lay_win_size);
        // 端末がキャンバスより大きくても clamp が panic しないよう下限で丸める
        let minx = ww / 2.0 + 2.0;
        let maxx = (LAY_VW - ww / 2.0 - 2.0).max(minx);
        let miny = wh / 2.0 + 2.0;
        let maxy = (LAY_VH - wh / 2.0 - 2.0).max(miny);
        let nx = (p.x - g.0).clamp(minx, maxx);
        let ny = (p.y - g.1).clamp(miny, maxy);
        if idx == usize::MAX || idx == act {
            *LAY_WIN.lock().unwrap_or_else(|e| e.into_inner()) = (nx, ny);
        } else {
            *LAY_DRAG_CENTER.lock().unwrap_or_else(|e| e.into_inner()) = (nx, ny);
        }
        let snd: unsafe extern "C" fn(ID, SEL, u8) =
            std::mem::transmute(crate::objc_msgSend as *const () as usize);
        snd(_self, sel(c"setNeedsDisplay:"), 1);
    }
}

/// ドロップ: ドラッグ中の端末について、最も近い Mac モニターとその辺を
/// 「画面の位置」として設定し、表示を計算位置へスナップし直す
unsafe extern "C" fn lay_up(_self: ID, _cmd: SEL, _ev: ID) {
    unsafe {
        if !LAY_DRAG.swap(false, Ordering::Relaxed) {
            return;
        }
        let idx = LAY_DRAG_IDX.load(Ordering::Relaxed);
        LAY_DRAG_IDX.store(usize::MAX, Ordering::Relaxed);
        // ドロップ位置(いま操作している端末の中心)から、最も近い Mac モニターと
        // その辺を決めて、その端末の「画面の位置」として設定する
        let sc = lay_scale();
        let (rects, _) = mac_group();
        let (mox, moy) = lay_mac_origin();
        let act = *crate::ACTIVE_PEER.lock().unwrap_or_else(|e| e.into_inner());
        let active_drag = idx == usize::MAX || idx == act;
        let (wc0, _ww, _wh) = if active_drag {
            (lay_win_center(), lay_win_size().0, lay_win_size().1)
        } else {
            let (sw, sh) = lay_peers()
                .iter()
                .find(|lp| lp.idx == idx)
                .map(|lp| (lp.size.0 * sc, lp.size.1 * sc))
                .unwrap_or((60.0, 40.0));
            (
                *LAY_DRAG_CENTER.lock().unwrap_or_else(|e| e.into_inner()),
                sw,
                sh,
            )
        };
        // ドロップ位置から最寄りの Mac モニター(edge 指定)と、8 方向セルの
        // side(Deskflow 式。斜めも置ける)を決める
        let mac_rect = lay_mac_rect();
        let mut best: Option<(usize, f64)> = None;
        for (mi, (x, y, w, h)) in rects.iter().enumerate() {
            let (mx, my, mw, mh) = (mox + x * sc, moy + y * sc, w * sc, h * sc);
            let (mcx, mcy) = (mx + mw / 2.0, my + mh / 2.0);
            let (dx, dy) = (wc0.0 - mcx, wc0.1 - mcy);
            let dist = (dx * dx + dy * dy).sqrt();
            if best.as_ref().map(|(_, d)| dist < *d).unwrap_or(true) {
                best = Some((mi, dist));
            }
        }
        if let Some((mi, _)) = best {
            let target = if active_drag { act } else { idx };
            let side = lay_cell_for_point(&mac_rect, wc0).unwrap_or(0);
            if target != usize::MAX {
                crate::set_peer_side(target, side, Some(mi));
                eprintln!(
                    "[lay] 配置を更新: モニター{} の{}",
                    mi + 1,
                    ["右", "左", "上", "下", "右上", "右下", "左上", "左下"][side.min(7) as usize]
                );
            } else {
                // 未接続(プレースホルダを動かした)。全体設定 SIDE へ反映し、
                // 次に繋がる端末の位置になる。ここで反映しないとドロップが
                // 常に捨てられ「動かせない」ように見える
                crate::set_side(side);
                eprintln!(
                    "[lay] 配置を更新(未接続のため全体設定へ): {}",
                    ["右", "左", "上", "下", "右上", "右下", "左上", "左下"][side.min(7) as usize]
                );
            }
        }
        // 表示位置は保存せず、設定(side/モニター)から計算し直した位置へスナップする
        *LAY_WIN.lock().unwrap_or_else(|e| e.into_inner()) = (0.0, 0.0);
        *LAY_DRAG_CENTER.lock().unwrap_or_else(|e| e.into_inner()) = (0.0, 0.0);
        let snd: unsafe extern "C" fn(ID, SEL, u8) =
            std::mem::transmute(crate::objc_msgSend as *const () as usize);
        snd(_self, sel(c"setNeedsDisplay:"), 1);
        // 接続範囲(斜め=辺の半分)は set_peer_side が選択端末分を設定済み。
        // ここで全域へ戻すと斜め配置の「辺の半分だけ接続」が効かなくなるため上書きしない
    }
    preferences::save();
}
/// 配置エディタの NSView サブクラスを登録して生成(初回のみ)
unsafe fn make_layout_view(target_frame_host: ID) -> ID {
    unsafe {
        let _ = target_frame_host;
        static CLS: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
        let cls = *CLS.get_or_init(|| {
            let super_cls = objc_getClass(c"NSView".as_ptr());
            if super_cls.is_null() {
                return 0usize;
            }
            let cls = objc_allocateClassPair(super_cls, c"TSLayView".as_ptr(), 0);
            if cls.is_null() {
                return 0usize;
            }
            let ok1 = class_addMethod(
                cls,
                sel(c"drawRect:"),
                lay_draw as *const () as usize,
                c"v@:{CGRect={CGPoint=dd}{CGSize=dd}}".as_ptr(),
            );
            let ok2 = class_addMethod(
                cls,
                sel(c"mouseDown:"),
                lay_down as *const () as usize,
                c"v@:@".as_ptr(),
            );
            let ok3 = class_addMethod(
                cls,
                sel(c"mouseDragged:"),
                lay_dragged as *const () as usize,
                c"v@:@".as_ptr(),
            );
            let ok4 = class_addMethod(
                cls,
                sel(c"mouseUp:"),
                lay_up as *const () as usize,
                c"v@:@".as_ptr(),
            );
            if ok1 == 0 || ok2 == 0 || ok3 == 0 || ok4 == 0 {
                return 0usize;
            }
            objc_registerClassPair(cls);
            cls as usize
        });
        if cls == 0 {
            return std::ptr::null_mut();
        }
        let init: unsafe extern "C" fn(ID, SEL, NSRect) -> ID =
            std::mem::transmute(crate::objc_msgSend as *const () as usize);
        init(
            msg0(cls as ID, sel(c"alloc")),
            sel(c"initWithFrame:"),
            NSRect {
                x: 0.0,
                y: 0.0,
                w: LAY_VW,
                h: LAY_VH,
            },
        )
    }
}

/// OSネイティブの4分類設定画面を構築する。
unsafe fn make_prefs_window(target: ID) -> ID {
    prefs::build(target)
}

unsafe extern "C" fn imp_quit(_s: ID, _c: SEL, _n: ID) {
    eprintln!("[gui] メニューから終了しました");
    // Windows 画面を操作したまま終了すると、カーソルの関連切断・非表示が
    // システムへ残る。正規の leave(カーソル復帰+Windows 側の後片付け)を
    // 経由してから終了する
    if crate::WIN_MODE.swap(false, Ordering::Relaxed) {
        crate::leave_win_mode_cursor_unlock(None);
    }
    std::process::exit(0);
}
unsafe extern "C" fn imp_check_update(_s: ID, _c: SEL, _n: ID) {
    if UI_PREVIEW.load(Ordering::Relaxed) {
        return;
    }
    crate::updater::on_click();
    refresh_status();
}
unsafe extern "C" fn imp_update(_s: ID, _c: SEL, _n: ID) {
    // --show-prefs: NSApp.run 開始後のタイマーコンテキストで開く
    // (run 前のウィンドウ操作は NSException で abort するため遅延させる)
    if SHOW_AT_START.swap(false, Ordering::Relaxed) {
        show_prefs();
        // 検証用: KNIT_SHOW_LAYOUT=1 で配置ウィンドウも同時オープン
        if !UI_PREVIEW.load(Ordering::Relaxed)
            && crate::envutil::get("KNIT_SHOW_LAYOUT").as_deref() == Some("1")
        {
            show_layout();
        }
    }
    refresh_status();
}

/// 状態表示の更新(メニューバーのボタンタイトル + メニュー内の動的項目)。
/// NSTimer から毎秒呼ばれる。メニュー開閉中も止まらないよう common modes で登録
// ===== 接続の診断(メニュー「接続を診断…」)=====

/// 診断の結果受け渡し。imp_diag がスレッドで実行して完了したら入れる。
/// NSAlert はメインスレッド必須のため、refresh_status(毎秒)が拾って表示する。
/// "__RUNNING__" は実行中のマーカー
static DIAG_RESULT: Mutex<Option<String>> = Mutex::new(None);

unsafe extern "C" fn imp_diag(_s: ID, _c: SEL, _n: ID) {
    {
        let mut g = DIAG_RESULT.lock().unwrap_or_else(|e| e.into_inner());
        if g.is_some() {
            return; // 実行中か表示待ち
        }
        *g = Some("__RUNNING__".into());
    }
    std::thread::spawn(|| {
        let text = crate::diag::run();
        *DIAG_RESULT.lock().unwrap_or_else(|e| e.into_inner()) = Some(text);
    });
}

/// 診断結果を NSAlert で出す(メインスレッドからのみ呼ぶ)
unsafe fn show_diag_alert(text: &str) {
    let alert = msg0(objc_getClass(c"NSAlert".as_ptr()), sel(c"new"));
    if alert.is_null() {
        return;
    }
    msg1_void_id(alert, sel(c"setMessageText:"), nsstring("接続の診断"));
    msg1_void_id(alert, sel(c"setInformativeText:"), nsstring(text));
    let _ = msg0(alert, sel(c"runModal"));
}

fn refresh_status() {
    unsafe {
        // 診断の完了を拾って表示する(NSAlert はここ(メインスレッド)で出す)
        {
            let mut g = DIAG_RESULT.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(text) = g.take() {
                if text == "__RUNNING__" {
                    *g = Some(text);
                } else {
                    show_diag_alert(&text);
                }
            }
        }
        let button = GUI_BUTTON.load(Ordering::Relaxed) as ID;
        if button.is_null() {
            return;
        }
        let connected = crate::CONNECTED.load(Ordering::Relaxed);
        // 転送中は進捗を常時表示(どちらの画面を見ていても分かる)。
        // 未接続の時だけ文字を出し、接続中は常時アイコンのみ
        //(どちらの画面を見ているかは操作の結果で分かる。ユーザー指示 459/494)
        let title = if let Some(pct) = crate::xfer_title() {
            pct
        } else if !connected {
            "未接続".to_string()
        } else {
            String::new()
        };
        msg1_void_id(button, sel(c"setTitle:"), nsstring(&title));

        let upd = GUI_UPDATE_ITEM.load(Ordering::Relaxed) as ID;
        if !upd.is_null() {
            msg1_void_id(upd, sel(c"setTitle:"), nsstring(&crate::updater::menu_title()));
        }
        // 転送の中止項目(進行中だけ押せる。Esc でも中止できる)
        let xfer_item = GUI_XFER_ITEM.load(Ordering::Relaxed) as ID;
        if !xfer_item.is_null() {
            if let Some(line) = crate::xfer_line() {
                msg1_void_id(
                    xfer_item,
                    sel(c"setTitle:"),
                    nsstring(&format!("転送を中止する({line}・Esc でも可)")),
                );
                msg1_void_u8(xfer_item, sel(c"setEnabled:"), 1);
            } else {
                msg1_void_id(xfer_item, sel(c"setTitle:"), nsstring("転送: なし"));
                msg1_void_u8(xfer_item, sel(c"setEnabled:"), 0);
            }
        }
        let state = GUI_STATE_ITEM.load(Ordering::Relaxed) as ID;
        if !state.is_null() {
            // 接続状態は「未接続 / 接続済み」の2語に統一する(設定画面・Windows 側と
            // 同じ表記)。まだ一度も登録していない人には再接続の案内を出さない
            let conn = if connected {
                "接続済み".to_string()
            } else if !crate::PAIRED.load(Ordering::Relaxed) {
                "未接続(設定の「端末を登録…」から相手と登録できます)".to_string()
            } else {
                "未接続(自動で再接続します・相手側アプリの起動を確認)".to_string()
            };
            let mode = if connected { "利用できます" } else { "このMacは操作できます" };
            let rtt = crate::RTT_MS.load(Ordering::Relaxed);
            let rtt_s = if connected && rtt > 0 {
                format!("・遅延 {rtt}ms")
            } else {
                String::new()
            };
            // 経路(LAN 直 / Tailscale)も出す: 中継へ落ちていないかの常時確認用
            let route = crate::route_label();
            let route_s = if connected && !route.is_empty() {
                format!("・{route}")
            } else {
                String::new()
            };
            let history = crate::HISTORY
                .lock()
                .map(|h| h.entries().len())
                .unwrap_or(0);
            // メニューに並ぶのは最近の 10 件まで(全件の保存上限は履歴 50 件)
            let history_s = if history > 0 {
                format!("・履歴{history}件(メニューは最近の10件)")
            } else {
                String::new()
            };
            // 転送の進捗と最終接続時刻(再接続の目安)
            let xfer_s = match crate::xfer_line() {
                Some(x) => format!("・{x}"),
                None => String::new(),
            };
            let last_s = if !connected {
                crate::last_connected_line()
                    .map(|l| format!("・{l}"))
                    .unwrap_or_default()
            } else {
                String::new()
            };
            // 未接続が続くときの見える化(Windows 側のステータス窓と同じ):
            // クライアントモードの再試行までの残り秒と、1 分を超えた「見つけられない」表示
            let retry_s = crate::next_retry_line()
                .map(|l| format!("・{l}"))
                .unwrap_or_default();
            let missing_s = crate::not_found_line()
                .map(|l| format!("・{l}"))
                .unwrap_or_default();
            let text = format!("{conn} ・ {mode}{rtt_s}{route_s}{history_s}{xfer_s}{last_s}{retry_s}{missing_s}");
            msg1_void_id(state, sel(c"setTitle:"), nsstring(&text));
        }
        // 設定ウィンドウが開いていればチェック状態も保ち直す
        sync_prefs_state();

        // クリップボード履歴メニュー(push があったときだけ作り直す。
        // 開いているメニューのちらつきを避けるため変化検知する)
        let history_menu = GUI_HISTORY_MENU.load(Ordering::Relaxed) as ID;
        if !history_menu.is_null() {
            let last = crate::HISTORY_LAST_ID.load(Ordering::Relaxed);
            if last != GUI_HISTORY_SEEN.load(Ordering::Relaxed) {
                GUI_HISTORY_SEEN.store(last, Ordering::Relaxed);
                rebuild_history_menu(history_menu);
            }
        }

    }
}

/// 接続先メニューの選択: representedObject の端末 id でピアを探してアクティブへ
unsafe extern "C" fn imp_peer_activate(_s: ID, _c: SEL, sender: ID) {
    if sender.is_null() {
        return;
    }
    let obj = msg0(sender, sel(c"representedObject"));
    if obj.is_null() {
        return;
    }
    let utf8 = crate::msg0_cstr(obj, sel(c"UTF8String"));
    if utf8.is_null() {
        return;
    }
    let id = std::ffi::CStr::from_ptr(utf8)
        .to_string_lossy()
        .into_owned();
    crate::activate_peer_by_id(&id, "接続先メニュー");
    refresh_status();
}

/// 履歴サブメニューの項目を作り直す(新しい順 10 件+「消す」)。
/// 本文は representedObject に載せ、クリックで sdHistoryRestore: へ渡す
unsafe fn rebuild_history_menu(menu: ID) {
    msg0(menu, sel(c"removeAllItems"));
    let target = GUI_TARGET.load(Ordering::Relaxed) as ID;
    let count = crate::HISTORY
        .lock()
        .map(|h| h.entries().len())
        .unwrap_or(0);
    // 見出しに件数を出す(メニューバー本体の項目タイトルも更新)
    let holder = GUI_HISTORY_ITEM.load(Ordering::Relaxed) as ID;
    if !holder.is_null() {
        let t = if count > 0 {
            format!("クリップボード履歴({count}件)")
        } else {
            "クリップボード履歴".to_string()
        };
        msg1_void_id(holder, sel(c"setTitle:"), nsstring(&t));
    }
    let items = crate::HISTORY.lock().ok().map(|h| {
        let now = knit_common::history::now_epoch_ms();
        h.recent(knit_common::history::MENU_ITEMS)
            .into_iter()
            .map(|e| (knit_common::history::label(e, now, 34), e.text.clone()))
            .collect::<Vec<_>>()
    });
    let items = items.unwrap_or_default();
    if items.is_empty() {
        let item = menu_item("履歴はまだありません(コピーすると記録されます)", None, "");
        msg1_void_u8(item, sel(c"setEnabled:"), 0);
        add_item(menu, item);
        return;
    }
    for (title, text) in items {
        let item = menu_item(&title, Some(c"sdHistoryRestore:"), "");
        if item.is_null() {
            continue;
        }
        msg1_void_id(item, sel(c"setTarget:"), target);
        msg1_void_id(item, sel(c"setRepresentedObject:"), crate::nsstring(&text));
        add_item(menu, item);
    }
    let sep = msg0(objc_getClass(c"NSMenuItem".as_ptr()), sel(c"separatorItem"));
    add_item(menu, sep);
    // 平文保存の常時通知(利用者が気づけるように履歴がある間は常に表示する)
    let note = menu_item("※履歴は平文で保存されています(残したくない場合は「履歴を消す」)", None, "");
    if !note.is_null() {
        msg1_void_u8(note, sel(c"setEnabled:"), 0);
        add_item(menu, note);
    }
    let clear = menu_item("履歴を消す", Some(c"sdHistoryClear:"), "");
    if !clear.is_null() {
        msg1_void_id(clear, sel(c"setTarget:"), target);
        add_item(menu, clear);
    }
}

/// メニューバー用テンプレートアイコン(アプリアイコンと同じリボン)。
/// CoreGraphics で 44px ビットマップに描き NSImage(template) 化する。
/// template なのでメニューバーの明暗に自動追従する(色ではなくアルファで描画)
unsafe fn make_menu_icon() -> ID {
    const C: usize = 44;
    let mut data = vec![0u8; C * 4 * C];
    let space = CGColorSpaceCreateDeviceRGB();
    let ctx = CGBitmapContextCreate(
        data.as_mut_ptr(),
        C,
        C,
        8,
        C * 4,
        space,
        2 | (2 << 12), // BGRA
    );
    if ctx.is_null() {
        return std::ptr::null_mut();
    }
    extern "C" {
        fn CGContextAddCurveToPoint(c: ID, x1: f64, y1: f64, x2: f64, y2: f64, x: f64, y: f64);
    }
    CGContextSetRGBStrokeColor(ctx, 0.0, 0.0, 0.0, 1.0);
    CGContextSetLineWidth(ctx, 4.0);
    CGContextSetLineCap(ctx, 1);
    CGContextBeginPath(ctx);
    CGContextMoveToPoint(ctx, 20.0, 19.0);
    CGContextAddCurveToPoint(ctx, 11.0, 8.0, 4.0, 13.0, 8.0, 21.0);
    CGContextAddCurveToPoint(ctx, 10.0, 25.0, 14.0, 28.0, 19.0, 29.0);
    CGContextStrokePath(ctx);
    CGContextBeginPath(ctx);
    CGContextMoveToPoint(ctx, 16.0, 14.0);
    CGContextAddCurveToPoint(ctx, 23.0, 22.0, 27.0, 36.0, 34.0, 29.0);
    CGContextAddCurveToPoint(ctx, 40.0, 22.0, 29.0, 16.0, 24.0, 15.0);
    CGContextStrokePath(ctx);

    let img = CGBitmapContextCreateImage(ctx);
    CGContextRelease(ctx);
    CFRelease(space);
    if img.is_null() {
        return std::ptr::null_mut();
    }
    // NSImage initWithCGImage:size: (NSSize は arm64 で d0/d1 レジスタ渡し)
    let f: unsafe extern "C" fn(ID, SEL, ID, f64, f64) -> ID =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    let nsimg = f(
        msg0(objc_getClass(c"NSImage".as_ptr()), sel(c"alloc")),
        sel(c"initWithCGImage:size:"),
        img,
        18.0,
        18.0,
    );
    CGImageRelease(img);
    if nsimg.is_null() {
        return std::ptr::null_mut();
    }
    msg1_void_u8(nsimg, sel(c"setTemplate:"), 1);
    nsimg
}

unsafe fn make_target() -> ID {
    let super_cls = objc_getClass(c"NSObject".as_ptr());
    if super_cls.is_null() {
        return std::ptr::null_mut();
    }
    let cls = objc_allocateClassPair(super_cls, c"SDMenuTarget".as_ptr(), 0);
    if cls.is_null() {
        return std::ptr::null_mut();
    }
    //IMP は (self, _cmd, sender) の v@:@ 型
    let types = c"v@:@".as_ptr();
    let methods: &[(&std::ffi::CStr, usize)] = &[
        (c"sdPinch:", prefs::pinch_toggle as *const () as usize),
        (c"sdTabletNav:", prefs::navigation_toggle as *const () as usize),
        (c"sdHotkey:", prefs::hotkey as *const () as usize),
        (c"sdNavigate:", prefs::navigate as *const () as usize),
        (
            c"sdSwitchMethod:",
            prefs::switch_method as *const () as usize,
        ),
        (c"sdReturnMac:", prefs::return_mac as *const () as usize),
        (c"sdEnterPeer:", prefs::enter_peer as *const () as usize),
        (c"sdSelectPeer:", prefs::select_peer as *const () as usize),
        (c"menuWillOpen:", prefs::peer_menu_open as *const () as usize),
        (c"menuDidClose:", prefs::peer_menu_close as *const () as usize),
        (
            c"sdRegistration:",
            setup::show_registration as *const () as usize,
        ),
        (
            c"sdResetRegistration:",
            setup::reset_registration as *const () as usize,
        ),
        (c"sdScrollSpeed:", prefs::scroll_speed as *const () as usize),
        (c"sdSwitchMode:", imp_switch_mode as *const () as usize),
        (c"sdEdgeTaps:", imp_edge_taps as *const () as usize),
        (c"sdOpenLog:", imp_open_log as *const () as usize),
        (
            c"sdHistoryRestore:",
            imp_history_restore as *const () as usize,
        ),
        (c"sdHistoryClear:", imp_history_clear as *const () as usize),
        (c"sdCancelXfer:", imp_cancel_xfer as *const () as usize),
        (c"sdPeerActivate:", imp_peer_activate as *const () as usize),
        (c"sdRestart:", imp_restart as *const () as usize),
        (c"sdCheckUpdate:", imp_check_update as *const () as usize),
        (c"sdDiagnose:", imp_diag as *const () as usize),
        (c"sdAudio:", imp_audio_toggle as *const () as usize),
        (c"sdAudioGain:", prefs::audio_gain as *const () as usize),
        (c"sdCmdMap:", imp_cmd_map as *const () as usize),
        (c"sdScroll:", imp_scroll_flip as *const () as usize),
        (c"sdSpkMute:", imp_spk_mute as *const () as usize),
        (c"sdVol:", imp_vol as *const () as usize),
        (c"sdSendFile:", imp_send_file as *const () as usize),
        (c"sdTabletText:", text_input::show as *const () as usize),
        (c"sdShowPrefs:", imp_show_prefs as *const () as usize),
        (c"sdScrollGain:", imp_scroll_gain as *const () as usize),
        (c"sdSide:", imp_side as *const () as usize),
        (c"sdClipShare:", imp_clip_share as *const () as usize),
        (c"sdFileShare:", imp_file_share as *const () as usize),
        (c"sdLocalHistory:", imp_local_history as *const () as usize),
        (c"sdScrollCompat:", imp_scroll_compat as *const () as usize),
        (c"sdRotateSide:", imp_rotate_side as *const () as usize),
        (c"sdLayout:", imp_show_layout as *const () as usize),
        (c"sdMouseScale:", imp_mouse_scale as *const () as usize),
        (c"sdQuit:", imp_quit as *const () as usize),
        (c"sdHostRole:", imp_host_role as *const () as usize),
        (c"sdRole:", imp_role as *const () as usize),
        (c"updateStatus:", imp_update as *const () as usize),
        (
            c"pollIncomingDrag:",
            direct_input::poll as *const () as usize,
        ),
    ];
    for (name, imp) in methods {
        if class_addMethod(cls, sel(name), *imp, types) == 0 {
            return std::ptr::null_mut();
        }
    }
    objc_registerClassPair(cls);
    msg0(cls, sel(c"new"))
}

unsafe fn menu_item(title: &str, action: Option<&std::ffi::CStr>, key: &str) -> ID {
    let t = nsstring(title);
    let k = nsstring(key);
    let a = action.map(|x| sel(x)).unwrap_or(std::ptr::null_mut());
    // alloc + initWithTitle:action:keyEquivalent:(インスタンス初期化子)。
    // NSMenuItem には itemWithTitle:/menuItemWithTitle: クラスファクトリが無い
    // (未認識セレクタで NS例外→abort になるため使わない)
    let alloc = msg0(objc_getClass(c"NSMenuItem".as_ptr()), sel(c"alloc"));
    if alloc.is_null() {
        return std::ptr::null_mut();
    }
    msg3_id(alloc, sel(c"initWithTitle:action:keyEquivalent:"), t, a, k)
}

unsafe fn add_item(menu: ID, item: ID) {
    msg1_void_id(menu, sel(c"addItem:"), item);
}

/// メニューバー常駐を構築する。失敗(ssh 由来等 AppKit 不可)は false を返し、
/// 呼び出し側は CUI(CFRunLoop)へフォールバックする
pub fn start() -> bool {
    unsafe {
        // NSApplication は最初に取得しておく(AppKit の初期化順序の慣例)
        let app = msg0(
            objc_getClass(c"NSApplication".as_ptr()),
            sel(c"sharedApplication"),
        );
        if app.is_null() {
            eprintln!("[gui] NSApplication を取得できません(CUI モードで継続)");
            return false;
        }
        let target = make_target();
        if target.is_null() {
            eprintln!("[gui] メニューターゲットクラスの登録に失敗(CUI モードで継続)");
            return false;
        }
        GUI_TARGET.store(target as usize, Ordering::Relaxed);

        let menu = msg0(objc_getClass(c"NSMenu".as_ptr()), sel(c"new"));
        if menu.is_null() {
            eprintln!("[gui] NSMenu 生成に失敗(CUI モードで継続)");
            return false;
        }
        // 自動有効化を切る(action 無しの状態行をクリック可能に見せないため)
        msg1_void_u8(menu, sel(c"setAutoenablesItems:"), 0);

        // 状態行(選択不可・refresh_status で毎秒更新)
        let state = menu_item("状態: …", None, "");
        if state.is_null() {
            return false;
        }
        msg1_void_u8(state, sel(c"setEnabled:"), 0);
        GUI_STATE_ITEM.store(state as usize, Ordering::Relaxed);
        add_item(menu, state);

        // ファイル転送の中止(進行中だけ有効化。タイトルは refresh_status が更新)
        let xfer = menu_item("転送: なし", Some(c"sdCancelXfer:"), "");
        if xfer.is_null() {
            return false;
        }
        msg1_void_id(xfer, sel(c"setTarget:"), target);
        msg1_void_sel(xfer, sel(c"setAction:"), sel(c"sdCancelXfer:"));
        msg1_void_u8(xfer, sel(c"setEnabled:"), 0);
        GUI_XFER_ITEM.store(xfer as usize, Ordering::Relaxed);
        add_item(menu, xfer);

        // ファイルを送る(NSOpenPanel。ドラッグや ⌘C 同期とは別の、
        // 明示的な送信入口。Android 中継時は Download への置き換えにも使う)
        let sendf = menu_item("ファイルを送る…", Some(c"sdSendFile:"), "");
        if !sendf.is_null() {
            msg1_void_id(sendf, sel(c"setTarget:"), target);
            msg1_void_sel(sendf, sel(c"setAction:"), sel(c"sdSendFile:"));
            add_item(menu, sendf);
        }

        add_item(
            menu,
            msg0(objc_getClass(c"NSMenuItem".as_ptr()), sel(c"separatorItem")),
        );

        let prefs = menu_item("設定…", Some(c"sdShowPrefs:"), ",");
        if prefs.is_null() {
            return false;
        }
        msg1_void_id(prefs, sel(c"setTarget:"), target);
        msg1_void_sel(prefs, sel(c"setAction:"), sel(c"sdShowPrefs:"));
        add_item(menu, prefs);

        // 接続の診断(繋がらない原因を順に確かめて教える)。
        // 実測は diag.rs、判定と対処の文言は common::diagnose
        let diagm = menu_item("接続を診断…", Some(c"sdDiagnose:"), "");
        if !diagm.is_null() {
            msg1_void_id(diagm, sel(c"setTarget:"), target);
            msg1_void_sel(diagm, sel(c"setAction:"), sel(c"sdDiagnose:"));
            add_item(menu, diagm);
        }

        // クリップボード履歴(送信・受信したテキストから選んで復元)。
        // 項目は refresh_status が履歴の変化だけ検知して作り直す
        let history_menu = msg0(objc_getClass(c"NSMenu".as_ptr()), sel(c"new"));
        if !history_menu.is_null() {
            msg1_void_u8(history_menu, sel(c"setAutoenablesItems:"), 0);
            let holder = menu_item("クリップボード履歴", None, "");
            if !holder.is_null() {
                msg1_void_id(holder, sel(c"setSubmenu:"), history_menu);
                add_item(menu, holder);
                GUI_HISTORY_ITEM.store(holder as usize, Ordering::Relaxed);
                GUI_HISTORY_MENU.store(history_menu as usize, Ordering::Relaxed);
            }
        }


        // 接続の方向(この Mac が待ち受けるか/Windows ホストへ接続しに行くか)。
        // 表示は refresh_status も更新する
        let role = menu_item(&role_menu_title(), Some(c"sdHostRole:"), "");
        if !role.is_null() {
            msg1_void_id(role, sel(c"setTarget:"), target);
            msg1_void_sel(role, sel(c"setAction:"), sel(c"sdHostRole:"));
            add_item(menu, role);
            GUI_ROLE_ITEM.store(role as usize, Ordering::Relaxed);
        }
        add_item(
            menu,
            msg0(objc_getClass(c"NSMenuItem".as_ptr()), sel(c"separatorItem")),
        );
        let upd = menu_item(&crate::updater::menu_title(), Some(c"sdCheckUpdate:"), "");
        if !upd.is_null() {
            msg1_void_id(upd, sel(c"setTarget:"), target);
            msg1_void_sel(upd, sel(c"setAction:"), sel(c"sdCheckUpdate:"));
            add_item(menu, upd);
            GUI_UPDATE_ITEM.store(upd as usize, Ordering::Relaxed);
        }
        let quit = menu_item("Knit を終了", Some(c"sdQuit:"), "q");
        if quit.is_null() {
            return false;
        }
        msg1_void_id(quit, sel(c"setTarget:"), target);
        msg1_void_sel(quit, sel(c"setAction:"), sel(c"sdQuit:"));
        add_item(menu, quit);

        // ステータスバー項目(可変幅)。button のタイトルで状態を常時表示する
        let sb = msg0(
            objc_getClass(c"NSStatusBar".as_ptr()),
            sel(c"systemStatusBar"),
        );
        if sb.is_null() {
            return false;
        }
        let item = msg1_id_f64(sb, sel(c"statusItemWithLength:"), -1.0); // NSVariableStatusItemLength
        if item.is_null() {
            return false;
        }
        let button = msg0(item, sel(c"button"));
        if button.is_null() {
            return false;
        }
        GUI_BUTTON.store(button as usize, Ordering::Relaxed);
        // アイコン(テンプレート)+状態テキストの併記。失敗時はテキストのみで継続
        let icon = make_menu_icon();
        if !icon.is_null() {
            msg1_void_id(button, sel(c"setImage:"), icon);
        }
        msg1_void_id(item, sel(c"setMenu:"), menu);

        // 毎秒の状態反映。メニュー追跡中も止まらないよう common modes へ登録する
        let timer = msg5_timer(
            objc_getClass(c"NSTimer".as_ptr()),
            sel(c"scheduledTimerWithTimeInterval:target:selector:userInfo:repeats:"),
            1.0,
            target,
            sel(c"updateStatus:"),
            std::ptr::null_mut(),
            1,
        );
        if !timer.is_null() {
            let rl = msg0(objc_getClass(c"NSRunLoop".as_ptr()), sel(c"mainRunLoop"));
            // メニュー追跡(NSEventTrackingRunLoopMode)中も更新が続くよう common modes へ
            msg2_void_id_id(
                rl,
                sel(c"addTimer:forMode:"),
                timer,
                kCFRunLoopCommonModes as ID,
            );
        }
        msg5_timer(
            objc_getClass(c"NSTimer".as_ptr()),
            sel(c"scheduledTimerWithTimeInterval:target:selector:userInfo:repeats:"),
            0.016,
            target,
            sel(c"pollIncomingDrag:"),
            std::ptr::null_mut(),
            1,
        );
        refresh_status();
        true
    }
}

/// アプリケーション実行(NSApp.run)。戻らない。終了はメニューの「終了」
pub unsafe fn run_app() {
    let app = msg0(
        objc_getClass(c"NSApplication".as_ptr()),
        sel(c"sharedApplication"),
    );
    if app.is_null() {
        return;
    }
    // Accessory ポリシー = Dock アイコン非表示(メニューバー常駐型の標準)
    msg1_void_i64(app, sel(c"setActivationPolicy:"), 1);
    msg0_void(app, sel(c"run"));
}

#[cfg(test)]
mod restart_tests {
    use super::restart_command;
    use std::path::Path;

    #[test]
    fn plain_binary_is_exec_with_quoted_args() {
        let c = restart_command(Path::new("/tmp/my dir/knit-mac"), &["--host".into(), "a'b".into()]);
        assert_eq!(c, "sleep 1; exec '/tmp/my dir/knit-mac' '--host' 'a'\\''b'");
    }

    #[test]
    fn app_bundle_is_opened_as_a_bundle() {
        let c = restart_command(Path::new("/Applications/Knit.app/Contents/MacOS/knit-mac"), &[]);
        assert_eq!(c, "sleep 1; /usr/bin/open -n '/Applications/Knit.app'");
    }
}

#[cfg(test)]
mod role_ack_tests {
    // ROLE_ACK は単一の static のため、テストは直列で1本にまとめる
    use super::{note_role_ack, wait_role_ack};
    use std::time::Duration;

    #[test]
    fn ack_is_taken_once_and_absent_without_note() {
        note_role_ack();
        // 届いた Ack は即返り、1 回の待ちで消費される(take パターン)
        assert!(wait_role_ack(Duration::from_millis(0)));
        // 消費済みなので、次の待ち(旧版相手相当)は false
        assert!(!wait_role_ack(Duration::from_millis(30)));
    }
}

#[cfg(test)]
mod lay_side_center_tests {
    use super::{lay_side_center, NSRect};

    fn base() -> NSRect {
        NSRect { x: 0.0, y: 0.0, w: 100.0, h: 100.0 }
    }

    #[test]
    fn eight_sides_get_distinct_positions() {
        let b = base();
        let pos = |s: u8| lay_side_center(s, &b, 40.0, 40.0);
        // 4 方向の基本位置
        assert_eq!(pos(0), (128.0, 50.0)); // 右
        assert_eq!(pos(1), (-28.0, 50.0)); // 左
        assert_eq!(pos(2), (50.0, 128.0)); // 上
        assert_eq!(pos(3), (50.0, -28.0)); // 下
        // 斜めは基の辺の上下半分の中心(ここが右扱いだと「斜めに置けない」回帰)
        assert_eq!(pos(4), (128.0, 75.0)); // 右上
        assert_eq!(pos(5), (128.0, 25.0)); // 右下
        assert_eq!(pos(6), (-28.0, 75.0)); // 左上
        assert_eq!(pos(7), (-28.0, 25.0)); // 左下
    }

    #[test]
    fn every_side_is_visibly_distinct() {
        let b = base();
        let mut seen: Vec<(f64, f64)> = Vec::new();
        for s in 0u8..8 {
            let p = lay_side_center(s, &b, 40.0, 40.0);
            assert!(
                seen.iter().all(|q| (q.0 - p.0).abs() > 1.0 || (q.1 - p.1).abs() > 1.0),
                "side {s} が他と同じ位置: {p:?}"
            );
            seen.push(p);
        }
    }
}

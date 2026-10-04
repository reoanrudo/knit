use crate::audio;
use crate::clipboard::{
    clipboard_seq, clipboard_write_text, sync_clipboard_to_mac, CLIP_MAX_CHARS, CLIP_OPEN_ERR,
    CLIP_SHARE_W, FILES_SHARE_W, history_push, LAST_RECV_CLIP, LAST_SYNC_SEQ,
};
use crate::dragdrop;
use crate::input::{
    ALT_TAB_ACTIVE, BTN_W, CMD_ALT, LockWorkStation, ModState, game_like, ime_set_open,
    ime_set_open_impl, inject_key, inject_scroll, is_extended_vk, refresh_vscreen,
    remote_mouse_button, remote_mouse_move_abs, remote_mouse_move_rel, vscreen,
};
use crate::state::{DEBUG_KEYS, log_safe, MAIN_SHUTDOWN, RTT_MS, SIDE_W, SPK_MUTE_MODE, WTX};
use crate::tray;
use crate::xfer::{RX_BYTES, RX_DRAG};
use knit_common::keymap::mac_kc_to_win_vk;
use knit_common::proto::{decode, encode, Msg};
use knit_common::secure;
use std::io::{BufRead, BufReader, Read, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::POINT;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::VK_MENU;
/// 既定ブラウザで URL を開く(Continue Here)。ShellExecuteW の "open" 動詞は
/// 拡張子/スキームの関連付けに従うため、ブラウザ選びは OS の既定に任せる
/// 越境 App Handoff(ビジョン§12)の照合に使う Windows 側のアプリ候補。
/// AppsReply を返したときの列挙を保持し、RunApp はこのパスと完全一致だけ許す
pub static WIN_APPS: std::sync::Mutex<Vec<(String, String)>> = std::sync::Mutex::new(Vec::new());

/// スタートメニューのショートカット(.lnk)を(表示名, パス)で列挙する。
/// アンインストーラー等は除外し、上限 200 件(名前順・重複名は除去)
pub fn list_win_apps() -> Vec<(String, String)> {
    let mut roots = Vec::new();
    if let Some(pd) = std::env::var_os("ProgramData") {
        roots.push(std::path::PathBuf::from(pd).join(r"Microsoft\Windows\Start Menu\Programs"));
    }
    if let Some(ad) = std::env::var_os("APPDATA") {
        roots.push(std::path::PathBuf::from(ad).join(r"Microsoft\Windows\Start Menu\Programs"));
    }
    let mut out = Vec::new();
    for root in roots {
        if !root.is_dir() {
            continue;
        }
        let mut stack = vec![root];
        while let Some(dir) = stack.pop() {
            let Ok(rd) = std::fs::read_dir(&dir) else {
                continue;
            };
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else if p
                    .extension()
                    .and_then(|x| x.to_str())
                    .map(|x| x.eq_ignore_ascii_case("lnk"))
                    .unwrap_or(false)
                {
                    let name = p
                        .file_stem()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    if !name.is_empty() && !name.to_lowercase().contains("uninstall") {
                        out.push((name, p.to_string_lossy().into_owned()));
                    }
                }
                if out.len() >= 200 {
                    break;
                }
            }
            if out.len() >= 200 {
                break;
            }
        }
    }
    out.sort_by_key(|a| a.0.to_lowercase());
    out.dedup_by(|a, b| a.0.eq_ignore_ascii_case(&b.0));
    out
}

/// パス(.lnk/.exe 等)をシェルの既定動作で起動する
pub fn launch_path(path: &str) -> bool {
    #[link(name = "shell32")]
    unsafe extern "system" {
        fn ShellExecuteW(
            hwnd: *mut core::ffi::c_void,
            verb: *const u16,
            file: *const u16,
            params: *const u16,
            dir: *const u16,
            show: i32,
        ) -> isize;
    }
    let file: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
    let verb: Vec<u16> = "open\0".encode_utf16().collect();
    unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            file.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            1,
        ) > 32
    }
}

fn open_default_browser(url: &str) -> bool {
    #[link(name = "shell32")]
    unsafe extern "system" {
        fn ShellExecuteW(
            hwnd: *mut core::ffi::c_void,
            verb: *const u16,
            file: *const u16,
            params: *const u16,
            dir: *const u16,
            show: i32,
        ) -> isize;
    }
    let verb: Vec<u16> = "open\0".encode_utf16().collect();
    let file: Vec<u16> = url.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            file.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            1,
        ) > 32
    }
}

use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;

/// 認証済みストリームの本体処理(接続/待受 両モード共通)
pub(crate) fn session(
    mut reader: BufReader<secure::Reader>,
    mut writer: secure::Writer,
) -> std::io::Result<()> {
    // 読み出しタイムアウト: Mac は 1 秒毎に ping を送るため 9 秒間無音は経路断。
    // タイムアウトで read がエラーを返し、再接続ループへ制御が戻る(半開対策)
    reader
        .get_ref()
        .set_read_timeout(Some(Duration::from_secs(9)));
    // 送信の単一ライタ化: 受信ループとクリップ監視スレッドが同一ソケットへ並行
    // write すると行が混線し、Mac 側 decode で黙って捨てられる(pong 欠損→偽切断)。
    // Mac 側と同じ mpsc+単一スレッド構成へ集約する(レビュー Wave1 X2/P0-3)
    let (wtx, wrx) = std::sync::mpsc::channel::<String>();
    *WTX.lock().unwrap_or_else(|e| e.into_inner()) = Some(wtx.clone());
    std::thread::spawn(move || {
        while let Ok(line) = wrx.recv() {
            if line == MAIN_SHUTDOWN {
                // 経路昇格などで外から本線を張り直す時の合図(read 側も err で終了)
                writer.shutdown();
                break;
            }
            if writer
                .write_all(line.as_bytes())
                .and_then(|_| writer.flush())
                .is_err()
            {
                writer.shutdown();
                break;
            }
        }
    });
    let mut mods = ModState::new();
    // マウス移動のサブピクセル残高。Mac のトラックパッドは 1px 未満の delta が
    // 連続するため、毎回 round すると遅い移動が消えてカクカクする。整数部のみ注入し
    // 端数は次イベントへ持ち越す。
    let mut accum = (0.0f64, 0.0f64);
    let mut last_return_notify = Instant::now() - Duration::from_secs(10);
    let running = Arc::new(AtomicBool::new(true));
    let running_w = running.clone();

    // (旧heartbeatスレッドは削除: ソケット生死は read のエラーで判定し、
    //  接続監視は Mac 側の ping/pong が担うため不要だった)

    // Windows 側からも 3 秒毎に ping を送る(Mac の再起動・スリープで経路が死んだ時、
    // 書き込み失敗として早く気づく。受信側は 9 秒=3 回分の無通信で切断扱い)
    {
        let tx = wtx.clone();
        let running = running.clone();
        std::thread::spawn(move || {
            while running.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_secs(3));
                if tx.send(encode(&Msg::Ping { ts: 0 })).is_err() {
                    break;
                }
            }
        });
    }

    // ゲームモード監視(250ms 毎)。隠れカーソルは「入力中にポインタを隠す」設定等でも
    // 起きるため、全画面かつ 1.5 秒継続した時だけ採用する。KNIT_GAME_MODE=0 で無効
    if knit_common::envutil::get("KNIT_GAME_MODE").as_deref() != Some("0") {
        let tx = wtx.clone();
        let running = running.clone();
        std::thread::spawn(move || {
            let mut on = false;
            let mut since: Option<Instant> = None;
            while running.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(250));
                let now = if game_like() {
                    since.get_or_insert_with(Instant::now).elapsed() >= Duration::from_millis(1500)
                } else {
                    since = None;
                    false
                };
                if now != on {
                    on = now;
                    println!(
                        "[game] ゲームモード -> {}",
                        if on {
                            "ON(相対移動)"
                        } else {
                            "OFF(絶対位置)"
                        }
                    );
                    if tx.send(encode(&Msg::Rel { on })).is_err() {
                        break;
                    }
                }
            }
        });
    }

    // 画面構成(解像度・モニター抜き差し)の変化を 2 秒毎に確認し Mac へ知らせる
    // (Mac 側の速度換算と端の判定が古い大きさのままになるのを防ぐ)
    {
        let tx = wtx.clone();
        let running = running.clone();
        std::thread::spawn(move || {
            let mut last = vscreen();
            while running.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_secs(2));
                let now = refresh_vscreen();
                if now != last {
                    println!("[screen] 仮想デスクトップ {:?} -> {:?}", last, now);
                    last = now;
                    if tx
                        .send(encode(&Msg::Screen { w: now.2, h: now.3 }))
                        .is_err()
                    {
                        break;
                    }
                }
            }
        });
    }

    // 接続時点のクリップボードは送らない(以後の変化だけを Mac へ戻る時に同期する)
    LAST_SYNC_SEQ.store(clipboard_seq(), Ordering::Relaxed);

    // 行バッファを使い回す(lines() は毎行 String を新規確保するため、
    // mouse_abs のような高頻度行で無駄なアロケーションになる。mac 側と同じ方式)
    let mut line = String::new();
    loop {
        line.clear();
        // 行長上限: 巨大1行を送る相手に読み切るまでメモリを膨らませない
        // (hello と同じ上限。超過は接続切断=mac と同一方式)
        match (&mut reader).take(crate::conn::MAX_LINE + 1).read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {}
            Err(e) => {
                mods.release_everything();
                running_w.store(false, Ordering::Relaxed);
                return Err(e);
            }
        }
        if line.len() as u64 > crate::conn::MAX_LINE {
            mods.release_everything();
            running_w.store(false, Ordering::Relaxed);
            return Err(std::io::Error::other("line exceeds MAX_LINE"));
        }
        if line.trim().is_empty() {
            continue;
        }
        let msg = match decode(&line) {
            Some(m) => m,
            None => continue,
        };
        match msg {
            // Mac から掴んだまま越える予告。直後の押下を引き継いだ操作として扱う。
            // 合計は受信進捗の表示にも、件数は受信結果の照合にも使う
            Msg::DragOffer { id, count, total, .. } => {
                dragdrop::expect(id, count);
                *RX_DRAG.lock().unwrap_or_else(|e| e.into_inner()) = Some((id, total));
                RX_BYTES.store(0, Ordering::Relaxed);
            }
            Msg::DragAccept { id } => dragdrop::edge::accept(id),
            Msg::DragReady { id } => dragdrop::edge::ready(id),
            Msg::DragCancel { id } | Msg::DragDone { id, .. } => {
                dragdrop::edge::cancel(id);
                // 掴み越境の受信表示は、キャンセル/完了が届いた時点で解く
                // (転送本体が始まる前に打ち切られた場合、Files イベントは来ない)
                let mut rx = RX_DRAG.lock().unwrap_or_else(|e| e.into_inner());
                if rx.is_some_and(|(cur, _)| cur == id) {
                    *rx = None;
                    RX_BYTES.store(0, Ordering::Relaxed);
                }
            }
            Msg::HelloOk {
                name,
                w: mw,
                h: mh,
                ver,
                ..
            } => {
                dragdrop::edge::PEER_VERSION.store(ver, Ordering::Relaxed);
                println!(
                    "[hello] (重複) ok from {} (mac screen {mw}x{mh})",
                    log_safe(&name)
                );
            }
            Msg::Ping { ts } => {
                let _ = wtx.send(encode(&Msg::Pong { ts }));
            }
            Msg::Cfg {
                cmd_alt,
                spk_mute,
                side,
                clip,
                files,
                listen,
            } => {
                CMD_ALT.store(cmd_alt, Ordering::Relaxed);
                CLIP_SHARE_W.store(clip, Ordering::Relaxed);
                // Mac 側のファイル共有・再生意思(版 15 以降)。Mac が受け取らない
                // ものを送らない・聞いていない間は音声を流さないために使う
                FILES_SHARE_W.store(files, Ordering::Relaxed);
                audio::LISTEN_MAC.store(listen, Ordering::Relaxed);
                SIDE_W.store(side.min(7), Ordering::Relaxed);
                println!("[cfg] ⌘キー -> {}", if cmd_alt { "Alt" } else { "Ctrl" });
                if SPK_MUTE_MODE.swap(spk_mute, Ordering::Relaxed) != spk_mute {
                    println!(
                        "[cfg] 接続中スピーカーミュート -> {}",
                        if spk_mute { "ON" } else { "OFF" }
                    );
                    audio::speaker_set_mode(spk_mute, true);
                }
            }
            Msg::Vol { op } => {
                // VK_VOLUME_UP(0xAF)/DOWN(0xAE)/MUTE(0xAD)。up/down は2回送って調整幅を稼ぐ。
                // op 3-5 はメディア制御(前へ/再生切替/次へ)= Mac の F7/F8/F9 転送
                const VK_VOL_UP: u16 = 0xAF;
                const VK_VOL_DOWN: u16 = 0xAE;
                const VK_VOL_MUTE: u16 = 0xAD;
                match op {
                    3..=5 => {
                        // メディア制御(前へ/再生切替/次へ)= Mac の F7/F8/F9 転送
                        if let Some(vk) = knit_common::proto::media_vk(op) {
                            inject_key(vk, false);
                            inject_key(vk, true);
                            println!("[vol] media op={op}");
                        }
                    }
                    0 => {
                        for _ in 0..2 {
                            inject_key(VK_VOL_UP, false);
                            inject_key(VK_VOL_UP, true);
                        }
                        println!("[vol] op={op}");
                    }
                    1 => {
                        for _ in 0..2 {
                            inject_key(VK_VOL_DOWN, false);
                            inject_key(VK_VOL_DOWN, true);
                        }
                        println!("[vol] op={op}");
                    }
                    2 => {
                        inject_key(VK_VOL_MUTE, false);
                        inject_key(VK_VOL_MUTE, true);
                        println!("[vol] op={op}");
                    }
                    // 未知の op は無視(他の操作に化けさせない)
                    _ => {}
                }
            }
            Msg::Role { host } => {
                // 適用済みを Mac へ知らせてから再起動へ移る(Mac はこの確認を待って
                // 再起動する。版 15 以降。旧 Mac は無視するため時間頼みで従来どおり)
                let _ = wtx.send(encode(&Msg::RoleAck));
                crate::tray::apply_peer_role(host);
            }
            Msg::RoleAck => {
                // Mac 側の役割適用完了(この PC が Role を送った側のときに届く)
                crate::tray::note_role_ack();
            }
            Msg::XferAck {
                accepted,
                rejected,
                scope,
            } => {
                // Mac 側の転送受理結果。送信スレッドの通知分岐に使う
                crate::xfer::note_xfer_ack(accepted, rejected, scope);
            }
            Msg::Prefs { json } => {
                if json.len() <= 16 * 1024 {
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&json) {
                        *crate::state::MAC_PREFS.lock().unwrap_or_else(|e| e.into_inner()) = Some(v);
                    }
                }
            }
            Msg::Stat { rtt } => {
                RTT_MS.store(rtt, Ordering::Relaxed);
            }
            Msg::Key {
                kc,
                down,
                ctrl,
                opt,
                cmd,
                shift,
                tr,
                rcmd,
            } => {
                if DEBUG_KEYS.load(Ordering::Relaxed) && down {
                    let ch = knit_common::charmap::mac_kc_to_char(kc);
                    println!("[key] kc={kc} ch={ch:?} mods c={ctrl} o={opt} m={cmd} s={shift}");
                }
                // 右⌘(kc 54)の到着と注入後の実状態を記録(「効かない」報告の切り分け)。
                // state が -32768(押下)なら注入は成功、0 なら競合や失敗
                if kc == 54 {
                    mods.apply(ctrl, opt, cmd, shift, rcmd);
                    let st = unsafe {
                        extern "system" {
                            fn GetAsyncKeyState(v_key: i32) -> i16;
                        }
                        GetAsyncKeyState(0xA3)
                    };
                    println!("[rcmd] arrived kc=54 down={down} rcmd={rcmd} -> win rctrl={st}");
                    if !down {
                        continue;
                    }
                }
                // Mac JIS の 英数(102)/かな(104)キーは Windows 側 IME の開閉に変換する
                // (HIToolbox 実測: kVK_JIS_Eisu=102, kVK_JIS_Kana=104)
                if down {
                    match kc {
                        104 => {
                            ime_set_open(true);
                            if DEBUG_KEYS.load(Ordering::Relaxed) {
                                println!("[ime] kana(kc=104) -> IME on");
                            }
                            continue;
                        }
                        102 => {
                            ime_set_open(false);
                            if DEBUG_KEYS.load(Ordering::Relaxed) {
                                println!("[ime] eisu(kc=102) -> IME off");
                            }
                            continue;
                        }
                        _ => {}
                    }
                }
                // Mac の cmd+Tab(ウィンドウ切替)は Windows の Alt+Tab へ変換する。
                // Alt は cmd が離されるまで保持し、離した瞬間に切替を確定させる。
                // 確定条件から prev_cmd 依存を外した: cmd+Tab down の mods.apply(false,...)
                // が self.cmd 相当を false に落とすため旧条件は恒偽で、VK_MENU up が
                // 誰にも注入されず Alt が押しっぱなしに残留する実績バグだった
                if ALT_TAB_ACTIVE.load(Ordering::Relaxed) && !cmd {
                    // cmd 離下 → Alt+Tab 確定
                    inject_key(0x09, true); // VK_TAB up
                    inject_key(VK_MENU, true);
                    ALT_TAB_ACTIVE.store(false, Ordering::Relaxed);
                    println!("[alttab] confirmed");
                }
                if kc == 49 && cmd && !opt && !ctrl && !tr {
                    // ⌘Space(Spotlight) → Windows キー: スタート/検索が開く。
                    // ⌘分の Ctrl を先に離さないと「Ctrl+Space(入力メソッド切替)」に
                    // なってしまうため、Space 面では Ctrl を上げてからタップする
                    if down {
                        mods.apply(false, opt, false, shift, rcmd);
                        inject_key(0x5B /*VK_LWIN*/, false);
                        inject_key(0x5B, true);
                    }
                    continue; // up も握る(離下の瞬間に Ctrl+Space が成立するのを防ぐ)
                }
                if kc == 48 && cmd && !opt && !ctrl && !tr {
                    if down {
                        // cmd 分の Ctrl 押下を抑制してから Alt+Tab を合成
                        mods.apply(false, opt, false, shift, false);
                        inject_key(VK_MENU, false);
                        inject_key(0x09, false);
                        ALT_TAB_ACTIVE.store(true, Ordering::Relaxed);
                    } else {
                        inject_key(0x09, true); // Tab up のみ(Alt は保持)
                    }
                    if DEBUG_KEYS.load(Ordering::Relaxed) {
                        println!("[alttab] cmd+tab -> alt+tab");
                    }
                    continue;
                }
                mods.apply(ctrl, opt, cmd, shift, rcmd);
                if let Some(vk) = mac_kc_to_win_vk(kc) {
                    // テンキー Enter(76)は通常 Enter と同じ VK で拡張フラグだけが違う
                    let ext = kc == 76 || is_extended_vk(vk);
                    let ok = mods.key(vk, down, ext);
                    if DEBUG_KEYS.load(Ordering::Relaxed) && !ok {
                        println!("[key] INJECT FAILED kc={kc}");
                    }
                }
            }
            Msg::MouseMove { dx, dy } => {
                accum.0 += dx;
                accum.1 += dy;
                // 異常な残高(1e6超)は何かの暴発なので捨てる
                if accum.0.abs() > 1.0e6 || accum.1.abs() > 1.0e6 {
                    accum.0 = 0.0;
                    accum.1 = 0.0;
                }
                let (ix, iy) = (accum.0.trunc(), accum.1.trunc());
                if ix != 0.0 || iy != 0.0 {
                    accum.0 -= ix;
                    accum.1 -= iy;
                    remote_mouse_move_rel(ix as i32, iy as i32);
                    // 実際にカーソルが動いたときだけ左端到達を判定する
                    maybe_notify_return(&wtx, &mut last_return_notify, &mut mods);
                }
            }
            Msg::MouseAbs { nx, ny } => {
                let x = (nx.clamp(0.0, 1.0) * 65535.0).round() as i32;
                let y = (ny.clamp(0.0, 1.0) * 65535.0).round() as i32;
                remote_mouse_move_abs(x, y);
                if DEBUG_KEYS.load(Ordering::Relaxed) {
                    println!("[abs] -> ({x},{y})");
                }
                maybe_notify_return(&wtx, &mut last_return_notify, &mut mods);
            }
            Msg::MouseButton { btn, down } => {
                remote_mouse_button(btn, down);
            }
            Msg::Scroll { dx, dy } => {
                inject_scroll(dx, dy);
            }
            Msg::Lock => {
                println!("[lock] Mac がロックされたため Windows もロックします");
                mods.release_everything();
                unsafe { LockWorkStation() };
            }
            Msg::Leave => {
                dragdrop::edge::CONTROLLED.store(false, Ordering::Relaxed);
                // Mac が制御を取り戻した: 押しっぱなしを残さず、Windows 側で
                // コピーされた内容があれば Mac へ渡す
                dragdrop::relay_cancel();
                mods.release_everything();
                if !dragdrop::edge::committed() {
                    sync_clipboard_to_mac();
                }
            }
            Msg::Warp { nx, ny } => {
                dragdrop::edge::CONTROLLED.store(true, Ordering::Relaxed);
                let (vx, vy, vw, vh) = vscreen();
                let x = vx + (nx.clamp(0.0, 1.0) * vw as f64) as i32;
                let y = vy + (ny.clamp(0.0, 1.0) * vh as f64) as i32;
                crate::input::set_cursor_pos(x, y);
                // Mac からの操作の開始点: テンキーが数字として届くよう
                // NumLock を ON に揃える(Windows を直接使っていた人が OFF にしていても)
                crate::input::sync_numlock_on();
                RETURN_ARMED_W.store(false, Ordering::Relaxed);
                last_return_notify = Instant::now();
            }
            Msg::Clip { text } => {
                if !knit_common::share::allow_clip() {
                    println!("[clip] この端末の設定でクリップボード共有がオフのため受け取りを拒否しました");
                    continue;
                }
                if text.len() > CLIP_MAX_CHARS {
                    // 巨大コピーの連投で通知が洪水にならないよう 60 秒に間引く
                    static BIG_CLIP_NOTIFY: std::sync::Mutex<Option<Instant>> =
                        std::sync::Mutex::new(None);
                    let now = Instant::now();
                    let due = BIG_CLIP_NOTIFY
                        .lock()
                        .map(|mut g| {
                            let ok = g
                                .map(|t| now.duration_since(t) >= Duration::from_secs(60))
                                .unwrap_or(true);
                            if ok {
                                *g = Some(now);
                            }
                            ok
                        })
                        .unwrap_or(false);
                    if due {
                        tray::notify(
                            "Knit",
                            &knit_common::history::too_large_clip_message(&text),
                        );
                    }
                    continue;
                }
                // Mac の LF を Windows の CRLF へ正規化(メモ帳等で貼り付けた時の
                // 行送りの乱れを防ぐ。既に CRLF が混ざる場合は壊さない)
                let text = if text.contains('\r') {
                    text
                } else {
                    text.replace('\n', "\r\n")
                };
                *LAST_RECV_CLIP.lock().unwrap_or_else(|e| e.into_inner()) = Some(text.clone());
                history_push(&text, "Mac");
                // 書き込みは、クリップボードマネージャ等の他プロセスが掴んでいる間は
                // 開けない。掴みが数秒続く実測があるため、ここで待つと受信ループ
                //(マウス・キー)まで止まるため、別スレッドで徐々に間隔を広げて
                // 再試行する(合計 最大4秒)
                std::thread::spawn(move || {
                    let mut ok = false;
                    for wait in [100u64, 200, 400, 700, 1100, 1600] {
                        if clipboard_write_text(&text) {
                            ok = true;
                            break;
                        }
                        std::thread::sleep(Duration::from_millis(wait));
                    }
                    if ok {
                        LAST_SYNC_SEQ.store(clipboard_seq(), Ordering::Relaxed);
                        println!("[clip] mac->win {} bytes", text.len());
                    } else {
                        println!(
                            "[clip] mac->win write failed (busy clipboard, err=0x{:08x})",
                            CLIP_OPEN_ERR.load(Ordering::Relaxed)
                        );
                    }
                });
            }
            Msg::Ime { kana } => {
                // Mac の IME 状態(かな/英数)を画面切替時に反映(IME Follow Cursor)。
                // 方向指定で設定する。IME ウィンドウが取れない窓ではスキップ
                //(トグルフォールバックは反転し続けるため手動キー専用)
                ime_set_open_impl(kana, false);
                println!(
                    "[ime] mac の状態へ同期: {}",
                    if kana { "かな(ON)" } else { "英数(OFF)" }
                );
            }
            Msg::Caps { on } => {
                // Mac の Caps Lock(alphaShift)を画面切替時に反映。Windows の
                // Caps は「押すたびに反転」のため、状態が異なる時だけ 1 回
                // トグル注入する(一致している時に注入すると逆にズれる)
                crate::input::sync_caps_state(on);
            }
            Msg::OpenUrl { url } => {
                // Continue Here: Mac の前面ブラウザの URL を既定ブラウザで開く。
                // 相手から来る文字列のため、検査(common urlx)を通るものだけ開く
                if knit_common::urlx::transferable(&url) {
                    if open_default_browser(&url) {
                        println!("[url] Continue Here: 既定ブラウザで開きました");
                    } else {
                        println!("[url] Continue Here: ShellExecute 失敗");
                    }
                } else {
                    println!("[url] 受け取り拒否(転送できない形式の URL)");
                }
            }
            Msg::AppsQuery => {
                // アプリ一覧の要求(越境 App Handoff の照合に使う): 列挙(数百件の
                // read_dir)で受信ループを塞がないため、応答は別スレッドで行う
                std::thread::spawn(|| {
                    let apps = list_win_apps();
                    *WIN_APPS.lock().unwrap_or_else(|e| e.into_inner()) = apps.clone();
                    if let Some(tx) = WTX.lock().unwrap_or_else(|e| e.into_inner()).clone() {
                        let _ = tx.send(encode(&Msg::AppsReply { apps }));
                        println!("[app] Windows アプリ一覧を返しました");
                    }
                });
            }
            Msg::RunApp { path } => {
                // 相手 PC からの起動指示は「直前に列挙したパスと完全一致」だけ許す
                //(任意パスの実行を防ぐ。入力転送と同じ暗号化経路だが口は狭く保つ)
                let known = {
                    let apps = WIN_APPS.lock().unwrap_or_else(|e| e.into_inner());
                    apps.iter().any(|(_, p)| p == &path)
                };
                if !known {
                    println!(
                        "[app] 拒否: 列挙されていないパスの起動要求({})",
                        log_safe(&path)
                    );
                } else if launch_path(&path) {
                    println!("[app] Windows アプリを起動: {}", log_safe(&path));
                    tray::notify("Knit", &format!("Mac から起動: {}", log_safe(&path)));
                } else {
                    println!("[app] 起動失敗: {}", log_safe(&path));
                }
            }
            Msg::Bye => {
                running_w.store(false, Ordering::Relaxed);
                break;
            }
            _ => {}
        }
    }
    dragdrop::edge::reset();
    mods.release_everything();
    running_w.store(false, Ordering::Relaxed);
    Ok(())
}

/// 復帰の発火判定: カーソルが境界の「そのもの」(仮想画面の端のピクセル)に
/// 達したときだけ戻す。OS はカーソルを仮想画面の端で止めるため、1px 手前で
/// 止まった場合は戻らない=境界を境に戻る
/// side 1/6/7=Mac は左(左上/左下含む)→ Win の右端、2=Mac は上→ Win の下端、
/// 3=Mac は下→ Win の上端、既定=Mac は右(右上/右下含む)→ Win の左端
/// 戻る辺までの距離(px)
fn return_edge_dist(side: u8, px: i32, py: i32, w: i32, h: i32) -> i32 {
    match side {
        1 | 6 | 7 => w - 1 - px,
        2 => h - 1 - py,
        3 => py,
        _ => px,
    }
}

/// 入った直後は戻る辺ちょうどにいる。そこから離れるまでは戻り判定をしない
/// (入ってすぐの小さな動きで Mac に押し戻されるのを防ぐ)
static RETURN_ARMED_W: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

fn return_edge_hit(side: u8, px: i32, py: i32, w: i32, h: i32) -> bool {
    match side {
        1 | 6 | 7 => px >= w - 1,
        2 => py >= h - 1,
        3 => py <= 0,
        _ => px <= 0,
    }
}

/// カーソルが Mac 側の境界(SIDE に応じた端)に達したら Mac へ復帰通知
/// (連打防止 0.7 秒クールダウン)。境界に沿った比率も送り、Mac 側の復帰位置に
/// 反映させる(境界の連続性)。side 0/1=縦比率、2/3=横比率を ny へ載せる
/// ドラッグ中(ボタン押下中)は通知しない: Mac 側の abs-edge 復帰が
/// EVT_MOUSE_MOVED 限定なのと同じ理由で、掴んでいる最中に制御が戻ると
/// release_everything でドラッグが強制キャンセルされてしまう
fn maybe_notify_return(
    wtx: &std::sync::mpsc::Sender<String>,
    last: &mut Instant,
    mods: &mut ModState,
) {
    if BTN_W[0].load(Ordering::Relaxed)
        || BTN_W[1].load(Ordering::Relaxed)
        || BTN_W[2].load(Ordering::Relaxed)
    {
        return;
    }
    let mut p = POINT { x: 0, y: 0 };
    // 保護デスクトップでは GetCursorPos が失敗し座標が (0,0) のまま残る。端と誤認して
    // 制御を Mac へ返してしまうため、取れない間・補助へ中継中は判定しない
    if crate::inputdesk::NEEDS_HELPER.load(Ordering::Relaxed)
        || unsafe { GetCursorPos(&mut p) } == 0
    {
        return;
    }
    let (vx, vy, w, h) = vscreen();
    let (px, py) = (p.x - vx, p.y - vy);
    let side = SIDE_W.load(Ordering::Relaxed);
    if return_edge_dist(side, px, py, w, h) > 24 {
        RETURN_ARMED_W.store(true, Ordering::Relaxed);
    }
    if RETURN_ARMED_W.load(Ordering::Relaxed)
        && return_edge_hit(side, px, py, w, h)
        && last.elapsed() >= Duration::from_millis(700)
    {
        let ny = match side {
            2 | 3 => {
                if w > 0 {
                    (px as f64 / w as f64).clamp(0.0, 1.0)
                } else {
                    0.5
                }
            }
            _ => {
                if h > 0 {
                    (py as f64 / h as f64).clamp(0.0, 1.0)
                } else {
                    0.5
                }
            }
        };
        let _ = wtx.send(encode(&Msg::Return { ny }));
        *last = Instant::now();
        // Mac へ制御を返すための後片付け: 押しっぱなしの修飾キーに加え、
        // (a) ドラッグ中のマウスボタンを離す(選択ドラッグの残留防止)
        // (b) Alt+Tab 変換が未確定なら確定する(スイッチャー残留防止)
        mods.release_everything();
    }
}

#[cfg(test)]
mod return_edge_tests {
    use super::return_edge_hit;

    /// 境界を境に戻す回帰: 端の 1px 手前で止まった場合は戻さない
    /// (旧実装は px<=1 / py<=1 で 1px 手前でも戻っていた)
    #[test]
    fn returns_only_at_the_edge_pixel_itself() {
        // 既定(Mac は右)→ Windows の左端
        assert!(return_edge_hit(0, 0, 400, 1920, 1080), "左端のピクセルで戻る");
        assert!(!return_edge_hit(0, 1, 400, 1920, 1080), "1px 手前では戻らない");
        // Mac は左(左上/左下含む)→ Windows の右端
        assert!(return_edge_hit(1, 1919, 400, 1920, 1080), "右端のピクセルで戻る");
        assert!(!return_edge_hit(1, 1918, 400, 1920, 1080), "1px 手前では戻らない");
        assert!(return_edge_hit(7, 1919, 400, 1920, 1080), "斜め配置も水平辺で同じ");
        // Mac は上 → Windows の下端
        assert!(return_edge_hit(2, 400, 1079, 1920, 1080));
        assert!(!return_edge_hit(2, 400, 1078, 1920, 1080));
        // Mac は下 → Windows の上端
        assert!(return_edge_hit(3, 400, 0, 1920, 1080));
        assert!(!return_edge_hit(3, 400, 1, 1920, 1080), "1px 手前では戻らない");
    }
}

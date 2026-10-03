use crate::*;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

/// Mac 流ショートカットの翻訳対応表。(kc, 修飾) → 翻訳先 (kc, ctrl, opt, cmd, shift)。
/// 翻訳先の修飾は「既定マップ(cmd→Ctrl / opt→Alt / ctrl→Win)」の意味で並べる
/// (CMD_ALT 交換は tap 側の送信時に行う)。対応をテストで固定するため表へ切り出した。
/// fn+F11 は FN フラグを見るためここに含めない
pub(crate) fn mac_shortcut_translation(
    kc: u16,
    ctrl: bool,
    opt: bool,
    cmd: bool,
    shift: bool,
) -> Option<(u16, bool, bool, bool, bool)> {
    if cmd && ctrl && !opt {
        // ⌘Ctrl+Q = 画面ロック(Win+L)
        return (kc == 12).then_some((37, true, false, false, false));
    }
    if cmd && opt && !ctrl {
        // ⌘⌥Esc = タスクマネージャ(Ctrl+Shift+Esc)
        return (kc == 53).then_some((53, false, false, true, true));
    }
    if cmd && !ctrl && !opt {
        // 戻り値: (翻訳先 kc, ctrl, opt, cmd, shift)
        let t = match kc {
            // ⌘←→ = 行頭/行末(Home/End)。Shift は透過(行選択)
            123 => Some((115, false, false, false, shift)),
            124 => Some((119, false, false, false, shift)),
            // ⌘↑↓ = 文書先頭/末尾(Ctrl+Home/End)。Shift 透過
            126 => Some((115, false, false, true, shift)),
            125 => Some((119, false, false, true, shift)),
            // ⌘M/⌘H = 最小化(Win+Down)
            43 | 4 => Some((125, true, false, false, false)),
            // ⌘] / ⌘[ = 次タブ / 前タブ(Ctrl(+Shift)+Tab)。⌘⇧[ も「前タブ」
            30 => Some((48, false, false, true, false)),
            33 => Some((48, false, false, true, true)),
            // ⌘⇧4 / ⌘⇧3 = スクリーンショット(Win+Shift+S)
            21 | 18 if shift => Some((1, true, false, false, true)),
            // ⌘⇧5 = 画面録画(Win+Alt+R)
            23 if shift => Some((15, false, true, false, false)),
            // ⌘Q = ウィンドウを閉じる(Alt+F4)
            12 => Some((118, false, true, false, false)),
            // ⌘G / ⌘⇧G = 次を検索 / 前を検索(F3 / Shift+F3)
            32 => Some((99, false, false, false, shift)),
            // ⌘. = キャンセル → Escape
            47 => Some((53, false, false, false, false)),
            // ⌘Space = IME/言語切替(Win+Space)。翻訳先の修飾は既定マップの意味で
            // 並ぶため ctrl フラグ=true が「Windows キー」を表す(cmd フラグは Win 側
            // で Ctrl になる。Space+ctrl ではない点に注意)
            49 => Some((49, true, false, false, false)),
            _ => None,
        };
        return t;
    }
    if opt && !cmd && !ctrl {
        // ⌥←→ = 単語移動(Ctrl+←→)。Shift は透過(単語選択)
        let t = match kc {
            123 => Some((123, false, false, true, shift)),
            124 => Some((124, false, false, true, shift)),
            _ => None,
        };
        return t;
    }
    None
}

/// ゲームモード(Windows 側がカーソルの閉じ込め等を検知して要求)。true の間は
/// 絶対位置ではなく相対移動で送る(FPS・3D ソフトの視点回転のため)
pub(crate) static GAME_REL: AtomicBool = AtomicBool::new(false);
/// 画面ロックの連動(KNIT_LOCK_SYNC=0 で無効)
pub(crate) static LOCK_SYNC: AtomicBool = AtomicBool::new(true);
/// 右⌘ → Windows の右 Ctrl(KNIT_RCMD_CTRL=0 で無効)。
/// 右⌘をホットキー(KNIT_HOTKEY_KC=54 等)に設定している場合は
/// 先の分岐で握られるため適用されない(競合しない)
pub(crate) static RCMD_CTRL: AtomicBool = AtomicBool::new(true);
/// 右⌘(kc 54)の押下状態。押下中は cmd フラグを rcmd へ置き換えて送る
///(フラグだけだと左右を区別できないため、kc 単位でここで分離する)
pub(crate) static R_RIGHT_CMD: AtomicBool = AtomicBool::new(false);
/// IME 状態同期: Windows へ入る時に Mac のかな/英数を相手の IME 開閉へ反映
///(KNIT_IME_SYNC=0 で無効)
pub(crate) static IME_SYNC: AtomicBool = AtomicBool::new(true);
/// Continue Here: Windows 画面操作中の ⌥⌘T で Mac の前面ブラウザの URL を
/// Windows の既定ブラウザで開く(KNIT_CONTINUE_HERE=0 で無効)
pub(crate) static CONTINUE_HERE: AtomicBool = AtomicBool::new(true);

#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    fn IsSecureEventInputEnabled() -> u8;
    static kTISPropertyInputModeID: CFStringRef;
    fn TISCopyCurrentKeyboardInputSource() -> *mut core::ffi::c_void;
    fn TISGetInputSourceProperty(
        source: *mut core::ffi::c_void,
        key: CFStringRef,
    ) -> *mut core::ffi::c_void;
    fn CFStringGetCString(
        s: *const core::ffi::c_void,
        buf: *mut core::ffi::c_char,
        size: usize,
        encoding: u32,
    ) -> u8;
}

/// 入力モード ID(InputModeID)から Windows の IME 開閉に対応する状態を引く。
/// macOS 標準の日本語入力と Apple 純正キーボードレイアウト(英字配列等・IME が
/// 乗っていないため日本語入力の英数モードと同じ扱い)のみ状態を返す。
/// サードパーティ IME・他言語の入力メソッドの ID は状態を反映しないことがある
/// ため対象外=同期しない(誤って ON/OFF を送らない)。
/// Roman(英数)とレイアウトは OFF、他の日本語系(ひらがな/カタカナ/半角カナ/
/// 全角英数)は ON
pub(crate) fn ime_mode_state(mode: &str) -> Option<bool> {
    if mode.starts_with("com.apple.inputmethod.Japanese") {
        return Some(!mode.ends_with(".Roman"));
    }
    if mode.starts_with("com.apple.keylayout.") {
        return Some(false);
    }
    None
}

/// 現在の入力ソースの入力モード ID。取得失敗(nil・変換失敗)は None
fn current_input_mode_id() -> Option<String> {
    unsafe {
        let src = TISCopyCurrentKeyboardInputSource();
        if src.is_null() {
            return None;
        }
        let v = TISGetInputSourceProperty(src, kTISPropertyInputModeID);
        let mut buf = [0u8; 128];
        let ok = !v.is_null()
            && CFStringGetCString(
                v,
                buf.as_mut_ptr() as *mut core::ffi::c_char,
                buf.len(),
                0x0800_0100, // kCFStringEncodingUTF8
            ) != 0;
        CFRelease(src);
        if !ok {
            return None;
        }
        let n = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
        Some(String::from_utf8_lossy(&buf[..n]).into_owned())
    }
}

/// osascript を 1 本実行し、成功時は stdout を返す。失敗は stderr を
/// ログへ出して(制御文字置換済み)None
fn osascript_output(script: &str) -> Option<String> {
    let out = std::process::Command::new("osascript")
        .arg("-e")
        .arg(script)
        .output()
        .ok()?;
    if out.status.success() {
        return Some(String::from_utf8_lossy(&out.stdout).into_owned());
    }
    let err: String = String::from_utf8_lossy(&out.stderr)
        .chars()
        .map(|c| if c.is_control() { '?' } else { c })
        .collect();
    eprintln!("[url] osascript: {}", err.trim());
    None
}

/// 前面ブラウザの前面タブの URL(Continue Here 用)。
/// 2 段階で実行する: ①System Events で前面アプリ名を取得、②そのアプリ専用の
/// 取得スクリプトだけ実行。1 スクリプトに全ブラウザの tell を並べると、
/// インストールされていないアプリ(Edge 等)の用語解決で構文エラー(-2741)に
/// なる実績があるため、静的な複合は組まない
/// Firefox は URL の AppleScript 対応が無いため対象外
fn frontmost_browser_url() -> Option<String> {
    let front = osascript_output(
        r#"tell application "System Events" to get name of first application process whose frontmost is true"#,
    )?;
    let get_url = match front.trim() {
        "Safari" => r#"tell application "Safari" to get URL of front document"#,
        "Google Chrome" => {
            r#"tell application "Google Chrome" to get URL of active tab of front window"#
        }
        "Microsoft Edge" => {
            r#"tell application "Microsoft Edge" to get URL of active tab of front window"#
        }
        "Brave Browser" => {
            r#"tell application "Brave Browser" to get URL of active tab of front window"#
        }
        _ => return None,
    };
    let url = osascript_output(get_url)?;
    let url = url.trim();
    (!url.is_empty()).then(|| url.to_string())
}

/// Continue Here の本体(⌥⌘T の押下エッジで別スレッドから呼ぶ)。
/// 失敗は TCC の自動化 未承認(初回に Mac 側で許可が必要)でも起きるため、
/// 通知は 60 秒に 1 回に間引いて出す
fn continue_here() {
    static LAST_ERR_MS: AtomicU64 = AtomicU64::new(0);
    match frontmost_browser_url() {
        Some(url) if knit_common::urlx::transferable(&url) => {
            send_msg(&Msg::OpenUrl { url });
            eprintln!("[url] Continue Here: 送信しました");
        }
        Some(_) => eprintln!("[url] Continue Here: 転送できない形式の URL"),
        None => {
            eprintln!("[url] Continue Here: 前面ブラウザの URL を取得できません");
            let now = now_ms();
            if now.saturating_sub(LAST_ERR_MS.swap(now, Ordering::Relaxed)) > 60_000 {
                notify(
                    "Knit",
                    "ブラウザの URL を取得できませんでした(初回は Mac 側で自動化の許可が必要です)",
                );
            }
        }
    }
}

/// Secure Input(パスワード欄等でキー入力の横取りを OS が止める状態)の原因アプリ名。
/// この間はキーボードを Windows へ送れないため、切替時に知らせる(Deskflow と同じ配慮)
fn secure_input_app() -> Option<String> {
    if unsafe { IsSecureEventInputEnabled() } == 0 {
        return None;
    }
    let out = std::process::Command::new("ioreg")
        .args(["-l", "-w", "0", "-d", "1"])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let pid = text
        .split("kCGSSessionSecureInputPID\"=")
        .nth(1)
        .and_then(|s| s.split(|c: char| !c.is_ascii_digit()).next())
        .filter(|s| !s.is_empty());
    let name = pid.and_then(|pid| {
        let o = std::process::Command::new("ps")
            .args(["-p", pid, "-o", "comm="])
            .output()
            .ok()?;
        let n = String::from_utf8_lossy(&o.stdout)
            .trim()
            .rsplit('/')
            .next()?
            .to_string();
        (!n.is_empty()).then_some(n)
    });
    Some(name.unwrap_or_else(|| "不明なアプリ".into()))
}

unsafe extern "C" {
    fn CGSessionCopyCurrentDictionary() -> *const core::ffi::c_void;
    fn CFDictionaryGetValue(
        d: *const core::ffi::c_void,
        key: *const core::ffi::c_void,
    ) -> *const core::ffi::c_void;
    fn CFBooleanGetValue(b: *const core::ffi::c_void) -> u8;
}

/// Mac の画面がロックされているか
pub(crate) fn screen_locked() -> bool {
    unsafe {
        let d = CGSessionCopyCurrentDictionary();
        if d.is_null() {
            return false;
        }
        let key = CFStringCreateWithCString(
            std::ptr::null_mut(),
            c"CGSSessionScreenIsLocked".as_ptr(),
            0x0800_0100,
        );
        let v = if key.is_null() {
            std::ptr::null()
        } else {
            CFDictionaryGetValue(d, key as *const _)
        };
        let locked = !v.is_null() && CFBooleanGetValue(v) != 0;
        if !key.is_null() {
            CFRelease(key as *mut _);
        }
        CFRelease(d as *mut _);
        locked
    }
}
/// 自分(Mac)の全モニターを列挙する(自動認知。CG 座標系のまま相手へ渡す)
pub(crate) fn mac_monitors() -> Vec<knit_common::proto::Monitor> {
    let mut out = Vec::new();
    unsafe {
        let mut ids = [0u32; 16];
        let mut n = 0;
        if CGGetActiveDisplayList(16, ids.as_mut_ptr(), &mut n) == 0 {
            for id in &ids[..(n as usize).min(16)] {
                let b = CGDisplayBounds(*id);
                out.push(knit_common::proto::Monitor {
                    x: b.origin.x as i32,
                    y: b.origin.y as i32,
                    w: b.size.w as i32,
                    h: b.size.h as i32,
                    name: mac_display_name(*id),
                });
            }
        }
    }
    out
}

/// NSScreen の機種名(「LG HDR WFHD」「内蔵 Retina ディスプレイ」など)。取れなければ空
pub(crate) fn mac_display_name(display_id: u32) -> String {
    crate::objc::with_pool(|| unsafe {
        let s = |n: &std::ffi::CStr| sel_registerName(n.as_ptr());
        let screens = msg0(objc_getClass(c"NSScreen".as_ptr()), s(c"screens"));
        if screens.is_null() {
            return String::new();
        }
        let count = msg0_isize(screens, s(c"count")).max(0) as usize;
        let at: unsafe extern "C" fn(ID, SEL, usize) -> ID =
            std::mem::transmute(objc_msgSend as *const () as usize);
        for i in 0..count {
            let scr = at(screens, s(c"objectAtIndex:"), i);
            let desc = msg0(scr, s(c"deviceDescription"));
            if desc.is_null() {
                continue;
            }
            let num = msg1_id(desc, s(c"objectForKey:"), nsstring("NSScreenNumber"));
            if num.is_null() || msg0_isize(num, s(c"unsignedIntValue")) as u32 != display_id {
                continue;
            }
            let p = msg0_cstr(msg0(scr, s(c"localizedName")), s(c"UTF8String"));
            if !p.is_null() {
                return std::ffi::CStr::from_ptr(p).to_string_lossy().into_owned();
            }
        }
        String::new()
    })
}

/// Mac のモニター 1 枚(配置の指定と表示に使う。CG 座標)
#[derive(Clone, Debug)]
pub(crate) struct MacDisplay {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    /// メインディスプレイ(内蔵)か
    pub main: bool,
    pub name: String,
}

/// Mac の全モニター(CG 座標。メイン判定付き)。配列の並びは安定しないため、
/// メインを先頭に並べ替えて返す(端末の「画面の位置」はこの並びの番号で指定する)
pub(crate) fn mac_displays() -> Vec<MacDisplay> {
    let mut out = Vec::new();
    unsafe {
        let main_id = CGMainDisplayID();
        let mut ids = [0u32; 16];
        let mut n = 0;
        if CGGetActiveDisplayList(16, ids.as_mut_ptr(), &mut n) == 0 {
            for id in &ids[..(n as usize).min(16)] {
                let b = CGDisplayBounds(*id);
                out.push(MacDisplay {
                    x: b.origin.x,
                    y: b.origin.y,
                    w: b.size.w,
                    h: b.size.h,
                    main: *id == main_id,
                    name: mac_display_name(*id),
                });
            }
        }
    }
    // メインを先頭へ(番号 0 = メイン。以降は面積の大きい順で安定させる)
    out.sort_by(|a, b| {
        b.main
            .cmp(&a.main)
            .then((b.w * b.h).partial_cmp(&(a.w * a.h)).unwrap_or(std::cmp::Ordering::Equal))
    });
    out
}
/// 押下時の基準値・取得中の世代・ファイル群を同じ状態で管理し、遅い読み出しが
/// 次のクリックに混ざらないようにする。境界切替の許可と切替時の送信に使う
pub(crate) static FILE_DRAG: Mutex<file_drag::FileDrag> = Mutex::new(file_drag::FileDrag::new());
/// 合成 LeftMouseUp(kCGEventSourceUserData=42)に刻む識別マジック。
/// 掴み切替直後の Mac 側ドラッグ完結用投稿であり、Win へ転送してはならない
pub(crate) const SYNTH_UP_MAGIC: i64 = 0x54554e41475550; // 自己投稿イベントを識別する一意値
pub(crate) const FIELD_EVENT_SOURCE_USER_DATA: i32 = 42;
/// 直近の物理左押下の位置。掴みドラッグ越境後の終了 UP を掴み始めの位置で
/// 出すために使う(H3: 端の座標のままだと Finder が端へのドロップと解釈する)
pub(crate) static PRESS_POS: Mutex<Option<CGPoint>> = Mutex::new(None);

pub(crate) unsafe fn make_drag_end_event(pos: CGPoint) -> CGEventRef {
    let event = CGEventCreateMouseEvent(std::ptr::null_mut(), EVT_LEFT_UP, pos, 0);
    if !event.is_null() {
        CGEventSetIntegerValueField(event, FIELD_EVENT_SOURCE_USER_DATA, SYNTH_UP_MAGIC);
    }
    event
}

/// 掴みドラッグ終了 UP の投稿位置。押下開始位置を優先し、無ければライブ位置、
/// 両方無ければ None(投稿側は従来どおり (0,0) へフォールバック)。
/// ライブ位置へのフォールバックは押下を取り逃した異常時のみで、端の座標に
/// 出す従来動作へ諦める経路である
pub(crate) fn drag_end_position(press_pos: Option<CGPoint>, live: Option<CGPoint>) -> Option<CGPoint> {
    press_pos.or(live)
}

/// 辺番号(0-7)を文字列表現に。端末ごとの配置(PeerEntry.side)の表示にも使う
pub fn side_label(side: u8) -> &'static str {
    match side {
        1 => "左",
        2 => "上",
        3 => "下",
        4 => "右上",
        5 => "右下",
        6 => "左上",
        7 => "左下",
        _ => "右",
    }
}

/// 斜め配置(4-7)を「接続する基の辺」(0=右/1=左)へ写す。斜めは水平の辺の
/// 上半分・下半分だけで接続する配置のため、距離・境界の計算は基の辺で行う
pub(crate) fn base_dir(side: u8) -> u8 {
    match side {
        4 | 5 => 0,
        6 | 7 => 1,
        other => other.min(3),
    }
}

/// 端末の side(0-7)が接続する範囲(境界に沿った比率。斜めは半分)
pub(crate) fn side_lay_range(side: u8) -> (f64, f64) {
    match side {
        4 | 6 => (0.0, 0.5),
        5 | 7 => (0.5, 1.0),
        _ => (0.0, 1.0),
    }
}

/// いま操作している相手の画面の辺(0=右/1=左/2=上/3=下)。端末ごとの配置
/// (PEERS の side。斜め 4-7 を含む)を優先し、未接続・未割当は全体設定(SIDE)を使う。
/// 境界方向として使うため斜めは基の辺へ正規化する(表示には side_label を使う)
pub(crate) fn side_dir() -> u8 {
    {
        let peers = PEERS.lock().unwrap_or_else(|e| e.into_inner());
        let act = *ACTIVE_PEER.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(p) = peers.get(act) {
            return base_dir(p.side);
        }
    }
    base_dir(SIDE.load(Ordering::Relaxed))
}

/// 画面の辺 d に配置されている接続先の添字(無ければ None)。斜め配置の端末は
/// 基の辺(右/左)に接続しているものとして数える
pub(crate) fn peer_at_side(d: u8) -> Option<usize> {
    let peers = PEERS.lock().unwrap_or_else(|e| e.into_inner());
    peers.iter().position(|p| base_dir(p.side) == d)
}

/// 端末の対象の辺までの距離。モニター指定(edge_monitor)ならその矩形の辺で
/// 測り、辺の延長線上にいなければ到達扱いにしない(f64::MAX)。
/// 全画面(None)は全体領域の端
pub(crate) fn peer_gap(g: &Geo, edge_monitor: Option<usize>, side: u8, x: f64, y: f64) -> f64 {
    // 斜め配置(4-7)の端末も、基の辺(右/左)までの距離で測る。接続できる
    // 範囲(上半分・下半分)は LAY_RANGE が担う
    let side = base_dir(side);
    let Some(mi) = edge_monitor else {
        return g.gap(side, x, y);
    };
    let Some(d) = g.displays.get(mi) else {
        return g.gap(side, x, y);
    };
    let (mx0, my0, mx1, my1) = (d.x, d.y, d.x + d.w, d.y + d.h);
    match side {
        1 if y >= my0 && y <= my1 => x - mx0,
        0 if y >= my0 && y <= my1 => mx1 - x,
        2 if x >= mx0 && x <= mx1 => y - my0,
        3 if x >= mx0 && x <= mx1 => my1 - y,
        _ => f64::MAX,
    }
}

/// その座標が到達している「端末の辺」を探す: 各端末の対象の辺までの距離を測り、
/// 最短の端末を返す(端末が無ければ全体領域の最短の辺)。
/// 注意: 呼び出し中に PEERS を「二重に」ロックしないこと(side_dir は PEERS を
/// 取るため、PEERS 保持中に呼ぶとデッドロックし、タップが固まって全入力が止まる)
fn find_enter_edge(g: &Geo, x: f64, y: f64) -> (u8, Option<usize>) {
    // 全体領域での最短の辺(端末が無い時の既定。PEERS を取る前に計算する)
    let fallback = {
        let mut best = side_dir();
        let mut best_gap = f64::MAX;
        for d in 0..4u8 {
            let gd = g.gap(d, x, y);
            if gd < best_gap {
                best_gap = gd;
                best = d;
            }
        }
        best
    };
    let peers = PEERS.lock().unwrap_or_else(|e| e.into_inner());
    if peers.is_empty() {
        return (fallback, None);
    }
    let mut best = fallback;
    let mut best_gap = f64::MAX;
    let mut best_idx = None;
    for (i, p) in peers.iter().enumerate() {
        let gd = peer_gap(g, p.edge_monitor, p.side, x, y);
        if gd < best_gap {
            best_gap = gd;
            best = p.side;
            best_idx = Some(i);
        }
    }
    (best, best_idx)
}

/// 端末ごとの画面の位置を設定する(配置エディタのドロップ・メニューから)。
/// side は 0-7(斜め=右上/右下/左上/左下)。on_main=true はメインモニター(内蔵)
/// のその辺、false は全画面の端
pub(crate) fn set_peer_side(index: usize, side: u8, edge_monitor: Option<usize>) {
    let side = side.min(7);
    let name;
    {
        let mut peers = PEERS.lock().unwrap_or_else(|e| e.into_inner());
        let Some(p) = peers.get_mut(index) else {
            return;
        };
        p.side = side;
        p.edge_monitor = edge_monitor;
        name = p.name.clone();
    }
    save_peer_sides();
    let selected = *ACTIVE_PEER.lock().unwrap_or_else(|e| e.into_inner()) == index;
    // 接続範囲(斜めは辺の半分)は境界判定で使う全端末共有の値のため、
    // この端末が選択中のときだけ書き換える(非選択端末の配置で境界が動かないように)
    if selected {
        *LAY_RANGE.lock().unwrap_or_else(|e| e.into_inner()) = side_lay_range(side);
        send_cfg();
    }
    let target = match edge_monitor {
        None => "全画面".to_string(),
        Some(0) => "メインモニター".to_string(),
        Some(i) => format!("モニター{}", i + 1),
    };
    eprintln!(
        "[cfg] {name} の画面の位置 -> {}の{}",
        target,
        side_label(side)
    );
}

/// 端末ごとの画面の位置を保存する(端末 id ごと。値は {side, monitor}。
/// 旧版の数値形式(side + 4*(モニター+1))も読み替える)
fn save_peer_sides() {
    let Some(dir) = knit_common::envutil::config_dir() else {
        return;
    };
    let peers = PEERS.lock().unwrap_or_else(|e| e.into_inner());
    let map: std::collections::BTreeMap<&str, serde_json::Value> = peers
        .iter()
        .map(|p| {
            let v = serde_json::json!({
                "side": p.side,
                "monitor": p.edge_monitor.map(|m| m as i64).unwrap_or(-1),
            });
            (p.id.as_str(), v)
        })
        .collect();
    if let Ok(json) = serde_json::to_string_pretty(&map) {
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(dir.join("peer-sides.json"), json);
    }
}

/// 端末 id に保存された画面の位置(無ければ None)。戻り値は (side, モニター番号)。
/// 旧形式(数値)は side 0-3 + モニター付き、新形式({side, monitor})は 0-7
fn saved_peer_side(id: &str) -> Option<(u8, Option<usize>)> {
    let dir = knit_common::envutil::config_dir()?;
    let text = std::fs::read_to_string(dir.join("peer-sides.json")).ok()?;
    let map: serde_json::Value = serde_json::from_str(&text).ok()?;
    let v = map.get(id)?;
    // 旧形式(数値): side(0-3) + 4*(モニター+1)
    if let Some(n) = v.as_u64() {
        if n > 35 {
            return None;
        }
        let n = n as u8;
        return Some(if n <= 3 {
            (n, None)
        } else {
            (n & 3, Some((n / 4) as usize - 1))
        });
    }
    // 新形式({side, monitor}): side 0-7(斜め含む)。monitor は -1=全画面
    let side = v.get("side")?.as_u64()?;
    if side > 7 {
        return None;
    }
    // monitor は保存側が常に書くが、手書き修正や版混在で欠けていても
    // side だけ生かす(配置がサイレントに既定へ落ちるのを防ぐ)
    let mon = v.get("monitor").and_then(|m| m.as_i64()).unwrap_or(-1);
    Some((
        side as u8,
        if mon < 0 { None } else { Some(mon as usize) },
    ))
}

/// 新しい端末の画面の位置を決める: 保存があればそれ、無ければ全体設定の辺が
/// 空いていればそれ、埋まっていれば空いている辺(左→右→上→下の順)。
/// taken は既存の端末が使っている辺(呼び出し側でロック済みの一覧から渡す)
pub(crate) fn assign_side_for_new_peer(id: &str, taken: &[u8]) -> (u8, Option<usize>) {
    if let Some((s, m)) = saved_peer_side(id) {
        return (s, m);
    }
    // 空きを探すのは基の辺(0-3)。斜め配置の端末もその辺を占有していると数える
    let taken_dirs: Vec<u8> = taken.iter().map(|t| base_dir(*t)).collect();
    let default = base_dir(SIDE.load(Ordering::Relaxed));
    if !taken_dirs.contains(&default) {
        return (default, None);
    }
    for d in [1u8, 0, 2, 3] {
        if !taken_dirs.contains(&d) {
            return (d, None);
        }
    }
    (default, None)
}

/// SIDE を設定し、Windows へ Cfg で同期する
pub fn set_side(v: u8) {
    let v = v.min(7);
    SIDE.store(v, Ordering::Relaxed);
    *LAY_RANGE.lock().unwrap_or_else(|e| e.into_inner()) = side_lay_range(v);
    let changed_peer = {
        let mut peers = PEERS.lock().unwrap_or_else(|e| e.into_inner());
        let active = *ACTIVE_PEER.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(peer) = peers.get_mut(active) {
            peer.side = v;
            true
        } else { false }
    };
    if changed_peer { save_peer_sides(); }
    send_cfg();
    eprintln!("[cfg] 接続先の位置 -> {}", side_label(v));
}
/// 画面構成。ディスプレイの抜き差し・配置変更の通知で作り直す(Deskflow と同じく
/// CGDisplayRegisterReconfigurationCallback を使う。旧実装は起動時に一度だけ確定させ、
/// モニターを抜き差しすると境界がずれたままだった)
#[derive(Clone, Debug)]
pub(crate) struct Geo {
    /// メイン画面の大きさ(絶対位置モードの速度換算・配置エディタ用)
    pub main_w: f64,
    pub main_h: f64,
    /// 全ディスプレイを合わせた領域の端(グローバル座標)
    pub(crate) min_x: f64,
    pub(crate) max_x: f64,
    pub(crate) min_y: f64,
    pub(crate) max_y: f64,
    /// 各辺で Windows に接する出口ディスプレイの、辺に沿った範囲
    /// (左右の辺は y の範囲、上下の辺は x の範囲)
    pub(crate) exit: [(f64, f64); 4],
    /// 各モニター(CG 座標。メインが先頭)。端末ごとの「画面の位置」の参照先
    pub displays: Vec<MacDisplay>,
}

const GEO_DEFAULT: Geo = Geo {
    main_w: 2056.0,
    main_h: 1329.0,
    min_x: 0.0,
    max_x: 2056.0,
    min_y: 0.0,
    max_y: 1329.0,
    exit: [(0.0, 1329.0), (0.0, 1329.0), (0.0, 2056.0), (0.0, 2056.0)],
    displays: Vec::new(),
};

static GEO: Mutex<Option<Geo>> = Mutex::new(None);

pub(crate) fn geo() -> Geo {
    GEO.lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .cloned()
        .unwrap_or_else(|| GEO_DEFAULT.clone())
}

impl Geo {
    /// dir(0=右/1=左/2=上/3=下)の境界までの距離。左右に別モニターがある環境でも
    /// 「全体の端」で測るため、Mac 内のモニター間移動では切り替わらない
    pub(crate) fn gap(&self, dir: u8, x: f64, y: f64) -> f64 {
        match dir {
            1 => x - self.min_x,
            2 => y - self.min_y,
            3 => self.max_y - y,
            _ => self.max_x - x,
        }
    }
    fn exit_span(&self, dir: u8) -> (f64, f64) {
        self.exit[dir.min(3) as usize]
    }
    /// 境界に沿った位置の比率(0..1)。左右の辺は y、上下の辺は x で測る
    pub(crate) fn along_ratio(&self, dir: u8, x: f64, y: f64) -> f64 {
        let (lo, hi) = self.exit_span(dir);
        let v = if dir >= 2 { x } else { y };
        if hi > lo {
            ((v - lo) / (hi - lo)).clamp(0.0, 1.0)
        } else {
            0.5
        }
    }
    /// 比率から境界に沿った座標へ(端から 20px は避ける)
    fn along_pos(&self, dir: u8, r: Option<f64>) -> f64 {
        let (lo, hi) = self.exit_span(dir);
        match r {
            Some(n) => {
                (lo + n.clamp(0.0, 1.0) * (hi - lo)).clamp(lo + 20.0, (hi - 20.0).max(lo + 20.0))
            }
            None => (lo + hi) / 2.0,
        }
    }
    /// 境界から inset だけ内側の、境界に沿った比率 r の点
    pub(crate) fn inside_point(&self, dir: u8, inset: f64, r: Option<f64>) -> (f64, f64) {
        let a = self.along_pos(dir, r);
        match dir {
            1 => (self.min_x + inset, a),
            2 => (a, self.min_y + inset),
            3 => (a, self.max_y - inset),
            _ => (self.max_x - inset, a),
        }
    }
}

fn compute_geo() -> Geo {
    unsafe {
        let mb = CGDisplayBounds(CGMainDisplayID());
        let mut g = Geo {
            main_w: mb.size.w,
            main_h: mb.size.h,
            min_x: mb.origin.x,
            max_x: mb.origin.x + mb.size.w,
            min_y: mb.origin.y,
            max_y: mb.origin.y + mb.size.h,
            exit: [
                (mb.origin.y, mb.origin.y + mb.size.h),
                (mb.origin.y, mb.origin.y + mb.size.h),
                (mb.origin.x, mb.origin.x + mb.size.w),
                (mb.origin.x, mb.origin.x + mb.size.w),
            ],
            displays: mac_displays(),
        };
        let mut ids = [0u32; 16];
        let mut n = 0u32;
        if CGGetActiveDisplayList(16, ids.as_mut_ptr(), &mut n) == 0 {
            for id in &ids[..(n as usize).min(16)] {
                let b = CGDisplayBounds(*id);
                let (l, r, t, btm) = (
                    b.origin.x,
                    b.origin.x + b.size.w,
                    b.origin.y,
                    b.origin.y + b.size.h,
                );
                // 同じ辺を複数のディスプレイが共有する(縦に並べた外部モニター等)場合は
                // 出口の範囲を合算する
                let widen = |e: &mut (f64, f64), lo: f64, hi: f64| *e = (e.0.min(lo), e.1.max(hi));
                if r > g.max_x {
                    g.max_x = r;
                    g.exit[0] = (t, btm);
                } else if r == g.max_x {
                    widen(&mut g.exit[0], t, btm);
                }
                if l < g.min_x {
                    g.min_x = l;
                    g.exit[1] = (t, btm);
                } else if l == g.min_x {
                    widen(&mut g.exit[1], t, btm);
                }
                if t < g.min_y {
                    g.min_y = t;
                    g.exit[2] = (l, r);
                } else if t == g.min_y {
                    widen(&mut g.exit[2], l, r);
                }
                if btm > g.max_y {
                    g.max_y = btm;
                    g.exit[3] = (l, r);
                } else if btm == g.max_y {
                    widen(&mut g.exit[3], l, r);
                }
            }
        }
        g
    }
}

pub(crate) fn refresh_geo() {
    let g = compute_geo();
    *GEO.lock().unwrap_or_else(|e| e.into_inner()) = Some(g.clone());
    eprintln!(
        "[screen] main {}x{} / 全体 x={:.0}..{:.0} y={:.0}..{:.0}",
        g.main_w, g.main_h, g.min_x, g.max_x, g.min_y, g.max_y
    );
}

/// ディスプレイ構成の変更通知(メイン RunLoop 上で呼ばれる)。変更完了時だけ作り直す
pub(crate) unsafe extern "C" fn display_reconfigured(
    _display: u32,
    flags: u32,
    _user: *mut core::ffi::c_void,
) {
    const BEGIN: u32 = 1; // kCGDisplayBeginConfigurationFlag
    if flags & BEGIN == 0 {
        refresh_geo();
    }
}
/// カーソル非表示状態の管理(hide/show の対称性を保証し、復帰時に必ず表示する)
pub(crate) static CURSOR_HIDDEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Windows モード開始: カーソル移動とマウス入力の関連を切断し、
/// Mac カーソルを画面右端の固定位置へ置く(Synergy/Deskflow 方式)
/// Deskflow hideCursor/showCursor 内の「SetsCursorInBackground」プロパティ設定。
/// バックグラウンド接続でもカーソル表示状態を維持し、非表示がランダムに解除されるのを防ぐ
unsafe fn set_cursor_in_background() {
    let key = CFStringCreateWithCString(
        std::ptr::null_mut(),
        c"SetsCursorInBackground".as_ptr(),
        0, // kCFStringEncodingMacRoman
    );
    if !key.is_null() {
        let cid = _CGSDefaultConnection();
        CGSSetConnectionProperty(cid, cid, key, kCFBooleanTrue);
        CFRelease(key);
    }
}

pub(crate) fn enter_win_mode_cursor_lock() {
    gui::direct_input::new_session();
    if let Some((control,_)) = active_android_app_permissions() {
        if !control {
            WIN_MODE.store(false,Ordering::Relaxed);
            // 権限拒否のまま端に触れ続けると、次の移動イベントで即再試行となり
            // 通知とログが連打される。再試行を 1.5 秒間隔に間引く
            EDGE_GUARD_UNTIL_MS.store(now_ms() + 1500, Ordering::Relaxed);
            notify("Knit","タブレットのKnitアプリで画面操作を許可してください。");
            return;
        }
    }
    trackpad::reset_session();
    sync_clipboard_to_win();
    // IME Follow Cursor(ビジョン§7): Mac のかな/英数の状態を Windows 側の
    // IME 開閉へ乗せていく。「画面を移る時だけ同期」の原則どおりここでだけ送る。
    // 対象外の入力ソース(サードパーティ IME 等)は同期しない(その旨をログへ出す)
    if IME_SYNC.load(Ordering::Relaxed) {
        if let Some(mode) = current_input_mode_id() {
            match ime_mode_state(&mode) {
                Some(on) => {
                    send_msg(&Msg::Ime { kana: on });
                    eprintln!(
                        "[ime] Mac の状態を Windows へ同期: {}",
                        if on { "かな(ON)" } else { "英数(OFF)" }
                    );
                }
                None => eprintln!(
                    "[ime] 入力ソース({mode})は同期対象外のため Windows の IME はそのままにします(かな/英数キーで切替できます)"
                ),
            }
        } else {
            eprintln!("[ime] 入力ソースを取得できなかったため同期しません");
        }
    }
    std::thread::spawn(|| {
        static LAST: AtomicU64 = AtomicU64::new(0);
        if let Some(app) = secure_input_app() {
            eprintln!("[secure] Secure Input 有効(原因: {app})。キーボードは Windows へ届きません");
            let now = now_ms();
            if now.saturating_sub(LAST.swap(now, Ordering::Relaxed)) > 60_000 {
                notify(
                    "Knit",
                    &format!("「{app}」がパスワード入力等の保護を有効にしているため、キーボードを Windows へ送れません。そのアプリの入力欄から離れてください"),
                );
            }
        }
    });
    // 持ち越していたスクロール残量を切替時に捨てる(切替直後の意図しないスクロール防止)
    *SCROLL_ACC.lock().unwrap_or_else(|e| e.into_inner()) = (0.0, 0.0);
    // Deskflow leave() 相当: hideCursor(プロパティ付き) → suppression間隔最小化 → 関連切断 → warp固定
    unsafe {
        set_cursor_in_background();
        let d = CGMainDisplayID();
        // hide が多重に積もると show が追いつかずカーソルが消えたままになるため
        // フラグで 1 回だけ隠す
        if !CURSOR_HIDDEN.swap(true, Ordering::Relaxed) {
            CGDisplayHideCursor(d);
        }
        CGSetLocalEventsSuppressionInterval(0.0001);
        CGAssociateMouseAndMouseCursorPosition(false);
        // 関連切断は非同期で効き始めるため、切替直後の漏れ移動が数ピクセル出る。
        // 固定位置を右端内側に warp しておき、以降の漏れは都度巻き戻す(境界の同時移動対策)
        let mut lock_y = 400.0;
        let ev = CGEventCreate(std::ptr::null_mut());
        if !ev.is_null() {
            let loc = CGEventGetLocation(ev);
            lock_y = loc.y;
            CFRelease(ev);
        }
        // 全ディスプレイを合わせた領域の、Windows 側の辺に固定する(メイン画面の端に
        // 固定すると、その先にサブモニターがある環境で隠れカーソルが別画面へ飛ぶ)
        let dir = side_dir();
        let g = geo();
        let lock_x2 = live_cursor()
            .map(|p| p.x)
            .unwrap_or((g.min_x + g.max_x) / 2.0);
        let (lock_x, lock_y) = match dir {
            1 => (g.min_x + 2.0, lock_y),
            2 => (lock_x2, g.min_y + 2.0),
            3 => (lock_x2, g.max_y - 2.0),
            _ => (g.max_x - 2.0, lock_y),
        };
        CGWarpMouseCursorPosition(CGPoint {
            x: lock_x,
            y: lock_y,
        });
        // タップが握った位置を CUR_POS にも反映(積算の起点を正しくする)
        *CUR_POS.lock().unwrap_or_else(|e| e.into_inner()) = (lock_x, lock_y);
        *LOCK_POS.lock().unwrap_or_else(|e| e.into_inner()) = Some((lock_x, lock_y));
        // 切替のきっかけになったスワイプの残り移動が、入り位置から戻り端へ届いて
        // 即復帰するのを防ぐ。移り先で体勢を立て直す時間として全接続先で 1.5 秒の
        // 猶予を置く(実機で Android・Windows とも切替直後の誤戻りが観測された)
        EDGE_GUARD_UNTIL_MS.store(now_ms() + 1500, Ordering::Relaxed);
    }
}

/// Windows モード終了: 関連を復元し、右端の内側へカーソルを戻す。
/// ny は Windows 側カーソルの高さ(0..1)。与えられた場合は同じ高さへ戻す(境界連続性)。
pub(crate) fn leave_win_mode_cursor_unlock(ny: Option<f64>) {
    // 冒頭で WIN_MODE を解除し再突入ガードを置く(WIN_MODE の解除を呼び出し元任せに
    // すると、解除を忘れた経路でイベントタップが入力を握り続ける。ガードを最初に
    // 置くことで、復帰処理の途中で再切替が割り込む競合も塞ぐ。既に解除済みの
    // 呼び出し元からの呼び出しでも冪等)
    WIN_MODE.store(false, Ordering::Relaxed);
    EDGE_GUARD_UNTIL_MS.store(now_ms() + 400, Ordering::Relaxed);
    gui::direct_input::new_session();
    trackpad::reset_session();
    // Windows が自力で検知できない離脱(ホットキー/Mac 内完結の左端/ウォッチドッグ)でも
    // 押下中のキー・ボタンが Windows に残らないよう、必ず後片付けを依頼する
    if CONNECTED.load(Ordering::Relaxed) {
        send_msg(&Msg::Leave);
    }
    // Deskflow enter() 相当: 関連復元 → showCursor(プロパティ付き) → suppression解除 → 位置復帰
    unsafe {
        // 前回位置の記憶は廃止: 入り方向も越えた境界の対応位置から入るため、
        // 覚えて戻す位置がなくなった
        // 時間ガードを 400ms に増強し、復帰直後の再突入を防ぐ(距離ガードの代替)
        EDGE_GUARD_UNTIL_MS.store(now_ms() + 400, Ordering::Relaxed);
        // 注意: ここでライブ位置を同期すると「復帰ワープ前」の境界位置
        // (2309)を掴んでしまい、ガード明けに即再突入する原因になる。
        // CUR_POS はこの後のワープ先で上書きするため、ここでは同期しない

        *LOCK_POS.lock().unwrap_or_else(|e| e.into_inner()) = None;
        CGAssociateMouseAndMouseCursorPosition(true);
        set_cursor_in_background();
        let d = CGMainDisplayID();
        // カーソルの再表示漏れ(透明のまま戻るバグ)を防ぐため、フラグが立って
        // いるときは show を複数回呼んで確実に表示する(呼び過ぎても無害)
        if CURSOR_HIDDEN.swap(false, Ordering::Relaxed) {
            for _ in 0..3 {
                CGDisplayShowCursor(d);
            }
        }
        CGSetLocalEventsSuppressionInterval(0.0); // Deskflow setZeroSuppressionInterval
                                                  // 復帰位置: ダブルタップ切替が有効な間は出た境界のすぐ内側(60px)へ戻す。
                                                  // 1回の到達では切替しなくなったため境界近くでも再突入せず、境界を
                                                  // 跨いで戻ってくる連続的な体験になる。
                                                  // 1回切替(KNIT_EDGE_TAPS=1)では従来どおり MacBook 側へ退けて
                                                  // 誤再突入を防ぐ
                                                  // SIDE(Windows の位置)に応じた復帰座標: 出てきた境界のすぐ内側へ。
                                                  // ny は Windows 側カーソルの「境界に沿った比率」(side 0/1=縦、2/3=横)
        let g = geo();
        let dir = side_dir();
        // 端末がモニター指定なら、復帰もそのモニターの辺の内側へ
        // (全画面指定なら全体領域の辺=従来どおり)
        let edge_monitor = {
            let peers = PEERS.lock().unwrap_or_else(|e| e.into_inner());
            let act = *ACTIVE_PEER.lock().unwrap_or_else(|e| e.into_inner());
            peers.get(act).and_then(|p| p.edge_monitor)
        };
        // 出てきた境界の端(1px 内側)へ戻す。再突入は時間ガード(400ms)とダブルタップ判定が防ぐ
        let inset: f64 = 1.0;
        let mon = edge_monitor.and_then(|mi| g.displays.get(mi));
        let (x, y) = if let Some(d) = mon {
            let r = ny.unwrap_or(0.5).clamp(0.0, 1.0);
            let ax = |len: f64| (r * len).clamp(20.0, (len - 20.0).max(20.0));
            match dir {
                1 => (d.x + inset, d.y + ax(d.h)),
                2 => (d.x + ax(d.w), d.y + inset),
                3 => (d.x + ax(d.w), (d.y + d.h - inset).max(d.y + inset)),
                _ => ((d.x + d.w - inset).max(d.x + inset), d.y + ax(d.h)),
            }
        } else {
            g.inside_point(dir, inset, ny)
        };
        CGWarpMouseCursorPosition(CGPoint { x, y });
        // ワープ先を CUR_POS へ反映(同期をワープ前に取ると境界値が残り
        // 復帰直後に必ず再突入して戻れなくなる)
        REENTRY_ARMED.store(false, Ordering::Relaxed);
        *CUR_POS.lock().unwrap_or_else(|e| e.into_inner()) = (x, y);
        eprintln!("[return] -> mac ({x:.0},{y:.0})");
    }
}

/// ホットキー/メニューバーGUI からの手動トグル(F13 とメニューの共通経路)。
/// 切替状態遷移はここを含む既存6経路のまま(一元化はモジュール分割 Phase5 で実施)
pub(crate) fn do_toggle(reason: &str) {
    if !CONNECTED.load(Ordering::Relaxed) {
        return;
    }
    let next = !WIN_MODE.load(Ordering::Relaxed);
    WIN_MODE.store(next, Ordering::Relaxed);
    if next {
        DIAG_MODE_COUNT.fetch_add(1, Ordering::Relaxed);
    }
    eprintln!("[mode] {} ({reason})", if next { "WINDOWS" } else { "MAC" });
    if next {
        enter_win_mode_cursor_lock();
    } else {
        // 戻り先の高さは Windows 側カーソルの現在高さに合わせる
        let ny = {
            let wc = *WIN_CUR.lock().unwrap_or_else(|e| e.into_inner());
            let (_ww, wh) = *WIN_SCREEN.lock().unwrap_or_else(|e| e.into_inner());
            if wh > 0.0 {
                (wc.1 / wh).clamp(0.0, 1.0)
            } else {
                0.5
            }
        };
        leave_win_mode_cursor_unlock(Some(ny));
    }
}

// ---------- イベントタップコールバック ----------
/// NSSystemDefined(Type 14)の内容を読む。CGEvent API に data1/subtype の
/// 取得フィールドが無いため、NSEvent(eventWithCGEvent:) 経由で読む。
/// F 行のメディアキーのときだけ呼ばれるため頻度は低い。subtype 8 以外は None
unsafe fn ns_media_event(event: CGEventRef) -> Option<(i64, i64)> {
    unsafe extern "C" {
        fn objc_retain(id: ID) -> ID;
        fn objc_release(id: ID);
    }
    let cls = objc_getClass(c"NSEvent".as_ptr());
    if cls.is_null() {
        return None;
    }
    let mk: unsafe extern "C" fn(ID, SEL, ID) -> ID =
        std::mem::transmute(objc_msgSend as *const () as usize);
    let get: unsafe extern "C" fn(ID, SEL) -> i64 =
        std::mem::transmute(objc_msgSend as *const () as usize);
    let e = mk(cls, sel_registerName(c"eventWithCGEvent:".as_ptr()), event);
    if e.is_null() {
        return None;
    }
    objc_retain(e);
    let sub = get(e, sel_registerName(c"subtype".as_ptr()));
    let data1 = get(e, sel_registerName(c"data1".as_ptr()));
    objc_release(e);
    if sub != 8 {
        return None; // 8 = NX_SUBTYPE_AUX_CONTROL_BUTTONS(メディアキー)
    }
    Some((sub, data1))
}

// ---------- 権限喪失時の安全な降参 ----------
// 操作中(入力を握っている状態)にアクセシビリティ権限を外されると、タップは死ぬが
// 「カーソルとマウスの関連切断」「カーソル固定」が戻されず、マウスが動かない
// フリーズ状態になる(実機で Mac の再起動を要した)。権限喪失を検知したら関連の
// 復帰だけは試みた上で即座に終了する(exit により macOS が入力を解放する)
#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXIsProcessTrusted() -> bool;
}

pub(crate) fn ax_trusted() -> bool {
    unsafe { AXIsProcessTrusted() }
}

/// 権限喪失時の緊急解放(どこから呼んでも良い。二重呼び出しは無害)
pub(crate) fn surrender_on_permission_loss() -> ! {
    unsafe {
        if WIN_MODE.swap(false, Ordering::Relaxed) {
            // 関連の復帰とカーソルの再表示のみ行う(権限が無いと失敗するが、
            // プロセスの終了で macOS が確実に解放する)
            CGAssociateMouseAndMouseCursorPosition(true);
            let d = CGMainDisplayID();
            for _ in 0..3 {
                CGDisplayShowCursor(d);
            }
            CGSetLocalEventsSuppressionInterval(0.0);
        }
    }
    eprintln!("[fatal] アクセシビリティ権限が外れました。入力を解放して Knit を終了します");
    std::process::exit(0);
}

/// 境界越えの発火判定: 積算カーソルと実カーソルの双方が境界そのもの(距離 0、
/// 許容は丸め誤差の EDGE_BOUNDARY_TOL のみ)に達しているときだけ越える。
/// 境界手前の帯や、高速移動で積算だけが先行している状態では発火しない
pub(crate) fn boundary_crossed(gap_accumulated: f64, gap_live: f64) -> bool {
    gap_accumulated <= 0.0 && gap_live <= EDGE_BOUNDARY_TOL
}

/// 越え先の境界(辺とモニター指定)を確定する。端末のいる世界では、触れた辺に
/// 端末が割り当てられていること(find_enter_edge が返す添字)を境界とする。
/// クライアントモード(単一接続で端末一覧が空)は全体設定の辺を境界とする。
/// どちらも確定できない辺は接続のない壁(越えるとカーソルの隠蔽と固定だけ
/// 起きてマウスが消えるため、呼び出し側は切替自体をしない)
pub(crate) fn boundary_of(
    peers: &[PeerEntry],
    active_side: u8,
    enter_peer: Option<usize>,
) -> Option<(u8, Option<usize>)> {
    if peers.is_empty() {
        return Some((active_side, None));
    }
    // 斜め(4-7)は基の辺(0-3)の一部で接続するため、境界の辺は基の辺へ
    // 正規化して返す(along_ratio_on や到達判定は dir 0-3 前提)
    enter_peer.and_then(|i| peers.get(i).map(|p| (base_dir(p.side), p.edge_monitor)))
}

/// 境界に沿った位置の比率(0..1)。モニター指定の境界はそのモニターの辺で測り、
/// 全体(None)は従来どおり全体領域の出口ディスプレイの辺で測る。モニター指定を
/// 無視して全体で測ると、上段モニターなど他の画面の範囲に正規化されて、
/// 常に端に張り付いた入り位置になる
pub(crate) fn along_ratio_on(g: &Geo, dir: u8, mon: Option<usize>, x: f64, y: f64) -> f64 {
    match mon.and_then(|mi| g.displays.get(mi)) {
        Some(d) => {
            let (lo, hi) = if dir >= 2 {
                (d.x, d.x + d.w)
            } else {
                (d.y, d.y + d.h)
            };
            let v = if dir >= 2 { x } else { y };
            if hi > lo {
                ((v - lo) / (hi - lo)).clamp(0.0, 1.0)
            } else {
                0.5
            }
        }
        None => g.along_ratio(dir, x, y),
    }
}

/// 境界越えの入り位置: 相手側の「境界の辺」そのもの・境界に沿った比率 r。
/// 越えた位置の対応点から動き始めるため、内側へ飛んで見えない(行きも戻りも同じ)。
/// 受け側は座標を画面内へ収めるので 0.0 / 1.0 のまま渡してよい。
/// 入り直後の戻り判定は ENTER_GUARD_UNTIL_MS(600ms)が抑える
pub(crate) fn win_entry_pos(dir: u8, _screen: (f64, f64), r: f64) -> (f64, f64) {
    match dir {
        1 => (1.0, r), // 左へ出る → 相手の右端
        2 => (r, 1.0), // 上へ出る → 相手の下端
        3 => (r, 0.0), // 下へ出る → 相手の上端
        _ => (0.0, r), // 右へ出る → 相手の左端
    }
}

/// abs モードで Mac へ戻る判定: 仮想カーソル(WIN_CUR)は移動のたびに
/// clamp(0.0, 幅-2.0) で境界に止まるため、その止まり値との一致=境界そのものに
/// 達したときだけ戻す。手前の帯では戻さない
/// 仮想カーソルから Mac へ戻る辺までの距離
pub(crate) fn abs_edge_dist(dir: u8, x: f64, y: f64, w: f64, h: f64) -> f64 {
    match dir {
        1 => w - 2.0 - x,
        2 => h - 2.0 - y,
        3 => y,
        _ => x,
    }
}

pub(crate) fn abs_edge_reached(dir: u8, x: f64, y: f64, w: f64, h: f64) -> bool {
    match dir {
        1 => x >= w - 2.0,
        2 => y >= h - 2.0,
        3 => y <= 0.0,
        _ => x <= 0.0,
    }
}

/// 越境時の Windows 側ボタン扱い。None=ボタンを一切送らない(MOVED 由来の
/// 通常切替)、Some(true)=押下の持ち込み、Some(false)=全ボタンを離す。
/// handoff(take の成立)は ready のスナップショット取得より後の状態のため
/// 優先する: 監視スレッドの complete が両読み取りの間に入るとスナップショット
/// だけが古くなり、MOVE 由来の切替で押下を一度も送らない「どこにも渡らない
/// 掴み」ができる(Windows 側は押下を待って保存へフォールバックする)
pub(crate) fn edge_button_carry(
    event_moved: bool,
    file_drag_ready: bool,
    drag_ok: bool,
    handoff: bool,
) -> Option<bool> {
    if event_moved && !file_drag_ready && !handoff {
        return None;
    }
    Some(drag_ok || handoff)
}

pub(crate) unsafe extern "C" fn tap_callback(
    _proxy: *mut core::ffi::c_void,
    event_type: u32,
    event: CGEventRef,
    _user_info: *mut core::ffi::c_void,
) -> CGEventRef {
    // Deskflow 同様、タイムアウトで無効化されたら再び有効化する(でないと抑制が静かに止まる)。
    // ただし権限を剥奪された場合の無効化なら、入力を握ったまま戻す手段が無いため
    // 解放して終了する(フリーズ防止)
    if event_type == 0xFFFFFFFE || event_type == 0xFFFFFFFD {
        if !ax_trusted() {
            surrender_on_permission_loss();
        }
        if let Some(&tap) = TAP_PORT.get() {
            CGEventTapEnable(tap as CFMachPortRef, true);
        }
        return std::ptr::null_mut();
    }
    let win_mode = WIN_MODE.load(Ordering::Relaxed);
    let connected = CONNECTED.load(Ordering::Relaxed);

    // ホットキー(F13 既定 / KNIT_HOTKEY_KC)= 手動トグル(常に有効、握る)。
    // 修飾キー(右Cmd=54 等)は flagsChanged として届くため、flags の該当ビットで
    // 押下/解放を判別し、押下側でのみトグルする(up・解放側は握るだけ)。
    // 旧実装は KEY_DOWN しかトグルせず(=修飾キー指定が機能しない)、かつ up 把握の
    // 分岐が外側条件により到達不能なデッドコードだった(レビュー Wave1-X5)
    if matches!(event_type, EVT_KEY_DOWN | EVT_KEY_UP | EVT_FLAGS_CHANGED) {
        let kc = CGEventGetIntegerValueField(event, FIELD_KEYCODE);
        // 本体キーボードの音量(F10-12)とメディア(F7-F9)キーは fn フラグ付きで届く。
        // Windows 側の音量・メディア操作として転送し、Mac 側の操作は握る。
        // fn 無しは F キーとしての使用のため素通り(誤転送防止)
        if event_type == EVT_KEY_DOWN && win_mode && CGEventGetFlags(event) & FLAG_FN != 0 {
            let op = match kc {
                72 => Some(0u8), // F12 音量 up
                73 => Some(1),   // F11 音量 down
                74 => Some(2),   // F10 ミュート
                100 => Some(3),  // F7 前の曲へ
                101 => Some(4),  // F8 再生・一時停止
                103 => Some(5),  // F9 次の曲へ
                _ => None,
            };
            if let Some(op) = op {
                send_msg(&Msg::Vol { op });
                return std::ptr::null_mut(); // Mac 側の音量・再生変更を抑制
            }
        }
        if kc == hotkey_kc() {
            let pressed = match event_type {
                EVT_KEY_DOWN => true,
                EVT_KEY_UP => false,
                _ => {
                    let flags = CGEventGetFlags(event);
                    match kc {
                        54 | 55 => flags & FLAG_CMD != 0,
                        58 | 61 => flags & FLAG_OPT != 0,
                        59 | 62 => flags & FLAG_CTRL != 0,
                        56 | 60 => flags & FLAG_SHIFT != 0,
                        _ => true,
                    }
                }
            };
            // キーリピートは down が連続で届くため、押下エッジ(未押下→押下)だけで
            // トグルする。押しっぱなしでの高速トグル暴発を防ぐ
            static HOTKEY_DOWN: AtomicBool = AtomicBool::new(false);
            if pressed && !HOTKEY_DOWN.swap(true, Ordering::Relaxed) && connected {
                do_toggle("hotkey");
            }
            if !pressed {
                HOTKEY_DOWN.store(false, Ordering::Relaxed);
            }
            return std::ptr::null_mut(); // トグル専用キーのため down/up 両方握る
        }
    }

    if matches!(
        event_type,
        EVT_MOUSE_MOVED | EVT_LEFT_DRAGGED | EVT_RIGHT_DRAGGED | EVT_OTHER_DRAGGED
    ) {
        DIAG_MOVE_COUNT.fetch_add(1, Ordering::Relaxed);
        LAST_EVENT_MS.store(now_ms(), Ordering::Relaxed);
    }
    if matches!(event_type, EVT_KEY_DOWN | EVT_KEY_UP | EVT_FLAGS_CHANGED) {
        DIAG_KEY_COUNT.fetch_add(1, Ordering::Relaxed);
    }
    // 自己投稿のUPは元アプリだけに届け、物理的な押下や転送先の状態を変えない。
    if event_type == EVT_LEFT_UP
        && CGEventGetIntegerValueField(event, FIELD_EVENT_SOURCE_USER_DATA) == SYNTH_UP_MAGIC
    {
        return event;
    }
    // マウスボタンの押下状態はモードに関係なく追跡する(ドラッグ中切替の
    // 持ち込み判定に使う。Mac モードの素通し経路でも更新が必要)
    match event_type {
        EVT_LEFT_DOWN => {
            // 掴みドラッグ越境後の終了 UP を掴み始めの位置で出すため、先に記録する
            *PRESS_POS.lock().unwrap_or_else(|e| e.into_inner()) = Some(CGEventGetLocation(event));
            // FinderへDownを渡す前に基準値だけ取得する。URLの読み出しは別スレッド。
            // 押下後のポーリングで基準を作ると、既に始まったドラッグを見逃す。
            let baseline = if !win_mode && connected {
                with_pool(|| {
                    let pb = drag_pasteboard();
                    (!pb.is_null())
                        .then(|| msg0_isize(pb, sel_registerName(c"changeCount".as_ptr())))
                })
            } else {
                None
            };
            FILE_DRAG
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .begin(baseline);
            BTN_DOWN[0].store(true, Ordering::Relaxed);
        }
        EVT_LEFT_DRAGGED => FILE_DRAG.lock().unwrap_or_else(|e| e.into_inner()).moved(),
        EVT_LEFT_UP => {
            // 次回押下で上書きされるが、非ドラッグ時に古い位置を使わない保険として消す
            *PRESS_POS.lock().unwrap_or_else(|e| e.into_inner()) = None;
            FILE_DRAG.lock().unwrap_or_else(|e| e.into_inner()).end();
            BTN_DOWN[0].store(false, Ordering::Relaxed);
        }
        EVT_RIGHT_DOWN => BTN_DOWN[1].store(true, Ordering::Relaxed),
        EVT_RIGHT_UP => BTN_DOWN[1].store(false, Ordering::Relaxed),
        EVT_OTHER_DOWN => BTN_DOWN[2].store(true, Ordering::Relaxed),
        EVT_OTHER_UP => BTN_DOWN[2].store(false, Ordering::Relaxed),
        _ => {}
    }

    if !win_mode {
        // Mac モード: 境界そのものに到達したら Windows モードへ。境界手前の
        // 帯(旧・敏感さ EDGE_PX や速度による先読み)では発火しない=境界を境に越える。
        // Deskflow onMouseMove 準拠: イベント位置はキュー滞留で数フレーム遅れるため、
        // CGEventCreate(NULL) のライブカーソル位置で判定する(境界の応答性の鍵)
        // ファイル掴み中(ドラッグペーストボードにファイル+左ボタン押下)は設定に
        // 依らず常に「掴んだまま境界越え」を許可する(掴んでいる意図が明確なため)
        let file_drag_ready = FILE_DRAG.lock().unwrap_or_else(|e| e.into_inner()).ready();
        // 受信ドラッグ進行中は掴み検出を無効化する: 受信ドラッグ自身が
        // ドラッグ用ペーストボードを変えるため、押下と無関係に偽の掴みになる
        let file_drag_ready = file_drag_ready && !incoming_drag::blocking();
        let drag_ok = DRAG_SWITCH.load(Ordering::Relaxed) || file_drag_ready;
        if (matches!(event_type, EVT_MOUSE_MOVED)
            || (drag_ok
                && matches!(event_type, EVT_LEFT_DRAGGED | EVT_RIGHT_DRAGGED | EVT_OTHER_DRAGGED)))
            && connected
            && !HOTKEY_ONLY.load(Ordering::Relaxed) // hotkey モードでは境界切替しない(ロック)
            && now_ms() >= EDGE_GUARD_UNTIL_MS.load(Ordering::Relaxed)
        {
            {
                // delta 積算でカーソル位置を追跡(Deskflow の m_xCursor 方式)。
                // 32イベントに1回ライブ位置へ同期しドリフトを補正する
                let dx = CGEventGetIntegerValueField(event, FIELD_DELTA_X) as f64;
                let dy = CGEventGetIntegerValueField(event, FIELD_DELTA_Y) as f64;
                // 速度計装: 100ms を超えたら窓を作り直す(§24 のデータ取り)
                {
                    let now = now_ms();
                    let mut r = RECENT_PX.lock().unwrap_or_else(|e| e.into_inner());
                    if now.saturating_sub(r.1) > 100 {
                        *r = (0.0, now);
                    }
                    r.0 += dx.abs() + dy.abs();
                }
                let n = CUR_SYNC_N.fetch_add(1, Ordering::Relaxed);
                let mut pos = CUR_POS.lock().unwrap_or_else(|e| e.into_inner());
                pos.0 += dx;
                pos.1 += dy;
                if n.is_multiple_of(16) {
                    if let Some(loc) = live_cursor() {
                        *pos = (loc.x, loc.y);
                    }
                }
                let (px, py) = *pos;
                drop(pos);
                let g = geo();
                // 到達した辺を選ぶ: 各端末の対象の辺(メインモニター指定を含む)までの
                // 距離を測り、最短の端末へ入る。端末が無ければ全体領域の最短の辺
                let (_dir, enter_peer) = find_enter_edge(&g, px, py);
                // 越え先の境界を確定する(壁ならここで終わり)。切替・跳ね返し・
                // 滞在計測は確定した境界に対してだけ行う。
                // (デッドロック注意: side_dir は PEERS を取るため、PEERS の保持中に
                // 呼ばない。find_enter_edge と同じ制約)
                let boundary = {
                    let active_side = side_dir();
                    let peers = PEERS.lock().unwrap_or_else(|e| e.into_inner());
                    boundary_of(&peers, active_side, enter_peer)
                };
                let Some((dir, boundary_mon)) = boundary else {
                    return event; // 接続のない辺は壁
                };
                let (lay_lo, lay_hi) = *LAY_RANGE.lock().unwrap_or_else(|e| e.into_inner());
                // 「切替境界までの距離」は確定した境界(モニター指定込み)で測る。
                // 従来は全体領域の端で測っていたため、メインモニター指定の境界では
                // 距離が 0 に届かず、逆に上・下・外部モニターの端のような接続のない
                // 辺では、積算のずれを距離が隠して勝手に越えていた(カーソル消失)。
                // Deskflow の links 相当+斜め(4-7)は境界の半分(上/下)でのみ接続
                let gap = |x: f64, y: f64| -> f64 {
                    let base = peer_gap(&g, boundary_mon, dir, x, y);
                    if base > 40.0 {
                        return base; // 境界から遠い=範囲判定不要
                    }
                    // 境界付近でのみ接続範囲(配置エディタ/斜め配置)の制限を適用する。
                    // 範囲は境界ディスプレイの辺に沿った比率で測る
                    let f = along_ratio_on(&g, dir, boundary_mon, x, y);
                    if f < lay_lo || f > lay_hi {
                        return f64::MAX; // 相手の画面が接している範囲外
                    }
                    base
                };
                // 境界から十分内側へ戻ったらヒット状態をリセット(次の到達を1回目として数える)
                if gap(px, py) > 8.0 {
                    EDGE_AT_EDGE.store(false, Ordering::Relaxed);
                    EDGE_STAY_SINCE_MS.store(0, Ordering::Relaxed);
                }
                // 戻った直後は境界ちょうどに出るため、境界から離れるまで再突入を受け付けない
                if gap(px, py) > EDGE_REARM_PX
                    && !REENTRY_ARMED.swap(true, Ordering::Relaxed)
                {
                    eprintln!("[edge] 境界から離れたため再突入を受け付けます(gap={:.0} 積算=({:.0},{:.0}))", gap(px, py), px, py);
                }
                // 積算カーソルが境界そのものに達したときだけ判定を始める(手前では
                // いかなる速度・設定でも発火しない)。実カーソルでの到達確認はブロック内
                if gap(px, py) <= 0.0 && REENTRY_ARMED.load(Ordering::Relaxed) {
                    // 受信ドラッグ(NSDraggingSession)進行中: 物理ボタンは押されたまま
                    // Mac 側のドロップを続けるため、ここでは Windows へ自動切替しない。
                    // 切替すると入力転送がセッションからマウス入力を奪い、ended も
                    // ドロップも来なくなる(実測)。掴んだまま境界に触れた場合は
                    // ボタンを離して終わらせるのが意図された復帰操作
                    // 渡せなかった掴みは、離すまで Mac 側のドラッグとして続ける
                    if FILE_DRAG
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .refused()
                    {
                        return event;
                    }
                    if incoming_drag::blocking() {
                        // 操作を宙吊りにした理由が利用者に分かるよう、
                        // 初回だけ案内を出す(連投しない)
                        static DRAG_GUIDE_MS: AtomicU64 = AtomicU64::new(0);
                        let now = now_ms();
                        if now.saturating_sub(DRAG_GUIDE_MS.swap(now, Ordering::Relaxed)) >= 120_000
                        {
                            notify("Knit", "ファイルを掴んだままです。Mac のドロップ先でボタンを離すとそこへ置けます(境界では切り替わりません)");
                        }
                        return event;
                    }
                    // switchCorners(+cornerSize): 四隅 N px 内では切替しない(誤爆防止)
                    let corner = CORNER_PX.load(Ordering::Relaxed) as f64;
                    if corner > 0.0
                        && (px < g.min_x + corner || px > g.max_x - corner)
                        && (py < g.min_y + corner || py > g.max_y - corner)
                    {
                        return event;
                    }
                    // 二段階判定: 実カーソル(ライブ位置)も境界そのものに達していなければ
                    // 発火しない。OS はカーソルを画面端で止めるため、高速移動で積算が
                    // 先に境界を跨いでいても、実カーソルが境界に触れた瞬間まで待つ
                    // (旧実装の「境界手前 40px まで許容」は速い動きが境界付近で
                    // 勝手に越える原因だった)。ドリフトもここで実位置へ補正する
                    let Some(loc) = live_cursor() else {
                        return event;
                    };
                    if !boundary_crossed(gap(px, py), gap(loc.x, loc.y)) {
                        // まだ境界に届いていない: 積算を実位置へ合わせて届くのを待つ
                        *CUR_POS.lock().unwrap_or_else(|e| e.into_inner()) = (loc.x, loc.y);
                        return event;
                    }
                    // Crossing Intelligence(ビジョン§24): 境界への速度が十分大きければ
                    // 「意図的な越え」とみなして滞在待ち(switchDelay)とダブルタップを
                    // スキップする。ゆっくり端に触れた場合だけ従来どおりの誤爆防止が働く。
                    // 計装(下のログ)で集めた分布をもとに閾値は 1200px/s とする。
                    // `KNIT_FAST_EDGE=0` で無効化
                    let speed = {
                        let r = *RECENT_PX.lock().unwrap_or_else(|e| e.into_inner());
                        px_per_sec(r.0, now_ms().saturating_sub(r.1))
                    };
                    let fast = FAST_EDGE.load(Ordering::Relaxed) && speed >= 1200.0;
                    // switchDelay: 端に N ms 滞ってから切替(0=無効)。
                    // 滞在判定は「端に到達し続けている」間のみ継続する
                    let delay = SWITCH_DELAY_MS.load(Ordering::Relaxed);
                    if delay > 0 && !fast {
                        let now = now_ms();
                        let since = EDGE_STAY_SINCE_MS.load(Ordering::Relaxed);
                        if since == 0 {
                            EDGE_STAY_SINCE_MS.store(now, Ordering::Relaxed);
                            return event; // 滞在計測を開始(まだ切替ない)
                        }
                        if now.saturating_sub(since) < delay {
                            return event; // まだ規定時間に達していない
                        }
                        EDGE_STAY_SINCE_MS.store(0, Ordering::Relaxed);
                    } else if delay == 0 && !fast && !EDGE_AT_EDGE.swap(true, Ordering::Relaxed) {
                        // switchDoubleTap: 閾値を「下から跨いだ瞬間」だけをヒットと数え、
                        // 判定窓(DOUBLE_TAP_MS)以内の 2回目のヒットでのみ切替する。
                        // カーソルが境界に張り付いたまま出す delta は継続扱いで数えない
                        let taps = EDGE_TAPS.load(Ordering::Relaxed);
                        let now = now_ms();
                        let win_ms = DOUBLE_TAP_MS.load(Ordering::Relaxed).max(100);
                        let prev = EDGE_LAST_HIT_MS.swap(now, Ordering::Relaxed);
                        let fire = taps <= 1 || (prev > 0 && now.saturating_sub(prev) <= win_ms);
                        if !fire {
                            // 1回目: 境界から少し内側へ弾き返す。壁に当たった感触で
                            // 「もう一度押すと通る」ことを体感させる(本質の可視化)
                            eprintln!("[edge] 1回目の到達(跳ね返し)");
                            // 配置番号(0-7)ではなく方向で判定する(旧実装は左上/左下配置で
                            // 右端へ弾き返していた)
                            let (bx, by) = match dir {
                                1 => (g.min_x + 15.0, loc.y),
                                2 => (loc.x, g.min_y + 15.0),
                                3 => (loc.x, g.max_y - 15.0),
                                _ => (g.max_x - 15.0, loc.y),
                            };
                            CGWarpMouseCursorPosition(CGPoint { x: bx, y: by });
                            *CUR_POS.lock().unwrap_or_else(|e| e.into_inner()) = (bx, by);
                            EDGE_AT_EDGE.store(false, Ordering::Relaxed);
                            return event;
                        }
                        EDGE_LAST_HIT_MS.store(0, Ordering::Relaxed);
                    }
                    // 予告もファイルも、実際に越える端末へ送る。先に送信先を確定する。
                    // この辺に割り当てられた端末へ入力の送信先を切り替える
                    // (端末ごとの配置。複数該当時は最短の端末を優先)
                    if let Some(idx) = enter_peer.or_else(|| peer_at_side(dir)) {
                        let act = *ACTIVE_PEER.lock().unwrap_or_else(|e| e.into_inner());
                        if idx != act {
                            activate_peer(idx, "境界");
                        }
                    }
                    // 掴んだファイルは切替より先に受け付け、予告を押下より前に本線へ積む。
                    // 渡せない掴みでは境界を越えず、Mac 側のドラッグをそのまま続けさせる
                    // (越えると元のドラッグを画面端で終わらせることになる)。
                    // 指紋登録と履歴の記録は転送スレッド側で行う(M6)
                    let dragged_files = FILE_DRAG.lock().unwrap_or_else(|e| e.into_inner()).take();
                    let mut handoff = None;
                    if let Some(files) = dragged_files {
                        eprintln!("[file] 掴みドラッグ切替: {} 件を転送します", files.len());
                        let Some(id) = offer_drag_to_win(&files) else {
                            // 離すまで越えさせず、境界の到達回数も数え直す
                            FILE_DRAG.lock().unwrap_or_else(|e| e.into_inner()).refuse();
                            EDGE_AT_EDGE.store(false, Ordering::Relaxed);
                            EDGE_LAST_HIT_MS.store(0, Ordering::Relaxed);
                            EDGE_STAY_SINCE_MS.store(0, Ordering::Relaxed);
                            return event;
                        };
                        handoff = Some((files, id));
                    }
                    WIN_MODE.store(true, Ordering::Relaxed);
                    // 入り直後の戻り判定を 600ms 抑制する: warp 直後の残りの移動入力で
                    // 仮想カーソルが端まで一気に運ばれ、操作する間もなく Mac へ戻される
                    // (「押し戻される」報告)のを防ぐ
                    ENTER_GUARD_UNTIL_MS.store(now_ms() + 600, Ordering::Relaxed);
                    DIAG_MODE_COUNT.fetch_add(1, Ordering::Relaxed);
                    // 到達時の速度を添える(§24 用の計装。速い=意図的な越え、
                    // 遅い=停止しようとして端に触れた、の分布を実機で見る)
                    // fast 経路は滞在/ダブルタップをスキップしたことが分かるよう明記する
                    eprintln!(
                        "[mode] WINDOWS (edge{}) at ({:.0},{:.0}) v={speed:.0}px/s",
                        if fast { ", fast" } else { "" },
                        loc.x,
                        loc.y
                    );
                    // 越境時にどの端末へ入ったかを記録する(タブレットからの戻り直後に
                    // 意図しない端末へ飛ぶ報告の原因特定用。端末の選択は find_enter_edge)
                    {
                        let peers = PEERS.lock().unwrap_or_else(|e| e.into_inner());
                        let target = enter_peer
                            .and_then(|i| peers.get(i))
                            .map(|p| format!("{}(side={})", p.name, p.side))
                            .unwrap_or_else(|| "なし".into());
                        eprintln!("[mode] ENTER peer={target}");
                    }

                    // ファイル掴み切替: 掴んだファイルを Windows へ流し、Mac 側の
                    // ドラッグは合成 LeftMouseUp で完結させる(Finder の宙吊り防止)。
                    // UP は tap コールバック内で post できないため別スレッド投稿。
                    // 投稿イベントは自分の HID タップを再通過する(定番の再帰問題)ため
                    // kCGEventSourceUserData にマジックを刻み、tap 側で識別して
                    // 「Mac へ素通し・Windows へは転送しない」処理をする(転送すると
                    // 押したままのユーザー意図に反して Win 側が離した扱いになる)
                    if handoff.is_some() {
                        // 位置は spawn 時点のコピーで渡す: 越境後60ms以内に来る物理
                        // UP が tap 側で PRESS_POS を消すため、sleep 後の読み直しは
                        // None→ライブ位置(=画面端)へ落ちて H3 が再発する
                        let press = *PRESS_POS.lock().unwrap_or_else(|e| e.into_inner());
                        std::thread::spawn(move || {
                            std::thread::sleep(Duration::from_millis(60));
                            unsafe {
                                let pos = drag_end_position(press, live_cursor())
                                    .unwrap_or(CGPoint { x: 0.0, y: 0.0 });
                                // 掴み始めの位置へ一度戻してから UP を出す: 画面端の
                                // 座標のままだと Finder が端へのドロップと解釈して
                                // 原本を動かすことがある(開始位置なら自己ドロップで
                                // no-op)。warp は移動イベントを生成しないのでタップを
                                // 乱さず、カーソルは非表示+関連切断済みで LOCK_POS
                                // 監視スレッドが巻き戻す
                                // enter 時に設定した抑制窓(0.0001s)に warp 直後の
                                // post が入ると合成 UP が捨てられるため、warp の前に
                                // 抑制を切る(Deskflow setZeroSuppressionInterval)
                                CGSetLocalEventsSuppressionInterval(0.0);
                                CGWarpMouseCursorPosition(pos);
                                let e = make_drag_end_event(pos);
                                if !e.is_null() {
                                    CGEventPost(0 /* kCGHIDEventTap */, e);
                                    CFRelease(e);
                                }
                            }
                        });
                    }
                    // 入り位置は常に越えた境界の対応位置(相手側の反対の辺の内側)。
                    // 前回位置の記憶・復元はせず、行きも戻りも「境界のすぐ内側から
                    // 動き始める」同じ連続体験にする(戻り方向と同じ 60px 内側)。
                    // 高さは境界ディスプレイ(モニター指定込み)の y 範囲で正規化
                    //(反転なし: 画面上端同士が対応)
                    let (nx, ny) = {
                        let (ww, wh) = *WIN_SCREEN.lock().unwrap_or_else(|e| e.into_inner());
                        let r = along_ratio_on(&g, dir, boundary_mon, loc.x, loc.y);
                        win_entry_pos(dir, (ww, wh), r)
                    };
                    send_msg(&Msg::Warp { nx, ny });
                    eprintln!("[warp] -> win ({:.2},{:.2})", nx, ny);
                    // ドラッグ中の切替: 既定は全ボタンを離して持ち込まない(誤ドラッグ防止。
                    // レビュー Wave1 C-S13)。KNIT_DRAG_SWITCH=1 では逆に押下中の
                    // ボタンを Windows 側で押し直す=「掴んだまま境界を越える」体験。
                    // 掴みドラッグ中は EVT_MOUSE_MOVED 由来の切替でも持ち込む
                    // (押下直後の軽い移動は MOVED として届くことがある=実測。
                    // 持ち込み漏れは Win 側のフォールバックを誘発する)。
                    // 押し直しはワープの後に送る(前回の Windows 位置にある物を押さない)
                    match edge_button_carry(
                        event_type == EVT_MOUSE_MOVED,
                        file_drag_ready,
                        drag_ok,
                        handoff.is_some(),
                    ) {
                        Some(true) => {
                            for b in 0u8..=2 {
                                if BTN_DOWN[b as usize].load(Ordering::Relaxed) {
                                    send_msg(&Msg::MouseButton { btn: b, down: true });
                                }
                            }
                        }
                        Some(false) => {
                            for b in 0u8..=2 {
                                send_msg(&Msg::MouseButton {
                                    btn: b,
                                    down: false,
                                });
                            }
                        }
                        None => {}
                    }
                    if let Some((files, id)) = handoff {
                        send_drag_files_to_win(files, id);
                    }
                    // 絶対位置モードの仮想カーソルを Warp 先で初期化
                    {
                        let (ww, wh) = *WIN_SCREEN.lock().unwrap_or_else(|e| e.into_inner());
                        RETURN_ARMED.store(false, Ordering::Relaxed);
                        *WIN_CUR.lock().unwrap_or_else(|e| e.into_inner()) = (nx * ww, ny * wh);
                    }
                    enter_win_mode_cursor_lock();
                    return std::ptr::null_mut();
                }
            }
        }
        return event; // 素通し
    }

    // Windows モード: 全イベントを握って転送
    let flags = CGEventGetFlags(event);
    // F 行のメディア(NSSystemDefined, subtype 8)の翻訳: 輝度(F1/F2)とキーボード
    // 照明(F5/F6)は macOS が key イベントではなく system-defined で配るため、
    // key 経路だけだと Windows で反応しない(実績: F5 等が効かなかった)。
    // 対応する F キーとして届け直す。音量・再生(F7〜F12)は key 経路
    //(kc 72-74/100/101/103)で処理済みのためここでは転送しない(二重送信の防止)
    if event_type == EVT_SYSTEM_DEFINED {
        if let Some((_, data1)) = ns_media_event(event) {
            let nx = (data1 >> 16) & 0xFFFF;
            let down = ((data1 >> 8) & 0xFF) == 0x0A; // 0x0A=押下 / 0x0B=解放
            let kc: u16 = match nx {
                3 => 122, // 輝度を下げる → F1
                2 => 120, // 輝度を上げる → F2
                22 => 96, // キーボード照明を下げる → F5
                21 => 97, // キーボード照明を上げる → F6
                // 未対応タイプ(聴写キー等がここに来る機種あり)は記録して次の
                // 対応表追加に備える。毎回 1 行だけで洪水にはならない
                _ => {
                    eprintln!(
                        "[media] 未対応 nx={nx}({})",
                        if down { "down" } else { "up" }
                    );
                    0
                }
            };
            if kc != 0 {
                send_msg(&Msg::Key {
                    kc,
                    down,
                    ctrl: false,
                    opt: false,
                    cmd: false,
                    shift: false,
                    tr: false,
                    rcmd: false,
                });
                eprintln!(
                    "[media] nx={nx} -> kc={kc}({})",
                    if down { "down" } else { "up" }
                );
            }
        }
        return std::ptr::null_mut();
    }
    let (ctrl, opt, cmd, shift) = (
        flags & FLAG_CTRL != 0,
        flags & FLAG_OPT != 0,
        flags & FLAG_CMD != 0,
        flags & FLAG_SHIFT != 0,
    );
    match event_type {
        EVT_KEY_DOWN | EVT_KEY_UP | EVT_FLAGS_CHANGED => {
            let kc = CGEventGetIntegerValueField(event, FIELD_KEYCODE) as u16;
            let down = if event_type == EVT_FLAGS_CHANGED {
                // flagsChanged は「その修飾が押された」イベントのみ来る(離す時は flags から消える)
                // 押下状態は flags から判定
                match kc {
                    54 | 55 => cmd,
                    56 | 60 => shift,
                    58 | 61 => opt,
                    59 | 62 => ctrl,
                    _ => false,
                }
            } else {
                event_type == EVT_KEY_DOWN
            };
            // 転送中の Esc は「転送の中止」として扱い、Windows へ流さない
            //(ドラッグ操作の中止という両 OS 共通の慣習に合わせる)。
            // ただし Mac で作業中(カーソルが Mac 画面にある間)は握らない:
            // Mac のアプリの取消(Esc)を奪って勝手に転送を止めないため
            if event_type == EVT_KEY_DOWN
                && kc == 53
                && WIN_MODE.load(Ordering::Relaxed)
                && cancel_active_xfer()
            {
                eprintln!("[file] Esc で転送を中止します");
                return std::ptr::null_mut();
            }
            // 右⌘(kc 54)の押下状態を追跡し、押下中は cmd を rcmd(右 Ctrl)へ
            // 置き換えて送る。左⌘(55)は従来どおり cmd のまま。combos でも
            // 「右⌘+C = 右 Ctrl+C」になる(rcmd 押下中は cmd を落とす)
            if event_type == EVT_FLAGS_CHANGED && kc == 54 {
                R_RIGHT_CMD.store(cmd, Ordering::Relaxed);
            }
            let rcmd = RCMD_CTRL.load(Ordering::Relaxed) && R_RIGHT_CMD.load(Ordering::Relaxed);
            let cmd = if rcmd { false } else { cmd };
            // 実機のキーコード特定用: 「右⌘が効かない」報告の切り分け。
            // ここに出ない=そのキーは 54 ではない(外付けの配列差・キーリマップ等)
            if event_type == EVT_FLAGS_CHANGED && (kc == 54 || kc == 55) {
                eprintln!(
                    "[rcmd] kc={} -> {}(rcmd={})",
                    kc,
                    if cmd || rcmd { "押下" } else { "解放" },
                    rcmd
                );
            }
            if down && (kc == 104 || kc == 102) {
                eprintln!(
                    "[ime] kc={kc} ({}) 転送",
                    if kc == 104 { "かな" } else { "英数" }
                );
            }
            // Caps Lock は Mac では押すたびに flagsChanged が 1 回だけ来る(押下/解放の
            // 区別がない)。Windows はキーの押し離しでトグルするため 1 回を down+up に展開する
            if event_type == EVT_FLAGS_CHANGED && kc == 57 {
                for d in [true, false] {
                    send_msg(&Msg::Key {
                        kc,
                        down: d,
                        ctrl,
                        opt,
                        cmd,
                        shift,
                        tr: false,
                        rcmd,
                    });
                }
                return std::ptr::null_mut();
            }
            // Continue Here(ビジョン§11): ⌥⌘T で Mac の前面ブラウザの URL を
            // Windows の既定ブラウザで開く。「Mac で見ていたページを Windows でもう
            // 一度探す」摩擦を 1 回で消す。osascript が 100-300ms かかるため
            // タップを塞がないよう別スレッドで取得する。
            // 押下エッジだけで発火する(キーリピートで osascript とタブが
            // 連発するのを防ぐ)。up も握る(down だけ握ると up 単体が
            // Windows へ転送され、修飾の押し替えが前面アプリへ漏れる)
            if kc == 17 && opt && cmd && !ctrl && !shift && CONTINUE_HERE.load(Ordering::Relaxed) {
                static CONT_T_DOWN: AtomicBool = AtomicBool::new(false);
                let down = event_type == EVT_KEY_DOWN;
                if down && !CONT_T_DOWN.swap(true, Ordering::Relaxed) {
                    // 直近の発火から 1.5 秒は再送しない(押し直しの連打対策)
                    static LAST_FIRE_MS: AtomicU64 = AtomicU64::new(0);
                    let now = now_ms();
                    if now.saturating_sub(LAST_FIRE_MS.load(Ordering::Relaxed)) >= 1_500 {
                        LAST_FIRE_MS.store(now, Ordering::Relaxed);
                        std::thread::spawn(continue_here);
                    }
                }
                if !down {
                    CONT_T_DOWN.store(false, Ordering::Relaxed);
                }
                return std::ptr::null_mut();
            }
            // Android text input needs the native Mac IME, not raw US keycodes.
            // Keep command shortcuts on the existing remote route. Plain text,
            // Option accents and editing keys reach the native input client.
            let composing = gui::direct_input::composing();
            let mapped = MAC_KEYS.load(Ordering::Relaxed) && mac_shortcut_translation(kc, ctrl, opt, cmd, shift).is_some();
            if !cmd && !rcmd && (!ctrl || composing) && (composing || !mapped) {
                if let Some(native) = gui::direct_input::capture(event) {
                    return if native { event } else { std::ptr::null_mut() };
                }
            }
            if event_type == EVT_KEY_DOWN { gui::direct_input::finish(); }
            // ---- Mac 流ショートカットの Windows 翻訳(指癖をそのまま通す) ----
            // 元キーは握りつぶし、翻訳先の Key を送る。修飾の対応:
            //   cmd→Win Ctrl / opt→Win Alt / ctrl→Win キー(既定マップ)
            // 注意: flagsChanged(mod キー単体)は翻訳しない。
            // 常時有効(マスト機能)。KNIT_MAC_KEYS=0 でのみオフ
            if event_type != EVT_FLAGS_CHANGED && MAC_KEYS.load(Ordering::Relaxed) {
                // 翻訳先の修飾は「既定マップ(cmd→Ctrl / opt→Alt)」で解釈させる。
                // CMD_ALT=true でも翻訳の意味が変わらないよう、cmd/opt を差し替える
                let swap = crate::CMD_ALT.load(Ordering::Relaxed);
                let send = |kc2: u16, d: bool, c: bool, o: bool, m: bool, sh: bool, r: bool| {
                    let (c2, o2, m2) = if swap { (c, m, o) } else { (c, o, m) };
                    send_msg(&Msg::Key {
                        kc: kc2,
                        down: d,
                        ctrl: c2,
                        opt: o2,
                        cmd: m2,
                        shift: sh,
                        tr: true,
                        rcmd: r,
                    });
                };
                // fn+F11(Mac のデスクトップ表示)= Win+D(FN フラグは表の外)
                if kc == 103 && flags & FLAG_FN != 0 {
                    send(2, down, true, false, false, false, rcmd); // D + ctrl フラグ(Win キー)
                    return std::ptr::null_mut();
                }
                if let Some((kc2, c, o, m, s)) = mac_shortcut_translation(kc, ctrl, opt, cmd, shift)
                {
                    send(kc2, down, c, o, m, s, rcmd);
                    return std::ptr::null_mut(); // 元キーは送らない
                }
            }
            send_msg(&Msg::Key {
                kc,
                down,
                ctrl,
                opt,
                cmd,
                shift,
                tr: false,
                rcmd,
            });
        }
        EVT_MOUSE_MOVED | EVT_LEFT_DRAGGED | EVT_RIGHT_DRAGGED | EVT_OTHER_DRAGGED => {
            let dx = CGEventGetIntegerValueField(event, FIELD_DELTA_X) as f64;
            let dy = CGEventGetIntegerValueField(event, FIELD_DELTA_Y) as f64;
            if dx != 0.0 || dy != 0.0 {
                let sc = mouse_scale();
                if MOUSE_ABS_MODE.load(Ordering::Relaxed) && !GAME_REL.load(Ordering::Relaxed) {
                    // 絶対位置モード: Mac の加速済み delta に Windows 側の加速が
                    // 二重に乗るのを防ぎつつ、画面比率で見た目の移動距離を揃える
                    let (ww, wh) = *WIN_SCREEN.lock().unwrap_or_else(|e| e.into_inner());
                    let (mw, mh) = {
                        let g = geo();
                        (g.main_w, g.main_h)
                    };
                    let (sx, sy) = (ww / mw, wh / mh); // 方向別スケール(改善B)
                                                       // 重要: WIN_CUR のガードをこのブロック内で必ず解放してから
                                                       // leave_win_mode_cursor_unlock を呼ぶ(内部で WIN_CUR を再ロック
                                                       // するため、保持したまま呼ぶと自己デッドロックでタップが固まる)
                    let (nx, ny, at_left) = {
                        let mut wc = WIN_CUR.lock().unwrap_or_else(|e| e.into_inner());
                        wc.0 = (wc.0 + dx * sc * sx).clamp(0.0, ww - 2.0);
                        wc.1 = (wc.1 + dy * sc * sy).clamp(0.0, wh - 2.0);
                        // Mac 側へ戻る辺は Windows の位置で決まる(右配置なら Windows の左端、
                        // 左配置なら右端、上配置なら下端、下配置なら上端)。旧実装は常に
                        // 左端で判定し、上下配置では Windows の左端に触れるだけで戻っていた
                        // 戻りも境界そのもの(clamp の止まり値)でのみ判定する
                        // 入った直後の位置は戻る辺ちょうど。そこから離れるまでは戻さない
                        if abs_edge_dist(side_dir(), wc.0, wc.1, ww, wh) > EDGE_REARM_PX {
                            RETURN_ARMED.store(true, Ordering::Relaxed);
                        }
                        let at_edge = RETURN_ARMED.load(Ordering::Relaxed)
                            && abs_edge_reached(side_dir(), wc.0, wc.1, ww, wh);
                        (
                            wc.0 / ww,
                            wc.1 / wh,
                            !HOTKEY_ONLY.load(Ordering::Relaxed)
                                && event_type == EVT_MOUSE_MOVED
                                && at_edge
                                && now_ms()
                                    >= EDGE_GUARD_UNTIL_MS.load(Ordering::Relaxed)
                                && now_ms()
                                    >= ENTER_GUARD_UNTIL_MS.load(Ordering::Relaxed),
                        )
                    };
                    // 毎イベント送信(量子化スキップは低速時にステップ感が出るため廃止)
                    LAST_ABS_MS.store(now_ms(), Ordering::Relaxed);
                    DIAG_ABS_COUNT.fetch_add(1, Ordering::Relaxed);
                    send_msg(&Msg::MouseAbs { nx, ny });
                    // 左端到達はMac内完結で即復帰(Win往復のRTT分を削減)
                    // ドラッグ中は意図しない復帰をしない(ボタン操作中の境界越えのため)
                    if at_left {
                        WIN_MODE.store(false, Ordering::Relaxed);
                        DIAG_MODE_COUNT.fetch_add(1, Ordering::Relaxed);
                        // 診断: どの位置で戻り判定になったか(推定と実物のずれの調査用)
                        let (aww, awh) =
                            *WIN_SCREEN.lock().unwrap_or_else(|e| e.into_inner());
                        let (wcx, wcy) = *WIN_CUR.lock().unwrap_or_else(|e| e.into_inner());
                        if aww > 0.0 && awh > 0.0 {
                            eprintln!(
                                "[mode] MAC (abs-edge) 推定 {:.2},{:.2}",
                                wcx / aww,
                                wcy / awh
                            );
                        } else {
                            eprintln!("[mode] MAC (abs-edge)");
                        }
                        // 境界に沿った比率: 左右の辺は縦位置、上下の辺は横位置
                        leave_win_mode_cursor_unlock(Some(if side_dir() >= 2 { nx } else { ny }));
                    }
                } else {
                    // 相対移動モード(従来互換)
                    send_msg(&Msg::MouseMove {
                        dx: dx * sc,
                        dy: dy * sc,
                    });
                }
            }
            // カーソル固定の巻き戻しは 200ms 監視スレッドに集約した
            // (タップ内で毎イベント CGEventCreate すると負荷でカクつくため)
        }
        EVT_LEFT_DOWN | EVT_LEFT_UP => {
            let d = event_type == EVT_LEFT_DOWN;
            if d {
                gui::direct_input::finish();
                gui::direct_input::new_session();
            }
            BTN_DOWN[0].store(d, Ordering::Relaxed);
            send_msg(&Msg::MouseButton { btn: 0, down: d });
        }
        EVT_RIGHT_DOWN | EVT_RIGHT_UP => {
            let d = event_type == EVT_RIGHT_DOWN;
            BTN_DOWN[1].store(d, Ordering::Relaxed);
            send_msg(&Msg::MouseButton { btn: 1, down: d });
        }
        EVT_OTHER_DOWN | EVT_OTHER_UP => {
            let d = event_type == EVT_OTHER_DOWN;
            BTN_DOWN[2].store(d, Ordering::Relaxed);
            send_msg(&Msg::MouseButton { btn: 2, down: d });
        }
        EVT_SCROLL_WHEEL => {
            let dy = CGEventGetIntegerValueField(event, FIELD_SCROLL_A1) as f64;
            let dx = CGEventGetIntegerValueField(event, FIELD_SCROLL_A2) as f64;
            if win_mode && active_peer_is_android() && trackpad::suppress_scroll(dx, dy) {
                return std::ptr::null_mut();
            }
            // 2本指の横スワイプ →「戻る/進む」。Mac の体感(1スワイプ=1ページ)を
            // 忠実に再現する: ジェスチャは「300ms イベントが途切れるまで」を一続きと
            // みなし、その間の発火は 1 回だけ(指を離した後の慣性 delta が届いても
            // 再発火しない=2段階戻りの防止)。閾値 60px・横優勢(|dx|*2>|dy|)のみ。
            // KNIT_SWIPE_NAV=0 で従来の横ホイールへ戻せる
            let swipe_nav = SWIPE_NAV.load(Ordering::Relaxed);
            if swipe_nav && win_mode && !active_peer_is_android() && dx != 0.0 && dx.abs() * 2.0 > dy.abs() {
                let now = now_ms();
                let mut acc = SWIPE_ACC.lock().unwrap_or_else(|e| e.into_inner());
                // 前回のイベントから 300ms 以上空いていたら新しいジェスチャ
                // (=累積と発火済みフラグの両方を引き直す)
                if now.saturating_sub(acc.1) > 300 {
                    if acc.2 != 0 {
                        eprintln!("[swipe] gesture 追加分={:.0}(発火済みのため不採用)", acc.0);
                    } else if acc.0.abs() > 8.0 {
                        eprintln!("[swipe] gesture total={:.0}(未達)", acc.0);
                    }
                    *acc = (0.0, now, 0);
                }
                acc.0 += dx;
                acc.1 = now;
                // acc.2 != 0 = このジェスチャで発火済み。以後の累積は破棄扱い
                if acc.2 == 0 && acc.0.abs() >= 60.0 {
                    // Android 接続先には送らない: BACK 相当が「履歴がない時にアプリを
                    // 終了させる」Android の標準動作を引き起こす(実機報告)。戻る操作は
                    // 別の手段(キー注入)で提供する
                    if active_peer_is_android() {
                        eprintln!("[swipe] Android 接続先のため横スワイプを無視します");
                    } else {
                        // Mac の操作感: 指を右へスワイプ(ページを左へめくる)=戻る。
                        // dx>0=指右 → XButton1(戻る)、dx<0=指左 → XButton2(進む)
                        let btn = if acc.0 > 0.0 { 3u8 } else { 4 }; // 3=戻る, 4=進む
                        send_msg(&Msg::MouseButton { btn, down: true });
                        send_msg(&Msg::MouseButton { btn, down: false });
                        eprintln!(
                            "[swipe] {} 送信(total={:.0})",
                            if btn == 3 { "戻る" } else { "進む" },
                            acc.0
                        );
                    }
                    *acc = (0.0, now, now); // 発火済みマーク(ジェスチャ完結まで保持)
                }
                // 横優勢ジェスチャはここで完結(縦の揺れも無視し二重発火を防ぐ)
                return std::ptr::null_mut();
            }
            if dx != 0.0 || dy != 0.0 {
                // ピクセル delta → ノッチ単位へ累積変換。0.05ノッチ(=6 wheel units)刻みで
                // 送る=Windows のプレシジョンタッチパッドと同じ高解像度スクロール。
                // 0.25刻み(30 units)は低速スクロールがカクつくため細かくした。
                // 除数を大きくすると遅くなる(設定ウィンドウのスライダーで可変)。端数は持ち越し
                // 互換モードは 1 ノッチ(120)単位に量子化(旧来のホイール相当)。
                // 既定は 0.05 ノッチ(=6 wheel units)の高解像度
                let q: f64 = if SCROLL_COMPAT.load(Ordering::Relaxed) {
                    1.0
                } else {
                    0.05
                };
                let div = scroll_div();
                // 方向: 既定は Mac の操作感に合わせる(自然スクロール設定を起動時に
                // 取得)。SCROLL_FLIP=true は「Windows 標準」への手動上書き。
                // 実測: 自然スクロール環境で Mac と同じ向きになるのは -1 側
                let aligned = if NATURAL_SCROLL.load(Ordering::Relaxed) {
                    -1.0
                } else {
                    1.0
                };
                let sgn = if SCROLL_FLIP.load(Ordering::Relaxed) {
                    -aligned
                } else {
                    aligned
                };
                let mut acc = SCROLL_ACC.lock().unwrap_or_else(|e| e.into_inner());
                acc.0 += sgn * dx / div;
                acc.1 += sgn * dy / div;
                // 異常な残高(1e6超)は何かの暴発なので捨てる
                if acc.0.abs() > 1.0e6 || acc.1.abs() > 1.0e6 {
                    *acc = (0.0, 0.0);
                }
                let (ix, iy) = ((acc.0 / q).trunc() * q, (acc.1 / q).trunc() * q);
                if ix != 0.0 || iy != 0.0 {
                    acc.0 -= ix;
                    acc.1 -= iy;
                    DIAG_SCROLL_COUNT.fetch_add(1, Ordering::Relaxed);
                    send_msg(&Msg::Scroll { dx: ix, dy: iy });
                }
            }
        }
        _ => {}
    }
    std::ptr::null_mut() // 握りつぶす
}

#[cfg(test)]
mod entry_pos_tests {
    use super::win_entry_pos;
    const SCREEN: (f64, f64) = (1920.0, 1080.0);

    #[test]
    fn enters_exactly_on_the_peer_side_that_hosts_the_boundary() {
        // 右へ出る = 相手は右 → 相手の左端そのものから入る
        assert_eq!(win_entry_pos(0, SCREEN, 0.5), (0.0, 0.5));
    }

    #[test]
    fn enters_on_the_peer_side_opposite_the_mac_exit_direction() {
        assert_eq!(win_entry_pos(1, SCREEN, 0.25), (1.0, 0.25)); // 左へ出る → 相手の右端
        assert_eq!(win_entry_pos(2, SCREEN, 0.5), (0.5, 1.0)); // 上へ出る → 相手の下端
        assert_eq!(win_entry_pos(3, SCREEN, 0.5), (0.5, 0.0)); // 下へ出る → 相手の上端
    }

    #[test]
    fn entry_does_not_depend_on_the_peer_screen_size() {
        assert_eq!(win_entry_pos(0, (0.0, 0.0), 0.5), (0.0, 0.5));
    }
}

#[cfg(test)]
mod side_geometry_tests {
    use super::{base_dir, boundary_of, side_lay_range, PeerEntry};

    fn peer(side: u8) -> PeerEntry {
        PeerEntry {
            id: "w1".into(),
            name: "w1".into(),
            ip: "127.0.0.1".parse().unwrap(),
            screen: (1920.0, 1080.0),
            monitors: Vec::new(),
            writer: None,
            gen: 1,
            side,
            ver: knit_common::proto::VERSION,
            edge_monitor: None,
        }
    }

    #[test]
    fn diagonal_sides_map_to_their_base_edges() {
        assert_eq!(base_dir(0), 0);
        assert_eq!(base_dir(1), 1);
        assert_eq!(base_dir(2), 2);
        assert_eq!(base_dir(3), 3);
        assert_eq!(base_dir(4), 0); // 右上 → 右
        assert_eq!(base_dir(5), 0); // 右下 → 右
        assert_eq!(base_dir(6), 1); // 左上 → 左
        assert_eq!(base_dir(7), 1); // 左下 → 左
    }

    #[test]
    fn diagonal_sides_connect_on_half_the_edge() {
        assert_eq!(side_lay_range(0), (0.0, 1.0));
        assert_eq!(side_lay_range(3), (0.0, 1.0));
        assert_eq!(side_lay_range(4), (0.0, 0.5)); // 上半分
        assert_eq!(side_lay_range(5), (0.5, 1.0)); // 下半分
        assert_eq!(side_lay_range(6), (0.0, 0.5));
        assert_eq!(side_lay_range(7), (0.5, 1.0));
    }

    #[test]
    fn boundary_of_normalizes_diagonal_sides_to_base_dirs() {
        // 斜め配置(4-7)の端末でも、境界の辺は基の辺(0-3)として返される。
        // ここを壊すと到達判定・入り位置が x/y 軸を取り違え、切替不能になる
        for (side, want) in [(4u8, 0u8), (5, 0), (6, 1), (7, 1), (2, 2), (3, 3)] {
            let peers = [peer(side)];
            assert_eq!(
                boundary_of(&peers, 0, Some(0)),
                Some((want, None)),
                "side {side} の境界は基の辺 {want} であること"
            );
        }
    }

    #[test]
    fn boundary_of_uses_the_edge_monitor_of_the_entered_peer() {
        let mut p = peer(1);
        p.edge_monitor = Some(2);
        let peers = [p];
        assert_eq!(boundary_of(&peers, 0, Some(0)), Some((1, Some(2))));
        assert_eq!(boundary_of(&peers, 0, None), None);
    }
}

use crate::dragdrop;
use std::sync::atomic::Ordering;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_KEYBOARD, KEYEVENTF_KEYUP, VK_CONTROL, VK_LWIN, VK_MENU, VK_SHIFT,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetSystemMetrics, SendMessageW, SM_CXSCREEN, SM_CYSCREEN,
};

const INPUT_MOUSE: u32 = 0;
const MOUSEEVENTF_MOVE: u32 = 0x0001;

// ---------- Win32 直宣言(IME 制御) ----------
#[link(name = "imm32")]
unsafe extern "system" {
    /// ウィンドウのデフォルトIMEウィンドウを取得(他プロセスのウィンドウでも可)
    fn ImmGetDefaultIMEWnd(hwnd: *mut core::ffi::c_void) -> *mut core::ffi::c_void;
}

const WM_IME_CONTROL: u32 = 0x283;
const IMC_SETOPENSTATUS: usize = 0x0006;

/// フォアグラウンドウィンドウの IME を開(かな)/閉じ(英数)する。
/// キーエミュレート(VK_KANJI 等)と違い方向指定が確実。
/// IME ウィンドウが取れない場合の扱いは呼び出し側で選ぶ:
/// - 手動のかな/英数キー → 半角/全角相当のキー注入(VK_KANJI)へフォールバック。
///   押した本人の意図が明確なためトグル動作でも実用になる
/// - 切替時の自動同期(Msg::Ime)→ 何もしない。フォールバックのトグルは方向を
///   保証できないため、同期に使うと切替のたびに IME が反転し続ける
pub(crate) fn ime_set_open_impl(open: bool, allow_toggle_fallback: bool) {
    unsafe {
        let hwnd = GetForegroundWindow();
        if !hwnd.is_null() {
            // 自ウィンドウを持たないプロセスは ImmGetContext が他プロセスの
            // ウィンドウに対して null を返すため、デフォルトIMEウィンドウへ
            // WM_IME_CONTROL(IMC_SETOPENSTATUS) を送る(方向指定が確実な定番手法)
            let ime_wnd = ImmGetDefaultIMEWnd(hwnd);
            if !ime_wnd.is_null() {
                // 設定後に開閉状態を照会し、届いていなければ再送する。UIPI(昇格
                // プロセスの前面窓)等では WM_IME_CONTROL が黙って無視され得るため、
                // 「送った」だけでなく「反映された」まで確かめる
                const IMC_GETOPENSTATUS: usize = 0x0005;
                for _ in 0..3 {
                    SendMessageW(ime_wnd, WM_IME_CONTROL, IMC_SETOPENSTATUS, open as isize);
                    let actual =
                        SendMessageW(ime_wnd, WM_IME_CONTROL, IMC_GETOPENSTATUS, 0) != 0;
                    if actual == open {
                        println!("[ime] WM_IME_CONTROL open={open} -> 反映を確認");
                        return;
                    }
                }
                println!(
                    "[ime] WM_IME_CONTROL open={open} -> 反映を確認できません(前面窓の権限等)。3回送信済み"
                );
                return;
            }
            println!(
                "[ime] default IME wnd=null -> {}",
                if allow_toggle_fallback {
                    "fallback"
                } else {
                    "skip"
                }
            );
        } else {
            println!(
                "[ime] no foreground window -> {}",
                if allow_toggle_fallback {
                    "fallback"
                } else {
                    "skip"
                }
            );
        }
        if !allow_toggle_fallback {
            return;
        }
        // フォールバック: 半角/全角キー(VK_KANJI=0x19)の押し離し(トグル動作)。
        // かつて未定義の 0xF4 を使っていたが規格値ではないため修正
        inject_key(0x19, false);
        inject_key(0x19, true);
    }
}

pub(crate) fn ime_set_open(open: bool) {
    ime_set_open_impl(open, true);
}

/// cmd+Tab → Alt+Tab 変換中(Alt を保持し、cmd 離下で確定する)
pub(crate) static ALT_TAB_ACTIVE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// ⌘キーのマップ先(false=Ctrl 既定 / true=Alt)。Mac から Cfg で同期される
pub(crate) static CMD_ALT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub(crate) static BTN_W: [std::sync::atomic::AtomicBool; 3] = [
    std::sync::atomic::AtomicBool::new(false),
    std::sync::atomic::AtomicBool::new(false),
    std::sync::atomic::AtomicBool::new(false),
];
/// XButton1/2 の押下状態(離脱時の up 注入漏れ防止。BTN_W と同じ運用)
static XBTN_W: [std::sync::atomic::AtomicBool; 2] = [
    std::sync::atomic::AtomicBool::new(false),
    std::sync::atomic::AtomicBool::new(false),
];

/// 終了・再起動前の後片付け: 押下中の入力を離す。セッションの ModState は
/// トレイ経路の終了から届かないため、追跡済み static と主要修飾キーで代用する
pub(crate) fn release_all_input() {
    for b in 0u8..=2 {
        if BTN_W[b as usize].swap(false, Ordering::Relaxed) {
            inject_mouse_btn(b, false);
        }
    }
    for i in 0u8..=1 {
        if XBTN_W[i as usize].swap(false, Ordering::Relaxed) {
            inject_xbutton(i, false);
        }
    }
    // 修飾は押下追跡の外(物理キーと重なる)ため無条件 up(未押下の up は無害)
    for vk in [VK_CONTROL, VK_MENU, VK_LWIN, VK_SHIFT] {
        inject_key(vk, true);
    }
    if ALT_TAB_ACTIVE.swap(false, Ordering::Relaxed) {
        inject_key(0x09, true);
        inject_key(VK_MENU, true);
    }
}

// ---------- INPUT 手動パック(type+pad+32byte共用体=40byte) ----------
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct InputBuf {
    itype: u32,
    _pad: u32,
    body: [u32; 6],
    extra: usize,
}

fn send_input_buf(buf: InputBuf) -> bool {
    assert_eq!(
        std::mem::size_of::<InputBuf>(),
        std::mem::size_of::<INPUT>()
    );
    // UAC・ロック画面・昇格した前面窓へは直接の注入が捨てられる。操作補助へ中継する
    if crate::inputdesk::NEEDS_HELPER.load(Ordering::Relaxed) && crate::helper::send_input(&buf) {
        return true;
    }
    unsafe {
        SendInput(
            1,
            &buf as *const _ as *const INPUT,
            std::mem::size_of::<InputBuf>() as i32,
        ) == 1
    }
}

/// 拡張キー(E0 プレフィクス付き scan)の VK。scan code を併記して注入するため、
/// この区別を付けないと矢印キー等がテンキーの 4/6/8/2 と同じ scan で届く
pub(crate) fn is_extended_vk(vk: u16) -> bool {
    matches!(
        vk,
        0x21..=0x28 | 0x2D | 0x2E | 0x6F | 0x5B | 0x5C | 0xA3 | 0xA5
    )
}

/// カーソルの絶対位置設定。保護デスクトップでは操作補助へ中継する
pub(crate) fn set_cursor_pos(x: i32, y: i32) {
    if crate::inputdesk::NEEDS_HELPER.load(Ordering::Relaxed) && crate::helper::send_cursor(x, y) {
        return;
    }
    unsafe { windows_sys::Win32::UI::WindowsAndMessaging::SetCursorPos(x, y) };
}

/// Mac のテンキー入力が数字として届くよう、NumLock を ON に揃える。
/// Mac に NumLock の概念はなく、Mac 側のテンキー送信は数字前提のため、
/// Mac からこの PC を操作し始める時点(Warp 到着時)で呼ぶ。
/// NumLock OFF のままテンキーを打つと矢印・Home/End に化ける
pub(crate) fn sync_numlock_on() {
    unsafe {
        // GetKeyState の VK_NUMLOCK は下位ビットがトグル状態(1=ON)
        extern "system" {
            fn GetKeyState(n_key: i32) -> i16;
        }
        const VK_NUMLOCK: i32 = 0x90;
        if GetKeyState(VK_NUMLOCK) & 1 == 0 {
            inject_key(0x90, false);
            inject_key(0x90, true);
            println!("[input] Mac からの操作を始めるため NumLock を ON に揃えました");
        }
    }
}

pub(crate) fn inject_key(vk: u16, up: bool) -> bool {
    inject_key_ex(vk, up, is_extended_vk(vk))
}

fn inject_key_ex(vk: u16, up: bool, extended: bool) -> bool {
    // wScan を必ず付ける: 日本語 IME 等は scan code 無しのキーを無視/不安定に
    // 扱うことがある(「ー」等の OEM キーで顕著)。vk と scan の併用が最も互換性が高い
    extern "system" {
        fn MapVirtualKeyW(code: u32, map_type: u32) -> u32;
    }
    let scan = unsafe {
        MapVirtualKeyW(vk as u32, 0 /*MAPVK_VK_TO_VSC*/)
    };
    // KEYBDINPUT の共用体先頭 u32 は「低16bit=wVk / 高16bit=wScan」
    let vk_scan = ((scan & 0xFFFF) << 16) | (vk as u32 & 0xFFFF);
    send_input_buf(InputBuf {
        itype: INPUT_KEYBOARD,
        _pad: 0,
        body: [
            vk_scan,
            (if up { KEYEVENTF_KEYUP } else { 0 })
                | if extended {
                    0x0001 /*EXTENDEDKEY*/
                } else {
                    0
                },
            0,
            0,
            0,
            0,
        ],
        extra: 0,
    })
}

fn inject_mouse_move_rel(dx: i32, dy: i32) -> bool {
    send_input_buf(InputBuf {
        itype: INPUT_MOUSE,
        _pad: 0,
        body: [dx as u32, dy as u32, 0, MOUSEEVENTF_MOVE, 0, 0],
        extra: 0,
    })
}

/// 絶対位置移動(0..65535 座標、MOUSEEVENTF_VIRTUALDESK なので仮想デスクトップ
/// 全体が対象。Mac 側の WIN_CUR も hello/Screen の仮想デスクトップサイズ基準で
/// 積算しているため一致する)。MOUSEEVENTF_ABSOLUTE は Windows のポインタ加速曲線を
/// 通らないため、Mac の速度感がそのまま再現される
fn inject_mouse_move_abs(x: i32, y: i32) -> bool {
    const ABSOLUTE: u32 = 0x8000;
    const VIRTUALDESK: u32 = 0x4000;
    send_input_buf(InputBuf {
        itype: INPUT_MOUSE,
        _pad: 0,
        body: [
            x as u32,
            y as u32,
            0,
            MOUSEEVENTF_MOVE | ABSOLUTE | VIRTUALDESK,
            0,
            0,
        ],
        extra: 0,
    })
}

pub(crate) fn inject_mouse_btn(btn: u8, down: bool) -> bool {
    const LEFTDOWN: u32 = 0x0002;
    const LEFTUP: u32 = 0x0004;
    const RIGHTDOWN: u32 = 0x0008;
    const RIGHTUP: u32 = 0x0010;
    const MIDDLEDOWN: u32 = 0x0020;
    const MIDDLEUP: u32 = 0x0040;
    let flags = match (btn, down) {
        (0, true) => LEFTDOWN,
        (0, false) => LEFTUP,
        (1, true) => RIGHTDOWN,
        (1, false) => RIGHTUP,
        (2, true) => MIDDLEDOWN,
        (2, false) => MIDDLEUP,
        _ => return true,
    };
    send_input_buf(InputBuf {
        itype: INPUT_MOUSE,
        _pad: 0,
        body: [0, 0, 0, flags, 0, 0],
        extra: 0,
    })
}

/// XButton1/2(ブラウザの戻る/進む)。idx: 0=戻る, 1=進む
fn inject_xbutton(idx: u8, down: bool) -> bool {
    const XDOWN: u32 = 0x0080;
    const XUP: u32 = 0x0100;
    send_input_buf(InputBuf {
        itype: INPUT_MOUSE,
        _pad: 0,
        body: [0, 0, (idx + 1) as u32, if down { XDOWN } else { XUP }, 0, 0],
        extra: 0,
    })
}

/// Mac 由来の共有マウス移動を注入し、進行中の OLE ドラッグ(掴み越境)へも中継する。
/// SendInput の共有入力は OLE のモーダルループに届かないため、こちらで拾わせる
pub(crate) fn remote_mouse_move_rel(dx: i32, dy: i32) -> bool {
    let ok = inject_mouse_move_rel(dx, dy);
    dragdrop::relay_move();
    ok
}

/// Mac 由来の共有マウス移動(絶対位置)を注入し、進行中の OLE ドラッグへも中継する
pub(crate) fn remote_mouse_move_abs(x: i32, y: i32) -> bool {
    let ok = inject_mouse_move_abs(x, y);
    dragdrop::relay_move();
    ok
}

/// Mac 由来の共有マウスボタンを注入する。左ボタンの解放は進行中の OLE ドラッグ
/// (掴み越境)へ中継してドロップさせる
pub(crate) fn remote_mouse_button(btn: u8, down: bool) {
    if down {
        dragdrop::edge::CONTROLLED.store(true, Ordering::Relaxed);
    }
    if btn >= 3 {
        // XButton1/2(トラックパッドの戻る/進むスワイプ)。押下状態を追跡して
        // 離脱時の up 注入に含める(スワイプ中の切断で押しっぱなし残留を防ぐ)
        if (btn as usize) < 5 {
            XBTN_W[btn as usize - 3].store(down, Ordering::Relaxed);
        }
        inject_xbutton(btn - 3, down);
    } else {
        BTN_W[btn as usize].store(down, Ordering::Relaxed);
        inject_mouse_btn(btn, down);
        if btn == 0 && !down {
            dragdrop::relay_up();
        }
    }
}

pub(crate) fn inject_scroll(dx: f64, dy: f64) -> bool {
    // WHEEL_DELTA=120 を 1 として 6 units(0.05 ノッチ)刻みで注入する。
    // プレシジョンタッチパッドと同じ高解像度スクロールで、主要アプリは
    // 120 未満の delta を正しく累積するため滑らかに動く(旧: 1.0 ノッチ未満は切り捨て)
    const WHEEL: u32 = 0x0800;
    const HWHEEL: u32 = 0x1000;
    let min_units = |v: f64| (v * 120.0).round().abs() >= 1.0;
    let mut ok = true;
    if min_units(dy) {
        let amount = (-dy * 120.0).round() as i32; // 下スクロール(Mac dy負)→Winは正
        ok &= send_input_buf(InputBuf {
            itype: INPUT_MOUSE,
            _pad: 0,
            body: [0, 0, amount as u32, WHEEL, 0, 0],
            extra: 0,
        });
    }
    if min_units(dx) {
        let amount = (dx * 120.0).round() as i32;
        ok &= send_input_buf(InputBuf {
            itype: INPUT_MOUSE,
            _pad: 0,
            body: [0, 0, amount as u32, HWHEEL, 0, 0],
            extra: 0,
        });
    }
    ok
}

// ---------- 前面アプリに応じたキー配置(ターミナルでは Control → Ctrl) ----------
#[link(name = "user32")]
unsafe extern "system" {
    fn GetWindowThreadProcessId(hwnd: *mut core::ffi::c_void, pid: *mut u32) -> u32;
    fn GetCursorInfo(info: *mut CursorInfo) -> i32;
    fn GetClipCursor(rect: *mut RectL) -> i32;
    fn GetWindowRect(hwnd: *mut core::ffi::c_void, rect: *mut RectL) -> i32;
    pub fn LockWorkStation() -> i32;
}
#[link(name = "kernel32")]
unsafe extern "system" {
    fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut core::ffi::c_void;
    fn QueryFullProcessImageNameW(
        h: *mut core::ffi::c_void,
        flags: u32,
        buf: *mut u16,
        size: *mut u32,
    ) -> i32;
}
#[repr(C)]
#[derive(Default, Clone, Copy, PartialEq)]
struct RectL {
    l: i32,
    t: i32,
    r: i32,
    b: i32,
}
#[repr(C)]
struct CursorInfo {
    size: u32,
    flags: u32,
    cursor: *mut core::ffi::c_void,
    x: i32,
    y: i32,
}

/// Mac の Control を Windows の Ctrl として送るアプリ(実行ファイル名、小文字)。
/// 既定のキー配置では Control→Win キーのため、ターミナルの Ctrl+A/E/R/C が
/// Win+A 等に化けて使えない。KNIT_CTRL_APPS(カンマ区切り)で置き換えられる
fn ctrl_apps() -> &'static Vec<String> {
    static APPS: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
    APPS.get_or_init(|| {
        let list = knit_common::envutil::get("KNIT_CTRL_APPS").unwrap_or_else(|| {
            "windowsterminal.exe,cmd.exe,powershell.exe,pwsh.exe,wsl.exe,conhost.exe,mintty.exe,\
             alacritty.exe,wezterm-gui.exe,putty.exe,kitty.exe,tabby.exe,hyper.exe"
                .into()
        });
        list.split(',')
            .map(|s| s.trim().to_ascii_lowercase())
            .filter(|s| !s.is_empty())
            .collect()
    })
}

fn foreground_exe(hwnd: *mut core::ffi::c_void) -> Option<String> {
    unsafe {
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, &mut pid);
        if pid == 0 {
            return None;
        }
        let h = OpenProcess(0x1000 /*PROCESS_QUERY_LIMITED_INFORMATION*/, 0, pid);
        if h.is_null() {
            return None;
        }
        let mut buf = [0u16; 512];
        let mut n = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, 0, buf.as_mut_ptr(), &mut n);
        windows_sys::Win32::Foundation::CloseHandle(h);
        if ok == 0 {
            return None;
        }
        let path = String::from_utf16_lossy(&buf[..n as usize]);
        path.rsplit('\\').next().map(|s| s.to_ascii_lowercase())
    }
}

/// 前面がターミナル系アプリか(ウィンドウが変わった時だけ調べ直す)
fn terminal_profile() -> bool {
    static CACHE: std::sync::Mutex<(usize, bool)> = std::sync::Mutex::new((0, false));
    let hwnd = unsafe { GetForegroundWindow() };
    let mut c = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if c.0 != hwnd as usize {
        let exe = foreground_exe(hwnd);
        let on = exe.as_ref().is_some_and(|e| ctrl_apps().contains(e));
        if on != c.1 {
            println!(
                "[keys] 前面 {:?}: Control → {}",
                exe.as_deref().unwrap_or("?"),
                if on { "Ctrl" } else { "Win" }
            );
        }
        *c = (hwnd as usize, on);
    }
    c.1
}

/// ゲームモード判定: カーソルが画面の一部に閉じ込められている(ClipCursor)、または
/// 全画面ウィンドウでカーソルが隠れ続けている。絶対座標の注入では FPS や 3D ソフトの
/// 視点回転が効かないため、この間は Mac に相対移動で送ってもらう
pub(crate) fn game_like() -> bool {
    unsafe {
        let (vx, vy, vw, vh) = vscreen();
        let mut clip = RectL::default();
        let clipped = GetClipCursor(&mut clip) != 0
            && clip
                != RectL {
                    l: vx,
                    t: vy,
                    r: vx + vw,
                    b: vy + vh,
                }
            && (clip.r - clip.l) < vw;
        if clipped {
            return true;
        }
        let mut ci = CursorInfo {
            size: std::mem::size_of::<CursorInfo>() as u32,
            flags: 0,
            cursor: std::ptr::null_mut(),
            x: 0,
            y: 0,
        };
        let hidden = GetCursorInfo(&mut ci) != 0 && ci.flags & 1 /*CURSOR_SHOWING*/ == 0;
        if !hidden {
            return false;
        }
        let fg = GetForegroundWindow();
        let mut r = RectL::default();
        GetWindowRect(fg, &mut r) != 0
            && (r.r - r.l) >= GetSystemMetrics(SM_CXSCREEN)
            && (r.b - r.t) >= GetSystemMetrics(SM_CYSCREEN)
    }
}

// ---------- 修飾キー状態管理(Mac mods → Win VK) ----------
/// 右 Ctrl(拡張キー)。Mac の右⌘(rcmd フラグ)の割当先
const VK_RCONTROL: u16 = 0xA3;

pub(crate) struct ModState {
    ctrl: bool,
    alt: bool,
    win: bool,
    shift: bool,
    rcmd: bool,
    /// 注入して押下中のキー((vk, 拡張))。離脱・切断時に全部 up を注入する
    /// (修飾だけ解放していた旧実装では、切替の瞬間に押していた矢印キー等が
    /// Windows 側で押下扱いのまま残った)
    pressed: Vec<(u16, bool)>,
}
impl ModState {
    pub(crate) fn new() -> Self {
        Self {
            ctrl: false,
            alt: false,
            win: false,
            shift: false,
            rcmd: false,
            pressed: Vec::new(),
        }
    }
    /// 通常キーの注入(押下状態を追跡する)
    pub(crate) fn key(&mut self, vk: u16, down: bool, extended: bool) -> bool {
        self.pressed.retain(|&(v, e)| !(v == vk && e == extended));
        if down {
            self.pressed.push((vk, extended));
        }
        inject_key_ex(vk, !down, extended)
    }
    /// 離脱時の後片付け: 押下中の通常キー・修飾・マウスボタン・Alt+Tab を全て離す
    pub(crate) fn release_everything(&mut self) {
        for (vk, ext) in std::mem::take(&mut self.pressed) {
            inject_key_ex(vk, true, ext);
        }
        self.release_all();
        for b in 0u8..=2 {
            if BTN_W[b as usize].swap(false, Ordering::Relaxed) {
                inject_mouse_btn(b, false);
            }
        }
        for i in 0u8..=1 {
            if XBTN_W[i as usize].swap(false, Ordering::Relaxed) {
                inject_xbutton(i, false);
            }
        }
        if ALT_TAB_ACTIVE.swap(false, Ordering::Relaxed) {
            inject_key(0x09, true);
            inject_key(VK_MENU, true);
        }
    }
    pub(crate) fn apply(&mut self, ctrl: bool, opt: bool, cmd: bool, shift: bool, rcmd: bool) {
        // Mac: cmd→Win Ctrl(既定)/ Alt(Cfg で切替可), option→もう一方, ctrl→Winキー, shift→Shift
        let (cmd_vk, opt_vk) = if CMD_ALT.load(Ordering::Relaxed) {
            (VK_MENU, VK_CONTROL)
        } else {
            (VK_CONTROL, VK_MENU)
        };
        // ターミナル系アプリでは Mac の Control を Windows の Ctrl として送る
        let ctrl_vk = if ctrl && terminal_profile() {
            VK_CONTROL
        } else {
            VK_LWIN
        };
        let want = [
            (cmd_vk, cmd),
            (opt_vk, opt),
            (ctrl_vk, ctrl),
            (VK_SHIFT, shift),
            // 右⌘は常に右 Ctrl(VK_RCONTROL・拡張)。CMD_ALT の影響も受けない
            (VK_RCONTROL, rcmd),
        ];
        let mut state = [
            (VK_CONTROL, &mut self.ctrl),
            (VK_MENU, &mut self.alt),
            (VK_LWIN, &mut self.win),
            (VK_SHIFT, &mut self.shift),
            (VK_RCONTROL, &mut self.rcmd),
        ];
        for (vk, cur) in &mut state {
            // 複数の Mac 修飾が同じ VK に対応し得るため、いずれかが押されていれば押下
            let target = want.iter().any(|(v, t)| *v == *vk && *t);
            if **cur != target {
                inject_key(*vk, !target);
            }
            **cur = target;
        }
    }
    fn release_all(&mut self) {
        self.apply(false, false, false, false, false);
    }
}

/// 仮想デスクトップ(全モニターを合わせた領域)の (x, y, w, h)。プライマリ画面だけを
/// 前提にすると、2 枚目以降のモニターへカーソルを動かせず端の判定もずれる
static VSCREEN: std::sync::Mutex<(i32, i32, i32, i32)> = std::sync::Mutex::new((0, 0, 1920, 1080));

pub(crate) fn vscreen() -> (i32, i32, i32, i32) {
    *VSCREEN.lock().unwrap_or_else(|e| e.into_inner())
}

/// 自環境の全モニターを仮想画面座標系で列挙する(接続先へ自動通知)。
/// モニターの増減は接続の再確立時に相手へ反映される
pub(crate) fn list_monitors() -> Vec<knit_common::proto::Monitor> {
    /// モニターの機種名。ドライバ未導入の汎用名は区別の役に立たないので空にする
    unsafe fn monitor_name(hm: *mut std::ffi::c_void) -> String {
        use windows_sys::Win32::Graphics::Gdi::{
            EnumDisplayDevicesW, GetMonitorInfoW, DISPLAY_DEVICEW, MONITORINFOEXW,
        };
        let mut mi: MONITORINFOEXW = std::mem::zeroed();
        mi.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
        if GetMonitorInfoW(hm, &mut mi as *mut _ as *mut _) == 0 {
            return String::new();
        }
        let mut dd: DISPLAY_DEVICEW = std::mem::zeroed();
        dd.cb = std::mem::size_of::<DISPLAY_DEVICEW>() as u32;
        if EnumDisplayDevicesW(mi.szDevice.as_ptr(), 0, &mut dd, 0) == 0 {
            return String::new();
        }
        let len = dd.DeviceString.iter().position(|&c| c == 0).unwrap_or(dd.DeviceString.len());
        let name = String::from_utf16_lossy(&dd.DeviceString[..len]);
        if name.contains("PnP") {
            String::new()
        } else {
            name
        }
    }
    unsafe extern "system" fn cb(
        hm: *mut std::ffi::c_void,
        _hdc: *mut std::ffi::c_void,
        rect: *mut windows_sys::Win32::Foundation::RECT,
        ctx: isize,
    ) -> i32 {
        let out = unsafe { &mut *(ctx as *mut Vec<knit_common::proto::Monitor>) };
        let r = unsafe { &*rect };
        out.push(knit_common::proto::Monitor {
            x: r.left,
            y: r.top,
            w: r.right - r.left,
            h: r.bottom - r.top,
            name: unsafe { monitor_name(hm) },
        });
        1
    }
    let mut out: Vec<knit_common::proto::Monitor> = Vec::new();
    unsafe {
        windows_sys::Win32::Graphics::Gdi::EnumDisplayMonitors(
            std::ptr::null_mut(),
            std::ptr::null(),
            Some(cb),
            std::ptr::addr_of_mut!(out) as isize,
        );
    }
    out
}

/// 仮想デスクトップの範囲を取り直す(変化があれば true を返せるよう値を返す)
pub(crate) fn refresh_vscreen() -> (i32, i32, i32, i32) {
    const SM_XVIRTUALSCREEN: i32 = 76;
    const SM_YVIRTUALSCREEN: i32 = 77;
    const SM_CXVIRTUALSCREEN: i32 = 78;
    const SM_CYVIRTUALSCREEN: i32 = 79;
    let v = unsafe {
        let (w, h) = (
            GetSystemMetrics(SM_CXVIRTUALSCREEN),
            GetSystemMetrics(SM_CYVIRTUALSCREEN),
        );
        if w > 0 && h > 0 {
            (
                GetSystemMetrics(SM_XVIRTUALSCREEN),
                GetSystemMetrics(SM_YVIRTUALSCREEN),
                w,
                h,
            )
        } else {
            (
                0,
                0,
                GetSystemMetrics(SM_CXSCREEN),
                GetSystemMetrics(SM_CYSCREEN),
            )
        }
    };
    *VSCREEN.lock().unwrap_or_else(|e| e.into_inner()) = v;
    v
}

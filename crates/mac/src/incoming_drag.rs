//! Windowsで準備されたファイルを、Macの標準ドラッグとして引き継ぐ。
use crate::*;
use knit_common::drag::Incoming;
use std::sync::atomic::{AtomicBool, AtomicUsize};

static INCOMING: Mutex<Incoming> = Mutex::new(Incoming::new());
/// 受け取り待ちの (id, 合計バイト, 立てた時刻)。**転送が終わるまで待たされる間**に
/// 手元へ進捗を出すため(Win→Mac は相手がドラッグを持っており、こちらは到着まで待つ)
static WAITING: Mutex<Option<(u64, u64, u64)>> = Mutex::new(None);


/// **停滞**の上限。進捗が来なくなってからこの時間で表示を打ち切る
/// (フレーム段階で拒否されると Files イベントもキャンセルも来ず、進捗だけが残り得る)。
/// 「offer からの経過」で切ると、正当に長い転送(1GB=34秒・10GB=5分41秒)の表示まで
/// 消えてしまうため、**最後に進捗を見た時刻**を基準にする
const WAITING_STALL_MS: u64 = 15_000;
/// どれだけ進捗が続いても、この時間を超えたら表示を打ち切る(異常の保険)
const WAITING_HARD_MS: u64 = 15 * 60 * 1000;
/// 最後に進捗を見た時刻(offer 受理時と、受信のたびに更新)
static WAITING_SEEN_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// 受信の進捗があったことを伝える(受信フックから)。表示の期限を延ばす
pub fn note_progress_seen() {
    if WAITING_SEEN_MS.load(Ordering::Relaxed) != 0 {
        WAITING_SEEN_MS.store(now_ms(), Ordering::Relaxed);
    }
}

/// 待ちが生きているか(純関数)。**停滞**か**全体の上限**で切る
fn waiting_alive(seen_ms: u64, started_ms: u64, now: u64) -> bool {
    if seen_ms == 0 {
        return false; // 未設定
    }
    now.saturating_sub(seen_ms) <= WAITING_STALL_MS && now.saturating_sub(started_ms) <= WAITING_HARD_MS
}

/// 受け取り待ちか(データ到着を待っている最中だけ Some)。期限切れは消して None
pub fn waiting_for_data() -> Option<(u64, u64)> {
    let mut g = WAITING.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((id, total, started)) = *g {
        let seen = WAITING_SEEN_MS.load(Ordering::Relaxed);
        if !waiting_alive(seen, started, now_ms()) {
            eprintln!("[drag] 受け取り待ちが停滞したため表示を打ち切ります({id})");
            *g = None;
            WAITING_SEEN_MS.store(0, Ordering::Relaxed);
            return None;
        }
        return Some((id, total));
    }
    None
}

fn clear_waiting(id: u64) {
    let mut g = WAITING.lock().unwrap_or_else(|e| e.into_inner());
    if g.as_ref().is_some_and(|(cur, _, _)| *cur == id) {
        *g = None;
        WAITING_SEEN_MS.store(0, Ordering::Relaxed);
    }
}
static COMMIT: Mutex<Option<u64>> = Mutex::new(None);
static AVAILABLE: AtomicBool = AtomicBool::new(false);
static SOURCE_CLASS: AtomicUsize = AtomicUsize::new(0);
static ACTIVE: Mutex<Option<Active>> = Mutex::new(None);
static FINISHED: Mutex<Vec<Active>> = Mutex::new(Vec::new());
// 終了を要求してもAppKitの終了通知まではsourceを生かす。
static RETIRED: Mutex<Vec<Retired>> = Mutex::new(Vec::new());
struct Active {
    id: u64,
    window: usize,
    source: usize,
    began_ms: u64,
}
/// RETIRED へ退避した1セッション分。retired_ms は退避した時刻(単調 ms)で、
/// ended が来ないまま残ったエントリの保険回収(poll)の期限判定に使う
struct Retired {
    active: Active,
    /// window を orderOut 済みか(初回の poll で隠す)
    hidden: bool,
    retired_ms: u64,
}

/// RETIRED への退避から回収期限が切れたか(poll の判定用の純関数)。
/// true を返したエントリは source/window を release して除去する
fn retired_expired(retired_ms: u64, now: u64) -> bool {
    const RETIRED_TIMEOUT_MS: u64 = 60_000;
    now.saturating_sub(retired_ms) > RETIRED_TIMEOUT_MS
}

/// 受信ドラッグ(NSDraggingSession)が進行中か。進行中は Mac→Win の境界
/// 自動切替と掴み検出を止める。物理ボタンは押されたまま Mac 側のドロップを
/// 続けるため、ここで切替すると入力が Windows へ転送され、セッションが
/// マウスアップを検知できず ended もドロップも来なくなる(実測)。
/// 解除は ended コールバックと、ボタン解放後の異常残骸回収(poll)で行う
pub fn blocking() -> bool {
    ACTIVE.lock().unwrap_or_else(|e| e.into_inner()).is_some()
}

unsafe extern "C" {
    fn objc_allocateClassPair(superclass: ID, name: *const std::ffi::c_char, extra: usize) -> ID;
    fn objc_registerClassPair(class: ID);
    fn class_addMethod(class: ID, name: SEL, imp: usize, types: *const std::ffi::c_char) -> u8;
    fn objc_getProtocol(name: *const std::ffi::c_char) -> ID;
    fn class_addProtocol(class: ID, protocol: ID) -> u8;
}

fn delete_received(files: Vec<std::path::PathBuf>) {
    for path in files {
        // 受信した空フォルダ(版 15)はディレクトリとして届くため、ファイルの
        // 削除に失敗したらフォルダ削除で保険する(bulk 切断時の片付けと同じ扱い)
        if std::fs::remove_file(&path).is_err() {
            let _ = std::fs::remove_dir_all(&path);
        }
    }
}

pub fn offer(id: u64, count: usize, total: u64, position: f64) {
    // 共有範囲(ファイル)が閉じているときは受信を即拒否する。オファーだけを通すと
    // 転送本体が共有範囲のフレーム検査で全て捨てられ、押下の間ずっと無音の待ちになる
    let accepted = AVAILABLE.load(Ordering::Relaxed)
        && WIN_MODE.load(Ordering::Relaxed)
        && BTN_DOWN[0].load(Ordering::Relaxed)
        && !HOTKEY_ONLY.load(Ordering::Relaxed)
        && knit_common::share::allow_files()
        && total <= bulk::MAX_TOTAL
        && crate::conn::active_screen_settings().is_none_or(|s| crate::screen_settings::contains(s.target, position))
        && INCOMING
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .offer(id, count, position);
    if accepted {
        let now = now_ms();
        *WAITING.lock().unwrap_or_else(|e| e.into_inner()) = Some((id, total, now));
        WAITING_SEEN_MS.store(now, Ordering::Relaxed);
        // 前のバッチの受信量で描かないよう、進捗もここで 0 に戻す
        crate::files::note_rx_begin();
    }
    send_msg(&if accepted {
        Msg::DragAccept { id }
    } else {
        Msg::DragCancel { id }
    });
    eprintln!(
        "[drag] win->mac offer {id}: {}",
        if accepted { "accepted" } else { "rejected" }
    );
}

/// テスト専用: 待ち状態を差し込む(offer は接続状態など多くの前提を要するため)
#[cfg(test)]
pub(crate) fn set_waiting_for_test(id: u64, total: u64) {
    let now = now_ms();
    *WAITING.lock().unwrap_or_else(|e| e.into_inner()) = Some((id, total, now));
    // 生きている待ちを模す(期限のテストは、この後に時刻を古くする)
    WAITING_SEEN_MS.store(now, Ordering::Relaxed);
}

pub fn receive(id: u64, paths: Vec<std::path::PathBuf>) {
    let result = INCOMING
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .receive(id, paths);
    match result {
        Ok(()) => {
            eprintln!("[drag] win->mac ready {id}");
            clear_waiting(id); // 到着したので待ちの表示を終える
            send_msg(&Msg::DragReady { id });
        }
        Err(paths) => {
            delete_received(paths);
            cancel(id);
            send_msg(&Msg::DragCancel { id });
        }
    }
}

pub fn commit(id: u64) {
    *COMMIT.lock().unwrap_or_else(|e| e.into_inner()) = Some(id);
}

pub fn cancel(id: u64) {
    clear_waiting(id);
    let paths = INCOMING
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .cancel(id);
    delete_received(paths);
    let mut queued = COMMIT.lock().unwrap_or_else(|e| e.into_inner());
    if *queued == Some(id) {
        *queued = None;
    }
}

pub fn reset() {
    *WAITING.lock().unwrap_or_else(|e| e.into_inner()) = None;
    WAITING_SEEN_MS.store(0, Ordering::Relaxed);
    *COMMIT.lock().unwrap_or_else(|e| e.into_inner()) = None;
    delete_received(INCOMING.lock().unwrap_or_else(|e| e.into_inner()).reset());
    // 接続が切れたら進行中のセッションも終わらせ、境界切替の抑制を解く。
    // AppKit操作はmainスレッドへ渡し、遅れる終了通知までsourceを保持する。
    if let Some(active) = ACTIVE.lock().unwrap_or_else(|e| e.into_inner()).take() {
        let id = active.id;
        RETIRED.lock().unwrap_or_else(|e| e.into_inner()).push(Retired {
            active,
            hidden: false,
            retired_ms: now_ms(),
        });
        send_msg(&Msg::DragDone {
            id,
            copied: false,
        });
        eprintln!("[drag] Mac drag {id} ended(disconnect): cleanup queued");
    }
}

unsafe fn hide_window(window: usize) {
    let hide: unsafe extern "C" fn(ID, SEL, ID) = std::mem::transmute(objc_msgSend as *const ());
    hide(
        window as ID,
        sel_registerName(c"orderOut:".as_ptr()),
        std::ptr::null_mut(),
    );
}

unsafe extern "C" fn operation(_this: ID, _sel: SEL, _session: ID, _context: isize) -> usize {
    1
}
unsafe extern "C" fn ignore_modifiers(_this: ID, _sel: SEL, _session: ID) -> u8 {
    1
}
unsafe extern "C" fn ended(_this: ID, _sel: SEL, _session: ID, _point: CGPoint, result: usize) {
    let active = {
        let mut active = ACTIVE.lock().unwrap_or_else(|e| e.into_inner());
        if active.as_ref().is_some_and(|a| a.source == _this as usize) { active.take() } else { None }
    };
    if let Some(active) = active {
        unsafe { hide_window(active.window) };
        INCOMING
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .cancel(active.id);
        send_msg(&Msg::DragDone {
            id: active.id,
            copied: result & 1 != 0,
        });
        eprintln!(
            "[drag] Mac drag {} ended: copied={}",
            active.id,
            result & 1 != 0
        );
        // このコールバックを呼んでいるAppKitが戻ってからsourceを解放する。
        FINISHED.lock().unwrap_or_else(|e| e.into_inner()).push(active);
    } else {
        // 古いセッションの通知は、新しいACTIVEを終了させない。
        let mut retired = RETIRED.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(index) = retired.iter().position(|r| r.active.source == _this as usize) {
            let removed = retired.remove(index);
            FINISHED.lock().unwrap_or_else(|e| e.into_inner()).push(removed.active);
        }
    }
}

unsafe fn register_source() -> ID {
    let known = SOURCE_CLASS.load(Ordering::Relaxed);
    if known != 0 {
        return known as ID;
    }
    let class = objc_allocateClassPair(
        objc_getClass(c"NSObject".as_ptr()),
        c"KnitIncomingFileDrag".as_ptr(),
        0,
    );
    if class.is_null() {
        return class;
    }
    let protocol = objc_getProtocol(c"NSDraggingSource".as_ptr());
    if protocol.is_null() || class_addProtocol(class, protocol) == 0 {
        return std::ptr::null_mut();
    }
    for (name, imp, types) in [
        (
            c"draggingSession:sourceOperationMaskForDraggingContext:",
            operation as *const () as usize,
            c"Q@:@q",
        ),
        (
            c"ignoreModifierKeysForDraggingSession:",
            ignore_modifiers as *const () as usize,
            c"c@:@",
        ),
        (
            c"draggingSession:endedAtPoint:operation:",
            ended as *const () as usize,
            c"v@:@{CGPoint=dd}Q",
        ),
    ] {
        if class_addMethod(class, sel_registerName(name.as_ptr()), imp, types.as_ptr()) == 0 {
            return std::ptr::null_mut();
        }
    }
    objc_registerClassPair(class);
    SOURCE_CLASS.store(class as usize, Ordering::Relaxed);
    class
}

unsafe fn set_bool(object: ID, name: &std::ffi::CStr, value: bool) {
    let f: unsafe extern "C" fn(ID, SEL, u8) = std::mem::transmute(objc_msgSend as *const ());
    f(object, sel_registerName(name.as_ptr()), value as u8);
}

unsafe fn begin(id: u64, paths: &[std::path::PathBuf], probe: bool) -> bool {
    let class = register_source();
    if class.is_null() || paths.is_empty() {
        return false;
    }
    let point_fn: unsafe extern "C" fn(ID, SEL) -> CGPoint =
        std::mem::transmute(objc_msgSend as *const ());
    let point = point_fn(
        objc_getClass(c"NSEvent".as_ptr()),
        sel_registerName(c"mouseLocation".as_ptr()),
    );
    let init: unsafe extern "C" fn(ID, SEL, CGRect, usize, usize, u8) -> ID =
        std::mem::transmute(objc_msgSend as *const ());
    let panel = init(
        msg0(
            objc_getClass(c"NSPanel".as_ptr()),
            sel_registerName(c"alloc".as_ptr()),
        ),
        sel_registerName(c"initWithContentRect:styleMask:backing:defer:".as_ptr()),
        CGRect {
            origin: CGPoint {
                x: point.x - 16.0,
                y: point.y - 16.0,
            },
            size: CGSize { w: 32.0, h: 32.0 },
        },
        128,
        2,
        0,
    );
    if panel.is_null() {
        return false;
    }
    set_bool(panel, c"setReleasedWhenClosed:", false);
    set_bool(panel, c"setOpaque:", false);
    set_bool(panel, c"setIgnoresMouseEvents:", true);
    let color = msg0(
        objc_getClass(c"NSColor".as_ptr()),
        sel_registerName(c"clearColor".as_ptr()),
    );
    msg1_id(
        panel,
        sel_registerName(c"setBackgroundColor:".as_ptr()),
        color,
    );
    let integer: unsafe extern "C" fn(ID, SEL, isize) =
        std::mem::transmute(objc_msgSend as *const ());
    integer(panel, sel_registerName(c"setLevel:".as_ptr()), 3);
    let items = msg0(
        objc_getClass(c"NSMutableArray".as_ptr()),
        sel_registerName(c"array".as_ptr()),
    );
    let workspace = msg0(
        objc_getClass(c"NSWorkspace".as_ptr()),
        sel_registerName(c"sharedWorkspace".as_ptr()),
    );
    for (index, path) in paths.iter().enumerate() {
        let name = nsstring(&path.to_string_lossy());
        let url = msg1_id(
            objc_getClass(c"NSURL".as_ptr()),
            sel_registerName(c"fileURLWithPath:".as_ptr()),
            name,
        );
        let item = msg1_id(
            msg0(
                objc_getClass(c"NSDraggingItem".as_ptr()),
                sel_registerName(c"alloc".as_ptr()),
            ),
            sel_registerName(c"initWithPasteboardWriter:".as_ptr()),
            url,
        );
        let icon = msg1_id(workspace, sel_registerName(c"iconForFile:".as_ptr()), name);
        let frame: unsafe extern "C" fn(ID, SEL, CGRect, ID) =
            std::mem::transmute(objc_msgSend as *const ());
        let offset = (index.min(4) * 4) as f64;
        frame(
            item,
            sel_registerName(c"setDraggingFrame:contents:".as_ptr()),
            CGRect {
                origin: CGPoint {
                    x: offset,
                    y: offset,
                },
                size: CGSize { w: 32.0, h: 32.0 },
            },
            icon,
        );
        msg1_id(items, sel_registerName(c"addObject:".as_ptr()), item);
        msg0(item, sel_registerName(c"release".as_ptr()));
    }
    let event_fn: unsafe extern "C" fn(
        ID,
        SEL,
        usize,
        CGPoint,
        usize,
        f64,
        isize,
        ID,
        isize,
        isize,
        f32,
    ) -> ID = std::mem::transmute(objc_msgSend as *const ());
    let info = msg0(
        objc_getClass(c"NSProcessInfo".as_ptr()),
        sel_registerName(c"processInfo".as_ptr()),
    );
    let uptime_fn: unsafe extern "C" fn(ID, SEL) -> f64 =
        std::mem::transmute(objc_msgSend as *const ());
    let event = event_fn(objc_getClass(c"NSEvent".as_ptr()),sel_registerName(c"mouseEventWithType:location:modifierFlags:timestamp:windowNumber:context:eventNumber:clickCount:pressure:".as_ptr()),1,CGPoint { x: 16.0,y: 16.0 },0,uptime_fn(info,sel_registerName(c"systemUptime".as_ptr())),msg0_isize(panel,sel_registerName(c"windowNumber".as_ptr())),std::ptr::null_mut(),0,1,1.0);
    let source = msg0(class, sel_registerName(c"new".as_ptr()));
    if probe {
        assert!(!event.is_null() && !source.is_null());
        assert_eq!(msg0_isize(event, sel_registerName(c"type".as_ptr())), 1);
        assert_eq!(
            msg0_isize(items, sel_registerName(c"count".as_ptr())),
            paths.len() as isize
        );
        assert_eq!(
            operation(source, std::ptr::null_mut(), std::ptr::null_mut(), 0),
            1
        );
        msg0(source, sel_registerName(c"release".as_ptr()));
        msg0(panel, sel_registerName(c"release".as_ptr()));
        eprintln!(
            "[drag-probe] AppKit source, hidden panel, mouse-down event, file URLs and icons: OK"
        );
        return true;
    }
    *ACTIVE.lock().unwrap_or_else(|e| e.into_inner()) = Some(Active {
        id,
        window: panel as usize,
        source: source as usize,
        began_ms: now_ms(),
    });
    msg0(panel, sel_registerName(c"orderFrontRegardless".as_ptr()));
    let view = msg0(panel, sel_registerName(c"contentView".as_ptr()));
    let start: unsafe extern "C" fn(ID, SEL, ID, ID, ID) -> ID =
        std::mem::transmute(objc_msgSend as *const ());
    let session = start(
        view,
        sel_registerName(c"beginDraggingSessionWithItems:event:source:".as_ptr()),
        items,
        event,
        source,
    );
    if session.is_null() {
        ended(source, std::ptr::null_mut(), session, point, 0);
        return false;
    }
    set_bool(
        session,
        c"setAnimatesToStartingPositionsOnCancelOrFail:",
        false,
    );
    eprintln!("[drag] Mac native drag {id} started: {} files", paths.len());
    true
}

pub unsafe extern "C" fn poll(_this: ID, _sel: SEL, _timer: ID) {
    AVAILABLE.store(true, Ordering::Relaxed);
    let hide = {
        let mut retired = RETIRED.lock().unwrap_or_else(|e| e.into_inner());
        let mut windows = Vec::new();
        for r in retired.iter_mut() {
            if !r.hidden {
                windows.push(r.active.window);
                r.hidden = true;
            }
        }
        windows
    };
    // orderOutが終了通知を同期的に呼んでも、RETIREDを二重ロックしない。
    for window in hide { hide_window(window); }
    let finished = std::mem::take(&mut *FINISHED.lock().unwrap_or_else(|e| e.into_inner()));
    for finished in finished {
        hide_window(finished.window);
        msg0(finished.source as ID, sel_registerName(c"release".as_ptr()));
        msg0(finished.window as ID, sel_registerName(c"release".as_ptr()));
    }
    // ended が来ないまま RETIRED に残ったセッションの保険回収: 退避から 60 秒
    // 経っても AppKit の終了通知が来なければ、source/window を release して
    // 除去する(生ポインタ未解放の恒久残留を防ぐ)。0 はテスト等のダミーのため
    // release しない。遅れて ended が来ても RETIRED に無いため FINISHED へは
    // 積まれず、二重 release にはならない(ended は ACTIVE/RETIRED の照合で
    // 該当が無ければ何もしない)
    let expired = {
        let mut retired = RETIRED.lock().unwrap_or_else(|e| e.into_inner());
        let now = now_ms();
        let mut expired = Vec::new();
        retired.retain(|r| {
            if retired_expired(r.retired_ms, now) {
                expired.push((r.active.id, r.active.source, r.active.window));
                false
            } else {
                true
            }
        });
        expired
    };
    for (id, source, window) in expired {
        if source != 0 { msg0(source as ID, sel_registerName(c"release".as_ptr())); }
        if window != 0 { msg0(window as ID, sel_registerName(c"release".as_ptr())); }
        eprintln!("[drag] Mac drag {id} ended(expired): 終了通知が来ないため退避から60秒で回収");
    }
    // ended が来ない異常残骸の回収。物理ボタンが解放されたのに一定時間
    // 経ってもセッションが終わらなければ、掴み切替を妨げ続けないよう
    // ここで終わらせる。掴んでいる間はセッションが生きているものとして
    // 抑制を維持する(ドロップ操作を優先)。AppKitが遅れて ended を呼んでも
    // ACTIVE は空なので二重処理にはならない
    let stale = {
        let active = ACTIVE.lock().unwrap_or_else(|e| e.into_inner());
        active.as_ref().is_some_and(|a| {
            !BTN_DOWN[0].load(Ordering::Relaxed) && now_ms().saturating_sub(a.began_ms) > 2_000
        })
    };
    if stale {
        let active = ACTIVE.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(active) = active {
            let (id, window) = (active.id, active.window);
            RETIRED.lock().unwrap_or_else(|e| e.into_inner()).push(Retired {
                active,
                hidden: true,
                retired_ms: now_ms(),
            });
            hide_window(window);
            INCOMING
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .cancel(id);
            send_msg(&Msg::DragDone {
                id,
                copied: false,
            });
            eprintln!(
                "[drag] Mac drag {} ended(stale): AppKitの終了通知が来ないため回収",
                id
            );
        }
    }
    let id = COMMIT.lock().unwrap_or_else(|e| e.into_inner()).take();
    let Some(id) = id else { return };
    if !WIN_MODE.load(Ordering::Relaxed) || !BTN_DOWN[0].load(Ordering::Relaxed) {
        cancel(id);
        send_msg(&Msg::DragCancel { id });
        return;
    }
    let prepared = INCOMING
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .commit(id);
    let Some((paths, position)) = prepared else {
        cancel(id);
        send_msg(&Msg::DragCancel { id });
        return;
    };
    if !crate::tap::return_from_peer(position) {
        delete_received(paths);
        cancel(id);
        send_msg(&Msg::DragCancel { id });
        return;
    }
    // 受信ドラッグ自身がドラッグ用ペーストボードへファイルを載せるため、
    // Windows側掴みの検出基準と混ざって偽の「掴み検出」を起こす(実測)。
    // この押下の掴み検出はここで確定して終わらせる
    FILE_DRAG.lock().unwrap_or_else(|e| e.into_inner()).end();
    if !begin(id, &paths, false) {
        cancel(id);
        send_msg(&Msg::DragCancel { id });
        notify(
            "Knit",
            "Macのドラッグを開始できませんでした。受信ファイルはDownloads/Knitに保存されています",
        );
    }
}

#[cfg(debug_assertions)]
pub fn probe() {
    let path = std::env::temp_dir().join(format!("knit-drag-appkit-{}.txt", std::process::id()));
    std::fs::write(&path, b"AppKit drag construction probe").unwrap();
    with_pool(|| unsafe {
        msg0(
            objc_getClass(c"NSApplication".as_ptr()),
            sel_registerName(c"sharedApplication".as_ptr()),
        );
        assert!(begin(0, std::slice::from_ref(&path), true));
    });
    std::fs::remove_file(path).unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disconnect_keeps_native_resources_for_main_thread_cleanup() {
        *ACTIVE.lock().unwrap() = Some(Active { id: 77, window: 0, source: 0, began_ms: 0 });
        reset();
        assert!(ACTIVE.lock().unwrap().is_none());
        assert_eq!(RETIRED.lock().unwrap().len(), 1);
        assert!(FINISHED.lock().unwrap().is_empty());
        *ACTIVE.lock().unwrap() = Some(Active { id: 78, window: 0, source: 99, began_ms: 0 });
        unsafe { ended(std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut(), CGPoint { x: 0.0, y: 0.0 }, 0); }
        assert_eq!(ACTIVE.lock().unwrap().as_ref().unwrap().id, 78);
        assert!(RETIRED.lock().unwrap().is_empty());
        let finished = std::mem::take(&mut *FINISHED.lock().unwrap());
        assert_eq!(finished.len(), 1);
        assert_eq!(finished[0].id, 77);
        ACTIVE.lock().unwrap().take();
    }

    /// RETIRED へ退避してからの期限判定: 60 秒を超えたら回収対象。
    /// ダミー構造(実ポインタなし)で判定だけを確かめる
    #[test]
    fn retired_entries_expire_sixty_seconds_after_retirement() {
        let now = 10_000_000u64;
        assert!(!retired_expired(now, now), "退避直後は回収しない");
        assert!(
            !retired_expired(now - 60_000, now),
            "ちょうど 60 秒ではまだ回収しない(境界)"
        );
        assert!(
            retired_expired(now - 60_001, now),
            "60 秒超で回収対象"
        );
        // retired_ms が now より未来(時計の巻き戻り相当)でも飽和して 0 扱いに
        // なり、誤回収しない
        assert!(!retired_expired(now + 5_000, now));
    }
}

#[cfg(test)]
mod waiting_tests {
    use super::*;

    /// 待ち状態はプロセス共有の static。テストは直列に走らせる
    static WAIT_TEST_LOCK: Mutex<()> = Mutex::new(());
    fn lock() -> std::sync::MutexGuard<'static, ()> {
        WAIT_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// **停滞**で消える(相手がハングしても進捗が残り続けない)。
    /// 一方、進捗が続いている間は長い転送でも切れない(1GB=34秒・10GB=5分41秒 を想定)
    #[test]
    fn a_silent_wait_expires_but_progress_keeps_it_alive() {
        let _g = lock();
        let start = 1_000;
        assert!(waiting_alive(start, start, start), "始まったばかりは生きている");
        assert!(
            waiting_alive(start + 300_000, start, start + 300_000),
            "進捗が続く限り 5 分でも生きている(長い転送を切らない)"
        );
        assert!(
            !waiting_alive(start, start, start + WAITING_STALL_MS + 1),
            "停滞したら消える"
        );
        assert!(
            !waiting_alive(start + WAITING_HARD_MS + 1, start, start + WAITING_HARD_MS + 1),
            "全体の上限を超えたら消える"
        );
        assert!(!waiting_alive(0, 0, 99_999), "未設定は生きていない");
    }

    /// 配線: 期限切れの待ちは `waiting_for_data` が消す(検証役の変異 m4)
    #[test]
    fn an_expired_wait_is_cleared_by_the_reader() {
        let _g = lock();
        set_waiting_for_test(1, 10);
        WAITING_SEEN_MS.store(now_ms().saturating_sub(WAITING_STALL_MS + 1_000), Ordering::Relaxed);
        assert!(waiting_for_data().is_none(), "停滞した待ちは見せない");
        assert!(waiting_for_data().is_none(), "消えたままになる");
    }

    /// 配線: 到着(`receive`)で待ちが消える(検証役の変異 m5)。
    /// 予告と同じIDで件数一致のファイルが届く = 実際の到着経路
    #[test]
    fn receiving_clears_the_waiting_state() {
        let _g = lock();
        assert!(
            INCOMING
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .offer(99, 1, 0.0),
            "予告を作れる"
        );
        set_waiting_for_test(99, 10);
        assert!(waiting_for_data().is_some());
        receive(99, vec![std::path::PathBuf::from("/tmp/knit-arrived")]);
        assert!(waiting_for_data().is_none(), "到着で待ちが終わる");
    }

    /// 中止で待ちが消える(検証役の変異 m6 が指していた経路)
    #[test]
    fn cancelling_clears_the_waiting_state() {
        let _g = lock();
        set_waiting_for_test(4242, 1024);
        assert!(waiting_for_data().is_some());
        cancel(4242);
        assert!(waiting_for_data().is_none(), "中止で消える");
    }

    /// 別のIDの中止では消えない(取り違え防止)
    #[test]
    fn cancelling_another_id_keeps_the_waiting_state() {
        let _g = lock();
        set_waiting_for_test(7, 1024);
        cancel(8);
        assert!(waiting_for_data().is_some(), "別IDでは消さない");
        cancel(7);
    }
}

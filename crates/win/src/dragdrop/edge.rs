//! ExplorerのOLEドラッグを接続辺で受け、準備が整ったMacへ引き継ぐ。
use super::*;
use knit_common::{
    bulk,
    proto::{encode, Msg},
};
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, AtomicU64},
    Mutex,
};
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINTL, WPARAM};
use windows_sys::Win32::System::Ole::{RegisterDragDrop, ReleaseStgMedium, RevokeDragDrop};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

pub static PEER_VERSION: AtomicU32 = AtomicU32::new(0);
pub static CONTROLLED: AtomicBool = AtomicBool::new(false);
static NEXT: AtomicU64 = AtomicU64::new(1);
static PENDING: Mutex<Option<Pending>> = Mutex::new(None);

/// 受け渡しが始まるのを待つ上限(これを過ぎたら無言で放置せず取り消して知らせる)
const SEND_WAIT: Duration = Duration::from_secs(30);

/// 受け渡しの待ちが長すぎるか(純関数)。**送信中は対象外**(転送そのものは止めない)
fn send_wait_expired(sending: bool, waited: Duration) -> bool {
    !sending && waited > SEND_WAIT
}

struct Pending {
    id: u64,
    /// 展開後の送信内容(フォルダは相対パス付きの中のファイルへ展開済み)。
    /// 予告(DragOffer)と同じ内容を送るため、ここで確定させる
    entries: Vec<bulk::OutFile>,
    sending: bool,
    ready: bool,
    committing: bool,
    committed: bool,
    began: Instant,
}

fn send(msg: Msg) -> bool {
    crate::WTX
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .is_some_and(|tx| tx.send(encode(&msg)).is_ok())
}

pub fn cancel(id: u64) {
    let mut state = PENDING.lock().unwrap_or_else(|e| e.into_inner());
    if state.as_ref().is_some_and(|p| p.id == id) {
        *state = None;
    }
}

fn cancel_current() {
    let pending = PENDING.lock().unwrap_or_else(|e| e.into_inner()).take();
    if let Some(p) = pending {
        send(Msg::DragCancel { id: p.id });
    }
}

pub fn reset() {
    CONTROLLED.store(false, Ordering::Relaxed);
    cancel_current();
}

pub fn committed() -> bool {
    PENDING
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .is_some_and(|p| p.committed)
}

pub fn accept(id: u64) {
    let entries = {
        let mut state = PENDING.lock().unwrap_or_else(|e| e.into_inner());
        let Some(p) = state.as_mut().filter(|p| p.id == id && !p.sending) else {
            return;
        };
        p.sending = true;
        p.entries.clone()
    };
    let total = bulk::entries_total(&entries);
    let label = entries.first().map(|e| e.name.clone()).unwrap_or_default();
    crate::begin_tx(id, total, &label);
    std::thread::spawn(move || {
        let mut cancelled = || {
            // 予告の取り消し(ボタン解放・30秒経過・切断)または Esc での中止
            knit_common::xfer::take(id)
                || !PENDING
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .as_ref()
                    .is_some_and(|p| p.id == id)
        };
        let result = crate::BULK_LINK.send(|w| {
            bulk::send_entries(
                w,
                &entries,
                true,
                Some(id),
                &mut cancelled,
                &mut |sent, total, name| crate::update_tx(id, sent, total, name),
            )
        });
        crate::end_tx(id);
        knit_common::xfer::discard(id);
        if let Err(error) = result {
            cancel(id);
            send(Msg::DragCancel { id });
            println!("[drag] win->mac transfer {id} failed: {error}");
            if error.kind() == std::io::ErrorKind::Interrupted {
                crate::tray::notify("Knit", "ファイル転送を中止しました");
            } else {
                crate::tray::notify(
                    "Knit",
                    &format!(
                        "相手へファイルを渡せませんでした({}。接続を確認してもう一度掴んでください)",
                        bulk::send_error_label(&error)
                    ),
                );
            }
        }
    });
}

pub fn ready(id: u64) {
    if let Some(p) = PENDING
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_mut()
        .filter(|p| p.id == id && p.sending)
    {
        p.ready = true;
    }
}

unsafe fn data_files(data: *mut c_void) -> Option<Vec<PathBuf>> {
    if data.is_null() {
        return None;
    }
    let vtbl = *(data as *const *const IDataObjectVtbl);
    let format = FormatEtc {
        cf_format: CF_HDROP,
        ptd: std::ptr::null_mut(),
        dw_aspect: DVASPECT_CONTENT,
        lindex: -1,
        tymed: TYMED_HGLOBAL,
    };
    if ((*vtbl).query_get_data)(data, &format) != S_OK {
        return None;
    }
    let mut medium: StgMedium = std::mem::zeroed();
    if ((*vtbl).get_data)(data, &format, &mut medium) != S_OK {
        return None;
    }
    let mut files = Vec::new();
    if medium.tymed == TYMED_HGLOBAL && !medium.h_global.is_null() {
        let n = crate::DragQueryFileW(medium.h_global, u32::MAX, std::ptr::null_mut(), 0);
        if n > 0 && n <= knit_common::drag::MAX_FILES as u32 {
            for i in 0..n {
                let len = crate::DragQueryFileW(medium.h_global, i, std::ptr::null_mut(), 0);
                if len == 0 || len > 32767 {
                    files.clear();
                    break;
                }
                let mut name = vec![0u16; len as usize + 1];
                if crate::DragQueryFileW(medium.h_global, i, name.as_mut_ptr(), len + 1) != len {
                    files.clear();
                    break;
                }
                use std::os::windows::ffi::OsStringExt;
                files.push(PathBuf::from(std::ffi::OsString::from_wide(
                    &name[..len as usize],
                )));
            }
        } else if n > knit_common::drag::MAX_FILES as u32 {
            crate::tray::notify(
                "Knit",
                "掴んだまま渡せるのは 64 件までです(先にフォルダへまとめてから掴んでください)",
            );
        }
    }
    ReleaseStgMedium((&mut medium as *mut StgMedium).cast());
    (!files.is_empty()).then_some(files)
}

#[repr(C)]
struct TargetVtbl {
    qi: QiFn,
    add_ref: AddRefFn,
    release: ReleaseFn,
    enter: unsafe extern "system" fn(*mut c_void, *mut c_void, u32, POINTL, *mut u32) -> HRESULT,
    over: unsafe extern "system" fn(*mut c_void, u32, POINTL, *mut u32) -> HRESULT,
    leave: unsafe extern "system" fn(*mut c_void) -> HRESULT,
    drop: unsafe extern "system" fn(*mut c_void, *mut c_void, u32, POINTL, *mut u32) -> HRESULT,
}
#[repr(C)]
struct Target {
    vtbl: &'static TargetVtbl,
    refs: AtomicU32,
}
const IID_TARGET: Guid = Guid {
    data1: 0x122,
    data2: 0,
    data3: 0,
    data4: [0xc0, 0, 0, 0, 0, 0, 0, 0x46],
};
unsafe extern "system" fn qi(
    this: *mut c_void,
    iid: *const Guid,
    out: *mut *mut c_void,
) -> HRESULT {
    if iid.is_null() || out.is_null() {
        return E_FAIL;
    }
    *out = std::ptr::null_mut();
    if guid_eq(&*iid, &IID_IUNKNOWN) || guid_eq(&*iid, &IID_TARGET) {
        *out = this;
        add_ref(this);
        S_OK
    } else {
        E_NOINTERFACE
    }
}
unsafe extern "system" fn add_ref(this: *mut c_void) -> u32 {
    (*(this as *mut Target))
        .refs
        .fetch_add(1, Ordering::Relaxed)
        + 1
}
unsafe extern "system" fn release(this: *mut c_void) -> u32 {
    let left = (*(this as *mut Target))
        .refs
        .fetch_sub(1, Ordering::Relaxed)
        - 1;
    if left == 0 {
        drop(Box::from_raw(this as *mut Target));
    }
    left
}
unsafe extern "system" fn enter(
    _this: *mut c_void,
    data: *mut c_void,
    keys: u32,
    pt: POINTL,
    effect: *mut u32,
) -> HRESULT {
    if effect.is_null() {
        return E_FAIL;
    }
    let allowed = *effect & DROPEFFECT_COPY != 0;
    *effect = DROPEFFECT_NONE;
    if !allowed || keys & MK_LBUTTON == 0 || !enabled() {
        return S_OK;
    }
    // 帯の再配置は16ms毎なので、Cfg更新直後の古いウィンドウ位置もここで拒否する。
    let Some(position) = position_at(pt) else {
        return S_OK;
    };
    // 自分が回している DoDragDrop を自分の辺で受けると Mac へ DragOffer を
    // 送り返す往復が起きるため、進行中の受けドラッグは他人のものだけにする
    if DRAG_THREAD.load(Ordering::Relaxed) != 0 {
        return S_OK;
    }
    // 保険: 渡されたデータが自前 DataSource(vtbl が同一)なら本物の
    // Explorer 由来ではないので弾く
    if !data.is_null() && std::ptr::eq(*(data as *const *const IDataObjectVtbl), &DATA_VTBL) {
        return S_OK;
    }
    if PENDING.lock().unwrap_or_else(|e| e.into_inner()).is_some() {
        return S_OK;
    }
    let Some(files) = data_files(data) else {
        return S_OK;
    };
    // 相手の版が持つ機能(フォルダ=版 14 以降・空フォルダ=版 15 以降)は
    // proto::peer_features に集約した判定を使う
    let f = knit_common::proto::peer_features(PEER_VERSION.load(Ordering::Relaxed));
    let entries = match bulk::collect(&files, true, f.dirs, f.empty_dirs) {
        Ok(e) if !e.is_empty() => e,
        Ok(_) => {
            crate::tray::notify("Knit", "掴んだ項目の中に送れるものがありません");
            return S_OK;
        }
        Err(e) => {
            crate::tray::notify(
                "Knit",
                &format!(
                    "渡せません({e})。1回は最大 {} 件・合計 {} まで、読み取れる項目のみです",
                    knit_common::drag::MAX_BATCH_FILES,
                    bulk::file_limit_label()
                ),
            );
            return S_OK;
        }
    };
    let total = bulk::entries_total(&entries);
    let count = entries.len();
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    *PENDING.lock().unwrap_or_else(|e| e.into_inner()) = Some(Pending {
        id,
        entries,
        sending: false,
        ready: false,
        committing: false,
        committed: false,
        began: Instant::now(),
    });
    if !send(Msg::DragOffer {
        id,
        count,
        total,
        position: position.clamp(0.0, 1.0),
    }) {
        cancel(id);
        return S_OK;
    }
    println!("[drag] win->mac offer {id}: {count} files, {total} bytes");
    if total > 8 * 1024 * 1024 {
        crate::tray::notify(
            "Knit",
            &format!(
                "相手へ転送中です({})。境界で押したまま待つと、準備後に相手へ移ります(サイズによって数十秒以上かかることがあります)",
                crate::human_bytes(total)
            ),
        );
    }
    *effect = DROPEFFECT_COPY;
    S_OK
}

unsafe extern "system" fn over(
    _this: *mut c_void,
    _keys: u32,
    pt: POINTL,
    effect: *mut u32,
) -> HRESULT {
    if effect.is_null() {
        return E_FAIL;
    }
    if !enabled() || position_at(pt).is_none() {
        *effect = DROPEFFECT_NONE;
        cancel_current();
        return S_OK;
    }
    *effect = if *effect & DROPEFFECT_COPY != 0
        && PENDING.lock().unwrap_or_else(|e| e.into_inner()).is_some()
    {
        DROPEFFECT_COPY
    } else {
        DROPEFFECT_NONE
    };
    S_OK
}
unsafe extern "system" fn leave(_this: *mut c_void) -> HRESULT {
    if !committed() {
        cancel_current();
    }
    S_OK
}
unsafe extern "system" fn receive_drop(
    _this: *mut c_void,
    _data: *mut c_void,
    _keys: u32,
    pt: POINTL,
    effect: *mut u32,
) -> HRESULT {
    if effect.is_null() {
        return E_FAIL;
    }
    *effect = DROPEFFECT_NONE;
    if !enabled() || position_at(pt).is_none() {
        cancel_current();
        return S_OK;
    }
    let id = {
        let mut state = PENDING.lock().unwrap_or_else(|e| e.into_inner());
        state
            .as_mut()
            .filter(|p| p.ready && p.committing && !p.committed)
            .map(|p| {
                p.committed = true;
                p.id
            })
    };
    if let Some(id) = id {
        if send(Msg::DragCommit { id }) {
            *effect = DROPEFFECT_COPY;
            println!("[drag] win->mac commit {id}");
        } else {
            cancel_current();
        }
    } else {
        cancel_current();
    }
    S_OK
}
static VTBL: TargetVtbl = TargetVtbl {
    qi,
    add_ref,
    release,
    enter,
    over,
    leave,
    drop: receive_drop,
};

fn enabled() -> bool {
    PEER_VERSION.load(Ordering::Relaxed) >= 12
        && crate::CONNECTED.load(Ordering::Relaxed)
        && CONTROLLED.load(Ordering::Relaxed)
        // ファイル共有が閉じているときは境界バンドの受信オファーを通さない
        // (転送本体が共有範囲の検査で全フレーム捨てられ、押下の間ずっと無音になるため)
        && knit_common::share::allow_files()
}

fn rect(
    side: u8,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    range: Option<[f64; 2]>,
) -> Option<(i32, i32, i32, i32)> {
    if w <= 0 || h <= 0 {
        return None;
    }
    let horizontal = matches!(side, 2 | 3);
    let (start, end) = crate::state::target_pixel_span(range, if horizontal { w } else { h })?;
    let width = 8.min(if horizontal { h } else { w });
    Some(match side {
        1 | 6 | 7 => (x + w - width, y + start, width, end - start),
        2 => (x + start, y + h - width, end - start, width),
        3 => (x + start, y, end - start, width),
        _ => (x, y + start, width, end - start),
    })
}

/// OLEが渡す画面座標を、現在の帯とtarget区間の両方で検査する。
fn position_in_rect(
    side: u8,
    px: i32,
    py: i32,
    w: i32,
    h: i32,
    range: Option<[f64; 2]>,
) -> Option<f64> {
    let position = crate::state::edge_position(side, px, py, w, h)?;
    let (x, y, rw, rh) = rect(side, 0, 0, w, h, range)?;
    (px >= x && px < x + rw && py >= y && py < y + rh
        && crate::state::target_range_contains(range, position))
    .then_some(position)
}

fn position_at(pt: POINTL) -> Option<f64> {
    let (x, y, w, h) = crate::vscreen();
    let (side, range) = crate::state::connection_edge();
    position_in_rect(side, pt.x - x, pt.y - y, w, h, range)
}

unsafe extern "system" fn window_proc(hwnd: HWND, message: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if message == WM_TIMER {
        let down = crate::BTN_W[0].load(Ordering::Relaxed);
        // 自分のドラッグ中に帯を出すと自前 DataSource が自分の辺へ入る
        // ため、進行中(DRAG_THREAD != 0)は表示しない
        let show = enabled() && down && !committed() && DRAG_THREAD.load(Ordering::Relaxed) == 0;
        let band = if show {
            let (x, y, w, h) = crate::vscreen();
            let (side, range) = crate::state::connection_edge();
            rect(side, x, y, w, h, range)
        } else {
            None
        };
        if let Some((x, y, w, h)) = band {
            SetWindowPos(
                hwnd,
                HWND_TOPMOST,
                x,
                y,
                w,
                h,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
        } else {
            ShowWindow(hwnd, SW_HIDE);
        }
        let mut cursor = windows_sys::Win32::Foundation::POINT { x: 0, y: 0 };
        let at_target = GetCursorPos(&mut cursor) != 0
            && position_at(POINTL { x: cursor.x, y: cursor.y }).is_some();
        let (cancel, finish, timed_out) = {
            let mut state = PENDING.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(p) = state.as_mut() {
                let timed_out = send_wait_expired(p.sending, p.began.elapsed());
                let cancel =
                    (!enabled() || !at_target || !down || timed_out) && !p.committed;
                // Cfg更新で区間外になった直後にも、マウスUpで強制確定させない。
                let finish = !cancel && at_target && p.ready && !p.committing;
                if finish {
                    p.committing = true;
                }
                (cancel, finish, cancel && timed_out)
            } else {
                (false, false, false)
            }
        };
        if cancel {
            cancel_current();
            // **無言で取り消さない**: 30秒待って受け渡しが始まらないときは理由を出す
            // (取り消したこと自体が分からないと「壊れた」と誤解される)
            if timed_out {
                crate::tray::notify(
                    "Knit",
                    "受け渡しが始まりませんでした(30秒待ちました)。ドラッグは取り消しました。もう一度掴んでください",
                );
            }
        }
        // 元のExplorerへだけUpを届ける。Macの物理押下は引き継ぎまで保持する。
        if finish && !crate::inject_mouse_btn(0, false) {
            cancel_current();
            crate::tray::notify(
                "Knit",
                "Windowsのドラッグを引き継げませんでした。もう一度掴んでください",
            );
        }
        return 0;
    }
    DefWindowProcW(hwnd, message, wp, lp)
}

pub fn start() {
    std::thread::spawn(|| unsafe {
        if OleInitialize(std::ptr::null()) < 0 {
            return;
        }
        let hwnd = create_window();
        if hwnd.is_null() {
            OleUninitialize();
            return;
        }
        SetLayeredWindowAttributes(hwnd, 0, 160, LWA_ALPHA);
        let target = Box::into_raw(Box::new(Target {
            vtbl: &VTBL,
            refs: AtomicU32::new(1),
        })) as *mut c_void;
        let hr = RegisterDragDrop(hwnd, target);
        if hr < 0 {
            println!("[drag] edge registration failed: {hr:#x}");
            release(target);
            DestroyWindow(hwnd);
            OleUninitialize();
            return;
        }
        NEXT.store(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
            Ordering::Relaxed,
        );
        SetTimer(hwnd, 1, 16, None);
        println!("[drag] Windows file edge ready");
        let mut msg = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        RevokeDragDrop(hwnd);
        release(target);
        DestroyWindow(hwnd);
        OleUninitialize();
    });
}

unsafe fn create_window() -> HWND {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetModuleHandleW(name: *const u16) -> *mut c_void;
    }
    let instance = GetModuleHandleW(std::ptr::null());
    let name: Vec<u16> = "KnitFileEdge\0".encode_utf16().collect();
    let class = WNDCLASSW {
        hInstance: instance,
        lpfnWndProc: Some(window_proc),
        lpszClassName: name.as_ptr(),
        hbrBackground: windows_sys::Win32::Graphics::Gdi::CreateSolidBrush(0x00D69648),
        ..std::mem::zeroed()
    };
    RegisterClassW(&class);
    CreateWindowExW(
        WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TOPMOST | WS_EX_LAYERED,
        name.as_ptr(),
        name.as_ptr(),
        WS_POPUP,
        0,
        0,
        8,
        8,
        std::ptr::null_mut(),
        std::ptr::null_mut(),
        instance,
        std::ptr::null(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn edge_matches_all_four_connection_sides() {
        assert_eq!(rect(0, -100, 20, 1000, 800, None), Some((-100, 20, 8, 800)));
        assert_eq!(rect(1, -100, 20, 1000, 800, None), Some((892, 20, 8, 800)));
        assert_eq!(rect(2, -100, 20, 1000, 800, None), Some((-100, 812, 1000, 8)));
        assert_eq!(rect(3, -100, 20, 1000, 800, None), Some((-100, 20, 1000, 8)));
    }

    #[test]
    fn edge_band_is_limited_to_target_without_reversing_either_axis() {
        let range = Some([0.25, 0.75]);
        assert_eq!(rect(0, -100, 20, 1000, 800, range), Some((-100, 220, 8, 401)));
        assert_eq!(rect(1, -100, 20, 1000, 800, range), Some((892, 220, 8, 401)));
        assert_eq!(rect(2, -100, 20, 1000, 800, range), Some((150, 812, 501, 8)));
        assert_eq!(rect(3, -100, 20, 1000, 800, range), Some((150, 20, 501, 8)));
        for side in [4, 5] {
            assert_eq!(rect(side, -100, 20, 1000, 800, range), rect(0, -100, 20, 1000, 800, range));
        }
        for side in [6, 7] {
            assert_eq!(rect(side, -100, 20, 1000, 800, range), rect(1, -100, 20, 1000, 800, range));
        }
    }

    #[test]
    fn stale_ole_band_does_not_accept_points_outside_current_target() {
        for side in 0..8 {
            let point = |position| match side {
                1 | 6 | 7 => (997, position),
                2 => (position, 997),
                3 => (position, 3),
                _ => (3, position),
            };
            let range = Some([0.25, 0.75]);
            for position in [250, 500, 750] {
                let (px, py) = point(position);
                assert_eq!(position_in_rect(side, px, py, 1000, 1000, range),
                    Some(position as f64 / 1000.0));
            }
            for position in [0, 249, 751, 999] {
                let (px, py) = point(position);
                assert_eq!(position_in_rect(side, px, py, 1000, 1000, range), None);
            }
            assert_eq!(rect(side, 0, 0, 1000, 1000, Some([0.8, 0.2])), None);
        }
        assert_eq!(position_in_rect(0, 8, 500, 1000, 1000, None), None);
        assert_eq!(rect(0, 0, 0, 100, 100, Some([0.251, 0.259])), None);
        assert_eq!(rect(0, 0, 0, 0, 100, None), None);
        assert_eq!(rect(2, 0, 0, 100, 0, None), None);
        assert_eq!(rect(0, 0, 0, 1, 1, None), Some((0, 0, 1, 1)));
    }
    #[test]
    fn reads_paths_from_an_ole_data_object_without_clipboard() {
        unsafe {
            let paths = vec![String::from(r"C:\test\資料.txt")];
            let source = Box::into_raw(Box::new(DataSource {
                vtbl: &DATA_VTBL,
                paths: paths.clone(),
                refs: AtomicU32::new(1),
            })) as *mut c_void;
            let read = data_files(source);
            ds_release(source);
            assert_eq!(read, Some(paths.into_iter().map(PathBuf::from).collect()));
        }
    }
    #[test]
    fn windows_registers_the_edge_as_an_ole_drop_target() {
        unsafe {
            assert!(OleInitialize(std::ptr::null()) >= 0);
            let hwnd = create_window();
            assert!(!hwnd.is_null());
            let target = Box::into_raw(Box::new(Target {
                vtbl: &VTBL,
                refs: AtomicU32::new(1),
            })) as *mut c_void;
            let result = RegisterDragDrop(hwnd, target);
            if result >= 0 {
                RevokeDragDrop(hwnd);
            }
            release(target);
            DestroyWindow(hwnd);
            OleUninitialize();
            assert_eq!(result, S_OK);
        }
    }
    #[test]
    fn ignores_the_drag_we_are_carrying_ourselves() {
        PEER_VERSION.store(12, Ordering::Relaxed);
        crate::CONNECTED.store(true, Ordering::Relaxed);
        CONTROLLED.store(true, Ordering::Relaxed);
        unsafe {
            let point = POINTL { x: 0, y: 0 };
            let no_pending = || PENDING.lock().unwrap_or_else(|e| e.into_inner()).is_none();
            DRAG_THREAD.store(1, Ordering::Relaxed);
            let mut effect = DROPEFFECT_COPY;
            assert_eq!(
                enter(
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    MK_LBUTTON,
                    point,
                    &mut effect
                ),
                S_OK
            );
            assert_eq!(effect, DROPEFFECT_NONE);
            assert!(no_pending(), "自分のドラッグをDragOfferへ繋がない");
            DRAG_THREAD.store(0, Ordering::Relaxed);
            // 保険: データが自前 DataSource でも DRAG_THREAD に関係なく弾く
            let paths = vec![String::from(r"C:\test\自己ドラッグ.txt")];
            let source = Box::into_raw(Box::new(DataSource {
                vtbl: &DATA_VTBL,
                paths,
                refs: AtomicU32::new(1),
            })) as *mut c_void;
            let mut effect = DROPEFFECT_COPY;
            assert_eq!(
                enter(std::ptr::null_mut(), source, MK_LBUTTON, point, &mut effect),
                S_OK
            );
            assert_eq!(effect, DROPEFFECT_NONE);
            assert!(no_pending());
            ds_release(source);
        }
        crate::CONNECTED.store(false, Ordering::Relaxed);
        CONTROLLED.store(false, Ordering::Relaxed);
        PEER_VERSION.store(0, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod send_wait_tests {
    use super::*;

    /// 送信が始まっていなければ、30秒を過ぎたら取り消して知らせる
    #[test]
    fn a_wait_that_never_starts_expires() {
        assert!(!send_wait_expired(false, Duration::from_secs(29)), "29秒はまだ待つ");
        assert!(send_wait_expired(false, Duration::from_secs(31)), "31秒は期限切れ");
        assert!(
            !send_wait_expired(false, SEND_WAIT),
            "ちょうど30秒はまだ待つ(境界)"
        );
    }

    /// **送信中は決して期限切れにしない**(長い転送を打ち切らない)
    #[test]
    fn an_active_transfer_is_never_expired() {
        assert!(!send_wait_expired(true, Duration::from_secs(31)));
        assert!(!send_wait_expired(true, Duration::from_secs(6 * 60 * 60)));
    }
}

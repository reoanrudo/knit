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

struct Pending {
    id: u64,
    files: Vec<PathBuf>,
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
    let files = {
        let mut state = PENDING.lock().unwrap_or_else(|e| e.into_inner());
        let Some(p) = state.as_mut().filter(|p| p.id == id && !p.sending) else {
            return;
        };
        p.sending = true;
        p.files.clone()
    };
    std::thread::spawn(move || {
        let cancelled = || {
            !PENDING
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .as_ref()
                .is_some_and(|p| p.id == id)
        };
        let result = crate::BULK_LINK.send(|w| bulk::send_drag_files(w, &files, id, cancelled));
        if let Err(error) = result {
            cancel(id);
            send(Msg::DragCancel { id });
            println!("[drag] win->mac transfer {id} failed: {error}");
            if error.kind() != std::io::ErrorKind::Interrupted {
                crate::tray::notify(
                    "Knit",
                    "Macへファイルを渡せませんでした。接続を確認してもう一度掴んでください",
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
    if PENDING.lock().unwrap_or_else(|e| e.into_inner()).is_some() {
        return S_OK;
    }
    let Some(files) = data_files(data) else {
        return S_OK;
    };
    if files
        .iter()
        .any(|p| !p.is_file() || std::fs::File::open(p).is_err())
        || bulk::total_size(&files) > bulk::MAX_TOTAL
    {
        crate::tray::notify("Knit", "通常のファイルを選んでください。フォルダ・読み取れない項目・転送上限を超える選択は渡せません");
        return S_OK;
    }
    let total = bulk::total_size(&files);
    let count = files.len();
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    let (x, y, w, h) = crate::vscreen();
    let position = if matches!(crate::SIDE_W.load(Ordering::Relaxed), 2 | 3) {
        (pt.x - x) as f64 / w.max(1) as f64
    } else {
        (pt.y - y) as f64 / h.max(1) as f64
    };
    *PENDING.lock().unwrap_or_else(|e| e.into_inner()) = Some(Pending {
        id,
        files,
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
                "Macへ転送中です({})。境界で押したまま待つと、準備後にMacへ移ります",
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
    _pt: POINTL,
    effect: *mut u32,
) -> HRESULT {
    if effect.is_null() {
        return E_FAIL;
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
    _pt: POINTL,
    effect: *mut u32,
) -> HRESULT {
    if effect.is_null() {
        return E_FAIL;
    }
    *effect = DROPEFFECT_NONE;
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
}

fn rect(side: u8, x: i32, y: i32, w: i32, h: i32) -> (i32, i32, i32, i32) {
    let width = 8;
    match side {
        1 | 6 | 7 => (x + w - width, y, width, h),
        2 => (x, y + h - width, w, width),
        3 => (x, y, w, width),
        _ => (x, y, width, h),
    }
}

unsafe extern "system" fn window_proc(hwnd: HWND, message: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if message == WM_TIMER {
        let down = crate::BTN_W[0].load(Ordering::Relaxed);
        let show = enabled() && down && !committed();
        if show {
            let (x, y, w, h) = crate::vscreen();
            let (x, y, w, h) = rect(crate::SIDE_W.load(Ordering::Relaxed), x, y, w, h);
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
        let (cancel, finish) = {
            let mut state = PENDING.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(p) = state.as_mut() {
                let cancel = (!enabled()
                    || !down
                    || (!p.sending && p.began.elapsed() > Duration::from_secs(30)))
                    && !p.committed;
                let finish = !cancel && p.ready && !p.committing;
                if finish {
                    p.committing = true;
                }
                (cancel, finish)
            } else {
                (false, false)
            }
        };
        if cancel {
            cancel_current();
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
        assert_eq!(rect(0, -100, 20, 1000, 800), (-100, 20, 8, 800));
        assert_eq!(rect(1, -100, 20, 1000, 800), (892, 20, 8, 800));
        assert_eq!(rect(2, -100, 20, 1000, 800), (-100, 812, 1000, 8));
        assert_eq!(rect(3, -100, 20, 1000, 800), (-100, 20, 1000, 8));
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
}

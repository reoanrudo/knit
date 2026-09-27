//! ファイル掴みドラッグ越域(Mac→Win)の本体: DoDragDrop を回して本物の
//! OLE ドラッグ&ドロップを提供する。Mac で掴んだファイルを境界越えで受け、
//! マウスが押されている間は「掴んだまま」の状態を Windows 全域で表現し、
//! 離した位置の窓へ渡す。SendInput による入力注入は通常入力と同じ経路を
//! 通るため、Mac からの移動転送がそのままドラッグ操作になる。
//!
//! windows-sys には COM インターフェースの vtbl 構造体が無いため自前定義する
//! (WASAPI の vtbl 直叩きと同じ方式。vtbl 並びは MSDN の IDataObject 等の
//! メソッド順序どおり。並びを間違えると即クラッシュするので変更時は注意)。

use std::ffi::c_void;
pub mod edge;
mod host;
pub use host::{relay_cancel, relay_move, relay_up};
#[cfg(test)]
mod live_tests;
/// 進行中の OLE ドラッグを回しているスレッドのID(0=なし)。共有入力の
/// 中継先を「進行中のドラッグ」に限定するために保持する
pub static DRAG_THREAD: AtomicU32 = AtomicU32::new(0);
#[cfg(test)]
static TEST_DRAG_READY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
use std::sync::atomic::{AtomicU32, Ordering};

use windows_sys::Win32::System::Com::DVASPECT_CONTENT;
use windows_sys::Win32::System::Ole::{DoDragDrop, OleInitialize, OleUninitialize};

pub type HRESULT = i32;

const S_OK: HRESULT = 0;
const S_FALSE: HRESULT = 1;
const E_NOTIMPL: HRESULT = 0x8000_4001u32 as i32;
const E_NOINTERFACE: HRESULT = 0x8000_4002u32 as i32;
const E_FAIL: HRESULT = 0x8000_4005u32 as i32;
const DV_E_FORMATETC: HRESULT = 0x8004_0064u32 as i32;
const OLE_E_ADVISENOTSUPPORTED: HRESULT = 0x8004_0003u32 as i32;
const DRAGDROP_S_DROP: HRESULT = 0x0004_0100;
const DRAGDROP_S_CANCEL: HRESULT = 0x0004_0101;
const DRAGDROP_S_USEDEFAULTCURSORS: HRESULT = 0x0004_0102;

const CF_HDROP: u16 = 15;
const TYMED_HGLOBAL: u32 = 1;
const DROPEFFECT_COPY: u32 = 1;
const DROPEFFECT_NONE: u32 = 0;
const MK_LBUTTON: u32 = 0x0001;
const DATADIR_GET: u32 = 1;

#[repr(C)]
struct Guid {
    data1: u32,
    data2: u16,
    data3: u16,
    data4: [u8; 8],
}

const IID_IUNKNOWN: Guid = Guid {
    data1: 0x0000_0000,
    data2: 0x0000,
    data3: 0x0000,
    data4: [0xC0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46],
};
const IID_IDATAOBJECT: Guid = Guid {
    data1: 0x0000_010E,
    data2: 0x0000,
    data3: 0x0000,
    data4: [0xC0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46],
};
const IID_IENUMFORMATETC: Guid = Guid {
    data1: 0x0000_0103,
    data2: 0x0000,
    data3: 0x0000,
    data4: [0xC0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46],
};
const IID_IDROPSOURCE: Guid = Guid {
    data1: 0x0000_0121,
    data2: 0x0000,
    data3: 0x0000,
    data4: [0xC0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46],
};

fn guid_eq(a: &Guid, b: &Guid) -> bool {
    a.data1 == b.data1 && a.data2 == b.data2 && a.data3 == b.data3 && a.data4 == b.data4
}

#[repr(C)]
struct FormatEtc {
    cf_format: u16,
    ptd: *mut c_void,
    dw_aspect: u32,
    lindex: i32,
    tymed: u32,
}

#[repr(C)]
struct StgMedium {
    tymed: u32,
    h_global: *mut c_void,
    p_unk_for_release: *mut c_void,
}

unsafe impl Send for FormatEtc {}

// ---------- vtbl 定義(MSDN のメソッド順) ----------

type QiFn = unsafe extern "system" fn(*mut c_void, *const Guid, *mut *mut c_void) -> HRESULT;
type AddRefFn = unsafe extern "system" fn(*mut c_void) -> u32;
type ReleaseFn = unsafe extern "system" fn(*mut c_void) -> u32;

#[repr(C)]
struct IEnumFORMATETCVtbl {
    query_interface: QiFn,
    add_ref: AddRefFn,
    release: ReleaseFn,
    next: unsafe extern "system" fn(*mut c_void, u32, *mut FormatEtc, *mut u32) -> HRESULT,
    skip: unsafe extern "system" fn(*mut c_void, u32) -> HRESULT,
    reset: unsafe extern "system" fn(*mut c_void) -> HRESULT,
    clone: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> HRESULT,
}

#[repr(C)]
struct IDataObjectVtbl {
    query_interface: QiFn,
    add_ref: AddRefFn,
    release: ReleaseFn,
    get_data: unsafe extern "system" fn(*mut c_void, *const FormatEtc, *mut StgMedium) -> HRESULT,
    get_data_here:
        unsafe extern "system" fn(*mut c_void, *const FormatEtc, *mut StgMedium) -> HRESULT,
    query_get_data: unsafe extern "system" fn(*mut c_void, *const FormatEtc) -> HRESULT,
    get_canonical_format_etc:
        unsafe extern "system" fn(*mut c_void, *const FormatEtc, *mut FormatEtc) -> HRESULT,
    set_data:
        unsafe extern "system" fn(*mut c_void, *mut FormatEtc, *mut StgMedium, i32) -> HRESULT,
    enum_format_etc: unsafe extern "system" fn(*mut c_void, u32, *mut *mut c_void) -> HRESULT,
    d_advise: unsafe extern "system" fn(
        *mut c_void,
        *const FormatEtc,
        u32,
        *mut c_void,
        *mut u32,
    ) -> HRESULT,
    d_unadvise: unsafe extern "system" fn(*mut c_void, u32) -> HRESULT,
    enum_d_advise: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> HRESULT,
}

#[repr(C)]
struct IDropSourceVtbl {
    query_interface: QiFn,
    add_ref: AddRefFn,
    release: ReleaseFn,
    query_continue_drag: unsafe extern "system" fn(*mut c_void, i32, u32) -> HRESULT,
    give_feedback: unsafe extern "system" fn(*mut c_void, u32) -> HRESULT,
}

// ---------- 実装 ----------

/// ファイル群を CF_HDROP で提供する IDataObject。Box の実体先頭に vtbl を
/// 置き、そのまま COM インターフェースポインタとして渡す
#[repr(C)]
struct DataSource {
    vtbl: &'static IDataObjectVtbl,
    paths: Vec<String>,
    refs: AtomicU32,
}

impl DataSource {
    unsafe fn from_raw(this: *mut c_void) -> &'static mut Self {
        &mut *(this as *mut Self)
    }
}

/// CF_HDROP 1 形式を列挙する IEnumFORMATETC
#[repr(C)]
struct EnumFmt {
    vtbl: &'static IEnumFORMATETCVtbl,
    pos: u32,
    refs: AtomicU32,
}

impl EnumFmt {
    unsafe fn from_raw(this: *mut c_void) -> &'static mut Self {
        &mut *(this as *mut Self)
    }
}

#[repr(C)]
struct DropSource {
    vtbl: &'static IDropSourceVtbl,
    refs: AtomicU32,
}

impl DropSource {
    unsafe fn from_raw(this: *mut c_void) -> &'static mut Self {
        &mut *(this as *mut Self)
    }
}

unsafe fn wanted(fe: &FormatEtc) -> bool {
    fe.cf_format == CF_HDROP && (fe.tymed & TYMED_HGLOBAL) != 0
}

// ---- IDataObject ----

unsafe extern "system" fn ds_qi(
    this: *mut c_void,
    iid: *const Guid,
    out: *mut *mut c_void,
) -> HRESULT {
    if iid.is_null() || out.is_null() {
        return E_FAIL;
    }
    *out = std::ptr::null_mut();
    if guid_eq(&*iid, &IID_IUNKNOWN) || guid_eq(&*iid, &IID_IDATAOBJECT) {
        *out = this;
        DataSource::from_raw(this)
            .refs
            .fetch_add(1, Ordering::Relaxed);
        return S_OK;
    }
    // COM 規約: QI の不支援は E_NOINTERFACE(E_NOTIMPL ではない)
    E_NOINTERFACE
}

unsafe extern "system" fn ds_add_ref(this: *mut c_void) -> u32 {
    DataSource::from_raw(this)
        .refs
        .fetch_add(1, Ordering::Relaxed)
        + 1
}

unsafe extern "system" fn ds_release(this: *mut c_void) -> u32 {
    let d = DataSource::from_raw(this);
    let r = d.refs.fetch_sub(1, Ordering::Relaxed) - 1;
    if r == 0 {
        drop(Box::from_raw(this as *mut DataSource));
    }
    r
}

unsafe extern "system" fn ds_get_data(
    this: *mut c_void,
    pfe: *const FormatEtc,
    pmed: *mut StgMedium,
) -> HRESULT {
    if pfe.is_null() || pmed.is_null() {
        return E_FAIL;
    }
    if !wanted(&*pfe) {
        return DV_E_FORMATETC;
    }
    let h = crate::make_hdrop_global(&DataSource::from_raw(this).paths);
    if h.is_null() {
        return E_FAIL;
    }
    // pUnkForRelease=NULL の場合、受け手の ReleaseStgMedium が GlobalFree する
    (*pmed).tymed = TYMED_HGLOBAL;
    (*pmed).h_global = h;
    (*pmed).p_unk_for_release = std::ptr::null_mut();
    S_OK
}

unsafe extern "system" fn ds_get_data_here(
    _this: *mut c_void,
    _pfe: *const FormatEtc,
    _pmed: *mut StgMedium,
) -> HRESULT {
    E_NOTIMPL
}

unsafe extern "system" fn ds_query_get_data(_this: *mut c_void, pfe: *const FormatEtc) -> HRESULT {
    if pfe.is_null() {
        return E_FAIL;
    }
    if wanted(&*pfe) {
        S_OK
    } else {
        DV_E_FORMATETC
    }
}

unsafe extern "system" fn ds_get_canonical(
    _this: *mut c_void,
    _in: *const FormatEtc,
    out: *mut FormatEtc,
) -> HRESULT {
    if !out.is_null() {
        (*out).ptd = std::ptr::null_mut();
    }
    E_NOTIMPL
}

unsafe extern "system" fn ds_set_data(
    _this: *mut c_void,
    _fe: *mut FormatEtc,
    _med: *mut StgMedium,
    _release: i32,
) -> HRESULT {
    E_NOTIMPL
}

unsafe extern "system" fn ds_enum_format_etc(
    _this: *mut c_void,
    direction: u32,
    out: *mut *mut c_void,
) -> HRESULT {
    if out.is_null() {
        return E_FAIL;
    }
    *out = std::ptr::null_mut();
    if direction != DATADIR_GET {
        return E_NOTIMPL;
    }
    let e = Box::into_raw(Box::new(EnumFmt {
        vtbl: &ENUM_VTBL,
        pos: 0,
        refs: AtomicU32::new(1),
    }));
    *out = e as *mut c_void;
    S_OK
}

unsafe extern "system" fn ds_d_advise(
    _this: *mut c_void,
    _fe: *const FormatEtc,
    _flags: u32,
    _sink: *mut c_void,
    _conn: *mut u32,
) -> HRESULT {
    OLE_E_ADVISENOTSUPPORTED
}

unsafe extern "system" fn ds_d_unadvise(_this: *mut c_void, _conn: u32) -> HRESULT {
    OLE_E_ADVISENOTSUPPORTED
}

unsafe extern "system" fn ds_enum_d_advise(_this: *mut c_void, _out: *mut *mut c_void) -> HRESULT {
    OLE_E_ADVISENOTSUPPORTED
}

static DATA_VTBL: IDataObjectVtbl = IDataObjectVtbl {
    query_interface: ds_qi,
    add_ref: ds_add_ref,
    release: ds_release,
    get_data: ds_get_data,
    get_data_here: ds_get_data_here,
    query_get_data: ds_query_get_data,
    get_canonical_format_etc: ds_get_canonical,
    set_data: ds_set_data,
    enum_format_etc: ds_enum_format_etc,
    d_advise: ds_d_advise,
    d_unadvise: ds_d_unadvise,
    enum_d_advise: ds_enum_d_advise,
};

// ---- IEnumFORMATETC ----

unsafe extern "system" fn ef_qi(
    this: *mut c_void,
    iid: *const Guid,
    out: *mut *mut c_void,
) -> HRESULT {
    if iid.is_null() || out.is_null() {
        return E_FAIL;
    }
    *out = std::ptr::null_mut();
    if guid_eq(&*iid, &IID_IUNKNOWN) || guid_eq(&*iid, &IID_IENUMFORMATETC) {
        *out = this;
        EnumFmt::from_raw(this).refs.fetch_add(1, Ordering::Relaxed);
        return S_OK;
    }
    E_NOINTERFACE
}

unsafe extern "system" fn ef_add_ref(this: *mut c_void) -> u32 {
    EnumFmt::from_raw(this).refs.fetch_add(1, Ordering::Relaxed) + 1
}

unsafe extern "system" fn ef_release(this: *mut c_void) -> u32 {
    let e = EnumFmt::from_raw(this);
    let r = e.refs.fetch_sub(1, Ordering::Relaxed) - 1;
    if r == 0 {
        drop(Box::from_raw(this as *mut EnumFmt));
    }
    r
}

unsafe extern "system" fn ef_next(
    this: *mut c_void,
    celt: u32,
    rgelt: *mut FormatEtc,
    fetched: *mut u32,
) -> HRESULT {
    if rgelt.is_null() {
        return E_FAIL;
    }
    let e = EnumFmt::from_raw(this);
    let mut n = 0u32;
    // 列挙内容は常に CF_HDROP 1 件だけ
    while n < celt && e.pos == 0 {
        *rgelt.add(n as usize) = FormatEtc {
            cf_format: CF_HDROP,
            ptd: std::ptr::null_mut(),
            dw_aspect: DVASPECT_CONTENT,
            lindex: -1,
            tymed: TYMED_HGLOBAL,
        };
        e.pos += 1;
        n += 1;
    }
    if !fetched.is_null() {
        *fetched = n;
    }
    if n == celt {
        S_OK
    } else {
        S_FALSE
    }
}

unsafe extern "system" fn ef_skip(this: *mut c_void, celt: u32) -> HRESULT {
    let e = EnumFmt::from_raw(this);
    e.pos = e.pos.saturating_add(celt);
    S_OK
}

unsafe extern "system" fn ef_reset(this: *mut c_void) -> HRESULT {
    EnumFmt::from_raw(this).pos = 0;
    S_OK
}

unsafe extern "system" fn ef_clone(_this: *mut c_void, _out: *mut *mut c_void) -> HRESULT {
    E_NOTIMPL
}

static ENUM_VTBL: IEnumFORMATETCVtbl = IEnumFORMATETCVtbl {
    query_interface: ef_qi,
    add_ref: ef_add_ref,
    release: ef_release,
    next: ef_next,
    skip: ef_skip,
    reset: ef_reset,
    clone: ef_clone,
};

// ---- IDropSource ----

unsafe extern "system" fn src_qi(
    this: *mut c_void,
    iid: *const Guid,
    out: *mut *mut c_void,
) -> HRESULT {
    if iid.is_null() || out.is_null() {
        return E_FAIL;
    }
    *out = std::ptr::null_mut();
    if guid_eq(&*iid, &IID_IUNKNOWN) || guid_eq(&*iid, &IID_IDROPSOURCE) {
        *out = this;
        DropSource::from_raw(this)
            .refs
            .fetch_add(1, Ordering::Relaxed);
        return S_OK;
    }
    E_NOINTERFACE
}

unsafe extern "system" fn src_add_ref(this: *mut c_void) -> u32 {
    DropSource::from_raw(this)
        .refs
        .fetch_add(1, Ordering::Relaxed)
        + 1
}

unsafe extern "system" fn src_release(this: *mut c_void) -> u32 {
    let s = DropSource::from_raw(this);
    let r = s.refs.fetch_sub(1, Ordering::Relaxed) - 1;
    if r == 0 {
        drop(Box::from_raw(this as *mut DropSource));
    }
    r
}

/// ボタンを離した=ドロップ、Esc=キャンセル、それ以外は継続。
/// fEscapePressed は DoDragDrop が Esc を検出して渡してくる
unsafe extern "system" fn src_query_continue(
    _this: *mut c_void,
    f_escape_pressed: i32,
    _key_state: u32,
) -> HRESULT {
    if f_escape_pressed != 0 || host::cancelled() {
        DRAGDROP_S_CANCEL
    } else if !crate::BTN_W[0].load(Ordering::Relaxed) {
        DRAGDROP_S_DROP
    } else {
        S_OK
    }
}

unsafe extern "system" fn src_give_feedback(_this: *mut c_void, _effect: u32) -> HRESULT {
    #[cfg(test)]
    TEST_DRAG_READY.store(true, Ordering::Relaxed);
    DRAGDROP_S_USEDEFAULTCURSORS
}

static SRC_VTBL: IDropSourceVtbl = IDropSourceVtbl {
    query_interface: src_qi,
    add_ref: src_add_ref,
    release: src_release,
    query_continue_drag: src_query_continue,
    give_feedback: src_give_feedback,
};

// ---- 起動 ----

/// 受信済みファイル群で OLE ドラッグを開始する(別スレッド。呼び出し元の
/// 受信ループは DoDragDrop のモーダルループの影響を受けない)。
/// 入力を捕捉できるウィンドウを同じSTAに用意する。背景のSTAから
/// DoDragDropだけを呼ぶと、他アプリ上のMouseUpを受け取れず終了しない。
pub fn start(paths: Vec<String>) {
    std::thread::spawn(move || unsafe {
        // ドラッグ&ドロップは STA 必須(既に初期化済みなら S_FALSE が返る=成功扱い)
        let hr_init = OleInitialize(std::ptr::null());
        if hr_init != S_OK && hr_init != S_FALSE {
            println!("[drag] OleInitialize 失敗(0x{hr_init:08x})");
            fallback(&paths);
            return;
        }
        if host::cancelled() || !crate::BTN_W[0].load(Ordering::Relaxed) {
            OleUninitialize();
            return;
        }
        let Some(host) = host::Host::create() else {
            fallback(&paths);
            OleUninitialize();
            return;
        };
        DRAG_THREAD.store(
            windows_sys::Win32::System::Threading::GetCurrentThreadId(),
            Ordering::Relaxed,
        );
        let ds = Box::into_raw(Box::new(DataSource {
            vtbl: &DATA_VTBL,
            paths: paths.clone(),
            refs: AtomicU32::new(1),
        })) as *mut c_void;
        let src = Box::into_raw(Box::new(DropSource {
            vtbl: &SRC_VTBL,
            refs: AtomicU32::new(1),
        })) as *mut c_void;
        println!(
            "[drag] DoDragDrop 開始({} 件、ボタンを離した位置へドロップ)",
            paths.len()
        );
        let mut effect: u32 = DROPEFFECT_NONE;
        let hr = DoDragDrop(ds, src, DROPEFFECT_COPY, &mut effect);
        DRAG_THREAD.store(0, Ordering::Relaxed);
        ds_release(ds); // 我々の保持分(DoDragDrop 内部の参照は既に解放済み)
        src_release(src);
        drop(host);
        if effect == DROPEFFECT_COPY {
            println!("[drag] ドロップ完了(コピー。元は Downloads\\Tsunagu に残ります)");
        } else if hr == DRAGDROP_S_CANCEL {
            println!("[drag] ドロップを取り消しました");
        } else {
            println!("[drag] ドロップ不成立(hr=0x{hr:08x})");
            fallback(&paths);
        }
        OleUninitialize();
    });
}

fn fallback(_paths: &[String]) {
    println!("[drag] 受信ファイルは Downloads\\Tsunagu に保持しています");
    #[cfg(not(test))]
    crate::tray::notify("Tsunagu", "ドロップを開始・完了できませんでした。受信ファイルはDownloads\\Tsunaguに保存されています。掴んだまま境界を越えて、相手の画面上で離すとその場に置けます");
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::System::Com::DVASPECT_CONTENT;

    #[link(name = "uuid", kind = "static")]
    unsafe extern "system" {
        #[link_name = "IID_IDropSource"]
        static SDK_IDROP_SOURCE: Guid;
        #[link_name = "IID_IShellItem"]
        static SDK_ISHELL_ITEM: Guid;
        #[link_name = "IID_IDropTarget"]
        static SDK_IDROP_TARGET: Guid;
        #[link_name = "BHID_SFUIObject"]
        static SDK_SHELL_UI_OBJECT: Guid;
    }

    #[test]
    fn shell_can_enumerate_file_contents() {
        unsafe {
            let mut enumerator = std::ptr::null_mut();
            assert_eq!(
                ds_enum_format_etc(std::ptr::null_mut(), DATADIR_GET, &mut enumerator),
                S_OK
            );
            let mut format: FormatEtc = std::mem::zeroed();
            let mut fetched = 0;
            let result = ef_next(enumerator, 1, &mut format, &mut fetched);
            ef_release(enumerator);
            assert_eq!(result, S_OK);
            assert_eq!(fetched, 1);
            assert_eq!(format.cf_format, CF_HDROP);
            assert_eq!(
                format.dw_aspect, DVASPECT_CONTENT,
                "Shellへファイル本体を提供する"
            );
            assert_eq!(format.lindex, -1);
            assert_eq!(ds_query_get_data(std::ptr::null_mut(), &format), S_OK);
        }
    }

    #[test]
    fn ole_can_query_the_standard_drop_source_interface() {
        unsafe {
            let source = Box::into_raw(Box::new(DropSource {
                vtbl: &SRC_VTBL,
                refs: AtomicU32::new(1),
            })) as *mut c_void;
            let mut queried = std::ptr::null_mut();
            let result = src_qi(source, &SDK_IDROP_SOURCE, &mut queried);
            if !queried.is_null() {
                src_release(queried);
            }
            src_release(source);
            assert_eq!(result, S_OK, "OLEの標準インターフェース要求を拒否しない");
            assert_eq!(queried, source);
        }
    }

    #[test]
    fn shell_can_read_the_received_unicode_file_paths() {
        use windows_sys::Win32::Foundation::GlobalFree;
        use windows_sys::Win32::System::Memory::{GlobalLock, GlobalUnlock};
        let paths = vec![
            String::from(r"C:\Tsunagu test\資料.txt"),
            String::from(r"C:\Tsunagu test\second.txt"),
        ];
        unsafe {
            let source = Box::into_raw(Box::new(DataSource {
                vtbl: &DATA_VTBL,
                paths: paths.clone(),
                refs: AtomicU32::new(1),
            })) as *mut c_void;
            let format = FormatEtc {
                cf_format: CF_HDROP,
                ptd: std::ptr::null_mut(),
                dw_aspect: DVASPECT_CONTENT,
                lindex: -1,
                tymed: TYMED_HGLOBAL,
            };
            let mut medium: StgMedium = std::mem::zeroed();
            let result = ds_get_data(source, &format, &mut medium);
            ds_release(source);
            assert_eq!(result, S_OK);
            let raw = GlobalLock(medium.h_global) as *const u32;
            assert!(!raw.is_null());
            let offset = *raw as usize;
            let wide = *raw.add(4);
            let expected: Vec<u16> = paths
                .iter()
                .flat_map(|p| p.encode_utf16().chain([0]))
                .chain([0])
                .collect();
            let actual = std::slice::from_raw_parts(
                (raw as *const u8).add(offset) as *const u16,
                expected.len(),
            )
            .to_vec();
            GlobalUnlock(medium.h_global);
            GlobalFree(medium.h_global);
            assert_eq!(medium.tymed, TYMED_HGLOBAL);
            assert!(medium.p_unk_for_release.is_null());
            assert_eq!(wide, 1);
            assert_eq!(actual, expected);
        }
    }

    #[test]
    fn windows_shell_accepts_and_copies_the_data_object() {
        #[link(name = "shell32")]
        unsafe extern "system" {
            fn SHCreateItemFromParsingName(
                name: *const u16,
                context: *mut c_void,
                iid: *const Guid,
                out: *mut *mut c_void,
            ) -> HRESULT;
        }
        #[repr(C)]
        struct UnknownVtbl {
            qi: QiFn,
            add_ref: AddRefFn,
            release: ReleaseFn,
        }
        #[repr(C)]
        struct ShellItemVtbl {
            unknown: UnknownVtbl,
            bind: unsafe extern "system" fn(
                *mut c_void,
                *mut c_void,
                *const Guid,
                *const Guid,
                *mut *mut c_void,
            ) -> HRESULT,
        }
        #[repr(C)]
        struct DropTargetVtbl {
            unknown: UnknownVtbl,
            enter: unsafe extern "system" fn(
                *mut c_void,
                *mut c_void,
                u32,
                windows_sys::Win32::Foundation::POINTL,
                *mut u32,
            ) -> HRESULT,
            over: unsafe extern "system" fn(
                *mut c_void,
                u32,
                windows_sys::Win32::Foundation::POINTL,
                *mut u32,
            ) -> HRESULT,
            leave: unsafe extern "system" fn(*mut c_void) -> HRESULT,
            drop: unsafe extern "system" fn(
                *mut c_void,
                *mut c_void,
                u32,
                windows_sys::Win32::Foundation::POINTL,
                *mut u32,
            ) -> HRESULT,
        }
        struct ComPtr(*mut c_void);
        impl Drop for ComPtr {
            fn drop(&mut self) {
                if !self.0.is_null() {
                    unsafe {
                        let vtbl = *(self.0 as *const *const UnknownVtbl);
                        ((*vtbl).release)(self.0);
                    }
                }
            }
        }
        struct Ole;
        impl Drop for Ole {
            fn drop(&mut self) {
                unsafe {
                    OleUninitialize();
                }
            }
        }
        struct Temp(std::path::PathBuf);
        impl Drop for Temp {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }

        unsafe {
            assert!(OleInitialize(std::ptr::null()) >= 0);
            let _ole = Ole;
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let temp = Temp(
                std::env::temp_dir()
                    .join(format!("tsunagu-shell-drop-{}-{stamp}", std::process::id())),
            );
            let dest = temp.0.join("destination");
            std::fs::create_dir_all(&dest).unwrap();
            let source_path = temp.0.join("越境テスト.txt");
            let contents = b"Tsunagu native Shell drop test\n";
            std::fs::write(&source_path, contents).unwrap();
            let dest_w: Vec<u16> = dest
                .as_os_str()
                .to_string_lossy()
                .encode_utf16()
                .chain([0])
                .collect();
            let mut item = ComPtr(std::ptr::null_mut());
            assert_eq!(
                SHCreateItemFromParsingName(
                    dest_w.as_ptr(),
                    std::ptr::null_mut(),
                    &SDK_ISHELL_ITEM,
                    &mut item.0
                ),
                S_OK
            );
            let shell = *(item.0 as *const *const ShellItemVtbl);
            let mut target = ComPtr(std::ptr::null_mut());
            assert_eq!(
                ((*shell).bind)(
                    item.0,
                    std::ptr::null_mut(),
                    &SDK_SHELL_UI_OBJECT,
                    &SDK_IDROP_TARGET,
                    &mut target.0
                ),
                S_OK
            );
            let data = ComPtr(Box::into_raw(Box::new(DataSource {
                vtbl: &DATA_VTBL,
                paths: vec![source_path.to_string_lossy().into_owned()],
                refs: AtomicU32::new(1),
            })) as *mut c_void);
            let drop_target = *(target.0 as *const *const DropTargetVtbl);
            let point = windows_sys::Win32::Foundation::POINTL { x: 0, y: 0 };
            let mut effect = DROPEFFECT_COPY;
            assert_eq!(
                ((*drop_target).enter)(target.0, data.0, MK_LBUTTON, point, &mut effect),
                S_OK
            );
            assert_eq!(
                effect, DROPEFFECT_COPY,
                "実際のShellフォルダがコピーを受け入れる"
            );
            assert_eq!(
                ((*drop_target).drop)(target.0, data.0, 0, point, &mut effect),
                S_OK
            );
            assert_eq!(effect, DROPEFFECT_COPY);
            assert_eq!(
                std::fs::read(dest.join("越境テスト.txt")).unwrap(),
                contents
            );
            assert_eq!(
                std::fs::read(source_path).unwrap(),
                contents,
                "原本を保持する"
            );
        }
    }
}

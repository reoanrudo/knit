//! 遅延提供の COM 面(Windows)。**目録だけで OLE ドラッグを開始し、内容は
//! 届いた分から `IStream` として返す**。
//!
//! なぜ必要か: 現行は「全ファイルの到着後にドラッグ開始」なので、実測で
//! Wi-Fi 実効 30MB/s のとき 1GB=34秒・10GB=5分41秒 のあいだ物理ボタンを
//! 押し続けることになる。ここでは Explorer が要求した時点で staging の
//! ファイルを待って返すので、**押下保持が転送量の関数にならない**。
//!
//! 既定は無効(`KNIT_LAZY=1` で有効)。vtbl の並びは MSDN の宣言順に合わせる
//! (既存の `IDataObject`/`IDropSource` と同じ方式)。
use super::{StgMedium, FormatEtc, IDataObjectVtbl, Guid, HRESULT, DV_E_FORMATETC, DVASPECT_CONTENT,
            DATADIR_GET, S_OK, E_NOTIMPL, E_NOINTERFACE, E_FAIL, TYMED_HGLOBAL, IID_IUNKNOWN,
            IID_IDATAOBJECT, guid_eq, QiFn, AddRefFn, ReleaseFn};
use std::ffi::c_void;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU16, AtomicU32, Ordering};

/// `TYMED_ISTREAM`
const TYMED_ISTREAM: u32 = 4;
/// `DV_E_LINDEX` / `DV_E_TYMED`(形式は合っているが添字・媒体が違う)
const DV_E_LINDEX: HRESULT = 0x8004_0068u32 as i32;
const DV_E_TYMED: HRESULT = 0x8004_0069u32 as i32;
/// `STGTY_STREAM`
const STGTY_STREAM: u32 = 2;
/// `STREAM_SEEK_SET` / `STREAM_SEEK_CUR` / `STREAM_SEEK_END`
const STREAM_SEEK_SET: u32 = 0;
const STREAM_SEEK_CUR: u32 = 1;
const STREAM_SEEK_END: u32 = 2;
/// ストリーム 1 回の読みで待つ上限(超えたら失敗させる。無限には待たない)
const READ_WAIT: std::time::Duration = std::time::Duration::from_secs(30);

const IID_ISTREAM: Guid = Guid {
    data1: 0x0000_000C,
    data2: 0x0000,
    data3: 0x0000,
    data4: [0xC0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46],
};

// ---------- クリップボード形式(実行時に登録する) ----------

// Win32 直宣言(既存の clipboard.rs と同じ方式。windows-sys のモジュールパスに依存しない)
#[link(name = "user32")]
unsafe extern "system" {
    fn RegisterClipboardFormatW(name: *const u16) -> u32;
}
#[link(name = "kernel32")]
unsafe extern "system" {
    fn GlobalAlloc(u_flags: u32, bytes: usize) -> *mut c_void;
    fn GlobalLock(handle: *mut c_void) -> *mut c_void;
    fn GlobalUnlock(handle: *mut c_void) -> i32;
}
const GMEM_MOVEABLE: u32 = 0x0002;

static CF_DESCRIPTORW: AtomicU16 = AtomicU16::new(0);
static CF_CONTENTS: AtomicU16 = AtomicU16::new(0);

/// `RegisterClipboardFormatW` で "FileGroupDescriptorW" / "FileContents" を登録する。
/// これらは `CFSTR_FILEDESCRIPTORW` / `CFSTR_FILECONTENTS` の実体(値は実行時に決まる)
pub fn ensure_formats() -> (u16, u16) {
    let d = CF_DESCRIPTORW.load(Ordering::Relaxed);
    let c = CF_CONTENTS.load(Ordering::Relaxed);
    if d != 0 && c != 0 {
        return (d, c);
    }
    unsafe {
        let dw = wide("FileGroupDescriptorW");
        let cw = wide("FileContents");
        let rd = RegisterClipboardFormatW(dw.as_ptr());
        let rc = RegisterClipboardFormatW(cw.as_ptr());
        if rd == 0 || rc == 0 {
            return (0, 0);
        }
        let rd = rd as u16;
        let rc = rc as u16;
        CF_DESCRIPTORW.store(rd, Ordering::Relaxed);
        CF_CONTENTS.store(rc, Ordering::Relaxed);
        (rd, rc)
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

// ---------- 供給するファイル(1件) ----------

#[derive(Clone)]
struct Staged {
    /// 受信先のルート(受信側が公開する場所)
    dir: PathBuf,
    /// 目録の相対名(受信側は同名衝突で「名前 (n)」へずらすことがある)
    rel: String,
    size: u64,
    kind: u8,
}

impl Staged {
    /// いま読むべき実体。公開名がずれていてもサイズ一致で見つける
    fn path(&self) -> PathBuf {
        super::lazy::resolve_received(&self.dir, &self.rel, self.size, self.kind)
    }
}

// ---------- IStream ----------

#[repr(C)]
struct IStreamVtbl {
    query_interface: QiFn,
    add_ref: AddRefFn,
    release: ReleaseFn,
    read: unsafe extern "system" fn(*mut c_void, *mut u8, u32, *mut u32) -> HRESULT,
    write: unsafe extern "system" fn(*mut c_void, *const u8, u32, *mut u32) -> HRESULT,
    seek: unsafe extern "system" fn(*mut c_void, i64, u32, *mut u64) -> HRESULT,
    set_size: unsafe extern "system" fn(*mut c_void, u64) -> HRESULT,
    copy_to: unsafe extern "system" fn(*mut c_void, *mut c_void, u64, *mut u64, *mut u64) -> HRESULT,
    commit: unsafe extern "system" fn(*mut c_void, u32) -> HRESULT,
    revert: unsafe extern "system" fn(*mut c_void) -> HRESULT,
    lock_region: unsafe extern "system" fn(*mut c_void, u64, u64, u32) -> HRESULT,
    unlock_region: unsafe extern "system" fn(*mut c_void, u64, u64, u32) -> HRESULT,
    stat: unsafe extern "system" fn(*mut c_void, *mut StatStg, u32) -> HRESULT,
    clone: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> HRESULT,
}

/// `STATSTG`(MSDN の並び。x64 で 80 バイト)
#[repr(C)]
pub struct StatStg {
    name: *const u16,
    type_: u32,
    size: u64,
    mtime: [u32; 2],
    ctime: [u32; 2],
    atime: [u32; 2],
    mode: u32,
    locks: u32,
    clsid: [u8; 16],
    state: u32,
    reserved: u32,
}

#[repr(C)]
struct Stream {
    vtbl: &'static IStreamVtbl,
    file: Staged,
    pos: u64,
    refs: AtomicU32,
}

impl Stream {
    unsafe fn from_raw(this: *mut c_void) -> &'static mut Self {
        &mut *(this as *mut Stream)
    }
}

unsafe extern "system" fn st_qi(this: *mut c_void, iid: *const Guid, out: *mut *mut c_void) -> HRESULT {
    if iid.is_null() || out.is_null() {
        return E_FAIL;
    }
    *out = std::ptr::null_mut();
    if guid_eq(&*iid, &IID_IUNKNOWN) || guid_eq(&*iid, &IID_ISTREAM) {
        *out = this;
        Stream::from_raw(this).refs.fetch_add(1, Ordering::Relaxed);
        return S_OK;
    }
    E_NOINTERFACE
}

unsafe extern "system" fn st_add_ref(this: *mut c_void) -> u32 {
    Stream::from_raw(this).refs.fetch_add(1, Ordering::Relaxed) + 1
}

unsafe extern "system" fn st_release(this: *mut c_void) -> u32 {
    let s = Stream::from_raw(this);
    let r = s.refs.fetch_sub(1, Ordering::Relaxed) - 1;
    if r == 0 {
        drop(Box::from_raw(this as *mut Stream));
    }
    r
}

/// `Read`: **宣言サイズに達するまで待って**から返す(届いた分から読める)。
/// 期限を過ぎたら失敗させる(無限に待たない)
unsafe extern "system" fn st_read(
    this: *mut c_void,
    buf: *mut u8,
    cb: u32,
    read: *mut u32,
) -> HRESULT {
    if !read.is_null() {
        *read = 0;
    }
    if buf.is_null() {
        return E_FAIL;
    }
    let s = Stream::from_raw(this);
    let remaining = s.file.size.saturating_sub(s.pos);
    if remaining == 0 {
        return S_OK; // 終端(0 バイト読了)
    }
    let want = (cb as u64).min(remaining) as u32;
    let need = s.pos + want as u64;
    if !super::lazy::wait_ready(&s.file.path(), need, s.file.kind, READ_WAIT) {
        eprintln!(
            "[drag] 遅延提供: データが {} バイトに達しません(期限 {:?})",
            need, READ_WAIT
        );
        return E_FAIL;
    }
    match std::fs::File::open(s.file.path()) {
        Ok(mut f) => {
            use std::io::{Read, Seek, SeekFrom};
            if f.seek(SeekFrom::Start(s.pos)).is_err() {
                return E_FAIL;
            }
            // 途中までしか来ていない場合に備え、読めた分だけ返す
            let mut tmp = vec![0u8; want as usize];
            match f.read(&mut tmp) {
                Ok(n) => {
                    std::ptr::copy_nonoverlapping(tmp.as_ptr(), buf, n);
                    s.pos += n as u64;
                    if !read.is_null() {
                        *read = n as u32;
                    }
                    S_OK
                }
                Err(_) => E_FAIL,
            }
        }
        Err(_) => E_FAIL,
    }
}

unsafe extern "system" fn st_write(
    _this: *mut c_void,
    _buf: *const u8,
    _cb: u32,
    _written: *mut u32,
) -> HRESULT {
    E_NOTIMPL // 読み出し専用
}

unsafe extern "system" fn st_seek(
    this: *mut c_void,
    move_: i64,
    origin: u32,
    new_pos: *mut u64,
) -> HRESULT {
    let s = Stream::from_raw(this);
    let base = match origin {
        STREAM_SEEK_SET => 0i64,
        STREAM_SEEK_CUR => s.pos as i64,
        STREAM_SEEK_END => s.file.size as i64,
        _ => return E_NOTIMPL,
    };
    let target = base.saturating_add(move_);
    if target < 0 || target as u64 > s.file.size {
        return E_FAIL; // 宣言サイズの外へは動かさない
    }
    s.pos = target as u64;
    if !new_pos.is_null() {
        *new_pos = s.pos;
    }
    S_OK
}

unsafe extern "system" fn st_set_size(_this: *mut c_void, _size: u64) -> HRESULT {
    E_NOTIMPL
}

unsafe extern "system" fn st_copy_to(
    _this: *mut c_void,
    _dst: *mut c_void,
    _cb: u64,
    _read: *mut u64,
    _written: *mut u64,
) -> HRESULT {
    E_NOTIMPL
}

unsafe extern "system" fn st_commit(_this: *mut c_void, _flags: u32) -> HRESULT {
    E_NOTIMPL
}

unsafe extern "system" fn st_revert(_this: *mut c_void) -> HRESULT {
    E_NOTIMPL
}

unsafe extern "system" fn st_lock(_this: *mut c_void, _off: u64, _len: u64, _flags: u32) -> HRESULT {
    E_NOTIMPL
}

unsafe extern "system" fn st_unlock(
    _this: *mut c_void,
    _off: u64,
    _len: u64,
    _flags: u32,
) -> HRESULT {
    E_NOTIMPL
}

/// `Stat`: 宣言サイズを返す(Explorer はこれで進捗と全体量を出す)
unsafe extern "system" fn st_stat(this: *mut c_void, out: *mut StatStg, _flags: u32) -> HRESULT {
    if out.is_null() {
        return E_FAIL;
    }
    let s = Stream::from_raw(this);
    std::ptr::write_bytes(out as *mut u8, 0, std::mem::size_of::<StatStg>());
    (*out).type_ = STGTY_STREAM;
    (*out).size = s.file.size;
    S_OK
}

unsafe extern "system" fn st_clone(_this: *mut c_void, _out: *mut *mut c_void) -> HRESULT {
    E_NOTIMPL
}

static STREAM_VTBL: IStreamVtbl = IStreamVtbl {
    query_interface: st_qi,
    add_ref: st_add_ref,
    release: st_release,
    read: st_read,
    write: st_write,
    seek: st_seek,
    set_size: st_set_size,
    copy_to: st_copy_to,
    commit: st_commit,
    revert: st_revert,
    lock_region: st_lock,
    unlock_region: st_unlock,
    stat: st_stat,
    clone: st_clone,
};

// ---------- IDataObject(遅延提供) ----------

#[repr(C)]
pub struct LazySource {
    vtbl: &'static IDataObjectVtbl,
    /// `FILEGROUPDESCRIPTORW` の実体(1回だけ作る)
    descriptor: Vec<u8>,
    files: Vec<Staged>,
    refs: AtomicU32,
}

impl LazySource {
    unsafe fn from_raw(this: *mut c_void) -> &'static mut Self {
        &mut *(this as *mut LazySource)
    }

    /// 目録と受信先から作る。`files` は (受信先ルート, 相対名, サイズ, kind) の並び
    pub fn new(dir: &std::path::Path, descriptor: Vec<u8>, files: Vec<(String, u64, u8)>) -> Box<Self> {
        Box::new(LazySource {
            vtbl: &LAZY_VTBL,
            descriptor,
            files: files
                .into_iter()
                .map(|(rel, size, kind)| Staged {
                    dir: dir.to_path_buf(),
                    rel,
                    size,
                    kind,
                })
                .collect(),
            refs: AtomicU32::new(1),
        })
    }
}

/// この形式を提供できるか(このオブジェクト版の `wanted`)
unsafe fn lazy_wanted(fe: &FormatEtc) -> bool {
    if fe.dw_aspect != DVASPECT_CONTENT {
        return false;
    }
    let (d, c) = ensure_formats();
    if d == 0 || c == 0 {
        return false;
    }
    if fe.cf_format == d {
        return fe.lindex == -1 && (fe.tymed & TYMED_HGLOBAL) != 0;
    }
    if fe.cf_format == c {
        return fe.lindex >= 0 && (fe.tymed & TYMED_ISTREAM) != 0;
    }
    false
}

unsafe extern "system" fn ls_qi(this: *mut c_void, iid: *const Guid, out: *mut *mut c_void) -> HRESULT {
    if iid.is_null() || out.is_null() {
        return E_FAIL;
    }
    *out = std::ptr::null_mut();
    if guid_eq(&*iid, &IID_IUNKNOWN) || guid_eq(&*iid, &IID_IDATAOBJECT) {
        *out = this;
        LazySource::from_raw(this).refs.fetch_add(1, Ordering::Relaxed);
        return S_OK;
    }
    E_NOINTERFACE
}

unsafe extern "system" fn ls_add_ref(this: *mut c_void) -> u32 {
    LazySource::from_raw(this).refs.fetch_add(1, Ordering::Relaxed) + 1
}

unsafe extern "system" fn ls_release(this: *mut c_void) -> u32 {
    let d = LazySource::from_raw(this);
    let r = d.refs.fetch_sub(1, Ordering::Relaxed) - 1;
    if r == 0 {
        drop(Box::from_raw(this as *mut LazySource));
    }
    r
}

unsafe extern "system" fn ls_get_data(
    this: *mut c_void,
    pfe: *const FormatEtc,
    pmed: *mut StgMedium,
) -> HRESULT {
    if pfe.is_null() || pmed.is_null() {
        return E_FAIL;
    }
    let d = LazySource::from_raw(this);
    let fe = &*pfe;
    if !lazy_wanted(fe) {
        return DV_E_FORMATETC;
    }
    let (dfmt, cfmt) = ensure_formats();
    if fe.cf_format == dfmt {
        let Some(h) = hglobal_from(&d.descriptor) else {
            return E_FAIL;
        };
        (*pmed).tymed = TYMED_HGLOBAL;
        (*pmed).h_global = h;
        (*pmed).p_unk_for_release = std::ptr::null_mut();
        return S_OK;
    }
    if fe.cf_format == cfmt {
        let idx = fe.lindex as usize;
        if idx >= d.files.len() {
            return DV_E_LINDEX;
        }
        let s = Box::into_raw(Box::new(Stream {
            vtbl: &STREAM_VTBL,
            file: d.files[idx].clone(),
            pos: 0,
            refs: AtomicU32::new(1),
        })) as *mut c_void;
        (*pmed).tymed = TYMED_ISTREAM;
        // union の同じ位置(ポインタ)にストリームを入れる
        (*pmed).h_global = s;
        (*pmed).p_unk_for_release = std::ptr::null_mut();
        return S_OK;
    }
    DV_E_FORMATETC
}

unsafe extern "system" fn ls_get_data_here(
    _this: *mut c_void,
    _pfe: *const FormatEtc,
    _pmed: *mut StgMedium,
) -> HRESULT {
    E_NOTIMPL
}

unsafe extern "system" fn ls_query_get_data(_this: *mut c_void, pfe: *const FormatEtc) -> HRESULT {
    if pfe.is_null() {
        return E_FAIL;
    }
    let fe = &*pfe;
    let (d, c) = ensure_formats();
    if d == 0 || c == 0 {
        return E_FAIL;
    }
    // 形式は合っているが添字・媒体が違う場合は、より具体的なエラーを返す(COM の作法)
    if (fe.cf_format == d || fe.cf_format == c) && fe.dw_aspect != DVASPECT_CONTENT {
        return DV_E_FORMATETC;
    }
    if fe.cf_format == d && fe.lindex != -1 {
        return DV_E_LINDEX;
    }
    if fe.cf_format == c && fe.lindex < 0 {
        return DV_E_LINDEX;
    }
    if (fe.cf_format == d && (fe.tymed & TYMED_HGLOBAL) == 0)
        || (fe.cf_format == c && (fe.tymed & TYMED_ISTREAM) == 0)
    {
        return DV_E_TYMED;
    }
    if lazy_wanted(fe) {
        S_OK
    } else {
        DV_E_FORMATETC
    }
}

unsafe extern "system" fn ls_get_canonical(
    _this: *mut c_void,
    _in: *const FormatEtc,
    out: *mut FormatEtc,
) -> HRESULT {
    if !out.is_null() {
        (*out).ptd = std::ptr::null_mut();
    }
    E_NOTIMPL
}

unsafe extern "system" fn ls_set_data(
    _this: *mut c_void,
    _fe: *mut FormatEtc,
    _med: *mut StgMedium,
    _release: i32,
) -> HRESULT {
    E_NOTIMPL
}

unsafe extern "system" fn ls_enum_format_etc(
    this: *mut c_void,
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
    let n = LazySource::from_raw(this).files.len();
    let e = Box::into_raw(Box::new(LazyEnum {
        vtbl: &LAZY_ENUM_VTBL,
        pos: 0,
        count: n,
        refs: AtomicU32::new(1),
    })) as *mut c_void;
    *out = e;
    S_OK
}

unsafe extern "system" fn ls_d_advise(
    _this: *mut c_void,
    _fe: *const FormatEtc,
    _flags: u32,
    _sink: *mut c_void,
    _conn: *mut u32,
) -> HRESULT {
    E_NOTIMPL
}

unsafe extern "system" fn ls_d_unadvise(_this: *mut c_void, _conn: u32) -> HRESULT {
    E_NOTIMPL
}

unsafe extern "system" fn ls_enum_d_advise(_this: *mut c_void, _out: *mut *mut c_void) -> HRESULT {
    E_NOTIMPL
}

static LAZY_VTBL: IDataObjectVtbl = IDataObjectVtbl {
    query_interface: ls_qi,
    add_ref: ls_add_ref,
    release: ls_release,
    get_data: ls_get_data,
    get_data_here: ls_get_data_here,
    query_get_data: ls_query_get_data,
    get_canonical_format_etc: ls_get_canonical,
    set_data: ls_set_data,
    enum_format_etc: ls_enum_format_etc,
    d_advise: ls_d_advise,
    d_unadvise: ls_d_unadvise,
    enum_d_advise: ls_enum_d_advise,
};

// ---------- IEnumFORMATETC(遅延提供の形式を列挙) ----------

#[repr(C)]
struct LazyEnum {
    vtbl: &'static IEnumFORMATETCVtbl,
    pos: u32,
    /// 供給するファイル数(FILECONTENTS を添字ごとに1件ずつ列挙する)
    count: usize,
    refs: AtomicU32,
}

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

impl LazyEnum {
    unsafe fn from_raw(this: *mut c_void) -> &'static mut Self {
        &mut *(this as *mut LazyEnum)
    }
    /// 添字 → 形式(0 が目録、以降が各ファイルの内容)
    fn format_at(&self, i: u32) -> FormatEtc {
        let (d, c) = ensure_formats();
        if i == 0 {
            FormatEtc {
                cf_format: d,
                ptd: std::ptr::null_mut(),
                dw_aspect: DVASPECT_CONTENT,
                lindex: -1,
                tymed: TYMED_HGLOBAL,
            }
        } else {
            FormatEtc {
                cf_format: c,
                ptd: std::ptr::null_mut(),
                dw_aspect: DVASPECT_CONTENT,
                lindex: (i - 1) as i32,
                tymed: TYMED_ISTREAM,
            }
        }
    }
    fn total(&self) -> u32 {
        1 + self.count as u32
    }
}

unsafe extern "system" fn le_qi(this: *mut c_void, iid: *const Guid, out: *mut *mut c_void) -> HRESULT {
    if iid.is_null() || out.is_null() {
        return E_FAIL;
    }
    *out = std::ptr::null_mut();
    if guid_eq(&*iid, &IID_IUNKNOWN) {
        *out = this;
        LazyEnum::from_raw(this).refs.fetch_add(1, Ordering::Relaxed);
        return S_OK;
    }
    E_NOINTERFACE
}

unsafe extern "system" fn le_add_ref(this: *mut c_void) -> u32 {
    LazyEnum::from_raw(this).refs.fetch_add(1, Ordering::Relaxed) + 1
}

unsafe extern "system" fn le_release(this: *mut c_void) -> u32 {
    let e = LazyEnum::from_raw(this);
    let r = e.refs.fetch_sub(1, Ordering::Relaxed) - 1;
    if r == 0 {
        drop(Box::from_raw(this as *mut LazyEnum));
    }
    r
}

unsafe extern "system" fn le_next(
    this: *mut c_void,
    celt: u32,
    out: *mut FormatEtc,
    fetched: *mut u32,
) -> HRESULT {
    if out.is_null() {
        return E_FAIL;
    }
    let e = LazyEnum::from_raw(this);
    let mut n = 0u32;
    while n < celt && e.pos < e.total() {
        *out.add(n as usize) = e.format_at(e.pos);
        e.pos += 1;
        n += 1;
    }
    if !fetched.is_null() {
        *fetched = n;
    }
    if n == celt {
        S_OK
    } else {
        1 // S_FALSE(要求より少ない)
    }
}

unsafe extern "system" fn le_skip(this: *mut c_void, celt: u32) -> HRESULT {
    let e = LazyEnum::from_raw(this);
    e.pos = (e.pos + celt).min(e.total());
    S_OK
}

unsafe extern "system" fn le_reset(this: *mut c_void) -> HRESULT {
    LazyEnum::from_raw(this).pos = 0;
    S_OK
}

unsafe extern "system" fn le_clone(_this: *mut c_void, _out: *mut *mut c_void) -> HRESULT {
    E_NOTIMPL
}

static LAZY_ENUM_VTBL: IEnumFORMATETCVtbl = IEnumFORMATETCVtbl {
    query_interface: le_qi,
    add_ref: le_add_ref,
    release: le_release,
    next: le_next,
    skip: le_skip,
    reset: le_reset,
    clone: le_clone,
};

// ---------- メモリ ----------

/// バイト列を `HGLOBAL`(TYMED_HGLOBAL 用)へ写す。受け手の `ReleaseStgMedium` が解放する
unsafe fn hglobal_from(bytes: &[u8]) -> Option<*mut c_void> {
    let h = GlobalAlloc(GMEM_MOVEABLE, bytes.len().max(1));
    if h.is_null() {
        return None;
    }
    let p = GlobalLock(h);
    if p.is_null() {
        return None;
    }
    std::ptr::copy_nonoverlapping(bytes.as_ptr(), p as *mut u8, bytes.len());
    GlobalUnlock(h);
    Some(h)
}

/// 解放(`IDataObject` の参照を落とす)
pub unsafe fn release(obj: *mut c_void) {
    ls_release(obj);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **ABI の固定**: COM に渡す構造体の大きさが MSDN の定義と一致すること。
    /// ずれると Explorer 側が隣のフィールドを読んで壊れる(実行時にしか出ない種類の事故を
    /// ここで止める)
    #[test]
    fn the_com_structs_match_the_documented_layout() {
        if std::mem::size_of::<usize>() != 8 {
            return; // 32bit では桁が変わる
        }
        assert_eq!(std::mem::size_of::<StatStg>(), 80, "STATSTG は 80 バイト(x64)");
        assert_eq!(std::mem::size_of::<super::FormatEtc>(), 24, "FORMATETC は 24 バイト");
        assert_eq!(std::mem::size_of::<super::StgMedium>(), 24, "STGMEDIUM は 24 バイト");
        assert_eq!(
            std::mem::size_of::<super::Guid>(),
            16,
            "GUID/IID は 16 バイト"
        );
    }

    #[test]
    fn the_stream_and_descriptor_formats_are_registered_once() {
        let (d, c) = ensure_formats();
        assert_ne!(d, 0, "FileGroupDescriptorW を登録できる");
        assert_ne!(c, 0, "FileContents を登録できる");
        assert_ne!(d, c, "別の形式として登録される");
        assert_eq!(ensure_formats(), (d, c), "2 回目は同じ値を返す(作り直さない)");
    }

    #[test]
    fn wanted_accepts_only_the_documented_shapes() {
        let (d, c) = ensure_formats();
        let fe = |cf, lindex, tymed| FormatEtc {
            cf_format: cf,
            ptd: std::ptr::null_mut(),
            dw_aspect: DVASPECT_CONTENT,
            lindex,
            tymed,
        };
        unsafe {
            assert!(lazy_wanted(&fe(d, -1, TYMED_HGLOBAL)), "目録は lindex=-1 + HGLOBAL");
            assert!(!lazy_wanted(&fe(d, 0, TYMED_HGLOBAL)), "目録に添字は無い");
            assert!(!lazy_wanted(&fe(d, -1, TYMED_ISTREAM)), "目録は HGLOBAL のみ");
            assert!(lazy_wanted(&fe(c, 0, TYMED_ISTREAM)), "内容は添字つき + ISTREAM");
            assert!(!lazy_wanted(&fe(c, -1, TYMED_ISTREAM)), "内容に添字は必須");
            assert!(!lazy_wanted(&fe(c, 0, TYMED_HGLOBAL)), "内容は ISTREAM のみ");
            assert!(!lazy_wanted(&fe(0, -1, TYMED_HGLOBAL)), "未知の形式は受けない");
        }
    }

    #[test]
    fn format_enumeration_lists_the_descriptor_then_each_file() {
        let e = LazyEnum {
            vtbl: &LAZY_ENUM_VTBL,
            pos: 0,
            count: 2,
            refs: AtomicU32::new(1),
        };
        assert_eq!(e.total(), 3, "目録 1 件 + 各ファイル");
        let f0 = e.format_at(0);
        assert_eq!(f0.lindex, -1, "目録は添字 -1");
        assert_eq!(f0.tymed, TYMED_HGLOBAL);
        let f1 = e.format_at(1);
        assert_eq!((f1.lindex, f1.tymed), (0, TYMED_ISTREAM));
        let f2 = e.format_at(2);
        assert_eq!((f2.lindex, f2.tymed), (1, TYMED_ISTREAM), "2件目は添字 1");
    }
}

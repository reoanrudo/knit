use std::{
    ffi::c_void,
    io,
    io::{Read, Write},
    path::PathBuf,
    ptr,
};
#[repr(C)]
struct Blob {
    len: u32,
    data: *mut u8,
}
#[link(name = "crypt32")]
unsafe extern "system" {
    fn CryptProtectData(
        input: *const Blob,
        description: *const u16,
        entropy: *const Blob,
        reserved: *const c_void,
        prompt: *const c_void,
        flags: u32,
        output: *mut Blob,
    ) -> i32;
    fn CryptUnprotectData(
        input: *const Blob,
        description: *mut *mut u16,
        entropy: *const Blob,
        reserved: *const c_void,
        prompt: *const c_void,
        flags: u32,
        output: *mut Blob,
    ) -> i32;
}
#[link(name = "bcrypt")]
unsafe extern "system" {
    fn BCryptGenRandom(algorithm: *mut c_void, buffer: *mut u8, len: u32, flags: u32) -> i32;
}
#[link(name = "kernel32")]
unsafe extern "system" {
    fn LocalFree(memory: *mut c_void) -> *mut c_void;
    fn MoveFileExW(from: *const u16, to: *const u16, flags: u32) -> i32;
}
fn path() -> io::Result<PathBuf> {
    crate::envutil::data_dir()
        .map(|dir| dir.join("connection.dpapi"))
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "ユーザーの保存先を確認できません。",
            )
        })
}
pub(super) fn random(bytes: &mut [u8]) -> io::Result<()> {
    let status =
        unsafe { BCryptGenRandom(ptr::null_mut(), bytes.as_mut_ptr(), bytes.len() as u32, 2) };
    if status == 0 {
        Ok(())
    } else {
        Err(io::Error::other("接続キーを生成できません。"))
    }
}
fn protect(bytes: &[u8], encrypt: bool) -> io::Result<Vec<u8>> {
    unsafe {
        let input = Blob {
            len: bytes.len() as u32,
            data: bytes.as_ptr() as _,
        };
        let mut output = Blob {
            len: 0,
            data: ptr::null_mut(),
        };
        let ok = if encrypt {
            CryptProtectData(
                &input,
                ptr::null(),
                ptr::null(),
                ptr::null(),
                ptr::null(),
                1,
                &mut output,
            )
        } else {
            CryptUnprotectData(
                &input,
                ptr::null_mut(),
                ptr::null(),
                ptr::null(),
                ptr::null(),
                1,
                &mut output,
            )
        };
        if ok == 0 {
            return Err(io::Error::other(
                "保存した接続キーを保護・読み出しできません。",
            ));
        }
        if output.data.is_null() || output.len == 0 {
            if !output.data.is_null() {
                LocalFree(output.data.cast());
            }
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "空の接続キーです。",
            ));
        }
        let data = std::slice::from_raw_parts(output.data, output.len as usize).to_vec();
        // OSが返す復号バッファは解放前に消去する。
        for i in 0..output.len as usize {
            ptr::write_volatile(output.data.add(i), 0);
        }
        LocalFree(output.data.cast());
        Ok(data)
    }
}
pub(super) fn load() -> io::Result<Option<String>> {
    load_at(&path()?)
}
fn load_at(path: &std::path::Path) -> io::Result<Option<String>> {
    let file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    let mut bytes = Vec::new();
    file.take(16385).read_to_end(&mut bytes)?;
    if bytes.len() > 16384 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "保存した接続キーが不正です。",
        ));
    }
    String::from_utf8(protect(&bytes, false)?)
        .map(Some)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "接続キーを読み取れません。"))
}
pub(super) fn save(token: &str) -> io::Result<()> {
    save_at(&path()?, token)
}
/// 登録の全初期化: 保護した接続キーのファイルを削除する。
/// すでに無い(NotFound)は成功として扱う(冪等)
pub(super) fn delete() -> io::Result<()> {
    match std::fs::remove_file(path()?) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}
fn save_at(destination: &std::path::Path, token: &str) -> io::Result<()> {
    if destination.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "登録済みの接続キーがあります。",
        ));
    }
    let encrypted = protect(token.as_bytes(), true)?;
    std::fs::create_dir_all(destination.parent().unwrap())?;
    let temp = destination.with_extension(format!("{}.tmp", std::process::id()));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)?;
    let result = (|| {
        file.write_all(&encrypted)?;
        file.sync_all()?;
        drop(file);
        use std::os::windows::ffi::OsStrExt;
        let from: Vec<u16> = temp.as_os_str().encode_wide().chain(Some(0)).collect();
        let to: Vec<u16> = destination
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect();
        // WRITE_THROUGHのみ。既存の資格情報を上書きしない。
        if unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), 8) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn protected_file_roundtrip_and_no_overwrite() {
        let token = super::super::generate().unwrap();
        let dir = std::env::temp_dir().join(format!(
            "knit-key-test-{}-{}",
            std::process::id(),
            &token[..12]
        ));
        std::fs::create_dir(&dir).unwrap();
        struct Cleanup(std::path::PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let _cleanup = Cleanup(dir.clone());
        let file = dir.join("connection.dpapi");
        assert!(load_at(&file).unwrap().is_none());
        save_at(&file, &token).unwrap();
        assert_eq!(load_at(&file).unwrap().as_deref(), Some(token.as_str()));
        let data = std::fs::read(&file).unwrap();
        assert!(!data.windows(64).any(|v| v == token.as_bytes()));
        assert!(save_at(&file, &super::super::generate().unwrap()).is_err());
        assert_eq!(data, std::fs::read(&file).unwrap());
        std::fs::write(&file, b"broken").unwrap();
        assert!(load_at(&file).is_err());
    }
    #[test]
    fn dpapi_roundtrip_and_tamper_rejection() {
        let token = super::super::generate().unwrap();
        let mut encrypted = protect(token.as_bytes(), true).unwrap();
        assert_ne!(encrypted, token.as_bytes());
        assert_eq!(protect(&encrypted, false).unwrap(), token.as_bytes());
        encrypted[0] ^= 0xff;
        assert!(protect(&encrypted, false).is_err());
    }
}

use std::{ffi::c_void, io, ptr};
type CF = *const c_void;
#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFStringCreateWithCString(allocator: CF, text: *const i8, encoding: u32) -> CF;
    fn CFDictionaryCreateMutable(
        allocator: CF,
        capacity: isize,
        keys: CF,
        values: CF,
    ) -> *mut c_void;
    fn CFDictionarySetValue(dict: *mut c_void, key: CF, value: CF);
    fn CFDataCreate(allocator: CF, bytes: *const u8, length: isize) -> CF;
    fn CFDataGetLength(data: CF) -> isize;
    fn CFDataGetBytePtr(data: CF) -> *const u8;
    fn CFRelease(value: CF);
    static kCFTypeDictionaryKeyCallBacks: u8;
    static kCFTypeDictionaryValueCallBacks: u8;
    static kCFBooleanTrue: CF;
}
#[link(name = "Security", kind = "framework")]
unsafe extern "C" {
    fn SecRandomCopyBytes(random: CF, count: usize, bytes: *mut u8) -> i32;
    fn SecItemCopyMatching(query: CF, result: *mut CF) -> i32;
    fn SecItemAdd(query: CF, result: *mut CF) -> i32;
    fn SecItemDelete(query: CF) -> i32;
    static kSecClass: CF;
    static kSecClassGenericPassword: CF;
    static kSecAttrService: CF;
    static kSecAttrAccount: CF;
    static kSecReturnData: CF;
    static kSecValueData: CF;
}
struct Owned(CF);
impl Drop for Owned {
    fn drop(&mut self) {
        unsafe {
            if !self.0.is_null() {
                CFRelease(self.0)
            }
        }
    }
}
fn error(status: i32) -> io::Error {
    io::Error::other(format!("キーチェーンを利用できません（{status}）。"))
}
const SERVICE: &str = "local.knit.connection";
unsafe fn query(service: &str) -> io::Result<Owned> {
    let service_name =
        std::ffi::CString::new(service).map_err(|_| io::Error::other("不正なサービス名"))?;
    let dict = CFDictionaryCreateMutable(
        ptr::null(),
        0,
        ptr::addr_of!(kCFTypeDictionaryKeyCallBacks).cast(),
        ptr::addr_of!(kCFTypeDictionaryValueCallBacks).cast(),
    );
    if dict.is_null() {
        return Err(io::Error::other("キーチェーン照会を作成できません。"));
    }
    let query = Owned(dict);
    let service = Owned(CFStringCreateWithCString(
        ptr::null(),
        service_name.as_ptr(),
        0x08000100,
    ));
    let account = Owned(CFStringCreateWithCString(
        ptr::null(),
        c"pairing-token-v1".as_ptr(),
        0x08000100,
    ));
    CFDictionarySetValue(dict, kSecClass, kSecClassGenericPassword);
    CFDictionarySetValue(dict, kSecAttrService, service.0);
    CFDictionarySetValue(dict, kSecAttrAccount, account.0);
    Ok(query)
}
pub(super) fn random(bytes: &mut [u8]) -> io::Result<()> {
    let status = unsafe { SecRandomCopyBytes(ptr::null(), bytes.len(), bytes.as_mut_ptr()) };
    if status == 0 {
        Ok(())
    } else {
        Err(error(status))
    }
}
pub(super) fn load() -> io::Result<Option<String>> {
    load_at(SERVICE)
}
fn load_at(service: &str) -> io::Result<Option<String>> {
    unsafe {
        let query = query(service)?;
        CFDictionarySetValue(query.0 as _, kSecReturnData, kCFBooleanTrue);
        let mut data = ptr::null();
        let status = SecItemCopyMatching(query.0, &mut data);
        if status == -25300 {
            return Ok(None);
        }
        if status != 0 {
            return Err(error(status));
        }
        if data.is_null() {
            return Err(io::Error::other("キーチェーンの接続キーが空です。"));
        }
        let data = Owned(data);
        let len = CFDataGetLength(data.0);
        if len != 64 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "保存した接続キーが不正です。",
            ));
        }
        let bytes = std::slice::from_raw_parts(CFDataGetBytePtr(data.0), len as usize);
        String::from_utf8(bytes.to_vec())
            .map(Some)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "接続キーを読み取れません。"))
    }
}
pub(super) fn save(token: &str) -> io::Result<()> {
    save_at(SERVICE, token)
}
/// 登録の全初期化: キーチェーンから接続キーを削除する。
/// すでに無い(-25300 errSecItemNotFound)は成功として扱う(冪等)
pub(super) fn delete() -> io::Result<()> {
    unsafe {
        let query = query(SERVICE)?;
        let status = SecItemDelete(query.0);
        if status == 0 || status == -25300 {
            Ok(())
        } else {
            Err(error(status))
        }
    }
}
fn save_at(service: &str, token: &str) -> io::Result<()> {
    unsafe {
        let query = query(service)?;
        let data = Owned(CFDataCreate(
            ptr::null(),
            token.as_ptr(),
            token.len() as isize,
        ));
        if data.0.is_null() {
            return Err(io::Error::other("接続キーを準備できません。"));
        }
        CFDictionarySetValue(query.0 as _, kSecValueData, data.0);
        let status = SecItemAdd(query.0, ptr::null_mut());
        if status == 0 {
            Ok(())
        } else {
            Err(error(status))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Cleanup(String);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            unsafe {
                if let Ok(q) = query(&self.0) {
                    SecItemDelete(q.0);
                }
            }
        }
    }
    #[test]
    #[ignore = "専用の一時キーチェーン項目を作成・削除する実機試験"]
    fn keychain_roundtrip_without_touching_registered_key() {
        let service = format!(
            "local.knit.test.{}.{}",
            std::process::id(),
            super::super::generate().unwrap()
        );
        let cleanup = Cleanup(service);
        assert!(load_at(&cleanup.0).unwrap().is_none());
        let token = super::super::generate().unwrap();
        save_at(&cleanup.0, &token).unwrap();
        assert_eq!(
            load_at(&cleanup.0).unwrap().as_deref(),
            Some(token.as_str())
        );
        assert!(save_at(&cleanup.0, &token).is_err());
    }
}

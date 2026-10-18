//! 音声設定はユーザープロファイルへ保存し、配布先フォルダの書込み権限に依存させない。
//! ロック解除の連動(Mac のロック解除に合わせてこの PC も解除する)の設定もここで
//! 持つ。パスワードは平文を一切保存せず、DPAPI(ユーザースコープ)で保護した
//! base64 文字列のみを書き出す
use super::*;
use std::sync::Mutex;
fn path() -> std::io::Result<std::path::PathBuf> {
    let dir = knit_common::envutil::data_dir()
        .ok_or_else(|| std::io::Error::other("LOCALAPPDATA is unavailable"))?;
    Ok(dir.join("preferences.json"))
}
pub(super) fn restore() {
    let Ok(path) = path() else { return };
    let Ok(bytes) = std::fs::read(path) else {
        return;
    };
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        return;
    };
    if let Some(enabled) = value["audio_enabled"].as_bool() {
        crate::audio::AUDIO_ENABLED.store(enabled, Ordering::Relaxed);
    }
    if let Some(on) = value["share_clip"].as_bool() {
        knit_common::share::set_user_clip(on);
    }
    if let Some(on) = value["share_files"].as_bool() {
        knit_common::share::set_user_files(on);
    }
    if let Some(on) = value["host_mode"].as_bool() {
        HOST_MODE.store(on, Ordering::Relaxed);
    }
    let (unlock_on, unlock_secret) = unlock_from_json(&value);
    UNLOCK_SYNC.store(unlock_on, Ordering::Relaxed);
    *unlock_secret_slot() = unlock_secret;
}

/// JSON 値からロック解除連動の 2 設定を読む(型が違うキーは既定のまま。
/// 保存側が base64 で書く unlock_secret は文字列だけを受け入れる)
fn unlock_from_json(value: &serde_json::Value) -> (bool, String) {
    let on = value["unlock_sync"].as_bool().unwrap_or(false);
    let secret = value["unlock_secret"]
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= MAX_UNLOCK_SECRET_B64)
        .unwrap_or("")
        .to_string();
    (on, secret)
}

/// このPCをホスト(待受側)にする設定。KNIT_ROLE=server の GUI 版。
/// 接続方向は起動時に決まるため、再起動後に反映される
pub(crate) static HOST_MODE: AtomicBool = AtomicBool::new(false);

// ---------- ロック解除の連動 ----------
/// ロック解除の連動(既定 OFF)。Mac のロック解除に合わせてこの PC のロック
/// 画面へ入力を注入するか。注入先は設定画面で入れたパスワード(無ければ Enter のみ)
pub(crate) static UNLOCK_SYNC: AtomicBool = AtomicBool::new(false);
/// DPAPI で保護したパスワードの base64(空=パスワード無し運用)。平文は保持しない
static UNLOCK_SECRET: Mutex<String> = Mutex::new(String::new());
/// 保護済みパスワード(base64)の保存上限。平文 128 文字程度の保護結果が収まる余裕
const MAX_UNLOCK_SECRET_B64: usize = 4096;
/// 解除に使える平文パスワードの長さ上限(ロック画面の入力として十分)
pub(super) const MAX_UNLOCK_SECRET_CHARS: usize = 256;

fn unlock_secret_slot() -> std::sync::MutexGuard<'static, String> {
    UNLOCK_SECRET.lock().unwrap_or_else(|e| e.into_inner())
}

/// ロック解除の連動が有効か(Msg::Unlock の受信側と UI の表示で使う)
pub(crate) fn unlock_enabled() -> bool {
    UNLOCK_SYNC.load(Ordering::Relaxed)
}

/// パスワードが保存済みか(空=Enter のみの運用)。中身は返さない
pub(crate) fn unlock_secret_stored() -> bool {
    !unlock_secret_slot().is_empty()
}

/// DPAPI 保護済みの base64 をそのまま保持へ反映するのは使う場面が無くなった
/// (UI は protect_and_set_unlock_secret を使い、復号は unlock_secret_plain が担う)

/// 保存したパスワードを平文へ復号する。未設定は None(Enter のみ運用)。
/// 復号の失敗(別ユーザー・壊れた設定)も None: 呼び出し側は何もしない
pub(crate) fn unlock_secret_plain() -> Option<String> {
    let b64 = unlock_secret_slot().clone();
    if b64.is_empty() {
        return None;
    }
    let plain = dpapi_protect(&b64_decode(&b64)?, false).ok()?;
    let s = String::from_utf8(plain).ok()?;
    (!s.is_empty()).then_some(s)
}

// ---------- DPAPI(ユーザースコープの保護。common の credentials と同じ手宣言) ----------
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
        reserved: *const core::ffi::c_void,
        prompt: *const core::ffi::c_void,
        flags: u32,
        output: *mut Blob,
    ) -> i32;
    fn CryptUnprotectData(
        input: *const Blob,
        description: *mut *mut u16,
        entropy: *const Blob,
        reserved: *const core::ffi::c_void,
        prompt: *const core::ffi::c_void,
        flags: u32,
        output: *mut Blob,
    ) -> i32;
}
use windows_sys::Win32::Foundation::LocalFree;

/// DPAPI で保護(encrypt=true)/復号(false)する。スコープはユーザー既定
/// (entropy 無し・UI 禁止フラグ=非対話で失敗しない。credentials と同じ条件)
fn dpapi_protect(bytes: &[u8], encrypt: bool) -> std::io::Result<Vec<u8>> {
    unsafe {
        let input = Blob {
            len: bytes.len() as u32,
            data: bytes.as_ptr() as *mut u8,
        };
        let mut output = Blob {
            len: 0,
            data: std::ptr::null_mut(),
        };
        let ok = if encrypt {
            CryptProtectData(
                &input,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                1,
                &mut output,
            )
        } else {
            CryptUnprotectData(
                &input,
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                1,
                &mut output,
            )
        };
        if ok == 0 || output.data.is_null() || output.len == 0 {
            if !output.data.is_null() {
                LocalFree(output.data.cast());
            }
            return Err(std::io::Error::other(
                "ロック解除のパスワードを保護・読み出しできません。",
            ));
        }
        let data = std::slice::from_raw_parts(output.data, output.len as usize).to_vec();
        // OSが返すバッファは解放前に消去する(平文の復号結果をメモリに残さない)
        for i in 0..output.len as usize {
            std::ptr::write_volatile(output.data.add(i), 0);
        }
        LocalFree(output.data.cast());
        Ok(data)
    }
}

/// UI の保存ハンドラから呼ぶ: 平文パスワードを DPAPI 保護して保持へ入れる。
/// 空文字列はパスワードのクリア(保護をかけない)。保護に失敗したときは
/// Err(呼び出し側は設定を拒否してエラー表示する)
pub(super) fn protect_and_set_unlock_secret(plain: &str) -> std::io::Result<()> {
    if plain.chars().count() > MAX_UNLOCK_SECRET_CHARS {
        return Err(std::io::Error::other("パスワードが長すぎます。"));
    }
    let b64 = if plain.is_empty() {
        String::new()
    } else {
        b64_encode(&dpapi_protect(plain.as_bytes(), true)?)
    };
    *unlock_secret_slot() = b64;
    Ok(())
}

// ---------- base64(RFC 4648 標準アルファベット。依存を増やさないため自前) ----------
const B64_ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn b64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(B64_ALPHABET[(n >> 18) as usize & 0x3F] as char);
        out.push(B64_ALPHABET[(n >> 12) as usize & 0x3F] as char);
        out.push(if chunk.len() > 1 {
            B64_ALPHABET[(n >> 6) as usize & 0x3F] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            B64_ALPHABET[n as usize & 0x3F] as char
        } else {
            '='
        });
    }
    out
}

fn b64_decode(s: &str) -> Option<Vec<u8>> {
    // パディングは末尾のみ・最大2つ。**過剰な = を黙って捨てない**
    // ("AAAA====" のような不正入力を復号できてしまうと、設定欄の検証が甘くなる)
    let body = s.trim_end_matches('=');
    if s.len() - body.len() > 2 || body.contains('=') {
        return None;
    }
    let s = body;
    if s.len() % 4 == 1 {
        return None;
    }
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let mut acc = 0u32;
    let mut bits = 0u32;
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        } as u32;
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Some(out)
}

/// 起動時のロール判定用(preferences::restore より前でも読める直接読み)
pub(crate) fn host_mode_pref() -> bool {
    path().ok()
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
        .and_then(|v| v["host_mode"].as_bool())
        .unwrap_or_else(|| HOST_MODE.load(Ordering::Relaxed))
}

/// 役割を直接決める(相手の切替に合わせる時)
pub(super) fn set_host_mode(on: bool) {
    HOST_MODE.store(on, Ordering::Relaxed);
}

pub(super) fn save() -> std::io::Result<()> {
    use std::io::Write;
    let path = path()?;
    std::fs::create_dir_all(path.parent().unwrap())?;
    let tmp = path.with_extension(format!("{}.tmp", std::process::id()));
    let mut f = std::fs::File::create(&tmp)?;
    f.write_all(serde_json::to_string_pretty(&serde_json::json!({"version":1,"audio_enabled":crate::audio::AUDIO_ENABLED.load(Ordering::Relaxed),"share_clip":knit_common::share::user_clip(),"share_files":knit_common::share::user_files(),"host_mode":HOST_MODE.load(Ordering::Relaxed),"unlock_sync":UNLOCK_SYNC.load(Ordering::Relaxed),"unlock_secret":unlock_secret_slot().clone()}))?.as_bytes())?;
    f.sync_all()?;
    drop(f);
    #[link(name = "kernel32")]
    extern "system" {
        fn MoveFileExW(from: *const u16, to: *const u16, flags: u32) -> i32;
    }
    if unsafe {
        MoveFileExW(
            wide(&tmp.to_string_lossy()).as_ptr(),
            wide(&path.to_string_lossy()).as_ptr(),
            1 | 8,
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// base64 の往復: 全 8 ビット値を含む入力が壊れず戻る。パディングの
    /// 3 パターン(4n/4n+1/4n+2)をカバーする
    #[test]
    fn base64_round_trips_all_padding_patterns() {
        let all_bytes: Vec<u8> = (0..=u8::MAX).collect();
        for len in [0usize, 1, 2, 3, 4, 5, 255, 256] {
            let data = &all_bytes[..len.min(all_bytes.len())];
            let enc = b64_encode(data);
            assert_eq!(enc.len() % 4, 0, "長さは常に 4 の倍数(len={len})");
            assert_eq!(b64_decode(&enc).as_deref(), Some(data), "len={len}");
        }
        assert_eq!(b64_encode(b""), "");
        assert_eq!(b64_encode(b"f"), "Zg==");
        assert_eq!(b64_encode(b"fo"), "Zm8=");
        assert_eq!(b64_encode(b"foo"), "Zm9v");
    }

    /// base64 の拒否: パディング過多・不正文字・長さ 4n+1 は None(復号失敗扱い)
    #[test]
    fn base64_rejects_malformed_input() {
        assert!(b64_decode("A").is_none(), "4n+1 は不可能な長さ");
        assert!(b64_decode("A===").is_none(), "3 つのパディングは不正");
        assert!(b64_decode("AB!D").is_none(), "不正文字");
        assert!(b64_decode("A B=").is_none(), "空白も不正");
        // trim_end_matches('=') のため末尾の過剰な = は 4n+1 で弾かれる
        assert!(b64_decode("AAAA====").is_none());
    }

    /// unlock_from_json: 型が違うキー・過長な secret は既定(false/空)へ落ちる。
    /// 正しい組み合わせはそのまま読まれる
    #[test]
    fn unlock_settings_read_only_well_typed_values() {
        let ok = serde_json::json!({"unlock_sync": true, "unlock_secret": "QUJD"});
        assert_eq!(unlock_from_json(&ok), (true, "QUJD".to_string()));
        // 型が違う(数値・bool)secret と sync は既定値のまま
        let bad_types = serde_json::json!({"unlock_sync": "yes", "unlock_secret": 123});
        assert_eq!(unlock_from_json(&bad_types), (false, String::new()));
        // キー無し(旧版の設定ファイル)も既定値
        assert_eq!(unlock_from_json(&serde_json::json!({})), (false, String::new()));
        // 過長な secret は壊れた値として保持しない
        let huge = serde_json::json!({"unlock_sync": true, "unlock_secret": "A".repeat(MAX_UNLOCK_SECRET_B64 + 1)});
        assert_eq!(unlock_from_json(&huge), (true, String::new()));
        // 空文字列の secret はそのまま(パスワード無し運用)
        let empty = serde_json::json!({"unlock_sync": true, "unlock_secret": ""});
        assert_eq!(unlock_from_json(&empty), (true, String::new()));
    }
}

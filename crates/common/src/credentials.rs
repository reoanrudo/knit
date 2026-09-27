//! 初回登録用の256-bit鍵。既存のenv設定とは分離し、OSの資格情報保護を使う。
use std::io;
#[cfg(target_os = "macos")]
mod mac;
#[cfg(target_os = "windows")]
mod win;
#[cfg(target_os = "macos")]
use mac as platform;
#[cfg(target_os = "windows")]
use win as platform;

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
mod platform {
    use std::io;
    pub fn random(_: &mut [u8]) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "未対応のOSです。",
        ))
    }
    pub fn load() -> io::Result<Option<String>> {
        Ok(None)
    }
    pub fn save(_: &str) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "未対応のOSです。",
        ))
    }
}

/// 入力の前後空白だけを許す。短いPIN、行の混入、URLなどを鍵として受け入れない。
pub fn parse_key(text: &str) -> io::Result<String> {
    let text = text.trim();
    let text = text.strip_prefix("tsunagu1:").unwrap_or(text);
    if text.len() != 64 || !text.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Macで表示した接続キーをすべて貼り付けてください。",
        ));
    }
    Ok(text.to_ascii_lowercase())
}
pub fn display_key(token: &str) -> String {
    format!("tsunagu1:{token}")
}
pub fn generate() -> io::Result<String> {
    let mut bytes = [0u8; 32];
    platform::random(&mut bytes)?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}
pub fn load() -> io::Result<Option<String>> {
    platform::load()?.map(|v| parse_key(&v)).transpose()
}
pub fn save(token: &str) -> io::Result<()> {
    platform::save(&parse_key(token)?)
}

#[cfg(all(test, any(target_os = "macos", target_os = "windows")))]
mod tests {
    use super::*;
    #[test]
    fn enrollment_key_roundtrip_and_invalid_input() {
        let token = generate().unwrap();
        assert_eq!(token.len(), 64);
        assert_ne!(token, generate().unwrap());
        assert_eq!(
            parse_key(&format!("  {}\n", display_key(&token))).unwrap(),
            token
        );
        assert_eq!(parse_key(&token.to_ascii_uppercase()).unwrap(), token);
        for bad in [
            "123456".to_string(),
            "0".repeat(63),
            "g".repeat(64),
            format!("{}\nTSUNAGU_HOST=other", token),
            format!("tsunagu2:{token}"),
        ] {
            assert!(parse_key(&bad).is_err());
        }
    }
}

// Non-secret reconnect hint. Authentication always uses the protected token.
fn peer_path() -> io::Result<std::path::PathBuf> {
    #[cfg(target_os = "windows")]
    let root =
        std::env::var_os("LOCALAPPDATA").map(|p| std::path::PathBuf::from(p).join("Tsunagu"));
    #[cfg(not(target_os = "windows"))]
    let root = std::env::var_os("HOME")
        .map(|p| std::path::PathBuf::from(p).join("Library/Application Support/Tsunagu"));
    root.map(|p| p.join("paired-host.txt"))
        .ok_or_else(|| io::Error::other("user directory unavailable"))
}
pub fn save_peer(peer: std::net::SocketAddr) -> io::Result<()> {
    let path = peer_path()?;
    std::fs::create_dir_all(path.parent().unwrap())?;
    std::fs::write(path, peer.to_string())
}
pub fn load_peer() -> Option<std::net::SocketAddr> {
    use std::io::Read;
    let mut text = String::new();
    std::fs::File::open(peer_path().ok()?)
        .ok()?
        .take(128)
        .read_to_string(&mut text)
        .ok()?;
    text.trim().parse().ok()
}

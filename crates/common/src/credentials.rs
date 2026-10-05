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
    pub fn delete() -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "未対応のOSです。",
        ))
    }
}

/// 入力の前後空白だけを許す。短いPIN、行の混入、URLなどを鍵として受け入れない。
pub fn parse_key(text: &str) -> io::Result<String> {
    let text = text.trim();
    let text = text.strip_prefix("knit1:").unwrap_or(text);
    if text.len() != 64 || !text.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Macで表示した接続キーをすべて貼り付けてください。",
        ));
    }
    Ok(text.to_ascii_lowercase())
}
/// Validate an existing transport key without trimming or changing its case.
/// This is for authenticated enrollment into stores that support legacy keys;
/// manual imports and the desktop credential store remain canonical-only.
pub fn validate_transport_key(token: &str) -> io::Result<()> {
    if !(32..=512).contains(&token.len()) || token.chars().any(char::is_control) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "unsupported transport credential"));
    }
    Ok(())
}
pub fn display_key(token: &str) -> String {
    format!("knit1:{token}")
}
pub fn generate() -> io::Result<String> {
    let mut bytes = [0u8; 32];
    platform::random(&mut bytes)?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// 手動直接接続(KNIT_TOKEN)の共有トークンの文字数。読み込み側(main)は
/// 32 文字未満で起動を止めるため、生成・検証ともこの下限を基準にする
pub const SHARED_TOKEN_LEN: usize = 32;
const SHARED_ALPHABET: &[u8; 62] =
    b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

/// 乱数バイト列を共有トークン(32 文字の半角英数字)へ対応させる純関数。
/// 62 は 256 の約数でないため、biased にならない上限(248=62*4)で弾いて使う。
/// 弾かれたバイトは読み捨て(再抽選は呼び出し側の乱数の量で担保)
pub fn encode_shared_token(bytes: &[u8]) -> String {
    bytes
        .iter()
        .filter(|&&b| (b as usize) < SHARED_ALPHABET.len() * 4)
        .map(|&b| SHARED_ALPHABET[b as usize % SHARED_ALPHABET.len()] as char)
        .take(SHARED_TOKEN_LEN)
        .collect()
}

/// 共有トークン(KNIT_TOKEN)の生成。登録鍵(credentials::generate)と同じ
/// OS の安全な乱数(SecRandomCopyBytes / BCrypt)から 32 文字の英数字を作る。
/// 1 文字あたり約 5.95bit のため全体で約 190bit(128bit の下限を上回る)。
/// 受け取り側が余分に乱数を渡すのは、弾き抽出で 32 文字に届かないのを防ぐため
/// (512 バイトで不足する確率は約 2^-254。それでも足りなければエラーにする)
pub fn generate_shared_token() -> io::Result<String> {
    let mut bytes = [0u8; 512];
    platform::random(&mut bytes)?;
    let token = encode_shared_token(&bytes);
    if token.len() != SHARED_TOKEN_LEN {
        return Err(io::Error::other("共有トークンを生成できませんでした。"));
    }
    Ok(token)
}

/// 設定画面の「直接つなぐ」入力欄の検証。読み込み側は 32 文字未満の
/// KNIT_TOKEN で起動が止まる(fatal)ため、保存前にここで弾く。半角英数字
/// だけを受け入れるのは、env ファイル(KEY=VALUE 行)を壊す文字・相手側に
/// 貼れなくなる文字を最初から除くため(生成結果も同じ文字種)
pub fn validate_shared_token(token: &str) -> Result<(), &'static str> {
    if token.is_empty() {
        return Ok(()); // 空欄=指定を外して通常の登録へ戻す(保存ハンドラが扱う)
    }
    if token.len() < SHARED_TOKEN_LEN {
        return Err("トークンが短すぎます。32 文字以上の半角英数字を入れてください(「生成」で作れます)");
    }
    if token.len() > 512 || !token.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return Err("トークンは 512 文字までの半角英数字だけで入れてください(記号・空白・全角は使えません)");
    }
    Ok(())
}
pub fn load() -> io::Result<Option<String>> {
    platform::load()?.map(|v| parse_key(&v)).transpose()
}
pub fn save(token: &str) -> io::Result<()> {
    platform::save(&parse_key(token)?)
}
/// 保存した接続キーを OS の資格情報保護から削除する(登録の全初期化で使う)。
/// 削除して保存し直すまで、既存の相手は再接続できなくなる
pub fn delete() -> io::Result<()> {
    platform::delete()
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
            format!("{}\nKNIT_HOST=other", token),
            format!("knit2:{token}"),
        ] {
            assert!(parse_key(&bad).is_err());
        }
    }

    /// 共有トークン(直接つなぐ): 生成は常に 32 文字の半角英数字で、
    /// 呼び出すたびに異なる値が出る(乱数の由来は登録鍵と同じ OS の安全な乱数)
    #[test]
    fn shared_token_generation_shape_and_uniqueness() {
        let a = generate_shared_token().unwrap();
        let b = generate_shared_token().unwrap();
        assert_eq!(a.len(), SHARED_TOKEN_LEN);
        assert!(a.bytes().all(|c| c.is_ascii_alphanumeric()), "{a}");
        assert_ne!(a, b, "毎回異なる値が出る");
        // 既定の登録鍵(64 桁 hex)とも形式が違う: 小文字 hex だけの 64 桁に
        // ならない(大文字・数字混在の 32 桁)
        assert_ne!(a.len(), 64);
    }

    /// 乱数→文字列の対応(純関数): 文字数・文字種・弾き抽出の確認。
    /// 248 以上のバイトは使われず、足りない分は渡した順に補充される
    #[test]
    fn shared_token_encoding_is_unbiased_and_pure() {
        // 均一バイト(全部 0)でも 32 文字揃う(0 は弾かれない)
        assert_eq!(encode_shared_token(&[0u8; 40]), "0".repeat(32));
        // バイト値 0..62 はそのまま文字種の先頭へ対応する(32 文字で打ち切り)
        let one_cycle: &str = &encode_shared_token(&(0u8..62).collect::<Vec<_>>());
        assert_eq!(
            one_cycle,
            &"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz"[..32]
        );
        // 248..=255 のバイトは弾かれる(62*4=248 を境に biased になるため)
        assert_eq!(encode_shared_token(&[248u8; 64]), "");
        assert_eq!(encode_shared_token(&[255u8; 64]), "");
        // 途中に弾かれる値が混ざっていても、残りで 32 文字を揃える
        let mixed = encode_shared_token(&[0, 248, 1, 255, 2, 250, 3]);
        assert_eq!(mixed, "0123");
        // 文字数より乱数が少ないときはその分だけ短くなる(呼び出し側で担保)
        assert_eq!(encode_shared_token(&[7; 3]).len(), 3);
    }

    /// 設定画面の入力検証: 空欄は許可(=解除)、32 文字未満・記号・空白・
    /// 512 文字超は弾く。読み込み側(main の起動判定)と下限を一致させる
    #[test]
    fn shared_token_validation_matches_loader() {
        assert!(validate_shared_token("").is_ok(), "空欄=通常の登録へ戻す");
        assert!(validate_shared_token("a").is_err(), "1 文字は起動が止まるため弾く");
        assert!(
            validate_shared_token(&"a".repeat(31)).is_err(),
            "31 文字は起動が止まるため弾く"
        );
        assert!(validate_shared_token(&"a".repeat(32)).is_ok(), "32 文字は下限");
        assert!(validate_shared_token(&generate_shared_token().unwrap()).is_ok());
        assert!(validate_shared_token(&"a".repeat(513)).is_err(), "512 文字上限");
        for bad in [
            format!("{} ", "a".repeat(32)),      // 末尾の空白
            format!("{}\n", "a".repeat(32)),     // 改行の混入
            format!("{}=x", "a".repeat(32)),     // env ファイルを壊す記号
            "あ".repeat(32),                     // 全角
        ] {
            assert!(validate_shared_token(&bad).is_err(), "弾く: {bad:?}");
        }
    }
}

// Non-secret reconnect hint. Authentication always uses the protected token.
fn peer_path() -> io::Result<std::path::PathBuf> {
    crate::envutil::data_dir()
        .map(|p| p.join("paired-host.txt"))
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

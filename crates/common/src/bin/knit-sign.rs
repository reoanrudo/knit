//! 更新情報の鍵生成・生成・署名(リリース担当が使う)。
//!   knit-sign keygen <秘密鍵の保存先>
//!   knit-sign manifest <version> <channel> <platform>=<zip>=<url> ...   (JSON を標準出力へ)
//!   knit-sign sign <秘密鍵ファイル> <署名対象ファイル>                (署名の16進を標準出力へ)
use ed25519_dalek::{Signer, SigningKey};
use knit_common::{proto, update};
use sha2::{Digest, Sha256};
use std::io::Read;

fn die(msg: &str) -> ! {
    eprintln!("knit-sign: {msg}");
    std::process::exit(2);
}

fn create_private(path: &str) -> std::io::Result<std::fs::File> {
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut o, 0o600);
    o.open(path)
}

fn load_key(path: &str) -> SigningKey {
    let text = std::fs::read_to_string(path).unwrap_or_else(|_| die("秘密鍵を読めません"));
    let bytes: [u8; 32] = update::from_hex(text.trim())
        .and_then(|b| b.try_into().ok())
        .unwrap_or_else(|| die("秘密鍵の形式が不正です(64桁の16進)"));
    SigningKey::from_bytes(&bytes)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("keygen") => {
            let path = args.get(1).unwrap_or_else(|| die("保存先を指定してください"));
            let mut seed = [0u8; 32];
            std::fs::File::open("/dev/urandom")
                .and_then(|mut f| f.read_exact(&mut seed))
                .unwrap_or_else(|_| die("乱数を取得できません"));
            let sk = SigningKey::from_bytes(&seed);
            // 秘密鍵は所有者のみ読める権限で新規作成する(既存ファイルは上書きしない)
            let mut f = create_private(path).unwrap_or_else(|_| die("保存先を作成できません(既に存在する場合は上書きしません)"));
            std::io::Write::write_all(&mut f, update::to_hex(&seed).as_bytes())
                .unwrap_or_else(|_| die("書き込みに失敗しました"));
            println!("{}", update::to_hex(&sk.verifying_key().to_bytes()));
            eprintln!("公開鍵を crates/common/update-keys.txt へ追加してください。秘密鍵は安全な場所に保管し、リポジトリへ入れないでください。");
        }
        Some("manifest") => {
            let (Some(version), Some(channel)) = (args.get(1), args.get(2)) else {
                die("manifest <version> <channel> <platform>=<zip>=<url>...")
            };
            if update::parse_version(version).is_none() {
                die("version は MAJOR.MINOR.PATCH");
            }
            if channel != "stable" && channel != "beta" {
                die("channel は stable か beta");
            }
            let mut artifacts = Vec::new();
            for spec in &args[3..] {
                let parts: Vec<&str> = spec.splitn(3, '=').collect();
                let [platform, file, url] = parts[..] else { die("成果物は platform=zip=url") };
                if !update::secure_url(url) {
                    die("成果物の URL は https のみ");
                }
                let data = std::fs::read(file).unwrap_or_else(|_| die("成果物を読めません"));
                artifacts.push(serde_json::json!({
                    "platform": platform,
                    "url": url,
                    "size": data.len(),
                    "sha256": update::to_hex(&Sha256::digest(&data)),
                }));
            }
            if artifacts.is_empty() {
                die("成果物が1件も指定されていません");
            }
            let m = serde_json::json!({
                "schema_version": 1,
                "channel": channel,
                "version": version,
                "protocol": proto::VERSION,
                "artifacts": artifacts,
            });
            println!("{}", serde_json::to_string_pretty(&m).unwrap());
        }
        Some("sign") => {
            let (Some(key), Some(file)) = (args.get(1), args.get(2)) else {
                die("sign <秘密鍵> <ファイル>")
            };
            let data = std::fs::read(file).unwrap_or_else(|_| die("署名対象を読めません"));
            let sig = load_key(key).sign(&data);
            println!("{}", update::to_hex(&sig.to_bytes()));
        }
        _ => die("keygen | manifest | sign"),
    }
}

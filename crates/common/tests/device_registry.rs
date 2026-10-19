//! 端末別資格の受け入れ基準 AC2(失効の実演)。
//! 「登録→失効→拒否・他端末は無影響・legacy は無影響・冪等」を、台帳と
//! 鍵束と本物の Noise ハンドシェイク(ループバック TCP)で通しで証明する。
//! 実機2台を前提にしない(house rule: common のテストは cargo test で完結)。
use knit_common::registry::{DeviceRegistry, RevokeOutcome};
use knit_common::secure::{self, Who};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::Arc;

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("knit-ac2-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// 1回の accept_as×connect。 accept 側は毎回 registry.accept_keys() を引き直す
/// (本線の handshake_and_hello と同じ呼び出し方=失効が次の接続に即座に効く)
fn try_connect(
    listener: &TcpListener,
    registry: &Arc<DeviceRegistry>,
    token: &str,
) -> Result<Who, String> {
    let addr = listener.local_addr().unwrap();
    let inner = listener.try_clone().unwrap();
    let registry = registry.clone();
    let server = std::thread::spawn(move || {
        let (stream, _) = inner.accept().unwrap();
        let keys = registry.accept_keys().map_err(|e| e.to_string())?;
        let (mut r, mut w, who) =
            secure::accept_as(stream, &keys, b"knit-main").map_err(|e| e.to_string())?;
        // 接続側の書いた4バイトをそのまま返す(輸送層が生きていることの確認)
        let mut buf = [0u8; 4];
        if r.read_exact(&mut buf).is_ok() {
            let _ = w.write_all(&buf);
            let _ = w.flush();
        }
        Ok::<Who, String>(who)
    });
    let client = std::net::TcpStream::connect(addr).unwrap();
    match secure::connect(client.try_clone().unwrap(), token, b"knit-main") {
        Ok((mut r, mut w)) => {
            // 輸送層まで生きていることを確認(ハンドシェイクだけ通って
            // いない接続を「成功」と数えない)
            w.write_all(b"ping").unwrap();
            w.flush().unwrap();
            let mut buf = [0u8; 4];
            r.read_exact(&mut buf).map_err(|e| e.to_string())?;
            client.shutdown(std::net::Shutdown::Both).ok();
        }
        Err(_) => {
            client.shutdown(std::net::Shutdown::Both).ok();
        }
    }
    server.join().unwrap()
}

/// AC2 の主軸: 登録→失効→拒否・他端末無影響・legacy 無影響・冪等
#[test]
fn enroll_revoke_reject_and_others_unaffected() {
    let dir = temp_dir("main");
    let master = knit_common::credentials::generate().unwrap();
    let registry = Arc::new(DeviceRegistry::open(&dir, &master).unwrap());
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();

    let a = registry.enroll("A").unwrap();
    let b = registry.enroll("B").unwrap();

    // 登録直後: A も B も master(legacy)も通る。Who はそれぞれ正しい端末
    assert_eq!(try_connect(&listener, &registry, &a.token).unwrap(), Who::Device(a.key_id.clone()));
    assert_eq!(try_connect(&listener, &registry, &b.token).unwrap(), Who::Device(b.key_id.clone()));
    assert_eq!(try_connect(&listener, &registry, &master).unwrap(), Who::Legacy);

    // 失効(A のみ)
    assert_eq!(registry.revoke(&a.key_id).unwrap(), RevokeOutcome::Revoked);

    // A は拒否。B と legacy は無影響
    assert!(try_connect(&listener, &registry, &a.token).is_err(), "失効した端末は拒否");
    assert!(try_connect(&listener, &registry, &b.token).is_ok(), "他端末は無影響");
    assert!(try_connect(&listener, &registry, &master).is_ok(), "legacy は無影響");

    // 冪等: もう1回失効しても台帳は変わらず、B は引き続き通る
    let before = std::fs::read(dir.join("devices.json")).unwrap();
    assert_eq!(registry.revoke(&a.key_id).unwrap(), RevokeOutcome::NotFound);
    let after = std::fs::read(dir.join("devices.json")).unwrap();
    assert_eq!(before, after);
    assert!(try_connect(&listener, &registry, &b.token).is_ok());

    // 台帳に秘密を書かない(AC の不変条件を通しでも固定)
    let text = std::fs::read_to_string(dir.join("devices.json")).unwrap();
    assert!(!text.contains(&master) && !text.contains(&a.token) && !text.contains(&b.token));

    std::fs::remove_dir_all(&dir).unwrap();
}

/// legacy の閉鎖(revoke_legacy)は旧形式だけを拒否し、端末別は生き続ける
/// (KNIT_TOKEN 運用での M2 相当の効き方)
#[test]
fn revoke_legacy_closes_only_the_legacy_door() {
    let dir = temp_dir("legacy-close");
    let master = knit_common::credentials::generate().unwrap();
    let registry = Arc::new(DeviceRegistry::open(&dir, &master).unwrap());
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let a = registry.enroll("A").unwrap();
    assert!(try_connect(&listener, &registry, &master).is_ok());
    registry.revoke_legacy().unwrap();
    assert!(try_connect(&listener, &registry, &master).is_err(), "旧形式は拒否");
    assert!(try_connect(&listener, &registry, &a.token).is_ok(), "端末別は無影響");
    // 再読込しても閉じたまま
    let reopened = Arc::new(DeviceRegistry::open(&dir, &master).unwrap());
    assert!(try_connect(&listener, &reopened, &master).is_err());
    assert!(try_connect(&listener, &reopened, &a.token).is_ok());
    std::fs::remove_dir_all(&dir).unwrap();
}

/// 失効は3経路(本線・音声・bulk のラベル)すべてで同時に効く
/// (鍵束から消えることが失効=経路ごとの失効チェックは存在しない)
#[test]
fn revocation_blocks_every_path_label() {
    let dir = temp_dir("labels");
    let master = knit_common::credentials::generate().unwrap();
    let registry = Arc::new(DeviceRegistry::open(&dir, &master).unwrap());
    for label in [b"knit-main".as_slice(), b"knit-audio".as_slice(), b"knit-bulk".as_slice()] {
        let a = registry.enroll("A").unwrap();
        assert!(path_probe(&registry, label, &a.token).is_ok(), "失効前は通る");
        registry.revoke(&a.key_id).unwrap();
        assert!(
            path_probe(&registry, label, &a.token).is_err(),
            "失効後は {label:?} 経路も拒否"
        );
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

/// 1接続ぶんの accept_as(毎回 registry.accept_keys() を引き直す)+ echo。
/// accept 側がエラーで終われば接続側も読み出し EOF で失敗する(ハングしない)
fn path_probe(registry: &Arc<DeviceRegistry>, label: &'static [u8], token: &str) -> Result<(), String> {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let inner = listener.try_clone().unwrap();
    let registry = registry.clone();
    let server = std::thread::spawn(move || {
        let Ok((stream, _)) = inner.accept() else { return };
        if let Ok(keys) = registry.accept_keys() {
            if let Ok((mut r, mut w, _who)) = secure::accept_as(stream, &keys, label) {
                // 接続側の書いた1バイトをそのまま返す(生存確認)
                let mut buf = [0u8; 1];
                if r.read_exact(&mut buf).is_ok() {
                    let _ = w.write_all(&buf);
                    let _ = w.flush();
                }
            }
        }
    });
    let result = connect_with(addr, token, label);
    server.join().unwrap();
    result
}

fn connect_with(addr: std::net::SocketAddr, token: &str, label: &[u8]) -> Result<(), String> {
    let stream = std::net::TcpStream::connect(addr).unwrap();
    let (mut r, mut w) = secure::connect(stream, token, label).map_err(|e| e.to_string())?;
    w.write_all(b"x").map_err(|e| e.to_string())?;
    w.flush().map_err(|e| e.to_string())?;
    let mut buf = [0u8; 1];
    r.read_exact(&mut buf).map_err(|e| e.to_string())
}

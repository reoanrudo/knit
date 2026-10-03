//! Android JNI shim calls this small C ABI. Authentication and framing remain
//! exactly the same as desktop Knit; Java never opens a plaintext TCP stream.
use knit_common::{bulk, pairing, secure};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    ffi::{c_char, c_void, CStr, CString},
    io::{self, BufRead, BufReader, Read, Write},
    net::{Shutdown, SocketAddr, TcpStream},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex, OnceLock,
    },
    time::Duration,
};

struct Session {
    reader: Mutex<BufReader<secure::Reader>>,
    writer: Mutex<secure::Writer>,
    control: TcpStream,
    bulk: Mutex<Option<bulk::Receiver>>,
}
static SESSIONS: OnceLock<Mutex<HashMap<u64, Arc<Session>>>> = OnceLock::new();
static NEXT: AtomicU64 = AtomicU64::new(1);
const MAX_LINE: u64 = 2 * 1024 * 1024;
type Save = unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char) -> bool;
fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, "invalid bridge argument")
}
fn text<'a>(v: &'a Value, key: &str) -> io::Result<&'a str> {
    v[key].as_str().ok_or_else(invalid)
}
fn sessions() -> &'static Mutex<HashMap<u64, Arc<Session>>> {
    SESSIONS.get_or_init(Default::default)
}
fn session(id: u64) -> io::Result<Arc<Session>> {
    sessions()
        .lock()
        .unwrap()
        .get(&id)
        .cloned()
        .ok_or_else(|| io::Error::from(io::ErrorKind::NotConnected))
}
fn open(v: &Value) -> io::Result<u64> {
    let token = text(v, "token")?;
    knit_common::credentials::validate_transport_key(token)?;
    let address: SocketAddr = text(v, "address")?.parse().map_err(|_| invalid())?;
    let label = if v["bulk"].as_bool() == Some(true) {
        b"knit-bulk".as_slice()
    } else {
        b"knit-main".as_slice()
    };
    let s = TcpStream::connect_timeout(&address, Duration::from_secs(3))?;
    s.set_nodelay(true)?;
    s.set_read_timeout(Some(Duration::from_secs(5)))?;
    s.set_write_timeout(Some(Duration::from_secs(5)))?;
    let control = s.try_clone()?;
    let (reader, writer) = secure::connect(s, token, label)?;
    control.set_read_timeout(Some(Duration::from_secs(if label == b"knit-main" {
        20
    } else {
        35
    })))?;
    let receiver = if label == b"knit-bulk" {
        let dir = PathBuf::from(text(v, "dir")?);
        std::fs::create_dir_all(&dir)?;
        Some(bulk::Receiver::new(&dir))
    } else {
        None
    };
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    sessions().lock().unwrap().insert(
        id,
        Arc::new(Session {
            reader: Mutex::new(BufReader::new(reader)),
            writer: Mutex::new(writer),
            control,
            bulk: Mutex::new(receiver),
        }),
    );
    Ok(id)
}
fn read_line(s: &Session) -> io::Result<Value> {
    let mut line = String::new();
    let mut reader = s.reader.lock().unwrap();
    let n = (&mut *reader).take(MAX_LINE + 1).read_line(&mut line)?;
    if n == 0 {
        return Err(io::Error::from(io::ErrorKind::UnexpectedEof));
    }
    if n as u64 > MAX_LINE || !line.ends_with('\n') {
        return Err(invalid());
    }
    serde_json::from_str(&line).map_err(|_| invalid())
}
fn read_bulk(s: &Session, v: &Value) -> io::Result<Value> {
    let mut reader = s.reader.lock().unwrap();
    let mut state = s.bulk.lock().unwrap();
    let receiver = state.as_mut().ok_or_else(invalid)?;
    let mut body = Vec::new();
    loop {
        let kind = bulk::read_frame(&mut *reader, &mut body)?;
        if let Some(event) = receiver.feed(kind, &body) {
            return match event {
                // Android ブリッジは共有範囲の管理外(転送試験用途)。拒否は件数だけで返す
                bulk::Event::Denied { files } => Ok(json!({"denied": files})),
                bulk::Event::Files { paths, .. } => Ok(json!({"files": paths})),
                // バッチ途中の切断・中止。保存済み件数だけ返す(ブリッジは通知しない)
                bulk::Event::Interrupted { saved } => Ok(json!({"interrupted": saved})),
                bulk::Event::Image(data) => {
                    let dir = PathBuf::from(text(v, "dir")?);
                    // 受信の流れと同じ形で: 一時ファイルへ書いてから最終名へ公開
                    std::fs::create_dir_all(&dir).map_err(|e| e)?;
                    let (mut f, tmp) = knit_common::files::create_temp(&dir)
                        .ok_or_else(|| io::Error::from(io::ErrorKind::AlreadyExists))?;
                    f.write_all(&data)?;
                    drop(f);
                    let path = knit_common::files::publish(&tmp, &dir.join("clipboard.dib"))?;
                    Ok(json!({"image":path}))
                }
            };
        }
    }
}
fn call(op: i32, id: u64, v: Value, ctx: *mut c_void, save: Option<Save>) -> io::Result<Value> {
    match op {
        0 => Ok(serde_json::to_value(pairing::discover()?).map_err(|_| invalid())?),
        1 => {
            let addr = pairing::manual_address(text(&v, "address")?)?;
            let save = save.ok_or_else(invalid)?;
            pairing::enroll_existing_key(addr, text(&v, "code")?, |token, peer| {
                let token = CString::new(token).map_err(|_| invalid())?;
                let peer = CString::new(peer.to_string()).unwrap();
                if unsafe { save(ctx, token.as_ptr(), peer.as_ptr()) } {
                    Ok(())
                } else {
                    Err(io::Error::other("credential storage failed"))
                }
            })?;
            Ok(json!({"ok":true}))
        }
        2 => Ok(json!({"handle":open(&v)?})),
        3 => read_line(session(id)?.as_ref()),
        4 => {
            let s = session(id)?;
            let data = serde_json::to_vec(&v).map_err(|_| invalid())?;
            if data.len() as u64 > MAX_LINE {
                return Err(invalid());
            }
            let mut w = s.writer.lock().unwrap();
            w.write_all(&data)?;
            w.write_all(b"\n")?;
            w.flush()?;
            Ok(json!({"ok":true}))
        }
        5 => {
            if let Some(s) = sessions().lock().unwrap().remove(&id) {
                let _ = s.control.shutdown(Shutdown::Both);
            }
            Ok(json!({"ok":true}))
        }
        6 => read_bulk(session(id)?.as_ref(), &v),
        7 => {
            let s = session(id)?;
            let paths: Vec<PathBuf> =
                serde_json::from_value(v["paths"].clone()).map_err(|_| invalid())?;
            let mut w = s.writer.lock().unwrap();
            let report = bulk::send_files(&mut *w, &paths, false)?;
            Ok(json!({"count": report.sent, "skipped": report.skipped.len()}))
        }
        8 => {
            let s = session(id)?;
            let mut w = s.writer.lock().unwrap();
            bulk::write_frame(&mut *w, bulk::KEEPALIVE, &[])?;
            w.flush()?;
            Ok(json!({"ok":true}))
        }
        // 9: 更新情報の署名検証と版の判定。10: 取得した更新ファイルのサイズ・ハッシュ検査
        9 => check_update(&v),
        10 => verify_update_file(&v),
        // 11: 承認方式の登録を依頼し、確認番号を返す。12: 持ち主の「つなぐ/やめる」を反映する
        11 => approval_begin(&v),
        12 => approval_finish(id, &v, ctx, save),
        _ => Err(invalid()),
    }
}
static APPROVALS: OnceLock<Mutex<HashMap<u64, pairing::Pending>>> = OnceLock::new();
static NEXT_APPROVAL: AtomicU64 = AtomicU64::new(1);
fn approvals() -> &'static Mutex<HashMap<u64, pairing::Pending>> {
    APPROVALS.get_or_init(Default::default)
}
/// 相手(Mac)に「つなぎたい」と伝え、確認番号を受け取る。持ち主が確認するまで何も保存しない
fn approval_begin(v: &Value) -> io::Result<Value> {
    let addr = pairing::manual_address(text(v, "address")?)?;
    let name = v["name"].as_str().filter(|n| !n.trim().is_empty()).unwrap_or("Android");
    let pending = pairing::request_approval(addr, name, Arc::new(AtomicBool::new(false)))?;
    let (sas, server) = (pending.sas.clone(), pending.server_name.clone());
    let choices = pairing::number_choices(&sas)?;
    let handle = NEXT_APPROVAL.fetch_add(1, Ordering::Relaxed);
    let mut map = approvals().lock().unwrap();
    map.clear(); // 待つのは常に1件だけ。古い依頼は取り消す
    map.insert(handle, pending);
    Ok(json!({"handle": handle, "sas": sas, "server": server, "choices": choices, "peer": addr.ip().to_string()}))
}
fn approval_finish(id: u64, v: &Value, ctx: *mut c_void, save: Option<Save>) -> io::Result<Value> {
    let pending = approvals().lock().unwrap().remove(&id).ok_or_else(invalid)?;
    if v["approve"] != true {
        drop(pending); // やめた: 依頼を取り消す
        return Ok(json!({"ok": true}));
    }
    let save = save.ok_or_else(invalid)?;
    let result = pending.confirm(false, |token, peer| {
        let token = CString::new(token).map_err(|_| invalid())?;
        let peer = CString::new(peer.to_string()).unwrap();
        if unsafe { save(ctx, token.as_ptr(), peer.as_ptr()) } {
            Ok(())
        } else {
            Err(io::Error::other("credential storage failed"))
        }
    });
    match result {
        Ok(_) => Ok(json!({"ok": true})),
        Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {
            // ホストは現状 Mac に限るが、文言を host 名に依存させない(第3ピア対応)
            Ok(json!({"error": "相手のMacで許可されませんでした。もう一度やり直してください。"}))
        }
        Err(e) => Err(e),
    }
}
/// 更新の失敗理由は利用者に見せてよい定型文なので、他の操作と違い本文をそのまま返す
fn update_error(message: impl Into<String>) -> Value {
    json!({"error": message.into()})
}
fn check_update(v: &Value) -> io::Result<Value> {
    check_update_with(v, &knit_common::update::trusted_keys())
}
fn check_update_with(v: &Value, keys: &[[u8; 32]]) -> io::Result<Value> {
    let cfg = knit_common::update::CheckConfig {
        manifest_url: "",
        keys,
        channel: "stable",
        current: text(v, "current")?,
        platform: "android-arm64",
    };
    if keys.is_empty() {
        return Ok(update_error(knit_common::update::UpdateError::NoTrustedKey.message()));
    }
    match knit_common::update::evaluate(&cfg, text(v, "manifest")?.as_bytes(), text(v, "signature")?) {
        Ok(Some(a)) => Ok(json!({"version": a.version, "url": a.artifact.url, "size": a.artifact.size, "sha256": a.artifact.sha256})),
        Ok(None) => Ok(json!({"latest": true})),
        Err(message) => Ok(update_error(message)),
    }
}
fn verify_update_file(v: &Value) -> io::Result<Value> {
    let artifact = knit_common::update::Artifact {
        platform: "android-arm64".into(),
        url: String::new(),
        size: v["size"].as_u64().ok_or_else(invalid)?,
        sha256: text(v, "sha256")?.to_string(),
    };
    let file = std::fs::File::open(text(v, "path")?)?;
    match knit_common::update::verify_artifact(file, &artifact) {
        Ok(()) => Ok(json!({"ok": true})),
        Err(e) => Ok(update_error(e.message())),
    }
}
// No exception may cross the C boundary. Error responses contain no peer token,
// short code, clipboard text, path, or Java exception details.
#[no_mangle]
pub unsafe extern "C" fn knit_android_call(
    op: i32,
    id: u64,
    arg: *const c_char,
    ctx: *mut c_void,
    save: Option<Save>,
) -> *mut c_char {
    let result = std::panic::catch_unwind(|| {
        if arg.is_null() {
            return Err(invalid());
        }
        let bytes = CStr::from_ptr(arg).to_bytes();
        if bytes.len() as u64 > MAX_LINE {
            return Err(invalid());
        }
        call(
            op,
            id,
            serde_json::from_slice(bytes).map_err(|_| invalid())?,
            ctx,
            save,
        )
    });
    let value = match result {
        Ok(Ok(v)) => v,
        Ok(Err(e)) => json!({"error": match e.kind() {
            io::ErrorKind::PermissionDenied => "認証できません。Macのコードで再登録してください。",
            io::ErrorKind::InvalidInput | io::ErrorKind::InvalidData => "接続先・コード・通信形式を確認してください。",
            io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock => "接続が応答しません。同じネットワークか確認してください。",
            io::ErrorKind::UnexpectedEof | io::ErrorKind::NotConnected => "接続が閉じられました。",
            _ => "処理できませんでした。接続と保存先を確認してください。"
        }}),
        Err(_) => json!({"error":"通信処理を停止しました。再接続してください。"}),
    };
    CString::new(value.to_string()).unwrap().into_raw()
}
#[no_mangle]
pub unsafe extern "C" fn knit_android_free(s: *mut c_char) {
    if !s.is_null() {
        drop(CString::from_raw(s));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn encrypted_utf8_roundtrip_and_close_unblocks_reader() {
        encrypted_roundtrip_with("a".repeat(64));
    }
    #[test]
    fn legacy_key_is_preserved_for_noise_authentication() {
        encrypted_roundtrip_with("Legacy-Key-With-Mixed-CASE-And-Symbols+/==".into());
    }
    fn encrypted_roundtrip_with(token: String) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server_token = token.clone();
        let server = std::thread::spawn(move || {
            let (s, _) = listener.accept().unwrap();
            let (r, mut w) = secure::accept(s, &server_token, b"knit-main").unwrap();
            let mut r = BufReader::new(r);
            let mut line = String::new();
            r.read_line(&mut line).unwrap();
            assert_eq!(
                serde_json::from_str::<Value>(&line).unwrap()["text"],
                "日本語🍵"
            );
            w.write_all(b"{\"t\":\"ping\",\"ts\":42}\n").unwrap();
            w.flush().unwrap();
            line.clear();
            assert!(r.read_line(&mut line).is_err());
        });
        let id = open(&json!({"address":addr.to_string(),"token":token})).unwrap();
        call(
            4,
            id,
            json!({"t":"text","text":"日本語🍵"}),
            std::ptr::null_mut(),
            None,
        )
        .unwrap();
        assert_eq!(
            call(3, id, json!({}), std::ptr::null_mut(), None).unwrap()["ts"],
            42
        );
        let waiting =
            std::thread::spawn(move || call(3, id, json!({}), std::ptr::null_mut(), None));
        call(5, id, json!({}), std::ptr::null_mut(), None).unwrap();
        assert!(waiting.join().unwrap().is_err());
        server.join().unwrap();
        assert!(session(id).is_err());
    }
    #[test]
    fn bulk_files_roundtrip_over_the_separate_encrypted_channel() {
        let root = std::env::temp_dir().join(format!("knit-bridge-bulk-{}", std::process::id()));
        let source = root.join("source");
        let received = root.join("received");
        let echoed = root.join("echoed");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::create_dir_all(&echoed).unwrap();
        let path = source.join("日本語.txt");
        std::fs::write(&path, "共有データ🍵").unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (s, _) = listener.accept().unwrap();
            let (mut r, mut w) = secure::accept(s, &"d".repeat(64), b"knit-bulk").unwrap();
            bulk::send_files(&mut w, &[path], false).unwrap();
            let mut receiver = bulk::Receiver::new(&echoed);
            let mut frame = Vec::new();
            loop {
                let kind = bulk::read_frame(&mut r, &mut frame).unwrap();
                if let Some(bulk::Event::Files { paths, .. }) = receiver.feed(kind, &frame) {
                    assert_eq!(std::fs::read_to_string(&paths[0]).unwrap(), "共有データ🍵");
                    break;
                }
            }
        });
        let id=open(&json!({"address":address.to_string(),"token":"d".repeat(64),"bulk":true,"dir":received})).unwrap();
        let event = call(6, id, json!({"dir":received}), std::ptr::null_mut(), None).unwrap();
        let paths = event["files"].clone();
        assert_eq!(
            std::fs::read_to_string(paths[0].as_str().unwrap()).unwrap(),
            "共有データ🍵"
        );
        call(7, id, json!({"paths":paths}), std::ptr::null_mut(), None).unwrap();
        server.join().unwrap();
        call(5, id, json!({}), std::ptr::null_mut(), None).unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn oversized_line_is_rejected_before_allocating_more() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (s, _) = listener.accept().unwrap();
            let (_, mut w) = secure::accept(s, &"b".repeat(64), b"knit-main").unwrap();
            let _ = w
                .write_all(&vec![b'x'; MAX_LINE as usize + 1])
                .and_then(|_| w.flush());
        });
        let id = open(&json!({"address":address.to_string(),"token":"b".repeat(64)})).unwrap();
        assert!(read_line(&session(id).unwrap()).is_err());
        call(5, id, json!({}), std::ptr::null_mut(), None).unwrap();
        server.join().unwrap();
    }
    #[test]
    fn update_ops_verify_signature_version_and_file() {
        use ed25519_dalek::{Signer, SigningKey};
        use sha2::{Digest, Sha256};
        let hex = |b: &[u8]| b.iter().map(|x| format!("{x:02x}")).collect::<String>();
        let sk = SigningKey::from_bytes(&[5u8; 32]);
        let keys = [sk.verifying_key().to_bytes()];
        let manifest = r#"{"schema_version":1,"channel":"stable","version":"0.2.0","protocol":13,"artifacts":[{"platform":"android-arm64","url":"https://e.com/Knit.apk","size":3,"sha256":"00"}]}"#;
        let sig = hex(&sk.sign(manifest.as_bytes()).to_bytes());
        let args = |current: &str, m: &str, sig: &str| json!({"current":current,"manifest":m,"signature":sig});
        let got = check_update_with(&args("0.1.0", manifest, &sig), &keys).unwrap();
        assert_eq!(got["version"], "0.2.0");
        assert_eq!(got["url"], "https://e.com/Knit.apk");
        assert_eq!(check_update_with(&args("0.2.0", manifest, &sig), &keys).unwrap()["latest"], true);
        assert!(check_update_with(&args("0.1.0", &manifest.replace("0.2.0", "9.0.0"), &sig), &keys).unwrap()["error"].is_string());
        assert!(check_update_with(&args("0.1.0", manifest, &sig), &[]).unwrap()["error"].is_string());
        // 鍵が未登録の組み込み状態では、どの更新情報も拒否される
        assert!(check_update(&args("0.1.0", manifest, &sig)).unwrap()["error"].is_string());
        // op 経由: 引数が欠けていれば入力不正
        assert!(call(9, 0, json!({}), std::ptr::null_mut(), None).is_err());

        let path = std::env::temp_dir().join(format!("knit-update-file-{}", std::process::id()));
        std::fs::write(&path, b"apk").unwrap();
        let digest = hex(&Sha256::digest(b"apk"));
        let file = |size: u64, sha: &str| json!({"path":path.to_str().unwrap(),"size":size,"sha256":sha});
        assert_eq!(call(10, 0, file(3, &digest), std::ptr::null_mut(), None).unwrap()["ok"], true);
        assert!(call(10, 0, file(3, &"0".repeat(64)), std::ptr::null_mut(), None).unwrap()["error"].is_string());
        assert!(call(10, 0, file(2, &digest), std::ptr::null_mut(), None).unwrap()["error"].is_string());
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn approval_ops_reject_bad_input_and_unknown_handles() {
        // 引数が欠ける・アドレスが不正・知らない依頼は、入力不正として拒否する
        assert!(call(11, 0, json!({}), std::ptr::null_mut(), None).is_err());
        assert!(call(11, 0, json!({"address":"not-an-ip"}), std::ptr::null_mut(), None).is_err());
        assert!(call(12, 999_999, json!({"approve":false}), std::ptr::null_mut(), None).is_err());
        assert!(call(12, 999_999, json!({"approve":true}), std::ptr::null_mut(), None).is_err());
    }
}

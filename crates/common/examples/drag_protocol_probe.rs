//! ドラッグ調査用。OSの入力・クリップボードを操作せず共通転送層の挙動を観測する。
//! cargo run --locked -q -p knit-common --example drag_protocol_probe
use std::io::Cursor;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use knit_common::bulk;

struct TempDir(PathBuf);
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn main() {
    let base = TempDir(std::env::temp_dir().join(format!(
        "knit-drag-probe-{}-{}",
        std::process::id(),
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos(),
    )));
    let mut rx = bulk::Receiver::new(&base.0.join("received"));
    let frames: &[(u8, &str, &[u8])] = &[
        (bulk::DROP_BEGIN, "DROP_BEGIN", b""),
        (
            bulk::FILE_BEGIN,
            "FILE_BEGIN",
            br#"{"name":"a.txt","size":3}"#,
        ),
        (bulk::DATA, "DATA", b"abc"),
        (bulk::FILE_END, "FILE_END", b""),
        (bulk::BATCH_END, "BATCH_END", b""),
        (bulk::DROP_END, "DROP_END", b""),
    ];
    let mut observations = Vec::new();
    for &(kind, label, body) in frames {
        let event = rx.feed(kind, body);
        observations.push(serde_json::json!({
            "frame": label,
            "event_emitted": event.is_some(),
            "drag_flag": match event { Some(bulk::Event::Files { drop, .. }) => Some(drop), _ => None },
        }));
    }
    assert!(observations
        .iter()
        .filter(|v| v["event_emitted"] == true)
        .all(|v| v["frame"] == "BATCH_END"));
    assert_eq!(
        observations
            .iter()
            .filter(|v| v["event_emitted"] == true)
            .count(),
        1
    );

    let folder = base.0.join("folder");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("nested.txt"), b"inside folder").unwrap();
    let mut wire = Vec::new();
    let count = bulk::send_files(&mut wire, &[folder], true).unwrap().sent;
    assert_eq!(count, 0);
    let mut reader = Cursor::new(wire);
    let mut buf = Vec::new();
    let mut rx = bulk::Receiver::new(&base.0.join("folder-received"));
    let mut folder_events = 0;
    while let Ok(kind) = bulk::read_frame(&mut reader, &mut buf) {
        if rx.feed(kind, &buf).is_some() {
            folder_events += 1;
        }
    }
    assert_eq!(folder_events, 0);
    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
        "scope": "共通転送層の実行観測。Windows/macOSのネイティブD&D実機試験ではない",
        "receiver_events": observations,
        "folder_with_one_file": { "sender_result": "Ok", "sent_files": count, "receiver_events": folder_events },
    })).unwrap());
}

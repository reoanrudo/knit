use std::{
    io::Cursor,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};
use knit_common::{
    bulk,
    proto::{self, Msg},
};

struct Temp(PathBuf);
static NEXT_TEMP: AtomicUsize = AtomicUsize::new(1);
impl Temp {
    fn new() -> Self {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let p = std::env::temp_dir().join(format!(
            "knit-drag-test-{}-{stamp}-{}",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&p).unwrap();
        std::fs::create_dir(p.join("received")).unwrap();
        Self(p)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn transfer_keeps_operation_id_separate_from_the_next_clipboard_batch() {
    let temp = Temp::new();
    let source = temp.0.join("資料.txt");
    std::fs::write(&source, b"drag content").unwrap();
    let mut wire = Vec::new();
    bulk::send_drag_files(&mut wire, std::slice::from_ref(&source), 42, || false).unwrap();
    bulk::send_files(&mut wire, &[source], false).unwrap();
    let mut receiver = bulk::Receiver::new(&temp.0.join("received"));
    let mut reader = Cursor::new(wire);
    let mut frame = Vec::new();
    let mut events = Vec::new();
    while let Ok(kind) = bulk::read_frame(&mut reader, &mut frame) {
        if let Some(event) = receiver.feed(kind, &frame) {
            events.push(event);
        }
    }
    assert_eq!(events.len(), 2);
    for (index, event) in events.into_iter().enumerate() {
        let bulk::Event::Files {
            paths,
            drop,
            drag_id,
            ..
        } = event
        else {
            panic!("expected files")
        };
        assert_eq!(drop, index == 0);
        assert_eq!(drag_id, if index == 0 { Some(42) } else { None });
        assert_eq!(paths.len(), 1);
        assert_eq!(std::fs::read(&paths[0]).unwrap(), b"drag content");
    }
}

#[test]
fn interrupted_drag_removes_its_staged_files() {
    let temp = Temp::new();
    let a = temp.0.join("a.txt");
    let b = temp.0.join("b.txt");
    std::fs::write(&a, b"a").unwrap();
    std::fs::write(&b, vec![1u8; bulk::CHUNK * 2]).unwrap();
    let calls = AtomicUsize::new(0);
    let mut wire = Vec::new();
    let error = bulk::send_drag_files(&mut wire, &[a, b], 7, || {
        calls.fetch_add(1, Ordering::Relaxed) >= 4
    })
    .unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::Interrupted);
    {
        let mut receiver = bulk::Receiver::new(&temp.0.join("received"));
        let mut reader = Cursor::new(wire);
        let mut frame = Vec::new();
        while let Ok(kind) = bulk::read_frame(&mut reader, &mut frame) {
            assert!(receiver.feed(kind, &frame).is_none());
        }
    }
    assert_eq!(
        std::fs::read_dir(temp.0.join("received")).unwrap().count(),
        0
    );
}

#[test]
fn disappearing_drag_file_fails_instead_of_waiting_forever_for_zero_files() {
    let temp = Temp::new();
    let source = temp.0.join("gone.txt");
    std::fs::write(&source, b"remove after preflight").unwrap();
    let calls = AtomicUsize::new(0);
    let result = bulk::send_drag_files(&mut Vec::new(), std::slice::from_ref(&source), 8, || {
        if calls.fetch_add(1, Ordering::Relaxed) == 0 {
            std::fs::remove_file(&source).unwrap();
        }
        false
    });
    assert!(
        result.is_err(),
        "元ファイルを読めない受け渡しは完了待ちにしない"
    );
}

#[test]
fn failed_receive_reports_the_operation_instead_of_waiting_for_ready() {
    let temp = Temp::new();
    let mut receiver = bulk::Receiver::new(&temp.0.join("received"));
    receiver.feed(bulk::DROP_ID_BEGIN, &11u64.to_le_bytes());
    receiver.feed(bulk::FILE_BEGIN, br#"{"name":"bad.txt","size":1}"#);
    receiver.feed(bulk::DATA, b"too long");
    receiver.feed(bulk::FILE_END, &[]);
    assert!(
        matches!(receiver.feed(bulk::BATCH_END, &[]), Some(bulk::Event::Files { paths, drag_id: Some(11), .. }) if paths.is_empty())
    );
}

#[test]
fn older_hello_disables_only_the_new_drag_capability() {
    let old = proto::decode(r#"{"t":"hello_ok","name":"Mac","w":1920,"h":1080}"#).unwrap();
    assert!(matches!(old, Msg::HelloOk { ver: 0, .. }));
    assert!(proto::compatible(11));
    let offer = Msg::DragOffer {
        id: 9,
        count: 2,
        total: 1234,
        position: 0.4,
    };
    assert!(
        matches!(proto::decode(&proto::encode(&offer)),Some(Msg::DragOffer {id:9,count:2,total:1234,position}) if position==0.4)
    );
}

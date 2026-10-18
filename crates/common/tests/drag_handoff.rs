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

/// I1: 掴み渡しの判断は理由つきの値(drag::Refusal)で決まり、通知の文言は現行
/// mac 側のまま(characterization)。8つ目の枝(スキップ警告)は拒否ではないので
/// 値にせず、文言だけ common に固定する。分岐や文言を足すときはこの表も足す
#[test]
fn refusal_reason_covers_every_reject_branch() {
    use knit_common::drag::{verdict_for, Collect, Refusal, Signals, Verdict, MAX_FILES, SKIPPED_NOTICE};
    let label = "上限を超えています";
    let s = |path_count, link_ok, busy, collect| Signals { path_count, link_ok, busy, collect };
    let cases: [(Signals, Refusal, &str); 8] = [
        (s(0, true, false, Collect::Pending), Refusal::EmptyGrab, "掴んだまま渡せるのは 64 フォルダ/ファイルまでです"),
        (s(MAX_FILES + 1, true, false, Collect::Pending), Refusal::TooMany, "掴んだまま渡せるのは 64 フォルダ/ファイルまでです"),
        (s(1, false, false, Collect::Pending), Refusal::LinkDown, "ファイルの転送経路がまだ準備中です。少し待ってもう一度掴んでください"),
        (s(1, true, false, Collect::Stale), Refusal::CollectStale, "接続の状態が変わったため、ファイルをもう一度掴み直してください"),
        // 集計が待ち時間内に終わらなかった場合。集計中(Pending)は Wait で断らない
        (s(1, true, false, Collect::Expired), Refusal::CollectTimeout, "ファイルの確認がまだ終わっていません。少し待ってからもう一度境界へ動かしてください"),
        (s(1, true, false, Collect::Failed { label: label.into() }), Refusal::CollectFailed { label: label.into() }, "掴んだまま渡せません(上限を超えています)"),
        (s(1, true, false, Collect::Done { entry_count: 0 }), Refusal::NothingToSend, "掴んだ項目の中に送れるものがありません"),
        (s(1, true, true, Collect::Done { entry_count: 2 }), Refusal::Busy, "前のファイルを転送中です。完了後にもう一度掴んでください"),
    ];
    for (signals, reason, body) in &cases {
        assert_eq!(verdict_for(signals), Verdict::Refused(reason.clone()), "{signals:?}");
        assert_eq!(reason.notice_body(), *body, "{reason:?}");
    }
    // 表が全理由を覆っている(理由を足したら表も足す)
    let all = [Refusal::TooMany, Refusal::EmptyGrab, Refusal::LinkDown, Refusal::CollectStale,
        Refusal::CollectTimeout, Refusal::CollectFailed { label: label.into() },
        Refusal::NothingToSend, Refusal::Busy];
    assert!(all.iter().all(|r| cases.iter().any(|(_, c, _)| c == r)), "表に無い理由がある");
    // 手書きの配列だけでは、理由を足してもテストは緑のまま。**網羅 match(ワイルドカード
    // 無し)**にして、バリアントを増やしたらコンパイラがここで止めるようにする
    for r in &all {
        match r {
            Refusal::TooMany
            | Refusal::EmptyGrab
            | Refusal::LinkDown
            | Refusal::CollectStale
            | Refusal::CollectTimeout
            | Refusal::CollectFailed { .. }
            | Refusal::NothingToSend
            | Refusal::Busy => {}
        }
    }
    assert_eq!(SKIPPED_NOTICE, "掴んだまま渡せるのは通常のファイルとフォルダだけです");
    // **連結**も固定する(区切りや空白を足しても golden list の多重集合には現れないため)
    assert_eq!(
        knit_common::drag::skipped_notice(" ※読めない・リンク等の 2 件をスキップ: a.txt ほか1件"),
        "掴んだまま渡せるのは通常のファイルとフォルダだけです ※読めない・リンク等の 2 件をスキップ: a.txt ほか1件"
    );
    assert_eq!(verdict_for(&s(1, true, false, Collect::Pending)), Verdict::Wait, "集計待ちは断らない");
    assert_eq!(verdict_for(&s(MAX_FILES, true, false, Collect::Done { entry_count: 3 })), Verdict::Ready { count: 3 });
}

/// 遅延提供(版 17)のハンドシェイクを、**実際のワイヤ表現**(JSON Lines)で端から端まで駆動する。
/// Windows 側は macOS ではビルドできないため、プロトコル(目録・受理・結果)と状態機械の
/// 噛み合わせをここで固定する(順序・件数照合・受理・結果の4点)
#[test]
fn lazy_handoff_runs_end_to_end_on_real_wire_messages() {
    use knit_common::drag::lazy::{LazyCommand, LazyHandoff, LazySide, LazyState};
    use knit_common::proto::{decode, encode, peer_features, ManifestEntry, Msg};

    // 版 17 の相手にだけ遅延提供を申し出る(旧版は目録を知らない)
    assert!(peer_features(17).lazy, "版 17 は対応");
    assert!(!peer_features(16).lazy, "版 16 は非対応 → 現行方式へフォールバック");

    let mut sender = LazyHandoff::new(LazySide::Sender);
    assert_eq!(
        sender.offer(peer_features(17).lazy, 2),
        vec![LazyCommand::SendManifest]
    );

    // 目録は実際に符号化・復号しても同じ内容が届く(行の名前と型の固定)
    let manifest = Msg::Manifest {
        id: 7,
        count: 2,
        total: 4096,
        files: vec![
            ManifestEntry {
                rel: "資料/a.txt".to_string(),
                size: 1024,
                kind: 0,
            },
            ManifestEntry {
                rel: "資料/sub".to_string(),
                size: 0,
                kind: 1,
            },
        ],
    };
    let Some(Msg::Manifest {
        id,
        count,
        total,
        files,
    }) = decode(&encode(&manifest))
    else {
        panic!("目録が往復する");
    };
    assert_eq!((id, count, total), (7, 2, 4096));
    assert_eq!(files[1].kind, 1, "フォルダの種別も運ぶ");
    assert_eq!(sender.manifest_sent(), vec![LazyCommand::StartNativeDrag]);

    // 相手は受理を返す(データ到着を待たない)
    let Some(Msg::DragAccept { id: accepted }) = decode(&encode(&Msg::DragAccept { id: 7 })) else {
        panic!("受理が往復する");
    };
    assert_eq!(accepted, 7);
    assert!(sender.accepted().is_empty(), "受理だけでは流さない");
    assert_eq!(sender.streaming(), vec![LazyCommand::Stream]);

    // 相手の結果(置けた)を受けて完了
    let Some(Msg::DragDone { id: done_id, copied }) = decode(&encode(&Msg::DragDone {
        id: 7,
        copied: true,
    })) else {
        panic!("結果が往復する");
    };
    assert!(done_id == 7 && copied);
    assert!(sender.completed(2).is_empty(), "件数一致でドロップ成立");
    assert_eq!(sender.state(), LazyState::Dropped);

    // 件数が合わない目録はドロップにしない(既存の整合性方針を遅延提供でも守る)
    let mut receiver = LazyHandoff::new(LazySide::Receiver);
    receiver.offer(true, 3);
    receiver.manifest_sent();
    receiver.accepted();
    receiver.streaming();
    let out = receiver.completed(2);
    assert!(
        out.contains(&LazyCommand::SendCancel),
        "欠けたら相手の待ちを解く: {out:?}"
    );
    assert_eq!(receiver.state(), LazyState::Aborted);
}

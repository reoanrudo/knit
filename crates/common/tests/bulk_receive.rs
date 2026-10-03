//! 大容量経路の受信強化(版 14)の結合試験。フォルダ展開・内容ハッシュ検証・
//! 原子的配置・パス遷移の拒否・キャンセルを、実際のフレーム列で確認する。
use std::io::Cursor;
use std::path::PathBuf;
use knit_common::bulk;

struct Temp(PathBuf);
impl Temp {
    fn new(tag: &str) -> Self {
        let p = std::env::temp_dir().join(format!(
            "knit-rx-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(p.join("received")).unwrap();
        Self(p)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// 送信側のフレーム列を Receiver へ通して、完了イベントを返す
fn deliver(wire: &[u8], dir: &std::path::Path) -> Vec<bulk::Event> {
    let mut rx = bulk::Receiver::new(dir);
    let mut reader = Cursor::new(wire);
    let mut frame = Vec::new();
    let mut events = Vec::new();
    while let Ok(kind) = bulk::read_frame(&mut reader, &mut frame) {
        if let Some(e) = rx.feed(kind, &frame) {
            events.push(e);
        }
    }
    events
}

fn make_tree(base: &std::path::Path) -> std::path::PathBuf {
    let root = base.join("資料フォルダ");
    std::fs::create_dir_all(root.join("sub/deep")).unwrap();
    std::fs::write(root.join("a.txt"), b"alpha").unwrap();
    std::fs::write(root.join("sub/b.bin"), vec![7u8; 300 * 1024]).unwrap();
    std::fs::write(root.join("sub/deep/c.md"), b"# nested").unwrap();
    // 空フォルダ・隠しファイル相当も混ぜる
    std::fs::create_dir_all(root.join("empty")).unwrap();
    std::fs::write(root.join(".hidden"), b"dot").unwrap();
    root
}

#[test]
fn folder_drag_roundtrips_with_structure_content_and_mtime() {
    let temp = Temp::new("folder");
    let root = make_tree(&temp.0);
    let paths = vec![root.clone(), temp.0.join("単独.txt")];
    std::fs::write(temp.0.join("単独.txt"), b"single").unwrap();
    let entries = bulk::collect(&paths, true, true, false).unwrap();
    assert_eq!(entries.len(), 5, "空フォルダは対象外: {entries:#?}");
    let total = bulk::entries_total(&entries);

    let mut wire = Vec::new();
    let sent = bulk::send_entries(
        &mut wire,
        &entries,
        true,
        Some(1234),
        &mut || false,
        &mut |_, _, _| {},
    )
    .unwrap();
    assert_eq!(sent.sent, 5);

    let dst = temp.0.join("received");
    let events = deliver(&wire, &dst);
    assert_eq!(events.len(), 1);
    let bulk::Event::Files {
        paths: got,
        drag_id,
        failed,
        ..
    } = &events[0]
    else {
        panic!("files event expected");
    };
    assert_eq!(drag_id, &Some(1234));
    assert!(failed.is_empty(), "{failed:?}");
    assert_eq!(got.len(), 5);
    // 構造の再現(相対パス)
    let names: Vec<String> = got
        .iter()
        .map(|p| p.strip_prefix(&dst).unwrap().to_string_lossy().into_owned())
        .collect();
    for expect in [
        "資料フォルダ/a.txt",
        "資料フォルダ/.hidden",
        "資料フォルダ/sub/b.bin",
        "資料フォルダ/sub/deep/c.md",
        "単独.txt",
    ] {
        assert!(
            names.iter().any(|n| n == expect),
            "{expect} が無い: {names:?}"
        );
    }
    // 内容と合計
    assert_eq!(
        std::fs::read(dst.join("資料フォルダ/sub/deep/c.md")).unwrap(),
        b"# nested"
    );
    let sum: u64 = got.iter().map(|p| std::fs::metadata(p).unwrap().len()).sum();
    assert_eq!(sum, total);
    // mtime の復元(秒精度)
    let src_mtime = std::fs::metadata(root.join("a.txt"))
        .unwrap()
        .modified()
        .unwrap()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let dst_mtime = std::fs::metadata(dst.join("資料フォルダ/a.txt"))
        .unwrap()
        .modified()
        .unwrap()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    assert_eq!(src_mtime, dst_mtime, "更新日時が保持される");
    // 受信ファイルに一時ファイルが残っていない
    for p in std::fs::read_dir(&dst).unwrap().flatten() {
        assert!(
            !p.file_name().to_string_lossy().starts_with(".knit-"),
            "一時ファイルが残留: {:?}",
            p.path()
        );
    }
}

#[test]
fn folder_to_a_legacy_peer_is_rejected_by_the_entry_point() {
    let temp = Temp::new("legacy");
    let root = make_tree(&temp.0);
    // 旧版相手向けの入口(allow_dirs=false)はフォルダを拒否する
    assert!(
        bulk::collect(std::slice::from_ref(&root), true, false, false).is_err(),
        "旧版へのフォルダ送信はエラー"
    );
    // 通常送信(非 strict)はフォルダを黙って飛ばす(従来動作)
    let entries = bulk::collect(std::slice::from_ref(&root), false, false, false).unwrap();
    assert!(entries.is_empty());
}

#[test]
fn corrupted_content_is_detected_by_hash_and_not_published() {
    let temp = Temp::new("hash");
    let src = temp.0.join("payload.bin");
    std::fs::write(&src, b"correct bytes").unwrap();
    let entries = bulk::collect(std::slice::from_ref(&src), true, true, false).unwrap();
    let mut wire = Vec::new();
    bulk::send_entries(&mut wire, &entries, false, None, &mut || false, &mut |_, _, _| {}).unwrap();

    // 経路で 1 バイト崩れた体で DATA を書き換える(FILE_BEGIN と FILE_END はそのまま)
    let mut tampered = wire.clone();
    let pos = tampered
        .windows(4)
        .position(|w| w == b"corr")
        .expect("DATA 内のバイト列を見つける");
    tampered[pos] = b'C';
    let dst = temp.0.join("received");
    let events = deliver(&tampered, &dst);
    assert_eq!(events.len(), 1);
    let bulk::Event::Files { paths, failed, .. } = &events[0] else {
        panic!("files event expected");
    };
    assert!(paths.is_empty(), "改ざんファイルは公開しない: {paths:?}");
    assert_eq!(failed.len(), 1, "{failed:?}");
    assert!(failed[0].1.contains("検証"), "{}", failed[0].1);
    assert!(!dst.join("payload.bin").exists());

    // 原本は普通に届く
    let events = deliver(&wire, &dst);
    let bulk::Event::Files { paths, failed, .. } = &events[0] else {
        panic!("files event expected");
    };
    assert!(failed.is_empty());
    assert_eq!(std::fs::read(&paths[0]).unwrap(), b"correct bytes");
}

#[test]
fn path_traversal_and_invalid_names_are_rejected_per_file() {
    let temp = Temp::new("traversal");
    let dst = temp.0.join("received");
    let mut rx = bulk::Receiver::new(&dst);
    for bad in [
        "../escape.txt",  // 遷移
        "a/../../escape", // 深い位置からの遷移
        "/absolute.txt",  // 絶対パス(先頭の空コンポーネント)
        "a//b.txt",       // 空コンポーネント
        "x/",             // 末尾区切り
    ] {
        let head = format!(r#"{{"name":"{bad}","size":1}}"#);
        rx.feed(bulk::FILE_BEGIN, head.as_bytes());
        rx.feed(bulk::DATA, b"z");
        rx.feed(bulk::FILE_END, &[0u8; 32]);
    }
    // 正常ファイルも混ぜる: 不正な名前があってもバッチは続行する
    //(ハッシュは旧版送信側を模して空 body = 検証スキップ)
    rx.feed(bulk::FILE_BEGIN, br#"{"name":"ok.txt","size":1}"#);
    rx.feed(bulk::DATA, b"z");
    rx.feed(bulk::FILE_END, &[]);
    let bulk::Event::Files { paths, failed, .. } =
        rx.feed(bulk::BATCH_END, &[]).expect("event")
    else {
        panic!("files event expected");
    };
    assert_eq!(failed.len(), 5, "不正名は全て拒否: {failed:?}");
    assert_eq!(paths.len(), 1, "正常ファイルは保存される: {paths:?}");
    assert!(!temp.0.join("escape.txt").exists(), "受信フォルダの外へ出ない");
    assert!(!temp.0.join("received/escape").exists());
}

#[test]
fn duplicate_names_in_one_batch_are_both_kept() {
    let temp = Temp::new("dup");
    let a = temp.0.join("same.txt");
    std::fs::write(&a, b"first").unwrap();
    let entries = vec![
        bulk::OutFile {
            src: a.clone(),
            name: "same.txt".into(),
            size: 5,
            mtime: 0,
            is_dir: false,
        },
        bulk::OutFile {
            src: a.clone(),
            name: "same.txt".into(),
            size: 5,
            mtime: 0,
            is_dir: false,
        },
    ];
    let mut wire = Vec::new();
    bulk::send_entries(&mut wire, &entries, false, None, &mut || false, &mut |_, _, _| {})
        .unwrap();
    let dst = temp.0.join("received");
    let bulk::Event::Files { paths, failed, renamed, .. } = &deliver(&wire, &dst)[0] else {
        panic!("files event expected");
    };
    assert!(failed.is_empty(), "{failed:?}");
    assert_eq!(*renamed, 1, "1 件が「名前 (n)」へ保存されたことが報告される");
    assert_eq!(paths.len(), 2, "同名を上書きしない: {paths:?}");
    let names: Vec<String> = paths
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert!(names.contains(&"same.txt".to_string()));
    assert!(names.contains(&"same (1).txt".to_string()));
    assert_eq!(std::fs::read(dst.join("same (1).txt")).unwrap(), b"first");
}

#[test]
fn cancel_mid_transfer_reports_interrupted_and_cleanup_removes_all_drag_files() {
    let temp = Temp::new("cancel");
    let src = temp.0.join("big.bin");
    std::fs::write(&src, vec![9u8; 700 * 1024]).unwrap();
    let entries = bulk::collect(std::slice::from_ref(&src), true, true, false).unwrap();
    let mut wire = Vec::new();
    let mut chunks = 0;
    let err = bulk::send_entries(
        &mut wire,
        &entries,
        true,
        Some(77),
        &mut || {
            chunks += 1;
            chunks > 2
        },
        &mut |_, _, _| {},
    )
    .unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::Interrupted);

    // 途中までのフレームを受信させ、切断(Drop)でドラッグ分が全削除されること
    let dst = temp.0.join("received");
    {
        let mut rx = bulk::Receiver::new(&dst);
        let mut reader = Cursor::new(&wire);
        let mut frame = Vec::new();
        while let Ok(kind) = bulk::read_frame(&mut reader, &mut frame) {
            let _ = rx.feed(kind, &frame);
        }
    }
    assert!(
        std::fs::read_dir(&dst).unwrap().count() == 0,
        "キャンセル済みドラッグの実体を残さない"
    );
}

#[test]
fn receiver_reports_progress_bytes_while_writing() {
    use std::sync::atomic::{AtomicU64, Ordering};
    let temp = Temp::new("progress");
    let src = temp.0.join("p.bin");
    std::fs::write(&src, vec![1u8; 300 * 1024]).unwrap();
    let entries = bulk::collect(std::slice::from_ref(&src), true, true, false).unwrap();
    let mut wire = Vec::new();
    bulk::send_entries(&mut wire, &entries, false, None, &mut || false, &mut |_, _, _| {}).unwrap();

    static SEEN: AtomicU64 = AtomicU64::new(0);
    let mut rx = bulk::Receiver::new(&temp.0.join("received"))
        .with_rx_progress(|n| {
            SEEN.fetch_max(n, Ordering::Relaxed);
        });
    let mut reader = Cursor::new(&wire);
    let mut frame = Vec::new();
    while let Ok(kind) = bulk::read_frame(&mut reader, &mut frame) {
        let _ = rx.feed(kind, &frame);
    }
    assert_eq!(SEEN.load(Ordering::Relaxed), 300 * 1024, "累計バイトが届く");
}

#[test]
fn incomplete_file_reports_failure_without_publishing_its_name() {
    let temp = Temp::new("incomplete");
    let dst = temp.0.join("received");
    let mut rx = bulk::Receiver::new(&dst);
    // 宣言 10 バイトに対し 3 バイトだけで FILE_END
    rx.feed(bulk::FILE_BEGIN, br#"{"name":"cut.bin","size":10}"#);
    rx.feed(bulk::DATA, b"abc");
    rx.feed(bulk::FILE_END, &[]);
    let bulk::Event::Files { paths, failed, .. } =
        rx.feed(bulk::BATCH_END, &[]).expect("event")
    else {
        panic!("files event expected");
    };
    assert!(paths.is_empty(), "未完のファイルを公開しない: {paths:?}");
    assert_eq!(failed.len(), 1, "{failed:?}");
    assert!(!dst.join("cut.bin").exists());
}

/// 版 15 の空フォルダ再現: 空ディレクトリ入りの一式を送ると、受け側でも
/// 空ディレクトリが同じ相対位置に再現される(消えない)
#[test]
fn empty_folders_roundtrip_as_directories() {
    let temp = Temp::new("emptydirs");
    let root = make_tree(&temp.0);
    // make_tree は「資料フォルダ/empty」(空)を含む
    let entries = bulk::collect(std::slice::from_ref(&root), true, true, true).unwrap();
    assert!(
        entries.iter().any(|e| e.is_dir && e.name.ends_with("/empty")),
        "空フォルダがディレクトリ印付きで列挙される: {entries:#?}"
    );
    let mut wire = Vec::new();
    let report = bulk::send_entries(
        &mut wire,
        &entries,
        false,
        None,
        &mut || false,
        &mut |_, _, _| {},
    )
    .unwrap();
    assert_eq!(report.sent, 5, "ファイル4件+空フォルダ1件: {report:?}");

    let dst = temp.0.join("received");
    let events = deliver(&wire, &dst);
    assert_eq!(events.len(), 1, "完了イベントは1つ: {events:?}");
    let bulk::Event::Files { paths, failed, .. } = &events[0] else {
        panic!("files event expected");
    };
    assert!(failed.is_empty(), "{failed:?}");
    let empty = dst.join("資料フォルダ/empty");
    assert!(empty.is_dir(), "空フォルダがディレクトリとして再現される: {paths:#?}");
}

/// 受信フォルダに同名ファイルが既にある場合の受信: 上書きせず
/// "名前 (n).ext" へ重複を保存する(既存分はそのまま残る)
#[test]
fn existing_file_is_not_overwritten_but_renamed() {
    let temp = Temp::new("existing");
    let src = temp.0.join("data.txt");
    std::fs::write(&src, b"new content").unwrap();
    let entries = bulk::collect(std::slice::from_ref(&src), true, true, false).unwrap();
    let mut wire = Vec::new();
    bulk::send_entries(&mut wire, &entries, false, None, &mut || false, &mut |_, _, _| {}).unwrap();

    let dst = temp.0.join("received");
    std::fs::create_dir_all(&dst).unwrap();
    std::fs::write(dst.join("data.txt"), b"existing content").unwrap();
    let events = deliver(&wire, &dst);
    assert_eq!(events.len(), 1, "完了イベントは1つ: {events:?}");
    let bulk::Event::Files { paths, failed, renamed, .. } = &events[0] else {
        panic!("files event expected");
    };
    assert!(failed.is_empty(), "{failed:?}");
    assert_eq!(*renamed, 1, "既存ファイルとの同名衝突が報告される");
    assert_eq!(paths.len(), 1);
    // 既存分は保たれ、届いた分は別名で公開される
    assert_eq!(std::fs::read(dst.join("data.txt")).unwrap(), b"existing content");
    assert_eq!(std::fs::read(dst.join("data (1).txt")).unwrap(), b"new content");
}

/// 掴みドラッグ(drag_id 付き)の途中切断: 空フォルダ(版 15)も含めて
/// 受け取った分は Drop の後片付けで消える(受信フォルダに残さない)
#[test]
fn cancelled_drag_removes_received_empty_dirs() {
    let temp = Temp::new("cancelempty");
    let root = make_tree(&temp.0);
    let entries = bulk::collect(std::slice::from_ref(&root), true, true, true).unwrap();
    let mut wire = Vec::new();
    let report = bulk::send_entries(
        &mut wire,
        &entries,
        true,
        Some(777),
        &mut || true, // 最初のチャンクで中止(Interrupted)
        &mut |_, _, _| {},
    );
    // 中止は Err(Interrupted)として返り、書きかけのフレーム列のみが残る
    assert!(report.is_err(), "中止時は Err: {report:?}");

    let dst = temp.0.join("received");
    let mut rx = bulk::Receiver::new(&dst);
    let mut reader = Cursor::new(&wire);
    let mut frame = Vec::new();
    let mut events = Vec::new();
    while let Ok(kind) = bulk::read_frame(&mut reader, &mut frame) {
        if let Some(e) = rx.feed(kind, &frame) {
            events.push(e);
        }
    }
    // drag_id 付きのため Receiver の drop で全て片付く(イベントは出ない)
    assert!(events.is_empty(), "中止時点で完了イベントは出ない: {events:?}");
    drop(rx);
    assert!(
        !dst.join("資料フォルダ").exists(),
        "ドラッグ中止で空フォルダ含め残さない"
    );
}

/// 受信側の件数上限: 0 バイトファイルの連打でも上限(送信側と同じ 512 件)を
/// 越えたら受け付けをやめる(ディスクエントリ・メモリの枯渇を防ぐ)
#[test]
fn file_count_limit_taints_receiver() {
    let temp = Temp::new("countlimit");
    let dst = temp.0.join("received");
    let mut rx = bulk::Receiver::new(&dst);
    let mut events = Vec::new();
    for i in 0..(knit_common::drag::MAX_BATCH_FILES + 3) {
        let head = format!(r#"{{"name":"z{i:03}.bin","size":0,"mtime":0}}"#);
        if let Some(e) = rx.feed(bulk::FILE_BEGIN, head.as_bytes()) {
            events.push(e);
        }
        if let Some(e) = rx.feed(bulk::FILE_END, &[]) {
            events.push(e);
        }
    }
    if let Some(e) = rx.feed(bulk::BATCH_END, &[]) {
        events.push(e);
    }
    let bulk::Event::Files { paths, .. } = &events[0] else {
        panic!("files event expected");
    };
    assert_eq!(
        paths.len(),
        knit_common::drag::MAX_BATCH_FILES,
        "上限ちょうどまで受け付け、以降は打ち切る"
    );
}

/// 相手が指定した異常に大きな mtime で panic しない(checked_add で無視される)
#[test]
fn set_mtime_ignores_absurd_values() {
    let temp = Temp::new("mtime");
    let p = temp.0.join("t.txt");
    std::fs::write(&p, b"x").unwrap();
    // UNIX_EPOCH + u64::MAX 秒はオーバーフローする値
    knit_common::files::set_mtime(&p, u64::MAX);
    knit_common::files::set_mtime(&p, 1_700_000_000);
}

/// strict=false の collect: サブフォルダが読み取れないときもバッチ全体を
/// 落とさず、そのフォルダを skipped へ報告する(空フォルダとしても送らない)
#[cfg(unix)]
#[test]
fn collect_with_skips_reports_unreadable_entries() {
    let temp = Temp::new("skips");
    let root = make_tree(&temp.0);
    let sub = root.join("sub");
    std::fs::set_permissions(&sub, <std::fs::Permissions as std::os::unix::fs::PermissionsExt>::from_mode(0o000)).unwrap();

    let cr = bulk::collect_with_skips(std::slice::from_ref(&root), false, true, true).unwrap();
    assert!(
        cr.skipped.iter().any(|p| p == &sub),
        "読めないフォルダが skipped に載る: {:?}",
        cr.skipped
    );
    assert!(
        !cr.entries.iter().any(|e| e.name == "資料フォルダ/sub" && e.is_dir),
        "読めなかったフォルダを空フォルダとして送らない: {:?}",
        cr.entries.iter().map(|e| &e.name).collect::<Vec<_>>()
    );
    // 読めた分(a.txt・.hidden・empty)はバッチに残る
    assert!(
        cr.entries.iter().any(|e| e.name.ends_with("a.txt")),
        "他の項目はバッチに残る"
    );
    // strict=true(掴みドラッグ)は予告と一致させるためエラー
    assert!(bulk::collect(std::slice::from_ref(&root), true, true, true).is_err());
    // 後片付けできるように権限を戻す
    std::fs::set_permissions(&sub, <std::fs::Permissions as std::os::unix::fs::PermissionsExt>::from_mode(0o755)).unwrap();
}

/// symlink を含む選択: リンクは辿らず skipped へ報告する。strict=true
///(掴みドラッグ経路)でもバッチを止めずに積むため、「送ったはずの項目が
/// 黙って欠ける」を通知で検出できる
#[cfg(unix)]
#[test]
fn collect_reports_symlinks_as_skipped_even_in_strict_mode() {
    let temp = Temp::new("symlink");
    let root = make_tree(&temp.0);
    std::os::unix::fs::symlink("a.txt", root.join("link-to-a")).unwrap();

    // strict=true(掴みドラッグ)でもリンクはエラーにせず skipped へ積む
    let cr = bulk::collect_with_skips(std::slice::from_ref(&root), true, true, true).unwrap();
    assert!(
        cr.skipped
            .iter()
            .any(|p| p.file_name().is_some_and(|n| n == "link-to-a")),
        "symlink が skipped に載る: {:?}",
        cr.skipped
    );
    assert!(
        !cr.entries.iter().any(|e| e.name.ends_with("link-to-a")),
        "symlink を実体として送らない: {:?}",
        cr.entries.iter().map(|e| &e.name).collect::<Vec<_>>()
    );
    // リンク以外は通常どおり展開される(件数の整合は壊れない)
    assert!(
        cr.entries.iter().any(|e| e.name.ends_with("a.txt")),
        "リンク以外の項目はバッチに残る"
    );
    // strict=false(⌘C 経路)でも同じ扱い
    let cr = bulk::collect_with_skips(std::slice::from_ref(&root), false, true, true).unwrap();
    assert!(
        cr.skipped
            .iter()
            .any(|p| p.file_name().is_some_and(|n| n == "link-to-a")),
        "strict=false でも symlink は skipped に載る: {:?}",
        cr.skipped
    );
}

/// バッチの途中で切断(Receiver の Drop)したとき、保存済み件数の部分報告が
/// フックへ届く(ドラッグ以外の経路。⌘C 送信中の切断・中止で「一部だけ届いた」
/// 状態が通知なしで残るのを防ぐ)
#[test]
fn interruption_reports_saved_count_via_hook() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static SAVED: AtomicUsize = AtomicUsize::new(usize::MAX);
    let temp = Temp::new("partial");
    let dst = temp.0.join("received");
    let mut wire = Vec::new();
    // 1 件目は FILE_END まで完了(保存済みになる)
    bulk::write_frame(&mut wire, bulk::FILE_BEGIN, br#"{"name":"a.txt","size":3}"#).unwrap();
    bulk::write_frame(&mut wire, bulk::DATA, b"one").unwrap();
    bulk::write_frame(&mut wire, bulk::FILE_END, &[]).unwrap();
    // 2 件目は FILE_BEGIN の直後に切断した体(BATCH_END は来ない)
    bulk::write_frame(&mut wire, bulk::FILE_BEGIN, br#"{"name":"b.txt","size":3}"#).unwrap();
    {
        let mut rx = bulk::Receiver::new(&dst).with_interrupted_report(|e| {
            if let bulk::Event::Interrupted { saved } = e {
                SAVED.store(saved, Ordering::Relaxed);
            }
        });
        let mut reader = Cursor::new(&wire);
        let mut frame = Vec::new();
        while let Ok(kind) = bulk::read_frame(&mut reader, &mut frame) {
            let _ = rx.feed(kind, &frame);
        }
    } // ここで Drop → 保存済み件数の部分報告
    assert_eq!(
        SAVED.load(Ordering::Relaxed),
        1,
        "保存済み 1 件の部分報告が出る"
    );
    assert_eq!(std::fs::read(dst.join("a.txt")).unwrap(), b"one", "完了済み分は保持される");
    assert!(!dst.join("b.txt").exists(), "書きかけの分は残らない");
}

/// ドラッグ経路(drag_id 付き)の中断は全削除が保証されているため、
/// 部分報告(Interrupted)を出さない
#[test]
fn cancelled_drag_does_not_report_partial_save() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static SAVED: AtomicUsize = AtomicUsize::new(usize::MAX);
    let temp = Temp::new("partialdrag");
    let dst = temp.0.join("received");
    let mut wire = Vec::new();
    bulk::write_frame(&mut wire, bulk::DROP_ID_BEGIN, &777u64.to_le_bytes()).unwrap();
    bulk::write_frame(&mut wire, bulk::FILE_BEGIN, br#"{"name":"a.txt","size":3}"#).unwrap();
    bulk::write_frame(&mut wire, bulk::DATA, b"one").unwrap();
    bulk::write_frame(&mut wire, bulk::FILE_END, &[]).unwrap();
    {
        let mut rx = bulk::Receiver::new(&dst).with_interrupted_report(|e| {
            if let bulk::Event::Interrupted { saved } = e {
                SAVED.store(saved, Ordering::Relaxed);
            }
        });
        let mut reader = Cursor::new(&wire);
        let mut frame = Vec::new();
        while let Ok(kind) = bulk::read_frame(&mut reader, &mut frame) {
            let _ = rx.feed(kind, &frame);
        }
    }
    assert_eq!(
        SAVED.load(Ordering::Relaxed),
        usize::MAX,
        "ドラッグ中止は全削除のため部分報告を出さない"
    );
    assert!(!dst.join("a.txt").exists(), "ドラッグ中止の実体は残さない");
}

/// 件数上限超過のエラーメッセージに実際の件数と上限が載る(通知の
/// 「展開後 N 件以上(上限 512 件)」の材料)
#[test]
fn collect_error_message_carries_actual_count_and_limit() {
    let dir = std::env::temp_dir().join(format!(
        "knit-limit-msg-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    for i in 0..=(knit_common::drag::MAX_BATCH_FILES) {
        std::fs::write(dir.join(format!("f{i:03}.txt")), b"x").unwrap();
    }
    let e = bulk::collect(&[dir.clone()], true, true, false).unwrap_err();
    let msg = e.to_string();
    assert!(
        msg.contains(&format!(
            "(limit {})",
            knit_common::drag::MAX_BATCH_FILES
        )),
        "メッセージに上限が載る: {msg}"
    );
    assert!(
        msg.contains(&format!(
            ": {} ",
            knit_common::drag::MAX_BATCH_FILES + 1
        )),
        "メッセージに検出時点の実際の件数が載る: {msg}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

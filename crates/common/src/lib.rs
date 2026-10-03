pub mod credentials;
pub mod diagnose;
pub mod doctor;
pub mod drag;
pub mod history;
pub mod pairing;
pub mod spkstate;
pub mod retry;
pub mod share;
pub mod smartguard;
pub mod update;
pub mod xfer;
// 共通プロトコル定義(JSON Lines over TCP)
pub mod envutil;
pub mod proto;
pub mod urlx;
pub mod files;
pub mod secure;
pub mod net;
pub mod discover;
pub mod connect;
pub mod bulk;
pub mod keymap;
pub mod charmap;

#[cfg(test)]
mod tests {
    #[test]
    fn tablet_japanese_capability_keeps_old_android_apps_compatible() {
        use crate::proto::Msg;
        let legacy: Msg = serde_json::from_str(r#"{"t":"tablet_info","width_mm":285.0,"height_mm":190.0,"control":true,"keyboard":true}"#).unwrap();
        assert!(matches!(legacy, Msg::TabletInfo { japanese: false, .. }));
        let new = Msg::TabletInfo { width_mm: 285.0, height_mm: 190.0, control: true, keyboard: true, japanese: true };
        let json = serde_json::to_string(&new).unwrap();
        assert!(matches!(serde_json::from_str::<Msg>(&json).unwrap(), Msg::TabletInfo { japanese: true, .. }));
    }
    #[test]
    fn peer_names_drop_invisible_and_direction_characters() {
        assert_eq!(crate::proto::safe_peer_name("Re\u{200b}o\u{200e}'s\u{feff} PC\u{202e}"), "Reo's PC");
        assert_eq!(crate::proto::safe_peer_name("普通の名前"), "普通の名前");
    }

    #[test]
    fn bulk_frames_follow_the_local_share_scope() {
        use crate::share::Scope;
        use crate::bulk::*;
        let clip_only = Scope { clip: true, files: false, audio: false };
        let files_only = Scope { clip: false, files: true, audio: false };
        // 画像はクリップボード、ファイル・ドラッグはファイルの範囲。KEEPALIVE は常に通す
        assert!(frame_allowed(KEEPALIVE, false, Scope::INPUT_ONLY));
        for k in [IMAGE_BEGIN, IMAGE_END] {
            assert!(frame_allowed(k, true, clip_only));
            assert!(!frame_allowed(k, true, files_only));
        }
        assert!(frame_allowed(DATA, true, clip_only) && !frame_allowed(DATA, true, files_only));
        assert!(frame_allowed(DATA, false, files_only) && !frame_allowed(DATA, false, clip_only));
        for k in [FILE_BEGIN, FILE_END, BATCH_END, DROP_BEGIN, DROP_END, DROP_ID_BEGIN] {
            assert!(frame_allowed(k, false, files_only));
            assert!(!frame_allowed(k, false, clip_only));
            assert!(!frame_allowed(k, false, Scope::INPUT_ONLY));
        }
        assert!(frame_allowed(FILE_BEGIN, false, Scope::ALL));
    }

    use super::keymap::mac_kc_to_win_vk;
    use super::proto::*;

    #[test]
    fn peer_names_drop_control_and_bidi_overrides() {
        // 制御文字と Bidi オーバーライド(見た目の文字順を偽装する不可視文字)は
        // 通知・ログへ載せない。hello と登録(pairing)で同じ規則を使う
        assert_eq!(safe_peer_name("PC\u{202e}gpj.exe"), "PCgpj.exe");
        assert_eq!(safe_peer_name("A\u{2066}B\u{2069}C"), "ABC");
        assert_eq!(safe_peer_name("Mac\nPro"), "MacPro");
        // 呼び出し側は trim 済みの名前を渡す(空白のみの名前は既定名へ落ちる)
        assert_eq!(safe_peer_name("  \u{7}\t".trim()), "");
        // 長さは 48 文字で切る
        assert_eq!(safe_peer_name(&"x".repeat(60)).chars().count(), 48);
    }

    #[test]
    fn hello_carries_monitors_and_stays_wire_compatible() {
        // 版 13: 端末 id と全モニター構成が hello で往復する
        let m = Monitor {
            x: -1080,
            y: 0,
            w: 1080,
            h: 1920,
            name: String::new(),
        };
        let wire = encode(&Msg::Hello {
            ver: 13,
            name: "win-a".into(),
            token: String::new(),
            w: 1920,
            h: 1080,
            id: "abc123".into(),
            monitors: vec![m.clone()],
        });
        match decode(&wire) {
            Some(Msg::Hello {
                monitors, id, ver, ..
            }) => {
                assert_eq!(
                    (ver, id.as_str(), monitors),
                    (13, "abc123", vec![m.clone()])
                );
            }
            other => panic!("hello が復元できない: {other:?}"),
        }
        // 旧版(12)の形式: id・monitors 無しの JSON も警告なく読める(空になる)
        let legacy = r#"{"t":"hello","ver":12,"name":"old","w":1920,"h":1080}"#;
        match decode(legacy) {
            Some(Msg::Hello {
                monitors, id, ver, ..
            }) => {
                assert_eq!(ver, 12);
                assert!(monitors.is_empty() && id.is_empty());
            }
            other => panic!("旧形式 hello が読めない: {other:?}"),
        }
        // hello_ok も同様に monitors を往復する
        let ok_wire = encode(&Msg::HelloOk {
            name: "mac".into(),
            w: 2056,
            h: 1329,
            ver: 13,
            id: "m1".into(),
            monitors: vec![m.clone()],
        });
        match decode(&ok_wire) {
            Some(Msg::HelloOk { monitors, id, .. }) => {
                assert_eq!((id.as_str(), monitors), ("m1", vec![m]));
            }
            other => panic!("hello_ok が復元できない: {other:?}"),
        }
        // 端末識別子は呼び出し間で安定し、表示用サマリも動く
        assert_eq!(device_id(), device_id());
        assert_eq!(Monitor::summary(&[]), "不明");
        assert!(Monitor::summary(&[Monitor {
            x: 0,
            y: 0,
            w: 1920,
            h: 1080,
            name: String::new()
        }])
        .contains("1920x1080"));
    }

    #[test]
    fn keymap_covers_full_keyboard_and_numpad_enter() {
        assert_eq!(mac_kc_to_win_vk(76), Some(0x0D));
        assert_eq!(mac_kc_to_win_vk(57), Some(0x14));
        assert_eq!(mac_kc_to_win_vk(114), Some(0x2D));
        assert_eq!(mac_kc_to_win_vk(90), Some(0x83));
        // 数字row の取り違え実績(21=4, 23=5)の回帰防止
        assert_eq!(mac_kc_to_win_vk(21), Some(0x34));
        assert_eq!(mac_kc_to_win_vk(23), Some(0x35));
        // かな/英数は win 側で IME 開閉へ変換されるため keymap の外(到達不能の固定)
        assert_eq!(mac_kc_to_win_vk(104), None);
        assert_eq!(mac_kc_to_win_vk(102), None);
        assert_eq!(mac_kc_to_win_vk(200), None);
    }

    #[test]
    fn url_transfer_accepts_only_web_urls() {
        use super::urlx::transferable as ok;
        assert!(ok("https://example.com/page?q=1"));
        assert!(ok("http://192.168.0.5:8080/"));
        // ローカルスキーム・変な先頭は拒否(受信側での意図しないプロトコル起動を塞ぐ)
        assert!(!ok("file:///etc/passwd"));
        assert!(!ok("javascript:alert(1)"));
        assert!(!ok("about:blank"));
        assert!(!ok("ftp://example.com/f"));
        assert!(!ok(""));
        // 過大な URL・空白/制御文字入り(ShellExecuteW への偽装)は拒否
        let long = format!("https://example.com/{}", "a".repeat(2048));
        assert!(!ok(&long));
        assert!(!ok("https://example.com/a b"));
        assert!(!ok("https://example.com/a\nb"));
        assert!(!ok("https://example.com/a\u{0}b"));
    }

    #[test]
    fn received_file_names_are_neutralized() {
        use super::files::sanitize;
        assert_eq!(sanitize("../../etc/passwd"), ".._.._etc_passwd");
        assert_eq!(sanitize("a\\b:c.txt"), "a_b_c.txt");
        // 先頭の '.' は隠しファイルに意味があるため残し、末尾の '.'・空白だけ整える
        assert_eq!(sanitize("  .hidden  "), ".hidden");
        assert_eq!(sanitize(".gitignore"), ".gitignore");
        assert_eq!(sanitize("report.. "), "report");
        assert_eq!(sanitize("CON.txt"), "_CON.txt");
        assert_eq!(sanitize("com1"), "_com1");
        assert_eq!(sanitize("console.txt"), "console.txt");
        assert_eq!(sanitize(""), "file");
        assert_eq!(sanitize("報告書.pdf"), "報告書.pdf");
    }

    #[test]
    fn files_key_is_stable_and_size_aware() {
        use super::files::key;
        let base = std::env::temp_dir().join(format!("knit-key-{}", std::process::id()));
        std::fs::create_dir_all(&base).unwrap();
        std::fs::write(base.join("a.txt"), b"hello").unwrap();
        let a = base.join("a.txt").to_string_lossy().into_owned();
        let k1 = key(std::slice::from_ref(&a));
        let k2 = key(std::slice::from_ref(&a));
        assert_eq!(k1, k2, "同じ選択は同じ指紋");
        std::fs::write(base.join("a.txt"), b"hello world").unwrap();
        assert_ne!(k1, key(std::slice::from_ref(&a)), "内容が変われば指紋も変わる");
        assert_eq!(
            key(&[a.clone(), a.clone()]).split('|').next(),
            key(&[a.clone(), a.clone()]).split('|').next(),
            "順序込みで一貫"
        );
        std::fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn same_name_is_never_overwritten() {
        let dir = std::env::temp_dir().join(format!("knit-files-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // 受信の流れと同じ形で: 一時ファイルに書き → 検疫属性 → 最終名へ公開
        let (fa, ta) = super::files::create_temp(&dir).unwrap();
        drop(fa);
        std::fs::write(&ta, b"a").unwrap();
        super::files::mark_untrusted(&ta);
        let a = super::files::publish(&ta, &dir.join("x.txt")).unwrap();
        let (fb, tb) = super::files::create_temp(&dir).unwrap();
        drop(fb);
        std::fs::write(&tb, b"b").unwrap();
        let b = super::files::publish(&tb, &dir.join("x.txt")).unwrap();
        assert_ne!(a, b);
        assert!(b.to_string_lossy().ends_with("x (1).txt"));
        #[cfg(target_os = "macos")]
        {
            let out = std::process::Command::new("xattr")
                .arg("-p")
                .arg("com.apple.quarantine")
                .arg(&a)
                .output()
                .unwrap();
            assert!(
                String::from_utf8_lossy(&out.stdout).contains(";Knit;"),
                "quarantine 属性が付いていない"
            );
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn bulk_roundtrip_files_and_image() {
        use super::bulk::*;
        let base = std::env::temp_dir().join(format!("knit-bulk-{}", std::process::id()));
        let src = base.join("src");
        std::fs::create_dir_all(&src).unwrap();
        let big: Vec<u8> = (0..(CHUNK * 2 + 123)).map(|i| (i % 251) as u8).collect();
        std::fs::write(src.join("big.bin"), &big).unwrap();
        std::fs::write(src.join("a.txt"), b"hello").unwrap();
        let mut wire = Vec::new();
        let paths = vec![src.join("big.bin"), src.join("a.txt")];
        assert_eq!(send_files(&mut wire, &paths, true).unwrap().sent, 2);
        let dib: Vec<u8> = (0..(CHUNK + 7)).map(|i| (i % 13) as u8).collect();
        send_image(&mut wire, &dib).unwrap();

        let mut rx = Receiver::new(&base.join("dst"));
        let mut r = std::io::Cursor::new(wire);
        let mut buf = Vec::new();
        let mut events = Vec::new();
        while let Ok(k) = read_frame(&mut r, &mut buf) {
            if let Some(e) = rx.feed(k, &buf) {
                events.push(e);
            }
        }
        assert_eq!(events.len(), 2);
        match &events[0] {
            Event::Files { paths, drop, .. } => {
                assert!(*drop);
                assert_eq!(std::fs::read(&paths[0]).unwrap(), big);
                assert_eq!(std::fs::read(&paths[1]).unwrap(), b"hello");
            }
            e => panic!("unexpected {e:?}"),
        }
        assert!(matches!(&events[1], Event::Image(d) if *d == dib));
        std::fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn bulk_rejects_oversized_frames_and_overflowing_data() {
        use super::bulk::*;
        let mut wire = Vec::new();
        wire.push(DATA);
        wire.extend_from_slice(&((MAX_FRAME as u32) + 1).to_le_bytes());
        assert!(read_frame(&mut std::io::Cursor::new(wire), &mut Vec::new()).is_err());

        // 宣言サイズより多いデータを送られたらファイルごと破棄する(失敗として報告)
        let base = std::env::temp_dir().join(format!("knit-bulk-bad-{}", std::process::id()));
        let mut rx = Receiver::new(&base);
        rx.feed(FILE_BEGIN, br#"{"name":"x.bin","size":3}"#);
        rx.feed(DATA, b"toolong");
        rx.feed(FILE_END, &[]);
        match rx.feed(BATCH_END, &[]) {
            Some(Event::Files { paths, failed, .. }) => {
                assert!(paths.is_empty());
                assert_eq!(failed.len(), 1, "破棄したファイルを失敗として報告: {failed:?}");
            }
            _ => panic!("失敗の報告イベントが出るべき"),
        }
        assert!(!base.join("x.bin").exists());
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn bulk_sender_rejects_files_over_the_limit_before_writing() {
        use super::bulk::*;
        // 一部だけ届いて成功に見えないよう、超過は送信開始前に拒否する。
        // 超過ファイルはスパース(実データなし)で作る
        let base = std::env::temp_dir().join(format!("knit-bulk-cap-{}", std::process::id()));
        std::fs::create_dir_all(&base).unwrap();
        std::fs::write(base.join("ok.txt"), b"ok").unwrap();
        let huge = std::fs::File::create(base.join("huge.bin")).unwrap();
        huge.set_len(crate::files::MAX_FILE + 1).unwrap();
        drop(huge);

        let mut wire = Vec::new();
        let paths = vec![base.join("huge.bin"), base.join("ok.txt")];
        assert_eq!(
            send_files(&mut wire, &paths, true).unwrap_err().kind(),
            std::io::ErrorKind::InvalidInput
        );
        assert!(wire.is_empty(), "上限超過時には DROP_BEGIN も送らない");
        // それぞれは上限内でも、合計が上限を超える選択は拒否する。
        let huge = std::fs::OpenOptions::new()
            .write(true)
            .open(base.join("huge.bin"))
            .unwrap();
        huge.set_len(MAX_TOTAL / 2 + 1).unwrap();
        drop(huge);
        let paths = vec![base.join("huge.bin"), base.join("huge.bin")];
        assert!(
            send_files_with_progress(&mut wire, &paths, false, |_, _| panic!(
                "送信前に拒否すべき"
            ))
            .is_err()
        );
        assert!(wire.is_empty());
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn bulk_sender_reports_files_shortened_during_transfer() {
        use super::bulk::*;
        let base = std::env::temp_dir().join(format!("knit-shortened-{}", std::process::id()));
        std::fs::create_dir_all(&base).unwrap();
        let src = base.join("source.bin");
        std::fs::write(&src, vec![5u8; CHUNK * 2]).unwrap();
        let mut wire = Vec::new();
        let err = send_files_with_progress(&mut wire, std::slice::from_ref(&src), false, |_, _| {
            std::fs::OpenOptions::new()
                .write(true)
                .open(&src)
                .unwrap()
                .set_len(CHUNK as u64)
                .unwrap();
        })
        .unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::UnexpectedEof);
        let dst = base.join("received");
        let mut rx = Receiver::new(&dst);
        let mut reader = std::io::Cursor::new(wire);
        let mut buf = Vec::new();
        while let Ok(kind) = read_frame(&mut reader, &mut buf) {
            assert!(rx.feed(kind, &buf).is_none(), "未完了の転送を成功にしない");
        }
        drop(rx);
        assert!(!dst.join("source.bin").exists());
        std::fs::remove_dir_all(base).unwrap();
    }

    /// seek_first: 応答があれば即座に返り、無ければタイムアウトで None を返す。
    /// ループバックで応答側ソケットを立てて実動作を確認する
    #[test]
    fn seek_first_returns_first_answer_and_times_out() {
        use super::discover::{room_id, seek_first};
        use std::net::{SocketAddr, UdpSocket};
        use std::time::{Duration, Instant};
        let token = "seek-first-test-token";

        // 応答側を立てて、そのポートへ問い合わせる
        let responder = UdpSocket::bind("127.0.0.1:0").unwrap();
        let port = responder.local_addr().unwrap().port();
        let ask = format!("KNIT?{}", room_id(token));
        let ans = format!("KNIT!{}", room_id(token));
        let answerer = std::thread::spawn(move || {
            let mut buf = [0u8; 128];
            let (n, from) = responder.recv_from(&mut buf).unwrap();
            assert_eq!(&buf[..n], ask.as_bytes());
            responder.send_to(ans.as_bytes(), from).unwrap();
        });
        let t0 = Instant::now();
        let found = seek_first(
            SocketAddr::from(([127, 0, 0, 1], port)),
            token,
            Duration::from_secs(3),
        );
        assert!(found.is_some(), "応答があるのに None");
        assert!(
            t0.elapsed() < Duration::from_secs(2),
            "応答があるなら長く待たない: {:?}",
            t0.elapsed()
        );
        answerer.join().unwrap();

        // 応答が無ければ wait 経過で None。quiet は保持したまま(閉じたポートへ送ると
        // ICMP unreachable が Err として即座に返り、タイムアウト計測が崩れる環境がある)
        let quiet = UdpSocket::bind("127.0.0.1:0").unwrap();
        let quiet_port = quiet.local_addr().unwrap().port();
        let t0 = Instant::now();
        assert_eq!(
            seek_first(
                SocketAddr::from(([127, 0, 0, 1], quiet_port)),
                token,
                Duration::from_millis(300)
            ),
            None,
            "応答が無いのに Some"
        );
        assert!(
            t0.elapsed() >= Duration::from_millis(250),
            "タイムアウト前に返った: {:?}",
            t0.elapsed()
        );
        drop(quiet);
    }

    /// media_vk: Vol のメディア拡張 op と Windows VK の対応の固定
    #[test]
    fn media_vk_maps_ops_to_windows_media_keys() {
        use super::proto::media_vk;
        assert_eq!(media_vk(3), Some(0xB1)); // 前へ
        assert_eq!(media_vk(4), Some(0xB3)); // 再生・一時停止
        assert_eq!(media_vk(5), Some(0xB0)); // 次へ
        assert_eq!(media_vk(0), None); // 音量 up は対象外
        assert_eq!(media_vk(2), None);
        assert_eq!(media_vk(6), None);
    }

    /// 受信側の合計上限: 超過で破棄し、以降の FILE_BEGIN も受け付けない
    #[test]
    fn bulk_receiver_caps_total_and_taints() {
        use super::bulk::*;
        let base = std::env::temp_dir().join(format!("knit-cap-{}", std::process::id()));
        let mut rx = Receiver::new(&base);
        rx.set_limit_for_test(1_000);
        // 600B × 2 ファイルで 1,200B > 上限 1,000B
        for i in 0..2 {
            rx.feed(
                FILE_BEGIN,
                format!("{{\"name\":\"c{i}.bin\",\"size\":600}}").as_bytes(),
            );
            rx.feed(DATA, &[0u8; 600]);
            rx.feed(FILE_END, &[]);
        }
        // 1 本目(600B)は正当に完了しているため、イベントは 1 件だけ返る
        match rx.feed(BATCH_END, &[]) {
            Some(Event::Files { paths, .. }) => {
                assert_eq!(paths.len(), 1, "超過分を除いた 1 件のみ: {paths:?}");
            }
            _ => panic!("1 本目の完了イベントが出るべき"),
        }
        assert!(base.join("c0.bin").exists(), "上限内の 1 本目は保持");
        assert!(!base.join("c1.bin").exists(), "2 本目(超過分)は破棄");
        // 不正な上限超過の後は、BATCH_END を送っても拒否を解除しない。
        rx.feed(FILE_BEGIN, br#"{"name":"z.bin","size":1}"#);
        rx.feed(DATA, b"z");
        rx.feed(FILE_END, &[]);
        assert!(
            rx.feed(BATCH_END, &[]).is_none(),
            "不正な接続の後続バッチも拒否"
        );
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn bulk_receiver_limit_applies_to_each_completed_batch() {
        use super::bulk::*;
        let base = std::env::temp_dir().join(format!("knit-batches-{}", std::process::id()));
        let mut rx = Receiver::new(&base);
        rx.set_limit_for_test(1_000);
        for i in 0..3 {
            rx.feed(
                FILE_BEGIN,
                format!("{{\"name\":\"{i}.bin\",\"size\":1000}}").as_bytes(),
            );
            rx.feed(DATA, &[7u8; 1_000]);
            rx.feed(FILE_END, &[]);
            match rx.feed(BATCH_END, &[]) {
                Some(Event::Files { paths, .. }) => {
                    assert_eq!(paths.len(), 1);
                    assert_eq!(std::fs::read(&paths[0]).unwrap(), vec![7u8; 1_000]);
                }
                e => panic!("正当な連続転送 {i} が失われた: {e:?}"),
            }
        }
        std::fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn bulk_receiver_checks_declared_capacity_and_cleans_unfinished_files() {
        use super::bulk::*;
        let base = std::env::temp_dir().join(format!("knit-declared-{}", std::process::id()));
        let mut rx = Receiver::new(&base);
        // 4GiB を超えるヘッダも扱える。実データなしで新上限の境界を確認する。
        let size = 10u64 * 1024 * 1024 * 1024;
        rx.feed(
            FILE_BEGIN,
            format!("{{\"name\":\"large.bin\",\"size\":{size}}}").as_bytes(),
        );
        assert!(
            !base.join("large.bin").exists(),
            "転送中は最終名を実体化しない(原子的配置)"
        );
        assert!(
            std::fs::read_dir(&base)
                .unwrap()
                .any(|e| e.unwrap().file_name().to_string_lossy().starts_with(".knit-")),
            "一時ファイルへ書き込み中"
        );
        rx.feed(DATA, b"partial");
        assert!(
            rx.feed(BATCH_END, &[]).is_none(),
            "未完了のファイルを公開しない"
        );
        assert!(!base.join("large.bin").exists());
        rx.feed(
            FILE_BEGIN,
            format!("{{\"name\":\"too-large.bin\",\"size\":{}}}", size + 1).as_bytes(),
        );
        assert!(
            !base.join("too-large.bin").exists(),
            "上限超過は作成前に拒否する"
        );

        let mut rx = Receiver::new(&base);
        rx.set_limit_for_test(10);
        rx.feed(FILE_BEGIN, br#"{"name":"first.bin","size":8}"#);
        rx.feed(DATA, b"12345678");
        rx.feed(FILE_END, &[]);
        rx.feed(FILE_BEGIN, br#"{"name":"overflow.bin","size":3}"#);
        assert!(
            !base.join("overflow.bin").exists(),
            "合計上限も作成前に拒否する"
        );
        std::fs::remove_dir_all(base).unwrap();
    }

    /// 切断(Drop)時に書き込み途中のファイルを破棄し、完了済みは残す
    #[test]
    fn bulk_receiver_discards_partial_on_drop() {
        use super::bulk::*;
        let base = std::env::temp_dir().join(format!("knit-drop-{}", std::process::id()));
        let mut rx = Receiver::new(&base);
        // 1 件目は完了、2 件目は DATA 途中で切断
        rx.feed(FILE_BEGIN, br#"{"name":"done.bin","size":3}"#);
        rx.feed(DATA, b"abc");
        rx.feed(FILE_END, &[]);
        assert!(rx.feed(BATCH_END, &[]).is_some());
        rx.feed(FILE_BEGIN, br#"{"name":"half.bin","size":100}"#);
        rx.feed(DATA, &[0u8; 10]);
        drop(rx); // ここで切断
        assert!(base.join("done.bin").exists(), "完了済みは残す");
        assert!(!base.join("half.bin").exists(), "書き込み途中は破棄");
        std::fs::remove_dir_all(base).unwrap();
    }

    /// 進捗コールバック: 単調非減少・最終値が合計に一致・チャンク毎に呼ばれる
    #[test]
    fn send_files_progress_is_monotonic_and_reaches_total() {
        use super::bulk::*;
        let base = std::env::temp_dir().join(format!("knit-prog-{}", std::process::id()));
        let src = base.join("src");
        std::fs::create_dir_all(&src).unwrap();
        // 1.5 チャンク分のファイル(複数チャンクをまたぐ)
        let data: Vec<u8> = (0..(CHUNK + CHUNK / 2)).map(|i| (i % 97) as u8).collect();
        std::fs::write(src.join("p.bin"), &data).unwrap();
        std::fs::write(src.join("q.bin"), b"second file").unwrap();
        let mut wire = Vec::new();
        let mut log: Vec<(u64, u64)> = Vec::new();
        send_files_with_progress(
            &mut wire,
            &[src.join("p.bin"), src.join("q.bin")],
            false,
            |s, t| log.push((s, t)),
        )
        .unwrap();
        let (last_s, last_t) = *log.last().unwrap();
        assert_eq!(last_t, data.len() as u64 + 11, "宣言合計=全ファイルサイズ");
        assert_eq!(last_s, last_t, "最終送信済み=合計");
        let mut prev = 0;
        for (s, _) in &log {
            assert!(*s >= prev, "進捗は単調非減少: {log:?}");
            prev = *s;
        }
        assert!(log.len() >= 2, "チャンク毎に呼ばれる: {}", log.len());
        std::fs::remove_dir_all(base).unwrap();
    }

    /// resolve の併合: 発見結果を先頭に、手動指定を重複排除して並べる
    #[test]
    fn merge_candidates_puts_discovery_first_and_dedups() {
        use super::connect::merge_candidates;
        use std::net::IpAddr;
        let found: Vec<IpAddr> = vec!["192.168.0.1".parse().unwrap()];
        let merged = merge_candidates(found, Some("100.100.10.9,192.168.0.1"), 24900);
        assert_eq!(merged.len(), 2, "発見と指定の同じ IP は 1 つに: {merged:?}");
        assert_eq!(merged[0].to_string(), "192.168.0.1:24900", "発見結果が先頭");
        assert_eq!(merged[1].to_string(), "100.100.10.9:24900");

        // 指定なし → 発見のみ
        let only = merge_candidates(vec!["192.168.0.1".parse().unwrap()], None, 24900);
        assert_eq!(only.len(), 1);
        // 発見なし → 指定のみ
        let fallback = merge_candidates(Vec::new(), Some("100.100.10.9"), 24900);
        assert_eq!(fallback.len(), 1);
        assert_eq!(fallback[0].to_string(), "100.100.10.9:24900");
        // 両方なし → 空(呼び出し側は再試行へ落ちる)
        assert!(merge_candidates(Vec::new(), None, 24900).is_empty());
    }

    /// 実ソケットでの結合確認: 待受・接続・認証・送信・受信・誤トークン拒否
    #[test]
    fn bulk_keepalive_confirms_server_liveness_over_noise() {
        use super::bulk::*;
        use std::io::Write;
        use std::time::{Duration, Instant};
        static LINK: Link = Link::new();
        static ENDPOINT: std::sync::OnceLock<Endpoint> = std::sync::OnceLock::new();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        let endpoint = ENDPOINT.get_or_init(|| Endpoint {
            link: &LINK, token: "bulk-heartbeat-test-key".into(),
            dir: std::env::temp_dir().join(format!("knit-heartbeat-{}", std::process::id())),
            on_event: |_| panic!("keepalive must not produce a file event"), log: |_| {},
            on_rx_bytes: |_| {},
        });
        std::thread::spawn(move || serve(endpoint, "127.0.0.1", address.port(), |_| true));
        let until = Instant::now() + Duration::from_secs(2);
        let socket = loop {
            match std::net::TcpStream::connect(address) {
                Ok(socket) => break socket,
                Err(_) if Instant::now() < until => std::thread::sleep(Duration::from_millis(10)),
                Err(error) => panic!("server did not start: {error}"),
            }
        };
        socket.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        let (mut reader, mut writer) = crate::secure::connect(socket, &endpoint.token, b"knit-bulk").unwrap();
        for _ in 0..2 {
            write_frame(&mut writer, KEEPALIVE, &[]).unwrap();
            writer.flush().unwrap();
            let mut payload = Vec::new();
            assert_eq!(read_frame(&mut reader, &mut payload).unwrap(), KEEPALIVE);
            assert!(payload.is_empty());
        }
        LINK.clear();
    }

    #[test]
    fn bulk_link_over_real_tcp() {
        use super::bulk::*;
        use std::sync::{Mutex, OnceLock};
        static SERVER_LINK: Link = Link::new();
        static CLIENT_LINK: Link = Link::new();
        static SERVER: OnceLock<Endpoint> = OnceLock::new();
        static CLIENT: OnceLock<Endpoint> = OnceLock::new();
        static GOT: Mutex<Vec<Vec<u8>>> = Mutex::new(Vec::new());
        let base = std::env::temp_dir().join(format!("knit-link-{}", std::process::id()));
        let port = {
            let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            l.local_addr().unwrap().port()
        };
        let server = SERVER.get_or_init(|| Endpoint {
            link: &SERVER_LINK,
            token: "secret".into(),
            dir: base.join("srv"),
            on_event: |e| {
                if let Event::Files { paths, .. } = e {
                    GOT.lock().unwrap().push(std::fs::read(&paths[0]).unwrap());
                }
            },
            log: |_| {},
            on_rx_bytes: |_| {},
        });
        let client = CLIENT.get_or_init(|| Endpoint {
            link: &CLIENT_LINK,
            token: "secret".into(),
            dir: base.join("cli"),
            on_event: |_| {},
            log: |_| {},
            on_rx_bytes: |_| {},
        });
        std::thread::spawn(move || serve(server, "127.0.0.1", port, |_| true));
        static ADDR: OnceLock<std::net::SocketAddr> = OnceLock::new();
        let addr = *ADDR.get_or_init(|| format!("127.0.0.1:{port}").parse().unwrap());
        std::thread::spawn(move || connect_loop(client, || ADDR.get().copied(), || true));
        let t0 = std::time::Instant::now();
        while !CLIENT_LINK.is_up() && t0.elapsed() < std::time::Duration::from_secs(10) {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert!(CLIENT_LINK.is_up(), "接続が張れない");
        std::fs::create_dir_all(&base).unwrap();
        let src = base.join("payload.bin");
        std::fs::write(&src, b"over-the-wire").unwrap();
        CLIENT_LINK
            .send(|w| send_files(w, std::slice::from_ref(&src), false))
            .unwrap();
        while GOT.lock().unwrap().is_empty() && t0.elapsed() < std::time::Duration::from_secs(10) {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert_eq!(
            GOT.lock().unwrap().first().map(|v| v.as_slice()),
            Some(&b"over-the-wire"[..])
        );

        // 誤トークンは拒否される
        let bad = std::net::TcpStream::connect(addr).unwrap();
        bad.set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        assert!(crate::secure::connect(bad, "wrong", b"knit-bulk").is_err());
        let _ = std::fs::remove_dir_all(base);
    }

    /// 送信が slot のロックを握ったまま書き込みに詰まっていても、clear は
    /// ロックを待たずに即座に戻り、切断が書き込み側へ伝わること(H5 の回帰)
    #[test]
    fn link_clear_cuts_a_blocked_send_without_waiting_for_the_slot_lock() {
        use super::bulk::Link;
        use crate::secure;
        use std::io::Write;
        use std::time::{Duration, Instant};

        static LINK: Link = Link::new();
        assert!(!LINK.is_up_fast(), "未接続の is_up_fast は偽");
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        // ハンドシェイクだけ完了させ、その後は読まない(書き込み側の
        // ソケットバッファを溢れさせて書き込みを詰まらせる)
        std::thread::spawn(move || {
            let (sock, _) = listener.accept().unwrap();
            let (_r, _w) = secure::accept(sock, "h5-test", b"knit-bulk").unwrap();
            std::thread::sleep(Duration::from_secs(30));
        });
        let socket = std::net::TcpStream::connect(addr).unwrap();
        socket.set_write_timeout(None).unwrap();
        let (_reader, writer) = secure::connect(socket, "h5-test", b"knit-bulk").unwrap();
        LINK.set(writer);
        assert!(LINK.is_up_fast(), "接続済みの is_up_fast は真");

        let sender = {
            // join に期限が無いと、shutdown がブロック中の書き込みを即座に失敗させない
            // 環境でテストが固まり得るため、channel + recv_timeout で待つ
            let (tx, rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let _ = tx.send(LINK.send(|w| {
                    let chunk = vec![0u8; 64 * 1024];
                    for _ in 0..4096 {
                        w.write_all(&chunk)?;
                        w.flush()?;
                    }
                    Ok(())
                }));
            });
            rx
        };
        // 書き込みがバッファ上限に達して詰まるのを待つ
        std::thread::sleep(Duration::from_millis(300));
        let t0 = Instant::now();
        LINK.clear();
        let elapsed = t0.elapsed();
        assert!(
            elapsed < Duration::from_secs(2),
            "clear が送信のロック待ちで止まった: {elapsed:?}"
        );
        let result = sender
            .recv_timeout(Duration::from_secs(10))
            .expect("切断後も送信スレッドが終わらない");
        assert!(result.is_err(), "切断後も書き込みが成功扱いになった");
    }

    #[test]
    fn connect_picks_a_reachable_candidate() {
        use super::connect::*;
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let good = l.local_addr().unwrap();
        // 閉じたポート(候補の 1 つ目)があっても、繋がる候補が選ばれる
        let dead = {
            let t = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            t.local_addr().unwrap()
        };
        let addrs = parse_hosts(
            &format!("127.0.0.1:{}, 127.0.0.1:{}", dead.port(), good.port()),
            1,
        );
        assert_eq!(addrs.len(), 2);
        let (_, picked) = first_reachable(&addrs, std::time::Duration::from_secs(2)).unwrap();
        assert_eq!(picked, good);
        assert_eq!(parse_hosts("127.0.0.1", 24900)[0].port(), 24900);
    }

    #[test]
    fn secure_channel_roundtrip_and_token_mismatch() {
        use super::secure::*;
        use std::io::{Read, Write};
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        let big: Vec<u8> = (0..200_000u32).map(|i| (i % 253) as u8).collect();
        let expect = big.clone();
        let srv = std::thread::spawn(move || {
            let (s, _) = l.accept().unwrap();
            let (mut r, mut w) = accept(s, "tok", b"test").unwrap();
            let mut got = vec![0u8; expect.len()];
            r.read_exact(&mut got).unwrap();
            assert_eq!(got, expect);
            w.write_all(b"pong").unwrap();
            w.flush().unwrap();
            // 誤トークンの接続は拒否される
            let (s2, _) = l.accept().unwrap();
            assert!(accept(s2, "tok", b"test").is_err());
        });
        let (mut r, mut w) =
            connect(std::net::TcpStream::connect(addr).unwrap(), "tok", b"test").unwrap();
        w.write_all(&big).unwrap(); // 64KB を超えて複数レコードに分かれる
        w.flush().unwrap();
        let mut p = [0u8; 4];
        r.read_exact(&mut p).unwrap();
        assert_eq!(&p, b"pong");
        assert!(connect(
            std::net::TcpStream::connect(addr).unwrap(),
            "wrong",
            b"test"
        )
        .is_err());
        srv.join().unwrap();
    }

    #[test]
    fn discovery_answers_only_the_same_room() {
        use super::discover::*;
        let port = {
            let s = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
            s.local_addr().unwrap().port()
        };
        std::thread::spawn(move || respond("127.0.0.1", port, "room-token", |_| true));
        std::thread::sleep(std::time::Duration::from_millis(100));
        let target: std::net::SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();
        let found = seek_first(target, "room-token", std::time::Duration::from_millis(500));
        assert_eq!(
            found,
            Some("127.0.0.1".parse::<std::net::IpAddr>().unwrap())
        );
        assert_eq!(
            seek_first(target, "other-token", std::time::Duration::from_millis(300)),
            None
        );
        assert_ne!(room_id("a"), room_id("b"));
        assert!(!room_id("secret").contains("secret"));
    }

    #[test]
    fn tailscale_range() {
        use super::net::{allowed, is_allowed, is_tailscale};
        assert!(is_tailscale("100.64.0.1".parse().unwrap()));
        assert!(is_tailscale("100.127.255.254".parse().unwrap()));
        assert!(!is_tailscale("100.128.0.1".parse().unwrap()));
        assert!(!is_tailscale("192.168.0.2".parse().unwrap()));
        assert!(is_allowed("169.254.10.2".parse().unwrap())); // 有線直結
        assert!(is_allowed("192.168.0.2".parse().unwrap()));
        assert!(is_allowed("fe80::1".parse().unwrap()));
        assert!(!is_allowed("8.8.8.8".parse().unwrap()));
        // Tailscale は既定で拒否・許可時(KNIT_ALLOW_TS=1)のみ通す。
        // 常時許可だと遠隔の Windows が勝手に再接続して境界に現れ、
        // マウスが消えたように見える事故の原因になる
        assert!(
            !allowed("100.84.0.2".parse().unwrap(), false),
            "既定では Tailscale を拒否"
        );
        assert!(
            allowed("100.84.0.2".parse().unwrap(), true),
            "許可時は通す"
        );
        // v4 mapped v6 も同じ判定に従う
        let mapped: std::net::IpAddr = "::ffff:100.84.0.2".parse().unwrap();
        assert!(!allowed(mapped, false));
        assert!(allowed(mapped, true));
    }

    #[test]
    fn version_negotiation_accepts_same_or_newer() {
        assert!(compatible(VERSION));
        assert!(compatible(VERSION + 5));
        assert!(!compatible(MIN_VERSION - 1));
    }

    #[test]
    fn unknown_message_is_ignored_not_fatal() {
        assert!(decode("{\"t\":\"future_feature\",\"x\":1}").is_none());
        assert!(matches!(decode(&encode(&Msg::Leave)), Some(Msg::Leave)));
        assert!(matches!(
            decode("{\"t\":\"return\"}"),
            Some(Msg::Return { .. })
        ));
    }

    #[test]
    fn apps_messages_round_trip_for_cross_pc_apps() {
        assert!(matches!(
            decode(&encode(&Msg::AppsQuery)),
            Some(Msg::AppsQuery)
        ));
        let reply = Msg::AppsReply {
            apps: vec![("メモ帳".into(), r"C:\Windows\notepad.exe".into())],
        };
        match decode(&encode(&reply)) {
            Some(Msg::AppsReply { apps }) => {
                assert_eq!(apps.len(), 1);
                assert_eq!(apps[0].0, "メモ帳");
                assert_eq!(apps[0].1, r"C:\Windows\notepad.exe");
            }
            other => panic!("AppsReply が復元できない: {other:?}"),
        }
        match decode(&encode(&Msg::RunApp {
            path: r"C:\x\y.lnk".into(),
        })) {
            Some(Msg::RunApp { path }) => assert_eq!(path, r"C:\x\y.lnk"),
            other => panic!("RunApp が復元できない: {other:?}"),
        }
    }
}

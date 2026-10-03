//! Disposable emulator only; fixed test key never used by the normal app.
use knit_common::{
    pairing,
    proto::{self, Msg},
    secure,
};
use std::{
    io::{BufRead, BufReader, Write},
    net::TcpListener,
    sync::{Arc, Mutex},
    time::Duration,
};
fn main() {
    let token = "c".repeat(64);
    let invitation = pairing::Invitation::open(token.clone(), 34900).unwrap();
    println!("TEST_CODE={}", invitation.code);
    std::thread::spawn(|| {
        let l = TcpListener::bind("127.0.0.1:34902").unwrap();
        for s in l.incoming().flatten() {
            std::thread::spawn(move || {
                let Ok((mut r, _w)) = secure::accept(s, &"c".repeat(64), b"knit-bulk") else {
                    return;
                };
                loop {
                    if knit_common::bulk::read_frame(&mut r, &mut Vec::new()).is_err() {
                        break;
                    }
                }
            });
        }
    });
    let listener = TcpListener::bind("127.0.0.1:34900").unwrap();
    for stream in listener.incoming().flatten() {
        let token = token.clone();
        std::thread::spawn(move || {
            let Ok((r, w)) = secure::accept(stream, &token, b"knit-main") else {
                return;
            };
            let w = Arc::new(Mutex::new(w));
            let mut r = BufReader::new(r);
            let mut line = String::new();
            if r.read_line(&mut line).is_err() {
                return;
            }
            if !matches!(proto::decode(&line), Some(Msg::Hello { .. })) {
                return;
            }
            {
                let mut writer = w.lock().unwrap();
                writer
                    .write_all(
                        proto::encode(&Msg::HelloOk {
                            name: "Knit test host".into(),
                            w: 1440,
                            h: 900,
                            ver: 13,
                            id: "test-mac".into(),
                            monitors: vec![],
                        })
                        .as_bytes(),
                    )
                    .unwrap();
                writer.flush().unwrap();
            }
            let heart = w.clone();
            std::thread::spawn(move || loop {
                std::thread::sleep(Duration::from_secs(3));
                let mut writer = heart.lock().unwrap();
                if writer
                    .write_all(proto::encode(&Msg::Ping { ts: 42 }).as_bytes())
                    .and_then(|_| writer.flush())
                    .is_err()
                {
                    break;
                }
            });
            loop {
                line.clear();
                if r.read_line(&mut line).is_err() {
                    break;
                }
                if let Some(Msg::Text { text }) = proto::decode(&line) {
                    let mut writer = w.lock().unwrap();
                    if writer
                        .write_all(proto::encode(&Msg::Clip { text }).as_bytes())
                        .and_then(|_| writer.flush())
                        .is_err()
                    {
                        break;
                    }
                }
            }
        });
    }
    drop(invitation);
}

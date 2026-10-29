use crate::{bulk_accept_keys, conn, extend_capture::Capture, vdisplay::VirtualDisplay};
use knit_common::{secure, video};
use std::io::Write;
use std::net::TcpListener;
use std::sync::atomic::Ordering;
use std::time::Duration;

pub fn start(port: u16) {
    if std::env::var("KNIT_EXTEND").as_deref() != Ok("1") {
        return;
    }
    let Some(video_port) = port.checked_add(video::PORT_OFFSET) else {
        eprintln!("[extend] 動画ポートが範囲外です");
        return;
    };
    std::thread::spawn(move || {
        if let Err(e) = run(video_port) {
            eprintln!("[extend] 拡張ディスプレイを無効にしました: {e}");
        }
    });
}
fn run(port: u16) -> Result<(), Box<dyn std::error::Error>> {
    let display = VirtualDisplay::create(1920, 1080, 60)?;
    let bind = knit_common::envutil::get("KNIT_BIND").unwrap_or_else(|| "0.0.0.0".into());
    let listener = TcpListener::bind((bind.as_str(), port))?;
    eprintln!(
        "[extend] 画面 {}、動画ポート {port} で待機中",
        display.display_id
    );
    let mut throttle = secure::FailThrottle::new();
    for incoming in listener.incoming() {
        let stream = match incoming {
            Ok(s) => s,
            Err(e) => {
                eprintln!("[extend] 接続の受理に失敗: {e}");
                continue;
            }
        };
        let ip = match stream.peer_addr() {
            Ok(a) => a.ip(),
            Err(_) => continue,
        };
        if !conn::allow_bulk_peer(ip) || !crate::CONNECTED.load(Ordering::Relaxed) {
            continue;
        }
        let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
        let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
        let keys = match bulk_accept_keys() {
            Ok(k) => k,
            Err(e) => {
                eprintln!("[extend] 認証鍵を取得できません: {e}");
                continue;
            }
        };
        let (_, mut writer, who) = match secure::accept_as(stream, &keys, b"knit-video") {
            Ok(result) => result,
            Err(e) => {
                eprintln!("[extend] 動画接続の認証に失敗: {e}");
                std::thread::sleep(throttle.fail());
                continue;
            }
        };
        if conn::active_peer_auth() != Some(who.clone()) || !conn::allow_bulk_peer(ip) {
            continue;
        }
        throttle.success();
        let (capture, rx) = match Capture::start(display.display_id, 1920, 1080, 60, 12_000_000) {
            Ok(result) => result,
            Err(e) => {
                eprintln!("[extend] {e}");
                return Err(e.into());
            }
        };
        let config = video::Config {
            w: 1920,
            h: 1080,
            fps: 60,
            codec: "h264".into(),
        };
        let payload = serde_json::to_vec(&config)?;
        if video::write_frame(&mut writer, video::CONFIG, 0, &payload)
            .and_then(|_| writer.flush())
            .is_err()
        {
            drop(capture);
            continue;
        }
        loop {
            if !crate::CONNECTED.load(Ordering::Relaxed)
                || !conn::allow_bulk_peer(ip)
                || conn::active_peer_auth() != Some(who.clone())
            {
                break;
            }
            match rx.recv_timeout(Duration::from_millis(250)) {
                Ok(frame) => {
                    let kind = if frame.key {
                        video::KEYFRAME
                    } else {
                        video::DELTA
                    };
                    if video::write_frame(&mut writer, kind, frame.timestamp, &frame.bytes)
                        .and_then(|_| writer.flush())
                        .is_err()
                    {
                        break;
                    }
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
                Err(_) => break,
            }
        }
        drop(capture);
    }
    Ok(())
}

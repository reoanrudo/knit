#[path = "../extend_capture.rs"]
mod extend_capture;
#[path = "../vdisplay.rs"]
mod vdisplay;
use std::{
    io::Write,
    time::{Duration, Instant},
};
fn run() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "extend.h264".into());
    let mut args = std::env::args().skip(2);
    let width = args.next().map(|v| v.parse()).transpose()?.unwrap_or(1920);
    let height = args.next().map(|v| v.parse()).transpose()?.unwrap_or(1080);
    let fps = args.next().map(|v| v.parse()).transpose()?.unwrap_or(60);
    let bitrate = args
        .next()
        .map(|v| v.parse())
        .transpose()?
        .unwrap_or(12_000_000);
    let display = vdisplay::VirtualDisplay::create(width, height, fps)?;
    let (capture, rx) =
        extend_capture::Capture::start(display.display_id, width, height, fps, bitrate)?;
    let mut file = std::fs::File::create(&path)?;
    let until = Instant::now() + Duration::from_secs(5);
    let mut frames = 0;
    while Instant::now() < until {
        if let Ok(frame) = rx.recv_timeout(Duration::from_millis(100)) {
            let _ = (frame.timestamp, frame.key);
            file.write_all(&frame.bytes)?;
            frames += 1;
        }
    }
    drop(capture);
    file.flush()?;
    if frames == 0 {
        return Err("フレームを取得できませんでした".into());
    }
    println!("{path} に {frames} フレームを書き出しました");
    match std::process::Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-count_frames",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=width,height,nb_read_frames",
            "-of",
            "default=noprint_wrappers=1",
            &path,
        ])
        .status()
    {
        Ok(status) if status.success() => (),
        Ok(status) => eprintln!("ffprobe が失敗しました: {status}"),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            eprintln!("ffprobe が無いため解析を省略します")
        }
        Err(e) => eprintln!("ffprobe を起動できません: {e}"),
    }
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("[extend] {e}");
        std::process::exit(1);
    }
}

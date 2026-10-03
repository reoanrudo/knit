//! 音声設定はユーザープロファイルへ保存し、配布先フォルダの書込み権限に依存させない。
use super::*;
fn path() -> std::io::Result<std::path::PathBuf> {
    let dir = knit_common::envutil::data_dir()
        .ok_or_else(|| std::io::Error::other("LOCALAPPDATA is unavailable"))?;
    Ok(dir.join("preferences.json"))
}
pub(super) fn restore() {
    let Ok(path) = path() else { return };
    let Ok(bytes) = std::fs::read(path) else {
        return;
    };
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        return;
    };
    if let Some(enabled) = value["audio_enabled"].as_bool() {
        crate::audio::AUDIO_ENABLED.store(enabled, Ordering::Relaxed);
    }
    if let Some(on) = value["share_clip"].as_bool() {
        knit_common::share::set_user_clip(on);
    }
    if let Some(on) = value["share_files"].as_bool() {
        knit_common::share::set_user_files(on);
    }
    if let Some(on) = value["host_mode"].as_bool() {
        HOST_MODE.store(on, Ordering::Relaxed);
    }
}

/// このPCをホスト(待受側)にする設定。KNIT_ROLE=server の GUI 版。
/// 接続方向は起動時に決まるため、再起動後に反映される
pub(crate) static HOST_MODE: AtomicBool = AtomicBool::new(false);

/// 起動時のロール判定用(preferences::restore より前でも読める直接読み)
pub(crate) fn host_mode_pref() -> bool {
    path().ok()
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
        .and_then(|v| v["host_mode"].as_bool())
        .unwrap_or_else(|| HOST_MODE.load(Ordering::Relaxed))
}

/// 役割を直接決める(相手の切替に合わせる時)
pub(super) fn set_host_mode(on: bool) {
    HOST_MODE.store(on, Ordering::Relaxed);
}

pub(super) fn save() -> std::io::Result<()> {
    use std::io::Write;
    let path = path()?;
    std::fs::create_dir_all(path.parent().unwrap())?;
    let tmp = path.with_extension(format!("{}.tmp", std::process::id()));
    let mut f = std::fs::File::create(&tmp)?;
    f.write_all(serde_json::to_string_pretty(&serde_json::json!({"version":1,"audio_enabled":crate::audio::AUDIO_ENABLED.load(Ordering::Relaxed),"share_clip":knit_common::share::user_clip(),"share_files":knit_common::share::user_files(),"host_mode":HOST_MODE.load(Ordering::Relaxed)}))?.as_bytes())?;
    f.sync_all()?;
    drop(f);
    #[link(name = "kernel32")]
    extern "system" {
        fn MoveFileExW(from: *const u16, to: *const u16, flags: u32) -> i32;
    }
    if unsafe {
        MoveFileExW(
            wide(&tmp.to_string_lossy()).as_ptr(),
            wide(&path.to_string_lossy()).as_ptr(),
            1 | 8,
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

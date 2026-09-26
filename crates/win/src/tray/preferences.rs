//! 音声設定はユーザープロファイルへ保存し、配布先フォルダの書込み権限に依存させない。
use super::*;
fn path() -> std::io::Result<std::path::PathBuf> {
    let root = std::env::var_os("LOCALAPPDATA")
        .ok_or_else(|| std::io::Error::other("LOCALAPPDATA is unavailable"))?;
    Ok(std::path::PathBuf::from(root).join("Tsunagu/preferences.json"))
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
}
pub(super) fn save() -> std::io::Result<()> {
    use std::io::Write;
    let path = path()?;
    std::fs::create_dir_all(path.parent().unwrap())?;
    let tmp = path.with_extension(format!("{}.tmp", std::process::id()));
    let mut f = std::fs::File::create(&tmp)?;
    f.write_all(serde_json::to_string_pretty(&serde_json::json!({"version":1,"audio_enabled":crate::audio::AUDIO_ENABLED.load(Ordering::Relaxed)}))?.as_bytes())?;
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

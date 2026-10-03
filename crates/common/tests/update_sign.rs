#![cfg(unix)] // knit-sign の鍵生成は /dev/urandom を使う(署名はリリース担当の Mac/CI で行う)
//! knit-sign が作った更新情報と署名を、検証コアが受理することを確かめる。
use knit_common::update;
use std::process::Command;

fn tool(args: &[&str]) -> (bool, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_knit-sign"))
        .args(args)
        .output()
        .unwrap();
    (
        out.status.success(),
        String::from_utf8(out.stdout).unwrap().trim().to_string(),
    )
}

#[test]
fn tool_output_verifies_and_tampering_is_rejected() {
    let dir = std::env::temp_dir().join(format!("knit-sign-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let key = dir.join("key");
    let zip = dir.join("Knit.zip");
    let json = dir.join("update.json");
    std::fs::write(&zip, b"release bytes").unwrap();

    let (ok, public) = tool(&["keygen", key.to_str().unwrap()]);
    assert!(ok);
    let public: [u8; 32] = update::from_hex(&public).unwrap().try_into().unwrap();
    // 既存の鍵は上書きしない
    assert!(!tool(&["keygen", key.to_str().unwrap()]).0);

    let spec = format!("macos-arm64={}=https://example.com/Knit.zip", zip.display());
    let (ok, manifest) = tool(&["manifest", "0.27.0", "stable", &spec]);
    assert!(ok);
    std::fs::write(&json, &manifest).unwrap();
    let (ok, sig) = tool(&["sign", key.to_str().unwrap(), json.to_str().unwrap()]);
    assert!(ok);

    let parsed = update::verify_manifest(manifest.as_bytes(), &sig, &[public]).unwrap();
    let a = update::select(&parsed, "stable", "0.26.0", "macos-arm64")
        .unwrap()
        .unwrap();
    update::verify_artifact(&b"release bytes"[..], a).unwrap();
    assert!(update::verify_artifact(&b"release bytez"[..], a).is_err());
    assert!(update::verify_manifest(
        manifest.replace("0.27.0", "9.9.9").as_bytes(),
        &sig,
        &[public]
    )
    .is_err());
    // http の成果物と不正な版は生成時点で拒否する
    assert!(!tool(&["manifest", "0.27.0", "stable", &spec.replace("https", "http")]).0);
    assert!(!tool(&["manifest", "0.27", "stable", &spec]).0);
    std::fs::remove_dir_all(dir).ok();
}

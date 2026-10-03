//! KNIT_SHARE が送信側で実際に効くことを、環境変数を変えた別プロセスで確かめる。
//! (共有範囲は起動時に1度だけ読むため、同じプロセスでは切り替えられない)
use knit_common::bulk;
use std::process::Command;

fn probe(spec: Option<&str>) -> (bool, String) {
    let mut c = Command::new(std::env::current_exe().unwrap());
    c.args(["--exact", "child_probe", "--nocapture", "--test-threads=1"])
        .env("KNIT_SHARE_PROBE", "1")
        .env_remove("KNIT_SHARE");
    if let Some(s) = spec {
        c.env("KNIT_SHARE", s);
    }
    let out = c.output().unwrap();
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).to_string(),
    )
}

/// 子プロセスでだけ動く。出力の1行目に「clip=… files=…」を出す
#[test]
fn child_probe() {
    if std::env::var_os("KNIT_SHARE_PROBE").is_none() {
        return;
    }
    let mut sink = Vec::new();
    let clip = bulk::send_image(&mut sink, &[0u8; 64]).is_ok();
    let dir = std::env::temp_dir().join(format!("knit-share-probe-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("a.txt");
    std::fs::write(&f, "x").unwrap();
    let files = bulk::send_files(&mut Vec::new(), &[f], false).is_ok();
    std::fs::remove_dir_all(&dir).ok();
    println!("RESULT clip={clip} files={files} audio={}", knit_common::share::allow_audio());
}

fn result(spec: Option<&str>) -> String {
    let (ok, out) = probe(spec);
    assert!(ok, "child failed: {out}");
    out.lines()
        .find_map(|l| l.split_once("RESULT ").map(|(_, r)| r.trim().to_string()))
        .unwrap_or_else(|| panic!("no RESULT in: {out}"))
}

#[test]
fn share_scope_is_enforced_by_the_environment() {
    assert_eq!(result(None), "clip=true files=true audio=true");
    assert_eq!(result(Some("all")), "clip=true files=true audio=true");
    assert_eq!(result(Some("input")), "clip=false files=false audio=false");
    assert_eq!(result(Some("clipboard")), "clip=true files=false audio=false");
    assert_eq!(result(Some("files,audio")), "clip=false files=true audio=true");
    // 綴り間違いは安全側(入力のみ)
    assert_eq!(result(Some("clipbord")), "clip=false files=false audio=false");
}

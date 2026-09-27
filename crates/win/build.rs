// exe へアプリアイコンを埋め込む(windres がある場合のみ。失敗時は静かに省略)
fn main() {
    let Ok(target) = std::env::var("TARGET") else {
        return;
    };
    if !target.contains("windows") {
        return;
    }
    let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default();
    let ico = std::path::Path::new(&manifest).join("../../win-dist/app.ico");
    let Ok(ico) = ico.canonicalize() else { return };
    println!("cargo:rerun-if-changed={}", ico.display());
    let out_dir = std::env::var("OUT_DIR").unwrap_or_default();
    let rc = std::path::Path::new(&out_dir).join("icon.rc");
    let obj = std::path::Path::new(&out_dir).join("icon.o");
    let _ = std::fs::write(&rc, format!("1 ICON \"{}\"\n", ico.display()));
    let ok = std::process::Command::new("x86_64-w64-mingw32-windres")
        .arg(&rc)
        .args(["-O", "coff"])
        .arg("-o")
        .arg(&obj)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if ok {
        println!("cargo:rustc-link-arg={}", obj.display());
    }
}

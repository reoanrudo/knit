use std::{env, path::PathBuf, process::Command};

fn main() {
    println!("cargo:rerun-if-changed=src/gui/direct_input.m");
    println!("cargo:rerun-if-changed=src/extend_capture.m");
    println!("cargo:rerun-if-changed=src/gui/webui.m");
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let arch = match env::var("CARGO_CFG_TARGET_ARCH").unwrap().as_str() {
        "aarch64" => "arm64",
        "x86_64" => "x86_64",
        _ => panic!("unsupported macOS architecture"),
    };
    assert!(Command::new("xcrun")
        .args([
            "clang",
            "-arch",
            arch,
            "-mmacosx-version-min=12.0",
            "-fobjc-arc",
            "-Wall",
            "-Wextra",
            "-Werror",
            "-c",
            "src/gui/direct_input.m",
            "-o"
        ])
        .arg(out.join("direct_input.o"))
        .status()
        .unwrap()
        .success());
    assert!(Command::new("xcrun")
        .args([
            "clang",
            "-arch",
            arch,
            "-mmacosx-version-min=12.3",
            "-fobjc-arc",
            "-Wall",
            "-Wextra",
            "-Werror",
            "-c",
            "src/extend_capture.m",
            "-o"
        ])
        .arg(out.join("extend_capture.o"))
        .status()
        .unwrap()
        .success());
    // 設定画面(WKWebView)の窓と合図の受け渡し
    assert!(Command::new("xcrun")
        .args([
            "clang",
            "-arch",
            arch,
            "-mmacosx-version-min=12.0",
            "-fobjc-arc",
            "-Wall",
            "-Wextra",
            "-Werror",
            "-c",
            "src/gui/webui.m",
            "-o"
        ])
        .arg(out.join("webui.o"))
        .status()
        .unwrap()
        .success());
    assert!(Command::new("ar")
        .arg("crs")
        .arg(out.join("libknit_direct_input.a"))
        .arg(out.join("direct_input.o"))
        .arg(out.join("extend_capture.o"))
        .arg(out.join("webui.o"))
        .status()
        .unwrap()
        .success());
    println!("cargo:rustc-link-search=native={}", out.display());
    println!("cargo:rustc-link-lib=static=knit_direct_input");
    println!("cargo:rustc-link-arg=-Wl,-weak_framework,ScreenCaptureKit");
    println!("cargo:rustc-link-lib=framework=WebKit");
}

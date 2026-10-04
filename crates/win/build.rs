// exe へアプリアイコンと DPI アウェアネス宣言を埋め込む(windres がある場合のみ。
// 失敗時は静かに省略)
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
    // DPI アウェアネス(Per-Monitor-V2)をマニフェストで宣言する。宣言が無いと
    // OS は DPI 仮想化(DPI 非対応扱い)を強制し、125%/150% スケーリングや
    // 混在 DPI の多モニターで注入座標(get_cursor_pos / set_cursor_pos /
    // MOUSEEVENTF_ABSOLUTE)と実画面の座標系がズレる。
    // dpiAware(true/pm)=旧ローダー向け、dpiAwareness(PerMonitorV2, system)=
    // 新ローダー向けの二段構え(定石)。マニフェスト無しで
    // SetProcessDpiAwarenessContext を呼ぶ方法もあるが、マニフェストは
    // プロセス起動直後から効き、API 呼び漏れが無い
    let app_manifest = std::path::Path::new(&out_dir).join("app.manifest");
    let manifest_xml = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <assemblyIdentity version="1.0.0.0" processorArchitecture="*" name="Knit" type="win32"/>
  <application xmlns="urn:schemas-microsoft-com:asm.v3">
    <windowsSettings>
      <dpiAware xmlns="http://schemas.microsoft.com/SMI/2005/WindowsSettings">true/pm</dpiAware>
      <dpiAwareness xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">PerMonitorV2, system</dpiAware>
    </windowsSettings>
  </application>
</assembly>
"#;
    if std::fs::write(&app_manifest, manifest_xml).is_err() {
        eprintln!("cargo:warning=app.manifest を書き込めませんでした");
    }
    // RT_MANIFEST(リソース種別 24)の ID 1 = 実行可能ファイルのマニフェスト
    let _ = std::fs::write(
        &rc,
        format!(
            "1 ICON \"{}\"\n1 24 \"{}\"\n",
            ico.display(),
            app_manifest.display()
        ),
    );
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
    } else {
        eprintln!("cargo:warning=windres が失敗したためアイコン・マニフェストを埋め込みません");
    }
}

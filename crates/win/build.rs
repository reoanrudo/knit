// exe へアプリアイコンを埋め込む(windres がある場合のみ。失敗時は静かに省略)。
//
// DPI アウェアネスは 2026-10-12 にマニフェスト埋め込みから実行時 API
// (main.rs の set_dpi_awareness)へ移した。経緯: Rust/mingw 側の更新後、
// windres で埋め込んだ RT_MANIFEST を Windows ローダーが
// 「マニフェスト 行 7 のエラー。無効な XML 構文です」と判定してプロセスが
// 起動しなくなった(イベントログ SideBySide Event ID 59。埋め込みデータ自体は
// icon.o と一致し XML も正当で、リンク後の解釈だけが壊れる)。
// マニフェストを外すと起動することを実機で確認したため、宣言は API 呼び出しへ切替。
// 起動直後(main の先頭)に呼ぶため、ウィンドウ作成前に DPI 設定が効く点は変わらない。
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
    // RT_ICON(3)/RT_GROUP_ICON(14)だけを埋め込む(マニフェストは埋め込まない)
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
    } else {
        eprintln!("cargo:warning=windres が失敗したためアイコンを埋め込みません");
    }
}

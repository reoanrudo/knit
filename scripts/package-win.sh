#!/bin/bash
# 公開用の配布物を作成する。利用者固有の設定・認証情報は同梱しない。
set -euo pipefail
cd "$(dirname "$0")/.."
if [ -f "$HOME/.cargo/env" ]; then source "$HOME/.cargo/env"; fi
VER=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
[ -n "$VER" ] || { echo '[package-win] version が取得できません' >&2; exit 1; }
mkdir -p dist
STAGE=$(mktemp -d dist/.package-win.XXXXXX)
trap 'rm -rf "$STAGE"' EXIT
OUT="$STAGE/Knit-win-$VER"
echo '[package-win] building...'
cargo build --locked --release -p knit-win --target x86_64-pc-windows-gnu
mkdir -p "$OUT"
cp target/x86_64-pc-windows-gnu/release/knit-win.exe "$OUT/"
cp win-dist/install.bat win-dist/uninstall.bat win-dist/run_knit.bat win-dist/run_knit.vbs win-dist/README-win.txt win-dist/app.ico "$OUT/"
cp LICENSE "$OUT/LICENSE.txt"
python3 - "$OUT" "$VER" <<'PY'
import datetime, hashlib, json, pathlib, subprocess, sys
out=pathlib.Path(sys.argv[1]); binary=out/'knit-win.exe'
data=dict(schema_version=1, version=sys.argv[2], platform='windows-x64', source_commit=subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip(), working_tree_modified=bool(subprocess.check_output(['git','status','--porcelain'],text=True)), built_at=datetime.datetime.now(datetime.timezone.utc).isoformat(), binary_sha256=hashlib.sha256(binary.read_bytes()).hexdigest(), distribution='development-candidate')
(out/'release-manifest.json').write_text(json.dumps(data,ensure_ascii=False,indent=2)+'\n')
PY
# --norsrc: Mac の拡張属性(._xxx の AppleDouble)を入れない。Windows 向け配布物に
# リソースフォークは無意味で、展開先にゴミファイルが残るのを防ぐ
ditto -c -k --keepParent --norsrc "$OUT" "$STAGE/Knit-win-$VER.zip"
python3 scripts/check-release.py "$STAGE/Knit-win-$VER.zip"
mv "$STAGE/Knit-win-$VER.zip" "dist/Knit-win-$VER.zip"
(cd dist && shasum -a 256 "Knit-win-$VER.zip" > "Knit-win-$VER.zip.sha256")
echo "[package-win] 完了: dist/Knit-win-$VER.zip (設定・トークン非同梱、開発候補版)"

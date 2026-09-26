#!/bin/bash
# 公開用の配布物を作成する。利用者固有の設定・認証情報は同梱しない。
set -euo pipefail
cd "$(dirname "$0")/.."
if [ -f "$HOME/.cargo/env" ]; then source "$HOME/.cargo/env"; fi
VER=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
[ -n "$VER" ] || { echo '[package-mac] version が取得できません' >&2; exit 1; }
mkdir -p dist
STAGE=$(mktemp -d dist/.package-mac.XXXXXX)
trap 'rm -rf "$STAGE"' EXIT
PACKAGE="$STAGE/Tsunagu-$VER"
APP="$PACKAGE/Tsunagu.app"
echo '[package-mac] building...'
cargo build --locked --release -p tsunagu-mac --bin tsunagu-mac
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/tsunagu-mac "$APP/Contents/MacOS/Tsunagu"
cp assets/AppIcon.icns "$APP/Contents/Resources/AppIcon.icns"
cp LICENSE "$APP/Contents/Resources/LICENSE.txt"
lipo "$APP/Contents/MacOS/Tsunagu" -verify_arch arm64
python3 - "$APP" "$VER" <<'PY'
import pathlib, plistlib, sys
app = pathlib.Path(sys.argv[1]); version = sys.argv[2]
with (app / 'Contents/Info.plist').open('wb') as f:
    plistlib.dump(dict(CFBundleName='Tsunagu', CFBundleDisplayName='Tsunagu', CFBundleIdentifier='local.tsunagu', CFBundleExecutable='Tsunagu', CFBundlePackageType='APPL', CFBundleShortVersionString=version, CFBundleVersion=version, CFBundleIconFile='AppIcon', LSUIElement=True, NSHighResolutionCapable=True, NSSupportsAutomaticTermination=False, NSSupportsSuddenTermination=False), f)
PY
# アドホック署名の失敗も配布失敗として扱う。公証済み製品の署名とは異なる。
codesign --force --sign - "$APP"
codesign --verify --deep --strict "$APP"
# 署名後のハッシュを記録する。マニフェストは.appの外へ置き、署名対象を変更しない。
python3 - "$APP" "$VER" <<'PY'
import datetime, hashlib, json, pathlib, subprocess, sys
app=pathlib.Path(sys.argv[1]); binary=app/'Contents/MacOS/Tsunagu'
data=dict(schema_version=1, version=sys.argv[2], platform='macos-arm64', source_commit=subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip(), working_tree_modified=bool(subprocess.check_output(['git','status','--porcelain'],text=True)), built_at=datetime.datetime.now(datetime.timezone.utc).isoformat(), binary_sha256=hashlib.sha256(binary.read_bytes()).hexdigest(), distribution='development-candidate')
(app.parent/'release-manifest.json').write_text(json.dumps(data,ensure_ascii=False,indent=2)+'\n')
PY
codesign --verify --deep --strict "$APP"
ditto -c -k --keepParent "$PACKAGE" "$STAGE/Tsunagu-$VER.zip"
python3 scripts/check-release.py "$STAGE/Tsunagu-$VER.zip"
# 検査がすべて通るまで、既存の配布物には触れない。
rm -rf dist/Tsunagu.app
mv "$APP" dist/Tsunagu.app
mv "$STAGE/Tsunagu-$VER.zip" "dist/Tsunagu-$VER.zip"
(cd dist && shasum -a 256 "Tsunagu-$VER.zip" > "Tsunagu-$VER.zip.sha256")
echo "[package-mac] 完了: dist/Tsunagu-$VER.zip (設定・トークン非同梱、開発候補版)"

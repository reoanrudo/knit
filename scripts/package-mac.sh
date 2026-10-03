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
PACKAGE="$STAGE/Knit-$VER"
APP="$PACKAGE/Knit.app"
echo '[package-mac] building...'
cargo build --locked --release -p knit-mac --bin knit-mac
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/knit-mac "$APP/Contents/MacOS/Knit"
cp assets/AppIcon.icns "$APP/Contents/Resources/AppIcon.icns"
cp LICENSE "$APP/Contents/Resources/LICENSE.txt"
cp mac-dist/README-Mac.txt "$PACKAGE/README-Mac.txt"
cp LICENSE "$PACKAGE/LICENSE.txt"
lipo "$APP/Contents/MacOS/Knit" -verify_arch arm64
python3 - "$APP" "$VER" <<'PY'
import pathlib, plistlib, sys
app = pathlib.Path(sys.argv[1]); version = sys.argv[2]
with (app / 'Contents/Info.plist').open('wb') as f:
    plistlib.dump(dict(CFBundleName='Knit', CFBundleDisplayName='Knit', CFBundleIdentifier='local.knit', CFBundleExecutable='Knit', CFBundlePackageType='APPL', CFBundleShortVersionString=version, CFBundleVersion=version, CFBundleIconFile='AppIcon', LSUIElement=True, NSHighResolutionCapable=True, NSSupportsAutomaticTermination=False, NSSupportsSuddenTermination=False), f)
PY
# 署名: KNIT_SIGN_IDENTITY(または KNIT_SIGN_IDENTITY 未設定でも、
# キーチェーンに Apple Development / Developer ID 証明書があれば自動で使う)。
# 安定した署名者で署名すると、アプリを更新しても macOS のアクセシビリティ
# 許可が維持される(アドホック署名は毎回別物扱いになり再許可が必要だった)。
# 公証(KNIT_NOTARY_PROFILE)は Developer ID 署名時にだけ提出する
SIGN_IDENTITY="${KNIT_SIGN_IDENTITY:-}"
if [ -z "$SIGN_IDENTITY" ]; then
    SIGN_IDENTITY=$(security find-identity -v -p codesigning 2>/dev/null \
        | sed -n 's/.*"\(Apple Development: [^"]*\)".*/\1/p' | head -1)
fi
if [ -z "$SIGN_IDENTITY" ]; then
    SIGN_IDENTITY=$(security find-identity -v -p codesigning 2>/dev/null \
        | sed -n 's/.*"\(Developer ID Application: [^"]*\)".*/\1/p' | head -1)
fi
NOTARY_PROFILE="${KNIT_NOTARY_PROFILE:-}"
if [ -n "$SIGN_IDENTITY" ]; then
    echo "[package-mac] コード署名: $SIGN_IDENTITY"
    codesign --force --options runtime --sign "$SIGN_IDENTITY" "$APP"
else
    echo "[package-mac] 署名証明書がありません(アドホック署名。更新のたびに権限の再許可が必要)"
    codesign --force --sign - "$APP"
fi
codesign --verify --deep --strict "$APP"
if [ -n "$SIGN_IDENTITY" ] && [ -n "$NOTARY_PROFILE" ]; then
    echo "[package-mac] 公証を提出します(承認まで数分かかります)..."
    ditto -c -k --keepParent "$APP" "$STAGE/notary-submit.zip"
    if ! xcrun notarytool submit "$STAGE/notary-submit.zip" --keychain-profile "$NOTARY_PROFILE" --wait; then
        echo '[package-mac] 公証が承認されませんでした。配布を中止します' >&2
        exit 1
    fi
    xcrun notarytool staple "$APP"
    xcrun stapler validate "$APP"
    codesign --verify --deep --strict "$APP"
fi
# 署名後のハッシュを記録する。マニフェストは.appの外へ置き、署名対象を変更しない。
python3 - "$APP" "$VER" <<'PY'
import datetime, hashlib, json, pathlib, subprocess, sys
app=pathlib.Path(sys.argv[1]); binary=app/'Contents/MacOS/Knit'
data=dict(schema_version=1, version=sys.argv[2], platform='macos-arm64', source_commit=subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip(), working_tree_modified=bool(subprocess.check_output(['git','status','--porcelain'],text=True)), built_at=datetime.datetime.now(datetime.timezone.utc).isoformat(), binary_sha256=hashlib.sha256(binary.read_bytes()).hexdigest(), distribution='development-candidate')
(app.parent/'release-manifest.json').write_text(json.dumps(data,ensure_ascii=False,indent=2)+'\n')
PY
codesign --verify --deep --strict "$APP"
ditto -c -k --keepParent "$PACKAGE" "$STAGE/Knit-$VER.zip"
python3 scripts/check-release.py "$STAGE/Knit-$VER.zip"
# 検査がすべて通るまで、既存の配布物には触れない。
rm -rf dist/Knit.app
mv "$APP" dist/Knit.app
mv "$STAGE/Knit-$VER.zip" "dist/Knit-$VER.zip"
(cd dist && shasum -a 256 "Knit-$VER.zip" > "Knit-$VER.zip.sha256")
echo "[package-mac] 完了: dist/Knit-$VER.zip (設定・トークン非同梱、開発候補版)"

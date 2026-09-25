#!/bin/bash
# 配布用 .app バンドルと zip を dist/ に作成する(配布の単位)。
# トークン設定(~/.config/seamless-desk/env)があればバンドル内の .env へ封入する
set -e
cd "$(dirname "$0")/.."
source $HOME/.cargo/env 2>/dev/null || true

VER=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
APP="SeamlessDesk.app"
ROOT="dist"

echo "[package-mac] BUILD_ID スタンプ..."
NEW_ID="build-$(date +%Y%m%d-%H%M%S)-$(git rev-parse --short HEAD)"
sed -i '' "s|const BUILD_ID:[^;]*;|const BUILD_ID: \&str = \"$NEW_ID\";|" crates/mac/src/main.rs

echo "[package-mac] building..."
cargo build --release 2>&1 | grep -E "^error" -A 3 && exit 1 || true
cargo build --release 2>&1 | tail -1 >/dev/null

rm -rf "$ROOT/$APP"
mkdir -p "$ROOT/$APP/Contents/MacOS"
cp target/release/sd-mac "$ROOT/$APP/Contents/MacOS/SeamlessDesk"

cat > "$ROOT/$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>SeamlessDesk</string>
  <key>CFBundleDisplayName</key><string>Seamless Desk</string>
  <key>CFBundleIdentifier</key><string>local.seamless-desk</string>
  <key>CFBundleExecutable</key><string>SeamlessDesk</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$VER</string>
  <key>CFBundleVersion</key><string>$NEW_ID</string>
  <key>LSUIElement</key><true/>
  <key>NSSupportsAutomaticTermination</key><false/>
  <key>NSSupportsSuddenTermination</key><false/>
</dict>
</plist>
PLIST

# トークン設定をバンドル内へ封入(配布先で最初から接続可能にする。
# 不要な場合はこのブロックを削除し、配布先で ~/.config/seamless-desk/env を設定する)
if [ -f "$HOME/.config/seamless-desk/env" ]; then
  mkdir -p "$ROOT/$APP/Contents/Resources"
  cp "$HOME/.config/seamless-desk/env" "$ROOT/$APP/Contents/Resources/.env"
  chmod 600 "$ROOT/$APP/Contents/Resources/.env"
  echo "[package-mac] トークン設定を Resources/.env に封入しました"
else
  echo "[package-mac] WARN: トークン未設定のため .env は封入されない(初回起動が fatal で停止する)"
fi

codesign --force --sign - "$ROOT/$APP" >/dev/null 2>&1 || echo "[package-mac] WARN: codesign 失敗(未署名で継続)"

rm -f "$ROOT/SeamlessDesk-$VER.zip"
ditto -c -k --keepParent "$ROOT/$APP" "$ROOT/SeamlessDesk-$VER.zip"
echo "[package-mac] 完了: $ROOT/$APP / $ROOT/SeamlessDesk-$VER.zip (v$VER, $NEW_ID)"

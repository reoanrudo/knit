#!/bin/bash
# このMacへ .app としてインストールする(ログイン時自動起動の LaunchAgent 登録付き)。
# 前提: scripts/package-mac.sh を先に実行して dist/SeamlessDesk.app を作成しておく
set -e
cd "$(dirname "$0")/.."
APP_SRC="dist/SeamlessDesk.app"
if [ ! -d "$APP_SRC" ]; then
  echo "[install-mac] $APP_SRC がありません。scripts/package-mac.sh を先に実行してください" >&2
  exit 1
fi

# 開発運用(restart-mac.sh)と併用しない(二重起動になるため先に止める)
pkill -9 -f "target/release/sd-mac" 2>/dev/null || true
pkill -9 -f "SeamlessDesk.app/Contents/MacOS/SeamlessDesk" 2>/dev/null || true
sleep 1

rm -rf "$HOME/Applications/SeamlessDesk.app"
cp -R "$APP_SRC" "$HOME/Applications/SeamlessDesk.app"
BIN="$HOME/Applications/SeamlessDesk.app/Contents/MacOS/SeamlessDesk"
chmod +x "$BIN"

# ログは開発と同じ /tmp/sd-mac-run.log へ(verify.sh 互換)。
# SEAMLESS_BIND は Tailscale IF に限定(失敗時は 0.0.0.0 で起動しピア判定で防御)
BIND_IP=$(tailscale ip -4 2>/dev/null | head -1)
[ -z "$BIND_IP" ] && BIND_IP="0.0.0.0"
PLIST="$HOME/Library/LaunchAgents/local.seamless-desk.plist"
cat > "$PLIST" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>local.seamless-desk</string>
  <key>ProgramArguments</key>
  <array>
    <string>$BIN</string>
  </array>
  <key>RunAtLoad</key><true/>
  <key>StandardOutPath</key><string>/tmp/sd-mac-run.log</string>
  <key>StandardErrorPath</key><string>/tmp/sd-mac-run.log</string>
  <key>EnvironmentVariables</key>
  <dict>
    <key>SEAMLESS_BIND</key>
    <string>$BIND_IP</string>
  </dict>
</dict>
</plist>
PLIST

launchctl unload "$PLIST" >/dev/null 2>&1 || true
launchctl load "$PLIST"
sleep 3
echo "[install-mac] インストール完了: ~/Applications/SeamlessDesk.app (bind=$BIND_IP)"
echo "[install-mac] ログイン時に自動起動します / ログ: /tmp/sd-mac-run.log"
if ! grep -q "tap active" /tmp/sd-mac-run.log 2>/dev/null; then
  echo "[install-mac] 注意: アクセシビリティ権限の再許可が必要な場合があります"
  echo "  システム設定 > プライバシーとセキュリティ > アクセシビリティ に SeamlessDesk を追加"
fi

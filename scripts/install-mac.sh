#!/bin/bash
# このMacへ .app としてインストールする(ログイン時自動起動の LaunchAgent 登録付き)。
# 前提: scripts/package-mac.sh を先に実行して dist/Tsunagu.app を作成しておく
set -e
cd "$(dirname "$0")/.."
APP_SRC="dist/Tsunagu.app"
if [ ! -d "$APP_SRC" ]; then
  echo "[install-mac] $APP_SRC がありません。scripts/package-mac.sh を先に実行してください" >&2
  exit 1
fi

# 開発運用(restart-mac.sh)と併用しない(二重起動になるため先に止める)
pkill -9 -f "target/release/tsunagu-mac" 2>/dev/null || true
# v0.7(seamless-desk)の旧 LaunchAgent を掃除する(移行処理)
if [ -f "$HOME/Library/LaunchAgents/local.seamless-desk.plist" ]; then
  launchctl unload "$HOME/Library/LaunchAgents/local.seamless-desk.plist" >/dev/null 2>&1 || true
  rm -f "$HOME/Library/LaunchAgents/local.seamless-desk.plist"
  rm -rf "$HOME/Applications/SeamlessDesk.app"
  echo "[install-mac] 旧版(SeamlessDesk)の自動起動を解除しました"
fi
pkill -9 -f "Tsunagu.app/Contents/MacOS/Tsunagu" 2>/dev/null || true
sleep 1

rm -rf "$HOME/Applications/Tsunagu.app"
cp -R "$APP_SRC" "$HOME/Applications/Tsunagu.app"
BIN="$HOME/Applications/Tsunagu.app/Contents/MacOS/Tsunagu"
chmod +x "$BIN"

# ログは開発と同じ /tmp/tsunagu-mac.log へ(verify.sh 互換)。
# TSUNAGU_BIND は Tailscale IF に限定(失敗時は 0.0.0.0 で起動しピア判定で防御)
BIND_IP=$(tailscale ip -4 2>/dev/null | head -1)
[ -z "$BIND_IP" ] && BIND_IP="0.0.0.0"
PLIST="$HOME/Library/LaunchAgents/local.tsunagu.plist"
cat > "$PLIST" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>local.tsunagu</string>
  <key>ProgramArguments</key>
  <array>
    <string>$BIN</string>
  </array>
  <key>RunAtLoad</key><true/>
  <key>StandardOutPath</key><string>/tmp/tsunagu-mac.log</string>
  <key>StandardErrorPath</key><string>/tmp/tsunagu-mac.log</string>
  <key>EnvironmentVariables</key>
  <dict>
    <key>TSUNAGU_BIND</key>
    <string>$BIND_IP</string>
  </dict>
</dict>
</plist>
PLIST

launchctl unload "$PLIST" >/dev/null 2>&1 || true
launchctl load "$PLIST"
sleep 3
echo "[install-mac] インストール完了: ~/Applications/Tsunagu.app (bind=$BIND_IP)"
echo "[install-mac] ログイン時に自動起動します / ログ: /tmp/tsunagu-mac.log"
if ! grep -q "tap active" /tmp/tsunagu-mac.log 2>/dev/null; then
  echo "[install-mac] 注意: アクセシビリティ権限の再許可が必要な場合があります"
  echo "  システム設定 > プライバシーとセキュリティ > アクセシビリティ に Tsunagu を追加"
fi

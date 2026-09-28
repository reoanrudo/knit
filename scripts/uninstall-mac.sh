#!/bin/bash
# Knit をこの Mac からアンインストールする(常駐解除 + アプリ削除)。
# 使い方: ./scripts/uninstall-mac.sh [--purge]
#   --purge を付けると設定(~/.config/knit = トークン)も削除する
set -e
cd "$(dirname "$0")/.."

echo "[uninstall-mac] 常駐を停止中..."
pkill -9 -f "target/release/knit-mac" 2>/dev/null || true
pkill -9 -f "Knit.app/Contents/MacOS/Knit" 2>/dev/null || true
pkill -9 -f "SeamlessDesk.app/Contents/MacOS/SeamlessDesk" 2>/dev/null || true
pkill -9 -f "target/release/tsunagu-mac" 2>/dev/null || true
pkill -9 -f "Tsunagu.app/Contents/MacOS/Tsunagu" 2>/dev/null || true

for plist in local.knit local.tsunagu local.seamless-desk; do
  P="$HOME/Library/LaunchAgents/$plist.plist"
  if [ -f "$P" ]; then
    launchctl unload "$P" >/dev/null 2>&1 || true
    rm -f "$P"
    echo "[uninstall-mac] 自動起動を解除しました: $plist"
  fi
done

rm -rf "$HOME/Applications/Knit.app" "$HOME/Applications/Tsunagu.app" "$HOME/Applications/SeamlessDesk.app"
echo "[uninstall-mac] アプリを削除しました"

if [ "${1:-}" = "--purge" ]; then
  rm -rf "$HOME/.config/knit" "$HOME/.config/tsunagu"
  echo "[uninstall-mac] 設定(トークン含む)も削除しました"
fi

echo "[uninstall-mac] 完了"
echo "  任意: システム設定 > プライバシーとセキュリティ > アクセシビリティ から Knit を外す"

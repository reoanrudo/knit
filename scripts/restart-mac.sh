#!/bin/bash
# tsunagu-mac のビルド鮮度を保証して再起動する(改善ループ用ワンコマンド)
set -e
cd "$(dirname "$0")/.."
source $HOME/.cargo/env 2>/dev/null || true

echo "[restart-mac] stamping BUILD_ID..."
NEW_ID="build-$(date +%Y%m%d-%H%M%S)-$(git rev-parse --short HEAD)"
sed -i '' "s|const BUILD_ID:[^;]*;|const BUILD_ID: \&str = \"$NEW_ID\";|" crates/mac/src/main.rs

echo "[restart-mac] building..."
touch crates/mac/src/main.rs
cargo build --release 2>&1 | grep -E "^error" -A 3 && exit 1 || true
cargo build --release 2>&1 | tail -1

# --hotkey/--edge を切替モードへ変換し、残りは tsunagu-mac へそのまま渡す
SWITCH_MODE=edge
ARGS=""
for a in "$@"; do
  case "$a" in
    --hotkey) SWITCH_MODE=hotkey ;;
    --edge)   SWITCH_MODE=edge ;;
    *)        ARGS="$ARGS $a" ;;
  esac
done

echo "[restart-mac] restarting (switch_mode=$SWITCH_MODE)..."
pkill -9 -f "target/release/tsunagu-mac" 2>/dev/null || true
sleep 1
# 待受を Tailscale IF に限定(取得失敗時は 0.0.0.0 + ピア範囲判定で防御)
BIND_IP=$(tailscale ip -4 2>/dev/null | head -1)
[ -z "$BIND_IP" ] && BIND_IP="0.0.0.0"
TSUNAGU_BIND=$BIND_IP TSUNAGU_SWITCH_MODE=$SWITCH_MODE nohup ./target/release/tsunagu-mac $ARGS > /tmp/tsunagu-mac.log 2>&1 &
disown
sleep 4

echo "[restart-mac] status (bind=$BIND_IP):"
head -1 /tmp/tsunagu-mac.log   # ビルドID確認用
grep -E "tap active|established|fatal|メニューバー" /tmp/tsunagu-mac.log || echo "(接続待ち: tsunagu-win が再接続します)"
if ! pgrep -q -f "target/release/tsunagu-mac"; then
  echo "[restart-mac] WARN: プロセスが起動直後に終了しました。ログ末尾:"
  tail -5 /tmp/tsunagu-mac.log
fi

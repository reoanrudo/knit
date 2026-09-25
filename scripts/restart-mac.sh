#!/bin/bash
# sd-mac のビルド鮮度を保証して再起動する(改善ループ用ワンコマンド)
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

echo "[restart-mac] restarting..."
pkill -9 -f "target/release/sd-mac" 2>/dev/null || true
sleep 1
nohup ./target/release/sd-mac "$@" > /tmp/sd-mac-run.log 2>&1 &
disown
sleep 4

echo "[restart-mac] status:"
head -1 /tmp/sd-mac-run.log   # ビルドID確認用
grep -E "tap active|established|fatal" /tmp/sd-mac-run.log || echo "(接続待ち: sd-win が再接続します)"
if ! pgrep -q -f "target/release/sd-mac"; then
  echo "[restart-mac] WARN: プロセスが起動直後に終了しました。ログ末尾:"
  tail -5 /tmp/sd-mac-run.log
fi

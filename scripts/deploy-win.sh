#!/bin/bash
# sd-win をビルドして Windows へ配布・再起動する(改善ループ用ワンコマンド)
set -e
cd "$(dirname "$0")/.."
source $HOME/.cargo/env 2>/dev/null || true

echo "[deploy-win] stamping BUILD_ID..."
NEW_ID="win-$(date +%Y%m%d-%H%M%S)-$(git rev-parse --short HEAD)"
sed -i '' "s|const BUILD_ID:[^;]*;|const BUILD_ID: \&str = \"$NEW_ID\";|" crates/win/src/main.rs

echo "[deploy-win] building..."
touch crates/win/src/main.rs
cargo build --release -p sd-win --target x86_64-pc-windows-gnu 2>&1 | grep -E "^error" -A 3 && exit 1 || true
cargo build --release -p sd-win --target x86_64-pc-windows-gnu 2>&1 | tail -1

echo "[deploy-win] deploying (stop -> copy -> start)..."
ssh -o BatchMode=yes home "schtasks /End /TN seamless_desk_run" >/dev/null 2>&1 || true
ssh -o BatchMode=yes home "taskkill /IM sd-win.exe /F" >/dev/null 2>&1 || true
sleep 2
scp -o BatchMode=yes target/x86_64-pc-windows-gnu/release/sd-win.exe home:C:/Users/<user>/seamless-desk/sd-win.exe
ssh -o BatchMode=yes home "schtasks /Run /TN seamless_desk_run" >/dev/null 2>&1
sleep 3

echo "[deploy-win] status:"
ssh -o BatchMode=yes home "tasklist | findstr sd-win" 2>&1 | grep -v "^\*\*" | head -1
ssh -o BatchMode=yes home "type C:\Users\<user>\seamless-desk\sd-win.log" 2>&1 | grep -v "^\*\*" | tail -2

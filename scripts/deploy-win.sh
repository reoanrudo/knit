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
cargo build --release -p sd-win --target x86_64-pc-windows-gnu 2>&1 | tail -1 >/dev/null

# win-dist の exe も最新化する(install.bat は win-dist からコピーするため)
cp target/x86_64-pc-windows-gnu/release/sd-win.exe win-dist/sd-win.exe

# トークン(.env)は Mac 側の設定から配布(無いと Windows 側で fatal 停止する)
TOKEN_SRC="$HOME/.config/seamless-desk/env"
if [ ! -f "$TOKEN_SRC" ]; then
  echo "[deploy-win] $TOKEN_SRC がありません。scripts/gen-token.sh を先に実行してください" >&2
  exit 1
fi

echo "[deploy-win] deploying (stop -> copy -> start)..."
# コンソール窓が出ないよう VBS 起動へタスクを更新(冪等)
ssh -o BatchMode=yes home "schtasks /Create /TN seamless_desk_run /TR "wscript.exe \"C:\\Users\\<user>\\seamless-desk\\run_sd.vbs\"" /SC ONCE /ST 23:59 /F" >/dev/null 2>&1 || true
ssh -o BatchMode=yes home "schtasks /End /TN seamless_desk_run" >/dev/null 2>&1 || true
ssh -o BatchMode=yes home "taskkill /IM sd-win.exe /F" >/dev/null 2>&1 || true
sleep 2
scp -o BatchMode=yes target/x86_64-pc-windows-gnu/release/sd-win.exe home:C:/Users/<user>/seamless-desk/sd-win.exe
# 起動資材(ログローテーション実効化のため bat 経由へ変更)+アイコン+トークンも更新
scp -o BatchMode=yes win-dist/run_sd.vbs win-dist/run_sd.bat home:C:/Users/<user>/seamless-desk/ >/dev/null
scp -o BatchMode=yes win-dist/app.ico home:C:/Users/<user>/seamless-desk/ >/dev/null
scp -o BatchMode=yes "$TOKEN_SRC" home:C:/Users/<user>/seamless-desk/.env >/dev/null
# 自動復帰ウォッチ(5分毎。二重起動は exe 側のミューテックスで即終了)
ssh -o BatchMode=yes home "schtasks /Create /TN seamless_desk_watch /TR "wscript.exe \"C:\\Users\\<user>\\seamless-desk\\run_sd.vbs\"" /SC MINUTE /MO 5 /F" >/dev/null 2>&1 || true
ssh -o BatchMode=yes home "schtasks /Run /TN seamless_desk_run" >/dev/null 2>&1
sleep 3

echo "[deploy-win] status:"
ssh -o BatchMode=yes home "tasklist | findstr sd-win" 2>&1 | grep -v "^\*\*" | head -1
ssh -o BatchMode=yes home "type C:\Users\<user>\seamless-desk\sd-win.log" 2>&1 | grep -v "^\*\*" | tail -2

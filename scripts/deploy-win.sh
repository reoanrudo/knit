#!/bin/bash
# tsunagu-win をビルドして Windows へ配布・再起動する(改善ループ用ワンコマンド)
set -e
cd "$(dirname "$0")/.."
source $HOME/.cargo/env 2>/dev/null || true

echo "[deploy-win] stamping BUILD_ID..."
NEW_ID="win-$(date +%Y%m%d-%H%M%S)-$(git rev-parse --short HEAD)"
sed -i '' "s|const BUILD_ID:[^;]*;|const BUILD_ID: \&str = \"$NEW_ID\";|" crates/win/src/main.rs
sleep 1

echo "[deploy-win] building..."
touch crates/win/src/main.rs
cargo build --release -p tsunagu-win --target x86_64-pc-windows-gnu 2>&1 | grep -E "^error" -A 3 && exit 1 || true
cargo build --release -p tsunagu-win --target x86_64-pc-windows-gnu 2>&1 | tail -1 >/dev/null

# 産物検証: exe 内の BUILD_ID がスタンプと一致することを確認する。
# cargo の差分検知が同秒 mtime で miss し「古い exe を配って実機が更新されない」
# 事故が実績があるため、不一致時は fingerprint を消して強制再ビルドする
EXE=target/x86_64-pc-windows-gnu/release/tsunagu-win.exe
verify_build() {
  strings -a "$EXE" 2>/dev/null | grep -q "$NEW_ID"
}
if ! verify_build; then
  echo "[deploy-win] BUILD_ID 不一致(古い産物)。クリーン再ビルドします..."
  rm -rf target/x86_64-pc-windows-gnu/release/.fingerprint/tsunagu-win*
  rm -f "$EXE"
  cargo build --release -p tsunagu-win --target x86_64-pc-windows-gnu 2>&1 | grep -E "^error" -A3 && exit 1 || true
  verify_build || { echo "[deploy-win] 再ビルドしても BUILD_ID が一致しません" >&2; exit 1; }
fi

# win-dist の exe も最新化する(install.bat は win-dist からコピーするため)
cp "$EXE" win-dist/tsunagu-win.exe

# トークン(.env)は Mac 側の設定から配布(無いと Windows 側で fatal 停止する)
TOKEN_SRC="$HOME/.config/tsunagu/env"
if [ ! -f "$TOKEN_SRC" ] && [ -f "$HOME/.config/seamless-desk/env" ]; then
  # v0.7(旧名称)からの移行: 旧設定を新パスへ移植する
  mkdir -p "$HOME/.config/tsunagu"
  cp "$HOME/.config/seamless-desk/env" "$TOKEN_SRC"
  echo "[deploy-win] 旧設定(seamless-desk)を ~/.config/tsunagu/env へ移植しました"
fi
if [ ! -f "$TOKEN_SRC" ]; then
  echo "[deploy-win] $TOKEN_SRC がありません。scripts/gen-token.sh を先に実行してください" >&2
  exit 1
fi

echo "[deploy-win] deploying (stop -> copy -> start)..."
# 配布中の自動復帰(watch)を一時停止し、配布完了後に再有効化する
ssh -o BatchMode=yes home "schtasks /Change /TN tsunagu_watch /DISABLE" >/dev/null 2>&1 || true
# 旧名称(v0.7)のタスクとプロセスを掃除する(二重常駐・混在を防ぐ移行処理)
# 注意: リモート cmd.exe は「;」をコマンド区切りにしない(実績バグ: 旧タスクが
# 消えず「run_sd.vbs が見つかりません」のダイアログが出続けた)。必ず「&」で区切る
for t in seamless_desk seamless_desk_run seamless_desk_watch; do
  ssh -o BatchMode=yes home "schtasks /End /TN $t & schtasks /Delete /TN $t /F" >/dev/null 2>&1 || true
done
ssh -o BatchMode=yes home "taskkill /IM sd-win.exe /F" >/dev/null 2>&1 || true
ssh -o BatchMode=yes home "if not exist C:\Users\<user>\tsunagu mkdir C:\Users\<user>\tsunagu" >/dev/null 2>&1 || true
# コンソール窓が出ないよう VBS 起動へタスクを更新(冪等)。
# 注意: クォートは外側をシングルにしないとリモート側で割れてタスク作成が
# 黙って失敗する(実績バグ: watch タスクが消えて自動復帰が無効化されていた)
ssh -o BatchMode=yes home 'schtasks /Create /TN tsunagu_run /TR "wscript.exe C:\Users\<user>\tsunagu\run_tsunagu.vbs" /SC ONCE /ST 23:59 /F' >/dev/null 2>&1 || true
ssh -o BatchMode=yes home "schtasks /End /TN tsunagu_run" >/dev/null 2>&1 || true
ssh -o BatchMode=yes home "taskkill /IM tsunagu-win.exe /F" >/dev/null 2>&1 || true
# 起動 bat のローテーションが「前プロセスが掴んだままのログ」に負けて失敗し、
# ログ先頭に古い起動が残るのを防ぐ(プロセス停止後にこちらで退避しておく)
ssh -o BatchMode=yes home "move /y C:\Users\<user>\tsunagu\tsunagu-win.log C:\Users\<user>\tsunagu\tsunagu-win.log.prev >nul 2>&1" >/dev/null 2>&1 || true
sleep 2
scp -o BatchMode=yes target/x86_64-pc-windows-gnu/release/tsunagu-win.exe home:C:/Users/<user>/tsunagu/tsunagu-win.exe
# 起動資材(ログローテーション実効化のため bat 経由へ変更)+アイコン+トークンも更新
scp -o BatchMode=yes win-dist/run_tsunagu.vbs win-dist/run_tsunagu.bat home:C:/Users/<user>/tsunagu/ >/dev/null
scp -o BatchMode=yes win-dist/app.ico home:C:/Users/<user>/tsunagu/ >/dev/null
scp -o BatchMode=yes "$TOKEN_SRC" home:C:/Users/<user>/tsunagu/.env >/dev/null
# 自動復帰ウォッチ(毎分。二重起動は exe 側のミューテックスで即終了)
ssh -o BatchMode=yes home 'schtasks /Create /TN tsunagu_watch /TR "wscript.exe C:\Users\<user>\tsunagu\run_tsunagu.vbs" /SC MINUTE /MO 1 /F' >/dev/null 2>&1 || true
ssh -o BatchMode=yes home "schtasks /Run /TN tsunagu_run" >/dev/null 2>&1
sleep 3
# 配布の隙間に watch が旧 exe を起こしてミューテックスで新 exe をはじく事故を防ぐ
ssh -o BatchMode=yes home "schtasks /Change /TN tsunagu_watch /ENABLE" >/dev/null 2>&1 || true

echo "[deploy-win] status:"
ssh -o BatchMode=yes home "tasklist | findstr tsunagu-win" 2>&1 | grep -v "^\*\*" | head -1
ssh -o BatchMode=yes home "type C:\Users\<user>\tsunagu\tsunagu-win.log" 2>&1 | grep -v "^\*\*" | tail -2

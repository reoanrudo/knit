#!/bin/bash
# 改善ループ用の自動検証: プロセス・接続・クリップボード双方向・diagログ
# 使い方: ./scripts/verify.sh
set -u
cd "$(dirname "$0")/.."
SSH="ssh -o BatchMode=yes home"

pass=0; fail=0
check() { # check "名前" "期待" "実際"
  if [ "$2" = "$3" ]; then echo "  OK  $1"; pass=$((pass+1)); else echo "  NG  $1 (expect=[$2] got=[$3])"; fail=$((fail+1)); fi
}

echo "[verify] Mac プロセス:"
if pgrep -q -f "target/release/sd-mac"; then echo "  OK  sd-mac 稼働中"; pass=$((pass+1)); else echo "  NG  sd-mac 不在"; fail=$((fail+1)); fi

echo "[verify] 接続状態(Macログ):"
tail -20 /tmp/sd-mac-run.log 2>/dev/null | grep -q established && { echo "  OK  established"; pass=$((pass+1)); } || { echo "  NG  未接続"; fail=$((fail+1)); }

echo "[verify] Windows プロセス:"
if $SSH "tasklist | findstr sd-win" 2>/dev/null | grep -q sd-win; then echo "  OK  sd-win 稼働中"; pass=$((pass+1)); else echo "  NG  sd-win 不在"; fail=$((fail+1)); fi

TS=$(date +%s)
echo "[verify] Win→Mac クリップボード:"
printf '@echo off\r\npowershell -NoProfile -Command "Set-Clipboard -Value '\''verify-wm-%s'\''"\r\n' "$TS" > /tmp/clip_set.bat
scp -q -o BatchMode=yes /tmp/clip_set.bat home:C:/Users/<user>/seamless-desk/ 2>/dev/null
$SSH "schtasks /Run /TN sd_clip_set" >/dev/null 2>&1
sleep 4
GOT=$(pbpaste 2>/dev/null | tr -d '\r\n')
check "Win→Mac ペースト一致" "verify-wm-$TS" "$GOT"

echo "[verify] Mac→Win クリップボード:"
printf 'verify-mw-%s' "$TS" | pbcopy
sleep 3
$SSH "schtasks /Run /TN sd_clip_get" >/dev/null 2>&1
sleep 3
GOT2=$($SSH "type C:\\Users\\<user>\\seamless-desk\\clip_get.txt" 2>/dev/null | tail -1 | tr -d '\r\n' | sed $'s/^\xEF\xBB\xBF//')
check "Mac→Win ペースト一致" "verify-mw-$TS" "$GOT2"

echo "[verify] 直近 diag:"
grep '\[diag\]' /tmp/sd-mac-run.log 2>/dev/null | tail -3

echo "[verify] 完了: pass=$pass fail=$fail"
[ "$fail" -eq 0 ]

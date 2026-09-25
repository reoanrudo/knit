#!/bin/bash
# 改善ループ用の自動検証: プロセス・接続・クリップボード双方向・diag集計・IMEログ
# 使い方: ./scripts/verify.sh
# WARN は fail にカウントしない(接続失敗等で検証不能な項目を明示するため)
set -u
cd "$(dirname "$0")/.."
SSH="ssh -o BatchMode=yes -o ConnectTimeout=5 home"
SCP="scp -q -o BatchMode=yes -o ConnectTimeout=5"

pass=0; fail=0; warn=0
check() { # check "名前" "期待" "実際"
  if [ "$2" = "$3" ]; then echo "  OK  $1"; pass=$((pass+1)); else echo "  NG  $1 (expect=[$2] got=[$3])"; fail=$((fail+1)); fi
}
warn_msg() { # warn_msg "メッセージ" — 検証不能な項目を fail にせず WARN 表示
  echo "  WARN $1"; warn=$((warn+1))
}

echo "[verify] Mac プロセス:"
if pgrep -q -f "target/release/sd-mac"; then echo "  OK  sd-mac 稼働中"; pass=$((pass+1)); else echo "  NG  sd-mac 不在"; fail=$((fail+1)); fi

echo "[verify] 接続可否(Windows ssh):"
WIN_OK=1
if $SSH "exit" 2>/dev/null; then
  echo "  OK  接続成功"; pass=$((pass+1))
else
  WIN_OK=0
  warn_msg "ssh 接続失敗のため Windows 側チェックはスキップ(WARN 扱い)"
fi

echo "[verify] 接続状態(Macログ):"
tail -20 /tmp/sd-mac-run.log 2>/dev/null | grep -q established && { echo "  OK  established"; pass=$((pass+1)); } || { echo "  NG  未接続"; fail=$((fail+1)); }

echo "[verify] Windows プロセス:"
if [ "$WIN_OK" -eq 1 ]; then
  WINPROC=$($SSH "tasklist | findstr sd-win" 2>/dev/null); RC=$?
  if [ "$RC" -eq 255 ]; then
    warn_msg "ssh エラーのため sd-win 稼働確認不能"
  elif printf '%s' "$WINPROC" | grep -q sd-win; then
    echo "  OK  sd-win 稼働中"; pass=$((pass+1))
  else
    echo "  NG  sd-win 不在"; fail=$((fail+1))
  fi
else
  warn_msg "sd-win 稼働確認スキップ(接続失敗)"
fi

TS=$(date +%s)
echo "[verify] Win→Mac クリップボード:"
if [ "$WIN_OK" -eq 1 ]; then
  printf '@echo off\r\npowershell -NoProfile -Command "Set-Clipboard -Value '\''verify-wm-%s'\''"\r\n' "$TS" > /tmp/clip_set.bat
  if $SCP /tmp/clip_set.bat home:C:/Users/<user>/seamless-desk/ 2>/dev/null; then
    $SSH "schtasks /Run /TN sd_clip_set" >/dev/null 2>&1
    sleep 4
    GOT=$(pbpaste 2>/dev/null | tr -d '\r\n')
    check "Win→Mac ペースト一致" "verify-wm-$TS" "$GOT"
  else
    warn_msg "clip_set.bat 転送失敗のため検証スキップ"
  fi
else
  warn_msg "Win→Mac クリップボード検証スキップ(接続失敗)"
fi

echo "[verify] Mac→Win クリップボード:"
if [ "$WIN_OK" -eq 1 ]; then
  printf 'verify-mw-%s' "$TS" | pbcopy
  sleep 3
  $SSH "schtasks /Run /TN sd_clip_get" >/dev/null 2>&1
  sleep 3
  RAW=$($SSH "type C:\\Users\\<user>\\seamless-desk\\clip_get.txt" 2>/dev/null); RC=$?
  if [ "$RC" -eq 255 ]; then
    warn_msg "ssh エラーのため Mac→Win 結果取得不能"
  else
    GOT2=$(printf '%s' "$RAW" | tail -1 | tr -d '\r\n' | sed $'s/^\xEF\xBB\xBF//')
    check "Mac→Win ペースト一致" "verify-mw-$TS" "$GOT2"
  fi
else
  warn_msg "Mac→Win クリップボード検証スキップ(接続失敗)"
fi

echo "[verify] IME ログ(Windows):"
if [ "$WIN_OK" -eq 1 ]; then
  IMELOG=$($SSH "type C:\\Users\\<user>\\seamless-desk\\sd-win.log" 2>/dev/null); RC=$?
  if [ "$RC" -eq 255 ]; then
    warn_msg "IMEは未検証(実機でかな/英数キーを押した実績なし)(sd-win.log 取得不能)"
  elif printf '%s' "$IMELOG" | grep -q '\[ime\]'; then
    echo "  OK  [ime] 行を検出"; pass=$((pass+1))
  else
    warn_msg "IMEは未検証(実機でかな/英数キーを押した実績なし)"
  fi
else
  warn_msg "IMEは未検証(実機でかな/英数キーを押した実績なし)"
fi

echo "[verify] diag 集計(/tmp/sd-mac-run.log 直近50行の [diag]):"
DIAG_ALL=$(tail -50 /tmp/sd-mac-run.log 2>/dev/null | grep -c '\[diag\]')
DIAG_WIN=$(tail -50 /tmp/sd-mac-run.log 2>/dev/null | grep '\[diag\]' | grep -c 'mode=WIN')
DIAG_WARP=$(tail -50 /tmp/sd-mac-run.log 2>/dev/null | grep '\[diag\]' | grep -c 'warp_fixed=[1-9]')
echo "  diag行数=$DIAG_ALL mode=WIN=$DIAG_WIN warp_fixed>0=$DIAG_WARP"

echo "[verify] 直近 diag:"
grep '\[diag\]' /tmp/sd-mac-run.log 2>/dev/null | tail -3

END_TIME=$(date '+%Y-%m-%d %H:%M:%S')
echo "[verify] 完了: 実行日時=$END_TIME pass=$pass fail=$fail warn=$warn"
[ "$fail" -eq 0 ]

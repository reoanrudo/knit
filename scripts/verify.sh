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
if pgrep -q -f "target/release/tsunagu-mac"; then echo "  OK  tsunagu-mac 稼働中"; pass=$((pass+1)); else echo "  NG  tsunagu-mac 不在"; fail=$((fail+1)); fi

echo "[verify] 接続可否(Windows ssh):"
WIN_OK=1
if $SSH "exit" 2>/dev/null; then
  echo "  OK  接続成功"; pass=$((pass+1))
else
  WIN_OK=0
  warn_msg "ssh 接続失敗のため Windows 側チェックはスキップ(WARN 扱い)"
fi

echo "[verify] 接続状態(Macログ):"
# 直近の接続イベントが established なら接続中(diag行で押し出されないよう
# ログ全体から最後の conn 行を見る)
LAST_CONN=$(grep -E "established|\[conn\] lost" /tmp/tsunagu-mac.log 2>/dev/null | tail -1)
[ -n "$LAST_CONN" ] && echo "$LAST_CONN" | grep -q established && { echo "  OK  established"; pass=$((pass+1)); } || { echo "  NG  未接続"; fail=$((fail+1)); }

echo "[verify] Windows プロセス:"
if [ "$WIN_OK" -eq 1 ]; then
  WINPROC=$($SSH "tasklist | findstr tsunagu-win" 2>/dev/null); RC=$?
  if [ "$RC" -eq 255 ]; then
    warn_msg "ssh エラーのため tsunagu-win 稼働確認不能"
  elif printf '%s' "$WINPROC" | grep -q tsunagu-win; then
    echo "  OK  tsunagu-win 稼働中"; pass=$((pass+1))
  else
    echo "  NG  tsunagu-win 不在"; fail=$((fail+1))
  fi
else
  warn_msg "tsunagu-win 稼働確認スキップ(接続失敗)"
fi

TS=$(date +%s)
echo "[verify] Win→Mac クリップボード:"
if [ "$WIN_OK" -eq 1 ]; then
  printf '@echo off\r\npowershell -NoProfile -Command "Set-Clipboard -Value '\''verify-wm-%s'\''"\r\n' "$TS" > /tmp/clip_set.bat
  if $SCP /tmp/clip_set.bat home:C:/Users/<user>/tsunagu/ 2>/dev/null; then
    $SSH "schtasks /Run /TN tsunagu_clip_set" >/dev/null 2>&1
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
  # 検証用 bat を都度生成・配置(検証の自己完結化: 手動前提をなくす)
  printf '@echo off\r\npowershell -NoProfile -Command "Get-Clipboard | Out-File -Encoding utf8 C:\\Users\\<user>\\tsunagu\\clip_get.txt"\r\n' > /tmp/clip_get.bat
  $SCP /tmp/clip_get.bat home:C:/Users/<user>/tsunagu/ 2>/dev/null
  printf 'verify-mw-%s' "$TS" | pbcopy
  sleep 3
  $SSH "schtasks /Run /TN tsunagu_clip_get" >/dev/null 2>&1
  sleep 3
  RAW=$($SSH "type C:\\Users\\<user>\\tsunagu\\clip_get.txt" 2>/dev/null); RC=$?
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
  IMELOG=$($SSH "type C:\\Users\\<user>\\tsunagu\\tsunagu-win.log" 2>/dev/null); RC=$?
  if [ "$RC" -eq 255 ]; then
    warn_msg "IMEは未検証(実機でかな/英数キーを押した実績なし)(tsunagu-win.log 取得不能)"
  elif printf '%s' "$IMELOG" | grep -q '\[ime\]'; then
    echo "  OK  [ime] 行を検出"; pass=$((pass+1))
  else
    warn_msg "IMEは未検証(実機でかな/英数キーを押した実績なし)"
  fi
else
  warn_msg "IMEは未検証(実機でかな/英数キーを押した実績なし)"
fi

echo "[verify] Win→Mac ファイル送信:"
if [ "$WIN_OK" -eq 1 ]; then
  # ファイル名にタイムスタンプを含める(同名同サイズだと Win 側の指紋チェックが
  # 同一コピーの再検出とみなし再送しないため、毎回別物にする)
  rm -f "$HOME/Downloads/Tsunagu/"win_verify_*.txt
  cat > /tmp/clip_file_set.bat <<BAT
@echo off
powershell -NoProfile -Command "Set-Content -Path C:\\Users\\<user>\\tsunagu\\win_verify_$TS.txt -Value 'verify-wf-$TS'; Set-Clipboard -Path C:\\Users\\<user>\\tsunagu\\win_verify_$TS.txt"
BAT
  sed -e 's/$/\r/' /tmp/clip_file_set.bat > /tmp/clip_file_set_crlf.bat && mv /tmp/clip_file_set_crlf.bat /tmp/clip_file_set.bat
  if $SCP /tmp/clip_file_set.bat home:C:/Users/<user>/tsunagu/ 2>/dev/null; then
    $SSH 'schtasks /Create /TN tsunagu_clip_file /TR "cmd /c C:\Users\<user>\tsunagu\clip_file_set.bat" /SC ONCE /ST 23:59 /F' >/dev/null 2>&1
    $SSH "schtasks /Run /TN tsunagu_clip_file" >/dev/null 2>&1
    sleep 8
    GOT_FILE=$(cat "$HOME/Downloads/Tsunagu/win_verify_$TS.txt" 2>/dev/null | tr -d '\r\n')
    check "Win→Mac ファイル内容一致" "verify-wf-$TS" "$GOT_FILE"
  else
    warn_msg "clip_file_set.bat 転送失敗のため検証スキップ"
  fi
else
  warn_msg "Win→Mac ファイル送信検証スキップ(接続失敗)"
fi

echo "[verify] 配布物(リブランド後の一式):"
[ -f LICENSE ] && { echo "  OK  LICENSE"; pass=$((pass+1)); } || { echo "  NG  LICENSE 不在"; fail=$((fail+1)); }
[ -f CHANGELOG.md ] && { echo "  OK  CHANGELOG.md"; pass=$((pass+1)); } || { echo "  NG  CHANGELOG.md 不在"; fail=$((fail+1)); }
[ -f win-dist/uninstall.bat ] && { echo "  OK  win-dist/uninstall.bat"; pass=$((pass+1)); } || { echo "  NG  win-dist/uninstall.bat 不在"; fail=$((fail+1)); }
[ -x scripts/uninstall-mac.sh ] && { echo "  OK  scripts/uninstall-mac.sh"; pass=$((pass+1)); } || { echo "  NG  scripts/uninstall-mac.sh 不在"; fail=$((fail+1)); }
[ -f win-dist/tsunagu-win.exe ] && { echo "  OK  win-dist/tsunagu-win.exe"; pass=$((pass+1)); } || { echo "  NG  win-dist/tsunagu-win.exe 不在"; fail=$((fail+1)); }
grep -q "Tsunagu" README.md && { echo "  OK  README ブランド表記"; pass=$((pass+1)); } || { echo "  NG  README ブランド表記"; fail=$((fail+1)); }
# 旧名称の残存チェック(機能本体=crates のみ。共通 lib.rs の移行フォールバックと
# スクリプト類の旧タスク掃除・旧設定移植の記述は移行処理として意図的なため除外)
if grep -rl "seamless" --include="*.rs" crates 2>/dev/null | grep -v "crates/common/src/lib.rs" | grep -q .; then
  echo "  WARN 旧名称(seamless)がコードに残存(移行フォールバック以外は要掃除)"
  warn=$((warn+1))
else
  echo "  OK  旧名称の残存なし"; pass=$((pass+1))
fi

echo "[verify] diag 集計(/tmp/tsunagu-mac.log 直近50行の [diag]):"
DIAG_ALL=$(tail -50 /tmp/tsunagu-mac.log 2>/dev/null | grep -c '\[diag\]')
DIAG_WIN=$(tail -50 /tmp/tsunagu-mac.log 2>/dev/null | grep '\[diag\]' | grep -c 'mode=WIN')
DIAG_WARP=$(tail -50 /tmp/tsunagu-mac.log 2>/dev/null | grep '\[diag\]' | grep -c 'warp_fixed=[1-9]')
echo "  diag行数=$DIAG_ALL mode=WIN=$DIAG_WIN warp_fixed>0=$DIAG_WARP"

echo "[verify] 切替モード:"
grep -o "switch_mode=[a-z]*" /tmp/tsunagu-mac.log 2>/dev/null | tail -1 || echo "  (switch_mode 未表示)"

echo "[verify] 直近 diag:"
grep '\[diag\]' /tmp/tsunagu-mac.log 2>/dev/null | tail -3

END_TIME=$(date '+%Y-%m-%d %H:%M:%S')
echo "[verify] 完了: 実行日時=$END_TIME pass=$pass fail=$fail warn=$warn"
[ "$fail" -eq 0 ]

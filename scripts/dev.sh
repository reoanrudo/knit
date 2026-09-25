#!/bin/bash
# 改善ループ用ワンコマンド: 両OS ビルド→配備→検証まで一気通貫。
# 使い方: ./scripts/dev.sh [--no-win] [--no-verify] [restart-mac/deploy-win への追加引数...]
#   --no-win    Windows 配備をスキップ(Mac 側だけ変えた時)
#   --no-verify 自動検証をスキップ
set -e
set -o pipefail
cd "$(dirname "$0")/.."
source $HOME/.cargo/env 2>/dev/null || true

SKIP_WIN=0; SKIP_VERIFY=0; EXTRA=()
for a in "$@"; do
  case "$a" in
    --no-win) SKIP_WIN=1 ;;
    --no-verify) SKIP_VERIFY=1 ;;
    *) EXTRA+=("$a") ;;
  esac
done

echo "[dev] === 1/3 Mac: ビルド+再起動 ==="
./scripts/restart-mac.sh "${EXTRA[@]}" 2>&1 | tail -2

if [ "$SKIP_WIN" -eq 0 ]; then
  echo "[dev] === 2/3 Windows: ビルド+配備 ==="
  ./scripts/deploy-win.sh 2>&1 | grep -vE "WARNING|post-quantum|openssh|vulnerable" | tail -3
else
  echo "[dev] === 2/3 Windows: スキップ(--no-win) ==="
fi

if [ "$SKIP_VERIFY" -eq 0 ]; then
  echo "[dev] === 3/3 自動検証 ==="
  ./scripts/verify.sh 2>&1 | grep -E "  NG|  OK|完了" | tail -20
fi
echo "[dev] 完了。ログ: /tmp/tsunagu-mac.log / Win: type tsunagu-win.log"

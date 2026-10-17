#!/usr/bin/env bash
# 性能計測をまとめて実行する(改善ループで作った計測器の入口)。
#
#   bash scripts/bench.sh           # 数値を表示するだけ
#   bash scripts/bench.sh --check   # 「壊滅的な回帰」だけを検出して失敗する
#
# --check の閾値は**マシンに依存しない緩い下限/上限**にしてある(CI のランナーは
# 手元の Mac より遅いことがある)。10倍級の回帰を捕まえるのが目的で、
# 数十%の比較をしたい場合は同じマシンで前後を測ること(改善ループの規律)。
set -uo pipefail
cd "$(dirname "$0")/.."

CHECK=0
[ "${1:-}" = "--check" ] && CHECK=1
FAIL=0

say() { printf '\n== %s ==\n' "$1"; }

# 数値を1つ抜き出す(grep の1行目から正規表現で取り出す)
pick() { # pick <text> <regex>
  printf '%s\n' "$1" | grep -oE "$2" | head -1
}

check_min() { # check_min <name> <value> <min>
  local name=$1 value=$2 min=$3
  if [ -z "$value" ]; then
    echo "  [warn] $name: 数値を読めませんでした(計測器の出力形式を確認)"
    return
  fi
  if awk "BEGIN{exit !($value < $min)}"; then
    echo "  [FAIL] $name = $value (下限 $min を下回った)"
    FAIL=1
  else
    echo "  [ok]   $name = $value (下限 $min)"
  fi
}

check_max() { # check_max <name> <value> <max>
  local name=$1 value=$2 max=$3
  if [ -z "$value" ]; then
    echo "  [warn] $name: 数値を読めませんでした(計測器の出力形式を確認)"
    return
  fi
  if awk "BEGIN{exit !($value > $max)}"; then
    echo "  [FAIL] $name = $value (上限 $max を超えた)"
    FAIL=1
  else
    echo "  [ok]   $name = $value (上限 $max)"
  fi
}

say "proto エンコード/デコード"
OUT=$(cargo run --release --quiet --example perf-input 2>&1 | tail -5)
printf '%s\n' "$OUT"
ENC=$(pick "$OUT" 'encode: p50=[0-9]+' | grep -oE '[0-9]+$')
DEC=$(pick "$OUT" 'decode: p50=[0-9]+' | grep -oE '[0-9]+$')
if [ "$CHECK" = 1 ]; then
  check_max "encode p50(ns)" "$ENC" 5000
  check_max "decode p50(ns)" "$DEC" 5000
fi

say "bulk 転送(loopback 64MB)"
OUT=$(cargo run --release --quiet --example perf-bulk 2>&1 | tail -5)
printf '%s\n' "$OUT"
MBPS=$(pick "$OUT" '\[256KB\].*=> [0-9.]+ MB/s' | grep -oE '[0-9.]+ MB/s' | grep -oE '^[0-9.]+')
if [ "$CHECK" = 1 ]; then
  check_min "転送 256KB(MB/s)" "$MBPS" 30
fi

say "小ファイル多数(4KB)"
OUT=$(cargo run --release --quiet --example perf-smallfiles 2>&1 | tail -5)
printf '%s\n' "$OUT"
FPS=$(pick "$OUT" 'files/s=[0-9]+' | grep -oE '[0-9]+$')
if [ "$CHECK" = 1 ]; then
  check_min "files/s" "$FPS" 1000
fi

say "履歴の保存 I/O(50件×8KB)"
OUT=$(cargo run --release --quiet --example perf-history 2>&1 | grep -v '^\[history\]' | tail -6)
printf '%s\n' "$OUT"
TOB=$(pick "$OUT" 'to_bytes\(ロック保持分\): p50=[0-9]+' | grep -oE '[0-9]+$')
SAVE=$(pick "$OUT" 'save_to: p50=[0-9]+' | grep -oE '[0-9]+$')
if [ "$CHECK" = 1 ]; then
  check_max "to_bytes p50(ns)" "$TOB" 5000000
  check_max "save_to p50(ns)" "$SAVE" 20000000
fi

say "画像経路(4K 1枚の分解計測)"
OUT=$(cargo test -p knit-mac --release perf_ -- --nocapture 2>&1 | grep '^\[perf\]' | head -8)
printf '%s\n' "$OUT"
PNG=$(pick "$OUT" 'B dib_to_png 4K: p50=[0-9]+' | grep -oE '[0-9]+$')
DISK=$(pick "$OUT" 'D write bmp 33MB to disk: p50=[0-9]+' | grep -oE '[0-9]+$')
if [ "$CHECK" = 1 ]; then
  # 単位は µs
  check_max "PNG 全体 p50(µs)" "$PNG" 2000000
  check_max "33MB 書き込み p50(µs)" "$DISK" 3000000
fi

if [ "$CHECK" = 1 ]; then
  say "判定"
  if [ "$FAIL" = 1 ]; then
    echo "壊滅的な回帰を検出しました(上の FAIL 行を参照)"
    exit 1
  fi
  echo "回帰なし(緩い下限/上限の範囲内)"
fi

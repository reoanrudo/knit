#!/bin/bash
# Knit の接続安定性を実測値として集計する。
# 検証計画(docs/plans/2026-10-01-validation-plan.md)の検証2「壊れない接続」の
# 定量に使う。Knit が記録する [conn-metric] 行を集計する。
#
# 使い方: scripts/stability-report.sh [ログファイル]
#   既定は /tmp/knit-mac.log(Mac 側)。Windows 側のログも同じ形式なら渡せる。
#   計測行はこのリポジトリのビルドで記録される(v0.27 以降の [conn-metric] 行)。
#   過去のログに計測行が無い場合は「計測行がありません」と出て何もしない。
set -euo pipefail
LOG="${1:-/tmp/knit-mac.log}"
if [ ! -f "$LOG" ]; then
  echo "[stability-report] ログがありません: $LOG" >&2
  exit 1
fi
echo "[stability-report] 対象ログ: $LOG"

awk '
# 集計本体。BSD awk(macOS 標準)で動く書き方にしている(gawk の match 第3引数は使わない)
{
  if (match($0, /connected unix_ms=[0-9]+ gap_ms=[0-9]+/)) {
    line = substr($0, RSTART, RLENGTH)
    gsub(/[^0-9 ]/, "", line)
    split(line, a, " ")
    # a[1]=unix_ms, a[2]=gap_ms。行頭の [conn-metric] の数字を除外するため
    # 末尾2個を取る: NF は数字列のトークン数で split 後の a に依存するため、
    # unix_ms と gap_ms は末尾の2個と決め打ちできる(ログ行に他の数値を含まないため)
    ms  = a[1]
    gap = a[2]
    n++
    conn_ms[n] = ms
    gap_ms[n] = gap
    if (n == 1) first = ms
    if (ms > last) last = ms
    # wake 行(スリープ復帰検知)以降の最初の再接続までの所要時間
    if (pending_wake && ms > wake_ms) {
      wakes++
      d = ms - wake_ms
      wake_sum += d
      if (d > wake_max) wake_max = d
      pending_wake = 0
    }
  } else if (match($0, /lost unix_ms=[0-9]+/)) {
    losts++
  } else if (match($0, /wake unix_ms=[0-9]+ slept_s=[0-9]+/)) {
    line = substr($0, RSTART, RLENGTH)
    gsub(/[^0-9 ]/, "", line)
    split(line, a, " ")
    # a[1]=unix_ms(復帰検知時刻), a[2]=slept_s(眠っていた秒数)
    wake_ms = a[1]
    pending_wake = 1
  }
}
END {
  if (n == 0) {
    print "[stability-report] 計測行([conn-metric])がありません。"
    print "  このリポジトリのビルドで Knit を運用したログを渡してください。"
    exit 0
  }
  days = (last - first) / 86400000
  if (days >= 1) {
    printf "観測期間: %.1f 日 (unix_ms %d 〜 %d)\n", days, first, last
  } else {
    printf "観測期間: %.1f 時間 (unix_ms %d 〜 %d)\n", days * 24, first, last
  }
  printf "接続確立: %d 回 / 断: %d 回\n", n, losts

  # gap_ms の統計(断なし初回接続の gap_ms=0 は除外)
  sum = 0; count = 0; max = 0
  over60 = 0; over10 = 0; under10 = 0
  for (i = 1; i <= n; i++) {
    g = gap_ms[i]
    if (g == 0) continue
    sum += g
    count++
    if (g > max) max = g
    if (g > 60000) { over60++ }
    else if (g > 10000) { over10++ }
    else { under10++ }
  }
  if (count == 0) {
    print "断: なし(観測期間中、接続は途切れませんでした)"
  } else {
    avg = sum / count
    printf "断の合計: %.1f 分 / 最大断: %.1f 秒 / 断の平均: %.1f 秒\n", sum/60000, max/1000, avg/1000
    printf "断の内訳: 10秒以下 %d 回 / 10秒超〜60秒以下 %d 回 / 60秒超 %d 回\n", under10, over10, over60
    if (days > 0.0001) {
      printf "接続率: %.2f %% (1 - 断の合計 / 観測期間)\n", (1 - sum / ((last - first) + sum)) * 100
      # 重み: 断を含む観測期間で割る(断の合計を加算して接続時間を復元)
      printf "1日あたりの断: %.2f 回\n", losts / days
    }
  }
  if (days > 0.01) {
    printf "1日あたりの再接続(connected行): %.2f 回\n", n / days
  } else {
    print "(観測期間が 14 分未満のため 1日あたりの指標は省略)"
  }

  # スリープ復帰検知([conn-metric] wake)→再接続確立(connected)の所要時間
  if (wakes > 0) {
    printf "スリープ復帰→再接続: %d 回 / 平均 %.1f 秒 / 最大 %.1f 秒\n", wakes, (wake_sum / wakes) / 1000, wake_max / 1000
  } else {
    print "スリープ復帰→再接続: 該当なし(観測期間中に wake 行がありません)"
  }
}
' "$LOG"

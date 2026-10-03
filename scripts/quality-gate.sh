#!/bin/bash
# 品質ゲート: コミット前の機械判定(deterministic)と水増し検出(anti-gaming)
#
# 使い方:
#   ./scripts/quality-gate.sh                       # 未コミット差分(tracked)を対象
#   ./scripts/quality-gate.sh HEAD~3..HEAD          # コミット範囲を対象(朝の監査用)
#   ./scripts/quality-gate.sh --with-win [範囲]     # win 差分が無くてもクロスチェック
#   ./scripts/quality-gate.sh --no-win [範囲]       # クロスチェックを省略
#   ./scripts/quality-gate.sh --allow-assert-removal [範囲]  # アサーション削除を容認(要審査)
#
# ゲートの内容:
#   G1 test   cargo test --locked --workspace --exclude knit-win (ビルドを含む)
#   G2 win    crates/win に .rs 差分があるとき cargo check --target x86_64-pc-windows-gnu --tests
#   G3 gaming diff の水増し検出(実装:テスト比率 / 削除アサーション / テストのみ変更)
#
# 判定:
#   FAIL = G1/G2 の失敗、またはテストコードの assert/#[test] 削除(--allow-assert-removal で容認)
#   WARN = .rs 変更がテストのみ(実装変更ゼロ)。ゲートは通すが記録に残る
#          (2026-09-29 夜の実測: テストのみのコミット 81 件/時で実装改善ゼロ、が動機)
#   exit 0 = PASS / exit 1 = FAIL
#
# 注意:
# - untracked ファイルは G3 の diff 解析対象外(先に git add する。実行可否は G1 が拾う)
# - G3 の #[cfg(test)] 位置は作業ツリー基準の近似(コミット範囲監査では数行ずれ得る)
# - 本体で cargo check --workspace そのものは android-bridge の SDK 要求で失敗するため実行しない
set -u
cd "$(dirname "$0")/.."

RANGE=""
WITH_WIN=0
NO_WIN=0
ALLOW_ASSERT_REMOVAL=0
for arg in "$@"; do
  case "$arg" in
    --with-win) WITH_WIN=1 ;;
    --no-win) NO_WIN=1 ;;
    --allow-assert-removal) ALLOW_ASSERT_REMOVAL=1 ;;
    *) RANGE="$arg" ;;
  esac
done

pass=0; fail=0; warn=0
mark() { # mark PASS|FAIL|WARN|SKIP "名前" "詳細"
  case "$1" in
    PASS) echo "  OK   $2 ${3:+($3)}"; pass=$((pass+1)) ;;
    FAIL) echo "  NG   $2 ${3:+($3)}"; fail=$((fail+1)) ;;
    WARN) echo "  WARN $2 ${3:+($3)}"; warn=$((warn+1)) ;;
    SKIP) echo "  --   $2 ${3:+($3)}" ;;
  esac
}

echo "[gate] 対象差分: ${RANGE:-未コミット(tracked)}"

# --- G2 の要否判定 ---
WIN_TOUCHED=0
if git diff ${RANGE:-HEAD} --name-only 2>/dev/null | grep -q '^crates/win/.*\.rs$'; then
  WIN_TOUCHED=1
fi

# --- G1: テスト(win 以外の workspace。ビルドを含む) ---
echo "[gate] G1 cargo test (workspace, win 除く):"
if cargo test --locked --workspace --exclude knit-win >/tmp/knit-gate-test.log 2>&1; then
  RESULTS=$(grep '^test result:' /tmp/knit-gate-test.log | awk '{gsub(/[^0-9]/,"",$4); s+=$4} END {print s+0}')
  FAILED=$(grep '^test result:' /tmp/knit-gate-test.log | awk '{gsub(/[^0-9]/,"",$6); s+=$6} END {print s+0}')
  mark PASS "G1 テスト" "passed=$RESULTS failed=$FAILED"
else
  tail -30 /tmp/knit-gate-test.log
  mark FAIL "G1 テスト" "cargo test 失敗(上のログ参照)"
fi

# --- G2: Windows クロスチェック(win 差分があるとき。homebrew rust には対象が無いため rustup を優先) ---
echo "[gate] G2 Windows クロスチェック:"
if [ "$NO_WIN" -eq 1 ]; then
  mark SKIP "G2 win check" "--no-win 指定"
elif [ "$WIN_TOUCHED" -eq 0 ] && [ "$WITH_WIN" -eq 0 ]; then
  mark SKIP "G2 win check" "crates/win の差分なし"
else
  if PATH="$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$PATH" \
     cargo check --locked -p knit-win --target x86_64-pc-windows-gnu --tests >/tmp/knit-gate-win.log 2>&1; then
    mark PASS "G2 win check" "x86_64-pc-windows-gnu --tests"
  else
    tail -20 /tmp/knit-gate-win.log
    mark FAIL "G2 win check" "クロスチェック失敗(上のログ参照)"
  fi
fi

# --- G3: 水増し検出(anti-gaming) ---
# 実測(2026-09-29):「テストが全緑」だけでは現状固定テストの量産を検出できない。
# 実装:テストの追加行比率と、テストコードからの assert / #[test] 削除を機械的に数える。
echo "[gate] G3 水増し検出(anti-gaming):"
G3_OUT=$(python3 - "$RANGE" "$ALLOW_ASSERT_REMOVAL" <<'PYEOF'
import re, subprocess, sys

rng = sys.argv[1] if sys.argv[1] else "HEAD"
allow_removal = sys.argv[2] == "1"
diff = subprocess.run(["git", "diff", rng, "--", "crates"],
                       capture_output=True, text=True).stdout

impl_add = impl_del = test_add = test_del = 0
removed_asserts = 0
state = {}   # path -> cfg(test) の行番号(None=無し)
pos = {}     # path -> hunk 内の新側行番号
cur = None

for line in diff.splitlines():
    m = re.match(r'^\+\+\+ b/(.*)$', line)
    if m:
        cur = m.group(1) if m.group(1).endswith(".rs") else None
        continue
    if cur is None:
        continue
    if cur not in state:
        try:
            with open(cur, encoding="utf-8", errors="replace") as f:
                lines = f.read().splitlines()
            state[cur] = next((i for i, l in enumerate(lines, 1) if "#[cfg(test)]" in l), None)
        except OSError:
            state[cur] = None
    m = re.match(r'^@@ -\d+(?:,\d+)? \+(\d+)(?:,\d+)? @@', line)
    if m:
        pos[cur] = int(m.group(1))
        continue
    if cur not in pos:
        continue
    n = pos[cur]
    in_test = "/tests/" in cur or (state[cur] is not None and n >= state[cur])
    if line.startswith("+") and not line.startswith("+++"):
        if in_test:
            test_add += 1
        else:
            impl_add += 1
        pos[cur] = n + 1
    elif line.startswith("-") and not line.startswith("---"):
        if in_test:
            test_del += 1
            if re.search(r'assert|#\[test\]', line):
                removed_asserts += 1
        else:
            impl_del += 1
        # 削除行は新側の行番号を進めない

total_add = impl_add + test_add
ratio = f"、テスト比率 {test_add*100//max(total_add,1)}%" if total_add else ""
print(f"       実装 +{impl_add}/-{impl_del} 行、テスト +{test_add}/-{test_del} 行{ratio}")
print(f"       削除されたアサーション/テスト属性: {removed_asserts}")

if removed_asserts > 0 and not allow_removal:
    print("VERDICT:FAIL 既存テストのアサーション削除を検出(--allow-assert-removal で容認可。正当な削除なら理由を記録)")
elif impl_add == 0 and impl_del == 0 and (test_add > 0 or test_del > 0):
    print("VERDICT:WARN テストのみの変更(実装変更ゼロ)。正当な境界テストもあるが、連続した場合は水増しを疑う")
elif total_add == 0 and impl_del == 0:
    print("VERDICT:INFO .rs の変更なし(ドキュメント等のみ)")
else:
    print("VERDICT:PASS")
PYEOF
)
echo "$G3_OUT"
verdict=$(printf '%s\n' "$G3_OUT" | grep -o 'VERDICT:[A-Z]*' | cut -d: -f2 | tail -1)
case "$verdict" in
  PASS) mark PASS "G3 水増し検出" ;;
  INFO) mark SKIP "G3 水増し検出" ".rs 変更なし" ;;
  WARN) mark WARN "G3 水増し検出" "テストのみ変更(記録対象)" ;;
  FAIL) mark FAIL "G3 水増し検出" ;;
  *)    mark FAIL "G3 水増し検出" "判定不能(verdict=$verdict)" ;;
esac

echo "[gate] 完了: $(date '+%Y-%m-%d %H:%M:%S') pass=$pass fail=$fail warn=$warn"
[ "$fail" -eq 0 ]

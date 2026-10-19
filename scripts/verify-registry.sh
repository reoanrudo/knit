#!/usr/bin/env bash
# 端末モデル(登録台帳)の CLI e2e。HOME を一時ディレクトリへ向けて実データを触らない。
# CI(macos)と手元の両方で同じコマンドを回せる:
#   bash scripts/verify-registry.sh [knit-mac のパス]
# 検証: list(空) → 偽端末を台帳へ → list(1台) → revoke → list(0台) →
# revoke-legacy → 台帳の legacy_revoked が立つ → prune(0台) → peer-sides 不変。
set -euo pipefail

BIN=${1:-target/debug/knit-mac}
if [ ! -x "$BIN" ]; then
  cargo build -q -p knit-mac
fi
[ -x "$BIN" ] || { echo "FAIL: $BIN が見つかりません"; exit 1; }

TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
export HOME="$TMP"
# キーチェーンを使わずに master を渡す(CI ランナーはキーチェーンが空)
export KNIT_TOKEN="$(printf 'ab%.0s' $(seq 1 32))"
DATA="$HOME/Library/Application Support/Knit"
CONF="$HOME/.config/knit"
mkdir -p "$DATA" "$CONF"

# 配置設定の種を置き、実行前後で不変なことを確かめる(CLI が空で上書きしない)
printf '[]' > "$CONF/peer-sides.json"
before=$(shasum -a 256 "$CONF/peer-sides.json" | awk '{print $1}')

step() { echo "--- $*"; }
expect() { grep -q -- "$1" <<<"$2" || { echo "FAIL: '$1' が出力に無い: $2"; exit 1; }; }

step "list(空)"
out=$("$BIN" --registry list); echo "$out"
expect "登録済みの端末: ありません" "$out"
expect "旧形式(共通キー)での接続: 受け付けています" "$out"

step "偽端末を台帳へ(テスト用の固定 key_id)"
cat > "$DATA/devices.json" <<'JSON'
{"v":1,"legacy_revoked":false,"devices":[{"key_id":"0123456789abcdef0123456789abcdef","name":"E2E端末","fp":"00000000","added_at":1}]}
JSON
out=$("$BIN" --registry list); echo "$out"
expect "登録済みの端末: 1台" "$out"
expect "E2E端末" "$out"

step "revoke(別プロセスで落ちないこと)"
out=$("$BIN" --registry revoke 0123456789abcdef0123456789abcdef); echo "$out"
expect "受け付けなくしました" "$out"
out=$("$BIN" --registry list); echo "$out"
expect "登録済みの端末: ありません" "$out"

step "revoke-legacy"
out=$("$BIN" --registry revoke-legacy); echo "$out"
expect "受け付けなくなりました" "$out"
out=$("$BIN" --registry list); echo "$out"
expect "旧形式(共通キー)での接続: 拒否中" "$out"

step "冪等(2回目の revoke-legacy)"
out=$("$BIN" --registry revoke-legacy); echo "$out"
expect "受け付けなくなりました" "$out"

step "prune(空)"
out=$("$BIN" --registry prune); echo "$out"
expect "ありません" "$out"

after=$(shasum -a 256 "$CONF/peer-sides.json" | awk '{print $1}')
[ "$before" = "$after" ] || { echo "FAIL: peer-sides.json が CLI に書き換えられた"; exit 1; }

echo "OK: registry e2e passed"

#!/bin/bash
# 自動更新の通し試験(一時フォルダのみ使用。インストール済みの Knit には触れない)。
# 署名付き更新情報 → 取得 → 検証 → 入れ替え、および改変された更新情報の拒否を確かめる。
# 前提: dist/Knit.app(scripts/package-mac.sh で作成)
set -euo pipefail
cd "$(dirname "$0")/../.."
[ -d dist/Knit.app ] || { echo "dist/Knit.app がありません。先に scripts/package-mac.sh を実行してください" >&2; exit 1; }
export PATH="$HOME/.cargo/bin:$PATH"
cargo build -q -p knit-mac -p knit-common --bin knit-mac --bin knit-sign
TMP="$(mktemp -d)"
SRV_PID=""
cleanup() { [ -n "$SRV_PID" ] && kill "$SRV_PID" 2>/dev/null || true; pkill -f -- "$TMP" 2>/dev/null || true; rm -rf "$TMP"; }
trap cleanup EXIT
SIGN=target/debug/knit-sign
PORT=$((20000 + RANDOM % 20000))
mkdir -p "$TMP/app" "$TMP/srv" "$TMP/new"

# 現行版(0.26.0 の .app に、デバッグビルドの実行ファイルを入れる)
cp -R dist/Knit.app "$TMP/app/Knit.app"
cp target/debug/knit-mac "$TMP/app/Knit.app/Contents/MacOS/Knit"
codesign --force --sign - "$TMP/app/Knit.app" 2>/dev/null

# 新版(0.27.0。実行ファイルは起動し続けるだけの偽物)。
# zip の構成は scripts/package-mac.sh と同じ「版付きフォルダ/Knit.app + release-manifest.json」にする
# (構成が違うと本番の更新だけが失敗する。手作りの構成では見逃す)
PKG="$TMP/new/Knit-0.27.0"
mkdir -p "$PKG"
cp -R dist/Knit.app "$PKG/Knit.app"
plutil -replace CFBundleShortVersionString -string 0.27.0 "$PKG/Knit.app/Contents/Info.plist"
printf '#!/bin/bash\nsleep 120\n' > "$PKG/Knit.app/Contents/MacOS/Knit"
chmod +x "$PKG/Knit.app/Contents/MacOS/Knit"
codesign --force --sign - "$PKG/Knit.app" 2>/dev/null
printf '{"schema_version":1,"version":"0.27.0","platform":"macos-arm64"}\n' > "$PKG/release-manifest.json"
printf 'README\n' > "$PKG/README-Mac.txt"
ditto -c -k --keepParent "$PKG" "$TMP/srv/Knit.zip"
LISTING=$(unzip -Z1 "$TMP/srv/Knit.zip")
case "$LISTING" in *"Knit-0.27.0/Knit.app/"*) ;; *) echo "NG: zip の構成が想定と違う" >&2; exit 1 ;; esac

PUB=$($SIGN keygen "$TMP/key" 2>/dev/null)
$SIGN manifest 0.27.0 stable "macos-arm64=$TMP/srv/Knit.zip=http://127.0.0.1:$PORT/Knit.zip" > "$TMP/srv/update.json"
$SIGN sign "$TMP/key" "$TMP/srv/update.json" > "$TMP/srv/update.json.sig"
(cd "$TMP/srv" && exec python3 -m http.server "$PORT" --bind 127.0.0.1 >/dev/null 2>&1) &
SRV_PID=$!
sleep 1

run() {
  KNIT_UPDATE_URL="http://127.0.0.1:$PORT/update.json" KNIT_UPDATE_PUBKEY="$PUB" KNIT_UPDATE_START_MODE=exec \
    "$TMP/app/Knit.app/Contents/MacOS/Knit" --update
}
version() { plutil -extract CFBundleShortVersionString raw -o - "$TMP/app/Knit.app/Contents/Info.plist"; }

echo "== 改変された更新情報は拒否される"
cp "$TMP/srv/update.json" "$TMP/srv/update.json.orig"
sed 's/0.27.0/9.9.9/' "$TMP/srv/update.json.orig" > "$TMP/srv/update.json"
if run; then echo "NG: 改変を受理した" >&2; exit 1; fi
[ "$(version)" = "0.26.0" ] || { echo "NG: 拒否したのに版が変わった" >&2; exit 1; }
cp "$TMP/srv/update.json.orig" "$TMP/srv/update.json"

echo "== 正しい更新情報で更新される"
run
for _ in $(seq 1 30); do [ "$(version)" = "0.27.0" ] && break; sleep 0.5; done
[ "$(version)" = "0.27.0" ] || { echo "NG: 版が 0.27.0 になっていない($(version))" >&2; exit 1; }
sleep 7  # 起動確認と後片付けの完了を待つ
[ ! -e "$TMP/app/Knit.app.knit-old" ] || { echo "NG: 旧版が残っている" >&2; exit 1; }
ls -A "$TMP/app" | grep -q '^\.knit-update-' && { echo "NG: 作業フォルダが残っている" >&2; exit 1; }
echo "OK: 更新の通し試験に成功しました"

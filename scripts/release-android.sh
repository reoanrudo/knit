#!/bin/bash
# Android アプリの配布物(署名済み APK と、署名付き更新情報)を dist/android-release/ に作る。
# 公開はこのスクリプトの後に手で行う(下の案内を参照)。
#   KNIT_ANDROID_KEYSTORE        配布用の署名鍵(keystore)のパス
#   KNIT_ANDROID_STORE_PASSWORD  keystore のパスワード(KNIT_ANDROID_KEY_PASSWORD / KNIT_ANDROID_KEY_ALIAS は任意)
#   KNIT_UPDATE_KEY              更新情報の署名鍵(`knit-sign keygen` で作った秘密鍵ファイル)
set -euo pipefail
cd "$(dirname "$0")/.."
export PATH="$HOME/.cargo/bin:$PATH"
: "${KNIT_ANDROID_KEYSTORE:?配布用の署名鍵(KNIT_ANDROID_KEYSTORE)を指定してください}"
: "${KNIT_ANDROID_STORE_PASSWORD:?KNIT_ANDROID_STORE_PASSWORD を指定してください}"
: "${KNIT_UPDATE_KEY:?更新情報の署名鍵(KNIT_UPDATE_KEY)を指定してください}"
TAG_REPO="${KNIT_REPO:-reoanrudo/knit}"
VER=$(sed -n "s/^ *versionName '\(.*\)'/\1/p" android/app/build.gradle | head -1)
[ -n "$VER" ] || { echo '[release-android] versionName が取得できません' >&2; exit 1; }
OUT=dist/android-release
rm -rf "$OUT"; mkdir -p "$OUT"
./scripts/build-android.sh assembleRelease
APK_SRC=android/app/build/outputs/apk/release/app-release.apk
[ -f "$APK_SRC" ] || { echo '[release-android] 署名済み APK が作られていません' >&2; exit 1; }
APK="$OUT/Knit-Android-$VER.apk"
cp "$APK_SRC" "$APK"
cargo build -q -p knit-common --bin knit-sign
SIGN=target/debug/knit-sign
URL="https://github.com/$TAG_REPO/releases/download/android-latest/Knit-Android-$VER.apk"
$SIGN manifest "$VER" stable "android-arm64=$APK=$URL" > "$OUT/update-android.json"
$SIGN sign "$KNIT_UPDATE_KEY" "$OUT/update-android.json" > "$OUT/update-android.json.sig"
(cd "$OUT" && shasum -a 256 "Knit-Android-$VER.apk" > "Knit-Android-$VER.apk.sha256")
echo "[release-android] 完了: $OUT"
echo "公開: gh release view android-latest -R $TAG_REPO >/dev/null 2>&1 || gh release create android-latest -R $TAG_REPO --prerelease --title 'Knit for Android' --notes 'Android アプリの更新配信用'"
echo "      gh release upload android-latest -R $TAG_REPO --clobber $OUT/*"

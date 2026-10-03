#!/bin/bash
# クリップボード画像設定ヘルパー(ClipSetter)を Android 用の dex(jar 形式)へ
# ビルドする。成果物は crates/mac/src/android/clip-setter.jar へ置き、Rust 側は
# include_bytes! で組込む(端末へ送り app_process で起動。apk インストール不要)。
# 要: Android SDK(platforms + build-tools/d8)と JDK。環境を変えたら再実行する
set -euo pipefail
cd "$(dirname "$0")/.."
SDK="${ANDROID_SDK_ROOT:-$HOME/Library/Android/sdk}"
PLATFORM=$(ls "$SDK/platforms" | sort -V | tail -1)
BT=$(ls "$SDK/build-tools" | sort -V | tail -1)
AJAR="$SDK/platforms/$PLATFORM/android.jar"
D8="$SDK/build-tools/$BT/d8"
[ -f "$AJAR" ] || { echo "[build-clip-helper] android.jar がありません: $AJAR" >&2; exit 1; }
[ -x "$D8" ] || { echo "[build-clip-helper] d8 がありません: $D8" >&2; exit 1; }
OUT=$(mktemp -d)
trap 'rm -rf "$OUT"' EXIT
javac -nowarn -Xlint:none -source 8 -target 8 -bootclasspath "$AJAR" \
  -d "$OUT" crates/mac/src/android/ClipSetter.java
"$D8" --release --lib "$AJAR" --output "$OUT" "$OUT"/ClipSetter*.class
(cd "$OUT" && zip -q -X -r knit-clip.jar classes.dex)
cp "$OUT/knit-clip.jar" crates/mac/src/android/clip-setter.jar
echo "[build-clip-helper] 完了: crates/mac/src/android/clip-setter.jar ($(stat -f%z crates/mac/src/android/clip-setter.jar) bytes)"

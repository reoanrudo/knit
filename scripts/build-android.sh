#!/usr/bin/env bash
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd)
cd "$ROOT"
source "$HOME/.cargo/env"
SDK="${ANDROID_SDK_ROOT:-${ANDROID_HOME:-/opt/homebrew/share/android-commandlinetools}}"
NDK="${ANDROID_NDK_HOME:-$SDK/ndk/28.2.13676358}"
case "$(uname -s)" in
    Darwin) NDK_HOST=darwin-x86_64 ;;
    Linux) NDK_HOST=linux-x86_64 ;;
    *) echo 'このスクリプトはMac・Linux向けです。' >&2; exit 1 ;;
esac
TOOLCHAIN="$NDK/toolchains/llvm/prebuilt/$NDK_HOST/bin"
export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$TOOLCHAIN/aarch64-linux-android29-clang"
export CC_aarch64_linux_android="$CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER"
export AR_aarch64_linux_android="$TOOLCHAIN/llvm-ar"
if [ ! -s android/app/src/main/jniLibs/arm64-v8a/libmozc.so ] || [ ! -s android/app/src/main/assets/mozc.data ] || [ "$(cat android/app/src/main/assets/mozc-revision.txt 2>/dev/null || true)" != a069a88d4cb5c011de0f9aebb6c149a1c808d904 ]; then
    scripts/build-mozc-android.sh
fi
rustup target add aarch64-linux-android
cargo build --locked --release -p knit-android-bridge --target aarch64-linux-android
mkdir -p android/app/src/main/jniLibs/arm64-v8a
"$CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER" -shared -fPIC -Wl,-z,max-page-size=16384 \
    android/app/src/main/cpp/bridge.c target/aarch64-linux-android/release/libknit_android_bridge.a \
    -ldl -llog -lm -o android/app/src/main/jniLibs/arm64-v8a/libknit_android.so
export ANDROID_HOME="$SDK"
export JAVA_HOME="${KNIT_ANDROID_JAVA_HOME:-${JAVA_HOME:-/opt/homebrew/opt/openjdk@17/libexec/openjdk.jdk/Contents/Home}}"
android/gradlew -p android --no-daemon testDebugUnitTest assembleDebug "$@"
mkdir -p dist
# 配布 APK は開発用署名(ビルド環境ごとに異なる)。実運用では固定リリース鍵
# (build.gradle の KNIT_ANDROID_KEYSTORE)へ移行し、署名の一致する更新配布に切り替える
cp android/app/build/outputs/apk/debug/app-debug.apk dist/Knit-Android-preview.apk
echo "$ROOT/dist/Knit-Android-preview.apk"

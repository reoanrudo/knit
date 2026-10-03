#!/usr/bin/env bash
# Offline Japanese engine and dictionary. Build products stay out of Git.
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd)
REV=a069a88d4cb5c011de0f9aebb6c149a1c808d904
CACHE="${KNIT_MOZC_CACHE:-$ROOT/android/.mozc-cache}"
SRC="$CACHE/mozc"
SDK="${ANDROID_SDK_ROOT:-${ANDROID_HOME:-/opt/homebrew/share/android-commandlinetools}}"
NDK="${ANDROID_NDK_HOME:-$SDK/ndk/28.2.13676358}"
JOBS="${KNIT_BUILD_JOBS:-4}"
mkdir -p "$CACHE"
[ -f "$NDK/source.properties" ] || { echo 'Mozc: Android NDK が見つかりません。' >&2; exit 1; }
if [ -n "${BAZELISK:-}" ]; then BAZEL="$BAZELISK"
elif command -v bazelisk >/dev/null; then BAZEL=$(command -v bazelisk)
elif [ "$(uname -s)-$(uname -m)" = Darwin-arm64 ]; then
    BAZEL="$CACHE/bazelisk-v1.29.0"
    [ -f "$BAZEL" ] || curl --fail --location --retry 3 https://github.com/bazelbuild/bazelisk/releases/download/v1.29.0/bazelisk-darwin-arm64 -o "$BAZEL"
    python3 - "$BAZEL" <<'PY'
import hashlib,sys
assert hashlib.sha256(open(sys.argv[1],'rb').read()).hexdigest() == 'cee851f726789227d5561004e9904a52be45c3efb56f8b38b6993d6adbaa0409', 'Bazelisk checksum mismatch'
PY
    chmod +x "$BAZEL"
else echo 'Mozc: Bazelisk をインストールするか BAZELISK を指定してください。' >&2; exit 1
fi
if [ ! -d "$SRC/.git" ]; then
    git init -q "$SRC"
    git -C "$SRC" remote add origin https://github.com/google/mozc.git
    git -C "$SRC" fetch --depth 1 origin "$REV"
    git -C "$SRC" checkout -q --detach FETCH_HEAD
fi
[ "$(git -C "$SRC" rev-parse HEAD)" = "$REV" ] || { echo 'Mozc: キャッシュのソース版が異なります。別の KNIT_MOZC_CACHE を指定してください。' >&2; exit 1; }
python3 - "$SRC" "$REV" "$NDK" <<'PY'
from pathlib import Path
import subprocess,sys
src,rev,ndk=sys.argv[1:]
original=subprocess.check_output(['git','-C',src,'show',rev+':src/MODULE.bazel']).decode()
assert '$WORKSPACE_ROOT/third_party/ndk/android-ndk-r29' in original
Path(src,'src/MODULE.bazel').write_text(original.replace('$WORKSPACE_ROOT/third_party/ndk/android-ndk-r29',str(Path(ndk).resolve())))
PY
cd "$SRC/src"
COMMON=(--config release_build --jobs="$JOBS" --spawn_strategy=local)
HOST_OPTIONS=(--repo_env=BAZEL_USE_CPP_ONLY_TOOLCHAIN=1)
if [ "$(uname -s)" = Darwin ]; then
    MAC_SDK=$(xcrun --sdk macosx --show-sdk-path)
    CLANG=$(xcrun --find clang)
    HOST_OPTIONS+=(--repo_env="CC=$CLANG" --repo_env="BAZEL_CXXOPTS=-std=c++20:-isystem$MAC_SDK/usr/include/c++/v1:-isysroot/$MAC_SDK" --repo_env="BAZEL_CONLYOPTS=-isysroot/$MAC_SDK" --host_linkopt="-isysroot$MAC_SDK")
fi
BAZEL_USE_CPP_ONLY_TOOLCHAIN=1 "$BAZEL" build //android/jni:mozc.arm64 --config oss_android "${COMMON[@]}" "${HOST_OPTIONS[@]}"
LIB=$(BAZEL_USE_CPP_ONLY_TOOLCHAIN=1 "$BAZEL" cquery //android/jni:mozc.arm64 --config oss_android "${COMMON[@]}" "${HOST_OPTIONS[@]}" --output=files | tail -1)
[ -s "$LIB" ] || { echo 'Mozc: ネイティブライブラリの出力がありません。' >&2; exit 1; }
mkdir -p "$ROOT/android/app/src/main/jniLibs/arm64-v8a" "$ROOT/android/app/src/main/assets"
install -m 644 "$LIB" "$ROOT/android/app/src/main/jniLibs/arm64-v8a/libmozc.so"
EXTERNAL=$("$BAZEL" info output_base)/external
LICENSES="$ROOT/android/app/src/main/assets/licenses"
mkdir -p "$LICENSES"
cp "$SRC/LICENSE" "$LICENSES/Mozc.txt"
cp data/dictionary_oss/README.txt "$LICENSES/Mozc-dictionary.txt"
for DEP in abseil-cpp protobuf zlib; do cp "$EXTERNAL/$DEP+/LICENSE" "$LICENSES/$DEP.txt"; done
cp "$EXTERNAL/protobuf+/third_party/utf8_range/LICENSE" "$LICENSES/utf8-range.txt"
if [ "$(uname -s)" = Darwin ]; then
    # CLT's SDK has complete libc++ headers; its standalone header directory may not.
    # The isolated toolchain patch affects host dictionary generators only.
    APPLE="$CACHE/apple-support"
    rm -rf "$APPLE"
    cp -RL "$EXTERNAL/apple_support+" "$APPLE"
    python3 - "$APPLE" <<'PY'
from pathlib import Path
import sys
root=Path(sys.argv[1])/'crosstool'
p=root/'universal_exec_tool.bzl';s=p.read_text();needle='-std=c++17'
assert needle in s
p.write_text(s.replace(needle,needle+' -isystem "$$(xcrun --sdk macosx --show-sdk-path)/usr/include/c++/v1"'))
p=root/'cc_toolchain_config.bzl';s=p.read_text();needle='    target_os_version = xcode_config.minimum_os_for_platform_type(platform_type)'
assert needle in s
p.write_text(s.replace(needle,needle+'\n    if ctx.attr.cpu.startswith("darwin"):\n        target_os_version = "13.0"'))
PY
    CPLUS_INCLUDE_PATH="$MAC_SDK/usr/include/c++/v1" "$BAZEL" build //data_manager/oss:mozc_dataset_for_oss --config oss_macos "${COMMON[@]}" \
        --override_module="apple_support=$APPLE" --repo_env=BAZEL_USE_CPP_ONLY_TOOLCHAIN=0 \
        --repo_env="CPLUS_INCLUDE_PATH=$MAC_SDK/usr/include/c++/v1" --action_env="CPLUS_INCLUDE_PATH=$MAC_SDK/usr/include/c++/v1" --host_action_env="CPLUS_INCLUDE_PATH=$MAC_SDK/usr/include/c++/v1" \
        --copt=-faligned-allocation --host_copt=-faligned-allocation
else
    "$BAZEL" build //data_manager/oss:mozc_dataset_for_oss --config oss_linux "${COMMON[@]}"
fi
install -m 644 bazel-bin/data_manager/oss/mozc.data "$ROOT/android/app/src/main/assets/mozc.data"
printf '%s\n' "$REV" > "$ROOT/android/app/src/main/assets/mozc-revision.txt"
echo 'Mozc: オフライン日本語エンジンと辞書を生成しました。'

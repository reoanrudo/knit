#!/bin/bash
# アイコン素材の生成(icongen 実行 + iconutil による icns 化)。
# アセットはリポジトリにコミット済みのため、通常は再実行不要(デザイン変更時のみ)
set -e
cd "$(dirname "$0")/.."
source $HOME/.cargo/env 2>/dev/null || true

cargo run -q -p knit-mac --bin icongen
iconutil -c icns assets/AppIcon.iconset -o assets/AppIcon.icns
echo "[gen-icons] assets/AppIcon.icns を生成しました"
ls -la assets/AppIcon.icns win-dist/app.ico

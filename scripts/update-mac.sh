#!/bin/bash
# Knit を最新のソースから作り直して、この Mac へ入れ替える(アップデート)。
# 署名は package-mac.sh が自動で付ける。キーチェーンに Apple Development /
# Developer ID 証明書があれば安定した署名になり、更新しても macOS の
# アクセシビリティ許可が維持される(無い場合は権限の再許可が必要になる)
set -euo pipefail
cd "$(dirname "$0")/.."
./scripts/package-mac.sh
./scripts/install-mac.sh

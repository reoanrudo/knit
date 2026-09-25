#!/bin/bash
# Windows 配布用 zip を dist/ に作成する(exe + インストーラ + 説明書 + トークン)。
# トークン設定(~/.config/tsunagu/env)があれば .env として同梱する
# (同梱したくない場合は環境変数 NO_TOKEN=1 を付けて実行)
set -e
cd "$(dirname "$0")/.."
source $HOME/.cargo/env 2>/dev/null || true

VER=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
OUT="dist/Tsunagu-win-$VER"
ZIP="dist/Tsunagu-win-$VER.zip"

echo "[package-win] building..."
cargo build --release -p tsunagu-win --target x86_64-pc-windows-gnu 2>&1 | grep -E "^error" -A3 && exit 1 || true
cargo build --release -p tsunagu-win --target x86_64-pc-windows-gnu 2>&1 | tail -1 >/dev/null

rm -rf "$OUT" && mkdir -p "$OUT"
cp target/x86_64-pc-windows-gnu/release/tsunagu-win.exe "$OUT/"
cp win-dist/install.bat win-dist/uninstall.bat win-dist/run_tsunagu.bat win-dist/run_tsunagu.vbs "$OUT/"
cp win-dist/README-win.txt "$OUT/" 2>/dev/null || true
cp win-dist/app.ico "$OUT/" 2>/dev/null || true

if [ -z "$NO_TOKEN" ] && [ -f "$HOME/.config/tsunagu/env" ]; then
  cp "$HOME/.config/tsunagu/env" "$OUT/.env"
  chmod 600 "$OUT/.env"
  echo "[package-win] トークン設定を .env として同梱しました"
else
  echo "[package-win] WARN: .env は同梱していません(導入先で設定が必要)"
fi

rm -f "$ZIP"
ditto -c -k --keepParent "$OUT" "$ZIP"
rm -rf "$OUT"
echo "[package-win] 完了: $ZIP (v$VER)"

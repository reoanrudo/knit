#!/bin/bash
# 共有トークンを生成し ~/.config/knit/env へ保存する(初回セットアップ)。
# トークンは画面に出力しない(ファイル参照のみ)。既存がある場合は上書きしない
set -e
DIR="$HOME/.config/knit"
FILE="$DIR/env"
# v0.25 まで(旧名 Tsunagu)の設定フォルダがあれば丸ごと複製する(旧版へ戻せるよう元は残す。
# 旧キー TSUNAGU_TOKEN / SEAMLESS_DESK_TOKEN はアプリ側が読み替える)
if [ ! -e "$DIR" ] && [ -d "$HOME/.config/tsunagu" ]; then
  cp -Rp "$HOME/.config/tsunagu" "$DIR"
  echo "[gen-token] 旧設定(Tsunagu)を $DIR へ複製しました"
fi
# v0.7(seamless-desk)の旧設定があれば新パスへ移植する(トークン値は変わらない)
OLD_DIR="$HOME/.config/seamless-desk"
if [ ! -f "$FILE" ] && [ -f "$OLD_DIR/env" ] && grep -q -e '^KNIT_TOKEN=' -e '^SEAMLESS_DESK_TOKEN=' "$OLD_DIR/env"; then
  mkdir -p "$DIR"
  sed 's/^SEAMLESS_DESK_TOKEN=/KNIT_TOKEN=/' "$OLD_DIR/env" > "$FILE"
  chmod 700 "$DIR"; chmod 600 "$FILE"
  echo "[gen-token] 旧設定(seamless-desk)を $FILE へ移植しました"
  exit 0
fi
if [ -f "$FILE" ] && grep -q -e '^KNIT_TOKEN=' -e '^TSUNAGU_TOKEN=' -e '^SEAMLESS_DESK_TOKEN=' "$FILE"; then
  echo "[gen-token] 既存のトークンがあります: $FILE(再生成する場合はファイルを削除してから)"
  exit 0
fi
mkdir -p "$DIR"
chmod 700 "$DIR"
TOK=$(openssl rand -hex 32)
{
  echo "# knit 共有設定(この値を配布先の .env にも設定する)"
  echo "KNIT_TOKEN=$TOK"
} > "$FILE"
chmod 600 "$FILE"
echo "[gen-token] 生成しました: $FILE (256bit ランダム)"
echo "[gen-token] Windows 側へは scripts/deploy-win.sh が同じトークンを .env として配布します"
echo "[gen-token] Mac 側は ~/.config/knit/env を自動的に読みます(再起動で反映)"

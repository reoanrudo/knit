#!/bin/bash
# 共有トークンを生成し ~/.config/seamless-desk/env へ保存する(初回セットアップ)。
# トークンは画面に出力しない(ファイル参照のみ)。既存がある場合は上書きしない
set -e
DIR="$HOME/.config/seamless-desk"
FILE="$DIR/env"
if [ -f "$FILE" ] && grep -q '^SEAMLESS_DESK_TOKEN=' "$FILE"; then
  echo "[gen-token] 既存のトークンがあります: $FILE(再生成する場合はファイルを削除してから)"
  exit 0
fi
mkdir -p "$DIR"
chmod 700 "$DIR"
TOK=$(openssl rand -hex 32)
{
  echo "# seamless-desk 共有設定(この値を配布先の .env にも設定する)"
  echo "SEAMLESS_DESK_TOKEN=$TOK"
} > "$FILE"
chmod 600 "$FILE"
echo "[gen-token] 生成しました: $FILE (256bit ランダム)"
echo "[gen-token] Windows 側へは scripts/deploy-win.sh が同じトークンを .env として配布します"
echo "[gen-token] Mac 側は ~/.config/seamless-desk/env を自動的に読みます(再起動で反映)"

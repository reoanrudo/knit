# seamless-desk 操作ガイド

Mac のキーボード/トラックパッドで Windows デスクトップを操作するツール。
Mac=サーバ(Listen 24900)、Windows=クライアント(接続し続ける逆転構成)。

## 基本操作

| 操作 | 動作 |
|------|------|
| Mac のカーソルを画面右端へ | Windows へ切替(同寸法の高さで境界を越える) |
| Windows のカーソルを画面左端へ | Mac へ戻る(同じ高さで右端内側に復帰) |
| F13 キー | 手動トグル(切替が効かないときの保険) |

## キーボード

- Mac の修飾キーは自動変換: Cmd→Ctrl、Option→Alt、Control→Win、Shift→Shift
- **かなキー** → Windows 側の IME を ON(ひらがな入力)
- **英数キー** → Windows 側の IME を OFF(英字入力)
- 変換・確定(Enter/Space)はそのまま転送され Windows の IME が処理する

## クリップボード(双方向同期)

- コピーして約 0.5 秒で相手側に反映(プレーンテキスト、512KB まで)
- 画像や書式は未対応(今後の課題)

## 調整用環境変数(sd-mac 起動時)

| 変数 | 既定 | 説明 |
|------|------|------|
| `SEAMLESS_SCROLL_DIV` | 120 | スクロール速度の除数。大きくすると遅い(40〜200で調整) |
| `SEAMLESS_MOUSE_SCALE` | 1.0 | マウス移動の倍率。0.7 で遅く、1.5 で速く |
| `SEAMLESS_DESK_TOKEN` | seamless-desk-dev | 両側共通の認証トークン |

## 改善ループ(開発者用)

変更→検証の1サイクルを回す手順:

```bash
# Mac 側: ビルド鮮度保証付きで再起動(引数はそのまま sd-mac へ)
./scripts/restart-mac.sh --diag

# Windows 側: ビルド→停止→配布→起動
./scripts/deploy-win.sh

# 自動検証(プロセス/接続/クリップボード双方向)
./scripts/verify.sh
```

- `restart-mac.sh` は起動のたび BUILD_ID(日時+git短縮sha)を埋め込み、
  `/tmp/sd-mac-run.log` 先頭行で配布物の鮮度を確認できる
- `--diag` は毎秒 `mode/move_recv/key_recv/sent/warp_fixed/cursor/cursor_moving` をログ出力。
  境界問題の切り分けは `warp_fixed`(カーソル巻き戻し回数)と `cursor_moving`(WIN中は false が正常)で行う
- ログ: Mac=`/tmp/sd-mac-run.log`、Windows=`C:\Users\<user>\seamless-desk\sd-win.log`(ssh home で type)

## 既知の制限

- クリップボードはテキストのみ
- Windows 側のウィンドウ操作(Focus/Minimize)はタイトル部分一致
- Mac 側の IME 状態とは独立(かな/英数キーで Windows 側だけ切替)

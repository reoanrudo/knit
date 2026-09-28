# Tsunagu 紹介動画の収録手順（開発者用メモ）

## 設計方針

- 長さ: 約 25 秒（SNS は最初の 3 秒で掴む。GIF は無音前提）
- 音なしで収録する（ユーザー制約）。音声共有は「テキストで説明＋画面の
  ミュート表示」で表現する
- 出力: `docs/media/demo.gif`(幅 720・SNS 貼り付け用)と
  `docs/media/demo.mp4`(高画量・README/Zenn 用)の 2 形式
- 録画は macOS 標準の `screencapture -V`（区間録画）+ ffmpeg 変換

## シナリオ（タイムテーブル）

| 秒 | 見せ場 | 操作 |
|---|---|---|
| 0-4 | Mac でテキスト入力（日本語 IME が見える） | 「Tsunagu なら 1 台で済みます」等を入力 |
| 4-8 | カーソルを右端へ → Windows へ越境（画面が切り替わる瞬間） | 右端へ 2 回タップ |
| 8-13 | Windows 側で ⌘C のテキストを Ctrl+V | 越境直後に貼り付け（クリップボード同期） |
| 13-18 | Mac のファイルを ⌘C → Windows で Ctrl+V | 受信通知と Downloads\Tsunagu を示す |
| 18-23 | メニューバーを開く（接続先・履歴・遅延表示） | 接続先サブメニューまで展開 |
| 23-25 | 終了カット | ロゴ表示 or 終了 |

## 収録コマンド

```bash
# 1) 録画（全体画面・無音・H.264）。停止は Ctrl+C
ffmpeg -f avfoundation -capture_cursor 1 -capture_clicks 0 \
  -i "1:0" -c:v libx264 -preset veryfast -crf 23 -pix_fmt yuv420p docs/media/demo-raw.mp4

# 2) トリム（不要な前後を切る。開始秒・長さは収録後に決める）
ffmpeg -ss 2 -t 25 -i docs/media/demo-raw.mp4 -c copy docs/media/demo.mp4

# 3) GIF 化（720px 幅・パレット 2 段階で高品質化）
ffmpeg -i docs/media/demo.mp4 -vf "fps=12,scale=720:-1:flags=lanczos,split[a][b];[a]palettegen[p];[b][p]paletteuse" docs/media/demo.gif
```

## 見せ場を確実に撮るコツ

- 録画前に Windows 側のターミナル/エディタを開いておく（越境先が白紙だと分かりにくい）
- 越境の瞬間は画面全体が切り替わるため見栄えが良い。急がず 1 秒停止してから次へ
- ファイル受信はバルーン通知が出るが録画に映りにくい場合、受信フォルダを開くカットで代替
- 音声の紹介は動画末尾のテキストオーバーレイ（ffmpeg drawtext）で補足する

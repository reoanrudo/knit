# seamless-desk

Mac のキーボード/トラックパッドで、同一 WiFi 上の Windows デスクトップを
画面端のカーソル移動でシームレスに操作する入力共有ツール(Rust 実装)。

[Deskflow](https://github.com/deskflow/deskflow)(Synergy 系 OSS)を参考に、
「入力のフック → ネットワーク転送 → 注入」の中核を独自実装している。
Deskflow との違いは接続の**逆転構成**: Mac = サーバ(TCP 24900 で待ち受け)、
Windows = クライアント(接続し続ける)。これは本環境(Tailscale + AP 隔離)の
実測で Mac 発コネクションが不安定なための採用(詳細は [design.md](design.md))。

## 構成

| パス | 内容 |
|---|---|
| `crates/common` | 通信プロトコル(JSON Lines の `Msg` 列挙)、Mac keycode → Windows VK キーマップ |
| `crates/mac` | `sd-mac`: Mac 側サーバ。CGEventTap で入力を取得・抑制し Windows へ転送 |
| `crates/win` | `sd-win`: Windows 側クライアント。受信イベントを SendInput で注入 |
| `scripts/` | `restart-mac.sh` / `deploy-win.sh` / `verify.sh` / `check-mouse.sh` |
| `win-dist/` | Windows 配布物(`install.bat`・`run_sd.bat`)とデバッグ用補助スクリプト |
| `poc/` | 実現前の検証コード(参照用) |

## 仕組みの概要

- **Mac 側(sd-mac)**: CGEventTap(kCGHIDEventTap・抑制可)で物理入力を取得。
  Windows モード中はイベントを握りつぶし(return NULL)、キー/マウス/スクロールを
  JSON Lines で TCP 24900 へ転送する。アクセシビリティ権限が必要
- **Windows 側(sd-win)**: Mac に接続し続け、受信したイベントを SendInput で注入。
  注入には対話セッションでの起動が必須(SSH 直接実行では OpenInputDesktop に失敗)
- **切替**: Mac カーソルの右端到達で Windows へ、Windows カーソルの左端到達(または
  F13 キー)で Mac へ。復帰位置は同じ高さになるよう正規化座標で同期
- **維持監視**: ping 3 秒間隔・pong 10 秒無応答で切断扱い。Windows 側は
  0.5〜3 秒のバックオフで自動再接続する

## クイックスタート

前提: Mac 側は「システム設定 → プライバシーとセキュリティ → アクセシビリティ」権限済み。
Windows 側は初回のみ `ssh home "C:\Users\<user>\seamless-desk\install.bat"` で
スタートアップ登録(以後、開発時の配布は deploy-win.sh のみでよい)。

```bash
# 1) Mac 側: ビルド鮮度保証付きで再起動(--diag で毎秒診断ログを出す)
./scripts/restart-mac.sh --diag

# 2) Windows 側: クロスビルド → 停止 → 配布 → 対話セッションで起動
./scripts/deploy-win.sh

# 3) 自動検証: プロセス / 接続 / クリップボード双方向 / IMEログ / diag集計
./scripts/verify.sh
```

- 両スクリプトはビルド前に BUILD_ID(日時+git短縮sha)をソースへ埋め込む。
  `/tmp/sd-mac-run.log`(`sd-win.log`)の先頭行で動作中バイナリの鮮度を確認できる
- ログ: Mac = `/tmp/sd-mac-run.log`、Windows = `C:\Users\<user>\seamless-desk\sd-win.log`

操作方法・調整用環境変数・既知の制限は [usage.md](usage.md) 参照。

## ドキュメント

- [design.md](design.md) — 設計書(POC 実測、アーキテクチャ、プロトコル、切替ロジック、Windows 起動方式)
- [usage.md](usage.md) — 操作ガイド(基本操作、IME・マウス・スクロール・クリップボードの仕様、環境変数、既知の制限)
- [improvement-log.md](improvement-log.md) — 改善履歴(改善ループ 1〜43 の一覧)

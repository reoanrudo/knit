# seamless-desk

Deskflow 快適版。MacBook のキーボード/トラックパッドで Windows デスクトップを操作する。
**v0.6: 音声転送(Windows→Mac)・Windows ステータスウィンドウ・タスクトレイ常駐・接続方向の選択**

設計: [docs/design.md](docs/design.md) / 配布手順: [docs/distribution.md](docs/distribution.md)

## 構成

- **Mac = サーバ**(`sd-mac`): CGEventTap で入力をフックし TCP で転送。画面右端で切替。
  **メニューバー常駐 GUI**(アイコン+状態表示・手動切替・切替方式トグル・ログ/再起動/終了)付き
- **Windows = クライアント**(`sd-win`): 受信イベントを SendInput で注入。カーソル左端で復帰通知。
  **タスクトレイ常駐+ステータスウィンドウ**(左クリックで状態/音声/ログ/終了)+
  5分毎の自動復帰ウォッチ付き。**Windowsの音をMacで再生**(WASAPIループバック→24901→AudioQueue)
- 経路: Tailscale(Mac: 100.100.10.9 / Win: 100.84.0.2)、TCP 24900、**共有トークン認証(必須)**
- 接続方向は .env で選択可(既定: Mac=サーバ/Win=クライアント。逆方向も実機検証済み)

## セットアップ(初回)

```bash
# 1. 共有トークン生成(Mac で 1 回。~/.config/seamless-desk/env に保存される)
./scripts/gen-token.sh

# 2. Windows 側へ配布(.env に同じトークンが入り、スタートアップ登録+起動まで行う)
./scripts/deploy-win.sh

# 3. Mac 側を起動(開発運用: ビルド→再起動。メニューバーに常駐する)
./scripts/restart-mac.sh

# 4. 自動検証(7 項目)
./scripts/verify.sh
```

初回のみ Mac 側でアクセシビリティ権限の許可が必要(システム設定 > プライバシーとセキュリティ > アクセシビリティ)。

## 操作

- **Mac→Windows**: カーソルを画面右端へ(既定はダブルタップ。1回目は跳ね返るのでもう一度)
- **Windows→Mac**: Windows のカーソルを左端へ、または F13 キーでトグル
- **メニューバー(SD·Mac / SD·Win / SD·✕)**: 状態常時表示。クリックで手動切替・
  切替方式(境界+ダブルタップ ⇄ ホットキーロック)・境界到達回数(ダブルタップ ⇄ 1回)・
  ログを開く・再起動・終了
- **ショートカット**: Mac の Cmd は Windows の Ctrl に自動変換(Cmd+C→Ctrl+C)、
  かな/英数キーで Windows の IME を開閉

## アプリとしてのインストールと配布

```bash
./scripts/package-mac.sh   # dist/SeamlessDesk.app + zip を作成(トークン封入)
./scripts/install-mac.sh   # このMacへインストール(ログイン時自動起動の LaunchAgent 登録)
```

Windows への新規配布は `win-dist/`(sd-win.exe・install.bat・run_sd.vbs/bat・.env)一式を
コピーして install.bat を管理者実行。詳細は [docs/distribution.md](docs/distribution.md)。

## ビルド

```bash
cargo build --release                                            # Mac 側
cargo build --release -p sd-win --target x86_64-pc-windows-gnu   # Windows 側(クロス)
cargo test                                                       # テスト(default-members)
```

## Windows 側の更新手順(開発時)

```bash
./scripts/deploy-win.sh   # ビルド→停止→配布(exe+起動資材+.env)→起動まで一括
# ログ: ssh home "type C:\Users\<user>\seamless-desk\sd-win.log"
```

## 開発ノート

- 第三者レビュー(Wave1 6視点 + Wave2 統合): [docs/reviews/](docs/reviews/wave2-integration.md)
- 改善履歴: [docs/improvement-log.md](docs/improvement-log.md)

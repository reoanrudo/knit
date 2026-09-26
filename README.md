# Tsunagu(つなぐ)

**2台のPCを、1つのキーボードで。**

MacBook のキーボード/トラックパッドで Windows デスクトップを操作する入力共有ツール。
カーソルを画面の端へ動かすだけで相手の画面へ移り、Windows の音声まで Mac に集約される。
キーボード/マウス/クリップボード/ファイル/音声を「つなぐ」ことから命名(Deskflow 等の
既存ソフトとは無関係の独立開発。方式は画面端での切り替え式)。

主な機能: 画面端での切替・Mac 流ショートカットの翻訳・クリップボード(テキスト/画像/ファイル、
画面を移る時に同期)・ファイルを掴んだまま境界越え・Windows の音を Mac で再生・
ターミナル/ゲーム向けの自動切替・画面ロック連動・全通信の暗号化。変更点は CHANGELOG を参照

設計: [docs/design.md](docs/design.md) / 配布手順: [docs/distribution.md](docs/distribution.md) /
変更履歴: [CHANGELOG.md](CHANGELOG.md) / ライセンス: [LICENSE](LICENSE)(MIT)

## 構成

- **Mac = サーバ**(`tsunagu-mac`): CGEventTap で入力をフックし TCP で転送。画面右端で切替。
  **メニューバー常駐 GUI**(アイコン+状態・遅延ms表示・手動切替・切替方式/境界回数/音声/⌘キー/
  スクロール方向トグル・Windowsへのファイル送信・Windows音量制御・ログ/再起動/終了)付き
- **Windows = クライアント**(`tsunagu-win`): 受信イベントを SendInput で注入。カーソル左端で復帰通知。
  **タスクトレイ常駐+ステータスウィンドウ**(左クリックで状態/遅延/音声/ログ/終了)+
  **毎分の自動復帰ウォッチ**付き。**Windowsの音をMacで再生**(WASAPIループバック→24901→AudioQueue)。
  **ターミナル/コンソールから起動しても自動で独立プロセスに置き換わり、閉じても切れない**
- 経路: TCP 24900(入力・制御)/ 24901(音声)/ 24902(ファイル・画像)、UDP 24903(LAN 自動発見)。
  **全経路を Noise プロトコルで暗号化**し、共有トークンから導いた鍵で相互認証する(トークンは回線に
  流れない)。受け入れるのは LAN・有線直結・Tailscale のアドレスのみ
- 接続方向は .env で選択可(既定: Mac=サーバ/Win=クライアント。逆方向も実機検証済み)

## アプリから初回接続する

1. MacでTsunaguを起動し、「接続キーを作成」を選びます。
2. 表示されたキーを自分のWindowsへ渡し、WindowsのTsunaguに貼り付けます。
3. Macの案内画面を完了し、求められたアクセシビリティ権限を許可します。
4. Windowsは同じネットワークのMacを探します。認証に成功した後にキーを保存します。

初回のキーの受け渡しは手動です。まだ接続されていない2台間ではTsunaguのクリップボード共有を使えないため、キーは自分が信頼する転送手段で渡してください。両画面の短い確認コードだけで登録する方式は今後の工程です。
新規の接続キーはMacのキーチェーンとWindowsユーザー単位の暗号化ファイルに保存します。既存のenv設定は優先し、上書きしません。
詳しい仕様・保存先・検証範囲は [初回登録](docs/first-connection.md) を参照してください。

## 開発用セットアップ（既存env方式）

```bash
# 1. 共有トークン生成(Mac で 1 回。~/.config/tsunagu/env に保存される)
./scripts/gen-token.sh

# 2. Windows 側へ配布(.env に同じトークンが入り、スタートアップ登録+起動まで行う)
./scripts/deploy-win.sh

# 3. Mac 側を起動(開発運用: ビルド→再起動。メニューバーに常駐する)
./scripts/restart-mac.sh

# 4. 自動検証
./scripts/verify.sh
```

初回のみ Mac 側でアクセシビリティ権限の許可が必要(システム設定 > プライバシーとセキュリティ > アクセシビリティ)。
v0.7(旧 seamless-desk)からの乗り換えは上記をそのまま実行するだけでよく、
旧タスク・旧自動起動は自動的に掃除され、トークン設定も引き継がれる。

## 日常の操作

- **Mac→Windows**: カーソルを境界へ(既定は右端ダブルタップ。Windows の位置は変更可)
- **Windows→Mac**: Windows のカーソルを対向する端へ、または F13 キー
- **コピー&ペースト**: テキスト/画像/ファイルがそのまま双方向で通る。画面を移る瞬間に同期する
  (Mac ⌘C → 切替 → Windows Ctrl+V、およびその逆。受信は各 PC の Downloads\Tsunagu)
- **音**: Windows の音は Mac から出る(接続中は Windows 側を自動ミュート)
- **調整**: ほぼすべて設定ウィンドウ(⌘,)で完結 — Windows の位置・切替の条件
  (ダブルタップ/滞在時間)・スクロールの速度と方向・⌘キーの割当・音声・
  クリップボード共有。メニューからも主要トグルと Windows の音量操作が可能

設定項目と env の全一覧は [docs/usage.md](docs/usage.md) へ。

## アプリとしてのインストールと配布

公開用ZIPには開発者の設定・認証トークンを含めません。現状は初回の接続キーの受け渡しが必要な開発候補版です。商品化の品質基準と未達項目は [商品化計画](docs/product-readiness.md) を参照してください。


```bash
./scripts/package-mac.sh   # dist/Tsunagu.app + zip を作成(設定・トークン非同梱)
./scripts/install-mac.sh   # このMacへインストール(ログイン時自動起動の LaunchAgent 登録)
./scripts/package-win.sh   # dist/Tsunagu-win-<ver>.zip(設定・トークン非同梱)
```

Windows への新規配布は `win-dist/`(tsunagu-win.exe・install.bat・run_tsunagu.vbs/bat・.env)一式を
コピーして install.bat を実行。詳細は [docs/distribution.md](docs/distribution.md)。

## アンインストール

- **Windows**: `win-dist/uninstall.bat` を実行(常駐・タスク登録を解除しファイルを削除)
- **Mac**: `./scripts/uninstall-mac.sh` を実行(LaunchAgent を解除しアプリを削除)

## ビルド

```bash
cargo build --release                                            # Mac 側
cargo build --release -p tsunagu-win --target x86_64-pc-windows-gnu   # Windows 側(クロス)
cargo test                                                       # テスト(default-members)
```

## Windows 側の更新手順(開発時)

```bash
./scripts/deploy-win.sh   # ビルド→停止→配布(exe+起動資材+.env)→起動まで一括
# ログ: ssh home "type C:\Users\<user>\tsunagu\tsunagu-win.log"
```

## 開発ノート

- **AI/自動化で扱う場合**の必読事項(環境固有の罠・ワンコマンド): [docs/agent-guide.md](docs/agent-guide.md)

- 第三者レビュー(Wave1 6視点 + Wave2 統合): [docs/reviews/](docs/reviews/wave2-integration.md)
- 改善履歴: [docs/improvement-log.md](docs/improvement-log.md)

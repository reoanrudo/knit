# Tsunagu(つなぐ)

**2台のPCを、1つのキーボードで。**

MacBook のキーボード/トラックパッドで Windows デスクトップを操作する入力共有ツール。
カーソルを画面の端へ動かすだけで相手の画面へ移り、Windows の音声まで Mac に集約される。
キーボード/マウス/クリップボード/ファイル/音声を「つなぐ」ことから命名(Deskflow 等の
既存ソフトとは無関係の独立開発。方式は画面端での切り替え式)。

**v0.9: 逆方向ファイル送信(エクスプローラーCtrl+C → Mac ⌘V)・Mac 設定ウィンドウ・
Windows 配布 zip 生成**
v0.8: Tsunagu へ改名・配布基盤整備(LICENSE/CHANGELOG/アンインストーラ/v0.7 からの自動移行)
v0.7 までの機能: ファイル送信(Mac ⌘C → Windows Ctrl+V)・完全常駐化・音声出力の集中・
低遅延音声転送(ビットパーフェクト主体)・RTT 表示・Windows 音量制御・⌘キー/スクロール方向の切替

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
- 経路: Tailscale 等のプライベート網を推奨、TCP 24900、**共有トークン認証(必須)**
- 接続方向は .env で選択可(既定: Mac=サーバ/Win=クライアント。逆方向も実機検証済み)

## セットアップ(初回)

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

## 操作

- **Mac→Windows**: カーソルを画面右端へ(既定はダブルタップ。1回目は跳ね返るのでもう一度)
- **Windows→Mac**: Windows のカーソルを左端へ、または F13 キーでトグル
- **メニューバー(SD·Mac / SD·Win / SD·✕)**: 状態常時表示(遅延msつき)。クリックで
  手動切替・切替方式(境界+ダブルタップ ⇄ ホットキーロック)・境界到達回数・
  音声転送・⌘キー(Ctrl ⇄ Alt)・スクロール方向(Windows準拠 ⇄ 反転)・
  Windowsへのファイル送信…・Windowsの音量 ▲▼/ミュート・ログを開く・再起動・終了
- **ファイル送信(双方向)**: Mac でファイルを ⌘C → 画面端で切替 → Windows で Ctrl+V。
  逆方向も同様: エクスプローラーで Ctrl+C → 切替 → Mac で ⌘V。
  Mac 側は `~/Downloads/Tsunagu` へ受信しクリップボードへファイル参照を載せる。
  Windows 側は `Downloads\Tsunagu` に受信しクリップボード(CF_HDROP)へ載せる。
  メニュー/設定ウィンドウの「Windows へファイルを送る…」からも送れる(合計200MBまで)
- **音声出力の集中(既定 ON)**: 接続中は Windows のスピーカーを自動ミュートし
  **Mac のみで発音**(切断で自動復元)。メニューで「常時鳴らす」へ切替可
- **ショートカット**: Mac の Cmd は Windows の Ctrl に自動変換(Cmd+C→Ctrl+C。
  メニューで Alt 行きに切替可)、かな/英数キーで Windows の IME を開閉

## アプリとしてのインストールと配布

```bash
./scripts/package-mac.sh   # dist/Tsunagu.app + zip を作成(トークン封入)
./scripts/install-mac.sh   # このMacへインストール(ログイン時自動起動の LaunchAgent 登録)
./scripts/package-win.sh   # dist/Tsunagu-win-<ver>.zip(Windows 配布 zip。exe+導入書+トークン)
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

- 第三者レビュー(Wave1 6視点 + Wave2 統合): [docs/reviews/](docs/reviews/wave2-integration.md)
- 改善履歴: [docs/improvement-log.md](docs/improvement-log.md)

# 変更履歴

このプロジェクトは Semantic Versioning に準拠します。

## [0.8.0] - 2026-09-25

### 追加
- **Tsunagu(つなぐ)へ改名**(旧: seamless-desk)。Deskflow 等の既存ソフトとの混同を排除し、
  独自ブランドとしての配布を可能にする
- LICENSE(MIT)を新設
- CHANGELOG.md を新設
- アンインストーラ(win-dist/uninstall.bat / scripts/uninstall-mac.sh)を新設
- v0.7(旧 seamless-desk)からの自動移行: 旧環境変数(SEAMLESS_*)・旧設定パス
  (~/.config/seamless-desk/env)・Windows 旧タスク・旧 LaunchAgent を新名称へ自動で引き継ぎ/掃除
- プロトコル VERSION 3→4(旧バイナリとの混在を接続時にはじく)

### 変更
- バイナリ名: sd-mac→tsunagu-mac / sd-win→tsunagu-win / クレート sd-common→tsunagu-common
- Windows 配布先: C:\Users\<user>\tsunagu(旧フォルダから自動移行)
- 受信フォルダ: Downloads\Tsunagu
- ログ: /tmp/tsunagu-mac.log / C:\Users\<user>\tsunagu\tsunagu-win.log
- README を配布物として全面改稿(概要・セットアップ・操作・アンインストール)

## [0.7.6] - 2026-09-25

- 音質優先の追い込み調整: 補間を線形→Catmull-Rom(4点3次)へ、僅かな滞留超過は
  素通りとするデッドバンド、追い込み速度上限を 2%→1% へ。通常時は
  1サンプルも変更しない(ビットパーフェクト)まま、遅延を目標値へ戻す

## [0.7.5] - 2026-09-25

- リサンプラの位相連続化: 読み出し位相をコールバック間で持ち越すことで
  毎コールバックの過剰消費(0.14%=66Hz周期ノイズ)を根絶。ユーザー実聴で改善を確認

## [0.7.4] - 2026-09-25

- 音割れの根治: フレーム間引きをリサンプル追い込みへ変更、
  WAVEFORMATEXTENSIBLE の SubFormat 厳密判別(f32/i32/s16)を実装

## [0.7.3] - 2026-09-25

- 8バイト(f32×2ch=1フレーム)境界の厳守: 境界外ドロップによる
  恒久的位相ずれ(元の音源が分からない破壊音)を全経路で防止
- プリロール(32KB)導入: 供給の到着むらによる断続音(モールス音)を解消

## [0.7.2] - 2026-09-25

- 音声の低遅延化: AudioQueue 15ms×4バッファ、バッファ目標の引き下げ

## [0.7.1] - 2026-09-25

- 接続中スピーカーミュート: Windows のスピーカーを自動ミュートし Mac のみで発音。
  切断時は元のミュート状態へ復元

## [0.7.0] - 2026-09-25

- Windows 完全常駐化(コンソールの生死に左右されない独立プロセス化)
- ファイル送信(Mac ⌘C → Windows Ctrl+V、CF_HDROP)
- RTT 計測・Windows 音量制御・設定トグル(⌘キー/スクロール方向/スピーカーミュート)

## [0.6.x] 以前

- 境界切替(ダブルタップ)・絶対座標送信・クリップボード双方向同期(テキスト/画像DIB)・
  音声転送(WASAPIループバック→AudioQueue)・メニューバー/タスクトレイ GUI・
  Tailscale 運用・共有トークン認証 ほか。詳細は docs/improvement-log.md

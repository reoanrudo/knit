# Knit

**One keyboard and trackpad for your Mac and Windows PC.**
**Macのキーボードとトラックパッドのまま、Windowsも操作。**

Knit lets you move the cursor off the edge of your Mac screen and keep working on your Windows PC.
Copy on one machine, paste on the other. Drag a file across the border. Hear Windows audio on your Mac.
All traffic between your PCs stays on your local network, encrypted end to end. The only outbound
connection is the optional update check against GitHub Releases (see the usage guide to disable it).

Macの画面の端までカーソルを動かすと、そのままWindowsの画面へ移ります。コピーしたテキスト・画像・ファイルは
もう1台でも貼り付けられ、Windowsの音はMacから出ます。PC間の通信はすべて暗号化され、家やオフィスのネットワークの外へは出ません
(例外はアプリの更新確認だけで、GitHub Releases へ接続します。設定で止めることもできます)。

> **Status: early preview (v0.26.0).** Unsigned builds; some setups are not yet verified.
> 開発中のプレビュー版です。署名なしの配布で、未検証の環境があります。
> 不具合の報告を歓迎します: [Issues](https://github.com/reoanrudo/knit/issues)

## できること / Features

| | |
|---|---|
| 画面の端で切替 | Mac の位置に合わせて、Windows の置き場所(右・左・上・下)を選べる。複数台も切替可能 |
| Mac のショートカットのまま | ⌘C / ⌘V などを Windows の Ctrl 操作へ自動で翻訳。かな/英数キーと入力言語も追従 |
| コピー&ペースト | テキスト・画像・ファイルが両方向で通る。履歴から選んで復元もできる |
| ファイルのドラッグ | 掴んだまま画面の境界を越えられる。**フォルダごと**も渡せる(展開後512ファイル・合計10GiBまで)。進捗表示・Escでの中止・内容ハッシュ検証つき |
| 音声 | Windows の音を Mac で再生(音量は Mac の設定画面で調整)。接続中は Windows 側を自動でミュートし、切断で元に戻す(異常終了時も Windows 側の次回起動時に自動で戻す) |
| 安全 | 相互認証 + Noise プロトコルで全通信を暗号化。受け付けるのは LAN・有線直結・Tailscale のみ |
| 接続の安定 | スリープ復帰後も自動で再接続。断が 60 秒を超えたときだけ通知。安定性はログで実測できる([集計方法](scripts/stability-report.sh)) |
| Android タブレット | 別の操作先として接続可能([案内](docs/android.md)) |

## はじめる / Get started

1. **Mac と Windows の両方に Knit を入れる。** [Releases](https://github.com/reoanrudo/knit/releases) から
   Mac 用と Windows 用の zip をダウンロードして展開。署名がないため初回だけ開き方に注意
   ([配布手順](docs/distribution.md))。
2. **Mac で Knit を起動する。** 初回は画面に6桁のコードが出ます(あとから出すには、設定「接続」の「端末を登録…」)。
3. **Windows で Knit を起動する。** 同じネットワークの Mac を自動で見つけ、1台だけなら自動で確認を求めます。Mac に出る4桁の番号を、Windows の4つの候補から選び、Mac で「許可」を押します(打つ必要はありません。見つからないときは Mac の IP を入力。従来の6桁コードも使えます)。
4. **Mac の案内に従い、アクセシビリティで Knit をオンにする。** オンにすると自動で次へ進みます。以上です。

コードは5分で失効し、試行は3回までです。登録に成功すると両 PC のキーチェーン/保護領域に鍵が保存され、
次からは自動で再接続します。詳細は [初回登録](docs/first-connection.md)。

## 毎日の使い方

- **Mac → Windows:** カーソルを設定した端へ(既定は右端をダブルタップ)
- **Windows → Mac:** Windows のカーソルを対向する端へ、または F13
- **設定:** メニューバーの Knit から設定ウィンドウ(⌘,)。切替方式・スクロール・音声・共有を調整

全操作は [使い方ガイド](docs/usage.md)、困ったときは同ガイドの「うまく動かない時」を参照してください。
つながらない・切れたときは、まずアプリ内の診断(Mac は「接続を診断…」、Windows は「接続診断」)が原因と対処を教えます。

## Barrier / Synergy から乗り換える

Barrier・Synergy・Input Leap のスリープ復帰・切断系の不満は Reddit や GitHub Issues で
繰り返し報告されており、Knit は「登録は一度きり・以降は自動再接続」を設計の中心に置いています。

| よくある不満 | Knit の動き |
|---|---|
| スリープ復帰後に手動で再読込・再接続が必要 | 自動で再接続。断が 60 秒を超えたときだけ通知する |
| 設定ファイルの手動編集(接続先・画面配置) | 登録は GUI で完結(自動発見 + 4桁確認 + 許可)。設定ファイル編集は不要 |
| 接続先の IP 手入力 | 同じネットワークの相手を自動発見。見つからないときだけ IP 入力 |
| 通信の暗号化が任意・不透明 | 相互認証 + Noise 暗号化が常時 |
| macOS 更新で登録が消える・壊れる | 登録鍵はキーチェーンに保存され、更新後も再登録なしで動く |

「より安定しています」という比較は、まだあなたの環境での実測を持っていません。
断・再接続はログで実測できるため、あなたの環境での実測データの提供を歓迎します
([集計方法](scripts/stability-report.sh))。

## 動作環境

- Mac: macOS(Apple Silicon)
- Windows: 対話セッションのデスクトップ環境
- 両 PC が同じ LAN、有線直結、または同じ Tailscale ネットワーク上にあること

## 開発・貢献

ビルド、テスト、配布物の作り方は [開発者向けガイド](docs/development.md)。
変更履歴は [CHANGELOG](CHANGELOG.md)、セキュリティ報告は [SECURITY](SECURITY.md)、
ライセンスは [MIT](LICENSE)。Rust 製。Deskflow など既存ソフトとは無関係の独立開発です。

*旧名 Tsunagu(v0.25 まで)とは互換がありません。*

- **両方の PC を同じ版の Knit へ。** 片側だけ入れ替えても接続できません(プロトコルが非互換です)。
  もう1台の zip は [Releases](https://github.com/reoanrudo/knit/releases) から
  (Mac 用 `Knit-<版>.zip` / Windows 用 `Knit-win-<版>.zip`)。アプリ内の
  「アップデートを確認」は今後の版で有効になるため、それまでの更新は手動です
- **設定とクリップボード履歴は初回起動時に自動で引き継がれます**(旧データも残ります)。
  やり直しになるのは「端末の登録」と Mac のアクセシビリティ許可だけです

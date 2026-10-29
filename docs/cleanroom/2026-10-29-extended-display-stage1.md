# 仕様: 拡張ディスプレイ(Windows を Mac の追加画面にする)— 第1段: Mac 側の送り出し

- 書いた人: lead エージェント(2026-10-29)。参考 OSS のソースを読んでいるため実装には関わらない
- 実装する人: 参考 OSS のソースを読んでいないエージェント(Codex / gpt-6.1-terra)
- 根拠とした資料(これ以外を根拠にしない):
  - `third_party/deskpad/CGVirtualDisplayPrivate.h`(MIT。DeskPad 由来。非公開 API の宣言。
    ライセンス表示は `THIRD_PARTY_NOTICES.md`)
  - macOS SDK のヘッダ: `ScreenCaptureKit.framework`、`VideoToolbox.framework`(`VTCompressionSession.h`)、
    `CoreMedia.framework`、`CoreVideo.framework`
  - Apple Developer Documentation(ScreenCaptureKit、VideoToolbox の公開ドキュメント)
  - Knit 自身のコード(`crates/common/src/bulk.rs` の別チャネル、`crates/common/src/secure.rs`、
    `crates/mac/src/audio.rs` の別ポート待受の作り)
- 注意: **DeskPad 以外の、仮想ディスプレイや画面転送を実装した OSS のコードは参照しない**
  (GPL のものが多いため)。DeskPad も参照してよいのはこのヘッダだけ

## 全体像(段階分け)

| 段 | 内容 | 検証 |
|---|---|---|
| **1(今回)** | Mac: 仮想ディスプレイの作成、その画面の取り込み、H.264 符号化、暗号化経路での送信。受信側は「保存して検証する」テスト用の受け手のみ | Mac 単体の自動テスト + 手元で動画ファイルに書き出して目視 |
| 2 | Windows: 受信、H.264 復号(Media Foundation)、全画面表示、Windows 側の入力を Mac の仮想画面の座標へ返す | Windows 実機 |
| 3 | 設定画面(画面配置で仮想画面を置く)、解像度選択、遅延計測と調整 | 両実機 |

## 第1段でやること

### A. 仮想ディスプレイ(`crates/mac/src/vdisplay.rs` など)

- `CGVirtualDisplayDescriptor` / `CGVirtualDisplaySettings` / `CGVirtualDisplayMode` / `CGVirtualDisplay` を
  Objective-C ランタイム経由で使う(Knit は既に `objc_getClass` / `objc_msgSend` で AppKit を呼んでいる。
  その書き方に合わせる。ヘッダは宣言の参照用で、Rust 側に必要な分だけ呼び出しを書く)
- 名前は「Knit 拡張ディスプレイ」、解像度は既定 1920x1080 @60Hz(引数で指定可能)、hiDPI はオフ
- 作成・破棄の関数を用意し、破棄すると画面が消えること。作成した `displayID` を返す
- 非公開 API のため、クラスが見つからない・作成に失敗した場合はエラーを返して機能全体を無効にする(落ちない)

### B. 取り込みと符号化

- ScreenCaptureKit で、A で作ったディスプレイだけを取り込む(`SCContentFilter` でそのディスプレイを指定)
  - 画面収録の許可が無い場合は、エラーを返して案内文(「システム設定 › プライバシーとセキュリティ › 画面収録 で Knit をオンにしてください」)をログに出す
- VideoToolbox の `VTCompressionSession` で H.264 に符号化する
  - リアルタイム(`kVTCompressionPropertyKey_RealTime`)、B フレーム無し、キーフレーム間隔は 2 秒
  - ビットレートは既定 12 Mbps(引数で指定可能)
- 出力は Annex B 形式(スタートコード付き)のバイト列に直し、SPS/PPS をキーフレームの前に付ける

### C. 送信路

- 動画は入力・クリップボードの制御接続とは **別の TCP 接続** で送る(大きなデータで入力が詰まらないように。
  既存の bulk の別チャネルと同じ考え方)。ポートは制御ポートと別の番号を共通側の定数に置く
- 暗号化は既存の `knit_common::secure`(Noise)をそのまま使い、同じ端末鍵で認証する
- フレームの形式(共通側 `crates/common` に置き、単体テストを付ける):
  `[種類 1byte(1=設定 2=キーフレーム 3=差分)][タイムスタンプ u64 LE(µs)][長さ u32 LE][本体]`
  - 種類1(設定)は JSON で `{w, h, fps, codec:"h264"}`
  - 受信側の上限(1フレーム 8 MiB)を超えるものは送らない・受けない
- 送信側は、回線が詰まったら差分フレームを捨てて次のキーフレームを要求する(古い画面を溜めない)。
  捨てた枚数を数えてログに出す

### D. テストと検証用の道具

- 単体テスト: フレーム形式の往復・上限・壊れたデータ、Annex B 変換(AVCC→Annex B)、詰まったときの間引きの判定(純関数)
- 検証用バイナリ(`crates/mac/src/bin/` に):仮想ディスプレイを作って 5 秒取り込み、受信側の代わりに
  `.h264` ファイルへ書き出す。`ffprobe` があれば解像度とフレーム数を表示する(無ければスキップ)
- 実画面を使う部分(A・B)は自動テストにしない(CI に画面が無いため)。検証用バイナリで手動確認する

## やらないこと(第1段)

- Windows 側の受信・表示(第2段)
- 設定画面・画面配置への組み込み(第3段)。今回は環境変数 `KNIT_EXTEND=1` のときだけ有効
- 音声(既存の音声経路を使う)
- 既存の入力共有の挙動の変更

## 受け入れ条件

- `./scripts/quality-gate.sh` が PASS
- D の単体テスト
- 検証用バイナリの実行方法を `docs/extended-display.md`(新規)に書く。第2段・第3段の残り作業も同じ文書に箇条書きで残す
- CHANGELOG の[未リリース]に1行(「実験的: 拡張ディスプレイの送り出し(Mac)」)

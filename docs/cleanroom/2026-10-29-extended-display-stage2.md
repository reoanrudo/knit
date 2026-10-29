# 仕様: 拡張ディスプレイ — 第2段: Windows 側の受信・復号・表示

- 書いた人: lead エージェント(2026-10-29)。参考 OSS のソースを読んでいるため実装には関わらない
- 実装する人: 参考 OSS のソースを読んでいないエージェント(Codex / gpt-6.1-terra)
- 根拠とした資料(これ以外を根拠にしない):
  - Microsoft Learn の公開ドキュメント: Media Foundation の H.264 Video Decoder
    (`CLSID_CMSH264DecoderMFT`、入力 `MFVideoFormat_H264`、出力 `MFVideoFormat_NV12`、
    `IMFTransform::ProcessInput` / `ProcessOutput`、`MF_E_TRANSFORM_NEED_MORE_INPUT`、
    `MF_E_TRANSFORM_STREAM_CHANGE`)、`MFStartup`、`MFCreateSample` / `MFCreateMemoryBuffer`、
    GDI の `StretchDIBits`(または `SetDIBitsToDevice`)、全画面のポップアップウィンドウ(`WS_POPUP`)
  - `windows` / `windows-sys` crate の定義(~/.cargo/registry)
  - Knit 自身のコード: `crates/common/src/video.rs`(フレーム形式・定数)、`crates/mac/src/extend.rs`(送り手)、
    `crates/win/src/audio.rs`(Mac への別 TCP 接続と `knit_common::secure::connect` の流儀)、
    `docs/extended-display.md`
- 注意: **画面共有・リモートデスクトップ・仮想ディスプレイを実装した OSS のコードは参照しない**

## 第1段で決まっていること(変えない)

- Mac は `KNIT_EXTEND=1` のとき、制御ポート+5(既定 24905)で待ち受ける。Noise のラベルは `knit-video`
- 接続できるのは、制御接続中の相手と同じ端末鍵で認証した端末だけ
- フレームは `knit_common::video::read_frame` で読める。最初が `CONFIG`(JSON `{w,h,fps,codec}`)、
  以降が `KEYFRAME` / `DELTA`(Annex B の H.264。キーフレームの前に SPS/PPS 付き)

## やること

### A. 受信(`crates/win/src/extend.rs` など)

- 環境変数 `KNIT_EXTEND=1` のときだけ動く(Mac 側と同じ)。`knit_common::envutil::get` で読む
- 制御接続が確立している間、Mac の動画ポートへ接続する(相手 IP の取り方・再試行・タイムアウト・
  `secure::connect` の使い方は `audio.rs` に合わせる。ラベルは `b"knit-video"`)
- 制御接続が切れたら動画も閉じ、表示ウィンドウを隠す

### B. 復号(Media Foundation)

- `MFStartup` 後、H.264 デコーダ MFT を作り、入力型を H.264、出力型を NV12 にする
- 受け取った Annex B のバイト列をそのままサンプルとして `ProcessInput` に渡し、`ProcessOutput` で取り出す
  (`MF_E_TRANSFORM_NEED_MORE_INPUT` は次の入力へ、`MF_E_TRANSFORM_STREAM_CHANGE` は出力型を設定し直す)
- 低遅延にする(`CODECAPI_AVLowLatencyMode` を有効。使えなければログだけ出して続行)
- NV12 → BGRA の変換は自前の純関数で行う(BT.601 の限定範囲。単体テスト付き。SIMD は不要)
- 復号エラーが続いたら、次のキーフレームまで差分を捨てる

### C. 表示

- 枠なしの全画面ウィンドウ(`WS_POPUP`、作業領域ではなくモニター全体)を、**拡張先として選んだモニター**に出す。
  モニターの選び方は環境変数 `KNIT_EXTEND_MONITOR`(0 始まりの番号。省略時はプライマリ以外の最初のモニター、
  1 枚しかなければプライマリ)
- 最新のフレームだけを描く(描画が遅れたら古いフレームは捨てる)。GDI の `StretchDIBits` で
  アスペクト比を保って拡大縮小する(余白は黒)
- Esc を 1 秒長押しで表示を閉じる(誤って閉じないよう長押し)。閉じても受信は続け、
  トレイから再表示できるようにする(トレイのメニューに「拡張ディスプレイを表示」を追加。既存の
  トレイの作りに合わせる)
- 表示中はウィンドウを最前面にしない(他のアプリの邪魔をしない)

### D. 入力(最小限)

第2段では **Windows 側からの入力を Mac の仮想画面へ返す処理は入れない**(既存の入力共有で、Mac のカーソルを
仮想画面へ動かせば Mac のキーボード・トラックパッドでそのまま操作できるため)。docs に第3段の課題として残す

### E. 計測

- 受信したフレーム数・捨てたフレーム数・復号にかかった時間の平均を 10 秒ごとに 1 行ログに出す
  (`[extend] 受信 N / 破棄 M / 復号 X ms`)

## やらないこと

- Windows → Mac の入力の返送(第3段)
- 設定画面での解像度・配置の選択(第3段)
- D3D11 / DXVA によるハードウェア描画(必要なら第3段で検討)

## 受け入れ条件

- `cargo check --target x86_64-pc-windows-gnu -p knit-win --tests` が通る
- NV12→BGRA 変換・アスペクト比の計算・モニター選択の純関数に単体テスト
- `./scripts/quality-gate.sh --with-win` が PASS
- `docs/extended-display.md` に第2段の使い方(両側で `KNIT_EXTEND=1`、`KNIT_EXTEND_MONITOR`)を追記
- CHANGELOG の[未リリース]に1行
- 実機確認(lead が行う): Windows に Mac の仮想画面が表示され、Mac 側でウィンドウを仮想画面へ動かすと Windows に映る

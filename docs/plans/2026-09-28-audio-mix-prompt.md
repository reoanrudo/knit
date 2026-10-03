# 指示文: すべての端末の音を Mac に集めて混ぜる

次のエージェントへそのまま貼り付ける。

```
Knit(Mac から Windows・Android タブレットを操作する入力共有ツール、Rust)に、
「すべての端末の音を Mac(ホスト)に集めて混ぜて鳴らす」機能を実装してください。
返答は日本語のです・ます調で。コミット・push は私が指示した時だけ行ってください。

リポジトリ: /Users/taguchireo/Documents/八幡平市地域おこし協力隊/05_個人/knit
最初に読むもの: docs/android.md、docs/plans/2026-09-28-android-handoff.md、
docs/product-definition.md(「共有の対象と行き先を明確にする」「対応機能と許可した機能を区別する」)。
作業前に `source $HOME/.cargo/env`、`git status`、`git log -5` で状態を確認すること。

## 目標(利用者の要望)
- Mac 自身の音・接続中の各 Windows の音・各 Android タブレットの音を、すべて同時に Mac で聞ける
  (混ぜて鳴らす。操作中の端末だけに切り替える方式ではない)
- Mac にイヤホンをつなげば、そのイヤホンから全端末の音が聞こえる
  (= Knit の再生は常に Mac の「現在の既定の出力」へ出て、途中で切り替えても追従する)
- 接続中は相手側のスピーカーを既定で消し、Mac に一本化する(設定で「両方で鳴らす」も選べる)

## 現状(前任者が確認した事実)
- Windows→Mac の音声は実装済み。Windows は再生中の音(WASAPI ループバック。マイクではない)を
  取り込み、TCP 24901 で暗号化(secure::connect、ラベル b"knit-audio")して送る。
  形式: 1 行目 "SDAUDIO3 <rate> s16\n" → Mac が "ok\n"。以後「u32 LE 長さ + s16 ステレオ PCM」、
  長さ 0 は 1 秒毎の生存確認(crates/win/src/audio.rs 506・559・567 行付近)
- Mac の受け口(crates/mac/src/audio.rs の start、359 行付近)は accept を直列に処理し、
  リングバッファ(RING)が 1 本。同時に 1 台しか鳴らせない。Windows は無音中も接続を保つため、
  2 台目は 1 台目が切れるまで待たされる
- Mac の再生は AudioQueueNewOutput(出力機器を指定せず既定の出力)。再生中に既定の出力が
  変わった時に追従するかは未確認
- Windows 側は既定の再生機器の変化を 2 秒毎に検知して取り込み直す(追従済み)
- 既知の不具合: Mac の「Windows の音声を再生」をオフにしても Windows は送信を続ける
  (Msg::Cfg に音声の項目が無い)。また「Windows スピーカーをミュート」がオンのまま Mac 再生を
  オフにする/Windows の「音声転送をオフにする」を押すと、どこからも音が出なくなる
- Android の音声は未実装。中継(crates/mac/src/android.rs)は scrcpy 4.1 のサーバー部品を
  video=false audio=false control=true で起動している

## Android の音の取り方(scrcpy 4.1 のソースで確認済み)
- 起動引数に audio=true audio_codec=raw send_stream_meta=false send_frame_meta=false を足すと、
  音声ソケットに 48kHz・s16・ステレオの PCM が区切りなしで流れる(Android 11 以上。
  利用者のタブレットは Android 16)
- audio_source=output(既定): 端末全体の音を取り、端末本体では鳴らなくなる(Mac に一本化)。
  audio_source=playback audio_dup=true: 端末でも鳴らしつつ取る(Android 13 以上。
  アプリ側が拒否した音は取れない)
- tunnel_forward=true では、サーバーが開くソケットの順が「音声 → 制御」になる。起動確認の
  1 バイト(dummy byte)は最初のソケット(音声)に来る。今の connect_control は制御 1 本しか
  想定していないので、2 本受ける形へ変える。音声が取れない端末ではサーバーが音声ソケットを
  閉じるので、その場合も制御(キーボード・マウス)は続ける
- 中継は受けた PCM を、Windows と同じ形式で 127.0.0.1:(本線ポート+1) へ送る

## 実装の方針(変えてよいが、変えるなら理由を書く)
1. Mac の受け口を複数同時接続にする: 接続ごとにスレッドとリングを持ち、AudioQueue の
   コールバックで全リングを足し合わせる(飽和はソフトクリップ)。送信元ごとの標本化周波数を
   出力側へそろえる。既存のプリロール・遅延の追い込みは送信元ごとに持つ
2. 送信元を識別する: 1 行目を "SDAUDIO3 <rate> s16 <端末id>" に拡張(4 語目は任意にし、
   旧版の 3 語も受ける)。同じ端末 id の再接続は古い方を置き換える
3. 既定の出力の変化に追従する(kAudioHardwarePropertyDefaultOutputDevice を監視して
   AudioQueue を作り直す等)。まず現状で追従するかをコードと Apple の資料で確認する
4. 設定をまとめる: Msg::Cfg に音声の項目を足し(serde の既定値で旧版と互換)、Mac で止めたら
   相手も取り込みと送信を止める。無音になる組み合わせを無くす
   (例: 「音を鳴らす場所: Mac に集める / 各端末で鳴らす / 両方」の 1 つの選択)
5. Android: 端末ごとの「操作する」を選んだ端末だけ音も取る。メニューの「できること」表示
   (crates/mac/src/android/state.rs の CAPABILITY)と docs/android.md を更新する
6. 端末ごとの音量・ミュートは今回は必須ではない(やるなら接続先メニューに置く)

## 検証
- 混ぜる処理・周波数変換・形式の解析は、音を出さない単体テストで確かめる
- Android の音声中継は、Knit の代役(試験用の受け口)で PCM を受け取り、中身が届くことを確かめる
  (Mac で再生しない)
- 実際に音を聞く確認は私が行う。手順(何を再生し、何が聞こえれば成功か)を書いて渡すこと

## 守ること
- 音が鳴る試験をしない(Mac でも端末でも)。どうしても必要なら事前に私に聞く
- 実機で e2e_device 試験を実行しない(入力欄とクリップボードを書き換えるため)。
  実機(Xiaomi Pad 6S Pro 12.4、シリアル e569c535)では e2e_consent と、音を出さない試験だけを使う
- タブレットの設定(settings put 等)を変えない
- 利用者の Knit を再起動・再インストールしない(必要なら手順を示して私に頼む)
- エミュレーター(pixel_8_shogi)は別プロジェクトの物。使う時は -read-only で起動し、終わったら止める
- Windows 機には今は届かない(利用者は出先)。Windows 側の変更はクロスビルドと単体テストまで行い、
  実機確認が残ることを報告に書く
```

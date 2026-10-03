# 指示文: Android 連携の問題整理(トラックパッド・クリップボード・設定画面)

次のエージェントへそのまま貼り付ける。

```
Knit(Mac から Windows・Android タブレットを操作する入力共有ツール、Rust)の
Android 連携について、残っている問題を整理してください。今回は「整理」が目的で、
修正に入るのは私が優先順位を決めてからです。
返答は日本語のです・ます調で。コミット・push は私が指示した時だけ行ってください。

リポジトリ: /Users/taguchireo/Documents/八幡平市地域おこし協力隊/05_個人/knit
最初に読むもの: docs/android.md、docs/plans/2026-09-28-android-handoff.md、
docs/product-definition.md(製品の思想。特に「端末ごとに利用できる機能」「未対応・権限不足・
停止中・通信断を同じ表示にまとめない」)。

## 現状(利用者の確認結果)
- 実機: Xiaomi Pad 6S Pro 12.4(HyperOS 3 / Android 16)、USB 接続、シリアル e569c535
- 成功: マウス共有、キーボード共有、画面配置(画面端での行き来)
- 問題あり(詳細は私に聞くこと): トラックパッド機能、クリップボードの共有、
  設定画面での Android の認識

## 進め方
1. 着手前に `source $HOME/.cargo/env`、`git status`、`git log -5` で状態を確認する
   (Android 機能がコミット済みかどうかも確認する)
2. 3 領域それぞれについて、私に症状を 1 領域ずつ質問する(期待した動き・実際の動き・
   再現手順)。自分で調べれば分かることは質問しない
3. コードを読み、症状ごとに「原因(2 層以上たどる)・該当箇所 file:line・修正案・
   修正の規模・実機でしか確かめられない点」をまとめる。推測は推測と明記する
4. 結果を docs/plans/ に Markdown で書き、優先順位の推奨とその理由を添えて私に返す

## 調べる起点(前任者のメモ。未検証の推測を含む)
### トラックパッド
- Mac はスクロールを「ノッチ単位・0.05 刻み」の Msg::Scroll で送る(crates/mac/src/main.rs の
  SCROLL_ACC 付近)。中継(crates/mac/src/android/hid.rs の Pointer::scroll)は整数ノッチに
  切り捨てて HID ホイールへ送るため、ゆっくりしたスクロールが段階的・粗くなる可能性がある
- 改善候補(未検証): HID の高解像度ホイール(Resolution Multiplier)、または scrcpy の
  INJECT_SCROLL_EVENT(浮動小数のスクロール量。ただしポインタ位置の指定が必要)
- 二本指の横スワイプは Mac 側で Windows 向けの特別扱いがある(main.rs「横優勢ジェスチャ」付近)。
  ピンチ・三本指スワイプ・慣性スクロールは Android へ何も送っていない
- Android の加速により、ポインタの推定位置と実物がずれる(docs/android.md の既知の制約)
### クリップボード
- Mac→相手: 画面を移る時だけ Msg::Clip を送る設計(README「画面を移る瞬間に同期」)
- Android→Mac: scrcpy サーバーの clipboard_autosync による端末側の変化通知を Msg::Clip へ変換
  (crates/mac/src/android.rs の device reader スレッド。last_clip で折り返しを防ぐ)
- Mac 側の Msg::Clip 受信処理(main.rs)はアクティブでない相手からの受信、CLIP_SHARE、
  履歴の表示名「Windows」固定などの条件がある。どこで止まっているかを確かめる
- 画像・ファイルは大容量経路(24902)を使うが、中継は大容量経路を実装していない
- HyperOS 固有のクリップボード制限の有無は未調査
### 設定画面での認識
- 設定ウィンドウ(crates/mac/src/gui/prefs.rs・preferences.rs)とメニュー(gui.rs)は
  「Windows の位置」「Windows スピーカー」など Windows 前提の文言・項目が多い
- 画面の位置(side)は全接続先で共通の 1 つ。端末ごとの配置や、端末ごとにできること
  (Android は画面・ファイル・音声が未対応)を設定画面が知らない
- Android の状態と「操作する/しない」はメニューの「Android タブレット」にしかない
  (crates/mac/src/android/state.rs)

## 守ること
- 実機で e2e_device 試験を実行しない(入力欄とクリップボードを書き換えるため)。
  実機では e2e_consent だけを使う
- タブレットの設定(settings put 等)を変えない
- 利用者の Knit を再起動・再インストールしない(必要なら手順を示して私に頼む)
- エミュレーター(pixel_8_shogi)は別プロジェクトの物。使う時は -read-only で起動し、終わったら止める
- 音が鳴る試験をしない
```

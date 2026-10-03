# 引き継ぎ: Android タブレット操作(2026-09-28)

次のエージェントへ貼り付ける指示文。下の「貼り付け用」をそのまま渡す。

---

## 貼り付け用

```
Knit(Mac⇄Windows の入力共有ツール、Rust)の Android タブレット操作機能の続きを担当してください。
返答は日本語のです・ます調で。コミット・push は私が指示した時だけ行ってください。

リポジトリ: /Users/taguchireo/Documents/八幡平市地域おこし協力隊/05_個人/knit
(GitHub: reoanrudo/knit。v0.26.0 まで公開済み。Android 機能は未コミット)
最初に読むもの: docs/android.md(仕様・検証状況)、docs/product-definition.md(製品の思想)、
docs/plans/2026-09-28-android-handoff.md(この引き継ぎ書の本文)。

作業の前に必ず:
- `source $HOME/.cargo/env` してから cargo を使う(既定の cargo は Windows ターゲットが無い)
- `git status` と `git log -3` で、別エージェントの並行編集がないか確認する
- `cargo test` が 122 件成功・5 件除外(ignored)であることを確認する

最初のタスク:
1. 私がタブレット(Xiaomi Pad 6S Pro 12.4、USB 接続、シリアル e569c535)で実際に操作した結果を
   伝えるので、不具合があれば直す。見てほしい点: ポインタの見え方、文字入力、⌘C/⌘V、
   画面端から Mac への戻り、JIS 記号、かな/英数
2. 問題なければ、Android 機能をコミットしてよいか私に確認する

守ること:
- 実機で e2e_device 試験を実行しない(タブレットの入力欄とクリップボードを書き換えるため)。
  実機では e2e_consent だけを使う
- タブレットの設定(settings put 等)を変えない
- エミュレーター(pixel_8_shogi)は別プロジェクトの物。使う時は -read-only で起動し、終わったら止める
- 音が鳴る試験をしない
```

---

## 現状

### コミット済み・公開済み

- `450d800` Tsunagu → Knit 改名、`38cb925` v0.26.0(GitHub Releases に Knit-0.26.0.zip / Knit-win-0.26.0.zip)
- 公開中の v0.26.0 には Android 機能は入っていない

### 未コミット(Android 機能)

| ファイル | 内容 |
|---|---|
| `crates/mac/src/android.rs` | 中継本体。監視スレッド(3 秒毎に `adb devices -l`)、端末ごとの接続スレッド、scrcpy サーバー部品の起動、Knit 本線(127.0.0.1)への接続、入力変換ループ、試験(e2e_device / e2e_pointer_gain / e2e_consent は `#[ignore]`) |
| `crates/mac/src/android/hid.rs` | 仮想キーボード・マウスの HID 記述子、Mac キーコード→HID、ポインタ位置の推定と相対移動への変換 |
| `crates/mac/src/android/scrcpy.rs` | scrcpy 4.1 の制御メッセージの形式(UHID 作成・入力、クリップボード、キー注入、端末からのメッセージ) |
| `crates/mac/src/android/state.rs` | 端末ごとの状態・「操作する/しない」の保存(Application Support/Knit/android-tablets.txt)・メニュー文言 |
| `crates/mac/src/main.rs` | 待受モードで `android::spawn` を起動(6 行)。接続経路表示で 127.0.0.1 を "adb" と出す |
| `crates/mac/src/gui.rs` | メニューバーに「Android タブレット」サブメニュー(状態表示・クリックで操作する/しない) |
| `docs/android.md` ほか | 仕様・準備・検証状況。CHANGELOG の [未リリース]、README、docs/README に追記 |

### 設計の要点

- タブレットに専用アプリは入れない。Mac が adb で scrcpy のサーバー部品
  (`/opt/homebrew/share/scrcpy/scrcpy-server`、版 4.1)を送り込み、UHID の仮想キーボード・マウスを作る
- 中継は Knit 本線へ Windows と同じ hello で入る「接続先の 1 台」。Mac の切替処理は無改造
- 製品定義に合わせ、見つけた端末は黙って操作しない。端末ごとに利用者が選び、取り消せる。
  状態(USB デバッグ許可待ち・選択待ち・接続中・失敗理由・adb 不足)を区別して表示する
- Android はマウス移動に画面密度倍率を掛けるため、既定で 160/dpi を掛けて送る

### 検証済み

- Xiaomi Pad 6S Pro 12.4(HyperOS 3 / Android 16、USB): e2e_consent 通過(発見→選択待ち→操作する→
  接続→取消でその場で切断→保存)。仮想デバイス登録とマウス移動の入力層への到達
- Android 16 エミュレーター: e2e_device 通過(キー入力・マウス移動・ping/pong)、
  移動量の補正後の比 0.78〜1.02 倍
- `cargo test` 122 件成功、Mac 配布パッケージの release-check OK

### 未確認・既知の制約

- 画面上のポインタの見え方、端での戻り、JIS 記号、かな/英数(利用者の目視待ち)
- 実機でのキー入力とクリップボード(試験が端末を書き換えるため未実施)
- HyperOS は dumpsys input にポインタ位置を出さず、実機での移動量の実測は不可
- タブレットでコピーした内容は Mac のクリップボード履歴で「Windows」と表示される
- ファイル・画像・音声は未対応。製品定義の方向 1〜3(タブレット→PC 操作、受け渡し、画面表示)は対象外
- 新ビルドは利用者の Mac に未インストール(`./scripts/package-mac.sh` → `./scripts/install-mac.sh`。
  初回はアクセシビリティ・入力監視の権限付与が要る)

### 環境の注意

- Homebrew の scrcpy は `4.1_1`(作り直し番号付き)。版は "_N" を除いて渡す(修正済み)
- scrcpy 本体は依存ライブラリの欠落で起動しないことがあるが、サーバー部品だけで動く
- エミュレーターの起動: `ANDROID_SDK_ROOT=/opt/homebrew/share/android-commandlinetools
  ~/Library/Android/sdk/emulator/emulator -avd pixel_8_shogi -read-only -no-window -no-audio`
- 利用者は出先で、Windows 機には届かない。旧 Tsunagu の env トークンが残っていると再登録画面を通らずに接続する

### 試験コマンド

```bash
source $HOME/.cargo/env
cargo test
KNIT_E2E_SERIAL=e569c535 cargo test -p knit-mac e2e_consent -- --ignored --nocapture
KNIT_E2E_SERIAL=emulator-5554 cargo test -p knit-mac e2e_device -- --ignored --nocapture
```

### 別件で残っている引き継ぎ

- ドラッグ&ドロップの改修: [2026-09-28-drag-handoff.md](2026-09-28-drag-handoff.md)

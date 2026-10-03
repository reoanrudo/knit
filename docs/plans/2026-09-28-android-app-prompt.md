# 指示文: Android アプリ(標準モード)の開発

次のエージェントへそのまま貼り付ける。

```
Knit(Mac から Windows・Android タブレットを操作する入力共有ツール、Rust)の
Android アプリ(標準モード)を開発してください。
返答は日本語のです・ます調で。コミット・push は私が指示した時だけ行ってください。

リポジトリ: /Users/taguchireo/Documents/八幡平市地域おこし協力隊/05_個人/knit
最初に読むもの: docs/android-app-vision.md(このアプリの理想の定義。判断に迷ったらここへ戻る)、
docs/product-definition.md(製品の思想)、docs/android.md(既存の adb 方式)、
docs/plans/2026-09-28-android-handoff.md、docs/first-connection.md(登録の仕組み)。
作業前に `source $HOME/.cargo/env`、`git status`、`git log -5` で状態を確認すること。

## 決まっていること(利用者の決定)
- 一般の利用者は開発者向けオプションを使わない。Android の入口は「アプリを入れるだけで使える
  標準モード」にする。担当: 登録、クリップボード・ファイル・URL の共有、疑似カーソルによる操作、
  専用キーボード(IME)による文字入力
- 既存の adb 方式(crates/mac/src/android.rs。本物のポインタと全キー)は、開発者向けオプションを
  使える人のための「精密モード」として残す。同じタブレットが両方で見えても 1 台として扱う
- この決定を docs/product-definition.md と docs/android.md に書き足す

## 前提(前任者の調査。未検証を含む)
- Android では通常のアプリが他アプリへ入力を注入できない。標準モードの手段は
  AccessibilityService(dispatchGesture によるタップ・スワイプ・ドラッグ、performGlobalAction
  による戻る・ホーム・履歴、TYPE_ACCESSIBILITY_OVERLAY の重ね描きで疑似カーソル)と、
  専用キーボード(InputMethodService。InputConnection でキー・文字を送る)
- Mac は相手画面の位置を自分で追い、正規化した絶対座標(Msg::MouseAbs)で送る。疑似カーソルは
  その座標にそのまま描けるので、adb 方式のような加速によるずれは起きない見込み
- Android 10 以降、裏で動くアプリはクリップボードを読めない。既定の IME か前面のアプリなら読める
  (Knit のキーボードが選ばれている間は読める見込み)。書き込みは裏からでもできる
- Google Play は、ユーザー補助の用途外での AccessibilityService の利用を厳しく審査する。
  配布経路(Play・APK 直接配布など)の判断材料を集める
- Xiaomi(HyperOS)などは裏で動くアプリを強く止める。常駐の維持に要る設定の案内が必要になる見込み
- Mac を Bluetooth のキーボード・マウスに見せかける方法は、macOS の CoreBluetooth が該当サービスを
  使わせないため不可(調査済み)

## Knit 側の仕組み(コードで確認済み)
- Mac が待ち受け(TCP 24900)、相手がつなぎに来る。全経路 Noise_NNpsk0_25519_ChaChaPoly_BLAKE2s で
  暗号化(crates/common/src/lib.rs の secure、ラベル b"knit-main")。1 行 1 JSON の Msg
  (同 proto)。接続後に hello(端末 id・画面サイズ・モニター)→ hello_ok。3 秒毎の ping/pong
- 登録は 6 桁コード+SPAKE2(crates/common/src/pairing.rs)。現状は「Mac が招待・Windows が参加」の
  前提で、Windows 固有のラベルがある。製品定義は OS に依存しない端末ごとの登録を求めている
- ファイル・画像は TCP 24902 の大容量経路(同 bulk)。音声は 24901
- 画面端での切替・接続先メニュー・複数台の切替は Mac 側に既にあり、hello で入る相手は
  OS に関係なく「接続先の 1 台」になる(adb 方式の中継もこの形で入っている)

## 進め方
1. 技術の選び方を決めて私に報告する。前任者の推奨は「画面とサービスは Kotlin、暗号化・登録・
   メッセージ形式は knit-common(Rust)を JNI で共有」(互換性を 1 か所で保つため)。
   ビルド環境(JDK・Gradle・NDK。/opt/homebrew/share/android-commandlinetools に SDK と ndk あり)を確認する
2. 小さな試作で次の 4 点を確かめ、結果を docs/plans/ に書いて私に返す(ここで一度止まる)
   - 疑似カーソルでの操作感: クリック・ドラッグ・スクロール・右クリック相当の再現度
   - 日本語入力: Knit のキーボードを選ぶと Gboard 等の日本語変換が使えない問題の解き方
     (例: Mac 側で変換を確定させた文字列を送る)
   - 常駐: Xiaomi Pad(HyperOS 3 / Android 16)で裏に回っても接続が保てるか
   - 配布: Google Play の審査方針と代わりの配布経路
3. 私の判断のあとで本実装に進む。対象は、アプリからの登録、入力、クリップボード、
   URL・ファイルの共有(共有メニューから Mac へ送る)、状態表示、初回案内
   (ユーザー補助とキーボードの有効化の手順)
4. 精密モード(adb)と同じ端末を 1 台として扱う設計を Mac 側に入れる

## 守ること
- 私のタブレット(Xiaomi Pad 6S Pro 12.4、シリアル e569c535)へのアプリの導入や設定の変更は、
  事前に私に聞く。試作はまずエミュレーターで行う
- エミュレーター(pixel_8_shogi)は別プロジェクトの物。使う時は -read-only で起動し、終わったら止める
  (新しい仮想端末を作るのは可)
- 音が鳴る試験をしない
- 利用者の Knit を再起動・再インストールしない(必要なら手順を示して私に頼む)
- 公開(Play への登録・リリース)は私の指示なしに行わない
```

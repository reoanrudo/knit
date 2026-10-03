# タブレット上の日本語入力

Knit IMEはGoogleの公開プロジェクト[Mozc](https://github.com/google/mozc)の変換エンジンと公開辞書を端末内で使用します。入力内容を外部サービスへ送信しません。設定はローマ字入力・MS-IME互換のキー操作です。未確定文字はAndroidの`InputConnection.setComposingText`で対象アプリの入力欄へ表示し、確定時に`commitText`を使用します。候補一覧はMozcのモバイル用`all_candidate_words`から表示します。

ソースの固定版は `a069a88d4cb5c011de0f9aebb6c149a1c808d904`。JNI、辞書、同梱のプロトコル定義をこの版に合わせています。Bazel 9.0.2は上流の`.bazeliskrc`に従い、Android NDKはKnitと同じ28.2.13676358を使います。Javaのprotobuf-lite/protocは4.34.1です。

```sh
scripts/build-mozc-android.sh
scripts/build-android.sh lintDebug assembleDebugAndroidTest
```

`KNIT_MOZC_CACHE`で専用のキャッシュ先、`BAZELISK`で実行ファイル、`KNIT_BUILD_JOBS`で並列数を指定できます。Mac arm64ではBazelisk 1.29.0を固定チェックサムで取得します。その他の環境はBazeliskが必要です。MacではXcode Command Line Tools、LinuxではC++20のホストコンパイラを用意します。

上流のNDKパスを専用キャッシュ内で差し替えます。MacのCLT環境ではSDK内のlibc++ヘッダーを指定し、辞書生成ツールだけをmacOS 13以降向けにビルドします。そのためのapple_support 2.4.0修正は専用コピーだけに適用し、共有BazelキャッシュやKnitのMac版の最低OSは変更しません。生成したAndroidライブラリはarm64-v8a、16KiBページ対応です。

生成物は`app/src/main/jniLibs/arm64-v8a/libmozc.so`と`app/src/main/assets/mozc.data`です。両方ともGit対象外で、APKへ同梱します。辞書は初回起動時にアプリ内へ展開し、版を含むファイル名で管理します。辞書のロード失敗では準備完了を通知しません。

Mozc本体はBSD 3-Clauseです。公開辞書はIPAdic・沖縄辞書の利用条件を含みます。ネイティブ依存のAbseil・protobuf・zlibを含め、著作権表示・利用条件を`app/src/main/assets/licenses/`に保存しAPKにも同梱します。公開辞書とGoogle日本語入力の辞書は異なります。

実行検証は個人情報のない一時Android 16エミュレーター専用です。通常の6桁登録テストとは分け、`SmokeTest`の`mode=tablet_ime`でエンジンの初期化、入力欄の未確定範囲、Space/Enter、候補タップ、全角英数、取消、英数、カーソル編集、周囲の文字の維持、入力欄の切替を確認します。実機上でテスト用の許可設定や固定キーを適用しません。

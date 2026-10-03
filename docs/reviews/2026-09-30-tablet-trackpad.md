# タブレットの表示比率・トラックパッド（2026-09-30）

## 配置図

Macは表示座標、Androidは液晶の生ピクセル数を使い、両者を同じ縮尺で描いていた。
液晶の大きさより解像度が図の大きさを決めていたため、高解像度タブレットがMacより大きく見えた。

Androidの`dumpsys display`の内蔵画面の物理DPIから液晶の寸法を得る。
`wm density`はUIの倍率なので物理寸法には使わない。
Macの`CGDisplayScreenSize`とメイン画面の表示座標から配置図の単位へ換算する。
描画とドラッグ判定は同じ換算後の寸法を使い、入力・通信の解像度は変更しない。

接続中のXiaomi Pad 6S Proから物理DPI約294、液晶約175×263mmを読み取った。
Macメイン画面は約345×223mmだった。横向きタブレットの図の幅はMacの約76%になる。
回転や解像度変更にも追従し、取得不能・不正なDPIでは小さい概算表示に戻る。

## Macのトラックパッド

ユーザーの希望は、Macのトラックパッドでタブレットを操作する方向。

- 縦スクロールは既存の高解像度スクロール注入を維持する。
- AndroidではWindows向けの横スワイプによる戻る・進む判定を通さず、横スクロールを送る。
- Session event tapでジェスチャーを受け、NSEventのtype・phase・magnificationを読み、ピンチを送る。
- 中継でカーソル近傍を中心とする2本指タッチへ変換し、拡大・縮小・終了を送る。
- 終了・取消・Macへの復帰・通信切断・一定時間更新がない場合に両指を解放する。
- 初回は設定の「操作」にピンチのスイッチを追加した。後続の追加で「ジェスチャー」へ移動した。
- Mac操作中とWindows操作中のピンチは変更しない。回転は未対応。3本指は下記の後続追加を参照。

入力にはscrcpyの既存のタッチ制御を使い、Androidへの専用アプリの追加はない。
参考にした一次資料:
[Appleのmagnification](https://developer.apple.com/documentation/appkit/nsevent/magnification)、
[Hammerspoonのイベント型の判定](https://github.com/Hammerspoon/hammerspoon/blob/master/extensions/eventtap/libeventtap_event.m)、
[scrcpyの制御メッセージ読取](https://github.com/Genymobile/scrcpy/blob/master/server/src/main/java/com/genymobile/scrcpy/control/ControlMessageReader.java)、
[scrcpyの複数指の注入](https://github.com/Genymobile/scrcpy/blob/master/server/src/main/java/com/genymobile/scrcpy/control/Controller.java)。

## 検証

| 内容 | 結果 |
|---|---|
| common・Mac通常テスト | 128件成功、0件失敗、5件ignored未実行 |
| 今回追加したテスト | 物理寸法3件、ピンチ状態3件、タッチ形式1件、暗号化通信から指の解放まで1件 |
| 暗号化通信の検証 | ローカルの端末の代役へ開始・更新・Leaveを送り、2本の指のDOWN→MOVE→UPを確認。突然の切断でもUPを確認 |
| Macリリースビルド | 成功 |
| Windows向けテストコードのコンパイル | 成功。新しいメッセージはWindowsでは既存の未知メッセージ処理へ回る |
| AppKitセレクター | SwiftでeventWithCGEvent:・type・phase・magnificationの実在を確認 |
| ネイティブUI | タブレットがMacより小さく描かれること、ピンチ設定とオン・オフ反映を確認 |

プレビューは`--preview-ui --preview-tablet --preview-page 1`で再現できる。
ここでは約12.4インチのタブレットを模した寸法を使い、入力・通信・音声は起動しない。

実際のMacの指操作→イベント取得→Androidアプリの拡大、各アプリの横スクロール方向と慣性は未検証。
常用アプリの差し替えは行っていない。ピンチが有効なアプリでの受け入れ試験が必要。


## 実機の設定ページに合わせた追加

ユーザーが表示したXiaomi Pad 6S Pro / HyperOS 3の設定ページを実画面で確認した。
「戻る」「アプリを切り替える」「ホーム画面に戻る」「最近のタスクを開く」
「スクリーンショットを撮影」の5項目を基準に実装した。
確認用のミラーは既存のscrcpyを音声・クリップボード自動同期なしで起動した。

| ジェスチャー | 判定 / Android側の入力 |
|---|---|
| 2本指で左端または右端から内側 | 両指が端12%以内から同じ向きに移動 / BACK |
| 3本指で左右 | 横の平行移動 / ALT+SHIFT+TAB または ALT+TAB |
| 3本指で上、指を離す | 上移動の後に全接触が終了 / HOME |
| 3本指で上、一時停止 | 約400msの停止、30ms周期の補助タイマー / APP_SWITCH |
| 3本指で下 | 下の平行移動 / SYSRQ |

中央からの横スクロール・指が別方向へ動くピンチ・追加の4本目の指・異常座標では
ナビゲーションを発火しない。発火後は指を離すまで繰り返さず、
最近のタスクを表示した後にホームも送る二重発火を防ぐ。
途中の指離しはホーム操作の終了とし、指を追加し直した場合は取消にする。
700ms以上フレームが途切れた操作を再開扱いにしない。

`TabletGesture { action }`を既存の暗号化経路で送る。
入力待ちキューには判定時の接続世代を付け、旧端末の操作を次の端末に送らない。
Macへ復帰する際には判定を消し、Leaveより後へ操作を持ち越さない。
Android側では残っているピンチの両指を解放した後にOS操作を送る。
アプリ切替のALT・SHIFT・TABも必ず解放する。

設定は新しい「ジェスチャー」ページに集約し、ナビゲーションとピンチを個別に保存する。
windowの寸法は拡大しない。ページに5種類の操作と利用状態を表示する。

### 取得方式と制約

公開NSEventだけではグローバルな指の本数・トラックパッド上の開始位置・停止を
一貫して取得できないため、接触取得を独立モジュールに隔離した。
MultitouchSupportの必要なシンボルを動的に取得し、センサー寸法でTouch Bar等を除外する。
このMacで内蔵トラックパッド1台、センサー22×30を読み取れることを確認した。
必要なAPI・デバイスがない場合はナビゲーションを無効にする。

非公開APIのため、構造体・コールバックABIはAppleによる互換保証がない。
現時点で商用向けの安定性を確認済みとは扱わない。OS更新、スリープ復帰、
トラックパッドの再接続後の監視再登録、および実際の指での反応と
HyperOSのアプリ切替方向・スクリーンショット動作は実機で確認が必要。
macOS自身のMission Control等が先に処理する場合には、macOSの3本指操作を4本指へ変更する。

参照した一次資料:
[Appleのトラックパッドイベント説明](https://developer.apple.com/library/archive/documentation/Cocoa/Conceptual/EventOverview/HandlingTouchEvents/HandlingTouchEvents.html)、
[接触構造体の実装資料](https://github.com/lauschue/Remotastic/blob/main/MultitouchSupport.h)、
[センサー寸法によるデバイス選別の実装](https://github.com/SomeGuyNamedDaveIsTaken/macOSMiddleClick)、
[AOSPのSYSRQ・ALT+TAB処理](https://android.googlesource.com/platform/frameworks/base/+/refs/heads/android11-release/services/core/java/com/android/server/policy/PhoneWindowManager.java)。

### 追加分の検証

- 指の位置・本数・方向・停止・指離し・重複発火・失効を純粋な入力列で検証。
- 各操作のJSON往復とキー押下の釣り合い、修飾キーの解放を検証。
- localhostのNoise暗号化接続からAndroid中継へ送り、scrcpy制御パケットの
  指解放・OSキーコード・ALT/SHIFT/TAB解放まで検証。実機への入力や音声は伴わない。
- 全通常テスト138件成功（common62、共通統合5、Mac71）、5件の無視指定は実行しない。
  最終ログ: `/tmp/knit-gesture-all-tests.log`。
- 最終のMacリリースビルド成功: `/tmp/knit-gesture-release.log`。
  Windows側のテスト対象を含むコンパイル成功: `/tmp/knit-gesture-win-check.log`。
- 同時に着地した最初のジェスチャーが設定・モード切替後も受け付けられること、
  操作中の接触は接続先やモードを切り替えると取り消されることも検証。
- `git diff --check`成功。
- 設定画面の実表示はMacのロック解除待ち。プレビュー起動の試行は表示確認の成功と扱わない。
- 常用アプリの置換・再起動と、実指での5種類の操作確認は未実施。

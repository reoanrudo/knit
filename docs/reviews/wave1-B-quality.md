# Wave1-B 機能別コード品質レビュー(第三者・読み取り専用・辛口)

- 実施日: 2026-09-25 / エージェント型: read-only-reviewer
- レビュー対象: ~/ZCodeProject/seamless-desk(読み取り専用・実行なし)
- 方針: 「動いているから正しい」を証明とみなさず、コードが偶然成立している箇所を特定した。推測はその旨を明記する。

## 総評

実績のある現場対応の積み重ねは本物で、Mutex 毒化回復・二段階境界判定・自己修復等多くの防御が効いている。一方で、(a) 既定オフの代替モード(相対移動)が実質使用不能、(b) ObjC の autorelease 無し運用による確定リーク、(c) Windows 側ソケットへのマルチスレッド書き込み、(d) 検証・ログ運用資材の再現不能性、という「本番がたまたま単一経路で動いている間だけ隠れる」欠陥が複数ある。状態遷移(WIN_MODE 入退出)が 5 か所にコピーされている構造が保守性の最大の負債。

## 機能×軸評価表(1=壊れている 〜 5=商用品質)

| # | 機能 | 正確性 | 堅牢性 | 保守性 | UX | 主根拠(file:line) |
|---|------|--------|--------|--------|-----|------------|
| 1 | 画面端切替 | 3 | 4 | 2 | 4 | ~/ZCodeProject/seamless-desk/crates/mac/src/main.rs:607-699(積算+16毎同期+二段階判定は良好)。だが UNION_MAX_X/EDGE_DISP_Y/SCREEN_W/H は起動時固定でディスプレイ構成変更に非追従(844-864)。tap_callback 内に約100行の切替分岐、マジックナンバー 8.0/40.0/15.0(627,638,655) |
| 2 | ダブルタップ切替 | 3 | 3 | 3 | 4 | mac/main.rs:643-661「下から跨いだ瞬間のみ」計数と 700ms 窓は実装どおり。跳ね返し 15px の体感設計(654-659)は優秀。閾値(700/15/8)が分散し docs と不一致(後述) |
| 3 | ホットキーロック | 2 | 4 | 2 | 4 | mac/main.rs:559-588 に到達不能デッドコード: 外側条件が `KEY_DOWN \|\| FLAGS_CHANGED` のため内側 `event_type == EVT_KEY_UP`(562)は永不成立。F13 keyup は握られず WIN 中 Key 転送される(714-732)。improvement-log「F13 up握り」(ループ24〜33)と実装が不一致 |
| 4 | 絶対位置送信 | 4 | 3 | 3 | 4 | mac/main.rs:739-775 クランプ・at_left 判定は正確。デッドロック回避(749-751)がコメント頼りで構造保証なし。スケール計算がインライン(742-747)でテスト不能 |
| 5 | 相対移動(fallback) | **1** | 2 | 3 | 2 | **P0バグ**: mac/main.rs:765 の LAST_ABS_MS 更新は abs ブランチのみ。ウォッチドッグ(1286)は MOUSE_ABS_MODE を見ないため、rel モードで WIN 中にマウスを動かすと 5 秒で必ず誤強制復帰。rel は事実上使用不能 |
| 6 | スクロール変換 | 4 | 4 | 3 | 4 | mac/main.rs:794-809(0.25ノッチ量子化・暴発ガード)+ win/main.rs:269-293(×120 注入)の整合。Q=0.25/div=120/120(WHEEL_DELTA)が各所で magic |
| 7 | キーマップ変換 | 3 | 4 | 4 | 3 | ~/ZCodeProject/seamless-desk/crates/common/src/lib.rs:143-243 は一覧性良好。テンキー Enter(76)/=(81) 欠落、JIS レイアウト依存キーは実機検証頼み。win/main.rs:566-571 で未マップキーが無言で沈黙(ログなし) |
| 8 | IME制御 | 4 | 3 | 4 | 4 | win/main.rs:75-96 WM_IME_CONTROL による方向指定は正攻法。フォールバック(92-94)はトグルで方向不保証(usage.md:136-137 に明記あり=誠実)。IME 不成立時の利用者へのフィードバックはログのみ |
| 9 | cmd+Tab→Alt+Tab | 3 | 3 | 3 | 4 | win/main.rs:540-564。cmd 押下維持のリピート・離下確定は正しく動く。ALT_TAB_ACTIVE 中に他キーを混ぜると mods.apply が入り Alt+Tab UI 状態と干渉しうる(565)。切断時の Alt 押しっぱなしは 673 の release_all で救済 |
| 10 | クリップ・テキスト双方向 | 3 | 4 | 3 | 3 | win/main.rs:466 同一内容の再コピーは last_sent 一致で送信されない。上限の単位不整合(mac は UTF-8 バイト mac/main.rs:136、win は UTF-16 要素 win/main.rs:105,123)。占有リトライ(658-659)・毒化回復は良好 |
| 11 | クリップ・画像 Win→Mac | 3 | **2** | 3 | 3 | mac/main.rs:239-286 は autorelease pool 無しで NSString/NSData/TIFF が毎回リーク(確定)。dib_to_bmp(207-230)は bpp<=4 パレット・BI_BITFIELDS 非対応(推測: 実データは32bppが大半のため潜在)。usage.md:64,132 は「画像未対応」と記載し実装と矛盾 |
| 12 | 接続通知 | 4 | 3 | 4 | 3 | mac/main.rs:312-324。通知許可がないと静かに失敗し `let _ = out` で握られる(323)。ユーザーは「通知が出ない」ことに気づけない |
| 13 | 再接続・ハートビート | 3 | 4 | 3 | 4 | win/main.rs:390-403 バックオフ、mac/main.rs:940-949 pong 監視は良好。切断時(1080-1087)WIN_MODE=false にするのみで leave_win_mode_cursor_unlock を呼ばず、カーソル後片付けを自己修復スレッドに丸投げ(usage.md:71「即 Mac 復帰(入力の閉じ込め防止)」と実装が乖離) |
| 14 | カーソル非表示/復帰 | 4 | 3 | 3 | 4 | mac/main.rs:440-441/492-496 の対称フラグ管理は正しい。show 3 連呼(493-495)は自認の対症療法。CGWarpMouseCursorPosition の戻り値無視(459,525,656,1300) |
| 15 | 復帰経路6本 | 3 | 4 | **1** | 4 | 同じ状態遷移(store(false)+leave)が 5 か所に重複: mac/main.rs:566-587 / 770-775 / 1034-1043 / 1282-1291 / 1259-1269。状態機械不在。経路⑥(ウォッチドッグ)は機能5を誤発火させる |
| 16 | ドラッグ持ち込み防止 | 3 | 3 | 4 | 3 | mac/main.rs:671-673 左ボタンのみ。右/中ドラッグ持ち込みは未対応(インベントリ自己申告どおり)。win/main.rs:673,693 の release_all は裏付けあり |
| 17 | チャタリング防止 | 4 | 4 | 3 | 4 | mac/main.rs:481(EDGE_GUARD 400ms)+ win/main.rs:688(0.7s クールダウン)の二重防御は整合。400/700/60/150px が分散 |
| 18 | Windows非表示常駐 | 3 | 3 | 3 | 3 | **run_sd.bat のログローテーションは実経路から不使用**: 起動は run_sd.vbs:2 が exe 直(`>>` 追記)で、run_sd.bat を呼ぶ経路が存在しない → sd-win.log 無限増殖。install.bat:14-17 も VBS を登録 |
| 19 | 自動検証 | 2 | 3 | 3 | 3 | verify.sh:55,70 の `sd_clip_set`/`sd_clip_get` タスクはリポジトリ内のどの資材(install.bat 含む)にも登録処理が無く、検証が過去に手動で作った環境依存のタスクに乗っている=新環境で再現不能 |

## P0(修正最優先)

**P0-1: 相対モードでウォッチドッグが必ず誤発火し、rel モードが事実上使用不能**
- File: ~/ZCodeProject/seamless-desk/crates/mac/src/main.rs:765(WINDOW: LAST_ABS_MS は abs ブランチのみ更新)、同 1282-1291(ウォッチドッグは MOUSE_ABS_MODE を判定しない)
- 問題: `SEAMLESS_MOUSE_MODE=rel` で WIN 中、マウスを動かしている限り `last_ev < 2000` が成立し、`last_abs` は 5 秒以上前(または 0)のため「転送停止」と誤判定され強制復帰する。
- 影響: 「絶対/相対双方がビルド・動作すること」という守るべき動作の違反。誰も rel で 5 秒以上使ったことがなければ未顕在化=たまたま動いていないだけ。
- 修正案: ウォッチドッグの条件に `MOUSE_ABS_MODE.load()` を加える(rel 時は LAST_ABS_MS の代わりに MouseMove 送信時刻を使うか、監視自体を無効化)。壊すリスク低。検証: `SEAMLESS_MOUSE_MODE=rel ./scripts/restart-mac.sh --diag` で WIN 突入後 10 秒間マウスを動かし `[watchdog]` ログが出ないことを確認。

## P1

**P1-1: ObjC autorelease pool 無しでオブジェクトが毎回リーク(テキスト・画像同期)**
- File: ~/ZCodeProject/seamless-desk/crates/mac/src/main.rs:171-179(nsstring)、239-286(画像)、289-304(読み取り)
- 問題: `stringWithUTF8String:`/`dataWithBytes:`/`TIFFRepresentation` 等は autorelease オブジェクトを返す。受信スレッド(mac/main.rs:1027-1079)と監視スレッド(1223-1247)には drain する NSAutoreleasePool が存在しない。画像同期 1 回で BMP+TIFF 数 MB 規模が解放されない。
- 影響: 長期運用でメモリ単調増。スナップショットの貼り付けを繰り返すと顕在化。
- 修正案: クリップボード操作関数を `NSAutoreleasePool(new)/drain` で挟む(objc_msgSend 直呼びでも `objc_getClass("NSAutoreleasePool")` + `new`/`drain` で可能)。または objc2-app-kit への置換(後述 unsafe 項)。検証: Leopard 以降 `leaks`/Instruments で同期前後の差分確認。

**P1-2: Windows 側 TcpStream へのマルチスレッド書き込み(行インターリーブの可能性)**
- File: ~/ZCodeProject/seamless-desk/crates/win/src/main.rs:443-485(cb_writer)、508-509(Ping/Pong)、690(Return)
- 問題: 受信ループとクリップボード監視スレッドが同一ソケットの clone にそれぞれ `writeln!` する。`writeln!` は1回の write syscall を保証せず、並行書き込みで JSON Lines の行が壊れうる。Mac 側は送信を単一スレッドに集約している(mac/main.rs:924-957)のに Win 側は集約していない。
- 影響: 壊れた行は decode 失敗で黙視(mac/main.rs:1000,1032 の `.ok()`)され、Return/Pong が消える。Pong 欠損は 10 秒後の偽切断(944-949)を招く。競合頻度は低いが、これは「たまたま動いている」典型。
- 修正案: Win 側も channel + 単一送信スレッドへ集約(Mac 側と対称に)。検証: クリップ画像送信中に ping 電文の健全性を Mac 側 decode 失敗カウンタで監視(要ログ追加)。

**P1-3: dev 既定トークン + restart-mac.sh が .env を source しない → 本番が弱い既定トークンの疑い**
- File: ~/ZCodeProject/seamless-desk/crates/mac/src/main.rs:834、crates/win/src/main.rs:340-341(既定 "seamless-desk-dev")、scripts/restart-mac.sh:5,30(.env 未読込)
- 問題: 環境変数が無ければ無条件で dev トークンで動く。restart-mac.sh は `$HOME/.cargo/env` しか source しておらず、.env 由来のトークン供給経路がスクリプトに存在しない。schtasks 起動の sd-win 側もユーザー環境変数に依存。
- 影響: tailnet 上の任意のノードが 24900 に接続して hello を通せばリモート入力注入が可能。認証は hello の1回だけのため、接続後は無制限。
- 修正案: (a) トークン未設定なら起動拒否(推奨)、(b) restart-mac.sh で .env を source、(c) dev 既定の廃止。検証: トークン無しで起動した際 fatal で終了すること、ssh home のタスク定義にトークン供給が含まれることの確認。
- 注: ユーザーシェル環境に SEAMLESS_DESK_TOKEN が export 済みなら nohup に継承される。その場合でも「設定なしで動いてしまう」構造は残る(推測として付記)。

**P1-4: run_sd.bat のログローテーションが実起動経路で使われていない(ログ無限増殖)**
- File: ~/ZCodeProject/seamless-desk/win-dist/run_sd.vbs:2(exe を `>>` 追記直起動)、win-dist/install.bat:14-17(VBS を登録)、win-dist/run_sd.bat:3-6(ローテーションはこちらにあるが誰も呼ばない)
- 問題: improvement-log(ループ59〜63)の「run_sd.bat ログローテーション」は資材上デッド。機能18のレビュー観点「ログローテーション」は現状成立していない。
- 修正案: VBS のコマンドを `run_sd.bat` 経由に変更(ローテーションは1世代だが無限増殖は止まる)。検証: タスク再実行ごとに sd-win.log.old が更新されることを ssh で確認。

**P1-5: verify.sh のクリップ検証がリポジトリ外の手動作成タスクに依存(検証の再現不能)**
- File: ~/ZCodeProject/seamless-desk/scripts/verify.sh:55,70(`schtasks /Run /TN sd_clip_set` `/TN sd_clip_get`)
- 問題: この2タスクを登録する資材がリポジトリに存在しない(install.bat は seamless_desk / seamless_desk_run のみ作成)。失敗は `>/dev/null 2>&1` で握られ、sleep 後の比較が NG(fail)として計上される。
- 影響: 「verify.sh 7項目合格」は現在の Windows 機の状態に依存し、再構築・別機移行で再現できない。検証インフラとしての信頼性を損なう。
- 修正案: clip_set.bat / clip_get.bat と対応タスクの登録を install.bat(または専用スクリプト)に含め、verify.sh の失敗を WARN 分離。検証: 新規フォルダからの install → verify で同一結果になること。

**P1-6: WIN モード中の切断(BYE/損失)でカーソル後片付けを自己修復スレッドに丸投げ**
- File: ~/ZCodeProject/seamless-desk/crates/mac/src/main.rs:1080-1087(STORE のみ、leave 不呼び出し)
- 問題: 切断時 `WIN_MODE.store(false)` のみで、leave_win_mode_cursor_unlock 相当(suppression 解除・EDGE_GUARD 設定・CUR_POS 反映)が無い。カーソル復元は 1253-1306 の自己修復(最大 150ms 遅延・warp なし)頼み。usage.md:71「Windows モード中に切断したら即 Mac モードへ復帰」との乖離。
- 修正案: 切断処理で `leave_win_mode_cursor_unlock(None)` を呼ぶ(WIN_MODE 判定を付けて冪等化)。検証: WIN 中に Windows 側 taskkill → Mac カーソルが表示され右端内側に復帰すること、`[cursor] self-heal` ではなく正常 leave 経路のログが出ること。

**P1-7: F13 keyup 把握のデッドコード(到達不能分岐)**
- File: ~/ZCodeProject/seamless-desk/crates/mac/src/main.rs:559-565
- 問題: 外側 `if event_type == EVT_KEY_DOWN || event_type == EVT_FLAGS_CHANGED` の内側で `event_type == EVT_KEY_UP` を判定しており、永不成立。F13 keyup は MAC モードで素通し、WIN モードで Windows へ Key 転送される(105 は keymap に無く注入されないため実害小)。improvement-log ループ24〜33 の意図と不一致。
- 修正案: 外側条件に EVT_KEY_UP を含め、ホットキー keycode の up/down を最初に判定して握る。ネストも平坦化。検証: `--debug-keys` 相当の Win 側ログで F13 up が届かないこと。

**P1-8: ディスプレイ構成変更(解像度・モニター増減)に全く追従しない**
- File: ~/ZCodeProject/seamless-desk/crates/mac/src/main.rs:838-864(SCREEN_W/H, UNION_MAX_X, EDGE_DISP_Y が起動時固定)
- 問題: プロジェクタ接続、モニター抜挿、解像度変更後に境界判定・復帰位置・スケールが旧座標のまま誤動作する。レビュー観点1が指摘する通り。CGDisplayRegisterReconfigurationCallback を使えば通知を受けられる。
- 修正案: ディスプレイ再構成コールバックで UNION_MAX_X/EDGE_DISP_Y/SCREEN_W/H を再計算(OnceLock を AtomicFx64 等に置換)。検証: WIN 中にモニターを取り外し、境界が新レイアウトに追従すること。

**P1-9: Win→Mac の同一内容再コピーが永遠に同期されない**
- File: ~/ZCodeProject/seamless-desk/crates/win/src/main.rs:466(テキスト last_sent 内容比較)、454(画像 last_img 比較)
- 問題: Windows のクリップボードに同一内容を再セットしても changeCount は増えるが、Win 側は内容比較でスキップする。Mac 側(mac/main.rs:1231-1235)は changeCount ベースなので再送される — 非対称。「同じスクショをもう一度コピー」が Mac に届かない。
- 修正案: Win 偆にクリップボード sequence number(GetClipboardSequenceNumber)ベースへ変更。検証: 同一テキスト連続コピーで両方向とも 2 回目が届くこと。

## P2

- **P2-1: ドキュメントとコードの数値・仕様の食違い(多数)**: docs/usage.md:20「250ms」→実装は 400ms(mac/main.rs:481)。usage.md:58「0.25 秒」→実装 200ms(mac/main.rs:1226, win/main.rs:449)。usage.md:94「500ms以内」→実装 700ms(mac/main.rs:650)。usage.md:43-44「32 イベント」→実装 16(mac/main.rs:612-616)。usage.md:64,132「画像や書式は未対応」→機能11は実装済み。usage.md:29-33 の IME 説明は ImmGetContext/ImmSetOpenStatus になっているが実装は ImmGetDefaultIMEWnd+WM_IME_CONTROL(win/main.rs:75-96)。共通資料インベントリ13「バックオフ 0.5s→max5s」も実装は max3s(win/main.rs:402)。docs/design.md:78-79 も旧構成の記述が残る。改善のたび docs が置き去りになっている証拠。
- **P2-2: 初回切替の「境界の高さ対応」が丸ごとデッドコード**: mac/main.rs:666-669 で計算した `ny` は 676 行の shadow で無効。`if nx < 0.0` 分岐(677-688)は LAST_WIN_POS 初期値 (0.05,0.5)(mac/main.rs:375)のため永不成立。usage.md:10「同寸法の高さで境界を越える」は初回に限り偽(常に前回位置または中央)。デッドコード削除と usage 訂正を推奨。
- **P2-3: serve() の Msg::Clip に hello_done ガードがない**: win/main.rs:653(他メッセージは全て `if !hello_done { continue; }` あり)。現状 Mac は hello_ok 後しか送らないため顕在化しない=たまたま。防御の一貫性欠如。
- **P2-4: クリップ上限の単位不整合**: mac/main.rs:136 は UTF-8 バイト、win/main.rs:105,123 は UTF-16 要素数。上限付近の巨大テキストで片方向だけ落ちうる。
- **P2-5: ClipData 受信のサイズ上限チェックなし**: mac/main.rs:1044-1056 は Win 側の 5MB b64 制限(win/main.rs:454)のみが頼り。TIFF 変換のメモリ膨張も無上限。
- **P2-6: 接続 2 本同時 accept 時の状態交錯**: mac/main.rs:1016-1024 は STREAM_SLOT を差し替えるが旧受信スレッドが残存(スレッドリーク+旧スレッドが Return を処理しうる)。旧接続の切断が CONNECTED=false を書き、生きている新接続と矛盾(1084)。
- **P2-7: abs-left 復帰直後の Windows 側 Return 二重復帰**: mac/main.rs:770-775 と 1034-1043 の競合で、復帰後に再度ワープされうる(improvement-log「次候補」に自己申告あり)。Return 受信時に WIN_MODE が既に false ならスキップするガードで済む。
- **P2-8: Windows マルチモニタ非対応**: win/main.rs:363-364(SM_CXSCREEN=プライマリのみ)、235-243(MOUSEEVENTF_ABSOLUTE はプライマリ基準)。MOUSEEVENTF_VIRTUALDESK 未使用。単画面前提の実測とはいえ docs の既知制限に未記載。
- **P2-9: deploy-win.sh:18 のクオート脆弱+`|| true` によるエラー握り**: ローカルシェルの二重クオート解析が複雑で、schtasks が失敗しても握り潰す。install.bat の既存タスクが生きているから動いている=たまたま。ヒアドキュメント+失敗時の明示で改善可能。
- **P2-10: restart-mac.sh:13 のビルド失敗検出が grep 汎用に依存**: `set -o pipefail` なし。grep が `^error` を拾えない失敗形式(リンクエラーの変種など)では古いバイナリのまま BUILD_ID だけ更新され再起動する。「鮮度保証」が自己矛盾する状態を作りうる。`cargo build ... || exit 1` の素直な形に戻すべき。
- **P2-11: 死蔵コード・資材の掃除去**: win/main.rs:19(GetAsyncKeyState 未使用 import)、30(VK_LBUTTON_SENTINEL)、mac/main.rs:823-827(--host 未使用)、365 と 376 の重複ログ、1092-1188 の test/test2 プロダクト混入、win-dist/run_sd.bat(不使用)、Msg::Focus/Minimize は test2 専用。
- **P2-12: キーマップ欠損**: common/src/lib.rs:223-239 テンキー Enter(76)/Equal(81) 等なし。未マップ時のログ出力もない(win/main.rs:566-571)ため、沈黙するキーの発見が困難。
- **P2-13: トークン比較が非定数時間**: mac/main.rs:1001 `t == token`。Tailscale 内とはいえ指摘として留める。

## unsafe レビュー(全棚卸し)

Mac(crates/mac/src/main.rs):
- objc_msgSend transmute 方式(143-169): 方式自体は正しい(可変引数宣言での直接呼び出しによるレジスタ不一致回避)。**代替**: objc2 / objc2-app-kit クレート。`NSPasteboard` の読み書きは unsafe ブロックをほぼ消せる上、autorelease pool も `objc2::rc::autoreleasepool` で提供され、P1-1 のリークが構造的に解消される。GUI 昇格(本次テーマ)でも AppKit を直叩きし続けるより移植価値が高い。
- nsstring 系(171-186): 戻りが autorelease のため pool なしでリーク(P1-1)。
- mac_set_clipboard / mac_set_clipboard_image_bmp(189-286): 同上。msg2_bool の u8 戻りは ARM64 BOOL(sign-true=1)と整合。
- mac_get_clipboard(289-304): `UTF8String` のポインタは autorelease オブジェクト参照。`to_string_lossy().into_owned()` でコピー後に pool を drain すれば安全。
- CGSSetConnectionProperty(418-429): key の CFRelease あり。kCFBooleanTrue はグローバル定数のため release 不要で正しい。
- live_cursor(384-392): probe の CFRelease あり。正しい。
- CFMachPortCreateRunLoopSource(1339-1343): 取得した src を CFRelease していない(CF Create 規則上 1 オブジェクトの持ち逃げ。実害は起動時 1 回限り)。
- 非メインスレッドからの AppKit(NSPasteboard)呼び出し(306-308, 1027-1079, 1223-1247): AppKit のスレッド安全性はメインスレッド推奨。NSPasteboard は実際には動くが保証外=たまたま動いている領域。objc2 化の際にメインスレッドへ dispatch する設計推奨。

Win(crates/win/src/main.rs):
- clipboard_read_text(107-135): GlobalLock/Unlock/CloseClipboard の対称性 OK。`len <= CLIP_MAX_CHARS` は null 終端前提(CF_UNICODETEXT は保証あり)。
- clipboard_read_dib(138-161): GlobalLock 失敗時も GlobalUnlock を呼ぶが無害。slice::from_raw_parts は GlobalSize 境界内で OK。
- clipboard_write_text(163-192): SetClipboardData 失敗時の GlobalFree 責務(184-187)は Win32 規則どおりで正しい。
- InputBuf パック(195-243): assert でサイズ検証(205)。`dx as u32`(224-231)は負数の 2 の補数ビット保持で MOUSEEVENTF の LONG 解釈と一致。正しい。
- OpenInputDesktop/SetThreadDesktop(349-361): desk ハンドルを CloseDesktop していないが、スレッドへの割当は維持されるため実害はプロセス終了までの 1 ハンドル。成否チェックはある。
- enum_cb(704-714): バッファ 256、len クランプなしの `&buf[..len.max(0) as usize]` — GetWindowTextW は最大 nCount-1 文字+null を返す仕様のため安全。

## unwrap/expect/Result 無視の棚卸しとポリシー割当

- `.lock().unwrap_or_else(|e| e.into_inner())`(mac 全27か所、win 657): **回復**ポリシーで一貫。正当。
- `STREAM_SLOT.get().unwrap()`(mac 930,946,950,1017,1081): main で set 済みのため安全だが、OnceLock 初期化順序に暗黙依存。`if let Some` 形へ降格推奨(保守性)。
- `serde_json::to_string(msg).unwrap_or_default()`(common/src/lib.rs:71): 現実に失敗しないが、失敗時は空行を送り相手が黙視する。**ログ**すら出ない。空文字列なら送信自体をスキップすべき。
- `let _ = tx.send(...)`(mac 537)、`set_nodelay().ok()` 系(mac 982-984, win 407): 受信者存続・受信エラー検知を別経路で担保済み。**無視**容認。
- `let _ = writeln!(writer, ...)`(win 508-509): 失敗は read ループで検知される。容認。
- `CGWarpMouseCursorPosition` 戻り値無視(mac 459,525,656,1300): 失敗時 CUR_POS と実位置が乖離するが 16 イベント毎同期(612-620)で回復。**ログ**のみ付ける価値。
- notify の `let _ = out`(mac 323): 失敗が完全に不可視(P2)。stderr 1 行でよい。
- `send_input_buf` の assert(win 205): release でも有効。起動時 1 回の検証として容認。
- ポリシー提案: 「回復(into_inner 系)/ログ(外部 I/O 失敗)/落とす(起動時の必須要件失敗=現状の [fatal] exit と整合)」の3分類を docs に明文化し、`unwrap()` の新規追加を禁止するだけで現状の質は維持できる。

## ログ体系

現状は eprintln!/println! + 手書き `[tag]`(conn/clip/edge/mode/ime/return/watchdog/cursor/diag)。grep しやすく実運用に合っているが、(a) レベル分けなし(1 行の診断と通常動作が同格)、(b) diag の集計値とタグ付きログの突き合わせが手作業、(c) tap_callback 内の eprintln は stderr ロック+リダイレクト先詰まり時にメイン RunLoop をブロックしうる。tracing 導入の価値は中程度: 接続単位の span(再接続のたびに新 span_id)で「どの接続のどの電文か」が追跡でき、P1-2 の検証(行壊れ検知)にも使える。ただしまずは「diag の構造化(JSON 出力)+ tap_callback 内からのログ退避(channel 経由)」だけで当面の実利は取れる。優先度は P0/P1 の後。

## 単体テスト可能な切り出し箇所(純粋ロジック)

1. dib_to_bmp(mac/main.rs:207-230): 純関数。bpp4/8 パレット、異常 biSize、BI_BITFIELDS のベクトルテストが即書ける(現状 b64 しかテストがない中で最優先)。
2. スクロール量子化(mac/main.rs:794-809): `accumulate_scroll(acc, dx_px, dy_px, div) -> (acc', out)` として切り出し。1e6 ガード含め検証可能。
3. 境界タップ状態機械(mac/main.rs:605-699): 位置と時刻を注入する構造体にすれば「700ms 窓・跳ね返し・guard・二段階判定」をシミュレート可能。改善履歴が示す通りここが最多バグ領域で、回帰テストの価値が最も高い。
4. ModState::apply(win/main.rs:296-333): inject を trait で抽象化すれば差分注入の順序と過不足を検証可能(cmd+Tab 変換との組み合わせ含む)。
5. inject_scroll の符号・丸め(win/main.rs:269-293)。
6. 絶対座標スケール・クランプ(mac/main.rs:742-760): `update_virtual_cursor(wc, dx, dy, scale, screen) -> (nx, ny, at_left)`。
7. maybe_notify_return のクールダウン(win/main.rs:680-695): Instant を注入。
8. keymap 網羅性(common/src/lib.rs:143-243): JIS ¥(93)/_(94) の往復と、意図的欠落リスト(104/102/105 等)の表明テーブル。
9. b64: 既存テストあり(common/tests/b64_roundtrip.rs)。パディング不正入力のネガティブケース追加余地。

## 共通資料(依頼文)とコードの食違い

- 「バックオフ 0.5s→max5s」(インベントリ13) → 実装は max 3s(win/main.rs:402)。
- 「16イベント毎同期」(インベントリ1)は正しい。usage.md/improvement-log の「32」が誤。
- 「700ms窓」(インベントリ2)は正しい。usage.md:94 の「500ms」が誤。
- 「200msポーリング」(インベントリ10)は正しい。usage.md:58 の「0.25秒」が誤。
- 「ping/pong 5秒毎、15秒無応答」(インベントリ13) → 実装は ping 3秒毎・10秒タイムアウト(mac/main.rs:940-949)。usage.md:68 は正しい。
- sed の `&` エスケープ(パターン8)は現在の両スクリプトで守られている(`\&str`。restart-mac.sh:9, deploy-win.sh:9)。
- 「Mutex 毒化回復 27箇所」は mac 側実測と概ね一致。
- 機能15「タップ定期再有効化 約1秒毎」は実装どおり(150ms×7=1.05s。mac/main.rs:1273-1277)。
- 機能19「verify.sh 7項目」: pass 計上されるのは最大6項目(IME は実績がない限り WARN。verify.sh:83-95)。

## まとめ

「動いている」状態のかなりの部分が、(1) 既定モード(abs)しか使っていない、(2) クリップの同一内容再コピーをしない、(3) ディスプレイ構成を変えない、(4) 画像同期を長期間繰り返さない、(5) 接続が 1 本しか張らない、という単一運用経路に支えられている。GUI 昇格・モジュール分割の前に、P0(rel モード誤発火)と P1-1〜P1-6(リーク・書き込み競合・トークン・ログ増殖・検証再現性・切断時後片付け)を潰すことを推奨する。いずれも既存の守るべき動作(abs/edge/IME/Alt+Tab/双方向クリップ)を壊さない範囲で修正可能であり、検証は既存の deploy→verify サイクルに「rel モード 10 秒操作」「同内容再コピー」「画像連続貼り付け×10 のメモリ観察」の 3 項目を足すことで足りる。

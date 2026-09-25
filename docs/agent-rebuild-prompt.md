# seamless-desk 第三者再構築エージェント プロンプト集

作成日: 2026-09-25
目的: 開発者本人(および開発に関与したLLM)のバイアスを排除するため、**本プロジェクトに一度も関与していない第三者エージェント**に
品質評価・改善方法の再構築・GUIアプリ化を担当させる。本書はそのための完全自己完結プロンプトである。

---

## 第1部【共通資料】全エージェントが最初に読む技術資料

(※ 以下を各エージェントの prompt の先頭に連結して使用すること)

```
あなたは seamless-desk プロジェクトに一度も関与していない第三者のシニアエンジニアです。
開発者の希望的観測を排し、本質から疑う辛口のレビューを求めます。顔は立てなくてよい。
「動いているから正しい」は証明になりません。「たまたま動いているだけ」の箇所を特定してください。

## プロジェクト概要

Mac のキーボード/トラックパッドで、同一ネットワーク上の Windows デスクトップを
Deskflow(Synergy後継)と同じ感覚(画面端で切替)で操作する Rust 製入力共有ツール。
CLI の2バイナリ構成で現在本番稼働中。今回の目標は「品質の再構築」と「GUI付きアプリへの昇格」。

- リポジトリ: ~/ZCodeProject/seamless-desk
- crates/mac/src/main.rs   (1347行) Mac側サーバ sd-mac
- crates/win/src/main.rs   ( 725行) Windows側クライアント sd-win
- crates/common/src/lib.rs ( 262行) プロトコル・キーマップ・base64
- scripts/{restart-mac.sh, deploy-win.sh, verify.sh} 検証・デプロイ
- win-dist/{run_sd.vbs, install.bat, run_sd.bat} Windows常駐起動
- docs/{design.md, usage.md, improvement-log.md}

## 環境(実測で確定済み・変更不可の前提)

- Mac: macOS arm64 (MacBook + 外付けウルトラワイド 2560x1080 + 右サブモニターの3画面)
- Windows: 別マシン、管理者権限あり、対話セッション運用
- 通信: Tailscale (100.84.0.2, RTT 約6ms)。WiFi AP隔離のため Mac→Win 接続は最初の1パケットで落ちる
  → **Mac=サーバ(TCP:24900待受)、Windows=クライアント(接続し続ける)の逆転構成は環境制約で確定**
- ビルド: Mac は aarch64-apple-darwin ネイティブ、Windows は Mac から x86_64-pc-windows-gnu クロスビルド+scp 配布
  (Windows側での cargo 実行は不可。ssh ホスト名は `home`、Windows側パスは C:\Users\<user>\seamless-desk)
- 認証: 事前共有トークン(環境変数/.env から供給、コミットしない)
- 通信は生TCP+JSON Lines(serde)。TLSは Tailscale が WireGuard 暗号化を持つため省略(設計判断)

## 現在の機能インベントリ(全19機能。各機能の実装方式と注意点)

| # | 機能 | 実装方式 | レビュー観点 |
|---|------|---------|------------|
| 1 | 画面端切替(edge) | delta積算のCUR_POS+16イベント毎同期、二段階判定(積算超過かつライブ位置が境界付近)。CGGetActiveDisplayListで全ディスプレイ和集合右端UNION_MAX_X、出口ディスプレイy範囲EDGE_DISP_Yを記録 | 積算とライブの乖離、3画面レイアウト変更への追従 |
| 2 | ダブルタップ切替(既定ON) | 「下から閾値を跨いだ瞬間」のみヒット計数、700ms窓2回。1回目は境界-15pxへ跳ね返し(CGWarpMouseCursorPosition)。SEAMLESS_EDGE_TAPS=1で1回切替に戻せる | 跳ね返し体感、窓長さ、誤カウント |
| 3 | ホットキーロックモード | --hotkey / SEAMLESS_SWITCH_MODE=hotkey。F13(変更可)でトグル、切替後は境界を超えても戻らないロック | hotkeyモード中の復帰経路整合 |
| 4 | 絶対位置送信モード(既定ON) | Mac が Windows 仮想カーソル WIN_CUR(f64)を管理し MouseAbs(0..1正規化)を毎イベント送信→Win は MOUSEEVENTF_MOVE\|MOUSEEVENTF_ABSOLUTE で注入。方向別スケール。二重加速(Windows加速曲線)を回避 | 高速移動時の精度、毎イベント送信の帯域 |
| 5 | 相対移動モード(fallback) | MouseMove + Windows側サブピクセル累積(f64累積し整数部のみ注入) | 絶対モードとの挙動一致度 |
| 6 | スクロール変換 | Mac のピクセルdeltaを SCROLL_DIV(120)で除算→0.25ノッチ量子化→Win はノッチ×120で注入 | 慣性スクロールの再現性、除数の妥当性 |
| 7 | キーマップ変換 | common の keymap。Cmd→Ctrl / Option→Alt / Control→Win。93=¥→0xDC, 94=_→0xBD | 欠損キー(日本語キーボード固有)、修飾キー置換のエッジ |
| 8 | IME制御 | かな(104)→ON / 英数(102)→OFF。Win側は ImmGetDefaultIMEWnd+SendMessageW(WM_IME_CONTROL, IMC_SETOPENSTATUS)。失敗時 VK_KANJI 注入フォールバック(トグルのため方向保証なし) | フォールバックの方向不確定性、IMEon状態での切替 |
| 9 | cmd+Tab→Alt+Tab変換 | Win側 ALT_TAB_ACTIVE static、cmd離下で確定、cmd分のVK_CONTROL同時押下を抑制 | エッジ(押したまま切替、他キー混在) |
| 10 | クリップボード・テキスト双方向 | 両側200msポーリング、changeCount比較、1MB上限、Win→MacはCRLF→LF正規化、書き込み失敗時150msリトライ | ポーリング vs 通知、巨大テキスト、競合 |
| 11 | クリップボード・画像(Win→Mac) | CF_DIB→GlobalLock→自前base64→ClipData→Mac: DIB→BMP変換(biSizeはDIB先頭4バイト)→NSBitmapImageRep→TIFF→NSPasteboard。last_img比較でループ防止 | Mac→Win方向が未実装(TIFF→DIBはGDI+必要で棚上げ)。形式制約 |
| 12 | 接続通知 | Mac側から osascript で通知センター表示 | osascript 起動コスト、失敗時の扱い |
| 13 | 再接続・ハートビート | Win側指数バックオフ(0.5s→max5s)再接続。ping/pong 5秒毎、15秒無応答で切断扱い。WINモード中の切断は即Mac復帰(入力閉じ込め防止) | 切断検知の遅れ、半開接続 |
| 14 | カーソル非表示/復帰管理 | enter時: set_cursor_in_background→hide(1回)→suppression 0.0001→associate(false)→LOCK_X(UNION_MAX_X-2)へワープ。leave時: LAST_WIN_POS記憶→EDGE_GUARD 400ms→show 3回連呼→suppression 0→EDGE_DISP_Y基準のyで境界内側へワープ→CUR_POS反映 | Deskflow OSXScreen.mm 準拠。show連呼は対症療法か |
| 15 | 復帰経路6本 | ①abs-left(Win仮想カーソル左端) ②Windows側Return通知(左端x<=1+700msクールダウン) ③F13 ④自己修復(WIN外なのに非表示を150msで検知し復元) ⑤タップ定期再有効化(約1秒毎・冪等) ⑥ウォッチドッグ(WIN中、タップ受信<2s なのに abs送信5秒停止で強制復帰) | 6本もの保険が必要な構造的脆弱性の根本原因 |
| 16 | ドラッグ持ち込み防止 | 切替時 Windows 側へ左ボタン解放送信、復帰時 mods.release_all | 抜け穴(他ボタン、修飾キー) |
| 17 | チャタリング防止 | EDGE_GUARD_UNTIL_MS(復帰後400ms判定無効)+Win側クールダウン0.7s | 二重防御の整合 |
| 18 | Windows非表示常駐 | run_sd.vbs(wscript が cmd /c をリダイレクト付きで非表示起動)+schtasks。MainWindowHandle=0を確認済み | ログローテーション、自動起動の堅牢さ |
| 19 | 自動検証 | verify.sh 7項目(両プロセス/ssh/established/sd-winプロセス/クリップ双方向/IMEログ/diag集計) | 実操作(E2E)を検証できない箇所 |

## 実装上の重要パターン(ここを崩すと実績のあるバグが再発する)

1. **objc_msgSend は可変引数宣言での直接呼び出しでは SIGSEGV(PAC failure)を出す実績**。
   呼び出しシグネチャごとに `std::mem::transmute(objc_msgSend as usize)` した固定シグネチャで呼ぶ方式を採用。
   NSPasteboard 系には `#[link(name = "AppKit", kind = "framework")]` が必須。
2. **全 Mutex ロックは `.lock().unwrap_or_else(|e| e.into_inner())`** で毒化回復(27箇所)。パニック後も凍結しない。
3. **CGEventTap のタイムアウト無効化**(0xFFFFFFFE/0xFFFFFFFD への再enable)は取り逃されることがあるため、
   監視スレッドが約1秒毎に冪等に再有効化する。
4. **切替判定は2段階**: 積算CUR_POSが閾値超過 + ライブカーソル(CGEventCreate(NULL))が境界付近(edge-40以内)。
   片方だけでは誤発火/取り逃しの実績あり。
5. **絶対位置モードの復帰処理で MutexGuard を保持したまま leave_win_mode_cursor_unlock を呼ぶと自己デッドロック**する実績。
   ガードはブロック内で解放してから leave を呼ぶ。
6. **復帰時はワープ後に CUR_POS をワープ先で上書き**する(順序を誤ると境界値が残存し無関係の位置で再突入した実績)。
7. **Windows への SendInput 系は対話デスクトップ必須**: OpenInputDesktop+SetThreadDesktop を起動時に実行。
   SSH 起動では別デスクトップ(1024x768)に飛ぶため schtasks 対話起動を使う。
8. sed で BUILD_ID をスタンプする際、置換側の `&` は `\&` にエスケープ必須。

## 守るべき動作(本番稼働中。ここを壊す変更は不可)

- 絶対位置送信モード(既定ON)/相対モードの双方がビルド・動作すること
- ダブルタップ切替・ホットキーロックモードの両モード
- IME(かな/英数)、cmd+Tab→Alt+Tab、クリップボード(テキスト双方向+画像Win→Mac)
- 6本の復帰経路(削減は可、ただし削減理由を明示し検証すること)
- Windows非表示常駐(VBS+タスク)と verify.sh 7項目合格
- 環境変数による既存チューニング(SEAMLESS_* 系)の互換(設定ファイル導入時は環境変数をフォールバックに)

## 既知の棚上げ・未解決(第三者視点で再評価してよい)

- Mac→Win 方向の画像クリップボード(TIFF→DIB は GDI+ デコードが必要)
- WIN_CUR の Mutex を毎イベントロック(パフォーマンス改善候補: AtomicF64 化等)
- 1347行の単一 main.rs(モジュール分割は今回の主要テーマ)
- TLS(Tailscale 既暗号化のため後回し)、スクリーンセーバー同期、LaunchAgent常駐化
- クリップ監視が両側ポーリング(200ms)
- テストは common の b64 ラウンドトリップのみ。mac/win に単体テスト・結合テストが無い

## 検証・デプロイ手順(変更後は必ずこのサイクルを回す)

1. `cargo build --release` (Macネイティブ) / `cargo build --release --target x86_64-pc-windows-gnu`
2. `cargo test --workspace`
3. Mac 再起動: `scripts/restart-mac.sh`(BUILD_ID スタンプ+起動確認)
4. Windows 配布: `scripts/deploy-win.sh`(scp + schtasks 再実行、/End→taskkill→/Run)
5. `scripts/verify.sh` 7項目合格を確認
6. 実機ログ: Mac /tmp/sd-mac-run.log、Win C:\Users\<user>\seamless-desk\sd-win.log
   (ssh home で Get-Content。`[edge]`/`[return]`/diag 行が切替挙動の証拠)
```

---

## 第2部【役割別ミッション】(共通資料に連結して使う)

### A. アーキテクチャレビュー(architecture-reviewer 系)

```
## あなたのミッション(アーキテクチャレビュー)

crates/mac/src/main.rs が 1347 行の単一ファイル。static なグローバル状態が20個超。
GUI アプリ化(メニューバー常駐+設定画面)を念頭に、以下を評価・提案せよ。

1. モジュール分割案: どの責務をどこへ切り出すか。ファイル名と移動対象を具体的に。
   目標は「GUI から状態を観測・操作できる境界」が作れる構造。
   例: input(タップ)/ switching(切替状態機械)/ net(接続)/ clipboard / cursor / config / ui
2. グローバル static の AtomicBool/Mutex/OnceLock 群をどう構造化するか。
   (全てを一気に struct へ集約するのか、状態を単一 ownership にするスレッド設計にするのか)
3. メッセージプロトコル(common::Msg)の拡張性。GUI・設定同期・統計を足すときの形状。
4. Mac/Win で重複する概念(クリップ監視、キュー、再接続)の共通化可否。
5. リスク: 大規模リファクタは動作実績を壊す。分割の段取り(ストラングラーパターン、
   機能フラグでの並行稼働等)を、各段階で verify.sh が緑のまま進むよう設計せよ。

出力: 現状評価(問題一覧+深刻度)→ 目標アーキテクチャ図 → 段階的移行計画(各ステップの検証方法付き)。
```

### B. 機能別コード品質レビュー(read-only-reviewer 系)

```
## あなたのミッション(コード品質レビュー)

機能インベントリ19項目のそれぞれについて、該当コードを読み、以下の4軸を5段階で評価せよ。
(1=壊れている〜5=商用品質)。全項目で一律の点数にならないこと。根拠には必ず file:line を添える。

- 正確性: 仕様どおりに動くか。エッジケースでの挙動
- 堅牢性: パニック・毒化・切断・権限消失時の挙動。エラーの握り潰し
- 保守性: 関数の長さ、責務の混在、マジックナンバー、命名
- UX: 遅延、体感の滑らかさ、失敗時の利用者への伝わり方

追加で:
1. unsafe ブロックの全部レビュー(特に objc 呼び出し・生ポインタ)。代替可能な安全なAPIはあるか
2. unwrap/expect/無視される Result の棚卸しと、それぞれポリシー(回復/ログ/落とす)の割当
3. ログ: 現状の println! の体系。tracing 等の構造化ログ化の価値
4. テスト: mac/win の純粋ロジック(スケール計算、量子化、DIB変換、タップ判定の時間窓等)を
   単体テスト可能な形に切り出せる箇所の特定

出力: 機能×軸の評価表(根拠 file:line 付き) → P0/P1/P2 の改善リスト。
```

### C. エッジケースハンティング(edge-case-hunter 系)

```
## あなたのミッション(エッジケースハンティング)

過去に「戻れなくなる」系のバグを5回以上出している。次の観点で、まだ起きていない最悪シナリオを特定せよ。

1. 競合・レース: タップコールバック vs 監視スレッド vs 受信ループの3者。ロック順序、
   チェック-アンド-アクトの隙間、swap/compare_exchange の粒度
2. 状態の組み合わせ爆発: {WIN_MODE × CURSOR_HIDDEN × CONNECTED × HOTKEY_ONLY × EDGE_AT_EDGE × ガード中}
   の全組合せで到達不能・脱出不能な組合せはあるか(カーソル消失・入力閉じ込えの温床)
3. ネットワーク異常: 半開、切断検知15秒間の入力喪失、Windowsスリープ/再接続直後、
   hello 再交換時の WIN_SCREEN/WIN_CUR 初期化競合
4. ディスプレイ変更: 接続中にモニター増減/解像度変更/ミラーリングで UNION_MAX_X と
   EDGE_DISP_Y が陳腐化する経路(現状は起動時に1回のみ計算)
5. CGEventTap 権限: 画面録画/アクセシビリティ権限が実行中に剥奪された場合の検知と復帰
6. Windows 側: セッションロック(UACプロンプトのセキュアデスクトップ)中の SendInput 行き先、
   管理者権限プロセスへの入力、IME が無効なアプリでのかな/英数の挙動

出力: シナリオ一覧(発生条件→現在の挙動→想定被害→再現手順)を深刻度順に。
```

### D. パフォーマンスレビュー(performance-reviewer 系)

```
## あなたのミッション(パフォーマンスレビュー)

このツールの生命指標は「入力遅延」と「カーソル運動の滑らかさ」(過去にカクつきで苦情があった)。

1. ホットパスの分析: タップコールバック→シリアライズ→送信(毎イベント)。アロケーション、
   Mutex ロック回数、serde_json の文字列生成コスト。 Zero-copy/再利用バッファ化の余地
2. WIN_CUR を毎イベント Mutex ロックしている箇所の AtomicF64 等への置換可否
3. ポーリング周期(クリップ200ms、監視150ms)の妥当性とイベント駆動化の費用対効果
4. ネットワーク: MouseAbs を毎イベント送る現状の帯域(60〜120Hz×約90B)は問題か。
   変化時のみ送信・バッチング・バイナリプロトコル化の得失
5. osascript による通知の起動コストと非同期化

出力: 計測可能な仮説(どこが何µs/msか) → 実測方法 → 改善の優先順位(体感インパクト順)。
```

### E. セキュリティレビュー(security-reviewer 系)

```
## あなたのミッション(セキュリティレビュー)

認証は事前共有トークンのみ、通信は Tailscale 前提の平文 TCP+JSON。

1. トークンの取り扱い: .env/環境変数からの供給、ログへの漏出、メモリ上の滞留
2. ネットワーク: TCP:24900 が Tailscale 外(IF追加時)に露出した場合の影響。
   トークン無しhelloの処理、JSON DoS(巨大メッセージ)、未認証でのリソース消費
3. 入力値検証: 受信 Msg の各フィールド(nx/ny の範囲、data のサイズ上限、text の制御文字)。
   異常値が objc/SendInput へ届く経路
4. Windows 側の SendInput は実質リモートコード実行と等価。接続可否の制御は十分か
   (ピン留め、単一ホスト制限等)
5. schtasks/VBS 常駐の権限(管理者?)と侵害された場合の爆発半径

出力: 脅威一覧(可能性×影響) → 即効ある対策 → Tailscale 前提を維持した範囲での推奨。
```

### F. UX本質レビュー(辛口・第三者)

```
## あなたのミッション(UX本質レビュー)

ユーザーは「まだストレスを覚える」「本質がわからない」と繰り返してきた。開発者視点を捨て、
初めて触るユーザーになりきって本質から疑え。対抗馬は Deskflow/Synergy(実績20年)と
Logitech Flow である。

1. 画面端切替+ダブルタップという現在の対話モデル自体は正しいか。
   跳ね返し15px・窓700msは伝わるか。1回切替(taps=1)を既定にすべきでないか
2. 「Windowsに行けない」「戻れない」が5回発生した履歴。ユーザーは原因を知るすべが無い。
   失敗が起きた瞬間、ユーザーは何を見るべきだったか(通知・音・メニューバー表示)
3. カーソル速度・スクロール速度の体感一致は「絶対位置モード」で本当に解決したか。
   トラックパッドの加速曲線(Mac)を Windows 側で再現できているか
4. 接続・切断・遅延・現在モードの可視性。全部ログにしか出ていない
5. セットアップの険しさ: .env、schtasks、権限許可。一般ユーザーが通れるか
6. これさえあれば他に要らない、という1つの核心体験は何か。それを損なう要素は

出力: 現状UXの辛口評価(10点満点で採点し理由を述べよ) → 核心体験の定義 →
体験を損なう欠陥のランキング → 改善提案(GUI化で解決すべきもの/しないものを選別)。
```

### G. GUI設計・実装(builder 系・Wave2で実行)

```
## あなたのミッション(GUIアプリ化の設計と実装)

seamless-desk を「アプリ」に昇格させる。CLI は従来どおり daemon 的中核として残し、
macOS 側に GUI を持たせる。Windows 側は現状のトレイなし常駐のままでよい(優先度低)。

### 要件

1. メニューバー常駐(NSStatusItem):
   - アイコンで状態表示: 接続済/切断/エラー(+現在モード Mac/Windows の区別)
   - メニュー項目: 「Windows へ切替 / Mac へ戻る」(手動トグル)、状態行(接続先・遅延・BUILD_ID)、
     設定…、ログを開く、再起動、終了
2. 設定画面(簡素でよい。メニューから開く):
   - 切替モード(境界/ダブルタップ回数/タップ窓ms/ホットキーモード+キー)
   - マウス(絶対/相対、倍率)、スクロール除数
   - ネットワーク(ポート、トークンは参照のみで編集不可でもよい)
   - 変更は設定ファイルに永続化。既存 SEAMLESS_* 環境変数は「設定ファイル<環境変数」の
     優先順で互換維持(未設定項目のみ環境変数フォールバック)
3. 実装方式の選定(以下を比較し、理由を示して1つ選べ):
   a) 現行方式の延長: objc_msgSend 固定シグネチャで NSStatusItem/NSMenu/NSWindow を直叩き
      (依存追加ゼロ。ただし設定UIを素のAppKitで組む労力)
   b) objc2 + objc2-app-kit クレートによる型安全な AppKit(依存追加、学習コスト、静的リンク確認要)
   c) Swift でフロントを書き Rust はソケット/ファイルで状態共有(ビルド系が複雑化)
   ※ 過去の実績: objc_msgSend 可変引数呼び出しは SIGSEGV。固定シグネチャtransmute方式は実績あり
4. 状態の接続: GUI は中核プロセスとどう通信するか。同一プロセス内で並走させるか、
   別プロセス+IPC(Unix socket/JSON)にするか。中核を落とさず GUI だけ差し替え可能な構造
5. LaunchAgent 化(ログイン時自動起動)も設定に含めよ

### 制約
- 動作中の入力パス(タップコールバック)に GUI 処理を入れて遅延を加えないこと
- 既存 CLI 起動(restart-mac.sh)と GUI 起動の両方が verify.sh で緑になること

出力: 方式選定の理由 → 画面・メニューの仕様(項目一覧) → 設定ファイル形式とスキーマ →
実装計画(ファイル構成、段階、各段階の検証)。
```

---

## 第3部 実行計画(オーケストレータ用)

1. **Wave 1(並列・読み取り専用)**: A〜F の6エージェントを並列起動。
   モデルは GLM-5.3-FlashX 指定(CreateWorkflow の subagent_model)。
   各エージェントには「第1部共通資料+各自のミッション」を連結した完全自己完結プロンプトを渡す。
2. **Wave 2(統合)**: レビュー結果を統合し、機能別品質評価表と P0/P1/P2 改善リスト、
   GUI 方式選定を確定。矛盾する指摘は第三者視点で裁定。
3. **Wave 3(実装)**: 承認された順に P0→GUI基盤→P1 の順で実装。
   各変更で cargo build/test → restart-mac.sh → deploy-win.sh → verify.sh を回す。
   実装は1機能ずつコミット(メッセージは「改善ループN(第8セッション)」形式を継続)。

## 成果物の受け渡し形式(全エージェント共通)

- 指摘には必ず 該当コードの file:line を添える
- 意見には根拠(コード実測/実行ログ/Deskflow等の先行実装との比較)を添える。推測は「推測」と明示
- 改善案には「壊すリスクと検証方法」を必ず添える

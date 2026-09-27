# 機能別 品質向上プロンプト集(Tsunagu)

機能一つ一つの品質を上げるためのセッション用プロンプト集。
「共通の土台」+ 対象機能のブロックを連結して、新しい ZCode セッションの先頭に貼る。

## 使い方

1. ZCode を `tsunagu` ディレクトリで開き、新しいセッションを始める
2. 「共通の土台」を貼る
3. 手を入れたい機能のブロックを土台の直後に連結して貼る
4. セッションは 1 機能ずつがおすすめ(棚上げが深くなり、コミットも追いやすい)
5. 実機が絡む検証(3 スクリプト等)はエージェントが項目リストを出すので、
   こちらで実行して結果を貼る

注意: LAN 直優先設計(docs/superpowers/specs/2026-09-26-lan-direct-design.md)が
承認待ちの間は、接続先解決(機能 1・機能 8)の仕様変更はこの設計と競合させないこと。

---

## 共通の土台(必ず貼る)

```text
Tsunagu(Mac⇄Windows 入力共有ツール、Rust)の「機能品質向上」セッションです。
リポジトリ: 

最初に必ず docs/agent-guide.md を読んでください(構成・環境固有の罠 16 項目・
コード規約・既知の制限の最重要事項が書いてあります)。

絶対守ること:
- 応答はすべて日本語(です/ます調)。事実と推測を分け、未検証は未検証と明記する
- cargo を使う前に必ず `source $HOME/.cargo/env` を実行(PATH 上の cargo には
  Windows ターゲットが無い)
- 実機配備(scripts/restart-mac.sh / deploy-win.sh / verify.sh / dev.sh)は必ず私が
  実行する。あなたは実行しない。実機検証が必要になったら項目とコマンドを提示して待つ
- 音が出るテストは事前に私の承認を得る(無断トーン事故の実績あり)
- トークン(TSUNAGU_TOKEN 等)の値は画面に出さない。~/.config/tsunagu/env の旧名
  キー(SEAMLESS_DESK_*)のフォールバックは消さない
- 未追跡の output/・AGENTS.md・.omc/ には触らない
- Deskflow(/tmp/deskflow、GPL-2.0)は設計参照のみ。コードは写さない
- コミット形式: 「改善ループN(第Nセッション): <要約>」+ 末尾に Co-Authored-By 行。
  N は git log の最新の「改善ループN」の番号 +1

進め方(この順で):
1. 指定箇所のコードと直近のログを読み、現状の要約をまず返す(まだ変更しない)
2. 品質リスクを棚上げする: 正常系が仕様どおりか / 異常系(切断・再接続・巨大データ・
   空データ・タイムアウト)/ 境界 / レース条件 / リソース漏れ / agent-guide の
   既知の罠との関連
3. 棚上げたリスクを優先度付きで私に提示し、どれから手を付けるか相談してから着手
4. 合意した項目から、自動テストできるものはテストを先に書き、最小スコープで改善
5. ビルドと全テストが通ることを確認してコミット
6. 実機で確認すべき項目をコマンド付きでリスト化し、私の実行結果を待つ

環境の事実(2026-09-26 時点):
- Mac = サーバ(192.168.0.1 Wi-Fi / Tailscale 100.100.10.9)
- Windows = クライアント(192.168.0.2 有線 / Tailscale 100.84.0.2、ssh ホスト `home`)
- ポート: 24900 本線(JSON Lines の Msg)/ 24901 音声 / 24902 bulk / 24903 UDP 発見
- プロトコル版 11(MIN_VERSION)。全 TCP は Noise NNpsk0 で暗号化
- v0.23.0 を実機配備済み。verify.sh は pass=17 fail=0

今回の対象機能:
```

---

## 機能 1: 本線接続と経路品質

```text
今回の対象機能: 本線接続・経路選択・keepalive(TCP 24900)

実装箇所:
- crates/common/src/lib.rs: connect モジュール(parse_hosts/resolve/first_reachable)、
  secure モジュール、Msg 定義(Ping/Pong)
- crates/win/src/main.rs: client_loop(再接続ループ。接続ごとに resolve を呼び直す)、
  ping 送信(ts: 0 送出)
- crates/mac/src/main.rs: 待ち受け、RTT_MS static(表示用)、Tailscale 経路診断
  (30 秒毎・通知のみで自動回復なし)

現在の状態:
- Windows は TSUNAGU_HOST のカンマ区切り候補へ同時接続レース、最初に繋がった採用
- keepalive: 3 秒毎 ping、pong 10 秒途絶で切断扱い→再接続ループ
- docs/superpowers/specs/2026-09-26-lan-direct-design.md(LAN 直優先)が承認待ち。
  接続先解決の仕様変更はこの設計と競合させない

品質向上の観点(例):
- 切断→再接続時の音声・bulk の PEER 追従との整合(取り残し・空白時間)
- ハンドシェイク失敗の要因別ログと統計
- keepalive 閾値の妥当性(Wi-Fi 環境での誤切断)
- 接続確立までの時間の計測
```

## 機能 2: キーボード入力共有と IME

```text
今回の対象機能: キーボード入力共有(キーマップ・修飾キー)と IME

実装箇所:
- crates/mac/src/main.rs: CGEventTap でのキー捕獲、Mac→Win への送出
- crates/win/src/main.rs: SendInput による注入(vk + scan 併用が IME 互換の鉄則 —
  agent-guide 罠 9)
- crates/common/src/lib.rs: キーイベントの Msg 定義

現在の状態 / 実機未検証:
- ターミナルの Control↔Ctrl マッピング: 未検証
- IME: Mac で日本語入力中の Windows 側への送出挙動は未検証(IME 同期は残課題)
- ⌘] 等のキー送信 E2E は手動検証領域(agent-guide 参照)

品質向上の観点(例):
- 修飾キーの組み合わせ漏れ(Cmd/Win/Alt/Ctrl の同時押し)
- キーリピート(押しっぱなし)の挙動
- 日本語キーボードの英数/かな・数字 row の keycode(罠 10: 18=1…29=0)
- 高速タイプ時の取りこぼし
- IME オン時の設計(確定文字だけ送る等)の確認と整備
```

## 機能 3: マウス入力と画面端切替

```text
今回の対象機能: マウス移動・クリック・スクロール、画面端での切替、ゲームモード

実装箇所:
- crates/mac/src/main.rs: CGEventTap でのマウス捕獲、画面端判定(境界閾値は
  AtomicU64 で実行中可変)、カーソル退避
- crates/win/src/main.rs: SendInput での注入(相対移動)、スクロール除数・
  カーソル速度(AtomicU64 で可変)

実機未検証:
- ゲームモード: 未検証
- ファイル転送中にマウス入力が止まらないこと: 未検証

品質向上の観点(例):
- 感度・加速の自然さ(カーソル速度/スクロール除数の既定値)
- 画面端の意図しない切替(角の扱い・境界閾値)
- スクロールの方向・量の自然さ
- 高負荷時のカーソル飛び・移動イベントのバッファリング
```

## 機能 4: クリップボード同期

```text
今回の対象機能: クリップボード双方向同期(テキスト・画像・ファイル)

設計原則(変更しない):
- 「画面を移る時」だけ同期する(Mac: enter_win_mode、Win: Leave 受信)。
  コピー毎に送る実装へ戻さない(大容量ファイルの無駄転送・秘匿データ流出の原因)

実装箇所:
- crates/mac/src/main.rs: sync_clipboard_to_win(LAST_SYNC_COUNT / LAST_SENT_FILES で
  重複除外)、pb_files(readObjectsForClasses: NSURL + FileURLsOnly)
- crates/win/src/main.rs: クリップボード監視と Win→Mac への送出
- 画像・ファイルの実体は 24902(bulk)経由

実機未検証:
- 画像双方向(Mac→Win は CF_DIB 互換性に懸念。BMP V4/V5 ヘッダを受け付けない
  アプリがある可能性)
- Finder での実 ⌘C によるファイルコピー(verify.sh は Swift writeObjects で代替)

既知の罠:
- osascript の「POSIX file」クリップボード書き込みは型ゼロの空ペーストボードに
  なることがある(検証スクリプトは Swift writeObjects 方式を使う)

品質向上の観点(例):
- 巨大クリップボード(数百 MB 画像)時のブロック・メモリ
- 同期ループ(自分が入れた物が戻る)防止の堅牢さ
- 他アプリの同時書き換え時の changeCount 除外の取りこぼし
- ファイル複数選択・フォルダの扱い
```

## 機能 5: ドラッグ&ドロップ(掴みドラッグ)

```text
今回の対象機能: Mac で掴んだまま Windows へ渡すドラッグ&ドロップ

実装箇所:
- crates/mac/src/main.rs: 掴み検出スレッド(押下開始時点からの changeCount 変化基準)、
  自己投稿イベントの識別(kCGEventSourceUserData=41 のマジック — 罠 15)
- crates/win/src/main.rs: dragdrop.rs(OLE の IDataObject/IDropSource/IEnumFORMATETC を
  自前定義。vtbl の並びは MSDN のメソッド順 — 罠 16。並び違いくらいで即クラッシュ)

実機未検証 / 残課題:
- ドラッグの各アプリでの成否は手動検証領域
- Win→Mac 方向の掴みドラッグ: 残課題(未実装)

品質向上の観点(例):
- ドラッグキャンセル時の状態残留(NSPasteboardNameDrag はキャンセル後もクリアされ
  ないことがある — 罠 14)
- ドラッグ種別の網羅(テキスト/ファイル/URL)
- ドロップ先の形式要求(CF_HDROP/CF_UNICODETEXT 等)への追従
```

## 機能 6: bulk 転送(ファイル・画像)

```text
今回の対象機能: ファイル・画像の bulk 転送(TCP 24902、256KB チャンク)

実装箇所:
- crates/common/src/lib.rs: bulk モジュール(バイナリフレーム化)
- crates/mac/src/main.rs: mac_on_bulk(受信側。Downloads/Tsunagu へ保存)、
  送信開始のトリガ
- crates/win/src/main.rs: bulk::connect_loop(本線が選んだ Mac アドレスへ追従)

品質向上の観点(例):
- 巨大ファイル転送中の切断(中断・再開の要否)
- 送信中のバックプレッシャ(本線・音声を優先)
- 同一名ファイルの衝突(上書き/リネーム)
- 受信側のディスク残量不足などの異常系
- 複数ファイルの連続送信
```

## 機能 7: 音声ストリーミング

```text
今回の対象機能: 音声ストリーミング(TCP 24901、48kHz s16 ステレオ ≈192KB/s)

実装箇所:
- crates/mac/src/main.rs: 音声キャプチャ・送出
- crates/win/src/main.rs: 再生
- PCM は 8 バイト境界を厳守(罠 11: 境界外ドロップは恒久的な位相ずれ=破壊音)

実機未検証:
- 音声デバイス(既定出力)の切替: 未検証
- 音が出る検証は事前に私の承認を得てから実施すること

品質向上の観点(例):
- バッファアンダーラン/オーバーラン(ドロップアウト)
- レイテンシとバッファサイズのトレード
- デバイス抜き差し・既定変更時の復帰
- 送受信のビット正確性(境界処理)
```

## 機能 8: LAN 発見と接続先解決

```text
今回の対象機能: LAN 自動発見(UDP 24903)と接続先の解決

実装箇所:
- crates/common/src/lib.rs: discover モジュール(room_id/seek/seek_lan/respond)、
  connect モジュールの resolve
- crates/win/src/main.rs / crates/mac/src/main.rs: discover::respond の常駐

現在の状態:
- トークンの BLAKE2s ハッシュで部屋 ID を生成し、UDP ブロードキャストで問い合わせ/応答
- seek_lan は 255.255.255.255 宛に 1.5 秒待ち
- resolve は TSUNAGU_HOST 指定時はホストリストのみ、未指定時は seek_lan のみ(排他)
- docs/superpowers/specs/2026-09-26-lan-direct-design.md(発見 ∪ 手動指定の併合設計)が
  承認待ち。接続先解決の仕様変更はこの設計と競合させない

品質向上の観点(例):
- 複数ネットワークインターフェース時のブロードキャスト先
- 応答のなりすまし耐性(部屋 ID による絞り込みの強度)
- 発見失敗時のフィードバック(ユーザーへの通知)
```

## 機能 9: ディスプレイ構成(DPI・マルチモニター)

```text
今回の対象機能: マルチモニター・DPI・モニター構成変化への追従

実装箇所:
- crates/mac/src/main.rs: 画面端判定に使うディスプレイ情報の取得・再計算
- crates/win/src/main.rs: Windows 側の画面構成

実機未検証 / 残課題:
- Windows マルチモニター: 未検証
- Mac のモニター抜き差し: 未検証
- Windows DPI 非対応: 残課題(対応するとトレイ/設定窓の描画倍率が変わるため
  GUI 側と同時に対応する)

品質向上の観点(例):
- モニター抜き差し時の切替境界の再計算タイミング
- DPI 100% 以外での座標ずれ
- ミラーリング時の挙動
```

## 機能 10: ロック・電源連動

```text
今回の対象機能: 画面ロック・電源まわりの連動

実機未検証 / 残課題:
- Mac ロック連動(ロックで入力を止める等): 未検証
- Windows のロック画面・UAC 画面は操作不可: 構成上の制限(SYSTEM サービス化が必要。
  対応するなら設計から)

品質向上の観点(例):
- ロック中の入力抑止/解除の取りこぼし
- スリープ・ディスプレイオフへの追従
- ロック連動の ON/OFF 設定の整備
```

## 機能 11: 運用品質(ログ・スクリプト・構造)

```text
今回の対象機能: 運用・保守性(ログ、スクリプト、コード構造、配布)

残課題:
- main.rs 分割(mac/win とも大きい)
- ログ統一・ローテーション([conn]/[bulk]/[audio] 等のタグはあるが統一基準が無い)
- 切替状態の一元化
- 設定のプリセット化
- 配布(ペアリング、コード署名、公証、Universal Binary、インストーラ、ライセンス、商標)

品質向上の観点(例):
- ログの構造化(タグ・レベル・経路の統一、量の抑制)
- スクリプト(dev.sh/restart-mac.sh/deploy-win.sh/verify.sh)のエラー処理と冪等性
- BUILD_ID スタンプの整備(現状デプロイ副産物として未コミット運用)
- 振る舞いを変えない保守性改善に限定する(機能追加は別セッションで)
```

---

## 機能 12: IME 引継ぎ(IME Follow Cursor)

```text
今回の対象機能: Windows へ入る瞬間の IME 状態同期(Msg::Ime)

実装箇所:
- crates/mac/src/main.rs: ime_mode_state(判定の純関数・テストあり)、
  current_ime_state(TIS の InputModeID 取得)、enter_win_mode_cursor_lock 内の送信、
  IME_SYNC(TSUNAGU_IME_SYNC=0 で無効)
- crates/win/src/main.rs: Msg::Ime 受信→ime_set_open_impl(kana, false)
  (WM_IME_CONTROL/IMC_SETOPENSTATUS。トグルフォールバックはしない)
- crates/common/src/lib.rs: Msg::Ime(版 11 のまま。旧側は未知行として無視)

現在の状態:
- Mac が Apple 純正の日本語入力(かな系=ON/Roman=OFF)の間だけ同期。
  英字レイアウト・サードパーティ IME は None=送らない(勝手に変えない)
- 手動のかな/英数キー(kc=104/102)は従来どおり受信ループで IME 開閉へ変換
  (IME ウィンドウが取れない窓では VK_KANJI のトグルへフォールバック)

品質向上の観点(例):
- 切替直後のフォアグラウンド確定前に ime_wnd が取れないタイミングの実測
- IMC_GETOPENSTATUS で現状を読める窓では「既に目標状態なら送らない」最適化
- Leave 時の逆方向(Win→Mac。TISSelectInputSource)は未実装(usage.md の既知の制限)
- 実機ログ: win 側「[ime] mac の状態へ同期」「WM_IME_CONTROL open=.. -> sent」
```

## 機能 13: Continue Here(⌥⌘T のブラウザ引継ぎ)

```text
今回の対象機能: Mac の前面ブラウザの URL を Windows の既定ブラウザで開く

実装箇所:
- crates/mac/src/main.rs: frontmost_browser_url(System Events で前面アプリ特定+
  try で防御した AppleScript)、continue_here(本体・60 秒デッドマン通知)、
  tap 内の ⌥⌘T 傍受(押下エッジ+1.5 秒デッドタイム+up も握る)
- crates/win/src/main.rs: Msg::OpenUrl 受信→open_default_browser(ShellExecuteW)
- crates/common/src/lib.rs: Msg::OpenUrl と urlx::transferable
  (http/https・2048 文字・制御文字/空白拒否。送受信両側で検査)

現在の状態:
- 読み取り対象は「実際に前面にある」Safari/Chrome/Edge/Brave の前面タブのみ
  (裏で常駐のブラウザは読まない=固定順探査の事故を 477 で修正済み)
- 初回は Mac 側に TCC(自動化)の許可ダイアログが出る。失敗時は通知 1 回

品質向上の観点(例):
- 逆方向(Windows の前面ブラウザ→Mac で開く)は未実装。Windows 側は
  UI Automation でアドレスバーを読む必要がある(ブラウザごとの UIA 名に注意)
- タブの選択状態(アクティブウィンドウが PWA/DevTools)の実測パターン収集
- スクロール位置・選択テキストの引き継ぎ(ビジョン§11 の拡張)
- 実機ログ: mac 側「[url] Continue Here: 送信しました」
```

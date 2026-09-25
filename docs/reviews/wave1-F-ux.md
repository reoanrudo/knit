# Wave1-F UX本質レビュー(第三者・辛口)

- 実施日: 2026-09-25 / エージェント型: general-purpose(Web検索による Deskflow/Logitech Flow 比較を含む)
- 読み込み対象: crates/mac/src/main.rs, crates/win/src/main.rs, crates/common/src/lib.rs, docs/{design.md, usage.md, improvement-log.md}, README.md, scripts/{restart-mac.sh, deploy-win.sh, verify.sh, check-mouse.sh}, win-dist/{run_sd.vbs, install.bat, run_sd.bat}。読み取り専用で作業(ビルド・実行は未実施)。実機挙動に関する断定できない事項は「推測」と明示する。

---

## 1. 現状UXの辛口評価: 5.0 / 10

**機能点(高)と体験点(低)が極端に分かれる製品。**

高い点は、Deskflow/Synergy が20年で酸っぱくならなかった領域に正面から食い込んでいることだ。具体的には、 (a) MOUSEEVENTF_ABSOLUTE で Windows 加速曲線を回避し Mac の加速済 delta をそのまま再現する絶対位置モード(mac/main.rs:739-767, win/main.rs:235-243)、(b) かな/英数キーによる IME 方向指定(win/main.rs:75-96 の WM_IME_CONTROL)、(c) JIS ¥/_ キーマップ(common/src/lib.rs:193-194)。この3点は Logitech Flow にも Synergy にもなく、日本語 Mac ユーザーにとっての固有価値である。

低い点は、 **「正常時は静かで、異常時はもっと静か」** なこと。接続・切断・モード・強制復帰・自己修復・ウォッチドッグ発動の一切が eprintln の世界にしか存在しない(mac/main.rs:1041, 1269, 1288 参照)。ユーザーが受け取るフィードバックは osascript の一瞬の通知2種(mac/main.rs:312-324, 1025, 1087)のみで、しかも通知許可の状態次第で消える。「まだストレスを覚える」「本質がわからない」というユーザーの反復は、機能不足ではなく **不可視性の症状** と判断する。5回の「行けない/戻れない」履歴に対し、ユーザーが事後参照できるのは /tmp/sd-mac-run.log のみ。これは開発者道具であって製品の観測手段ではない。

さらに減点材料として、ドキュメントが実装と5箇所食違っている(§4-8に一覧)。「動いているから正しい」の対極にあるのが「資料が嘘で、現状把握が開発者自身にもできていない」状態で、GUI化・リファクタの前にこれは致命的な債務である。

---

## 2. 核心体験の定義

> **「MacBook のトラックパッドの手触りと、かな/英数によるIMEの筋肉記憶を、机の隣の Windows まで何の違和感もなく延長すること。そして、境界を押せば必ず向こうへ行け、戻ろうと思えば必ず戻れる、という双方向の信頼。」**

対抗馬との差別化は「延長」にある。Deskflow/Synergy の切替モデル(境界即時、[既定では double tap も delay も無効](https://github.com/deskflow/deskflow/issues/9840)、[Barrier issue #672](https://github.com/deskflow/deskflow/issues/672) 参照)はマウス前提で、トラックパッド加速の再現を本体機能としては持たない。[Logitech Flow](https://support.logi.com) は専用ハード(MXシリーズ等)が必須で、ハードを持たない既存 Mac ユーザーには届かない。seamless-desk が唯一持てるのは「Mac 純正の入力体感をそのまま Windows へ運ぶ」ことである。よって、 **絶対位置モードの速度感と IME が核心であり、切替所作(ダブルタップ等)は核心を守るための入退出管理に過ぎない**。

これを損なう要素: Alt 押しっぱなし疑い(§4-2)、rel モードのウォッチドッグ干渉(§4-3)、状態不可視(§4-1)。

---

## 3. ミッション質問への直接回答

**Q1. 画面端+ダブルタップの対話モデルは正しいか / 1回切替を既定にすべきか**

モデル自体は正当。Deskflow にも "Switch on double tap within X" が実在し、[窓が短すぎると逆に切り替わりにくいという不満](https://github.com/deskflow/deskflow/issues/7920)が知られている。700ms(mac/main.rs:650)は同機能の典型値(50〜500ms)より寛容で妥当。ただし:

- 跳ね返し15px(mac/main.rs:655-656)が **完全に無音・無表示**。初見ユーザーには「なんか壁にぶつかった?」以上の意味を持たない。「もう一度押すと通る」ことを体感させるというコメント(mac/main.rs:653-654)は、フィードバックが存在しない以上、設計者の自己満足である。跳ね返し時に音1つ(例: NSBeep 相当)か境界発光があれば別物になる。
- 1回切替を既定に戻すべきか: **戻すべきでない**。改善ループ201〜209(improvement-log.md:180-184)の誤爆・再突入の歴史は、この3画面環境では1回切替が成立しなかった記録であり、ダブルタップは現実解。ただしこれは「レイアウト検出が起動時固定」(§4-6)という根本の棚上げの上の対症療法であることは自覚すべき。
- 15px の跳ね返しは `EDGE_PX=2` 既定でリセット条件が edge-8px(mac/main.rs:627)より深いため EDGE_AT_EDGE が即リセットされ、高速フリックで再到達すれば正しく2回目と数えられる。ここは整合しており問題ない。

**Q2. 「行けない/戻れない」発生時、ユーザーは何を見るべきだったか**

現状の答え: **何も見えないので「F13を押す」以外の学習が成立しない**。6本の復帰経路(インベントリ#15)のうち⑤タップ再有効化と⑥ウォッチドッグは発動しても通知ゼロ(mac/main.rs:1273-1291、DIAG_SELF_HEAL はログカウンタのみ)。ユーザーは「たまたま直った」体験をするだけで、原因も直ったことも知覚できない。あるべき姿: メニューバーの常時状態表示(接続/モード)+復帰強制時の音・通知+「最後の失敗理由」の1クリック参照。これが GUI 化の第一目的になるべき(§5)。

**Q3. カーソル速度の体感一致は本当に解決したか**

構造としては良くできている。Mac 加速済 delta を Mac 側で積算し Win は加速を通さず注入するため、「Mac の加速曲線」は再現される(Windows 側の曲線と合成されない)。ただし2つの留保:

- 方向別スケール sx=ww/mw, sy=wh/mh(mac/main.rs:747)の **mw がメイン画面幅(2056)で union 全幅ではない**。ウルトラワイド(2560)やサブモニター上にカーソルがある状態で切替すると、速度基準は常にメイン画面。モニター毎に速度感が異なる Mac 側実態とは厳密に一致しない(軽微だが「本当に解決したか」への誠実な答えは「メイン画面基準では解決、全画面基準では未解決」)。
- Windows 側がマルチモニタだと破綻する。`GetSystemMetrics(SM_CXSCREEN)` はプライマリのみ(win/main.rs:363-364)、MOUSEEVENTF_ABSOLUTE の 65535 正規化もプライマリ基準。現環境 1920x1080 シングルだから動く。「たまたま動いている」の部類。

**Q4. 接続・切断・遅延・モードの可視性**

§1の通り。遅延に至っては計測値をどこにも出していない(RTT 6ms は設計時の pcap 値。運用中のレイテンシ表示なし)。GUI の必須項目。

**Q5. セットアップの険しさ**

一般ユーザーは通れない。具体的:

- **トークンが実運用で設定されていない可能性が高い(推測)**。`.env` を読むコードはリポジトリに存在しない(dotenv クレートなし、`std::env::var` のみ: mac/main.rs:834, win/main.rs:340-341)。restart-mac.sh も run_sd.vbs もトークンを設定しない。よって両側ともフォールバックの `seamless-desk-dev`(ソース公開で誰でも知れる値)で動いているはず。Tailnet 内とはいえ同一 Tailnet の他デバイスが接続できる状態。ミッション資料の「環境変数/.env から供給」は **実装と食違う**。
- Mac 常駐化(LaunchAgent)は棚上げ(improvement-log.md:216)。OS 再起動のたびに開発スクリプト `restart-mac.sh` を叩く運用で、これは開発者専用。
- アクセシビリティ権限未付与時の案内が stderr 1行(mac/main.rs:1335-1336)。初回起動が黙って死に、ユーザーに原因は伝わらない。
- Windows 側は install.bat(要管理者)で schtasks 登録まで自動化されており相対的によくできているが、`win-dist/sd-win.exe` が 15:46 时点の古いバイナリ(455KB)としてリポジトリに同居し、install.bat:5 はそれをコピーする。**install.bat 経由だと古い exe が入る**。

**Q6. 核心体験とそれを損なう要素**

§2に定義した通り。最大の敵は「不可視性」と「保険の積み上げで隠された構造的脆弱性」の2つ。

---

## 4. 体験を損なう欠陥ランキング(コード根拠付き)

### 第1位: 異常時の完全な不可視性(状態・原因・復旧の全てがログ専用)

- 根拠： モード/接続/自己修復/ウォッチドッグの全ユーザー可視化が存在しない(mac/main.rs:1041, 1269, 1288)。通知は接続/切断の2種のみ(mac/main.rs:1025, 1087)。
- 影響： 「5回の行けない/戻れない」が再発してもユーザーは学習できない。開発者への報告も「なんか壊れた」止まりになる。

### 第2位: cmd+Tab 変換で Alt が離されず残る疑い(強い推測・要実機検証)

- 根拠(コード追跡)： cmd+Tab down の処理が `mods.apply(false, opt, false, shift)` で self.ctrl(cmd相当)を false に落とす(win/main.rs:553)。cmd 離下の確定ブロックは `prev_cmd = mods.cmd_pressed()` が true を要求するが(win/main.rs:542-543)、上記により cmd up 到達時は常に false。**確定ブロックは到達不能で VK_MENU up は誰も注入しない**。その後の `mods.apply(...)` も self.alt と want が一致しており差分注入が起きない(win/main.rs:306-324)。物理 Alt が押されたまま mods はそれを知覚しない不整合が残る。回復は option を一度押し離す場合の差分注入か、左端復帰時の release_all(win/main.rs:693)のみ。
- 影響(推測): Windows の Alt+Tab パレット確定が Alt up で行われないため、パレット挙動が不安定になり、直後の単キーが Alt+キーとして解釈される恐れ。「まだストレスを覚える」の未説明成分の最有力候補。
- 検証方法： `--debug-keys` 起動で cmd+Tab を実機で1回、直後に A キー単押し。メニューバーが開く(Alt+A 扱い)なら確定。修正は確定条件から prev_cmd を削除し ALT_TAB_ACTIVE && !cmd で Alt up を注入する形への変更が簡潔。

### 第3位: 相対モード(rel)でウォッチドッグが誤発火する構造(推測・高確率)

- 根拠： LAST_ABS_MS の更新は abs ブランチ内のみ(mac/main.rs:764-765)。rel ブランチは更新しない。ウォッチドッグは `マウス操作2秒以内 && abs停滞5秒超` で強制復帰するが MOUSE_ABS_MODE を参照しない(mac/main.rs:1282-1291)。rel モードでマウスを5秒以上動かし続けると反復的に Mac へ強制復帰される。
- 影響： 「絶対/相対両モードが動作すること」という保全天領域に反する潜在バグ。テストが common の b64 のみ(後述)のため未検出のまま。
- 検証方法： `SEAMLESS_MOUSE_MODE=rel` で起動し WIN モードで5秒間連続マウス移動。`[watchdog]` ログが出れば確定。

### 第4位: 切断時の「即Mac復帰」が実装されておらず自己修復スレッドの「たまたま」に依存

- 根拠： 切断処理は `WIN_MODE.store(false)` と通知だけで `leave_win_mode_cursor_unlock` を呼ばない(mac/main.rs:1084-1087)。カーソル復元は150ms後の自己修復(mac/main.rs:1259-1269)が拾う。ただし自己修復は `CGSetLocalEventsSuppressionInterval(0.0)` を戻さず(enter 時の 0.0001 が残留: mac/main.rs:443)、復帰ワープも CUR_POS 更新も無く、カーソルは右端 LOCK_X=UNION_MAX_X-2 付近に放置される。再接続後、CUR_POS が右端のままだと最初の右方向移動で意図しない再突入の余地がある(EDGE_GUARD は leave が設定するため未設定)。
- 影響： 切断→再接続の直後に「勝手に Windows へ飛んだ」体験が起き得る。設計書の「Windows モード中に切断したら即 Mac モードへ復帰」(design.md:81, usage.md:71)は実装と乖離。

### 第5位: 復帰経路6本の存在そのものが示す構造的脆弱性+経路間の後処理不統一

- 根拠： abs-left 復帰は Mac 内完結のため Windows 側の `mods.release_all` が呼ばれない(win/main.rs:680-695 は Return 通知経路のみ)。abs-left で戻ると Windows 側に押しっぱなし修飾キー・ボタンが残留する。また切替時のボタン解放は左ボタンのみで右ドラッグ持ち込みは未対応(mac/main.rs:670-673)。さらに abs-left 復帰後、遅延して届いた Return が WIN_MODE を確認せず再度 leave を呼び、二重ワープする(mac/main.rs:1040-1042)。improvement-log.md:172 に自覚あり。
- 影響： 経路毎に「何が片付けられないか」が異なり、6本の保険が逆に挙動の予測不能性を生んでいる。

### 第6位: ディスプレイ構成が起動時固定

- 根拠： UNION_MAX_X / EDGE_DISP_Y は main 起動時に一度だけ計算(mac/main.rs:844-862)。CGDisplayRegisterReconfigurationCallback 等の追従機構なし。Win 側も SM_CXSCREEN は起動時一度だけ(win/main.rs:363-364)。
- 影響： モニター付け外し・解像度変更・ミラーリング切替で境界判定が静かに破綻する。「3画面レイアウト変更への追従」はインベントリ#1の懸念として正しい。

### 第7位: セットアップとトークン運用の形骸化

- §3-Q5 の通り。トークン供給機構の不存在、Mac 常駐化の不存在、権限エラーの診断性ゼロ、install.bat が古い exe を配る。

### 第8位: ドキュメントが実装と5箇所食違う(信頼性の侵食)

| 資料の記載 | 実コード |
|---|---|
| usage.md:20「復帰直後250msは右端判定無効」 | 400ms(mac/main.rs:481) |
| usage.md:44「32イベントに1回同期」 | 16イベント毎(mac/main.rs:614) |
| usage.md:58「0.25秒間隔ポーリング」 | 200ms(mac/main.rs:1226, win/main.rs:449) |
| usage.md:94 と mac/main.rs:644 コメント「500ms以内」のダブルタップ窓 | 700ms(mac/main.rs:650) |
| design.md:78-79「バックオフmax5s / ping5s / pong15秒」 | max3s(win/main.rs:402)/ 3s(mac/main.rs:940)/ 10s(mac/main.rs:944) |
| usage.md:29-33 IMEはImmGetContext優先+VK_KANJIフォールバック | ImmGetDefaultIMEWnd一本(win/main.rs:75-96)。ImmGetContext/ImmSetOpenStatus はコード上どこにも無い |

さらに **usage.md:89 は「右 Cmd=54」をホットキー例として推奨しているが、これは動かない**。修飾キーは flagsChanged イベントとしてのみ報告されるのに、トグル条件は `event_type == EVT_KEY_DOWN` を要求するため(mac/main.rs:566)。usage.md:90 の F6=97 の例は動くが、54 は機能しない推奨をしている。なお F13 の keyUp を握るという mac/main.rs:562-565 は、外側の if が `KEY_DOWN || FLAGS_CHANGED` で絞られているため **EVT_KEY_UP では到達不能な dead code**。

### 第9位: ログローテーションが実運用で無効

- 根拠： run_sd.bat:3-6 に1世代ローテーションがあるが、**実際の起動経路(schtasks→run_sd.vbs→`cmd /c sd-win.exe >> sd-win.log`)は run_sd.bat を経由しない**(run_sd.vbs:2, install.bat:14, deploy-win.sh:18)。improvement-log のループ59-63「run_sd.bat ログローテーション」は現運用では一度も効いていない。sd-win.log は無限増殖する。
- 影響： 長期運用で C ドライブを侵食し、しかも「ローテ対応済み」という虚の安心を documentation に与えている。

### 第10位: テスト・回帰防護の不存在(GUI化・モジュール分割の足場が無い)

- 根拠： テストは common/tests/b64_roundtrip.rs のみ。mac/win の tests ディレクトリなし。verify.sh は7項目を標榜するが切替・カーソル・キー注入の E2E は含まない(verify.sh:18-107)。--test/--test2 の E2E モード(mac/main.rs:1091-1188)はあるが検証サイクルに組み込まれていない。
- 影響： 今回の目標「品質の再構築」において最も危険。第2位・第3位のバグが潜伏できたのはこのため。

### その他の指摘(軽微・参考)

- 切替時の ny 計算(mac/main.rs:666-669)は直後 L676 で LAST_WIN_POS により上書きされる dead store。しかも `1.0 -` で反転しており、leave 側の「反転なし」設計(mac/main.rs:675 コメント)と混乱を招く。
- LAST_ABS_SENT は書き込みのみで読み出しがない残骸(mac/main.rs:373, 764)。
- 切断の pong timeout から Win 側再接続まで最長約22秒(Mac の read_timeout 12s + pong timeout 10s の直列、win/main.rs:439-440 で Win 側死活監視スレッドを削除済み)。ウォッチドッグが入力閉じ込めは防ぐが「20秒操作不能」の体感期間が残る。
- CLIP_MAX_CHARS の名前に対し Win 側の実判定はバイト数(win/main.rs:466)。動作は Mac と対称だが命名が誤解を招く。

---

## 5. 改善提案 — GUI化で「解決すべきもの」と「しないもの」

### GUI化で解決すべき(優先順)

1. **メニューバー常駐+状態の常時可視**(第1位への対処): 接続状態(緑/赤/再接続中)、現在モード(MAC/WIN)、RTT 表示、稼働 BUILD_ID。失敗時に「何が起きて何が復旧したか」を通知+音。自己修復・ウォッチドッグ発動時も必ずユーザー可視化。これは実装が既に CONNECTED/WIN_MODE を AtomicBool で保持している(mac/main.rs:327-328)ため、表示層を足すだけで成立する。
2. **セットアップウィザード**(第7位): アクセシビリティ権限の状態検出と案内(CGEventTapCreate 失敗時にシステム設定の該当ペーンを開く)、トークン生成・配布(現状の dev 既定運用を終わらせる)、Tailscale 到達性確認、LaunchAgent 登録(Mac 常駐化は GUI の一部として初めて意味を持つ)。
3. **設定UI**(環境変数のフロント): SEAMLESS_* 系(usage.md:73-96)を GUI 化し環境変数はフォールバック(互換要件どおり)。特に EDGE_TAPS=1/2、SCROLL_DIV、HOTKEY_KC は体感調整項目であり、対話的に変えられる価値が高い。なおホットキー候補は flagsChanged 型のキー(右Cmd等)を除外するか、FLAGS_CHANGED でもトグルできるよう先に直すべき(第8位の usage.md:89 問題)。
4. **簡易ログ/失敗履歴ビューア**: 過去の [edge]/[return]/[watchdog]/self-heal イベントの時系列を1クリックで表示。ユーザーの「原因を知るすべが無い」への直接回答。
5. **画面レイアウトの可視化・再取得**(第6位): GUI のレイアウト図表示と、CGDisplayRegisterReconfigurationCallback による UNION_MAX_X/EDGE_DISP_Y の動的再計算をセットで。

### GUI化では解決しない(先にコードで直すべき)

1. **Alt stuck 疑いの修正と実機検証**(第2位): GUI の有無と無関係。確定条件の prev_cmd 依存を削る修正は10行未満。
2. **rel モードのウォッチドッグ誤発火**(第3位): `MOUSE_ABS_MODE` をウォッチドッグ条件へ組み込むのみ。
3. **切断経路で leave_win_mode_cursor_unlock を呼ぶ**(第4位): 自己修復への依存をやめ、復帰処理を一本化。
4. **Return 二重復帰の抑止**(第5位): `WIN_MODE` が true のときだけ leave する1行ガード。
5. **ログローテーションの実経路への接続**(第9位): run_sd.vbs を run_sd.bat 経由に変更するだけ。
6. **ドキュメントの数値全面照合**(第8位): モジュール分割着手前に必須。資料が嘘のままリファクタすると「守るべき動作」の定義自体が信頼できない。
7. **mac/win の単体テスト足場**(第10位): モジュール分割と GUI 化の前に、モード遷移・エッジ判定・ウォッチドッグ条件・Alt+Tab 状態機械を純ロジックとして切り出し、状態遷移テストを書く。1347行の main.rs 分割(改善の主要テーマ)は、このテスト足場を作ってからでないと「実績のあるバグの再発」を検知できない。

### まとめ

正常系の設計(絶対位置モード、Deskflow 準拠のカーソル管理、IME 方向指定)は第三者目線でも筋が良い。問題は、異常系を「6本の保険」と「ログ」で受け止める設計思想にあり、それがユーザーの「本質がわからない」という言葉の正体である。GUI はこの不可視性を払拭する手段としては正しいが、第2位〜第4位の「たまたま動いている」層を先に潰してから足を踏み出さないと、GUI は不安定な土台の上に化粧を塗るだけになる。

検索ソース: [Deskflow issue #9840](https://github.com/deskflow/deskflow/issues/9840), [Deskflow issue #7920](https://github.com/deskflow/deskflow/issues/7920), [Barrier issue #672](https://github.com/deskflow/deskflow/issues/672), [Logitech Flow サポート](https://support.logi.com)

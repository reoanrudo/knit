# Wave1-A アーキテクチャレビュー(第三者・読み取り専用・辛口)

- 実施日: 2026-09-25 / エージェント型: architecture-reviewer
- 対象: ~/ZCodeProject/seamless-desk
- 読了ファイル: crates/mac/src/main.rs(1347行)、crates/win/src/main.rs(725行)、crates/common/src/lib.rs(262行)、scripts/{verify.sh, restart-mac.sh, deploy-win.sh}、win-dist/{run_sd.vbs, run_sd.bat, install.bat}、docs/{design.md, usage.md, improvement-log.md, agent-rebuild-prompt.md}、Cargo.toml 群

## 0. 総評

「動いている」のは主因が絶対位置モード(既定ON)であるためで、**fallback 相対モード・資料記載の設定例・一部の「対策済み」主張は、コード上では壊れているか実装されていない**。構造面では、切替状態機械が 6 箇所に分散した WIN_MODE への直接 store で成り立っており、後処理(カーソル復帰)は呼び出し側の規律と 6 本の保険スレッドに依存している。これが「戻れない」系バグを 5 回出した根本原因であり、GUI 化より先に状態遷移の一元化が必要。なお static の実数は 41 個(後述)で、共通資料の「20個超」は過小申告。

---

## 1. 現状評価: 問題一覧(深刻度順)

### 高(実バグ・入力喪失・データ破損リスク)

**H1. 相対モード(SEAMLESS_MOUSE_MODE=rel)でウォッチドッグが誤発火し、WIN モードが 5 秒で強制解除される**
- ウォッチドッグは「タップ受信 2 秒以内 なのに abs 送信 5 秒停止」を検知する(~/ZCodeProject/seamless-desk/crates/mac/src/main.rs:1282-1292)。
- LAST_EVENT_MS は移動イベントで無条件更新(main.rs:590-593)する一方、LAST_ABS_MS は絶対モードのブロック内でしか更新されない(main.rs:765)。相対モードの送信パス(main.rs:778)は LAST_ABS_MS を更新しない。
- したがって rel モードで WIN に入り、5 秒以上経過してからマウスを動かすと「操作中なのに転送停止」と誤判定され、強制的に Mac へ復帰させられる。**「守るべき動作」と明記された相対モードが、切替後 5 秒しか維持できない「たまたま動いているだけ」の状態**。improvement-log のループ 232-233(docs/improvement-log.md:195)に rel モード考慮の記述は無く、開発者は未認識と推測。
- 修正案： watchdog 判定に「abs モードか」を含める(例: `MOUSE_ABS_MODE.load() && ...`)、または rel 送信時にも LAST_ABS_MS(名称は LAST_TX_MS に変更)を更新する。後者がモード非依存で安全。
- 検証方法： `SEAMLESS_MOUSE_MODE=rel ./scripts/restart-mac.sh` → 境界切替 → 5 秒待機 → マウス移動。修正前は /tmp/sd-mac-run.log に `[watchdog]` が出て強制復帰することをまず確認してから、修正後に出ないことを確認。

**H2. Win 側で 2 スレッドが同一 TcpStream に同期 write しており、行インターリーブで JSON が壊れ得る**
- クリップ監視スレッドが cb_writer へ writeln(~/ZCodeProject/seamless-desk/crates/win/src/main.rs:456, 477)、メイン受信ループが pong と Return で writer へ writeln(win/src/main.rs:508, 690)。
- `writeln!` は write_fmt 経由で複数ピースに分けて write を呼び得るため、大きなテキスト(1MB の Clip)送信中に pong/Return が割り込むと同一ソケットに交互に書き込まれ、行が分断される。分断行は Mac 側 decode で None になり**黙って捨てられる**(main.rs:1074 の `_ => {}`、win/src/main.rs:500 の `None => continue`)。「たまにクリップが届かない」の温床で、ログに何も出ない。
- Mac 側は mpsc チャネル+単一送信スレッド(main.rs:918-958)で正しく直列化している。**Win 側に同じパターンを適用するだけで解決する**(共通化の具体論は第 4 節)。
- 壊すリスク： 低い。送信を 1 スレッドに集約するだけで受信ロジックは不変。検証方法： Mac→Win に 1MB テキストを連続コピーしつつ pong を乱発させ、両側ログで欠落がないことを確認。

**H3. バルク転送(クリップ画像、最大 5MB)と入力ホットパス(MouseAbs 毎イベント)が同一 TCP ストリーム**
- 画像は base64 で最大約 5MB を単一行で送る(win/src/main.rs:454-457)。送信中は同一ストリームの MouseAbs が遅延する(head-of-line blocking)。RTT 6ms の Tailscale でも 5MB の転送中はカーソルが固まる。「画像コピーの頻度が低いため露呈していないだけ」。
- 最小対処： ClipData を 64KB 種別チャンク+シーケンス番号で分割(受信側で再組み立て)。本質対処： 制御/入力とバルクの 2 コネクション分離(hello に capability 追加)。ただし後者はファイアウォール 24901 追加とデプロイ手順が増えるため、段階計画では分割方式を推奨(トレードオフは第 6 節)。
- 検証方法： Win 側でスクリーンショットをコピーした直後にマウスを激しく動かし、diag の abs カウンタ間隔(main.rs:1191-1220)で停滞を測る。

**H4. 接続切断時に leave_win_mode_cursor_unlock を呼んでいない。カーソル復帰を自己修復スレッドに丸投げ**
- 切断処理は `CONNECTED=false; WIN_MODE=false` のみ(~/ZCodeProject/seamless-desk/crates/mac/src/main.rs:1080-1088)。カーソル hide・マウス関連切断・LOCK_POS・CUR_POS 再同期・EDGE_GUARD 設定のいずれも実行されない。
- 実際は自己修復(main.rs:1257-1270)が 150ms 以内に show+associate を復元するが、これは show と associate のみを行い、LOCK_POS クリア・CUR_POS 同期・EDGE_GUARD 設定を行わない。つまり**「復帰経路 6 本」の 4 番(自己修復)が、切断という正規経路の後処理として機能している**。6 本もの保険が必要な構造的脆弱性の正体はここ: WIN_MODE の各 store箇所が、後処理を呼ぶかどうか独自に判断している。
- 修正案： 切断処理で `WIN_MODE.load() なら leave_win_mode_cursor_unlock(None)` を呼ぶ(自己修復は異常系専用に戻す)。壊すリスク： leave 内の Warp が切断直後の不正な位置で走る可能性は低い(ny=None は画面中央固定、main.rs:517-523)。検証方法： WIN モード中に Windows 側 sd-win を taskkill → 自己修復ログ `[cursor] self-heal` が出る代わりに `[return]` が出ることを確認。

**H5. WIN_MODE への store が 6 箇所(テスト除く)に分散し、チェック-アンド-アクトの隙間がある**
- store 箇所： F13(main.rs:569)、edge(main.rs:663)、abs-left(main.rs:771)、Return 受信(main.rs:1040)、切断(main.rs:1085)、ウォッチドッグ(main.rs:1287)。加えて --test/--test2 で 4 箇所(main.rs:1105, 1156, 1174, 1186)。
- 例： tap_callback は先頭で connected を読み(main.rs:556)、edge 判定後に WIN_MODE.store(true) する(main.rs:663)。この隙間に切断が入ると WIN_MODE=true & CONNECTED=false で入力を握り続け、送信先が無い。ウォッチドッグで最大 5 秒後に復帰するが、その間入力喪失。
- 対処： 第 2 節の switching モジュールで遷移関数(try_enter/leave)に一本化し、`CONNECTED && !WIN_MODE` を関数内で原子的に判定する(比較交換か、単一 Mutex 内で判定)。壊すリスク： 遷移経路を集約すると既存の呼び出し時序(実績のあるパターン 4/5/6、資料 67-79 行)が動くことを全経路で再検証する必要がある。検証方法： 実機で F13/edge/abs-left/Return/切断/ウォッチドッグの 6 経路それぞれ復帰できることをログで確認。

### 中(条件付き機能不良・保守リスク)

**M1. F13 keyUp を握るコードはデッドコード。「up も握る」改善ループ 24-33 の主張は未実装**
- ~/ZCodeProject/seamless-desk/crates/mac/src/main.rs:559-565。外側条件が `EVT_KEY_DOWN || EVT_FLAGS_CHANGED` のため、内側の `if event_type == EVT_KEY_UP` は永不達。実際の F13 up は素通りする(Mac モード時)か Key メッセージとして Win へ転送される(WIN モード時。kc=105 は keymap に無いので Win 側で無視)。実害は無い=「たまたま動いている」。docs/improvement-log.md:128 の記述と実装が不一致。

**M2. usage.md 推奨の「右Cmd=54」ホットキーは動作しない**
- 修飾キーの押下は EVT_FLAGS_CHANGED として届くが、トグル条件は `event_type == EVT_KEY_DOWN` のみ(main.rs:566)。SEAMLESS_HOTKEY_KC=54(docs/usage.md:89 の推奨例)では発火しない。F6=97 等の通常キーは動く。修正は FLAGS_CHANGED でも hotkey_kc をトグル対象にするか、usage.md の例を削る。検証方法： `SEAMLESS_HOTKEY_KC=54` で起動し右 Cmd を押してトグルしないことを確認してから修正。

**M3. ディスプレイ構成(UNION_MAX_X/EDGE_DISP_Y)が起動時に 1 回しか計算されない**
- main.rs:838-862。モニター増減・解像度変更・ミラーリングで境界判定が陳腐化し、右端切替が誤爆/不能になる(3 画面環境では現実的頻度)。修正案： CGDisplayReconfigurationCallback 登録か、監視スレッドでの周期再計算(display モジュール化の動機)。検証方法： WIN 中にモニター接続を変えて切替挙動を観察(現状壊れることをまず記録)。

**M4. 切断検知に最大 12 秒かかり得る**
- read_timeout 12 秒(main.rs:983)。pong timeout で STREAM_SLOT を外しても(main.rs:944-949)、受信スレッドの read_line は TCP が生きていれば待ち続ける。実質はウォッチドッグが 5 秒で先に復帰させるが、CONNECTED=false 化は受信ループの終了待ち。許容するなら設計文書に明記すべき。

**M5. decode が未知タグ/壊行を黙捨てし、バージョン不一致・行破損がログに出ない**
- ~/ZCodeProject/seamless-desk/crates/common/src/lib.rs:76-78 と両受信側(mac main.rs:1032, win main.rs:498-501)。H2 と複合して「届かない」問題の切り分けが不能。decode を `Result<Msg, DecodeError>` 化し Unknown(tag)/構文エラーをログ化するのがプロトコル拡張の前提(第 3 節)。

**M6. ClipData 受信側にサイズ上限が無い**
- Win 側送信時の 5MB 制限(win/src/main.rs:454)のみで、Mac 側の read_line は行長無制限、受信後のサイズ検証も無い(main.rs:1044-1056)。テキストの Clip は CLIP_MAX_BYTES 検査がある(main.rs:1058)のに画像には無い。受信側上限を課すべき(防御的設計)。

**M7. トークンの既定フォールバック "seamless-desk-dev" が fail-open**
- main.rs:834、win/src/main.rs:340-341。未設定で起動しても弱い認証で黙って稼働する。config 化の際に「未設定なら起動拒否+通知」へ変更を推奨(ただし既存運用の手順書更新が必要)。

**M8. 本番パス(run_sd.vbs)がログローテーションをバイパスし、ログ無限増殖が復活している**
- run_sd.bat にローテーションがあるが(~/ZCodeProject/seamless-desk/win-dist/run_sd.bat:3-6)、install.bat:14,17 と deploy-win.sh:18 の両方が直接 vbs を起動し、vbs は sd-win.exe を直接リダイレクトする(win-dist/run_sd.vbs:2)。改善ループ 59-63「run_sd.bat ログローテーション」は実経路では死んでいる。修正は vbs から run_sd.bat を起動するよう 1 行変更。検証方法： デプロイ後 sd-win.log.old が生成されることを確認。

**M9. notify() の osascript が受信スレッドを同期ブロックする**
- main.rs:312-324 を受信ループから直接呼ぶ(main.rs:1025, 1087)。osascript 起動は数百 ms オーダーで、その間 Return/Pong 受理が遅延。`std::thread::spawn` で包むだけの修正。実害は pong タイムアウト 10 秒に比べ小さいが、受信スレッドでの外部プロセス実行は設計汚点。

**M10. abs-left 復帰後に Win 側 Return が届くと二重 leave が走る**
- main.rs:1034-1042 は WIN_MODE が既に false でも無条件に leave を実行する。CURSOR_HIDDEN フラグで show は抑止されるが、ワープと EDGE_GUード再設定が 2 回走る。`if !WIN_MODE { continue; }` ガード追加で解消。improvement-log の次候補にも挙がっている既知事実(docs/improvement-log.md:172)。

**M11. Win 側クリップ監視が全文読み+文字列比較**
- win/src/main.rs:446-465。200ms 毎に OpenClipboard→全文コピー→String 比較。GetClipboardSequenceNumber() に置き換えれば O(1) かつ他プロセスとの OpenClipboard 競合も減る。Mac 側は changeCount(main.rs:306-308)を使っており、Win 側だけ非対称。

### 低(品質・整理)

- **L1. 資料と実コードの数値食違い**(後述の表を参照)。
- **L2. 死んだコード**: main.rs:666-669 の ny 計算は 676 で上書きされるデッドストア。main.rs:677 の `if nx < 0.0` は永不達(LAST_WIN_POS は clamp 0.05..0.95 で格納、main.rs:477)。LAST_ABS_SENT は書き込みのみ(main.rs:764, 695)。main.rs:822-827 の `_host` は完全に未使用(Mac はサーバなので --host は不要)。common/src/lib.rs:126 の `let _ = i;`。
- **L3. EDGE_TAPS=3 が 2 と同じ挙動**: 判定は `taps <= 1 || (700ms 以内の 2回目)` のみ(main.rs:647-650)。バリデーションは 1..=3 を許可(main.rs:896)。3 を実装するか 1..=2 に縮めるか決める。
- **L4. --test/--test2 の E2E シーケンスが製品バイナリに常駐**(main.rs:1091-1188)。notepad 保存まで含むコードが常時コンパイルされる。feature フラグ化/e2e モジュール分離を推奨。
- **L5. usage.md:29-31 の IME 記述が実装と不一致**： 実装は ImmGetDefaultIMEWnd+WM_IME_CONTROL のみで ImmGetContext/ImmSetOpenStatus は存在しない(win/src/main.rs:75-96)。
- **L6. ハードコード IP の混乱**: Win の接続先既定は 100.100.10.9(win/src/main.rs:375)だが、資料・design.md は Mac を 100.84.0.2 と主張。Mac 側 main.rs:827 の既定 IP は死んだ変数(L2)。実際に稼働しているなら 100.100.10.9 が現行 Mac の IP と推測(推測。Tailscale IP の更新履歴は未確認)。config 化すべき根拠。
- **L7. static の実数は 41 個**(main.rs の static 宣言を数えた実測。135, 327-412 他)。共通資料の「20個超」は過小。
- **L8. PORT 24900 は Synergy/Deskflow の既定ポートと同一**。併用時に衝突する。config の port 設定化で自然に解決。

### 資料・ドキュメントとコードの食違い一覧(検証済み)

| 項目 | 資料の記述 | 実コード | 根拠 |
|---|---|---|---|
| 復帰ガード | 250ms(usage.md:20) | 400ms | mac main.rs:481 |
| ライブ同期間隔 | 32 イベント毎(usage.md:44) | 16 イベント毎 | mac main.rs:616 |
| ダブルタップ窓 | 500ms(usage.md:94) | 700ms | mac main.rs:650 |
| バックオフ上限 | max5s(資料・design.md:78) | max3s | win main.rs:402 |
| ping/pong | 5秒毎/15秒(資料・design.md:79) | 3秒毎/10秒 | mac main.rs:940, 944 |
| IME 方式 | ImmGetContext(usage.md:29-31) | ImmGetDefaultIMEWnd のみ | win main.rs:75-96 |
| F13 up 握り | 実装済み(improvement-log:128) | デッドコード | mac main.rs:559-565 |
| ログローテーション | run_sd.bat で対応(improvement-log:146) | 本番経路(vbs)はバイパス | run_sd.vbs:2, deploy-win.sh:18 |
| 接続先 IP | 100.84.0.2(資料) | 100.100.10.9 | win main.rs:375 |

docs/improvement-log.md は「git log HEAD=48f261e 基準」と明記されている一方、環境情報は「Is directory a git repo: No」と表示されています。ワークスペース全体が git 管理外で seamless-desk 配下のみ git 管理という可能性はありますが、**移行計画はコミット単位の差分管理を前提とするため、まず git 状態を確認してください**(推測)。

---

## 2. 目標アーキテクチャ

### 2.1 モジュール分割案(crates/mac/src/ 以下、移動対象に行番号付き)

```
crates/mac/src/
  main.rs        … main() のみ:Context 構築→スレッド起動→RunLoop(GUI 後は NSApp.run へ)
  config.rs      … Config 構造体 + SEAMLESS_* env パース(main.rs:870-904 を集約)+起動ログ
  ffi.rs         … CG/CF/objc 宣言と msg0〜msgN ヘルパ(main.rs:79-179)+live_cursor(384-392)
  display.rs     … DisplayLayout{screen_w/h, union_max_x, edge_disp_y} 検出(main.rs:838-862)+将来の再計算 API
  switching.rs   … 切替状態機械:WIN_MODE/EDGE_AT_EDGE/EDGE_LAST_HIT_MS/EDGE_GUARD_UNTIL_MS/
                   EDGE_TAPS/HOTKEY_ONLY + try_enter()/leave() の一元化(全遷移はここを通す)
  cursor.rs      … enter/leave_win_mode_cursor_lock(431-531)、set_cursor_in_background(418-429)、
                   CURSOR_HIDDEN/LOCK_POS/SCROLL_ACC ※switching からのみ呼ばれる
  input.rs       … tap_callback の転送部(705-815):abs 計算/スクロール量子化/キー転送。
                   純粋変換(scroll_quantize, edge_hit 判定)は fn として分離しテスト可能に
  net.rs         … listener/accept/hello/受信ループ(962-1089)+送信スレッド(924-958)+TX/STREAM_SLOT/
                   CONNECTED/LAST_PONG_MS/WIN_SCREEN
  clipboard.rs   … mac_get/set_clipboard(289-304)、dib_to_bmp(207-230)、画像書き込み(233-286)、
                   LAST_RECV_CLIP、監視スレッド(1223-1247)
  observe.rs     … diag(1191-1220)/自己修復/タップ再有効化/ウォッチドッグ(1253-1306)の集約
  e2e.rs         … --test/--test2 シーケンス(1091-1188)。feature = "e2e" を推奨
```

分離の原則： **switching が唯一の状態遷移口、cursor が唯一のカーソル操作口、net が唯一の送受信口**。input と observe は switching/cursor/net のクライアントにする。これで GUI が観測・操作すべき境界は switching::snapshot()(モード、ガード状態)と net::status()(接続、pong RTT)と diag の 3 点に限定される。

### 2.2 グローバル static の構造化(段階 3 ステップ)

「一気に単一 struct へ」は勧めない。tap_callback は extern "C" 境界で、Run Loop スレッド・受信スレッド・監視スレッドの 3 者が非同期に来るため、actor 化(単一 ownership + メッセージキュー)はホットパスにレイテンシを足すリスクがある。Atomics+小 Mutex を維持したまま「書き込み口の一元化」だけで H2/H5 の競合クラスは消える。

- **ステップ 1(機械的移動)**： static を各モジュールへ pub(crate) で移動。コード変更なし。diff は移動のみ。
- **ステップ 2(グループ化)**： 関心ごとに束ねて `OnceLock<Arc<Context>>` を 1 本持たせる。
  - `SwitchState { win_mode, hotkey_only, edge_at_edge, edge_last_hit_ms, edge_guard_until_ms, edge_taps }`
  - `PeerState { connected, last_pong_ms, win_screen }`
  - `VirtualCursor { win_cur, last_win_pos }`
  - `Diag { 9 個の AtomicU64 }`(観測専用で GUI のデータソース)
  - `Config`(読み取り専用)
- **ステップ 3(本来形)**： CGEventTapCreate の第 6 引数 user_info(現状 null_mut、main.rs:1331)へ `Arc::into_raw(Context)` を渡し、tap_callback は static 参照をやめる。自己修復系スレッドにも Arc を clone して渡す。アプリケーション全体で static が消える。
- WIN_CUR の AtomicF64 化(棚上げ項目)はステップ 2 と独立に可能だが、競合実害が現状無く(毎イベントの単一ロック)、H1〜H5 修正の後に着手を推奨。

### 2.3 GUI 接続境界

- 現状 tap はメイン RunLoop の kCFRunLoopCommonModes に載っている(main.rs:1340-1343)。NSApplication の run も同じメイン RunLoop を回すため、**tap と GUI(NSStatusItem)は同一プロセス・同一メイン RunLoop で共存可能**。CFRunLoopRun を NSApp.run に置き換えるだけでよい。
- 推奨構成： 中核+GUI を同一バイナリ(NSApplication は堅牢で、restart-mac.sh の起動異常検知がそのまま使える)。GUI クラッシュ懸念より、別プロセス IPC 二重管理のコストを優先して排除。トレードオフは第 6 節。
- 実装方式は objc2 + objc2-app-kit を推奨。理由： メニューバー UI は target/action と delegate のイベント配信が必須で、手書き objc_msgSend 直叩き(実績はあるが単発呼び出し向け)で組む労力と保守性が見合わない。NSPasteboard 系の既存 transmute 方式(main.rs:143-169)はそのまま残して併用できる。

---

## 3. プロトコル(common::Msg)の拡張性

現状の評価(~/ZCodeProject/seamless-desk/crates/common/src/lib.rs:8-68):

- serde tag="t" の internally tagged enum は**タグ追加に対して閉じた拡張**は可能だが、未知タグは decode で None→黙捨て(lib.rs:76-78)。前方互換の握り潰しがログに出ないため、新旧バイナリ混在時のデバッグが不能(M5)。
- VERSION は hello のみ(lib.rs:6)。HelloOk 側にバージョンが無く、Mac→Win 方向の能力通知ができない。

拡張方針の提言:

1. **GUI・統計は Msg に足さない**(明確な推奨)。GUI はプロセス内の Context snapshot で観測し、設定は config.rs が単一の真実源になる。プロトコルは入力共有の用途を維持する。プロトコルに統計を足すと、ポーリングが入力ストリームを圧迫する(自身が H3 を悪化させる)。
2. 足すなら、まず decode を `Result<Msg, DecodeError>`(Unknown(String) を含む)に変更し、両受信側で unknown をログ化。これをプロトコル拡張の前提とする。
3. バルクの分離(H3)をする場合、hello に capability ビットを足し、HelloOk で応答確認してから第 2 コネクション(例： 24901)を張る。旧バイナリは capability 無しと解釈して現状動作にフォールバック。
4. VERSION のインクリメント規則(docs/design.md に明文化)を定める: 「フィールド追加で serde default 付きなら VERSION 不変、セマンティクス変更なら +1」等。

---

## 4. Mac/Win 重複概念の共通化可否

**共通化する(利益が具体的)**:
- **送信の直列化パターン**: Mac の mpsc+単一送信スレッド(main.rs:918-958)を sd-common にジェネリックな `LineSender` として切り出し、Win に適用(H2 の解消そのもの)。これが共通化の最優先。
- **バックオフ計算**(win main.rs:389-403 の純粋部): 数十行だがテスト可能な純関数として common へ。
- **行デコーダ**: read_line の行長上限付きラッパ(M6 の解消)。両側で同じ穴があるため共通化価値がある。
- 将来的に設定構造(Config): 両側で SEAMLESS_DESK_TOKEN/ポート解析が重複(win main.rs:340-347 と mac main.rs:828-833)。

**共通化しない(過剰抽象化)**:
- **クリップ監視の trait 抽象**: プラットフォーム API(NSPasteboard changeCount vs Win32 Clipboard)が違いすぎ、インターフェースだけ共有しても実装は毎回書き直し。むしろ Win 側に GetClipboardSequenceNumber を導入して Mac 側と「シーケンス番号ポーリング」というパターンを揃える(M11)。
- **再接続ループ**: Mac(accept ループ)と Win(connect+バックオフ)は対称ではない。共通化すると不自然な抽象になる。

---

## 5. 段階的移行計画(各ステップの検証方法付き)

前提： 各 Phase の終わりに必ず `cargo build --release`(両ターゲット)→ `cargo test --workspace` → `scripts/restart-mac.sh` → `scripts/deploy-win.sh`(該当側のみで可) → `scripts/verify.sh` 合格、を実施。加えて「移動のみのコミット」と「ロジック変更のコミット」を厳密に分離し、移動コミットは diff の移動行一致を確認する。

**Phase 0: 安全網づくり(先にやる。これなしに分割は進めない)**
- 0a: 純粋ロジックの関数化と単体テスト: dib_to_bmp(main.rs:207-230)、スクロール量子化(main.rs:794-810 を `fn scroll_quantize(acc,(dx,dy),div)->((f64,f64),(f64,f64))` へ)、エッジ時間窓判定(main.rs:646-662 の純粋部)、ModState::apply(win main.rs:296-333)、b64 の追加ケース(空入力、パディング 1/2)。
- 0b: verify.sh への入力パス E2E 追加。現状 verify.sh(~/ZCodeProject/seamless-desk/scripts/verify.sh)はプロセス・接続・クリップ・IME ログしか見ず、**切替・abs/rel・cmd+Tab を一切検証しない**(diag は 97-107 行目で表示のみ)。sd-mac --test(接続後にキー列送信、main.rs:1161-1188)を verify.sh から起動し、Win 側に受信ログモード(--echo: 受信 Msg の行ダンプ)を足して ssh で突き合わせる項目を追加する。これで tap を除く送信経路の E2E が自動化される。
- 0c: H1 の修正(watchdog の rel モード誤発火)。Phase 0 中のバグ修正は単独コミットにする。検証は H1 に記載の手順。

**Phase 1: config.rs**
- main.rs:870-904 の環境変数パースを Config 構造体へ。SEAMLESS_* の名前・意味・既定値は不変(互換要件)。
- 検証： 起動ログの `[info] screen ... scroll_div=...`(main.rs:905-915)が既存と完全一致することを diff で確認+verify.sh。

**Phase 2: ffi.rs + display.rs**
- FFI 宣言(79-179)と DisplayLayout 計算(838-862)を移動。ロジック変更なし。
- 検証： ビルド+verify.sh+実機切替 10 往復で `[edge]`/`[mode]` ログが従来どおり出ること。

**Phase 3: clipboard.rs**
- 移動対象は 135-308 と監視スレッド 1223-1247。このタイミングで M6(ClipData 受信上限)のみ追加可(1 行)。
- 検証： verify.sh のクリップ双方向+画像(Win でスクリーンショット→Mac でペースト)手動 1 回。

**Phase 4: net.rs**
- 924-1089 を移動。M5(decode の unknown ログ化)と M9(notify の spawn 化)をこの Phase で追加。
- 検証： established/再接続(Win 側 sd-win 再起動)/切断通知。WIN 中の taskkill で H4 修正済みなら `[return]` 経由の復帰を確認。

**Phase 5: switching.rs + cursor.rs(最大リスク区間)**
- 先に H4(切断時 leave)と H5(遷移一元化)を「バグ修正コミット」と「構造化コミット」に分けて実施。enter/leave の呼び出し時序(資料の実績パターン 4/5/6: 二段階判定、ガード解放後 leave、ワープ後 CUR_POS 上書き)は switching/cursor 内に閉じ込め、呼び出し側が時序を知らない設計にする。
- 検証： 6 復帰経路すべて実機確認(F13/edge/abs-left/Return/自己修復は kill にて/ウォッチドッグは diag にて)。rel モードと abs モード両方で。

**Phase 6: input.rs + e2e.rs**
- tap_callback をスリム化。--test/--test2 を feature "e2e" 配下へ(L4)。
- 検証： 実機で切替・abs/rel・スクロール・cmd+Tab・IME(かな/英数)一式。verify.sh の 0b 項目。

**Phase 7: observe.rs**
- 監視系 3 スレッド(1253-1306)と diag(1191-1220)を集約。
- 検証： `--diag` 出力形式の完全一致(usage.md:117-121 の形式を壊さない)+self_heal/warp_fixed カウンタ挙動。

**Phase 8: GUI(objc2-app-kit)**
- CFRunLoopRun→NSApp.run 置換、NSStatusItem、設定画面(config.rs の読み書き)。GUI は switching/net/diag の snapshot のみ参照し、static や Mutex を直接触らない。
- 検証： GUI 起動時・CLI 起動時(restart-mac.sh)の両方で verify.sh 合格(要件)。加えて GUI からの切替トグル・設定変更が反映されることを実機確認。

---

## 6. トレードオフ

- **static 一掃を最終目標に据えすぎない**: ステップ 3(user_info 経由)まで行く価値はあるが、Phase 2 〜 5 の安全性を優先する。中途の `OnceLock<Arc<Context>>` 形で長く留まることは許容コストである。
- **Win 側 mpsc 化(H2)**: 送信がキュー経由になりクリップ送信開始がマイクロ秒オーダー遅れる。実害無視可能。逆にやらないと「たまにクリップが消える」が残る。
- **バルク分離(H3)**: 2 コネクション方式はファイアウォールとデプロイ手順(install.bat)の更新が必要。当面は 64KB チャンク分割で様子を見る、という判断も成立する。ただし 5MB 単行送信を放置するのは最悪の選択肢。
- **GUI を同一バイナリにする**: GUI パニックが中核を道連れにする理論リスク vs 別プロセス IPC の運用複雑化。NSApplication の堅牢性と restart-mac.sh の既存検知を重視し同一バイナリを推奨するが、 observable boundary(switching::snapshot)を先に作ることが前提で、逆順で GUI を作ると中核を直接触る UI コードが再生産される。
- **objc2-app-kit 導入**: 依存追加と静的リンク確認が必要。既存 transmute 方式と混在可能なので、移行は新規コード(GUI)のみに限定し、既存 NSPasteboard コードの書き換えは不要。

## 7. 未解決の質問(オーケストレータへの質疑)

1. Win 側接続先既定 100.100.10.9(win/src/main.rs:375)と資料の 100.84.0.2 のどちらが現行の Mac か。稼働中の事実確認と、config 化(Phase 1)での扱いを決めてほしい。
2. rel モード(H1)を「守るべき動作」として維持するのか、abs モードに一本化して rel を削除候補にするのか。維持するなら Phase 0c の修正が必須。
3. H3(バルク分離)を Phase 4 に組み込むか、チャンク分割で暫定対処とするか。
4. EDGE_TAPS=3 を仕様として実装するか、バリデーションを 1..=2 に狭めるか(L3)。
5. リポジトリの git 管理状態(第 1 節末尾の推測)。移行計画はコミット単位の差分検証を前提としており、git が無ければ Phase 0 で初期化が必要。
6. Mac→Win 画像クリップボード(TIFF→DIB)を GUI 化スコープに含めるか。含めないなら docs の棚上げ項目として明示されたままになる。

---

補足: 本レビューは読み取り専用で実施し、ファイル編集・ビルド・プロセス実行は行っていません。H1 の誤発火はコード経路の静的追跡に基づく結論です(LAST_ABS_MS の更新箇所は mac main.rs:765 の 1 か所のみ、watchdog 条件は main.rs:1286)。実行確認は上記検証手順で最初に「修正前に `[watchdog]` が再現すること」を確認してから修正に入ることを推奨します。

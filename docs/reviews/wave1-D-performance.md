# Wave1-D パフォーマンスレビュー(第三者・読み取り専用・辛口)

- 実施日: 2026-09-25 / エージェント型: performance-reviewer

## 総括(結論先出し)

まず辛口な結論から。**共有資料が「パフォーマンス改善候補」として挙げている項目(WIN_CUR の Mutex、serde_json の文字列コスト、毎イベント送信の帯域)は、数字上ほぼ無罪です**。非競合 Mutex のロック解放は 1 イベントあたり数十 ns、serde_json の 60B シリアライズは数百 ns、帯域は 125Hz でも 20KB/s 未満であり、いずれも RTT 約 6ms の 0.1% 未満にしか効きません。これらを最適化しても体感は一切変わりません。

一方で、**コード全体の中で最も重い定常的な無駄仕事は Windows 側クリップボード監視スレッド**です。画像がクリップボードに載っている間、200ms 毎に「DIB 全コピー+ base64 全量エンコード」を繰り返し、SendInput 注入と同じプロセス内で推定 100MB/s 超のメモリトラフィックを恒久的に発生させています。これは「動いているから正しい」の最たる例で、機能的には何も壊れていないため誰も気づいていないだけです。ここが最優先です。

次に、**過去の「カクつき」苦情の真因として最も疑わしいのはシリアライズ系ではなく CGWarpMouseCursorPosition の巻き戻し(warp_fixed)**です。diag に計測基整備済みなので、変更前に必ず計測してください。

優先順位(体感インパクト順):

| # | Finding | 分類 | 体感インパクト |
|---|---------|------|--------------|
| F1 | Win クリップポーリングが毎回全量コピー+b64エンコード(画像滞留時は無限再試行) | 実証済みの無駄(遅延寄与は推測) | 大 |
| F2 | warp_fixed(カーソル巻き戻し)がカクつき真因の候補。未計測のままシリアライズ系をいじるのは筋違い | 要計測 | 大(ならば) |
| F3 | osascript 通知が accept/受信スレッドを数百 ms ブロック | 実証済みのブロッキング(コストは推測) | 中 |
| F4 | 全送信が二重改行(\n\n)で受信側にゴミ行を生成+1イベント1 flush の syscall | 実証済み(影響は小) | 小〜中 |
| F5 | MouseAbs バースト時のバッチング(最新のみ送信) | 改善余地 | 状況次第 |
| F6 | LAST_ABS_SENT がデッド状態なのにホットパスで毎イベントロック | 実証済みの無駄 | 極小(削除は無料) |
| F7 | WIN_CUR の AtomicF64 化は性能目的では不要、デッドロック撲滅目的なら正当 | 評価修正 | 極小(性能面) |

---

## F1: Windows クリップボード監視が 200ms 毎に全量コピー+全量 base64 エンコードを行う(最優先)

**Finding**
テキストが無いとき(=画像が載っているとき)、`clipboard_read_dib()` で DIB 全量をコピーし、その後で base64 エンコードし、その後でサイズ上限判定と前回比較を行う。つまり「変化検出を全量処理の後に行う」という順序の逆転が起きており、しかもサイズ超過時は `last_img` が更新されないため**永遠に全量処理を再試行し続ける**。

**Evidence**
- ~/ZCodeProject/seamless-desk/crates/win/src/main.rs:449-465
  - 449行： 200ms sleep
  - 450行： `clipboard_read_text()` がまず全量実行される(OpenClipboard → GlobalLock → 最大 1MB コピー+ UTF16→UTF8 変換)し、**その後** 466行で `last_sent` との比較。変化してなくても毎回全量コピー+変換。
  - 452-454行： テキスト無し時に `clipboard_read_dib()`(全量コピー)→ `b64::encode(&dib)`(全量エンコード)→ **その後で** `b64.len() <= 5 * 1024 * 1024` の判定。フルスクリーン 32bpp の CF_DIB(1920x1080 で約 8.3MB)は b64 後 約 11MB となり上限を超えるため、`last_img` は永久に更新されず、**200ms 毎に 8.3MB コピー+ 11MB エンコードが無限継続**する。
- ~/ZCodeProject/seamless-desk/crates/mac/src/main.rs:306-308, 1231-1234: Mac 側は正しく `changeCount` を先に見てから読んでいる。Windows 側だけがこのパターンを抜けている。

**Expected impact**
- 画像滞留時の定常メモリトラフィック: 1ポーリングあたり DIB コピー(読み8.3MB+書き8.3MB)+ b64 生成(11MB)で約 25-30MB、5回/秒で**約 130-150MB/s**(推定。実測前の計算値)。
- 大ブロックの確保/解放が毎 200ms 発生し、ページフォルトとキャッシュ汚染を恒常的に引起す。このスレッドは SendInput 注入ループと同一プロセスであり、Windows 側の注入ジッタへの寄与が疑われる(寄与の有無自体は未計測のため「推測」と明示)。
- また 200ms 毎の `OpenClipboard` は他アプリのクリップボード占有と衝突し得る(失敗時は握り潰されるため気づかない)。
- テキスト滞留時も毎 200ms の最大 1MB コピー+変換は無駄(こちらは数 MB/s 程度で影響小)。

**Suggested change**
`GetClipboardSequenceNumber()`(user32、OpenClipboard 不要、シーケンス番号だけ返す軽量 API)をポーリング先頭で呼び、変化時のみ全量処理する。Mac 側の changeCount と完全に対になる修正。サイズ上限判定はエンコード前の `dib.len()` で行う(上限 5MB b64 ≒ 3.75MB 生データ)。壊すリスクはほぼゼロ(判定追加のみ)。将来的な完全イベント駆動化(AddClipboardFormatListener + WM_CLIPCLIPUPDATE のメッセージ専用ウィンドウ)は、コピー反映レイテンシを 200ms→ほぼ 0 にしたいときの第二段階。費用対効果的にまずシーケンス番号で十分。

**How to measure or verify**
- 修正前後で「スクリーンショットを撮って放置した状態」の sd-win.exe の CPU 使用率とプロセスのコミット増減をタスクマネージャで比較(修正前は定常 1-3% 程度出ているはず、修正後は実質 0%)。
- ポーリング関数の elapsed を一時ログに出し、画像滞留時の p50/p99 を確認。
- 動作回帰は verify.sh の Win→Mac/Mac→Win クリップ双方向 2 項目で担保。

---

## F2: 「カクつき」の真因候補は warp 巻き戻し。シリアライズ系を触る前に必ず計測せよ

**Finding**
生命指標である「カーソル運動の滑らかさ」への影響度として、ホットパスの CPU コスト(後述の F4/F6/F7、合計 1イベントあたり推定 1-3µs)より、**WIN モード中の 150ms 監視スレッドによる CGWarpMouseCursorPosition の巻き戻し**の方がはるかに大きい可視ジッタ源になり得る。ワープはカーソルを強制的に LOCK_POS へ飛ばすため、頻発すればユーザーには「カーソルが引き戻されるカクつき」として見える。過去の「マウスがカクカク」という苦情(improvement-log 第2セッション)の対策履歴はサブピクセル累積(win側)と tap 軽量化だったが、warp 頻度そのものの計測記録が改善履歴に無い。

**Evidence**
- ~/ZCodeProject/seamless-desk/crates/mac/src/main.rs:1296-1304: 150ms 毎に live_cursor() と LOCK_POS を比較し、1px 以上ズレていれば CGWarpMouseCursorPosition で巻き戻す。`CGAssociateMouseAndMouseCursorPosition(false)` の効き遅れ・慣性がある環境では毎サイクル発火し得る。
- ~/ZCodeProject/seamless-desk/crates/mac/src/main.rs:1302: `DIAG_WARP_COUNT` に累積記録済み。
- ~/ZCodeProject/seamless-desk/scripts/verify.sh:100: `warp_fixed=[1-9]` の検出はあるが、**増加速度の評価は無い**。

**Expected impact**
warp_fixed が WIN モード中に毎秒数回増えるようであれば、これが体感カクつきの主因(実証には計測が必要)。逆に増えなければ本 finding は棄却でき、F1(Windows 側のアロケーションストーム)と F3 に嫌疑が移る。

**Suggested change**
まず変更なし。`--diag` 起動で WIN モード 30 秒間マウスを動かし、diag 行の `warp_fixed=` 増加を記録する。増加が続く場合のみ、巻き戻しのヒステリシス(例： 4px まで許容)や関連切断の再確認を検討する。**闇雲にシリアライズや Mutex を最適化してこの計測を取らないのは筋違い**です。

**How to measure or verify**
`grep '\[diag\]' /tmp/sd-mac-run.log | tail -30` の warp_fixed 差分/30秒。0 に近ければ F2 棄却。

---

## F3: osascript 通知が accept スレッドと接続直後の受信開始をブロックする

**Finding**
`notify()` は `Command::output()` でプロセス終了を待つ同期呼び出しで、これがサーバ accept/受信スレッド上で直接呼ばれている。osascript は fork/exec + AppleScript コンパイル/実行で一般的に 150-600ms(推測。要実測)かかる。

**Evidence**
- ~/ZCodeProject/seamless-desk/crates/mac/src/main.rs:312-324: `std::process::Command::new("osascript") ... .output()`(同期)。
- ~/ZCodeProject/seamless-desk/crates/mac/src/main.rs:1025: 接続確立直後、**受信ループ(1027行)の前**に呼ばれる。つまり接続直後 約 0.3-0.6 秒の Return/Pong/Clip 受信処理が遅延する。
- ~/ZCodeProject/seamless-desk/crates/mac/src/main.rs:1087: 切断時に accept ループ内で呼ばれる。Windows 側は 500ms 間隔で再接続に来る(~/ZCodeProject/seamless-desk/crates/win/src/main.rs:389-403)ため、通知表示中の再接続 accept が同程度遅延する。

**Expected impact**
切断→再接続の実測レイテンシに数百 ms 直接加算。入力の閉じ込め自体は WIN_MODE 強制解除(1085行)で守られているため悪化しないが、「切断通知を見てから繋がるまでの体感」が不要に長い。カクつきではなくライフサイクル遅延の問題。

**Suggested change**
`notify()` 内部を `std::thread::spawn` で発火させて即座に返す(fire-and-forget)。あるいは将来的に既存 objc ブリッジ経由で UNUserNotificationCenter を呼ぶ。リスク: 通知の表示順が入れ替わり得る程度(化粧品的)。同スレッドで notify を待つ理由はコード上一切ない。

**How to measure or verify**
ログの `[conn] established` から最初の受信処理まで、および `[conn] lost` から次の `accepted from` までのタイムスタンプ差を修正前後で比較。修正後は通知の有無にかかわらず accept 即時になるはず。

---

## F4: 全送信ラインが実質「二重改行」であり、受信側は毎イベントゴミ行をパースしている。かつ 1 イベント 1 flush

**Finding**
`encode()` が末尾に `\n` を付けるのに、全送信箇所がさらに `writeln!` で改行を足しているため、**ワイヤ上は全メッセージが `...}\n\n` で送られる**。受信側はこの空行を毎回アロケートして parse 前に skip している。「たまたま動いている」の好例で、厳格なパーサに変える日や行数カウント-based のデバッグをした瞬間に破綻する。加えて Mac 送信スレッドは 1 メッセージ受ける毎に lock→writeln→flush し、syscall とパケットをイベント数分消費する。

**Evidence**
- ~/ZCodeProject/seamless-desk/crates/common/src/lib.rs:70-74: `encode` が `s.push('\n')` で終わる。
- ~/ZCodeProject/seamless-desk/crates/mac/src/main.rs:932: `writeln!(s, "{line}")` → line 自体が `\n` 終端のため `\n\n`。952行(Ping)も同様。
- ~/ZCodeProject/seamless-desk/crates/win/src/main.rs:419, 456, 477, 508, 690: Windows 側も全送信が同じ二重改行。
- ~/ZCodeProject/seamless-desk/crates/win/src/main.rs:487-497: `reader.lines()` が空行も返すため、`line.trim().is_empty()` で skip。**MouseAbs 毎イベント + ゴミ 1 行**が String アロケート→trim→分岐を通る。
- ~/ZCodeProject/seamless-desk/crates/mac/src/main.rs:1028-1032: Mac 受信側も空行を `decode` 失敗として黙って捨てている。

**Expected impact**
- 受信ループの処理行数が実質 2 倍(毎イベント 余分な String アロケート+分岐、数百 ns)。単体では体感不可。
- Mac 送信が 1 イベントにつき write syscall 1-2 回+即 flush。TCP_NODELAY(982行)なのでイベント毎に独立パケット化し、WireGuard 暗号化もパケット単位で乗る。RTT 6ms に比べれば影響は小さいが、burst 時の syscall 数は無駄。
- 正確な遅延寄与は小さい(推定 50µs 以下)。**これは性能よりプロトコル衛生の問題**として片付けるべき。

**Suggested change**
`writeln!(s, "{line}")` を `s.write_all(line.as_bytes())` に置換するか、`encode()` から `push('\n')` を除去して writeln 側に統一する(どちらか一方)。さらに Mac 送信スレッド(924-957行)では、最初の `recv` 後に `try_recv` で drain し、纏めて 1 回の `write_all`+`flush` にする。drain は既にキューに入っているものだけを扱うため**遅延追加はゼロ**で、バースト時の syscall/パケットを 1/2〜1/10 に削減できる。壊すリスク: バッチ化でメッセージ順序を変えないこと(チャネルは FIFO なので naturally 保持)。

**How to measure or verify**
- 修正前後で `tcpdump -i lo0 port 24900 -X` を取り、行末が `\n\n`→`\n` になること、バースト時のパケット数減を確認。
- Windows 側に一時カウンタを足して skip した空行数を比較(修正後 0)。
- 回帰は verify.sh 7 項目+実機で cmd+Tab/IME/クリップを一通り。

---

## F5: MouseAbs のバースト時コアレッシング(最新のみ送信)は意味がある。「変化時のみ送信」はしてはならない

**Finding**
ミッション Q4 への回答。帯域の数値的な答えは「問題ない」。毎イベント送信 60-90B × 60-125Hz ≈ 4-11KB/s、ヘッダ込みでも 20KB/s 未満で Tailscale には全く効かない。一方、高速フリック時のトラックパッドは 125Hz を超えるイベント密度になり得るため、**絶対位置の性質上「古い MouseAbs は新しい MouseAbs で上書き可能」**であることを利用したコアレッシングだけは価値がある。

**Evidence**
- ~/ZCodeProject/seamless-desk/crates/mac/src/main.rs:763: 「量子化スキップは低速時にステップ感が出るため廃止」とある通り、**変化時のみ送信は低速時のステップ感という実績のある体感劣化を再発させるので禁止**。
- ~/ZCodeProject/seamless-desk/crates/mac/src/main.rs:767: 毎イベント `send_msg(&Msg::MouseAbs {...})`。

**Expected impact**
コアレッシング(F4 の drain 実装時に、バッチ内の MouseAbs は最後の 1 件だけ残して破棄)により、最悪時のパケットレートと Windows 側の SendInput 呼び出し回数に上限をかけられる。通常時は無変化。バイナリプロトコル化は 30B/イベントと数百 ns の削除効果しかなく、RTT 6ms の陰に隠れて検出不可能。**現状では不要**。GUI 化で画面ストリームを載せる話が出た時に再検討すればよい。

**How to measure or verify**
F4 と同じ実装箇所。`DIAG_ABS_COUNT`(送信数)と Windows 側注入回数の比が 1:1 から下がること、および高速フリック時の体感劣化がないことを実機確認。

---

## F6: LAST_ABS_SENT はデッド状態。ホットパスで毎イベント Mutex を取っている

**Finding**
宣言と 2 箇所の書き込みのみで、**読み出しがコードベース全体に存在しない**(grep で確認)。かつ WIN モードの移動イベント毎にロックされている。

**Evidence**
- ~/ZCodeProject/seamless-desk/crates/mac/src/main.rs:373(宣言)、695(初期化)、764(毎イベント書き込み)。読み出し箇所なし。

**Expected impact**
影響は 1 イベントあたり数十 ns で体感ゼロ。ただし純粋な無駄であり、タップコールバックという最もセンシティブなパス上のロックを 1 つ減らせる。削除は無料。

**Suggested change**
`LAST_ABS_SENT` を削除する。量子化スキップ廃止(763行)の際にwriteだけ残った名残と推定。

**How to measure or verify**
不要(デッドコード)。cargo build+verify.sh で回帰確認。

---

## F7: WIN_CUR の AtomicF64 化 — 性能目的では不要、デッドロック撲滅目的なら正当(ミッション Q2 への回答)

**Finding**
「WIN_CUR を毎イベント Mutex ロックしている」ことがパフォーマンス問題である、という共有資料の前提は**数字が支持しない**。書き込みは tap コールバックのみ、競合するのはモード切替時の読み出しのみで、非競合時の lock/unlock は arm64 で約 20-40ns。125Hz なら毎秒 5µs 未満。ホットパス 3 つの Mutex(WIN_SCREEN/WIN_CUR/LAST_ABS_SENT)合計でも 1 イベントあたり 0.1µs 程度(推定)。

**Evidence**
- ~/ZCodeProject/seamless-desk/crates/mac/src/main.rs:742(WIN_SCREEN lock)、752(WIN_CUR lock)、764(LAST_ABS_SENT lock)。
- 一方、~/ZCodeProject/seamless-desk/crates/mac/src/main.rs:748-752 のコメントと improvement-log ループ101〜104 が示す通り、**この Mutex はガード保持中の leave 呼び出しで自己デッドロックを起こした実績のある危険物**。

**Expected impact**
AtomicF64 化(2つの AtomicU64 に f64::to_bits で格納、単一書き込み者なので Relaxed store で十分)による性能利得は測定誤差以下。しかし**デッドロックのクラスごと消せる**ため、モジュール分割(今回の主題)の前処理としての価値は高い。

**Suggested change**
性能ではなく正しさを目的として AtomicF64 ペア化する。leave 側(473-478行)は WIN_CUR と WIN_SCREEN を同時読みするため、atomic 2 つに分けると値のペアが撕裂し得る(最悪でも LAST_WIN_POS の記憶位置が 1 イベント分古くなるだけで実害は小さいが、コメントで明示すること)。壊すリスク: 低。検証は F13 トグル・abs-left 復帰・Windows Return 復帰の 3 経路の実機確認。

**How to measure or verify**
性能を主張するなら tap コールバック所要時間のヒストグラム(後述)で前後比較。Δはほぼ出ないはずで、だからこそ「デッドロック撲滅が目的」と表明すべき。

---

## その他の確認事項(ミッション Q1/Q3/Q5 の残りと、資料との食い違い)

### Q1 ホットパスの遅延予算(推定値と計測法)

WIN モード移動 1 イベントの Mac 側追加コスト(推定、arm64 典型値に基づく):

| 区間 | 推定コスト |
|---|---|
| Mutex 3 lock/unlock(F6/F7) | 約 0.1µs |
| now_ms()(SystemTime、vDSO) | 約 0.03µs |
| serde_json to_string(約 65B)+ String アロケート | 約 0.3-0.5µs |
| mpsc send(ノード確保) | 約 0.1µs |
| 送信スレッド唤醒(コンテキストスイッチ) | 約 10-30µs |
| write syscall 1-2 回+ flush | 約 2-5µs |
| ネットワーク RTT | 約 6ms |
| Windows 側 lines() アロケート+空行 skip+ JSON parse | 約 0.5-1µs |
| SendInput(MOUSEEVENTF_ABSOLUTE)+ GetCursorPos | 約 5-50µs |

合計は約 6.1-6.2ms で 120Hz フレーム(8.3ms)に収まる。**CPU サイクル的なホットパスは遅延予算の 1% 未満であり、RTT が支配的**。Zero-copy 化(to_writer 直書き、再利用バッファ)は F4 の write_all 化+drain で十分。

計測方法： `--diag` に(1) tap_callback 入退の Instant 差分ヒストグラム、(2) send_msg 挿入時刻と送信スレッド write 完了時刻の差、を AtomicU64 の p50/p99 集計として追加する。diag インフラが既にあるので半日作業。**改善前にこれを取らない限り、どの最適化も効果を主張できない**。

### Q3 ポーリング周期の妥当性

- Mac クリップ 200ms(mac/main.rs:1226): changeCount 先行チェック済みで 1 ポール数µs。NSPasteboard には変更通知の公開 API が無く、changeCount ポーリングは macOS の標準作法。**このままでよい**。同期レイテンシ最大 200ms を詰めたいなら 100ms にしてもコスト増は無視できるが、体感上の価値は薄い。
- 監視 150ms(mac/main.rs:1256): WIN 中の live_cursor()+LOCK_POS チェックは約 10-50µs×6.7回/秒で無視できる。問題は周期ではなく warp の発火頻度(F2)。
- Win クリップ 200ms(win/main.rs:449): 周期は妥当、**中身が F1**。

### Q5 osascript

F3 に集約。起動コストの推定 150-600ms は実測待ち。`std::thread::spawn` での非同期化が最小変更。

### 資料・コメントとコードの食い違い(指摘義務のため記載)

1. **ハートビート**: 共有資料機能表#13 は「ping/pong 5秒毎、15秒無応答で切断」。実コードは ping 3秒毎(mac/main.rs:940)、pong タイムアウト 10秒(mac/main.rs:944)。improvement-log.md のループ20〜23(62行)が 3s/10s への変更を記録しており、**プロンプト側資料が古い**。~/ZCodeProject/seamless-desk/docs/design.md:79 も「5 秒毎 ping。15 秒」のまま stale。
2. **再接続バックオフ**: 資料は「0.5s→max5s」。実コードは `min(3000)` で max 3s(win/main.rs:402)。design.md:78 も max5s のまま stale。
3. **CUR_POS 同期間隔**: mac/main.rs:609 のインラインコメントは「32イベントに1回」だが実コードは `n % 16`(616行)。プロンプト資料の「16イベント毎」と improvement-log ループ59〜63 が正しく、**コード内コメントが古い**。
4. **Mutex 毒化回復「27箇所」**: 実測では mac 31 箇所+ win 2 箇所の `.lock()`(grep 集計)。件数の主張が古いだけで構造的問題はない。
5. docs/design.md:97-98 の成果物構成には `poc/` と `docs/usage.md` が記載されているが、poc ディレクトリは現状存在しない(glob 確認)。docs 配下は usage/improvement-log/design/README/agent-rebuild-prompt の 5 ファイル。

### 注意すべき隣接リスク(性能リファクタの制約になるため付記)

- **Windows 側の二重ライタ**: クリップボードスレッド(cb_writer)と serve ループ(writer)が同じソケットに直接 write する(win/main.rs:443-485 と 508)。`writeln!(w, "{}", encode(..))` は非バッファ TcpStream への 2 回の write syscall(本文+改行)に展開され得るため、マルチ MB の ClipData 送信中に 3 秒毎の Pong が割り込み、**行の途中に Pong が紛れ込み得る**。Mac 側が decode 失敗行を黙って捨てる設計のため今は「たまたま動いている」が、F4 の改行整理や Windows 側バッチ化を行うなら、Mac と同じ「単一送信チャネル+単一ライタ」構成への集約を同時にやるべき。実害の観測記録は無いため「可能性」として扱うこと。

---

## 改善着手順序(最終まとめ)

1. **計測を先に**: tap コールバックと送信レイテンシのヒストグラム追加+ warp_fixed 増加率の記録(F2)。これなしにシリアライズ系を最適化しても効果を証明できない。
2. **F1(GetClipboardSequenceNumber 先行チェック+エンコード前サイズ判定)**: 実装 30 分、Windows 側の定常無駄をほぼ撲滅。リスク最小。
3. **F3(notify の fire-and-forget 化)**: 実装 5 分。
4. **F4+F5(改行の単一化+送信スレッドの drain バッチ+ MouseAbs コアレッシング)**: プロトコル衛生と syscall 削減。Windows 側は単一ライタ集約とセットで。
5. **F6(LAST_ABS_SENT 削除)**: ついでに。
6. **F7(WIN_CUR atomic 化)**: 目的は「性能」ではなく「デッドロッククラスの撲滅」と明記して実施。モジュール分割の前処理にちょうどよい。
7. serde の zero-copy 化・バイナリプロトコル化・クリップの完全イベント駆動化は、上記の計測で上限が見えてから判断。現時点の数字では RTT 6ms の陰に隠れて検証不能です。

すべての変更後に指定サイクル(`cargo test --workspace` → restart-mac.sh → deploy-win.sh → verify.sh 7 項目 → 実機ログで `[edge]`/`[return]`/diag 確認)を回すことが前提です。

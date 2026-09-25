# Wave1-C エッジケースハンティング(第三者・読み取り専用・辛口)

- 実施日: 2026-09-25 / エージェント型: edge-case-hunter
- 対象: `~/ZCodeProject/seamless-desk`(mac 1347行 / win 725行 / common 262行 + scripts + win-dist + docs を全読了・相互照合)

## 総評

「6本の復帰経路」は入力閉じ込めへの対症療法であり、そのほぼすべてが**タップコールバックが生きていることを暗黙の前提**にしている。前提が崩れる経路(権限剥奪、sendブロック)で全経路が同時に死ぬ構造がある。また、verify.sh は相対モード・再接続・切断直後を一切検証しておらず、「動いているから正しい」の実態は「abs+edge+常時接続という1つの組合せでしか検証していない」である。以下、深刻度順。

---

## S1(致命): 相対モード(`SEAMLESS_MOUSE_MODE=rel`)でウォッチドッグが必ず誤発火し、モードとして成立していない

- **場所**: `~/ZCodeProject/seamless-desk/crates/mac/src/main.rs:765`(`LAST_ABS_MS`の更新が abs ブロック内のみ)、`同:776-779`(相対分岐は更新なし)、`同:1282-1291`(ウォッチドッグは `MOUSE_ABS_MODE` を判定外)
- **発生条件**: `SEAMLESS_MOUSE_MODE=rel` で起動 → edge切替 → マウスを動かす
- **現在の挙動**: `LAST_ABS_MS` は一度も更新されず初期値 0 のまま。ウォッチドッグ条件 `last_ev > 0 && now-last_ev < 2000 && now-last_abs > 5000` が「abs を送らない」だけで成立し、切替後最初の監視サイクル(150ms後)で強制 Mac 復帰。以後、切替のたびに即復帰を繰り返す。
- **想定被害**: 相対モードが実質使用不能。「守るべき動作：絶対/相対モードの双方が動作すること」に違反。fallback としての意味を失う。
- **再現手順**: `SEAMLESS_MOUSE_MODE=rel ./scripts/restart-mac.sh` → 右端ダブルタップで切替 → マウスを動かす → `/tmp/sd-mac-run.log` に `[watchdog]` が出る。
- **改善案**： ウォッチドッグ条件に `MOUSE_ABS_MODE` を組み込む(abs のときのみ last_abs を評価)、または相対モードでは `MouseMove` 送信時刻で更新。リスク: ウォッチドッグ無効化範囲の拡大は閉じ込め検知力を下げるため、`send_msg` 成功時点ではなく送信スレッドの実書き込み成功時に `LAST_ABS_MS` を更新する方が本来の意図に合う。検証： rel モードで 60 秒操作して `[watchdog]` が 0 回、かつ送信経路を人為的に殺した時に発火すること。

## S2(致命): タップコールバックがバッファレスチャネルの `tx.send` でブロックし、Mac 全入力がフリーズする

- **場所**: `~/ZCodeProject/seamless-desk/crates/mac/src/main.rs:918`(`mpsc::channel` はランデブー)、`同:533-539`(`send_msg` は同期 `send`)、`同:928-936`(送信スレッドは `writeln!` が write timeout 5秒までブロックし得る)
- **発生条件**： Windows が半開(スリープ遷移中、Tailscale 経路断)で TCP 受信ウィンドウが閉じる。Mac 側送信スレッドの `writeln!` が 5 秒ブロックしている間、tap_callback の `tx.send` が待たされる。
- **現在の挙動**： CGEventTap のコールバックがブロックされ、macOS がタップを timeout 無効化。監視スレッドの 1 秒毎再 enable により「数秒入力が固まる→戻る」が繰り返される。F13 や abs-left もタップ内処理のため、この間は**復帰経路を含めてすべて死ぬ**。Cmd+Q 等の Mac 側キーもタップ握りの後 OS へ流れるため遅延/消失する。
- **想定被害**： ユーザーから見て「Mac ごとフリーズ」。過去 5 回の「戻れなくなる」系と同根だがより広範(入力全体)。
- **再現手順**： Windows をスリープ直前のネットワーク断(Tailscale down)にし、Mac 側でマウスを高速に動かし続ける。MouseAbs の洪水で送信バッファが満杯になると発現(数秒〜数十秒)。
- **改善案**： `try_send` で即ドロップ(入力は数イベント失われるが OS 入力は守られる)か、`sync_channel(N)` + 満杯時ドロップ。リスク： 短い輻輳で入力が欠けるため、「N 連続ドロップで切断扱い」の併設が必要。検証： `iptables`/ファイアウォールで Windows 側受信を落とした状態で Mac 入力が固まらないこと。

## S3(高)： WIN モード中にアクセシビリティ権限が剥奪されると、6本の復帰経路が全て死ぬ

- **場所**: `~/ZCodeProject/seamless-desk/crates/mac/src/main.rs:549-554`(timeout 通知のみ再 enable)、`同:1273-1277`(1秒毎の再 enable は権限剥奪後は効かない)、`同:1282-1291`(ウォッチドッグは `LAST_EVENT_MS` 依存=タップ生存が前提)、`同:1259-1270`(self-heal は `WIN_MODE=false` の隠れカーソルのみ扱う)
- **発生条件**： WIN モード中(カーソル非表示+`CGAssociateMouseAndMouseCursorPosition(false)`)に、システム設定でアクセシビリティ権限を外す/tccutil reset/macOS アップデートで権限がリセットされる。
- **現在の挙動**： タップは通知なく死ぬ。F13・abs-left はタップ内なので死ぬ。ウォッチドッグは `LAST_EVENT_MS` が更新されなくなり `last_ev < 2000` が偽のため発動しない。self-heal は `WIN_MODE=true` のままなので発動しない。**カーソル非表示・関連切断・WIN_MODE=true の組み合わせで到達不能状態**。キーボードだけは OS 素通りで生きるため、ターミナルで kill すれば復帰できるが、マウスはない。
- **想定被害**： 完全なカーソル閉じ込め。状態組み合わせ爆発の中で唯一の「脱出不能」状態(質問2への回答の中心)。
- **再現手順**： WIN モード中にシステム設定 → プライバシーとセキュリティ → アクセシビリティで本バイナリを外す。
- **改善案**： 監視スレッドで「WIN モードかつ直近 N 秒タップ受信ゼロ」を異常扱いにする(現在は `last_ev > 0` かつ `<2000` しか見ないため「静かなだけ」を区別できない。`last_ev` が 5 秒以上更新されない WIN モードは異常とみなす検討。ただし Windows 側でキーボードだけ使う静穏運用との区別が要るため、「タップ死検知は CGEventTapIsEnabled 等の別手段」が望ましい——推測： この API は非公開のため、自己送出のプローブイベントで確認する方法が現実的)。

## S4(高): abs-left 復帰は Windows へ無通知のため、Windows 側の修飾キー・Alt+Tab 状態が残留する

- **場所**: `~/ZCodeProject/seamless-desk/crates/mac/src/main.rs:768-775`(abs-left は Mac 内完結・送信なし)。一方 Windows の解放は `crates/win/src/main.rs:693`(Return 通知時)と `同:673`(切断時)のみ。
- **発生条件**： WIN 中に cmd(→Windows では Ctrl)を押したままマウスを左端へ、または cmd+Tab(Switcher 開いたまま)で左端へ。
- **現在の挙動**： Windows の `mods.ctrl` が true・`ALT_TAB_ACTIVE`(win/main.rs:104, 静的で接続越しに残留)が true のまま。物理の cmd 離下は Mac モードなので転送されない。
- **想定被害**： 次回切替直後、最初のキーイベントまで Windows は Ctrl 押下状態。初回クリックが Ctrl+クリックとして誤動作(リンクのバックグラウンド開く、項目の複数選択開始)。残留 ALT_TAB_ACTIVE により無関係な cmd 離下で Tab/Alt up が注入される(win/main.rs:543-549)。
- **再現手順**： Windows 側で cmd を押しながら左端へ → Mac 復帰 → 再切替 → 直後にエクスプローラでクリック。
- **改善案**： abs-left 復帰時にも `Msg::Return`(ny 付き)を送信する(RTT が増えるが復帰自体は Mac 内完結のまま、解放通知だけ後送り)。RTT 削減の目的を崩さないよう fire-and-forget でよい。リスク： Windows 到達が遅れる間の 100ms 程度の残留は残る。検証： 上記再現手順のログで release_all が走ること。

## S5(高): 切断検知が最大 12 秒。pong timeout 後も `CONNECTED=true` のまま切替を許す

- **場所**: `~/ZCodeProject/seamless-desk/crates/mac/src/main.rs:944-949`(pong 10秒で `STREAM_SLOT` を外すだけ)、`同:983`(read timeout 12秒)、`同:1022-1024 / 1084-1086`(CONNECTED の false 化は受信ループ終了後)
- **発生条件**： Windows スリープ/ネットワーク断。
- **現在の挙動**： pong timeout(10秒)で送信だけ死ぬが `CONNECTED` は true のまま、受信ループは read timeout(12秒)まで生きる。**この 2 秒以上の窓で右端へ行くと WIN モードに入り、入力はすべて捨てられカーソル非表示のまま最大 12 秒の入力喪失**。WATCHDOG も救えない(`LAST_ABS_MS` は「送信試行」で更新されるため、`同:765` の更新は実送信成否と無関係)。
- **想定被害**： 「切断検知 15 秒間の入力喪失」の実態は最悪 12 秒+初動。スリープ復帰直後の再接続まで復帰できない。
- **再現手順**： Windows をスリープ → 直後に Mac で右端へカーソル。
- **改善案**： pong timeout で `STREAM_SLOT=None` にすると同時に `CONNECTED=false` を立て、WIN モード中なら leave する。リスク： 一時的な pong 欠測での誤切断(現在の 10 秒閾値は十分長い)。検証： スリープテストで `[conn] pong timeout` から 200ms 以内に `[mode] MAC` が出ること。
- **補足**： `STREAM_SLOT=None` 化のみで CONNECTED を維持する現在の設計は、S1 のウォッチドッグが「実送信」を見ないことと合わせ、検知の二重の穴になっている。

## S6(高)： Windows 側に read timeout / keepalive が無く、半開で永久再接続不能

- **場所**: `~/ZCodeProject/seamless-desk/crates/win/src/main.rs:406-407`(`set_nodelay` のみ)
- **発生条件**： Mac がスリープ/電源断/ネットワーク変更で黙って消え、Mac 側が発した FIN/RST が Windows に届かない。
- **現在の挙動**： `reader.lines()`(win/main.rs:487)が永久ブロック。再接続ループ(win/main.rs:390-403)へ制御が戻らず、Mac 側は Windows の再接続を待つだけ。**双方待機で永久に復帰しない**(推測： Tailscale は通常 FIN を届けるため頻度は低いが、スリープ中のパケット廃棄では起こり得る)。
- **改善案**： `stream.set_read_timeout(6秒)`(Mac の ping 3秒間隔に対し余裕あり)。timeout を Pong 欠測扱いで切断。リスク： 無通信 6 秒が正常運用で起きないことの確認(clip 無操作時は ping 以外来ないため成立する)。検証： Mac 側を kill -9 + 経路断で Windows が 6 秒強で再接続ループに入ること。

## S7(中): ディスプレイ構成変更で `UNION_MAX_X`/`EDGE_DISP_Y` が陳腐化(起動時 1 回のみ)

- **場所**: `~/ZCodeProject/seamless-desk/crates/mac/src/main.rs:843-862`(計算は main 起動時のみ)。使用箇所は `同:456-459`(lock_x)、`同:504-524`(復帰位置/EDGE_DISP_Y)、`同:625`(edge 判定)
- **発生条件と挙動**：
  - **モニター増設**(右へ): 旧 `UNION_MAX_X` を横切っただけで切替判定の積算が閾値を超える。二段階判定のライブチェック(`同:638` `loc.x < edge_x - edge - 40`)は「旧右端より右」では偽のまま発火を止められないため、**新モニターへ移動しようとすると Windows へ飛ぶ**(Mac 内新モニターに到達不能)。
  - **モニター減**： `UNION_MAX_X` が画面外を指し、ライブカーソルが到達し得ないため edge 切替不能。F13 で入った場合は `lock_x=UNION_MAX_X-2` が無効座標になり、固定監視ワープ(`同:1300`)が空振り連発。
  - **ミラーリング/解像度変更**： `EDGE_DISP_Y` 陳腐化で復帰 y がずれる(clamp あり、画面外には出ない)。
- **改善案**： `CGDisplayRegisterReconfigurationCallback` で再計算(`OnceLock` を `Mutex` 化 or AtomicF64)。リスク： 再計算中の切替判定との競合があるため、EDGE_GUARD を再設定してから差し替える。検証： 接続中にモニター増減して 1 秒以内に `[info]` で新 union が出ること。

## S8(中)： 切断時の WIN 復帰が `leave_win_mode_cursor_unlock` を通らず、ガード・位置整合が失われる

- **場所**: `~/ZCodeProject/seamless-desk/crates/mac/src/main.rs:1080-1087`(`WIN_MODE.store(false)` のみ)
- **現在の挙動**： カーソル表示は self-heal(`同:1259-1270`)に任せるが、(a) `EDGE_GUARD` 未設定、(b) 復帰ワープなし(実カーソルは LOCK 位置=右端-2 のまま)、(c) `CUR_POS` も LOCK 位置残留、(d) `LAST_WIN_POS` の退避なし。
- **想定被害**： 切断直後のマウス移動で CUR_POS 積算が右端から始まり、わずかな右移動で即ヒット(ダブルタップ 1 回目の跳ね返し)。接続復旧直後に意図せず Windows へ飛ぶ体感。自己修復経路と leave 経路で事後状態が非対称になるのは、今後の再発時の原因切り分けを難しくする(self_heal カウンタと diag の解釈が分岐する)。
- **改善案**： 切断時も `leave_win_mode_cursor_unlock(None)` を呼ぶ(冪等性は既に S9 以外で問題ない)。リスク： 二重 leave(F13 と同時切断)は 2 回ワープするだけで実害なし。検証： 切断時に `[return] -> mac` が出ること。

## S9(中): 監視スレッドの LOCK 巻き戻しと leave の TOCTOU で、復帰直後にカーソルが右端へ引き戻される

- **場所**: 監視 `crates/mac/src/main.rs:1296-1304`(WIN_MODE 確認 → LOCK_POS 取得 → live_cursor → ワープ)と leave `同:486-528`(LOCK_POS=None → ワープ)
- **発生条件**： 監視スレッドが `WIN_MODE=true` と `LOCK_POS=Some` を読んだ後、leave が完了するまでの数 ms にプリエンプト。
- **現在の挙動**： leave の復帰ワープ直後に、監視スレッドが古い LOCK 位置(右端)へ巻き戻しワープ。`EDGE_GUARD 400ms` が再突入は防ぐが、カーソルが一瞬右端へ飛び、`CUR_POS`(復帰位置)と実位置(右端)が乖離。16 イベント毎の同期で補正されるまで積算がずれる。
- **改善案**： 巻き戻し直前に `WIN_MODE` を再確認する(double-check)。監視は 150ms 周期で高頻度に leave と競合するため、発生確率は無視できない。検証： `--diag` で `[return]` 直後の `cursor=` が境界値を示す回数を数える。

## S10(中): `SEAMLESS_HOTKEY_KC=54`(右 Cmd)等の修飾キー指定が機能しない + F13 keyUp 握りがデッドコード

- **場所**: `~/ZCodeProject/seamless-desk/crates/mac/src/main.rs:559-566`(ホットキー発火は `event_type == EVT_KEY_DOWN` のみ)。外側 `if`(同 559 行)は `EVT_KEY_DOWN || EVT_FLAGS_CHANGED` であり、修飾キー押下は `EVT_FLAGS_CHANGED` で届くため発火しない。
- **矛盾**： `同:71-72` のコメントと `docs/usage.md:88-90` は「右 Cmd=54 に変えられる」と案内。**動かない機能をドキュメントしている**。MacBook 内蔵キーボード(F13 無し)での hotkey モードは事実上 F6 等の通常キーでしか代替できない。
- **追加**： `同:562-565` の `if event_type == EVT_KEY_UP` は外側 if の中にあり**到達不能**(デッドコード)。improvement-log の「F13 keyUp も握る」(24〜33)は現在の配置では効いておらず、F13 up は macOS へ素通りする。
- **改善案**： FLAGS_CHANGED でも keycode 一致なら握ってトグルする分岐を追加。検証： `SEAMLESS_HOTKEY_KC=54` で右 Cmd がトグルすること、F13 の down/up が OS に漏れないこと。

## S11(中): Windows 側で複数スレッドが同一 TCP ソケットへ同時書き込み、行が混線し得る

- **場所**: `~/ZCodeProject/seamless-desk/crates/win/src/main.rs:508`(受信スレッド:Pong)、`同:690`(Return)、`同:456/477`(クリップ監視スレッド:`cb_writer`=try_clone)
- **現在の挙動**： `writeln!` はフォーマット断片を複数回 write しうる。1MB 画像送信中(数百 ms)に Pong/Return が同時 write すると JSON 行が混ざり、Mac 側 `decode` が `None` でスキップ。**Pong が欠損すれば pong timeout → 偽切断 → 再接続**(S5 と連鎖)。
- **改善案**： Mac 側と同じ単一送信チャネル構成に寄せるか、writer を `Mutex` 化。検証： 大画像コピー中に切断が起きないこと。

## S12(中): `clipboard_read_text` のオフバイワン(1MB 境界を 1 文字超過)

- **場所**: `~/ZCodeProject/seamless-desk/crates/win/src/main.rs:122-126`
- **現在の挙動**： `while *p.add(len) != 0 && len <= CLIP_MAX_CHARS` は `len == CLIP_MAX_CHARS` でもう 1 回評価し、NUL でなければ `len` が `CLIP_MAX_CHARS + 1` になり 2 バイト分グローバルメモリを余分に読む。割り当て境界ぎりぎりの 1MB テキストで理論上オーバーラン。
- **改善案**： `len < CLIP_MAX_CHARS`。検証： 正確に 1MB 文字+NUL のクリップボードを CF_UNICODETEXT で作って読む単体テスト(mac/win のテストが無い現状、まず common 以外の最初のテスト候補)。

## S13(中)： ドラッグ持ち込み防止は左ボタンのみ。右/中ドラッグ切替で Windows 側にボタン残留

- **場所**: `~/ZCodeProject/seamless-desk/crates/mac/src/main.rs:671-673`(`btn: 0` のみ送信)
- **現在の挙動**： `EVT_RIGHT_DRAGGED` で切替すると Windows の右ボタンが押されたまま。次回 Windows 操作の初回マウス移動/クリックでコンテキストメニュー等が誤発火。中ボタンも同様。資料の既知の抜け穴だが、`EVT_OTHER_DRAGGED` を切替条件に含めている(`同:602`)以上、3 ボタン全部の up を送るのが整合的。
- **改善案**： 切替時に btn 0/1/2 の up をまとめて送信。リスク： 特になし(冪等)。

## S14(低〜中)： `run_sd.bat` のログローテーションが実起動経路から外れている

- **場所**: `~/ZCodeProject/seamless-desk/win-dist/run_sd.vbs:2`(exe を直接 `>>` リダイレクト)。起動登録は `install.bat:14,17` と `scripts/deploy-win.sh:18` のすべてが vbs 経由。`run_sd.bat:4-5` のローテーションはどの経路からも使われない。
- **現在の挙動**： `sd-win.log` は追記のみで無限増殖(improvement-log 59〜63 の「run_sd.bat ログローテーション」は効果がない状態)。clip/ime/conn のたびに println! が出るため長期運用でディスク圧迫。機能 18「ログローテーション」は実在しない。
- **改善案**： vbs から `run_sd.bat` を起動する。リスク： bat 経由の一段増加で起動失敗モードが増えるため、deploy 後 verify.sh でのプロセス確認を必須に。

## S15(低〜中)： その他の品質・整合性指摘

1. **既定トークンへのフォールバック**: `mac/main.rs:834`、`win/main.rs:340-341` が `unwrap_or("seamless-desk-dev")`。`.env` 忘れで無警告の実質無認証運用になる。権限を落とすか、環境変数不在時に fatal にするのが望ましい。
2. **未使用コード**: `win/main.rs:19 GetAsyncKeyState`、`同:30-32 VK_LBUTTON_SENTINEL/VK_XBUTTON1/2`。デッドコード。
3. **KEYEVENTF_EXTENDEDKEY 未設定**： `win/main.rs:215-221`。矢印(0x25-0x28)・Enter・Delete 等の拡張キーが NumPad キーと区別されない(NumLock 状態で挙動が変わり得る)。
4. **BI_BITFIELDS 非対応**： `mac/main.rs:207-230 dib_to_bmp` は biSize=40(BI_RGB)前提のパレット計算。16/32bpp の BI_BITFIELDS な CF_DIB で offBits がずれ、画像同期が静かに失敗する。
5. **b64 デコードのパディング検証緩さ**: `common/src/lib.rs:107-138` は途中の `=` を受け入れる。自前実装と対なので実害は限定的だが、well-formed 検査を入れる余地。
6. **now_ms() のシステム時刻依存**： `mac/main.rs:356-361`。SystemTime の巻き戻り(NTP ステップ)で `EDGE_GUARD`/pong 判定が一時的に狂う。saturating_sub でパニックはしないが、ガードが長時間化し得る。Instant ベースが望ましい。

## 資料(共通資料・docs)と実コードの不整合

| 項目 | 資料の記述 | 実コード |
|---|---|---|
| 復帰ガード | usage.md:20「250ms」 | `mac/main.rs:481` で **400ms** |
| カーソル同期間隔 | usage.md:44「32イベントに1回」 | `mac/main.rs:616` で **16 イベント毎** |
| ダブルタップ窓 | usage.md:94「500ms以内」/機能一覧「700ms」 | `mac/main.rs:650` は **700ms**(usage.md が古い) |
| ping/pong | 共通資料・design.md:79「5秒毎/15秒」 | `mac/main.rs:940/944` は **3秒毎/10秒**(usage.md:68 のみ正しい) |
| 再接続 | design.md:78「Mac がバックオフ再接続」 | 逆転構成で再接続は **Windows 側**(`win/main.rs:390-403`)。design.md に逆転前の記述が残存 |
| IME 実装 | usage.md:29-31「ImmGetContext/ImmSetOpenStatus」 | 実装は **ImmGetDefaultIMEWnd + WM_IME_CONTROL**(`win/main.rs:75-96`)。usage.md が古い |
| ログローテーション | 機能18「run_sd.vbs+ローテーション」 | **機能していない**(S14) |
| 復帰経路数 | 「6本」 | ⑤タップ再有効化と⑥ウォッチドッグはタップ/送信前提で、S2/S3 の条件下では**同時に死ぬ**(実効 4 本) |
| 「KEY_UP も握る」 | improvement-log 24〜33 | `mac/main.rs:562` は**到達不能**(S10) |

チューニング値の記録が実態とずれていること自体がリスクである。再発防止の根拠(「なぜ 400ms なのか」)が検証できなくなるため、docs 更新を CI(crate 版の定数抽出+テスト)で担保する価値がある。

## 検証優先順位の提言

1. `SEAMLESS_MOUSE_MODE=rel` でのウォッチドッグ誤発火(S1)— 1 回の起動で確認可能。最優先。
2. Windows スリープ/kill -9 での入力フリーズと再接続(S2/S5/S6)— 再現手順明確。半開は `sudo ifconfig` 等でエミュレート可能。
3. WIN 中の権限剥奪(S3)— 手動 1 回で確認可能。致命度に対して検証コストが最も低い。
4. abs-left 復帰時の Windows 側修飾残留(S4)— `--debug-keys` + Windows 側ログで Ctrl 残留を観察。
5. モニター増減(S7)、混線(S11)、ローテーション(S14)は運用タスクとして。

とくに S1 は「fallback が実は 1 庋りも動作検証されていない」ことを意味し、verify.sh 7 項目合格が品質の証明になっていない典型例である。

# Knit 操作権(コントロール権)の設計 — M02・M03

- 作成: 2026-10-19 / 対象: HEAD `725b13e`(作業ツリーはクリーン)
- 状態: **設計のみ。コードは変更していない。cargo build / cargo test も実行していない。**
- 入力: [端末モデル設計書](2026-10-19-knit-device-model-design.md) §3.3・§9、[ギャップ分析](../gap-analysis-20260927.md) M02/M03、[要件定義](../../output/2026-09-27_tsunagu-product-requirements/requirements.json) M02/M03、現行コード(§0.1 で file:line を確認)
- M02/M03 は要件定義で **status=提案・未承認 / verification=未検証**。承認済みとして扱わない。
- 前提: 失効・切断基盤と M03 のリース初版(5秒でカーソル復帰、commit `3d49c7a`)は入っている。本書はそれを置き換えず、**受信側の管理**を足す。
- 検証の制約: macOS でビルドできるのは `crates/common` と `crates/mac` のみ。`crates/win` の配線は CI(windows-latest)と実機が最初の検証([端末モデル設計書](2026-10-19-knit-device-model-design.md) §5 と同じ)。

---

## 0. 先に結論(3行)

1. 操作権を「2つの暗黙フラグ」から**受信側が持つ期限付きリース+世代**へ移す。受信側(いまは Windows)が唯一の裁定者になり、要求 `control_request` / 応答 `control_state`(許可・拒否+理由) / 解放 `control_release` / 強制終了 `control_revoke` の4行を、版18の `peer_features.control` で門にする。旧端末には1行も送らない。
2. **通常経路の遅延は増やさない。** 送信側は要求と同時に入力を流し始め(楽観)、受信側は「拒否・失効と決めたときだけ門を閉じる」。奪おうとした入力は OS に1つも届かない。拒否の伝播だけが片道 RTT ぶん遅れる。
3. 期限は**5秒の生存リース**(操作のアイドル期限ではない)。更新は既存の ping 1秒/pong で、押下状態の期限はリースに従属させる。失効・切断・ロック・権限取消・失効のどの経路でも `release_everything` はちょうど1回。ただし **M02 の「C が同時に要求する」実機シナリオは、受信側の複数セッション化(今回の範囲外)が前提**で、現行トポロジでは再現できない(§8-1)。

---

## 0.1 設計の前提になる現行実装(自分で読んだ結果)

| # | 事実 | 根拠(file:line) | 設計への影響 |
|---|---|---|---|
| **C1** | 操作権は型ではなく2フラグ+副作用。Mac は `WIN_MODE`(bool)と `ACTIVE_PEER`(添字)、受信側は `CONTROLLED`(bool)。期限・保持者・理由を持つ場所が無い。`WIN_MODE` を書く箇所は13(HEAD `725b13e` の grep 実測) | `crates/mac/src/state.rs:7`、`crates/mac/src/conn.rs:105`、`crates/win/src/dragdrop/edge.rs:18` | 状態機械を新設し、両フラグをその射影に降格する |
| **C2** | 受信側は入力の受理を操作権で判定していない。`Key`/`MouseMove`/`MouseAbs`/`MouseButton`/`Scroll` の分岐に `CONTROLLED` の参照が無い。参照するのはドラッグ・転送系だけ | `crates/win/src/session.rs:444-573`、`crates/win/src/dragdrop/host.rs:17`、`crates/win/src/dragdrop/edge.rs:453-461`、`crates/win/src/xfer.rs:187`、`crates/win/src/doctor.rs:90` | M02 の「入力が混ざらない」を担保する場所が今日は存在しない。門を新設(§2.5) |
| **C3** | `CONTROLLED` は遷移ではなく副作用でも立つ。`Warp` で true、**マウス押下でも true**、`Leave`/reset で false | `crates/win/src/session.rs:602-603,592-593`、`crates/win/src/input.rs:352-356`、`crates/win/src/dragdrop/edge.rs:106` | キーボードだけで操作している間は「操作中」でない状態があり得る。F13/メニューの入口は `Warp` を送らない(`crates/mac/src/tap.rs:1544-1551`)ので実際に起きる |
| **C4** | 受信側の期限は暗黙の9秒だけ。受信ループの read timeout が9秒で、無音なら `release_everything` | `crates/win/src/session.rs:144-146,270,276,748` | 「9秒間は押しっぱなしが残り得る」。状態としては表現されない |
| **C5** | 5秒リースは**送信側だけ**・実装済み。`lease_expired` を writer スレッドの1秒周期から呼び、pong 5秒途絶で leave → `Leave` 送信 | `crates/mac/src/conn.rs:565-573`、`crates/mac/src/main.rs:1294-1306`、`crates/mac/src/tap.rs:1465-1467` | 受信側が送信側の生存を測る仕組みは無い。逆向きを足す(§4) |
| **C6** | 離脱の経路が複数・Windows 向け `Leave` の送信は3箇所(Android 向けの送信は別系統)。コード自身が「既存6経路」と書く | 通常離脱 `tap.rs:1466`、ピア切替 `conn.rs:413`、失効 `conn.rs:750`、コメント `tap.rs:1543` | 遷移の一元化が本設計の主目的の1つ |
| **C7** | 送信の世代は**全行**に掛かる。`send_msg_reported` が全メッセージに `OUTBOUND_GENERATION` を刻み、古い世代は書かない。進めるのは7箇所 | `crates/mac/src/main.rs:592-605`、`crates/mac/src/outgoing.rs:8-31`、`conn.rs:402,653,736`、`main.rs:1278,1325`、`session.rs:350,807` | **離脱経路は世代を進めない**。古い入力の破棄は受信側の門と順序保証で行う(§3.5、§7-4) |
| **C8** | 未知行は両側で無視される。拡張の作法は「版ゲートで送らない」 | `crates/mac/src/session.rs:81`、`crates/win/src/session.rs:283-286`、`crates/common/src/proto.rs:4-25,32-55` | 新行は版18の相手にだけ送る。MIN_VERSION=11 は据え置き |
| **C9** | 受信側は同時に1セッションしか持てない。Win の待受は accept ループの中で `session()` を直接呼ぶ | `crates/win/src/conn.rs:353-458` | M02 の「C が B を要求」は接続段で起きない(2台目は待たされる)。§8-1 の弱点 |
| **C10** | ロック・権限の既存動作。入力が届かない環境では `Return` を送って制御を返す。ロックは押下を解放するが `CONTROLLED` は変えない。緊急解除は Mac の F13/メニューと Win のトレイ | `crates/win/src/inputdesk.rs:150-168`、`crates/win/src/session.rs:574-578`、`crates/mac/src/tap.rs:1544-1569`、`crates/win/src/tray.rs:897-910` | 遷移として表現し直す(§2.4)。緊急解除の導線は維持する |

未確認: `crates/win` の OLE 周辺の細部と実機挙動(この Mac ではビルドできない)。

---

## 1. 目標体験

### 1.1 3つの約束

| 約束 | 利用者が感じること | 支える機構 |
|---|---|---|
| **奪わない** | 他の端末が操作中なら、入っても入力は1つも届かない。理由が手元に出る | 単一保持者+理由付き拒否(§2.2)+門(§2.5)+通知(§3.3) |
| **待たせない** | 入る瞬間に追加の待ちが無い | 楽観送信(§2.3)。拒否は片道 RTT で戻る |
| **切れたら戻る** | 5秒で手元に戻り、押しっぱなしが残らない。復旧後に古い入力が発火しない | 生存リース(§4)+全経路の解放+順序保証(§3.5) |

### 1.2 いま壊れる機構(感情 → 原因)

| 感情 | 原因 | 根拠 |
|---|---|---|
| 「無断で奪われた」 | `Warp` は受信側の同意を取らない。今日は2台目が接続できないので表面化していないだけ | `crates/win/src/session.rs:602-613`、C9 |
| 「入ったのに効かない」 | F13/メニューの入口は `Warp` を送らないので、受信側の `CONTROLLED` が立たない(マウスを押すまで立たない) | `crates/mac/src/tap.rs:1544-1551`、`crates/win/src/input.rs:352-356` |
| 「切れたのに押しっぱなし」 | 無音9秒まで解放されない | C4 |
| 「戻ったのに古い入力が発火」 | 離脱は世代を進めない。受信側に門が無いので、送信待ちの入力が離脱直後に注入される | C7、C2 |

---

## 2. 状態機械(受信側=操作される側が管理)

### 2.1 用語(3つの世代を混ぜない)

| 名前 | 所有者 | 意味 | 実装 |
|---|---|---|---|
| `control generation`(新) | 受信側 | 許可のたびに+1する fencing token。押下状態の所属 | 新規(メモリのみ) |
| `OUTBOUND_GENERATION` | Mac | 本線の送信世代。古い行を捨てる | `crates/mac/src/main.rs:592-605`、`outgoing.rs:8-31` |
| `PEER_GEN` / `PeerEntry.gen` | Mac | セッション(接続)の世代 | `crates/mac/src/conn.rs:81` |

- 保持者(holder) = `{ session, device_id, name, since, gen }`。受信側プロセスのメモリだけに置く(永続化しない。§7-7)。
- 期限 = `last_seen + 5000ms`。**生存の期限**。
- gen は受信側プロセスの起動で 0 に戻る。セッションも同時に切れるので不変条件は壊れない。

### 2.2 受信側(Authority)の状態と遷移

```
Idle ── Request(S) ──▶ Held{ S, gen }
Held ── Release(S, gen一致) / Tick(期限) / SessionEnded(S) / Lock / PermissionLost / EdgeReturn ──▶ Idle
Held ── Request(S'≠S) ──▶ Held(変更なし) + Deny(Busy, holder=S)
```

- 状態は2つだけ。**承認待ち(Pending)は作らない**(v1 は受信側が即断する。明示承認UIは §7-2)。
- `Held` 中の別セッションの要求は状態を変えず `Deny(Busy)`。理由に保持者の表示名を載せる(名前は hello で交換済みで、両側 `safe_peer_name` を通る: `crates/mac/src/session.rs:456-463`、`crates/win/src/conn.rs:308-315`)。
- 門は**セッションごと**に持つ: `NeverRequested | Granted{gen} | Closed{reason}`。保持者とは別の地図で、`Release` 後は `Closed{Released}`(次の要求で再評価)。
- 効果は `SendState / SendRevoke / GateOpen / GateClose / ReleasePressed / Notice / Log`。**効果は冪等**(同じ Tick・同じ SessionEnded で `ReleasePressed` は1回)。
- 遷移表(Event × State → 新状態・効果)を単体テストの表にする(§9 I1)。

### 2.3 要求側(Requester)の状態

```
Idle ── 入口(境界越え/F13) ──▶ Requesting{req} ── granted ──▶ Holding{gen}
                                        └── denied ──▶ Rejected{reason} ──▶ Idle
Holding ── revoke / 期限 / 切断 ──▶ Idle
```

- `may_forward_input() = Requesting | Holding`。**Requesting が楽観区間**で、ここで入力を流すから入口の待ちが増えない。
- `control_state` が3秒来なければ `Holding{gen:0, degraded}` に落とす。理由: 版18の相手が応答しない実装不整合でも、今日の動作(操作できている)を壊さないため。ログに残す。
- `Rejected` で 入力停止 → カーソル復帰 → 通知 → `Idle`。復帰は既存の `leave_win_mode_cursor_unlock`(`crates/mac/src/tap.rs:1443-1452`)を通す。
- 受信ループの早期フィルタ(`crates/mac/src/session.rs:85`)は非アクティブ相手の行を捨てるので、要求は**必ずアクティブ相手にだけ**送る(現行の入口は有効化後なので満たしている: `tap.rs:2081`)。

### 2.4 切断・ロック・権限取消・失効の遷移

| 事象 | 受信側 | 要求側 | 押下 | 通知 |
|---|---|---|---|---|
| セッション終了(切断・EOF・9秒 timeout) | Held→Idle | `on_disconnect` で Idle(`crates/mac/src/session.rs:341-393`) | ReleasePressed 1回 | 既存の切断通知(60秒間引き、`session.rs:384-391`) |
| リース失効 | Held→Idle + `control_revoke(LeaseExpired)` | Holding→Idle + カーソル復帰 | ReleasePressed | 「5秒応答が無いため戻した」(新規) |
| 画面ロック(Win 自身/`Lock` 受信) | Held→Idle + `control_revoke(Locked)` | Holding→Idle | ReleasePressed | ロックの既存文言に合わせる。解除後の自動復帰はしない(§10-5) |
| 権限取消(保護デスクトップ・UIPI・補助なし) | Held→Idle + `control_revoke(NoPermission)` | Holding→Idle | ReleasePressed | 既存の inputdesk の文言(`crates/win/src/inputdesk.rs:150-168`)を流用 |
| 台帳の失効(revoke / 外部 revoke) | セッション終了として処理 | `control_release(Revoked)` を直接書いてから shutdown(`crates/mac/src/conn.rs:750` の置換) | ReleasePressed | 既存の切断通知(理由は汎用。§8-8) |
| 受信側の端到達(`Return` を送る) | Held→Idle | 既存の `Return` で復帰(`crates/mac/src/session.rs:153-167`) | ReleasePressed | 通知しない(今日と同じ) |

### 2.5 入力の門: 「拒否と決めたときだけ閉じる」

- セッションの状態は `NeverRequested | Granted{gen} | Closed{reason}`(§2.2)。
- **`NeverRequested` は開**。旧 Mac(版17以下)・要求が落ちた場合でも今日の動作を壊さない。
- `Closed` は閉。閉じている間は Key・MouseMove・MouseAbs・MouseButton・Scroll を**注入しない**(ドラッグ系も同じ判定を通す)。
- 置き場所は受信ループの `match` の直前1箇所(`crates/win/src/session.rs:287`)。判定は common の純関数なので macOS でテストできる。
- 既定開の理由: 「まだ要求を見ていない」ことを理由に黙って入力を捨てると、旧端末・要求欠落時に**無言の機能停止**になる。この repo の「無言を作らない」に反する。

---

## 3. メッセージ設計

### 3.1 新規4行(版18)

| t | 方向 | フィールド | 意味 |
|---|---|---|---|
| `control_request` | 要求側→受信側 | `req: u64` | 操作権の要求。位置は運ばない |
| `control_state` | 受信側→要求側 | `req: u64`, `granted: bool`, `reason: u8`, `generation: u64`, `lease_ms: u32`, `holder: String` | 許可/拒否。拒否のとき `reason` と `holder` |
| `control_release` | 要求側→受信側 | `generation: u64` | 自発解放。冪等 |
| `control_revoke` | 受信側→要求側 | `generation: u64`, `reason: u8` | 強制終了(期限・ロック・権限・失効) |

- 追加先は `Msg`(`crates/common/src/proto.rs:228-465`)。全フィールドに `#[serde(default)]`(端末モデル設計書 §6-2)。
- 版ゲート: `VERSION=18`(`proto.rs:21`)、`MIN_VERSION=11` 据え置き、`PeerFeatures.control = ver >= 18`(`proto.rs:32-55`)。**`control` が false の相手には新行を1行も送らない**(呼び出し側に版リテラルを書かない)。
- 旧側は未知行として無視する(C8)。したがって「送っても無害」だが、送らない方がログが静かで、旧側の未知行処理に頼らない。
- `request` の `req` は要求の識別子(1から採番)。応答の対応付けとログにだけ使う。

### 3.2 要求の運び方(Warp との関係・F13 の穴)

- 境界越えの入口(`crates/mac/src/tap.rs:2081` の有効化 → `:2114` → `:2185`): `control_request` → `Warp` の順に同じ書き込みバッチへ積む。順序は mpsc+単一 writer が保証する(`crates/mac/src/main.rs:1264-1286`)。
- 手動トグル(F13/メニュー、`crates/mac/src/tap.rs:1544-1569`)は**今日 `Warp` を送らない**。したがって `control_request` だけを送る(位置は変えない)。これで C3 の「キーボードだけだと操作中でない」が新 Win 相手では解消する。
- 受信側は `Warp` を「位置指示」に限定し、**許可されていないセッションの `Warp` は捨てる**(拒否された要求で相手のカーソルを動かさない)。旧 Mac の `Warp` は従来どおり許可扱い(legacy)。

### 3.3 理由コード(u8・安定)

| code | 名前 | 使える方向 | 文言の要点 |
|---|---|---|---|
| 1 | `Busy` | deny | 「⟨保持者⟩ が操作中です。終わってから入り直してください」 |
| 2 | `LocalPriority` | deny | 「この PC は直接操作中です」 |
| 3 | `Locked` | deny・revoke | 「ロック中のため操作できません」 |
| 4 | `NoPermission` | deny・revoke | 「入力の権限が無いため操作できません」 |
| 5 | `Disabled` | deny | 「この PC は遠隔操作を受け付けない設定です」 |
| 6 | `LeaseExpired` | revoke | 「5秒応答が無かったため手元へ戻しました」 |
| 7 | `DeviceRevoked` | revoke | 「端末が登録解除されました」 |
| 8 | `ProtocolError` | deny | 「要求を解釈できませんでした」 |
| 9 | `PeerReleased` | 記録のみ | 通知しない |

- 文言は `common::notice`(`crates/common/src/notice.rs:1-80`)へ寄せ、**成功語を入れない**(dnd I5 と同じ作法)。理由と次の一手を必ず含める。
- コードは追加のみ。既存コードの意味を変えない。

### 3.4 Warp/Leave との共存・置換

| 相手 | 入口 | 解放 | 受信側の扱い |
|---|---|---|---|
| 版18 | `control_request`+`Warp` | `control_release` | 状態機械。`Leave` を受けても同じ後片付け(互換) |
| 版17以下 | `Warp` のみ(現行) | `Leave`(現行) | legacy: `Warp`=無条件許可、`Leave`=解放。状態機械は `Held{gen}` を作るが拒否しない |

- **`Leave` を消さない。** `Leave` は「操作権の解放」と「Win 側の後片付け(クリップボード同期・ドラッグ中止)」を兼ねている(`crates/win/src/session.rs:592-601`)。版18では `control_release` を**同じ関数**に通す。
- 版18で `Leave` と `control_release` を両方送らない(クリップボード同期が2回走る)。送信側の `release_plan(features)` で1つに決める。
- `Warp` は版18でも座標を運ぶので残す。置き換えるのは「`Warp` が暗黙に操作権の取得を兼ねる」という意味だけ。

### 3.5 入力行に世代を足さない(理由と代替の保証)

- 入力行は毎イベント通るホットパスで、構築箇所も macOS 側に散る。フィールド追加は費用対効果が悪い。
- 代替: **同一 TCP ストリームは順序を持つ**。要求は「それ以前の入力より必ず後」に積まれ、解放は「次の要求より前」に積まれる。したがって再許可の後に、解放前の入力が届くことはない。
- 受信側の門(§2.5)が「解放後に届いた入力」を捨てる。世代は制御メッセージの fencing(同じセッション内の古い `release`/`revoke` を無視)とログにだけ使う。
- 不変条件: **キューを迂回する直接書き込み(`crates/mac/src/conn.rs:413,750`)は、そのセッションを終了させる経路だけで使う**。破ると順序保証が崩れる。

---

## 4. 期限(リース)

### 4.1 何の期限か

- **生存の期限**であって、操作のアイドル期限ではない。1秒ごとの ping/pong が5秒途絶えたら「相手はもう居ない」とみなす。無操作で解除されたら作業が壊れる。
- 片側だけでは足りない。送信側は「受信側が生きているか」(pong)、受信側は「送信側が生きているか」(ping)。両方が同じ 5秒を持つ。

### 4.2 更新と数値

| 定数 | 値 | 根拠 | 現在 |
|---|---|---|---|
| `LEASE_MS` | 5000 | 更新間隔1秒の5倍(3連続損失+余裕) | Mac のみ実装(`crates/mac/src/conn.rs:567`) |
| 更新信号(Mac→Win) | ping 1秒(`crates/mac/src/main.rs:1320-1323`) | 既存 | 実装済み |
| 更新信号(Win→Mac) | ping 3秒(`crates/win/src/session.rs:183-196`) | 既存 | 実装済み |
| 受信側の更新 | 保持セッションから**デコードできた任意の行**で `last_seen` 更新 | 新しい keepalive 行を作らない | 未実装 |
| 判定周期 | Mac 1秒(`main.rs:1294`)、Win 250ms(新設) | 実発火は Mac 5〜6秒 / Win 5.0〜5.25秒 | Win 側は未実装 |

- 「任意の行」にする理由: 入力が流れている間は ping より高頻度。ping だけに限定すると、ping が詰まったときに動いているのに切れる。入力だけで更新すると、動いていないときに切れない。
- 数値は**未測定**(§8-5)。Wi-Fi の遅延・損失・スリープ復帰の分布を測っていない。

### 4.3 押下状態の期限(リースに従属)と受信側の押下世代

- 押下は独立したタイムアウトを持たない(長押しを壊すため)。**押下の期限 = 操作権の期限**。
- 受信側の押下世代 = `Held.gen`。`Held` に入る時点で押下集合は空であることを不変条件にする(前の保持者の押下を引き継がない)。
- `Held` から出る全経路(解放・失効・ロック・権限・切断)で `ReleasePressed` を**ちょうど1回**。
- 押下状態の実体は既存の `ModState`(`crates/win/src/input.rs:556-597`)。状態機械は「解放せよ」という効果だけを出し、VK の注入は Win 側が実行する(判断は common、OS 依存は実行だけ)。

### 4.4 既存5秒リースとの統合

- `lease_expired(win_mode, connected, last_pong, now)`(`crates/mac/src/conn.rs:571-573`)は**そのまま残す**。第1引数を `WIN_MODE` から `requester.may_forward_input()` に変える(`main.rs:1299-1304`)。
- 失効時に2つ足す: ①`control_release` を送る(届かなくてもよい) ②通知(「5秒応答が無いため戻した」)。今はログだけ(`main.rs:1305`)で、利用者には何も出ない。
- 通知は60秒に1回へ間引く(切断通知と同じ作法、`crates/mac/src/session.rs:384-391`)。

### 4.5 時計とスリープ

- 期限は単調時計(`crates/common/src/clock.rs:1-22`)。**スリープ中は進まない**。復帰は既存の別経路が拾う(Mac のスリープ検知 `drop_stream`: `main.rs:1243-1256`、Win の read timeout 9秒)。
- したがって「スリープ復帰直後にリースが即失効」は起きない。復帰時の後始末は既存経路に依存する(§8-6)。

---

## 5. 後方互換と移行

| 組み合わせ | 挙動 | リスク |
|---|---|---|
| 新 Mac × 新 Win | 状態機械で裁定 | なし(今回の検証対象) |
| 新 Mac × 旧 Win(版17以下) | `peer_features.control=false` なので `Warp`/`Leave` のみ。要求側は `Idle` のまま | 今日と同じ |
| 旧 Mac × 新 Win | `Warp` のみ受信 → legacy 許可(無条件)。新 Win は `control_state` を返さない | 今日と同じ |
| 片側ロールバック | 新 Win を旧版へ戻すと hello の ver が17に戻り、新 Mac は従来経路へ落ちる | 壊れない |

- 不変条件: `MIN_VERSION=11` 据え置き。未知行の無視(C8)を破らない。`serde(default)` を全フィールドに付ける。
- 版番号は hello/hello_ok で交換される(`crates/mac/src/session.rs:749-757`、`crates/win/src/conn.rs:334-346`)。`peer_features` 以外で版を直接見ない(端末モデル設計書 §6-1)。

---

## 6. 受け入れ基準(M02/M03 から導く)

### 6.1 反証可能な述語

| # | 基準 | 証明方法 | 現状 |
|---|---|---|---|
| **AC1** | 単一保持者。S1 が `Held` のとき S2 の要求は `Deny(Busy, holder=S1)`。S1 の入力は受理、S2 の入力は効果ゼロ | common の単体+結合テスト | 未実装 |
| **AC2** | 混入ゼロ。`Closed` のセッションの Key down/up・MouseMove/Abs/Button・Scroll は `Effect::None` | 入力種別ごとの表テスト | 未実装 |
| **AC3** | 期限。`last_seen+4999ms` は Held、`+5000ms` で Idle + `control_revoke(LeaseExpired)` + ReleasePressed。同じ Tick の2回目は効果なし | 境界テスト | 定数と Mac 側の純関数のみ実装済み(`conn.rs:565-573`) |
| **AC4** | 押しっぱなし無し。Held→Idle の全経路(解放・失効・ロック・権限・切断・端到達)で `ReleasePressed` がちょうど1回 | 経路の表テスト | 未実装(Win の実行は CI/実機) |
| **AC5** | 古い入力。`Idle`/`Rejected` で `may_forward_input()==false`。解放→再要求→許可の後、解放前の入力が効果を持たない | 表テスト+順序の議論(§3.5) | 未実装 |
| **AC6** | 旧端末。`peer_features(17).control==false`、`(18)==true`、`MAX==true`。v17 相手の `enter_plan`/`release_plan` に `control_*` が含まれない | 表テスト | 未実装 |
| **AC7** | 無言にしない。拒否・失効の通知に成功語が無く、理由と次の一手がある | 文言テスト(notice と同型) | 未実装 |
| **AC8** | 回帰なし。`cargo test --workspace` が緑。既存の WIN_MODE/Leave 経路のテストが無変更で緑 | CI | 設計時点では未実行 |
| **AC9** | 遅延を増やさない。入口から最初の入力までの追加待ちが0(楽観送信)。測るなら `[mode] WINDOWS` と最初の転送イベントの時刻差を前後比較 | **未測定**(方法だけ定める) | 未測定 |

### 6.2 要件 → 設計 → テストの対応

| 要件の句 | 設計 | テスト |
|---|---|---|
| M02「どの対応端末も入力元・操作先になれる」 | 状態機械を common に置き、要求側/受信側の両ロールを同じ型で表現(今回は受信=Win だけ配線) | AC1 |
| M02「受信側が操作権を管理し、同時操作の競合を解決する」 | Authority が唯一の裁定者。単一保持者 | AC1・AC2 |
| M02「相手が操作中なら理由を示し、無断で奪わない」 | `Deny(Busy)`+保持者名、門で入力遮断、通知 | AC1・AC7 |
| M02「手元の物理操作と緊急解除を優先する」 | 既存の Win トレイ・Mac の F13/メニューを維持。物理入力の検出は §7-5(やらない) | 実機(未) |
| M02 受け入れ「A が B を操作中に C が B を要求しても入力が混ざらず、承認された引き継ぎ後に A の入力は届かない」 | AC1(S1/S2)+AC2(門)+解放→再要求の順序(AC5) | **実機は複数セッション待ち**(§8-1) |
| M03「独立した解除操作と共有一時停止」 | 解除は既存(F13/メニュー/トレイ)。一時停止は「解放」で代替(§7-9) | 実機 |
| M03「押下状態と操作権に期限」 | 5秒生存リース+押下はリースに従属 | AC3・AC4 |
| M03「切断・ロック・権限取消で手元に戻し、修飾キー・ボタンを解放」 | 遷移表(§2.4)+ReleasePressed | AC4 |
| M03「古い入力は再送しない」 | 順序保証+門+`may_forward_input` | AC5 |
| M03 受け入れ「切断を押下中に起こしても押しっぱなしが残らず、復旧時に古いキーやクリックが発火しない」 | AC4/AC5(Win 実機の最終確認は必要) | AC4・AC5 |

---

## 7. 実装しないと決めたこと(理由つき)

| # | やらないこと | 理由 |
|---|---|---|
| 1 | **受信側の複数セッション化**(M02 の同時要求の前提) | Win の待受は accept ループ内で `session()` を実行し(`crates/win/src/conn.rs:353-458`)、状態の大半がグローバル。ローカル検証不能で規模が大きい。状態機械は N 要求者対応で先に置き、配線は単一セッションのまま |
| 2 | 明示的な引き継ぎ承認 UI | 承認の主体が未決(§10-2)。v1 は「保持者の解放=承認」 |
| 3 | 入力行への世代フィールド | ホットパスと構築箇所の多さ。同一 TCP の順序で代替できる(§3.5) |
| 4 | 離脱時の `OUTBOUND_GENERATION` 加算で古い入力を捨てる案 | 世代は**全行**に掛かる(`main.rs:592-605`)ため、離脱の瞬間に並んでいた XferAck/DragDone まで捨てる。入力だけを捨てるには行種別が要る(別増分)。代替は受信側の門+順序保証 |
| 5 | ローカル物理入力の検出と優先 | `GetLastInputInfo` は注入も含むため物理と区別できない。LL フック(`LLKHF_INJECTED`)は常駐フックの追加でリスクが高く、Win 実機でのみ検証可能。既存の「トレイから戻す」「左端到達」で代替 |
| 6 | 押下の独立タイムアウト(長押しの強制解放) | 正当な長押し(ゲーム・スクロール)を壊す |
| 7 | 操作権の永続化 | 受信側の再起動=セッション断=解放が正しい。「無断で奪わない」に反する |
| 8 | 部分操作権(ポインタのみ/キーボードのみ) | M02 の基準は「混ざらない」。v1 は all-or-nothing。`want` は将来フィールドで足せる |
| 9 | 「解放せず入力を止める」一時停止状態 | 状態が増える割に、F13/メニュー/トレイの解放で足りる |
| 10 | Android の `TabletInfo.control` の統一 | 権限の意味が違う(アプリ側の許可)。§10-6 |

---

## 8. 弱点・未検証の前提(正直に)

1. **M02 の同時要求は実機で再現できない**(C9)。ループバックの裁定器テストが上限。製品要件を満たすには受信側の複数セッション化が要る。
2. **Win 側はこの Mac でビルドできない**。I5〜I7 の配線は CI(windows-latest)と実機が最初の検証。
3. **遅延は未測定**。楽観送信で通常経路は増えない設計だが、拒否の伝播は RTT 依存。LAN/Tailscale の実 RTT は `RTT_MS` で測れるが、本設計では測っていない。
4. **F13 の `Warp` 欠落(C3)は新設計で塞がるが、旧 Win(版17以下)相手では塞がらない**。実害の範囲(キーボードだけの操作でドラッグ中継が効かない等)は未検証。
5. **5秒は設計値で未測定**。短すぎれば誤解放、長すぎれば押しっぱなし。ping 1秒との比だけが根拠。
6. **スリープ中はリースが進まない**(§4.5)。復帰の後始末は既存経路依存で、本設計のテスト範囲外。
7. **`CONTROLLED` にはドラッグ系の読み手が4箇所ある**(`host.rs:17`、`edge.rs:453-461`、`xfer.rs:187`、`doctor.rs:90`)。状態機械の射影に降格するが、移行中に片方だけ更新するとドラッグが黙って効かなくなる。**更新は必ず遷移関数1箇所**。
8. **失効(台帳)経路の理由通知は弱い**。`cut_sessions_where` は `Leave` を書いて切る(`conn.rs:750`)ので、受信側から見ると「切断」で、理由は「操作権を失った」ではなく汎用の切断通知になる。
9. **`Leave` の副作用**(クリップボード同期・ドラッグ中止、`crates/win/src/session.rs:592-601`)を `control_release` が通らないと D01/D04 が退行する。増分のテストで「同じ後片付け関数を通る」ことを固定する必要がある(Win 側のテストは CI)。
10. **門は「入力の注入」だけを止める**。既に注入済みの押下は解放しないと残る。門を閉じる遷移と `ReleasePressed` を必ず同時に出す(AC4)。
11. **本設計は HEAD `725b13e` のコードを読んで書いた**。cargo build / cargo test は実行していない(設計タスクのため)。増分の着手時に基準の緑を確認すること。

---

## 9. 増分リスト(1増分=1コミット)とテスト計画

### 9.1 増分

最初の増分を小さくするため、状態機械を受信側(I1)と要求側(I2)に割る。**I1 だけで「受信側が誰に操作権を渡すか」がテストで固定でき、まだ何も配線しない**(振る舞いの変化ゼロ)。

| 増分 | 内容 | 触る場所 | 検証 | 依存 | 行数の目安 |
|---|---|---|---|---|---|
| I1 | 受信側の裁定器(Authority) | `crates/common/src/control.rs`(新)、`lib.rs` | 単体テスト | なし | 150〜200 |
| I2 | 要求側(Requester) | `crates/common/src/control.rs` | 単体テスト | I1 | 100〜150 |
| I3 | 版18とメッセージ型 | `crates/common/src/proto.rs` | 表テスト | I1・I2 | 80〜120 |
| I4 | Mac の要求側配線 | mac の tap/session/conn | 単体+既存回帰 | I3 | 120〜160 |
| I5 | Win の裁定側配線 | win の session/conn | CI ビルド+Win 単体+実機 | I3 | 150〜200 |
| I6 | Win の受信側リースと押しっぱなし | win の session/main | 判定は common、配線は CI+実機 | I5 | 100〜150 |
| I7 | ロック・権限・失効の遷移 | win の session/inputdesk、mac の conn | 表テスト+実機 | I5 | 80〜120 |
| I8 | 通知文言と文書 | `common/notice.rs`、docs、`decisions.tsv`、CHANGELOG | 文言テスト+レビュー | I1〜I7 | 60〜100 |

---

### I1. 受信側の裁定器(Authority)

- **何を変えるか**: `crates/common/src/control.rs`(新)に `Authority` と `Event`/`Effect`/`Reason`、`LEASE_MS = 5000` を置く。`now` は引数で受け、`proto` には依存しない。`lib.rs` に `pub mod control;` を足す。**配線はしない**。
- **なぜ**: 設計の核で、macOS だけで全遷移をテストできる。最初の増分として「誰に渡すか・いつ失効するか・押下をいつ解放するか」だけを固定する。
- **検証**: `cargo test -p knit-common` — 全 Event × 全 State の遷移表、期限境界(4999/5000/5001)、冪等(Tick 2回・SessionEnded 2回)、legacy(`Warp`/`Leave` 列)、`ReleasePressed` のちょうど1回、`Busy` の保持者名。
- **リスク**: 状態を増やしすぎる。→ v1 は `Idle`/`Held` の2状態。`Pending` は作らない。
- **行数の目安**: 150〜200(本体+テスト)

### I2. 要求側(Requester)

- **何を変えるか**: 同じ `control.rs` に `Requester`(Idle/Requesting/Holding/Rejected)と `may_forward_input()`、要求タイムアウト(3秒)を足す。
- **なぜ**: 楽観区間と拒否時の復帰を、配線前に値として固定する。
- **検証**: 表テスト(拒否で `Idle` に戻る・期限で `Holding`→`Idle`・タイムアウトで `degraded`・`Rejected` で入力を流さない)。
- **リスク**: 受信側と二重に状態を持つ。→ 要求側は「自分が信じている状態」だけを持ち、真実は受信側に置く(§2.1)。
- **行数の目安**: 100〜150

### I3. 版18とメッセージ型

- **何を変えるか**: `VERSION=18`、`PeerFeatures.control`、4つの `Msg` 行、効果→Msg の写像、送信計画(`enter_plan(features)`/`release_plan(features)`)。
- **なぜ**: ワイヤ形式を1箇所に固定し、呼び出し側に版リテラルを書かせない(端末モデル設計書 §6-1)。
- **検証**: 版ゲート表(17/18/`u32::MAX`)、encode→decode 往復、v17 の計画に `control_*` が無いこと、未知行の decode が `None` になること(旧側の無視の前提)。
- **リスク**: 版を上げると両側同時配備が要る。→ `control` は能力フラグで、片側だけでも従来経路に落ちる(§5)。
- **行数の目安**: 80〜120

### I4. Mac の要求側配線

- **何を変えるか**: 入口2つ(境界越え `tap.rs:2114-2185`、手動トグル `tap.rs:1544-1569`)で `control_request` を送る。受信ループ(`session.rs:81-330`)に `control_state`/`control_revoke` の分岐を足して `Requester` を更新。拒否時は `leave_win_mode_cursor_unlock` + 通知 + `Idle`。離脱3箇所(`tap.rs:1466` を `release_plan` 経由に、`conn.rs:413`/`750` は `control_release` を直接書く)。
- **なぜ**: Windows 側を待たずに、要求・拒否・解放の挙動を macOS で作れる。旧 Win 相手では1行も出ない。
- **検証**: 単体(写像・拒否で復帰・v17 で送らない)。既存の tap/session テストが緑。実機は版18の Win が要る(未)。
- **リスク**: 受信ループの早期フィルタ(`session.rs:85`)が非アクティブ相手の行を捨てる。→ 要求は必ずアクティブ相手にだけ送る(現行の入口はそうなっている)。
- **行数の目安**: 120〜160

### I5. Win の裁定側配線(CI・実機のみ)

- **何を変えるか**: `Authority` をグローバル(Mutex)に置き、`session()` にセッションIDを渡す。`control_request` で裁定し `control_state` を返す。`Warp` は許可済みのときだけ適用。入力分岐の先頭に門(§2.5)。`Leave` と `control_release` を同じ後片付け関数へ。`CONTROLLED` は `Held` の射影として遷移関数だけで更新する。
- **なぜ**: 受信側が裁定者になる本体。
- **検証**: CI の windows ビルド+Win の単体(門・後片付け)。実機: 1台で「入る→操作→戻る」が従来どおり。
- **リスク**: 門の置き場所を誤ると全入力が止まる。→ `NeverRequested` は開(§2.5)。門の判定は common の純関数でテスト済み。
- **行数の目安**: 150〜200

### I6. Win の受信側リースと押しっぱなし(CI・実機のみ)

- **何を変えるか**: 250ms のウォッチドッグを1本追加。`ModState` を `Arc<Mutex<>>` にしてウォッチドッグから `release_everything` を呼べるようにする。期限で `control_revoke(LeaseExpired)`。`last_seen` は decode 成功時に更新。
- **なぜ**: 受信側の期限が今日は9秒の read timeout だけ(C4)。
- **検証**: 判定は common(AC3)。Win 側は CI+実機(切断中の押下が5〜6秒で解放されること)。
- **リスク**: ロック競合。→ 入力イベントごとに1回ロックするだけ(競合は250msに1回)。既存の `release_all_input`(`win/src/input.rs:108-120`)では `ModState::pressed` を解放できないため `Arc` 化が要る。
- **行数の目安**: 100〜150

### I7. ロック・権限・失効の遷移(両側)

- **何を変えるか**: 受信側(Win)が `Lock` を受けたとき(Mac がロックした)と Win 自身がロックしたときに `revoke(Locked)`。inputdesk の `fall_back_to_mac`(`150-168`)を `revoke(NoPermission)` へ。`cut_sessions_where` の `Leave` を `control_release(Revoked)` に(版18のみ)。通知文言。
- **検証**: 理由コードの表テスト、Win 実機(ロック中に入れない・ロックで戻る)。
- **リスク**: ロック連動の既存動作(`crates/mac/src/session.rs:319-326`、`crates/win/src/session.rs:574-578`)を壊す。→ 失効は「操作権だけ」で、ロックの連動自体は変えない。
- **行数の目安**: 80〜120

### I8. 通知文言と文書

- **何を変えるか**: `notice.rs` に拒否・失効の文言を追加(成功語なし)。`decisions.tsv` に各増分の行、CHANGELOG、runbook 追記。
- **検証**: 文言テスト+レビュー。
- **行数の目安**: 60〜100

### 9.2 テスト計画

| 層 | 何を証明するか | 置き場 | 実行 |
|---|---|---|---|
| 純関数(common) | AC1〜AC7 | `crates/common/src/control.rs` の `#[cfg(test)]` | macOS/CI |
| 結合(common) | 2要求者の裁定、メッセージ往復(版18/版17)、解放→再要求 | `crates/common/tests/control_rights.rs` | macOS/CI |
| Mac 配線 | 入口・拒否復帰・v17 で送らない | mac の単体 | macOS/CI |
| Win 配線 | 門・後片付け・リース | win の単体 | CI(windows)+実機 |
| 実機 | 混入ゼロ(2台)、押しっぱなし、遅延、スリープ復帰 | runbook 追記 | 実機のみ |

- **実機でしか証明できないもの**: 実際の混入ゼロ(M02 の C は複数セッション待ち)、遅延、スリープ復帰、Windows の入力注入の実挙動。
- **テストで担保しないもの**: 通知の見た目・表示時間、Win のトレイ UI、ネットワークの実損失。

---

## 10. 未決事項(利用者の判断が要る)

1. **M02 の「C が B を要求」を製品として満たすか。** 満たすなら受信側の複数セッション化が次の設計対象(今回の範囲外)。満たさないなら、M02 の受け入れ基準を「1対1運用での割り込み」に読み替えることを要件側に明記する必要がある。
2. **引き継ぎの承認主体。** v1 案は「保持者の解放=承認」。受信 PC の前に人がいるとは限らないため、明示承認 UI の要否を決める。
3. **5秒の妥当性。** 実測を先に取るか、5秒で出して実機で調整するか。
4. **拒否・失効の通知先。** Mac の通知センターのみ / Win のトレイにも / 両方。
5. **ロック解除後に自動で復帰させるか。** 要件は「ロックで手元に戻す」なので v1 は失効。利用者が「解除したら続きから」を期待するなら別途。
6. **Android の `TabletInfo.control`**(`crates/common/src/proto.rs:349-350`、`crates/mac/src/session.rs:284-300`)を同じ状態機械に載せるか。
7. **Win 実機の検証担当と時期。** I5〜I7 の受け入れは実機が要る。

---

## 11. 実装状況(追記用)

- 2026-10-19: 起草。実装は未着手。M03 のリース初版(5秒でカーソル復帰)は commit `3d49c7a` で実装済み。M02(受信側の管理)は設計のみ。

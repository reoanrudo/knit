# Knit ドラッグ&ドロップの内部地図(2026-10-18)

**これは保守者向けの文書**です。「どこで何が起きるか」「どの不変条件を壊してはいけないか」
「何かあったとき、どのログを見るか」を1枚にまとめています。利用者向けの手順は
[実機確認ランナップ](./2026-10-18-knit-dnd-verification-runbook.md)、経緯と数値は
[最終報告書](./2026-10-18-knit-dnd-final-report.md)を参照してください。

---

## 1. 2つの向き

### 1.1 Mac → Windows(掴んで運ぶ)

```
Finder でファイルを掴む
  └ tap.rs: MouseDown → FILE_DRAG.begin(changeCount)      crates/mac/src/tap.rs:1851
      └ 監視スレッド(main.rs:1340 付近)が 16ms 間隔で probe
          └ ペーストボードの changeCount が変わった → ファイル URL を読む
              └ FILE_DRAG.complete(...) → Ready(files)
境界を越える(tap.rs:2097 付近)
  └ FILE_DRAG.take() → files::offer_drag_to_win()          crates/mac/src/files.rs:332
      ├ 件数・経路・集計・送信枠を drag::verdict_for() で判定(理由は値)
      ├ Msg::DragOffer を本線へ
      ├ 版17の相手なら Msg::Manifest(名前・サイズだけ)     crates/mac/src/files.rs:923
      └ files::send_drag_files_to_win() → bulk で実体を流す  crates/mac/src/files.rs:495
Windows 側
  ├ Msg::DragOffer → dragdrop::expect(id, count)            crates/win/src/dragdrop.rs:28
  ├ (版17 + KNIT_LAZY=1)Msg::Manifest → dragdrop::start_lazy()
  │     crates/win/src/dragdrop.rs:593 → lazy_com.rs(目録だけで DoDragDrop)
  └ 実体が届く → xfer.rs が claim → Carried なら dragdrop::start()(CF_HDROP)
ドロップ後
  └ Windows が Msg::DragDone{id, copied} を返す
      └ Mac 側 files.rs が通知を3状態で言い切る
```

### 1.2 Windows → Mac(相手のファイルを運ぶ)

```
Windows でファイルを掴み、Mac の画面へ
  └ win/edge.rs: 境界で Msg::DragOffer(entries を予告)
      └ Mac incoming_drag::offer()                          crates/mac/src/incoming_drag.rs:116
          ├ 共有範囲・WIN_MODE・押下・容量・画面内を確認
          ├ 受ける → Msg::DragAccept / 断る → Msg::DragCancel
          └ 受けたら「受け取り待ち」を立てる(進捗表示のため)  crates/mac/src/incoming_drag.rs:9
Windows が実体を流す(accept 後)
  └ Mac incoming_drag::receive()                            crates/mac/src/incoming_drag.rs:178
      ├ 予告と一致 → Msg::DragReady → ネイティブ NSDraggingSession を開始
      └ 一致しない → 通知(無言にしない)                      crates/mac/src/incoming_drag.rs:174
ドロップ後
  └ Mac が Msg::DragDone{id, copied} を返す
```

---

## 2. 状態機械(すべて `crates/common` = macOS でテストできる)

| 型 | 役割 | 場所 |
|---|---|---|
| `drag::Carried` | 「越えてきた押下が続いている間だけ」転送をドラッグとして引き継ぐ | `crates/common/src/drag.rs` |
| `drag::Incoming` | 相手が用意したファイルを、予告IDと件数で照合して受け取る | 同上 |
| `drag::lazy::LazyHandoff` | 遅延提供(目録→受理→流す→結果)の遷移と命令 | `crates/common/src/drag/lazy.rs` |
| `drag::forecast` | 「今離すとどうなるか」(置ける/保存のみ/待ち/渡らない/不明) | `crates/common/src/drag/forecast.rs` |
| `notice` | 結果の3状態と文言(保存のみに成功語を入れない) | `crates/common/src/notice.rs` |
| `hud` | カーソル近傍の案内の位置決め(画面外へはみ出さない) | `crates/common/src/hud.rs` |

---

## 3. 壊してはいけない不変条件(テストが守っている)

| 不変条件 | なぜ | テスト |
|---|---|---|
| **離しの記録は粘着させない**(押下の開始と claim の消費で消す) | 粘着すると「一度クリックしただけで以後ずっと保存のみ」になる(実際にそのバグがあった) | `drag::tests::a_new_drag_after_a_release_is_carried_again` |
| **新しい押下で「前の操作の事実」を覆さない** | 覆すと、離した後に届いた古い転送で OLE ドラッグを開始してしまう | `drag::tests::a_new_press_does_not_resurrect_a_transfer_that_arrived_after_release` |
| **予告と実受信の件数が一致したときだけドロップにする** | 欠けたまま置かせない | `drag::tests::a_partial_transfer_is_not_carried_as_a_drag`, `lazy::tests::a_count_mismatch_never_becomes_a_drop` |
| **遅延提供は全データ到着を待たない** | 待つと「押し続ける時間 = 転送量」に戻る | `lazy::tests::lazy_delivery_does_not_wait_for_the_bytes` |
| **能力(版)ではなく受理(事実)で「置ける」と言う** | 相手が遅延提供を無効にしていても版は17になり得る | `forecast::tests::capability_without_acceptance_still_waits` |
| **保存のみの本文に成功語を入れない** | 「置いたのに置かれていない」を成功通知と同じ見た目にしない | `notice::tests::saved_only_never_uses_success_words` |
| **言い切りは相手の結果が届いた経路だけ** | 送信側が観測できるのは「転送が終わった」ことまで | `notice::tests::only_a_peer_confirmed_drop_claims_the_place` |
| **OLE ドラッグを二重に立てない** | 二重になると操作が壊れる | `dragdrop::lazy_start_tests::lazy_start_needs_enabled_no_drag_and_a_non_empty_manifest` |
| **どの中止理由でも片付けは一度だけ** | 二重に消すと受信途中の実体を壊す | `lazy::tests::releasing_before_acceptance_aborts_and_cleans_up_once` |
| **無言にしない**(拒否理由・待ち・失敗・結果) | ユーザーが「壊れた」と誤解する最大の原因 | 各文言のテスト + `incoming_drag::receive_failed_tests` |

---

## 4. ゲート(既定の挙動を変えない仕組み)

| ゲート | 意味 | 既定 |
|---|---|---|
| `proto::peer_features(ver).lazy` | 相手が版17以上か(能力) | 版16以下とは現行方式 |
| `KNIT_LAZY=1`(Windows) | 遅延提供を**実際に使う**か | **無効** |
| 受理(`Msg::DragAccept`) | 相手が**この掴み**を先に受理した事実 | 受理が来るまでは「待つ」と言う |

**遅延提供が効かない条件**(どれか1つでも欠ければ現行方式):
相手が版16以下 / Windows 側が `KNIT_LAZY` 未設定 / 受理の前に離した / 押下が続いていない。

---

## 5. 困ったときの見方(ログ)

| 症状 | まず見るログ |
|---|---|
| 越えたのに何も起きない | Mac: `[drag] mac->win 対象外:` の理由行 / `<何もしない>` の通知 |
| 置けずに受信フォルダに入る | Mac: `[drag] mac->win offer` → `[file] 掴みドラッグ切替` → `転送完了`。Windows: `[drag] 受信のみ扱い` |
| 遅延提供が効かない | Mac: `[drag] mac->win manifest`(出るか)/ Windows: `[drag] 遅延提供…`(開始・受理・終了) |
| 受け取りが進まない | Mac: カーソル近傍の HUD / Windows: `[drag] 遅延提供: データが N バイトに達しません` |
| 掴みが断られる | Mac: カーソル近傍の HUD + 通知 / メニューの「直前: …」 |

Mac のログは `~/Library/Logs/knit/knit-mac.log`(5MB でローテーション)。

---

## 6. 変更するときの作法

1. **判断は `crates/common` の純関数に置く**(macOS で全遷移をテストできる。Windows 側は命令を実行するだけ)。
2. **Windows 固有コードは macOS でビルドできない**(`cfg` で除外)。変更は
   **WIP ブランチ + ドラフトPR** で CI(windows-latest)を通してから main へ。
3. **文言を変えたら「ゴールデンリスト」で照合**(`verify.md` §1 に抽出器)。
   既存の文言を意図せず変えていないか、多重集合で確認する。
4. **テストを書いたら変異を入れて赤を確かめる**(実装は正しいがテストが守っていない、が繰り返し起きた)。
5. **無言を作らない**: 失敗・待ち・結果・拒否には必ず理由か次の一手を出す。

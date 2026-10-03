# 9/28ドラッグレビュー指摘の現行コード照合

作成: 2026-09-30。[ロードマップ §10-2](../plans/2026-09-30-global-commercialization-roadmap.md)
「9/28ドラッグレビューを項目単位で現在コードと照合し、解決済み・再現あり・未確認を証拠付きで整理する」の実施。
対象は 2026-09-30 の作業ツリー(未コミット変更込み、プロトコル v14)。読み取り専用の別エージェント照合。

判定(2026-10-01 未明の改善サイクル後): **解決済み 18件 / 設計上の意図 1件(M8) /
残り 1件(L3 caps 導入)**。
当初の照合では解決済み 4件(H4・M3・L5・L7)だったが、2026-09-30 夜のサイクルで
H1・H5・M1・M4〜M7 を、2026-10-01 未明のサイクル(Cycle 5/7)で H2・H3・M2・L1・L2・L4 を
追加解決した(各項目の「解決済み」印を参照)。
H2/H3/M2/L1 はコード解決済みで実機での e2e 確認が残る。残る L3 と実機確認は次サイクル以降の候補。

## HIGH

### H1 65件以上の掴みが無通知で64件に切り詰め — **解決済み(2026-09-30 Cycle 1)**

- 修正: `pb_files`(crates/mac/src/main.rs)と NSOpenPanel 経路(crates/mac/src/gui.rs)の
  `n.min(64)` 切り詰めを除去し、本当の件数を返す。掴みドラッグは `offer_drag_to_win` の
  件数判定(> MAX_FILES で拒否通知)が、⌘C は `bulk::collect` の上限審査(512件・10GiB)が
  拒否と通知を担う。独立レビューで拒否経路の推移(Refused 保持・通知の非重複)を確認済み。

### H2 予告より先に転送完了が届くとドラッグにならない — **解決済み(2026-10-01 Cycle 5・実機確認残る)**

- 修正: `crates/common/src/drag.rs` に `await_claim`(5ms ステップで claim を再試行し、
  Unknown 以外が返れば即返す)を追加。`crates/win/src/main.rs` win_on_bulk は ID 付き転送が
  Unknown なら deadline 500ms(= `dragdrop::CLAIM_WAIT_MS`、start の押下待ちと対)まで予告の
  遅着を待ち、Carried/Released/Mismatch を既存分岐へ合流する。bulk 読み取りスレッドは本線と
  分離しているため待ち中も本線受信は止まらない。旧版ピア(ID 無し転送)は待機なし。
  テスト `await_claim_picks_up_a_late_announce_within_the_deadline` ほか2件

### H3 合成 LeftMouseUp が端で原本を移動し得る — **解決済み(2026-10-01 Cycle 7・実機確認残る)**

- 修正: `crates/mac/src/main.rs` — 押下位置を `PRESS_POS` に記録し、越境後60msスレッドは
  spawn 時コピーした押下開始位置へ `CGWarpMouseCursorPosition` で戻してから合成 Up を投稿
  (自己ドロップ=Finder の no-op に変える)。位置決定は純関数 `drag_end_position`。
  warp 前に `CGSetLocalEventsSuppressionInterval(0.0)` を呼び、抑制窓(0.0001s)で Up が
  破棄される経路を塞ぐ。テスト3件+位置読み戻し assert

### H4 セッションスレッドから NSWindow の orderOut: — **解決済み**

- 証拠: `crates/mac/src/incoming_drag.rs:99-113` — reset は AppKit を一切触らず ACTIVE を RETIRED へ
  退避して DragDone を送るだけ。hide_window・release は poll(メインスレッドタイマー、
  `incoming_drag.rs:369-389`)で実行。ended は source 単位で照合(`130-159`)。
  回帰テスト `disconnect_keeps_native_resources_for_main_thread_cleanup`(`472-486`)も追加済み。

### H5 転送中の切断でカーソルが最大約20秒固定 — **解決済み(2026-09-30 Cycle 1)**

- 修正: `bulk::Link` に世代付き shutdown ハンドル(`sd` スロット、`Writer::shutdown_handle`
  の try_clone)を追加し、`clear`/`clear_if` は送信中でも slot のロックを待たず即座に切断。
  `on_disconnect`(crates/mac/src/main.rs)は WIN_MODE 復帰と切断通知を `BULK_LINK.clear()` の
  前に移動。回帰テスト `link_clear_cuts_a_blocked_send_without_waiting_for_the_slot_lock`
  (common)で「送信が詰まっていても clear が2秒以内に戻る」ことを検証。

## MEDIUM

| # | 判定 | 証拠と残る問題 |
|---|---|---|
| M1 | **解決済み(2026-09-30 Cycle 2)** | `Carried::claim` が予告件数との照合を返すように変更(`common/src/drag.rs` の `Claim`)。`win/main.rs` の win_on_bulk で不一致はドラッグにせず通知。テスト `a_partial_transfer_is_not_carried_as_a_drag` 追加 |
| M2 | **解決済み(2026-10-01 Cycle 5・実機確認残る)** | `win/src/dragdrop/edge.rs` enter に `DRAG_THREAD != 0` ガードと自前 `DATA_VTBL` 保険ガードを追加。WM_TIMER の帯表示条件にも `DRAG_THREAD == 0` を追加し自己ドラッグ中は帯を出さない。テスト `ignores_the_drag_we_are_carrying_ourselves` |
| M3 | **解決済み** | `mac/src/main.rs:1910-1923` — activate_peer_locked は旧端末へ Leave を送ってから待機へ戻す。Windows 側は Leave 受信で relay_cancel + mods.release_everything(`win/src/main.rs:2808-2816`) |
| M4 | 未解決 | `mac/src/main.rs:556・577・583` — LAST_SYNC_COUNT/LAST_SENT_FILES を更新した後 `send_files_to_win` が FILE_TX_BUSY で黙って return(`1121-1124`)。印が戻らず再送されない |
| M5 | **解決済み(2026-09-30 Cycle 2)** | `win/src/main.rs` win_on_bulk — ID付きで claim が Released/Unknown のときクリップボードを変えず「保存先へ保存」通知のみ。件数不一致はクリップボードへ載せつつ「n/m 件のみの受信」と通知 |
| M6 | **解決済み(2026-09-30 Cycle 3)** | 掴み検出時(ポーリングスレッド)に `precompute_drag_entries` で集計を先に済ませ、越境時はキャッシュを使う(未完なら従来どおり同期実行)。指紋登録・履歴記録は tap から転送スレッド(`send_drag_files_to_win`)へ移動 |
| M7 | **解決済み(2026-09-30 Cycle 3)** | `offer_drag_to_win` の先頭で `BULK_LINK.is_up_fast()`(try_lock で絶対にブロックしない)を確認し、未接続なら拒否して Mac 側のドラッグを続行させる |
| M8 | **設計上の意図(対応しない)** | 本レビュー(9/28)より後の File Drop 設計で「取消は bulk 接続切断で伝播する」(新フレームを増やさず受信側の既存切断クリーンアップを利用)と決定済み。中断フレームを増やすとこの設計と衝突する。再接続は約2秒周期で張り直し+送信側の再試行窓(約8秒)があり、影響は「取消直後の数秒だけファイル経路が使えない」に留まる |

## LOW

| # | 判定 | 証拠 |
|---|---|---|
| L1 | **解決済み(2026-10-01 Cycle 5)** | mac/main.rs — 押下持ち込み判定を純関数 `edge_button_carry` に抽出し、take() 成立(handoff)を ready スナップショットより優先。競合窓でボタンを一切送らない組合せを構造排除。テスト edge_carry_tests 3件 |
| L2 | **解決済み(2026-10-01 Cycle 5)** | win/dragdrop.rs — fallback() を win_on_bulk フォールスルーと同一挙動に統一(クリップボード+LAST_SYNC_SEQ+履歴+Ctrl+V通知)。掴み成功時(DoDragDrop COPY)の履歴へのせも追加 |
| L3 | 未解決(コメント更新のみ) | `common/src/lib.rs:190-198` VERSION コメントは更新済み。ID付き送信自体は相手版を見ず。caps 未導入 |
| L4 | **解決済み(2026-10-01 Cycle 5)** | mac/main.rs — history_push_files を送信完了時(r.is_ok() のみ)へ移動。掴み・⌘C両経路。失敗転送が「送った」と履歴に残らない。テスト file_tx_tests 追加 |
| L5 | **解決済み** | `mac/src/main.rs:1115` — コメントは offer_drag_to_win に修正済み(grep 0件) |
| L6 | 未解決 | `win/src/dragdrop.rs:620-633` — テストがグローバル BTN_W/CARRIED を直列化せず書き換え |
| L7 | **解決済み** | `mac/src/incoming_drag.rs:428-436` — commit 失敗分岐で cancel(id) を呼び INCOMING を片付け |

## 残る項目と次の優先順位(2026-10-01 未明のサイクル後)

1. **実機確認(利用者の対話的操作が必要)** — H2 の順序逆転・H3 の越境後原本不動・M2 の帯非表示・
   L1 の競合窓実タイミング・H5 の操作中切断。ロードマップ §10-3 の再現手順と一体で実施
2. **L3(caps 導入)** — 版交渉の仕組み。v14 同士なら問題なく、将来の版分散で効くため単独で優先度は低い
3. **L6** — win テストのグローバル状態直列化(win テストは Mac 上で走らないため優先度低)
4. **Cycle 5 レビューの Low 残り** — Low-1(送信スレッド panic-safe)・Low-2残り(Released 保存時の
   履歴載せ判断)・Low-3(予告遅延+離し先行の稀ケース)

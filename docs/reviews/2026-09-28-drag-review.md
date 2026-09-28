# ドラッグ＆ドロップ コードレビュー

実施：2026-09-28。対象：掴んだまま境界を越えるファイルドラッグのサブシステム全体（同日の未コミット変更を含む作業ツリー）。
方法：別コンテキストの読み取り専用レビュー。HIGH は親セッションでコードを照合した（照合欄）。
判定：**変更要求**。HIGH 5件・MEDIUM 8件・LOW 7件。

実行した検証：`cargo test --locked -p tsunagu-common -p tsunagu-mac`（95件成功・2件 ignore）、`cargo check -p tsunagu-win --target x86_64-pc-windows-gnu --tests`（成功）。
clippy は未導入のため未実行。Windows 実機試験・E2E はこのレビューでは未実施。

対象ファイル：`crates/common/src/{drag.rs,lib.rs(bulk)}`、`crates/mac/src/{main.rs,file_drag.rs,incoming_drag.rs}`、`crates/win/src/{main.rs,dragdrop.rs,dragdrop/host.rs,dragdrop/edge.rs}`。
行番号は 2026-09-28 時点の作業ツリー。

凡例：「推測」は確信度が低い。「要実機確認」は実機でしか確かめられない。「既知」は [調査](../drag-drop-investigation.md) に記載済み。

## HIGH

### H1 65件以上の掴みは、先頭64件だけが無通知で渡される

- 場所：`crates/mac/src/main.rs:833-840`（`pb_files`）、`main.rs:1015`（`offer_drag_to_win` の件数判定）
- 根拠：`pb_files` は65件以上をログだけ出して64件に切り詰める。このため `paths.len() <= MAX_FILES` は常に真になる。⌘C の同期経路も同じ切り詰めを受ける。
- 状況：Finder で100件を掴んで越えると64件だけが送られ、元のドラッグは合成 Up で終わる。利用者への通知はない。
- 修正案：`pb_files` が本当の件数（またはあふれ）を返し、`offer_drag_to_win` で拒否する（Refused へ）。⌘C 経路は通知して送らない。
- 照合：確認済み（`n.min(64)`）。同日の CHANGELOG・調査資料の「65件以上では越えない」は誤りだったため訂正した。

### H2 予告（本線）より先に転送完了（bulk）が届くと、ドラッグにならない（推測・要実機確認）

- 場所：Mac `main.rs:1048`（本線キューへ積む）・`1064-1067`/`3418-3420`（bulk は別スレッドが直接書く）、Windows `crates/win/src/main.rs:884-887`、`crates/win/src/dragdrop.rs:35-39`、`crates/common/src/drag.rs:95-103`
- 根拠：`Carried::claim` は予告前に呼ばれると偽を返し、後から再照合しない。`start` の500ms待ちが吸収するのは押下の遅れだけ。本線はマウス移動で混み、bulk は独立した接続。
- 状況：数KBのファイルを速く越えると、Windows ではクリップボード受信になり、Mac の元ドラッグは終わっている。
- 修正案：`Carried` に「予告前に届いた転送」を短時間（例 500ms）保持する枠を設け、予告が届いた時点で照合する。保持対象は Mac 由来（最上位ビットが立つ）ID に限る。
- 照合：`claim` の挙動は確認済み。到着順の逆転が実際に起きるかは未確認。

### H3 【既知・問題5】合成 LeftMouseUp が画面端で Finder のドロップになり、原本が移動し得る（要実機確認）

- 場所：`main.rs:3367-3378`（60ms後に `live_cursor()` の位置へ Up を投稿）、`main.rs:2894-2903`（Windows モードのカーソル固定位置は接続辺の2px内側）
- 状況：端のデスクトップアイコン列、端に置いた Dock（ゴミ箱・フォルダ）、端に寄せた Finder ウィンドウの上で確定する。同じボリューム内では「移動」になり、原本が動く。複数件では移動後に `send_drag_files` の `is_file` 判定が失敗し、転送も失敗する。
- 修正案：マジック値付きの合成 Esc（Windows へは転送しない）で Finder のドラッグを取り消す。または Up の直前に押下開始位置へ戻す。どちらも実機で挙動を確かめる。

### H4 切断時に、セッションスレッドから NSWindow の `orderOut:` を呼ぶ

- 場所：`crates/mac/src/incoming_drag.rs:97-110`（`reset`）。呼び出し元は `main.rs:4707-4708`（`on_disconnect`）で、これは `main.rs:4949`・`5039` のセッションスレッドから呼ばれる。
- 根拠：AppKit のウィンドウ操作はメインスレッド専用。加えて `reset` と stale 回収（`incoming_drag.rs:372-387`）は ACTIVE を取り出すだけで FINISHED に入れず、panel と source の retain がリークする。
- 状況：受信ドラッグ中の切断で、例外によるクラッシュまたは表示の不定（要実機確認）。
- 修正案：`reset` は「終了待ち」へ移すだけにし、`hide_window` と release は `poll`（メインスレッドのタイマー）で行う。stale 回収も同じ経路にする。
- 照合：`on_disconnect` の先頭で `reset` を呼ぶことを確認済み。

### H5 転送中の切断で、Mac のカーソルが最大約20秒固定されたまま戻らない（要実機確認）

- 場所：`main.rs:4720`（`BULK_LINK.clear()`）が WIN_MODE の復帰（`4723`）より前にある。`crates/common/src/lib.rs:1380-1384`（`clear` はロックを取る）、`1398-1416`（`send` は転送の全期間ロックを持つ）、書き込みタイムアウト20秒（`1483`・`1532`）。
- 状況：Mac→Windows の掴みは越えた瞬間に転送を始めるため、転送中はほぼ Windows モード。この間に Windows のスリープや回線断が起きると、`on_disconnect` が `clear` で止まり、カーソルが隠れて固定されたままになる。`activate_peer`（`main.rs:1552`）も同様。
- 修正案：`on_disconnect` で WIN_MODE の復帰を先に行う。`Link` はロックの外に shutdown 用の複製ハンドルを持ち、`clear` がロックを待たずに切断できるようにする。
- 照合：処理順と `send` のロック保持を確認済み。

## MEDIUM

| # | 内容 | 場所 | 修正案 |
|---|---|---|---|
| M1 | Mac→Win で受信件数を照合しない。一部欠落・0件でも黙って進む（0件は通知なしで return） | `win/src/main.rs:871-873`、`common/src/drag.rs:77-104` | `announce` で `count` を保持し、`claim` で不一致ならドラッグにせず通知する |
| M2 | 運んでいる OLE ドラッグが Windows の接続辺の帯に入ると、同じファイルを Mac へ送り返す処理が始まる（推測・要実機確認） | `win/src/dragdrop/edge.rs:209-277`・`361-365` | `edge::enter` で `DRAG_THREAD != 0` または自前の `DATA_VTBL` なら受け付けない |
| M3 | 複数台接続で、ドラッグ中に別端末が接続してアクティブが移ると、旧端末に Up も Leave も届かず DoDragDrop が残る（推測） | `mac/src/main.rs:4902-4920`・`1523-1560` | `activate_peer` で旧端末へ Leave を送ってから切り替える |
| M4 | 越境時のクリップボード同期が `FILE_TX_BUSY` で送信をやめるのに、同期済みの印（`LAST_SYNC_COUNT`・`LAST_SENT_FILES`）を更新してしまい、以後も再送されない | `mac/src/main.rs:480`・`491-499`・`1037` | 送れなかった時は印を戻す。またはドラッグ転送の完了後に同期をやり直す |
| M5 | 【既知・問題3】取消・離した後・Mac へ戻った後に届いた ID 付き転送が、Windows のクリップボードを上書きする | `win/src/main.rs:884-899` | ID 付きで `claim` が偽なら、クリップボードを変えず「保存先に保持」とだけ通知する |
| M6 | イベントタップのコールバック内で同期 I/O（最大64件の `fs::metadata`、`history_save` のディスク書き込み）。遅い媒体でタップが無効化され得る | `mac/src/main.rs:1013-1025`・`3345`・`278` | metadata はポーリングスレッドの `complete` で事前計算し Ready に持たせる。履歴保存は別スレッド |
| M7 | 越えることを確定する前に bulk 経路を確かめない。張り直し中だと2.5秒後の再試行でも失敗し、ファイルはどこにも渡らない | `mac/src/main.rs:1037-1093` | `offer` の条件に `BULK_LINK.is_up()` を加え、失敗時は Refused |
| M8 | Win→Mac で取消した送信が bulk 接続ごと切断する（`Interrupted` もエラーとして shutdown）。直後の操作が数秒 NotConnected | `common/src/lib.rs:1409-1414`、`win/src/dragdrop/edge.rs:84-95` | 中断フレームで接続を保つ。または `Interrupted` では切断しない（受信側の片付けと整合させる） |

## LOW

| # | 内容 | 修正案 |
|---|---|---|
| L1 | `file_drag_ready` の読み取り（`main.rs:3170`）と `take`（`3330`）の間に Ready へ変わる競合。MOVED 由来の切替では押下を送らず、Windows は500ms後に fallback | 冒頭で一度だけ `take` し、その結果で `drag_ok` を決める |
| L2 | `dragdrop.rs:545-556` の fallback はクリップボード・履歴に載せず、`main.rs:896` の経路と挙動が不一致。通知文（`dragdrop.rs:600`）が DROPEFFECT_NONE の時にも「掴んだまま越えて」と案内する | 経路ごとの結果表示を段階5で統一する |
| L3 | Mac は相手の版を見ずに DROP_ID_BEGIN を送る。v11 の Windows はクリップボード受信へ退行。`proto::VERSION` のコメント（`lib.rs:110-115`）が未更新 | caps 導入時に条件化し、コメントを更新する |
| L4 | `history_push_files`（`main.rs:3345`）が転送成功前に「送った」と記録する | 転送完了時に記録する |
| L5 | `main.rs:925` のコメントが存在しない `send_drag_to_win` を指す | `offer_drag_to_win` へ直す |
| L6 | `win/src/dragdrop.rs:620-633` のテストがグローバルの `BTN_W`・`CARRIED` を直列化せず書き換える | テスト用のロックで直列化するか、状態を注入できる形にする |
| L7 | `incoming_drag.rs:396-403` で commit が None の分岐は DragCancel を返すだけで `INCOMING` を片付けない。以後の Win→Mac offer が拒否され続け得る（推測・確信度低） | 同分岐で `cancel(id)` を呼ぶ |

確信度の低い候補：`host.rs:118` の `SetForegroundWindow` がフォアグラウンドロックで失敗する条件は未確認（失敗時は fallback）。要実機確認。

## 良い点

- 押下ごとの世代管理（`file_drag.rs`）と MouseDown 時点の基準値取得で、ポーリング位相による取りこぼしを構造的に防いでいる。
- 渡せない掴みを Refused として離すまで保持し、元のドラッグを画面端で終わらせない。
- ワープの後に押下を送る順序へ直し、前回の Windows 位置を押す事故を防いでいる。
- `Receiver::drop` が ID 付きの途中受信を全削除する。
- COM の vtbl の並び、QI の E_NOINTERFACE、参照カウント、STA 上の DoDragDrop は、読んだ範囲で問題なし。

## テストの不足

- `tap_callback` 内の越境順序（予告→Warp→押下→bulk）と Refused への遷移の結合テスト
- 予告より先に完了が届く場合（H2）
- 64件を超える掴みの拒否（H1）
- Mac→Win の件数不一致・0件。`win_on_bulk` 自体の単体テスト（M1）
- 切断時の `on_disconnect` の所要時間と WIN_MODE 復帰（H5）
- `incoming_drag::reset` のスレッド制約とリーク（H4）
- 複数台接続でのアクティブ切替中のドラッグ（M3）
- 運んでいるドラッグが Windows の辺の帯に入る場合（M2）
- 越境時のクリップボード同期と `FILE_TX_BUSY` の衝突（M4）
- Finder⇄Explorer の実操作 E2E、Esc・戻り・直後に離す操作、合成 Up が落ちる位置（すべて未実施）

## 対応状況

修正は [引き継ぎ書](../plans/2026-09-28-drag-handoff.md) の「レビュー指摘の修正」で別エージェントが行う。対応したら各指摘に「対応済み（コミットID）」を追記する。

# 機能別品質監査と修正(2026-10-01)

利用者の指示「機能一つ一つのクオリティと安定性を上げてほしい」に基づく。4領域(接続・入力・転送・音声/更新)を
独立レビュー(読み取り専用)で監査し、42件の指摘から20件を修正した。プロトコルのバイト列は不変。
検証: G1 215 passed / G2 合格 / G3 は既知18件のみ(新規のテスト削除ゼロ・テストは2件追加)。

## 適用した修正(20件)

| # | 重要度 | 場所 | 内容 |
|---|---|---|---|
| 1 | High | win tray/updater | 終了・更新適用の全 exit 経路(4箇所)で `speaker_disconnect()` を呼び、接続中ミュートの恒久化を防止 |
| 2 | High | win conn | accept ループからハンドシェイク+hello+hello_ok を接続ごとのスレッドへ分離し、絶対期限15秒+同時4件の上限を設けた(黙る相手が待ち受けを塞ぐ問題) |
| 3 | High | mac session | 同上(mac 版)。ハンドシェイク+hello 検証をヘルパーへ分離して期限付き待ちに |
| 4 | High | mac tap | `leave_win_mode_cursor_unlock` 冒頭で WIN_MODE 解除+再突入ガードを集約(WIN_MODE 残留による入力凍結と復帰直後の再切替競合を同時解消) |
| 5 | M | mac tap | Android 権限拒否分岐に EDGE ガード1.5秒を追加(境界での通知連打を間引き) |
| 6 | M | mac trackpad | reset_session と callback の !remote_active 経路でピンチ終端(phase 3)を送り、Android 側の合成2指残留を防止 |
| 7 | M | mac conn | keepalive の ping を「ロック下で writer を取り出す→解放→書込み→id+gen 一致時のみ戻し入れ/detach」へ変更(半開き相手への書込みで切替UIと受信ループが止まる問題) |
| 8 | M | mac session | pong 返信も同様にロック外書込みへ |
| 9 | M | win+mac | 本線受信ループに行長上限(8MB)を追加し、超過で切断(hello と同一方式)。巨大1行による無制限メモリ確保を防止 |
| 10 | M | win session | DragCancel/DragDone で受信表示(RX_DRAG)を解除+サーバー側切断後始末にも追加(トレイの「受信中」残留) |
| 11 | M | win audio | 接続時ミュートのガードに AUDIO_ENABLED を追加(音声転送 OFF でもミュートが掛かり音がどこからも出ない問題) |
| 12 | M | win audio | ミックス形式が 16/32bit 以外(24bit 等)なら対応外としてエラー(音声スレッドの範囲外索引 panic を防止) |
| 13 | M | common pairing | SAS ワイヤに載せる名前を UTF-8 境界を壊さず 64 バイトへ切詰め(長い・日本語ホスト名で登録が必ず失敗する問題)+テスト追加 |
| 14 | M | common connect | parse_hosts が IPv6 リテラル(`[::1]:port`・素の `::1`)を正しく扱い、解決失敗はログへ+テスト追加 |
| 15 | M | common history | 履歴ファイルを作成時点から 0600 に(644 で作ってから chmod する間の漏えい窓を除去)。restrict は成否を返し失敗をログ |
| 16 | M | mac files | 掴み転送の指紋(LAST_SENT_FILES)を「成功時のみ」記録(失敗転送が同期済み扱いになり同内容 ⌘C が永久停止する問題: 9/28 M4 再発防止) |
| 17 | M | mac files | 両送信スレッドを TxGuard(Drop)で包み、panic でも FILE_TX_BUSY・進捗表示・中止要求を解く(既知 Low-1 の解消) |
| 18 | M | mac incoming_drag + win dragdrop/edge | 掴みオファーにファイル共有範囲ゲートを追加(共有 OFF 時に 30 秒の無音待ちを即拒否へ) |
| 19 | M | mac audio | ANDROID_MAX_BACKLOG を実質 800ms→80ms へ(コメントと実値の10倍ズレ修正) |
| 20 | L | mac conn/session | DragCancel 由来の中止要求、update KEEP 外削除のログ等は未実施(下記参照)。代わりに検証系で動いた項目のみ反映 |

## 第2弾: 残りの適用(2026-10-01・追加16件)

| # | 場所 | 内容 |
|---|---|---|
| 21 | mac main/session | Win→Mac ロック連動を双方向化: win は WTS_SESSION_LOCK(Win+L)で既存 Msg::Lock を送信(KNIT_LOCK_SYNC=0 で無効)、mac は受信で ⌘Ctrl+Q(System Events)を発生。ロック検知スレッドの送信失敗もログ化 |
| 22 | win input | IME 設定後に IMC_GETOPENSTATUS で照会し、不一致なら再送(最大3回)。反映を確認できない場合はログ(昇格窓・UIPI 対策) |
| 23 | win clipboard | 履歴復元後にエコー防止印(テキスト/ファイル)を置き、復元内容が次の切替で再送されるのを防止(mac と同一挙動に) |
| 24 | win clipboard | HDROP 64件切詰めを通知つきに+掴みドラッグの65件超を通知つき拒否 |
| 25 | win clipboard | 履歴保存を直列化(同時保存で tmp が壊れるのを防止)。mac 側も同様に直列化 |
| 26 | win conn | Tailscale 昇格監視に接続時刻(世代の代用)を持たせ、別セッションの張り直しを阻止 |
| 27 | win updater | 更新ロック作成失敗時に更新を中止(自動復帰タスクとの競合防止) |
| 28 | mac session | DragCancel の中止要求を自発信 id(最上位ビット1)に限定し、相手発信 id の滞留を解消 |
| 29 | common xfer | 中止要求一覧に64件の上限(消費者を失った id の滞留を封じる) |
| 30 | common drag | released_once を追加し、予告より先に来た離しを announce が消さない(既知 Low-3 の解消)+テスト |
| 31 | common discover | 応答ループの黙って終了をログ化(LAN 発見だけが永久失敗する状態の発見可能化) |
| 32 | common envutil | 設定移行の部分コピーで打ち切らない(失敗時は不完全ディレクトリを消して次回再試行可能に) |
| 33 | common proto | 端末 id 保存失敗のログ化(毎回新しい端末として振る舞う状態の発見可能化) |
| 34 | common update | KEEP 未登録ファイルの削除をログ化(新版が新 DLL 同梱した際の永久失敗を発見可能に) |
| 35 | mac audio | ensure_playback を直列化(2スレッド同時通過で AudioQueue が漏えいし二重再生になる TOCTOU を解消) |
| 36 | mac updater | 更新ステージ残留(.knit-update-*)の起動時掃除(win 相当。60分経過を残留とみなす) |

## 監査で確認したが問題なしと判断された領域

更新の署名検証(Ed25519・フェイルクローズ・SHA-256・版/platform 二重確認)・bulk 受信状態機械の境界検査・
世代管理(id+gen 照合・detach・OUTBOUND_GENERATION)・ペアリング(SPAKE2+Noise・3回制限)・
切替時の入力解放(release_everything)・スクロールの飽和・KNIT_SHARE キャップの適用一貫性。

## 未実施(残置・根拠つき)

- **feed_s16 の呼び出し条件変更**(mac audio): 監査案は既存の「レート変更で作り直す」正規経路と衝突し、
  適用すると旧バグ(音程ずれ)を再導入しかねない。TOCTOU 側は直列化(#35)で解消済み。
  44.1kHz デバイスでの実測を条件に再検討
- **音声コールバック内 mutex**(win audio): 実測(drop カウンタ)を要するため対象外のまま
- **L3 caps(版交渉)**: 欠陥修正ではなく機能設計(v14 の互換を保った交渉)。利用者判断の対象
- **L6 win dragdrop テストの直列化**: win テストは Mac 上で実行できないため効果が検証できない
- **KEEP リストの恒久再設計**: #34 のログで事故は発見可能になった。恒久対応はリリース手順
  (KEEP 更新必須の明記)とセットで行う
- 実機確認: 第1弾・第2弾の全変更(接続遮断の15秒・権限剥奪からの復帰・ロック連動の往復・
  IME 照会・共有 OFF の即拒否など)。分割+修正後の実行パスは Cycle 4 手順での確認を推奨

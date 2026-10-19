# 端末モデル PR の敵対的レビュー記録(2026-10-19)

対象: PR #13 ブランチ wip/device-model(増分A〜D+レビュー対応 commit 9622e80 まで)
方法: interrogate(2名・brief-diverse。DSH は1セッション1モデルのため、モデル多様ではなく
レンズ多様: A=認証/暗号/攻撃面、B=並行性/統合/後方互換)。ローカル実測 cargo test 643件緑。

## レビュー者A(認証/暗号/攻撃面)

### findings
- 中: devices.json のプロセス横断 read-modify-write 競合で、失効直後の端末が台帳へ復活しうる(registry.rs:134-151/154-158/310-333 vs 235-247)。note_wrote が他人の rename を自分の stamp として記憶し得る問題も同根。→ flock か楽観排他
- 低: 自己申告 device_id が bulk 宛先トークン解決の鍵になり、登録済み悪意端末が bulk 経路を落とせる(session.rs:469→registry.rs:318→337→main.rs:398)
- 低: 設計書が約束した「鍵位置別タイミング分散の回帰テスト」が未実装(secure.rs:555 は総時間上限のみ)
- 低: discover 応答が UDP パケットごとに全部屋を再計算(discover.rs:276→registry.rs:358)
- 低: CLI 失効は稼働中セッションを切れない+orphan 掃除経路が無い
- 低(移行): Mac のみ旧版へロールバックすると新規登録済み Win が締め出される

### 良い点
- ドメイン分離の完全性(device_token は key_id 固定長32hex で曖昧さが構造的に無い)
- 台帳の非秘密性がテストで固定(ledger_contains_no_secrets・通し確認・0600)
- 自己申告 id と認証主体の型レベル分離(PeerEntry.auth は accept_as のみ由来)
- Compute 増幅は65鍵×X25519/接続に収まり、FailThrottle と直列化で包まれる

## レビュー者B(並行性/統合/後方互換)

### findings
- 高: H1 二重プロセスの台帳書き込みロストアップデート(A の中と同一。独立合致=最高シグナル)
- 中: M1 drop_stream 競合で別端末を切り、失効端末が自動切替で復活(conn.rs:604-636)
- 中: M2 3ロック保持中の Leave ブロッキング書き込み(最大5秒/台。keepalive のコメントが禁じている形)
- 中: M3 keepalive の writer 貸与と競合すると失効端末の接続が切れない(Reader が生きたまま半永続化)
- 中: M4 「すべての登録を初期化」が devices.json を消さない(format 台数表示・64枠の食い潰し)
- 低: L1 discover 再計算 / L2 audio のログ文言不整合 / L3 peer-sides 整理スキップ /
  L4 beacon の鍵不漏えい表明の消失 / L5 Mac が本線クライアントの経路は master のまま /
  L6 mtime+size 再読込判定の理論穴

### 良い点
- accept_as は accept 側に共有可変状態を置かず多鍵を実現
- CLI の revoke が key_id だけで動き、2プロセスの master 解決食い違いに強い
- AC2 ループバックは輸送生存確認・3経路ラベルまで踏んでいる

## リード判定(interrogate Step 5)

**Act on(修正済み・commit 9622e80)**
1. H1/A 中: 台帳書込にロックファイル(devices.json.lock・5秒で自動破棄)+バイト列完全比較の
   楽観排他+再試行。割り込み再現テストと並行ハンマーテストを追加。ついでに persist の
   一時ファイル名を pid+連番へ(同一プロセスの2ハンドル衝突をハンマーが発見)
2. M1/M2/M3: 失効切断を2段階化(cut_sessions_where)。ロック内は収集のみ・Leave はロック外・
   アクティブは切替が割り込めない同一ロック区間でスロットごと外す。PeerEntry.shut
   (writer 貸与中でも切れる)
3. A 低1: 宛先トークン解決を認証済み Who(token_for)へ。links 機構を削除
4. A 低5/B: 外部プロセスでの失効を keepalive が1秒周期で切る(Device と Legacy の両方)
5. M4: 初期化で台帳 clear
6. 小修正: L2/L3/L4/L1・設計書§9 の文言(タイミングは総時間上限で固定・
   ダウングレード・ロックファイルの陳腐化)

**Noted(修正せず記録)**
- L5: Mac が本線クライアントになる経路は master のまま(設計書 §9-8。役割反転は実験的位置づけ)
- orphan(未接続エントリ)の掃除 UI: GUI 増分で載せる(設計書 §9)
- 1秒ポーリングは、別プロセスと本体の間で IPC を持たない現構成での唯一の反映経路(対症であることを記録)

## 検証記録(再現できる形で)

- レビュー対応 commit: 9622e80。ローカル: `cargo test --workspace --exclude knit-win` exit=0(14スイート)
- 失効切断の新コードの回帰テスト: conn::tests::cut_sessions_where_shuts_down_only_the_target
  (対象のセッションだけ切れ、第三者は書けるまで確認)
- CI(PR #13): 増分C-2 までと D は success(run 39181894036 / 39182795145 / 39183980614)。
  レビュー対応 9622e80 は run 39188521193(macos 5分超・windows 1分9秒)。最終 commit の run は PR 参照
- snow 0.10 の IKpsk2 実証(候補2案の根前提。source は同ディレクトリ snowcheck/):
  `parse ok` / `msg1 len = 105`(payload 9B)/ `msg2 len = 57` / `transport 往復 ok` /
  `wrong-static reveals true key ok` / `ALL OK`(exit 0)。
  要点: msg1 は 96B+payload で旧 NNpsk0(48B+payload)と長さが分かれ、msg1 復号直後の
  get_remote_static で台帳照合ができる。第三者の静的鍵は真正に開示され、未知鍵の拒否は
  暗号層ではなく台帳照合の位置で行うのが正しい(設計の前提をここで確定)
- arena: 候補1(candidate-1-inheritance.md)を base に採点 29 対 24、graft 3件。
  採点の内訳は設計書 §3.1.5(audit 時点ではセッション内だったが本文書と設計書に残した)
- 実バイナリ: `./target/debug/knit-mac --registry list` を本体起動中に実行し rc=0
  (二重起動ロックより前に処理され、実キーチェーンから master を解決して台帳を開く)

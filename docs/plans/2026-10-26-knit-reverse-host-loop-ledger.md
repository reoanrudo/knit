# 逆方向(Win=サーバ)ホスト強化ループ — 台帳

- 設計: docs/plans/2026-10-26-knit-reverse-host-loop.md
- 基準コミット: 9f88635 / ブランチ: loop/reverse-host(worktree 隔離・WIP は本体チェックアウトに残置)
- 基準値: cargo test --workspace --exclude knit-win **695 passed / 0 failed**(集計方法: 全 `test result` 行の合計。初期記録の 389 は tail 切り詰めによる過少集計で、周3時に訂正)・cargo build -p knit-mac 警告 **9件**
- 実行: 2026-10-26〜 lead 単独(R0.5 で確定した直接実行方針)

| 周 | 観点 | 指摘 | 対処/見送り | 検証 | コミット |
|---|---|---|---|---|---|
| 1 | 用語・文書の整合 | usage.md:287/291 で「逆方向」の基準点が逆転・README:74 の Server=操作側が構成依存・distribution §6 に定義の錨なし | 4件全対処(向きの矢印表記化・README 補文・§6 定義行) | usage.md に無限定「逆方向」0 件 | 3dad273 |
| 2 | 設定導線 | G1 は実態と不一致: 両側 GUI 切替・相手適用・自動再起動が実装済み(zero-settings_ui.rs:216,300・mac gui.rs:580)。distribution §6 が .env 手動のまま旧い | §6 を GUI 第一に書き換え(.env は固定用と位置づけ) | usage.md:453-460 の記述と整合・文言は実装引用 | 22159c5 |
| 3 | ファイアウォール | F1: Mac がクライアント役で「常駐停止」と誤診(mac/diag.rs:25)。F2: 待受役の probe 必然 false→「相手に届きません」誤誘導+ポート一覧なし(common/diagnose.rs:79,306)。F3: §6 netsh が 24900 のみで §9 と不整合 | F1=win と対称化・F2=host_role 分岐+ポート入り案内+テスト3件・F3=netsh を 24900-24902 TCP+24903 UDP に | workspace 698 passed/0 failed(基準695+3)・警告9(不変) | 7ba6945 |
| 4 | 音声経路 | 逆方向で KNIT_AUDIO_HOST 必須(win main.rs:510)。Mac 音声受信は役割無条件(mac main.rs:1461)・Win サーバは受理時に PEER 記録(conn.rs:424)のため未指定でも追従可能 | main.rs のサーバモード特別扱いを解除し audio::start(None)=受理相手追従に。変数は明示上書き用に残す | workspace 698/0・**CI 必須ゲート: PR#14 macos+windows 全緑**(run 40316324580) | cd234fc |
| 5 | 発見と診断 | Mac diag の audio_server_role が !is_client_role を含み main の実条件(役割無条件)と不整合→逆方向で音声待受行が出ず 24901 bind 失敗が無言。発見の逆方向挙動は文書化されていない | 条件を main と同一化(役割無条件)。§6 に「発見の応答は待受側のみ・接続側が探す」を追記 | workspace 698/0・警告9 | 89be2c4 |
| 6 | 再接続・堅牢性 | **Tailscale 逆方向が手順どおりでも rejected**(既定で 100.x 拒否・KNIT_ALLOW_TS=1 必要)が §6 に書かれていない。§9 拒否文言が旧実装のまま。見送り: PEER 残存は引き直し方式の設計意図・LanWatch テストは網羅済み | §6 に両側 KNIT_ALLOW_TS=1 必須を明記・§9 文言を現行実装へ | docs のみ(コード不変) | bb625e4 |
| 7 | 双方向同時転送の回帰防壁 | keepalive try_lock 回帰の単体テストは既存。**1 接続の双方向同時 e2e が未整備**(逆方向の本質経路) | connect_loop+link.send で 8MiB×2 を 500ms/chunk 引き伸ばし同時送信(接続側 keepalive 10s が送信中ロックに当たる窓を作る)。60 秒完了期限で回帰を壁に | 新テスト 1 件 20.7s で合格・workspace **699/0**(基準695+4) | dd3695e |
| 8 | UI・状態表示 | Win トレイ切断時「未接続・自動再接続中」がホスト役(逆方向)では誤解(再接続しに行かない)。他の役割 UI(両側設定・メニュー)は実装と整合し指摘なし | host_mode_pref で「未接続・相手からの接続を待機中」へ役割分岐 | workspace 699/0(ローカル可能範囲)・win 文言は最終 CI で検証 | 113f2c7 |
| 9 | 総仕上げ・検収 | — | CHANGELOG・usage 反応・実機確認リスト別紙化・保護リスト照合・最終検証 | 下記の検収記録 | (本コミット) |

## 検収記録(周9・lead 実測)

- cargo test --workspace --exclude knit-win: **699 passed / 0 failed**(基準 695 → +4。既存テストの削除・骨抜き 0)
- cargo build -p knit-mac 警告: **9 件(基準と同数)**
- ブランチ CI(ドラフト PR #14): 周4時点で macos+windows 全緑。最終 head は最終 push 徒に再確認
- 保護リスト照合: (1) 既定経路の変更は診断文案・逆方向分岐のみで既定挙動は不変(client 分岐の従来文言は既存テスト client_role_keeps_peer_unreachable_advice で固定) (2) CGNAT 即拒否=net.rs 無変更 (3) 認証失敗鈍化=無変更 (4) ポート・プロトコル=proto/bulk フレーム無変更(テストと文案のみ) (5) .env 優先順=envutil 無変更 (6) Win ホストトグル=host_mode_pref の読み出しのみ追加(設定経路は無変更) (7) role-loop 成果=該当関数は無変更
- 実機確認リスト: docs/plans/2026-10-26-knit-reverse-host-loop-manual-checks.md へ引継ぎ

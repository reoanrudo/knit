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

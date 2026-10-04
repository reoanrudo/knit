# Deskflow 設定UI を参考にした改善ループ(2026-10-04 実行済み)

## 目的

Deskflow(Synergy/Barrier 系譜・Qt GUI)の設定UI を参照軸に、Knit の設定UI のギャップを
10観点で洗い出し、シニアエンジニアが順次実装する交互ループ。
前々回「想定ユーザー10名」(UX 懸念)・前回「探検者10領域」(潜在欠陥)に続く第3弾で、
**競合の完成度を借りて自製品の UI を鍛える**タイプのループ。

## 事前準備(各エージェントに Web 調査させないための仕掛け)

1. 参照先の実物を取得(github.com/deskflow/deskflow の `src/lib/gui/` 配下):
   - MainWindow.ui(Server/Client ラジオ・Configure Server/Client・Start/Restart・コンピューター名+IP)
   - dialogs/SettingsDialog.ui(General/Window/Logs/Network/Advanced の全項目)
   - dialogs/ServerConfigDialog.ui(Computers グリッド/Hotkeys/Advanced)
2. 要点を抽出して共通参照ドキュメント(旧 /tmp/deskflow-ref.md)にまとめ、
   各観点エージェントのプロンプトの先頭で「最初に Read させる」
3. Knit 側の読み替えメモ(どのファイルが相当UIか)も同ドキュメントに併記

## 実行方法(ZCode)

各ペアを直列に実行(後のペアが前の実装後のコードを見る):
1. 観点担当: `Agent(subagent_type: read-only-reviewer)` で参照ドキュメント+Knit 実装を読み
   「Deskflow の要素→Knit の現状(file:行)→ギャップ→提案(最小実装案)」を最大5件
2. シニア: `Agent(subagent_type: builder)` に報告を貼り全件対処
   - 検証: `cargo check/test --workspace --exclude knit-win`・`cargo build -p knit-mac`
     ・`--preview-ui` スモーク・ドキュメント中心のペアでも回帰テスト
   - 方針: Knit の思想(登録は GUI で完結・常時暗号化・自動再接続)を壊す提案は退ける
   - コミット禁止(ユーザー指示待ち)

## 10観点の定義

| # | 観点 | Deskflow 参照要素 |
|---|---|---|
| 1 | 画面配置エディタ | Computers タブのドラッグ配置・ゴミ箱・ダブルクリック設定 |
| 2 | サーバー/クライアント役割 | Server/Client ラジオ・Connect to:・Start/Restart |
| 3 | スクリーン名・端末識別 | This computer's name(編集可)+IP 表示 |
| 4 | ネットワーク設定 | Network IP・Port・TLS グループ |
| 5 | ホットキー・切替 | Hotkeys/Actions リスト・switch delay/double tap 数値 |
| 6 | セキュリティ | TLS 有効化・証明書情報・再生成・クライアント証明書要求 |
| 7 | ログ | Level コンボ・Log to file+パス・GUI デバッグ |
| 8 | 初回ウィザード | セットアップウィザードの段階進行・再実行 |
| 9 | 設定の保存・移行・リセット | Remove all settings・外部設定ファイル・エクスポート |
| 10 | 乗り換え体験 | 用語対応(server/client 等)・README 比較表からの導線 |

## 2026-10-04 実行結果

- 検出50件・対処50件(コード実装38件+ドキュメント整備・「対応しない」明確化12件)
- 主な実装: 画面配置エディタの強化(全画面指定へ戻す・重複警告・クリック選択・現在値表示)、
  Mac 側 KNIT_HOST 入力(~/.config/knit/env への保存・Windows と対称)、KNIT_ROLE 固定の表示、
  自IP表示、端末エイリアス(peer-sides.json 拡張)・IP 併記・履歴ラベルの端末名化、
  自名の編集(hello へ反映)、入力検証の共通化、bind 失敗の通知とポート一覧、診断ボタン、
  KNIT_BIND 公開、音声・発見の死活診断、速度越境の説明・滞在時間スライダ・右⌘候補、
  暗号化の常時表示と鍵フィンガープリント、登録台数表示、更新の署名検証表示、
  メニューバーのログ導線・診断へのログ末尾連結・1世代退避・--diag の GUI 切替、
  期限切れ/ロックからの新コード作成導線・Win 側あとで案内・残り手順予告・完了表示、
  全設定リセット・設定フォルダを開く・設定の書き出し/読み込み・env 上書きの項目注記、
  乗り換え手順と用語対応の文書化
- テスト: ループ開始時359件 → 終了時388件(全緑)
- 副産物: 前ループ実装に混入した自己デッドロック(history_device_for_active)を検出修正

## 運用の注意

- 観点エージェントは read-only(Web 不可)でも参照ドキュメントを渡せば成立する。
  参照先の情報は鮮度が重要なので、実行のたびに事前取得(手順1)をやり直すこと
- 参照UIの「全項目」を一度に移植しようとしない: 各ペアで「Knit の思想に合うか」を
  毎回判定させる(本ループでは Start ボタン・ポート変更・TLS トグル・外部設定ファイル・
  組合せホットキー・クリップボード上限 GUI 化を意図的に退けた)
- UI 実装は preview スモークに加え、可能ならスクリーンショット+OCR での重なり検証が有効
  (本ループで実際に重なりを発見・解消した)

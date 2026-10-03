# 開発者向けガイド

利用者向けの説明は [README](../README.md) と [usage.md](usage.md)。ここはビルド・配備・構成の記録。
AI/自動化で扱う場合は [agent-guide.md](agent-guide.md) を先に読む。

## 構成

- **Mac = サーバ**(`knit-mac`): CGEventTap で入力をフックし TCP で転送。メニューバー常駐 GUI
  (状態・遅延・手動切替・設定ウィンドウ・Windows へのファイル送信・音量制御)付き
- **Windows = クライアント**(`knit-win`): 受信イベントを SendInput で注入。タスクトレイ常駐+
  ステータスウィンドウ、毎分の自動復帰ウォッチ、WASAPI ループバックによる音声送出付き。
  コンソールから起動しても独立プロセスに置き換わり、閉じても切れない
- **経路**: TCP 24900(入力・制御)/ 24901(音声)/ 24902(ファイル・画像)、UDP 24903(LAN 自動発見)、
  TCP/UDP 24904(登録画面を開いている間だけ)。全経路を Noise プロトコルで暗号化し、共有トークンから
  導いた鍵で相互認証する(トークンは回線に流れない)。受け入れるのは LAN・有線直結・Tailscale のみ
- 接続方向は .env で選択可(既定: Mac=サーバ / Win=クライアント)

設計詳細は [design.md](design.md)、初回登録の仕様は [first-connection.md](first-connection.md)。

## ビルドとテスト

```bash
cargo build --release                                              # Mac 側
cargo build --release -p knit-win --target x86_64-pc-windows-gnu   # Windows 側(クロス)
cargo test --workspace --exclude knit-win                          # Mac/共通のテスト
```

push / PR ごとに [.github/workflows/ci.yml](../.github/workflows/ci.yml) が同じテストを実行する。
タグ(`v*`)を push すると release.yml が両 OS の配布 zip を作って公開する。

## 配布物の作成

```bash
./scripts/package-mac.sh   # dist/Knit-<ver>.zip(設定・トークン非同梱)
./scripts/package-win.sh   # dist/Knit-win-<ver>.zip(設定・トークン非同梱)
```

署名・公証・検査は [distribution.md](distribution.md)。

## 開発用セットアップ(既存 env 方式)

```bash
./scripts/gen-token.sh     # 共有トークン生成(~/.config/knit/env に保存)
./scripts/deploy-win.sh    # Windows へ配備(ビルド→停止→配布→起動)
./scripts/restart-mac.sh   # Mac 側をビルドして再起動
./scripts/verify.sh        # 自動検証
```

初回のみ Mac のアクセシビリティ権限が必要。旧 seamless-desk からの乗り換えは上記のままでよく、
旧タスク・旧自動起動は自動で掃除される。旧名 Tsunagu(v0.25 まで)とは互換がなく、再登録が必要。

## 記録

- 品質基準と未達項目: [product-readiness.md](product-readiness.md)
- 改善計画: [plans/2026-09-30-global-commercialization-roadmap.md](plans/2026-09-30-global-commercialization-roadmap.md)
- 機能の取捨判断: [reviews/2026-09-30-feature-audit.md](reviews/2026-09-30-feature-audit.md)
- 第三者レビュー: [reviews/](reviews/) / 改善履歴: [improvement-log.md](improvement-log.md)
- 全文書の索引: [README.md](README.md)

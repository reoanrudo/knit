# セキュリティポリシー / Security Policy

## 対応バージョン / Supported versions

開発の先行段階のため、最新のタグ（[Releases](https://github.com/reoanrudo/tsunagu/releases)）
のみを対象とします。

## 脆弱性の報告 / Reporting a vulnerability

**公開の issue には報告しないでください。** 次のいずれかへお願いします:

- **GitHub Security Advisories（推奨）**: このリポジトリの
  [Security タブ → Report a vulnerability](https://github.com/reoanrudo/tsunagu/security/advisories/new)
  から非公開で報告できます
- またはメンテナへ直接ご連絡ください（GitHub 上のメンションでも構いません）

報告には、再現手順・影響範囲・（あれば）修正案を添えていただけると助かります。
受け取った報告は 72 時間以内に確認し、影響評価と対応方針をお返しします。
修正の公開まで当該情報を非公開に保つようご協力をお願いします。

## セキュリティ設計の概要 / Design overview

侵入経路の評価に役立つ要点（詳細は [docs/design.md](docs/design.md)）:

- 全通信経路（入力 24900・音声 24901・ファイル 24902・登録 24904）を
  Noise プロトコル（`Noise_NNpsk0_25519_ChaChaPoly_BLAKE2s`）で暗号化。
  共有トークンは回線に流れず、PSK の導出にのみ使用（前方秘匿あり）
- 接続元は LAN・有線直結・Tailscale 範囲に限定（`TSUNAGU_ALLOW_ANY` で全域許可に変更可）
- 初回登録は SPAKE2 + 6桁コード（5 分期限・3 回試行制限・単発使用）
- 受信ファイルは無害化・隔離属性付与・上限付き。URL は http/https のみ、
  アプリ起動は列挙済み .lnk との完全一致のみ
- ログ・通知にクリップボード本文・トークンを出力しない

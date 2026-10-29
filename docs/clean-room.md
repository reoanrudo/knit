# 他プロジェクト由来のアイデアの扱い(クリーンルーム方針)

Knit は MIT ライセンスです。GPL など互換性のないライセンスの OSS(例: TwinDesk、Beamer。
いずれも GPL-3.0)からは **コードを一切持ち込みません**。持ち込めるのは「何を解決するか」という
アイデアと、OS ベンダーの公開ドキュメントに書かれた事実だけです。

## ルール

1. **コードを見た人は実装しない。** 互換性のないライセンスのソースを読んだ人(エージェントを含む)は
   仕様書だけを書き、実装・レビューでのコード提示は、そのソースを読んでいない人が行う。
2. **仕様書の根拠は一次資料に限る。** Microsoft Learn・Apple Developer Documentation などの公開
   ドキュメント、Knit 自身のコードと計測。参考 OSS のファイル名・関数名・定数の選び方・コメントの
   言い回しを仕様書に書かない。
3. **コピー・逐語移植・行単位の翻訳をしない。** 他言語のコードを Rust へ「訳す」のも移植にあたる。
4. **記録を残す。** 仕様書は `docs/cleanroom/` に置き、冒頭に「根拠とした資料」「書いた人」
   「実装した人」を書く。実装コミットのメッセージに仕様書のパスを入れる。
5. **MIT / Apache-2.0 などの互換ライセンスの部品**は、著作権表示とライセンス文を
   `THIRD_PARTY_NOTICES` に入れれば使ってよい(例: DeskPad の `CGVirtualDisplayPrivate.h` は MIT)。

## 一覧

| 仕様書 | アイデアの出どころ(問題の発見元) | 実装者 |
|---|---|---|
| [Windows の電力スロットリング解除](cleanroom/2026-10-29-win-power-throttling.md) | TwinDesk issue #16(公開の不具合報告) | TwinDesk のソースを読んでいないエージェント |
| [Mac のスリープ/一時的な目覚めの扱い](cleanroom/2026-10-29-mac-sleep-gate.md) | Power Nap 中の再接続の空回り(一般に知られた macOS の挙動) | Codex(gpt-6.1-terra)。TwinDesk のソースを読んでいない |
| [クリップボードの論理時計](cleanroom/2026-10-29-clip-ordering.md) | 「最後のコピーが勝つ」(HLC の論文) | Codex(gpt-6.1-terra) |
| [登録失敗の段階別案内](cleanroom/2026-10-29-pairing-stages.md) | 登録失敗時の案内の改善 | Codex(gpt-6.1-terra) |
| [耐性テスト](cleanroom/2026-10-29-robustness-checks.md) | 公開 issue の症状(無通信切断・高速移動の誤切替) | Codex(gpt-6.1-terra) |
| [拡張ディスプレイ 第1段](cleanroom/2026-10-29-extended-display-stage1.md) | 拡張ディスプレイ機能。DeskPad のヘッダ(MIT)を同梱 | Codex(gpt-6.1-terra) |

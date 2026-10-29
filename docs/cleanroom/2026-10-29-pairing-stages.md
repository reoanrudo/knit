# 仕様: 登録(ペアリング)の失敗を段階ごとに見分けて案内する

- 書いた人: lead エージェント(2026-10-29)。参考 OSS のソースを読んでいるため実装には関わらない
- 実装する人: 参考 OSS のソースを読んでいないエージェント(Codex / gpt-6.1-terra)
- 根拠とした資料: Knit 自身のコードのみ
  (`crates/common/src/pairing.rs` の `request_approval` / `enroll_impl` / `Pending::confirm`、
  `crates/win/src/tray/setup.rs` の `save_error_message`)

## 解決したい問題

Windows の登録画面は、失敗を `io::ErrorKind` で4種類に丸めて表示している(`save_error_message`)。
`ConnectionRefused` / `TimedOut` / `WouldBlock` がまとめて「相手の端末に接続できません」になるため、
次の別々の状況が同じ文言になり、利用者が何を直せばよいか分からない。

1. その IP に何も応答しない(相手の Knit が起動していない・IP 違い・ファイアウォール)
2. 応答はあるが、相手が登録画面を開いていない(招待が無い)
3. 接続できたが、相手の持ち主が「許可」を押さないまま時間切れ
4. 6桁コードの方式で、コードが違う・期限切れ

## やること

### A. 段階を表す型(`crates/common/src/pairing.rs`)

失敗がどの段階で起きたかを表す列挙型を追加する。例:

- `Connect` … TCP 接続そのものが失敗(相手が応答しない)
- `Invitation` … 接続はできたが、最初の招待IDを受け取れない/形式が違う(相手が登録画面を開いていない)
- `Exchange` … 鍵交換・PAKE の途中で失敗(コード違い・期限切れ・改ざん)
- `Approval` … 承認待ちで時間切れ、または拒否された
- `Save` … 認証後の保存に失敗

`request_approval` / `enroll_impl` / `Pending::confirm` が返す `io::Error` に、この段階を載せる
(`io::Error::new(kind, PairError { stage, source })` のように独自エラーを包み、`fn stage_of(&io::Error) -> Option<Stage>`
で取り出せるようにする)。既存の `ErrorKind` は変えない(呼び出し側の既存判定を壊さない)。

### B. 文言(純関数・単体テスト)

`fn pairing_hint(stage: Option<Stage>, kind: ErrorKind) -> &'static str` を共通側に置き、段階ごとの
日本語の案内を返す。目安:

- Connect:「相手の端末から応答がありません。相手で Knit が起動しているか、IP アドレス、同じネットワークにいるかを確認してください。」
- Invitation:「相手の端末で登録画面が開かれていません。相手の設定「接続」から「端末を登録…」を開いてください。」
- Exchange:「コードが違うか、期限が切れています。相手の画面に出ている新しいコードでやり直してください。」
- Approval(時間切れ):「相手の端末で「許可」が押されませんでした。相手の画面を確認して、もう一度やり直してください。」
- Approval(拒否 = PermissionDenied):「相手の端末で拒否されました。」
- Save:既存の保存失敗の文言
- 段階不明:既存の汎用文言

Windows の `save_error_message` はこの関数を使うように置き換える。Mac 側に同等の表示箇所があれば同じ関数を使う
(無ければ Windows だけでよい。どうしたか報告すること)。

### C. 段階ごとの時間制限の見直し

いまは接続 3 秒・ワイヤ 20 秒・承認 120 秒。承認待ち(利用者が押すまで)には制限が要らないという考え方もあるが、
相手が席を外している場合に画面が待ち続けないよう、**現状の値は変えない**。変えるのは表示だけ。

## やらないこと

- 登録の暗号プロトコル・ワイヤ形式の変更
- 画面のレイアウト変更(文言の差し替えのみ)

## 受け入れ条件

- `./scripts/quality-gate.sh --with-win` が PASS
- `stage_of` が各段階のエラーで正しい値を返す単体テスト(ローカルの TCP で、相手が招待を送らない・応答しない等を作れる範囲で)
- `pairing_hint` の全分岐の単体テスト
- CHANGELOG の[未リリース]に1行

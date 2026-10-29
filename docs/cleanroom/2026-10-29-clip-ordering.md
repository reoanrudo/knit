# 仕様: クリップボードは「最後にコピーした方が勝つ」を論理時計で保証する

- 書いた人: lead エージェント(2026-10-29)。参考 OSS のソースを読んでいるため実装には関わらない
- 実装する人: 参考 OSS のソースを読んでいないエージェント(Codex / gpt-6.1-terra)
- 根拠とした資料:
  - Kulkarni, Demirbas ほか「Logical Physical Clocks and Consistent Snapshots in Globally Distributed
    Databases」(2014、Hybrid Logical Clock の原論文。公開の学術論文)
  - Lamport「Time, Clocks, and the Ordering of Events in a Distributed System」(1978)
  - Knit 自身のコード(`crates/common/src/proto.rs` の `Msg::Clip`、`crates/mac/src/clip.rs`、
    `crates/win/src/clipboard.rs`、`crates/win/src/session.rs`、`crates/mac/src/session.rs`)

## 解決したい問題

いまの `Msg::Clip { text }` には「いつのコピーか」が無い。受信側は届いた順に上書きする。
次の場合に、新しいコピーが古いコピーで上書きされうる。

- Mac でコピー → 直後に Windows でコピー。両方の送信が回線上ですれ違うと、Mac は Windows の
  新しい方を受けて上書きし、Windows は Mac の古い方を受けて上書きする。**両者の内容が食い違ったまま**残る
- 再接続直後に、切断前の古いコピーが遅れて届く
- 3台以上(Mac に複数の Windows・Android)で、別経路から届いた古いコピー

## やること

### A. 共通の論理時計(`crates/common` に新モジュール。例: `clipclock.rs`)

Hybrid Logical Clock(HLC)を実装する。タイムスタンプは `(l: u64 ミリ秒, c: u32 カウンタ, node: String 端末ID)`。

- `now(pt)`:ローカルのコピー時に呼ぶ。`l' = max(l, pt)`、`l' == l` なら `c += 1`、違えば `c = 0`
- `observe(remote, pt)`:受信時に呼ぶ。論文の受信規則どおり `l' = max(l, remote.l, pt)` と `c` を更新
- 比較は `(l, c, node)` の辞書順(node は同時刻のときの決着用。端末IDの文字列比較)
- `pt` は壁時計の unix ms を渡す(HLC は壁時計の巻き戻りに強い設計なので、ここでは壁時計でよい)
- 純粋な構造体として実装し、単体テストで次を確認:単調増加、受信後のローカル時刻は受信した時刻より大きい、
  壁時計が巻き戻っても減らない、比較の全順序、同時刻の node による決着

### B. メッセージの拡張(互換を保つ)

- `Msg::Clip` に省略可能なフィールドを追加する:`ts: Option<ClipStamp>`(`#[serde(default, skip_serializing_if = "Option::is_none")]`)。
  旧版は未知フィールドとして無視でき、旧版から来た `ts` 無しのメッセージは「受信時刻のコピー」として扱う
  (= 受信側で `now(pt)` を打って受け入れる。従来どおりの挙動)
- プロトコルの版(`VERSION`)は上げない(未知フィールド無視で互換が取れるため)。この判断をコメントに書く

### C. 受信と送信の規則(Mac・Windows とも)

- 各端末は「自分が知っている最新のクリップボード時刻」を1つ持つ(自分のコピーも相手から受けたものも含む)
- **送信**:ローカルでコピーを検知したら `now(pt)` で時刻を打ち、最新時刻を更新してから送る
- **受信**:`observe` で時計を進めたうえで、受け取った時刻が最新時刻 **より新しいときだけ** 適用する。
  古ければ適用せず、ログに1行(`[clip] 古いコピーのため適用しません`)
- エコー防止の既存の仕組み(最後に受信したテキストとの比較)は残す
- 対象はテキストの `Msg::Clip`。画像・ファイル(bulk 経路)は今回は対象外(やらないこと参照)
- Android 経路(`crates/mac/src/android.rs`)も `Msg::Clip` を送受信するので、Mac 側の受信規則は
  共通の関数を通して同じにする

## やらないこと

- 画像・ファイルのクリップボード(bulk 経路)への時刻付与。フレーム形式の変更が必要なため別仕様にする
- Android アプリ(Kotlin)側の変更。Android から来る `ts` 無しのメッセージは B の互換規則で扱う
- 時刻の永続化(再起動で時計はリセットしてよい。壁時計が基準なので実害は小さい)

## 受け入れ条件

- `./scripts/quality-gate.sh --with-win` が PASS
- HLC の単体テスト(A の項目)と、受信規則の単体テスト:「新しい方は適用」「古い方は捨てる」
  「ts 無しは適用」「すれ違い(双方が同時にコピー)で両者が同じ内容に収束する」をシミュレーションで確認
- `Msg::Clip` の旧形式(ts 無し JSON)を新しい側が読めること、新形式を serde で往復できることのテスト
- CHANGELOG の[未リリース]に1行

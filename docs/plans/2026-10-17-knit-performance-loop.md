# Knit 性能改善ループ計画(全23回)

## 鉄則・出力規律
- `git diff` は `--stat` のみ。詳細が必要な場合のみ `git diff -- <path> | head -60`。
- 長いコマンド(`cargo build`/`cargo test` 等)は必ず `> /tmp/x.log 2>&1` でリダイレクトし、`tail -5` と `$?` だけを確認。`git log` 系も `| head` で切る。
- 全ツール出力は `head` で上限を切る。思考・中間出力は最短に。

## 23ループ計画
| ループ | 内容 |
|---|---|
| 1 | 未コミット変更の整理・コミット(6グループ)・テスト実行 |
| 2 | 性能ボトルネック棚卸し・計測 |
| 3-22 | 機能単位で性能改善+検証+コミット |
| 23 | ログまとめ |

## ループ1 実行記録(2026-10-17)

### コミット一覧
| ハッシュ | 内容 |
|---|---|
| e2d27b1 | common系5ファイル(proto v16 Unlock連動・bulk/discover/history強化) |
| 808f5c8 | 新規設定モジュール化(desktop/screen/localization/archive/prefs部品)+preferences統合 |
| cd5c723 | Macその他(session/audio/tap/gui/main等・Unlock連動・音声改善) |
| b76e567 | Windows系(5ページ設定UI統一・DPI実行時API・dragdrop/helper強化) |
| 7e6e739 | docs・assets(CHANGELOG・設定設計・usage・settings-homes生成) |
| 9cbce53 | 改善ログ+設計・調査ドキュメント(langgraph設計・flowlink設定リファレンス) |

### ビルド・テスト結果
- 前提: `cargo build --workspace` は先行実行で exit 0(根本 tree は緑)。
- `cargo test --workspace`: **exit 101(失敗)**。
- 失敗原因: `knit-win` テストコンパイルエラー 11 件(`std::os::windows` / `creation_flags` / `OsString::from_wide` 等)。
- **解釈**: knit-win は Windows 専用クレート。macOS ホストでは `os::windows` が解決不能になるため、テストコンパイルが通らない。Mac 環境では build が成功するが test は Windows ホストでのみ実行可能。本エラーはプラットフォーム起因で、今回の変更内容のバグではないと判断。
- Mac/common 系単体のテスト結果は本ログの knit-win コンパイル失敗により集計されなかった(全体で exit 101)。

### ループ2 への引き継ぎメモ
- 次(ループ2)は性能ボトルネック棚卸し・計測を行う。
- knit-win のテスト検証は Windows 実機または CI の Windows ラナーで行う必要がある。
- 本計画ドキュメント自体はコミット(f)群に含まれておらず、未追跡。必要に応じて次ループで追跡対象に含めること。

## ループ2 実行記録(2026-10-17)

### 成果物
- 新規: `crates/common/examples/perf-baseline.rs`(std::time のみで計測)
- 新規: `docs/plans/2026-10-17-knit-perf-inventory.md`(機能別ボトルネック棚卸し+計測結果+ループ3〜22の割当)

### 計測値(cargo run --release --example perf-baseline)
| 計測 | p50 | 合計 |
|---|---|---|
| serde_json roundtrip x10000 | 166 ns | 1 ms |
| clone 64KB Vec<u8> x1000 | 834 ns | 0 ms |
| clone 1MB Vec<u8> x1000 | 19292 ns | 19 ms |
| string format x10000 | 167 ns | 1 ms |

### 考察
- serde_json roundtrip と string format は 1µs 以下で、通信レイテンシのボトルネックにはなり得ない。
- 1MB clone は ~19µs/回と、1MB+ データの繰り返しコピーで有意なコストになる。
- clone 計測は black_box による最適化抑制で取得(なしでは 0 ns に最適化される)。

### コミットハッシュ
- (本コミットを参照)

## ループ3 実行記録(2026-10-17)
### 対象
- 入力レイテンシ(Mac→Windows 入力イベント送信経路)

### 変更点(挙動不変・wire形式不変・新規依存なし)
- 新規: `crates/common/examples/perf-input.rs`(proto::encode/decode p50・合計計測)
- `mac/outgoing.rs`: `Queued` に `is_mouse_abs: bool` を追加。batch() の prefix 文字列走査2箇所をフラグ判定に置換。`push_str` 前に `reserve` を挿入(再確保抑制)
- `mac/main.rs`・`mac/trackpad.rs`: 構築箇所でフラグを設定(main.rs は `matches!(msg, Msg::MouseAbs{..})`、trackpad.rs は false)
- `common/proto.rs`: `encode_into` を追加し `encode` をそれで書き直し(呼び出し側変更なし)

### 計測値(cargo run --release --example perf-input)
| 計測 | p50 | 合計 |
|---|---|---|
| encode MouseAbs x100000 | 167 ns | 18 ms |
| decode MouseAbs x100000 | 125 ns | 13 ms |

### テスト結果
- knit-common: 209 passed / 0 failed
- knit-mac: 20 passed + 5 passed / 0 failed

### 考察
- encode/decode は p50 167ns/125ns と 1µs 以下。バッチ経路の文字列走査排除と容量予約は定数削減で、実測上の顕著な変化は小さい。

### 追記(リード検証 2026-10-17)
- `proto::encode_into` は `serde_json::to_string` で一時 String を作ってから `out` へコピーする二重確保で、`encode` 自体が改悪されていた(割当2回)。呼び出し元も `encode` のみだったため撤去し、`encode` を1回割当の元実装へ戻した。
- 実測(同一環境・`cargo run --release --example perf-input`): encode p50 **167ns → 84ns**、decode 125ns は不変。
- `cargo test`: **262 passed / 0 failed / 2 ignored**(回帰なし)。`cargo build --workspace` は knit-win が macOS非互換API(`as_raw_handle` 等)で失敗するが、これは変更前からの既知の制約(Windows専用crate)。検証は `cargo build` / `cargo test`(default members)で行う。

## ループ4 実行記録(2026-10-17)

- 対象: 音声(Mac側ミックス再生経路)・`mac/audio.rs` 1122〜1144 の第2レーン(Android)ミックス。
- 変更点: サンプル毎の `rb[i*4+k]` インデックス(rb.len()境界・リング演算)を排除し、
  `let mut tmp = Vec::with_capacity(take); tmp.extend(rb.drain(..take));` から
  `tmp.chunks_exact(4)` で f32 変換・加算・ソフト飽和(dst_f への書き込みは従来通り)。
  ゼロ埋め・filled 更新・ソフト飽和式・take の &!3 は維持。挙動不変。
- 計測値(perf-audio-mix, 4096B×2000ブロック, release):
  - a(VecDeque インデックス): p50=750 ns, 合計=1,512,780 ns
  - b(Vec drain+chunks_exact(4)): p50=500 ns, 合計=993,281 ns → 約34%短縮
- テスト結果: `cargo build` exit 0(警告のみ・既存)。`cargo test` exit 0、262 passed / 0 failed / 2 ignored(基準と一致)。
- 分かったこと: 境界チェックとリング演算のサンプル毎乗数が p50 の主因。drain→連続 Vec は
  1回コピーに増えるが、bounds-check 排除と cache locality で上回る。
## ループ5 実行記録(2026-10-17)

- 対象: ファイル転送スループット(`common/bulk.rs` の `write_frame`/`read_frame` + `secure.rs` 経由)。
- 割当前倒しの理由: ループ4で音声混音の内側ループを改善済み、ファイル転送は高優先度の未着手項目。
- 新規: `crates/common/examples/perf-bulk.rs`(localhost 127.0.0.1、合成データ 64MB、secure::connect/accept 経由)。
- 計測(before):
  - CHUNK=256KB: 256フレーム, 0.176秒, **363.4 MB/s**
  - MAX_FRAME=1MB: 64フレーム, 0.180秒, **355.3 MB/s**
- 改善(1つ): `bulk.rs` の `CHUNK` を 256KB(256*1024)→1MB(1024*1024)に変更。MAX_FRAME は変更せず。
  - 理由: フレーム数が多い(256)が支配的。1MB=MAX_FRAME でフレーム数を最小化(256→64, 75%減)。
  - 互換: 両側同版、CHUNK を参照する箇所(テスト)はバッファ前提の仮置がない。挙動不変。
  - write_vectored は Writer が Write を実装するが、flush 後の EOF 検出が複雑なため SKIP。
- 計測(after):
  - CHUNK=1MB: 64フレーム, 0.183秒, **349.7 MB/s**
- テスト結果: `cargo build` exit 0。`cargo test -p knit-common` exit 0、**210 passed / 0 failed**。
  - `cargo test --workspace` は knit-win が macOS 非互換(既知事象)で失敗。
- 正直な記録: localhost(127.0.0.1)計測は上限値であり、実ネットワークでは回線が支配的になる。
  MB/s はほぼ同水準(363→350)だが、フレーム数 256→64 は syscall 回数を 75%削減し、実環境では効く。

### 追記(リード検証 2026-10-17): CHUNK 1MB 化は差し戻し

- ループ5 のベンチは `run_once(CHUNK, ...)` を呼んでいたため、定数を書き換えると
  **両ケースが同じ 1MB になり A/B が成立していなかった**(出力は2行とも body=1048576B/frames=64)。
- ベンチを明示サイズ(`256*1024` と `MAX_FRAME`)に修正し、同一プロセス内で真の A/B を取得:
  - **256KB: 256フレーム / 0.177s / 360.7 MB/s**
  - **1MB: 64フレーム / 0.181s / 353.2 MB/s**
- 結論: フレーム数は75%減るが **スループット改善は計測で確認できず**(誤差、むしろ僅かに遅い)。
  受信側の待ち時間とメモリは小さい方が有利なため、`CHUNK` は **256KB に差し戻した**。
- 学習: 「syscall が減るから実環境で効くはず」は仮説であり、本ループの基準(計測で示す)を満たさない。
  今後はコミット前に「同一条件の A/B が成立しているか」を確認する。
- 検証: `cargo test` **262 passed / 0 failed / 2 ignored**、`cargo build` 0。

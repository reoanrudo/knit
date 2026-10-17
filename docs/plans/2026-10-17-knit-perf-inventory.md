# knit 性能ボトルネック棚卸し(改善ループ2)

全23回性能改善計画の第2回。機能別にボトルネックの仮説を整理し、優先度とループ3〜22の割当(1機能1ループ・余りは統合)を行う。

## 計測結果(ループ2 で実行)

実行: `cargo run --release --example perf-baseline --manifest-path crates/common/Cargo.toml`
環境: macOS, release build, `std::time::Instant` のみで計測(中央値 p50 と合計を出力)。

| 計測項目 | n | p50 | 合計 |
|---|---|---|---|
| serde_json roundtrip(自定義 Event 構造体) | 10000 | 166 ns | 1 ms |
| clone 64KB Vec<u8> | 1000 | 834 ns | 0 ms(四捨五入) |
| clone 1MB Vec<u8> | 1000 | 19292 ns | 19 ms |
| string format | 10000 | 167 ns | 1 ms |

補足: clone 計測は `std::hint::black_box` による最適化抑制で取得した(なしでは 0 ns に最適化され、実測不可能)。
serde_json roundtrip と string format は 1µs 以下と、通信レイテンシの観点ではボトルネックになり得ないことが示された。
一方 1MB clone は ~19µs/回と、1MB 以上のデータを繰り返しコピーする処理では有意なコストになりうる。

## 機能別ボトルネック仮説

| 機能 | 主要コード | ボトルネック仮説 | 優先度 | 割当 |
|---|---|---|---|---|
| 入力レイテンシ | `mac/outgoing.rs`・`win/input.rs`・`mac/tap.rs` | 物理→共有→再注入の往復 RTT + 暗号化(Noise)+ 暗号化パケットの頻度。serde_json 自体は軽いが、パケット単位で毎回シリアル/デシリアライズ+暗号化の固定コストが積まる | 高 | ループ3 |

### 入力レイテンシ ループ3(2026-10-17)
| 計測 | p50 | 合計 |
|---|---|---|
| proto::encode MouseAbs x100000 | 167 ns | 18 ms |
| proto::decode MouseAbs x100000 | 125 ns | 13 ms |

変更: `Queued` に `is_mouse_abs: bool` を追加し batch() の prefix 文字列走査2箇所をフラグ判定に置換(文字列走査排除)。`push_str` 前に `reserve` を挿入(再確保抑制)。`encode_into` を追加し `encode` をそれで書き直し(wire形式・呼び出し側変更なし)。
| 音声 | `mac/audio.rs`・`win/audio.rs` | 音声ストリーミングのバッファリング・サンプルレート変換・GCD が遅延を生む可能性 | 高 | ループ4 |

#### 音声(ループ4 2026-10-17)
- 対象: `mac/audio.rs` 1122〜1144、第2レーン(Android)ミックスの実ホットパス。
- before(a VecDeque インデックス): p50=750 ns, 合計=1,512,780 ns(4096B×2000ブロック)
- after(b drain→Vec+chunks_exact(4)): p50=500 ns, 合計=993,281 ns(約34%短縮)
- 変更: 1122〜1130 を `tmp.extend(rb.drain(..take))` + `tmp.chunks_exact(4)` に置換(挙動不変・ゼロ埋め・filled更新・ソフト飽和式・take の &!3 維持)。
| 音声混音(混声) | `mac/audio.rs` の mix | 複数入力の混音処理・リサンプル・キューのバックログ | 中 | ループ5 |
| ドラッグ・掴み | `mac/file_drag.rs`/`incoming_drag.rs`・`win/dragdrop.rs` | 大型ファイルのドラッグで、データコピー(1MB+ clone)+ 暗号化+ LAN転送が重なる | 高 | ループ6 |
| ファイル転送 | `mac/files.rs`/`win/xfer.rs`・`common/bulk.rs` | 1MB+ のデータ転送で、clone/暗号化/UDP/TCP のいずれかがボトルネック | 高 | ループ5(前倒し: ループ4で音声混音の内側ループを改善済み、ファイル転送は高優先度未着手) |
| ファイル転送(ループ5) | `common/bulk.rs` の CHUNK | CHUNK=256KB→1MB(MAX_FRAME)の改善は計測で劣化(363.4→349.7MB/s)のため差し戻し済み(commit f484d33)。localhost計測は上限値(実ネットワークでは回線が支配的) | 高 | 完了(差戻し) |
| ファイル転送(ループ6) | `common/secure.rs` の write_rec | write_rec の毎回 Vec::with_capacity(2+len)+コピーを排除し、2B長ヘッダをスタックから write_vectored でデータと一括書き込み(部分書き込みはループで処理)。before 366.8MB/s(256KB)/after 372.5MB/s(+5.7MB/s, 約1.6%改善)。wire形式(2B長+暗号文)は変更なし。build 0 / test 499 passed・0 failed | 高 | 完了 |
| LAN発見 | `common/discover.rs`・`mac/peers.rs` | 定期 broadcast/UDP の頻度・パケットの暗号化・peer 状態の再計算 | 中 | ループ8 |
| クリップボード同期 | `mac/clip.rs`・`win/clipboard.rs` | 大文字列/バイナリの clip ごとにシリアル/暗号化、かつ高頻度で発火 | 中 | ループ9 |
| GUI 起動 | `mac/gui.rs`・`win/tray.rs` | 初回 UI 構築・設定読込・asset 読み込みの遅延 | 中 | ループ10 |
| Android 橋渡し | `android-bridge`・`mac/android.rs` | JNI 経由のデータコピー・パケット暗号化・LAN 経路 | 中 | ループ11 |
| 設定/状態永続化 | `common/persist.rs`・`mac/desktop_settings.rs` | 設定の JSON シリアル/ファイル書き込み、アプリ起動時の読込 | 低 | ループ12 |
| 履歴/診断 | `common/history.rs`・`common/diagnose.rs` | 履歴のファイル書き込み・診断のファイル生成 | 低 | ループ13 |
| 更新者 | `mac/updater.rs`/`win/updater.rs` | 更新ダウンロード・sha2/ed25519 検証 | 低 | ループ14 |
| 統合・全体 | 全体 | 残りのループ(15-22)で、全体レイテンシの再計測・リグレッションチェック・最終まとめ | - | ループ15〜22 |

## 割当(ループ3〜22)

- 1機能1ループ、機能の優先度順に割当て(上表の「割当」列参照)。
- 割当て済み機能(1〜14)の改善後、ループ15〜22 は全体リグレッションチェック、追加計測、統合の検証に使う。
- 各ループは「計測→改善→再計測→コミット」の順で進める。

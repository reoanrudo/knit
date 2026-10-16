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

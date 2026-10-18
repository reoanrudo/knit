# Knit ドラッグ&ドロップ改善 — 成果物の索引(2026-10-18)

このディレクトリに散らばった D&D 改善の文書を、**読む順**に並べた索引です。
「今どうなっているか」だけ知りたい場合は **最終報告書** と **実機確認ランナップ** の2つで足ります。

## 1. まず読むもの

| 文書 | 行数 | 何が書いてあるか |
|---|---|---|
| [最終報告書](./2026-10-18-knit-dnd-final-report.md) | 386 | **これだけ読めば全体像が分かる**。調査→設計→実装→検証→未完了の理由→確認手順 |
| [実機確認ランナップ](./2026-10-18-knit-dnd-verification-runbook.md) | 185 | **あなたが20〜30分で確かめる手順**。各手順に「違ったときに集めるログ行」つき |

## 2. 調査(3役が独立に実施)

| 文書 | 行数 | 担当・内容 |
|---|---|---|
| [競合UX調査](./2026-10-18-knit-dnd-ux-research.md) | 359 | 他社10系統(ShareMouse/MWB/Flow/Universal Control/KDE Connect/LocalSend ほか)の D&D 体験。称賛点9・不満点15(すべて出典URLつき) |
| [現状の摩擦](./2026-10-18-knit-dnd-friction.md) | 290 | ユーザー視点の時系列棚卸し(掴む→離す→転送→完了→失敗)。すべて `file:line` で裏取り |
| [体感遅延の実測](./2026-10-18-knit-dnd-latency.md) | 224 | 掴み検出 8〜22ms / 集計 512件で 1.14ms / **転送 1GB=34秒・10GB=5分41秒**(Wi-Fi 実効 30MB/s) |

## 3. 設計

| 文書 | 行数 | 内容 |
|---|---|---|
| [喜ばれる体験の設計](./2026-10-18-knit-dnd-design.md) | 453 | 目標体験の4条件、増分 I1〜I9 と Windows側 W1〜W5、**実装しないと決めたこと(理由つき)**、§11 に実装状況の対応表 |

## 4. 検証の記録(役割別チームの要)

| 文書 | 行数 | 内容 |
|---|---|---|
| [敵対的検証の記録](./2026-10-18-knit-dnd-verify.md) | 1961 | 検証役の全ラウンドの判定・根拠・**変異テストの表**・見つけた欠陥。文言ゴールデンリスト(抽出器つき) |
| [CIで見つけた4件](./2026-10-18-knit-dnd-ci-findings.md) | — | push を有効にして初めて出た製品バグ(掴みドラッグが「一度クリックすると以後ずっと保存のみ」等)と、その修正 |

## 5. この取り組みで入った主な変更(コード)

| 領域 | 内容 |
|---|---|
| `crates/common/src/drag.rs` + `drag/forecast.rs` + `drag/lazy.rs` | 掴み判定の理由を値に(I1)/「今離すとどうなるか」の予報(I2)/ 遅延提供の状態機械(I8) |
| `crates/common/src/notice.rs` | 結果を3状態で言い切る通知(I5)。**保存のみに成功語を入れない**規則をテストで強制 |
| `crates/common/src/hud.rs` | カーソル近傍の案内の位置決め(画面外へはみ出さない) |
| `crates/mac/src/hud.rs` | クリック透過の HUD(I3)。断りの理由と、Win→Mac の受け取り進捗 |
| `crates/mac/src/files.rs` | 目録の送信(版17)/ 受理の追跡 / ドロップ結果の反映 / 進捗の記録 |
| `crates/win/src/dragdrop/lazy.rs` + `lazy_com.rs` | 遅延提供の中核(FILEGROUPDESCRIPTORW の組み立て、`IDataObject`/`IStream`/`IEnumFORMATETC` の自前 vtbl) |
| `crates/win/src/dragdrop.rs` | `start_lazy`(目録だけで `DoDragDrop` を開始) |
| `crates/win/src/session.rs` / `main.rs` | `Msg::Manifest` の処理、受理の返送、結果の返送、受信先の一本化 |

## 6. まだ終わっていないこと(実機が要る)

1. **遅延提供の実機E2E**(`KNIT_LAZY=1`): Explorer が遅延提供を受けて実際にコピーできるか・
   部分ファイルが残らないか(ランナップ §5)
2. **HUDの目視**: 2行で読めるか・クリックを奪わないか(ランナップ §2・§2.5)
3. **W3**: Windows 側の HUD(ユーザーが Windows 画面を見ている向き)。**既定を変えるため実機待ち**
4. **W2本体**: 200ms 閾値とツールチップの進捗(見た目の調整)
5. **I9**: Win→Mac の File Promise。**PoC 先行**(ランナップ §5.6 に判断の目安)

## 7. 再現のしかた(コマンド)

```bash
# 検証(これが通ることが最低条件)
cargo build
cargo test                 # 611 passed / 0 failed が現在の基準

# 遅延提供を試す(Windows 側)
cargo build --release -p knit-win
set KNIT_LAZY=1            # 既定は無効。外せば従来方式に戻る
target\release\knit-win.exe
```

変更を入れるときは **WIP ブランチ + ドラフトPR** で CI(macos / windows)を通してから main へ
(Windows 固有コードは macOS では `cfg` で除外され、ローカルでは型検査すらできません)。

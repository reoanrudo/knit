# 設定画面デザイン(2026-10-28)

このディレクトリの PNG を実装の「正」とする。画像はデザイン確認用の生成画像で、
文言は英語。実装では `knit-common` の設定仕様(JA/EN)から文言を入れる。

## 画像

| ページ | デザイン | 実装(スクリーンショット) |
|---|---|---|
| 接続 | connection.png | impl/mac-1-connection.png |
| 画面配置 | displays.png | impl/mac-2-displays.png |
| 共有 | sharing.png | impl/mac-3-sharing.png |
| その他 | general.png | impl/mac-4-general.png |
| ダークモード | — | impl/mac-dark.png |

## 方針

- 内容(ページ構成・順番・操作)は両 OS で同一
- 見た目は各 OS 標準: Mac = Liquid Glass(サイドバーのみ NSVisualEffectView)、
  Windows = Mica + Fluent
- 細部(壁紙・アイコン・サムネイル)は実装時に実アセット/実データへ置き換える

## 実装の現状

- 実装: `crates/mac/src/gui/settings_new.rs`、`--preview-ui --settings-new` で切替
- 仕様(実測値・トークン・原則)は [SPEC.md](SPEC.md)

- [x] 4 ページの見た目(接続 / 画面配置 / 共有 / その他)
- [x] サイドバー(.sidebar 素材)・カード・行・ボタン・スイッチ・スライダーの部品化
- [x] ライト/ダーク追従(意味色のみ使用。CGColor 化の前に外観を解決)
- [x] ページ内スクロール(内容の高さは行の高さから計算し、カードと中身がずれない)
- [x] ナビ切替(4 ページ、クリックで切替・選択ピルを accent で表示)
- [x] フッター(区切り線と「元に戻す / 適用」の見た目)
- [ ] 操作の配線(各トグル・スライダー・接続/診断/再接続・フォルダ・言語)
- [ ] 新旧切替(旧 UI を外す)
- [ ] Windows 4 ページ(Mica/Fluent)

## 未達の扱い

フッターの「元に戻す / 適用」と各コントロールは、いまは見た目のみで値の保存は
行わない(プレビュー専用)。配線は次の段階で `prefs` / `preferences` へ委譲する。

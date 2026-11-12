# 設定UI WIP(未コミット)改善案チェッカー

- 日付: 2026-10-24
- 対象: 未コミット差分(`gui.rs` +6 / `gui/prefs.rs` +7 / `gui/prefs/refined.rs` +291/−110 変更 / 新規 `gui/settings_new.rs` 580行 / `docs/design/` / `spike/`)
- 方法: 差分全文・新規ファイル全文の精読、`cargo check -p knit-mac`(合格・警告10件)、AppKit SDK ヘッダによる定数検証
- 結論: **Must 1件(定数誤り)・Should 4件・Nice 4件**。コンパイルは緑。デザイン(docs/design/settings-2026-10-19)の方向と実装は整合している

## 良い点(変えないこと)

- 1040×700・カード812幅など新旧UIで寸法が一致(refined.rs:447-448 と settings_new.rs:14-16)
- sidebar の material 7 は SDK 定義どおり Sidebar(検証済み)
- `--settings-new` ゲートは sync 経路3箇所(gui.rs:1079 / prefs.rs:1380 / prefs.rs:1679)へ正しく置けている
- プレビュー運用(impl2-*.png スクショ + README のチェックリスト)が証拠として残る形なのは優れている

---

## 1. [Must] NSVisualEffectMaterial 15 は underWindowBackground でなく **FullScreenUI**

- **現状**: 両UIの全面ガラスで `glass(cv, …, 15)`。コメントは「15=underWindowBackground」(refined.rs / settings_new.rs の `glass` 定義直上)
- **証拠**: SDK ヘッダ `AppKit/NSVisualEffectView.h` — `UnderWindowBackground = 21`、`FullScreenUI = 15`、`Sidebar = 7`(sidebar のみコメントどおり)
- **改善案**: 15 → 21 へ。あわせてマジックナンバーを `const MATERIAL_UNDER_WINDOW: i64 = 21;` `const MATERIAL_SIDEBAR: i64 = 7;` のように名前付きにする(2ファイルで重複しないよう共通化は案2と併せ)
- **リスク**: 見た目が変わる(15 でも透過はしているが、ブラー/強調の挙動が異なる material)
- **検証**: `--settings-new --preview-dark` と通常起動の旧UIでライト/ダーク両方をスクショ比較し docs/design の impl2-*.png を差し替え

## 2. [Should] ガラス/カード部品の二重実装(refined.rs と settings_new.rs)

- **現状**: `is_dark/glass/card/dot/page_title/label` が両ファイルにほぼ同一実装で存在(計約200行の重複)。design README は「新旧切替(旧UIを外す)」未完了なので当面両方が生きる
- **改善案**: 共通キット(`gui/ui.rs` 等)へ抽出し、両UIから参照。settings_new を正とし、refined は削減方向で持つ
- **リスク**: refined 側の呼び出し置換ミス。**検証**: cargo check + スクショ比較(現行 impl2 系と見た目不変)

## 3. [Should] WIP 起因のデッドコードと発火しないハンドラ

- **現状/証拠**: `cargo check` 警告10件のうち2件が今回追加 — `refined.rs:16 static LAST_THEME is never used`、`refined.rs:220 function color is never used`(refresh_surface が空関数化したため)。さらに「手動接続」ボタン削除後も `MANUAL_OPEN` トグルと `sdRefinedManual`(refined.rs:2075-2076)が残り、発火経路が無い
- **改善案**: LAST_THEME / color を削除し警告を旧水準へ戻す。`sdRefinedManual` と MANUAL_OPEN も削除(MANUAL は常に見えるカードになったので、残すなら名前を ROLE_CARD 等へ)
- **リスク**: 旧UIの「手動接続」を復活させる道が消える(= 本WIPの方向確定を意味する。意図確認済みなら実施)
- **検証**: cargo check の警告が 8 件(旧来分のみ)になること

## 4. [Should] 新UIの文言がハードコード日本語

- **現状/証拠**: settings_new.rs の全ラベルが直書き("このMacと接続中の端末。" 等)。design README の方針は「実装では knit-common の設定仕様(JA/EN)から文言を入れる」。旧UIには sdLanguage: による言語切替が存在
- **改善案**: 操作の配線フェーズの前に knit-common の文言ソースへ差し替え(英語側が無い項目は common へ追加)
- **リスク**: なし(設計方針どおり)。**検証**: 言語切替で新UIが EN 表示になること

## 5. [Should] resizable(8) を残した絶対座標レイアウト

- **現状/証拠**: 両 build() の styleMask `1|2|4|8|32768`。全要素が絶対座標(例: page_title y=626)で、リサイズすると破綻する
- **改善案**: `setMinSize`/`setMaxSize` を W×H に固定(最小変更)。将来可変にする場合は autolayout 導入時に改めて
- **リスク**: なし。**検証**: 実機でリサイズ不可になること

## 6. [Nice] ページ構成の不一致: 新Mac 4ページ vs Windows 5ページ

- **証拠**: settings_new.rs の nav は4項目(接続/画面配置/共有/その他)。win settings_ui.rs は PAGE_COUNT=5(切替を含む)。design README は「内容は両OSで同一」
- **改善案**: Windows 側の切替ページをその他へ統合する計画を README の未達項目に紐づけて明記
- **リスク**: win の既存導線の変更

## 7. [Nice] page_arg に UI_PREVIEW ガードが無い(mac 側のみ)

- **証拠**: win settings_ui.rs:686-696 は `UI_PREVIEW` のときだけ `--preview-page` を解釈するが、refined.rs:639 と settings_new.rs:25 は常時解釈する
- **改善案**: win 実装に揃えて UI_PREVIEW ガードを追加(`args()` の Vec 割当も build 内1回にまとまる)
- **リスク**: なし

## 8. [Nice] `let _ = current;`(settings_new.rs ビルド末尾)は不要行

- 削除のみ。ついでに `build_displays` の `let _ = (mac, win);` は、配線フェーズで参照が必要になるため statics への store に変えておくとよい

## 9. [Nice] 検証導線: `--settings-new` が CLI 直打ちのみ

- **改善案**: scripts/dev.sh に `settings-new [page] [dark]` のようなサブコマンドを足し、`--settings-new --preview-page N --preview-dark` を一発で渡せるようにする
- **リスク**: なし

---

## 次段階(README の未達)への補助線

操作の配線は README 未達項目どおり。配線時に本チェッカーの 1(定数)と 4(文言)を先に済ませると、スクショ証跡の取り直しが1回で済む。

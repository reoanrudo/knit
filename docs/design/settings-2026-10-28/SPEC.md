# 設定画面 デザイン仕様(2026-10-28 正)

正: `docs/design/settings-2026-10-28/*.png`(生成画像。細部ではなく体系を写す)
実装: `crates/mac/src/gui/settings_new.rs`

## 実測値(ウィンドウ pt)

デザイン画像は 1040x700 のウィンドウを約 1.43 px/pt で描いたもの。
以下の値は画像から機械的に測った実測値で、コードの定数と 1:1 に対応する。

| 項目 | 値 | コード |
|---|---|---|
| ウィンドウ | 1040 x 700 | `W` / `H` |
| サイドバー幅 | 219 | `SIDEBAR_W` |
| カード左端 / 右端 | 244 / 1015 | `CARD_X` / `CARD_W` |
| カード角丸 | 12 | `R_CARD` |
| カード内パディング | 15 | `CARD_PAD` |
| ページ見出しの上端 | 36 | `PAGE_TOP` |
| ページ見出し | 30pt bold | `F_TITLE` |
| ページ副題 | 13pt secondary | `F_SUBTITLE` |
| カード見出し | 17pt bold | `F_CARD_TITLE` |
| カード副題 | 12pt secondary | `F_CARD_SUB` |
| 行ラベル | 13pt | `F_ROW` |
| 行の補足 | 11pt secondary | `F_NOTE` |
| ボタン高 | 28 | `PILL_H` |
| ナビ行送り | 50 | `NAV_PITCH` |
| ナビ項目高 | 38 | `NAV_ITEM_H` |
| フッター高 | 60 | `FOOTER_H` |

## 適用している原則

1. **意味色のみ**: `controlBackgroundColor` / `separatorColor` / `labelColor` /
   `secondaryLabelColor` / `controlAccentColor` / `windowBackgroundColor`。
   白黒を手計算した tint は使わない(ダークで破綻するため)。
2. **外観の解決**: 意味色は `CGColor` へ変換した時点の外観で固定される。
   そのため `NSAppearance.currentAppearance` を一時的に切り替えてから
   `CGColor` 化する(`cg_color`)。これを怠るとダークで背景だけ明るく残る。
3. **素材**: サイドバーのみ `NSVisualEffectView`(material `.sidebar` = 7、
   blending `behindWindow`)。全面ガラスは使わない。
   参考: SDK 定義では 15 = FullScreenUI、21 = UnderWindowBackground、
   7 = Sidebar。「15 = underWindowBackground」というコメントは誤りだった。
4. **座標系**: 内容面は `isFlipped=YES` の `SDCanvasView`。
   部品は「上端からの距離」で置く。ウィンドウ直下(下端基準)に置く
   フッターだけは反転して計算する。
5. **カードの高さは計算する**: `card_height(&[行の高さ…])` が唯一の算出元。
   固定値を書かないので、中身と枠がずれない。
6. **余白は 4pt グリッド**。

## 検証

- ライト/ダーク両方をスクリーンショットで確認(`impl/` 配下)
- 4 ページのナビ切替を実際にクリックして、表示ページが一致することを確認
- 内容がウィンドウより高いページはスクロールで全項目に到達できることを確認
- 単体テスト: トークンが仕様表と一致すること、カード高が行数に比例すること

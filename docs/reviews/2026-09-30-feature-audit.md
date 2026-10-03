# 機能取捨監査 — 本質を疑き、不要な機能を判定する

作成: 2026-09-30。[監査指示](../plans/2026-09-30-feature-audit-prompt.md)に基づく判定案。
物差しは [本質を疑く整理](../research/2026-09-28-essence-review.md) の5つの問い
(境界が原因か/毎日起きるか/代わりがあるか/信頼を削らないか/境界を意識させないか)と、
ユーザー目線3場面(最初の5分/毎日の机/無くなった時)。判定は案。最終決定は利用者。

## 監査前に確定した事実(コード・CHANGELOG 確認)

- 検索窓(⌥⌘S)・⌥Enter Throw・ファイル索引・内蔵コマンド 5 種は **CHANGELOG「削除(デスク横断検索)」
  で撤去済み**。監査対象から除外し、代わりに「同じ基準で切る前例」として扱う
- `Msg::RunApp` / アプリ一覧(`AppsReply`)は現存するが、送信元は App Handoff の照合のみ
  (crates/mac/src/main.rs:448-478・crates/win/src/main.rs:1001-1024)。検索窓の残骸ではない
- App Handoff は CHANGELOG で「維持」との利用者判断済み。既定 OFF
  (crates/mac/src/main.rs:1907 `static APP_HANDOFF = false`)
- smartguard は既定 ON(`KNIT_SMART_SECRET != 0`、crates/common/src/smartguard.rs:26)。
  ollaya が動いていない環境では実質無効(usage.md の記載どおり)
- 音声ミックスは「全端末(複数 Windows + タブレット)の音を Mac に集めて混ぜる」機能
  ([指示文](../plans/2026-09-28-audio-mix-prompt.md))。crates/mac/src/audio.rs に
  lane 方式(feed_s16 / clear_lane_b)で部分的に実装済み
- 設定ウィンドウは5ページ(接続/画面配置/操作/共有/ジェスチャー。crates/mac/src/gui/prefs.rs:655-660)

## 判定表

### 維持(コア)— 境界を消す本体。品質投資の対象

| 機能 | 理由(落ちない問い・場面) | 根拠 |
|---|---|---|
| 画面端切替(edge+hotkey+fast edge) | 製品そのもの。「最初の5分」も案内1行で通る | vision §23・§24、usage.md 基本操作 |
| 修飾キー変換・ショートカット翻訳 | 毎日数十回の認知コストを消す。MVP2 | vision §5-§6、usage.md |
| かな/英数キー・IME Follow Cursor | 越えるたびの IME ずれ(§2 の摩擦)。MVP3・日本市場の柱 | vision §7、usage.md |
| クリップボード同期(テキスト/画像/ファイル) | MVP4。代わり(クラウド経由等)より速く確実 | vision §8、usage.md |
| 掴みドラッグ境界越え | MVP5。「どっちの PC だっけ」を消す | vision §9、usage.md |
| クリップボード履歴 | Mac に OS 標準の履歴が無い(Win+V は Windows 専用)。越境コピーの行方を見せる | vision §10、usage.md。※保存範囲・削除は roadmap U2 の品質課題 |
| 音声集約 + 自動ミュート | essence-review で「残す」判定済み(境界が原因: 機器ごとのスピーカー) | essence-review、usage.md |
| 音声ミックス(全端末を同時) | 上記の延長で利用者が理想として要望。開発中 | audio-mix-prompt、audio.rs |
| 画面ロック連動 | essence-review で「残す」判定済み | essence-review、KNIT_LOCK_SYNC |
| 複数台同時接続・切替・モニター構成交換 | 製品定義(3 台以上)の土台。PeerEntry 世代管理は Q5 の基盤 | product-definition、roadmap §2 |
| ターミナル Control(KNIT_CTRL_APPS) | 対象利用者(開発者)に毎日。既定値で働く | usage.md、vision §28 |
| ゲームモード | FPS 等では必須(視点回転)。自動検出のみで静か | usage.md、KNIT_GAME_MODE |
| 音量・メディアキー転送(F7〜F12) | 音声集約と一体の体験。イヤホン1本で済む | usage.md |
| Continue Here(⌥⌘T) | 「Mac で見ていたページを Windows でもう一度探す」摩擦を 1 キーで消す。MVP6。※発見可能性は課題(下記論点1) | vision §11、usage.md |
| RunApp/アプリ一覧 | App Handoff 専用の仕組み(上記の確定事実) | main.rs:448-478 |
| 有線直結の案内・DERP 中継の通知 | 品質の可視性(遅延の理由を利用者が知れる)。機能追加ではなく品質情報 | usage.md、roadmap §3 |

### 実験的のまま(機能は残す・既定 OFF または環境依存、通常案内から外す)

| 機能 | 理由 | 場所 |
|---|---|---|
| App Handoff(KNIT_APP_HANDOFF) | 「勝手にアプリが開く驚き」があり精度課題。CHANGELOG で維持判断済み・既定 OFF | main.rs:1907 |
| smartguard(ollaya 機密検査) | ollaya 依存(環境が無いと動かない)・誤検知で止まる体験の劣化リスク。usage.md でも実験的表記 | smartguard.rs |
| F1〜F6 のけ直し | Windows の画面輝度は変えられず価値が限定(usage.md 自身が実験的表記)。問い②③で弱いが小規模のため維持コストも小 | usage.md |

### 隠す(動作は残すが、設定 UI・通常ドキュメントに出すのをやめる)

| 項目 | 理由 | 依存・規模 |
|---|---|---|
| マウス倍率(sdMouseScale) | abs 方式は「Mac の速度感をそのまま再現」が目的で、倍率 1.0 以外を使う場面は特殊(usage.md)。最初の5分で触るものではない | prefs.rs の該当行のみ。規模 S。設定ファイルの既存値は尊重 |
| スクロール互換(sdScrollCompat) | 一部の古いアプリ向けの回避策。既定 0。トラブル時の救済として docs のトラブルシュート節へ移動すれば足りる | 同上。規模 S |
| env 変数のうち調整系(KNIT_EDGE_PX・CORNER_PX・DOUBLE_TAP_MS・EDGE_TAPS・MOUSE_MODE・BIND 等) | 利用者が触るべきでない内部値。essence-review「env 49 個」の指摘どおり | ドキュメント整理のみ。規模 S(下記方針) |

### 削除候補

**該当なし。** 大規模な不要機能(横断検索・Throw・ファイル索引・内蔵コマンド)は 2026-09-28 の撤去で
すでに除去されており、今回の物差しで新たに「削除」に落ちる機能はなかった。
残る判断はすべて「実験的のまま/隠す」の段階にとどまる。強いて挙げれば F1〜F6 のけ直しが
最も削除に近い(問い②③が弱い)が、実装が小さいため維持コストを下回ると判断した。

## 設定項目の整理方針(env・設定ウィンドウ)

docs/usage.md の環境変数テーブル(41 行)を3段階に分ける。

1. **利用者が触るもの**: 設定ウィンドウに出している項目のみ(Windows の位置・切替方式/条件・
   スクロールの速度と方向・⌘キー割当・音声・クリップボード・ファイル送信・ジェスチャー)。
   usage.md の通常節に残す
2. **トラブル時の救済**: KNIT_SCROLL_COMPAT・KNIT_SMART_SECRET・KNIT_CONTINUE_HERE・
   KNIT_ALLOW_ANY 等。「うまく動かない時」節へ移動
3. **開発・検証専用**: KNIT_BIND・KNIT_MOUSE_MODE・KNIT_EDGE_PX・KNIT_CORNER_PX・
   KNIT_CTRL_APPS・KNIT_GAME_MODE 等。docs/agent-guide.md へ移動

設定ウィンドウ自体は5ページ構成で判断良好。「隠す」2 項目(マウス倍率・スクロール互換)を
除けば、画面に出している項目は利用者向けとして妥当。

## 監査の限界

- 静的確認(コード・ドキュメント)のみ。実機での利用頻度・実測は含まない
- 「毎日の机」の場面判定は利用者像(vision §28)に基づく推定。roadmap B1 の聞き取りが本来の根拠
- タブレット標準モードは開発中のため、将来機能(狙いは確定)として扱い、個別機能の物差し適用は
  実装が固まってから行うべき

## 私が決めるべき論点(5 つ)

1. **Continue Here の存続**: 維持案。ただし essence-review は保留(「クリップボード同期で
   代用できる、手間の差を聞き取りで確かめる」)。⌥⌘T を覚えてもらえるか(発見可能性)も未解決
2. **F1〜F6 のけ直し**: 実験的のまま案。使っている実感が無ければ削除候補(最有力)
3. **smartguard の扱い**: 実験的継続案。品質が上がるまで usage.md の通常節から「実験的」節へ
   移動するかどうか
4. **adb 精密モードの将来**: 標準モード完成後も開発者向けに残すか、役割を終えた時点で削すか。
   Android まわりは開発中のため今は維持
5. **履歴の画像 60 件保持**: 利便(履歴から画像復元)とプライバシー(機密スクショの滞留)の両方を
   持つ。U2(保存範囲・削除・共有停止の明確化)の中で方針を決めるべきで、機能の存続とは別論点

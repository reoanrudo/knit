# tsunagu 操作ガイド

Mac のキーボード/トラックパッドで Windows デスクトップを操作するツール。
Mac=サーバ(TCP 24900 で待ち受け)、Windows=クライアント(接続し続ける逆転構成)。

## 基本操作

| 操作 | 動作 |
|------|------|
| Mac のカーソルを画面右端へ | Windows へ切替(同寸法の高さで境界を越える) |
| Windows のカーソルを画面左端へ | Mac へ戻る(同じ高さで右端内側に復帰) |
| F13 キー | 手動トグル(切替が効かないときの保険) |

切替まわりの細部の挙動:

- 右端判定は Mac 画面右端から `TSUNAGU_EDGE_PX`(既定 2px)の内側。
  Windows 側の復帰判定は左端(x ≤ 1)で、実際にカーソルが動いたときだけ判定する
- 切替時にドラッグしていた場合は Windows 側へ左ボタンを離すイベントを送り、
  誤ドラッグを持ち込まない
- Mac への復帰直後 250ms は右端判定を無効化し、切替の往復チャタリングを防止
- Windows 側の左端復帰は 0.7 秒のクールダウン付き(連打による往復を防止)

## キーボード

- Mac の修飾キーは自動変換: Cmd→Ctrl、Option→Alt、Control→Win、Shift→Shift
  (Win 側では「Mac と同じ修飾の組合せ」になるよう差分で押し替え、復帰時に全解放)
- **かなキー**(Mac keycode 104)→ Windows 側の IME を ON(ひらがな入力)
- **英数キー**(Mac keycode 102)→ Windows 側の IME を OFF(英字入力)
  - フォアグラウンドウィンドウの IME コンテキスト(`ImmGetContext`)が取れる場合は
    `ImmSetOpenStatus` で方向指定どおりに開閉
  - IME コンテキストが取れないウィンドウでは半角/全角相当のキー注入
    (VK_KANJI 押し離し)へフォールバック。キー注入はトグル動作のため、
    この場合のみ開閉の方向が保証されない
  - 成否は Mac 側ログ(`[ime] kc=104 (かな) 転送` 等)と Windows 側ログ
    (`[ime] ImmSetOpenStatus(true) ok` 等)で切り分けられる
- 変換・確定(Enter/Space)はそのまま転送され Windows の IME が処理する

## マウス

- **Windows 側はサブピクセル累積方式**: 受け取った移動量(dx, dy)を f64 で累積し、
  整数部だけ SendInput で注入、端数は次のイベントへ持ち越す。1px 未満の
  トラックパッドの細かい動きも消えず滑らかに動く
- **Mac 側は delta 積算 + 間欠同期で切替判定**: タップコールバック内で移動 delta を
  積算して自前のカーソル位置を追跡し、32 イベントに 1 回だけ実カーソル位置へ同期
  する(毎イベントの位置取得は負荷が高くカクつくため)。切替の瞬間だけ実位置を
  取って正確な高さを Windows 側へ引き継ぐ
- 移動倍率は `TSUNAGU_MOUSE_SCALE` で調整(既定 1.0)

## 有線直結(遅延・揺らぎの低減)

WiFi の瞬間的な揺らぎがカーソルのカクつきの原因になる場合、Mac と Windows を
有線で結ぶと改善します。**通常の USB ケーブルでの Mac⇄PC 直結はできません**
(USB はホスト↔デバイス接続のため、PC 同士は双方ホストになる)。
実用的な選択肢:

| 方法 | 内容 | 備考 |
|---|---|---|
| **USB-LAN アダプタ×2 + LAN ケーブル** | 両側に変換アダプタを付け直結(またはルータへ有線接続) | 最も安価・確実。推奨 |
| **Thunderbolt ブリッジ** | Thunderbolt 対応 PC と Thunderbolt ケーブルで直結 | 高速。Windows 側の Thunderbolt Networking 対応が必要 |
| USB リンクケーブル | ブリッジチップ入りの転送用ケーブル | Mac 対応品がほぼ無い |

いずれも IP リンクが張れれば Tsunagu はそのまま動きます(TCP のみのため)。
直結 IP への切り替えは Windows 側 `.env` に `TSUNAGU_HOST=<Macの直結IP>` を
設定(例: リンクローカル 169.254.x.x、または手動 IP)。Mac の IP は
`ifconfig` で確認(Thunderbolt ブリッジは bridge0、USB-LAN は en* )。
Tailscale 経続併用も可能(戻す場合は .env の行を削除)。

## Mac 流ショートカットの自動翻訳(Windows 画面操作中)

Mac の指癖がそのまま Windows で通るように、以下を翻訳します:

| Mac の操作 | Windows での動作 |
|---|---|
| ⌘← / ⌘→ | Home / End(行頭・行末) |
| ⌘↑ / ⌘↓ | Ctrl+Home / Ctrl+End(文書の先頭・末尾) |
| ⌥← / ⌥→ | Ctrl+← / Ctrl+→(単語単位の移動) |
| ⌘M / ⌘H | Win+↓(ウィンドウの最小化) |
| ⌘Q | Alt+F4(ウィンドウを閉じる) |
| ⌘] / ⌘[ | ブラウザの次/前のタブ(Ctrl(+Shift)+Tab) |
| ⌘⇧4 / ⌘⇧3 | スクリーンショット(Win+Shift+S の切取り) |
| ⌘⇧←→ / ⌘⇧↑↓ | 行選択 / 文書選択(Shift+Home/End 等) |
| ⌥⇧←→ | 単語単位の選択 |
| ⌘⌥Esc | タスクマネージャ(Ctrl+Shift+Esc) |
| ⌘Ctrl+Q | 画面ロック(Win+L) |
| fn+F11 | デスクトップ表示(Win+D) |
| Ctrl+クリック | 右クリック(`TSUNAGU_CTRL_CLICK=1` のみ有効。既定は 2本指クリックで右クリック) |
| ⌘Space | Win+Space(IME/言語の切替) |
| ⌘C/⌘V/⌘A 等 | Ctrl+C/V/A(⌘→Ctrl 自動変換、従来どおり) |

トラックパッドジェスチャ: 横スワイプ=戻る/進む、Ctrl+2本指スクロール=
ズーム(Chrome/Edge 等)、3本指ドラッグ=ドラッグ(アクセシビリティ設定で有効時)。
ピンチ等の追加ジェスチャは macOS が一般アプリに公開していないため現状非対応。

## スクロール

- Mac のスクロールのピクセル delta を除数(既定 60・設定ウィンドウのスライダーで可変)で変換して
  ノッチ単位へ変換し、**0.25 ノッチ刻み**で Windows へ送信する(端数は持ち越し)
- Windows 側はノッチ × 120 ホイールユニットで注入
- 除数を大きくすると遅くなる(40〜200 程度で調整)

## クリップボード(双方向同期)

- 両側で 0.25 秒間隔にポーリングし、コピーして約 0.25 秒で相手側に反映
  (プレーンテキスト、1MB まで)
- Win→Mac は CRLF を LF へ正規化して書き込む。Windows 側の書き込みが
  他プロセスのクリップボード占有で失敗した場合は 150ms 後に 1 回だけ再試行
- 相手から受信して書き込んだ内容は送り返さない(ループ防止)
- 接続が切れている間にコピーした内容も、再接続後に自動送信される
- 画像や書式は未対応(今後の課題)

## 接続の挙動

- ping を 3 秒間隔で送り、10 秒間 pong が無ければ実質切断扱い(TCP が生きていても
  相手プロセスが固まった場合を拾う)
- Windows 側は切断後 0.5 秒から最大 3 秒のバックオフで自動再接続
- Windows モード中に切断したら即 Mac モードへ復帰(入力の閉じ込め防止)

## 調整用環境変数(tsunagu-mac 起動時)

| 変数 | 既定 | 説明 |
|------|------|------|
| `TSUNAGU_SCROLL_DIV` | 60 | スクロール速度の除数(初期値)。大きくすると遅い。実行中は設定ウィンドウのスライダーで可変 |
| `TSUNAGU_SCROLL_FLIP` | (未設定) | スクロール方向。未設定=macOS の設定(自然スクロール ON/OFF)に自動追従。`1` で Windows 標準へ固定 |
| `TSUNAGU_MOUSE_MODE` | abs | マウス転送方式。`abs`=絶対位置(Macの速度感をそのまま再現、画面比率も自動補正)/`rel`=従来の相対移動 |
| `TSUNAGU_MOUSE_SCALE` | 1.0 | マウス移動の倍率。0.7 で遅く、1.5 で速く |
| `TSUNAGU_SWITCH_MODE` | edge | 切替方式。`edge`=画面右端とF13の両方(既定)/`hotkey`=F13のみで切替し、切替後は境界を超えても戻らないロック状態(F13で戻すまで固定) |
## ホットキーロックモード(オプション)

`./scripts/restart-mac.sh --hotkey` で起動すると、画面境界での自動切替をやめ、
**ホットキー1つだけで Mac⇄Windows を切替**できます。切替後は境界を超えても
勝手に切り替わらないロック状態になり、もう一度ホットキーを押すまで戻りません
(Windows 側で左端に行っても戻りません)。`--edge` で従来モードに戻ります。

- 既定のホットキーは **F13**(Mac keycode 105)。MacBook 内蔵キーボードに F13 が
  無い場合は `TSUNAGU_HOTKEY_KC` で変更できます(例: 右 Cmd=54、F6=97)
  - 起動例: `TSUNAGU_HOTKEY_KC=54 ./scripts/restart-mac.sh --diag --hotkey`
- 起動ログに `switch_mode=hotkey(ロック) hotkey_kc=105` の形式で反映状況が出ます

| 変数 | 既定 | 説明 |
| `TSUNAGU_SIDE` | right | Windows 画面の位置。right/left/up/down + upright/lowright(右下)/upleft/lowleft。設定窓の配置エディタ(ドラッグ)が同じ結果を視覚的に作れる |
| `TSUNAGU_SIDE` | right | Windows 画面の位置(Deskflow links 相当)。right/left/up/down。切替境界と戻り端が連動 |
| `TSUNAGU_SWITCH_DELAY` | 0 | 端に N ms 滞ってから切替(switchDelay。0=無効でダブルタップ/即時) |
| `TSUNAGU_DOUBLE_TAP_MS` | 700 | ダブルタップの判定窓 ms(switchDoubleTap) |
| `TSUNAGU_CORNER_PX` | 0 | 四隅 N px 内では切替しない(switchCorners/cornerSize) |
| `TSUNAGU_SWIPE_NAV` | 1 | 2本指横スワイプをブラウザの戻る/進むへ翻訳(XButton)。0 で従来の横ホイール |
| `TSUNAGU_SCROLL_COMPAT` | 0 | スクロール互換モード(1 で 1ノッチ=120単位送信。一部の古いアプリでスクロールが効かない時) |
| `TSUNAGU_CLIP` | 1 | クリップボード共有(clipboardSharing)。0 で無効 |
| `TSUNAGU_EDGE_TAPS` | 2 | 境界到達回数。既定2=境界に続けて2回当てた時(500ms以内)だけ切替(誤爆防止)。1=従来の1回切替 |
| `TSUNAGU_EDGE_PX` | 2 | 右端切替の判定幅(右端からの距離 px)。0 以上 100 未満 |
| `TSUNAGU_TOKEN` | tsunagu-dev | 両側共通の認証トークン |

## 改善ループ(開発者用)

変更→検証の 1 サイクルを回す手順:

```bash
# Mac 側: ビルド鮮度保証付きで再起動(引数はそのまま tsunagu-mac へ)
# 起動直後に異常終了した場合はログ末尾とともに WARN を表示する
./scripts/restart-mac.sh --diag

# Windows 側: ビルド → 停止 → 配布 → 対話起動
./scripts/deploy-win.sh

# 自動検証(プロセス/接続/クリップボード双方向/IMEログ/diag集計)
./scripts/verify.sh
```

- `restart-mac.sh` / `deploy-win.sh` は起動のたび BUILD_ID(日時+git短縮sha)を
  埋め込み、`/tmp/tsunagu-mac.log`・`C:\Users\<user>\tsunagu\tsunagu-win.log` の
  先頭行(`[info] tsunagu-mac ...` / `[info] tsunagu-win ...`)で配布物の鮮度を確認できる
- `--diag` は毎秒
  `mode/moves/keys/sent/scrolls/warp_fixed/switches/cursor/moving` をログ出力。
  境界問題の切り分けは `warp_fixed`(カーソル巻き戻し回数)と
  `moving`(WIN中は false が正常)で行う。`switches` はモード切替回数、
  `scrolls` はスクロール送信回数
- 起動ログにスクロール除数・マウス倍率・切替判定幅・クリップボード上限の
  調整値がまとめて出る(`[info] screen ... scroll_div=... mouse_scale=...`)
- `verify.sh` は Windows へ ssh できない場合、該当項目を NG にせず WARN 扱いにして
  続行する(検証不能と失敗を区別)
- `check-mouse.sh` は切替後に Mac 側カーソルが凍結(抑制)されているかを検証する
- ログ: Mac=`/tmp/tsunagu-mac.log`、Windows=`C:\Users\<user>\tsunagu\tsunagu-win.log`
  (ssh home で type)

## 既知の制限

- クリップボードはプレーンテキストのみ(1MB 上限)。画像・書式・ファイルは未対応
- Windows 側のウィンドウ操作(Focus/Minimize)は可視ウィンドウのタイトル部分一致
  (先に見つかった 1 枚に対して動作)
- Mac 側の IME 状態とは独立(かな/英数キーで Windows 側だけ切替)
- IME コンテキストが取れないウィンドウでのみ、IME フォールバックがトグル動作のため
  開閉の方向が保証されない
- UAC 昇格中のプロセスには UIPI により SendInput が弾かれる
  (design.md「残リスク」参照)
- 通信の暗号化は Tailscale(WireGuard)層に依存し、アプリ層はトークン認証のみ
  (TLS なし)

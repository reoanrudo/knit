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
直結 IP は Windows 側 `.env` の `TSUNAGU_HOST` に **カンマ区切りで並べて** 指定できます
(例: `TSUNAGU_HOST=169.254.10.2,100.100.10.9`)。起動のたびに全候補へ同時に接続を試み、
最初に繋がった経路(=遅延の小さい経路)を使うため、直結を抜いても Tailscale へ自動で戻ります。
Mac の IP は `ifconfig` で確認(Thunderbolt ブリッジは bridge0、USB-LAN は en*)。
接続先は **LAN 自動発見(UDP 24903)と `TSUNAGU_HOST` の併用**です。接続のたびにまず同じ
LAN の Mac を探し(最初の応答 or 600ms)、見つかった LAN IP を先頭に `TSUNAGU_HOST` の
候補を並べて同時接続レースへかけます。つまり **同じ LAN では Tailscale の状態に
関係なく常時 LAN 直**、LAN 外(AP 隔離・外出先)では `TSUNAGU_HOST` の Tailscale IP へ
自動フォールバックします。

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

## クリップボード(画面を移る時に同期)

- **画面を移る瞬間に同期します**(Deskflow と同じ方式)。Mac で ⌘C → Windows へ移ると
  Mac の内容が Windows に渡り、Windows で Ctrl+C → Mac へ戻ると Windows の内容が Mac に渡る。
  コピーのたびには送らないため、Mac の中だけのコピペで大きなファイルが流れることはない
- 対応形式: テキスト(1MB まで)・画像(両方向)・ファイル(両方向、合計 200MB まで)
- パスワードマネージャ等が「共有しない」印を付けたコピーは送らない
  (Mac: nspasteboard の Concealed/Transient、Windows: ExcludeClipboardContentFromMonitorProcessing)
- 受信ファイルは各 PC の Downloads/Tsunagu に保存し、同名は「名前 (1).拡張子」で回避する。
  外部から来たファイルとして Mac は quarantine、Windows は Zone.Identifier を付ける
  (開く時に OS の確認が出る)
- Win→Mac は CRLF を LF へ正規化して書き込む。相手から受信した内容は送り返さない
- 設定窓でクリップボード共有を OFF にすると、Windows 側も送らない
## 音量・メディアキー(Windows 画面操作中)

Windows 画面を操作している間、Mac 本体キーボードの音量キーとメディアキーは
**Windows 側の操作**として転送されます(Mac 側では変わらない):

| Mac のキー(fn を併用、または「F1〜F12 を標準のファンクションキーとして使用」が OFF) | Windows での動作 |
|---|---|
| F10/F11/F12(音量) | Windows の音量ミュート/下げ/上げ |
| F7 / F8 / F9(メディア) | 前の曲へ / 再生・一時停止 / 次の曲へ |

fn を押さない F7〜F12 は従来どおり F キーとして渡ります。

## 接続の挙動

- 経路: 本線 TCP 24900(入力・制御)/ 音声 TCP 24901 / ファイル・画像 TCP 24902 /
  自動発見 UDP 24903。ファイル転送は本線と別経路のため、転送中もマウスが止まらない
- **全経路を暗号化**(Noise プロトコル。トークンから導いた鍵で相互認証し、トークン自体は
  回線に流れない)。接続を受け入れるのは LAN・有線直結・Tailscale のアドレスのみ
  (`TSUNAGU_ALLOW_ANY=1` で全許可)
- 生存確認は双方向: Mac・Windows とも 3 秒毎に ping し、9〜10 秒応答が無ければ張り直す
- Mac のスリープ復帰を検知すると即座に張り直す(復帰直後の死んだ接続を待たない)
- Tailscale の経路が直結から中継(DERP)に落ちると通知する(遅延が数倍になるため)
- Windows モード中に切断・Mac の画面ロックが起きたら即 Mac モードへ復帰(入力の閉じ込め防止)
- 画面を離れる時は、Windows 側で押下中の全キー・ボタンを解放する(押しっぱなしを残さない)

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
| `TSUNAGU_SWITCH_DELAY` | 0 | 端に N ms 滞ってから切替(switchDelay。0=無効でダブルタップ/即時) |
| `TSUNAGU_DOUBLE_TAP_MS` | 700 | ダブルタップの判定窓 ms(switchDoubleTap) |
| `TSUNAGU_CORNER_PX` | 0 | 四隅 N px 内では切替しない(switchCorners/cornerSize) |
| `TSUNAGU_SWIPE_NAV` | 1 | 2本指横スワイプをブラウザの戻る/進むへ翻訳(XButton)。0 で従来の横ホイール |
| `TSUNAGU_SCROLL_COMPAT` | 0 | スクロール互換モード(1 で 1ノッチ=120単位送信。一部の古いアプリでスクロールが効かない時) |
| `TSUNAGU_CLIP` | 1 | クリップボード共有(clipboardSharing)。0 で無効 |
| `TSUNAGU_EDGE_TAPS` | 2 | 境界到達回数。既定2=境界に続けて2回当てた時(500ms以内)だけ切替(誤爆防止)。1=従来の1回切替 |
| `TSUNAGU_EDGE_PX` | 2 | 右端切替の判定幅(右端からの距離 px)。0 以上 100 未満 |
| `TSUNAGU_TOKEN` | (必須) | 両側共通の秘密。暗号化の鍵の元になる。未設定だと起動しない |
| `TSUNAGU_BIND` | 0.0.0.0 | Mac 側の待受アドレス。既定は全インターフェース(LAN 直を受け入れる) |
| `TSUNAGU_HOST` | (未設定) | Windows 側の接続先(フォールバック候補)。LAN 自動発見とは併用で、見つかった LAN IP が優先される |
| `TSUNAGU_ALLOW_ANY` | 0 | 1 で LAN・直結・Tailscale 以外のアドレスからの接続も受け入れる |
| `TSUNAGU_CTRL_APPS` | ターミナル系 | Windows 側。Mac の Control を Win キーではなく Ctrl として送るアプリ(実行ファイル名のカンマ区切り) |
| `TSUNAGU_GAME_MODE` | 1 | Windows 側。0 でゲームモード(カーソル閉じ込め時の相対移動への自動切替)を無効化 |
| `TSUNAGU_LOCK_SYNC` | 1 | Mac の画面ロックで Windows もロックする。0 で無効 |

## 機能別品質向上(開発者用)

機能ごとの品質改善を回すときのセッション用プロンプト集は
`docs/feature-quality-prompts.md` にあります(共通の土台+機能別ブロックを
新しいセッションへ貼るだけで、文脈ゼロから着手できます)。

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

## 開発者・ゲーム向けの自動切替

- **ターミナルでの Control**: Windows の前面が Windows Terminal・cmd・PowerShell・WSL 等の間は、
  Mac の Control を Windows の Ctrl として送る(Ctrl+A/E/R/C がそのまま効く)。それ以外のアプリでは
  従来どおり Win キー。対象は `TSUNAGU_CTRL_APPS` で変更できる
- **ゲームモード**: Windows でカーソルが閉じ込められた(FPS 等)、または全画面でカーソルが
  1.5 秒以上隠れた間は相対移動で送る(視点回転が効く)。戻るにはホットキーを使う
- **Secure Input の通知**: Mac でパスワード欄などの保護入力が有効だと、キーボードを Windows へ
  送れない。Windows へ移った時にこの状態なら、原因のアプリ名を通知する

## 既知の制限

- リッチテキスト(書式付き)のコピーは未対応(プレーンテキストとして渡る)
- Mac 側の IME 状態とは独立(かな/英数キーで Windows 側だけ切替)
- IME コンテキストが取れないウィンドウでのみ、IME フォールバックがトグル動作のため
  開閉の方向が保証されない
- UAC 昇格中のプロセスには UIPI により SendInput が弾かれる
  (design.md「残リスク」参照)
- Windows のロック画面・UAC の確認画面は操作できない(SendInput が保護デスクトップに届かない)
- Windows 側は DPI 非対応のまま動作する(座標は OS が拡大率に応じて換算する)

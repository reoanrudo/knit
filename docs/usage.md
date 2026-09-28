# knit 操作ガイド

Mac のキーボード/トラックパッドで Windows デスクトップを操作するツール。
Mac=サーバ(TCP 24900 で待ち受け)、Windows=クライアント(接続し続ける逆転構成)。

## 基本操作

| 操作 | 動作 |
|------|------|
| Mac のカーソルを画面右端へ | Windows へ切替(同寸法の高さで境界を越える) |
| Windows のカーソルを画面左端へ | Mac へ戻る(同じ高さで右端内側に復帰) |
| F13 キー | 手動トグル(切替が効かないときの保険) |

切替まわりの細部の挙動:

- 右端判定は Mac 画面右端から `KNIT_EDGE_PX`(既定 2px)の内側。
  Windows 側の復帰判定は左端(x ≤ 1)で、実際にカーソルが動いたときだけ判定する
- 切替時にドラッグしていた場合は Windows 側へ左ボタンを離すイベントを送り、
  誤ドラッグを持ち込まない
- Mac への復帰直後 400ms は右端判定を無効化し、切替の往復チャタリングを防止
- Windows 側の左端復帰は 0.7 秒のクールダウン付き(連打による往復を防止)。
  通常の選択やウィンドウのドラッグ中は復帰しない。ファイルを掴んでいる場合は
  下記のファイル引き継ぎでMacへ戻れる。

## キーボード

- Mac の修飾キーは自動変換: Cmd→Ctrl、Option→Alt、Control→Win、Shift→Shift
  (Win 側では「Mac と同じ修飾の組合せ」になるよう差分で押し替え、復帰時に全解放)
- **右⌘キーは Windows の右 Ctrl として動きます**(既定)。`KNIT_RCMD_CTRL=0` で無効
  (右⌘をホットキーに設定している場合はホットキーが優先される)
- **かなキー**(Mac keycode 104)→ Windows 側の IME を ON(ひらがな入力)
- **英数キー**(Mac keycode 102)→ Windows 側の IME を OFF(英字入力)
- **切替時に Mac の IME 状態を引き継ぐ**(IME Follow Cursor): Windows へ画面を
  移る瞬間、Mac がかな入力中なら Windows の IME を ON、英数モードなら OFF へ
  合わせる。日本語入力以外(英字レイアウト)の間は Windows 側を変えない
  (`KNIT_IME_SYNC=0` で無効化)
  - フォアグラウンドウィンドウのデフォルト IME ウィンドウへ
    `WM_IME_CONTROL`(IMC_SETOPENSTATUS)を送る(方向指定が確実な定番手法)
  - IME ウィンドウが取れないアプリ(コンソール等)では、手動のかな/英数キーは
    半角/全角相当のトグルへフォールバックし、**切替時の自動同期はスキップ**します
    (方向を保証できないトグルで同期すると、切替のたびに IME が反転し続けるため)
  - 成否は Mac 側ログ(`[ime] kc=104 (かな) 転送` 等)と Windows 側ログ
    (`[ime] WM_IME_CONTROL open=true -> sent` 等)で切り分けられる
- 変換・確定(Enter/Space)はそのまま転送され Windows の IME が処理する

## マウス

- **Windows 側はサブピクセル累積方式**: 受け取った移動量(dx, dy)を f64 で累積し、
  整数部だけ SendInput で注入、端数は次のイベントへ持ち越す。1px 未満の
  トラックパッドの細かい動きも消えず滑らかに動く
- **Mac 側は delta 積算 + 間欠同期で切替判定**: タップコールバック内で移動 delta を
  積算して自前のカーソル位置を追跡し、16 イベントに 1 回だけ実カーソル位置へ同期
  する(毎イベントの位置取得は負荷が高くカクつくため)。切替の瞬間だけ実位置を
  取って正確な高さを Windows 側へ引き継ぐ
- 移動倍率は `KNIT_MOUSE_SCALE` で調整(既定 1.0)

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

いずれも IP リンクが張れれば Knit はそのまま動きます(TCP のみのため)。
直結 IP は Windows 側 `.env` の `KNIT_HOST` に **カンマ区切りで並べて** 指定できます
(例: `KNIT_HOST=169.254.10.2,100.100.10.9`)。起動のたびに全候補へ同時に接続を試み、
最初に繋がった経路(=遅延の小さい経路)を使うため、直結を抜いても Tailscale へ自動で戻ります。
Mac の IP は `ifconfig` で確認(Thunderbolt ブリッジは bridge0、USB-LAN は en*)。
接続先は **LAN 自動発見(UDP 24903)と `KNIT_HOST` の併用**です。接続のたびにまず同じ
LAN の Mac を探し(最初の応答 or 600ms)、見つかった LAN IP を先頭に `KNIT_HOST` の
候補を並べて同時接続レースへかけます。つまり **同じ LAN では Tailscale の状態に
関係なく常時 LAN 直**、LAN 外(AP 隔離・外出先)では `KNIT_HOST` の Tailscale IP へ
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
  ノッチ単位へ変換し、**0.05 ノッチ(=6 ホイールユニット)刻み**で Windows へ送る(端数は持ち越し。
  プレシジョンタッチパッドと同じ高解像度スクロール。低速でもカクつかない)
- Windows 側はノッチ × 120 ホイールユニットで注入
- 除数を大きくすると遅くなる(40〜200 程度で調整)

## クリップボード(画面を移る時に同期)

- **画面を移る瞬間に同期します**(Deskflow と同じ方式)。Mac で ⌘C → Windows へ移ると
  Mac の内容が Windows に渡り、Windows で Ctrl+C → Mac へ戻ると Windows の内容が Mac に渡る。
  コピーのたびには送らないため、Mac の中だけのコピペで大きなファイルが流れることはない
- 対応形式: テキスト(1MB まで)・画像(両方向)・ファイル(両方向、1回の合計 10GiB まで)
- ファイルは1件でも合計でも最大10GiB(10,737,418,240バイト)。Mac・Windowsの両方を更新すると利用できる。
  256KiBずつ読み書きするため、ファイル全体をメモリに載せない。受信先には転送量に応じた空き容量が必要。
- パスワードマネージャ等が「共有しない」印を付けたコピーは送らない
  (Mac: nspasteboard の Concealed/Transient、Windows: ExcludeClipboardContentFromMonitorProcessing)
- 受信ファイルは各 PC の Downloads/Knit に保存し、同名は「名前 (1).拡張子」で回避する。
  外部から来たファイルとして Mac は quarantine、Windows は Zone.Identifier を付ける
  (開く時に OS の確認が出る)
- Win→Mac は CRLF を LF へ正規化して書き込む。相手から受信した内容は送り返さない
- 設定窓でクリップボード共有を OFF にすると、Windows 側も送らない
- (実験的)同じ PC で ollaya が動いている場合、テキストの送信前にローカルの判定モデルで
  機密らしさ(パスワード・API キー等)を検査し、確度が高いときだけ送信を止めます(smartguard)。
  判定は完全にローカルで、テキストが外へ出ることはありません。ollaya が無ければ従来どおり。
  誤検知時は `KNIT_SMART_SECRET=0` で無効化。対象はテキストのみ(画像・ファイルは従来どおり)
- Mac→Win は LF を CRLF へ正規化して書き込む(メモ帳等で貼り付けた時の行送りの乱れを防ぐ)

## クリップボード履歴(Universal Clipboard History)

- 画面を越えて送信・受信したテキスト・URL・**ファイル参照・画像**が履歴に記録されます
  (新しい順 10 件を表示、上限 50 件)。
  **Windows はトレイ右クリック →「クリップボード履歴」、Mac はメニューバー →「クリップボード履歴」**
  から選ぶと、その PC のクリップボードへ復元できます(貼り付け操作で使えます)
- 項目は「3分前・Mac・本文の先頭…」の形式。ファイルは「ファイル・名前 ほかN件」、
  画像は「画像・NKB」と種別が分かるように出ます。どちらの PC で発生したコピーか一目で分かります
- 履歴は起動しても保持されます(Windows: %LOCALAPPDATA%\Knit\history.json、
  Mac: ~/.config/knit/history.json)。「履歴を消す」で全消去できます
- 画像の本体は各 PC のデータ領域(Windows: %LOCALAPPDATA%\Knit\images、
  Mac: ~/.config/knit/images)に内容ハッシュ名で保存され、新しい 60 件へ
  自動的に刈り込まれます(履歴から選ぶと画像が戻ります)
- 機密判定(smartguard)で止めたテキストや「共有しない」印のコピーは履歴に載りません
- 1MB を超えるテキストは同期も履歴も対象外です(通知でお知らせします)

## Search My Desk(デスク横断検索)

- Mac 操作中に **⌥⌘S**(またはメニューバーの「Search My Desk…」)で検索窓を開きます。
  窓は**カーソルがある画面の中央**へ出ます。もう一度 **⌥⌘S か Esc で閉じます**。
  **両 PC のアプリ・デスクのファイル/フォルダ・コマンド・クリップボード履歴・URL** を
  1 つの窓から扱えます(ビジョン§14 の実装)。
- 検索窓が開いている間のキー入力は knit が直接受け取ります(IME に影響されず、
  日本語入力モードでもローマ字でそのまま検索できます。Spotlight と同じ挙動)。
  入力するたびに候補が絞られます。**↑↓ で候補を選び**(選択行に ► が付きます)、
  **アプリは Enter かクリックで起動**、
  **ファイル/フォルダは Enter で既定アプリ/Finder で開く**、
  **履歴はクリックでこの Mac のクリップボードへ復元**、
  **コマンド(Windows をロック等)は Enter で実行**、
  **全文が URL の入力は先頭候補として「Windows で開く」が出ます**(Continue Here と同じ経路)
- **⌥Enter で先頭のファイル候補を Windows へ投げられます**(ビジョン§13 Throw の
  ファイル版。Windows 側で Ctrl+V で貼り付け可)
- ファイル/フォルダ候補は Desktop・Downloads・Documents(直下と 1 階層下)から
  10 分キャッシュで索引します(隠しファイル・node_modules 等は除外)。検索窓を
  開いた時点で裏から索引を更新するため、作成直後のファイルは次回オープンで出ます
- 履歴のファイル参照(コピー・受信したファイル)も「履歴・パス」候補として出ます。
  選ぶとその端末のクリップボードへ ⌘C/Ctrl+C 相当で復元します(実体が移動・削除
  済みなら通知して復元しません)
- Windows 側のアプリ候補は「**Windows・アプリ・名前**」の形式。クリックで **Windows で起動**します
  (検索窓を開いた時点で Windows へ一覧を問い合わせ、届き次第候補に混ざります)
- 対象アプリ: Mac は /Applications・/System/Applications・~/Applications(Utilities 含む)、
  Windows はスタートメニューのショートカット(最大 200 件)
- Windows 側は「列挙したパスと完全一致する起動指示だけ」を実行します
  (任意パスの実行は受けません)
- 実機検証: 対話セッションで `knit-mac --probe-search` を実行すると
  「窓/入力/8 ボタン OK」と出ます
- `KNIT_DESK_SEARCH=0` で無効化

## 越境 App Handoff(実験的)

- `KNIT_APP_HANDOFF=1` を設定すると、**Windows へ切り替えた瞬間**に Mac の
  最前面アプリと同じアプリを Windows でも起動します(«作業を別 PC へ投げた» 体感の
  第一歩。ビジョン§12)。例: Mac の「Terminal」→ Windows の「Windows Terminal」
- 照合はアプリ一覧(検索窓で使う列挙)に対する完全一致→部分一致(Mac 名 4 文字以上)。
  対応するアプリが無ければ何もしません(ログに記録)
- 勝手にアプリが開く驚きを避けるため**既定は無効**です

## 境界越えの賢い判定(速度)

- 境界へ**速く**進んだ場合(目安 1200px/s 以上)は「意図的な越え」とみなし、
  滞在待ち(switchDelay)やダブルタップをスキップして即座に切替します。
  ゆっくり端に触れた場合だけ従来どおりの誤爆防止が働きます
- どちらで切替したかはログに記録されます(`[mode] WINDOWS (edge, fast)`)。
  `KNIT_FAST_EDGE=0` で無効化

## Windowsのファイルを掴んでMacへ渡す

1. 共有中のMacのマウス・トラックパッドで、WindowsのExplorerからファイルを掴む。
2. Macと接する画面端の細い青い帯へ運び、押したまま待つ。
3. 転送が終わるとMacへ移り、ファイルのアイコンを掴んだ状態になる。
4. Finderで開いたフォルダ内や、ファイルを受け取れるアプリで離す。

- 原本を残してコピーする。複数選択は64件まで、合計10GiBまで。
- 大容量では画面端で転送待ちになる。準備中に端から戻す・ボタンを離すと取り消す。
- Macへ移った後はEscで取り消せる。通常のクリップボードは書き換えない。
- フォルダ・仮想ファイルは未対応。通常のファイルだけを選ぶ。
- 両アプリのプロトコル12対応版が必要。旧版との接続では従来のコピー・貼り付けを利用する。
- この機能はファイルを逆方向へ渡すもの。Windowsに接続した別のマウスでMacを操作する機能は含まない。

## Continue Here(ブラウザの引継ぎ)

Windows 画面を操作している時に **⌥⌘T** を押すと、Mac の前面ブラウザの現在ページを
**Windows の既定ブラウザで開きます**。「Mac で見ていたページを Windows でもう一度
探す」手間を 1 回で消します(ビジョン§11)。

- 対応ブラウザ(Mac 側の読み取り): Safari / Google Chrome / Microsoft Edge / Brave
  (Firefox は URL の AppleScript 対応が無いため対象外)。**読み取るのは実際に
  前面にあるブラウザ**です(裏で起動しているだけのブラウザは読み取りません)
- 開く側は Windows の**既定ブラウザ**に関連付けに従う
- 転送は `http`/`https` のみ(上限 2048 文字)。送信側・受信側の両方で検査する
- 初回は Mac 側で「自動化の許可」(System Events とブラウザ)のダイアログが
  出ます。Windows 画面を操作している間は気づきにくいため、失敗時は Mac に
  通知を出します
- ⌥⌘T は専用ショートカットとして Mac・Windows のどちらにも転送されません
  (Ctrl+Alt+T 系のショートカットと衝突する場合は `KNIT_CONTINUE_HERE=0`)
- `KNIT_CONTINUE_HERE=0` で無効化

## 音量・メディアキー(Windows 画面操作中)

Windows 画面を操作している間、Mac 本体キーボードの音量キーとメディアキーは
**Windows 側の操作**として転送されます(Mac 側では変わらない):

| Mac のキー(fn を併用、または「F1〜F12 を標準のファンクションキーとして使用」が OFF) | Windows での動作 |
|---|---|
| F10/F11/F12(音量) | Windows の音量ミュート/下げ/上げ |
| F7 / F8 / F9(メディア) | 前の曲へ / 再生・一時停止 / 次の曲へ |

fn を押さない F7〜F12 は従来どおり F キーとして渡ります。
F1〜F6 は macOS が輝度・キーボード照明のメディアイベントとして配るため、Windows では
対応する F1/F2・F5/F6 キーとして届け直します(実験的。輝度そのものは Windows 側の
画面を変えられないため)。F3/F4 は OS が直接処理するためイベントが届かないことがあります

## 実機での確認手順(IME 引継ぎ・Continue Here)

どちらも「画面を移る時だけ」動く機能のため、次の手順で確認します:

**IME 引継ぎ(IME Follow Cursor)**

1. Mac を日本語入力(かなモード)にしてから Windows 画面へ移る
   → Windows 側が日本語入力になっている(Windows のログに
   `[ime] mac の状態へ同期: かな(ON)`)
2. Mac 側で英数キーを押してから Windows 画面へ移る
   → Windows 側が英字入力になっている(`英数(OFF)` のログ)
3. Mac を英字レイアウト(日本語入力ではない)にして Windows へ移る
   → Windows 側の IME は変化しない(ログも出ない)

**Continue Here(⌥⌘T)**

1. Mac の Safari/Chrome/Edge/Brave で任意のページを開く
2. Windows 画面へ移り、⌥⌘T を押す
3. Windows の既定ブラウザで同じページが開く(Mac 側ログに `[url] Continue Here: 送信しました`)
   - Firefox は Mac 側の読み取り対象外(開く側は Windows の既定ブラウザなら何でも可)

## 接続の挙動

- 経路: 本線 TCP 24900(入力・制御)/ 音声 TCP 24901 / ファイル・画像 TCP 24902 /
  自動発見 UDP 24903。ファイル転送は本線と別経路のため、転送中もマウスが止まらない
- **全経路を暗号化**(Noise プロトコル。トークンから導いた鍵で相互認証し、トークン自体は
  回線に流れない)。接続を受け入れるのは LAN・有線直結・Tailscale のアドレスのみ
  (`KNIT_ALLOW_ANY=1` で全許可)
- 生存確認は双方向: Mac・Windows とも 3 秒毎に ping し、9〜10 秒応答が無ければ張り直す
- Windows の解像度変更・モニター抜き差しを検知すると Mac へ通知し、
  カーソル速度の換算と Windows 画面内の仮想カーソル位置を自動で追従させる
- Mac のスリープ復帰を検知すると即座に張り直す(復帰直後の死んだ接続を待たない)
- Tailscale の経路が直結から中継(DERP)に落ちると通知する(遅延が数倍になるため)
- Windows モード中に切断・Mac の画面ロックが起きたら即 Mac モードへ復帰(入力の閉じ込め防止)
- 画面を離れる時は、Windows 側で押下中の全キー・ボタンを解放する(押しっぱなしを残さない)

## 調整用環境変数(knit-mac 起動時)

| 変数 | 既定 | 説明 |
|------|------|------|
| `KNIT_SCROLL_DIV` | 60 | スクロール速度の除数(初期値)。大きくすると遅い。実行中は設定ウィンドウのスライダーで可変 |
| `KNIT_SCROLL_FLIP` | (未設定) | スクロール方向。未設定=macOS の設定(自然スクロール ON/OFF)に自動追従。`1` で Windows 標準へ固定 |
| `KNIT_MOUSE_MODE` | abs | マウス転送方式。`abs`=絶対位置(Macの速度感をそのまま再現、画面比率も自動補正)/`rel`=従来の相対移動 |
| `KNIT_MOUSE_SCALE` | 1.0 | マウス移動の倍率。0.7 で遅く、1.5 で速く |
| `KNIT_SWITCH_MODE` | edge | 切替方式。`edge`=画面右端とF13の両方(既定)/`hotkey`=F13のみで切替し、切替後は境界を超えても戻らないロック状態(F13で戻すまで固定) |

## ホットキーロックモード(オプション)

`./scripts/restart-mac.sh --hotkey` で起動すると、画面境界での自動切替をやめ、
**ホットキー1つだけで Mac⇄Windows を切替**できます。切替後は境界を超えても
勝手に切り替わらないロック状態になり、もう一度ホットキーを押すまで戻りません
(Windows 側で左端に行っても戻りません)。`--edge` で従来モードに戻ります。

- 既定のホットキーは **F13**(Mac keycode 105)。MacBook 内蔵キーボードに F13 が
  無い場合は `KNIT_HOTKEY_KC` で変更できます(例: 右 Cmd=54、F6=97)
  - 起動例: `KNIT_HOTKEY_KC=54 ./scripts/restart-mac.sh --diag --hotkey`
- 起動ログに `switch_mode=hotkey(ロック) hotkey_kc=105` の形式で反映状況が出ます

## 調整用環境変数(つづき)

| 変数 | 既定 | 説明 |
|------|------|------|
| `KNIT_SIDE` | right | Windows 画面の位置。right/left/up/down + upright/lowright(右下)/upleft/lowleft。設定窓の配置エディタ(ドラッグ)が同じ結果を視覚的に作れる |
| `KNIT_SWITCH_DELAY` | 0 | 端に N ms 滞ってから切替(switchDelay。0=無効でダブルタップ/即時) |
| `KNIT_DOUBLE_TAP_MS` | 700 | ダブルタップの判定窓 ms(switchDoubleTap) |
| `KNIT_CORNER_PX` | 0 | 四隅 N px 内では切替しない(switchCorners/cornerSize) |
| `KNIT_SWIPE_NAV` | 1 | 2本指横スワイプをブラウザの戻る/進むへ翻訳(XButton)。0 で従来の横ホイール |
| `KNIT_SCROLL_COMPAT` | 0 | スクロール互換モード(1 で 1ノッチ=120単位送信。一部の古いアプリでスクロールが効かない時) |
| `KNIT_CLIP` | 1 | クリップボード共有(clipboardSharing)。0 で無効 |
| `KNIT_EDGE_TAPS` | 2 | 境界到達回数。既定2=境界に続けて2回当てた時(`KNIT_DOUBLE_TAP_MS` 既定 700ms 以内)だけ切替(誤爆防止)。1=従来の1回切替 |
| `KNIT_EDGE_PX` | 2 | 右端切替の判定幅(右端からの距離 px)。0 以上 100 未満 |
| `KNIT_TOKEN` | (必須) | 両側共通の秘密。暗号化の鍵の元になる。未設定だと起動しない |
| `KNIT_BIND` | 0.0.0.0 | Mac 側の待受アドレス。既定は全インターフェース(LAN 直を受け入れる) |
| `KNIT_HOST` | (未設定) | Windows 側の接続先(フォールバック候補)。LAN 自動発見とは併用で、見つかった LAN IP が優先される |
| `KNIT_ALLOW_ANY` | 0 | 1 で LAN・直結・Tailscale 以外のアドレスからの接続も受け入れる |
| `KNIT_CTRL_APPS` | ターミナル系 | Windows 側。Mac の Control を Win キーではなく Ctrl として送るアプリ(実行ファイル名のカンマ区切り) |
| `KNIT_GAME_MODE` | 1 | Windows 側。0 でゲームモード(カーソル閉じ込め時の相対移動への自動切替)を無効化 |
| `KNIT_LOCK_SYNC` | 1 | Mac の画面ロックで Windows もロックする。0 で無効 |
| `KNIT_IME_SYNC` | 1 | Windows へ入る時の IME 状態引継ぎ(かな=ON/英数=OFF)。0 で無効 |
| `KNIT_CONTINUE_HERE` | 1 | ⌥⌘T でのブラウザ引継ぎ(Continue Here)。0 で無効 |

## 機能別品質向上(開発者用)

機能ごとの品質改善を回すときのセッション用プロンプト集は
`docs/feature-quality-prompts.md` にあります(共通の土台+機能別ブロックを
新しいセッションへ貼るだけで、文脈ゼロから着手できます)。

## 改善ループ(開発者用)

変更→検証の 1 サイクルを回す手順:

```bash
# Mac 側: ビルド鮮度保証付きで再起動(引数はそのまま knit-mac へ)
# 起動直後に異常終了した場合はログ末尾とともに WARN を表示する
./scripts/restart-mac.sh --diag

# Windows 側: ビルド → 停止 → 配布 → 対話起動
./scripts/deploy-win.sh

# 自動検証(プロセス/接続/クリップボード双方向/IMEログ/diag集計)
./scripts/verify.sh
```

- `restart-mac.sh` / `deploy-win.sh` は起動のたび BUILD_ID(日時+git短縮sha)を
  埋め込み、`/tmp/knit-mac.log`・`C:\Users\<user>\knit\knit-win.log` の
  先頭行(`[info] knit-mac ...` / `[info] knit-win ...`)で配布物の鮮度を確認できる
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
- ログ: Mac=`/tmp/knit-mac.log`、Windows=`C:\Users\<user>\knit\knit-win.log`
  (ssh home で type)

## 開発者・ゲーム向けの自動切替

- **ターミナルでの Control**: Windows の前面が Windows Terminal・cmd・PowerShell・WSL 等の間は、
  Mac の Control を Windows の Ctrl として送る(Ctrl+A/E/R/C がそのまま効く)。それ以外のアプリでは
  従来どおり Win キー。対象は `KNIT_CTRL_APPS` で変更できる
- **ゲームモード**: Windows でカーソルが閉じ込められた(FPS 等)、または全画面でカーソルが
  1.5 秒以上隠れた間は相対移動で送る(視点回転が効く)。戻るにはホットキーを使う
- **Secure Input の通知**: Mac でパスワード欄などの保護入力が有効だと、キーボードを Windows へ
  送れない。Windows へ移った時にこの状態なら、原因のアプリ名を通知する

## 既知の制限

- リッチテキスト(書式付き)のコピーは未対応(プレーンテキストとして渡る)
- IME ウィンドウが取れないアプリでは、手動のかな/英数キーだけが効きます
  (トグル動作。切替時の自動同期はスキップされるため反転しません)
- Mac→Windows 方向のみ自動引継ぎ。Windows で変えた IME 状態は Mac へ戻る時に
  Mac へは反映されない(かな/英数キーで手動切替)
- UAC 昇格中のプロセスには UIPI により SendInput が弾かれる
  (design.md「残リスク」参照)
- Windows のロック画面・UAC の確認画面は操作できない(SendInput が保護デスクトップに届かない)
- Windows 側は DPI 非対応のまま動作する(座標は OS が拡大率に応じて換算する)

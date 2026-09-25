# tsunagu 配布手順書(v0.7)

作成日: 2026-09-25 / 対象バージョン: 0.6.0(音声転送・Windowsステータスウィンドウ・トレイ常駐・接続方向の選択)

## 1. 配布物の単位

| 対象 | 配布物 | 生成方法 |
|---|---|---|
| Mac | `dist/Tsunagu-<ver>.zip`(Tsunagu.app、アイコン・トークン同梱) | `scripts/package-mac.sh` |
| Windows | `win-dist/` 一式(tsunagu-win.exe・app.ico・install.bat・run_tsunagu.bat/vbs・.env) | `scripts/deploy-win.sh` 実行後に win-dist を zip 等 |

tsunagu-win.exe にはアプリケーションアイコンが埋め込まれ(windres)、トレイ用に
`app.ico` も同梱される(exe と同じフォルダに置くとトレイが使う)。

トークンの扱い: `~/.config/tsunagu/env` の `TSUNAGU_TOKEN` が
Mac は .app 内 `.env` に封入、Windows は `win-dist/.env` として配布される。
**zip を持つ者は誰でも接続できる**ため、配布範囲=信頼範囲であること(自tailnet内の想定)。

## 2. 前提(配布先の環境)

- 両機が同じ Tailscale ネットワークに参加済み(WireGuard 暗号化を通信の前提とする)
- Mac: macOS(arm64)。アクセシビリティ権限(CGEventTap)と画面録画は不要・入力監視は不要
- Windows: 対話セッション運用(SendInput の制約)。管理者権限は install.bat 実行時にのみ使用

## 3. Mac へのインストール

```bash
./scripts/package-mac.sh    # dist/ に .app と zip を生成
./scripts/install-mac.sh    # ~/Applications へ配置 + LaunchAgent 登録 + 即時起動
```

- ログイン時に自動起動(LaunchAgent `local.tsunagu`)。ログは /tmp/tsunagu-mac.log
- 初回起動時、アクセシビリティ権限の許可を求められたら許可する
  (許可がないと `[fatal] CGEventTapCreate failed` で終了する)
- メニューバーに「SD·Mac / SD·Win / SD·✕」が表示されれば起動完了
- アンインストール: `launchctl unload ~/Library/LaunchAgents/local.tsunagu.plist`
  → plist と ~/Applications/Tsunagu.app を削除

手動起動(.app を使わない): `./scripts/restart-mac.sh`(開発運用。ビルド鮮度保証付き)

## 4. Windows へのインストール

1. `win-dist/` 一式(tsunagu-win.exe・app.ico・install.bat・run_tsunagu.bat・run_tsunagu.vbs・.env)を
   `C:\Users\<user>\tsunagu` へコピー
2. install.bat を**管理者として実行**(スタートアップ登録のため。以降の起動に管理者権限は不要)
3. **タスクトレイ(通知領域)にアイコンが常駐する**。左クリックでステータスウィンドウ
   (状態/ビルド/音声の表示と「ログを開く」「音声 ON/OFF」「終了」)、右クリックでメニュー

- ログオン時に自動起動(schtasks `tsunagu` / ONLOGON)
- **自動復帰ウォッチ**(schtasks `tsunagu_watch` / 毎分): 何らかの理由で落ちても
  1分以内に自動再起動する(二重起動は exe 内蔵の名前付きミューテックスが即終了させる)
- **コンソール/ターミナル不要**: GUI サブシステム化済みで、exe を直接ダブルクリック
  しても動く。**ターミナル/cmd/Windows Terminal から起動した場合も、exe は起動直後に
  コンソールから独立したプロセスへ自動置換されるため、ターミナルを閉じても接続は維持される**
- 接続/切断はバルーン通知で可視化される
- ログ: `C:\Users\<user>\tsunagu\tsunagu-win.log`(1世代ローテーション)
- **.env がないと起動が fatal 停止する**(トークン必須化のため)

## 5. トークン運用

- 生成: `./scripts/gen-token.sh`(~/.config/tsunagu/env に 256bit ランダム値)
- 参照順序(両バイナリ共通): 環境変数 > 実行ファイル同階層の .env > ~/.config/tsunagu/env
- Mac と Windows で**同じ値**である必要がある(不一致は `[conn] invalid hello` で接続拒否)
- トークンを変更する場合: 両側の .env を更新して両側を再起動(片方だけ更新すると切断が続く)

## 6. 接続方向の選択(配布先のネットワークに応じて)

既定は **Mac=サーバ(待受)/ Windows=クライアント(接続)** です。
配布先によっては逆方向(Win=サーバ/Mac=クライアント)が好ましい場合もあるため、
両対応しています。**どちらの方向も実機で動作検証済みです**(verify.sh 7項目合格)。

| | 既定(Mac=サーバ) | 逆方向(Win=サーバ) |
|---|---|---|
| Mac | TSUNAGU_ROLE 未設定 | `.env` に `TSUNAGU_ROLE=client` と `TSUNAGU_HOST=<Windows側Tailscale IP>` |
| Windows | TSUNAGU_ROLE 未設定 | `.env` に `TSUNAGU_ROLE=server` |
| 追加作業 | なし(下記参考: Mac の受信は Tailscale IF へ限定推奨) | Windows 側に Tailscale 網限定の受信許可が必要(管理者権限で 1 回): `netsh advfirewall firewall add rule name="tsunagu-in" dir=in action=allow protocol=TCP localport=24900 remoteip=100.64.0.0/10` |

設定は両側とも .env(環境変数 > exe同階層の .env > ~/.config/tsunagu/env)。
変更後は**両側の再起動**が必要(片方だけ変えると切断が続く)。
Windows 側はサーバモードでも Tailscale CGNAT(100.64.0.0/10)外の接続元を即拒否します。

## 6.5 音声転送(Windows→Mac)

- 既定で ON。Windows の再生音(既定デバイスのループバック)を f32/48kHz/stereo で
  Mac で再生する(独立ポート 24901)。**低遅延設計(v0.7.2)**: 再生バッファ 10ms×3、
  受信滞留は自動クリップで約 62ms に固定、Windows 取得ポーリング 8ms。
  実効遅延はおおむね 60〜110ms で、時間が経っても増えない(クロック差の蓄積を
  クリップが吸収する)。diag ログの `lag=` が実効滞留遅延
- ON/OFF: Windows=ステータスウィンドウ/トレイメニューの「音声 ON/OFF」、
  Mac=メニューバー「音声転送」。完全無効化は .env に `TSUNAGU_AUDIO=0`
- 帯域: 無音時はキープアライブのみ(約 4B/秒)。鳴っている間は約 384KB/s
- 注意: Windows の**システム通知音**は環境によって既定デバイス以外へ流れる場合がある
  (実機では通知音はごく僅かしか取得できず、メディア再生は全量取得を確認済み)。
  音が来ない場合は Windows のサウンド設定で既定デバイスを確認する
- 逆方向(Win=サーバ)モードでは TSUNAGU_AUDIO_HOST=<Mac側IP> の指定が必要
- **音声出力の集中(接続中スピーカーミュート、既定 ON)**: 接続中は Windows 側の
  スピーカーを自動ミュートし、**Mac のみで音を鳴らす**(二重発音の防止)。
  切断すると元の状態へ自動復元する。Mac メニュー「Windowsスピーカー」で
  「接続中ミュート(Macのみ発音)」⇄「常時鳴らす」を切替(即時反映)。
  Windows のステータス窓にもスピーカー状態を表示。
  無効化は .env に `TSUNAGU_MUTE_SPK=0`(ミュートするとキャプチャも止まる
  環境では OFF にすること。実機ではミュート中もキャプチャ継続を確認済み)

## 6.6 ファイル送信(Mac → Windows)

Deskflow 系の定番機能。**Mac でファイルを ⌘C → 画面端で切替 → Windows で Ctrl+V**:

- Mac 側のクリップボード監視がファイル参照(Finder の ⌘C)を検出すると自動送信される
  (`readObjectsForClasses` で file URL を読むため、Finder 以外のアプリの ⌘C でも動く)
- Windows 側は `Downloads\Tsunagu\` へ保存し、クリップボード(CF_HDROP)へ載せる。
  エクスプローラで Ctrl+V による貼り付けがそのまま使える。受信はバルーン通知で分かる
- メニューバー「Windows へファイルを送る…」からファイル選択ダイアログで選んでも送れる
- 上限: 1回あたり合計 200MB・64ファイル(チャンク分割送信。メイン接続と同じ TCP 24900)
- 同じ選択の再 ⌘C は指紋チェックで再送しない。未接続時は通知して送らない

## 6.7 接続品質表示・Windows 音量制御・キー配置(v0.7)

- **RTT(遅延)表示**: Mac が ping/pong で測定した往復 ms を、Mac メニューの状態行と
  Windows ステータスウィンドウ(「遅延: NNms」)に毎秒表示
- **Windows の音量制御**: Mac メニューバーから「Windows の音量 ▲ / ▼ / ミュート」を送れる
- **⌘キーの行き先**: メニューで Ctrl(既定)⇄ Alt を切替。接続確立時に自動同期される
- **スクロール方向**: メニューで「標準(Windows準拠)⇄ 反転(Mac準拠)」を切替。
  .env は `TSUNAGU_SCROLL_FLIP=1` / `TSUNAGU_CMD_ALT=1` でも指定可

## 7. 検証

```bash
./scripts/verify.sh
```

確認項目: 両プロセス稼働 / ssh 接続 / established / クリップボード双方向 / IME ログ / diag 集計。
実操作(境界切替・cmd+Tab・IME)は手動確認。メニューバー GUI の表示はログの
`[gui] メニューバー常駐を開始しました` で確認できる。

## 8. トラブルシュート

| 症状 | 原因と対処 |
|---|---|
| Mac 起動直後に終了(`[fatal] CGEventTapCreate failed`) | アクセシビリティ権限がない。システム設定で許可して再起動 |
| Mac 起動直後に終了(`[fatal] TSUNAGU_TOKEN が未設定`) | gen-token.sh 未実行 or .env 未封入 |
| `[conn] invalid hello` が続く | トークン不一致。両側の .env を確認 |
| `[fatal] listen ... failed` | Tailscale IP 変更後の TSUNAGU_BIND 指定が旧IPのまま。install-mac.sh を再実行 |
| `[conn] rejected: ... は Tailscale 範囲外です` | 接続元が tailnet 外。Tailscale の接続状態を確認 |
| 接続したのに操作できない | Windows 側が SSH 起動(別デスクトップ)の疑い。schtasks 起動に戻す |
| メニューバーに表示されない | AppKit が使えないセッション(ssh 経由等)。CUI で稼働は継続する。`[gui]` ログを確認 |

## 9. 既知の制限(v0.7 時点)

- Windows の設定ウィンドウはステータス表示と音声/ログ/終了のみ(入力の詳細設定は .env)
- 音声の Windows 側ボリューム追従は未検証(マスター音量が 0 だと無音になる可能性)
- Windows の**システム通知音**は環境によって既定デバイス以外へ流れる場合がある
- ファイル送信は Mac→Windows の一方向(逆は不可。同名ファイルは上書き)
- ディスプレイ構成変更(モニター抜挿)への追従は未実装(起動時の配置で動作)
- Windows 側マルチモニタ非対応(プライマリ画面前提)
- コード署名は ad-hoc(他Macへ配布する場合、初回起動時の右クリック>開く が必要な場合あり)

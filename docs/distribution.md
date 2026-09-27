# Tsunagu 配布手順書

更新日: 2026-09-27。公開用パッケージと、自分の2台へ設定を配る開発運用を分けます。

## 1. 公開用パッケージ

| 対象 | 生成 | 出力 |
|---|---|---|
| Mac | `bash scripts/package-mac.sh` | `dist/Tsunagu-<ver>.zip` と SHA-256 |
| Windows | `bash scripts/package-win.sh` | `dist/Tsunagu-win-<ver>.zip` と SHA-256 |

両スクリプトは `.env`・個人設定・認証トークンを同梱しません。`NO_TOKEN` の指定は不要です。
ビルドは `--locked` で1回実行し、失敗すると終了します。検査に成功するまでは既存ZIPを保持します。
パッケージ内の `release-manifest.json` にバージョン、コミット、未コミット変更の有無、ビルド時刻、実行ファイルのSHA-256を記録します。
`python3 scripts/check-release.py <zip>` で構成、設定・鍵・ログのファイル名、実行ファイルのハッシュを検査できます。
この検査は暗号実装の監査や、あらゆる秘密情報を検出する検査の代替ではありません。

**現状は開発候補版です。** 初回は自動検出したMacを選び、6桁コードで登録します。Macはアドホック署名、Windowsは未署名で、一般販売向けの初回導入と署名・更新経路は未完成です。
商品化の完了条件は [商品化計画](product-readiness.md) を参照してください。

`deploy-win.sh` は自分のWindowsへ認証設定を転送して再起動する開発専用コマンドです。公開配布物の作成には使いません。既存の古いZIPや `win-dist/.env` を第三者に配布しないでください。

## 2. 前提(配布先の環境)

- 両機が同じLAN、直結ネットワーク、または同じTailscaleネットワークに参加。自動発見が届かない環境ではMacのIPを指定
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
- メニューバーに Tsunagu のアイコンが出れば起動完了(未接続の間は「未接続」と
  併記。接続中はアイコンのみで、メニュー内に状態・遅延・履歴件数が出る)
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
- 新規環境では登録画面が開きます。自動復帰・ログオン用の `--background` 起動では、未登録なら静かに終了します。初回はアプリを直接起動してください。

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

- 既定で ON。Windows の再生音(既定デバイスのループバック)を 16bit/stereo(v0.23 から。帯域 192KB/s)で
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

## 6.6 ファイル送信(双方向。v0.23 から専用経路 TCP 24902・暗号化)

Deskflow 系の定番機能。**Mac でファイルを ⌘C → 画面端で切替 → Windows で Ctrl+V**:

- Mac 側のクリップボード監視がファイル参照(Finder の ⌘C)を検出すると自動送信される
  (`readObjectsForClasses` で file URL を読むため、Finder 以外のアプリの ⌘C でも動く)
- Windows 側は `Downloads\Tsunagu\` へ保存し、クリップボード(CF_HDROP)へ載せる。
  エクスプローラで Ctrl+V による貼り付けがそのまま使える。受信はバルーン通知で分かる
- メニューバー「Windows へファイルを送る…」からファイル選択ダイアログで選んでも送れる
- 上限: 1ファイル・1回の合計ともに10GiB(10,737,418,240バイト)。正常に完了した転送の容量は次回に持ち越さない。
  クリップボード経由は最大64ファイル。256KiBずつ専用の暗号化経路 TCP 24902 で送信する。
  Mac・Windowsの両方の更新と、受信先の空き容量が必要。クリップボード画像は引き続き64MiBまで。
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

## 9. 既知の制限(v0.24 時点)

- Windows のロック画面・UAC の確認画面は操作できない(保護デスクトップには SendInput が届かない。
  解消には SYSTEM サービス構成が必要)
- 管理者権限で動くウィンドウへは入力が届かない(UIPI。uiAccess 付きの署名と Program Files への
  配置が必要)
- コード署名は ad-hoc(他 Mac へ配布する場合、初回起動時の右クリック>開く が必要な場合あり)
- ファイアウォール: 逆方向(Windows=サーバ)で使う場合は TCP 24900〜24902 と UDP 24903 の受信許可が必要
- 待受は連続する認証失敗で受け付けが徐々に鈍ります(最大5秒。正規接続の成功で即回復)
- Mac のクリップボード履歴・設定(~/.config/tsunagu)は所有者のみの権限(600/700)で
  保存されます。旧版の 644 ファイルは起動時に自動で 600 へ是正されます

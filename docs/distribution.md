# seamless-desk 配布手順書(v0.6)

作成日: 2026-09-25 / 対象バージョン: 0.6.0(音声転送・Windowsステータスウィンドウ・トレイ常駐・接続方向の選択)

## 1. 配布物の単位

| 対象 | 配布物 | 生成方法 |
|---|---|---|
| Mac | `dist/SeamlessDesk-<ver>.zip`(SeamlessDesk.app、アイコン・トークン同梱) | `scripts/package-mac.sh` |
| Windows | `win-dist/` 一式(sd-win.exe・app.ico・install.bat・run_sd.bat/vbs・.env) | `scripts/deploy-win.sh` 実行後に win-dist を zip 等 |

sd-win.exe にはアプリケーションアイコンが埋め込まれ(windres)、トレイ用に
`app.ico` も同梱される(exe と同じフォルダに置くとトレイが使う)。

トークンの扱い: `~/.config/seamless-desk/env` の `SEAMLESS_DESK_TOKEN` が
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

- ログイン時に自動起動(LaunchAgent `local.seamless-desk`)。ログは /tmp/sd-mac-run.log
- 初回起動時、アクセシビリティ権限の許可を求められたら許可する
  (許可がないと `[fatal] CGEventTapCreate failed` で終了する)
- メニューバーに「SD·Mac / SD·Win / SD·✕」が表示されれば起動完了
- アンインストール: `launchctl unload ~/Library/LaunchAgents/local.seamless-desk.plist`
  → plist と ~/Applications/SeamlessDesk.app を削除

手動起動(.app を使わない): `./scripts/restart-mac.sh`(開発運用。ビルド鮮度保証付き)

## 4. Windows へのインストール

1. `win-dist/` 一式(sd-win.exe・app.ico・install.bat・run_sd.bat・run_sd.vbs・.env)を
   `C:\Users\<user>\seamless-desk` へコピー
2. install.bat を**管理者として実行**(スタートアップ登録のため。以降の起動に管理者権限は不要)
3. **タスクトレイ(通知領域)にアイコンが常駐する**。左クリックでステータスウィンドウ
   (状態/ビルド/音声の表示と「ログを開く」「音声 ON/OFF」「終了」)、右クリックでメニュー

- ログオン時に自動起動(schtasks `seamless_desk` / ONLOGON)
- **自動復帰ウォッチ**(schtasks `seamless_desk_watch` / 5分毎): 何らかの理由で落ちても
  5分以内に自動再起動する(二重起動は exe 内蔵の名前付きミューテックスが即終了させる)
- **コンソール/ターミナル不要**: GUI サブシステム化済みで、exe を直接ダブルクリック
  しても動く(その場合はログは出ない。トレイで状態確認)
- 接続/切断はバルーン通知で可視化される
- ログ: `C:\Users\<user>\seamless-desk\sd-win.log`(1世代ローテーション)
- **.env がないと起動が fatal 停止する**(トークン必須化のため)

## 5. トークン運用

- 生成: `./scripts/gen-token.sh`(~/.config/seamless-desk/env に 256bit ランダム値)
- 参照順序(両バイナリ共通): 環境変数 > 実行ファイル同階層の .env > ~/.config/seamless-desk/env
- Mac と Windows で**同じ値**である必要がある(不一致は `[conn] invalid hello` で接続拒否)
- トークンを変更する場合: 両側の .env を更新して両側を再起動(片方だけ更新すると切断が続く)

## 6. 接続方向の選択(配布先のネットワークに応じて)

既定は **Mac=サーバ(待受)/ Windows=クライアント(接続)** です。
配布先によっては逆方向(Win=サーバ/Mac=クライアント)が好ましい場合もあるため、
両対応しています。**どちらの方向も実機で動作検証済みです**(verify.sh 7項目合格)。

| | 既定(Mac=サーバ) | 逆方向(Win=サーバ) |
|---|---|---|
| Mac | SEAMLESS_ROLE 未設定 | `.env` に `SEAMLESS_ROLE=client` と `SEAMLESS_HOST=<Windows側Tailscale IP>` |
| Windows | SEAMLESS_ROLE 未設定 | `.env` に `SEAMLESS_ROLE=server` |
| 追加作業 | なし(下記参考: Mac の受信は Tailscale IF へ限定推奨) | Windows 側に Tailscale 網限定の受信許可が必要(管理者権限で 1 回): `netsh advfirewall firewall add rule name="seamless-desk-in" dir=in action=allow protocol=TCP localport=24900 remoteip=100.64.0.0/10` |

設定は両側とも .env(環境変数 > exe同階層の .env > ~/.config/seamless-desk/env)。
変更後は**両側の再起動**が必要(片方だけ変えると切断が続く)。
Windows 側はサーバモードでも Tailscale CGNAT(100.64.0.0/10)外の接続元を即拒否します。

## 6.5 音声転送(Windows→Mac)

- 既定で ON。Windows の再生音(既定デバイスのループバック)を f32/48kHz/stereo で
  Mac で再生する(独立ポート 24901。遅延は約 80〜150ms)
- ON/OFF: Windows=ステータスウィンドウ/トレイメニューの「音声 ON/OFF」、
  Mac=メニューバー「音声転送」。完全無効化は .env に `SEAMLESS_AUDIO=0`
- 帯域: 無音時はキープアライブのみ(約 4B/秒)。鳴っている間は約 384KB/s
- 注意: Windows の**システム通知音**は環境によって既定デバイス以外へ流れる場合がある
  (実機では通知音はごく僅かしか取得できず、メディア再生は全量取得を確認済み)。
  音が来ない場合は Windows のサウンド設定で既定デバイスを確認する
- 逆方向(Win=サーバ)モードでは SEAMLESS_AUDIO_HOST=<Mac側IP> の指定が必要

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
| Mac 起動直後に終了(`[fatal] SEAMLESS_DESK_TOKEN が未設定`) | gen-token.sh 未実行 or .env 未封入 |
| `[conn] invalid hello` が続く | トークン不一致。両側の .env を確認 |
| `[fatal] listen ... failed` | Tailscale IP 変更後の SEAMLESS_BIND 指定が旧IPのまま。install-mac.sh を再実行 |
| `[conn] rejected: ... は Tailscale 範囲外です` | 接続元が tailnet 外。Tailscale の接続状態を確認 |
| 接続したのに操作できない | Windows 側が SSH 起動(別デスクトップ)の疑い。schtasks 起動に戻す |
| メニューバーに表示されない | AppKit が使えないセッション(ssh 経由等)。CUI で稼働は継続する。`[gui]` ログを確認 |

## 9. 既知の制限(v0.5 時点)

- Windows の設定ウィンドウはステータス表示と音声/ログ/終了のみ(入力の詳細設定は .env)
- 音声の Windows 側ボリューム追従は未検証(マスター音量が 0 だと無音になる可能性)

- Windows の設定ウィンドウはステータス表示と音声/ログ/終了のみ(入力の詳細設定は .env)
- 音声の Windows 側ボリューム追従は未検証(マスター音量が 0 だと無音になる可能性)
- ディスプレイ構成変更(モニター抜挿)への追従は未実装(起動時の配置で動作)
- Windows 側マルチモニタ非対応(プライマリ画面前提)
- コード署名は ad-hoc(他Macへ配布する場合、初回起動時の右クリック>開く が必要な場合あり)

# seamless-desk 配布手順書(v0.4)

作成日: 2026-09-25 / 対象バージョン: 0.4.0(メニューバー GUI・トークン必須化以降)

## 1. 配布物の単位

| 対象 | 配布物 | 生成方法 |
|---|---|---|
| Mac | `dist/SeamlessDesk-<ver>.zip`(SeamlessDesk.app、LSUIElement=メニューバー常駐型) | `scripts/package-mac.sh` |
| Windows | `win-dist/` 一式(sd-win.exe・install.bat・run_sd.bat/vbs・.env) | `scripts/deploy-win.sh` 実行後に win-dist を zip 等 |

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

1. `win-dist/` 一式(sd-win.exe・install.bat・run_sd.bat・run_sd.vbs・.env)を
   `C:\Users\<user>\seamless-desk` へコピー
2. install.bat を**管理者として実行**(スタートアップ登録のため。以降の起動に管理者権限は不要)
3. タスクトレイには出ない(仕様)。`tasklist | findstr sd-win` で稼働確認

- ログオン時に自動起動(schtasks `seamless_desk` / ONLOGON)
- ログ: `C:\Users\<user>\seamless-desk\sd-win.log`(1世代ローテーション)
- **.env がないと起動が fatal 停止する**(トークン必須化のため)

## 5. トークン運用

- 生成: `./scripts/gen-token.sh`(~/.config/seamless-desk/env に 256bit ランダム値)
- 参照順序(両バイナリ共通): 環境変数 > 実行ファイル同階層の .env > ~/.config/seamless-desk/env
- Mac と Windows で**同じ値**である必要がある(不一致は `[conn] invalid hello` で接続拒否)
- トークンを変更する場合: 両側の .env を更新して両側を再起動(片方だけ更新すると切断が続く)

## 6. 検証

```bash
./scripts/verify.sh
```

確認項目: 両プロセス稼働 / ssh 接続 / established / クリップボード双方向 / IME ログ / diag 集計。
実操作(境界切替・cmd+Tab・IME)は手動確認。メニューバー GUI の表示はログの
`[gui] メニューバー常駐を開始しました` で確認できる。

## 7. トラブルシュート

| 症状 | 原因と対処 |
|---|---|
| Mac 起動直後に終了(`[fatal] CGEventTapCreate failed`) | アクセシビリティ権限がない。システム設定で許可して再起動 |
| Mac 起動直後に終了(`[fatal] SEAMLESS_DESK_TOKEN が未設定`) | gen-token.sh 未実行 or .env 未封入 |
| `[conn] invalid hello` が続く | トークン不一致。両側の .env を確認 |
| `[fatal] listen ... failed` | Tailscale IP 変更後の SEAMLESS_BIND 指定が旧IPのまま。install-mac.sh を再実行 |
| `[conn] rejected: ... は Tailscale 範囲外です` | 接続元が tailnet 外。Tailscale の接続状態を確認 |
| 接続したのに操作できない | Windows 側が SSH 起動(別デスクトップ)の疑い。schtasks 起動に戻す |
| メニューバーに表示されない | AppKit が使えないセッション(ssh 経由等)。CUI で稼働は継続する。`[gui]` ログを確認 |

## 8. 既知の制限(v0.4 時点)

- 設定ウィンドウは未実装(メニューから切替方式・境界到達回数のみ変更可。設定ファイルは .env 形式のみ)
- Mac 側アプリケーションアイコン未添付(LSUIElement のため Dock には出ない)
- ディスプレイ構成変更(モニター抜挿)への追従は未実装(起動時の配置で動作)
- Windows 側マルチモニタ非対応(プライマリ画面前提)
- コード署名は ad-hoc(他Macへ配布する場合、初回起動時の右クリック>開く が必要な場合あり)

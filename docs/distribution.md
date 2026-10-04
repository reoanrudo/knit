# Knit 配布手順書

更新日: 2026-09-27。公開用パッケージと、自分の2台へ設定を配る開発運用を分けます。

## 1. 公開用パッケージ

| 対象 | 生成 | 出力 |
|---|---|---|
| Mac | `bash scripts/package-mac.sh` | `dist/Knit-<ver>.zip` と SHA-256 |
| Windows | `bash scripts/package-win.sh` | `dist/Knit-win-<ver>.zip` と SHA-256 |

両スクリプトは `.env`・個人設定・認証トークンを同梱しません。`NO_TOKEN` の指定は不要です。
ビルドは `--locked` で1回実行し、失敗すると終了します。検査に成功するまでは既存ZIPを保持します。
パッケージ内の `release-manifest.json` にバージョン、コミット、未コミット変更の有無、ビルド時刻、実行ファイルのSHA-256を記録します。
`python3 scripts/check-release.py <zip>` で構成、設定・鍵・ログのファイル名、実行ファイルのハッシュを検査できます。
この検査は暗号実装の監査や、あらゆる秘密情報を検出する検査の代替ではありません。

**現状は開発候補版です。** 初回は自動検出したMacを選び、Macの画面の4桁の確認番号を選んで許可します(自動で見つからないときは6桁コード)。Macはアドホック署名、Windowsは未署名で、一般販売向けの初回導入と署名・更新経路は未完成です。
Mac zip には README-Mac.txt(初回の開き方・権限・登録手順)が同梱されます。

**署名と公証**: `package-mac.sh` は開発者証明書に対応済みです。環境変数
`KNIT_SIGN_IDENTITY`(Developer ID Application の証明書名)がある場合は
hardened runtime 付きで本署名し、`KNIT_NOTARY_PROFILE`(事前に
`xcrun notarytool store-credentials` で登録したプロファイル)がある場合は
公証を提出・承認後にステープルします。どちらも未設定の場合は従来どおり
アドホック署名で動作します(他 Mac では初回に右クリック>開く が必要)。
Windows のコード署名(SmartScreen 対応)には別途証明書の購入と Windows 側での
署名作業が必要です。証明書の購入・契約は開発者の判断と準備が必要です。

商品化の完了条件は [商品化計画](product-readiness.md) を参照してください。

開発環境では自前の配布スクリプト(リポジトリ外)で認証設定を転送して再起動します。公開配布物の作成には使いません。既存の古いZIPや `win-dist/.env` を第三者に配布しないでください。

## 2. 前提(配布先の環境)

- 両機が同じLAN、直結ネットワーク、または同じTailscaleネットワークに参加。自動発見が届かない環境ではMacのIPを指定
- Mac: macOS(arm64)。初回にアクセシビリティ権限の許可が必要(画面録画・入力監視の権限は不要)
- Windows: 対話セッション運用(SendInput の制約)。管理者権限は install.bat 実行時にのみ使用

## 3. Mac へのインストール

```bash
./scripts/package-mac.sh    # dist/ に .app と zip を生成
./scripts/install-mac.sh    # ~/Applications へ配置 + LaunchAgent 登録 + 即時起動
```

- ログイン時に自動起動(LaunchAgent `local.knit`)。ログは /tmp/knit-mac.log
  (起動時と実行中の定期チェックで 5MB を超えていたら 1 世代退避(knit-mac.log.old)
  して切り詰め。実行中は追記)
- 初回起動時、アクセシビリティ権限の許可を求められたら許可する
  (許可がないと `[fatal] CGEventTapCreate failed` で終了する)
- メニューバーに Knit のアイコンが出れば起動完了(未接続の間は「未接続」と
  併記。接続中はアイコンのみで、メニュー内に状態・遅延・履歴件数が出る)
- アンインストール: `launchctl unload ~/Library/LaunchAgents/local.knit.plist`
  → plist と ~/Applications/Knit.app を削除

手動起動(.app を使わない)は開発環境専用スクリプト(リポジトリ外)で行う

## 4. Windows へのインストール

1. `win-dist/` 一式(knit-win.exe・app.ico・install.bat・run_knit.bat・run_knit.vbs・.env)を
   `C:\Users\<user>\knit` へコピー
2. install.bat を**管理者として実行**(スタートアップ登録のため。以降の起動に管理者権限は不要)
3. **タスクトレイ(通知領域)にアイコンが常駐する**。左クリックでステータスウィンドウ
   (状態/ビルド/音声の表示と「ログを開く」「音声 ON/OFF」「終了」)、右クリックでメニュー

- ログオン時に自動起動(schtasks `knit` / ONLOGON)
- **自動復帰ウォッチ**(schtasks `knit_watch` / 毎分): 何らかの理由で落ちても
  1分以内に自動再起動する(二重起動は exe 内蔵の名前付きミューテックスが即終了させる)
- **コンソール/ターミナル不要**: GUI サブシステム化済みで、exe を直接ダブルクリック
  しても動く。**ターミナル/cmd/Windows Terminal から起動した場合も、exe は起動直後に
  コンソールから独立したプロセスへ自動置換されるため、ターミナルを閉じても接続は維持される**
- 接続/切断はバルーン通知で可視化される
- ログ: `C:\Users\<user>\knit\knit-win.log`(起動ごとに 1 世代ローテーションして
  knit-win.log.old へ移し、実行中は追記)
- 新規環境では登録画面が開きます。自動復帰・ログオン用の `--background` 起動では、未登録なら静かに終了します。初回はアプリを直接起動してください。

## 4.5 旧版(Tsunagu v0.25 まで)からの移行と戻し

- **両 OS を同時に更新する**(プロトコルが非互換のため、片側だけ入れ替えても
  接続できない)。初回登録(ペアリング)は識別子・キー形式の変更によりやり直し
- 設定(~/.config/tsunagu → ~/.config/knit)と Windows の端末データ
  (%LOCALAPPDATA%\Tsunagu → Knit)は初回起動時に複製で引き継ぐ(旧フォルダは残る)。
  Windows の .env は install.bat が引き継ぐ
- **Windows**: install.bat が旧タスク(tsunagu 系)の停止・削除、旧プロセスの終了、
  旧 .env の引き継ぎまで行う。利用者の手動作業は不要
- **Mac**: 公開 zip に install-mac.sh は同梱されないため、旧版が残っている Mac では
  先に旧版を止めてもらうこと(旧版が listen したままだと Knit が
  `[fatal] listen ... failed` で即終了する)。開発用 install-mac.sh が行う内容の
  手動版:
  1. メニューバーの Tsunagu を終了
  2. システム設定「一般」→「ログイン項目と拡張機能」から Tsunagu を削除
     (開発運用の LaunchAgent `local.tsunagu` が残っていれば
     `launchctl unload ~/Library/LaunchAgents/local.tsunagu.plist` 実行後に
     plist を削除)
  3. Tsunagu.app をゴミ箱へ(旧設定 ~/.config/tsunagu は残る)
- **旧版へ戻す**: Mac はゴミ箱から Tsunagu.app を「戻す」して開く(必要なら上記 2 の
  自動起動を再登録)。Windows は `%USERPROFILE%\tsunagu` の install.bat を再実行
  (旧タスクを再登録して起動する)

## 5. トークン運用

- 生成: `./scripts/gen-token.sh`(~/.config/knit/env に 256bit ランダム値)
- 参照順序(両バイナリ共通): 環境変数 > 実行ファイル同階層の .env > ~/.config/knit/env
- **Mac の .app で .env を使う場合の注意**: 署名済み .app へ後から .env を
  Contents/Resources/ へ置くとコード署名の検証が壊れます(sealed resource)。
  トークンは ~/.config/knit/env へ置くのが最も簡単です。
  .app 内へ同梱したい場合は配置後に `codesign --force --sign - Knit.app`
  で再署名すると検証が通ります(ad-hoc 署名のためローカルで再署名可)
- Mac と Windows で**同じ値**である必要がある(不一致は `[conn] invalid hello` で接続拒否)
- トークンを変更する場合: 両側の .env を更新して両側を再起動(片方だけ更新すると切断が続く)

## 6. 接続方向の選択(配布先のネットワークに応じて)

既定は **Mac=サーバ(待受)/ Windows=クライアント(接続)** です。
配布先によっては逆方向(Win=サーバ/Mac=クライアント)が好ましい場合もあるため、
両対応しています。**どちらの方向も実機で動作検証済みです**(開発環境の自動検証 7 項目に合格)。

| | 既定(Mac=サーバ) | 逆方向(Win=サーバ) |
|---|---|---|
| Mac | KNIT_ROLE 未設定 | `.env` に `KNIT_ROLE=client` と `KNIT_HOST=<Windows側Tailscale IP>` |
| Windows | KNIT_ROLE 未設定 | `.env` に `KNIT_ROLE=server` |
| 追加作業 | なし(待受は全インターフェース。受信の防御は接続元制限と暗号化が担う。※ Tailscale IP へ限定すると AP 隔離の環境で LAN 直の受け口が消え、Windows の経路昇格が成功しないため再接続ループになる実績あり) | Windows 側に Tailscale 網限定の受信許可が必要(管理者権限で 1 回): `netsh advfirewall firewall add rule name="knit-in" dir=in action=allow protocol=TCP localport=24900 remoteip=100.64.0.0/10` |

設定は両側とも .env(環境変数 > exe同階層の .env > ~/.config/knit/env)。
変更後は**両側の再起動**が必要(片方だけ変えると切断が続く)。
Windows 側はサーバモードでも Tailscale CGNAT(100.64.0.0/10)外の接続元を即拒否します。

## 6.5 音声転送(Windows→Mac)

- 既定で ON。Windows の再生音(既定デバイスのループバック)を 16bit/stereo(v0.23 から。帯域 192KB/s)で
  Mac で再生する(独立ポート 24901)。**低遅延設計**: 再生バッファ 15ms×4=60ms、
  受信側のプリロールと滞留目標は約 43ms(16KB)、緊急クリップ上限は約 171ms(64KB)、
  Windows 取得は 50ms バッファを 8ms ポーリング。
  実効遅延はおおむね 90〜130ms(デバイスとネットワークによる)で、時間が経っても
  増えない(クロック差の蓄積を滞留調整が吸収する)。diag ログの `lag=` が実効滞留遅延。
  Wi-Fi が不安定なときは古い音から捨てられるため音が途切れることがある
- ON/OFF: Windows=ステータスウィンドウ/トレイメニューの「音声 ON/OFF」、
  Mac=メニューバー「音声転送」。完全無効化は .env に `KNIT_AUDIO=0`
- 音量: Mac の設定「共有」の「再生音量」スライダ(受信サンプルへのソフトゲイン。
  0〜200%)。Mac の音量キーは Windows 操作中は Windows 側へ転送されるため、
  Knit 内で完結する調整口として用意。初期値は .env の `KNIT_AUDIO_GAIN`(0.0〜2.0)
- 帯域: 無音時はキープアライブのみ(約 4B/秒)。鳴っている間は約 192KB/s(s16/stereo)
- 注意: Windows の**システム通知音**は環境によって既定デバイス以外へ流れる場合がある
  (実機では通知音はごく僅かしか取得できず、メディア再生は全量取得を確認済み)。
  音が来ない場合は Windows のサウンド設定で既定デバイスを確認する
- 逆方向(Win=サーバ)モードでは KNIT_AUDIO_HOST=<Mac側IP> の指定が必要
- **音声出力の集中(接続中スピーカーミュート、既定 ON)**: 接続中は Windows 側の
  スピーカーを自動ミュートし、**Mac のみで音を鳴らす**(二重発音の防止)。
  切断すると元の状態へ自動復元する。Mac メニュー「Windowsスピーカー」で
  「接続中ミュート(Macのみ発音)」⇄「常時鳴らす」を切替(即時反映)。
  Windows のステータス窓にもスピーカー状態を表示。
  異常終了(強制終了・電源断)でミュートが残った場合も、Windows 側は次回起動時に
  退避記録(端末固有データ置き場の speaker-mute.json)から検知して自動復元する。
  本線だけが瞬断した場合(音声ストリームが生きている間)は二重発音防止のため
  ミュートを維持し、音声線も切れた後に復元する
  無効化は .env に `KNIT_MUTE_SPK=0`(ミュートするとキャプチャも止まる
  環境では OFF にすること。実機ではミュート中もキャプチャ継続を確認済み)

## 6.6 ファイル送信(双方向。v0.23 から専用経路 TCP 24902・暗号化)

Deskflow 系の定番機能。**Mac でファイルを ⌘C → 画面端で切替 → Windows で Ctrl+V**:

- Mac 側のクリップボード監視がファイル参照(Finder の ⌘C)を検出すると自動送信される
  (`readObjectsForClasses` で file URL を読むため、Finder 以外のアプリの ⌘C でも動く)
- Windows 側は `Downloads\Knit\` へ保存し、クリップボード(CF_HDROP)へ載せる。
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
  .env は `KNIT_SCROLL_FLIP=1` / `KNIT_CMD_ALT=1` でも指定可

## 7. 検証

```bash
(開発環境専用の検証スクリプト(リポジトリ外)で実施)
```

確認項目: 両プロセス稼働 / ssh 接続 / established / クリップボード双方向 / IME ログ / diag 集計。
実操作(境界切替・cmd+Tab・IME)は手動確認。メニューバー GUI の表示はログの
`[gui] メニューバー常駐を開始しました` で確認できる。

## 8. トラブルシュート

| 症状 | 原因と対処 |
|---|---|
| Mac 起動直後に終了(`[fatal] CGEventTapCreate failed`) | アクセシビリティ権限がない。システム設定で許可して再起動 |
| Mac 起動直後に終了(`[fatal] KNIT_TOKEN が未設定`) | gen-token.sh 未実行 or .env 未封入 |
| `[conn] invalid hello` が続く | トークン不一致。両側の .env を確認 |
| `[fatal] listen ... failed` | Tailscale IP 変更後の KNIT_BIND 指定が旧IPのまま。install-mac.sh を再実行 |
| `[conn] rejected: ... は Tailscale 範囲外です` | 接続元が tailnet 外。Tailscale の接続状態を確認 |
| 接続したのに操作できない | Windows 側が SSH 起動(別デスクトップ)の疑い。schtasks 起動に戻す |
| メニューバーに表示されない | AppKit が使えないセッション(ssh 経由等)。CUI で稼働は継続する。`[gui]` ログを確認 |

## 9. 既知の制限(v0.24 時点)

- Windows のロック画面・UAC の確認画面は操作できない(保護デスクトップには SendInput が届かない。
  解消には SYSTEM サービス構成が必要)
- 管理者権限で動くウィンドウへは入力が届かない(UIPI。uiAccess 付きの署名と Program Files への
  配置が必要)
- コード署名は現状 ad-hoc(他 Mac へ配布する場合、初回起動時の右クリック>開く が必要。
  zip 同梱の README-Mac.txt に手順を記載)。Developer ID 署名と公証は
  package-mac.sh が対応済みで、証明書入手後に環境変数を設定するだけで有効化できる
- Windows は未署名のため初回起動時に SmartScreen の警告が出る場合がある
  (「詳細情報」→「実行」で進める。解除にはコード署名証明書の購入が必要)
- ファイアウォール: 逆方向(Windows=サーバ)で使う場合は TCP 24900〜24902 と UDP 24903 の受信許可が必要
- 待受は連続する認証失敗で受け付けが徐々に鈍ります(最大5秒。正規接続の成功で即回復)
- Mac のクリップボード履歴・設定(~/.config/knit)は所有者のみの権限(600/700)で
  保存されます。旧版の 644 ファイルは起動時に自動で 600 へ是正されます

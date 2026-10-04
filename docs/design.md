# knit 設計書 (Phase 1)

## 目的

MacBook のキーボード/トラックパッドで、同一ネットワーク上の Windows デスクトップを
Deskflow と同じ感覚(画面端で切替)で操作する。Phase 1 は入力共有のみ。

## POC で確定した事実

| 項目 | 結果 |
|---|---|
| Mac CGEventTap フック | 成功。アクセシビリティ権限済み。合成イベント捕捉も確認 |
| Windows SendInput | 成功。ただし対話セッション起動が必須 |
| OpenInputDesktop | SSH 起動では不可 / schtasks 対話起動なら可。アクセス権 0x01FF で成功 |
| Windows クリップボード | SSH/対話どちらでも読み書き可(Phase 2 で使用) |
| 画面解像度(Windows) | 1920x1080(対話セッション)。SSH セッションは 1024x768 の別デスクトップ |
| 通信経路 | Tailscale(100.84.0.2)RTT 平均 6.3ms。LAN 直(192.168.0.2)は AP 隔離で不通 |
| ビルド | Mac(cargo aarch64-apple-darwin)× Windows(x86_64-pc-windows-gnu クロスビルド+scp 配布)。Windows 側 cargo は rustup 実行不可のため不使用 |

## アーキテクチャ(実装で確定: 逆転構成)

```
MacBook (knit-mac = サーバ)              Windows Desktop (knit-win = クライアント)
┌────────────────────┐   Tailscale   ┌─────────────────────────┐
│ TcpListener(:24900) │   TCP :24900   │ connect + 再接続ループ  │
│ CGEventTap(フック)  │ ←───────────  │  hello 送信             │
│  → イベント変換     │  JSON Lines   │  → キーマップ変換       │
│  → JSON Lines 送信  │ ───────────→  │  → SendInput 注入       │
│ モード管理(Mac/Win) │ ←───────────  │ カーソル左端監視→Return │
└────────────────────┘               └─────────────────────────┘
```

### なぜ Windows 発(クライアント)接続か(実測で判明した環境特性)

本環境(Tailscale + 同一 WiFi、AP 隔離)では:
- **Mac が TCP を開く(Mac→Win connect)と、最初の1パケットしか通らず以降が落ちる**
- **Windows が TCP を開く(Win→Mac connect)と、双方向・複数パケットとも完全に安定**
(双方 PowerShell/Python/nc での隔離テストで実証。Tailscale ping は常に 4ms で健全)

このため Mac=サーバ(Listen)、Win=クライアント(connect+再接続)の逆転構成を採用した。
hello は Win が送り、Mac が hello_ok で応答する。

- 通信: 生 TCP + JSON Lines(serde_json)。Tailscale が WireGuard 暗号化を持つため
  Phase 1 では TLS 省略。事前共有トークンで認証。

## 切替ロジック

- Mac モード → Windows モード: Mac カーソルが画面右端に到達
  - 以後、Mac 側イベントを握りつぶし(return NULL)Windows へ転送
  - マウスは相対 delta(kCGMouseEventDeltaX/Y)を送る
- Windows モード → Mac モード:
  - Windows カーソルが左端 (x<=0) に達したら knit-win が `return` 通知
  - またはホットキー F13(常に有効)
- ホットキー F13: モード切替(双方向)

## キーマッピング(Mac → Windows)

- Cmd → Ctrl / Option → Alt / Control → Win キー
- Mac keycode(HIToolbox)→ Windows VK 変換テーブル(common に保持)
- 修飾キー置換により Cmd+C → Ctrl+C 等が自動的に成立

## プロトコル(common crate, JSON Lines・版 11)

本線 TCP 24900。全経路 Noise NNpsk0 で暗号化(ラベル main/audio/bulk で経路識別)。

| 型 | 方向 | フィールド | 用途 |
|---|---|---|---|
| hello | W→M | ver, name, token(版11は空), w, h | 接続時の版交渉と画面サイズ交換 |
| hello_ok | M→W | name, w, h | 受理 |
| screen | W→M | w, h | 解像度・モニター構成の変化通知 |
| rel | W→M | on | ゲームモード(相対移動切替)要求 |
| lock | M→W | - | Mac の画面ロック連動 |
| key | M→W | kc, down, ctrl, opt, cmd, shift, tr | キー(tr=翻訳済み) |
| mouse_move / mouse_abs | M→W | dx,dy / nx,ny | 相対移動 / 絶対位置(0..1) |
| mouse_btn | M→W | btn(0-2、3/4=戻る/進む), down | ボタン |
| scroll | M→W | dx, dy | スクロール |
| return | W→M | ny | Windows 側境界到達での復帰 |
| clip | 双方向 | text | クリップボード同期(画面を移る時) |
| leave | M→W | - | Mac へ戻った(全入力解放と Win→Mac 同期の合図) |
| warp | M→W | nx, ny | 切替時のカーソル位置引継ぎ |
| ping / pong | 双方 | ts | 生存確認・RTT 測定(3 秒毎) |
| cfg | M→W | cmd_alt, spk_mute, side, clip | 設定同期 |
| vol | M→W | op(0-2 音量/3-5 メディア) | 音量・メディアキー転送 |
| stat | M→W | rtt | 接続品質の表示用 |
| ime | M→W | kana | IME 状態引継ぎ(IME Follow Cursor) |
| open_url | M→W | url | Continue Here(⌥⌘T) |
| bye | 双方 | - | 終了 |

**拡張ポリシー(後方互換)**: 新しいメッセージ型を足しても旧側は decode に
失敗した行を無視するだけで壊れない(版番号を上げない拡張は片側配備が可能。
ime/open_url はこの方式)。既存型へのフィールド追加は `#[serde(default)]`
付きのみ(旧側の送信を新側が読める)。画像・ファイルは本線を経由せず
bulk 24902 へ、音声は 24901 へ分離(本線の入力遅延を守る)。

## 再接続・安定性

- Mac: 接続断を検知したら指数バックオフ(0.5s→1s→2s→max5s)で再接続
- Heartbeat: 5 秒毎 ping。15 秒無応答で切断扱い
- Windows モード中に切断したら即 Mac モードへ復帰(入力閉じ込め防止)

## Windows 起動方式(重要)

SSH 起動プロセスは入力デスクトップに接続できないため:
- `schtasks /Create /SC ONLOGON /TN knit /TR <exe>` でログオン時対話起動
- 開発中は `/SC ONCE` タスクを `schtasks /Run` で都度起動(POC で実証済み)
- exe 起動直後に `OpenInputDesktop(0, 0, 0x01FF)` + `SetThreadDesktop` を実行
- ファイアウォール: `netsh advfirewall` で TCP 24900 を許可(install.bat で実施)

## 成果物構成

```
knit/
  Cargo.toml            (workspace)
  crates/
    common/             プロトコル・キーコード表
    mac/                knit-mac: CGEventTap・TCP クライアント・モード管理
    win/                knit-win: TCP サーバ・SendInput 注入・Return 監視
  win-dist/
    install.bat         配布+スタートアップ登録+ファイアウォール許可
    run_knit.bat          対話起動用(開発時 schtasks 経由)
  docs/design.md        本書
  poc/                  検証コード(参照用)
```

## 開発運用(ssh 経由)

1. Mac: `cargo build --release`→ knit-mac 実行
2. Win: `cargo build --target x86_64-pc-windows-gnu --release`
3. `scp` で exe 配布 → `schtasks /Run` で対話起動
4. ログ: 両側カレントディレクトリに `sd-*.log` + stdout

## フェーズ計画

- Phase 1: 入力共有・切替・キーマップ・再接続(本書)
- Phase 2: クリップボード双方向同期(POC 済み。CF_UNICODETEXT+NSPasteboard)
- Phase 3: ファイル転送(ドラッグ&ドロップ)
- Phase 4: AI 操作(画面キャプチャ→LLM→入力実行)

## 残リスク

- Windows 側フォアグラウンドが UAC 昇格プロセスだと UIPI で SendInput が弾かれる
  (対処: タスクを /RL HIGHEST で登録する選択肢)
- Tailscale の遅延変動(WiFi 状況次第)。有線 Tailscale/直接 LAN は将来検討
- Mac 側キーリピート・日本語IMEの状態同期は未対応(Phase 1 は英語入力前提)
- 受信ファイル名は Windows の実制限(コンポーネント 255・パス 260 UTF-16 units)
  に合わせて切り詰めるが、exe への longPathAware マニフェスト付与は見送り
  (影響が大きいため将来課題)

## マウス絶対位置送信モード(2026-09-25 追加)

Mac の加速済み delta に Windows のポインタ加速が二重に乗るのを避けるため、
WIN モード中は Mac 側が Windows 画面の仮想カーソル(px, f64)を管理し、
正規化座標 MouseAbs を毎イベント送信する。Windows 側は MOUSEEVENTF_ABSOLUTE
で注入(加速曲線を通らない)。画面比率の見た目距離は方向別スケール
(win_w/mac_w, win_h/mac_h)で自動補正。左端到達の復帰は Mac 内完結で即時。
KNIT_MOUSE_MODE=rel で従来の相対移動に切替可能。

# seamless-desk

Deskflow 快適版。MacBook のキーボード/トラックパッドで Windows デスクトップを操作する。
Phase 1(入力共有)が動作済み。設計は [docs/design.md](docs/design.md) 参照。

## 構成

- **Mac = サーバ**(`sd-mac`): CGEventTap で入力をフックし TCP で転送。画面右端で切替
- **Windows = クライアント**(`sd-win`): 受信イベントを SendInput で注入。カーソル左端で復帰通知
- 経路: Tailscale(Mac: 100.100.10.9 / Win: 100.84.0.2)、TCP 24900、トークン認証

## 使い方

### Windows 側(初回 1 回だけ)

```bat
# Mac から配布して実行(スタートアップ登録+起動)
ssh home "C:\Users\<user>\seamless-desk\install.bat"
```

- ログオン時に自動起動(schtasks seamless_desk / ONLOGON)
- 対話セッションでの起動が必須(SendInput のため)。SSH 直接実行では入力不可

### Mac 側

```bash
cd ~/ZCodeProject/seamless-desk
cargo build --release
./target/release/sd-mac          # サーバ起動(アクセシビリティ権限が必要)
```

### 操作

- **Mac→Windows**: カーソルを画面右端へ → 入力が Windows へ
- **Windows→Mac**: Windows のカーソルを左端へ、または F13 キーでトグル
- **ショートカット**: Mac の Cmd は Windows の Ctrl に自動変換(Cmd+C→Ctrl+C)

## ビルド

```bash
cargo build --release                                     # Mac 側
cargo build --release -p sd-win --target x86_64-pc-windows-gnu   # Windows 側(クロス)
```

## Windows 側の更新手順(開発時)

```bash
# 実行中 exe はロックされるので 停止→配布→起動 の順を守る
ssh home "schtasks /End /TN seamless_desk_run & taskkill /IM sd-win.exe /F"
scp target/x86_64-pc-windows-gnu/release/sd-win.exe home:C:/Users/<user>/seamless-desk/
ssh home "schtasks /Run /TN seamless_desk_run"
# ログ: ssh home "type C:\Users\<user>\seamless-desk\sd-win.log"
```

## 今後のフェーズ

- Phase 2: クリップボード双方向同期(POC 済み)
- Phase 3: ファイル転送(ドラッグ&ドロップ)
- Phase 4: AI 操作(画面キャプチャ→LLM→入力)

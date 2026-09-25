# Tsunagu エージェント作業ガイド(AI/自動化向け)

このファイルは、AI エージェント(または自動化スクリプト)が本プロジェクトを安全に扱うための
最重要事項を集約したもの。経緯は CHANGELOG.md と docs/improvement-log.md 参照。

## プロジェクトの本質

「Mac のキーボードとトラックパッドで Windows を操作し、クリップボードと音を繋ぐ」。
機能の取捨はこの一文で判定する。

## 構成と定数

- **Mac = サーバ**(`tsunagu-mac`, TCP 24900 受信)/ **Win = クライアント**(`tsunagu-win`, 接続ループ)。
  逆転構成は環境固有(本環境は Mac 発 TCP が不通なため)。`TSUNAGU_ROLE=server` で反転可
- 音声: Win の WASAPI ループバック → TCP 24901 → Mac AudioQueue(f32/48k/stereo)
- プロトコル: JSON Lines、`crates/common/src/lib.rs` の `Msg`。**両側同時更新が前提**
  (VERSION を上げて混在検知)
- 実機: ssh ホスト名 `home`(Windows)/ Tailscale(Mac 100.100.10.9, Win 100.84.0.2)

## ワンコマンド(改善ループの標準手順)

```bash
./scripts/dev.sh            # 両OS ビルド→配備→verify まで
./scripts/dev.sh --no-win   # Mac 側だけ変えた時
./scripts/restart-mac.sh [args...]   # Mac 再起動(--show-prefs で設定窓自動表示)
./scripts/deploy-win.sh             # Win 配備(旧タスク掃除付き)
./scripts/verify.sh                 # 自動検証(15 項目。WARN は fail 扱いでない)
```

ログ: Mac `/tmp/tsunagu-mac.log`、Win `ssh home "type C:\Users\<user>\tsunagu\tsunagu-win.log"`。

## 環境固有の罠(実績バグ集 — 絶対に再発させない)

1. **ssh 先は cmd.exe**: 複数コマンドは `&` 区切り。`;` は区切りと解釈されず
   **黙って失敗**する(旧タスク掃除が機能しない実績)
2. **ssh 由来の Set-Clipboard は対話セッションのクリップボードに届かない**(セッション分離)。
   クリップボード検証は schtasks 経由で対話実行すること(verify.sh 参照)
3. **cargo の差分検知が sed と同秒 mtime で miss** し「古い exe を配る」ことがある。
   deploy-win.sh が strings で BUILD_ID を検証・不一致なら強制再ビルドする
4. **配布中、毎分の watch タスクが旧 exe を起こす競合**がある。deploy は配布中 watch を
   DISABLE し完了後に戻す
5. Windows の実行中 exe はロックされる。taskkill → sleep → scp の順を守る
6. **objc セレクタ名は実在確認してから書く**(swift -e で1行検証)。
   実績: `labelWithString:`(≠labelWithTitle:)、`checkboxWithTitle:`(≠checkWithTitle:)。
   未認識セレクタは NSException → Rust の "panic in a function that cannot unwind" で abort
7. **objc の構造体渡し**: NSRect(f64×4)は repr(C) の型付き transmute で OK(HFA)。
   ただし `setContentMinSize:` の引数は **NSSize(f64×2)** — fn(ID,SEL,f64,f64) で渡す
   (NSRect のまま渡すと窓が 0x0 に潰れる)
8. `NSFont boldSystemFontOfSize:` は**クラスメソッド**(インスタンスに送らない)
9. **SendInput のキー注入は vk + scan(MapVirtualKeyW)併用**が IME 互換の鉄則
   (scan 無しは日本語 IME が無視/不安定。「ー」キー事故の実績)
10. Mac keycode の数字row: **18=1, 19=2, 20=3, 21=4, 23=5, 22=6, 26=7, 28=8, 25=9, 29=0**
    (21=4 と 23=5 を取り違えると ⌘⇧3/⌘⇧5 が効かない — 実績)
11. **PCM 音声は 8 バイト(f32×2ch)境界**を厳守。境界外ドロップは恒久位相ずれ=破壊音
12. トークン(TSUNAGU_TOKEN)は画面出力しない。`.env`/`dist/` は gitignore 済み
13. 音が出る E2E テストはユーザーの事前承認が必要(無断トーン事故の実績)

## コード規約(このプロジェクト固有)

- objc は依存追加なしの「objc_msgSend 固定シグネチャ transmute」方式。ヘルパは
  main.rs の msg0/msg1 系、gui.rs のローカル sel()
- 設定値は AtomicU64(f64 ビット)で実行中可変(スクロール除数/カーソル速度/境界閾値)
- Win 側の状態は static Atomic + 単一 writer スレッド(mpsc)構成。並行書き込みを足さない
- コメントは「なぜそうするか」のみ書く(日本語)

## 既知の制限(今後の課題)

- 境界判定の左/上/下端はメイン画面基準(右端のみ union 対応)。左にサブモニタが
  ある環境では左端判定が全域ヒットする。union の min/max を起動時計算して gap へ
  使う修正が望ましい(レビュー #7)
- キー送信の E2E(⌘]/ドラッグ切替)は手動検証領域。自動化するなら schtasks 経由で
  キーログを取る検証を追加する

## 主なドキュメント

- 利用者向け全設定: docs/usage.md
- 配布手順: docs/distribution.md
- 履歴: CHANGELOG.md / docs/improvement-log.md
- 設計詳細: docs/design.md / レビュー: docs/reviews/

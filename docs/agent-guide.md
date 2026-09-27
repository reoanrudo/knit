# Tsunagu エージェント作業ガイド(AI/自動化向け)

このファイルは、AI エージェント(または自動化スクリプト)が本プロジェクトを安全に扱うための
最重要事項を集約したもの。経緯は CHANGELOG.md と docs/improvement-log.md 参照。

## プロジェクトの本質

「Mac のキーボードとトラックパッドで Windows を操作し、クリップボードと音を繋ぐ」。
機能の取捨はこの一文で判定する。

## 構成と定数

- **Mac = サーバ**(`tsunagu-mac`, TCP 24900 受信)/ **Win = クライアント**(`tsunagu-win`, 接続ループ)。
  逆転構成は環境固有(本環境は Mac 発 TCP が不通なため)。`TSUNAGU_ROLE=server` で反転可
- 経路: 本線 TCP 24900(JSON Lines の `Msg`)/ 音声 TCP 24901(s16 PCM)/
  ファイル・画像 TCP 24902(`common::bulk` のバイナリフレーム)/ LAN 自動発見 UDP 24903(`common::discover`)
- **全 TCP 経路は `common::secure`(Noise NNpsk0)で包む**。平文で読み書きしない。
  `secure::Writer` は flush で送信されるため、書いたら必ず flush する
- プロトコル: `crates/common/src/lib.rs` の `Msg`。`MIN_VERSION` 以上なら接続を受け入れ、
  未知のメッセージは無視される。互換を壊す変更は MIN_VERSION も上げる(両側同時更新)
- クリップボードは「画面を移る時」だけ同期する(Mac: enter_win_mode、Win: Leave 受信)。
  コピー毎に送る実装へ戻さない(大容量ファイルの無駄な転送・秘匿データ流出の原因)
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
14. **ドラッグ用ペーストボード(NSPasteboardNameDrag)はキャンセル後もクリアされず
    残ることがある**(実測)。掴み判定は「押下開始時点からの changeCount 変化」基準に
    すること(crates/mac/main.rs の掴み検出スレッド)。swift の writeObjects で載せる
    URL はファイルが実在しないと readObjectsForClasses(FileURLsOnly)で読めない
15. **CGEventPost したイベントは自分の HID タップを再通過する**(定番の再帰)。
    自己投稿は kCGEventSourceUserData(41) にマジックを刻み tap 側で識別する
    (掴み切替直後の Mac 完結用 LeftMouseUp = SYNTH_UP_MAGIC)
16. **windows-sys に COM インターフェースの vtbl は無い**→自前定義
    (crates/win/src/dragdrop.rs の IDataObject/IDropSource/IEnumFORMATETC)。
    vtbl の並びは MSDN のメソッド順どおり。並びを間違えると即クラッシュ
17. **static Mutex の guard を保持したまま、同じ Mutex を取る関数を呼ぶと
    自己デッドロック**(std::sync::Mutex は再入不可。実績: 受信履歴テストが
    guard 保持中に push_recent_rx を呼び、テストが永久ブロック)。テストでは
    scope を切って guard を確実に落としてから関数を呼ぶ
18. **Link::send はクロージャを消費する(FnOnce)**。再試行で同じクロージャを
    2 回使いたい時は生成を関数化/macro で書く(転送の進捗ログの実績)
19. **かな(kc=104)/英数(kc=102)は win 側の受信ループが先に傍受して IME 開閉
    (ime_set_open)へ変換する**。keymap(mac_kc_to_win_vk)に対応を足しても
    到達しない二重経路になるだけ(実績: VK_DBE_* 追加を撤去)。
    「keymap に無い = 捨てられている」と読み飛ばさず、受信ループの傍受を確認する
20. **sed の置換先テキスト内の `&` は「マッチした全体」に展開される**
    (エスケープは `\&`)。`&str` を含む Rust 行を sed で書き換える時に
    `s/.../const X: \&str = "..."/` とエスケープを忘れると、行が自己複製した
    ような破壊行になる(実績: BUILD_ID 一時差し戻しで `const BUILD_ID: const
    BUILD_ID: …` となりビルドが壊れた)。**改善ループ467(97dd5d6)に破壊行が
    入ったままコミットされており bisect でこの 1 点だけビルドが失敗する**
    (468 で自然修復済み・履歴修正はしない)。deploy-win.sh のスタンプは正しく
    `\&` 済み(壊れた行も最初の `;` までマッチして正常化するため自己修復する)
21. **AppleScript に複数アプリの tell を並べた複合スクリプトは、インストール
    されていないアプリ(この Mac の Microsoft Edge 等)の用語解決で構文エラー
    (-2741)になる**。文字列を組み立てたら必ず osascript で実行検証を通す
    (実績: Continue Here の 477 が未検証のまま壊れ、492 で「前面アプリ名を
    取ってから該当アプリ専用の単文だけ実行」の 2 段階に再構成して根絶)。
    2 段階なら tell 先は必ず起動中=インストール済みのため用語解決が成功する

## コード規約(このプロジェクト固有)

- objc は依存追加なしの「objc_msgSend 固定シグネチャ transmute」方式。ヘルパは
  main.rs の msg0/msg1 系、gui.rs のローカル sel()
- 設定値は AtomicU64(f64 ビット)で実行中可変(スクロール除数/カーソル速度/境界閾値)
- Win 側の状態は static Atomic + 単一 writer スレッド(mpsc)構成。並行書き込みを足さない
- コメントは「なぜそうするか」のみ書く(日本語)

## 既知の制限(今後の課題)

- Windows のロック画面・UAC 画面は操作不可(SYSTEM サービス構成が必要)。Windows 側は DPI 非対応
  のまま(DPI 対応にするとトレイ/設定窓の描画倍率が変わるため GUI 側と同時に行う)
- 実機での未検証項目: 暗号化後の 3 経路の疎通、ゲームモード、ロック連動、Mac→Win 画像の
  CF_DIB 互換性(BMP 書き出しの V4/V5 ヘッダを受け付けないアプリがある可能性)
- キー送信の E2E(⌘]/ドラッグ切替)は手動検証領域。自動化するなら schtasks 経由で
  キーログを取る検証を追加する

## 主なドキュメント

- 利用者向け全設定: docs/usage.md
- 配布手順: docs/distribution.md
- 履歴: CHANGELOG.md / docs/improvement-log.md
- 設計詳細: docs/design.md / レビュー: docs/reviews/

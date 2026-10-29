# 仕様: Mac のスリープ/復帰を OS の通知で扱い、一時的な目覚め(Power Nap 等)では接続しない

- 書いた人: lead エージェント(2026-10-29)。参考 OSS のソースを読んでいるため実装には関わらない
- 実装する人: 参考 OSS のソースを読んでいないエージェント(Codex / gpt-6.1-terra)
- 根拠とした資料(これ以外を根拠にしない):
  - macOS SDK のヘッダ(`xcrun --show-sdk-path` 配下。実装者も直接読んでよい)
    - `IOKit.framework/Headers/pwr_mgt/IOPMLib.h` の `IORegisterForSystemPower` / `IOAllowPowerChange`
      の説明(各メッセージの意味と、応答が必要なもの・不要なもの)
    - `IOKit.framework/Headers/IOMessage.h` の `kIOMessageSystemWillSleep` /
      `kIOMessageSystemWillPowerOn` / `kIOMessageSystemHasPoweredOn` / `kIOMessageCanSystemSleep`
    - `AppKit.framework/Headers/NSWorkspace.h` の `NSWorkspaceWillSleepNotification` /
      `NSWorkspaceDidWakeNotification` / `NSWorkspaceScreensDidWakeNotification`
    - `CoreGraphics.framework/Headers/CGDisplayConfiguration.h` の `CGDisplayIsAsleep`
  - Apple の公開ドキュメント(Power Nap は画面を点けずに短時間だけ起き、ネットワーク処理などを行う)
  - Knit 自身のコード(`crates/mac/src/main.rs` の送信スレッドのスリープ検知、
    `crates/mac/src/session.rs` の待受/再接続ループ、`crates/common/src/retry.rs` の `Wakeable`)

## 解決したい問題

現状の Mac 側は、送信スレッドで「壁時計と単調時計の進みの差」を見てスリープ明けを推定し
(`slept_duration`)、接続を張り直している。この方式には次の弱点がある。

1. **眠る直前を知らない。** 相手から見ると突然無応答になり、相手側は生存確認のタイムアウトまで
   待ってから切断を判断する。
2. **一時的な目覚めと本当の復帰を区別できない。** スリープ中の Mac は Power Nap などで数秒〜数十秒
   だけ起きることがある(画面は消えたまま)。このとき再接続・待受を受け入れると、すぐまた眠って
   接続が無言で死に、相手側の再接続の試行と失敗が繰り返される。
3. 推定は「送信スレッドが200ms ごとに回っている」ことに依存しており、間接的。

## やること

### A. OS の電源通知を受け取る

`IORegisterForSystemPower` で電源通知を受け取る専用スレッドを1本立てる
(通知ポートの run loop source を、そのスレッドの CFRunLoop に追加して `CFRunLoopRun`)。
ヘッダの規則どおりに応答する:

- `kIOMessageCanSystemSleep`: `IOAllowPowerChange` で許可する(Knit はスリープを拒否しない)
- `kIOMessageSystemWillSleep`: 下記 B の「眠る前の処理」を **短時間(目安 1 秒以内)で** 済ませてから
  `IOAllowPowerChange`。処理が詰まっても応答が遅れないよう、重い処理は別スレッドへ投げるか
  タイムアウトを付ける
- `kIOMessageSystemWillPowerOn`: 応答しない。「起き始めた」と記録だけする(ネットワークはまだ使えない)
- `kIOMessageSystemHasPoweredOn`: 応答しない。下記 C の判定へ進む

登録に失敗した場合は `[power] 電源通知の登録に失敗` を1行ログに出し、従来の時計差による検知だけで
動く(機能を後退させない)。

### B. 眠る前の処理

- 状態を「眠り中」にする(下記 C の状態機械)
- 現在の接続を、相手に分かる形で閉じる(既存の `drop_stream` 相当。ソケットを閉じれば相手は即座に
  切断を検知でき、生存確認のタイムアウトを待たずに済む)。理由文字列は「スリープに入るため」
- ログ: `[power] スリープに入ります(接続を閉じました)`

### C. 復帰の判定(状態機械)

状態: `Awake` / `Asleep` / `DarkWake`(画面が消えたままの一時的な目覚め)

- `kIOMessageSystemHasPoweredOn` を受けたら、メインディスプレイが眠っているか
  (`CGDisplayIsAsleep(CGMainDisplayID())`)を確認する
  - 眠っていない → `Awake`(本当の復帰)
  - 眠っている → `DarkWake`
- `NSWorkspaceScreensDidWakeNotification` 相当(画面が点いた)を受けたら `Awake`。
  NSWorkspace の通知を購読するのが難しければ、`DarkWake` 中に 2 秒ごとに `CGDisplayIsAsleep` を
  見直して点いたら `Awake` にする、のでもよい(どちらにしたかを報告すること)
- `kIOMessageSystemWillSleep` を受けたら、どの状態からでも `Asleep`

遷移はすべて純関数(`fn next(state, event) -> state`)で表し、単体テストで網羅する。
OS の API 呼び出しは「イベントを作る薄い層」に閉じ込め、判定ロジックに混ぜない。

### D. 状態に応じた振る舞い

- `Asleep` / `DarkWake` の間:
  - クライアント役の再接続ループは試行しない(待機する)
  - サーバ役の待受は、来た接続を受け入れずにすぐ閉じる(ログ `[power] 画面が消えている間は接続を受け付けません`
    は最初の1回だけ。連打でログを汚さない)
- `Awake` になった瞬間:
  - 既存の `WAKE.notify()` で再接続の待機を飛ばし、すぐ張り直させる
  - ログ: `[conn-metric] wake unix_ms=… slept_s=… source=iokit`(既存の stability-report.sh が集計できる
    形を保つ。既存の時計差検知の行と区別するため `source=` を付ける。時計差の行には `source=clock` を付ける)
- 既存の時計差による検知(`slept_duration`)は **残す**(電源通知が取れない環境の保険)。ただし電源通知で
  既に処理した復帰を二重に処理しないこと(例: 直近数秒以内に iokit で復帰処理済みなら時計差側は
  ログだけ出して張り直しを省く)

### E. 逃がし道

- 環境変数 `KNIT_NO_SLEEP_GATE=1` で C・D(一時的な目覚めでの接続拒否)を無効化できる。
  A・B(眠る前に接続を閉じる)は有効のまま。判定は純関数+単体テスト
- 万一 `DarkWake` から抜けられない不具合に備え、`DarkWake` が 10 分続いたら `Awake` 扱いにする
  (ログに残す)

## やらないこと

- Windows 側の変更
- スリープの拒否(`IOCancelPowerChange`)や、スリープ防止のアサーション
- 設定画面の追加

## 実装上の注意

- 新しいモジュール(例: `crates/mac/src/power.rs`)にまとめる。FFI 宣言は既存の書き方
  (`#[link(name = "...", kind = "framework")]` と `extern "C"`)に合わせる。IOKit のリンクが必要
- 状態は他スレッドから読めるよう atomic などで持ち、`session.rs` 側からは
  `power::should_connect()` のような1関数で問い合わせる
- コメント・ログは日本語。既存のテスト名の流儀に合わせる

## 受け入れ条件

- `./scripts/quality-gate.sh` が PASS(exit 0)。Mac 側なので G1 で新しいテストが実際に走ること
- 状態機械の単体テスト: 少なくとも「眠る→画面が消えたまま起きる→接続しない」「眠る→普通に起きる→
  すぐ張り直す」「一時的な目覚め中に画面が点く→張り直す」「10分の上限」「KNIT_NO_SLEEP_GATE=1」
- `CHANGELOG.md` の[未リリース]に1行追加
- 実機確認(利用者が行う): Mac をスリープ→復帰で、Windows 側が即座に切断を検知し、復帰後すぐ
  再接続すること。`pmset -g log` で DarkWake が記録された時間帯に Knit の再接続ログが出ていないこと

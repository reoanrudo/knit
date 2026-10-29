# 仕様: Windows 側をバックグラウンドの電力スロットリングから外す

- 書いた人: lead エージェント(2026-10-29)。参考 OSS のソースを読んでいるため実装には関わらない
- 実装する人: 参考 OSS のソースを読んでいないエージェント
- 根拠とした資料(これ以外を根拠にしない):
  - Microsoft Learn「SetProcessInformation function (processthreadsapi.h)」
    https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-setprocessinformation
  - Microsoft Learn「Quality of Service」(Windows の QoS レベルの説明)
    https://learn.microsoft.com/en-us/windows/win32/procthread/quality-of-service
  - Microsoft Learn「SetPriorityClass function」
    https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-setpriorityclass
  - Knit 自身のコード(`crates/win/src/main.rs` ほか)

## 解決したい問題

Knit の Windows 側はトレイ常駐で、表示中のウィンドウを持たないことが多い。Microsoft Learn によれば、
Windows 11 は「ウィンドウが完全に隠れている・最小化されている・音を出していない」プロセスについて、
QoS を自動で推定して EcoQoS 扱いにし(CPU 周波数を下げる・効率コアへ寄せる)、
タイマー分解能の要求を無視することがある。

Knit の Windows 側は Mac から届くマウス・キー入力をリアルタイムに注入し、生存確認(ping/pong)にも
応答する。ここが間引かれると、カーソルのカクつき・入力の遅延・生存確認の取りこぼしによる切断に
つながる。似た構成のアプリでは「トレイに入れると画面端の切替がときどき効かない」という報告が
公開の issue として出ている。

## やること

1. プロセス起動時(`fn main` の早い段階。診断やプレビュー用の早期 return より後、常駐処理の前)に、
   `SetProcessInformation(GetCurrentProcess(), ProcessPowerThrottling, …)` で
   **EXECUTION_SPEED と IGNORE_TIMER_RESOLUTION の両方を「制御する・オフ」** にする
   (ドキュメントの「HighQoS」例と「Always honor Timer Resolution Requests」例に相当。
   ControlMask に両方のビット、StateMask は 0、Version は CURRENT_VERSION)。
2. 同じタイミングで、プロセスの優先度クラスを **一段だけ** 上げる(`ABOVE_NORMAL_PRIORITY_CLASS`)。
   `HIGH` / `REALTIME` は使わない(他のアプリを妨げるため)。
3. どちらも **失敗しても致命的にしない**。古い Windows では ProcessPowerThrottling が使えない。
   失敗時は `GetLastError` の値を1行ログに出して続行する。成功時も1行ログを出す
   (例: `[power] スロットリング解除: ok` / `[power] スロットリング解除に失敗: err=87`)。
4. 環境変数 `KNIT_NO_POWER_TUNE=1` で 1 と 2 の両方を無効化できるようにする(問題切り分け用)。
   判定は純関数にして単体テストを書く。

## やらないこと

- `timeBeginPeriod` などでタイマー分解能を新たに要求すること(今回の範囲外)
- 実行中に QoS を切り替えること(接続中だけ上げる等)。起動時に一度設定するだけ
- Mac 側の変更

## 実装上の注意

- `windows-sys` 0.59 を使う。必要な feature(`Win32_System_Threading` は既にある)と、
  定数・構造体の名前は crate 側の定義を確認して使う。無ければ Microsoft Learn の値で定義する
  (値の出典をコメントに書く)。
- コードは新しいモジュール(例: `crates/win/src/power.rs`)にまとめ、`main.rs` からは1行で呼ぶ。
- ログ・コメントは日本語(リポジトリの既存の書き方に合わせる)。

## 受け入れ条件

- `cargo check --target x86_64-pc-windows-gnu -p knit-win --tests` が通る
- 環境変数判定の純関数に単体テストがあり、`./scripts/quality-gate.sh --with-win` が PASS
- `CHANGELOG.md` の[未リリース]に1行追加
- 実機確認(利用者が行う): Windows でタスクマネージャーの「詳細」→ 該当プロセスの
  「効率モード」が付いていないこと、ログに `[power]` 行が出ること

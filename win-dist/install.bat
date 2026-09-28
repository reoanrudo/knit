@echo off
REM knit installer: deploy + startup task + start now
set DIR=%USERPROFILE%\knit
mkdir %DIR% 2>nul
copy /Y %~dp0knit-win.exe %DIR%\knit-win.exe
copy /Y %~dp0run_knit.bat %DIR%\run_knit.bat
copy /Y %~dp0run_knit.vbs %DIR%\run_knit.vbs
REM 共有トークン(同梱されていれば配布。無いと knit-win 起動が fatal 停止する)
if exist %~dp0.env copy /Y %~dp0.env %DIR%\.env >nul

REM === v0.25 まで(旧名 Tsunagu)からの移行: 設定を引き継ぎ、旧タスク・旧プロセスを止める ===
REM 旧フォルダ %USERPROFILE%\tsunagu は旧版へ戻せるよう残す
if not exist %DIR%\.env if exist %USERPROFILE%\tsunagu\.env copy /Y %USERPROFILE%\tsunagu\.env %DIR%\.env >nul
for %%T in (tsunagu tsunagu_run tsunagu_watch) do (
  schtasks /End /TN %%T >nul 2>&1
  schtasks /Delete /TN %%T /F >nul 2>&1
)
taskkill /IM tsunagu-win.exe /F >nul 2>&1
netsh advfirewall firewall delete rule name="tsunagu" >nul 2>&1

REM === v0.7(seamless-desk)からの移行: 旧タスク・旧規則・旧プロセスを掃除 ===
for %%T in (seamless_desk seamless_desk_run seamless_desk_watch) do (
  schtasks /End /TN %%T >nul 2>&1
  schtasks /Delete /TN %%T /F >nul 2>&1
)
REM 名前不明の旧タスクも取りこぼさない: 実行パスに seamless-desk / run_sd を
REM 含むタスクをすべて列挙して削除する(旧残骸が「vbs が見つかりません」の
REM ダイアログを出し続ける事故の根治)
powershell -NoProfile -Command "Get-ScheduledTask | Where-Object { ($_.Actions.Execute + ' ' + $_.Actions.Arguments) -match 'seamless-desk|run_sd' } | ForEach-Object { schtasks /End /TN $_.TaskName; schtasks /Delete /TN $_.TaskName /F }" >nul 2>&1
netsh advfirewall firewall delete rule name="seamless-desk" >nul 2>&1
netsh advfirewall firewall delete rule name="knit" >nul 2>&1
taskkill /IM sd-win.exe /F >nul 2>&1

REM firewall: knit-win は outbound 接続のみのため inbound 許可は不要
REM (誤設定の受信許可は上の移行ブロックで削除済み)

REM startup on logon (interactive session = required for SendInput)
schtasks /Create /TN knit /TR "wscript.exe \"%DIR%\run_knit.vbs\"" /SC ONLOGON /F >nul 2>&1

REM one-shot task for immediate/restart (run via: schtasks /Run /TN knit_run)
schtasks /Create /TN knit_run /TR "wscript.exe \"%DIR%\run_knit.vbs\"" /SC ONCE /ST 23:59 /F >nul 2>&1

REM auto-recovery: 毎分起動を試みる(既に起動していれば exe 側の
REM 二重起動防止(名前付きミューテックス)が即終了する=落ちても自動復帰)。
REM コンソール付きで起動してしまった場合も exe 自身が DETACHED プロセスへ
REM 置き換わるため、ターミナルを閉じても接続は維持される
schtasks /Create /TN knit_watch /TR "wscript.exe \"%DIR%\run_knit.vbs\"" /SC MINUTE /MO 1 /F >nul 2>&1

REM kill old instance and start fresh
taskkill /IM knit-win.exe /F >nul 2>&1
timeout /t 1 /nobreak >nul
schtasks /Run /TN knit_run
REM 初回導入だけは登録画面を開く。自動復帰からは繰り返し表示しない。
start "" "%DIR%\knit-win.exe" --retry-setup
if not exist %DIR%\.env (
  echo SETUP: KnitでMacを選び、表示された6桁コードを入力してください
) else (
  echo INSTALL_DONE
)

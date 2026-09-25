@echo off
REM tsunagu installer: deploy + startup task + start now
set DIR=C:\Users\<user>\tsunagu
mkdir %DIR% 2>nul
copy /Y %~dp0tsunagu-win.exe %DIR%\tsunagu-win.exe
copy /Y %~dp0run_tsunagu.bat %DIR%\run_tsunagu.bat
copy /Y %~dp0run_tsunagu.vbs %DIR%\run_tsunagu.vbs
REM 共有トークン(同梱されていれば配布。無いと tsunagu-win 起動が fatal 停止する)
if exist %~dp0.env copy /Y %~dp0.env %DIR%\.env >nul

REM === v0.7(seamless-desk)からの移行: 旧タスク・旧規則・旧プロセスを掃除 ===
for %%T in (seamless_desk seamless_desk_run seamless_desk_watch) do (
  schtasks /End /TN %%T >nul 2>&1
  schtasks /Delete /TN %%T /F >nul 2>&1
)
netsh advfirewall firewall delete rule name="seamless-desk" >nul 2>&1
netsh advfirewall firewall delete rule name="tsunagu" >nul 2>&1
taskkill /IM sd-win.exe /F >nul 2>&1

REM firewall: tsunagu-win は outbound 接続のみのため inbound 許可は不要
REM (誤設定の受信許可は上の移行ブロックで削除済み)

REM startup on logon (interactive session = required for SendInput)
schtasks /Create /TN tsunagu /TR "wscript.exe \"%DIR%\run_tsunagu.vbs\"" /SC ONLOGON /F >nul 2>&1

REM one-shot task for immediate/restart (run via: schtasks /Run /TN tsunagu_run)
schtasks /Create /TN tsunagu_run /TR "wscript.exe \"%DIR%\run_tsunagu.vbs\"" /SC ONCE /ST 23:59 /F >nul 2>&1

REM auto-recovery: 毎分起動を試みる(既に起動していれば exe 側の
REM 二重起動防止(名前付きミューテックス)が即終了する=落ちても自動復帰)。
REM コンソール付きで起動してしまった場合も exe 自身が DETACHED プロセスへ
REM 置き換わるため、ターミナルを閉じても接続は維持される
schtasks /Create /TN tsunagu_watch /TR "wscript.exe \"%DIR%\run_tsunagu.vbs\"" /SC MINUTE /MO 1 /F >nul 2>&1

REM kill old instance and start fresh
taskkill /IM tsunagu-win.exe /F >nul 2>&1
timeout /t 1 /nobreak >nul
schtasks /Run /TN tsunagu_run
if not exist %DIR%\.env (
  echo WARN: .env がありません。TSUNAGU_TOKEN を %DIR%\.env に設定してください
) else (
  echo INSTALL_DONE
)

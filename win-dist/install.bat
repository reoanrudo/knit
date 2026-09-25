@echo off
REM seamless-desk installer: deploy + startup task + start now
set DIR=C:\Users\<user>\seamless-desk
mkdir %DIR% 2>nul
copy /Y %~dp0sd-win.exe %DIR%\sd-win.exe
copy /Y %~dp0run_sd.bat %DIR%\run_sd.bat
copy /Y %~dp0run_sd.vbs %DIR%\run_sd.vbs
REM 共有トークン(同梱されていれば配布。無いと sd-win 起動が fatal 停止する)
if exist %~dp0.env copy /Y %~dp0.env %DIR%\.env >nul

REM firewall: sd-win は outbound 接続のみのため inbound 許可は不要。
REM 旧版が作成した誤設定の受信許可が残っていれば削除する(レビュー Wave1 E-M-2)
netsh advfirewall firewall delete rule name="seamless-desk" >nul 2>&1

REM startup on logon (interactive session = required for SendInput)
schtasks /Create /TN seamless_desk /TR "wscript.exe \"%DIR%\run_sd.vbs\"" /SC ONLOGON /F >nul 2>&1

REM one-shot task for immediate/restart (run via: schtasks /Run /TN seamless_desk_run)
schtasks /Create /TN seamless_desk_run /TR "wscript.exe \"%DIR%\run_sd.vbs\"" /SC ONCE /ST 23:59 /F >nul 2>&1

REM kill old instance and start fresh
taskkill /IM sd-win.exe /F >nul 2>&1
timeout /t 1 /nobreak >nul
schtasks /Run /TN seamless_desk_run
if not exist %DIR%\.env (
  echo WARN: .env がありません。SEAMLESS_DESK_TOKEN を %DIR%\.env に設定してください
) else (
  echo INSTALL_DONE
)

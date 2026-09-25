@echo off
REM seamless-desk installer: deploy + firewall + startup task + start now
set DIR=C:\Users\<user>\seamless-desk
mkdir %DIR% 2>nul
copy /Y %~dp0sd-win.exe %DIR%\sd-win.exe
copy /Y %~dp0run_sd.bat %DIR%\run_sd.bat

REM firewall (requires admin; fails silently if not elevated)
netsh advfirewall firewall delete rule name="seamless-desk" >nul 2>&1
netsh advfirewall firewall add rule name="seamless-desk" dir=in action=allow protocol=TCP localport=24900 >nul 2>&1

REM startup on logon (interactive session = required for SendInput)
schtasks /Create /TN seamless_desk /TR "%DIR%\run_sd.bat" /SC ONLOGON /F >nul 2>&1

REM one-shot task for immediate/restart (run via: schtasks /Run /TN seamless_desk_run)
schtasks /Create /TN seamless_desk_run /TR "%DIR%\run_sd.bat" /SC ONCE /ST 23:59 /F >nul 2>&1

REM kill old instance and start fresh
taskkill /IM sd-win.exe /F >nul 2>&1
timeout /t 1 /nobreak >nul
schtasks /Run /TN seamless_desk_run
echo INSTALL_DONE

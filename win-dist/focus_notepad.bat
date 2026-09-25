@echo off
powershell -NoProfile -Command "$ws = New-Object -ComObject WScript.Shell; [void]$ws.AppActivate('Notepad')"

@echo off
cd /d %~dp0
rem ログの無限増殖を防ぐため起動ごとに1世代ローテーションする
if exist knit-win.log.old del knit-win.log.old >nul 2>&1
if exist knit-win.log move /y knit-win.log knit-win.log.old >nul 2>&1
"%~dp0knit-win.exe" --background >> "%~dp0knit-win.log" 2>&1

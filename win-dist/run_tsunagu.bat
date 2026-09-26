@echo off
cd /d %~dp0
rem ログの無限増殖を防ぐため起動ごとに1世代ローテーションする
if exist tsunagu-win.log.old del tsunagu-win.log.old >nul 2>&1
if exist tsunagu-win.log move /y tsunagu-win.log tsunagu-win.log.old >nul 2>&1
"%~dp0tsunagu-win.exe" >> "%~dp0tsunagu-win.log" 2>&1

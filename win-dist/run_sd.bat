@echo off
cd /d %~dp0
rem ログの無限増殖を防ぐため起動ごとに1世代ローテーションする
if exist sd-win.log.old del sd-win.log.old >nul 2>&1
if exist sd-win.log move /y sd-win.log sd-win.log.old >nul 2>&1
C:\Users\<user>\seamless-desk\sd-win.exe >> C:\Users\<user>\seamless-desk\sd-win.log 2>&1

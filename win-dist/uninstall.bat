@echo off
REM Tsunagu uninstaller: 常駐・タスク登録を解除し、インストール済みファイルを削除する
REM (トークン(.env)も含め C:\Users\<user>\tsunagu 配下をすべて削除する点に注意)
set DIR=C:\Users\<user>\tsunagu

echo [uninstall] 停止中...
for %%T in (tsunagu tsunagu_run tsunagu_watch seamless_desk seamless_desk_run seamless_desk_watch) do (
  schtasks /End /TN %%T >nul 2>&1
  schtasks /Delete /TN %%T /F >nul 2>&1
)
taskkill /IM tsunagu-win.exe /F >nul 2>&1
taskkill /IM sd-win.exe /F >nul 2>&1
netsh advfirewall firewall delete rule name="tsunagu" >nul 2>&1
netsh advfirewall firewall delete rule name="seamless-desk" >nul 2>&1

echo [uninstall] ファイル削除中...
if exist "%DIR%" rmdir /s /q "%DIR%"

if exist "%DIR%" (
  echo ERROR: %DIR% の削除に失敗しました(使用中のファイルがあります。再起動後に再実行してください)
) else (
  echo UNINSTALL_DONE
)

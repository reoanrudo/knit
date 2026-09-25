' seamless-desk: run_sd.bat(ログ1世代ローテーション付き)をウィンドウなしで起動する。
' 旧版は exe を直接リダイレクトしていたため bat のローテーションが実経路で
' 無効化され sd-win.log が無限増殖していた(レビュー Wave1 X6 の修正)
CreateObject("WScript.Shell").Run "cmd /c ""C:\Users\<user>\seamless-desk\run_sd.bat""", 0, False

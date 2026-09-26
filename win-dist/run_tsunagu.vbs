' tsunagu: run_tsunagu.bat(ログ1世代ローテーション付き)をウィンドウなしで起動する。
' 旧版は exe を直接リダイレクトしていたため bat のローテーションが実経路で
' 無効化され tsunagu-win.log が無限増殖していた(レビュー Wave1 X6 の修正)
' 置き場所はこのスクリプト自身のフォルダ(利用者ごとに異なるため固定パスにしない)
dir = CreateObject("Scripting.FileSystemObject").GetParentFolderName(WScript.ScriptFullName)
CreateObject("WScript.Shell").Run "cmd /c """ & dir & "\run_tsunagu.bat""", 0, False

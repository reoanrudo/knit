' seamless-desk: sd-win をウィンドウなし(コンソール非表示)で起動する
CreateObject("WScript.Shell").Run "cmd /c ""C:\Users\<user>\seamless-desk\sd-win.exe >> C:\Users\<user>\seamless-desk\sd-win.log 2>&1""", 0, False

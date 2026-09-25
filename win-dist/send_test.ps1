$c = New-Object Net.Sockets.TcpClient
try { $c.Connect('100.100.10.9', 24901) } catch { Write-Output ("CONNECT_FAIL: " + $_.Exception.Message); exit 1 }
$s = $c.GetStream()
$b = [Text.Encoding]::UTF8.GetBytes('hello-from-win')
$s.Write($b, 0, $b.Length)
$s.Flush()
Start-Sleep -Milliseconds 300
$s.Close()
Write-Output SENT_OK

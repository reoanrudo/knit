$c = New-Object Net.Sockets.TcpClient
$c.NoDelay = $true
try { $c.Connect('100.100.10.9', 24902) } catch { Write-Output ("CONNECT_FAIL: " + $_.Exception.Message); exit 1 }
$s = $c.GetStream()
$b = [Text.Encoding]::UTF8.GetBytes('nodelay-test-line-1
nodelay-test-line-2')
$s.Write($b, 0, $b.Length)
$s.Flush()
Start-Sleep -Milliseconds 500
$b2 = [Text.Encoding]::UTF8.GetBytes('nodelay-test-line-3')
$s.Write($b2, 0, $b2.Length)
$s.Flush()
Start-Sleep -Milliseconds 500
Write-Output SENT_OK
$s.Close()

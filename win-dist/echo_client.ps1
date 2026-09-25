$c = New-Object Net.Sockets.TcpClient
$c.NoDelay = $true
$c.Connect('100.100.10.9', 24903)
$s = $c.GetStream()
$buf = New-Object byte[] 1024
$n = $s.Read($buf, 0, 1024)
Write-Output ("MAC1: " + [Text.Encoding]::UTF8.GetString($buf, 0, $n).Trim())
$b = [Text.Encoding]::UTF8.GetBytes('win-data-1')
$s.Write($b, 0, $b.Length); $s.Flush()
$n = $s.Read($buf, 0, 1024)
Write-Output ("MAC2: " + [Text.Encoding]::UTF8.GetString($buf, 0, $n).Trim())
$b2 = [Text.Encoding]::UTF8.GetBytes('win-data-2')
$s.Write($b2, 0, $b2.Length); $s.Flush()
Start-Sleep -Milliseconds 300
$c.Close()

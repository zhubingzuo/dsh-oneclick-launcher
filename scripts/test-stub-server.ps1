param([int]$Port = 3099)
# Stub HTTP server for regression scenario C: serves a UI path WITHOUT any token,
# i.e. it stands in for a DSH old enough not to authenticate. Runs until killed.
$listener = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, $Port)
$listener.Start()
Write-Output "stub listening on $Port"
$body = "HTTP/1.1 200 OK`r`nContent-Type: text/html`r`nContent-Length: 5`r`nConnection: close`r`n`r`nhello"
while ($true) {
    try {
        $client = $listener.AcceptTcpClient()
        $stream = $client.GetStream()
        $buf = New-Object byte[] 512
        $null = $stream.Read($buf, 0, $buf.Length)
        $bytes = [System.Text.Encoding]::ASCII.GetBytes($body)
        $stream.Write($bytes, 0, $bytes.Length)
        $stream.Close(); $client.Close()
    } catch { }
}

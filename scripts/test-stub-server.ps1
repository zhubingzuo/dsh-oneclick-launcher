param(
    [int]$Port = 3099,
    # 'dsh'   -> a token-less page that looks like the DSH UI (must be reused)
    # 'other' -> some unrelated local web app (must NOT be reused)
    [ValidateSet('dsh', 'other')]
    [string]$Mode = 'dsh'
)
# Stub HTTP server for the regression suite: answers GET / with 200 and no token,
# standing in for a DSH old enough not to authenticate. Runs until killed.
$listener = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, $Port)
$listener.Start()
Write-Output "stub listening on $Port (mode=$Mode)"
# Marker file so the regression script can prove THIS stub owns the port, instead
# of just seeing that something answers 200 there.
$markerDir = Join-Path $PSScriptRoot '..\.testdata'
New-Item -ItemType Directory -Force -Path $markerDir | Out-Null
Set-Content -Path (Join-Path $markerDir "stub-$Port-$Mode.ok") -Value "listening"

if ($Mode -eq 'dsh') {
    # Mimics @deepseek-ai/dsh-web-frontend/dist/index.html (title + assets).
    $page = '<!doctype html><html lang="en"><head><meta charset="utf-8" />' +
            '<link rel="manifest" href="./manifest.webmanifest" />' +
            '<link rel="icon" type="image/svg+xml" href="./favicon.svg" />' +
            '<title>DeepSeek Harness</title><script type="module" src="./assets/index-abc.js"></script>' +
            '</head><body><div id="root"></div></body></html>'
} else {
    $page = '<!doctype html><html><head><title>Some Other Local App</title></head><body>hello</body></html>'
}

$response = "HTTP/1.1 200 OK`r`nContent-Type: text/html; charset=utf-8`r`nContent-Length: $($page.Length)`r`nConnection: close`r`n`r`n$page"
while ($true) {
    try {
        $client = $listener.AcceptTcpClient()
        $stream = $client.GetStream()
        $buf = New-Object byte[] 1024
        $null = $stream.Read($buf, 0, $buf.Length)
        $bytes = [System.Text.Encoding]::UTF8.GetBytes($response)
        $stream.Write($bytes, 0, $bytes.Length)
        $stream.Close(); $client.Close()
    } catch { }
}

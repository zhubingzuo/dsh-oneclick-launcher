param(
    [int]$Port = 3098,
    [int]$DelaySeconds = 8
)
# Mimics the real dsh startup ORDER that caused the reviewed defect: the port is
# bound and answers 401 immediately, while the tokenized URL is printed only once
# the app has finished loading (seconds later). The launcher must keep waiting
# instead of declaring failure and killing this process.
$markerDir = Join-Path $PSScriptRoot '..\.testdata'
New-Item -ItemType Directory -Force -Path $markerDir | Out-Null
Set-Content -Path (Join-Path $markerDir "slow-$Port.ok") -Value "listening"

$listener = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, $Port)
$listener.Start()

function Reply($client, [string]$status, [string]$body) {
    $bytes = [System.Text.Encoding]::UTF8.GetBytes($body)
    $head = "HTTP/1.1 $status`r`nContent-Type: text/plain`r`nContent-Length: $($bytes.Length)`r`nConnection: close`r`n`r`n"
    $stream = $client.GetStream()
    $hb = [System.Text.Encoding]::ASCII.GetBytes($head)
    $stream.Write($hb, 0, $hb.Length)
    $stream.Write($bytes, 0, $bytes.Length)
    $stream.Close(); $client.Close()
}

$start = Get-Date
$announced = $false
while ($true) {
    if (-not $announced -and ((Get-Date) - $start).TotalSeconds -ge $DelaySeconds) {
        [Console]::Out.WriteLine("dsh web: http://127.0.0.1:$Port/?token=slow$Port")
        [Console]::Out.Flush()
        $announced = $true
    }
    if ($listener.Pending()) {
        try {
            $client = $listener.AcceptTcpClient()
            $buf = New-Object byte[] 1024
            $null = $client.GetStream().Read($buf, 0, $buf.Length)
            if ($announced) { Reply $client '200 OK' 'ok' } else { Reply $client '401 Unauthorized' '' }
        } catch { }
    } else {
        Start-Sleep -Milliseconds 100
    }
}

# End-to-end regression suite for dsh-launcher.
#
#   A: preferred port free                 -> launcher starts its own dsh with --port <preferred>
#   B: preferred port taken by a token-    -> launcher starts its own instance with --port 0 and
#      protected dsh                          leaves the other service untouched
#   C: preferred port taken by a token-    -> launcher reuses it: no own server, nothing killed
#      less DSH-like page
#   D: preferred port taken by an          -> launcher refuses to adopt it, starts its own instance
#      unrelated web app                      on a free port and leaves that app alone
#   E: server that binds the port early,   -> launcher keeps waiting for the tokenized URL instead
#      answers 401, prints the URL late       of failing and killing it (reviewed defect)
#
# Usage:  cargo build --release ; pwsh -NoProfile -ExecutionPolicy Bypass -File scripts/regression.ps1
# Runs alongside an already running launcher (it sets DSH_SKIP_SINGLE_INSTANCE) and only ever
# touches ports/data directories of its own.
$ErrorActionPreference = 'Continue'

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$exe      = Join-Path $repoRoot 'target\release\dsh-launcher.exe'
$stub     = Join-Path $PSScriptRoot 'test-stub-server.ps1'
$slow     = Join-Path $PSScriptRoot 'test-slow-server.ps1'
$work     = Join-Path $repoRoot '.testdata'
$pref     = 3099
$slowPort = 3098
$results  = @()

if (-not (Test-Path $exe)) { throw "build first: cargo build --release ($exe missing)" }

function Probe([int]$p) {
    try {
        $c = New-Object Net.Sockets.TcpClient
        $iar = $c.BeginConnect('127.0.0.1', $p, $null, $null)
        if ($iar.AsyncWaitHandle.WaitOne(400)) { $c.EndConnect($iar); $c.Close(); return $true }
        $c.Close(); return $false
    } catch { return $false }
}

function HttpStatus([int]$p) {
    try {
        $c = New-Object Net.Sockets.TcpClient
        $iar = $c.BeginConnect('127.0.0.1', $p, $null, $null)
        if (-not $iar.AsyncWaitHandle.WaitOne(600)) { $c.Close(); return -1 }
        $c.EndConnect($iar)
        $s = $c.GetStream(); $s.ReadTimeout = 5000
        $b = [Text.Encoding]::ASCII.GetBytes("GET / HTTP/1.1`r`nHost: 127.0.0.1:$p`r`nConnection: close`r`n`r`n")
        $s.Write($b, 0, $b.Length)
        $buf = New-Object byte[] 128
        $n = $s.Read($buf, 0, 128)
        $line = [Text.Encoding]::ASCII.GetString($buf, 0, $n) -split "`r`n" | Select-Object -First 1
        $c.Close()
        return [int]($line -split ' ')[1]
    } catch { return -1 }
}

function Check([string]$name, [bool]$ok, [string]$detail = '') {
    $script:results += [pscustomobject]@{ Test = $name; Pass = $ok; Detail = $detail }
    "{0} {1} {2}" -f $(if ($ok) { 'PASS' } else { 'FAIL' }), $name, $detail
}

function New-DataDir([string]$name) {
    $d = Join-Path $work $name
    Remove-Item $d -Recurse -Force -ErrorAction SilentlyContinue
    New-Item -ItemType Directory -Force -Path $d | Out-Null
    return $d
}

# The config file (not an env var) drives the port, so scenario A really proves
# that config parsing works.
function Write-Config([string]$dataDir, [int]$port) {
    $lines = @(
        '# regression config',
        'command = npx --yes @deepseek-ai/dsh web',
        'extra_args = --no-open',
        "port = $port",
        'ui_marker = DeepSeek Harness',
        'url_marker = dsh web:',
        'timeout_secs = 120'
    )
    Set-Content -Path (Join-Path $dataDir 'launcher.conf') -Value $lines -Encoding UTF8
}

function Run-Launcher([string]$dataDir, [int]$port = $pref, [string]$serverExtra = '') {
    Write-Config $dataDir $port
    $env:DSH_DATA_DIR = $dataDir
    $env:DSH_FAKE_CHROME = '4'
    $env:DSH_NO_UI = '1'
    $env:DSH_SKIP_SINGLE_INSTANCE = '1'
    $env:DSH_LAUNCHER_CONFIG = Join-Path $dataDir 'launcher.conf'
    Remove-Item Env:DSH_PORT -ErrorAction SilentlyContinue # the config file is the source of truth
    if ($serverExtra) { $env:DSH_SERVER_EXTRA = $serverExtra } else { Remove-Item Env:DSH_SERVER_EXTRA -ErrorAction SilentlyContinue }
    $p = Start-Process -FilePath $exe -PassThru -WindowStyle Hidden
    $finished = $p.WaitForExit(180000)
    [pscustomobject]@{
        Process   = $p
        Finished  = $finished
        ExitCode  = $p.ExitCode
        Log       = (Join-Path $dataDir 'launcher.log')
        ServerLog = (Join-Path $dataDir 'server.log')
    }
}

function CleanEnv {
    Remove-Item Env:DSH_DATA_DIR, Env:DSH_PORT, Env:DSH_FAKE_CHROME, Env:DSH_NO_UI,
        Env:DSH_SKIP_SINGLE_INSTANCE, Env:DSH_LAUNCHER_CONFIG, Env:DSH_SERVER_EXTRA -ErrorAction SilentlyContinue
}

# Stub processes drop a marker file, so a scenario can prove that IT owns the
# port rather than some other service that happens to answer the same way.
function Wait-Marker([string]$name, [int]$timeoutMs = 20000) {
    $path = Join-Path $work $name
    $deadline = (Get-Date).AddMilliseconds($timeoutMs)
    while ((Get-Date) -lt $deadline) {
        if (Test-Path $path) { return $true }
        Start-Sleep -Milliseconds 100
    }
    return $false
}

function Clear-Marker([string]$name) {
    Remove-Item (Join-Path $work $name) -Force -ErrorAction SilentlyContinue
}

New-Item -ItemType Directory -Force -Path $work | Out-Null

# ============================ SCENARIO A ============================
Write-Host "`n===== SCENARIO A: preferred port $pref free =====" -ForegroundColor Cyan
if (Probe $pref) { Write-Host "port $pref is busy - stop that service first"; exit 1 }
$dataA = New-DataDir 'dataA'
$a = Run-Launcher $dataA
$logA = if (Test-Path $a.Log) { Get-Content $a.Log -Raw } else { '' }
Check 'A: launcher exited 0' ($a.Finished -and $a.ExitCode -eq 0) "exit=$($a.ExitCode) finished=$($a.Finished)"
Check 'A: config file was written and parsed (port=3099)' ($logA -match 'port=3099')
Check 'A: command carries --no-open and --port 3099' ($logA -match 'server command: cmd /C npx --yes @deepseek-ai/dsh web --no-open --port 3099')
Check 'A: readiness URL has a token' ($logA -match "server ready: http://127\.0\.0\.1:$pref/\?token=\S+")
Check 'A: server tree stopped on window close' ($logA -match 'stopping server tree')
Check 'A: port released after exit' (-not (Probe $pref))
CleanEnv

# ============================ SCENARIO C ============================
Write-Host "`n===== SCENARIO C: token-less DSH-like page already on $pref =====" -ForegroundColor Cyan
if (Probe $pref) { Write-Host "port $pref is busy - aborting scenario C"; exit 1 }
Clear-Marker "stub-$pref-dsh.ok"
$stubProc = Start-Process pwsh -ArgumentList '-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', $stub, "$pref", 'dsh' -PassThru -WindowStyle Hidden
Check 'C: our DSH-like stub owns port 3099' (Wait-Marker "stub-$pref-dsh.ok") "marker stub-$pref-dsh.ok"
$dataC = New-DataDir 'dataC'
$c = Run-Launcher $dataC
$logC = if (Test-Path $c.Log) { Get-Content $c.Log -Raw } else { '' }
Check 'C: launcher exited 0' ($c.Finished -and $c.ExitCode -eq 0) "exit=$($c.ExitCode)"
Check 'C: reused the existing token-less service' ($logC -match 'already serves the DSH UI without a token; reusing it')
Check 'C: did NOT start its own server' (-not ($logC -match 'server command:'))
Check 'C: did NOT stop the reused service' (-not ($logC -match 'stopping server tree'))
Check 'C: stub still alive after exit' ((HttpStatus $pref) -eq 200) "status=$(HttpStatus $pref)"
Stop-Process -Id $stubProc.Id -Force -ErrorAction SilentlyContinue
Start-Sleep -Milliseconds 500
CleanEnv

# ============================ SCENARIO D ============================
Write-Host "`n===== SCENARIO D: unrelated web app on $pref must not be adopted =====" -ForegroundColor Cyan
if (Probe $pref) { Write-Host "port $pref is busy - aborting scenario D"; exit 1 }
Clear-Marker "stub-$pref-other.ok"
$stubProc2 = Start-Process pwsh -ArgumentList '-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', $stub, "$pref", 'other' -PassThru -WindowStyle Hidden
Check 'D: our unrelated-app stub owns port 3099' (Wait-Marker "stub-$pref-other.ok") "marker stub-$pref-other.ok"
$dataD = New-DataDir 'dataD'
$d = Run-Launcher $dataD
$logD = if (Test-Path $d.Log) { Get-Content $d.Log -Raw } else { '' }
Check 'D: launcher exited 0' ($d.Finished -and $d.ExitCode -eq 0) "exit=$($d.ExitCode)"
Check 'D: refused to adopt the non-DSH page' ($logD -match 'does not contain "DeepSeek Harness"; starting a separate instance')
Check 'D: asked dsh for a free port (--port 0)' ($logD -match 'server command: cmd /C npx --yes @deepseek-ai/dsh web --no-open --port 0')
$md = [regex]::Match($logD, 'server ready: http://127\.0\.0\.1:(\d+)/\?token=')
Check 'D: own instance used another port' ($md.Success -and [int]$md.Groups[1].Value -ne $pref) "port=$($md.Groups[1].Value)"
Check 'D: other app still alive after exit' ((HttpStatus $pref) -eq 200) "status=$(HttpStatus $pref)"
Stop-Process -Id $stubProc2.Id -Force -ErrorAction SilentlyContinue
Start-Sleep -Milliseconds 500
CleanEnv

# ============================ SCENARIO E ============================
# Regression for the reviewed critical defect: the port is bound and answers 401
# long before the tokenized URL is printed. The old code failed after 5s and
# killed the server; it must now wait for the URL.
Write-Host "`n===== SCENARIO E: late tokenized URL must not be treated as failure =====" -ForegroundColor Cyan
if (Probe $slowPort) { Write-Host "port $slowPort is busy - aborting scenario E"; exit 1 }
Clear-Marker "slow-$slowPort.ok"
$extra = "pwsh -NoProfile -ExecutionPolicy Bypass -File `"$slow`" $slowPort 8"
$dataE = New-DataDir 'dataE'
$e = Run-Launcher $dataE $slowPort $extra
$logE = if (Test-Path $e.Log) { Get-Content $e.Log -Raw } else { '' }
Check 'E: slow server bound the port early (precondition)' (Wait-Marker "slow-$slowPort.ok") "marker slow-$slowPort.ok"
Check 'E: launcher exited 0' ($e.Finished -and $e.ExitCode -eq 0) "exit=$($e.ExitCode)"
Check 'E: saw the 401 but kept waiting' ($logE -match 'answered 401; waiting for the tokenized launch URL')
Check 'E: did NOT declare a failure' (-not ($logE -match 'no launch URL was printed|无法取得访问令牌|server did not become ready'))
Check 'E: used the late tokenized URL' ($logE -match "server ready: http://127\.0\.0\.1:$slowPort/\?token=")
Check 'E: stopped its own server on exit' ($logE -match 'stopping server tree')
Check 'E: port released after exit' (-not (Probe $slowPort))
CleanEnv

# ============================ SCENARIO B ============================
Write-Host "`n===== SCENARIO B: token-protected dsh already on $pref =====" -ForegroundColor Cyan
if (Probe $pref) { Write-Host "port $pref is busy - aborting scenario B"; exit 1 }
$other = Start-Process cmd -ArgumentList '/C', "npx --yes @deepseek-ai/dsh web --no-open --port $pref" -PassThru -WindowStyle Hidden
$up = $false
for ($i = 0; $i -lt 160; $i++) { if ((HttpStatus $pref) -eq 401) { $up = $true; break }; Start-Sleep -Milliseconds 500 }
Check 'B: other dsh answers 401 (precondition)' $up "status=$(HttpStatus $pref)"
$dataB = New-DataDir 'dataB'
$b = Run-Launcher $dataB
$logB = if (Test-Path $b.Log) { Get-Content $b.Log -Raw } else { '' }
Check 'B: launcher exited 0' ($b.Finished -and $b.ExitCode -eq 0) "exit=$($b.ExitCode)"
Check 'B: recognised the token requirement' ($logB -match 'requires its own token \(HTTP 401\)')
Check 'B: asked dsh for a free port (--port 0)' ($logB -match 'server command: cmd /C npx --yes @deepseek-ai/dsh web --no-open --port 0')
$m = [regex]::Match($logB, 'server ready: http://127\.0\.0\.1:(\d+)/\?token=')
Check 'B: own instance used another port' ($m.Success -and [int]$m.Groups[1].Value -ne $pref) "port=$($m.Groups[1].Value)"
Check 'B: other service still listening after launcher exit' ((HttpStatus $pref) -eq 401) "status=$(HttpStatus $pref)"
Check 'B: own server tree stopped' ($logB -match 'stopping server tree')
taskkill /PID $other.Id /T /F 2>&1 | Out-Null
CleanEnv

# ============================ SUMMARY ============================
Write-Host "`n===== SUMMARY =====" -ForegroundColor Cyan
$results | Format-Table -AutoSize
$failed = ($results | Where-Object { -not $_.Pass }).Count
"total=$($results.Count) passed=$(($results | Where-Object Pass).Count) failed=$failed"
if ($failed -eq 0) { 'ALL PASS'; exit 0 } else { 'SOME FAILED'; exit 1 }

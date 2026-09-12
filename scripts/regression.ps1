# End-to-end regression suite for dsh-launcher.
#
#   A: preferred port free                 -> launcher starts its own dsh with --port <preferred>
#   B: preferred port taken by a token-    -> launcher starts its own instance with --port 0 and
#      protected dsh                          leaves the other service untouched
#   C: preferred port taken by a token-    -> launcher reuses it: no own server, nothing killed
#      less (legacy) service
#
# Usage:  cargo build --release ; pwsh -NoProfile -ExecutionPolicy Bypass -File scripts/regression.ps1
# Runs alongside an already running launcher (it sets DSH_SKIP_SINGLE_INSTANCE) and only ever
# touches ports/data directories of its own.
$ErrorActionPreference = 'Continue'

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$exe      = Join-Path $repoRoot 'target\release\dsh-launcher.exe'
$stub     = Join-Path $PSScriptRoot 'test-stub-server.ps1'
$work     = Join-Path $repoRoot '.testdata'
$pref     = 3099
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

function Run-Launcher([string]$dataDir) {
    $env:DSH_DATA_DIR = $dataDir
    $env:DSH_PORT = "$pref"
    $env:DSH_FAKE_CHROME = '4'
    $env:DSH_NO_UI = '1'
    $env:DSH_SKIP_SINGLE_INSTANCE = '1'
    $env:DSH_LAUNCHER_CONFIG = Join-Path $dataDir 'launcher.conf'
    Remove-Item Env:DSH_SERVER_EXTRA -ErrorAction SilentlyContinue
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

function New-DataDir([string]$name) {
    $d = Join-Path $work $name
    Remove-Item $d -Recurse -Force -ErrorAction SilentlyContinue
    New-Item -ItemType Directory -Force -Path $d | Out-Null
    return $d
}

New-Item -ItemType Directory -Force -Path $work | Out-Null

# ============================ SCENARIO A ============================
Write-Host "`n===== SCENARIO A: preferred port $pref free =====" -ForegroundColor Cyan
if (Probe $pref) { Write-Host "port $pref is busy - stop that service first"; exit 1 }
$dataA = New-DataDir 'dataA'
$a = Run-Launcher $dataA
$logA = if (Test-Path $a.Log) { Get-Content $a.Log -Raw } else { '' }
Check 'A: launcher exited 0' ($a.Finished -and $a.ExitCode -eq 0) "exit=$($a.ExitCode) finished=$($a.Finished)"
Check 'A: config file auto-created' (Test-Path (Join-Path $dataA 'launcher.conf'))
Check 'A: config parsed (port=3099)' ($logA -match 'port=3099')
Check 'A: command carries --no-open and --port 3099' ($logA -match 'server command: cmd /C npx --yes @deepseek-ai/dsh web --no-open --port 3099')
Check 'A: readiness URL has a token' ($logA -match "server ready: http://127\.0\.0\.1:$pref/\?token=\S+")
Check 'A: server tree stopped on window close' ($logA -match 'stopping server tree')
Check 'A: port released after exit' (-not (Probe $pref))
CleanEnv

# ============================ SCENARIO C ============================
Write-Host "`n===== SCENARIO C: token-less service already on $pref =====" -ForegroundColor Cyan
$stubProc = Start-Process pwsh -ArgumentList '-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', $stub, "$pref" -PassThru -WindowStyle Hidden
$up = $false
for ($i = 0; $i -lt 40; $i++) { if ((HttpStatus $pref) -eq 200) { $up = $true; break }; Start-Sleep -Milliseconds 250 }
Check 'C: stub answers 200 (precondition)' $up "status=$(HttpStatus $pref)"
$dataC = New-DataDir 'dataC'
$c = Run-Launcher $dataC
$logC = if (Test-Path $c.Log) { Get-Content $c.Log -Raw } else { '' }
Check 'C: launcher exited 0' ($c.Finished -and $c.ExitCode -eq 0) "exit=$($c.ExitCode)"
Check 'C: reused the existing token-less service' ($logC -match 'already serves the UI without a token; reusing it')
Check 'C: did NOT start its own server' (-not ($logC -match 'server command:'))
Check 'C: did NOT stop the reused service' (-not ($logC -match 'stopping server tree'))
Check 'C: stub still alive after exit' ((HttpStatus $pref) -eq 200) "status=$(HttpStatus $pref)"
Stop-Process -Id $stubProc.Id -Force -ErrorAction SilentlyContinue
Start-Sleep -Milliseconds 500
CleanEnv

# ============================ SCENARIO B ============================
Write-Host "`n===== SCENARIO B: token-protected dsh already on $pref =====" -ForegroundColor Cyan
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

# Generates assets/icon.ico from the official DSH brand mark (favicon.svg path data).
# Draws the whale on a rounded DeepSeek-blue tile at 16..256 px, packs as PNG-based .ico.
param(
    # Path to DSH's favicon.svg. If omitted, the script probes common npm-global
    # install locations for @deepseek-ai/dsh.
    [string]$SvgPath = ""
)
$ErrorActionPreference = 'Stop'

if ([string]::IsNullOrWhiteSpace($SvgPath) -or -not (Test-Path -LiteralPath $SvgPath)) {
    $candidates = @(
        "$env:APPDATA\npm\node_modules\@deepseek-ai\dsh\node_modules\@deepseek-ai\dsh-web-frontend\dist\favicon.svg",
        "$env:APPDATA\npm\node_modules\@deepseek-ai\dsh-web-frontend\dist\favicon.svg"
    )
    $found = $candidates | Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1
    if ($found) { $SvgPath = $found }
}
if (-not $SvgPath -or -not (Test-Path -LiteralPath $SvgPath)) {
    throw "favicon.svg not found. Pass its path: gen_icon.ps1 -SvgPath C:\path\to\favicon.svg"
}

$outDir  = Join-Path $PSScriptRoot '..\assets'
New-Item -ItemType Directory -Force -Path $outDir | Out-Null
$outIco  = Join-Path $outDir 'icon.ico'

Add-Type -AssemblyName System.Drawing

$svg = Get-Content -LiteralPath $SvgPath -Raw
# match the path element's own d attribute (not id="path")
$m = [regex]::Match($svg, '<path[^>]*\bd="([^"]+)"')
if (-not $m.Success) { throw 'path data not found in svg' }
$d = $m.Groups[1].Value

# ---------- SVG path tokenizer ----------
function Get-PathTokens([string]$d) {
    $tokens = New-Object System.Collections.Generic.List[object]
    $i = 0
    $num = ''
    while ($i -lt $d.Length) {
        $ch = $d[$i]
        if ($ch -match '[A-Za-z]') {
            if ($num -ne '') { $tokens.Add([double]$num); $num = '' }
            $tokens.Add([string]$ch)
        }
        elseif ($ch -match '[0-9.\-]') {
            $num += $ch
            if ($i + 1 -lt $d.Length -and $d[$i+1] -match '[eE]') {
                $num += $d[$i+1]; $i++
                if ($i + 1 -lt $d.Length -and $d[$i+1] -match '[+\-]') { $num += $d[$i+1]; $i++ }
            }
        }
        else {
            if ($num -ne '') { $tokens.Add([double]$num); $num = '' }
        }
        $i++
    }
    if ($num -ne '') { $tokens.Add([double]$num) }
    ,$tokens
}

# ---------- Build GraphicsPath from tokens (absolute + relative) ----------
# Uses script-scope cx/cy as current absolute point, sx/sy as subpath start.
$script:cx = 0.0; $script:cy = 0.0; $script:sx = 0.0; $script:sy = 0.0
$script:minx = [double]::MaxValue; $script:miny = [double]::MaxValue
$script:maxx = [double]::MinValue; $script:maxy = [double]::MinValue

function Add-Pt([double]$px, [double]$py) {
    if ($px -lt $script:minx) { $script:minx = $px }
    if ($px -gt $script:maxx) { $script:maxx = $px }
    if ($py -lt $script:miny) { $script:miny = $py }
    if ($py -gt $script:maxy) { $script:maxy = $py }
}

function Build-Whale([string]$d) {
    $tokens = Get-PathTokens $d
    $path = New-Object System.Drawing.Drawing2D.GraphicsPath
    $idx = 0
    while ($idx -lt $tokens.Count) {
        $cmd = [string]$tokens[$idx]; $idx++
        $upper = $cmd.ToUpper()
        $rel = ($cmd -cne $upper)
        switch ($upper) {
            'M' {
                $nx = [double]$tokens[$idx]; $ny = [double]$tokens[$idx+1]; $idx += 2
                if ($rel) { $nx += $script:cx; $ny += $script:cy }
                $script:cx = $nx; $script:cy = $ny
                $script:sx = $nx; $script:sy = $ny
                $path.StartFigure()
                Add-Pt $nx $ny
            }
            'L' {
                $nx = [double]$tokens[$idx]; $ny = [double]$tokens[$idx+1]; $idx += 2
                if ($rel) { $nx += $script:cx; $ny += $script:cy }
                $path.AddLine([float]$script:cx, [float]$script:cy, [float]$nx, [float]$ny)
                $script:cx = $nx; $script:cy = $ny; Add-Pt $nx $ny
            }
            'H' {
                $nx = [double]$tokens[$idx]; $idx += 1
                if ($rel) { $nx += $script:cx }
                $path.AddLine([float]$script:cx, [float]$script:cy, [float]$nx, [float]$script:cy)
                $script:cx = $nx; Add-Pt $nx $script:cy
            }
            'V' {
                $ny = [double]$tokens[$idx]; $idx += 1
                if ($rel) { $ny += $script:cy }
                $path.AddLine([float]$script:cx, [float]$script:cy, [float]$script:cx, [float]$ny)
                $script:cy = $ny; Add-Pt $script:cx $ny
            }
            'C' {
                $c1x=[double]$tokens[$idx];   $c1y=[double]$tokens[$idx+1]
                $c2x=[double]$tokens[$idx+2]; $c2y=[double]$tokens[$idx+3]
                $ex =[double]$tokens[$idx+4]; $ey =[double]$tokens[$idx+5]; $idx += 6
                if ($rel) {
                    $c1x += $script:cx; $c1y += $script:cy
                    $c2x += $script:cx; $c2y += $script:cy
                    $ex  += $script:cx; $ey  += $script:cy
                }
                $path.AddBezier([float]$script:cx, [float]$script:cy,
                                [float]$c1x, [float]$c1y, [float]$c2x, [float]$c2y,
                                [float]$ex, [float]$ey)
                $script:cx = $ex; $script:cy = $ey; Add-Pt $ex $ey
            }
            'S' {
                $c2x=[double]$tokens[$idx]; $c2y=[double]$tokens[$idx+1]
                $ex =[double]$tokens[$idx+2]; $ey =[double]$tokens[$idx+3]; $idx += 4
                if ($rel) { $c2x += $script:cx; $c2y += $script:cy; $ex += $script:cx; $ey += $script:cy }
                $path.AddBezier([float]$script:cx, [float]$script:cy,
                                [float]$script:cx, [float]$script:cy,
                                [float]$c2x, [float]$c2y, [float]$ex, [float]$ey)
                $script:cx = $ex; $script:cy = $ey; Add-Pt $ex $ey
            }
            'Q' {
                $cx2=[double]$tokens[$idx]; $cy2=[double]$tokens[$idx+1]
                $ex =[double]$tokens[$idx+2]; $ey =[double]$tokens[$idx+3]; $idx += 4
                if ($rel) { $cx2 += $script:cx; $cy2 += $script:cy; $ex += $script:cx; $ey += $script:cy }
                $path.AddBezier([float]$script:cx, [float]$script:cy,
                                [float]$cx2, [float]$cy2, [float]$cx2, [float]$cy2,
                                [float]$ex, [float]$ey)
                $script:cx = $ex; $script:cy = $ey; Add-Pt $ex $ey
            }
            'T' {
                $ex =[double]$tokens[$idx]; $ey =[double]$tokens[$idx+1]; $idx += 2
                if ($rel) { $ex += $script:cx; $ey += $script:cy }
                $path.AddLine([float]$script:cx, [float]$script:cy, [float]$ex, [float]$ey)
                $script:cx = $ex; $script:cy = $ey; Add-Pt $ex $ey
            }
            'A' { throw 'A (arc) commands are not supported by this generator' }
            'Z' {
                $path.CloseFigure()
                $script:cx = $script:sx; $script:cy = $script:sy
            }
            default { throw "unknown command: $cmd" }
        }
    }
    return $path
}

$whale = Build-Whale $d
if ($script:maxx -le $script:minx -or $script:maxy -le $script:miny) { throw 'whale bounds invalid' }
"whale bounds: x [$($script:minx), $($script:maxx)] y [$($script:miny), $($script:maxy)]"

function New-RoundedRectPath([single]$w, [single]$h, [single]$r) {
    $p = New-Object System.Drawing.Drawing2D.GraphicsPath
    $d = 2 * $r
    $p.AddArc(0, 0, $d, $d, 180, 90)
    $p.AddArc($w - $d, 0, $d, $d, 270, 90)
    $p.AddArc($w - $d, $h - $d, $d, $d, 0, 90)
    $p.AddArc(0, $h - $d, $d, $d, 90, 90)
    $p.CloseFigure()
    return $p
}

function New-IconBitmap([int]$S) {
    $bmp = New-Object System.Drawing.Bitmap($S, $S, ([System.Drawing.Imaging.PixelFormat]::Format32bppArgb))
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
    $g.Clear([System.Drawing.Color]::Transparent)

    $r = [single]($S * 0.20)
    $tile = New-RoundedRectPath ([single]$S) ([single]$S) $r
    $cTop = [System.Drawing.Color]::FromArgb(255, 92, 122, 255)
    $cBot = [System.Drawing.Color]::FromArgb(255, 44, 60, 214)
    $grad = New-Object System.Drawing.Drawing2D.LinearGradientBrush(
        (New-Object System.Drawing.RectangleF(0, 0, $S, $S)), $cTop, $cBot, 90.0)
    $g.FillPath($grad, $tile)

    $pad = [single]($S * 0.17)
    $innerW = [single]($S - 2 * $pad)
    $innerH = [single]($S - 2 * $pad)
    $bw = [single]($script:maxx - $script:minx)
    $bh = [single]($script:maxy - $script:miny)
    $scale = [single]([Math]::Min($innerW / $bw, $innerH / $bh))
    $offX = [single](($S - $bw * $scale) / 2 - $script:minx * $scale)
    $offY = [single](($S - $bh * $scale) / 2 - $script:miny * $scale)

    $clone = $whale.Clone()
    $mat = New-Object System.Drawing.Drawing2D.Matrix
    $mat.Translate($offX, $offY)
    $mat.Scale($scale, $scale)
    $clone.Transform($mat)
    $white = New-Object System.Drawing.SolidBrush([System.Drawing.Color]::White)
    $g.FillPath($white, $clone)

    $grad.Dispose(); $white.Dispose(); $mat.Dispose(); $tile.Dispose(); $g.Dispose()
    return $bmp
}

# ---------- render PNG blobs per size ----------
$sizes = @(16, 20, 24, 32, 40, 48, 64, 128, 256)
$pngs = @()
foreach ($S in $sizes) {
    $bmp = New-IconBitmap $S
    $ms = New-Object System.IO.MemoryStream
    $bmp.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png)
    $pngs += ,$ms.ToArray()
    $ms.Dispose(); $bmp.Dispose()
    "rendered ${S}px: $($pngs[-1].Length) bytes"
}

# ---------- pack as .ico (PNG-compressed entries) ----------
$fs = [System.IO.File]::Create($outIco)
$bw = New-Object System.IO.BinaryWriter($fs)
$bw.Write([uint16]0)
$bw.Write([uint16]1)
$bw.Write([uint16]$sizes.Count)
$offset = 6 + 16 * $sizes.Count
for ($i = 0; $i -lt $sizes.Count; $i++) {
    $s = $sizes[$i]
    $bw.Write([byte]($(if ($s -ge 256) { 0 } else { $s })))
    $bw.Write([byte]($(if ($s -ge 256) { 0 } else { $s })))
    $bw.Write([byte]0)
    $bw.Write([byte]0)
    $bw.Write([uint16]1)
    $bw.Write([uint16]32)
    $bw.Write([uint32]$pngs[$i].Length)
    $bw.Write([uint32]$offset)
    $offset += $pngs[$i].Length
}
foreach ($p in $pngs) { $bw.Write($p) }
$bw.Dispose(); $fs.Dispose()
"icon written: $outIco ($((Get-Item $outIco).Length) bytes)"

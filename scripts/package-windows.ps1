# Compositor - Windows packaging & release script.
#
# Usage:
#   powershell -ExecutionPolicy Bypass -File scripts\package-windows.ps1
#
# Steps:
#   1. Generate assets\compositor.ico if missing (drawn with System.Drawing).
#   2. cargo build --release.
#   3. Produce an installer using the first available tool:
#      a) Inno Setup 6 (ISCC.exe on PATH or default install dir)  -> dist\Compositor-Setup-<ver>.exe
#      b) cargo-wix (WiX toolset installed)                        -> target\wix\compositor-<ver>-x86_64.msi
#      c) Portable zip fallback                                    -> dist\Compositor-win64.zip
#
# Install Inno Setup:  https://jrsoftware.org/isdl.php
# Install WiX + cargo-wix:
#   winget install --id WiXToolset.WiXToolset.31
#   cargo install cargo-wix
#   cargo wix --help

param(
    [switch]$Force   # regenerate assets\compositor.ico even if it exists
)

$ErrorActionPreference = "Stop"

$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

function New-CompositorIcon {
    param([string]$IcoPath)
    Add-Type -AssemblyName System.Drawing

    function New-IconBitmap([int]$Size) {
        $bmp = New-Object System.Drawing.Bitmap($Size, $Size)
        $g = [System.Drawing.Graphics]::FromImage($bmp)
        $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
        $g.TextRenderingHint = [System.Drawing.Text.TextRenderingHint]::AntiAlias
        $g.Clear([System.Drawing.Color]::FromArgb(30, 30, 30))

        # Rounded blue gradient square.
        $rect = New-Object System.Drawing.RectangleF(0, 0, $Size, $Size)
        $path = New-Object System.Drawing.Drawing2D.GraphicsPath
        $d = [float]($Size * 0.22)
        $path.AddArc(0, 0, $d, $d, 180, 90)
        $path.AddArc($Size - $d, 0, $d, $d, 270, 90)
        $path.AddArc($Size - $d, $Size - $d, $d, $d, 0, 90)
        $path.AddArc(0, $Size - $d, $d, $d, 90, 90)
        $path.CloseFigure()
        $brush = New-Object System.Drawing.Drawing2D.LinearGradientBrush(
            $rect,
            [System.Drawing.Color]::FromArgb(90, 155, 232),
            [System.Drawing.Color]::FromArgb(47, 111, 176),
            45.0)
        $g.FillPath($brush, $path)

        # White "C" letter.
        $font = New-Object System.Drawing.Font("Segoe UI", [float]($Size * 0.55),
            [System.Drawing.FontStyle]::Bold, [System.Drawing.GraphicsUnit]::Pixel)
        $sf = New-Object System.Drawing.StringFormat
        $sf.Alignment = [System.Drawing.StringAlignment]::Center
        $sf.LineAlignment = [System.Drawing.StringAlignment]::Center
        $g.DrawString("C", $font, [System.Drawing.Brushes]::White, $rect, $sf)
        $g.Dispose(); $font.Dispose(); $brush.Dispose(); $path.Dispose()
        return $bmp
    }

    # Convert a 32bpp ARGB bitmap to a bottom-up BGRA DIB (BITMAPINFOHEADER + XOR + AND mask).
    # Classic rc.exe only accepts BMP/DIB entries (PNG is limited to the 256x256 slot).
    function ConvertTo-Dib([System.Drawing.Bitmap]$bmp) {
        $w = $bmp.Width; $h = $bmp.Height
        $rect = New-Object System.Drawing.Rectangle(0, 0, $w, $h)
        $data = $bmp.LockBits($rect, [System.Drawing.Imaging.ImageLockMode]::ReadOnly,
            [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
        $stride = $data.Stride
        $bytes = New-Object byte[] ($stride * $h)
        [System.Runtime.InteropServices.Marshal]::Copy($data.Scan0, $bytes, 0, $bytes.Length)
        $bmp.UnlockBits($data)

        $ms = New-Object System.IO.MemoryStream
        $bw = New-Object System.IO.BinaryWriter($ms)
        # BITMAPINFOHEADER
        $bw.Write([int32]40)        # biSize
        $bw.Write([int32]$w)        # biWidth
        $bw.Write([int32]($h * 2))  # biHeight (XOR + AND mask)
        $bw.Write([uint16]1)        # biPlanes
        $bw.Write([uint16]32)       # biBitCount
        $bw.Write([uint32]0)        # biCompression = BI_RGB
        $bw.Write([uint32]0)        # biSizeImage
        $bw.Write([int32]0)         # biXPelsPerMeter
        $bw.Write([int32]0)         # biYPelsPerMeter
        $bw.Write([uint32]0)        # biClrUsed
        $bw.Write([uint32]0)        # biClrImportant
        # XOR data: bottom-up rows (stride already padded to 4 bytes).
        for ($y = $h - 1; $y -ge 0; $y--) {
            $bw.Write($bytes, $y * $stride, $stride)
        }
        # AND mask: 1 bit per pixel, zero-filled (alpha lives in XOR), rows padded to 4 bytes.
        $maskRowBytes = [math]::Ceiling($w / 8.0)
        $maskStride = [int]([math]::Ceiling($maskRowBytes / 4.0) * 4)
        $maskRow = New-Object byte[] $maskStride
        for ($y = 0; $y -lt $h; $y++) { $bw.Write($maskRow) }
        $bw.Flush()                       # do NOT Close(): that disposes $ms before ToArray()
        $result = $ms.ToArray()
        $bw.Dispose()
        return ,$result                   # comma keeps the byte[] from being unrolled by PowerShell
    }

    $sizes = @(16, 32, 48, 64, 128, 256)
    $images = @{}
    foreach ($s in $sizes) {
        $bmp = New-IconBitmap $s
        $images[$s] = ConvertTo-Dib $bmp
        $bmp.Dispose()
    }

    # ICO container: ICONDIR + ICONDIRENTRY*N + PNG blobs.
    $fs = [System.IO.File]::Create($IcoPath)
    $bw = New-Object System.IO.BinaryWriter($fs)
    try {
        $bw.Write([uint16]0)                     # reserved
        $bw.Write([uint16]1)                     # type: icon
        $bw.Write([uint16]$sizes.Count)
        $offset = 6 + 16 * $sizes.Count
        foreach ($s in $sizes) {
            $data = $images[$s]
            $bw.Write([byte]$(if ($s -ge 256) { 0 } else { $s }))
            $bw.Write([byte]$(if ($s -ge 256) { 0 } else { $s }))
            $bw.Write([byte]0)                   # palette
            $bw.Write([byte]0)                   # reserved
            $bw.Write([uint16]1)                 # planes
            $bw.Write([uint16]32)                # bpp
            $bw.Write([uint32]$data.Length)
            $bw.Write([uint32]$offset)
            $offset += $data.Length
        }
        foreach ($s in $sizes) { $bw.Write($images[$s]) }
    } finally {
        $bw.Close(); $fs.Close()
    }
    Write-Host "Icon generated: $IcoPath"
}

# 1. Icon must exist before build.rs (winres) embeds it into the exe.
$icoDir = Join-Path $root "assets"
$ico = Join-Path $icoDir "compositor.ico"
if ($Force -or -not (Test-Path $ico)) {
    New-Item -ItemType Directory -Force -Path $icoDir | Out-Null
    New-CompositorIcon -IcoPath $ico
} else {
    Write-Host "Icon exists, skipping (use -Force to regenerate): $ico"
}

# 2. Release build.
# cargo may not be on PATH in non-interactive shells; resolve it explicitly.
$cargo = $null
if (Get-Command cargo -ErrorAction SilentlyContinue) {
    $cargo = (Get-Command cargo).Source
} else {
    $cargoDefault = Join-Path $env:USERPROFILE ".cargo\bin\cargo.exe"
    if (Test-Path $cargoDefault) { $cargo = $cargoDefault }
}
if (-not $cargo) { throw "cargo not found. Install Rust: https://rustup.rs" }

Write-Host "`n==> cargo build --release ..."
& $cargo build --release
if ($LASTEXITCODE -ne 0) { throw "cargo build failed (exit $LASTEXITCODE)" }

New-Item -ItemType Directory -Force -Path (Join-Path $root "dist") | Out-Null

$exe = Join-Path $root "target\release\compositor.exe"
if (-not (Test-Path $exe)) { throw "Release exe not found: $exe" }
$version = (Select-String -Path (Join-Path $root "Cargo.toml") -Pattern '^version = "(.+)"').Matches[0].Groups[1].Value

# 3a. Inno Setup.
$iscc = $null
if (Get-Command ISCC.exe -ErrorAction SilentlyContinue) {
    $iscc = (Get-Command ISCC.exe).Source
} else {
    foreach ($p in @("${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe", "$env:ProgramFiles\Inno Setup 6\ISCC.exe")) {
        if (Test-Path $p) { $iscc = $p; break }
    }
}
if ($iscc) {
    Write-Host "`n==> Inno Setup: $iscc"
    & $iscc "installer.iss"
    if ($LASTEXITCODE -ne 0) { throw "ISCC failed (exit $LASTEXITCODE)" }
    Write-Host "Done: dist\Compositor-Setup-$version.exe"
    exit 0
}

# 3b. cargo-wix (WiX toolset).
if (Get-Command cargo-wix -ErrorAction SilentlyContinue) {
    Write-Host "`n==> cargo wix ..."
    & cargo wix
    if ($LASTEXITCODE -ne 0) { throw "cargo wix failed (exit $LASTEXITCODE)" }
    Write-Host "Done: see target\wix directory."
    exit 0
}

# 3c. Portable zip fallback.
Write-Host "`n==> Neither Inno Setup nor cargo-wix found; producing a portable zip ..."
$zip = Join-Path $root "dist\Compositor-win64.zip"
if (Test-Path $zip) { Remove-Item $zip }
$tmp = Join-Path $env:TEMP "compositor-pack"
if (Test-Path $tmp) { Remove-Item $tmp -Recurse -Force }
New-Item -ItemType Directory -Force -Path $tmp | Out-Null
Copy-Item $exe (Join-Path $tmp "compositor.exe")
Copy-Item $ico (Join-Path $tmp "compositor.ico")
Compress-Archive -Path (Join-Path $tmp "*") -DestinationPath $zip -Force
Remove-Item $tmp -Recurse -Force
Write-Host "Done: $zip"
Write-Host "Hint: install Inno Setup and re-run this script to produce an installer."

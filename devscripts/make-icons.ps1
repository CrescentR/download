# Generates Yuki's icon set (ICO + PNGs) from code so the binary assets are
# reproducible instead of opaque. Run from anywhere:
#   pwsh -File devscripts/make-icons.ps1
#
# Design: a rounded app tile with a matte graphite fill, a white six-arm
# snowflake (雪) whose lower arm becomes a download arrow (下载).

Add-Type -AssemblyName System.Drawing

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
if (-not (Test-Path (Join-Path $root 'gui'))) { $root = (Get-Location).Path }

$iconDir = Join-Path $root 'gui/src-tauri/icons'
$publicDir = Join-Path $root 'gui/public'
New-Item -ItemType Directory -Force -Path $iconDir, $publicDir | Out-Null

function New-YukiBitmap([int]$size) {
  $bmp = New-Object System.Drawing.Bitmap($size, $size, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
  $g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
  $g.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality

  $s = [double]$size
  $pad = $s * 0.055
  $radius = $s * 0.235

  # --- rounded tile (drawn as a path so we can fill AND clip) --------------
  $tile = New-Object System.Drawing.Drawing2D.GraphicsPath
  $d = $radius * 2
  $x0 = $pad; $y0 = $pad; $x1 = $s - $pad; $y1 = $s - $pad
  $tile.AddArc($x0, $y0, $d, $d, 180, 90)
  $tile.AddArc($x1 - $d, $y0, $d, $d, 270, 90)
  $tile.AddArc($x1 - $d, $y1 - $d, $d, $d, 0, 90)
  $tile.AddArc($x0, $y1 - $d, $d, $d, 90, 90)
  $tile.CloseFigure()

  # Matte graphite tile; the app mark stays monochrome in both themes.
  $brush = New-Object System.Drawing.SolidBrush([System.Drawing.Color]::FromArgb(255, 24, 25, 28))
  $g.FillPath($brush, $tile)

  # --- snowflake: bold six-arm mark ---------------------------------------
  # Plain arms only. Barbs were tried and read as noise at 16-32px, which is
  # where this icon spends most of its life.
  $cx = $s * 0.5
  $cy = $s * 0.425
  $arm = $s * 0.205
  $thick = [Math]::Max(1.5, $s * 0.062)
  $white = New-Object System.Drawing.SolidBrush([System.Drawing.Color]::FromArgb(255, 255, 255, 255))

  $pen = New-Object System.Drawing.Pen($white, $thick)
  $pen.StartCap = [System.Drawing.Drawing2D.LineCap]::Round
  $pen.EndCap = [System.Drawing.Drawing2D.LineCap]::Round

  # The lower two arms flank the arrow; everything else fans upward.
  foreach ($degrees in @(0.0, 60.0, 120.0, 240.0, 300.0)) {
    $angle = $degrees * [Math]::PI / 180.0
    $ex = $cx + [Math]::Sin($angle) * $arm
    $ey = $cy - [Math]::Cos($angle) * $arm
    $g.DrawLine($pen, [float]$cx, [float]$cy, [float]$ex, [float]$ey)
  }

  # --- download arrow ------------------------------------------------------
  # The shaft continues straight out of the flake's downward arm, so the two
  # shapes form a single silhouette rather than two stacked icons.
  $shaftPen = New-Object System.Drawing.Pen($white, ($thick * 1.15))
  $shaftPen.StartCap = [System.Drawing.Drawing2D.LineCap]::Round
  $shaftPen.EndCap = [System.Drawing.Drawing2D.LineCap]::Round
  $g.DrawLine($shaftPen, [float]($s * 0.5), [float]($s * 0.425), [float]($s * 0.5), [float]($s * 0.645))

  $halfWidth = $s * 0.145
  $head = New-Object System.Drawing.Drawing2D.GraphicsPath
  $head.AddPolygon(@(
    (New-Object System.Drawing.PointF([float]($s * 0.5), [float]($s * 0.845))),
    (New-Object System.Drawing.PointF([float]($s * 0.5 - $halfWidth), [float]($s * 0.638))),
    (New-Object System.Drawing.PointF([float]($s * 0.5 + $halfWidth), [float]($s * 0.638)))
  ))
  $g.FillPath($white, $head)

  # --- baseline: the "captured" tray --------------------------------------
  $trayPen = New-Object System.Drawing.Pen($white, ($thick * 0.9))
  $trayPen.StartCap = [System.Drawing.Drawing2D.LineCap]::Round
  $trayPen.EndCap = [System.Drawing.Drawing2D.LineCap]::Round
  $trayY = $s * 0.905
  $g.DrawLine($trayPen, [float]($s * 0.295), [float]$trayY, [float]($s * 0.705), [float]$trayY)

  foreach ($item in @($pen, $shaftPen, $trayPen, $white, $brush)) { $item.Dispose() }
  $tile.Dispose(); $head.Dispose(); $g.Dispose()
  return $bmp
}

function Save-Png([System.Drawing.Bitmap]$bmp, [string]$path) {
  $bmp.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
}

# --- PNG set ---------------------------------------------------------------
$pngSizes = @{
  '32x32.png'         = 32
  '128x128.png'       = 128
  '128x128@2x.png'    = 256
  'icon.png'          = 512
  'Square30x30Logo.png'   = 30
  'Square44x44Logo.png'   = 44
  'Square71x71Logo.png'   = 71
  'Square89x89Logo.png'   = 89
  'Square107x107Logo.png' = 107
  'Square142x142Logo.png' = 142
  'Square150x150Logo.png' = 150
  'Square284x284Logo.png' = 284
  'Square310x310Logo.png' = 310
  'StoreLogo.png'         = 50
}

foreach ($name in $pngSizes.Keys) {
  $size = $pngSizes[$name]
  $bmp = New-YukiBitmap $size
  Save-Png $bmp (Join-Path $iconDir $name)
  $bmp.Dispose()
}

# --- favicon + in-app mark -------------------------------------------------
$fav = New-YukiBitmap 64
Save-Png $fav (Join-Path $publicDir 'favicon.png')
$fav.Dispose()
$fav32 = New-YukiBitmap 32
Save-Png $fav32 (Join-Path $publicDir 'favicon-32.png')
$fav32.Dispose()

# --- multi-resolution ICO --------------------------------------------------
# PNG-compressed ICO entries keep 256px crisp while staying small.
$icoSizes = @(16, 24, 32, 48, 64, 128, 256)
$pngs = @()
foreach ($size in $icoSizes) {
  $bmp = New-YukiBitmap $size
  $stream = New-Object System.IO.MemoryStream
  $bmp.Save($stream, [System.Drawing.Imaging.ImageFormat]::Png)
  $pngs += , @{ Size = $size; Bytes = $stream.ToArray() }
  $stream.Dispose(); $bmp.Dispose()
}

foreach ($target in @((Join-Path $iconDir 'icon.ico'), (Join-Path $publicDir 'favicon.ico'))) {
  $fs = [System.IO.File]::Create($target)
  $bw = New-Object System.IO.BinaryWriter($fs)
  $bw.Write([UInt16]0)                 # reserved
  $bw.Write([UInt16]1)                 # type: icon
  $bw.Write([UInt16]$pngs.Count)       # image count

  $offset = 6 + (16 * $pngs.Count)
  foreach ($entry in $pngs) {
    $dim = if ($entry.Size -ge 256) { 0 } else { $entry.Size }   # 0 means 256
    $bw.Write([Byte]$dim)              # width
    $bw.Write([Byte]$dim)              # height
    $bw.Write([Byte]0)                 # palette
    $bw.Write([Byte]0)                 # reserved
    $bw.Write([UInt16]1)               # color planes
    $bw.Write([UInt16]32)              # bits per pixel
    $bw.Write([UInt32]$entry.Bytes.Length)
    $bw.Write([UInt32]$offset)
    $offset += $entry.Bytes.Length
  }
  foreach ($entry in $pngs) { $bw.Write($entry.Bytes) }
  $bw.Flush(); $bw.Dispose(); $fs.Dispose()
}

Write-Host "Yuki icons written to $iconDir and $publicDir"
Get-ChildItem $iconDir | Select-Object Name, Length | Format-Table -AutoSize

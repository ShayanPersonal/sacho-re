param(
    [Parameter(Mandatory=$true, Position=0)]
    [string]$SessionDir
)

# Resolve GStreamer bin directory from the environment variable set by the installer
$gstRoot = [System.Environment]::GetEnvironmentVariable('GSTREAMER_1_0_ROOT_MSVC_X86_64', 'Machine')
if (-not $gstRoot) {
    $gstRoot = [System.Environment]::GetEnvironmentVariable('GSTREAMER_1_0_ROOT_MSVC_X86_64', 'User')
}
if (-not $gstRoot) {
    Write-Error "GStreamer not found. Set GSTREAMER_1_0_ROOT_MSVC_X86_64 or install GStreamer."
    exit 1
}
$gstBin = Join-Path $gstRoot "bin"
$gstDiscoverer = Join-Path $gstBin "gst-discoverer-1.0.exe"
$gstLaunch = Join-Path $gstBin "gst-launch-1.0.exe"

if (-not (Test-Path $gstDiscoverer)) {
    Write-Error "gst-discoverer-1.0.exe not found at $gstDiscoverer"
    exit 1
}
if (-not (Test-Path $gstLaunch)) {
    Write-Error "gst-launch-1.0.exe not found at $gstLaunch"
    exit 1
}

if (-not (Test-Path $SessionDir)) {
    Write-Error "Directory not found: $SessionDir"
    exit 1
}

$videoFiles = Get-ChildItem -Path $SessionDir -Filter "*.mkv" -File
if ($videoFiles.Count -eq 0) {
    Write-Host "No .mkv files found in $SessionDir"
    exit 0
}

foreach ($file in $videoFiles) {
    Write-Host "`n=== $($file.Name) ===" -ForegroundColor Cyan

    # --- Header info via gst-discoverer ---
    Write-Host "`n  Header (container metadata):" -ForegroundColor Yellow
    $discOutput = & $gstDiscoverer $file.FullName 2>&1

    $width = $null; $height = $null; $fpsNum = $null; $fpsDen = $null; $duration = $null; $codec = $null; $durationNs = $null
    foreach ($line in $discOutput) {
        $s = $line.ToString().Trim()
        if ($s -match "^\s*Width:\s*(\d+)")          { $width = [int]$Matches[1] }
        if ($s -match "^\s*Height:\s*(\d+)")         { $height = [int]$Matches[1] }
        if ($s -match "^\s*Frame rate:\s*(\d+)/(\d+)") {
            $fpsNum = [double]$Matches[1]; $fpsDen = [double]$Matches[2]
        }
        if ($s -match "^\s*Duration:\s*(\d+):(\d+):([\d.]+)") {
            $h = [double]$Matches[1]; $m = [double]$Matches[2]; $sec = [double]$Matches[3]
            $durationNs = ($h * 3600 + $m * 60 + $sec)
            $duration = $s -replace "^\s*Duration:\s*", ""
        }
        if ($s -match "^\s*video #\d+:\s*(.+)")      { $codec = $Matches[1] }
    }

    $headerFps = if ($fpsNum -and $fpsDen -and $fpsDen -ne 0) { [math]::Round($fpsNum / $fpsDen, 2) } else { $null }

    if ($width -and $height) { Write-Host "    Resolution: ${width}x${height}" }
    if ($headerFps)          { Write-Host "    Framerate:  $headerFps fps ($fpsNum/$fpsDen)" }
    if ($codec)              { Write-Host "    Codec:      $codec" }
    if ($duration)           { Write-Host "    Duration:   $duration" }

    # --- Measured: decode all frames, count them, compute actual fps ---
    Write-Host "`n  Measured (decoded frames):" -ForegroundColor Yellow

    # Decode all frames with fakesink and use GST_DEBUG=basesink:5 to get per-buffer
    # debug lines. Count unique PTS timestamps to get the true frame count.
    $filePath = $file.FullName.Replace('\', '/')
    $pipeline = "filesrc location=`"$filePath`" ! decodebin ! videoconvert ! `"video/x-raw`" ! fakesink sync=false"

    $pinfo = New-Object System.Diagnostics.ProcessStartInfo
    $pinfo.FileName = $gstLaunch
    $pinfo.Arguments = $pipeline
    $pinfo.RedirectStandardOutput = $true
    $pinfo.RedirectStandardError = $true
    $pinfo.UseShellExecute = $false
    $pinfo.CreateNoWindow = $true
    $pinfo.EnvironmentVariables["GST_DEBUG"] = "basesink:5"

    try {
        $proc = [System.Diagnostics.Process]::Start($pinfo)
        $stdoutTask = $proc.StandardOutput.ReadToEndAsync()
        $stderrTask = $proc.StandardError.ReadToEndAsync()
        $proc.WaitForExit(120000) | Out-Null
        $stdout = $stdoutTask.Result
        $stderr = $stderrTask.Result
    } catch {
        Write-Host "    ERROR: Failed to run gst-launch: $_" -ForegroundColor Red
        continue
    }

    $allOutput = "$stdout`n$stderr"

    # Parse resolution from negotiated caps in stderr
    if ($allOutput -match "video/x-raw.*?width=\(int\)(\d+).*?height=\(int\)(\d+)") {
        $measuredW = $Matches[1]; $measuredH = $Matches[2]
        Write-Host "    Resolution: ${measuredW}x${measuredH}"
    }

    # Count frames by extracting unique PTS timestamps from "got times start:" debug lines.
    # Each unique PTS corresponds to exactly one video frame.
    $uniquePts = @{}
    foreach ($line in ($allOutput -split "`n")) {
        if ($line -match "got times start:\s*([\d:.]+)") {
            $uniquePts[$Matches[1]] = $true
        }
    }
    $frameCount = $uniquePts.Count

    if ($frameCount -gt 0 -and $durationNs -and $durationNs -gt 0) {
        $measuredFps = [math]::Round($frameCount / $durationNs, 2)
        Write-Host "    Framerate:  $measuredFps fps ($frameCount frames / $([math]::Round($durationNs, 3))s)"

        if ($headerFps -and $headerFps -gt 0) {
            $ratio = $measuredFps / $headerFps
            if ($ratio -lt 0.75 -or $ratio -gt 1.25) {
                Write-Host "    ** MISMATCH: measured $measuredFps fps vs header $headerFps fps **" -ForegroundColor Red
            } else {
                Write-Host "    OK: measured fps matches header" -ForegroundColor Green
            }
        }
    } elseif ($frameCount -gt 0) {
        Write-Host "    Frames: $frameCount (could not compute fps - duration unknown)"
    } else {
        Write-Host "    Could not count frames (no buffer messages from pipeline)" -ForegroundColor DarkGray
    }
}

Write-Host ""

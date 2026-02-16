# Scans all bundled DLLs for references to patent-encumbered codecs.
#
# Checks two categories:
#   1. Codec implementations (ff_*_encoder/decoder symbols) — these mean actual
#      codec code is linked in. FAIL if found.
#   2. Codec name strings (e.g. "h264", "aac") — these are normal in FFmpeg's
#      libavcodec (AVCodecID descriptor table) and GStreamer plugin registries.
#      Reported as INFO, not a failure.
#
# Usage: powershell ./check_codecs.ps1

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path

# Patent-encumbered codec families to scan for
$codecPattern = '(?i)(h\.?264|h\.?265|hevc|libx264|libx265|aac|mpeg2video|mpeg4|mp3|amr_[nw]b|amr|ac3|eac3|dts|wmv[0-9]?|wma[a-z]*|rv40|cook|atrac|vorbis)'

# Actual codec implementation symbols — these indicate real codec code
$implPattern = 'ff_(h264|hevc|aac|mp3|mpeg[24]|ac3|eac3|amr|dts|wmv|wma|libx26[45]|rv40|cook|atrac|vorbis)[a-z0-9_]*(encoder|decoder)'

$dlls = Get-ChildItem -Path $scriptDir -Filter '*.dll' | Sort-Object Name
Write-Host "Scanning $($dlls.Count) DLLs for patent-encumbered codec references...`n" -ForegroundColor Cyan

$foundImpl = $false
$dllsWithNames = @()

foreach ($dll in $dlls) {
    $bytes = [System.IO.File]::ReadAllBytes($dll.FullName)
    $text = [System.Text.Encoding]::ASCII.GetString($bytes)

    # Check for actual codec implementations (FAIL)
    $implHits = [regex]::Matches($text, $implPattern)
    if ($implHits.Count -gt 0) {
        $unique = @{}
        foreach ($m in $implHits) { $unique[$m.Value] = $true }
        $list = ($unique.Keys | Sort-Object) -join ', '
        Write-Host "  FAIL  $($dll.Name)" -ForegroundColor Red
        Write-Host "        Codec implementations: $list" -ForegroundColor Red
        $foundImpl = $true
        continue
    }

    # Check for codec name strings (INFO only)
    $nameHits = [regex]::Matches($text, $codecPattern)
    if ($nameHits.Count -gt 0) {
        $unique = @{}
        foreach ($m in $nameHits) { $unique[$m.Value.ToLower()] = $true }
        $list = ($unique.Keys | Sort-Object) -join ', '
        Write-Host "  INFO  $($dll.Name)" -ForegroundColor Yellow
        Write-Host "        Codec name strings ($($unique.Count)): $list" -ForegroundColor DarkGray
        $dllsWithNames += $dll.Name
    } else {
        Write-Host "  OK    $($dll.Name)" -ForegroundColor Green
    }
}

# Summary
Write-Host ""
if ($foundImpl) {
    Write-Host "FAILED: Found DLLs with actual codec implementations." -ForegroundColor Red
    Write-Host "These DLLs contain patent-encumbered codec code that must be removed." -ForegroundColor Red
    exit 1
} elseif ($dllsWithNames.Count -gt 0) {
    Write-Host "PASSED: No codec implementations found." -ForegroundColor Green
    Write-Host "Note: $($dllsWithNames.Count) DLL(s) contain codec name strings (descriptor tables, not code)." -ForegroundColor DarkGray
    exit 0
} else {
    Write-Host "PASSED: No codec references found at all." -ForegroundColor Green
    exit 0
}

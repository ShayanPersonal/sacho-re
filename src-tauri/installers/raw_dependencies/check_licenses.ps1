# Check GStreamer plugin licenses
# This script uses gst-inspect-1.0 to verify all plugin DLLs are LGPL licensed
# It queries the system GStreamer installation for license info

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$gstInspect = "gst-inspect-1.0"

# Check if gst-inspect is available
try {
    $null = & $gstInspect --version 2>$null
    if ($LASTEXITCODE -ne 0) { throw "gst-inspect failed" }
} catch {
    Write-Host "ERROR: gst-inspect-1.0 not found. Make sure GStreamer is installed and in PATH." -ForegroundColor Red
    exit 1
}

Write-Host "Checking licenses for DLLs in: $scriptDir`n" -ForegroundColor Cyan
Write-Host "Using system GStreamer installation for license info.`n" -ForegroundColor Gray

$results = @{
    LGPL = @()
    GPL = @()
    Other = @()
    NotPlugin = @()
    Error = @()
}

# Get all DLL files
$dlls = Get-ChildItem -Path $scriptDir -Filter "*.dll"

foreach ($dll in $dlls) {
    $name = $dll.Name
    
    # Skip runtime DLLs (not plugins) - these have version numbers in the name
    # Runtime: gstbase-1.0-0.dll, glib-2.0-0.dll, etc.
    # Plugins: gstcoreelements.dll, gstplayback.dll, etc.
    if ($name -match "-\d+\.\d+-\d+\.dll$" -or $name -match "-\d+-\d+\.dll$" -or $name -notmatch "^gst") {
        $results.NotPlugin += $name
        continue
    }
    
    # Extract plugin name from DLL (remove gst prefix and .dll suffix)
    # e.g., gstcoreelements.dll -> coreelements
    $pluginName = $name -replace "^gst", "" -replace "\.dll$", ""
    
    # Query system GStreamer for plugin info
    $output = & $gstInspect $pluginName 2>&1 | Out-String
    
    # GStreamer output format: "  License                  LGPL"
    if ($output -match "License\s+(\S+)") {
        $license = $matches[1].Trim()
        
        if ($license -match "LGPL") {
            $results.LGPL += [PSCustomObject]@{ Name = $name; Plugin = $pluginName; License = $license }
        } elseif ($license -match "GPL") {
            $results.GPL += [PSCustomObject]@{ Name = $name; Plugin = $pluginName; License = $license }
        } else {
            $results.Other += [PSCustomObject]@{ Name = $name; Plugin = $pluginName; License = $license }
        }
    } else {
        $results.Error += [PSCustomObject]@{ Name = $name; Plugin = $pluginName }
    }
}

# Print results
Write-Host "=== LGPL Licensed (OK to distribute) ===" -ForegroundColor Green
if ($results.LGPL.Count -gt 0) {
    $results.LGPL | ForEach-Object { 
        Write-Host ("  {0,-35} {1}" -f $_.Name, $_.License)
    }
} else {
    Write-Host "  (none)"
}

Write-Host "`n=== GPL Licensed (Copyleft - review required) ===" -ForegroundColor Yellow
if ($results.GPL.Count -gt 0) {
    $results.GPL | ForEach-Object { 
        Write-Host ("  {0,-35} {1}" -f $_.Name, $_.License) -ForegroundColor Yellow
    }
} else {
    Write-Host "  (none)"
}

Write-Host "`n=== Other Licenses (Review required) ===" -ForegroundColor Red
if ($results.Other.Count -gt 0) {
    $results.Other | ForEach-Object { 
        Write-Host ("  {0,-35} {1}" -f $_.Name, $_.License) -ForegroundColor Red
    }
} else {
    Write-Host "  (none)"
}

Write-Host "`n=== Runtime/Support DLLs (Not GStreamer plugins) ===" -ForegroundColor Gray
if ($results.NotPlugin.Count -gt 0) {
    $results.NotPlugin | ForEach-Object { Write-Host "  $_" -ForegroundColor Gray }
    Write-Host "`n  Note: Runtime DLLs are from GLib/GStreamer core (LGPL)" -ForegroundColor DarkGray
}

Write-Host "`n=== Could not inspect (plugin not found in system GStreamer) ===" -ForegroundColor Magenta
if ($results.Error.Count -gt 0) {
    $results.Error | ForEach-Object { 
        Write-Host ("  {0,-35} (tried: {1})" -f $_.Name, $_.Plugin) -ForegroundColor Magenta
    }
} else {
    Write-Host "  (none)"
}

# Summary
Write-Host "`n=== Summary ===" -ForegroundColor Cyan
Write-Host "  LGPL:        $($results.LGPL.Count)"
Write-Host "  GPL:         $($results.GPL.Count)"
Write-Host "  Other:       $($results.Other.Count)"
Write-Host "  Runtime:     $($results.NotPlugin.Count)"
Write-Host "  Not found:   $($results.Error.Count)"

if ($results.GPL.Count -gt 0 -or $results.Other.Count -gt 0) {
    Write-Host "`nWARNING: Some plugins may have licensing concerns. Review before distribution." -ForegroundColor Yellow
    exit 1
} elseif ($results.Error.Count -gt 0) {
    Write-Host "`nWARNING: Some plugins could not be verified. They may not exist in your GStreamer installation." -ForegroundColor Yellow
    exit 1
} else {
    Write-Host "`nAll plugins are LGPL licensed. Safe to distribute." -ForegroundColor Green
    exit 0
}

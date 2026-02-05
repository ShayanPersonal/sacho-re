# Recursively check all DLL dependencies for bundled GStreamer plugins
#
# This script scans each bundled DLL binary for ALL .dll string references,
# catching both static PE imports AND dynamically loaded DLLs (LoadLibrary/g_module_open).
# It recursively follows dependencies and reports:
#   - Missing DLLs: referenced but not bundled
#   - Unused DLLs: bundled but not needed by any plugin
#
# Usage: .\check_deps.ps1 [-GStreamerRoot "C:\Program Files\gstreamer\1.0\msvc_x86_64"]

param(
    [string]$GStreamerRoot = ""
)

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path

# Try to find GStreamer installation
if (-not $GStreamerRoot) {
    $GStreamerRoot = $env:GSTREAMER_1_0_ROOT_MSVC_X86_64
}
if (-not $GStreamerRoot) {
    # Common locations
    $candidates = @(
        "C:\Program Files\gstreamer\1.0\msvc_x86_64",
        "C:\gstreamer\1.0\msvc_x86_64",
        "C:\gstreamer\1.0\x86_64"
    )
    foreach ($c in $candidates) {
        if (Test-Path "$c\bin") {
            $GStreamerRoot = $c
            break
        }
    }
}

if (-not $GStreamerRoot -or -not (Test-Path "$GStreamerRoot\bin")) {
    Write-Host "ERROR: Could not find GStreamer installation." -ForegroundColor Red
    Write-Host "Set GSTREAMER_1_0_ROOT_MSVC_X86_64 or pass -GStreamerRoot" -ForegroundColor Red
    exit 1
}

Write-Host "GStreamer root: $GStreamerRoot" -ForegroundColor Cyan
Write-Host "Bundle dir:    $scriptDir`n" -ForegroundColor Cyan

# Known search paths for GStreamer DLLs
$gstBinDir = "$GStreamerRoot\bin"
$gstPluginDir = "$GStreamerRoot\lib\gstreamer-1.0"

# Get all bundled DLLs
$bundledDlls = @{}
Get-ChildItem -Path $scriptDir -Filter "*.dll" | ForEach-Object {
    $bundledDlls[$_.Name.ToLower()] = $_.FullName
}

Write-Host "Bundled DLLs: $($bundledDlls.Count)" -ForegroundColor Gray

# Broad pattern to match ANY .dll reference in a binary (catches both static and dynamic imports)
$dllPattern = '[A-Za-z][A-Za-z0-9_.-]+\.dll'

# Windows system DLLs that are always present - no need to bundle these
$systemDlls = @(
    # Core Windows
    'kernel32.dll','kernelbase.dll','ntdll.dll','user32.dll','gdi32.dll',
    'advapi32.dll','shell32.dll','ole32.dll','oleaut32.dll','comctl32.dll',
    'comdlg32.dll','shlwapi.dll','version.dll','setupapi.dll','powrprof.dll',
    'userenv.dll','imm32.dll','normaliz.dll','dwmapi.dll','uxtheme.dll',
    'msimg32.dll','winspool.drv','propsys.dll',
    # Networking
    'ws2_32.dll','wsock32.dll','winhttp.dll','iphlpapi.dll','dnsapi.dll',
    'wldap32.dll',
    # CRT / MSVC runtime
    'msvcrt.dll','msvcp140.dll','vcruntime140.dll','vcruntime140_1.dll','ucrtbase.dll',
    # COM / RPC
    'combase.dll','rpcrt4.dll','windowsapp.dll','runtimeobject.dll',
    # Security / Crypto
    'crypt32.dll','secur32.dll','bcrypt.dll','ncrypt.dll',
    # Media Foundation
    'mfplat.dll','mf.dll','mfreadwrite.dll','mfuuid.dll','evr.dll',
    'wmvcore.dll','wmcodecdspuuid.dll',
    # DirectX / D3D
    'dxgi.dll','d3d9.dll','d3d11.dll','d3d12.dll','d3dcompiler_47.dll',
    'd3dcompiler_46.dll','d3dcompiler_45.dll','d3dcompiler_44.dll','d3dcompiler_43.dll',
    'dxva2.dll','opengl32.dll','dwrite.dll','d2d1.dll',
    # D3D debug/SDK layers (only present in dev environments)
    'd3d11sdklayers.dll','d3d11_1sdklayers.dll','d3d11_2sdklayers.dll','d3d11_3sdklayers.dll',
    'dxgidebug.dll',
    # Audio
    'winmm.dll','avrt.dll','ksuser.dll',
    # DirectShow
    'strmiids.dll','quartz.dll','msdmo.dll','dmoguids.dll','amstrmid.dll',
    # Misc
    'psapi.dll','dbghelp.dll','xmllite.dll',
    # Logitech injector (shows up in some webcam binaries)
    'lvprcinj.dll'
)

# Set to track which DLLs we've already checked (avoids infinite loops)
$checked = @{}
# Set to track all missing DLLs and what needs them
$missing = @{}
# Set to track DLLs we couldn't find anywhere
$notFound = @{}
# Dependency graph: dll -> list of deps
$depGraph = @{}

function Get-GstDependencies {
    param([string]$DllPath)
    
    if (-not (Test-Path $DllPath)) { return @() }
    
    try {
        $bytes = [System.IO.File]::ReadAllBytes($DllPath)
        $text = [System.Text.Encoding]::ASCII.GetString($bytes)
        $matches = [regex]::Matches($text, $dllPattern)
        $deps = $matches | ForEach-Object { $_.Value.ToLower() } | Sort-Object -Unique
        
        # Filter out self-references, Windows system DLLs, and api-ms/ext-ms shims
        $dllName = (Split-Path -Leaf $DllPath).ToLower()
        $deps = $deps | Where-Object { 
            $_ -ne $dllName -and
            $_ -notin $systemDlls -and
            $_ -notmatch '^(api-ms-|ext-ms-)' -and
            $_ -notmatch 'l1-1-0\.dll$'
        }
        
        return $deps
    } catch {
        Write-Host "  WARNING: Could not read $DllPath : $_" -ForegroundColor Yellow
        return @()
    }
}

function Find-GstDll {
    param([string]$DllName)
    
    # Check plugin dir first, then bin dir
    $pluginPath = Join-Path $gstPluginDir $DllName
    if (Test-Path $pluginPath) { return $pluginPath }
    
    $binPath = Join-Path $gstBinDir $DllName
    if (Test-Path $binPath) { return $binPath }
    
    return $null
}

function Check-DllRecursive {
    param(
        [string]$DllName,
        [string]$NeededBy
    )
    
    $dllNameLower = $DllName.ToLower()
    
    # Skip if already checked
    if ($checked.ContainsKey($dllNameLower)) { return }
    $checked[$dllNameLower] = $true
    
    # Find the DLL - first in bundle, then in GStreamer installation
    $dllPath = $null
    if ($bundledDlls.ContainsKey($dllNameLower)) {
        $dllPath = $bundledDlls[$dllNameLower]
    } else {
        $dllPath = Find-GstDll $dllNameLower
        if (-not $dllPath) {
            if (-not $notFound.ContainsKey($dllNameLower)) {
                $notFound[$dllNameLower] = @()
            }
            $notFound[$dllNameLower] += $NeededBy
            return
        }
    }
    
    # Get dependencies
    $deps = Get-GstDependencies -DllPath $dllPath
    $depGraph[$dllNameLower] = $deps
    
    foreach ($dep in $deps) {
        # Check if this dep is bundled
        if (-not $bundledDlls.ContainsKey($dep)) {
            if (-not $missing.ContainsKey($dep)) {
                $missing[$dep] = @()
            }
            $missing[$dep] += $dllNameLower
        }
        
        # Recursively check this dependency
        Check-DllRecursive -DllName $dep -NeededBy $dllNameLower
    }
}

# Check all bundled DLLs recursively
Write-Host "Scanning dependencies recursively...`n" -ForegroundColor Gray

foreach ($dllName in $bundledDlls.Keys | Sort-Object) {
    Check-DllRecursive -DllName $dllName -NeededBy "(bundled)"
}

# ============================================================================
# Classify DLLs: plugins vs runtime libraries
# ============================================================================
# Plugins: gst*.dll WITHOUT version numbers (e.g., gstmatroska.dll)
# Runtime: DLLs WITH version numbers (e.g., gstreamer-1.0-0.dll, glib-2.0-0.dll)
# Non-gst runtime: DLLs not starting with gst (e.g., ffi-7.dll, intl-8.dll)

$pluginDlls = @()
$runtimeDlls = @()

foreach ($dll in $bundledDlls.Keys | Sort-Object) {
    if ($dll -match "^gst" -and $dll -notmatch "-\d+\.\d+-\d+\.dll$") {
        $pluginDlls += $dll
    } else {
        $runtimeDlls += $dll
    }
}

# ============================================================================
# Find unused DLLs: bundled but not required by any plugin
# ============================================================================
# Walk the dependency graph from each plugin to find all reachable runtime DLLs

$reachable = @{}

function Trace-Reachable {
    param([string]$DllName)
    
    $dllNameLower = $DllName.ToLower()
    if ($reachable.ContainsKey($dllNameLower)) { return }
    $reachable[$dllNameLower] = $true
    
    if ($depGraph.ContainsKey($dllNameLower)) {
        foreach ($dep in $depGraph[$dllNameLower]) {
            Trace-Reachable -DllName $dep
        }
    }
}

# Trace from every plugin
foreach ($plugin in $pluginDlls) {
    Trace-Reachable -DllName $plugin
}

# Find unused: bundled DLLs that are not reachable from any plugin
$unusedDlls = @()
foreach ($dll in $bundledDlls.Keys | Sort-Object) {
    if (-not $reachable.ContainsKey($dll)) {
        $unusedDlls += $dll
    }
}

# ============================================================================
# Report results
# ============================================================================

$hasIssues = $false

# Missing DLLs
if ($missing.Count -gt 0) {
    $hasIssues = $true
    Write-Host "=== MISSING DLLs (not bundled but required) ===" -ForegroundColor Red
    foreach ($dll in $missing.Keys | Sort-Object) {
        $neededBy = ($missing[$dll] | Sort-Object -Unique) -join ", "
        $systemPath = Find-GstDll $dll
        $sizeInfo = ""
        if ($systemPath) {
            $size = (Get-Item $systemPath).Length
            $sizeKB = [math]::Round($size / 1024)
            $sizeInfo = " (${sizeKB} KB, found at: $systemPath)"
        }
        Write-Host "  $dll" -ForegroundColor Red -NoNewline
        Write-Host "  <- needed by: $neededBy$sizeInfo" -ForegroundColor Gray
    }
    Write-Host ""
}

# Not found DLLs
if ($notFound.Count -gt 0) {
    Write-Host "=== NOT FOUND (not in bundle or GStreamer installation) ===" -ForegroundColor Yellow
    foreach ($dll in $notFound.Keys | Sort-Object) {
        $neededBy = ($notFound[$dll] | Sort-Object -Unique) -join ", "
        Write-Host "  $dll" -ForegroundColor Yellow -NoNewline
        Write-Host "  <- needed by: $neededBy" -ForegroundColor Gray
    }
    Write-Host "  Note: These may be Windows system DLLs (usually OK to ignore)`n" -ForegroundColor DarkGray
}

# Unused DLLs
if ($unusedDlls.Count -gt 0) {
    $hasIssues = $true
    Write-Host "=== UNUSED DLLs (bundled but not needed by any plugin) ===" -ForegroundColor Yellow
    foreach ($dll in $unusedDlls) {
        $size = (Get-Item $bundledDlls[$dll]).Length
        $sizeKB = [math]::Round($size / 1024)
        Write-Host "  $dll" -ForegroundColor Yellow -NoNewline
        Write-Host "  (${sizeKB} KB)" -ForegroundColor Gray
    }
    Write-Host ""
}

# Plugin and runtime breakdown
Write-Host "=== Bundle breakdown ===" -ForegroundColor Cyan
Write-Host "  Plugins ($($pluginDlls.Count)):" -ForegroundColor White
foreach ($p in $pluginDlls) {
    $marker = if ($unusedDlls -contains $p) { " (UNUSED)" } else { "" }
    $color = if ($marker) { "Yellow" } else { "Gray" }
    Write-Host "    $p$marker" -ForegroundColor $color
}
Write-Host "  Runtime libraries ($($runtimeDlls.Count)):" -ForegroundColor White
foreach ($r in $runtimeDlls) {
    $marker = if ($unusedDlls -contains $r) { " (UNUSED)" } else { "" }
    $color = if ($marker) { "Yellow" } else { "Gray" }
    Write-Host "    $r$marker" -ForegroundColor $color
}

# Summary
Write-Host "`n=== Summary ===" -ForegroundColor Cyan
Write-Host "  Bundled:     $($bundledDlls.Count) ($($pluginDlls.Count) plugins + $($runtimeDlls.Count) runtime)"
Write-Host "  Checked:     $($checked.Count) (recursive)"
Write-Host "  Missing:     $($missing.Count)" -ForegroundColor $(if ($missing.Count -gt 0) { "Red" } else { "Green" })
Write-Host "  Unused:      $($unusedDlls.Count)" -ForegroundColor $(if ($unusedDlls.Count -gt 0) { "Yellow" } else { "Green" })

if ($missing.Count -eq 0 -and $unusedDlls.Count -eq 0) {
    Write-Host "`nALL CLEAR - No missing or unused DLLs." -ForegroundColor Green
    exit 0
} elseif ($missing.Count -gt 0) {
    Write-Host "`nTo fix missing, copy the DLLs to: $scriptDir" -ForegroundColor Yellow
    exit 1
} else {
    Write-Host "`nUnused DLLs can be removed to reduce bundle size." -ForegroundColor Yellow
    exit 0
}

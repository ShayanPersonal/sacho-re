# Check largest folders in user profile
Write-Host "`n=== Largest folders in C:\Users\Moejoe ===`n"
Get-ChildItem 'C:\Users\Moejoe' -Directory -ErrorAction SilentlyContinue | ForEach-Object {
    $s = (Get-ChildItem $_.FullName -Recurse -File -ErrorAction SilentlyContinue | Measure-Object -Property Length -Sum).Sum
    if ($s -gt 50MB) {
        [PSCustomObject]@{Folder=$_.Name; SizeGB=[math]::Round($s/1GB,2)}
    }
} | Sort-Object SizeGB -Descending | Format-Table -AutoSize

# Check Rust cargo cache
Write-Host "`n=== Cargo cache ===`n"
$cargoPath = "$env:USERPROFILE\.cargo"
if (Test-Path $cargoPath) {
    Get-ChildItem $cargoPath -Directory -ErrorAction SilentlyContinue | ForEach-Object {
        $s = (Get-ChildItem $_.FullName -Recurse -File -ErrorAction SilentlyContinue | Measure-Object -Property Length -Sum).Sum
        if ($s -gt 10MB) {
            [PSCustomObject]@{Folder=".cargo\$($_.Name)"; SizeMB=[math]::Round($s/1MB,0)}
        }
    } | Sort-Object SizeMB -Descending | Format-Table -AutoSize
}

# Check for Rust target dirs in code folder
Write-Host "`n=== Rust target/ dirs in Desktop\code ===`n"
Get-ChildItem 'C:\Users\Moejoe\Desktop\code' -Directory -Recurse -Filter 'target' -Depth 2 -ErrorAction SilentlyContinue | ForEach-Object {
    $s = (Get-ChildItem $_.FullName -Recurse -File -ErrorAction SilentlyContinue | Measure-Object -Property Length -Sum).Sum
    if ($s -gt 50MB) {
        [PSCustomObject]@{Path=$_.FullName.Replace('C:\Users\Moejoe\Desktop\code\',''); SizeGB=[math]::Round($s/1GB,2)}
    }
} | Sort-Object SizeGB -Descending | Format-Table -AutoSize

# Check temp folders
Write-Host "`n=== Temp folders ===`n"
$tempPaths = @($env:TEMP, "$env:LOCALAPPDATA\Temp")
foreach ($tp in $tempPaths) {
    if (Test-Path $tp) {
        $s = (Get-ChildItem $tp -Recurse -File -ErrorAction SilentlyContinue | Measure-Object -Property Length -Sum).Sum
        Write-Host "$tp : $([math]::Round($s/1GB,2)) GB"
    }
}

# Check node_modules in code folder
Write-Host "`n=== node_modules dirs in Desktop\code ===`n"
Get-ChildItem 'C:\Users\Moejoe\Desktop\code' -Directory -Recurse -Filter 'node_modules' -Depth 2 -ErrorAction SilentlyContinue | ForEach-Object {
    $s = (Get-ChildItem $_.FullName -Recurse -File -ErrorAction SilentlyContinue | Measure-Object -Property Length -Sum).Sum
    if ($s -gt 50MB) {
        [PSCustomObject]@{Path=$_.FullName.Replace('C:\Users\Moejoe\Desktop\code\',''); SizeGB=[math]::Round($s/1GB,2)}
    }
} | Sort-Object SizeGB -Descending | Format-Table -AutoSize

# Check AppData\Local for big items
Write-Host "`n=== Largest in AppData\Local (>500MB) ===`n"
Get-ChildItem "$env:LOCALAPPDATA" -Directory -ErrorAction SilentlyContinue | ForEach-Object {
    $s = (Get-ChildItem $_.FullName -Recurse -File -ErrorAction SilentlyContinue | Measure-Object -Property Length -Sum).Sum
    if ($s -gt 500MB) {
        [PSCustomObject]@{Folder=$_.Name; SizeGB=[math]::Round($s/1GB,2)}
    }
} | Sort-Object SizeGB -Descending | Format-Table -AutoSize

# Check AppData\Local\Packages
Write-Host "`n=== Largest in AppData\Local\Packages (>1GB) ===`n"
Get-ChildItem "$env:LOCALAPPDATA\Packages" -Directory -ErrorAction SilentlyContinue | ForEach-Object {
    $s = (Get-ChildItem $_.FullName -Recurse -File -ErrorAction SilentlyContinue | Measure-Object -Property Length -Sum).Sum
    if ($s -gt 1GB) {
        [PSCustomObject]@{Folder=$_.Name; SizeGB=[math]::Round($s/1GB,2)}
    }
} | Sort-Object SizeGB -Descending | Format-Table -AutoSize

# Check Documents
Write-Host "`n=== Largest in Documents (>1GB) ===`n"
Get-ChildItem "$env:USERPROFILE\Documents" -Directory -ErrorAction SilentlyContinue | ForEach-Object {
    $s = (Get-ChildItem $_.FullName -Recurse -File -ErrorAction SilentlyContinue | Measure-Object -Property Length -Sum).Sum
    if ($s -gt 1GB) {
        [PSCustomObject]@{Folder=$_.Name; SizeGB=[math]::Round($s/1GB,2)}
    }
} | Sort-Object SizeGB -Descending | Format-Table -AutoSize

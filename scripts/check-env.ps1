# QuickWin environment check script
# Usage: powershell -ExecutionPolicy Bypass -File scripts/check-env.ps1

$ErrorActionPreference = 'SilentlyContinue'
$script:pass = 0
$script:fail = 0

function Check-Tool {
    param($name, $cmd)
    
    $output = & cmd /c "$cmd" 2>&1
    if ($LASTEXITCODE -eq 0 -and $output) {
        $version = ($output | Select-Object -First 1).ToString().Trim()
        Write-Host "  [OK] $name : $version" -ForegroundColor Green
        $script:pass++
        return $version
    } else {
        Write-Host "  [FAIL] $name : not found or failed" -ForegroundColor Red
        $script:fail++
        return $null
    }
}

Write-Host "`n=== QuickWin Environment Check ===" -ForegroundColor Cyan

# OS
$os = [System.Environment]::OSVersion.VersionString
$arch = $env:PROCESSOR_ARCHITECTURE
if ([System.Environment]::OSVersion.Version.Major -ge 10) {
    Write-Host "  [OK] OS : $os ($arch)" -ForegroundColor Green
    $script:pass++
} else {
    Write-Host "  [FAIL] OS : $os (requires Windows 10 1809+)" -ForegroundColor Red
    $script:fail++
}

# Node
$nodeVer = Check-Tool "Node.js" "node -v"
if ($nodeVer -and $nodeVer -match 'v(\d+)') {
    $major = [int]$Matches[1]
    if ($major -lt 20) {
        Write-Host "       Node version too old (requires >= 20)" -ForegroundColor Yellow
    }
}

# npm
Check-Tool "npm" "npm.cmd -v"

# Rust
Check-Tool "Rust" "rustc -V"
Check-Tool "Cargo" "cargo -V"

$rustupOut = & rustup show 2>&1
if ($rustupOut -match 'stable-x86_64-pc-windows-msvc') {
    Write-Host "  [OK] Rustup : stable-x86_64-pc-windows-msvc" -ForegroundColor Green
    $script:pass++
} else {
    Write-Host "  [FAIL] Rustup : stable-x86_64-pc-windows-msvc not found" -ForegroundColor Red
    $script:fail++
}

# rustfmt / clippy
$components = & rustup component list --installed 2>&1
if ($components -match 'rustfmt') {
    Write-Host "  [OK] rustfmt : installed" -ForegroundColor Green
    $script:pass++
} else {
    Write-Host "  [FAIL] rustfmt : not installed" -ForegroundColor Red
    $script:fail++
}

if ($components -match 'clippy') {
    Write-Host "  [OK] clippy : installed" -ForegroundColor Green
    $script:pass++
} else {
    Write-Host "  [FAIL] clippy : not installed" -ForegroundColor Red
    $script:fail++
}

# MSVC
$vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
if (Test-Path $vswhere) {
    $vsPath = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
    if ($vsPath) {
        Write-Host "  [OK] MSVC : $vsPath" -ForegroundColor Green
        $script:pass++
    } else {
        Write-Host "  [FAIL] MSVC : VC.Tools.x86.x64 component not found" -ForegroundColor Red
        $script:fail++
    }
} else {
    Write-Host "  [FAIL] MSVC : vswhere not found" -ForegroundColor Red
    $script:fail++
}

# WebView2
$wvPath = 'C:\Program Files (x86)\Microsoft\EdgeWebView\Application'
if (Test-Path $wvPath) {
    $wvVersions = Get-ChildItem $wvPath -Directory | Select-Object -ExpandProperty Name
    Write-Host "  [OK] WebView2 : $($wvVersions -join ', ')" -ForegroundColor Green
    $script:pass++
} else {
    Write-Host "  [FAIL] WebView2 : not found" -ForegroundColor Red
    $script:fail++
}

# Git
Check-Tool "Git" "git --version"

# Summary
Write-Host "`n=== Results ===" -ForegroundColor Cyan
Write-Host "  Passed: $script:pass" -ForegroundColor Green
if ($script:fail -gt 0) {
    Write-Host "  Failed: $script:fail" -ForegroundColor Red
    Write-Host "`nPlease install missing dependencies." -ForegroundColor Yellow
    exit 1
} else {
    Write-Host "`nAll dependencies ready!" -ForegroundColor Green
    exit 0
}
# QuickWin 环境自检脚本
# 用法: .\scripts\check-env.ps1
# 输出: 表格化的通过/失败状态

$ErrorActionPreference = 'Continue'

function Write-Status {
    param([string]$Name, [bool]$Pass, [string]$Detail)
    $icon = if ($Pass) { '✅' } else { '❌' }
    Write-Host "$icon $Name".PadRight(20) -NoNewline
    Write-Host " $Detail"
}

Write-Host "`n=== QuickWin 环境检查 ===" -ForegroundColor Cyan

# Rust
try {
    $rustc = rustc -V 2>$null
    if ($rustc -match 'rustc (\d+\.\d+\.\d+)') {
        Write-Status 'Rust' $true "v$($Matches[1])"
    } else {
        Write-Status 'Rust' $false '无法解析版本'
    }
} catch {
    Write-Status 'Rust' $false '未安装'
}

# Rustup
try {
    $rustup = rustup show 2>$null | Select-String 'Default host'
    if ($rustup -match 'x86_64-pc-windows-msvc') {
        Write-Status 'Rustup' $true 'MSVC target'
    } else {
        Write-Status 'Rustup' $false '非 MSVC target'
    }
} catch {
    Write-Status 'Rustup' $false '未安装'
}

# Rust components
$components = rustup component list --installed 2>$null
$hasFmt = $components -contains 'rustfmt'
$hasClippy = $components -contains 'clippy'
Write-Status 'rustfmt' $hasFmt $(if ($hasFmt) { '已安装' } else { '未安装' })
Write-Status 'clippy' $hasClippy $(if ($hasClippy) { '已安装' } else { '未安装' })

# Node
try {
    $node = node -v 2>$null
    if ($node -match 'v(\d+)\.') {
        $major = [int]$Matches[1]
        $ok = $major -ge 20
        Write-Status 'Node' $ok "$node (要求 >= 20)"
    } else {
        Write-Status 'Node' $false '无法解析版本'
    }
} catch {
    Write-Status 'Node' $false '未安装'
}

# npm
try {
    $npm = npm.cmd -v 2>$null
    if ($npm -match '^(\d+)\.') {
        $major = [int]$Matches[1]
        $ok = $major -ge 10
        Write-Status 'npm' $ok "v$npm (要求 >= 10)"
    } else {
        Write-Status 'npm' $false '无法解析版本'
    }
} catch {
    Write-Status 'npm' $false '未安装或执行策略拦截（用 npm.cmd）'
}

# MSVC Build Tools
try {
    $vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
    if (Test-Path $vswhere) {
        $vc = & $vswhere -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -format value -property installationPath 2>$null
        if ($vc) {
            Write-Status 'MSVC' $true 'C++ 桌面开发已安装'
        } else {
            Write-Status 'MSVC' $false '缺少 VC++ 工具集'
        }
    } else {
        Write-Status 'MSVC' $false 'vswhere 未找到'
    }
} catch {
    Write-Status 'MSVC' $false '检测失败'
}

# WebView2
$wv2Paths = @(
    "${env:ProgramFiles(x86)}\Microsoft\EdgeWebView\Application",
    "${env:ProgramFiles}\Microsoft\EdgeWebView\Application"
)
$wv2Found = $wv2Paths | Where-Object { Test-Path $_ }
Write-Status 'WebView2' ($null -ne $wv2Found) $(if ($wv2Found) { '已安装' } else { '未找到' })

# PowerShell 执行策略
$policy = Get-ExecutionPolicy -Scope CurrentUser
$policyOk = $policy -in @('RemoteSigned', 'Unrestricted')
Write-Status 'PS Policy' $policyOk "$policy (建议 RemoteSigned)"

Write-Host "`n=== 检查完成 ===" -ForegroundColor Cyan

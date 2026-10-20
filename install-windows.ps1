<#
.SYNOPSIS
    Install screen-ruler and register a global keyboard shortcut on Windows.

.DESCRIPTION
    Copies screen-ruler.exe to an install directory (default: current
    directory), creates a Start Menu shortcut with a Ctrl+Shift+R
    hotkey, and adds the binary to the user PATH.

.PARAMETER InstallDir
    Directory to install the binary into (default: current directory).

.PARAMETER Binary
    The executable to install (default: screen-ruler.exe next to this script,
    as in a release archive, else target\release\screen-ruler.exe after
    `cargo build --release`).

.EXAMPLE
    .\install-windows.ps1
    .\install-windows.ps1 -InstallDir "$env:LOCALAPPDATA\screen-ruler"
#>

param(
    [string]$InstallDir = (Get-Location).Path,
    [string]$Binary
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$AppName = 'Screen Ruler'
$AppId   = 'screen-ruler'

# --- Locate the built binary ------------------------------------------------

if (-not $Binary) {
    $Binary = @(
        (Join-Path $PSScriptRoot 'screen-ruler.exe'),
        (Join-Path $PSScriptRoot 'target\release\screen-ruler.exe')
    ) | Where-Object { Test-Path $_ } | Select-Object -First 1
}
if (-not $Binary -or -not (Test-Path $Binary)) {
    Write-Error "screen-ruler.exe not found.`nBuild it first:  cargo build --release   (or pass -Binary C:\path\to\screen-ruler.exe)" -ErrorAction Continue
    exit 1
}

# --- Copy binary to install dir ---------------------------------------------

if (-not (Test-Path $InstallDir)) {
    New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
}
$InstallDir = (Resolve-Path $InstallDir).Path
$ExecPath   = Join-Path $InstallDir "$AppId.exe"

$BinaryFull = (Resolve-Path $Binary).Path
if ($ExecPath -ne $BinaryFull) {
    Copy-Item -Path $BinaryFull -Destination $ExecPath -Force
    Write-Host "Installed binary -> $ExecPath"
} else {
    Write-Host "Binary already at $ExecPath"
}

# --- Add to user PATH (if not already present) ------------------------------

$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')

# Normalize install directory for comparison
$normalizedInstallDir = [IO.Path]::GetFullPath($InstallDir).TrimEnd('\').ToLowerInvariant()

# Split existing PATH into entries, normalize, and check for presence
$pathEntries = @()
if (-not [string]::IsNullOrEmpty($userPath)) {
    $pathEntries = $userPath -split ';' | Where-Object { -not [string]::IsNullOrWhiteSpace($_) }
}

$hasInstallDir = $false
foreach ($entry in $pathEntries) {
    $expandedEntry = [Environment]::ExpandEnvironmentVariables($entry)
    try {
        $normalizedEntry = [IO.Path]::GetFullPath($expandedEntry).TrimEnd('\').ToLowerInvariant()
    } catch {
        continue
    }
    if ($normalizedEntry -eq $normalizedInstallDir) {
        $hasInstallDir = $true
        break
    }
}

if (-not $hasInstallDir) {
    if ([string]::IsNullOrEmpty($userPath)) {
        $newPath = $InstallDir
    } else {
        $newPath = $userPath.TrimEnd(';') + ';' + $InstallDir
    }
    [Environment]::SetEnvironmentVariable('Path', $newPath, 'User')
    Write-Host "Added $InstallDir to user PATH (restart your terminal to pick it up)."
}

# --- Create Start Menu shortcut with hotkey ---------------------------------

$startMenu   = [Environment]::GetFolderPath('StartMenu')
$programsDir = Join-Path $startMenu 'Programs'
New-Item -ItemType Directory -Path $programsDir -Force | Out-Null
$lnkPath     = Join-Path $programsDir "$AppName.lnk"

$shell    = New-Object -ComObject WScript.Shell
$shortcut = $shell.CreateShortcut($lnkPath)
$shortcut.TargetPath  = $ExecPath
$shortcut.Description = 'Measure on-screen UI elements using edge detection'
$shortcut.WorkingDirectory = $InstallDir
# Hotkey format: "Modifier+Key" — Ctrl+Shift+R
$shortcut.Hotkey = 'Ctrl+Shift+R'
$shortcut.Save()

Write-Host "Created Start Menu shortcut -> $lnkPath"
Write-Host "  Hotkey: Ctrl+Shift+R"

# --- Done -------------------------------------------------------------------

Write-Host ''
Write-Host "Done. Press Ctrl+Shift+R to launch $AppName."
Write-Host 'Note: the shortcut hotkey requires the .lnk to stay in the Start Menu folder.'

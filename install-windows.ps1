<#
.SYNOPSIS
    Install screen-ruler and register a global keyboard shortcut on Windows.

.DESCRIPTION
    Copies the built screen-ruler.exe binary to an install directory (default:
    current directory), creates a Start Menu shortcut with a Ctrl+Shift+R
    hotkey, and adds the binary to the user PATH.

.PARAMETER InstallDir
    Directory to install the binary into (default: current directory).

.EXAMPLE
    .\install-windows.ps1
    .\install-windows.ps1 -InstallDir "$env:LOCALAPPDATA\screen-ruler"
#>

param(
    [string]$InstallDir = (Get-Location).Path
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$AppName = 'Screen Ruler'
$AppId   = 'screen-ruler'
$Binary  = Join-Path $PSScriptRoot 'dist' 'screen-ruler.exe'

# --- Locate the built binary ------------------------------------------------

if (-not (Test-Path $Binary)) {
    Write-Error "Binary not found at $Binary.`nBuild it first:  pyinstaller screen_ruler.spec" -ErrorAction Continue
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
    $normalizedEntry = [IO.Path]::GetFullPath($entry).TrimEnd('\').ToLowerInvariant()
    if ($normalizedEntry -eq $normalizedInstallDir) {
        $hasInstallDir = $true
        break
    }
}

if (-not $hasInstallDir) {
    if ([string]::IsNullOrEmpty($userPath)) {
        $newPath = $InstallDir
    } else {
        $newPath = "$userPath;$InstallDir"
    }
    [Environment]::SetEnvironmentVariable('Path', $newPath, 'User')
    Write-Host "Added $InstallDir to user PATH (restart your terminal to pick it up)."
}

# --- Create Start Menu shortcut with hotkey ---------------------------------

$startMenu   = [Environment]::GetFolderPath('StartMenu')
$programsDir = Join-Path $startMenu 'Programs'
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

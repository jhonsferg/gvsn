#!/usr/bin/env pwsh
# gvsn (Go Version Manager) - Windows installer
#
# Usage (one-liner):
#   irm https://raw.githubusercontent.com/jhonsferg/gvsn/main/install/install.ps1 | iex
#
# Customise with environment variables before piping:
#   $env:GVSN_INSTALL_DIR = "$env:USERPROFILE\.local\bin"
#   $env:GVSN_VERSION     = "v1.0.0"
#
# Override base URLs for local/offline testing:
#   $env:GVSN_TEST_API_BASE = "http://localhost:8765"   # replaces https://api.github.com
#   $env:GVSN_TEST_DL_BASE  = "http://localhost:8765"   # replaces https://github.com

$ErrorActionPreference = "Stop"

# Older PowerShell 5.1 / .NET Framework hosts default to TLS 1.0/1.1, which
# GitHub rejects outright - that shows up as a confusing generic connection
# error with no mention of TLS. Force 1.2 (and 1.3 where the host knows it)
# up front so failures further down are real failures, not a protocol
# mismatch.
try {
    [Net.ServicePointManager]::SecurityProtocol = `
        [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
} catch {}
try {
    [Net.ServicePointManager]::SecurityProtocol = `
        [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls13
} catch {}

$REPO        = "jhonsferg/gvsn"
$InstallDir  = if ($env:GVSN_INSTALL_DIR)    { $env:GVSN_INSTALL_DIR }    else { "$env:USERPROFILE\.local\bin" }
$Version     = if ($env:GVSN_VERSION)        { $env:GVSN_VERSION }        else { "latest" }
$ApiBase     = if ($env:GVSN_TEST_API_BASE)  { $env:GVSN_TEST_API_BASE }  else { "https://api.github.com" }
$DlBase      = if ($env:GVSN_TEST_DL_BASE)   { $env:GVSN_TEST_DL_BASE }   else { "https://github.com" }

# -- Terminal helpers ----------------------------------------------------------
function Write-Step([string]$msg) { Write-Host "  -> $msg" -ForegroundColor Cyan }
function Write-Ok([string]$msg)   { Write-Host "  v  $msg" -ForegroundColor Green }
function Abort([string]$msg) {
    Write-Host "`n  x  $msg" -ForegroundColor Red
    exit 1
}

# Extracts the HTTP status code from a web-request error, if any. Works
# across both the WebException thrown by Windows PowerShell 5.1 and the
# HttpResponseException thrown by PowerShell 7+.
function Get-HttpStatusCode($ErrorRecord) {
    try {
        if ($ErrorRecord.Exception.Response) {
            return [int]$ErrorRecord.Exception.Response.StatusCode
        }
    }
    catch {}
    return $null
}

# Downloads $Uri to $OutFile, retrying transient failures with a short
# exponential back-off instead of aborting on the first hiccup.
function Invoke-DownloadWithRetry([string]$Uri, [string]$OutFile, [int]$Retries = 3, [int]$TimeoutSec = 300) {
    $attempt = 0
    while ($true) {
        try {
            $ProgressPreference = "SilentlyContinue"
            Invoke-WebRequest -Uri $Uri -OutFile $OutFile -UseBasicParsing -TimeoutSec $TimeoutSec
            return
        }
        catch {
            $attempt++
            if ($attempt -gt $Retries) { throw }
            Write-Step "Download failed, retrying ($attempt/$Retries)..."
            Start-Sleep -Seconds ([Math]::Pow(2, $attempt))
        }
    }
}

Write-Host ""
Write-Host "  gvsn" -ForegroundColor Cyan -NoNewline
Write-Host " -- Go Version Manager installer" -ForegroundColor White
Write-Host ""

# -- 1. Detect architecture ----------------------------------------------------
# OSArchitecture (not ProcessArchitecture) so this reports the real hardware
# even when PowerShell itself is running under x64 emulation on Windows ARM64.
$isArm = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture `
         -eq [System.Runtime.InteropServices.Architecture]::Arm64

$Arch = if ($isArm) { "arm64" } else { "x86_64" }

if (-not [System.Environment]::Is64BitOperatingSystem) {
    Abort "32-bit Windows is not supported."
}

Write-Step "Detected platform: windows-$Arch"

# -- 2. Resolve version --------------------------------------------------------
if ($Version -eq "latest") {
    Write-Step "Fetching latest release from $ApiBase..."
    try {
        $rel     = Invoke-RestMethod "$ApiBase/repos/$REPO/releases/latest" -TimeoutSec 30
        $Version = $rel.tag_name
    }
    catch {
        $status = Get-HttpStatusCode $_
        if ($status -eq 403) {
            Abort "GitHub API rate limit exceeded (HTTP 403). Try again later, or set `$env:GVSN_VERSION explicitly."
        }
        elseif ($status) {
            Abort "Could not fetch latest version (HTTP $status): $_"
        }
        else {
            Abort "Could not fetch latest version: $_"
        }
    }
}

Write-Step "Installing gvsn $Version"

# -- 3. Download archive -------------------------------------------------------
$ArchiveName   = "gvsn_windows_$Arch.zip"
$DownloadUrl   = "$DlBase/$REPO/releases/download/$Version/$ArchiveName"
$ChecksumsUrl  = "$DlBase/$REPO/releases/download/$Version/checksums.txt"
$TmpZip        = [System.IO.Path]::Combine(
                     [System.IO.Path]::GetTempPath(),
                     "gvsn-install-$([System.Guid]::NewGuid()).zip"
                 )
$TmpChecksums  = [System.IO.Path]::Combine(
                     [System.IO.Path]::GetTempPath(),
                     "gvsn-install-$([System.Guid]::NewGuid()).checksums.txt"
                 )

Write-Step "Downloading $ArchiveName from $DownloadUrl..."

try {
    Invoke-DownloadWithRetry -Uri $DownloadUrl -OutFile $TmpZip
}
catch {
    if (Test-Path $TmpZip) { Remove-Item -Force $TmpZip }
    Abort "Download failed.`n  URL: $DownloadUrl`n  Error: $_"
}

Write-Step "Verifying checksum..."

try {
    Invoke-DownloadWithRetry -Uri $ChecksumsUrl -OutFile $TmpChecksums -TimeoutSec 30
}
catch {
    Remove-Item -Force $TmpZip, $TmpChecksums -ErrorAction SilentlyContinue
    Abort "Failed to download checksums.txt for verification.`n  URL: $ChecksumsUrl`n  Error: $_"
}

$expectedLine = Select-String -Path $TmpChecksums -Pattern "\s$([regex]::Escape($ArchiveName))$"
if (-not $expectedLine) {
    Remove-Item -Force $TmpZip, $TmpChecksums -ErrorAction SilentlyContinue
    Abort "No checksum entry found for $ArchiveName in checksums.txt"
}
$expectedSha = ($expectedLine.Line -split '\s+')[0].ToLowerInvariant()
$actualSha   = (Get-FileHash -Path $TmpZip -Algorithm SHA256).Hash.ToLowerInvariant()

if ($expectedSha -ne $actualSha) {
    Remove-Item -Force $TmpZip, $TmpChecksums -ErrorAction SilentlyContinue
    Abort "Checksum mismatch for $ArchiveName!`n  expected: $expectedSha`n  got:      $actualSha"
}
Remove-Item -Force $TmpChecksums -ErrorAction SilentlyContinue
Write-Ok "Checksum verified"

# -- 4. Extract and install binary ---------------------------------------------
if (-not (Test-Path $InstallDir)) {
    New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
}

$Dest       = Join-Path $InstallDir "gvsn.exe"
$TmpExtract = [System.IO.Path]::Combine(
                  [System.IO.Path]::GetTempPath(),
                  "gvsn-install-extract-$([System.Guid]::NewGuid())"
              )

Write-Step "Extracting..."

try {
    Expand-Archive -Path $TmpZip -DestinationPath $TmpExtract -Force
    $ExtractedExe = Join-Path $TmpExtract "gvsn.exe"
    if (-not (Test-Path $ExtractedExe)) {
        Abort "Archive did not contain gvsn.exe"
    }
    if (Test-Path $Dest) { Remove-Item -Force $Dest -ErrorAction SilentlyContinue }
    Move-Item -Force $ExtractedExe $Dest
}
catch {
    Abort "Extraction failed: $_"
}
finally {
    if (Test-Path $TmpExtract) { Remove-Item -Recurse -Force $TmpExtract -ErrorAction SilentlyContinue }
    if (Test-Path $TmpZip)     { Remove-Item -Force $TmpZip -ErrorAction SilentlyContinue }
}

Write-Ok "Installed to $Dest"

try {
    $installedVersion = & $Dest --version 2>&1
    Write-Ok "Binary check: $installedVersion"
}
catch {
    Abort "Installed binary failed to run: $_"
}

# -- 5. Run gvsn setup ----------------------------------------------------------
# Run setup via full path so it works even before InstallDir is on PATH.
Write-Host ""
Write-Host "  Configuring shell environment..." -ForegroundColor White
Write-Host ""
& $Dest setup

# -- 6. Summary ----------------------------------------------------------------
Write-Host ""
Write-Host "  gvsn $Version installed and configured!" -ForegroundColor Green
Write-Host ""
Write-Host "  Next steps:" -ForegroundColor White
Write-Host ""
Write-Host "  1. Restart your terminal, then install and activate Go:" -ForegroundColor White
Write-Host "       gvsn install latest" -ForegroundColor Cyan
Write-Host "       gvsn use latest" -ForegroundColor Cyan
Write-Host ""
Write-Host "  Run 'gvsn doctor' to verify the setup." -ForegroundColor DarkGray
Write-Host ""

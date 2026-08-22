#!/usr/bin/env pwsh
# gvm (Go Version Manager) - Windows uninstaller
#
# Removes every trace of gvm from the system:
#   - The data directory (~\.gvm by default): all installed Go versions, the
#     version/current pointers, the lock file, and the tmp\ staging area.
#   - The gvm binary itself.
#   - The gvm entries from the Windows user PATH (HKCU\Environment).
#   - Every gvm-managed block from your PowerShell profile (and, best-effort,
#     any Bash/Zsh profile that gvm setup was pointed at under Git Bash/WSL
#     interop on this machine).
#   - Leftover temp files from an install/upgrade that was interrupted
#     mid-way.
#
# Usage (one-liner):
#   irm https://raw.githubusercontent.com/jhonsferg/gvm-rs/main/install/uninstall.ps1 | iex
#
# A piped script can still prompt interactively here (unlike a piped POSIX
# shell script) because `iex` evaluates in the current host, not a
# subprocess reading a consumed stdin pipe. To skip the prompt anyway (e.g.
# in CI), set these before piping:
#   $env:GVM_UNINSTALL_FORCE = "1"
#   $env:GVM_UNINSTALL_DRY_RUN = "1"
#
# Customise the locations to clean (only needed if you used these at
# install time):
#   $env:GVM_DIR = "D:\gvm-data"
#   $env:GVM_INSTALL_DIR = "C:\tools\gvm"

$ErrorActionPreference = "Stop"

$GvmDirPath = if ($env:GVM_DIR)         { $env:GVM_DIR }         else { "$env:USERPROFILE\.gvm" }
$InstallDir = if ($env:GVM_INSTALL_DIR) { $env:GVM_INSTALL_DIR } else { "$env:USERPROFILE\.local\bin" }
$Force      = ($env:GVM_UNINSTALL_FORCE -eq "1")
$DryRun     = ($env:GVM_UNINSTALL_DRY_RUN -eq "1")
foreach ($a in $args) {
    if ($a -eq "--force" -or $a -eq "-Force")   { $Force = $true }
    if ($a -eq "--dry-run" -or $a -eq "-DryRun") { $DryRun = $true }
}

# -- Terminal helpers ----------------------------------------------------------
function Write-Ok([string]$msg) { Write-Host "  v  $msg" -ForegroundColor Green }
function Abort([string]$msg) {
    Write-Host "`n  x  $msg" -ForegroundColor Red
    exit 1
}

Write-Host ""
Write-Host "  gvm" -ForegroundColor Red -NoNewline
Write-Host " -- uninstaller" -ForegroundColor White
Write-Host ""

# -- Strip gvm-managed blocks from a profile file -------------------------------
#
# Mirrors the block model used by gvm's own profile.rs: a gvm block starts at
# one of the recognised marker lines and ends at the next blank line (or
# EOF). Only gvm-managed content is touched; everything else - and the file
# itself if gvm never touched it - is left exactly as it was.
#
# Returns $true if the file was changed.
function Remove-GvmBlocks([string]$Path) {
    if (-not (Test-Path $Path)) { return $false }
    $markers = @("# gvm init", "# gvm wrapper", "# gvm path", "# gvm: binary location")
    $original = Get-Content -Path $Path
    $output = New-Object System.Collections.Generic.List[string]
    $inBlock = $false
    foreach ($line in $original) {
        $trimmed = $line.Trim()
        if ($markers -contains $trimmed) {
            $inBlock = $true
            continue
        }
        if ($inBlock -and $trimmed -eq "") {
            $inBlock = $false
            continue
        }
        if ($inBlock) { continue }
        $output.Add($line)
    }
    if (($output -join "`n") -eq ($original -join "`n")) {
        return $false
    }
    Set-Content -Path $Path -Value $output
    return $true
}

# -- Locate the binary -----------------------------------------------------
$GvmCmd = Get-Command gvm -ErrorAction SilentlyContinue
$GvmBin = if ($GvmCmd) { $GvmCmd.Source } else { $null }
if (-not $GvmBin -and (Test-Path (Join-Path $InstallDir "gvm.exe"))) {
    $GvmBin = Join-Path $InstallDir "gvm.exe"
}

# -- Profiles that gvm setup can write to ---------------------------------------
$PsProfile = Join-Path $env:USERPROFILE "Documents\PowerShell\profile.ps1"
# Best-effort: gvm setup --shell bash/zsh works on Windows too under Git
# Bash/MSYS/WSL interop, so sweep those profiles if present.
$BashProfile = Join-Path $env:USERPROFILE ".bashrc"
$ZshProfile  = Join-Path $env:USERPROFILE ".zshrc"
$AllProfiles = @($PsProfile, $BashProfile, $ZshProfile)

$FoundProfiles = @()
foreach ($p in $AllProfiles) {
    if ((Test-Path $p) -and (Select-String -Path $p -Pattern "^# gvm " -Quiet -ErrorAction SilentlyContinue)) {
        $FoundProfiles += $p
    }
}

# -- Registry PATH entries this uninstaller will strip --------------------------
# Path-boundary-aware matching (not a raw string prefix): a folder like
# ".gvmbackup" must never be treated as being inside ".gvm" just because the
# text happens to start the same way.
$GvmDirPrefix = $GvmDirPath.TrimEnd('\') + '\'
$RegPathEntries = @()
try {
    $currentPath = (Get-ItemProperty -Path "HKCU:\Environment" -Name PATH -ErrorAction SilentlyContinue).PATH
    if ($currentPath) {
        $RegPathEntries = $currentPath -split ';' | Where-Object {
            $_ -ne "" -and (
                $_.TrimEnd('\') -ieq $InstallDir.TrimEnd('\') -or
                $_.TrimEnd('\') -ieq $GvmDirPath.TrimEnd('\') -or
                $_.StartsWith($GvmDirPrefix, [StringComparison]::OrdinalIgnoreCase)
            )
        }
    }
}
catch {}

$VersionCount = 0
$VersionsDir = Join-Path $GvmDirPath "versions"
if (Test-Path $VersionsDir) {
    $VersionCount = (Get-ChildItem -Path $VersionsDir -Directory -ErrorAction SilentlyContinue | Measure-Object).Count
}

# -- Print removal plan ----------------------------------------------------------
Write-Host "  This will permanently remove:" -ForegroundColor White
Write-Host ""
if (Test-Path $GvmDirPath) {
    Write-Host "  -> $GvmDirPath ($VersionCount installed version(s), plus cache/tmp)" -ForegroundColor Cyan
} else {
    Write-Host "  -> $GvmDirPath (not found)" -ForegroundColor Cyan
}
if ($GvmBin) {
    Write-Host "  -> $GvmBin" -ForegroundColor Cyan
} else {
    Write-Host "  -> gvm binary (not found in PATH or $InstallDir)" -ForegroundColor Cyan
}
if ($RegPathEntries.Count -gt 0) {
    $entryWord = if ($RegPathEntries.Count -eq 1) { "entry" } else { "entries" }
    Write-Host "  -> $($RegPathEntries.Count) gvm $entryWord from the user PATH (HKCU\Environment)" -ForegroundColor Cyan
} else {
    Write-Host "  -> no gvm entries found in the user PATH registry" -ForegroundColor Cyan
}
if ($FoundProfiles.Count -gt 0) {
    foreach ($p in $FoundProfiles) {
        Write-Host "  -> $p (gvm lines removed)" -ForegroundColor Cyan
    }
} else {
    Write-Host "  -> no gvm entries found in any shell profile" -ForegroundColor Cyan
}
Write-Host "  -> any leftover temp files from an interrupted install/upgrade" -ForegroundColor Cyan
Write-Host ""

if ($DryRun) {
    Write-Host "  Dry run - nothing was removed." -ForegroundColor Yellow
    Write-Host ""
    exit 0
}

# -- Confirm ---------------------------------------------------------------------
if (-not $Force) {
    $reply = Read-Host "  Type 'yes' to confirm"
    if ($reply.Trim().ToLowerInvariant() -notin @("y", "yes")) {
        Abort "Aborted."
    }
    Write-Host ""
}

# -- Remove data directory (junction first, to avoid any ambiguity about
#    -Recurse following the reparse point into its target) ---------------------
if (Test-Path $GvmDirPath) {
    $currentLink = Join-Path $GvmDirPath "current"
    if (Test-Path $currentLink) {
        cmd /c rmdir "$currentLink" 2>$null | Out-Null
    }
    Remove-Item -Recurse -Force $GvmDirPath -ErrorAction SilentlyContinue
    Write-Ok "Removed $GvmDirPath"
}

# -- Clean shell profiles ---------------------------------------------------------
foreach ($p in $AllProfiles) {
    if (Remove-GvmBlocks -Path $p) {
        Write-Ok "Cleaned $p"
    }
}

# -- Remove gvm entries from the user PATH registry ------------------------------
if ($RegPathEntries.Count -gt 0) {
    try {
        $remaining = ($currentPath -split ';') | Where-Object { $RegPathEntries -notcontains $_ }
        Set-ItemProperty -Path "HKCU:\Environment" -Name PATH -Value ($remaining -join ';')
        Write-Ok "Removed gvm entries from the user PATH (registry)"
    }
    catch {
        Write-Host "  !  Could not update the registry PATH: $_" -ForegroundColor Yellow
    }
}

# -- Sweep leftover temp files from interrupted installs/upgrades -----------------
$tmpDir = [System.IO.Path]::GetTempPath()
foreach ($pattern in @("gvm-install-*", "gvm-upgrade-*")) {
    Get-ChildItem -Path $tmpDir -Filter $pattern -ErrorAction SilentlyContinue |
        Remove-Item -Recurse -Force -ErrorAction SilentlyContinue
}
$oldBinary = Join-Path $InstallDir "gvm.exe.old"
if (Test-Path $oldBinary) {
    Remove-Item -Force $oldBinary -ErrorAction SilentlyContinue
}

# -- Remove the binary (last, so earlier steps still had it available) -----------
if ($GvmBin -and (Test-Path $GvmBin)) {
    try {
        Remove-Item -Force $GvmBin -ErrorAction Stop
        Write-Ok "Removed $GvmBin"
    }
    catch {
        # Same rename-then-delete fallback gvm implode uses: a running
        # executable can't be unlinked directly on Windows, but it can be
        # renamed, freeing the original path immediately.
        $staged = "$GvmBin.old"
        try {
            Rename-Item -Path $GvmBin -NewName (Split-Path -Leaf $staged) -Force
            Remove-Item -Force $staged -ErrorAction SilentlyContinue
            Write-Ok "Removed $GvmBin"
        }
        catch {
            Write-Host "  !  Could not remove $GvmBin - it may still be running. Delete it manually." -ForegroundColor Yellow
        }
    }
}

Write-Host ""
Write-Host "  gvm has been completely removed." -ForegroundColor Green
Write-Host "  Restart your terminal for the PATH and profile changes to take effect."
Write-Host ""
Write-Host "  Note: if you saved shell completions manually (gvm completions ...)," -ForegroundColor Yellow
Write-Host "  remove that file yourself - gvm does not track where it was written." -ForegroundColor Yellow
Write-Host ""

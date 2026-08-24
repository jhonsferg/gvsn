#!/usr/bin/env pwsh
# gvsn (Go Version Manager) - Windows uninstaller
#
# Removes every trace of gvsn from the system:
#   - The data directory (~\.gvsn by default): all installed Go versions, the
#     version/current pointers, the lock file, and the tmp\ staging area.
#   - The gvsn binary itself.
#   - The gvsn entries from the Windows user PATH (HKCU\Environment).
#   - Every gvsn-managed block from your PowerShell profile (and, best-effort,
#     any Bash/Zsh profile that gvsn setup was pointed at under Git Bash/WSL
#     interop on this machine).
#   - Leftover temp files from an install/upgrade that was interrupted
#     mid-way.
#
# Usage (one-liner):
#   irm https://raw.githubusercontent.com/jhonsferg/gvsn/main/install/uninstall.ps1 | iex
#
# A piped script can still prompt interactively here (unlike a piped POSIX
# shell script) because `iex` evaluates in the current host, not a
# subprocess reading a consumed stdin pipe. To skip the prompt anyway (e.g.
# in CI), set these before piping:
#   $env:GVSN_UNINSTALL_FORCE = "1"
#   $env:GVSN_UNINSTALL_DRY_RUN = "1"
#
# Customise the locations to clean (only needed if you used these at
# install time):
#   $env:GVSN_DIR = "D:\gvsn-data"
#   $env:GVSN_INSTALL_DIR = "C:\tools\gvsn"

$ErrorActionPreference = "Stop"

$GvsnDirPath = if ($env:GVSN_DIR)         { $env:GVSN_DIR }         else { "$env:USERPROFILE\.gvsn" }
$InstallDir = if ($env:GVSN_INSTALL_DIR) { $env:GVSN_INSTALL_DIR } else { "$env:USERPROFILE\.local\bin" }
$Force      = ($env:GVSN_UNINSTALL_FORCE -eq "1")
$DryRun     = ($env:GVSN_UNINSTALL_DRY_RUN -eq "1")
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
Write-Host "  gvsn" -ForegroundColor Red -NoNewline
Write-Host " -- uninstaller" -ForegroundColor White
Write-Host ""

# -- Strip gvsn-managed blocks from a profile file -------------------------------
#
# Mirrors the block model used by gvsn's own profile.rs: a gvsn block starts at
# one of the recognised marker lines and ends at the next blank line (or
# EOF). Only gvsn-managed content is touched; everything else - and the file
# itself if gvsn never touched it - is left exactly as it was.
#
# Returns $true if the file was changed.
function Remove-GvsnBlocks([string]$Path) {
    if (-not (Test-Path $Path)) { return $false }
    $markers = @("# gvsn init", "# gvsn wrapper", "# gvsn path", "# gvsn: binary location")
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
$GvsnCmd = Get-Command gvsn -ErrorAction SilentlyContinue
$GvsnBin = if ($GvsnCmd) { $GvsnCmd.Source } else { $null }
if (-not $GvsnBin -and (Test-Path (Join-Path $InstallDir "gvsn.exe"))) {
    $GvsnBin = Join-Path $InstallDir "gvsn.exe"
}

# -- Profiles that gvsn setup can write to ---------------------------------------
$PsProfile = Join-Path $env:USERPROFILE "Documents\PowerShell\profile.ps1"
# Best-effort: gvsn setup --shell bash/zsh works on Windows too under Git
# Bash/MSYS/WSL interop, so sweep those profiles if present.
$BashProfile = Join-Path $env:USERPROFILE ".bashrc"
$ZshProfile  = Join-Path $env:USERPROFILE ".zshrc"
$AllProfiles = @($PsProfile, $BashProfile, $ZshProfile)

$FoundProfiles = @()
foreach ($p in $AllProfiles) {
    if ((Test-Path $p) -and (Select-String -Path $p -Pattern "^# gvsn " -Quiet -ErrorAction SilentlyContinue)) {
        $FoundProfiles += $p
    }
}

# -- Registry PATH entries this uninstaller will strip --------------------------
# Path-boundary-aware matching (not a raw string prefix): a folder like
# ".gvsnbackup" must never be treated as being inside ".gvsn" just because the
# text happens to start the same way.
$GvsnDirPrefix = $GvsnDirPath.TrimEnd('\') + '\'
$RegPathEntries = @()
try {
    $currentPath = (Get-ItemProperty -Path "HKCU:\Environment" -Name PATH -ErrorAction SilentlyContinue).PATH
    if ($currentPath) {
        $RegPathEntries = $currentPath -split ';' | Where-Object {
            $_ -ne "" -and (
                $_.TrimEnd('\') -ieq $InstallDir.TrimEnd('\') -or
                $_.TrimEnd('\') -ieq $GvsnDirPath.TrimEnd('\') -or
                $_.StartsWith($GvsnDirPrefix, [StringComparison]::OrdinalIgnoreCase)
            )
        }
    }
}
catch {}

$VersionCount = 0
$VersionsDir = Join-Path $GvsnDirPath "versions"
if (Test-Path $VersionsDir) {
    $VersionCount = (Get-ChildItem -Path $VersionsDir -Directory -ErrorAction SilentlyContinue | Measure-Object).Count
}

# -- Print removal plan ----------------------------------------------------------
Write-Host "  This will permanently remove:" -ForegroundColor White
Write-Host ""
if (Test-Path $GvsnDirPath) {
    Write-Host "  -> $GvsnDirPath ($VersionCount installed version(s), plus cache/tmp)" -ForegroundColor Cyan
} else {
    Write-Host "  -> $GvsnDirPath (not found)" -ForegroundColor Cyan
}
if ($GvsnBin) {
    Write-Host "  -> $GvsnBin" -ForegroundColor Cyan
} else {
    Write-Host "  -> gvsn binary (not found in PATH or $InstallDir)" -ForegroundColor Cyan
}
if ($RegPathEntries.Count -gt 0) {
    $entryWord = if ($RegPathEntries.Count -eq 1) { "entry" } else { "entries" }
    Write-Host "  -> $($RegPathEntries.Count) gvsn $entryWord from the user PATH (HKCU\Environment)" -ForegroundColor Cyan
} else {
    Write-Host "  -> no gvsn entries found in the user PATH registry" -ForegroundColor Cyan
}
if ($FoundProfiles.Count -gt 0) {
    foreach ($p in $FoundProfiles) {
        Write-Host "  -> $p (gvsn lines removed)" -ForegroundColor Cyan
    }
} else {
    Write-Host "  -> no gvsn entries found in any shell profile" -ForegroundColor Cyan
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
if (Test-Path $GvsnDirPath) {
    $currentLink = Join-Path $GvsnDirPath "current"
    if (Test-Path $currentLink) {
        cmd /c rmdir "$currentLink" 2>$null | Out-Null
    }
    Remove-Item -Recurse -Force $GvsnDirPath -ErrorAction SilentlyContinue
    Write-Ok "Removed $GvsnDirPath"
}

# -- Clean shell profiles ---------------------------------------------------------
foreach ($p in $AllProfiles) {
    if (Remove-GvsnBlocks -Path $p) {
        Write-Ok "Cleaned $p"
    }
}

# -- Remove gvsn entries from the user PATH registry ------------------------------
if ($RegPathEntries.Count -gt 0) {
    try {
        $remaining = ($currentPath -split ';') | Where-Object { $RegPathEntries -notcontains $_ }
        Set-ItemProperty -Path "HKCU:\Environment" -Name PATH -Value ($remaining -join ';')
        Write-Ok "Removed gvsn entries from the user PATH (registry)"
    }
    catch {
        Write-Host "  !  Could not update the registry PATH: $_" -ForegroundColor Yellow
    }
}

# -- Sweep leftover temp files from interrupted installs/upgrades -----------------
$tmpDir = [System.IO.Path]::GetTempPath()
foreach ($pattern in @("gvsn-install-*", "gvsn-upgrade-*")) {
    Get-ChildItem -Path $tmpDir -Filter $pattern -ErrorAction SilentlyContinue |
        Remove-Item -Recurse -Force -ErrorAction SilentlyContinue
}
$oldBinary = Join-Path $InstallDir "gvsn.exe.old"
if (Test-Path $oldBinary) {
    Remove-Item -Force $oldBinary -ErrorAction SilentlyContinue
}

# -- Remove the binary (last, so earlier steps still had it available) -----------
if ($GvsnBin -and (Test-Path $GvsnBin)) {
    try {
        Remove-Item -Force $GvsnBin -ErrorAction Stop
        Write-Ok "Removed $GvsnBin"
    }
    catch {
        # Same rename-then-delete fallback gvsn implode uses: a running
        # executable can't be unlinked directly on Windows, but it can be
        # renamed, freeing the original path immediately.
        $staged = "$GvsnBin.old"
        try {
            Rename-Item -Path $GvsnBin -NewName (Split-Path -Leaf $staged) -Force
            Remove-Item -Force $staged -ErrorAction SilentlyContinue
            Write-Ok "Removed $GvsnBin"
        }
        catch {
            Write-Host "  !  Could not remove $GvsnBin - it may still be running. Delete it manually." -ForegroundColor Yellow
        }
    }
}

Write-Host ""
Write-Host "  gvsn has been completely removed." -ForegroundColor Green
Write-Host "  Restart your terminal for the PATH and profile changes to take effect."
Write-Host ""
Write-Host "  Note: if you saved shell completions manually (gvsn completions ...)," -ForegroundColor Yellow
Write-Host "  remove that file yourself - gvsn does not track where it was written." -ForegroundColor Yellow
Write-Host ""

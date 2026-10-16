//! PowerShell shell integration.

use crate::shell::EnvContext;
use std::path::{Path, PathBuf};

pub fn profile_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join("Documents").join("PowerShell").join("profile.ps1"))
}

/// Generates the PowerShell initialisation script.
///
/// Wraps `Set-Location` for directory-change detection. The `_gvsn_hook`
/// checks `$env:GVSN_SHELL_VERSION` first so that session-scoped activations
/// from `gvsn shell <version>` are not overridden when the user changes
/// directories.
pub fn env_script(ctx: &EnvContext<'_>) -> String {
    let gvsn_dir = ctx.gvsn_dir.display().to_string();

    let path_stmt = ctx.active_bin.map_or_else(String::new, |bin| {
        format!(
            "$env:PATH = \"{bin};\" + \
             ((($env:PATH -split \";\") | Where-Object {{ $_ -ne \"\" -and $_ -notlike \"$env:GVSN_DIR\\versions\\*\\bin\" }}) -join \";\")\n",
            bin = bin.display(),
        )
    });

    let goroot_stmt = ctx.active_root.map_or_else(String::new, |root| {
        format!("$env:GOROOT = \"{}\"\n", root.display())
    });

    format!(
        r#"$env:GVSN_DIR = "{gvsn_dir}"
{goroot_stmt}{path_stmt}
if (-not (Get-Command _gvsn_hook -ErrorAction SilentlyContinue)) {{
    function _gvsn_hook {{
        if ($env:GVSN_SHELL_VERSION) {{ return }}
        $p = & gvsn path 2>$null
        if ($p -and $p -ne "") {{
            $goroot = Split-Path $p -Parent
            $env:GOROOT = $goroot
            $env:PATH = "$p;" + (
                (($env:PATH -split ";") |
                Where-Object {{ $_ -ne "" -and $_ -notlike "$env:GVSN_DIR\versions\*\bin" }}) -join ";"
            )
        }}
    }}
    function Set-Location {{
        Microsoft.PowerShell.Management\Set-Location @args
        _gvsn_hook
    }}
    Set-Alias -Name cd -Value Set-Location -Force -Option AllScope
}}"#,
        gvsn_dir = gvsn_dir,
        goroot_stmt = goroot_stmt,
        path_stmt = path_stmt,
    )
}

pub fn wrapper_function() -> &'static str {
    r#"function gvsn {
    $gvsnBin = (Get-Command gvsn -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1).Source
    if (-not $gvsnBin) { Write-Error 'gvsn binary not found in PATH'; return }
    if ($args.Count -gt 0 -and $args[0] -eq 'shell') {
        $script = & $gvsnBin @args
        $script:_gvsnExit = $LASTEXITCODE
        if ($script:_gvsnExit -eq 0 -and $script) {
            $script | Out-String | Invoke-Expression
        }
    } else {
        & $gvsnBin @args
        $script:_gvsnExit = $LASTEXITCODE
        if ($args.Count -gt 0 -and $args[0] -in @('use', 'default', 'local')) {
            & $gvsnBin env --shell powershell 2>$null | Out-String | Invoke-Expression
        }
    }
    $global:LASTEXITCODE = $script:_gvsnExit
}"#
}

pub fn shell_version_script(tag: &str, bin: &Path, root: &Path) -> String {
    format!(
        r#"$env:GVSN_SHELL_VERSION = "{tag}"
$env:GOROOT = "{root}"
$env:PATH = "{bin};" + ((($env:PATH -split ";") | Where-Object {{ $_ -ne "" -and $_ -notlike "$env:GVSN_DIR\versions\*\bin" }}) -join ";")
"#,
        tag = tag,
        root = root.display(),
        bin = bin.display(),
    )
}

pub fn shell_unset_script() -> &'static str {
    "Remove-Item env:GVSN_SHELL_VERSION -ErrorAction SilentlyContinue\n_gvsn_hook 2>$null\n"
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shell::EnvContext;
    use std::path::Path;

    #[test]
    fn hook_checks_gvsn_shell_version() {
        let ctx = EnvContext {
            gvsn_dir: Path::new(r"C:\Users\user\.gvsn"),
            active_bin: None,
            active_root: None,
        };
        let script = env_script(&ctx);
        assert!(script.contains("GVSN_SHELL_VERSION"));
    }

    #[test]
    fn wrapper_handles_shell_subcommand() {
        let w = wrapper_function();
        assert!(w.contains("'shell'") || w.contains("\"shell\"") || w.contains("-eq 'shell'"));
        assert!(w.contains("Invoke-Expression"));
    }

    #[test]
    fn shell_version_script_uses_ps_syntax() {
        let s = shell_version_script(
            "go1.22.4",
            Path::new(r"C:\Users\user\.gvsn\versions\go1.22.4\bin"),
            Path::new(r"C:\Users\user\.gvsn\versions\go1.22.4"),
        );
        assert!(s.contains("$env:GVSN_SHELL_VERSION"));
        assert!(s.contains("go1.22.4"));
        assert!(s.contains("$env:GOROOT"));
        assert!(s.contains("$env:PATH"));
    }

    #[test]
    fn shell_unset_script_removes_env_var() {
        let s = shell_unset_script();
        assert!(s.contains("GVSN_SHELL_VERSION"));
        assert!(s.contains("_gvsn_hook"));
    }
}

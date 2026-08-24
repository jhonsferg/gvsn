//! Zsh shell integration.

use crate::shell::EnvContext;
use std::path::{Path, PathBuf};

pub fn profile_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".zshrc"))
}

/// Generates the Zsh initialisation script.
///
/// Uses `add-zsh-hook chpwd` for directory-change detection. The `_gvsn_hook`
/// checks `GVSN_SHELL_VERSION` first so that `gvsn shell <version>` activations
/// are not overridden when the user changes directories.
pub fn env_script(ctx: &EnvContext<'_>) -> String {
    let gvsn_dir = ctx.gvsn_dir.display().to_string();

    let path_stmt = ctx.active_bin.map_or_else(String::new, |bin| {
        format!("export PATH=\"{}:$PATH\"\n", bin.display())
    });

    let goroot_stmt = ctx.active_root.map_or_else(String::new, |root| {
        format!("export GOROOT=\"{}\"\n", root.display())
    });

    format!(
        r#"export GVSN_DIR="{gvsn_dir}"
{goroot_stmt}{path_stmt}
if ! (( $+functions[_gvsn_hook] )); then
    _gvsn_hook() {{
        [ -n "$GVSN_SHELL_VERSION" ] && return
        [ "$PWD" = "${{_GVSN_PREV_PWD-}}" ] && return
        export _GVSN_PREV_PWD="$PWD"
        local p; p=$(gvsn path 2>/dev/null)
        if [[ -n "$p" ]]; then
            export GOROOT="$(dirname "$p")"
            PATH="$p:$(printenv PATH | tr ':' '\n' | grep -v "$GVSN_DIR/versions" | tr '\n' ':' | sed 's/:$//')"
            export PATH
        fi
    }}
    autoload -Uz add-zsh-hook
    add-zsh-hook chpwd _gvsn_hook
    add-zsh-hook precmd _gvsn_hook
fi"#,
        gvsn_dir = gvsn_dir,
        goroot_stmt = goroot_stmt,
        path_stmt = path_stmt,
    )
}

pub fn wrapper_function() -> &'static str {
    r#"gvsn() {
    local _gvsn_exit
    case "$1" in
        shell)
            local _gvsn_shell_script
            _gvsn_shell_script="$(command gvsn "$@")"
            _gvsn_exit=$?
            if [ "$_gvsn_exit" -eq 0 ]; then
                eval "$_gvsn_shell_script"
            fi
            ;;
        use|default|local)
            command gvsn "$@"
            _gvsn_exit=$?
            eval "$(command gvsn env --shell zsh 2>/dev/null)"
            ;;
        *)
            command gvsn "$@"
            _gvsn_exit=$?
            ;;
    esac
    return $_gvsn_exit
}"#
}

pub fn shell_version_script(tag: &str, bin: &Path, root: &Path) -> String {
    format!(
        r#"export GVSN_SHELL_VERSION="{tag}"
export GOROOT="{root}"
export PATH="{bin}:$(printenv PATH | tr ':' '\n' | grep -v "$GVSN_DIR/versions" | tr '\n' ':' | sed 's/:$//')"
"#,
        tag = tag,
        root = root.display(),
        bin = bin.display(),
    )
}

pub fn shell_unset_script() -> &'static str {
    "unset GVSN_SHELL_VERSION\n_gvsn_hook 2>/dev/null || true\n"
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shell::EnvContext;
    use std::path::Path;

    #[test]
    fn hook_checks_gvsn_shell_version() {
        let ctx = EnvContext {
            gvsn_dir: Path::new("/home/user/.gvsn"),
            active_bin: None,
            active_root: None,
        };
        let script = env_script(&ctx);
        assert!(script.contains("GVSN_SHELL_VERSION"));
    }

    #[test]
    fn zsh_hook_registered_to_precmd_for_startup() {
        let ctx = EnvContext {
            gvsn_dir: Path::new("/home/user/.gvsn"),
            active_bin: None,
            active_root: None,
        };
        let script = env_script(&ctx);
        assert!(
            script.contains("precmd"),
            "env_script must register hook via precmd for startup detection"
        );
    }

    #[test]
    fn wrapper_handles_shell_subcommand() {
        assert!(wrapper_function().contains("shell)"));
    }

    #[test]
    fn shell_version_script_sets_version_var() {
        let s = shell_version_script(
            "go1.21.0",
            Path::new("/home/.gvsn/versions/go1.21.0/bin"),
            Path::new("/home/.gvsn/versions/go1.21.0"),
        );
        assert!(s.contains("GVSN_SHELL_VERSION=\"go1.21.0\""));
    }

    #[test]
    fn shell_unset_script_clears_and_hooks() {
        let s = shell_unset_script();
        assert!(s.contains("unset GVSN_SHELL_VERSION"));
        assert!(s.contains("_gvsn_hook"));
    }
}

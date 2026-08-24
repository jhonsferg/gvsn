//! Fish shell integration.

use crate::shell::EnvContext;
use std::path::{Path, PathBuf};

pub fn profile_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".config").join("fish").join("config.fish"))
}

/// Generates the Fish initialisation script.
///
/// Uses Fish's native `--on-variable PWD` event handler. The `_gvsn_hook`
/// checks `GVSN_SHELL_VERSION` first so that session-scoped activations from
/// `gvsn shell <version>` persist across directory changes.
pub fn env_script(ctx: &EnvContext<'_>) -> String {
    let gvsn_dir = ctx.gvsn_dir.display().to_string();

    let path_stmt = ctx.active_bin.map_or_else(String::new, |bin| {
        format!("fish_add_path -pm \"{}\"\n", bin.display())
    });

    let goroot_stmt = ctx.active_root.map_or_else(String::new, |root| {
        format!("set -gx GOROOT \"{}\"\n", root.display())
    });

    format!(
        r#"set -gx GVSN_DIR "{gvsn_dir}"
{goroot_stmt}{path_stmt}
if not functions -q _gvsn_hook
    function _gvsn_hook --on-variable PWD --on-event fish_prompt
        if set -q GVSN_SHELL_VERSION
            return
        end
        if set -q _GVSN_PREV_PWD; and test "$_GVSN_PREV_PWD" = "$PWD"
            return
        end
        set -g _GVSN_PREV_PWD "$PWD"
        set -l p (gvsn path 2>/dev/null)
        if test -n "$p"
            set -gx GOROOT (dirname $p)
            set -gx PATH $p (string match -rv "$GVSN_DIR/versions" $PATH)
        end
    end
end"#,
        gvsn_dir = gvsn_dir,
        goroot_stmt = goroot_stmt,
        path_stmt = path_stmt,
    )
}

pub fn wrapper_function() -> &'static str {
    r#"function gvsn
    if contains -- $argv[1] shell
        set -l _gvsn_shell_script (command gvsn $argv)
        set -l _gvsn_exit $status
        if test $_gvsn_exit -eq 0
            string join \n $_gvsn_shell_script | source
        end
        return $_gvsn_exit
    end
    command gvsn $argv
    set -l _gvsn_exit $status
    if contains -- $argv[1] use default local
        command gvsn env --shell fish 2>/dev/null | source
    end
    return $_gvsn_exit
end"#
}

pub fn shell_version_script(tag: &str, bin: &Path, root: &Path) -> String {
    format!(
        r#"set -gx GVSN_SHELL_VERSION "{tag}"
set -gx GOROOT "{root}"
set -gx PATH "{bin}" (string match -rv "$GVSN_DIR/versions" $PATH)
"#,
        tag = tag,
        root = root.display(),
        bin = bin.display(),
    )
}

pub fn shell_unset_script() -> &'static str {
    "set -e GVSN_SHELL_VERSION\n_gvsn_hook 2>/dev/null\n"
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shell::EnvContext;
    use std::path::Path;

    #[test]
    fn fish_hook_registered_to_fish_prompt_for_startup() {
        let ctx = EnvContext {
            gvsn_dir: Path::new("/home/user/.gvsn"),
            active_bin: None,
            active_root: None,
        };
        let script = env_script(&ctx);
        assert!(
            script.contains("fish_prompt"),
            "env_script must register hook via fish_prompt for startup detection"
        );
    }

    #[test]
    fn hook_checks_gvsn_shell_version() {
        let ctx = EnvContext {
            gvsn_dir: Path::new("/home/user/.gvsn"),
            active_bin: None,
            active_root: None,
        };
        let script = env_script(&ctx);
        assert!(script.contains("GVSN_SHELL_VERSION"));
        assert!(script.contains("set -q GVSN_SHELL_VERSION"));
    }

    #[test]
    fn wrapper_handles_shell_subcommand() {
        let w = wrapper_function();
        assert!(w.contains("shell"));
        assert!(w.contains("source"));
        // Must use string join to restore newlines before sourcing (fish list quirk)
        assert!(
            w.contains("string join"),
            "must use 'string join \\n' not echo to preserve newlines"
        );
    }

    #[test]
    fn shell_version_script_uses_fish_syntax() {
        let s = shell_version_script(
            "go1.22.0",
            Path::new("/home/.gvsn/versions/go1.22.0/bin"),
            Path::new("/home/.gvsn/versions/go1.22.0"),
        );
        assert!(s.contains("set -gx GVSN_SHELL_VERSION"));
        assert!(s.contains("go1.22.0"));
    }

    #[test]
    fn shell_unset_script_uses_fish_syntax() {
        let s = shell_unset_script();
        assert!(s.contains("set -e GVSN_SHELL_VERSION"));
        assert!(s.contains("_gvsn_hook"));
    }
}

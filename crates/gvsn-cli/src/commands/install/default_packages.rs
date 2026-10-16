//! Default packages installation for `gvsn install`.

use colored::Colorize;

use gvsn_core::{config::Config, version::GoVersion};

/// Reads `~/.gvsn/default-packages` and runs `go install <pkg>` for each entry.
///
/// Errors from individual package installs are printed as warnings rather than
/// propagated so they never block the overall `gvsn install` flow.
pub fn install_default_packages(config: &Config, version: &GoVersion) {
    let pkg_file = config.default_packages_file();
    if !pkg_file.exists() {
        return;
    }

    let content = match std::fs::read_to_string(&pkg_file) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{} Could not read default-packages: {e}", "!".yellow());
            return;
        }
    };

    let packages: Vec<&str> = content
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect();

    if packages.is_empty() {
        return;
    }

    let go_exe = if cfg!(windows) { "go.exe" } else { "go" };
    let go_bin = config.version_bin_dir(&version.tag()).join(go_exe);

    println!("{} Installing default packages...", "->".cyan());

    let sep = if cfg!(windows) { ";" } else { ":" };
    let new_path = format!(
        "{}{}{}",
        config.version_bin_dir(&version.tag()).display(),
        sep,
        std::env::var("PATH").unwrap_or_default()
    );

    for pkg in &packages {
        use std::io::Write as _;
        print!("    {} {}... ", "→".cyan(), pkg);
        let _ = std::io::stdout().flush();

        let result = std::process::Command::new(&go_bin)
            .arg("install")
            .arg(pkg)
            .env("GOROOT", config.version_dir(&version.tag()))
            .env("PATH", &new_path)
            .output();

        match result {
            Ok(o) if o.status.success() => println!("{}", "✓".green()),
            Ok(o) => {
                println!("{}", "✗".red());
                let stderr = String::from_utf8_lossy(&o.stderr);
                eprintln!("      {}", stderr.trim());
            }
            Err(e) => {
                println!("{}", "✗".red());
                eprintln!("      {e}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn version() -> GoVersion {
        GoVersion::parse("1.22.4").unwrap()
    }

    #[test]
    fn no_op_when_default_packages_file_missing() {
        let dir = tempdir().unwrap();
        let config = Config {
            root: dir.path().to_path_buf(),
        };
        // Should return immediately without touching the filesystem further.
        install_default_packages(&config, &version());
    }

    #[test]
    fn no_op_when_file_has_only_comments_and_blank_lines() {
        let dir = tempdir().unwrap();
        let config = Config {
            root: dir.path().to_path_buf(),
        };
        std::fs::write(
            config.default_packages_file(),
            "# a comment\n\n   \n# another\n",
        )
        .unwrap();

        install_default_packages(&config, &version());
    }

    #[test]
    fn attempts_install_for_each_listed_package_even_without_go_binary() {
        let dir = tempdir().unwrap();
        let config = Config {
            root: dir.path().to_path_buf(),
        };
        std::fs::write(
            config.default_packages_file(),
            "golang.org/x/tools/gopls@latest\n# comment\n\ngithub.com/go-delve/delve/cmd/dlv@latest\n",
        )
        .unwrap();

        // The `go` binary does not exist in this tempdir, so each install
        // attempt fails, but the function must not panic and must process
        // every non-comment, non-blank line.
        install_default_packages(&config, &version());
    }
}

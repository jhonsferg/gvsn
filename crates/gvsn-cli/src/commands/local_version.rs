//! `gvsn local` - pin a Go version for the current project.
//!
//! Writes a `.go-version` file in the current working directory. The shell
//! hook reads this file on every directory change and activates the pinned
//! version automatically.

use anyhow::{Context, Result};
use colored::Colorize;

use gvsn_core::{config::Config, toolchain, user_version::VersionSpec};

/// Writes the specified version as a `.go-version` pin in the current directory.
///
/// If the requested version is not yet installed, a warning is printed but the
/// file is still written so the user can install the version later without
/// editing the file again.
///
/// `latest` is written literally to the file rather than being resolved to a
/// specific version number, giving the project flexibility to track the current
/// stable release.
///
/// # Errors
///
/// Returns an error if `spec_str` is not a valid version spec or if the
/// `.go-version` file cannot be written.
pub fn run(config: &Config, spec_str: &str) -> Result<()> {
    let spec = VersionSpec::parse(spec_str)?;
    let tag = spec_to_tag(&spec);

    // Warn if the version is not installed, but still write the file.
    if !matches!(spec, VersionSpec::Latest) {
        if let Ok(v) = gvsn_core::version::GoVersion::parse(&tag) {
            if !toolchain::is_installed(config, &v) {
                println!(
                    "{} Go {} is not installed yet. Run {} first.",
                    "!".yellow(),
                    tag.bold(),
                    format!("gvsn install {tag}").cyan()
                );
            }
        }
    }

    std::fs::write(".go-version", &tag).context("Failed to write .go-version")?;

    println!(
        "{} Local Go version set to {} (.go-version)",
        "✓".green(),
        tag.bold()
    );
    Ok(())
}

/// Formats a [`VersionSpec`] as the literal string written to `.go-version`.
///
/// `Latest` is written as the literal `"latest"` (not resolved to a concrete
/// version) so the pin tracks the current stable release over time.
fn spec_to_tag(spec: &VersionSpec) -> String {
    match spec {
        VersionSpec::Latest => "latest".to_string(),
        VersionSpec::Partial { major, minor } => format!("go{major}.{minor}"),
        VersionSpec::Exact {
            major,
            minor,
            patch,
        } => format!("go{major}.{minor}.{patch}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_to_tag_latest_is_literal() {
        assert_eq!(spec_to_tag(&VersionSpec::Latest), "latest");
    }

    #[test]
    fn spec_to_tag_partial_omits_patch() {
        let spec = VersionSpec::Partial {
            major: 1,
            minor: 22,
        };
        assert_eq!(spec_to_tag(&spec), "go1.22");
    }

    #[test]
    fn spec_to_tag_exact_includes_patch() {
        let spec = VersionSpec::Exact {
            major: 1,
            minor: 22,
            patch: 4,
        };
        assert_eq!(spec_to_tag(&spec), "go1.22.4");
    }
}

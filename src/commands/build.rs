//! `gvm build` - compile a Go version from source.
//!
//! Unlike `gvm install` which downloads a precompiled binary, this command
//! fetches the official Go source tarball from go.dev, locates or downloads a
//! suitable bootstrap compiler, and runs the platform build script
//! (`src/make.bash` on Unix, `src/make.bat` on Windows) to produce a fully
//! functional Go toolchain installed into `~/.gvm/versions/go<X>.<Y>.<Z>/`.
//!
//! # Steps
//!
//! 1. Resolve the requested version against the go.dev release index.
//! 2. Skip if already installed (unless `--force`).
//! 3. Find the source tarball (`kind == "source"`) in the release.
//! 4. Resolve the bootstrap compiler (explicit `--bootstrap`, previous patch if
//!    installed locally, or a temporarily downloaded previous patch/minor release).
//! 5. Download and verify the source tarball.
//! 6. Extract to a unique staging directory.
//! 7. Run the build script with `GOROOT_BOOTSTRAP` and any user-supplied env vars.
//! 8. Move the compiled tree to `~/.gvm/versions/<tag>/`.
//! 9. Clean up staging and temporary bootstrap directories.

use anyhow::{anyhow, Context, Result};
use colored::Colorize;
use indicatif::{ProgressBar, ProgressStyle};
use std::collections::VecDeque;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use crate::{
    archive::{download, extract},
    commands::bootstrap,
    config::{Config, ConfigMut},
    fs as gvm_fs,
    http::HttpClient,
    lock,
    remote::index,
    tempdir::TempDir,
    toolchain,
    user_version::VersionSpec,
};

/// Compiles `version_str` from source and installs it to the gvm versions store.
pub fn run(
    config: &Config,
    client: &HttpClient,
    version_str: &str,
    force: bool,
    no_cgo: bool,
    bootstrap_spec: Option<&str>,
    env_vars: &[String],
) -> Result<()> {
    config.ensure_dirs()?;

    // Resolve version against the remote index.
    let spec = VersionSpec::parse(version_str)?;
    println!("{} Fetching available Go versions...", "->".cyan());
    let releases = index::fetch_releases(client)?;
    let release = index::resolve(&spec, &releases)?;
    let version = release
        .go_version()
        .ok_or_else(|| anyhow!("Could not parse version tag '{}'", release.version))?;

    // Bail out early if already installed (unless --force).
    if toolchain::is_installed(config, &version) {
        if force {
            println!(
                "{} Removing existing Go {} installation...",
                "->".cyan(),
                version.tag().bold()
            );
            let lock_path = config.root.join(".lock");
            lock::with_lock(&lock_path, || {
                std::fs::remove_dir_all(config.version_dir(&version.tag()))
                    .context("Failed to remove existing installation")
            })?;
        } else {
            println!(
                "{} Go {} is already installed.",
                "✓".green(),
                version.tag().bold()
            );
            println!("  Use {} to rebuild from source.", "--force".cyan());
            return Ok(());
        }
    }

    // Locate the source tarball entry for this release.
    let src_file = release.source_file().ok_or_else(|| {
        anyhow!(
            "No source tarball found for {}. \
             Source tarballs are only available for stable releases.",
            version.tag()
        )
    })?;

    println!();
    println!(
        "  {} Building {} from source.",
        "->".cyan(),
        version.tag().bold()
    );
    println!(
        "  {} This will take 5-15 minutes and requires ~3 GB of disk space.",
        "!".yellow()
    );
    println!();

    // Resolve the bootstrap compiler before downloading source.
    let bootstrap =
        bootstrap::resolve_bootstrap(config, client, &version, bootstrap_spec, &releases)?;
    println!("{} Bootstrap: {}", "->".cyan(), bootstrap.label.bold());

    // Download and extract the source tarball under a lock: the destination
    // path in tmp/ is deterministic per version, so a concurrent `gvm build`
    // or `gvm install` of the same version must not write to it at the same
    // time. The lock is released before compilation starts below, since
    // compilation only touches the unique staging directory.
    let lock_path = config.root.join(".lock");
    let src_archive = config.tmp_dir().join(&src_file.filename);
    let staging_dir = {
        let _lock = lock::FileLock::lock(&lock_path)?;

        println!(
            "{} Downloading {}...",
            "->".cyan(),
            src_file.filename.bold()
        );
        if let Err(e) = download::fetch(
            client,
            &index::download_url(&src_file.filename),
            &src_archive,
        ) {
            let _ = std::fs::remove_file(&src_archive);
            bootstrap::cleanup_bootstrap(&bootstrap);
            return Err(e).context("Failed to download source tarball");
        }

        // Verify SHA-256 checksum.
        if !src_file.sha256.is_empty() {
            println!("{} Verifying checksum...", "->".cyan());
            if let Err(e) = download::verify_sha256(&src_archive, &src_file.sha256) {
                let _ = std::fs::remove_file(&src_archive);
                bootstrap::cleanup_bootstrap(&bootstrap);
                return Err(e);
            }
        }

        // Extract source into a unique staging dir to avoid races with other commands.
        let staging_dir = TempDir::new_in(config.tmp_dir(), format!("src-{}", version.tag()))?;

        if let Err(e) = extract::unpack(&src_archive, staging_dir.path()) {
            let _ = std::fs::remove_file(&src_archive);
            bootstrap::cleanup_bootstrap(&bootstrap);
            return Err(e).context("Failed to extract source tarball");
        }
        let _ = std::fs::remove_file(&src_archive);
        staging_dir
    };

    // The Go source tarball always extracts to a `go/` subdirectory.
    let source_root = staging_dir.path().join("go");
    if !source_root.exists() {
        bootstrap::cleanup_bootstrap(&bootstrap);
        anyhow::bail!(
            "Unexpected archive layout: expected 'go/' inside {}",
            staging_dir.path().display()
        );
    }

    // Prevent auto-cleanup of staging dir
    staging_dir.keep();

    println!("{} Compiling Go {}...", "->".cyan(), version.tag().bold());
    if !client.is_verbose() {
        println!(
            "  {} This will take 5-15 minutes. Run with {} to see build output.",
            "i".yellow(),
            "-v".cyan()
        );
    }
    println!();

    let compiled = compile(
        client,
        &source_root,
        &bootstrap.path,
        no_cgo,
        env_vars,
        &config.tmp_dir(),
    );
    bootstrap::cleanup_bootstrap(&bootstrap);

    // TempDir will auto-cleanup on drop if compilation failed
    compiled?;

    // Move the compiled tree to the versions store.
    let dest = config.version_dir(&version.tag());
    let lock_path = config.root.join(".lock");
    if let Err(e) = lock::with_lock(&lock_path, || gvm_fs::move_dir(&source_root, &dest)) {
        // TempDir will auto-cleanup on drop
        return Err(e).context("Failed to move compiled Go to versions directory");
    }

    // TempDir will auto-cleanup on drop

    println!();
    println!(
        "{} Go {} built and installed successfully.",
        "✓".green(),
        version.tag().bold()
    );
    println!(
        "  Run {} to activate.",
        format!("gvm use {}", version).cyan()
    );

    Ok(())
}

/// Runs the Go build script inside `source_root` with the supplied environment.
///
/// A live spinner tracks elapsed time and updates its label as build phases
/// are detected from the output. In verbose mode every line from stdout/stderr
/// is printed with a `│ ` prefix so the user can watch exactly what the build
/// system is doing. In quiet mode the output is buffered and only shown if the
/// build fails, making it easy to diagnose errors without cluttering the
/// normal flow.
///
/// `gvm_tmp` is the gvm scratch directory (`~/.gvm/tmp/`). A process-unique
/// subdirectory is created there and passed as `TEMP`/`TMP`/`TMPDIR` so that
/// Go's intermediate artifacts never leave `~/.gvm/`.
fn compile(
    client: &HttpClient,
    source_root: &Path,
    bootstrap_path: &Path,
    no_cgo: bool,
    env_vars: &[String],
    gvm_tmp: &Path,
) -> Result<()> {
    let src_dir = source_root.join("src");

    let build_tmp = gvm_tmp.join(format!("go-build-{}", std::process::id()));
    std::fs::create_dir_all(&build_tmp)
        .context("Failed to create build scratch directory inside .gvm/tmp")?;

    #[cfg(windows)]
    let (script_name, mut cmd) = {
        let script = src_dir.join("make.bat");
        if !script.exists() {
            anyhow::bail!(
                "Build script not found at {}. The source archive may be corrupt.",
                script.display()
            );
        }
        let mut c = std::process::Command::new("cmd.exe");
        c.args(["/c", script.to_str().unwrap_or("make.bat")]);
        ("make.bat", c)
    };

    #[cfg(not(windows))]
    let (script_name, mut cmd) = {
        let script = src_dir.join("make.bash");
        if !script.exists() {
            anyhow::bail!(
                "Build script not found at {}. The source archive may be corrupt.",
                script.display()
            );
        }
        let mut c = std::process::Command::new("bash");
        c.arg(&script);
        ("make.bash", c)
    };

    cmd.current_dir(&src_dir);
    cmd.env("GOROOT_BOOTSTRAP", bootstrap_path);

    #[cfg(windows)]
    {
        cmd.env("TEMP", &build_tmp);
        cmd.env("TMP", &build_tmp);
    }
    #[cfg(not(windows))]
    {
        cmd.env("TMPDIR", &build_tmp);
    }

    if no_cgo {
        cmd.env("CGO_ENABLED", "0");
    }
    for (key, val) in parse_env_vars(env_vars) {
        cmd.env(key, val);
    }

    // Always pipe both streams so we can show/buffer them.
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    let mut child = cmd
        .spawn()
        .with_context(|| format!("Failed to start {script_name}"))?;

    let stdout_pipe = child.stdout.take().expect("stdout was piped");
    let stderr_pipe = child.stderr.take().expect("stderr was piped");

    // Merge stdout and stderr into a single channel so we process them in
    // arrival order without risk of deadlocking on a full pipe buffer.
    let (tx, rx) = mpsc::channel::<String>();
    let tx2 = tx.clone();

    let stdout_thread = thread::spawn(move || {
        BufReader::new(stdout_pipe)
            .lines()
            .map_while(Result::ok)
            .for_each(|l| {
                tx.send(l).ok();
            });
    });
    let stderr_thread = thread::spawn(move || {
        BufReader::new(stderr_pipe)
            .lines()
            .map_while(Result::ok)
            .for_each(|l| {
                tx2.send(l).ok();
            });
    });

    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::default_spinner()
            .template("  {spinner:.cyan}  {msg}  {elapsed_precise}")
            .unwrap(),
    );
    pb.set_message("Starting build...");
    pb.enable_steady_tick(Duration::from_millis(120));

    let verbose = client.is_verbose();
    let mut tail: VecDeque<String> = VecDeque::with_capacity(100);

    for line in rx {
        // Update spinner label when a recognised phase marker appears.
        if let Some(msg) = phase_message(&line) {
            pb.set_message(msg);
        }

        if verbose {
            pb.println(format!("  {} {}", "│".dimmed(), line));
        }

        if tail.len() >= 100 {
            tail.pop_front();
        }
        tail.push_back(line);
    }

    stdout_thread.join().ok();
    stderr_thread.join().ok();

    let status = child.wait().context("Build process was interrupted")?;
    pb.finish_and_clear();
    let _ = std::fs::remove_dir_all(&build_tmp);

    if !status.success() {
        // In quiet mode print the captured tail so the user can diagnose
        // without re-running with -v.
        if !verbose && !tail.is_empty() {
            eprintln!();
            eprintln!(
                "  {} Build output (last {} lines):",
                "!".yellow(),
                tail.len()
            );
            for line in &tail {
                eprintln!("  {} {}", "│".dimmed(), line);
            }
        }

        #[cfg(windows)]
        {
            let gvm_root = gvm_tmp.parent().unwrap_or(gvm_tmp);
            println!();
            println!(
                "  {} Build failed. If the output above contains 'Access denied' or",
                "!".yellow()
            );
            println!("    'Acceso denegado', your antivirus is blocking intermediate");
            println!("    executables compiled during the build.");
            println!();
            println!(
                "  {} All gvm build artifacts are written exclusively inside:",
                "i".cyan()
            );
            println!("      {}", gvm_root.display());
            println!();
            println!(
                "  {} Add that directory as an exclusion in your antivirus:",
                "->".cyan()
            );
            println!("    Windows Security -> Virus & threat protection ->");
            println!("    Manage settings -> Exclusions -> Add an exclusion -> Folder");
            println!("    Then retry: gvm build <version>");
        }

        anyhow::bail!(
            "Build failed with exit code {}.",
            status.code().unwrap_or(-1)
        );
    }

    Ok(())
}

/// Maps a line of `make.bash`/`make.bat` output to a human-readable spinner
/// label when it marks the start of a recognised build phase.
///
/// Returns `None` for lines that don't match any known phase marker, in
/// which case the spinner keeps its previous label.
fn phase_message(line: &str) -> Option<&'static str> {
    if line.contains("Building C bootstrap") {
        Some("Building C bootstrap tool...")
    } else if line.contains("Building compilers") || line.contains("Building Go bootstrap") {
        Some("Building Go compiler...")
    } else if line.contains("Building packages") || line.contains("Building commands") {
        Some("Building standard library...")
    } else if line.contains("Installed Go for") || line.contains("Installed commands") {
        Some("Finalizing...")
    } else {
        None
    }
}

/// Parses `--env KEY=VALUE` command-line arguments into `(key, value)` pairs.
///
/// Entries without an `=` are silently dropped rather than erroring, matching
/// the previous inline behaviour: a malformed `--env` flag should not abort
/// an otherwise-valid build.
fn parse_env_vars(env_vars: &[String]) -> Vec<(&str, &str)> {
    env_vars
        .iter()
        .filter_map(|kv| kv.split_once('='))
        .collect()
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::{parse_env_vars, phase_message};

    #[test]
    fn phase_message_detects_c_bootstrap() {
        assert_eq!(
            phase_message("##### Building C bootstrap tool."),
            Some("Building C bootstrap tool...")
        );
    }

    #[test]
    fn phase_message_detects_compiler_phase() {
        assert_eq!(
            phase_message("##### Building compilers and Go bootstrap tool for host, linux/amd64."),
            Some("Building Go compiler...")
        );
        assert_eq!(
            phase_message("##### Building Go bootstrap cmd/go (go_bootstrap) using Go."),
            Some("Building Go compiler...")
        );
    }

    #[test]
    fn phase_message_detects_stdlib_phase() {
        assert_eq!(
            phase_message("##### Building packages and commands for linux/amd64."),
            Some("Building standard library...")
        );
        assert_eq!(
            phase_message("##### Building commands only."),
            Some("Building standard library...")
        );
    }

    #[test]
    fn phase_message_detects_finalizing_phase() {
        assert_eq!(
            phase_message("Installed Go for linux/amd64 in /tmp/go"),
            Some("Finalizing...")
        );
        assert_eq!(
            phase_message("Installed commands in /tmp/go/bin"),
            Some("Finalizing...")
        );
    }

    #[test]
    fn phase_message_returns_none_for_unrecognized_line() {
        assert_eq!(phase_message("some unrelated compiler output"), None);
        assert_eq!(phase_message(""), None);
    }

    #[test]
    fn parse_env_vars_splits_key_value_pairs() {
        let vars = vec!["FOO=bar".to_string(), "BAZ=qux".to_string()];
        assert_eq!(parse_env_vars(&vars), vec![("FOO", "bar"), ("BAZ", "qux")]);
    }

    #[test]
    fn parse_env_vars_keeps_value_with_embedded_equals() {
        let vars = vec!["FOO=a=b=c".to_string()];
        assert_eq!(parse_env_vars(&vars), vec![("FOO", "a=b=c")]);
    }

    #[test]
    fn parse_env_vars_drops_malformed_entries() {
        let vars = vec!["NOVALUE".to_string(), "FOO=bar".to_string()];
        assert_eq!(parse_env_vars(&vars), vec![("FOO", "bar")]);
    }

    #[test]
    fn parse_env_vars_handles_empty_input() {
        let vars: Vec<String> = vec![];
        assert!(parse_env_vars(&vars).is_empty());
    }
}

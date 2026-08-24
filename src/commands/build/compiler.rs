//! Go compiler invocation for building from source.

use anyhow::{anyhow, Context, Result};
use colored::Colorize;
use indicatif::{ProgressBar, ProgressStyle};
use std::collections::VecDeque;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use crate::{http::HttpClient, lock};

/// Runs the Go build script inside `source_root` with the supplied environment.
///
/// A live spinner tracks elapsed time and updates its label as build phases
/// are detected from the output. In verbose mode every line from stdout/stderr
/// is printed with a `│ ` prefix so the user can watch exactly what the build
/// system is doing. In quiet mode the output is buffered and only shown if the
/// build fails, making it easy to diagnose errors without cluttering the
/// normal flow.
///
/// `gvsn_tmp` is the gvsn scratch directory (`~/.gvsn/tmp/`). A process-unique
/// subdirectory is created there and passed as `TEMP`/`TMP`/`TMPDIR` so that
/// Go's intermediate artifacts never leave `~/.gvsn/`.
pub fn compile(
    client: &HttpClient,
    source_root: &Path,
    bootstrap_path: &Path,
    no_cgo: bool,
    env_vars: &[String],
    gvsn_tmp: &Path,
) -> Result<()> {
    let src_dir = source_root.join("src");

    let build_tmp = gvsn_tmp.join(format!("go-build-{}", std::process::id()));
    std::fs::create_dir_all(&build_tmp)
        .context("Failed to create build scratch directory inside .gvsn/tmp")?;

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
        // Properly quote the script path for cmd.exe /c to handle spaces
        let script_arg = format!("\"{}\"", script.display());
        c.args(["/c", &script_arg]);
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
    for kv in env_vars {
        if let Some((key, val)) = kv.split_once('=') {
            cmd.env(key, val);
        }
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
        if line.contains("Building C bootstrap") {
            pb.set_message("Building C bootstrap tool...");
        } else if line.contains("Building compilers") || line.contains("Building Go bootstrap") {
            pb.set_message("Building Go compiler...");
        } else if line.contains("Building packages") || line.contains("Building commands") {
            pb.set_message("Building standard library...");
        } else if line.contains("Installed Go for") || line.contains("Installed commands") {
            pb.set_message("Finalizing...");
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
            let gvsn_root = gvsn_tmp.parent().unwrap_or(gvsn_tmp);
            println!();
            println!(
                "  {} Build failed. If the output above contains 'Access denied' or",
                "!".yellow()
            );
            println!("    'Acceso denegado', your antivirus is blocking intermediate");
            println!("    executables compiled during the build.");
            println!();
            println!(
                "  {} All gvsn build artifacts are written exclusively inside:",
                "i".cyan()
            );
            println!("      {}", gvsn_root.display());
            println!();
            println!(
                "  {} Add that directory as an exclusion in your antivirus:",
                "->".cyan()
            );
            println!("    Windows Security -> Virus & threat protection ->");
            println!("    Manage settings -> Exclusions -> Add an exclusion -> Folder");
            println!("    Then retry: gvsn build <version>");
        }

        anyhow::bail!(
            "Build failed with exit code {}.",
            status.code().unwrap_or(-1)
        );
    }

    Ok(())
}
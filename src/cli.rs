//! Command-line interface definition.
//!
//! This module declares the top-level [`Cli`] struct and the [`Command`] enum
//! that together form the complete CLI surface of `gvm`. Every subcommand,
//! flag, and argument is defined here using [`clap`]'s derive macros.
//! The doc-comment on each variant becomes the help text shown by `gvm --help`.

use clap::{Args, Parser, Subcommand};

/// Download tuning flags shared by commands that fetch files from the network.
///
/// Embed with `#[command(flatten)]` on any subcommand that downloads Go
/// archives, source tarballs, or the gvm binary itself.
#[derive(Args, Clone, Debug)]
pub struct DownloadArgs {
    /// Maximum retry attempts on network failure (default: 3).
    ///
    /// Retries use exponential back-off (1 s, 2 s, 4 s, …). Set to `0` to
    /// fail immediately without retrying.
    #[arg(long, default_value = "3", value_name = "N")]
    pub retries: u8,
}

/// Top-level CLI structure parsed from `argv`.
#[derive(Parser)]
#[command(
    name    = "gvm",
    version = env!("CARGO_PKG_VERSION"),
    about   = "A fast, cross-platform Go version manager",
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,

    /// Print HTTP request/response details (method, URL, status, headers).
    ///
    /// Useful for diagnosing network issues, proxy behaviour, or unexpected
    /// server responses. Output is written to stderr so it does not interfere
    /// with commands that emit machine-readable text to stdout.
    ///
    /// On `gvm build` this flag additionally streams the Go compiler's
    /// output live instead of showing a progress message.
    #[arg(long, short = 'v', global = true)]
    pub verbose: bool,
}

/// All subcommands exposed by `gvm`.
#[derive(Subcommand)]
pub enum Command {
    /// Compile a Go version from source and install it.
    ///
    /// Downloads the official Go source tarball from go.dev, locates a
    /// bootstrap compiler (or downloads one temporarily), then runs the
    /// platform build script (`src/make.bash` on Unix, `src/make.bat` on
    /// Windows) to produce a fully functional toolchain installed into
    /// `~/.gvm/versions/go<X>.<Y>.<Z>/`.
    ///
    /// # Examples
    ///
    /// ```text
    /// gvm build 1.25.0
    /// gvm build 1.25.0 --no-cgo
    /// gvm build 1.25.0 --bootstrap 1.22.6
    /// gvm build 1.25.0 --env GOAMD64=v3 --env CC=clang
    /// ```
    ///
    /// **Note**: building from source takes 5-15 minutes and requires ~3 GB of
    /// disk space. Pass the global `-v`/`--verbose` flag to stream the
    /// compiler's output live instead of a progress message.
    Build {
        /// Version spec to build: an exact version (`1.22.4`), a minor range
        /// (`1.22`), or the keyword `latest`.
        version: String,

        /// Rebuild even if already installed.
        #[arg(long, short = 'f')]
        force: bool,

        /// Disable CGO during compilation (`CGO_ENABLED=0`).
        #[arg(long)]
        no_cgo: bool,

        /// Bootstrap Go version to use as the host compiler.
        ///
        /// Must be installed via `gvm install`. Defaults to the highest
        /// installed version; downloads a temporary bootstrap if none exists.
        #[arg(long, value_name = "VERSION")]
        bootstrap: Option<String>,

        /// Set an environment variable for the build (e.g. `GOAMD64=v3`).
        ///
        /// May be repeated: `--env GOAMD64=v3 --env CC=clang`.
        #[arg(long = "env", value_name = "KEY=VALUE")]
        env_vars: Vec<String>,

        #[command(flatten)]
        download: DownloadArgs,
    },

    /// Install a Go version (e.g. `gvm install 1.22.4` or `gvm install latest`).
    Install {
        /// Version spec to install: an exact version (`1.22.4`), a minor range
        /// (`1.22`), or the keyword `latest`.
        version: String,

        /// Reinstall the version even if it is already present on disk.
        #[arg(long, short = 'f')]
        force: bool,

        #[command(flatten)]
        download: DownloadArgs,
    },

    /// Set the global default Go version.
    ///
    /// The version must already be installed. Use `gvm install <version>` first.
    Use {
        /// Version spec to activate globally.
        version: String,
    },

    /// Set the global default Go version (alias for `use`).
    Default {
        /// Version spec to activate globally.
        version: String,
    },

    /// Pin a Go version for the current project by writing a `.go-version` file.
    ///
    /// The file is placed in the current working directory and can be committed
    /// to version control so all contributors use the same toolchain.
    Local {
        /// Version spec to pin (`1.22`, `1.22.4`, or `latest`).
        version: String,
    },

    /// Remove an installed Go version from disk.
    ///
    /// A confirmation prompt is shown unless `--force` is passed. The command
    /// always refuses to remove the currently active version, regardless of
    /// `--force`.
    Uninstall {
        /// Version spec to remove.
        version: String,

        /// Skip the confirmation prompt and remove the version immediately.
        #[arg(long, short = 'f')]
        force: bool,
    },

    /// List all locally installed Go versions.
    List {
        /// Print the list as JSON instead of formatted text, for scripting.
        #[arg(long)]
        json: bool,
    },

    /// List available Go versions from go.dev.
    #[command(name = "list-remote")]
    ListRemote {
        /// Show every patch release instead of only the latest patch per minor.
        #[arg(long)]
        all: bool,

        /// Print the list as JSON instead of formatted text, for scripting.
        #[arg(long)]
        json: bool,
    },

    /// Print the currently active Go version and its source.
    ///
    /// The source is either `local (.go-version)` when a project pin is active
    /// or `global` when the system-wide default is used.
    Current,

    /// Print the `bin/` directory path for the active (or specified) Go version.
    ///
    /// Output is a plain path suitable for shell capture, e.g.
    /// `export PATH="$(gvm path):$PATH"`.
    Path {
        /// Optional version spec. Defaults to the currently active version.
        version: Option<String>,
    },

    /// Print shell initialisation commands that configure `PATH` and `GOROOT`.
    ///
    /// Pipe the output to your shell's eval mechanism so the active Go version
    /// is applied to the current session:
    ///
    /// - Bash / Zsh: `eval "$(gvm env --shell bash)"`
    /// - Fish: `gvm env --shell fish | source`
    /// - PowerShell: `gvm env --shell powershell | Out-String | Invoke-Expression`
    Env {
        /// Target shell. Auto-detected when omitted.
        /// Accepted values: `powershell`, `bash`, `zsh`, `fish`.
        #[arg(long)]
        shell: Option<String>,
    },

    /// Configure the shell environment for gvm.
    ///
    /// Injects the `gvm env` hook into the shell profile, adds a static PATH
    /// entry to the login profile (Linux/macOS) or the Windows registry so that
    /// `go` is visible to all applications including GUI editors like VSCode.
    ///
    /// Re-running is safe: existing up-to-date blocks are left unchanged and
    /// stale ones are updated automatically.
    ///
    /// Pass `--reset` to strip all previous gvm configuration and re-apply it
    /// cleanly. This is safe: only gvm-managed blocks (marked with `# gvm ...`)
    /// are touched; all other content in profile files is preserved.
    Setup {
        /// Target shell. Auto-detected when omitted.
        /// Accepted values: `powershell`, `bash`, `zsh`, `fish`.
        #[arg(long)]
        shell: Option<String>,

        /// Remove all previous gvm configuration and re-apply it cleanly.
        #[arg(long)]
        reset: bool,
    },

    /// Run a command using a specific Go version without changing the global default.
    ///
    /// The chosen version's `bin/` directory is prepended to `PATH` and `GOROOT`
    /// is set for the duration of the subprocess only.
    ///
    /// # Example
    ///
    /// ```text
    /// gvm exec 1.21 go test ./...
    /// ```
    Exec {
        /// Version spec to use for this invocation.
        version: String,

        /// Command and its arguments to execute.
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },

    /// Diagnose the gvm environment and report configuration issues.
    ///
    /// Exits with status code `1` if any issue is found, making it suitable
    /// for use in CI health checks.
    Doctor {
        /// Target shell for profile check. Auto-detected when omitted.
        #[arg(long)]
        shell: Option<String>,

        /// Automatically repair fixable issues (missing/stale shell hook,
        /// missing `~/.gvm/current` link) instead of only reporting them.
        #[arg(long)]
        fix: bool,
    },

    /// Print a shell completion script to stdout.
    ///
    /// Redirect the output to the appropriate location for your shell.
    Completions {
        /// Target shell: `bash`, `zsh`, `fish`, or `powershell`.
        shell: String,
    },

    /// Check which installed Go versions have newer patch releases available.
    ///
    /// Queries go.dev and compares each locally installed version against the
    /// latest available patch for the same major.minor line. Versions that are
    /// behind are highlighted so you can decide whether to update or remove them.
    Outdated {
        /// Print the report as JSON instead of formatted text, for scripting.
        #[arg(long)]
        json: bool,
    },

    /// Remove installed Go versions that are no longer referenced.
    ///
    /// A version is considered referenced when it matches the global default
    /// (`~/.gvm/version`), appears in a `.go-version` file found by walking up
    /// from the current directory, or appears in a `.go-version` file inside
    /// `--scan-dir`. Everything else is offered for removal.
    Prune {
        /// Skip the confirmation prompt and remove unreferenced versions immediately.
        #[arg(long, short = 'f')]
        force: bool,

        /// Print what would be removed without actually removing anything.
        #[arg(long, short = 'n')]
        dry_run: bool,

        /// Additional directory to scan recursively for `.go-version` files
        /// (up to 5 levels deep). Useful when your projects live outside the
        /// current working directory tree.
        #[arg(long)]
        scan_dir: Option<String>,
    },

    /// Activate a Go version for the current shell session only.
    ///
    /// Unlike `gvm use`, this command does not write any files. The activation
    /// lasts only for the current terminal session (or until `--unset` is run).
    /// The `_gvm_hook` respects this override and skips automatic switching
    /// while `GVM_SHELL_VERSION` is set.
    ///
    /// # Examples
    ///
    /// ```text
    /// gvm shell 1.21       # activate 1.21 for this session
    /// gvm shell --unset    # revert to .go-version / global default
    /// ```
    ///
    /// **Note**: the shell wrapper injected by `gvm setup` must be active for
    /// this command to take effect immediately. Without it you must manually
    /// run `eval "$(gvm shell 1.21)"`.
    Shell {
        /// Version spec to activate for this session only.
        version: Option<String>,

        /// Clear the session-scoped override and revert to the file-based version.
        #[arg(long)]
        unset: bool,

        /// Target shell for the output format. Auto-detected when omitted.
        #[arg(long)]
        shell: Option<String>,
    },

    /// Update gvm itself to the latest release from GitHub.
    ///
    /// Downloads the correct binary for the current platform from
    /// `github.com/jhonsferg/gvm-rs` and replaces the running executable
    /// in-place. The operation is atomic on Unix and best-effort on Windows.
    Upgrade {
        /// Re-install the latest version even if gvm is already up to date.
        #[arg(long, short = 'f')]
        force: bool,

        #[command(flatten)]
        download: DownloadArgs,
    },

    /// Completely remove gvm and all installed Go versions from the system.
    ///
    /// Deletes the gvm data directory (`~/.gvm`), the `gvm` binary, and every
    /// gvm-managed line from the detected shell's profile. A confirmation
    /// prompt is shown unless `--force` is passed.
    Implode {
        /// Skip the confirmation prompt and remove everything immediately.
        #[arg(long, short = 'f')]
        force: bool,
    },
}

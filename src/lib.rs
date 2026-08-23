//! Library crate exposing gvm's internals for the benchmark targets under
//! `benches/`. The `gvm` binary (`src/main.rs`) is a thin wrapper around
//! [`run`]; every module listed here is the exact same code that ships in
//! the binary, just also reachable from outside `src/main.rs`.
//!
//! This crate is not published or intended for use as a dependency by other
//! projects - it exists solely so `cargo bench` has something to link
//! against.

pub mod archive;
pub mod cli;
pub mod commands;
pub mod config;
pub mod dispatch;
pub mod fs;
pub mod gvm_release;
pub mod http;
pub mod lock;
pub mod profile;
pub mod prompt;
pub mod remote;
pub mod shell;
pub mod tempdir;
pub mod toolchain;
pub mod update_check;
pub mod user_version;
pub mod version;

use cli::Cli;
use config::Config;
use http::HttpClient;

/// Loads configuration and runs the selected command.
///
/// # Errors
///
/// Returns an error if configuration cannot be loaded or if the command fails.
pub fn run(cli: Cli) -> anyhow::Result<()> {
    let config = Config::load()?;

    // Create HTTP client with verbosity and retry settings from CLI
    let retries = match &cli.command {
        cli::Command::Build { download, .. } => download.retries,
        cli::Command::Install { download, .. } => download.retries,
        cli::Command::Upgrade { download, .. } => download.retries,
        _ => 3,
    };
    let client = HttpClient::new(cli.verbose, retries)?;

    // Extract command name and args from clap Command enum
    let (cmd_name, args) = dispatch::command_to_name_and_args(&cli.command);

    // Dispatch through registry
    dispatch::dispatch(
        &config,
        &config as &dyn config::ConfigMut,
        &client,
        cmd_name,
        args,
    )
}

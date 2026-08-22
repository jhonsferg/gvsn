//! Entry point for the `gvm` binary.
//!
//! Parses the command-line interface, loads the user configuration, and
//! dispatches to the appropriate command handler. No business logic lives
//! here; every command is implemented in its own submodule under
//! [`commands`].

mod archive;
mod cli;
mod commands;
mod config;
mod dispatch;
mod fs;
mod gvm_release;
mod http;
mod lock;
mod profile;
mod prompt;
mod remote;
mod shell;
mod tempdir;
mod toolchain;
mod update_check;
mod user_version;
mod version;

use clap::Parser;
use colored::Colorize;

use cli::{Cli, Command};
use config::Config;
use http::HttpClient;

fn main() {
    let cli = Cli::parse();

    // Started before the command's own work so a cache-miss network check
    // normally overlaps with it instead of adding visible latency. See
    // `update_check` for the full design (caching, timeouts, suppression).
    let check = Config::load()
        .ok()
        .and_then(|config| update_check::spawn(&config, &cli.command));

    let result = run(cli);

    update_check::print_if_ready(check);

    if let Err(e) = result {
        eprintln!("{} {e:#}", "error:".red().bold());
        std::process::exit(1);
    }
}

/// Loads configuration and runs the selected command.
///
/// # Errors
///
/// Returns an error if configuration cannot be loaded or if the command fails.
fn run(cli: Cli) -> anyhow::Result<()> {
    let config = Config::load()?;

    // Create HTTP client with verbosity and retry settings from CLI
    let retries = match &cli.command {
        Command::Build { download, .. } => download.retries,
        Command::Install { download, .. } => download.retries,
        Command::Upgrade { download, .. } => download.retries,
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

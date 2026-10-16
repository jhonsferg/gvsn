//! Entry point for the `gvsn` binary.
//!
//! Parses the command-line interface, loads the user configuration, and
//! dispatches to the appropriate command handler. No business logic lives
//! here; every command is implemented in its own submodule under
//! [`commands`], on top of the `gvsn-core`, `gvsn-net`, `gvsn-shell` and
//! `gvsn-platform` crates.

#![forbid(unsafe_code)]

mod cli;
mod commands;
mod dispatch;
mod progress;
mod prompt;
mod update_check;

use clap::Parser;
use colored::Colorize;

use gvsn_core::config::Config;
use gvsn_net::http::HttpClient;

use cli::Cli;

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
        &config as &dyn gvsn_core::config::ConfigMut,
        &client,
        cmd_name,
        args,
    )
}

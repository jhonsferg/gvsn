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
mod http;
mod lock;
mod profile;
mod prompt;
mod remote;
mod shell;
mod tempdir;
mod toolchain;
mod user_version;
mod version;

use clap::Parser;
use colored::Colorize;

use cli::{Cli, Command};
use config::Config;
use http::HttpClient;

fn main() {
    if let Err(e) = run() {
        eprintln!("{} {e:#}", "error:".red().bold());
        std::process::exit(1);
    }
}

/// Parses the CLI arguments, loads configuration, and runs the selected command.
///
/// # Errors
///
/// Returns an error if configuration cannot be loaded or if the command fails.
fn run() -> anyhow::Result<()> {
    let cli = Cli::parse();
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

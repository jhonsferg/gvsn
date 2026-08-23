//! Entry point for the `gvm` binary.
//!
//! Parses the command-line interface, loads the user configuration, and
//! dispatches to the appropriate command handler. No business logic lives
//! here; every command is implemented in its own submodule under
//! [`gvm_rs::commands`], reachable through the [`gvm_rs`] library crate.

use clap::Parser;
use colored::Colorize;

use gvm_rs::{cli::Cli, config::Config, run, update_check};

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

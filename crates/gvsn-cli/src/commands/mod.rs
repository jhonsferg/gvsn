//! Command handlers.
//!
//! Each subcommand of `gvsn` is implemented in its own submodule. The
//! corresponding `run` function receives a [`gvsn_core::config::Config`] reference
//! (when file-system access is needed) plus any command-specific arguments,
//! and returns `anyhow::Result<()>`.
//!
//! [`crate::main`] is the only caller; it dispatches to the correct `run`
//! function after parsing the CLI.

pub mod bootstrap;
pub mod build;
pub mod completions;
pub mod current;
pub mod default;
pub mod doctor;
pub mod env;
pub mod exec;
pub mod implode;
pub mod install;
pub mod list;
pub mod list_remote;
pub mod local_version;
pub mod outdated;
pub mod path;
pub mod prune;
pub mod setup;
pub mod shell;
pub mod uninstall;
pub mod upgrade;
pub mod use_version;

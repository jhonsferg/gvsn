//! `gvsn default` - alias for `gvsn use`.
//!
//! Provided for users familiar with tools like `nvm` or `rbenv` that use
//! `default` instead of `use` for the same operation.

use crate::config::Config;
use anyhow::Result;

/// Sets the global default Go version.
///
/// Delegates entirely to [`super::use_version::run`].
///
/// # Errors
///
/// See [`super::use_version::run`] for the error conditions.
pub fn run(config: &Config, spec_str: &str) -> Result<()> {
    super::use_version::run(config, spec_str)
}

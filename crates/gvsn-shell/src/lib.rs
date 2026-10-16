//! gvsn-shell: generation of per-shell integration scripts.
//!
//! Crate with no internal dependencies. It generates text (environment
//! scripts, wrappers and profile blocks) and reads and writes the shell
//! profile files that hold them.
//!
//! Supported shells: bash, zsh, fish and PowerShell.

#![forbid(unsafe_code)]

pub mod profile;
pub mod shell;

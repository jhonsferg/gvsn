//! gvsn-core: gvsn's domain (versions, toolchain store, release index,
//! extraction, locks and checksums).
//!
//! This crate knows nothing about the CLI. Network access goes through
//! `gvsn-net` and OS-specific file operations through `gvsn-platform`.

#![forbid(unsafe_code)]

pub mod archive;
pub mod checksum;
pub mod config;
pub mod gvsn_release;
pub mod lock;
pub mod remote;
pub mod tempdir;
pub mod toolchain;
pub mod user_version;
pub mod version;

//! Remote Go release index.
//!
//! This module provides access to the public Go release API at `go.dev/dl`.
//! It is split into two submodules:
//!
//! - [`release`] - data structures that mirror the JSON returned by the API.
//! - [`index`] - functions to fetch the release list and resolve a
//!   [`crate::user_version::VersionSpec`] to a concrete [`release::Release`].

pub mod index;
pub mod release;

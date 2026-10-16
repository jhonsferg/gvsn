//! gvsn-net: HTTP client (ureq + rustls), resumable downloads and retries.
//!
//! It knows nothing about Go or versions. Verifying integrity (SHA-256) is
//! the responsibility of `gvsn-core`.

#![forbid(unsafe_code)]

pub mod download;
pub mod http;

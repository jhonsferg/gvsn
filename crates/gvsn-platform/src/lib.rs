//! gvsn-platform: where and how gvsn runs.
//!
//! Provides the cross-platform file-system helpers behind the fixed
//! `current` link and, on Windows, the user PATH stored in the registry. It
//! contains no business logic.

pub mod fs;
pub mod user_path;

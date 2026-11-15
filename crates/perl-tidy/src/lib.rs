//! Rust-native Perl formatting with exact source bytes and typed edit plans.
//!
//! The library performs no filesystem access, process execution, tool discovery,
//! editor position conversion or implicit external fallback. Applications own
//! selection, I/O, request policy and presentation.
#![deny(unsafe_code)]
#![warn(rust_2018_idioms)]
#![warn(missing_docs)]
#![deny(clippy::map_err_ignore)]

pub mod native;
pub use native::*;

/// Version of the native formatting library actually linked by the caller.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

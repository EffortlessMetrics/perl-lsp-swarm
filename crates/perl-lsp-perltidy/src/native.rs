//! Native formatter public surface.
//!
//! The implementation remains in `native/implementation.rs`; typed terminal outcomes live
//! beside it so compatibility callers can retain [`FormatResult`] while new
//! callers consume an explicit outcome contract.

#[path = "native/implementation.rs"]
mod implementation;
#[path = "native/outcome.rs"]
mod outcome;

pub use implementation::*;
pub use outcome::*;

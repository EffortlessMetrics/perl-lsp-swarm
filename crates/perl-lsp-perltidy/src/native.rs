//! Native formatter public surface.
//!
//! The implementation remains in `native/implementation.rs`; typed terminal outcomes live
//! beside it so compatibility callers can retain [`FormatResult`] while new
//! callers consume an explicit outcome contract.
//!
//! [`native::terminal_sequence`] and [`native::edit_application`] are the
//! #8048 shift-left proof seam: pure, production-unwired policy and oracle
//! code whose integration owners are #10239/#10242.

#[path = "native/edit_application.rs"]
mod edit_application;
#[path = "native/implementation.rs"]
mod implementation;
#[path = "native/outcome.rs"]
mod outcome;
#[path = "native/terminal_sequence.rs"]
mod terminal_sequence;

pub use edit_application::{EditApplicationError, EditSpec, PositionEncoding, apply_edits_exact};
pub use implementation::*;
pub use outcome::*;
pub use terminal_sequence::{
    FinalNewlinePolicy, PolicyOutcome, TerminalChange, TerminalNewlineEvidence, TerminalRun,
    TerminalSequence, apply_terminal_sequence_policy, source_convention, trailing_run,
};

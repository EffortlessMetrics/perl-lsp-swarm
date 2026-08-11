//! Transition classification for compile observations against retained ratchets.

mod classify;
mod model;

pub use classify::{Classification, classify_transition};
pub use model::{AcceptedBaseline, TransitionRunState};

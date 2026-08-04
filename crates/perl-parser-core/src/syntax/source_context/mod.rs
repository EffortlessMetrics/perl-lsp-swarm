//! Generation-bound lexical source region index.
//!
//! Classifies non-code spans (comments, literals, POD, data sections) for
//! provider evidence. Built from lexer token spans plus a line scanner lifted
//! from completion `lexical_context`.

mod collector;
mod index;
mod kind;
mod region;

pub use index::{RangeClassification, SourceRegionIndex, hash_source_content};
pub use kind::SourceRegionKind;
pub use region::SourceRegion;

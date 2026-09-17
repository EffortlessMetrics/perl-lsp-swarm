//! Quote-like lexical matrix modules owned by `quote_like_lexical_matrix.rs`.

mod completeness;
mod harness;
mod oracle;
mod rows;
mod schema;

pub use completeness::{validate, without_operator};
pub use harness::observe_and_assert;
pub use oracle::{OracleResult, probe_identity};
pub use rows::all_rows;
pub use schema::{Axis, ExpectedKind, NextOrdinary, OperatorFamily, PERL_PROFILE, SCHEMA_VERSION};

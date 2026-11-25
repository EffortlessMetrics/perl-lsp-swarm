//! Pre-compiled tree-sitter query strings for the Perl C grammar.
//!
//! Query sources are embedded at compile time from the crate's vendored
//! [`queries/`](../queries/) directory so the published package stays
//! self-contained. Use [`load_injections_query`], [`load_highlights_query`], and
//! the other `load_*` helpers for typed compilation, or the `*_query_str`
//! accessors for the raw text.

use tree_sitter::QueryError;

/// Raw vendored [`injections.scm`](../queries/injections.scm) source.
pub const INJECTIONS_QUERY: &str = include_str!("../queries/injections.scm");

/// Raw vendored [`highlights.scm`](../queries/highlights.scm) source.
///
/// The full file is retained for snapshot comparison; it is not promised to
/// compile against the frozen grammar until grammar and query snapshots refresh
/// together (see the highlights tripwire test in `lib.rs`).
pub const HIGHLIGHTS_QUERY: &str = include_str!("../queries/highlights.scm");

/// Raw vendored [`folds.scm`](../queries/folds.scm) source.
pub const FOLDS_QUERY: &str = include_str!("../queries/folds.scm");

/// Raw vendored [`matchup.scm`](../queries/matchup.scm) source.
pub const MATCHUP_QUERY: &str = include_str!("../queries/matchup.scm");

/// Returns the raw injections query string (Inline::C/CPP heredocs).
pub fn injections_query_str() -> &'static str {
    INJECTIONS_QUERY
}

/// Returns the raw highlights query string.
pub fn highlights_query_str() -> &'static str {
    HIGHLIGHTS_QUERY
}

/// Returns the raw folds query string.
pub fn folds_query_str() -> &'static str {
    FOLDS_QUERY
}

/// Returns the raw matchup query string.
pub fn matchup_query_str() -> &'static str {
    MATCHUP_QUERY
}

fn load_query(source: &str) -> Result<tree_sitter::Query, QueryError> {
    tree_sitter::Query::new(&crate::language(), source)
}

/// Creates a compiled [`tree_sitter::Query`] for the injections query.
///
/// # Example
///
/// ```rust
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let query = tree_sitter_perl_c::load_injections_query()?;
/// if !query
///     .capture_names()
///     .iter()
///     .any(|name| *name == "injection.content")
/// {
///     return Err("expected injection.content capture".into());
/// }
/// # Ok(())
/// # }
/// ```
///
/// # Errors
///
/// Returns [`QueryError`] if the embedded source stops matching the compiled
/// grammar (a snapshot consistency fault, not caller input).
pub fn load_injections_query() -> Result<tree_sitter::Query, QueryError> {
    load_query(INJECTIONS_QUERY)
}

/// Creates a compiled [`tree_sitter::Query`] for the highlights query.
///
/// # Errors
///
/// Returns [`QueryError`] when the vendored highlights snapshot is incompatible
/// with the frozen C grammar (expected until a joint refresh).
pub fn load_highlights_query() -> Result<tree_sitter::Query, QueryError> {
    load_query(HIGHLIGHTS_QUERY)
}

/// Creates a compiled [`tree_sitter::Query`] for the folds query.
///
/// # Errors
///
/// Returns [`QueryError`] if the embedded source stops matching the compiled
/// grammar.
pub fn load_folds_query() -> Result<tree_sitter::Query, QueryError> {
    load_query(FOLDS_QUERY)
}

/// Creates a compiled [`tree_sitter::Query`] for the matchup query.
///
/// # Errors
///
/// Returns [`QueryError`] if the embedded source stops matching the compiled
/// grammar.
pub fn load_matchup_query() -> Result<tree_sitter::Query, QueryError> {
    load_query(MATCHUP_QUERY)
}

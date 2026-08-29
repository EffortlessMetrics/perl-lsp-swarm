//! Range builder for pragma state snapshots.
//!
//! This module owns AST traversal and delegates pragma directive semantics to
//! `directives`, keeping the public tracker facade focused on querying.

use crate::{PerlVersion, PragmaState, enable_effective_version_semantics};

mod directives;
mod walk;

pub(crate) use walk::build_ranges;

/// Internal pragma state plus the lexical version declaration that selected it.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct TrackedPragmaState {
    /// Effective pragma state exposed through legacy and snapshot queries.
    pub(crate) state: PragmaState,
    /// Perl version declaration (major.minor.patch) active for this tracked
    /// state, normalized from the lexical `use VERSION` source form.
    pub(crate) perl_version: Option<PerlVersion>,
}

impl TrackedPragmaState {
    /// Apply one version declaration while retaining its exact lexical authority.
    pub(crate) fn enable_version_semantics(&mut self, version: PerlVersion) {
        enable_effective_version_semantics(&mut self.state, version);
        self.perl_version = Some(version);
    }
}

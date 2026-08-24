#![warn(missing_docs)]
#![cfg_attr(clippy, allow(missing_docs))]

//! Pragma tracker for Perl code analysis
//!
//! Tracks `use` and `no` pragmas throughout the codebase to determine
//! effective pragma state at any point in the code.

use perl_ast::ast::Node;
use std::ops::Range;

mod args;
mod conditional;
mod features;
mod import_into;
mod map;
mod range_builder;
mod version;

pub use import_into::{ImportIntoCall, ImportIntoSource, ImportIntoTarget, find_import_into_calls};
pub use map::{
    CompileTimePragmaEnvironment, PragmaEntry, PragmaMap, PragmaQueryCursor, PragmaStateQuery,
};
pub use version::{
    PerlVersion, features_enabled_by_version, parse_perl_version, version_implies_strict,
    version_implies_warnings,
};

pub(crate) use args::{
    add_disabled_warning_category, apply_builtin_imports_if_changed, builtin_import_names,
    normalized_pragma_token, pragma_arg_items,
};
pub(crate) use conditional::conditional_pragma_target;
pub(crate) use features::{apply_feature_state, canonical_feature_query};
pub(crate) use map::normalize_state;
pub(crate) use version::enable_effective_version_semantics;

/// Pragma state at a given point in the code
#[derive(Debug, Clone, PartialEq)]
pub struct PragmaState {
    /// Whether strict vars is enabled
    pub strict_vars: bool,
    /// Whether strict subs is enabled
    pub strict_subs: bool,
    /// Whether strict refs is enabled

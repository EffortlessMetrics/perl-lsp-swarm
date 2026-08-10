//! Deterministic and secure Perl module resolution helpers.
//!
//! Combines URI and filesystem module resolution strategies. The `use_lib`
//! submodule extracts additional include paths from `use lib` pragmas and
//! `FindBin` patterns in Perl source text.

pub mod path;
pub mod uri;
pub mod use_lib;

pub use path::resolve_module_path;
pub use uri::{
    IncRoot, IncRootKind, ModuleUriCandidate, ModuleUriCandidateReport, ModuleUriResolution,
    build_effective_inc_roots, collect_module_uri_candidates_with_effective_inc,
    resolve_module_uri, resolve_module_uri_with_effective_inc,
};

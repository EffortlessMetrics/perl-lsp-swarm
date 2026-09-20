//! Package-local fixture runner for `perl-parser-pest`.
//!
//! This module is test support only. It does not change parser behavior and
//! does not promote current parse returns into acceptance verdicts.

mod digest;
mod error;
mod manifest;
mod observe;
mod path;

pub use error::FixtureError;
pub use manifest::{
    Classification, DEFAULT_MANIFEST_RELATIVE, Disposition, ExecutionMode, LoadedManifest,
    MANIFEST_SCHEMA, NewlineVariant, ResolvedFixture, Selection, SourceKind, load_manifest,
    load_manifest_at, package_root,
};
pub use observe::{
    CurrentObservation, ParseObservation, observe_resolved, observe_with_embedded_parser,
    run_embedded, run_embedded_loaded,
};

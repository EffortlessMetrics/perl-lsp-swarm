#![warn(missing_docs)]
#![cfg_attr(clippy, allow(missing_docs))]

//! Unified Perl module facade.
//!
//! This crate absorbs 13 `perl-module-*` microcrates into a single published
//! facade with internal module folders.
//!
//! Implementation modules are private (#8810): every supported public item is
//! reachable only through the crate root via [`api`]. Internal layout such as
//! `perl_module::resolution` or `perl_module::rename` is not a compatibility
//! contract and must not be imported directly.

mod boundary;
mod import;
mod import_match;
mod name;
mod path;
mod provenance;
mod reference;
mod rename;
mod resolution;
mod token;
mod token_core;
mod token_parser;

/// Stable facade exports for external consumers.
pub mod api;
pub use api::*;

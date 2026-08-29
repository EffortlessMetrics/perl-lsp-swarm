#![warn(missing_docs)]
#![cfg_attr(clippy, allow(missing_docs))]

//! Neutral semantic fact vocabulary for Perl analysis layers.
//!
//! This crate defines strongly-typed IDs and serializable fact records that can be shared
//! between parser-derived semantics, semantic analyzer synthesis, and workspace indexing.
//!
//! It intentionally does **not** parse Perl, implement LSP providers, or own workspace
//! storage backends.
#![deny(clippy::map_err_ignore)] // Cohort C0 activation (#12598): census-clean on all targets; new findings move the crate to C1.

use serde::{Deserialize, Serialize};

mod envelope;
pub mod framework;
/// Concrete registry-backed framework adapters built on the SDK.
pub mod framework_adapters;
/// Canonical framework handler relation shared by the route and hook fact
/// families (#8924).
pub mod handler;
/// Canonical framework hook fact family (#8924).
pub mod hook;
/// Dependency-neutral versioned contracts for interprocedural composition
/// (#12672).
pub mod interprocedural;
/// Transport-neutral reachability operation, work-budget, and
/// terminal-outcome contract (#11553).
pub mod reachability_operation;
/// Transport-neutral semantic query outcomes and completeness requirements
/// (#8911).
pub mod semantic_query;
/// Canonical framework route fact family (#8918).
pub mod route;
/// Transport-neutral stable semantic identity and ownership contract (#12121).
pub mod semantic_identity;

pub use envelope::*;
pub use handler::*;
pub use hook::*;
pub use route::*;
pub use semantic_query::*;

macro_rules! id_newtype {
    ($name:ident) => {
        #[derive(
            Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
        )]
        pub struct $name(pub u64);
    };
}

id_newtype!(FileId);
id_newtype!(ScopeId);
id_newtype!(EntityId);
id_newtype!(AnchorId);
id_newtype!(OccurrenceId);
id_newtype!(EdgeId);
id_newtype!(DiagnosticId);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum EntityKind {
    Package,
    Class,
    Role,
    Subroutine,
    Method,
    Variable,
    Constant,
    Field,
    Label,
    Format,
    Module,
    GeneratedMember,
    ExternalSymbol,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum OccurrenceKind {
    Definition,
    Reference,
    Read,
    Write,
    Call,
    MethodCall,
    StaticMethodCall,
    CoderefReference,
    TypeglobReference,
    Import,

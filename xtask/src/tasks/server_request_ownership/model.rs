//! Typed shape of `policy/server-request-ownership.v1.toml` (#13223).
//!
//! The matrix is an ownership and proof map. It deliberately does not restate
//! the wire classification owned by
//! `crates/perl-lsp-rs/src/protocol/method_direction.rs`; the checker joins the
//! two instead.

use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};

/// Sentinel prefix marking a cell whose owner is known but whose evidence does
/// not exist yet. Rendered as `missing:#NNNN`.
pub(super) const MISSING_PREFIX: &str = "missing";

/// Whole matrix file.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Matrix {
    pub(super) meta: Meta,
    #[serde(default)]
    pub(super) request: Vec<RequestRow>,
}

/// Closed vocabulary and the source paths the checker joins against.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Meta {
    pub(super) schema: String,
    pub(super) owner_issue: u64,
    pub(super) direction_registry: String,
    pub(super) feature_catalog: String,
    pub(super) emission_scan_root: String,
    pub(super) allowed_protocol_baselines: Vec<String>,
    pub(super) allowed_emission_states: Vec<String>,
    pub(super) allowed_response_decoders: Vec<String>,
    pub(super) allowed_dispositions: Vec<String>,
}

/// One server-initiated request family.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RequestRow {
    pub(super) id: String,
    pub(super) method: String,
    pub(super) spec: String,
    pub(super) protocol_baseline: String,
    pub(super) emission: String,
    #[serde(default)]
    pub(super) emitters: Vec<String>,
    pub(super) feature_catalog_row: String,
    pub(super) capability_gate: String,
    pub(super) capability_gate_owner: String,
    pub(super) ux_default_response_owner: String,
    pub(super) programmable_actions_owner: String,
    pub(super) response_decoder: String,
    pub(super) terminal_state_owner: String,
    pub(super) timeout_cleanup_policy: String,
    pub(super) exact_process_proof: String,
    pub(super) schema_evidence: String,
    pub(super) disposition: String,
    pub(super) limitations: String,
}

impl RequestRow {
    /// True when the cell records a known-absent owner (`missing` or
    /// `missing:#NNNN`). Such a cell can never satisfy a proof requirement.
    pub(super) fn is_missing(cell: &str) -> bool {
        cell == MISSING_PREFIX || cell.starts_with("missing:")
    }

    /// Split `path#symbol` into its two halves.
    pub(super) fn split_emitter(emitter: &str) -> Option<(&str, &str)> {
        emitter.split_once('#')
    }
}

/// One `[[request]]`-shaped finding. Findings are values, not panics, so the
/// checker can report every violation in one deterministic pass.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct Violation {
    /// Stable machine-readable rule identity.
    pub(super) rule: &'static str,
    /// Row id, method, or `<matrix>` when the finding is file-wide.
    pub(super) subject: String,
    /// Human-readable detail.
    pub(super) detail: String,
}

impl Violation {
    pub(super) fn new(
        rule: &'static str,
        subject: impl Into<String>,
        detail: impl Into<String>,
    ) -> Self {
        Self { rule, subject: subject.into(), detail: detail.into() }
    }
}

/// What the direction registry says about one method.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RegistryKind {
    ServerToClientRequest,
    ServerToClientNotification,
    ClientToServer,
}

/// One `features.toml` row this join consumes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct CatalogRow {
    /// Declared LSP specification version.
    pub(super) spec: String,
    /// Declared protocol area (`workspace`, `window`, `protocol`, …).
    pub(super) area: String,
    /// Whether the catalog advertises the method.
    pub(super) advertised: bool,
    /// Catalog maturity (`proven`, `not_proven`, …).
    pub(super) maturity: String,
    /// Catalog state owner, `missing` when none is recorded.
    pub(super) state_owner: String,
}

/// The joined view of the three discovered surfaces.
#[derive(Debug, Clone, Default)]
pub(super) struct Discovered {
    /// Every method classified by the direction registry.
    pub(super) registry: BTreeMap<String, RegistryKind>,
    /// Methods observed at a production emission call site, mapped to
    /// `path#symbol` references for the functions that emit them.
    pub(super) emitted: BTreeMap<String, Vec<String>>,
    /// Feature-catalog rows declaring `direction = "server_to_client"`.
    pub(super) catalog_rows: BTreeMap<String, CatalogRow>,
    /// `path#symbol` references whose symbol name is declared more than once in
    /// its file, so attribution to it is ambiguous.
    pub(super) ambiguous_symbols: BTreeSet<String>,
}

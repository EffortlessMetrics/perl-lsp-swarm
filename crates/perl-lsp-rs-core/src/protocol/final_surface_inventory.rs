//! Final-surface capability inventory (#9662, train #8032 stage S01).
//!
//! A deterministic, machine-readable ledger of every static capability
//! field, post-hoc initialize-time mutation, dynamic registration, refresh
//! request, suppression branch, compatibility exception and execute-command
//! identity involved in the final LSP surface assembled by
//! `capabilities_json()` ([`crate::protocol::capabilities`]) plus the
//! runtime initialize path (`perl-lsp-rs`
//! `runtime/lifecycle/capabilities.rs::handle_initialize`).
//!
//! Completeness denominator: the census in [`static_surface_census`] walks
//! the *actual serialized* output of [`crate::protocol::capabilities::
//! capabilities_json`] for representative build profiles. Every observed
//! pointer must be owned by exactly one ledger row, and every row must be
//! observable in at least one profile census. Unknown pointers fail the
//! check instead of being silently dropped; stale rows fail symmetrically.
//!
//! This inventory is migration evidence for the #8032 train, not a second
//! capability catalog. It changes no production behavior.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use super::capabilities::{SUPPORTED_COMMANDS, capabilities_json};
use crate::features::flags::BuildFlags;

/// Inventory schema version. Bump on breaking row-schema change.
pub const INVENTORY_SCHEMA_VERSION: u64 = 1;

/// Controlling issue for this inventory.
pub const INVENTORY_ISSUE: &str = "#9662";

/// Parent architecture train that consumes this ledger.
pub const TRAIN_CONTROLLER: &str = "#8032";

/// What kind of final-surface entity a row describes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SurfaceKind {
    /// A static field of `InitializeResult.serverCapabilities`.
    CapabilityField,
    /// An initialize-time post-hoc JSON mutation performed by the runtime.
    Mutation,
    /// A `client/registerCapability` dynamic registration (or its absence).
    Registration,
    /// A capability-gated server-to-client refresh request.
    RefreshRequest,
    /// A config/profile/tool suppression branch feeding the builder inputs.
    Suppression,
    /// A client-specific compatibility exception with bounded reason/expiry.
    Compatibility,
    /// An `executeCommand` command identity.
    Command,
}

/// Current advertisement disposition of a surface.
///
/// Exactly the triple required by #9662: `static` (advertised through the
/// static builder), `dynamic` (delivered through
/// `client/registerCapability`), `unadvertised` (currently never advertised
/// or intentionally absent).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Disposition {
    /// Advertised by the static builder path.
    Static,
    /// Delivered by dynamic registration.
    Dynamic,
    /// Not currently advertised (recorded finding, not silent omission).
    Unadvertised,
}

/// A known divergent competing builder/mutator path for a surface, with the
/// exact difference preserved rather than normalized away (#9662).
#[derive(Debug, Clone, Serialize)]
pub struct CompetingPath {
    /// Source path of the competing writer.
    pub path: &'static str,
    /// Exact difference between this row's primary writer and the competitor.
    pub delta: &'static str,
}

/// A client-specific compatibility exception with exact boundaries (#6735).
#[derive(Debug, Clone, Serialize)]
pub struct CompatBoundary {
    /// Exact subject (client identity predicate, protocol spelling, ...).
    pub subject: &'static str,
    /// Why the exception exists, with its originating issue.
    pub reason: &'static str,
    /// Explicit exit condition; never silently permanent.
    pub expiry: &'static str,
}

/// The `BuildFlags` field a suppression branch zeroes, kept machine-checkable.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct BuildFlagEffect {
    /// `BuildFlags` field name zeroed by the suppression input.
    pub flag: &'static str,
}

/// One ledger row: the normative #9662 schema plus machine-checkable
/// ownership annotations used by the coverage checker.
#[derive(Debug, Clone, Serialize)]
pub struct SurfaceRow {
    /// Stable surface/variant ID (never repurposed; retire instead).
    pub surface_id: &'static str,
    /// Row kind.
    pub kind: SurfaceKind,
    /// Protocol field (dot pointer into `serverCapabilities`), registration
    /// `id@method`, refresh/request method, suppressed input, or command id.
    pub protocol_field: &'static str,
    /// Current primary builder/mutator path (module + function).
    pub builder_mutator_path: &'static str,
    /// Exact client capability inputs consumed (JSON pointers / predicates).
    pub client_capability_inputs: &'static [&'static str],
    /// Build/profile/config/tool inputs feeding this surface.
    pub build_profile_config_tool_inputs: &'static [&'static str],
    /// static | dynamic | unadvertised current disposition.
    pub disposition: Disposition,
    /// Runtime route/provider/command owner (method or handler path).
    pub runtime_route_owner: &'static str,
    /// Evidence/schema owner (catalog ID, spec anchor, owning test).
    pub evidence_owner: &'static str,
    /// Known divergent competing paths with exact differences.
    pub competing_paths: Vec<CompetingPath>,
    /// Target #8032-train issue that will absorb this surface.
    pub target_issue: &'static str,
    /// Compatibility exception boundary, when applicable.
    pub compatibility: Option<CompatBoundary>,
    /// Additional final-surface pointers owned by this row beyond
    /// `protocol_field` (used by multi-pointer mutation rows).
    pub additional_owned_pointers: &'static [&'static str],
    /// Pointer this row *rewrites in place* without owning new pointers;
    /// ownership stays with the cited capability-field row.
    pub rewrites_surface_pointer: Option<&'static str>,
    /// `BuildFlags` effect asserted by the suppression flip check.
    pub build_flag_effect: Option<BuildFlagEffect>,
}

const NO_INPUTS: &[&str] = &[];
const NO_POINTERS: &[&str] = &[];

#[allow(clippy::too_many_arguments)]
fn capability(
    surface_id: &'static str,
    protocol_field: &'static str,
    builder_mutator_path: &'static str,
    client_capability_inputs: &'static [&'static str],
    build_profile_config_tool_inputs: &'static [&'static str],
    runtime_route_owner: &'static str,
    evidence_owner: &'static str,
    target_issue: &'static str,
) -> SurfaceRow {
    SurfaceRow {
        surface_id,
        kind: SurfaceKind::CapabilityField,
        protocol_field,
        builder_mutator_path,
        client_capability_inputs,
        build_profile_config_tool_inputs,
        disposition: Disposition::Static,
        runtime_route_owner,
        evidence_owner,
        competing_paths: Vec::new(),
        target_issue,
        compatibility: None,
        additional_owned_pointers: NO_POINTERS,
        rewrites_surface_pointer: None,
        build_flag_effect: None,
    }
}

#[allow(clippy::too_many_arguments)]
fn mutation(
    surface_id: &'static str,
    protocol_field: &'static str,
    builder_mutator_path: &'static str,
    client_capability_inputs: &'static [&'static str],
    runtime_route_owner: &'static str,
    evidence_owner: &'static str,
    target_issue: &'static str,
) -> SurfaceRow {
    SurfaceRow {
        surface_id,
        kind: SurfaceKind::Mutation,
        protocol_field,
        builder_mutator_path,
        client_capability_inputs,
        build_profile_config_tool_inputs: NO_INPUTS,
        disposition: Disposition::Static,
        runtime_route_owner,
        evidence_owner,
        competing_paths: Vec::new(),
        target_issue,
        compatibility: None,
        additional_owned_pointers: NO_POINTERS,
        rewrites_surface_pointer: None,
        build_flag_effect: None,
    }
}

fn suppression(
    surface_id: &'static str,
    disabled_feature_id: &'static str,
    flag: &'static str,
) -> SurfaceRow {
    SurfaceRow {
        surface_id,
        kind: SurfaceKind::Suppression,
        protocol_field: disabled_feature_id,
        builder_mutator_path: "crates/perl-lsp-rs/src/runtime/lifecycle/capabilities.rs apply_disabled_feature_id",
        client_capability_inputs: NO_INPUTS,
        build_profile_config_tool_inputs: &["initializationOptions.disabledFeatures"],
        disposition: Disposition::Unadvertised,
        runtime_route_owner: "perl-lsp-rs/src/runtime/dispatch dispatch gating (-32601 method_not_advertised)",
        evidence_owner: "features.toml feature catalog; perl-lsp-rs lifecycle tests",
        competing_paths: Vec::new(),
        target_issue: "#9665",
        compatibility: None,
        additional_owned_pointers: NO_POINTERS,
        rewrites_surface_pointer: None,
        build_flag_effect: Some(BuildFlagEffect { flag }),
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn command(command_identity: &'static str) -> SurfaceRow {
    SurfaceRow {
        surface_id: command_identity,
        kind: SurfaceKind::Command,
        protocol_field: command_identity,
        builder_mutator_path: "crates/perl-lsp-rs-core/src/protocol/capabilities.rs SUPPORTED_COMMANDS",
        client_capability_inputs: NO_INPUTS,
        build_profile_config_tool_inputs: &["BuildFlags.execute_command"],
        disposition: Disposition::Static,
        runtime_route_owner: "perl-lsp-rs/src/runtime/dispatch/workspace.rs execute-command dispatcher",
        evidence_owner: "#8285 command descriptors; features.toml#lsp.execute_command",
        competing_paths: Vec::new(),
        target_issue: "#8285",
        compatibility: None,
        additional_owned_pointers: NO_POINTERS,
        rewrites_surface_pointer: None,
        build_flag_effect: None,
    }
}

fn compat(
    surface_id: &'static str,
    protocol_field: &'static str,
    builder_mutator_path: &'static str,
    client_capability_inputs: &'static [&'static str],
    evidence_owner: &'static str,
    subject: &'static str,
    reason: &'static str,
    expiry: &'static str,
) -> SurfaceRow {
    SurfaceRow {
        surface_id,
        kind: SurfaceKind::Compatibility,
        protocol_field,
        builder_mutator_path,
        client_capability_inputs,
        build_profile_config_tool_inputs: NO_INPUTS,
        disposition: Disposition::Unadvertised,
        runtime_route_owner: "n/a (negotiation-time behavior branch)",
        evidence_owner,
        competing_paths: Vec::new(),
        target_issue: "#9665",
        compatibility: Some(CompatBoundary { subject, reason, expiry }),
        additional_owned_pointers: NO_POINTERS,
        rewrites_surface_pointer: None,
        build_flag_effect: None,
    }
}

/// Stable surface IDs referenced across crates by the runtime census proof.
pub mod ids {
    /// Static inline-completion provider advertisement row.
    pub const CAP_INLINE_COMPLETION_PROVIDER: &str = "cap.inlineCompletionProvider";
    /// Static textDocumentSync save row (runtime override recorded as competitor).
    pub const CAP_TEXT_DOCUMENT_SYNC_SAVE: &str = "cap.textDocumentSync.save";
    /// Runtime replacement of textDocumentSync after client parsing.
    pub const MUT_TEXT_DOCUMENT_SYNC_OVERRIDE: &str =
        "mut.handle_initialize.textDocumentSyncOverride";
    /// Runtime positionEncoding utf-16 pin row.
    pub const MUT_POSITION_ENCODING_PIN: &str = "mut.handle_initialize.positionEncodingPin";
    /// Runtime workspace capability wholesale-replacement row.
    pub const MUT_WORKSPACE_REPLACEMENT: &str = "mut.handle_initialize.workspaceReplacement";
    /// Runtime file-operations intersection row.
    pub const MUT_FILE_OPERATIONS_INTERSECTION: &str =
        "mut.handle_initialize.fileOperationsIntersection";
    /// Runtime codeActionProvider.documentation insertion row.
    pub const MUT_CODE_ACTION_DOCUMENTATION_INSERT: &str =
        "mut.handle_initialize.codeActionDocumentationInsert";
    /// Runtime experimental.perlInlineCompletionStream merge row.
    pub const MUT_EXPERIMENTAL_STREAM_MERGE: &str =
        "mut.handle_initialize.experimentalPerlInlineCompletionStreamMerge";
    /// Runtime declarationProvider in-place rewrite row.
    pub const MUT_DECLARATION_PROVIDER_REWRITE: &str =
        "mut.handle_initialize.declarationProviderRewrite";
    /// Runtime inline-completion remove/re-insert tri-state row.
    pub const MUT_INLINE_COMPLETION_TRI_STATE: &str =
        "mut.handle_initialize.inlineCompletionTriState";
    /// Dynamic file-watcher registration row.
    pub const REG_DID_CHANGE_WATCHED_FILES: &str = "reg.perl-didChangeWatchedFiles";
    /// Dynamic inline-completion registration row.
    pub const REG_INLINE_COMPLETION: &str = "reg.perl-inlineCompletion";
}

mod rows;

use rows::rows;

/// The hand-maintained ledger; the coverage checker enforces bijection
/// against the live census.
fn ledger_rows() -> Vec<SurfaceRow> {
    rows()
}

/// Public accessor for the full row set.
pub fn final_surface_rows() -> Vec<SurfaceRow> {
    ledger_rows()
}

/// Representative build profiles used for the census and embedded snapshots.
///
/// `ga-lock` and `production` bracket the shipped defaults; `all` is the
/// maximal in-tree surface including preview flags. Tool availability
/// (`perltidy`) enters through `FeatureProfile::runtime_flags` upstream of
/// `BuildFlags` and is represented by the suppression/systemic rows.
pub fn census_profiles() -> Vec<(&'static str, BuildFlags)> {
    vec![
        ("ga-lock", BuildFlags::ga_lock()),
        ("production", BuildFlags::production()),
        ("all", BuildFlags::all()),
    ]
}

/// Flatten a serialized capabilities `Value` into deterministic dot pointers.
///
/// Scalar leaves become `a.b.c`; empty objects/arrays terminate at their own
/// path (so presence-only providers such as `inlineCompletionProvider: {}`
/// are visible); array elements recurse under the `a.b[]` marker path so
/// row granularity is per-field, not per-element (`a.e[].f` for object
/// elements, deduplicated onto `a.b[]` for scalar elements).
pub fn flatten_surface_pointers(value: &serde_json::Value) -> BTreeSet<String> {
    fn walk(prefix: &str, value: &serde_json::Value, out: &mut BTreeSet<String>) {
        match value {
            serde_json::Value::Object(map) => {
                if map.is_empty() {
                    out.insert(prefix.to_string());
                    return;
                }
                for (key, child) in map {
                    let child_path = format!("{prefix}.{key}");
                    match child {
                        serde_json::Value::Object(inner) if inner.is_empty() => {
                            out.insert(child_path);
                        }
                        serde_json::Value::Array(items) => {
                            let array_path = format!("{child_path}[]");
                            out.insert(array_path.clone());
                            for item in items {
                                walk(&array_path, item, out);
                            }
                        }
                        other => walk(&child_path, other, out),
                    }
                }
            }
            serde_json::Value::Array(items) => {
                if items.is_empty() {
                    out.insert(prefix.to_string());
                    return;
                }
                for item in items {
                    walk(prefix, item, out);
                }
            }
            _ => {
                out.insert(prefix.to_string());
            }
        }
    }

    let mut out = BTreeSet::new();
    if let serde_json::Value::Object(map) = value {
        for (key, child) in map {
            match child {
                serde_json::Value::Object(inner) if inner.is_empty() => {
                    out.insert(key.clone());
                }
                serde_json::Value::Array(items) => {
                    let array_path = format!("{key}[]");
                    out.insert(array_path.clone());
                    for item in items {
                        walk(&array_path, item, &mut out);
                    }
                }
                other => walk(key, other, &mut out),
            }
        }
    }
    out
}

/// Walk the real serialized static builder output per census profile.
pub fn static_surface_census() -> BTreeMap<&'static str, BTreeSet<String>> {
    census_profiles()
        .into_iter()
        .map(|(name, flags)| {
            let pointers = flatten_surface_pointers(&capabilities_json(flags));
            (name, pointers)
        })
        .collect()
}

/// Union of census pointers across all census profiles.
pub fn census_pointer_union() -> BTreeSet<String> {
    static_surface_census().into_values().flatten().collect()
}

/// Every final-surface pointer the ledger covers: the live static census
/// union plus mutation-owned pointers. Runtime initialize responses must be
/// subsets of this set; the `perl-lsp-rs` final-surface census tests enforce
/// that against the exact emitted surface.
pub fn covered_final_surface_pointers() -> BTreeSet<String> {
    let mut covered = owned_surface_pointers(&final_surface_rows());
    covered.extend(census_pointer_union());
    covered
}

/// Pointers owned by ledger rows: `protocol_field` plus
/// `additional_owned_pointers` on capability/mutation rows.
pub fn owned_surface_pointers(rows: &[SurfaceRow]) -> BTreeSet<String> {
    let mut owned = BTreeSet::new();
    for row in rows {
        if matches!(row.kind, SurfaceKind::CapabilityField | SurfaceKind::Mutation) {
            owned.insert(row.protocol_field.to_string());
            for pointer in row.additional_owned_pointers {
                owned.insert((*pointer).to_string());
            }
        }
    }
    owned
}

/// Coverage problems: unmapped census pointers, stale rows, duplicates,
/// malformed rows. Empty means the ledger is bijective with the live
/// static-builder census.
pub fn coverage_errors(rows: &[SurfaceRow]) -> Vec<String> {
    let mut errors = Vec::new();
    let census = census_pointer_union();
    let owned = owned_surface_pointers(rows);

    for pointer in &census {
        if !owned.contains(pointer) {
            errors.push(format!("unmapped census pointer (no inventory row): {pointer}"));
        }
    }
    for row in rows {
        match row.kind {
            SurfaceKind::CapabilityField => {
                if row.disposition != Disposition::Static {
                    errors.push(format!(
                        "malformed capability row {}: disposition must be static",
                        row.surface_id
                    ));
                }
                if !census.contains(row.protocol_field) {
                    errors.push(format!(
                        "stale capability row {}: pointer {} absent from every profile census",
                        row.surface_id, row.protocol_field
                    ));
                }
                if row.build_flag_effect.is_some() {
                    errors.push(format!(
                        "malformed capability row {}: unexpected build-flag effect",
                        row.surface_id
                    ));
                }
            }
            SurfaceKind::Mutation => {
                if row.disposition != Disposition::Static {
                    errors.push(format!(
                        "malformed mutation row {}: disposition must be static",
                        row.surface_id
                    ));
                }
                let owns_primary_pointer = !row.protocol_field.starts_with("(rewrite)");
                if !owns_primary_pointer
                    && row.additional_owned_pointers.is_empty()
                    && row.rewrites_surface_pointer.is_none()
                {
                    errors.push(format!(
                        "malformed mutation row {}: owns no pointers and rewrites none",
                        row.surface_id
                    ));
                }
                if row.build_flag_effect.is_some() {
                    errors.push(format!(
                        "malformed mutation row {}: unexpected build-flag effect",
                        row.surface_id
                    ));
                }
            }
            SurfaceKind::Suppression => {
                let names_disabled_feature =
                    row.protocol_field.starts_with("initializationOptions.disabledFeatures:");
                if !names_disabled_feature
                    && !row.protocol_field.starts_with("profile:")
                    && !row.protocol_field.starts_with("config:")
                    && !row.protocol_field.starts_with("tool:")
                {
                    errors.push(format!(
                        "malformed suppression row {}: protocol_field must name a known suppression input",
                        row.surface_id
                    ));
                }
                if names_disabled_feature && row.build_flag_effect.is_none() {
                    errors.push(format!(
                        "malformed suppression row {}: missing build_flag_effect",
                        row.surface_id
                    ));
                }
            }
            SurfaceKind::Compatibility => {
                let Some(boundary) = &row.compatibility else {
                    errors.push(format!(
                        "malformed compatibility row {}: missing boundary",
                        row.surface_id
                    ));
                    continue;
                };
                if boundary.subject.is_empty()
                    || boundary.reason.is_empty()
                    || boundary.expiry.is_empty()
                {
                    errors.push(format!(
                        "compatibility row {}: subject/reason/expiry must be exact",
                        row.surface_id
                    ));
                }
            }
            SurfaceKind::Command => {
                if row.disposition != Disposition::Static {
                    errors.push(format!(
                        "malformed command row {}: disposition must be static",
                        row.surface_id
                    ));
                }
                let Some(command_id) = row.protocol_field.strip_prefix("cmd.") else {
                    errors.push(format!(
                        "malformed command row {}: protocol_field must be cmd.<id>",
                        row.surface_id
                    ));
                    continue;
                };
                if !SUPPORTED_COMMANDS.contains(&command_id) {
                    errors.push(format!(
                        "stale command row {}: {} not in SUPPORTED_COMMANDS",
                        row.surface_id, command_id
                    ));
                }
            }
            SurfaceKind::Registration => {
                let is_dynamic_registration =
                    row.protocol_field.starts_with("register ") && row.protocol_field.contains('@');
                let is_unadvertised_finding = row.disposition == Disposition::Unadvertised;
                if !is_dynamic_registration && !is_unadvertised_finding {
                    errors.push(format!(
                        "malformed registration row {}: protocol_field must be \
                         'register <id>@<method>' or an unadvertised finding",
                        row.surface_id
                    ));
                }
            }
            SurfaceKind::RefreshRequest => {
                if row.disposition != Disposition::Dynamic {
                    errors.push(format!(
                        "malformed refresh row {}: disposition must be dynamic",
                        row.surface_id
                    ));
                }
                if !row.protocol_field.starts_with("workspace/")
                    || !row.protocol_field.ends_with("/refresh")
                {
                    errors.push(format!(
                        "malformed refresh row {}: protocol_field must be a \
                         workspace/*/refresh request method",
                        row.surface_id
                    ));
                }
            }
        }
    }

    // Command parity in the other direction: every advertised command needs a row.
    let command_rows: BTreeSet<&str> = rows
        .iter()
        .filter(|row| row.kind == SurfaceKind::Command)
        .filter_map(|row| row.protocol_field.strip_prefix("cmd."))
        .collect();
    #[cfg(not(target_arch = "wasm32"))]
    {
        for command in SUPPORTED_COMMANDS {
            if !command_rows.contains(command) {
                errors.push(format!("unmapped execute-command identity (no row): {command}"));
            }
        }
    }
    #[cfg(target_arch = "wasm32")]
    if !command_rows.is_empty() {
        errors.push("wasm32 inventory must not contain execute-command rows".to_string());
    }

    // Duplicate surface IDs anywhere in the ledger.
    let mut seen = BTreeSet::new();
    for row in rows {
        if !seen.insert(row.surface_id) {
            errors.push(format!("duplicate surface_id: {}", row.surface_id));
        }
    }
    // Every owned pointer, including a mutation's primary `protocol_field`,
    // participates in duplicate-claim detection. This deliberately catches
    // a row that lists the same pointer in both ownership locations.
    let mut pointer_claimants: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for row in rows {
        if matches!(row.kind, SurfaceKind::CapabilityField | SurfaceKind::Mutation) {
            pointer_claimants.entry(row.protocol_field).or_default().push(row.surface_id);
        }
        for pointer in row.additional_owned_pointers {
            pointer_claimants.entry(pointer).or_default().push(row.surface_id);
        }
    }
    for (pointer, claimants) in &pointer_claimants {
        if claimants.len() > 1 {
            errors.push(format!(
                "duplicate builder claim for pointer {pointer}: {}",
                claimants.join(", ")
            ));
        }
    }

    errors.sort();
    errors.dedup();
    errors
}

/// [`coverage_errors`] plus existence validation of every row's cited
/// builder/mutator path against the workspace `source_root`.
///
/// The first space-delimited token of `builder_mutator_path` is treated as a
/// repository-relative path and must exist on disk; rows citing no path
/// (`builder_mutator_path` starting with `none`) are skipped. This keeps
/// Registration/RefreshRequest citations — and every other kind — honest
/// after refactors move or delete files. Opt-in because it performs
/// filesystem IO.
pub fn coverage_errors_with_source_check(
    rows: &[SurfaceRow],
    source_root: &std::path::Path,
) -> Vec<String> {
    let mut errors = coverage_errors(rows);
    for row in rows {
        let cited = row.builder_mutator_path.split(' ').next().unwrap_or("");
        if cited.is_empty() || cited == "none" {
            continue;
        }
        if !source_root.join(cited).exists() {
            errors.push(format!(
                "stale citation in row {}: path {cited} does not exist under the workspace root",
                row.surface_id
            ));
        }
    }
    errors.sort();
    errors.dedup();
    errors
}

/// Top-level generated artifact.
#[derive(Debug, Serialize)]
pub struct FinalSurfaceInventory<'a> {
    /// Artifact schema version.
    pub schema_version: u64,
    /// Provenance and scope metadata.
    pub metadata: InventoryMetadata,
    /// Census profile descriptions embedded for reviewers.
    pub census_profiles: Vec<CensusProfile>,
    /// Flattened static-builder pointers per census profile (sorted).
    pub static_surface_census: BTreeMap<String, Vec<String>>,
    /// Exact serialized `serverCapabilities` snapshots per census profile.
    pub profile_snapshots: BTreeMap<&'a str, serde_json::Value>,
    /// The full ledger, sorted by surface ID.
    pub rows: Vec<SurfaceRow>,
    /// Derived review view of every row with competing builders/mutators.
    pub competing_builder_diff: Vec<CompetingBuilderDiff>,
    /// Embedded coverage verdict; empty in every shipped artifact.
    pub coverage: InventoryCoverage,
}

/// Artifact metadata.
#[derive(Debug, Serialize)]
pub struct InventoryMetadata {
    /// Human-readable artifact title.
    pub title: &'static str,
    /// Always `generated`; hand-edited artifacts fail the staleness check.
    pub status: &'static str,
    /// Controlling inventory issue (#9662).
    pub issue: &'static str,
    /// Parent architecture train controller (#8032).
    pub train_controller: &'static str,
    /// Train stage this ledger serves.
    pub train_stage: &'static str,
    /// Canonical regeneration command.
    pub generator: &'static str,
    /// Canonical staleness-check command.
    pub check: &'static str,
    /// Packages whose source the census and runtime proof read.
    pub source_packages: &'static [&'static str],
    /// Scope boundary statement (migration evidence, not a catalog).
    pub scope_note: &'static str,
}

/// A census profile description embedded in the artifact.
#[derive(Debug, Serialize)]
pub struct CensusProfile {
    /// Profile name used as the census/snapshot map key.
    pub name: &'static str,
    /// One-line profile description.
    pub description: &'static str,
}

/// Derived review view: every row with competing builder/mutator paths.
#[derive(Debug, Serialize)]
pub struct CompetingBuilderDiff {
    /// Row surface ID the competitors claim.
    pub surface_id: String,
    /// Protocol pointer or identity the competitors claim.
    pub protocol_field: String,
    /// Primary writer path recorded on the row.
    pub primary_builder: String,
    /// Competing writers with exact deltas preserved.
    pub competitors: Vec<CompetingPath>,
    /// Target train issue that will resolve the divergence.
    pub target_issue: String,
}

/// Embedded coverage verdict; must stay empty in the checked-in artifact.
#[derive(Debug, Serialize)]
pub struct InventoryCoverage {
    /// Coverage problems; rendering fails instead of embedding any.
    pub errors: Vec<String>,
}

/// Render the deterministic inventory artifact.
///
/// Fails when the ledger is not bijective with the live census — callers
/// must fix the ledger rather than ship a partial inventory.
pub fn render_final_surface_inventory_json() -> Result<String, InventoryError> {
    render_with_rows(&final_surface_rows())
}

/// Render using an explicit row set (exercised by the negative controls).
pub fn render_with_rows(rows: &[SurfaceRow]) -> Result<String, InventoryError> {
    let errors = coverage_errors(rows);
    if !errors.is_empty() {
        return Err(InventoryError { problems: errors });
    }

    let census = static_surface_census();
    let census_serialized: BTreeMap<String, Vec<String>> = census
        .iter()
        .map(|(name, pointers)| ((*name).to_string(), pointers.iter().cloned().collect()))
        .collect();

    let profile_snapshots: BTreeMap<&str, serde_json::Value> = census_profiles()
        .into_iter()
        .map(|(name, flags)| (name, capabilities_json(flags)))
        .collect();

    let mut sorted_rows = rows.to_vec();
    sorted_rows.sort_by(|left, right| left.surface_id.cmp(right.surface_id));

    let competing_builder_diff = sorted_rows
        .iter()
        .filter(|row| !row.competing_paths.is_empty())
        .map(|row| CompetingBuilderDiff {
            surface_id: row.surface_id.to_string(),
            protocol_field: row.protocol_field.to_string(),
            primary_builder: row.builder_mutator_path.to_string(),
            competitors: row.competing_paths.clone(),
            target_issue: row.target_issue.to_string(),
        })
        .collect();

    let inventory = FinalSurfaceInventory {
        schema_version: INVENTORY_SCHEMA_VERSION,
        metadata: InventoryMetadata {
            title: "LSP final-surface builder/mutation/registration inventory",
            status: "generated",
            issue: INVENTORY_ISSUE,
            train_controller: TRAIN_CONTROLLER,
            train_stage: "S01 differential inventory (no production behavior change)",
            generator: "cargo test -p perl-lsp-rs-core --lib \
                        final_surface_inventory::tests::regenerate_checked_in_artifact --locked -- --ignored",
            check: "cargo test -p perl-lsp-rs-core --lib final_surface_inventory --locked",
            source_packages: &["crates/perl-lsp-rs-core", "crates/perl-lsp-rs"],
            scope_note: "Migration evidence for the #8032 train, not a second capability catalog. Rows are bijective with the serialized static-builder census plus the runtime mutation/registration surface proven by perl-lsp-rs final-surface census tests.",
        },
        census_profiles: vec![
            CensusProfile {
                name: "ga-lock",
                description: "Conservative GA-lock BuildFlags baseline.",
            },
            CensusProfile {
                name: "production",
                description: "Default supported public-beta baseline.",
            },
            CensusProfile {
                name: "all",
                description: "All in-tree capabilities including preview flags.",
            },
        ],
        static_surface_census: census_serialized,
        profile_snapshots,
        rows: sorted_rows,
        competing_builder_diff,
        coverage: InventoryCoverage { errors: Vec::new() },
    };

    let mut rendered = serde_json::to_string_pretty(&inventory)
        .map_err(|err| InventoryError { problems: vec![format!("serialization failed: {err}")] })?;
    rendered.push('\n');
    Ok(rendered)
}

/// Ledger inconsistency: rendering refused rather than shipping a partial
/// inventory.
#[derive(Debug, Clone)]
pub struct InventoryError {
    /// Human-readable problems; deterministic order.
    pub problems: Vec<String>,
}

impl std::fmt::Display for InventoryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "final-surface inventory is incomplete:")?;
        for problem in &self.problems {
            writeln!(f, "  - {problem}")?;
        }
        Ok(())
    }
}

impl std::error::Error for InventoryError {}

#[cfg(test)]
mod tests;

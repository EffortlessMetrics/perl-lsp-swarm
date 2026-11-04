//! Writer and invalidation evidence for `DerivedWorkspaceFolder` catalog rows (#16827).
//!
//! The structural catalog check only proves that field and marker strings exist
//! in implementation source. This module requires an explicit lifecycle record
//! per derived workspace-folder authority: production writer identity, a
//! registered initialization/invalidation route, absence disposition, owner,
//! and (for live rows) a discriminating `#[test]` identity.
//!
//! Catalog `source_markers` plus evidence `contributing_sources` are the
//! invalidation set. Exact production call-graphs are not inferred: a writer
//! must be called from the named route's function body, not merely mentioned
//! elsewhere in the same file. Planned, dormant, and retired rows stay
//! non-live until their owner lands the missing route.

use super::{CONFIGURATION_AUTHORITY, ConfigScope, FieldAuthority};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

const CONFIG_MOD: &str = "crates/perl-lsp-rs-core/src/config/mod.rs";
const METADATA_DEPENDENCIES: &str = "crates/perl-lsp-rs-core/src/config/metadata_dependencies.rs";
const WORKSPACE_FOLDER: &str = "crates/perl-lsp-rs/src/runtime/workspace_folder.rs";
const METADATA_INVALIDATION_TESTS: &str =
    "crates/perl-lsp-rs/src/runtime/metadata_invalidation_tests.rs";

/// Lifecycle state declared for a derived workspace-folder authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DerivedWorkspaceLifecycleState {
    /// Production writer, route, and behavior proof are present and checked.
    Live,
    /// Intended derived authority whose writer/route/proof has not landed.
    Planned,
    /// Reviewed non-support; must not be presented as live derived configuration.
    IntentionallyDormant,
    /// Removed from the live derived-configuration surface.
    Retired,
}

/// How a derived fact behaves when a contributing source disappears or cannot
/// be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AbsenceDisposition {
    /// Drop only the missing source and recompute from remaining contributions.
    RecomputeFromRemainingSources,
    /// Keep the previous contribution of an unreadable source and record limitation.
    RetainPreviousWithLimitation,
    /// Replace the fact with an exact empty/default value.
    ExactEmpty,
    /// Keep the previous value with no explicit limitation or recompute.
    SilentRetention,
}

/// One derived workspace-folder authority's checked lifecycle evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DerivedWorkspaceEvidence {
    id: &'static str,
    writer: &'static str,
    writer_source: &'static str,
    init_route: Option<&'static str>,
    init_route_source: Option<&'static str>,
    refresh_writer: Option<&'static str>,
    invalidation_route: Option<&'static str>,
    invalidation_route_source: Option<&'static str>,
    invalidation_markers: &'static [&'static str],
    initialization_only_markers: &'static [&'static str],
    /// Extra contributing files that are not catalog `source_markers` but still
    /// feed the derived fact and must be covered by invalidation evidence.
    contributing_sources: &'static [&'static str],
    marker_authority_source: Option<&'static str>,
    delete_disposition: AbsenceDisposition,
    unavailable_disposition: AbsenceDisposition,
    owning_issue: u32,
    proof_test: Option<&'static str>,
    proof_source: Option<&'static str>,
    state: DerivedWorkspaceLifecycleState,
    wake: Option<&'static str>,
    fan_out_group: Option<&'static str>,
}

/// Machine-readable status for one derived workspace-folder row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DerivedWorkspaceRowStatus {
    /// Stable catalog authority ID.
    pub id: &'static str,
    /// Declared lifecycle state.
    pub state: DerivedWorkspaceLifecycleState,
    /// True only for a live row that currently satisfies the writer/route/proof contract.
    pub live: bool,
    /// Owning issue that must land remaining evidence, or that proved the live row.
    pub owning_issue: u32,
    /// Named production writer/refresh operation.
    pub writer: &'static str,
}

/// Row-specific rejection of derived workspace-folder lifecycle evidence.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DerivedWorkspaceViolation {
    /// A `DerivedWorkspaceFolder` catalog row has no lifecycle record.
    MissingEvidence {
        /// Catalog authority ID.
        id: &'static str,
    },
    /// Lifecycle evidence names an ID that is not a derived workspace-folder row.
    ExtraEvidence {
        /// Evidence authority ID.
        id: &'static str,
    },
    /// Two lifecycle records claim the same authority ID.
    DuplicateEvidence {
        /// Catalog authority ID.
        id: &'static str,
    },
    /// A live row names a writer that is not defined in the named production source.
    LiveMissingWriter {
        /// Catalog authority ID.
        id: &'static str,
        /// Named writer.
        writer: &'static str,
        /// Source path that should define the writer.
        source: &'static str,
    },
    /// A live writer exists but the registered production route does not call it.
    WriterUnreachable {
        /// Catalog authority ID.
        id: &'static str,
        /// Named writer.
        writer: &'static str,
        /// Named route that should call the writer.
        route: &'static str,
        /// Source path of that route.
        source: &'static str,
    },
    /// A live catalog marker has neither an invalidation route nor an init-only disposition.
    MissingInvalidation {
        /// Catalog authority ID.
        id: &'static str,
        /// Source marker lacking an invalidation route.
        marker: &'static str,
    },
    /// A live row has no discriminating behavior-test identity.
    LiveMissingProof {
        /// Catalog authority ID.
        id: &'static str,
    },
    /// A live row's proof test ID is absent from the named test source.
    StaleProofId {
        /// Catalog authority ID.
        id: &'static str,
        /// Named proof test.
        proof_test: &'static str,
        /// Source path that should define the test.
        source: &'static str,
    },
    /// Unrelated live authorities share one writer without an explicit fan-out group.
    DuplicateWriter {
        /// Shared writer identity.
        writer: &'static str,
        /// Authority IDs that named the writer.
        ids: Vec<&'static str>,
    },
    /// A live row treats delete or unavailability as exact empty or silent retention.
    LiveExactEmpty {
        /// Catalog authority ID.
        id: &'static str,
    },
    /// A non-live row has no owning issue.
    NonLiveMissingOwner {
        /// Catalog authority ID.
        id: &'static str,
    },
    /// A planned or dormant row has no wake condition.
    NonLiveMissingWake {
        /// Catalog authority ID.
        id: &'static str,
    },
    /// A live row is missing a required named production source file.
    UnknownSource {
        /// Catalog authority ID.
        id: &'static str,
        /// Missing source path.
        source: &'static str,
    },
    /// A live route or writer was registered against a test file.
    NonProductionSource {
        /// Catalog authority ID.
        id: &'static str,
        /// Source path that is not governed production.
        source: &'static str,
    },
}

impl fmt::Display for DerivedWorkspaceViolation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingEvidence { id } => {
                write!(f, "{id}: derived workspace-folder row has no lifecycle evidence")
            }
            Self::ExtraEvidence { id } => {
                write!(f, "{id}: lifecycle evidence is not a DerivedWorkspaceFolder catalog row")
            }
            Self::DuplicateEvidence { id } => {
                write!(f, "{id}: duplicate derived workspace-folder lifecycle evidence")
            }
            Self::LiveMissingWriter { id, writer, source } => {
                write!(f, "{id}: writer `{writer}` is not defined in {source}")
            }
            Self::WriterUnreachable { id, writer, route, source } => {
                write!(f, "{id}: live writer `{writer}` is not called from `{route}` in {source}")
            }
            Self::MissingInvalidation { id, marker } => {
                write!(
                    f,
                    "{id}: marker `{marker}` has no invalidation route or init-only disposition"
                )
            }
            Self::LiveMissingProof { id } => {
                write!(f, "{id}: live derived row has no discriminating behavior test")
            }
            Self::StaleProofId { id, proof_test, source } => {
                write!(f, "{id}: proof test `{proof_test}` is absent from {source}")
            }
            Self::DuplicateWriter { writer, ids } => {
                write!(
                    f,
                    "writer `{writer}` is shared by live rows {ids:?} without a fan-out group"
                )
            }
            Self::LiveExactEmpty { id } => {
                write!(
                    f,
                    "{id}: live delete/unavailable disposition is exact empty or silent retention"
                )
            }
            Self::NonLiveMissingOwner { id } => {
                write!(f, "{id}: planned/dormant/retired row has no owning issue")
            }
            Self::NonLiveMissingWake { id } => {
                write!(f, "{id}: planned/dormant row has no wake condition")
            }
            Self::UnknownSource { id, source } => {
                write!(f, "{id}: named source {source} is not in the checked corpus")
            }
            Self::NonProductionSource { id, source } => {
                write!(f, "{id}: writer/route source {source} is not production code")
            }
        }
    }
}

/// Deterministic configuration-authority checker report (#16827).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ConfigurationAuthorityReport {
    /// One status object per derived workspace-folder catalog row, in catalog order.
    pub rows: Vec<DerivedWorkspaceRowStatus>,
    /// Row-specific violations; empty when the derived-workspace contract holds.
    pub violations: Vec<DerivedWorkspaceViolation>,
}

impl ConfigurationAuthorityReport {
    /// Whether the derived-workspace contract currently holds.
    #[must_use]
    pub fn is_ok(&self) -> bool {
        self.violations.is_empty()
    }
}

/// Lifecycle evidence for every current `DerivedWorkspaceFolder` catalog row.
const DERIVED_WORKSPACE_EVIDENCE: &[DerivedWorkspaceEvidence] = &[
    DerivedWorkspaceEvidence {
        id: "workspace.declared_dependencies",
        writer: "refresh_declared_dependencies",
        writer_source: CONFIG_MOD,
        init_route: Some("refresh_workspace_metadata"),
        init_route_source: Some(WORKSPACE_FOLDER),
        refresh_writer: Some("apply_declared_dependency_reads"),
        invalidation_route: Some("refresh_workspace_metadata_from_reads"),
        invalidation_route_source: Some(WORKSPACE_FOLDER),
        invalidation_markers: &[
            "META.json",
            "cpanfile",
            "Makefile.PL",
            "Build.PL",
            "dist.ini",
            "META.yml",
        ],
        initialization_only_markers: &["declared_dependencies"],
        contributing_sources: &["Makefile.PL", "Build.PL", "dist.ini", "META.yml"],
        marker_authority_source: Some(METADATA_DEPENDENCIES),
        delete_disposition: AbsenceDisposition::RecomputeFromRemainingSources,
        unavailable_disposition: AbsenceDisposition::RetainPreviousWithLimitation,
        owning_issue: 13640,
        proof_test: Some("deleted_cpanfile_downgrades_declared_dependencies"),
        proof_source: Some(METADATA_INVALIDATION_TESTS),
        state: DerivedWorkspaceLifecycleState::Live,
        wake: None,
        fan_out_group: None,
    },
    DerivedWorkspaceEvidence {
        id: "workspace.native_build_hints",
        writer: "refresh_native_build_hints",
        writer_source: CONFIG_MOD,
        init_route: None,
        init_route_source: None,
        refresh_writer: None,
        invalidation_route: None,
        invalidation_route_source: None,
        invalidation_markers: &["Makefile.PL", "Build.PL"],
        initialization_only_markers: &["native_build_hints"],
        contributing_sources: &[],
        marker_authority_source: None,
        delete_disposition: AbsenceDisposition::RecomputeFromRemainingSources,
        unavailable_disposition: AbsenceDisposition::RetainPreviousWithLimitation,
        owning_issue: 16826,
        proof_test: None,
        proof_source: None,
        state: DerivedWorkspaceLifecycleState::Planned,
        wake: Some(
            "Wire refresh_native_build_hints through WorkspaceFolderState metadata initialization and the #13640 captured-read invalidation route, with a discriminating lifecycle test (#16826).",
        ),
        fan_out_group: None,
    },
];

/// Workspace-relative sources the derived-workspace checker must observe.
///
/// The checker does not `include_str!` sibling crates into this library crate:
/// `perl-lsp-rs-core` is packaged without those files. Tests and
/// `cargo xtask check-configuration-authority` supply the corpus.
#[must_use]
pub fn derived_workspace_corpus_paths() -> &'static [&'static str] {
    &[CONFIG_MOD, METADATA_DEPENDENCIES, WORKSPACE_FOLDER, METADATA_INVALIDATION_TESTS]
}

/// Validate derived workspace-folder writer and invalidation evidence.
///
/// `corpus` maps the paths from [`derived_workspace_corpus_paths`] to their
/// current file text. This is a catalog/test hook; it does not implement
/// missing writers.
#[must_use]
pub fn check_configuration_authority(
    corpus: &BTreeMap<&str, &str>,
) -> ConfigurationAuthorityReport {
    check_with(CONFIGURATION_AUTHORITY, DERIVED_WORKSPACE_EVIDENCE, corpus)
}

fn check_with(
    catalog: &[FieldAuthority],
    evidence: &[DerivedWorkspaceEvidence],
    corpus: &BTreeMap<&str, &str>,
) -> ConfigurationAuthorityReport {
    let violations = validate_derived_workspace(catalog, evidence, corpus);
    let failing: BTreeSet<&str> = violations.iter().flat_map(violation_ids).collect();
    let evidence_by_id: BTreeMap<&str, &DerivedWorkspaceEvidence> =
        evidence.iter().map(|row| (row.id, row)).collect();

    let rows = catalog
        .iter()
        .filter(|field| field.scope == ConfigScope::DerivedWorkspaceFolder)
        .map(|field| {
            let row = evidence_by_id.get(field.id);
            let state = row.map(|row| row.state).unwrap_or(DerivedWorkspaceLifecycleState::Planned);
            let owning_issue = row.map(|row| row.owning_issue).unwrap_or(0);
            let writer = row.map(|row| row.writer).unwrap_or("");
            let live = state == DerivedWorkspaceLifecycleState::Live && !failing.contains(field.id);
            DerivedWorkspaceRowStatus { id: field.id, state, live, owning_issue, writer }
        })
        .collect();

    ConfigurationAuthorityReport { rows, violations }
}

fn violation_id(violation: &DerivedWorkspaceViolation) -> Option<&'static str> {
    match violation {
        DerivedWorkspaceViolation::MissingEvidence { id }
        | DerivedWorkspaceViolation::ExtraEvidence { id }
        | DerivedWorkspaceViolation::DuplicateEvidence { id }
        | DerivedWorkspaceViolation::LiveMissingWriter { id, .. }
        | DerivedWorkspaceViolation::WriterUnreachable { id, .. }
        | DerivedWorkspaceViolation::MissingInvalidation { id, .. }
        | DerivedWorkspaceViolation::LiveMissingProof { id }
        | DerivedWorkspaceViolation::StaleProofId { id, .. }
        | DerivedWorkspaceViolation::LiveExactEmpty { id }
        | DerivedWorkspaceViolation::NonLiveMissingOwner { id }
        | DerivedWorkspaceViolation::NonLiveMissingWake { id }
        | DerivedWorkspaceViolation::UnknownSource { id, .. }
        | DerivedWorkspaceViolation::NonProductionSource { id, .. } => Some(*id),
        DerivedWorkspaceViolation::DuplicateWriter { .. } => None,
    }
}

fn violation_ids(violation: &DerivedWorkspaceViolation) -> Vec<&'static str> {
    match violation {
        DerivedWorkspaceViolation::DuplicateWriter { ids, .. } => ids.clone(),
        other => violation_id(other).into_iter().collect(),
    }
}

fn validate_derived_workspace(
    catalog: &[FieldAuthority],
    evidence: &[DerivedWorkspaceEvidence],
    corpus: &BTreeMap<&str, &str>,
) -> Vec<DerivedWorkspaceViolation> {
    let mut violations = Vec::new();
    let derived: BTreeMap<&str, &FieldAuthority> = catalog
        .iter()
        .filter(|field| field.scope == ConfigScope::DerivedWorkspaceFolder)
        .map(|field| (field.id, field))
        .collect();

    let mut seen_evidence = BTreeSet::new();
    let mut live_writers: BTreeMap<&str, Vec<(&str, Option<&str>)>> = BTreeMap::new();

    for row in evidence {
        if !seen_evidence.insert(row.id) {
            violations.push(DerivedWorkspaceViolation::DuplicateEvidence { id: row.id });
            continue;
        }
        let Some(field) = derived.get(row.id) else {
            violations.push(DerivedWorkspaceViolation::ExtraEvidence { id: row.id });
            continue;
        };
        validate_row(field, row, corpus, &mut violations);
        if row.state == DerivedWorkspaceLifecycleState::Live {
            live_writers.entry(row.writer).or_default().push((row.id, row.fan_out_group));
        }
    }

    for id in derived.keys() {
        if !seen_evidence.contains(id) {
            violations.push(DerivedWorkspaceViolation::MissingEvidence { id });
        }
    }

    for (writer, owners) in live_writers {
        if owners.len() < 2 {
            continue;
        }
        let groups: BTreeSet<Option<&str>> = owners.iter().map(|(_, group)| *group).collect();
        let shared = groups.len() == 1 && groups.iter().any(|group| group.is_some());
        if !shared {
            violations.push(DerivedWorkspaceViolation::DuplicateWriter {
                writer,
                ids: owners.iter().map(|(id, _)| *id).collect(),
            });
        }
    }

    violations.sort();
    violations
}

fn validate_row(
    field: &FieldAuthority,
    row: &DerivedWorkspaceEvidence,
    corpus: &BTreeMap<&str, &str>,
    violations: &mut Vec<DerivedWorkspaceViolation>,
) {
    match row.state {
        DerivedWorkspaceLifecycleState::Live => validate_live_row(field, row, corpus, violations),
        DerivedWorkspaceLifecycleState::Planned
        | DerivedWorkspaceLifecycleState::IntentionallyDormant => {
            if row.owning_issue == 0 {
                violations.push(DerivedWorkspaceViolation::NonLiveMissingOwner { id: row.id });
            }
            if row.wake.is_none() {
                violations.push(DerivedWorkspaceViolation::NonLiveMissingWake { id: row.id });
            }
            if !row.writer.is_empty() {
                require_defined(row.id, row.writer, row.writer_source, corpus, violations);
            }
        }
        DerivedWorkspaceLifecycleState::Retired => {
            if row.owning_issue == 0 {
                violations.push(DerivedWorkspaceViolation::NonLiveMissingOwner { id: row.id });
            }
        }
    }
}

fn validate_live_row(
    field: &FieldAuthority,
    row: &DerivedWorkspaceEvidence,
    corpus: &BTreeMap<&str, &str>,
    violations: &mut Vec<DerivedWorkspaceViolation>,
) {
    if row.owning_issue == 0 {
        violations.push(DerivedWorkspaceViolation::NonLiveMissingOwner { id: row.id });
    }
    if matches!(
        row.delete_disposition,
        AbsenceDisposition::ExactEmpty | AbsenceDisposition::SilentRetention
    ) || matches!(
        row.unavailable_disposition,
        AbsenceDisposition::ExactEmpty | AbsenceDisposition::SilentRetention
    ) {
        violations.push(DerivedWorkspaceViolation::LiveExactEmpty { id: row.id });
    }

    require_production_source(row.id, row.writer_source, violations);
    require_defined(row.id, row.writer, row.writer_source, corpus, violations);
    if let Some(refresh) = row.refresh_writer {
        require_defined(row.id, refresh, row.writer_source, corpus, violations);
    }

    match (row.init_route, row.init_route_source) {
        (Some(route), Some(source)) => {
            require_production_source(row.id, source, violations);
            require_defined(row.id, route, source, corpus, violations);
            require_call(row.id, row.writer, route, source, corpus, violations);
        }
        _ => violations.push(DerivedWorkspaceViolation::WriterUnreachable {
            id: row.id,
            writer: row.writer,
            route: row.init_route.unwrap_or(""),
            source: row.init_route_source.unwrap_or(""),
        }),
    }

    let file_markers = file_markers_for(field, row);
    if !file_markers.is_empty() {
        match (row.invalidation_route, row.invalidation_route_source) {
            (Some(route), Some(source)) => {
                require_production_source(row.id, source, violations);
                require_defined(row.id, route, source, corpus, violations);
                let called = row.refresh_writer.unwrap_or(row.writer);
                require_call(row.id, called, route, source, corpus, violations);
            }
            _ => {
                for marker in file_markers.iter().copied() {
                    violations.push(DerivedWorkspaceViolation::MissingInvalidation {
                        id: row.id,
                        marker,
                    });
                }
            }
        }
        let marker_source = row.marker_authority_source.or(row.invalidation_route_source);
        for marker in file_markers {
            if !row.invalidation_markers.contains(&marker) {
                violations
                    .push(DerivedWorkspaceViolation::MissingInvalidation { id: row.id, marker });
                continue;
            }
            let Some(source) = marker_source else {
                violations
                    .push(DerivedWorkspaceViolation::MissingInvalidation { id: row.id, marker });
                continue;
            };
            match corpus.get(source) {
                None => {
                    violations.push(DerivedWorkspaceViolation::UnknownSource { id: row.id, source })
                }
                Some(text) if !text.contains(marker) => violations
                    .push(DerivedWorkspaceViolation::MissingInvalidation { id: row.id, marker }),
                Some(_) => {}
            }
        }
    }

    match (row.proof_test, row.proof_source) {
        (Some(proof_test), Some(source)) => match corpus.get(source) {
            None => {
                violations.push(DerivedWorkspaceViolation::UnknownSource { id: row.id, source })
            }
            Some(text) if !defines_test_function(text, proof_test) => {
                violations.push(DerivedWorkspaceViolation::StaleProofId {
                    id: row.id,
                    proof_test,
                    source,
                });
            }
            Some(_) => {}
        },
        _ => violations.push(DerivedWorkspaceViolation::LiveMissingProof { id: row.id }),
    }
}

fn require_production_source(
    id: &'static str,
    source: &'static str,
    violations: &mut Vec<DerivedWorkspaceViolation>,
) {
    if source.contains("/tests/") || source.contains("_tests.rs") {
        violations.push(DerivedWorkspaceViolation::NonProductionSource { id, source });
    }
}

fn require_defined(
    id: &'static str,
    name: &'static str,
    source: &'static str,
    corpus: &BTreeMap<&str, &str>,
    violations: &mut Vec<DerivedWorkspaceViolation>,
) {
    match corpus.get(source) {
        None => violations.push(DerivedWorkspaceViolation::UnknownSource { id, source }),
        Some(text) if !defines_function(text, name) => {
            violations.push(DerivedWorkspaceViolation::LiveMissingWriter {
                id,
                writer: name,
                source,
            });
        }
        Some(_) => {}
    }
}

fn require_call(
    id: &'static str,
    writer: &'static str,
    route: &'static str,
    source: &'static str,
    corpus: &BTreeMap<&str, &str>,
    violations: &mut Vec<DerivedWorkspaceViolation>,
) {
    match corpus.get(source) {
        None => violations.push(DerivedWorkspaceViolation::UnknownSource { id, source }),
        Some(text) => {
            let reachable =
                function_body(text, route).is_some_and(|body| mentions_call(body, writer));
            if !reachable {
                violations.push(DerivedWorkspaceViolation::WriterUnreachable {
                    id,
                    writer,
                    route,
                    source,
                });
            }
        }
    }
}

fn file_markers_for(field: &FieldAuthority, row: &DerivedWorkspaceEvidence) -> Vec<&'static str> {
    let mut markers = Vec::new();
    let mut seen = BTreeSet::new();
    let candidates =
        field.source_markers.iter().copied().chain(row.contributing_sources.iter().copied());
    for marker in candidates {
        if marker == field.rust_field || row.initialization_only_markers.contains(&marker) {
            continue;
        }
        if seen.insert(marker) {
            markers.push(marker);
        }
    }
    markers
}

fn defines_function(source: &str, name: &str) -> bool {
    function_definition_name_start(source, name).is_some()
}

fn defines_test_function(source: &str, name: &str) -> bool {
    function_definition_name_start(source, name)
        .is_some_and(|start| has_test_attribute(&source[..start]))
}

fn function_body<'a>(source: &'a str, name: &str) -> Option<&'a str> {
    let start = function_definition_name_start(source, name)?;
    let after_sig = start.checked_add(name.len())?.checked_add(1)?;
    let open = find_next_code_byte(source, after_sig, b'{')?;
    let close = matching_brace_end(source, open)?;
    source.get(open.checked_add(1)?..close)
}

fn function_definition_name_start(source: &str, name: &str) -> Option<usize> {
    if name.is_empty() {
        return None;
    }
    let needle = format!("{name}(");
    let mut rest = source;
    let mut absolute: usize = 0;
    while let Some(idx) = rest.find(&needle) {
        let start = absolute.checked_add(idx)?;
        if is_fn_definition(&source[..start]) {
            return Some(start);
        }
        let skip = idx.checked_add(needle.len())?;
        rest = rest.get(skip..)?;
        absolute = absolute.checked_add(skip)?;
    }
    None
}

fn mentions_call(source: &str, name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    let needle = format!("{name}(");
    let mut rest = source;
    while let Some(idx) = rest.find(&needle) {
        if !is_fn_definition(&rest[..idx]) {
            return true;
        }
        let Some(skip) = idx.checked_add(needle.len()) else {
            return false;
        };
        let Some(next) = rest.get(skip..) else {
            return false;
        };
        rest = next;
    }
    false
}

fn is_fn_definition(prefix: &str) -> bool {
    prefix.trim_end().ends_with("fn")
}

fn has_test_attribute(prefix: &str) -> bool {
    let trimmed = prefix.trim_end();
    let Some(without_fn) = trimmed.strip_suffix("fn") else {
        return false;
    };
    let without_fn = without_fn.trim_end();
    let attr_start = without_fn.rfind(['}', ';', '{']).map(|idx| idx + 1).unwrap_or(0);
    let Some(attrs) = without_fn.get(attr_start..) else {
        return false;
    };
    contains_test_attr(attrs)
}

fn contains_test_attr(attrs: &str) -> bool {
    let mut rest = attrs;
    while let Some(idx) = rest.find("#[test") {
        let Some(after) = rest.get(idx + 6..) else {
            return false;
        };
        if after.starts_with(']') || after.starts_with('(') {
            return true;
        }
        rest = after;
    }
    false
}

fn find_next_code_byte(source: &str, from: usize, needle: u8) -> Option<usize> {
    walk_code(source, from, |byte, _depth| byte == needle)
}

fn matching_brace_end(source: &str, open: usize) -> Option<usize> {
    if source.as_bytes().get(open).copied() != Some(b'{') {
        return None;
    }
    walk_code(source, open, |byte, depth| byte == b'}' && depth == 0)
}

fn walk_code(source: &str, from: usize, mut hit: impl FnMut(u8, usize) -> bool) -> Option<usize> {
    let bytes = source.as_bytes();
    let mut i = from;
    let mut depth = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'/' if bytes.get(i + 1) == Some(&b'/') => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                i += 2;
                while i < bytes.len() && !(bytes[i] == b'*' && bytes.get(i + 1) == Some(&b'/')) {
                    i += 1;
                }
                i = i.saturating_add(2);
                continue;
            }
            b'"' => {
                i += 1;
                while i < bytes.len() {
                    if bytes[i] == b'\\' {
                        i = i.saturating_add(2);
                        continue;
                    }
                    if bytes[i] == b'"' {
                        break;
                    }
                    i += 1;
                }
            }
            b'\'' => {
                i += 1;
                while i < bytes.len() {
                    if bytes[i] == b'\\' {
                        i = i.saturating_add(2);
                        continue;
                    }
                    if bytes[i] == b'\'' {
                        break;
                    }
                    i += 1;
                }
            }
            b'{' => {
                if hit(b'{', depth) {
                    return Some(i);
                }
                depth = depth.saturating_add(1);
            }
            b'}' => {
                depth = depth.saturating_sub(1);
                if hit(b'}', depth) {
                    return Some(i);
                }
            }
            byte => {
                if hit(byte, depth) {
                    return Some(i);
                }
            }
        }
        i += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::super::{
        ConfigConsumer, ConfigOwner, ConfigScope, ConfigSensitivity, ConfigSource,
        ConfigValidation, ConfigValueKind, EvidencePolicy, InvalidValueFallback, InvalidationClass,
    };
    use super::*;
    use perl_test_must::{must_some_with, must_with};

    fn current_corpus() -> BTreeMap<&'static str, &'static str> {
        BTreeMap::from([
            (CONFIG_MOD, include_str!("../config/mod.rs")),
            (METADATA_DEPENDENCIES, include_str!("../config/metadata_dependencies.rs")),
            (
                WORKSPACE_FOLDER,
                include_str!("../../../perl-lsp-rs/src/runtime/workspace_folder.rs"),
            ),
            (
                METADATA_INVALIDATION_TESTS,
                include_str!("../../../perl-lsp-rs/src/runtime/metadata_invalidation_tests.rs"),
            ),
        ])
    }

    static NO_CONSUMERS: &[ConfigConsumer] = &[];
    static PROJECT_METADATA: &[ConfigSource] =
        &[ConfigSource::CompiledDefault, ConfigSource::ProjectMetadata];

    fn derived_row(
        id: &'static str,
        rust_field: &'static str,
        markers: &'static [&'static str],
    ) -> FieldAuthority {
        FieldAuthority {
            id,
            owner: ConfigOwner::Workspace,
            rust_field,
            scope: ConfigScope::DerivedWorkspaceFolder,
            value_kind: ConfigValueKind::DerivedList,
            sources: PROJECT_METADATA,
            validation: ConfigValidation::Derived,
            invalid_fallback: InvalidValueFallback::RecomputeDerived,
            sensitivity: ConfigSensitivity::Ordinary,
            evidence_policy: EvidencePolicy::DerivedDigestOnly,
            invalidation: InvalidationClass::WorkspaceDiscovery,
            consumers: NO_CONSUMERS,
            source_markers: markers,
        }
    }

    fn live_template(id: &'static str, writer: &'static str) -> DerivedWorkspaceEvidence {
        DerivedWorkspaceEvidence {
            id,
            writer,
            writer_source: "prod.rs",
            init_route: Some("init_folder"),
            init_route_source: Some("route.rs"),
            refresh_writer: Some("apply_reads"),
            invalidation_route: Some("refresh_from_reads"),
            invalidation_route_source: Some("route.rs"),
            invalidation_markers: &["META.json", "cpanfile"],
            initialization_only_markers: &["declared_dependencies"],
            contributing_sources: &[],
            marker_authority_source: Some("markers.rs"),
            delete_disposition: AbsenceDisposition::RecomputeFromRemainingSources,
            unavailable_disposition: AbsenceDisposition::RetainPreviousWithLimitation,
            owning_issue: 1,
            proof_test: Some("deleted_source_downgrades_fact"),
            proof_source: Some("proof.rs"),
            state: DerivedWorkspaceLifecycleState::Live,
            wake: None,
            fan_out_group: None,
        }
    }

    fn valid_corpus() -> BTreeMap<&'static str, &'static str> {
        BTreeMap::from([
            (
                "prod.rs",
                "pub fn refresh_declared_dependencies(&mut self, root: &Path) {}\npub fn apply_reads(&mut self, reads: &[]) {}\n",
            ),
            (
                "route.rs",
                "pub fn init_folder(&mut self) { self.refresh_declared_dependencies(path); }\npub fn refresh_from_reads(&mut self, reads: &[]) { self.apply_reads(reads); }\n",
            ),
            ("markers.rs", "const FILES: &[&str] = &[\"META.json\", \"cpanfile\"];\n"),
            ("proof.rs", "#[test]\nfn deleted_source_downgrades_fact() {}\n"),
        ])
    }

    #[test]
    fn current_derived_workspace_rows_are_classified_and_the_live_row_is_proven() {
        let corpus = current_corpus();
        let report = check_configuration_authority(&corpus);
        assert!(
            report.violations.is_empty(),
            "current derived workspace evidence must hold: {:?}",
            report.violations
        );

        let declared = must_some_with(
            report.rows.iter().find(|row| row.id == "workspace.declared_dependencies"),
            "declared_dependencies row",
        );
        assert_eq!(declared.state, DerivedWorkspaceLifecycleState::Live);
        assert!(declared.live, "proven live row must be reported live: {declared:?}");
        assert_eq!(declared.writer, "refresh_declared_dependencies");

        let hints = must_some_with(
            report.rows.iter().find(|row| row.id == "workspace.native_build_hints"),
            "native_build_hints row",
        );
        assert_eq!(hints.state, DerivedWorkspaceLifecycleState::Planned);
        assert!(!hints.live, "planned row must not be reported live: {hints:?}");
        assert_eq!(hints.owning_issue, 16826);
        assert_eq!(hints.writer, "refresh_native_build_hints");
        assert_eq!(report.rows.len(), 2);
    }

    #[test]
    fn marking_native_build_hints_live_without_bound_refresh_writer_fails() {
        let field = must_some_with(
            CONFIGURATION_AUTHORITY.iter().find(|field| field.id == "workspace.native_build_hints"),
            "native_build_hints catalog row",
        );
        let evidence = DerivedWorkspaceEvidence {
            id: "workspace.native_build_hints",
            writer: "refresh_native_build_hints",
            writer_source: CONFIG_MOD,
            init_route: Some("refresh_workspace_metadata"),
            init_route_source: Some(WORKSPACE_FOLDER),
            // Deliberately omit the captured-read writer: falling back to the disk
            // writer must not establish invalidation reachability.
            refresh_writer: None,
            invalidation_route: Some("refresh_workspace_metadata_from_reads"),
            invalidation_route_source: Some(WORKSPACE_FOLDER),
            invalidation_markers: &["Makefile.PL", "Build.PL"],
            initialization_only_markers: &["native_build_hints"],
            contributing_sources: &[],
            marker_authority_source: Some(CONFIG_MOD),
            delete_disposition: AbsenceDisposition::RecomputeFromRemainingSources,
            unavailable_disposition: AbsenceDisposition::RetainPreviousWithLimitation,
            owning_issue: 16826,
            proof_test: Some("deleted_cpanfile_downgrades_declared_dependencies"),
            proof_source: Some(METADATA_INVALIDATION_TESTS),
            state: DerivedWorkspaceLifecycleState::Live,
            wake: None,
            fan_out_group: None,
        };
        let violations = validate_derived_workspace(&[*field], &[evidence], &current_corpus());
        assert!(
            violations.iter().any(|violation| matches!(
                violation,
                DerivedWorkspaceViolation::WriterUnreachable {
                    id: "workspace.native_build_hints",
                    writer: "refresh_native_build_hints",
                    route: "refresh_workspace_metadata_from_reads",
                    source: WORKSPACE_FOLDER
                }
            )),
            "{violations:?}"
        );
        assert!(
            !check_with(&[*field], &[evidence], &current_corpus()).rows[0].live,
            "unproven live claim must not be reported live"
        );
    }

    #[test]
    fn valid_synthetic_live_row_passes() {
        let catalog = [derived_row(
            "workspace.declared_dependencies",
            "declared_dependencies",
            &["declared_dependencies", "META.json", "cpanfile"],
        )];
        let evidence =
            [live_template("workspace.declared_dependencies", "refresh_declared_dependencies")];
        let report = check_with(&catalog, &evidence, &valid_corpus());
        assert!(report.violations.is_empty(), "{:?}", report.violations);
        assert!(report.rows[0].live);
    }

    #[test]
    fn planned_and_dormant_rows_cannot_be_reported_as_live() {
        let catalog = [derived_row(
            "workspace.native_build_hints",
            "native_build_hints",
            &["native_build_hints", "Makefile.PL"],
        )];
        let mut planned =
            live_template("workspace.native_build_hints", "refresh_native_build_hints");
        planned.state = DerivedWorkspaceLifecycleState::Planned;
        planned.wake = Some("owner lands the writer route");
        planned.owning_issue = 16826;
        planned.init_route = None;
        planned.init_route_source = None;
        planned.proof_test = None;
        planned.proof_source = None;
        planned.writer_source = "prod.rs";
        let corpus =
            BTreeMap::from([("prod.rs", "pub fn refresh_native_build_hints(&mut self) {}\n")]);
        let report = check_with(&catalog, &[planned], &corpus);
        assert!(report.violations.is_empty(), "{:?}", report.violations);
        assert!(!report.rows[0].live);

        planned.state = DerivedWorkspaceLifecycleState::IntentionallyDormant;
        let report = check_with(&catalog, &[planned], &corpus);
        assert!(report.violations.is_empty(), "{:?}", report.violations);
        assert!(!report.rows[0].live);
        assert_eq!(report.rows[0].state, DerivedWorkspaceLifecycleState::IntentionallyDormant);
    }

    #[test]
    fn live_row_fails_when_the_only_writer_call_is_removed() {
        let catalog = [derived_row(
            "workspace.declared_dependencies",
            "declared_dependencies",
            &["declared_dependencies", "META.json", "cpanfile"],
        )];
        let evidence =
            [live_template("workspace.declared_dependencies", "refresh_declared_dependencies")];
        let mut corpus = valid_corpus();
        corpus.insert(
            "route.rs",
            "pub fn init_folder(&mut self) {}\npub fn refresh_from_reads(&mut self, reads: &[]) { self.apply_reads(reads); }\n",
        );
        let violations = validate_derived_workspace(&catalog, &evidence, &corpus);
        assert!(
            violations.iter().any(|violation| matches!(
                violation,
                DerivedWorkspaceViolation::WriterUnreachable {
                    id: "workspace.declared_dependencies",
                    writer: "refresh_declared_dependencies",
                    route: "init_folder",
                    source: "route.rs"
                }
            )),
            "{violations:?}"
        );
    }

    #[test]
    fn live_row_fails_when_writer_exists_but_has_no_production_route() {
        let catalog = [derived_row(
            "workspace.native_build_hints",
            "native_build_hints",
            &["native_build_hints", "Makefile.PL", "Build.PL"],
        )];
        let mut evidence =
            live_template("workspace.native_build_hints", "refresh_native_build_hints");
        evidence.init_route = None;
        evidence.init_route_source = None;
        evidence.initialization_only_markers = &["native_build_hints"];
        evidence.invalidation_markers = &["Makefile.PL", "Build.PL"];
        let mut corpus = valid_corpus();
        corpus.insert(
            "prod.rs",
            "pub fn refresh_native_build_hints(&mut self) {}\npub fn apply_reads(&mut self, reads: &[]) {}\n",
        );
        let violations = validate_derived_workspace(&catalog, &[evidence], &corpus);
        assert!(
            violations.iter().any(|violation| matches!(
                violation,
                DerivedWorkspaceViolation::WriterUnreachable {
                    id: "workspace.native_build_hints",
                    writer: "refresh_native_build_hints",
                    ..
                }
            )),
            "{violations:?}"
        );
    }

    #[test]
    fn live_row_fails_when_invalidation_markers_are_dropped() {
        let catalog = [derived_row(
            "workspace.declared_dependencies",
            "declared_dependencies",
            &["declared_dependencies", "META.json", "cpanfile"],
        )];
        let mut evidence =
            live_template("workspace.declared_dependencies", "refresh_declared_dependencies");
        evidence.invalidation_markers = &["cpanfile"];
        let violations = validate_derived_workspace(&catalog, &[evidence], &valid_corpus());
        assert!(
            violations.iter().any(|violation| matches!(
                violation,
                DerivedWorkspaceViolation::MissingInvalidation {
                    id: "workspace.declared_dependencies",
                    marker: "META.json"
                }
            )),
            "{violations:?}"
        );
    }

    #[test]
    fn live_row_fails_without_a_behavior_test() {
        let catalog = [derived_row(
            "workspace.declared_dependencies",
            "declared_dependencies",
            &["declared_dependencies", "META.json", "cpanfile"],
        )];
        let mut evidence =
            live_template("workspace.declared_dependencies", "refresh_declared_dependencies");
        evidence.proof_test = None;
        evidence.proof_source = None;
        let violations = validate_derived_workspace(&catalog, &[evidence], &valid_corpus());
        assert_eq!(
            violations
                .iter()
                .filter(|violation| matches!(
                    violation,
                    DerivedWorkspaceViolation::LiveMissingProof {
                        id: "workspace.declared_dependencies"
                    }
                ))
                .count(),
            1,
            "{violations:?}"
        );
    }

    #[test]
    fn live_row_fails_when_the_proof_test_id_is_stale() {
        let catalog = [derived_row(
            "workspace.declared_dependencies",
            "declared_dependencies",
            &["declared_dependencies", "META.json", "cpanfile"],
        )];
        let mut evidence =
            live_template("workspace.declared_dependencies", "refresh_declared_dependencies");
        evidence.proof_test = Some("renamed_without_catalog_update");
        let violations = validate_derived_workspace(&catalog, &[evidence], &valid_corpus());
        assert!(
            violations.iter().any(|violation| matches!(
                violation,
                DerivedWorkspaceViolation::StaleProofId {
                    id: "workspace.declared_dependencies",
                    proof_test: "renamed_without_catalog_update",
                    source: "proof.rs"
                }
            )),
            "{violations:?}"
        );
    }

    #[test]
    fn two_live_rows_cannot_share_a_generic_writer_without_fan_out() {
        let catalog = [
            derived_row("workspace.one", "one", &["one"]),
            derived_row("workspace.two", "two", &["two"]),
        ];
        let mut first = live_template("workspace.one", "refresh_all_metadata");
        first.initialization_only_markers = &["one"];
        first.invalidation_markers = &[];
        first.invalidation_route = None;
        first.invalidation_route_source = None;
        first.refresh_writer = None;
        let mut second = first;
        second.id = "workspace.two";
        second.initialization_only_markers = &["two"];
        let mut corpus = valid_corpus();
        corpus.insert("prod.rs", "pub fn refresh_all_metadata(&mut self) {}\n");
        corpus
            .insert("route.rs", "pub fn init_folder(&mut self) { self.refresh_all_metadata(); }\n");
        let report = check_with(&catalog, &[first, second], &corpus);
        assert!(
            report.violations.iter().any(|violation| matches!(
                violation,
                DerivedWorkspaceViolation::DuplicateWriter { writer: "refresh_all_metadata", .. }
            )),
            "{:?}",
            report.violations
        );
        assert!(
            report.rows.iter().all(|row| !row.live),
            "every duplicate-writer owner must be non-live: {:?}",
            report.rows
        );
        assert_eq!(report.rows.len(), 2);
    }

    #[test]
    fn live_row_rejects_exact_empty_or_silent_retention() {
        let catalog = [derived_row(
            "workspace.declared_dependencies",
            "declared_dependencies",
            &["declared_dependencies", "META.json", "cpanfile"],
        )];
        let mut evidence =
            live_template("workspace.declared_dependencies", "refresh_declared_dependencies");
        evidence.delete_disposition = AbsenceDisposition::ExactEmpty;
        let violations = validate_derived_workspace(&catalog, &[evidence], &valid_corpus());
        assert!(
            violations.iter().any(|violation| matches!(
                violation,
                DerivedWorkspaceViolation::LiveExactEmpty { id: "workspace.declared_dependencies" }
            )),
            "{violations:?}"
        );

        evidence.delete_disposition = AbsenceDisposition::RecomputeFromRemainingSources;
        evidence.unavailable_disposition = AbsenceDisposition::SilentRetention;
        let violations = validate_derived_workspace(&catalog, &[evidence], &valid_corpus());
        assert!(
            violations.iter().any(|violation| matches!(
                violation,
                DerivedWorkspaceViolation::LiveExactEmpty { id: "workspace.declared_dependencies" }
            )),
            "{violations:?}"
        );
    }

    #[test]
    fn writer_definition_alone_is_not_a_production_call() {
        assert!(defines_function(
            "pub fn refresh_native_build_hints(&mut self, workspace_root: &Path) {}",
            "refresh_native_build_hints"
        ));
        assert!(!mentions_call(
            "pub fn refresh_native_build_hints(&mut self, workspace_root: &Path) {}",
            "refresh_native_build_hints"
        ));
        assert!(mentions_call(
            "self.effective_workspace_config.refresh_native_build_hints(path);",
            "refresh_native_build_hints"
        ));
    }

    #[test]
    fn writer_call_outside_the_registered_route_does_not_count() {
        let catalog = [derived_row(
            "workspace.declared_dependencies",
            "declared_dependencies",
            &["declared_dependencies", "META.json", "cpanfile"],
        )];
        let evidence =
            [live_template("workspace.declared_dependencies", "refresh_declared_dependencies")];
        let mut corpus = valid_corpus();
        corpus.insert(
            "route.rs",
            concat!(
                "pub fn init_folder(&mut self) {}\n",
                "pub fn refresh_from_reads(&mut self, reads: &[]) { self.apply_reads(reads); }\n",
                "pub fn unrelated(&mut self) { self.refresh_declared_dependencies(path); }\n",
            ),
        );
        let violations = validate_derived_workspace(&catalog, &evidence, &corpus);
        assert!(
            violations.iter().any(|violation| matches!(
                violation,
                DerivedWorkspaceViolation::WriterUnreachable {
                    id: "workspace.declared_dependencies",
                    writer: "refresh_declared_dependencies",
                    route: "init_folder",
                    source: "route.rs"
                }
            )),
            "{violations:?}"
        );
    }

    #[test]
    fn live_row_fails_when_the_proof_symbol_is_not_a_test() {
        let catalog = [derived_row(
            "workspace.declared_dependencies",
            "declared_dependencies",
            &["declared_dependencies", "META.json", "cpanfile"],
        )];
        let evidence =
            [live_template("workspace.declared_dependencies", "refresh_declared_dependencies")];
        let mut corpus = valid_corpus();
        corpus.insert("proof.rs", "fn deleted_source_downgrades_fact() { assert!(true); }\n");
        let violations = validate_derived_workspace(&catalog, &evidence, &corpus);
        assert!(
            violations.iter().any(|violation| matches!(
                violation,
                DerivedWorkspaceViolation::StaleProofId {
                    id: "workspace.declared_dependencies",
                    proof_test: "deleted_source_downgrades_fact",
                    source: "proof.rs"
                }
            )),
            "{violations:?}"
        );
    }

    #[test]
    fn cfg_test_module_is_not_a_behavior_test_identity() {
        assert!(!defines_test_function(
            "#[cfg(test)]\nfn deleted_source_downgrades_fact() {}\n",
            "deleted_source_downgrades_fact"
        ));
        assert!(defines_test_function(
            "#[test]\nfn deleted_source_downgrades_fact() {}\n",
            "deleted_source_downgrades_fact"
        ));
    }

    #[test]
    fn live_row_fails_when_a_contributing_source_is_omitted_from_invalidation() {
        let catalog = [derived_row(
            "workspace.declared_dependencies",
            "declared_dependencies",
            &["declared_dependencies", "META.json", "cpanfile"],
        )];
        let mut evidence =
            live_template("workspace.declared_dependencies", "refresh_declared_dependencies");
        evidence.contributing_sources = &["Makefile.PL", "Build.PL", "dist.ini", "META.yml"];
        let violations = validate_derived_workspace(&catalog, &[evidence], &valid_corpus());
        for marker in ["Makefile.PL", "Build.PL", "dist.ini", "META.yml"] {
            assert!(
                violations.iter().any(|violation| matches!(
                    violation,
                    DerivedWorkspaceViolation::MissingInvalidation {
                        id: "workspace.declared_dependencies",
                        marker: omitted
                    } if *omitted == marker
                )),
                "missing {marker}: {violations:?}"
            );
        }
    }

    #[test]
    fn missing_lifecycle_evidence_names_the_catalog_row() {
        let catalog = [derived_row(
            "workspace.native_build_hints",
            "native_build_hints",
            &["native_build_hints"],
        )];
        let violations = validate_derived_workspace(&catalog, &[], &BTreeMap::new());
        assert_eq!(
            violations,
            vec![DerivedWorkspaceViolation::MissingEvidence { id: "workspace.native_build_hints" }]
        );
        assert!(violations[0].to_string().contains("workspace.native_build_hints"));
    }

    #[test]
    fn machine_readable_report_omits_live_for_failing_live_claims() {
        let catalog = [derived_row(
            "workspace.native_build_hints",
            "native_build_hints",
            &["native_build_hints", "Makefile.PL"],
        )];
        let evidence =
            [live_template("workspace.native_build_hints", "refresh_native_build_hints")];
        let report = check_with(&catalog, &evidence, &valid_corpus());
        assert!(!report.is_ok());
        assert!(!report.rows[0].live);
        assert_eq!(report.rows[0].state, DerivedWorkspaceLifecycleState::Live);
        let encoded = must_with(serde_json::to_string(&report), "report serializes");
        assert!(encoded.contains("\"live\":false"));
        assert!(encoded.contains("workspace.native_build_hints"));
    }
}

//! Checked pre-registration manifest for reusable Emacs host journeys,
//! fixture/subset bindings, and receipt cells (#11768).
//!
//! Ownership split — consumed, never duplicated:
//!
//! - `editor_client_compat.v1` ([`crate::editor_client_compat`]) owns the
//!   generic actual-editor contract this manifest binds into, including the
//!   `EvidenceStage` ladder a cell may ever reach.
//! - The canonical expectation package `perl-agent-client-v1`
//!   ([`crate::client_compat_fixture`]) owns underlying Perl semantic truth.
//!   Cells *reference* expectation owners by id and version digest; missing
//!   canonical truth blocks a cell instead of inviting an Emacs-local oracle.
//! - `.ci/editor-clients/emacs-subjects.v1.json` (#11744 SUBJ_CORE) owns the
//!   exact client/source subject rows; this manifest binds subject identity
//!   only through cohort membership over that landed fixture authority, and
//!   tests verify the fixture still exists so an absent authority fails
//!   closed.
//! - #11366 remains the root-fixture authority: root-sensitive cells record a
//!   `root_11366.<role>` reference token only. Root fixture bytes are never
//!   copied here, and a manually prebound root can never be represented as
//!   stock discovery because only discovery-generation dimensions and role
//!   tokens are expressible.
//! - #11360/#11361 own typed host observations and checked observation →
//!   receipt mapping; every cell records which `11361.*` producer mapping may
//!   consume its future receipts without becoming an oracle itself.
//!
//! What this manifest decides (#11768 authority split): the Emacs journey
//! classes current main governs, which diagnostic cohorts each belongs to,
//! the exact pull-diagnostic protocol cells, required false-subject controls,
//! coordinate/newline applicability, terminal limitations, and the maximum
//! evidence stage any cell may ever claim. It binds membership and controls,
//! not pass/fail outcomes: validating the manifest proves no behavior.
//!
//! Fail-closed laws enforced by [`validate_registry`] and
//! [`validate_compiled_registry`]:
//!
//! - unknown/duplicate cell IDs, an ID outside `emacs.<class>.<name>`, or an
//!   ID whose class segment disagrees with its declared class are rejected;
//! - a pull-protocol surface outside the standalone-Eglot-pull cohort, or a
//!   protocol-membership cell promising host-visible semantics without its
//!   mandatory limitation, is rejected (a push transcript can never satisfy a
//!   pull cell, and a protocol frame alone can never satisfy host visibility);
//! - a citation of an unknown canonical expectation id, set id, producer
//!   mapping, generation dimension, control token, or coordinate domain is
//!   rejected — missing canonical truth blocks the cell;
//! - a core-coverage registry carrying an optional #9413 documented-feature
//!   cell (or the inverse) is rejected, so feature depth can never silently
//!   become core-required;
//! - empty bindings — cohorts, fixtures, expectations, dimensions, controls,
//!   ceilings, claim stages — are rejected;
//! - digests cover every binding field, so any semantic edit is a visible
//!   identity change and second-run output is byte-stable.

use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::Path;

use crate::client_compat_fixture::{
    CANONICAL_EXPECTATION_IDS, CANONICAL_EXPECTATION_SET_ID, canonical_expectation_set_digest,
};
use crate::editor_client_compat::EvidenceStage;

/// Identity of this registration model.
pub const MANIFEST_SCHEMA_VERSION: &str = "emacs_host_journeys.v1";

/// The only cell-ID namespace this manifest admits: `emacs.<class>.<name>`,
/// where `<class>` must be one of the registered journey classes below.
pub const CELL_ID_PREFIX: &str = "emacs.";

/// Landed fixture authority cells may bind (#11744 SUBJ_CORE). Tests verify
/// `.ci/editor-clients/<id>.json` exists on disk, so a row can never bind a
/// fixture authority absent from the tree.
pub const SUBJECT_FIXTURE_SUBSTRATE: &[&str] = &["emacs-subjects.v1"];

/// Baseline journey classes current main governs (#11768 "required baseline
/// journey classes"), in fixed published order. Pull diagnostics get their own
/// class so protocol membership stays separable from host visibility.
pub const BASELINE_CLASSES: &[&str] = &[
    "registration_subject_selection",
    "mode_attachment",
    "workspace_readiness",
    "diagnostics_host_visibility",
    "diagnostics_pull_protocol",
    "completion_capf_buffer_state",
    "eldoc_hover_observation",
    "xref_navigation",
    "multi_file_rename_workspace_edit",
    "stale_generation_rejection",
    "configuration_behavior",
    "clean_shutdown_cleanup",
    "coordinate_discriminators",
    "wrong_competing_selection",
];

/// Optional documented-feature classes under #9413. Additive only: their
/// existence never strengthens the bounded core profile.
pub const OPTIONAL_CLASSES: &[&str] =
    &["opt_native_formatting", "opt_code_action_application", "opt_inlay_hints"];

/// False-subject control vocabulary (#11768 "Required false subjects"). Each
/// token names one independently selectable wrong-subject/wrong-state control
/// a cell's receipts must distinguish from its positive discriminator.
pub const CONTROL_VOCABULARY: &[&str] = &[
    "wrong_semantic_entity_same_spelling",
    "same_basename_wrong_root_file",
    "same_version_string_wrong_bytes",
    "cross_family_observation",
    "alternate_perl_server_expected_answer",
    "prior_generation_stale_result",
    "protocol_response_without_host_effect",
    "action_without_semantic_observation",
    "partial_multi_file_edit_or_result",
    "adjacent_unicode_utf16_or_crlf_coordinate",
    "prebound_root_as_stock_discovery",
    "local_source_subject_as_released",
];

/// Required host/document/session generation dimensions a cell's receipts
/// must bind (#11768 fixture/cell model). Unknown dimension tokens fail
/// closed; the five families below are the issue's bounded grammar.
pub const GENERATION_DIMENSIONS: &[&str] = &[
    "document.generation",
    "root.discovery_generation",
    "config.generation",
    "process.identity",
    "session.generation",
];

/// The #11366 root-fixture roles this manifest cites. #11366 remains the
/// fixture authority; this closed list only keeps a cited role from being a
/// silent typo or an unowned invention.
pub const ROOT_ROLE_TOKENS: &[&str] = &["stock_project", "stock_discovery"];

/// Coordinate applicability grammar: newline domains and Unicode position
/// discriminators a cell exercises. An adjacent-coordinate control rides on
/// [`CONTROL_VOCABULARY`].
pub const COORDINATE_DOMAINS: &[&str] = &["lf", "crlf", "unicode_non_bmp"];

/// Terminal-limitation vocabulary. Every token a cell admits must be here;
/// protocol-membership cells additionally require
/// `protocol_membership_not_host_visible` verbatim.
pub const LIMITATION_VOCABULARY: &[&str] = &[
    "capability_not_advertised",
    "client_not_exposed",
    "observation_incomplete",
    "protocol_membership_not_host_visible",
    "feature_depth_optional",
    "not_proven",
];

/// Producer-mapping namespace under #11361 (checked observation → receipt
/// mapping). A cell records which mapping may consume its receipts; the
/// mapping machinery itself lives in #11360/#11361 and is not defined here.
pub const PRODUCER_MAPPING_PREFIX: &str = "11361.";

/// The registered `#11361` observation → receipt mappings a cell may cite.
/// Namespace syntax alone is not ownership: an invented `11361.<token>` row
/// must fail closed so no cell can cite a mapping no `#11361`-owned producer
/// can consume.
pub const PRODUCER_MAPPINGS: &[&str] = &["11361.observation_to_receipt.v1"];

/// Platform applicability tokens `emacs_host_journeys.v1` admits. Exactly
/// one value is meaningful today: every registered cell applies to every
/// governed subject. A narrower scope must arrive with the authority that
/// defines the platform grammar — inventing OS triples here would duplicate
/// subject authority this manifest only consumes.
pub const PLATFORM_APPLICABILITY_TOKENS: &[&str] = &["all"];

// ---------------------------------------------------------------------------
// Model
// ---------------------------------------------------------------------------

/// Independent diagnostic cohort (#11768 "Diagnostic cohort contract"). A
/// cell's receipt may only ever be earned inside a cohort its own membership
/// admits: a push transcript cannot satisfy pull membership, and neither
/// satisfies an lsp-mode observed-path cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticCohort {
    /// Bundled Eglot subject, push diagnostics.
    BundledEglotPush,
    /// Released/source standalone Eglot subjects, pull diagnostics.
    StandaloneEglotPull,
    /// Released lsp-mode subject, observed diagnostic path.
    LspModeObserved,
}

impl DiagnosticCohort {
    /// The closed cohort contract in published order. Membership summaries
    /// and registry rows both read this list, so a new variant cannot be
    /// admitted by a row while staying invisible in the published summary.
    pub const ALL: [Self; 3] =
        [Self::BundledEglotPush, Self::StandaloneEglotPull, Self::LspModeObserved];
}

/// Required host surface / action class. Distinguishes genuinely host-visible
/// semantics from protocol-frame observation: [`HostSurface::is_host_visible`]
/// is false only for [`HostSurface::DiagnosticsPollProtocol`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostSurface {
    ClientRegistrationExactProcess,
    ModeAttachmentMajorModeLanguageId,
    WorkspaceReadinessStockRoot,
    FlymakeDiagnosticLifecycle,
    DiagnosticsPollProtocol,
    CapfCompletionBufferState,
    EldocHoverObservation,
    XrefDefinitionReferences,
    MultiFileRenameWorkspaceEdit,
    StaleResultRejection,
    ConfigurationBehaviorEffect,
    CleanShutdownProcessCleanup,
    CoordinateDiscriminators,
    WrongCompetingSelectionGuard,
    DocumentFormattingApplication,
    CodeActionApplicationRefusal,
    InlayHintRequestRefresh,
}

impl HostSurface {
    /// Whether success on this surface is a host-visible semantic effect as
    /// opposed to a protocol-frame observation. Protocol membership is load
    /// bearing for the pull cohort but can never substitute for the surfaces
    /// above it in a pass claim.
    pub fn is_host_visible(self) -> bool {
        !matches!(self, Self::DiagnosticsPollProtocol)
    }
}

/// Evidence kind a cell admits. A protocol-membership cell may ever only bind
/// membership facts; its ceiling excludes host-visible semantic passes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceKind {
    HostVisibleObservation,
    ProtocolMembershipOnly,
}

/// Claim depth class. `Optional` marks the #9413 documented-feature cells:
/// additive families whose existence can never strengthen the core profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DepthClass {
    Core,
    Optional,
}

/// Reference into the canonical expectation package. Referenced, never
/// copied: the ids resolve against `perl-agent-client-v1`'s landed membership
/// at validation time, so an unknown id fails closed and the manifest cannot
/// grow a private semantic oracle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExpectationRef {
    pub set_id: String,
    pub set_digest: String,
    pub ids: Vec<String>,
}

/// Root-fixture role reference. Only a `root_11366.<role>` role token is
/// expressible; root bytes and root subsets stay owned by #11366.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RootReference {
    pub role_token: String,
}

/// One pre-registered journey cell. Every field is load-bearing at
/// validation time; the digest covers all of them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JourneyCell {
    /// Stable id: `emacs.<class>.<name>`, `<class>` equal to `journey_class`.
    pub cell_id: String,
    pub cell_version: u32,
    /// Registered journey-class token (member of [`BASELINE_CLASSES`] or
    /// [`OPTIONAL_CLASSES`]).
    pub journey_class: String,
    /// Claim depth: core bounded profile vs #9413 optional documented depth.
    pub depth: DepthClass,
    /// Diagnostic cohorts whose membership may ever earn this cell.
    pub cohorts: Vec<DiagnosticCohort>,
    /// Subject-fixture authorities bound to this cell from the registry-wide
    /// [`SUBJECT_FIXTURE_SUBSTRATE`].
    pub fixture_owners: Vec<String>,
    /// Canonical expectation owners referenced, not copied.
    pub expectation_owner: ExpectationRef,
    /// Root-authority role reference when the cell is root-sensitive.
    pub root_reference: Option<RootReference>,
    /// Required generation dimensions ([`GENERATION_DIMENSIONS`]).
    pub dimensions: Vec<String>,
    /// Required host surfaces/action classes.
    pub host_surfaces: Vec<HostSurface>,
    /// Kind of evidence the cell admits (host-visible vs protocol-only).
    pub evidence_kind: EvidenceKind,
    /// Positive discriminator: the distinguishing success fact a receipt for
    /// this cell must establish.
    pub positive_discriminator: String,
    /// Independently selectable false-subject controls ([`CONTROL_VOCABULARY`]).
    pub false_subject_controls: Vec<String>,
    /// Coordinate applicability ([`COORDINATE_DOMAINS`]): newline domains and
    /// Unicode position discriminators exercised.
    pub coordinate_domains: Vec<String>,
    /// Platform applicability; a token from [`PLATFORM_APPLICABILITY_TOKENS`].
    pub platform_applicability: String,
    /// Allowed terminal limitations ([`LIMITATION_VOCABULARY`]).
    pub allowed_limitations: Vec<String>,
    /// Maximum evidence stage any receipt for this cell may ever claim.
    pub max_stage: EvidenceStage,
    /// Maximum claim stage: what a receipt proves and never proves.
    pub claim_ceiling: String,
    /// Producer mapping under #11361 permitted to consume this cell's
    /// receipts (`11361.<token>`).
    pub producer_mapping: String,
}

/// Validated summary of the whole compiled manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RegistrySummary {
    pub schema_version: &'static str,
    pub cell_count: usize,
    pub core_cell_count: usize,
    pub optional_cell_count: usize,
    /// Per-cohort membership counts, proving cohort independence is explicit
    /// rather than inherited.
    pub cohort_membership: BTreeMap<String, usize>,
    pub digest: String,
}

// ---------------------------------------------------------------------------
// Compiled registry
// ---------------------------------------------------------------------------

/// The ledgers/classes current main registers: baseline classes plus the
/// #9413 optional documented-feature classes.
pub fn registered_classes() -> Vec<&'static str> {
    let mut all = BASELINE_CLASSES.to_vec();
    all.extend_from_slice(OPTIONAL_CLASSES);
    all
}

/// Whether `class` is optional documented-feature depth (#9413).
pub fn is_optional_class(class: &str) -> bool {
    OPTIONAL_CLASSES.contains(&class)
}

fn expect_ref(ids: &[&str]) -> Result<ExpectationRef> {
    Ok(ExpectationRef {
        set_id: CANONICAL_EXPECTATION_SET_ID.to_string(),
        set_digest: canonical_expectation_set_digest()?,
        ids: ids.iter().map(|id| (*id).to_string()).collect(),
    })
}

fn root_ref(role: &str) -> Option<RootReference> {
    Some(RootReference { role_token: format!("root_11366.{role}") })
}

const PULL_ONLY: [DiagnosticCohort; 1] = [DiagnosticCohort::StandaloneEglotPull];

const DIM_DOCUMENT_PROCESS: [&str; 2] = ["document.generation", "process.identity"];
const DIM_DOCUMENT_SESSION: [&str; 2] = ["document.generation", "session.generation"];
const DIM_ROOT_READY: [&str; 3] =
    ["document.generation", "root.discovery_generation", "session.generation"];

const CORE_CLAIM_CEILING: &str = "registration only: binds one pre-registered governed Emacs \
                                 journey cell for a future actual-host leaf; validates \
                                 membership and controls, proves no host behavior, awards no \
                                 support profile, and consumes generic canonical expectations \
                                 without owning Perl semantic truth";
const PROTOCOL_CLAIM_CEILING: &str = "registration only: binds pull-protocol membership for \
                                      the standalone-Eglot-pull cohort; a transcript earning \
                                      this cell can never satisfy a host-visible \
                                      diagnostics cell and awards no support profile";
const OPTIONAL_CLAIM_CEILING: &str = "registration only: binds one #9413 documented-feature \
                                      depth cell, additive by construction; its existence \
                                      never strengthens the bounded core profile";

fn claim_ceiling_for(depth: DepthClass, evidence_kind: EvidenceKind) -> &'static str {
    match depth {
        DepthClass::Optional => OPTIONAL_CLAIM_CEILING,
        DepthClass::Core if evidence_kind == EvidenceKind::ProtocolMembershipOnly => {
            PROTOCOL_CLAIM_CEILING
        }
        DepthClass::Core => CORE_CLAIM_CEILING,
    }
}

/// Declaration shape for one row; [`registry`] fills the shared bindings
/// every row carries so rows stay reviewable as diffs.
struct CellSpec<'a> {
    cell_id: &'a str,
    class: &'static str,
    depth: DepthClass,
    cohorts: &'a [DiagnosticCohort],
    expectations: &'a [&'a str],
    root_role: Option<&'a str>,
    dimensions: &'a [&'a str],
    surfaces: &'a [HostSurface],
    evidence_kind: EvidenceKind,
    discriminator: &'a str,
    controls: &'a [&'a str],
    coordinates: &'a [&'a str],
    max_stage: EvidenceStage,
}

impl<'a> CellSpec<'a> {
    /// Registry-wide v1 invariants live here rather than on every row: the
    /// subject-fixture substrate, `platform_applicability`, and the `11361.*`
    /// producer mapping have exactly one admitted value in
    /// `emacs_host_journeys.v1`. They stay validated per cell because
    /// [`validate_registry`] also accepts registries this constructor did not
    /// build; the first row that needs a second value must become a `CellSpec`
    /// field rather than a second constant here.
    fn build(self) -> Result<JourneyCell> {
        let limitations: &[&str] = match self.evidence_kind {
            EvidenceKind::ProtocolMembershipOnly => &["protocol_membership_not_host_visible"],
            EvidenceKind::HostVisibleObservation => &["capability_not_advertised"],
        };
        let ceiling = claim_ceiling_for(self.depth, self.evidence_kind);
        Ok(JourneyCell {
            cell_id: self.cell_id.to_string(),
            cell_version: 1,
            journey_class: self.class.to_string(),
            depth: self.depth,
            cohorts: self.cohorts.to_vec(),
            fixture_owners: SUBJECT_FIXTURE_SUBSTRATE.iter().map(|f| (*f).to_string()).collect(),
            expectation_owner: expect_ref(self.expectations)?,
            root_reference: self.root_role.and_then(root_ref),
            dimensions: self.dimensions.iter().map(|d| (*d).to_string()).collect(),
            host_surfaces: self.surfaces.to_vec(),
            evidence_kind: self.evidence_kind,
            positive_discriminator: self.discriminator.to_string(),
            false_subject_controls: self.controls.iter().map(|c| (*c).to_string()).collect(),
            coordinate_domains: self.coordinates.iter().map(|c| (*c).to_string()).collect(),
            platform_applicability: "all".to_string(),
            allowed_limitations: limitations.iter().map(|l| (*l).to_string()).collect(),
            max_stage: self.max_stage,
            claim_ceiling: ceiling.to_string(),
            producer_mapping: "11361.observation_to_receipt.v1".to_string(),
        })
    }
}

/// The compiled registry current main governs. Rows live directly in this
/// function (one PR-level registry), ordered by class in published order;
/// every change to membership or controls is a visible digest change.
pub fn registry() -> Result<Vec<JourneyCell>> {
    use EvidenceKind::{HostVisibleObservation as HostVisible, ProtocolMembershipOnly};
    use HostSurface::*;

    Ok(vec![
        // -- registration ---------------------------------------------------
        CellSpec {
            cell_id: "emacs.registration_subject_selection.exact_selected_perllsp",
            class: "registration_subject_selection",
            depth: DepthClass::Core,
            cohorts: &DiagnosticCohort::ALL,
            expectations: &["lifecycle.shutdown"],
            root_role: None,
            dimensions: &["process.identity", "session.generation"],
            surfaces: &[ClientRegistrationExactProcess],
            evidence_kind: HostVisible,
            discriminator:
                "the host selected exactly one perllsp server process and holds its identity",
            controls: &[
                "alternate_perl_server_expected_answer",
                "same_version_string_wrong_bytes",
                "cross_family_observation",
            ],
            coordinates: &["lf"],
            max_stage: EvidenceStage::ReleaseCandidate,
        }
        .build()?,
        // -- major-mode + language-id attachment ----------------------------
        CellSpec {
            cell_id: "emacs.mode_attachment.perl_mode_language_id",
            class: "mode_attachment",
            depth: DepthClass::Core,
            cohorts: &DiagnosticCohort::ALL,
            expectations: &["hover.widget_name"],
            root_role: Some("stock_project"),
            dimensions: &DIM_DOCUMENT_PROCESS,
            surfaces: &[ModeAttachmentMajorModeLanguageId],
            evidence_kind: HostVisible,
            discriminator:
                "perl-mode attached through major-mode/language-id with eglot/lsp-mode binding",
            controls: &["wrong_semantic_entity_same_spelling", "cross_family_observation"],
            coordinates: &["lf"],
            max_stage: EvidenceStage::ReleaseCandidate,
        }
        .build()?,
        CellSpec {
            cell_id: "emacs.mode_attachment.cperl_mode_language_id",
            class: "mode_attachment",
            depth: DepthClass::Core,
            cohorts: &DiagnosticCohort::ALL,
            expectations: &["hover.widget_name"],
            root_role: Some("stock_project"),
            dimensions: &DIM_DOCUMENT_PROCESS,
            surfaces: &[ModeAttachmentMajorModeLanguageId],
            evidence_kind: HostVisible,
            discriminator:
                "cperl-mode attached through major-mode/language-id with eglot/lsp-mode binding",
            controls: &["wrong_semantic_entity_same_spelling", "cross_family_observation"],
            coordinates: &["lf"],
            max_stage: EvidenceStage::ReleaseCandidate,
        }
        .build()?,
        // -- workspace readiness --------------------------------------------
        CellSpec {
            cell_id: "emacs.workspace_readiness.stock_root_ready",
            class: "workspace_readiness",
            depth: DepthClass::Core,
            cohorts: &DiagnosticCohort::ALL,
            expectations: &["workspace.partial_not_ready"],
            root_role: Some("stock_discovery"),
            dimensions: DIM_ROOT_READY.as_slice(),
            surfaces: &[WorkspaceReadinessStockRoot],
            evidence_kind: HostVisible,
            discriminator: "the stock-discovered root reached readiness before journey traffic",
            controls: &[
                "same_basename_wrong_root_file",
                "prebound_root_as_stock_discovery",
                "partial_multi_file_edit_or_result",
            ],
            coordinates: &["lf"],
            max_stage: EvidenceStage::ReleaseCandidate,
        }
        .build()?,
        // -- host-visible diagnostics ---------------------------------------
        CellSpec {
            cell_id: "emacs.diagnostics_host_visibility.arrival_update_clear",
            class: "diagnostics_host_visibility",
            depth: DepthClass::Core,
            cohorts: &DiagnosticCohort::ALL,
            expectations: &["diagnostic.syntax"],
            root_role: Some("stock_project"),
            dimensions: &DIM_DOCUMENT_SESSION,
            surfaces: &[FlymakeDiagnosticLifecycle],
            evidence_kind: HostVisible,
            discriminator:
                "a Flymake-visible diagnostic arrived, updated, and cleared in the buffer",
            controls: &[
                "protocol_response_without_host_effect",
                "wrong_semantic_entity_same_spelling",
                "action_without_semantic_observation",
            ],
            coordinates: &["lf", "crlf"],
            max_stage: EvidenceStage::ReleaseCandidate,
        }
        .build()?,
        CellSpec {
            cell_id: "emacs.diagnostics_host_visibility.pull_flymake_state",
            class: "diagnostics_host_visibility",
            depth: DepthClass::Core,
            cohorts: &PULL_ONLY,
            expectations: &["diagnostic.syntax"],
            root_role: Some("stock_project"),
            dimensions: &DIM_DOCUMENT_SESSION,
            surfaces: &[FlymakeDiagnosticLifecycle],
            evidence_kind: HostVisible,
            discriminator:
                "the pull cohort rendered polled diagnostics into host-visible Flymake state",
            controls: &["protocol_response_without_host_effect", "cross_family_observation"],
            coordinates: &["lf"],
            max_stage: EvidenceStage::ReleaseCandidate,
        }
        .build()?,
        // -- pull-protocol membership cells (#11768 pull contract) ----------
        CellSpec {
            cell_id: "emacs.diagnostics_pull_protocol.poll_request_full_result_id",
            class: "diagnostics_pull_protocol",
            depth: DepthClass::Core,
            cohorts: &PULL_ONLY,
            expectations: &["diagnostic.syntax"],
            root_role: None,
            dimensions: &DIM_DOCUMENT_SESSION,
            surfaces: &[DiagnosticsPollProtocol],
            evidence_kind: ProtocolMembershipOnly,
            discriminator:
                "an actual textDocument/diagnostic poll returned the full result with resultId",
            controls: &["protocol_response_without_host_effect", "cross_family_observation"],
            coordinates: &["lf"],
            max_stage: EvidenceStage::ReleaseCandidate,
        }
        .build()?,
        CellSpec {
            cell_id: "emacs.diagnostics_pull_protocol.previous_result_id_roundtrip",
            class: "diagnostics_pull_protocol",
            depth: DepthClass::Core,
            cohorts: &PULL_ONLY,
            expectations: &["diagnostic.syntax"],
            root_role: None,
            dimensions: &DIM_DOCUMENT_SESSION,
            surfaces: &[DiagnosticsPollProtocol],
            evidence_kind: ProtocolMembershipOnly,
            discriminator:
                "previousResultId resolved against the exact prior pulled report identity",
            controls: &["prior_generation_stale_result", "protocol_response_without_host_effect"],
            coordinates: &["lf"],
            max_stage: EvidenceStage::ReleaseCandidate,
        }
        .build()?,
        CellSpec {
            cell_id: "emacs.diagnostics_pull_protocol.unchanged_result_reported",
            class: "diagnostics_pull_protocol",
            depth: DepthClass::Core,
            cohorts: &PULL_ONLY,
            expectations: &["diagnostic.syntax"],
            root_role: None,
            dimensions: &DIM_DOCUMENT_SESSION,
            surfaces: &[DiagnosticsPollProtocol],
            evidence_kind: ProtocolMembershipOnly,
            discriminator: "the unchanged report was identified without re-sending results",
            controls: &["prior_generation_stale_result", "protocol_response_without_host_effect"],
            coordinates: &["lf"],
            max_stage: EvidenceStage::ReleaseCandidate,
        }
        .build()?,
        CellSpec {
            cell_id: "emacs.diagnostics_pull_protocol.edit_invalidation_new_identity",
            class: "diagnostics_pull_protocol",
            depth: DepthClass::Core,
            cohorts: &PULL_ONLY,
            expectations: &["edit_requery.widget_greet", "diagnostic.syntax"],
            root_role: None,
            dimensions: &DIM_DOCUMENT_SESSION,
            surfaces: &[DiagnosticsPollProtocol],
            evidence_kind: ProtocolMembershipOnly,
            discriminator:
                "an edit invalidated the prior report and the next poll carried a new identity",
            controls: &["prior_generation_stale_result", "partial_multi_file_edit_or_result"],
            coordinates: &["lf"],
            max_stage: EvidenceStage::ReleaseCandidate,
        }
        .build()?,
        CellSpec {
            cell_id: "emacs.diagnostics_pull_protocol.final_clear",
            class: "diagnostics_pull_protocol",
            depth: DepthClass::Core,
            cohorts: &PULL_ONLY,
            expectations: &["diagnostic.syntax"],
            root_role: None,
            dimensions: &DIM_DOCUMENT_SESSION,
            surfaces: &[DiagnosticsPollProtocol],
            evidence_kind: ProtocolMembershipOnly,
            discriminator: "the final pull poll reported the empty diagnostic set",
            controls: &["protocol_response_without_host_effect", "prior_generation_stale_result"],
            coordinates: &["lf"],
            max_stage: EvidenceStage::ReleaseCandidate,
        }
        .build()?,
        // -- completion + CAPF -----------------------------------------------
        CellSpec {
            cell_id: "emacs.completion_capf_buffer_state.capf_selection_applied",
            class: "completion_capf_buffer_state",
            depth: DepthClass::Core,
            cohorts: &DiagnosticCohort::ALL,
            expectations: &["edit_requery.widget_greet"],
            root_role: Some("stock_project"),
            dimensions: &DIM_DOCUMENT_SESSION,
            surfaces: &[CapfCompletionBufferState],
            evidence_kind: HostVisible,
            discriminator:
                "CAPF item selection applied the exact expected buffer state after completion",
            controls: &[
                "wrong_semantic_entity_same_spelling",
                "action_without_semantic_observation",
                "protocol_response_without_host_effect",
            ],
            coordinates: &["lf"],
            max_stage: EvidenceStage::ReleaseCandidate,
        }
        .build()?,
        // -- ElDoc/hover ------------------------------------------------------
        CellSpec {
            cell_id: "emacs.eldoc_hover_observation.hover_rendered",
            class: "eldoc_hover_observation",
            depth: DepthClass::Core,
            cohorts: &DiagnosticCohort::ALL,
            expectations: &["hover.widget_name"],
            root_role: Some("stock_project"),
            dimensions: &DIM_DOCUMENT_SESSION,
            surfaces: &[EldocHoverObservation],
            evidence_kind: HostVisible,
            discriminator: "hover content was observed in the host ElDoc surface",
            controls: &[
                "wrong_semantic_entity_same_spelling",
                "protocol_response_without_host_effect",
            ],
            coordinates: &["lf"],
            max_stage: EvidenceStage::ReleaseCandidate,
        }
        .build()?,
        // -- Xref --------------------------------------------------------------
        CellSpec {
            cell_id: "emacs.xref_navigation.definition_and_references",
            class: "xref_navigation",
            depth: DepthClass::Core,
            cohorts: &DiagnosticCohort::ALL,
            expectations: &["definition.widget_new", "references.widget_greet"],
            root_role: Some("stock_project"),
            dimensions: &DIM_DOCUMENT_SESSION,
            surfaces: &[XrefDefinitionReferences],
            evidence_kind: HostVisible,
            discriminator:
                "Xref resolved definition and reference targets to the exact entities",
            controls: &[
                "wrong_semantic_entity_same_spelling",
                "same_basename_wrong_root_file",
            ],
            coordinates: &["lf"],
            max_stage: EvidenceStage::ReleaseCandidate,
        }
        .build()?,
        // -- multi-file rename/workspace-edit ---------------------------------
        CellSpec {
            cell_id: "emacs.multi_file_rename_workspace_edit.exact_application",
            class: "multi_file_rename_workspace_edit",
            depth: DepthClass::Core,
            cohorts: &DiagnosticCohort::ALL,
            expectations: &["rename_preview.greet"],
            root_role: Some("stock_project"),
            dimensions: &DIM_DOCUMENT_SESSION,
            surfaces: &[MultiFileRenameWorkspaceEdit],
            evidence_kind: HostVisible,
            discriminator:
                "the multi-file WorkspaceEdit applied exactly across every touched document",
            controls: &[
                "partial_multi_file_edit_or_result",
                "wrong_semantic_entity_same_spelling",
                "action_without_semantic_observation",
            ],
            coordinates: &["lf"],
            max_stage: EvidenceStage::ReleaseCandidate,
        }
        .build()?,
        // -- stale rejection -----------------------------------------------------
        CellSpec {
            cell_id: "emacs.stale_generation_rejection.prior_generation_rejected",
            class: "stale_generation_rejection",
            depth: DepthClass::Core,
            cohorts: &DiagnosticCohort::ALL,
            expectations: &["edit_requery.widget_greet"],
            root_role: None,
            dimensions: &DIM_DOCUMENT_PROCESS,
            surfaces: &[StaleResultRejection],
            evidence_kind: HostVisible,
            discriminator:
                "a result bound to a prior document or process generation was rejected",
            controls: &[
                "prior_generation_stale_result",
                "partial_multi_file_edit_or_result",
                "local_source_subject_as_released",
            ],
            coordinates: &["lf"],
            max_stage: EvidenceStage::ReleaseCandidate,
        }
        .build()?,
        // -- configuration --------------------------------------------------------
        CellSpec {
            cell_id: "emacs.configuration_behavior.config_effect_observed",
            class: "configuration_behavior",
            depth: DepthClass::Core,
            cohorts: &DiagnosticCohort::ALL,
            expectations: &["workspace.partial_not_ready"],
            root_role: None,
            dimensions: &["config.generation", "session.generation"],
            surfaces: &[ConfigurationBehaviorEffect],
            evidence_kind: HostVisible,
            discriminator: "a configuration change produced its expected host-visible effect",
            controls: &[
                "prior_generation_stale_result",
                "action_without_semantic_observation",
            ],
            coordinates: &["lf"],
            max_stage: EvidenceStage::ReleaseCandidate,
        }
        .build()?,
        // -- clean shutdown ---------------------------------------------------------
        CellSpec {
            cell_id: "emacs.clean_shutdown_cleanup.process_clean_after_exit",
            class: "clean_shutdown_cleanup",
            depth: DepthClass::Core,
            cohorts: &DiagnosticCohort::ALL,
            expectations: &["lifecycle.shutdown"],
            root_role: None,
            dimensions: &["process.identity", "session.generation"],
            surfaces: &[CleanShutdownProcessCleanup],
            evidence_kind: HostVisible,
            discriminator:
                "shutdown left no surviving perllsp process for the closed session identity",
            controls: &[
                "prior_generation_stale_result",
                "same_version_string_wrong_bytes",
            ],
            coordinates: &["lf"],
            max_stage: EvidenceStage::ReleaseCandidate,
        }
        .build()?,
        // -- coordinate discriminators ------------------------------------------------
        CellSpec {
            cell_id: "emacs.coordinate_discriminators.unicode_non_bmp_positions",
            class: "coordinate_discriminators",
            depth: DepthClass::Core,
            cohorts: &DiagnosticCohort::ALL,
            expectations: &["unicode.utf16"],
            root_role: Some("stock_project"),
            dimensions: &DIM_DOCUMENT_SESSION,
            surfaces: &[CoordinateDiscriminators],
            evidence_kind: HostVisible,
            discriminator:
                "non-BMP character positions stayed UTF-16-exact end to end through the host",
            controls: &["adjacent_unicode_utf16_or_crlf_coordinate"],
            coordinates: &["unicode_non_bmp", "lf"],
            max_stage: EvidenceStage::ReleaseCandidate,
        }
        .build()?,
        CellSpec {
            cell_id: "emacs.coordinate_discriminators.lf_crlf_newlines",
            class: "coordinate_discriminators",
            depth: DepthClass::Core,
            cohorts: &DiagnosticCohort::ALL,
            expectations: &["unicode.utf16"],
            root_role: Some("stock_project"),
            dimensions: &DIM_DOCUMENT_SESSION,
            surfaces: &[CoordinateDiscriminators],
            evidence_kind: HostVisible,
            discriminator:
                "LF and CRLF documents produced byte-exact positions through the same journey",
            controls: &["adjacent_unicode_utf16_or_crlf_coordinate"],
            coordinates: &["lf", "crlf"],
            max_stage: EvidenceStage::ReleaseCandidate,
        }
        .build()?,
        // -- wrong competing selection --------------------------------------------------
        CellSpec {
            cell_id: "emacs.wrong_competing_selection.competing_server_rejected",
            class: "wrong_competing_selection",
            depth: DepthClass::Core,
            cohorts: &DiagnosticCohort::ALL,
            expectations: &["lifecycle.shutdown"],
            root_role: None,
            dimensions: &["process.identity", "session.generation"],
            surfaces: &[WrongCompetingSelectionGuard],
            evidence_kind: HostVisible,
            discriminator:
                "a competing Perl client/server candidate did not win selection nor answer the \
                 session",
            controls: &[
                "alternate_perl_server_expected_answer",
                "same_version_string_wrong_bytes",
                "cross_family_observation",
            ],
            coordinates: &["lf"],
            max_stage: EvidenceStage::ReleaseCandidate,
        }
        .build()?,
        // -- #9413 optional documented-feature depth (additive only) --------------------
        CellSpec {
            cell_id: "emacs.opt_native_formatting.format_document_depth",
            class: "opt_native_formatting",
            depth: DepthClass::Optional,
            cohorts: &DiagnosticCohort::ALL,
            expectations: &["rename_preview.greet"],
            root_role: Some("stock_project"),
            dimensions: &DIM_DOCUMENT_SESSION,
            surfaces: &[DocumentFormattingApplication],
            evidence_kind: HostVisible,
            discriminator: "native formatting produced its documented buffer effect",
            controls: &["action_without_semantic_observation", "prior_generation_stale_result"],
            coordinates: &["lf"],
            max_stage: EvidenceStage::ReleaseCandidate,
        }
        .build()?,
        CellSpec {
            cell_id: "emacs.opt_code_action_application.apply_or_refuse_depth",
            class: "opt_code_action_application",
            depth: DepthClass::Optional,
            cohorts: &DiagnosticCohort::ALL,
            expectations: &["code_action_preview.syntax"],
            root_role: Some("stock_project"),
            dimensions: &DIM_DOCUMENT_SESSION,
            surfaces: &[CodeActionApplicationRefusal],
            evidence_kind: HostVisible,
            discriminator: "code-action application or refusal matched the advertised action",
            controls: &["action_without_semantic_observation", "prior_generation_stale_result"],
            coordinates: &["lf"],
            max_stage: EvidenceStage::ReleaseCandidate,
        }
        .build()?,
        CellSpec {
            cell_id: "emacs.opt_inlay_hints.request_render_refresh_depth",
            class: "opt_inlay_hints",
            depth: DepthClass::Optional,
            cohorts: &DiagnosticCohort::ALL,
            expectations: &["hover.widget_name"],
            root_role: Some("stock_project"),
            dimensions: &DIM_DOCUMENT_SESSION,
            surfaces: &[InlayHintRequestRefresh],
            evidence_kind: HostVisible,
            discriminator: "inlay hints requested, rendered, and refreshed as documented",
            controls: &[
                "action_without_semantic_observation",
                "wrong_semantic_entity_same_spelling",
            ],
            coordinates: &["lf"],
            max_stage: EvidenceStage::ReleaseCandidate,
        }
        .build()?,
    ])
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

/// Validate the compiled registry of current main: shared cell laws, then the
/// registry-level coverage laws (baseline classes covered by core cells only,
/// and, when optional depth is present, every optional class covered).
///
/// The advertised fail-closed command resolves the landed subject authority
/// from disk and certifies the exact governed client denominator before
/// emitting the summary: cohort counts are meaningful only when that
/// denominator is complete and current. A deleted, malformed, or
/// sparse-checkout-omitted `.ci/editor-clients/emacs-subjects.v1.json` must
/// fail here, not only in a contract test.
pub fn validate_compiled_registry() -> Result<RegistrySummary> {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .context("xtask must live below the repository root")?;
    validate_compiled_registry_against(repo_root)
}

/// [`validate_compiled_registry`] against an explicit repository root, so the
/// fail-closed subject-authority path is executable-proof without mutating the
/// real checkout.
pub fn validate_compiled_registry_against(repo_root: &Path) -> Result<RegistrySummary> {
    let manifest = crate::emacs_subject_manifest::SubjectManifest::load(repo_root)?;
    if let Err(failure) = crate::emacs_subject_fan_in::validate_subject_lane_denominator(&manifest)
    {
        bail!("emacs subject denominator does not certify the governed client set: {failure}");
    }
    for fixture in SUBJECT_FIXTURE_SUBSTRATE {
        let path = repo_root.join(".ci/editor-clients").join(format!("{fixture}.json"));
        ensure!(
            path.is_file(),
            "registry binds fixture authority {fixture} that is absent from the tree: {}",
            path.display()
        );
    }
    validate_registry(&registry()?)
}

/// Validate a whole registry: every cell against the shared laws, then the
/// registry-level laws (unique ids, baseline coverage, conditional optional
/// coverage, depth containment).
pub fn validate_registry(cells: &[JourneyCell]) -> Result<RegistrySummary> {
    ensure!(!cells.is_empty(), "a journey registry requires at least one cell");
    let classes = registered_classes();
    let mut seen_ids = BTreeSet::new();
    // Depth containment: a core-coverage cell can never sit in an optional
    // class and an optional-depth cell can never cover a baseline class.
    for cell in cells {
        let optional_row = is_optional_class(&cell.journey_class);
        ensure!(
            optional_row == (cell.depth == DepthClass::Optional),
            "cell {} mixes depth {:?} with class {}",
            cell.cell_id,
            cell.depth,
            cell.journey_class
        );
    }
    // Baseline coverage: every baseline class owns at least one core cell,
    // and none arrives from an optional class.
    for class in BASELINE_CLASSES {
        let covered =
            cells.iter().any(|cell| cell.journey_class == *class && cell.depth == DepthClass::Core);
        ensure!(covered, "baseline class {class} is not covered by any core cell");
    }
    if cells.iter().any(|cell| cell.depth == DepthClass::Optional) {
        for class in OPTIONAL_CLASSES {
            let covered = cells
                .iter()
                .any(|cell| cell.journey_class == *class && cell.depth == DepthClass::Optional);
            ensure!(covered, "optional class {class} is not covered by any optional-depth cell");
        }
    }

    for cell in cells {
        validate_cell(cell, &classes)?;
        ensure!(seen_ids.insert(cell.cell_id.as_str()), "duplicate cell id: {}", cell.cell_id);
    }

    // Cohort independence made explicit: publish per-cohort membership counts
    // so a cohort can only ever earn what its own cells admit.
    let mut cohort_membership = BTreeMap::new();
    for cohort in DiagnosticCohort::ALL {
        let count = cells.iter().filter(|cell| cell.cohorts.contains(&cohort)).count();
        let spelling = wire(&cohort)?;
        cohort_membership.insert(spelling, count);
    }

    let core_cell_count = cells.iter().filter(|cell| cell.depth == DepthClass::Core).count();
    let optional_cell_count = cells.len() - core_cell_count;
    Ok(RegistrySummary {
        schema_version: MANIFEST_SCHEMA_VERSION,
        cell_count: cells.len(),
        core_cell_count,
        optional_cell_count,
        cohort_membership,
        digest: registry_digest(cells)?,
    })
}

/// Shared laws for one cell registration.
fn validate_cell(cell: &JourneyCell, classes: &[&str]) -> Result<()> {
    // ID shape and class binding.
    let Some(rest) = cell.cell_id.strip_prefix(CELL_ID_PREFIX) else {
        bail!("cell id {} is outside the {CELL_ID_PREFIX} namespace", cell.cell_id);
    };
    let segments: Vec<&str> = rest.split('.').collect();
    ensure!(
        segments.len() == 2 && segments.iter().all(|s| is_reason_token(s)),
        "cell id {} must be {CELL_ID_PREFIX}<class>.<name> with stable tokens",
        cell.cell_id
    );
    ensure!(
        classes.contains(&segments[0]),
        "cell {} cites unregistered journey class {}",
        cell.cell_id,
        segments[0]
    );
    ensure!(
        cell.journey_class == segments[0],
        "cell {} declares class {} but its id carries {}",
        cell.cell_id,
        cell.journey_class,
        segments[0]
    );
    ensure!(cell.cell_version >= 1, "cell {} must carry a positive version", cell.cell_id);

    // Cohorts: non-empty, unique. Pull-protocol surfaces are exclusive to the
    // standalone-Eglot-pull cohort: no push or lsp-mode cohort can inherit a
    // pull cell, so a push transcript can never satisfy pull membership.
    ensure!(!cell.cohorts.is_empty(), "cell {} must admit at least one cohort", cell.cell_id);
    let mut cohorts = BTreeSet::new();
    for cohort in &cell.cohorts {
        let spelling = wire(cohort)?;
        ensure!(cohorts.insert(*cohort), "duplicate cohort {spelling} in cell {}", cell.cell_id);
    }
    let pull_surface = cell.host_surfaces.contains(&HostSurface::DiagnosticsPollProtocol);
    ensure!(
        !(pull_surface && cohorts != BTreeSet::from([DiagnosticCohort::StandaloneEglotPull])),
        "cell {} exposes a pull-protocol surface outside the standalone_eglot_pull cohort",
        cell.cell_id
    );

    // Evidence-kind honesty: a protocol-membership cell can never promise
    // host-visible semantics; a host-visible cell requires at least one
    // host-visible surface.
    match cell.evidence_kind {
        EvidenceKind::ProtocolMembershipOnly => {
            ensure!(
                pull_surface,
                "protocol-membership cell {} must expose DiagnosticsPollProtocol",
                cell.cell_id
            );
            ensure!(
                cell.host_surfaces.iter().all(|surface| !surface.is_host_visible()),
                "protocol-membership cell {} must not expose host-visible surfaces",
                cell.cell_id
            );
            ensure!(
                cell.allowed_limitations
                    .iter()
                    .any(|l| l == "protocol_membership_not_host_visible"),
                "protocol-membership cell {} must admit the \
                 protocol_membership_not_host_visible limitation",
                cell.cell_id
            );
        }
        EvidenceKind::HostVisibleObservation => {
            ensure!(
                cell.host_surfaces.iter().any(|surface| surface.is_host_visible()),
                "host-visible cell {} requires at least one host-visible surface",
                cell.cell_id
            );
            ensure!(
                !cell
                    .allowed_limitations
                    .iter()
                    .any(|l| l == "protocol_membership_not_host_visible"),
                "host-visible cell {} must not admit the protocol-membership limitation",
                cell.cell_id
            );
        }
    }
    ensure!(
        !cell.host_surfaces.is_empty(),
        "cell {} must require at least one host surface",
        cell.cell_id
    );
    let mut surfaces = BTreeSet::new();
    for surface in &cell.host_surfaces {
        ensure!(
            surfaces.insert(*surface),
            "duplicate host surface {surface:?} in cell {}",
            cell.cell_id
        );
    }

    // Fixtures: within the landed subject substrate.
    ensure!(
        !cell.fixture_owners.is_empty(),
        "cell {} must bind at least one fixture authority",
        cell.cell_id
    );
    let mut fixtures = BTreeSet::new();
    for fixture in &cell.fixture_owners {
        ensure!(
            fixtures.insert(fixture.as_str()),
            "duplicate fixture owner {fixture} in cell {}",
            cell.cell_id
        );
        ensure!(
            SUBJECT_FIXTURE_SUBSTRATE.contains(&fixture.as_str()),
            "cell {} binds fixture authority {fixture} outside the declared substrate",
            cell.cell_id
        );
    }

    // Expectations: referenced canonical truth, never copied. Unknown ids or
    // sets fail closed so missing canonical truth blocks the cell instead of
    // manufacturing local truth.
    ensure!(
        cell.expectation_owner.set_id == CANONICAL_EXPECTATION_SET_ID,
        "cell {} must reference canonical expectation set {CANONICAL_EXPECTATION_SET_ID}, found {}",
        cell.cell_id,
        cell.expectation_owner.set_id
    );
    ensure!(
        cell.expectation_owner.set_digest == canonical_expectation_set_digest()?,
        "cell {} binds a stale or foreign canonical expectation-set digest {}",
        cell.cell_id,
        cell.expectation_owner.set_digest
    );
    ensure!(
        !cell.expectation_owner.ids.is_empty(),
        "cell {} must reference at least one canonical expectation id",
        cell.cell_id
    );
    let mut expectations = BTreeSet::new();
    for expectation in &cell.expectation_owner.ids {
        ensure!(
            expectations.insert(expectation.as_str()),
            "duplicate expectation id {expectation} in cell {}",
            cell.cell_id
        );
        ensure!(
            CANONICAL_EXPECTATION_IDS.contains(&expectation.as_str()),
            "cell {} references unknown canonical expectation id {expectation}; missing \
             canonical truth blocks the cell",
            cell.cell_id
        );
    }

    // Root sensitivity: reference-only tokens, never material duplication.
    // A cell whose receipts require a discovery-generation distinction must
    // bind the `root_11366.<role>` it claims, so dropping the reference can
    // never silently un-govern a root-sensitive cell.
    ensure!(
        !cell.dimensions.iter().any(|d| d == "root.discovery_generation")
            || cell.root_reference.is_some(),
        "cell {} requires root.discovery_generation but binds no root_11366 role reference",
        cell.cell_id
    );
    if let Some(root) = &cell.root_reference {
        let Some(role) = root.role_token.strip_prefix("root_11366.") else {
            bail!(
                "cell {} root reference must be a root_11366.<role> role token owned by #11366",
                cell.cell_id
            );
        };
        ensure!(
            !role.contains('/') && !role.contains('\\') && is_reason_token(role),
            "root role token must be a stable reason token with no path structure: {role}"
        );
        ensure!(
            ROOT_ROLE_TOKENS.contains(&role),
            "cell {} cites unregistered root_11366 role {role}",
            cell.cell_id
        );
    }

    // Dimensions, controls, coordinates: bounded vocabularies, non-empty.
    ensure!(
        !cell.dimensions.is_empty(),
        "cell {} must require at least one generation dimension",
        cell.cell_id
    );
    let mut dimensions = BTreeSet::new();
    for dimension in &cell.dimensions {
        ensure!(
            dimensions.insert(dimension.as_str()),
            "duplicate dimension {dimension} in cell {}",
            cell.cell_id
        );
        ensure!(
            GENERATION_DIMENSIONS.contains(&dimension.as_str()),
            "cell {} requires unknown generation dimension {dimension}",
            cell.cell_id
        );
    }

    ensure!(
        !cell.false_subject_controls.is_empty(),
        "cell {} must register at least one false-subject control",
        cell.cell_id
    );
    let mut controls = BTreeSet::new();
    for control in &cell.false_subject_controls {
        ensure!(
            controls.insert(control.as_str()),
            "duplicate control {control} in cell {}",
            cell.cell_id
        );
        ensure!(
            CONTROL_VOCABULARY.contains(&control.as_str()),
            "cell {} registers unknown false-subject control {control}",
            cell.cell_id
        );
    }

    ensure!(
        !cell.coordinate_domains.is_empty(),
        "cell {} must declare coordinate applicability",
        cell.cell_id
    );
    let mut coordinates = BTreeSet::new();
    for coordinate in &cell.coordinate_domains {
        ensure!(
            coordinates.insert(coordinate.as_str()),
            "duplicate coordinate domain {coordinate} in cell {}",
            cell.cell_id
        );
        ensure!(
            COORDINATE_DOMAINS.contains(&coordinate.as_str()),
            "cell {} declares unknown coordinate domain {coordinate}",
            cell.cell_id
        );
    }

    ensure!(
        !cell.allowed_limitations.is_empty(),
        "cell {} must admit at least one terminal limitation",
        cell.cell_id
    );
    let mut limitations = BTreeSet::new();
    for limitation in &cell.allowed_limitations {
        ensure!(
            limitations.insert(limitation.as_str()),
            "duplicate limitation {limitation} in cell {}",
            cell.cell_id
        );
        ensure!(
            LIMITATION_VOCABULARY.contains(&limitation.as_str()),
            "cell {} admits unknown limitation {limitation}",
            cell.cell_id
        );
    }

    ensure!(
        PLATFORM_APPLICABILITY_TOKENS.contains(&cell.platform_applicability.as_str()),
        "cell {} declares unregistered platform applicability {}",
        cell.cell_id,
        cell.platform_applicability
    );

    // Discriminator, ceiling, producer mapping, stage.
    ensure!(
        !cell.positive_discriminator.trim().is_empty(),
        "cell {} must record a positive discriminator",
        cell.cell_id
    );
    ensure!(
        cell.claim_ceiling == claim_ceiling_for(cell.depth, cell.evidence_kind),
        "cell {} claim ceiling is not the registered ceiling for its depth/evidence kind",
        cell.cell_id
    );
    ensure!(
        !matches!(cell.max_stage, EvidenceStage::PublicArtifact),
        "cell {} may not claim public-artifact evidence in {MANIFEST_SCHEMA_VERSION}",
        cell.cell_id
    );
    let Some(producer) = cell.producer_mapping.strip_prefix(PRODUCER_MAPPING_PREFIX) else {
        bail!(
            "cell {} producer mapping {} is outside the {PRODUCER_MAPPING_PREFIX}#11361 namespace",
            cell.cell_id,
            cell.producer_mapping
        );
    };
    ensure!(
        is_reason_token(producer),
        "producer mapping token must be a stable reason token: {producer}"
    );
    ensure!(
        PRODUCER_MAPPINGS.contains(&cell.producer_mapping.as_str()),
        "cell {} cites unregistered #11361 producer mapping {}",
        cell.cell_id,
        cell.producer_mapping
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Lookup and digests
// ---------------------------------------------------------------------------

/// Resolve one subject (a stable cell id or a journey-class token) to its
/// registry rows. Returns `(matched_class_or_none, cells)`.
pub fn lookup<'a>(
    cells: &'a [JourneyCell],
    subject: &str,
) -> Result<(Option<String>, Vec<&'a JourneyCell>)> {
    if subject == "summary" {
        return Ok((None, cells.iter().collect()));
    }
    if let Some(cell) = cells.iter().find(|cell| cell.cell_id == subject) {
        return Ok((Some(cell.journey_class.clone()), vec![cell]));
    }
    if registered_classes().contains(&subject) {
        let matching: Vec<&JourneyCell> =
            cells.iter().filter(|cell| cell.journey_class == subject).collect();
        ensure!(!matching.is_empty(), "journey class {subject} is registered but owns no cells");
        return Ok((Some(subject.to_string()), matching));
    }
    bail!("no journey class or cell matches {subject:?}")
}

/// Stable digest of one cell's full binding. Order-insensitive over the list
/// fields; sensitive to every identity, version, binding, and boundary field.
pub fn cell_digest(cell: &JourneyCell) -> Result<String> {
    let view = CellDigestView {
        cell_id: cell.cell_id.clone(),
        cell_version: cell.cell_version,
        journey_class: cell.journey_class.clone(),
        depth: wire(&cell.depth)?,
        cohorts: sorted_wire(&cell.cohorts)?,
        fixture_owners: sorted(cell.fixture_owners.clone()),
        expectation_set: cell.expectation_owner.set_id.clone(),
        expectation_set_digest: cell.expectation_owner.set_digest.clone(),
        expectation_ids: sorted(cell.expectation_owner.ids.clone()),
        root_reference: cell.root_reference.as_ref().map(|r| r.role_token.clone()),
        dimensions: sorted(cell.dimensions.clone()),
        host_surfaces: sorted_wire(&cell.host_surfaces)?,
        evidence_kind: wire(&cell.evidence_kind)?,
        positive_discriminator: cell.positive_discriminator.clone(),
        false_subject_controls: sorted(cell.false_subject_controls.clone()),
        coordinate_domains: sorted(cell.coordinate_domains.clone()),
        platform_applicability: cell.platform_applicability.clone(),
        allowed_limitations: sorted(cell.allowed_limitations.clone()),
        max_stage: wire(&cell.max_stage)?,
        claim_ceiling: cell.claim_ceiling.clone(),
        producer_mapping: cell.producer_mapping.clone(),
    };
    let canonical = serde_json::to_string(&view)
        .with_context(|| format!("serializing cell binding for digest: {}", cell.cell_id))?;
    digest_of(canonical.as_bytes())
}

/// Deterministic registry digest: identity-independent of row order, sensitive
/// to any binding change.
pub fn registry_digest(cells: &[JourneyCell]) -> Result<String> {
    let mut digests = Vec::new();
    for cell in cells {
        digests.push(cell_digest(cell)?);
    }
    digests.sort_unstable();
    let canonical = serde_json::to_string(&(MANIFEST_SCHEMA_VERSION, digests))
        .context("serializing registry digest")?;
    digest_of(canonical.as_bytes())
}

fn digest_of(bytes: &[u8]) -> Result<String> {
    // Same spelling rule as `client_compat_fixture::digest_identity`: the
    // byte-wise hex walk keeps the identity stable across sha2 versions.
    let mut identity = String::with_capacity("sha256:".len() + 64);
    identity.push_str("sha256:");
    for byte in Sha256::digest(bytes) {
        write!(&mut identity, "{byte:02x}")?;
    }
    Ok(identity)
}

fn sorted(mut values: Vec<String>) -> Vec<String> {
    values.sort_unstable();
    values
}

fn sorted_wire<T: Serialize>(values: &[T]) -> Result<Vec<String>> {
    let mut wired = Vec::new();
    for value in values {
        wired.push(wire(value)?);
    }
    wired.sort_unstable();
    Ok(wired)
}

/// The wire spelling of an enum, taken from its own serialization so a digest
/// can never drift from what the contract actually writes.
fn wire(value: &impl Serialize) -> Result<String> {
    match serde_json::to_value(value)? {
        serde_json::Value::String(text) => Ok(text),
        other => bail!("expected a string wire spelling, found {other}"),
    }
}

fn is_reason_token(token: &str) -> bool {
    crate::client_compat_fixture::is_reason_token(token)
}

/// Canonical JSON projection of one cell for identity purposes.
#[derive(Serialize)]
struct CellDigestView {
    cell_id: String,
    cell_version: u32,
    journey_class: String,
    depth: String,
    cohorts: Vec<String>,
    fixture_owners: Vec<String>,
    expectation_set: String,
    expectation_set_digest: String,
    expectation_ids: Vec<String>,
    root_reference: Option<String>,
    dimensions: Vec<String>,
    host_surfaces: Vec<String>,
    evidence_kind: String,
    positive_discriminator: String,
    false_subject_controls: Vec<String>,
    coordinate_domains: Vec<String>,
    platform_applicability: String,
    allowed_limitations: Vec<String>,
    max_stage: String,
    claim_ceiling: String,
    producer_mapping: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_spellings_are_snake_case() -> Result<()> {
        ensure!(wire(&DepthClass::Optional)? == "optional", "depth wire spelling drifted");
        ensure!(
            wire(&EvidenceKind::ProtocolMembershipOnly)? == "protocol_membership_only",
            "evidence-kind wire spelling drifted"
        );
        ensure!(
            wire(&HostSurface::FlymakeDiagnosticLifecycle)? == "flymake_diagnostic_lifecycle",
            "host-surface wire spelling drifted"
        );
        Ok(())
    }

    #[test]
    fn compiled_registry_digest_is_order_insensitive() -> Result<()> {
        let forward = registry_digest(&registry()?)?;
        let mut reversed = registry()?;
        reversed.reverse();
        ensure!(forward == registry_digest(&reversed)?, "registry digest depends on row order");
        Ok(())
    }

    #[test]
    fn cohort_summary_publishes_every_registered_cohort() -> Result<()> {
        let summary = validate_registry(&registry()?)?;
        ensure!(
            summary.cohort_membership.len() == DiagnosticCohort::ALL.len(),
            "cohort summary omitted a registered cohort"
        );
        for cohort in DiagnosticCohort::ALL {
            let spelling = wire(&cohort)?;
            ensure!(
                summary.cohort_membership.contains_key(&spelling),
                "cohort summary omitted {spelling}"
            );
        }
        for (index, cohort) in DiagnosticCohort::ALL.iter().enumerate() {
            let spelling = wire(cohort)?;
            for other in DiagnosticCohort::ALL.iter().skip(index + 1) {
                ensure!(
                    spelling != wire(other)?,
                    "cohort wire spellings are not pairwise distinct"
                );
            }
        }
        Ok(())
    }
}

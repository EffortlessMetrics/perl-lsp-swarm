//! Typed, bounded observation model for the native Neovim built-in-LSP
//! action contract (#11409).
//!
//! Every observation retains, per the issue: the action/scenario/fixture/cell
//! IDs; the exact host/client/server/config/root/document subject; the ordered
//! action identity; an expected-result reference kept structurally separate
//! from the observed result/effect identity; the currentness/generation
//! snapshot where available; the result token
//! (`observed | mismatch | unsupported | not_proven | instrument_failed`);
//! bounded evidence references; and a limitation/failure class.
//!
//! Boundedness/privacy law: every string field is a stable reason token, a
//! `sha256:`-prefixed digest, a fixture-relative path, or a grammar-checked
//! API/surface spelling; unknown fields are rejected outright
//! (`deny_unknown_fields`), so private/unbounded paths, logs, or source cannot
//! ride along inside durable evidence.
//!
//! The model is receipt-agnostic on purpose: it validates and classifies, it
//! never writes a receipt, registers a journey cell, or decides support.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use super::predicate::{GenerationSnapshot, PredicateEvidence};
use crate::client_compat_fixture::is_reason_token;

/// Identity of the backend that produced the observation. The fake backend is
/// a first-class test instrument, never confused with a real host adapter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "backend", rename_all = "snake_case", deny_unknown_fields)]
pub enum BackendIdentity {
    Fake,
    HostAdapter { adapter_digest: String },
}

/// The observation plane. Product, instrument, reporting, and cleanup
/// observations stay separate for downstream composition; the core registry
/// emits product/instrument/cleanup planes only, and the `reporting` plane is
/// reserved for the generic reporting/receipt owners (never this contract).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservationPlane {
    Product,
    Instrument,
    Reporting,
    Cleanup,
}

/// The #11409 observation result vocabulary, verbatim.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservationResult {
    Observed,
    Mismatch,
    Unsupported,
    NotProven,
    InstrumentFailed,
}

impl ObservationResult {
    /// True when this result requires an admitted limitation/failure class to
    /// be honest.
    pub fn requires_limitation(self) -> bool {
        !matches!(self, ObservationResult::Observed)
    }
}

/// What actually executed to produce the observation. Each variant maps to
/// one public-API-boundary classification of the #11409 contract; a route
/// variant is representable precisely so mismatches can be detected rather
/// than silently coerced.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "route", rename_all = "snake_case", deny_unknown_fields)]
pub enum ObservedRoute {
    /// A public stable Neovim API the action declared.
    PublicStableApi { api: String },
    /// A public but version-scoped Neovim API the action declared, with the
    /// exact scope the run bound.
    VersionScopedApi { api: String, scope: String },
    /// A native editor surface (Ex command, autocmd event, user keys) the
    /// action declared.
    NativeEditorSurface { surface: String },
    /// A checked companion protocol control. Lawful only for companion-class
    /// actions; relabeling one as ordinary Neovim traffic is a falsifier.
    CompanionControl { control: String },
    /// An instrument-only hook with its exact owner.
    InstrumentHook { hook: String, owner: String },
    /// A deliberate test stimulus that must never be labeled product
    /// behavior.
    TestStimulus { stimulus: String },
    /// A bounded handoff to the #10894 shared host-execution authority
    /// (process spawn/deadline/cleanup stay there, never here).
    HostHandoff { handoff: String },
}

/// Where the observed effect reached. Requested, returned, applied, and
/// visible/current stay distinct: a server response is `Returned`, an edit
/// the editor applied is `Applied`, and state current at the claimed
/// generation is `VisibleCurrent`. Ordered — a stage can only be claimed
/// after the ones before it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectStage {
    Requested,
    Returned,
    Applied,
    VisibleCurrent,
}

/// What kind of state/effect the observation speaks about. The closed
/// vocabulary keeps effect claims checkable against each action's declared
/// emissions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectClass {
    HostSessionState,
    ConfigState,
    BufferState,
    FileState,
    Filetype,
    ClientIdentity,
    InitializeIdentity,
    ForeignClientSet,
    DiagnosticState,
    CompletionItems,
    CompletionApplied,
    HoverContent,
    NavigationTarget,
    OptionalCellResult,
    SettingEffect,
    CursorState,
    SelectionState,
    DidChangeTraffic,
    CompanionResult,
    RecoveryState,
    HeldWorkDisposition,
    RootChange,
    TerminalState,
    HandoffState,
}

/// Where an expected result came from. `ObservedOutput` is representable
/// precisely so a self-derived expectation (expected value captured from the
/// production observation itself) can be detected and rejected: expectations
/// are independent, fixture-owned facts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ExpectationSource {
    FixtureAuthority,
    ObservedOutput,
}

/// The expected-result reference an observation retains. Kept structurally
/// separate from [`ObservedEffect`]: the expectation is an independent input
/// owned by the #10903 fixture/expectation manifest, never a value read back
/// from the observation it is compared against.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectationBinding {
    pub source: ExpectationSource,
    /// Stable token naming the expectation row (authority #10903).
    pub expectation_id: String,
    /// `sha256:` digest over the bounded expected value.
    pub expectation_digest: String,
}

/// Bounded artifact evidence reference kinds (the generic receipt artifact
/// vocabulary; references are digests or fixture-relative paths, never raw
/// logs).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceKind {
    ClientLog,
    ServerStderr,
    DriverOutput,
    CapabilitySnapshot,
    ProcessLedger,
    FailureDiagnostics,
}

/// One bounded evidence reference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceRef {
    pub kind: EvidenceKind,
    /// `sha256:` digest or fixture-relative path of the retained artifact.
    pub reference: String,
}

/// A resolved content anchor position. Anchors are named in the fixture
/// authority and resolved to bounded zero-based positions before use; an
/// unresolved anchor never reaches an observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnchorPosition {
    pub line: u32,
    pub character: u32,
}

/// The exact host/client/server/config/root/document subject binding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubjectBinding {
    /// Pinned host product (`neovim`).
    pub host_product: String,
    /// Stable token naming the pinned host build; exact Neovim bytes are
    /// owned by #11406 and cited here by identity token, never re-pinned.
    pub host_version_scope: String,
    /// The built-in LSP client identity (`vim.lsp`); a Coc or other-client
    /// observation is rejected.
    pub client_id: String,
    /// The server executable (`perllsp`).
    pub server_executable: String,
    /// Stable token naming the exact canonical config subject (#10502/#7768).
    pub config_id: String,
    /// Stable token naming the workspace-root subject.
    pub root_id: String,
    /// The document binding: fixture-relative path plus buffer number.
    pub document: DocumentBinding,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DocumentBinding {
    /// Fixture-root-relative path (bounded by the fixture-path law).
    pub fixture_path: String,
    /// Editor buffer number the document is bound to.
    pub buffer: u32,
}

/// The observed result/effect identity. Distinct from the expectation: this
/// channel only carries what was actually requested/returned/applied/seen,
/// at the generation it was computed against.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservedEffect {
    pub stage: EffectStage,
    /// Effect classes this observation reports; must be within the action's
    /// declared emissions.
    pub effect_classes: Vec<EffectClass>,
    /// `sha256:` digest over the bounded observed result identity.
    pub result_digest: String,
    /// `sha256:` digest over the applied effect (buffer/file bytes) where the
    /// stage reached `Applied` or beyond.
    pub effect_digest: Option<String>,
    /// Resolved anchor positions keyed by the anchor token (bounded).
    pub anchor_positions: BTreeMap<String, AnchorPosition>,
    /// Identity digests keyed by stable token (capabilities, process,
    /// filetype, root identities, …); values are bounded digests.
    pub identity_digests: BTreeMap<String, String>,
    /// Request/invocation/cardinality counters keyed by stable token.
    pub cardinalities: BTreeMap<String, u64>,
    /// The currentness snapshot the effect was computed or applied against.
    pub generations: GenerationSnapshot,
}

/// The typed, bounded observation one native Neovim action emits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TypedObservation {
    pub schema_version: String,
    /// Ordered action identity: strictly increasing within one run.
    pub sequence: u64,
    pub action_id: String,
    /// BDD scenario reference (authority #10888); retained, never owned.
    pub scenario_id: Option<String>,
    /// Fixture manifest reference (authority #10903); retained, never owned.
    pub fixture_id: Option<String>,
    /// Journey-cell reference; retained, never owned.
    pub cell_id: Option<String>,
    pub plane: ObservationPlane,
    pub backend: BackendIdentity,
    pub subject: SubjectBinding,
    pub route: ObservedRoute,
    pub predicate_evidence: Vec<PredicateEvidence>,
    pub expectation: Option<ExpectationBinding>,
    pub observed: ObservedEffect,
    /// The currentness snapshot the observation settled against.
    pub generations: GenerationSnapshot,
    pub result: ObservationResult,
    /// Limitation/failure class; required for every result except `observed`.
    pub limitation_class: Option<String>,
    pub evidence: Vec<EvidenceRef>,
}

/// `sha256:`-prefixed bounded digest spelling (same rule as the cell
/// catalog).
pub fn is_bounded_digest(value: &str) -> bool {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return false;
    };
    hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// Fixture-root-relative path law (same rule as the specialized Vim driver):
/// no scheme, no drive letter, no home marker, no parent traversal, no
/// environment interpolation — just governed fixture tree positions.
pub fn is_fixture_relative_path(value: &str) -> bool {
    if value.is_empty() || value.len() > 200 {
        return false;
    }
    if value.starts_with('~')
        || value.starts_with('/')
        || value.starts_with('\\')
        || value.contains(':')
        || value.contains("..")
        || value.contains('$')
    {
        return false;
    }
    value.split(['/', '\\']).all(|segment| {
        !segment.is_empty()
            && segment
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-'))
            && segment.as_bytes().first().is_some_and(u8::is_ascii_alphanumeric)
    })
}

/// A reference is bounded when it is a digest or a fixture-relative path.
pub fn is_bounded_reference(value: &str) -> bool {
    is_bounded_digest(value) || is_fixture_relative_path(value)
}

/// Shared byte cap for every free-token field. A stable reason token alone
/// has no length bound, so durable evidence could otherwise carry
/// megabyte-scale "tokens"; one cap keeps the model honestly bounded.
pub const MAX_TOKEN_BYTES: usize = 200;

/// Per-collection caps: a host-produced record can neither pad collections
/// arbitrarily nor duplicate entries to inflate durable evidence.
pub const MAX_PREDICATE_EVIDENCE: usize = 16;
pub const MAX_EFFECT_CLASSES: usize = 8;
pub const MAX_EVIDENCE_REFS: usize = 16;
pub const MAX_ANCHOR_POSITIONS: usize = 16;
pub const MAX_IDENTITY_DIGESTS: usize = 16;
pub const MAX_CARDINALITIES: usize = 16;

/// True when the value is a stable reason token inside the shared byte cap.
pub fn is_bounded_token(value: &str) -> bool {
    is_reason_token(value) && value.len() <= MAX_TOKEN_BYTES
}

/// Structural boundedness validation shared by every field of the model.
/// Grammar-checked spellings (APIs, native surfaces) are completed by
/// [`super::is_neovim_api_spelling`]/[`super::is_native_editor_surface`] at
/// the contract layer; here every free-token field must already be a bounded
/// stable token and every collection must stay inside its cap without
/// duplicates.
pub fn validate_bounded(observation: &TypedObservation) -> Result<(), String> {
    if !is_bounded_token(&observation.action_id) {
        return Err(format!("action id is not a bounded stable token: {}", observation.action_id));
    }
    for (label, token) in [
        ("scenario", observation.scenario_id.as_deref()),
        ("fixture", observation.fixture_id.as_deref()),
        ("cell", observation.cell_id.as_deref()),
    ] {
        if let Some(token) = token
            && !is_bounded_token(token)
        {
            return Err(format!("{label} reference is not a bounded stable token: {token}"));
        }
    }
    let subject = &observation.subject;
    for (label, token) in [
        ("host product", subject.host_product.as_str()),
        ("host version scope", subject.host_version_scope.as_str()),
        ("client id", subject.client_id.as_str()),
        ("server executable", subject.server_executable.as_str()),
        ("config id", subject.config_id.as_str()),
        ("root id", subject.root_id.as_str()),
    ] {
        if !is_bounded_token(token) {
            return Err(format!("subject {label} is not a bounded stable token: {token}"));
        }
    }
    if !is_fixture_relative_path(&subject.document.fixture_path) {
        return Err(format!(
            "document path is not fixture-root-relative: {}",
            subject.document.fixture_path
        ));
    }
    if let BackendIdentity::HostAdapter { adapter_digest } = &observation.backend
        && !is_bounded_digest(adapter_digest)
    {
        return Err("host adapter backend must bind its adapter digest".to_string());
    }
    match &observation.route {
        ObservedRoute::PublicStableApi { api } | ObservedRoute::VersionScopedApi { api, .. } => {
            if !super::is_neovim_api_spelling(api) {
                return Err(format!("route api spelling is outside the grammar: {api}"));
            }
            if let ObservedRoute::VersionScopedApi { scope, .. } = &observation.route
                && !is_bounded_token(scope)
            {
                return Err(format!("version scope is not a bounded stable token: {scope}"));
            }
        }
        ObservedRoute::NativeEditorSurface { surface } => {
            if !super::is_native_editor_surface(surface) {
                return Err(format!("native surface spelling is outside the grammar: {surface}"));
            }
        }
        ObservedRoute::CompanionControl { control } => {
            if !is_bounded_token(control) {
                return Err(format!("companion control token is not bounded: {control}"));
            }
        }
        ObservedRoute::InstrumentHook { hook, owner } => {
            if !super::is_neovim_api_spelling(hook) {
                return Err(format!("instrument hook spelling is outside the grammar: {hook}"));
            }
            if !is_bounded_token(owner) {
                return Err(format!("instrument hook owner is not bounded: {owner}"));
            }
        }
        ObservedRoute::TestStimulus { stimulus } => {
            if !is_bounded_token(stimulus) {
                return Err(format!("test stimulus token is not bounded: {stimulus}"));
            }
        }
        ObservedRoute::HostHandoff { handoff } => {
            if !is_bounded_token(handoff) {
                return Err(format!("host handoff token is not bounded: {handoff}"));
            }
        }
    }
    if observation.predicate_evidence.len() > MAX_PREDICATE_EVIDENCE {
        return Err(format!(
            "observation carries {} predicate-evidence records; the cap is {MAX_PREDICATE_EVIDENCE}",
            observation.predicate_evidence.len()
        ));
    }
    if let Some(expectation) = &observation.expectation {
        if !is_bounded_token(&expectation.expectation_id) {
            return Err(format!(
                "expectation id is not a bounded stable token: {}",
                expectation.expectation_id
            ));
        }
        if !is_bounded_digest(&expectation.expectation_digest) {
            return Err("expectation digest is not a bounded digest".to_string());
        }
    }
    let effect = &observation.observed;
    if !is_bounded_digest(&effect.result_digest) {
        return Err("observed result digest is not bounded".to_string());
    }
    if let Some(digest) = &effect.effect_digest
        && !is_bounded_digest(digest)
    {
        return Err("observed effect digest is not bounded".to_string());
    }
    if effect.effect_classes.len() > MAX_EFFECT_CLASSES {
        return Err(format!(
            "effect carries {} effect classes; the cap is {MAX_EFFECT_CLASSES}",
            effect.effect_classes.len()
        ));
    }
    let mut seen_classes = std::collections::BTreeSet::new();
    for class in &effect.effect_classes {
        if !seen_classes.insert(class) {
            return Err(format!("duplicate effect class {class:?}"));
        }
    }
    if effect.anchor_positions.len() > MAX_ANCHOR_POSITIONS {
        return Err(format!(
            "effect binds {} anchor positions; the cap is {MAX_ANCHOR_POSITIONS}",
            effect.anchor_positions.len()
        ));
    }
    for anchor in effect.anchor_positions.keys() {
        if !is_bounded_token(anchor) {
            return Err(format!("anchor token is not bounded: {anchor}"));
        }
    }
    if effect.identity_digests.len() > MAX_IDENTITY_DIGESTS {
        return Err(format!(
            "effect binds {} identity digests; the cap is {MAX_IDENTITY_DIGESTS}",
            effect.identity_digests.len()
        ));
    }
    for (key, value) in &effect.identity_digests {
        if !is_bounded_token(key) {
            return Err(format!("identity digest key is not bounded: {key}"));
        }
        if !is_bounded_digest(value) {
            return Err(format!("identity digest {key} is not a bounded digest"));
        }
    }
    if effect.cardinalities.len() > MAX_CARDINALITIES {
        return Err(format!(
            "effect binds {} cardinalities; the cap is {MAX_CARDINALITIES}",
            effect.cardinalities.len()
        ));
    }
    for (key, value) in &effect.cardinalities {
        if !is_bounded_token(key) {
            return Err(format!("cardinality key is not bounded: {key}"));
        }
        if *value > u32::MAX as u64 {
            return Err(format!("cardinality {key} is unbounded: {value}"));
        }
    }
    if observation.evidence.len() > MAX_EVIDENCE_REFS {
        return Err(format!(
            "observation carries {} evidence references; the cap is {MAX_EVIDENCE_REFS}",
            observation.evidence.len()
        ));
    }
    let mut seen_evidence = std::collections::BTreeSet::new();
    for reference in &observation.evidence {
        if !is_bounded_reference(&reference.reference) {
            return Err(format!(
                "evidence reference is not a bounded digest/path: {}",
                reference.reference
            ));
        }
        if !seen_evidence.insert((reference.kind, reference.reference.as_str())) {
            return Err(format!(
                "duplicate evidence reference for kind {:?}: {}",
                reference.kind, reference.reference
            ));
        }
    }
    if let Some(limitation) = &observation.limitation_class
        && !is_bounded_token(limitation)
    {
        return Err(format!("limitation class is not a bounded stable token: {limitation}"));
    }
    Ok(())
}

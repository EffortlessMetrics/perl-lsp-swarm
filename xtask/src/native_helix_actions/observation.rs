//! Typed, bounded observations for the Helix hosted-session contract
//! (#12832). The shapes deliberately track `native_neovim_actions`
//! (#11409/#12638) so downstream receipt owners read both dialects the same
//! way, while every channel that does not exist for released Helix is absent
//! rather than stubbed.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Identity of the backend that produced the observation. The fake backend is
/// a first-class test instrument, never confused with a real host adapter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "backend", rename_all = "snake_case", deny_unknown_fields)]
pub enum BackendIdentity {
    Fake,
    HostAdapter { adapter_digest: String },
}

/// The observation plane. Product, instrument, reporting, and cleanup
/// observations stay separate for downstream composition. Released Helix has
/// no state-query API, so this contract produces product-plane facts only
/// through command-line and keystroke surfaces, instrument-plane facts only
/// through the bounded `--log` capture hook, and cleanup-plane facts only
/// through the bounded host handoffs; the `reporting` plane stays reserved
/// for the generic receipt owners.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservationPlane {
    Product,
    Instrument,
    Reporting,
    Cleanup,
}

/// The observation result vocabulary, aligned with #11409 so failure classes
/// are comparable across editors.
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

/// What actually executed to produce the observation. Released Helix exposes
/// no API route at all, so there is no API variant to abuse: surfaces are the
/// command line, ordinary keystrokes, the bounded log capture hook, or a
/// bounded host handoff.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "route", rename_all = "snake_case", deny_unknown_fields)]
pub enum ObservedRoute {
    /// A command-line surface the action declared (`argv files`,
    /// `config -c PATH`).
    CommandLineSurface { surface: String },
    /// An ordinary keystroke sequence routed through the PTY stimulus
    /// channel (`keys :q!`).
    NativeKeys { keys: String },
    /// The offline read-only capture of the session log helix writes via its
    /// public `--log` option, with its exact owning authority.
    InstrumentHook { hook: String, owner: String },
    /// A bounded handoff to the shared host-execution authority analog;
    /// process spawn/deadline/cleanup stay there, never here.
    HostHandoff { handoff: String },
}

/// Where the observed effect reached. Requested, returned, applied, and
/// visible/current stay distinct and ordered. For this contract the
/// instrument plane (log text) can never claim beyond `returned`: a log line
/// proves transport-level traffic, never rendered/applied editor state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectStage {
    Requested,
    Returned,
    Applied,
    VisibleCurrent,
}

/// The closed effect-class vocabulary of the hosted-session arc.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectClass {
    HostSessionState,
    ConfigState,
    DocumentOpened,
    ClientIdentity,
    DiagnosticTraffic,
    TerminalState,
    HandoffState,
}

/// The exact host/client/server/config/root/run-binding subject. The pinned
/// tokens are registered by [`super`] and fail closed on any other value;
/// exact release bytes and canonical config bytes remain owned by their
/// cited issues and are never re-pinned here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubjectBinding {
    /// Pinned host product (`helix`).
    pub host_product: String,
    /// Stable token naming the pinned host subject scope; the released-stable
    /// subject is owned by #7714 and the source-subject row by #7780.
    pub host_version_scope: String,
    /// The built-in language-server client identity; `perlnavigator` or any
    /// other configured Perl server observation is rejected.
    pub client_id: String,
    /// The server executable (`perllsp`); the retired `perl-lsp` spelling is
    /// rejected by the pin comparison.
    pub server_executable: String,
    /// Stable token naming the canonical config subject (#7724).
    pub config_id: String,
    /// Stable token naming the workspace-root subject of this run.
    pub root_id: String,
    /// The opened-document run binding when the action opens a document.
    pub document: Option<DocumentBinding>,
}

/// One opened-document run binding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DocumentBinding {
    /// Fixture-root-relative path (bounded by the fixture-path law).
    pub fixture_path: String,
}

/// The observed result/effect identity. Distinct from any expectation: this
/// channel carries what was requested/returned/applied and the identities it
/// binds, plus the generation snapshot it was computed against.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservedEffect {
    pub stage: EffectStage,
    /// Effect classes this observation reports; must be within the action's
    /// declared emissions.
    pub effect_classes: Vec<EffectClass>,
    /// `sha256:` digest over the bounded observed-result identity.
    pub result_digest: String,
    /// `sha256:` digest over the applied effect where the stage reached
    /// `applied` or beyond.
    pub effect_digest: Option<String>,
    /// Identity digests keyed by stable token (server process identity,
    /// spawned-server argv identity, log segment identity, ...).
    pub identity_digests: BTreeMap<String, String>,
    /// The currentness snapshot the effect was computed against.
    pub generations: super::predicate::GenerationSnapshot,
}

/// The typed, bounded observation one native Helix action emits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TypedObservation {
    pub schema_version: String,
    /// Ordered action identity: strictly increasing within one run.
    pub sequence: u64,
    pub action_id: String,
    pub plane: ObservationPlane,
    pub backend: BackendIdentity,
    pub subject: SubjectBinding,
    pub route: ObservedRoute,
    pub predicate_evidence: Vec<super::predicate::PredicateEvidence>,
    pub observed: ObservedEffect,
    /// The currentness snapshot the observation settled against.
    pub generations: super::predicate::GenerationSnapshot,
    pub result: ObservationResult,
    /// Limitation/failure class; required for every result except `observed`.
    pub limitation_class: Option<String>,
}

/// `sha256:`-prefixed bounded digest spelling (same rule as the Neovim
/// dialect).
pub fn is_bounded_digest(value: &str) -> bool {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return false;
    };
    hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// Fixture-root-relative path law (same rule as the Vim specialized driver):
/// no scheme, no drive letter, no home marker, no parent traversal, no
/// environment interpolation.
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

/// Shared byte cap for free-token fields; evidence stays bounded and
/// privacy-safe.
pub const MAX_TOKEN_BYTES: usize = 200;

/// Per-collection caps so a host-produced record cannot pad durable evidence.
pub const MAX_PREDICATE_EVIDENCE: usize = 16;
pub const MAX_EFFECT_CLASSES: usize = 8;
pub const MAX_IDENTITY_DIGESTS: usize = 16;

/// True when the value is a stable reason token inside the shared byte cap.
pub fn is_bounded_token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_TOKEN_BYTES
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_' | b'/'))
}

/// Structural boundedness validation shared by every field of the model.
/// Surface/hook grammars are completed by [`super`] at the contract layer.
pub fn validate_bounded(observation: &TypedObservation) -> Result<(), String> {
    if !is_bounded_token(&observation.action_id) {
        return Err(format!("action id is not a bounded stable token: {}", observation.action_id));
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
    if let Some(document) = &subject.document
        && !is_fixture_relative_path(&document.fixture_path)
    {
        return Err(format!(
            "document path is not fixture-root-relative: {}",
            document.fixture_path
        ));
    }
    if let BackendIdentity::HostAdapter { adapter_digest } = &observation.backend
        && !is_bounded_digest(adapter_digest)
    {
        return Err("host adapter identity is not a bounded digest".to_string());
    }
    if observation.observed.effect_classes.len() > MAX_EFFECT_CLASSES {
        return Err("effect classes exceed the collection cap".to_string());
    }
    if observation.predicate_evidence.len() > MAX_PREDICATE_EVIDENCE {
        return Err("predicate evidence exceeds the collection cap".to_string());
    }
    if observation.observed.identity_digests.len() > MAX_IDENTITY_DIGESTS {
        return Err("identity digests exceed the collection cap".to_string());
    }
    for (token, digest) in &observation.observed.identity_digests {
        if !is_bounded_token(token) {
            return Err(format!("identity token is not bounded: {token}"));
        }
        if !is_bounded_digest(digest) {
            return Err(format!("identity digest for {token} is unbounded"));
        }
    }
    if !is_bounded_digest(&observation.observed.result_digest) {
        return Err("observed result digest is unbounded".to_string());
    }
    if let Some(digest) = &observation.observed.effect_digest
        && !is_bounded_digest(digest)
    {
        return Err("effect digest is unbounded".to_string());
    }
    match &observation.limitation_class {
        // A successful observation carries no limitation: attaching a failure
        // token to `observed` produces a contradictory record.
        Some(class) if !observation.result.requires_limitation() => {
            return Err(format!("an observed result must not carry a limitation class: {class}"));
        }
        Some(class) if is_bounded_token(class) => {}
        Some(class) => return Err(format!("limitation class is not a bounded token: {class}")),
        None if observation.result.requires_limitation() => {
            return Err(format!("result {:?} requires a limitation class", observation.result));
        }
        None => {}
    }
    Ok(())
}

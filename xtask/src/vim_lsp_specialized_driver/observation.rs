//! Typed, bounded, privacy-safe observation model for the specialized
//! Vim/vim-lsp driver (#11380).
//!
//! A durable observation may carry identities, generation snapshots, digests,
//! cardinalities, class tokens, and typed dispositions. It may never carry
//! arbitrary source text, home paths, environment dumps, full logs, or
//! unbounded client state: every string field is either a stable reason token,
//! a `sha256:`-prefixed digest, or a fixture-relative path, and unknown fields
//! are rejected outright (`deny_unknown_fields`), so private data cannot ride
//! along inside the durable record.
//!
//! The model is receipt-agnostic on purpose: it validates and classifies, it
//! never registers a journey cell, writes a receipt, or decides support.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use super::barrier::{BarrierEvidence, GenerationSnapshot, validate_barrier_evidence};
use crate::client_compat_fixture::is_reason_token;

/// Identity of the backend that produced the observation. The fake backend is
/// a first-class test instrument, never confused with the real adapter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "backend", rename_all = "snake_case")]
pub enum BackendIdentity {
    Fake,
    Adapter { script_digest: String },
}

/// What actually executed to produce the observation. `RawProtocolRequest` is
/// representable precisely so a substitution can be detected: it is never a
/// lawful route for any action of this vocabulary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "route", rename_all = "snake_case")]
pub enum ObservedRoute {
    /// A native Vim surface (command, option, autocmd event) executed; the
    /// spelling must be one of the action's declared native surfaces.
    NativeVimSurface { surface: String },
    /// A vim-lsp public API call from the #11369 classified inventory.
    PublicClientApi { api: String },
    /// A deliberate test stimulus (process kill, malformed config) that must
    /// never be labeled product behavior.
    TestStimulus { stimulus: String },
    /// A bounded handoff to the #10894/#10944 host-process authorities.
    HostHandoff { handoff: String },
    /// A raw LSP protocol request bypassing the client. Always rejected.
    RawProtocolRequest { method: String },
}

/// How a format result was triggered. The save-format family distinguishes
/// save-triggered formatting from an explicit manual comparator run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SaveTrigger {
    SaveEvent,
    ManualComparator,
}

/// How the filetype of an activation row came to be Perl.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DetectionRoute {
    /// Native detection settled by Vim itself after open.
    Native,
    /// The narrowly declared override row applied a user-equivalent override.
    DeclaredOverride,
    /// The filetype was pre-forced before open. Rejected for native rows and
    /// never a lawful substitute for detection.
    PreForced,
}

/// Owner identity for a save-format route or a semantic provider answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OwnerIdentity {
    pub owner_class: String,
    pub owner_token: String,
}

/// A bounded semantic probe: the independent semantic oracle input a
/// freshness/currentness observation must carry so an event log alone can
/// never substitute for the actual semantic result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SemanticProbe {
    pub probe_class: String,
    /// Provider/service identity that answered (server identity, not a client).
    pub provider_identity: String,
    /// Generation scope the answer was computed against.
    pub generation_scope: GenerationSnapshot,
    /// `sha256:` digest over the bounded probe result.
    pub result_digest: String,
}

/// One protocol event observation: class plus bounded digest, never the
/// payload itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtocolEventDigest {
    pub event_class: String,
    pub digest: String,
}

/// Server process tree disposition for the observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessDisposition {
    Running {
        generation: u32,
    },
    ExitedClean {
        generation: u32,
    },
    ExitedUnclean {
        generation: u32,
    },
    SupersededBy {
        old_generation: u32,
        new_generation: u32,
    },
    /// Unknown process state forces `not_proven`; it can never pass.
    Unknown,
}

/// Cleanup ledger state at observation time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CleanupLedger {
    Settled,
    Pending,
    Unknown,
}

/// Classified action outcome. Mirrors the #11380 family vocabulary:
/// applied / no_change / disabled / refused / failure / stale / cancelled for
/// settled dispositions, `not_proven` / `unsupported` / `client_not_exposed`
/// for honest boundaries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionResult {
    Applied,
    NoChange,
    Disabled,
    Refused,
    Failure,
    Stale,
    Cancelled,
    NotProven,
    Unsupported,
    ClientNotExposed,
}

impl ActionResult {
    /// True when this outcome requires an admitted limitation to be honest.
    pub fn requires_limitation(self) -> bool {
        matches!(
            self,
            ActionResult::NotProven | ActionResult::Unsupported | ActionResult::ClientNotExposed
        )
    }
}

/// Fixture binding: which landed fixture authorities the observation ran
/// against. Only landed #11369 substrate tokens exist today; #11378 fixture
/// tokens arrive with that issue and will extend this vocabulary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FixtureBinding {
    pub fixture_owners: Vec<String>,
    /// Fixture-root-relative paths (for example `workspace/lib/main.pm`).
    pub fixture_relative_paths: Vec<String>,
}

/// The typed, bounded observation one specialized action emits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TypedObservation {
    pub schema_version: String,
    pub action_id: String,
    pub backend: BackendIdentity,
    /// Subject identities the observation binds (host/client/server).
    pub host_product: String,
    pub client_id: String,
    pub server_executable: String,
    pub fixture: FixtureBinding,
    pub generations: GenerationSnapshot,
    pub route: ObservedRoute,
    pub trigger: Option<SaveTrigger>,
    /// Number of save-format owners configured when the action ran.
    pub configured_owner_count: Option<u32>,
    pub owner: Option<OwnerIdentity>,
    pub semantic_probe: Option<SemanticProbe>,
    /// Request/invocation/replay cardinalities, keyed by stable token.
    pub cardinalities: BTreeMap<String, u64>,
    /// Buffer/file/source/config digests, keyed by stable token.
    pub digests: BTreeMap<String, String>,
    pub barriers: Vec<BarrierEvidence>,
    pub protocol_events: Vec<ProtocolEventDigest>,
    pub process: ProcessDisposition,
    pub cleanup: CleanupLedger,
    pub session_iterations: Option<u32>,
    pub detection_route: Option<DetectionRoute>,
    pub outcome: ActionResult,
    pub limitation: Option<String>,
}

impl TypedObservation {
    /// Barrier evidence of one kind, if present.
    pub fn barrier(&self, kind: super::barrier::BarrierKind) -> Option<&BarrierEvidence> {
        self.barriers.iter().find(|evidence| evidence.kind() == kind)
    }
}

/// `sha256:`-prefixed bounded digest spelling, same rule as the cell catalog.
pub fn is_bounded_digest(value: &str) -> bool {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return false;
    };
    hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// Fixture-root-relative path law: no scheme, no drive letter, no home marker,
/// no parent traversal, no environment interpolation. Just governed fixture
/// tree positions (Perl module files are conventionally capitalized, so the
/// segment charset is mixed-case but bounded).
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

/// Structural boundedness validation shared by every field of the model.
pub fn validate_bounded(observation: &TypedObservation) -> Result<(), String> {
    if !is_reason_token(&observation.action_id) {
        return Err(format!("action id is not a stable token: {}", observation.action_id));
    }
    for token in [&observation.host_product, &observation.client_id, &observation.server_executable]
    {
        if !is_reason_token(token) {
            return Err(format!("identity token is not stable: {token}"));
        }
    }
    if let BackendIdentity::Adapter { script_digest } = &observation.backend
        && !is_bounded_digest(script_digest)
    {
        return Err("adapter backend must bind its script digest".to_string());
    }
    for owner in &observation.fixture.fixture_owners {
        if !is_reason_token(owner) {
            return Err(format!("fixture owner token is not stable: {owner}"));
        }
    }
    for path in &observation.fixture.fixture_relative_paths {
        if !is_fixture_relative_path(path) {
            return Err(format!("fixture path is not fixture-root-relative: {path}"));
        }
    }
    match &observation.route {
        ObservedRoute::NativeVimSurface { surface } => {
            if !super::is_native_vim_surface(surface) {
                return Err(format!("native surface spelling is outside the grammar: {surface}"));
            }
        }
        ObservedRoute::TestStimulus { stimulus } => {
            if !is_reason_token(stimulus) {
                return Err(format!("route token is not stable: {stimulus}"));
            }
        }
        ObservedRoute::HostHandoff { handoff } => {
            if !is_reason_token(handoff) {
                return Err(format!("route token is not stable: {handoff}"));
            }
        }
        ObservedRoute::PublicClientApi { api } => {
            // The api string is the #11369 inventory spelling, which includes
            // human-readable rows like `User autocmd lsp_server_init`; the
            // binding laws are membership in the action's declared surfaces
            // and the contract test's inventory check. Boundedness here only
            // blocks free text: sane length, no control characters.
            if api.is_empty() || api.len() > 120 || api.chars().any(|char| char.is_control()) {
                return Err(format!("public api token is not bounded: {api}"));
            }
        }
        ObservedRoute::RawProtocolRequest { method } => {
            if !is_reason_token(method) {
                return Err(format!("raw protocol method token is not stable: {method}"));
            }
        }
    }
    if let Some(owner) = &observation.owner
        && (!is_reason_token(&owner.owner_class) || !is_reason_token(&owner.owner_token))
    {
        return Err("owner identity tokens must be stable".to_string());
    }
    if let Some(probe) = &observation.semantic_probe {
        if !is_reason_token(&probe.probe_class) || !is_reason_token(&probe.provider_identity) {
            return Err("semantic probe tokens must be stable".to_string());
        }
        if !is_bounded_digest(&probe.result_digest) {
            return Err("semantic probe result must be a bounded digest".to_string());
        }
    }
    for (key, value) in &observation.cardinalities {
        if !is_reason_token(key) {
            return Err(format!("cardinality key is not stable: {key}"));
        }
        if *value > u32::MAX as u64 {
            return Err(format!("cardinality {key} is unbounded: {value}"));
        }
    }
    for (key, value) in &observation.digests {
        if !is_reason_token(key) || !is_bounded_digest(value) {
            return Err(format!("digest entry {key} is not a bounded digest pair"));
        }
    }
    for evidence in &observation.barriers {
        validate_barrier_evidence(evidence)?;
    }
    for event in &observation.protocol_events {
        if !is_reason_token(&event.event_class) || !is_bounded_digest(&event.digest) {
            return Err(format!("protocol event {} is not bounded", event.event_class));
        }
    }
    if let Some(limitation) = &observation.limitation
        && !is_reason_token(limitation)
    {
        return Err(format!("limitation token is not stable: {limitation}"));
    }
    Ok(())
}

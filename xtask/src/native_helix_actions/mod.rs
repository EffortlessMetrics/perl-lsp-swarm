//! Checked native Helix hosted-session action and observation contract
//! (`native_helix_actions.v1`, #12832).
//!
//! This is the Helix analog of the Neovim #11409/#12638 arc, adapted to the
//! mechanically observed boundary of released Helix rather than copied: Helix
//! exposes no daemon, headless, IPC, or scripting surface (see #12832 for the
//! researched flag set), so there are no editor-API routes to classify. Every
//! observation today can only come from:
//!
//! - command-line surfaces (`hx [flags] FILE`, `-c/--config`),
//! - ordinary keystrokes driven through a PTY stimulus channel,
//! - the offline read-only capture of the `--log` session file
//!   (instrument-only hook, exact owner `hx_log_capture`), or
//! - bounded handoffs for process spawn/deadline/cleanup, which stay with a
//!   shared host-execution authority exactly like the Neovim #10894 boundary.
//!
//! Fail-closed laws enforced by [`validate_observation`] and
//! [`validate_table`]:
//!
//! - the subject must be the pinned released-stable host + built-in client +
//!   `perllsp`; a foreign product/client/server/config observation is
//!   rejected;
//! - instrument-plane evidence (log text) can never claim beyond
//!   `returned`: a log line does not prove rendered/applied/visible-current
//!   editor state;
//! - handoff actions stay fail-closed (`not_proven`/`unsupported`) until a
//!   shared host-execution authority lands; no row implements process policy;
//! - a satisfied predicate names its settled state (bounded digest); elapsed
//!   time alone is never satisfaction; a timed-out predicate forces
//!   `not_proven`; substitutions are hard failures;
//! - an `observed` result reaches at least the action's declared minimum
//!   stage, applied-or-beyond binds an effect digest, and predicate settlement
//!   cannot be newer than the observation snapshot nor older than the floor
//!   dimensions of its kind;
//! - observations are bounded and privacy-safe: stable tokens, `sha256:`
//!   digests, fixture-relative paths, no unknown fields.
//!
//! Deliberately absent from this layer: receipts, journeys, compat
//! projection into `editor_client_compat.v1`, and any per-cell expectation
//! authority. Those activate only after a real hosted-session runner lands
//! under #7714/#7780 rules; until then no honest path to an `observed`
//! result exists outside the fake backend.

pub mod fake;
pub mod observation;
pub mod predicate;

use anyhow::Result;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

use observation::{
    EffectClass, EffectStage, ObservationPlane, ObservationResult, ObservedRoute, TypedObservation,
};
use predicate::{GenerationDimension, PredicateEvidence, PredicateKind, PredicateRequirement};

/// Identity of this contract, for consumers that name the semantics they
/// validated against.
pub const CONTRACT_SCHEMA_VERSION: &str = "native_helix_actions.v1";

/// The action-ID namespace: `helix.native.<family>.<name>`.
pub const ACTION_ID_PREFIX: &str = "helix.native.";

/// The one family this contract registers; later families join as reviewed,
/// digest-visible table edits rather than namespace drift.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionFamily {
    HostSession,
}

impl ActionFamily {
    pub fn token(self) -> &'static str {
        match self {
            ActionFamily::HostSession => "host_session",
        }
    }
}

/// How one action's execution is owned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionClass {
    /// An ordinary user-shaped action through real public surfaces.
    UserAction,
    /// A read-only observation of current session/host state.
    Observation,
    /// A process/session operation owned by the shared host-execution
    /// authority; this contract only emits the bounded fail-closed handoff.
    HostHandoff,
}

/// The surface classification of one action's helper surface. There are no
/// Neovim-style API classifications because Helix has no API route today;
/// new variants only enter with a researched mechanical surface (#12832).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceClassification {
    /// Documented command-line behavior (`hx [flags] FILE`, `-c PATH`).
    CommandLineSurface,
    /// Ordinary keystrokes through the PTY stimulus channel.
    NativeKeys,
    /// An instrument-only hook with its exact owner; never labeled product
    /// behavior.
    InstrumentOnlyHook { owner: &'static str },
    /// Not exposed: no launchable public surface exists without the shared
    /// host-execution authority; these actions stay fail-closed.
    NotExposed,
}

/// A justified instrument-only hook citation.
#[derive(Debug, Clone, Copy)]
pub struct InstrumentHookUse {
    /// Public surface spelling of the hook.
    pub api: &'static str,
    /// Why the classified observation needs this hook.
    pub justification: &'static str,
    /// Under which researched condition the hook retires.
    pub retirement: &'static str,
}

/// Typed input parameter kinds; actions declare named bindings so free-text
/// parameter channels cannot appear.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputKind {
    /// Fixture/session owner token.
    SessionOwner,
    /// Canonical config digest binding (`sha256:` over config bytes).
    ConfigDigest,
    /// Fixture-root-relative document path.
    FixtureDocument,
    /// Bounded user key sequence.
    KeySequence,
}

/// One typed input binding.
#[derive(Debug, Clone, Copy)]
pub struct InputBinding {
    pub name: &'static str,
    pub kind: InputKind,
}

/// Per-action observation shape rules beyond the shared laws.
#[derive(Debug, Clone, Copy)]
pub struct ShapeRules {
    /// Identity digests the observed effect must bind (server process
    /// identity, argv identity, log segment identity, ...).
    pub required_identity_digests: &'static [&'static str],
    /// Whether the action opens a document run binding.
    pub requires_document: bool,
}

pub const DEFAULT_SHAPE: ShapeRules =
    ShapeRules { required_identity_digests: &[], requires_document: false };

/// One registered action. Every field is load-bearing at validation time;
/// any semantic edit is a visible identity change of the vocabulary.
#[derive(Debug, Clone, Copy)]
pub struct ActionSpec {
    /// Stable ID in the `helix.native.<family>.<name>` namespace.
    pub action_id: &'static str,
    pub family: ActionFamily,
    pub class: ActionClass,
    pub surface: SurfaceClassification,
    pub summary: &'static str,
    /// Surfaces/hooks the action routes through (grammar-checked spellings).
    pub surface_uses: &'static [&'static str],
    /// Instrument-only hooks cited by this action, each justified.
    pub instrument_hooks: &'static [InstrumentHookUse],
    /// Typed inputs.
    pub inputs: &'static [InputBinding],
    /// Effect classes the action may emit.
    pub emits: &'static [EffectClass],
    /// Bounded observable predicates asynchronous settlement waits on.
    pub required_predicates: &'static [PredicateRequirement],
    /// The minimum honest effect stage for an `observed` result.
    pub claim: EffectStage,
    pub shape: ShapeRules,
    /// Result vocabulary this action may report.
    pub allowed_results: &'static [ObservationResult],
}

const fn predicate(kind: PredicateKind, max_wait_ms: u64) -> PredicateRequirement {
    PredicateRequirement { kind, max_wait_ms }
}

/// Result vocabulary admitted by launchable actions once lawful evidence
/// exists.
const FULL_RESULTS: &[ObservationResult] = &[
    ObservationResult::Observed,
    ObservationResult::Mismatch,
    ObservationResult::Unsupported,
    ObservationResult::NotProven,
    ObservationResult::InstrumentFailed,
];

/// Fail-closed vocabulary for host handoffs: with no shared host-execution
/// authority landed, nothing that exists today can honestly produce an
/// `observed` handoff. Admitting it is a reviewed vocabulary edit landing
/// with its runner.
const HANDOFF_RESULTS: &[ObservationResult] =
    &[ObservationResult::NotProven, ObservationResult::Unsupported];

/// Budgets follow the Neovim dialect so downstream deadlines stay comparable.
const HANDSHAKE_BUDGET_MS: u64 = 30_000;
const TERMINAL_BUDGET_MS: u64 = 45_000;

const LOG_CAPTURE_HOOK: &str = "--log";
const LOG_CAPTURE_OWNER: &str = "hx_log_capture";
const HOST_HANDOFF_LAUNCH: &str = "host_process_handoff";
const HOST_HANDOFF_POST_RUN: &str = "post_run_observation_handoff";

/// The published hosted-session action vocabulary. Rows mirror the Neovim
/// #11409 host-session family at the depth the researched boundary supports.
pub const ACTIONS: &[ActionSpec] = &[
    // -----------------------------------------------------------------
    // Launch and teardown handoffs (process policy stays out of band)
    // -----------------------------------------------------------------
    ActionSpec {
        action_id: "helix.native.host_session.launch_isolated_host",
        family: ActionFamily::HostSession,
        class: ActionClass::HostHandoff,
        surface: SurfaceClassification::NotExposed,
        summary: "launch an isolated released-stable hx subject through the shared host-execution handoff",
        surface_uses: &[],
        instrument_hooks: &[],
        inputs: &[InputBinding { name: "subject", kind: InputKind::SessionOwner }],
        emits: &[EffectClass::HandoffState],
        required_predicates: &[],
        claim: EffectStage::Requested,
        shape: DEFAULT_SHAPE,
        allowed_results: HANDOFF_RESULTS,
    },
    ActionSpec {
        action_id: "helix.native.host_session.post_run_observation_handoff",
        family: ActionFamily::HostSession,
        class: ActionClass::HostHandoff,
        surface: SurfaceClassification::NotExposed,
        summary: "hand post-run observations and cleanup to the shared host-execution authority",
        surface_uses: &[],
        instrument_hooks: &[],
        inputs: &[],
        emits: &[EffectClass::HandoffState],
        required_predicates: &[predicate(PredicateKind::ServerTerminalState, TERMINAL_BUDGET_MS)],
        claim: EffectStage::Requested,
        shape: ShapeRules {
            required_identity_digests: &["server_process_identity"],
            ..DEFAULT_SHAPE
        },
        allowed_results: HANDOFF_RESULTS,
    },
    // -----------------------------------------------------------------
    // Product-plane configuration/open surfaces
    // -----------------------------------------------------------------
    ActionSpec {
        action_id: "helix.native.host_session.load_canonical_config",
        family: ActionFamily::HostSession,
        class: ActionClass::UserAction,
        surface: SurfaceClassification::CommandLineSurface,
        summary: "load the canonical languages.toml through hx's public -c/--config command-line surface",
        surface_uses: &["config -c PATH"],
        instrument_hooks: &[],
        inputs: &[
            InputBinding { name: "config", kind: InputKind::ConfigDigest },
            InputBinding { name: "subject", kind: InputKind::SessionOwner },
        ],
        emits: &[EffectClass::ConfigState],
        required_predicates: &[],
        claim: EffectStage::Applied,
        shape: DEFAULT_SHAPE,
        allowed_results: FULL_RESULTS,
    },
    ActionSpec {
        action_id: "helix.native.host_session.open_document_argv",
        family: ActionFamily::HostSession,
        class: ActionClass::UserAction,
        surface: SurfaceClassification::CommandLineSurface,
        summary: "open the fixture document through hx's documented argv file-selection surface",
        surface_uses: &["argv files"],
        instrument_hooks: &[],
        inputs: &[InputBinding { name: "document", kind: InputKind::FixtureDocument }],
        emits: &[EffectClass::DocumentOpened, EffectClass::ClientIdentity],
        required_predicates: &[predicate(
            PredicateKind::ServerHandshakeSettled,
            HANDSHAKE_BUDGET_MS,
        )],
        claim: EffectStage::Applied,
        shape: ShapeRules {
            required_identity_digests: &["server_process_identity"],
            requires_document: true,
        },
        allowed_results: FULL_RESULTS,
    },
    ActionSpec {
        action_id: "helix.native.host_session.quit_host_keys",
        family: ActionFamily::HostSession,
        class: ActionClass::UserAction,
        surface: SurfaceClassification::NativeKeys,
        summary: "quit the hosted session through the ordinary keystroke route and await terminal state",
        surface_uses: &["keys :q!", "keys :qa!"],
        instrument_hooks: &[],
        inputs: &[InputBinding { name: "keys", kind: InputKind::KeySequence }],
        emits: &[EffectClass::HostSessionState, EffectClass::TerminalState],
        required_predicates: &[predicate(PredicateKind::ServerTerminalState, TERMINAL_BUDGET_MS)],
        claim: EffectStage::Applied,
        shape: ShapeRules { required_identity_digests: &["exit_status"], ..DEFAULT_SHAPE },
        allowed_results: FULL_RESULTS,
    },
    // -----------------------------------------------------------------
    // Instrument-plane observations from the bounded --log capture
    // -----------------------------------------------------------------
    ActionSpec {
        action_id: "helix.native.host_session.observe_handshake_from_log",
        family: ActionFamily::HostSession,
        class: ActionClass::Observation,
        surface: SurfaceClassification::InstrumentOnlyHook { owner: LOG_CAPTURE_OWNER },
        summary: "observe the settled language-server handshake from the offline read-only --log capture",
        surface_uses: &[LOG_CAPTURE_HOOK],
        instrument_hooks: &[InstrumentHookUse {
            api: LOG_CAPTURE_HOOK,
            justification: "released Helix exposes no state-query API; lifecycle facts exist only in the --log file, parsed offline and read-only",
            retirement: "retire if Helix exposes a public session-state inspection surface",
        }],
        inputs: &[],
        emits: &[EffectClass::ClientIdentity],
        required_predicates: &[predicate(
            PredicateKind::ServerHandshakeSettled,
            HANDSHAKE_BUDGET_MS,
        )],
        claim: EffectStage::Returned,
        shape: ShapeRules {
            required_identity_digests: &["server_process_identity", "log_segment_identity"],
            ..DEFAULT_SHAPE
        },
        allowed_results: FULL_RESULTS,
    },
    ActionSpec {
        action_id: "helix.native.host_session.observe_diagnostic_traffic_from_log",
        family: ActionFamily::HostSession,
        class: ActionClass::Observation,
        surface: SurfaceClassification::InstrumentOnlyHook { owner: LOG_CAPTURE_OWNER },
        summary: "observe published diagnostic traffic in the --log capture; rendering is never claimed",
        surface_uses: &[LOG_CAPTURE_HOOK],
        instrument_hooks: &[InstrumentHookUse {
            api: LOG_CAPTURE_HOOK,
            justification: "publication/cardinality facts exist only in the --log file; UI consumption has no observable surface on released Helix",
            retirement: "retire if Helix exposes public diagnostics telemetry",
        }],
        inputs: &[],
        emits: &[EffectClass::DiagnosticTraffic],
        required_predicates: &[predicate(
            PredicateKind::ServerHandshakeSettled,
            HANDSHAKE_BUDGET_MS,
        )],
        claim: EffectStage::Returned,
        shape: ShapeRules { required_identity_digests: &["log_segment_identity"], ..DEFAULT_SHAPE },
        allowed_results: FULL_RESULTS,
    },
];

/// Look up one action by ID. Unknown IDs are the caller's typed error.
pub fn action_by_id(action_id: &str) -> Option<&'static ActionSpec> {
    ACTIONS.iter().find(|action| action.action_id == action_id)
}

/// The pinned subject identity tokens of this contract. Exact release bytes
/// are owned by the cited subjects and referenced here by token only.
pub const PINNED_HOST_PRODUCT: &str = "helix";
pub const PINNED_CLIENT_ID: &str = "helix_builtin_lsp";
pub const PINNED_SERVER_EXECUTABLE: &str = "perllsp";
/// Released-stable host subject scope (owned by #7714); master/source rows
/// belong to #7780 and use their own scope tokens after review.
pub const PINNED_HOST_VERSION_SCOPE: &str = "helix_release_subject_7714";
/// Canonical config subject token (owned by #7724).
pub const PINNED_CONFIG_ID: &str = "canonical_config_7724";

/// The closed host-handoff channel vocabulary.
pub const HOST_HANDOFF_TOKENS: &[&str] = &[HOST_HANDOFF_LAUNCH, HOST_HANDOFF_POST_RUN];

/// Which generation dimensions each predicate kind floors at the observation
/// snapshot: stale state can never prove currentness (#11409 falsifier 8).
pub fn predicate_floor_dimensions(kind: PredicateKind) -> &'static [GenerationDimension] {
    match kind {
        PredicateKind::ServerHandshakeSettled => &[GenerationDimension::Session],
        PredicateKind::ServerTerminalState => &[GenerationDimension::Process],
    }
}

/// Grammar for command-line surface spellings: exactly the two documented
/// shapes this contract registers.
pub fn is_command_line_surface(spelling: &str) -> bool {
    matches!(spelling, "argv files" | "config -c PATH")
}

/// Grammar for the PTY keystroke channel: `keys <sequence>` where the
/// sequence stays short and alphanumeric plus the punctuation ordinary Helix
/// typable commands need.
pub fn is_native_keys(spelling: &str) -> bool {
    let Some(keys) = spelling.strip_prefix("keys ") else {
        return false;
    };
    !keys.is_empty()
        && keys.len() <= 24
        && keys.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b':' | b'!' | b'<' | b'>' | b'-')
        })
}

fn expected_plane(class: ActionClass, surface: &SurfaceClassification) -> ObservationPlane {
    if matches!(surface, SurfaceClassification::InstrumentOnlyHook { .. }) {
        // Route-derived refinement: an action routed through an instrument-
        // only hook emits instrument-plane evidence even though its class is
        // observational.
        return ObservationPlane::Instrument;
    }
    match class {
        ActionClass::UserAction | ActionClass::Observation => ObservationPlane::Product,
        ActionClass::HostHandoff => ObservationPlane::Cleanup,
    }
}

/// Validate the registered table itself: unique prefix-consistent IDs,
/// non-empty emissions, hooks justified, handoff rows fail-closed, claims
/// inside admitted stages. Semantic edits must visibly change identity.
pub fn validate_table() -> Result<()> {
    let mut ids = BTreeSet::new();
    for action in ACTIONS {
        if !action.action_id.starts_with(ACTION_ID_PREFIX) {
            anyhow::bail!("action id lacks namespace prefix: {}", action.action_id);
        }
        let family_token = format!("{}.{}.", ACTION_ID_PREFIX, action.family.token());
        let rest = &action.action_id[ACTION_ID_PREFIX.len()..];
        let Some(name) = rest.strip_prefix(action.family.token()).and_then(|r| r.strip_prefix('.'))
        else {
            anyhow::bail!(
                "action id {} does not sit under its family namespace {family_token}*",
                action.action_id
            );
        };
        if name.is_empty() || !name.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        {
            anyhow::bail!("action id {} has an invalid leaf name", action.action_id);
        }
        if !ids.insert(action.action_id) {
            anyhow::bail!("duplicate action id: {}", action.action_id);
        }
        if action.emits.is_empty() {
            anyhow::bail!("action {} declares no effect classes", action.action_id);
        }
        if matches!(action.surface, SurfaceClassification::InstrumentOnlyHook { .. }) {
            let hook_ok =
                action.instrument_hooks.iter().any(|hook| action.surface_uses.contains(&hook.api));
            if !hook_ok {
                anyhow::bail!(
                    "instrument-classified action {} cites no registered hook",
                    action.action_id
                );
            }
        } else if !action.instrument_hooks.is_empty() {
            anyhow::bail!("non-instrument action {} declares instrument hooks", action.action_id);
        }
        for hook in action.instrument_hooks {
            if hook.api != LOG_CAPTURE_HOOK {
                anyhow::bail!(
                    "action {} cites unregistered instrument hook {}",
                    action.action_id,
                    hook.api
                );
            }
        }
        if matches!(action.class, ActionClass::HostHandoff) {
            if !matches!(action.surface, SurfaceClassification::NotExposed) {
                anyhow::bail!("handoff {} must be not-exposed", action.action_id);
            }
            if action.allowed_results != HANDOFF_RESULTS {
                anyhow::bail!(
                    "handoff {} must carry the fail-closed result vocabulary",
                    action.action_id
                );
            }
        } else if action.allowed_results == HANDOFF_RESULTS {
            anyhow::bail!(
                "non-handoff {} carries the fail-closed result vocabulary",
                action.action_id
            );
        }
    }
    Ok(())
}

/// Validate one observation against its action's laws. Returns `Ok(())` or a
/// precise violation; never mutates receipt or catalog state.
pub fn validate_observation(observation: &TypedObservation) -> Result<(), String> {
    if observation.schema_version != CONTRACT_SCHEMA_VERSION {
        return Err(format!(
            "schema version {} does not match {CONTRACT_SCHEMA_VERSION}",
            observation.schema_version
        ));
    }
    let action = action_by_id(&observation.action_id)
        .ok_or_else(|| format!("unknown action id: {}", observation.action_id))?;

    observation::validate_bounded(observation)?;

    // Subject pin: only the pinned released-stable Helix + built-in client +
    // perllsp + canonical-config subject observes; anything else is rejected.
    let subject = &observation.subject;
    if subject.host_product != PINNED_HOST_PRODUCT
        || subject.client_id != PINNED_CLIENT_ID
        || subject.server_executable != PINNED_SERVER_EXECUTABLE
        || subject.host_version_scope != PINNED_HOST_VERSION_SCOPE
        || subject.config_id != PINNED_CONFIG_ID
    {
        return Err(format!(
            "observation subject {}/{}/{}/{}/{} is not the pinned released-stable helix subject",
            subject.host_product,
            subject.client_id,
            subject.server_executable,
            subject.host_version_scope,
            subject.config_id
        ));
    }

    if action.shape.requires_document && subject.document.is_none() {
        return Err(format!("action {} requires a document run binding", action.action_id));
    }

    // Plane law: derived from classification, reporting plane reserved.
    let wanted_plane = expected_plane(action.class, &action.surface);
    if observation.plane == ObservationPlane::Reporting {
        return Err("the reporting plane is reserved for the generic receipt owners".to_string());
    }
    if observation.plane != wanted_plane {
        return Err(format!(
            "plane {:?} does not match classification (expected {wanted_plane:?})",
            observation.plane
        ));
    }

    // Route/surface law plus the instrument ceiling: log text never proves
    // rendered or applied state, so instrument-plane claims stop at
    // `returned`.
    match &observation.route {
        ObservedRoute::CommandLineSurface { surface } => {
            if action.surface != SurfaceClassification::CommandLineSurface {
                return Err(format!(
                    "action {} does not classify {surface} as a command-line surface",
                    action.action_id
                ));
            }
            if !is_command_line_surface(surface) || !action.surface_uses.contains(&surface.as_str())
            {
                return Err(format!(
                    "surface {surface} is outside what {} declares",
                    action.action_id
                ));
            }
        }
        ObservedRoute::NativeKeys { keys } => {
            if action.surface != SurfaceClassification::NativeKeys {
                return Err(format!(
                    "action {} does not classify a native keys route",
                    action.action_id
                ));
            }
            if !is_native_keys(&format!("keys {keys}")) {
                return Err(format!("unbounded key sequence spelling: {keys}"));
            }
            let spelled = format!("keys {keys}");
            let declared = action.surface_uses.iter().any(|declared| is_native_keys(declared))
                && action
                    .surface_uses
                    .iter()
                    .any(|declared| strip_keys_channel(declared) == keys.as_str());
            if !declared {
                return Err(format!(
                    "key sequence {spelled} is undeclared by {}",
                    action.action_id
                ));
            }
        }
        ObservedRoute::InstrumentHook { hook, owner } => {
            let SurfaceClassification::InstrumentOnlyHook { owner: pinned_owner } = action.surface
            else {
                return Err(format!(
                    "instrument hook {hook} offered as the route of {}; it is not instrument-classified",
                    action.action_id
                ));
            };
            if owner != pinned_owner {
                return Err(format!(
                    "instrument owner {owner} does not match the exact owner {pinned_owner}"
                ));
            }
            if !action.instrument_hooks.iter().any(|use_| use_.api == *hook) {
                return Err(format!(
                    "action {} does not declare instrument hook {hook}",
                    action.action_id
                ));
            }
            if observation.observed.stage > EffectStage::Returned {
                return Err(
                    "instrument-plane evidence can never prove rendered or applied state; \
                     its stage ceiling is returned"
                        .to_string(),
                );
            }
        }
        ObservedRoute::HostHandoff { handoff } => {
            if !matches!(action.class, ActionClass::HostHandoff) {
                return Err(format!(
                    "host handoff {handoff} offered as the route of {}; process policy stays with \
                     the shared host-execution authority, never an ordinary action",
                    action.action_id
                ));
            }
            if !HOST_HANDOFF_TOKENS.contains(&handoff.as_str()) {
                return Err(format!(
                    "host handoff {handoff} is outside the closed handoff vocabulary"
                ));
            }
            // Each handoff row binds exactly its own token: attributing a
            // launch to the post-run channel (or vice versa) mislabels which
            // process operation produced the evidence.
            let expected = expected_handoff_token(action.action_id);
            if handoff != expected {
                return Err(format!(
                    "action {} binds handoff {expected:?}, not {handoff}",
                    action.action_id
                ));
            }
        }
    }

    // Handoff results stay fail-closed regardless of payload completeness.
    if matches!(action.class, ActionClass::HostHandoff)
        && !HANDOFF_RESULTS.contains(&observation.result)
    {
        return Err(format!(
            "result {:?} is outside what handoff {} may honestly report before a host-execution \
             authority lands",
            observation.result, action.action_id
        ));
    }

    // Predicate law: every required kind present, budgets honored, state named,
    // generations floored at the observation snapshot on the relevant
    // dimensions, and settlement never newer than the snapshot.
    let mut required: BTreeMap<PredicateKind, u64> = BTreeMap::new();
    for requirement in action.required_predicates {
        required.insert(requirement.kind, requirement.max_wait_ms);
    }
    let mut seen_kinds = BTreeSet::new();
    for evidence in &observation.predicate_evidence {
        let kind = evidence.kind();
        let budget = required.get(&kind).copied().ok_or_else(|| {
            format!(
                "observation for {} carries {kind:?} evidence the action does not require",
                action.action_id
            )
        })?;
        if !seen_kinds.insert(kind) {
            return Err(format!(
                "duplicate predicate evidence for {kind:?} in {}",
                action.action_id
            ));
        }
        match evidence {
            PredicateEvidence::Satisfied {
                settled_state_digest,
                settled_generations,
                polls,
                waited_ms,
                ..
            } => {
                if !observation::is_bounded_digest(settled_state_digest) {
                    return Err(format!(
                        "predicate {kind:?} satisfaction does not name its settled state; elapsed \
                         time alone is never satisfaction"
                    ));
                }
                if *polls == 0 {
                    return Err(format!(
                        "predicate {kind:?} claims satisfaction without a single poll"
                    ));
                }
                if *waited_ms > budget {
                    return Err(format!(
                        "predicate {kind:?} waited {waited_ms}ms beyond the {budget}ms budget"
                    ));
                }
                for dimension in GENERATION_DIMENSIONS_COPIED {
                    let settled = settled_generations.dimension(*dimension);
                    let observed = observation.generations.dimension(*dimension);
                    if settled > observed {
                        return Err(format!(
                            "predicate {kind:?} settled at a newer {dimension:?} generation than \
                             the observation snapshot"
                        ));
                    }
                }
                for dimension in predicate_floor_dimensions(kind) {
                    let settled = settled_generations.dimension(*dimension);
                    let observed = observation.generations.dimension(*dimension);
                    if settled < observed {
                        return Err(format!(
                            "predicate {kind:?} settled below the observation's own {dimension:?} \
                             generation; stale state cannot prove a current result"
                        ));
                    }
                }
            }
            PredicateEvidence::TimedOut { polls, waited_ms, .. } => {
                if *polls == 0 || *waited_ms == 0 {
                    return Err(format!(
                        "predicate {kind:?} timeout must record its bounded polls and wait"
                    ));
                }
                if *waited_ms > budget {
                    return Err(format!(
                        "predicate {kind:?} timed out past the {budget}ms budget; an unbounded run \
                         is not evidence"
                    ));
                }
                if observation.result != ObservationResult::NotProven {
                    return Err(format!(
                        "predicate {kind:?} timed out but the result is {:?}; a timeout must \
                         classify not_proven",
                        observation.result
                    ));
                }
            }
            PredicateEvidence::Substituted { substitution, .. } => {
                return Err(format!(
                    "predicate {kind:?} evidence is a {substitution:?} substitution; \
                     substitutions are never state"
                ));
            }
        }
    }
    for kind in required.keys() {
        if !seen_kinds.contains(kind) {
            return Err(format!(
                "action {} requires predicate {kind:?} but the observation carries none",
                action.action_id
            ));
        }
    }

    // Effect routing law.
    if observation.observed.effect_classes.is_empty() {
        return Err(format!("observation for {} reports no effect class", action.action_id));
    }
    for class in &observation.observed.effect_classes {
        if !action.emits.contains(class) {
            return Err(format!(
                "effect class {class:?} is outside what {} may emit",
                action.action_id
            ));
        }
    }

    // Application/currentness law: minimum-stage honesty, effect digest
    // binding from applied upward, and required identity digests present.
    if observation.observed.stage < action.claim
        && matches!(observation.result, ObservationResult::Observed | ObservationResult::Mismatch)
    {
        return Err(format!(
            "result {:?} claims stage {:?} below {}'s declared minimum {:?}",
            observation.result, observation.observed.stage, action.action_id, action.claim
        ));
    }
    if observation.observed.stage >= EffectStage::Applied
        && observation.observed.effect_digest.is_none()
    {
        return Err(format!(
            "applied-or-beyond observation for {} does not bind its effect digest",
            action.action_id
        ));
    }
    for token in action.shape.required_identity_digests {
        if !observation.observed.identity_digests.contains_key(*token) {
            return Err(format!(
                "observation for {} omits its required {token:?} identity binding",
                action.action_id
            ));
        }
    }
    // Currentness of the effect itself: a successful observation's effect
    // snapshot must be the run's own settlement snapshot. Predicates staying
    // current while `observed` editor state is old is exactly the forgery
    // #11409 falsifier 8 names.
    if matches!(observation.result, ObservationResult::Observed)
        && observation.observed.generations != observation.generations
    {
        return Err(format!(
            "successful observation for {} carries an effect snapshot from a different \
             generation than the settlement snapshot",
            action.action_id
        ));
    }
    Ok(())
}

/// The exact handoff token one handoff row binds.
fn expected_handoff_token(action_id: &str) -> &'static str {
    if action_id.ends_with("post_run_observation_handoff") {
        "post_run_observation_handoff"
    } else {
        "host_process_handoff"
    }
}

/// Validate an ordered run of observations: nonempty, every record lawful,
/// and strictly increasing `sequence` so replay order is deterministic and
/// duplicate/zero/decreasing identities cannot pass any validation seam.
pub fn validate_run(observations: &[TypedObservation]) -> Result<(), String> {
    if observations.is_empty() {
        return Err("a run records at least one observation".to_string());
    }
    let mut previous: Option<u64> = None;
    for observation in observations {
        validate_observation(observation)?;
        let sequence = observation.sequence;
        if sequence == 0 {
            return Err("run sequences start at 1; 0 is not an ordered identity".to_string());
        }
        if let Some(prior) = previous.filter(|seen| sequence <= *seen) {
            return Err(format!("sequence {sequence} does not strictly increase past {prior}"));
        }
        previous = Some(sequence);
    }
    Ok(())
}

/// Local re-export so validation reads all three dimensions explicitly.
const GENERATION_DIMENSIONS_COPIED: &[GenerationDimension] = predicate::GENERATION_DIMENSIONS;

/// Strip the `keys ` channel token for membership comparison.
fn strip_keys_channel(spelling: &str) -> &str {
    spelling.strip_prefix("keys ").unwrap_or(spelling)
}

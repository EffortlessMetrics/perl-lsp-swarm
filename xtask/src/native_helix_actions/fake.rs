//! The first-class fake backend for the Helix hosted-session contract
//! (#12832). It exists so contract tests can exercise every action's honest
//! path and every forgery class without a real host; a fake observation is
//! always labeled [`BackendIdentity::Fake`] and is never product evidence.
//!
//! Honest paths here are deliberately modest: launchable actions settle
//! `observed`, while handoffs stay on their fail-closed vocabulary — the
//! terminal-state wait times out lawfully and classifies `not_proven`, which
//! is exactly what a world without a host-execution runner proves today.

use sha2::{Digest as _, Sha256};

use super::observation::{
    BackendIdentity, DocumentBinding, EffectStage, ObservationPlane, ObservationResult,
    ObservedEffect, ObservedRoute, SubjectBinding, TypedObservation,
};
use super::predicate::{GenerationSnapshot, PredicateEvidence};
use super::{ActionClass, ActionSpec};
use super::{
    CONTRACT_SCHEMA_VERSION, PINNED_CLIENT_ID, PINNED_CONFIG_ID, PINNED_HOST_PRODUCT,
    PINNED_HOST_VERSION_SCOPE, PINNED_SERVER_EXECUTABLE, SurfaceClassification, action_by_id,
};

/// Deterministic bounded digest for fake fixtures.
pub fn fake_digest(seed: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(seed.as_bytes());
    let digest: [u8; 32] = hasher.finalize().into();
    let mut out = String::with_capacity("sha256:".len() + 64);
    out.push_str("sha256:");
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// The settled-world fixture every lawful fake observation derives from.
#[derive(Debug, Clone, Copy)]
pub struct FakeWorld {
    pub host_generation: u32,
    pub process_generation: u32,
    pub session_generation: u32,
}

impl FakeWorld {
    /// A world where handshake and terminal waits have settled at current
    /// generations.
    pub fn settling() -> Self {
        Self { host_generation: 1, process_generation: 1, session_generation: 1 }
    }

    fn snapshot(&self) -> GenerationSnapshot {
        GenerationSnapshot {
            host: self.host_generation,
            process: self.process_generation,
            session: self.session_generation,
        }
    }
}

/// The pinned fake subject; mirrors the registered constants exactly.
pub fn fake_subject(with_document: bool) -> SubjectBinding {
    SubjectBinding {
        host_product: PINNED_HOST_PRODUCT.to_string(),
        host_version_scope: PINNED_HOST_VERSION_SCOPE.to_string(),
        client_id: PINNED_CLIENT_ID.to_string(),
        server_executable: PINNED_SERVER_EXECUTABLE.to_string(),
        config_id: PINNED_CONFIG_ID.to_string(),
        root_id: "fixture_root_v1".to_string(),
        document: with_document
            .then(|| DocumentBinding { fixture_path: "perl/basic.pl".to_string() }),
    }
}

/// The default lawful route spelling for one action.
pub fn default_route(action: &ActionSpec) -> ObservedRoute {
    match action.surface {
        SurfaceClassification::NotExposed => ObservedRoute::HostHandoff {
            handoff: if action.action_id.ends_with("post_run_observation_handoff") {
                "post_run_observation_handoff"
            } else {
                "host_process_handoff"
            }
            .to_string(),
        },
        SurfaceClassification::CommandLineSurface => match action.surface_uses.first() {
            Some(&"config -c PATH") => {
                ObservedRoute::CommandLineSurface { surface: "config -c PATH".to_string() }
            }
            _ => ObservedRoute::CommandLineSurface { surface: "argv files".to_string() },
        },
        SurfaceClassification::NativeKeys => ObservedRoute::NativeKeys {
            keys: spelling_keys(action.surface_uses.first().copied().unwrap_or("keys :q!")),
        },
        SurfaceClassification::InstrumentOnlyHook { owner } => ObservedRoute::InstrumentHook {
            hook: action.instrument_hooks.first().map(|hook| hook.api).unwrap_or("--log").into(),
            owner: owner.into(),
        },
    }
}

fn spelling_keys(spelling: &str) -> String {
    spelling.strip_prefix("keys ").unwrap_or(spelling).to_string()
}

fn plane_for(action: &ActionSpec) -> ObservationPlane {
    match (&action.class, &action.surface) {
        (_, SurfaceClassification::InstrumentOnlyHook { .. }) => ObservationPlane::Instrument,
        (ActionClass::HostHandoff, _) => ObservationPlane::Cleanup,
        (ActionClass::UserAction | ActionClass::Observation, _) => ObservationPlane::Product,
    }
}

fn default_predicates(action: &ActionSpec, world: FakeWorld) -> Vec<PredicateEvidence> {
    let snapshot = world.snapshot();
    action
        .required_predicates
        .iter()
        .map(|requirement| match requirement.kind {
            // Handoff rows cannot honestly settle anything yet: their waits
            // time out at budget and force the fail-closed classification.
            _ if is_handoff(action) => PredicateEvidence::TimedOut {
                kind: requirement.kind,
                polls: 2,
                waited_ms: requirement.max_wait_ms,
            },
            kind => PredicateEvidence::Satisfied {
                kind,
                settled_state_digest: fake_digest(&format!(
                    "{}:{kind:?}:settled",
                    action.action_id
                )),
                settled_generations: snapshot,
                polls: 3,
                waited_ms: 120,
            },
        })
        .collect()
}

fn is_handoff(action: &ActionSpec) -> bool {
    matches!(action.class, ActionClass::HostHandoff)
}

/// Build the lawful-path observation for one registered action.
pub fn observation_for(action_id: &str, sequence: u64) -> Result<TypedObservation, String> {
    let world = FakeWorld::settling();
    let action = action_by_id(action_id).ok_or_else(|| format!("unknown action id {action_id}"))?;
    let route = default_route(action);
    let plane = plane_for(action);
    let observed_stage = if matches!(route, ObservedRoute::InstrumentHook { .. }) {
        EffectStage::Returned.min(action.claim)
    } else {
        action.claim
    };
    let handoff = is_handoff(action);
    let observed_result =
        if handoff { ObservationResult::NotProven } else { ObservationResult::Observed };
    let stage = if handoff { EffectStage::Requested } else { observed_stage };

    Ok(TypedObservation {
        schema_version: CONTRACT_SCHEMA_VERSION.to_string(),
        sequence,
        action_id: action.action_id.to_string(),
        plane,
        backend: BackendIdentity::Fake,
        subject: fake_subject(action.shape.requires_document),
        route,
        predicate_evidence: default_predicates(action, world),
        observed: ObservedEffect {
            stage,
            effect_classes: action.emits.to_vec(),
            result_digest: fake_digest(&format!("result:{action_id}:{sequence}")),
            effect_digest: (!handoff && stage >= EffectStage::Applied)
                .then(|| fake_digest(&format!("effect:{action_id}:{sequence}"))),
            identity_digests: action
                .shape
                .required_identity_digests
                .iter()
                .map(|token| ((*token).to_string(), fake_digest(&format!("{action_id}:{token}"))))
                .collect(),
            generations: world.snapshot(),
        },
        generations: world.snapshot(),
        result: observed_result,
        limitation_class: if handoff {
            Some("host_execution_authority_not_landed".to_string())
        } else {
            None
        },
    })
}

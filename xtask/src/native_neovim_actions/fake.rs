//! Deterministic fake backend for the native Neovim action contract (#11409).
//!
//! The fake backend exists so predicate-wait, timeout, generation, routing,
//! expectation-separation, and result semantics are proven without launching
//! Neovim: tests script compact world facts (what settled, what timed out,
//! which generation answered, which route executed, which anchors resolved)
//! and the builder produces a well-formed [`TypedObservation`] that the one
//! real authority — [`super::validate_observation`] — then accepts or
//! rejects. The fake never bypasses validation, never invents an action,
//! never talks to a process, and never captures an expected value from
//! production output: expectations are fixed digest literals.

use std::collections::BTreeMap;

use super::observation::{
    AnchorPosition, BackendIdentity, DocumentBinding, EffectClass, EffectStage, EvidenceKind,
    EvidenceRef, ExpectationBinding, ExpectationSource, ObservationPlane, ObservationResult,
    ObservedEffect, ObservedRoute, SubjectBinding, TypedObservation,
};
use super::predicate::{GenerationSnapshot, PredicateEvidence, PredicateKind};
use super::{
    ActionClass, ActionSpec, CONTRACT_SCHEMA_VERSION, PINNED_CLIENT_ID, PINNED_CONFIG_ID,
    PINNED_HOST_PRODUCT, PINNED_HOST_VERSION_SCOPE, PINNED_SERVER_EXECUTABLE,
    SurfaceClassification,
};

/// Bounded fake digest spelling (64 hex characters after `sha256:`).
/// Deterministic so tests can pin values. Not real SHA-256 — a fake backend
/// must never be mistaken for a real host adapter's digests.
pub fn fake_digest(seed: &str) -> String {
    let mut value: u64 = 0xcbf2_9ce4_8422_2325;
    for &byte in seed.as_bytes() {
        value ^= u64::from(byte);
        value = value.wrapping_mul(0x1000_0000_01b3);
    }
    let rotated = value.rotate_left(17) ^ 0x9e37_79b9_7f4a_7c15;
    format!("sha256:{value:016x}{rotated:016x}{value:016x}{rotated:016x}")
}

/// The fake content-anchor table. Real anchors are owned by the #10903
/// fixture manifest; the fake table proves the resolution law (anchors are
/// named, resolved, bounded — never free text) with deterministic literals.
pub struct FakeAnchors {
    positions: BTreeMap<&'static str, AnchorPosition>,
}

impl FakeAnchors {
    pub fn new() -> Self {
        let mut positions = BTreeMap::new();
        positions.insert("sub_declaration", AnchorPosition { line: 3, character: 4 });
        positions.insert("call_site", AnchorPosition { line: 11, character: 8 });
        Self { positions }
    }

    /// Resolve one anchor token. Unknown anchors fail closed before any
    /// observation is built.
    pub fn resolve(&self, token: &str) -> Result<AnchorPosition, String> {
        self.positions.get(token).copied().ok_or_else(|| {
            format!("unknown content anchor {token}; anchors resolve through the fixture authority, never inline")
        })
    }
}

impl Default for FakeAnchors {
    fn default() -> Self {
        Self::new()
    }
}

/// The default anchor token the fake binds for anchor-taking actions.
pub const DEFAULT_ANCHOR: &str = "sub_declaration";

/// A scripted world snapshot the builder turns into one typed observation.
#[derive(Debug, Clone)]
pub struct FakeWorld {
    pub generations: GenerationSnapshot,
    /// Predicates that time out inside their budget (lawful, forces
    /// `not_proven`).
    pub timed_out_predicates: Vec<PredicateKind>,
    /// Overrides the default route (to probe route-class mismatches).
    pub route_override: Option<ObservedRoute>,
    /// Overrides the derived result.
    pub result: Option<ObservationResult>,
    /// Overrides the effect stage (to probe returned-not-applied shapes).
    pub stage_override: Option<EffectStage>,
}

impl FakeWorld {
    /// A world where the action's own required predicates all settle through
    /// named state, the pinned subject runs, and the outcome classifies as
    /// the first honest admitted result.
    pub fn settling() -> Self {
        Self {
            generations: GenerationSnapshot {
                host_generation: 1,
                process_generation: 1,
                document_generation: 1,
                root_generation: 1,
                source_generation: 1,
                config_generation: 1,
            },
            timed_out_predicates: Vec::new(),
            route_override: None,
            result: None,
            stage_override: None,
        }
    }
}

/// The lawful default route for an action's class and surface
/// classification.
pub fn default_route(action: &ActionSpec) -> ObservedRoute {
    if let Some(route) = &action_route_hint(action) {
        return route.clone();
    }
    match action.class {
        ActionClass::UserAction | ActionClass::Observation => {
            if let Some(api) = action.api_uses.first() {
                if let SurfaceClassification::PublicVersionScoped { scope } = action.surface {
                    return ObservedRoute::VersionScopedApi {
                        api: api.to_string(),
                        scope: scope.to_string(),
                    };
                }
                return ObservedRoute::PublicStableApi { api: api.to_string() };
            }
            if let Some(surface) = action.native_surfaces.first() {
                return ObservedRoute::NativeEditorSurface { surface: surface.to_string() };
            }
            // User-shaped actions always carry a declared surface; reaching
            // this arm is a table bug the contract test catches.
            ObservedRoute::NativeEditorSurface { surface: ":e".to_string() }
        }
        ActionClass::CompanionControl => ObservedRoute::CompanionControl {
            control: action.api_uses.first().copied().unwrap_or_default().to_string(),
        },
        ActionClass::TestStimulus => {
            ObservedRoute::TestStimulus { stimulus: "deliberate_stimulus".to_string() }
        }
        ActionClass::HostHandoff => {
            ObservedRoute::HostHandoff { handoff: "host_process_handoff".to_string() }
        }
    }
}

fn action_route_hint(action: &ActionSpec) -> Option<ObservedRoute> {
    match action.surface {
        SurfaceClassification::InstrumentOnlyHook { owner } => {
            action.instrument_hooks.first().map(|hook| ObservedRoute::InstrumentHook {
                hook: hook.api.to_string(),
                owner: owner.to_string(),
            })
        }
        _ => None,
    }
}

/// The pinned subject the fake binds.
pub fn fake_subject() -> SubjectBinding {
    SubjectBinding {
        host_product: PINNED_HOST_PRODUCT.to_string(),
        host_version_scope: PINNED_HOST_VERSION_SCOPE.to_string(),
        client_id: PINNED_CLIENT_ID.to_string(),
        server_executable: PINNED_SERVER_EXECUTABLE.to_string(),
        config_id: PINNED_CONFIG_ID.to_string(),
        root_id: "fixture_root".to_string(),
        document: DocumentBinding { fixture_path: "workspace/lib/main.pm".to_string(), buffer: 1 },
    }
}

/// Build one typed observation from a scripted world for one action, honoring
/// the action's shape rules so the positive case validates. Anchor inputs
/// resolve through the fake anchor table (unknown anchors fail closed).
pub fn observation_for(
    action: &ActionSpec,
    sequence: u64,
    world: &FakeWorld,
) -> Result<TypedObservation, String> {
    let anchors = FakeAnchors::new();
    let mut anchor_positions = BTreeMap::new();
    if action.requires_anchor() {
        anchor_positions.insert(DEFAULT_ANCHOR.to_string(), anchors.resolve(DEFAULT_ANCHOR)?);
    }

    let mut identity_digests = BTreeMap::new();
    for key in action.shape.required_identity_digests {
        identity_digests.insert(key.to_string(), fake_digest(key));
    }
    let mut cardinalities = BTreeMap::new();
    if action.shape.requires_client_exclusion_cardinalities {
        cardinalities.insert("pinned_clients_attached".to_string(), 1u64);
        cardinalities.insert("foreign_clients_attached".to_string(), 0u64);
    }
    if action.emits.contains(&EffectClass::DidChangeTraffic) {
        cardinalities.insert("didchange_requests".to_string(), 1u64);
    }

    let stage = world.stage_override.unwrap_or(action.claim);
    let mut effect_classes = Vec::new();
    for class in action.emits {
        if !effect_classes.contains(class) {
            effect_classes.push(*class);
        }
    }
    // The honest fake drives the matched path: where the action requires an
    // expectation, the observed result digest binds the same fixed literal
    // as the expectation digest (the comparison outcome is `observed`).
    let result_literal =
        if action.requires_expectation() { "expected_value" } else { "observed_result" };
    let effect = ObservedEffect {
        stage,
        effect_classes,
        result_digest: fake_digest(result_literal),
        effect_digest: (stage >= EffectStage::Applied).then(|| fake_digest("applied_effect")),
        anchor_positions,
        identity_digests,
        cardinalities,
        generations: world.generations,
    };

    let predicate_evidence: Vec<PredicateEvidence> = action
        .required_predicates
        .iter()
        .map(|requirement| {
            if world.timed_out_predicates.contains(&requirement.kind) {
                PredicateEvidence::TimedOut {
                    kind: requirement.kind,
                    polls: 3,
                    waited_ms: requirement.max_wait_ms,
                }
            } else {
                PredicateEvidence::Satisfied {
                    kind: requirement.kind,
                    settled_state_digest: fake_digest("settled_state"),
                    settled_generations: world.generations,
                    polls: 2,
                    waited_ms: 50,
                }
            }
        })
        .collect();

    let result =
        world.result.unwrap_or(if action.allowed_results.contains(&ObservationResult::Observed) {
            ObservationResult::Observed
        } else {
            ObservationResult::NotProven
        });
    let limitation_class = match result {
        ObservationResult::Observed => None,
        _ => Some("fake_backend_limitation".to_string()),
    };

    let plane = if matches!(action.surface, SurfaceClassification::InstrumentOnlyHook { .. }) {
        ObservationPlane::Instrument
    } else {
        match action.class {
            ActionClass::UserAction | ActionClass::Observation => ObservationPlane::Product,
            ActionClass::CompanionControl | ActionClass::TestStimulus => {
                ObservationPlane::Instrument
            }
            ActionClass::HostHandoff => ObservationPlane::Cleanup,
        }
    };

    Ok(TypedObservation {
        schema_version: CONTRACT_SCHEMA_VERSION.to_string(),
        sequence,
        action_id: action.action_id.to_string(),
        scenario_id: Some("neovim.bdd.activation".to_string()),
        fixture_id: Some("neovim.fixture.workspace".to_string()),
        cell_id: Some("neovim.cell.baseline.filetype".to_string()),
        plane,
        backend: BackendIdentity::Fake,
        subject: fake_subject(),
        route: world.route_override.clone().unwrap_or_else(|| default_route(action)),
        predicate_evidence,
        expectation: action.requires_expectation().then(|| ExpectationBinding {
            source: ExpectationSource::FixtureAuthority,
            expectation_id: "expectation_row_10903".to_string(),
            expectation_digest: fake_digest("expected_value"),
        }),
        observed: effect,
        generations: world.generations,
        result,
        limitation_class,
        evidence: vec![EvidenceRef {
            kind: EvidenceKind::DriverOutput,
            reference: fake_digest("driver_output"),
        }],
    })
}

//! Deterministic fake action backend for the specialized driver (#11380).
//!
//! The fake backend exists so barrier/timeout/generation/result semantics are
//! proven without launching full Vim: tests script compact world facts (what
//! settled, what timed out, which generation answered, which route executed)
//! and the builder produces a well-formed [`TypedObservation`] that the one
//! real authority — [`super::validate_observation`] — then accepts or rejects.
//! The fake never bypasses validation and never talks to a process.

use std::collections::BTreeMap;

use super::barrier::{BarrierEvidence, BarrierKind, GenerationSnapshot};
use super::observation::{
    ActionResult, BackendIdentity, CleanupLedger, DetectionRoute, FixtureBinding, ObservedRoute,
    OwnerIdentity, ProcessDisposition, SaveTrigger, SemanticProbe, TypedObservation,
};
use crate::vim_lsp_cell_catalog::vim_vim_lsp_subject;

/// Bounded fake digest spelling (64 hex characters after `sha256:`).
/// Deterministic so tests can pin values.
pub fn fake_digest(seed: &str) -> String {
    let mut value: u64 = 0xcbf2_9ce4_8422_2325;
    for &byte in seed.as_bytes() {
        value ^= u64::from(byte);
        value = value.wrapping_mul(0x1000_0000_01b3);
    }
    let rotated = value.rotate_left(17) ^ 0x9e37_79b9_7f4a_7c15;
    format!("sha256:{value:016x}{rotated:016x}{value:016x}{rotated:016x}")
}

/// A scripted world snapshot the builder turns into a typed observation.
#[derive(Debug, Clone)]
pub struct FakeWorld {
    pub generations: GenerationSnapshot,
    pub settled_barriers: Vec<BarrierKind>,
    pub timed_out_barriers: Vec<BarrierKind>,
    pub route: ObservedRoute,
    pub outcome: ActionResult,
    pub limitation: Option<String>,
}

impl FakeWorld {
    /// A world where the action's own required barriers all settle through
    /// real state, the pinned subject runs, and the outcome classifies as the
    /// first honest admitted result (`applied` where the vocabulary admits
    /// it, otherwise `not_proven` with a limitation).
    pub fn settling(action: &super::ActionSpec) -> Self {
        let generations = GenerationSnapshot {
            host_generation: 1,
            process_generation: 1,
            document_generation: 1,
            root_generation: 1,
            source_generation: 1,
            config_generation: 1,
        };
        let outcome = if action.allowed_results.contains(&ActionResult::Applied) {
            ActionResult::Applied
        } else {
            ActionResult::NotProven
        };
        let limitation =
            (outcome == ActionResult::NotProven).then(|| "no_landed_host_runner".to_string());
        Self {
            generations,
            settled_barriers: action.required_barriers.iter().map(|r| r.kind).collect(),
            timed_out_barriers: Vec::new(),
            route: default_route(action),
            outcome,
            limitation,
        }
    }
}

/// The lawful default route for an action class: a declared public surface
/// when one exists, else a declared native surface, else the class's own
/// stimulus/handoff channel.
fn default_route(action: &super::ActionSpec) -> ObservedRoute {
    match action.class {
        super::ActionClass::UserAction | super::ActionClass::Observation => {
            if let Some(api) = action.public_surfaces.first() {
                ObservedRoute::PublicClientApi { api: api.to_string() }
            } else if let Some(surface) = action.native_vim_surfaces.first() {
                ObservedRoute::NativeVimSurface { surface: surface.to_string() }
            } else {
                ObservedRoute::NativeVimSurface { surface: ":e".to_string() }
            }
        }
        super::ActionClass::TestStimulus => {
            ObservedRoute::TestStimulus { stimulus: "deliberate_stimulus".to_string() }
        }
        super::ActionClass::HostHandoff => {
            ObservedRoute::HostHandoff { handoff: "host_process_handoff".to_string() }
        }
    }
}

/// Build a typed observation from a scripted world for one action, honoring
/// the action's shape rules so the positive case validates.
pub fn observation_for(action: &super::ActionSpec, world: &FakeWorld) -> TypedObservation {
    let pinned = vim_vim_lsp_subject();
    let mut cardinalities: BTreeMap<String, u64> = BTreeMap::new();
    cardinalities.insert("replayed_buffers".to_string(), 1);
    cardinalities.insert("save_format_requests".to_string(), 1);
    let mut digests: BTreeMap<String, String> = BTreeMap::new();
    digests.insert("buffer_before".to_string(), fake_digest("buffer_before"));
    digests.insert("buffer_after".to_string(), fake_digest("buffer_after"));
    let shape = action.shape;
    let trigger = shape.requires_save_trigger.then_some(if shape.save_trigger_must_be_save_event {
        SaveTrigger::SaveEvent
    } else {
        SaveTrigger::ManualComparator
    });
    let detection_route = shape.expected_detection_route.or(Some(DetectionRoute::Native));
    let semantic_probe =
        (shape.requires_semantic_probe || shape.requires_provider_owner).then(|| SemanticProbe {
            probe_class: "hover".to_string(),
            provider_identity: "perllsp".to_string(),
            generation_scope: world.generations,
            result_digest: fake_digest("semantic_probe"),
        });
    let owner =
        (shape.requires_single_configured_owner || shape.requires_provider_owner).then(|| {
            OwnerIdentity {
                owner_class: if shape.requires_provider_owner
                    && !shape.requires_single_configured_owner
                {
                    "service_provider".to_string()
                } else {
                    "save_format_owner".to_string()
                },
                owner_token: "configured_owner".to_string(),
            }
        });
    let mut protocol_events = Vec::new();
    if shape.requires_generation_replay_sequence {
        for class in ["lsp_server_init", "lsp_buffer_enabled"] {
            protocol_events.push(super::observation::ProtocolEventDigest {
                event_class: class.to_string(),
                digest: fake_digest(class),
            });
        }
    }
    let barriers = action
        .required_barriers
        .iter()
        .map(|requirement| {
            if world.timed_out_barriers.contains(&requirement.kind) {
                BarrierEvidence::TimedOut {
                    kind: requirement.kind,
                    waited_ms: requirement.max_wait_ms,
                }
            } else {
                BarrierEvidence::Satisfied {
                    kind: requirement.kind,
                    settled_generations: world.generations,
                    waited_ms: 5,
                }
            }
        })
        .collect();
    TypedObservation {
        schema_version: super::DRIVER_SCHEMA_VERSION.to_string(),
        action_id: action.action_id.to_string(),
        backend: BackendIdentity::Fake,
        host_product: pinned.host_product,
        client_id: pinned.client_id,
        server_executable: pinned.server_executable,
        fixture: FixtureBinding {
            fixture_owners: vec!["vim-vim-lsp-subject.v1".to_string()],
            fixture_relative_paths: vec!["workspace/lib/main.pm".to_string()],
        },
        generations: world.generations,
        route: world.route.clone(),
        trigger,
        configured_owner_count: shape.requires_single_configured_owner.then_some(1),
        owner,
        semantic_probe,
        cardinalities,
        digests,
        barriers,
        protocol_events,
        process: ProcessDisposition::Running { generation: world.generations.process_generation },
        cleanup: CleanupLedger::Settled,
        session_iterations: shape.min_session_iterations,
        detection_route,
        outcome: world.outcome,
        limitation: world.limitation.clone(),
    }
}

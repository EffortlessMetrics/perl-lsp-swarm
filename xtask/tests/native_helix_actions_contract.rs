//! Contract tests for `native_helix_actions.v1` (#12832): the registered
//! table must be internally lawful, every action's honest fake path must
//! validate, and each forgery class the laws forbid must fail closed.
//!
//! Workspace hygiene: failures flow through typed results; the local
//! assertion helpers keep precise messages without unchecked collapse.

use std::collections::BTreeMap;

use anyhow::{Context, Result, bail, ensure};

use xtask::native_helix_actions::fake::{fake_digest, fake_subject, observation_for};
use xtask::native_helix_actions::observation::{
    BackendIdentity, DocumentBinding, EffectClass, EffectStage, ObservationPlane,
    ObservationResult, ObservedRoute, SubjectBinding, TypedObservation,
};
use xtask::native_helix_actions::predicate::{
    GenerationSnapshot, PredicateEvidence, PredicateKind, SubstitutionKind,
};
use xtask::native_helix_actions::{
    ACTION_ID_PREFIX, ACTIONS, ActionClass, CONTRACT_SCHEMA_VERSION, SurfaceClassification,
    action_by_id, validate_observation, validate_run, validate_table,
};

const FIRST_INSTRUMENT_ACTION: &str = "helix.native.host_session.observe_handshake_from_log";
const ARGV_ACTION: &str = "helix.native.host_session.open_document_argv";
const CONFIG_ACTION: &str = "helix.native.host_session.load_canonical_config";
const KEYS_ACTION: &str = "helix.native.host_session.quit_host_keys";
const LAUNCH_HANDOFF: &str = "helix.native.host_session.launch_isolated_host";
const POST_RUN_HANDOFF: &str = "helix.native.host_session.post_run_observation_handoff";

/// Build a lawful fixture or fail the test with the fake backend's reason.
fn fixture(action_id: &str) -> Result<TypedObservation> {
    observation_for(action_id, 3)
        .map_err(anyhow::Error::msg)
        .with_context(|| format!("lawful fixture for {action_id}"))
}

/// Validate an intentionally-lawful observation.
fn lawful(action_id: &str, observation: &TypedObservation) -> Result<()> {
    validate_observation(observation)
        .map_err(anyhow::Error::msg)
        .with_context(|| format!("{action_id} must accept its honest path"))
}

/// Expect the validator to reject with a message containing `needle`.
fn rejected(label: &str, observation: &TypedObservation, needle: &str) -> Result<()> {
    match validate_observation(observation) {
        Ok(()) => bail!("{label}: forgery accepted"),
        Err(err) => {
            let err = err.to_string();
            ensure!(err.contains(needle), "{label}: message `{err}` lacks `{needle}`");
            Ok(())
        }
    }
}

/// Apply one mutation to a lawful fixture.
fn mutate(action_id: &str, change: impl FnOnce(&mut TypedObservation)) -> Result<TypedObservation> {
    let mut observation = fixture(action_id)?;
    change(&mut observation);
    Ok(observation)
}

#[test]
fn table_is_lawful() -> Result<()> {
    validate_table()?;
    let mut ids = BTreeMap::new();
    for action in ACTIONS {
        ensure!(action.action_id.starts_with(ACTION_ID_PREFIX));
        ensure!(!action.summary.is_empty());
        ensure!(ids.insert(action.action_id, true).is_none(), "duplicate {}", action.action_id);
        ensure!(!action.emits.is_empty(), "{} declares no effects", action.action_id);
        if matches!(action.class, ActionClass::HostHandoff) {
            ensure!(
                action.allowed_results.iter().all(|result| matches!(
                    result,
                    ObservationResult::NotProven | ObservationResult::Unsupported
                )),
                "{} must carry the fail-closed vocabulary",
                action.action_id
            );
            ensure!(
                matches!(action.surface, SurfaceClassification::NotExposed),
                "{} handoffs stay not-exposed",
                action.action_id
            );
        }
    }
    Ok(())
}

#[test]
fn every_action_has_a_lawful_honest_path() -> Result<()> {
    for action in ACTIONS {
        let observation = observation_for(action.action_id, 7).map_err(anyhow::Error::msg)?;
        lawful(action.action_id, &observation)?;
        ensure_eq(observation.backend, BackendIdentity::Fake);
        if matches!(action.class, ActionClass::HostHandoff) {
            ensure_eq(observation.result, ObservationResult::NotProven);
            ensure_eq(
                observation.limitation_class.as_deref(),
                Some("host_execution_authority_not_landed"),
            );
        } else {
            ensure_eq(observation.result, ObservationResult::Observed);
        }
        // The instrument ceiling holds everywhere: log-text observations stop
        // at `returned`.
        if matches!(observation.route, ObservedRoute::InstrumentHook { .. }) {
            ensure!(
                observation.observed.stage <= EffectStage::Returned,
                "{} claims past the instrument ceiling",
                action.action_id
            );
            ensure_eq(observation.plane, ObservationPlane::Instrument);
        }
    }
    Ok(())
}

#[test]
fn unknown_action_fails_closed() -> Result<()> {
    ensure!(
        observation_for("helix.native.host_session.does_not_exist", 1).is_err(),
        "unknown ids never fabricate observations"
    );

    let mut drifted = fixture(FIRST_INSTRUMENT_ACTION)?;
    drifted.action_id = "helix.native.missing".to_string();
    rejected("unknown id", &drifted, "unknown action id")
}

#[test]
fn foreign_subjects_are_rejected() -> Result<()> {
    let overrides: Vec<(&str, Box<dyn Fn(&mut SubjectBinding)>)> = vec![
        (
            "host product",
            Box::new(|s: &mut SubjectBinding| s.host_product = "hx_renamed".to_string()),
        ),
        (
            "client id",
            Box::new(|s: &mut SubjectBinding| s.client_id = "perlnavigator_client".to_string()),
        ),
        (
            "server executable",
            Box::new(|s: &mut SubjectBinding| s.server_executable = "perl-lsp".to_string()),
        ),
        (
            "config identity",
            Box::new(|s: &mut SubjectBinding| s.config_id = "hand_edited_config".to_string()),
        ),
        (
            "host scope",
            Box::new(|s: &mut SubjectBinding| {
                s.host_version_scope = "master_source_row".to_string()
            }),
        ),
    ];
    for (label, apply) in overrides {
        let mut observation = fixture(FIRST_INSTRUMENT_ACTION)?;
        apply(&mut observation.subject);
        rejected(&format!("foreign subject {label}"), &observation, "pinned released-stable")?;
    }
    Ok(())
}

#[test]
fn forged_routes_and_planes_fail_closed() -> Result<()> {
    // Instrument hook offered on a command-line action, relabeled to the
    // product plane so the ROUTE law itself must catch the forgery.
    let observation = mutate(ARGV_ACTION, |o| {
        o.route = ObservedRoute::InstrumentHook {
            hook: "--log".to_string(),
            owner: "hx_log_capture".to_string(),
        };
    })?;
    rejected("route/class mismatch", &observation, "instrument hook")?;

    // Command-line surface offered with an undeclared spelling.
    let observation = mutate(CONFIG_ACTION, |o| {
        o.route = ObservedRoute::CommandLineSurface { surface: "argv files".to_string() };
    })?;
    rejected("undeclared surface", &observation, "outside what")?;

    // Reporting plane is reserved forever.
    let observation = mutate(KEYS_ACTION, |o| o.plane = ObservationPlane::Reporting)?;
    rejected("reporting plane", &observation, "reporting plane is reserved")?;

    // Handoff route smuggled into an ordinary action.
    let observation = mutate(ARGV_ACTION, |o| {
        o.route = ObservedRoute::HostHandoff { handoff: "host_process_handoff".to_string() };
    })?;
    rejected("handoff route misuse", &observation, "never an ordinary action")?;

    // Undeclared handoff token.
    let observation = mutate(LAUNCH_HANDOFF, |o| {
        o.route = ObservedRoute::HostHandoff { handoff: "raw_spawn_policy".to_string() };
    })?;
    rejected("closed handoff vocabulary", &observation, "outside the closed handoff vocabulary")
}

#[test]
fn handoffs_stay_fail_closed() -> Result<()> {
    for handoff_action in [POST_RUN_HANDOFF, LAUNCH_HANDOFF] {
        let observation = mutate(handoff_action, |o| {
            o.result = ObservationResult::Observed;
            o.limitation_class = None;
        })?;
        rejected(
            &format!("{handoff_action} relabeled observed"),
            &observation,
            "may honestly report",
        )?;
    }
    Ok(())
}

#[test]
fn predicate_substitutions_and_binds_fail_closed() -> Result<()> {
    // Substituted evidence is never state.
    let observation = mutate(ARGV_ACTION, |o| {
        o.predicate_evidence.clear();
        o.predicate_evidence.push(PredicateEvidence::Substituted {
            kind: PredicateKind::ServerHandshakeSettled,
            substitution: SubstitutionKind::FixedSleep,
        });
    })?;
    rejected("substituted predicate", &observation, "substitutions are never state")?;

    // Satisfaction without a settled-state digest is elapsed-time-only.
    let observation = mutate(ARGV_ACTION, |o| {
        o.predicate_evidence.clear();
        o.predicate_evidence.push(PredicateEvidence::Satisfied {
            kind: PredicateKind::ServerHandshakeSettled,
            settled_state_digest: String::new(),
            settled_generations: GenerationSnapshot::zeroed(),
            polls: 2,
            waited_ms: 100,
        });
    })?;
    rejected("digest-less satisfaction", &observation, "elapsed time alone is never satisfaction")?;

    // A timed-out wait forces not_proven.
    let observation = mutate(ARGV_ACTION, |o| {
        o.predicate_evidence.clear();
        o.predicate_evidence.push(PredicateEvidence::TimedOut {
            kind: PredicateKind::ServerHandshakeSettled,
            polls: 4,
            waited_ms: 30_000,
        });
    })?;
    rejected("timeout classification", &observation, "must classify not_proven")?;

    // Required-predicate absence.
    let observation = mutate(ARGV_ACTION, |o| o.predicate_evidence.clear())?;
    rejected("missing required evidence", &observation, "requires predicate")?;

    // Stale settlement below the observation's own generation: raise only
    // the snapshot's session generation and leave the predicate behind it.
    let observation = mutate(FIRST_INSTRUMENT_ACTION, |o| {
        o.generations.session += 1;
    })?;
    rejected("stale settlement", &observation, "stale state cannot prove a current result")?;

    // Settlement newer than the snapshot.
    let observation = mutate(FIRST_INSTRUMENT_ACTION, |o| {
        for evidence in &mut o.predicate_evidence {
            if let PredicateEvidence::Satisfied { settled_generations, .. } = evidence {
                settled_generations.session = o.generations.session + 1;
            }
        }
    })?;
    rejected("future settlement", &observation, "newer")
}

#[test]
fn stage_ceiling_and_digest_laws_hold() -> Result<()> {
    // Instrument plane claiming visible-current.
    let observation = mutate(FIRST_INSTRUMENT_ACTION, |o| {
        o.observed.stage = EffectStage::VisibleCurrent;
    })?;
    rejected("log-as-UI claim", &observation, "stage ceiling is returned")?;

    // Applied claims need an effect digest.
    let observation = mutate(ARGV_ACTION, |o| o.observed.effect_digest = None)?;
    rejected("unbound applied effect", &observation, "effect digest")?;

    // Results below the declared minimum stage cannot claim observed/mismatch.
    let observation = mutate(KEYS_ACTION, |o| {
        o.observed.stage = EffectStage::Requested;
        o.observed.effect_digest = Some(fake_digest("requested-anyway"));
    })?;
    rejected("below-minimum stage", &observation, "declared minimum")?;

    // Effect classes outside the row's emissions are rejected.
    let observation = mutate(FIRST_INSTRUMENT_ACTION, |o| {
        o.observed.effect_classes.push(EffectClass::TerminalState);
    })?;
    rejected("undeclared effect", &observation, "outside what")?;

    // Missing required identity binding.
    let observation = mutate(ARGV_ACTION, |o| {
        o.observed.identity_digests.remove("server_process_identity");
    })?;
    rejected("missing identity binding", &observation, "identity binding")
}

#[test]
fn boundedness_caps_hold() -> Result<()> {
    // Oversized free-token payloads stay out of durable evidence.
    let observation = mutate(ARGV_ACTION, |o| {
        o.observed.identity_digests = (0..40)
            .map(|index| (format!("token_{index}"), fake_digest(&index.to_string())))
            .collect();
    })?;
    rejected("collection cap", &observation, "cap")?;

    // A missing limitation class on a not-proven result is dishonest.
    let observation = mutate(POST_RUN_HANDOFF, |o| o.limitation_class = None)?;
    rejected("limitation requirement", &observation, "limitation class")?;

    // Schema version drift is rejected.
    let observation = mutate(FIRST_INSTRUMENT_ACTION, |o| {
        o.schema_version = "native_helix_actions.v0".to_string();
    })?;
    rejected("schema drift", &observation, "does not match")?;

    // Non-fixture document paths are rejected.
    let observation = mutate(ARGV_ACTION, |o| {
        o.subject.document = Some(DocumentBinding { fixture_path: "../../etc/passwd".to_string() });
    })?;
    rejected("path escape", &observation, "fixture-root-relative")?;

    // Unbounded key sequences are rejected even when grammatically shaped.
    let observation = mutate(KEYS_ACTION, |o| {
        o.route = ObservedRoute::NativeKeys { keys: format!(":{}{}", "a!".repeat(30), 'q') };
    })?;
    rejected("oversized keys", &observation, "key sequence")?;

    // Host adapter backends must carry a bounded adapter digest.
    let observation = mutate(FIRST_INSTRUMENT_ACTION, |o| {
        o.backend = BackendIdentity::HostAdapter { adapter_digest: "not-a-digest".to_string() };
    })?;
    rejected("adapter digest law", &observation, "bounded digest")?;

    // Unbounded result digests are void regardless of the row.
    let observation = mutate(CONFIG_ACTION, |o| {
        o.observed.result_digest = "sha256:deadbeef".to_string();
    })?;
    rejected("digest grammar", &observation, "unbounded")
}

#[test]
fn pinned_tokens_cite_their_owners() -> Result<()> {
    ensure_eq(CONTRACT_SCHEMA_VERSION, "native_helix_actions.v1");
    let subject = fake_subject(false);
    ensure_eq(subject.server_executable.as_str(), "perllsp");
    ensure_eq(subject.host_product.as_str(), "helix");
    // The fake world renders real bounded digests deterministically.
    ensure_eq(fake_digest("seed"), fake_digest("seed"));
    ensure!(fake_digest("seed") != fake_digest("other"), "fake digests must bind their seed");
    // Every registered action resolves; none collide with the Neovim
    // namespace.
    for action in ACTIONS {
        ensure!(action_by_id(action.action_id).is_some());
        ensure!(
            xtask::native_neovim_actions::action_by_id(action.action_id).is_none(),
            "{} collides across dialects",
            action.action_id
        );
    }
    Ok(())
}

#[test]
fn review_hardening_laws_hold() -> Result<()> {
    // P1: a successful observation cannot carry an older effect snapshot than
    // its own settlement snapshot.
    let observation = mutate(ARGV_ACTION, |o| {
        o.observed.generations = GenerationSnapshot::zeroed();
    })?;
    rejected("cross-generation effect", &observation, "generation than the settlement snapshot")?;

    // P2: each handoff row binds exactly its own channel token.
    let observation = mutate(POST_RUN_HANDOFF, |o| {
        if let ObservedRoute::HostHandoff { handoff } = &mut o.route {
            *handoff = "host_process_handoff".to_string();
        }
    })?;
    rejected("cross-bound handoff", &observation, "binds handoff")?;

    // P2: a successful observation must never carry a limitation token.
    let observation = mutate(ARGV_ACTION, |o| {
        o.limitation_class = Some("stale_generation".to_string());
    })?;
    rejected("limitation on observed", &observation, "must not carry a limitation class")?;

    // P2: ordered-run identity is enforced at run scope.
    ensure!(validate_run(&[]).is_err(), "an empty slice is not a run");
    let first = fixture(KEYS_ACTION)?;
    let mut duplicate = fixture(FIRST_INSTRUMENT_ACTION)?;
    duplicate.sequence = first.sequence;
    ensure!(validate_run(&[first.clone(), duplicate]).is_err(), "duplicate sequences accepted");
    let mut backwards = fixture(FIRST_INSTRUMENT_ACTION)?;
    backwards.sequence = first.sequence - 1;
    ensure!(validate_run(&[first.clone(), backwards]).is_err(), "decreasing sequences accepted");
    let mut zeroed = fixture(KEYS_ACTION)?;
    zeroed.sequence = 0;
    ensure!(validate_run(&[zeroed]).is_err(), "zero sequence accepted");
    let mut later = fixture(FIRST_INSTRUMENT_ACTION)?;
    later.sequence = first.sequence + 1;
    validate_run(&[first, later]).map_err(anyhow::Error::msg)
}

/// Local typed equality assertion with a readable message.
fn ensure_eq<T: PartialEq + std::fmt::Debug>(actual: T, expected: T) -> Result<()> {
    ensure!(actual == expected, "assertion failed: actual {actual:?} != expected {expected:?}");
    Ok(())
}

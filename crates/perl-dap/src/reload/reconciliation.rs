//! Session wiring and reconciliation for the loaded-module reload family
//! (reload train R03, #10102).
//!
//! This module binds the negotiated wire family ([`ReloadFamilySession`],
//! R01B #10138) into the debug session lifecycle and applies the frozen
//! contract's generation, invalidation, and reconciliation decisions:
//!
//! - one session-scoped [`ReloadSessionWiring`] per debug session
//!   lifecycle, bound to a session epoch that is replaced on restart
//!   (prior family and operation identities never survive it);
//! - [`ReloadSessionWiring::route_terminal`] routes every frozen terminal
//!   kind exactly: `reloaded` and `indeterminate_possibly_applied` advance
//!   the runtime-module generation through the single clock authority and
//!   invalidate exactly the composed table; refusals and pre-mutation
//!   failures move nothing and invalidate nothing;
//! - [`ReloadSessionWiring::reconcile_observation`] refuses
//!   stale-generation or replaced-epoch reconciliation claims with typed
//!   reasons, so late results from a previous generation can never become
//!   current;
//! - [`verify_reconciliation_claim`] fail-closes the forbidden shapes: a
//!   mutating outcome (in particular `indeterminate_possibly_applied`)
//!   projected as clean, a non-mutating outcome claiming invalidation, and
//!   any divergence from the frozen per-object-kind table.
//!
//! The runtime transaction itself is #10098's (R02 remainder) and is not
//! implemented here: version 1 has no mechanism backing, admitted requests
//! without a mechanism terminal refuse honestly, and every mutating
//! reconciliation is fail-closed about what it cannot reacquire
//! (`unavailable`), never clean. Thread references stay adapter-synthetic
//! projections (`ProjectionReprojected`) and durable desired breakpoint
//! configuration is preserved for reconciliation, never invalidated.

use super::generation::{RuntimeModuleGeneration, RuntimeModuleGenerationClock};
use super::invalidation::{
    InvalidationPlanError, ReloadInvalidationPlan, invalidation_plan_for, verify_invalidation_plan,
};
use super::transaction::LoadedModuleReloadOutcome;
use crate::reload_family::{
    ClientFamilyDeclaration, FamilyNegotiationRefusal, LoadedModuleReloadWireResponse,
    ReloadFamilySession, ReloadRequestEvaluation, WireReconciliation,
    WireReconciliationDisposition, project_outcome,
};
use std::collections::VecDeque;

/// Bound on retained completed reload operations per session wiring,
/// following the family registry's retained-operations precedent.
pub const MAX_RETAINED_COMPLETIONS: usize = 64;

/// Standard DAP `invalidated` event areas a terminal mutation outcome
/// actually invalidates. Inspection references (`stacks`, `variables`) and
/// applied breakpoint state (`breakpoints`) move; thread references are
/// adapter projections and are re-projected, never invalidated as runtime
/// facts, so no `threads` area is claimed.
pub const MUTATION_INVALIDATED_AREAS: [&str; 3] = ["breakpoints", "stacks", "variables"];

/// Why the session wiring refused an operation, in a closed typed
/// vocabulary of its own. These are composition-layer refusals, not wire
/// rejections and not transaction outcomes: the frozen
/// #10097/#10138 vocabularies are untouched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReloadWiringRefusal {
    /// The claim names a session epoch this wiring already replaced; prior
    /// identities never survive a restart.
    SessionEpochReplaced {
        /// The epoch the claim was minted under.
        claim_epoch: u64,
        /// The current epoch.
        current_epoch: u64,
    },
    /// The claim was minted under a runtime-module generation this session
    /// has already advanced past; late results from a previous generation
    /// cannot become current.
    GenerationSuperseded {
        /// The generation the claim was minted under.
        claim_generation: RuntimeModuleGeneration,
        /// The current generation.
        current_generation: RuntimeModuleGeneration,
    },
    /// The claim names a runtime-module generation this session has not
    /// reached; nothing has been observed under a future generation, so
    /// the claim is not current in either direction.
    GenerationAhead {
        /// The generation the claim was minted under.
        claim_generation: RuntimeModuleGeneration,
        /// The current generation.
        current_generation: RuntimeModuleGeneration,
    },
    /// No admitted, not-yet-terminal operation carries this identity.
    OperationNotAdmitted {
        /// The refused operation identity.
        operation_id: u64,
    },
    /// The operation already reached a terminal kind; a second terminal is
    /// a replay, not a new result.
    OperationAlreadyCompleted {
        /// The replayed operation identity.
        operation_id: u64,
    },
    /// The runtime-module generation is exhausted; no further mutation
    /// outcome can be distinguished, so the wiring fails closed instead of
    /// risking a reused generation.
    GenerationExhausted,
    /// A mutating outcome — in particular `indeterminate_possibly_applied`
    /// — was claimed with a clean (empty or `not_applicable`)
    /// reconciliation. The forbidden indeterminate-as-clean shape.
    MutationProjectedClean,
    /// The claimed reconciliation diverges from the frozen composed table
    /// or from the honest disposition derivation, carrying the frozen
    /// invalidation-plan error code.
    ComposedTableViolated(InvalidationPlanError),
}

impl ReloadWiringRefusal {
    /// Stable closed-vocabulary code (snake_case, bounded), for tests and
    /// bounded reason surfaces.
    pub const fn code(self) -> &'static str {
        match self {
            ReloadWiringRefusal::SessionEpochReplaced { .. } => "session_epoch_replaced",
            ReloadWiringRefusal::GenerationSuperseded { .. } => "generation_superseded",
            ReloadWiringRefusal::GenerationAhead { .. } => "generation_ahead",
            ReloadWiringRefusal::OperationNotAdmitted { .. } => "operation_not_admitted",
            ReloadWiringRefusal::OperationAlreadyCompleted { .. } => "operation_already_completed",
            ReloadWiringRefusal::GenerationExhausted => "generation_exhausted",
            ReloadWiringRefusal::MutationProjectedClean => "mutation_projected_clean",
            ReloadWiringRefusal::ComposedTableViolated(_) => "composed_table_violated",
        }
    }

    /// The frozen invalidation-plan error code when this refusal carries
    /// one.
    pub const fn invalidation_error(self) -> Option<InvalidationPlanError> {
        match self {
            ReloadWiringRefusal::ComposedTableViolated(error) => Some(error),
            _ => None,
        }
    }
}

/// A reconciliation claim's bind point against the wiring authorities.
///
/// Every state refresh or installation the composition performs after a
/// reload (module/source observation, breakpoint reconciliation result,
/// inspection query result) must carry the epoch and runtime-module
/// generation it was minted under; [`ReloadSessionWiring::
/// reconcile_observation`] refuses claims the session has moved past.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ObservationClaim {
    /// Session epoch at which the claim was minted.
    pub epoch: u64,
    /// Runtime-module generation at which the claim was minted.
    pub generation: RuntimeModuleGeneration,
}

/// One routed terminal: the wire response plus everything the debug
/// session must apply around it. The response is the only client-facing
/// artifact; the rest drives session-state invalidation and event
/// sequencing in the debug adapter wiring.
#[derive(Debug, Clone, PartialEq)]
pub struct RoutedReloadTerminal {
    /// The typed family response, ready to publish once session state has
    /// been invalidated and events emitted.
    pub response: LoadedModuleReloadWireResponse,
    /// The frozen composed invalidation table for this outcome (empty for
    /// non-mutating kinds).
    pub invalidation: ReloadInvalidationPlan,
    /// The reconciliation dispositions carried on the response body.
    pub reconciliation: WireReconciliation,
    /// Whether the terminal outcome mutated runtime code (both advancing
    /// kinds).
    pub mutated: bool,
}

impl RoutedReloadTerminal {
    /// The standard DAP `invalidated` event areas this terminal actually
    /// invalidates; empty for non-mutating kinds, exactly
    /// [`MUTATION_INVALIDATED_AREAS`] for mutating kinds.
    pub fn invalidated_areas(&self) -> &'static [&'static str] {
        if self.mutated { &MUTATION_INVALIDATED_AREAS } else { &[] }
    }
}

/// The honest reconciliation dispositions for one terminal outcome
/// (the R03 fill of the surface R01B registered).
///
/// - Non-mutating kinds (refusals, pre-mutation failures): `not_applicable`
///   everywhere — nothing was invalidated, old exact state remains valid,
///   and a clean disposition is honest precisely because nothing mutated.
/// - Mutating kinds (`reloaded`, `indeterminate_possibly_applied`):
///   inspection state `invalidated` per the composed table; desired
///   breakpoint configuration preserved and `pending`; loaded-source
///   refresh `unavailable` — without the #10098 mechanism read-back the
///   new exact source/module facts cannot be reacquired, and fail-closed
///   unavailability is the honest projection, never the old row.
pub fn reconciliation_dispositions_for(outcome: &LoadedModuleReloadOutcome) -> WireReconciliation {
    if outcome_is_mutating(outcome) {
        WireReconciliation {
            loaded_source_refresh: WireReconciliationDisposition::Unavailable,
            inspection_invalidation: WireReconciliationDisposition::Invalidated,
            breakpoint_reconciliation: WireReconciliationDisposition::Pending,
        }
    } else {
        WireReconciliation::all(WireReconciliationDisposition::NotApplicable)
    }
}

/// Whether the outcome is one of the two terminal mutation kinds.
pub fn outcome_is_mutating(outcome: &LoadedModuleReloadOutcome) -> bool {
    matches!(
        outcome,
        LoadedModuleReloadOutcome::Reloaded
            | LoadedModuleReloadOutcome::IndeterminatePossiblyApplied { .. }
    )
}

/// Verify a claimed reconciliation against the frozen semantics.
///
/// The discriminating seam for wrong candidates (and the machine-checked
/// statement of the fail-closed law): a mutating outcome — in particular
/// `indeterminate_possibly_applied` — claimed with a clean table or a
/// `not_applicable` disposition anywhere is
/// [`ReloadWiringRefusal::MutationProjectedClean`]; any divergence from
/// the frozen per-object-kind table or the honest disposition derivation
/// is [`ReloadWiringRefusal::ComposedTableViolated`] carrying the frozen
/// invalidation-plan error code; a non-mutating outcome may claim only the
/// empty table and `not_applicable` everywhere.
pub fn verify_reconciliation_claim(
    outcome: &LoadedModuleReloadOutcome,
    dispositions: &WireReconciliation,
    plan: &ReloadInvalidationPlan,
) -> Result<(), ReloadWiringRefusal> {
    if let Err(error) = verify_invalidation_plan(plan, outcome) {
        if outcome_is_mutating(outcome) && plan.is_empty() {
            // A mutating outcome claimed an empty (clean) table is the
            // forbidden projection, more specific than a generic
            // divergence.
            return Err(ReloadWiringRefusal::MutationProjectedClean);
        }
        return Err(ReloadWiringRefusal::ComposedTableViolated(error));
    }
    if *dispositions != reconciliation_dispositions_for(outcome) {
        if outcome_is_mutating(outcome)
            && dispositions
                == &WireReconciliation::all(WireReconciliationDisposition::NotApplicable)
        {
            // A mutating outcome claimed clean dispositions: the
            // indeterminate-as-clean shape (and its `reloaded` sibling).
            return Err(ReloadWiringRefusal::MutationProjectedClean);
        }
        return Err(ReloadWiringRefusal::ComposedTableViolated(
            InvalidationPlanError::StaleIdentitySurvivesPossiblyApplied,
        ));
    }
    Ok(())
}

/// One completed reload operation retained for replay rejection and
/// late-claim refusal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CompletedReload {
    operation_id: u64,
    terminal_kind: &'static str,
    generation: RuntimeModuleGeneration,
}

/// Session-scoped reload wiring: the negotiated family state, the session
/// epoch, and the admitted/completed operation bookkeeping that makes
/// late and replayed results refuse typed. The runtime-module generation
/// clock is injected per call (it lives on the debug session, resetting
/// only when the debuggee process is replaced); the epoch replaces with
/// the session lifecycle.
pub struct ReloadSessionWiring {
    epoch: u64,
    family: ReloadFamilySession,
    pending: VecDeque<u64>,
    completed: VecDeque<CompletedReload>,
}

impl ReloadSessionWiring {
    /// Fresh wiring for a session epoch with the given mechanism backing.
    /// Production version 1 constructs sessions unbacked: registration is
    /// not behavior and the reload mechanism (#10098) does not exist yet.
    pub fn new(epoch: u64, backed: bool) -> ReloadSessionWiring {
        ReloadSessionWiring {
            epoch,
            family: ReloadFamilySession::new(epoch, backed),
            pending: VecDeque::new(),
            completed: VecDeque::new(),
        }
    }

    /// The current session epoch.
    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    /// Whether the session has reload mechanism backing.
    pub fn is_backed(&self) -> bool {
        self.family.is_backed()
    }

    /// Negotiate the family against a client declaration (R01B rules:
    /// fail-closed for absent declarations, wrong family identities, and
    /// no overlapping version).
    pub fn negotiate(
        &mut self,
        declaration: Option<&ClientFamilyDeclaration>,
    ) -> Result<u32, FamilyNegotiationRefusal> {
        self.family.negotiate(declaration)
    }

    /// Evaluate one wire request through the R01B fail-closed gates. An
    /// admitted operation is recorded as pending until it reaches a
    /// terminal kind.
    pub fn evaluate(&mut self, raw: &serde_json::Value) -> ReloadRequestEvaluation {
        let evaluation = self.family.evaluate(raw);
        if let ReloadRequestEvaluation::Admitted { operation_id } = evaluation {
            self.pending.push_back(operation_id);
        }
        evaluation
    }

    /// Route one terminal outcome for an admitted operation, applying the
    /// generation clock exactly as the contract demands (advance for both
    /// mutating kinds, hold otherwise) and returning the composed
    /// invalidation table and reconciliation dispositions.
    ///
    /// Typed refusals: unknown operations ([`ReloadWiringRefusal::
    /// OperationNotAdmitted`] — a replaced session no longer admits prior
    /// identities), replays ([`ReloadWiringRefusal::
    /// OperationAlreadyCompleted`]), and an exhausted generation
    /// ([`ReloadWiringRefusal::GenerationExhausted`]) fail closed before
    /// the clock moves.
    pub fn route_terminal(
        &mut self,
        operation_id: u64,
        outcome: &LoadedModuleReloadOutcome,
        clock: &mut RuntimeModuleGenerationClock,
        reasons: &[String],
    ) -> Result<RoutedReloadTerminal, ReloadWiringRefusal> {
        if self.completed.iter().any(|completed| completed.operation_id == operation_id) {
            return Err(ReloadWiringRefusal::OperationAlreadyCompleted { operation_id });
        }
        let pending_position = self
            .pending
            .iter()
            .position(|pending| *pending == operation_id)
            .ok_or(ReloadWiringRefusal::OperationNotAdmitted { operation_id })?;
        if outcome_is_mutating(outcome) && clock.current().is_exhausted() {
            return Err(ReloadWiringRefusal::GenerationExhausted);
        }

        // Single clock authority: `project_outcome` applies the outcome to
        // the injected clock and fails closed on contract-invalid
        // phase/kind pairings before the clock can move.
        let response =
            project_outcome(outcome, operation_id, clock, reasons, None).map_err(|_| {
                ReloadWiringRefusal::ComposedTableViolated(
                    InvalidationPlanError::StaleIdentitySurvivesPossiblyApplied,
                )
            })?;

        let invalidation = invalidation_plan_for(outcome);
        let reconciliation = reconciliation_dispositions_for(outcome);
        // Fail closed on the derived pair (unreachable by construction;
        // kept so the routing can never silently diverge from the frozen
        // table it just published).
        verify_reconciliation_claim(outcome, &reconciliation, &invalidation)?;

        self.pending.remove(pending_position);
        self.completed.push_back(CompletedReload {
            operation_id,
            terminal_kind: outcome.kind_code(),
            generation: clock.current(),
        });
        if self.completed.len() > MAX_RETAINED_COMPLETIONS {
            self.completed.pop_front();
        }

        Ok(RoutedReloadTerminal {
            mutated: outcome_is_mutating(outcome),
            invalidation,
            reconciliation,
            response,
        })
    }

    /// Refuse reconciliation claims minted under an epoch this session
    /// replaced or a generation that is not the current one — late results
    /// from a previous generation cannot become current, and nothing has
    /// been observed under a generation the session has not reached. Only
    /// claims minted under the current authorities are admitted.
    pub fn reconcile_observation(
        &self,
        claim: &ObservationClaim,
        current_generation: RuntimeModuleGeneration,
    ) -> Result<(), ReloadWiringRefusal> {
        if claim.epoch != self.epoch {
            return Err(ReloadWiringRefusal::SessionEpochReplaced {
                claim_epoch: claim.epoch,
                current_epoch: self.epoch,
            });
        }
        if claim.generation < current_generation {
            return Err(ReloadWiringRefusal::GenerationSuperseded {
                claim_generation: claim.generation,
                current_generation,
            });
        }
        if claim.generation > current_generation {
            return Err(ReloadWiringRefusal::GenerationAhead {
                claim_generation: claim.generation,
                current_generation,
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reload::LoadedModuleReloadEligibility;
    use crate::reload::transaction::{
        IndeterminateCause, PreMutationFailureCause, ReloadTransactionPhase,
    };
    use crate::reload_family::{LOADED_MODULE_RELOAD_FAMILY, LOADED_MODULE_RELOAD_FAMILY_VERSION};
    use serde_json::Value;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    /// Route and surface a wiring refusal as a test error instead of
    /// panicking.
    fn routed(
        result: Result<RoutedReloadTerminal, ReloadWiringRefusal>,
        context: &str,
    ) -> Result<RoutedReloadTerminal, Box<dyn std::error::Error>> {
        result.map_err(|refusal| format!("{context}: {}", refusal.code()).into())
    }

    fn reloaded() -> LoadedModuleReloadOutcome {
        LoadedModuleReloadOutcome::Reloaded
    }

    fn indeterminate() -> LoadedModuleReloadOutcome {
        LoadedModuleReloadOutcome::IndeterminatePossiblyApplied {
            phase: ReloadTransactionPhase::RuntimeAcknowledgementReadBack,
            cause: IndeterminateCause::TimeoutAfterMutationBegan,
        }
    }

    fn refused_unsupported() -> LoadedModuleReloadOutcome {
        LoadedModuleReloadOutcome::Refused {
            disposition: LoadedModuleReloadEligibility::UnsupportedRuntime,
        }
    }

    fn failed_cancelled() -> LoadedModuleReloadOutcome {
        LoadedModuleReloadOutcome::FailedBeforeMutation {
            phase: ReloadTransactionPhase::Prepare,
            cause: PreMutationFailureCause::CancelledBeforeMutationBegan,
        }
    }

    fn failed_preflight() -> LoadedModuleReloadOutcome {
        LoadedModuleReloadOutcome::FailedBeforeMutation {
            phase: ReloadTransactionPhase::Preflight,
            cause: PreMutationFailureCause::PrepareFailed,
        }
    }

    fn negotiated_wiring(epoch: u64) -> ReloadSessionWiring {
        let mut wiring = ReloadSessionWiring::new(epoch, true);
        let declaration = ClientFamilyDeclaration {
            family: LOADED_MODULE_RELOAD_FAMILY.to_string(),
            versions: vec![LOADED_MODULE_RELOAD_FAMILY_VERSION],
        };
        assert_eq!(wiring.negotiate(Some(&declaration)), Ok(LOADED_MODULE_RELOAD_FAMILY_VERSION));
        wiring
    }

    fn request_value(operation_id: u64, epoch: u64) -> Value {
        serde_json::json!({
            "family": LOADED_MODULE_RELOAD_FAMILY,
            "familyVersion": LOADED_MODULE_RELOAD_FAMILY_VERSION,
            "sessionEpoch": epoch,
            "operationId": operation_id,
            "subject": {
                "moduleIdentity": "opaque-module-token-1a2b",
                "savedSourceDigest": "sha256:0f12e4d6a9b8c7d5e3f1a0b9c8d7e6f5a4b3c2d1e0f9a8b7c6d5e4f3a2b1c0d",
                "logicalSourceUri": "perl-lsp-subject:epoch=7;observation=3",
                "observationGeneration": 3
            },
            "deadlineMs": 5000
        })
    }

    fn admit(wiring: &mut ReloadSessionWiring, operation_id: u64) {
        let evaluation = wiring.evaluate(&request_value(operation_id, wiring.epoch()));
        assert_eq!(
            evaluation,
            ReloadRequestEvaluation::Admitted { operation_id },
            "the seeded request must be admitted for the routing cells"
        );
    }

    #[test]
    fn both_mutating_kinds_advance_the_generation_and_invalidate_exactly_the_composed_table()
    -> TestResult {
        for outcome in [reloaded(), indeterminate()] {
            let mut wiring = negotiated_wiring(7);
            admit(&mut wiring, 1);
            let mut clock = RuntimeModuleGenerationClock::new();
            let routed =
                routed(wiring.route_terminal(1, &outcome, &mut clock, &[]), outcome.kind_code())?;
            assert!(routed.mutated, "{} must route as mutating", outcome.kind_code());
            assert_eq!(clock.current(), RuntimeModuleGeneration::new(1));

            // Exactly the frozen composed table: full coverage, thread
            // references re-projected, durable configuration preserved.
            assert_eq!(routed.invalidation, invalidation_plan_for(&outcome));
            assert!(routed.invalidation.covers_every_kind());
            assert_eq!(
                routed.invalidated_areas(),
                &MUTATION_INVALIDATED_AREAS[..],
                "mutation invalidates exactly breakpoints/stacks/variables"
            );

            // The wire response: indeterminate never success; reloaded is.
            assert_eq!(
                routed.response.success,
                matches!(outcome, LoadedModuleReloadOutcome::Reloaded)
            );
            let crate::reload_family::LoadedModuleReloadResponseBody::Outcome(body) =
                &routed.response.body
            else {
                return Err("a terminal must project an outcome body".into());
            };
            assert_eq!(
                body.possibly_applied,
                matches!(outcome, LoadedModuleReloadOutcome::IndeterminatePossiblyApplied { .. })
            );
            let witness = body.generation.ok_or("terminal bodies carry a witness")?;
            assert!(witness.advanced);
            assert_eq!(witness.previous + 1, witness.current);
            assert_eq!(body.reconciliation, routed.reconciliation);
            assert_eq!(
                body.reconciliation,
                WireReconciliation {
                    loaded_source_refresh: WireReconciliationDisposition::Unavailable,
                    inspection_invalidation: WireReconciliationDisposition::Invalidated,
                    breakpoint_reconciliation: WireReconciliationDisposition::Pending,
                }
            );
        }
        Ok(())
    }

    #[test]
    fn non_mutating_cells_move_nothing_and_invalidate_nothing() -> TestResult {
        // The issue's no-movement cells: admission refusal, unsupported
        // runtime, preflight failure, cancellation-before-mutation, and a
        // pre-boundary timeout (a pre-mutation failure cause).
        let cells = [
            refused_unsupported(),
            LoadedModuleReloadOutcome::Refused {
                disposition: LoadedModuleReloadEligibility::OutsideLaunchAuthority,
            },
            failed_cancelled(),
            failed_preflight(),
            LoadedModuleReloadOutcome::FailedBeforeMutation {
                phase: ReloadTransactionPhase::Prepare,
                cause: PreMutationFailureCause::PrepareFailed,
            },
        ];
        for outcome in cells {
            let mut wiring = negotiated_wiring(1);
            admit(&mut wiring, 1);
            let mut clock = RuntimeModuleGenerationClock::new();
            let routed =
                routed(wiring.route_terminal(1, &outcome, &mut clock, &[]), outcome.kind_code())?;
            assert!(!routed.mutated);
            assert_eq!(clock.current(), RuntimeModuleGeneration::INITIAL);
            assert!(routed.invalidation.is_empty());
            assert!(routed.invalidated_areas().is_empty());
            assert!(!routed.response.success);
            assert_eq!(
                routed.reconciliation,
                WireReconciliation::all(WireReconciliationDisposition::NotApplicable)
            );
        }
        Ok(())
    }

    #[test]
    fn every_refusal_disposition_routes_without_movement() -> TestResult {
        use crate::reload::LoadedModuleReloadEligibility as Eligibility;
        let mut refused = 0;
        for disposition in
            Eligibility::ALL.into_iter().filter(|disposition| !disposition.is_admitted())
        {
            let mut wiring = negotiated_wiring(1);
            admit(&mut wiring, 1);
            let outcome = LoadedModuleReloadOutcome::Refused { disposition };
            let mut clock = RuntimeModuleGenerationClock::new();
            let routed = routed(wiring.route_terminal(1, &outcome, &mut clock, &[]), "refusal")?;
            assert!(!routed.mutated && !routed.response.success);
            assert_eq!(clock.current(), RuntimeModuleGeneration::INITIAL);
            refused += 1;
        }
        assert_eq!(refused, 12, "exactly the twelve refusal classes");
        Ok(())
    }

    #[test]
    fn second_reload_advances_to_another_distinct_generation() -> TestResult {
        let mut wiring = negotiated_wiring(1);
        admit(&mut wiring, 1);
        admit(&mut wiring, 2);
        let mut clock = RuntimeModuleGenerationClock::new();
        let first = routed(wiring.route_terminal(1, &reloaded(), &mut clock, &[]), "first")?;
        let second = routed(wiring.route_terminal(2, &indeterminate(), &mut clock, &[]), "second")?;
        assert_eq!(clock.current(), RuntimeModuleGeneration::new(2));
        assert!(first.response.success);
        assert!(!second.response.success, "an indeterminate terminal is never success");
        let crate::reload_family::LoadedModuleReloadResponseBody::Outcome(body) =
            &second.response.body
        else {
            return Err("an indeterminate terminal must project an outcome body".into());
        };
        assert!(body.possibly_applied);
        Ok(())
    }

    #[test]
    fn stale_generation_reconciliation_refuses_typed() -> TestResult {
        let mut wiring = negotiated_wiring(4);
        admit(&mut wiring, 1);
        let mut clock = RuntimeModuleGenerationClock::new();
        routed(wiring.route_terminal(1, &reloaded(), &mut clock, &[]), "seed")?;
        // Generation advanced to 1: a claim minted at the previous
        // generation refuses with the typed supersession reason.
        let stale = ObservationClaim { epoch: 4, generation: RuntimeModuleGeneration::INITIAL };
        assert_eq!(
            wiring.reconcile_observation(&stale, clock.current()),
            Err(ReloadWiringRefusal::GenerationSuperseded {
                claim_generation: RuntimeModuleGeneration::INITIAL,
                current_generation: RuntimeModuleGeneration::new(1),
            })
        );
        // A claim minted under the current generation is admitted.
        let current = ObservationClaim { epoch: 4, generation: clock.current() };
        assert_eq!(wiring.reconcile_observation(&current, clock.current()), Ok(()));
        // A claim minted under a generation the session has not reached
        // is not current either: nothing has been observed under a future
        // generation, so it refuses typed (review finding: future claims
        // must not pass as current.
        let ahead = ObservationClaim { epoch: 4, generation: clock.current().next() };
        assert_eq!(
            wiring.reconcile_observation(&ahead, clock.current()),
            Err(ReloadWiringRefusal::GenerationAhead {
                claim_generation: clock.current().next(),
                current_generation: clock.current(),
            })
        );
        Ok(())
    }

    #[test]
    fn replaced_epoch_reconciliation_refuses_typed() {
        let old = negotiated_wiring(4);
        let claim = ObservationClaim { epoch: 4, generation: RuntimeModuleGeneration::INITIAL };
        let replaced = ReloadSessionWiring::new(5, true);
        assert_eq!(
            replaced.reconcile_observation(&claim, RuntimeModuleGeneration::INITIAL),
            Err(ReloadWiringRefusal::SessionEpochReplaced { claim_epoch: 4, current_epoch: 5 })
        );
        // The old wiring still accepts its own epoch's claims.
        assert_eq!(old.reconcile_observation(&claim, RuntimeModuleGeneration::INITIAL), Ok(()));
    }

    #[test]
    fn replaced_session_operations_refuse_and_late_terminals_discard() {
        let mut wiring = negotiated_wiring(1);
        admit(&mut wiring, 1);
        // Session replacement: a fresh wiring at a new epoch does not know
        // the prior operation; its late terminal refuses typed and moves
        // no clock.
        let mut replaced = ReloadSessionWiring::new(2, true);
        let mut clock = RuntimeModuleGenerationClock::new();
        assert_eq!(
            replaced.route_terminal(1, &reloaded(), &mut clock, &[]),
            Err(ReloadWiringRefusal::OperationNotAdmitted { operation_id: 1 })
        );
        assert_eq!(clock.current(), RuntimeModuleGeneration::INITIAL);
    }

    #[test]
    fn replayed_terminal_refuses_typed() -> TestResult {
        let mut wiring = negotiated_wiring(1);
        admit(&mut wiring, 1);
        let mut clock = RuntimeModuleGenerationClock::new();
        routed(wiring.route_terminal(1, &reloaded(), &mut clock, &[]), "first")?;
        assert_eq!(
            wiring.route_terminal(1, &reloaded(), &mut clock, &[]),
            Err(ReloadWiringRefusal::OperationAlreadyCompleted { operation_id: 1 })
        );
        // The replay advanced nothing.
        assert_eq!(clock.current(), RuntimeModuleGeneration::new(1));
        Ok(())
    }

    #[test]
    fn exhausted_generation_fails_closed_before_the_clock_moves() {
        let mut wiring = negotiated_wiring(1);
        admit(&mut wiring, 1);
        let mut clock = RuntimeModuleGenerationClock::at_generation_for_test(
            RuntimeModuleGeneration::new(u64::MAX),
        );
        assert!(clock.current().is_exhausted());
        assert_eq!(
            wiring.route_terminal(1, &reloaded(), &mut clock, &[]),
            Err(ReloadWiringRefusal::GenerationExhausted)
        );
        // A non-advancing outcome still routes at exhaustion.
        assert_eq!(
            wiring
                .route_terminal(1, &refused_unsupported(), &mut clock, &[])
                .map(|routed| routed.mutated),
            Ok(false)
        );
    }

    #[test]
    fn indeterminate_as_clean_is_refused_by_claim_verification() {
        // The forbidden shapes: indeterminate with an empty table, and
        // indeterminate with clean dispositions.
        let honest = reconciliation_dispositions_for(&indeterminate());
        let empty = ReloadInvalidationPlan::from_dispositions(&[]);
        assert_eq!(
            verify_reconciliation_claim(&indeterminate(), &honest, &empty),
            Err(ReloadWiringRefusal::MutationProjectedClean)
        );
        let clean = WireReconciliation::all(WireReconciliationDisposition::NotApplicable);
        let table = invalidation_plan_for(&indeterminate());
        assert_eq!(
            verify_reconciliation_claim(&indeterminate(), &clean, &table),
            Err(ReloadWiringRefusal::MutationProjectedClean)
        );
        // The honest pair verifies.
        assert_eq!(verify_reconciliation_claim(&indeterminate(), &honest, &table), Ok(()));
        // The reloaded sibling of the same shape refuses identically.
        assert_eq!(
            verify_reconciliation_claim(&reloaded(), &clean, &invalidation_plan_for(&reloaded())),
            Err(ReloadWiringRefusal::MutationProjectedClean)
        );
    }

    #[test]
    fn divergent_claims_carry_the_frozen_invalidation_error_codes() -> TestResult {
        let table = invalidation_plan_for(&reloaded());
        let honest = reconciliation_dispositions_for(&reloaded());
        // Wrong table: durable configuration invalidated.
        let mut entries: Vec<_> = crate::reload::DapObjectKind::ALL
            .into_iter()
            .map(|kind| {
                let disposition = match kind {
                    crate::reload::DapObjectKind::DurableClientBreakpointConfiguration => {
                        crate::reload::InvalidationDisposition::AlwaysStale
                    }
                    other => table
                        .disposition_for(other)
                        .unwrap_or(crate::reload::InvalidationDisposition::AlwaysStale),
                };
                (kind, disposition)
            })
            .collect();
        let wrong = ReloadInvalidationPlan::from_dispositions(&entries);
        assert_eq!(
            verify_reconciliation_claim(&reloaded(), &honest, &wrong),
            Err(ReloadWiringRefusal::ComposedTableViolated(
                InvalidationPlanError::DurableConfigurationInvalidated
            ))
        );
        // Wrong table: thread references treated as runtime facts.
        entries.clear();
        for kind in crate::reload::DapObjectKind::ALL {
            let disposition = match kind {
                crate::reload::DapObjectKind::ThreadReference => {
                    crate::reload::InvalidationDisposition::AlwaysStale
                }
                other => table
                    .disposition_for(other)
                    .unwrap_or(crate::reload::InvalidationDisposition::AlwaysStale),
            };
            entries.push((kind, disposition));
        }
        let wrong_threads = ReloadInvalidationPlan::from_dispositions(&entries);
        assert_eq!(
            verify_reconciliation_claim(&reloaded(), &honest, &wrong_threads),
            Err(ReloadWiringRefusal::ComposedTableViolated(
                InvalidationPlanError::ThreadReferenceNotProjection
            ))
        );
        // A non-mutating outcome claiming a mutating table refuses with
        // the frozen stale-identity code.
        assert_eq!(
            verify_reconciliation_claim(
                &refused_unsupported(),
                &honest,
                &invalidation_plan_for(&reloaded())
            ),
            Err(ReloadWiringRefusal::ComposedTableViolated(
                InvalidationPlanError::StaleIdentitySurvivesPossiblyApplied
            ))
        );
        // A non-mutating outcome claiming non-clean dispositions refuses.
        assert_eq!(
            verify_reconciliation_claim(
                &failed_cancelled(),
                &honest,
                &ReloadInvalidationPlan::from_dispositions(&[])
            ),
            Err(ReloadWiringRefusal::ComposedTableViolated(
                InvalidationPlanError::StaleIdentitySurvivesPossiblyApplied
            ))
        );
        // The honest non-mutating pair verifies.
        assert_eq!(
            verify_reconciliation_claim(
                &failed_cancelled(),
                &reconciliation_dispositions_for(&failed_cancelled()),
                &ReloadInvalidationPlan::from_dispositions(&[])
            ),
            Ok(())
        );
        Ok(())
    }

    #[test]
    fn disposition_vocabulary_is_closed_and_matches_the_derivation() {
        assert_eq!(WireReconciliationDisposition::ALL.len(), 4);
        for disposition in WireReconciliationDisposition::ALL {
            assert_eq!(
                WireReconciliationDisposition::parse(disposition.as_str()),
                Some(disposition)
            );
        }
        assert_eq!(WireReconciliationDisposition::parse("deferred"), None);
        assert_eq!(
            WireReconciliationDisposition::parse("reacquired"),
            None,
            "reacquisition codes belong to the mechanism leaf, not R03"
        );
        // Defaults claim nothing.
        assert_eq!(
            WireReconciliation::default(),
            WireReconciliation::all(WireReconciliationDisposition::NotApplicable)
        );
    }

    #[test]
    fn an_unbacked_wiring_rejects_admission_with_the_typed_wire_code() -> TestResult {
        let mut wiring = ReloadSessionWiring::new(1, false);
        // Negotiation is available without backing; the backing gate
        // follows it in the registry precedence.
        let declaration = ClientFamilyDeclaration {
            family: LOADED_MODULE_RELOAD_FAMILY.to_string(),
            versions: vec![LOADED_MODULE_RELOAD_FAMILY_VERSION],
        };
        assert_eq!(wiring.negotiate(Some(&declaration)), Ok(LOADED_MODULE_RELOAD_FAMILY_VERSION));
        let evaluation = wiring.evaluate(&request_value(1, 1));
        match evaluation {
            ReloadRequestEvaluation::Response(response) => {
                assert!(!response.success);
                match response.body {
                    crate::reload_family::LoadedModuleReloadResponseBody::Rejected(rejection) => {
                        assert_eq!(
                            rejection.code.as_str(),
                            "family_not_backed_for_session",
                            "no mechanism backing is a typed rejection, never a terminal"
                        );
                    }
                    other => {
                        return Err(format!("expected a typed rejection body: {other:?}").into());
                    }
                }
            }
            admitted => {
                return Err(format!("an unbacked session must not admit: {admitted:?}").into());
            }
        }
        // Nothing was admitted, so nothing can reach a terminal.
        let mut clock = RuntimeModuleGenerationClock::new();
        assert_eq!(
            wiring.route_terminal(1, &reloaded(), &mut clock, &[]),
            Err(ReloadWiringRefusal::OperationNotAdmitted { operation_id: 1 })
        );
        Ok(())
    }

    #[test]
    fn wiring_refusal_codes_are_closed_and_bounded() {
        let samples = [
            ReloadWiringRefusal::SessionEpochReplaced { claim_epoch: 1, current_epoch: 2 },
            ReloadWiringRefusal::GenerationSuperseded {
                claim_generation: RuntimeModuleGeneration::INITIAL,
                current_generation: RuntimeModuleGeneration::new(1),
            },
            ReloadWiringRefusal::GenerationAhead {
                claim_generation: RuntimeModuleGeneration::new(2),
                current_generation: RuntimeModuleGeneration::new(1),
            },
            ReloadWiringRefusal::OperationNotAdmitted { operation_id: 9 },
            ReloadWiringRefusal::OperationAlreadyCompleted { operation_id: 9 },
            ReloadWiringRefusal::GenerationExhausted,
            ReloadWiringRefusal::MutationProjectedClean,
            ReloadWiringRefusal::ComposedTableViolated(
                InvalidationPlanError::StaleIdentitySurvivesPossiblyApplied,
            ),
        ];
        let codes: Vec<&str> = samples.iter().map(|refusal| refusal.code()).collect();
        assert_eq!(codes.len(), 8);
        for code in &codes {
            assert!(
                !code.is_empty()
                    && code
                        .chars()
                        .all(|character| { character.is_ascii_lowercase() || character == '_' }),
                "wiring refusal codes are bounded snake_case: {code}"
            );
        }
    }
}

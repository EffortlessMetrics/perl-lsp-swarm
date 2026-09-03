//! Runtime-module generation: the per-debuggee-process monotonic authority.
//!
//! `RuntimeModuleGeneration` is the module-reload analogue of the session's
//! `stopped_generation` suspension authority
//! (`debug_adapter/session.rs`, advanced fail-closed in
//! `debug_adapter/process.rs`): an opaque, monotonic counter that only ever
//! moves forward within one debuggee process. It advances on **both**
//! terminal mutation outcomes — `reloaded` and
//! `indeterminate_possibly_applied` — and never on refusals or pre-mutation
//! failures. It resets only when the debuggee process/session is replaced.
//! #10098/#10102 carry it on `DebugSession`; this module owns its meaning.

use super::transaction::LoadedModuleReloadOutcome;
use std::collections::VecDeque;

/// Opaque monotonic runtime-module generation of one debuggee process.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct RuntimeModuleGeneration(u64);

impl RuntimeModuleGeneration {
    /// Construct an opaque generation from a numeric value (the
    /// workspace-generation precedent shape; wire meaning, if any, is
    /// #10138's).
    pub const fn new(value: u64) -> RuntimeModuleGeneration {
        RuntimeModuleGeneration(value)
    }

    /// The initial generation of a fresh debuggee process.
    pub const INITIAL: RuntimeModuleGeneration = RuntimeModuleGeneration(0);

    /// Whether the generation counter is exhausted.
    ///
    /// At `u64::MAX` the counter can no longer distinguish further
    /// mutations; following the fail-closed ceiling precedent of
    /// `current_stopped_frame_id` (`debug_adapter/process.rs`), everything
    /// observed before exhaustion must be treated as stale rather than
    /// risking a reused generation.
    pub const fn is_exhausted(self) -> bool {
        self.0 == u64::MAX
    }

    /// Saturating successor; never rolls over and never reuses a value.
    pub const fn next(self) -> RuntimeModuleGeneration {
        RuntimeModuleGeneration(if self.0 == u64::MAX { u64::MAX } else { self.0 + 1 })
    }

    /// Distance from `self` to `other` if `other` is at least `self`.
    pub const fn distance_to(self, other: RuntimeModuleGeneration) -> Option<u64> {
        if other.0 >= self.0 { Some(other.0 - self.0) } else { None }
    }
}

/// Whether an outcome requires the runtime-module generation to advance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GenerationEffect {
    /// The generation must advance (terminal mutation outcomes).
    Advance,
    /// The generation must not advance (refusals, pre-mutation failures).
    None,
}

impl GenerationEffect {
    /// Stable closed-vocabulary code used by the `.spec` fixtures.
    pub const fn as_str(self) -> &'static str {
        match self {
            GenerationEffect::Advance => "advance",
            GenerationEffect::None => "none",
        }
    }
}

/// The result of applying one transaction outcome to the generation clock.
///
/// It carries both endpoints of the transition, not just the resulting
/// value, so that whoever reports the advance does not have to re-apply the
/// outcome — or hold the clock — to learn where the transaction started.
/// That is what keeps the clock single-owner: the component that applies it
/// hands this witness to the component that publishes it (#14550).
///
/// # Unforgeable by construction
///
/// The endpoints are private and there is no public constructor. The only
/// way to obtain a witness is [`RuntimeModuleGenerationClock::apply`], so a
/// caller cannot mint one describing a transition no clock performed — a
/// decreasing pair, a skipped generation, or an advance that never
/// happened. That matters because a publisher reports these endpoints
/// verbatim to a client: an opaque witness keeps "what the transaction did"
/// and "what the client is told" the same fact.
///
/// The witness also carries the operation identity as internal bookkeeping,
/// but the serialized witness remains `{previous, current, advanced}`. The
/// outcome and witness are kept together by [`ReloadExecution`], so an
/// operation ID may be reused after admission eviction without allowing an
/// earlier witness to be paired with the later outcome (#14670).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GenerationAdvance {
    previous: RuntimeModuleGeneration,
    current: RuntimeModuleGeneration,
    advanced: bool,
    operation: u64,
}

impl GenerationAdvance {
    /// The operation identity of the transaction that produced this witness.
    ///
    /// The identity is retained by the transaction-owned execution for
    /// correlation; the wire projector receives that execution as one value.
    #[cfg(test)]
    pub(crate) fn operation(self) -> u64 {
        self.operation
    }

    /// The generation after the advance attempt.
    pub fn generation(self) -> RuntimeModuleGeneration {
        self.current
    }

    /// The generation before the advance attempt.
    ///
    /// For an unchanged outcome this equals [`Self::generation`]; nothing
    /// moved, so the transaction both started and ended there.
    pub fn previous(self) -> RuntimeModuleGeneration {
        self.previous
    }

    /// Whether the generation moved.
    pub fn advanced(self) -> bool {
        self.advanced
    }

    /// Whether the endpoints describe one contiguous step of the clock.
    ///
    /// An advance moves to exactly the successor of where it started, and
    /// an unchanged outcome stays put. At the saturating ceiling the
    /// successor of `u64::MAX` is itself, so an exhausted advance reports
    /// equal endpoints and remains contiguous.
    ///
    /// [`RuntimeModuleGenerationClock::apply`] can only produce contiguous
    /// witnesses. This predicate exists so a publisher can state that
    /// invariant at its own boundary rather than assume it (#14550).
    pub fn is_contiguous(self) -> bool {
        if self.advanced {
            self.previous.next() == self.current
        } else {
            self.previous == self.current
        }
    }

    /// Stable closed-vocabulary code used by the `.spec` fixtures.
    pub fn code(self) -> &'static str {
        if self.advanced { "advance" } else { "none" }
    }

    /// Build a witness with arbitrary endpoints and identity.
    ///
    /// Test seam only, and deliberately the single hole in the opacity
    /// above: the fail-closed contiguity guard in the wire projector cannot
    /// be tested without a way to construct the malformed witnesses it
    /// exists to reject. Production code reaches a witness only through
    /// [`RuntimeModuleGenerationClock::apply`].
    #[cfg(test)]
    pub(crate) const fn forged(
        previous: RuntimeModuleGeneration,
        current: RuntimeModuleGeneration,
        advanced: bool,
        operation: u64,
    ) -> GenerationAdvance {
        GenerationAdvance { previous, current, advanced, operation }
    }
}

/// Per-debuggee-process monotonic clock for the runtime-module generation.
///
/// Fresh construction (`RuntimeModuleGenerationClock::new`) models a fresh
/// debuggee process/session; the generation resets with it and never
/// carries across process replacement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeModuleGenerationClock {
    current: RuntimeModuleGeneration,
}

impl Default for RuntimeModuleGenerationClock {
    fn default() -> Self {
        RuntimeModuleGenerationClock::new()
    }
}

impl RuntimeModuleGenerationClock {
    /// A fresh clock at the initial generation of a new debuggee process.
    pub const fn new() -> RuntimeModuleGenerationClock {
        RuntimeModuleGenerationClock { current: RuntimeModuleGeneration::INITIAL }
    }

    /// The current generation.
    pub const fn current(&self) -> RuntimeModuleGeneration {
        self.current
    }

    /// A clock positioned at an arbitrary generation.
    ///
    /// Test seam only. A production clock always starts fresh with its
    /// debuggee process and only ever moves through [`Self::apply`]; this
    /// exists so the exhaustion ceiling can be reached in a test without
    /// counting to `u64::MAX`. It adds no production behavior and changes
    /// no frozen semantics.
    #[cfg(test)]
    pub(crate) const fn at_generation(
        current: RuntimeModuleGeneration,
    ) -> RuntimeModuleGenerationClock {
        RuntimeModuleGenerationClock { current }
    }

    /// Apply one transaction outcome, advancing only for the two terminal
    /// mutation outcomes. Returns the resulting generation and whether it
    /// advanced.
    pub fn apply(
        &mut self,
        outcome: &LoadedModuleReloadOutcome,
        operation: u64,
    ) -> GenerationAdvance {
        match outcome.generation_effect() {
            GenerationEffect::Advance => {
                let previous = self.current;
                self.current = self.current.next();
                GenerationAdvance { previous, current: self.current, advanced: true, operation }
            }
            GenerationEffect::None => GenerationAdvance {
                previous: self.current,
                current: self.current,
                advanced: false,
                operation,
            },
        }
    }
}

/// Bound on retained per-generation observations, following the
/// `runtime_generation/core.rs` ring precedent (`MAX_OBSERVATIONS = 128`).
pub const MAX_RETAINED_OBSERVATIONS: usize = 128;

/// One retained observation of loaded-module state, stamped with the
/// runtime-module generation it was taken at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetainedModuleObservation {
    /// Generation at which the observation was taken.
    pub generation: RuntimeModuleGeneration,
    /// Runtime `%INC` key of the observed module.
    pub inc_key: String,
    /// Saved content digest observed at that generation.
    pub saved_content_digest: String,
}

/// Bounded ring of retained per-generation module observations.
///
/// Older observations are evicted beyond [`MAX_RETAINED_OBSERVATIONS`].
/// An observation is current only while its generation equals the current
/// generation; any advance (including an indeterminate advance) makes every
/// earlier observation stale.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetainedModuleObservations {
    ring: VecDeque<RetainedModuleObservation>,
}

impl Default for RetainedModuleObservations {
    fn default() -> Self {
        RetainedModuleObservations::new()
    }
}

impl RetainedModuleObservations {
    /// An empty bounded ring.
    pub const fn new() -> RetainedModuleObservations {
        RetainedModuleObservations { ring: VecDeque::new() }
    }

    /// Record an observation at the given generation, evicting the oldest
    /// entry beyond the bound.
    pub fn record(&mut self, observation: RetainedModuleObservation) {
        if self.ring.len() >= MAX_RETAINED_OBSERVATIONS {
            self.ring.pop_front();
        }
        self.ring.push_back(observation);
    }

    /// Number of retained observations.
    pub fn len(&self) -> usize {
        self.ring.len()
    }

    /// Whether nothing is retained.
    pub fn is_empty(&self) -> bool {
        self.ring.is_empty()
    }

    /// Whether the observation for `inc_key` taken at `generation` is still
    /// current: retained, and stamped at exactly the current generation.
    /// Fail closed — an unknown or evicted observation is stale.
    pub fn is_current(&self, inc_key: &str, current: RuntimeModuleGeneration) -> bool {
        self.ring
            .iter()
            .any(|observation| observation.inc_key == inc_key && observation.generation == current)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reload::LoadedModuleReloadEligibility;
    use crate::reload::transaction::{
        IndeterminateCause, PreMutationFailureCause, ReloadTransactionPhase,
    };

    /// Operation identity used by the clock tests in this module.
    ///
    /// These tests assert what the clock does to the counter, not how a
    /// witness is paired with an outcome on the wire; the ownership guard
    /// that consumes the identity is proven in `reload_family`.
    const OP: u64 = 1;

    fn reloaded() -> LoadedModuleReloadOutcome {
        LoadedModuleReloadOutcome::Reloaded
    }

    fn indeterminate() -> LoadedModuleReloadOutcome {
        LoadedModuleReloadOutcome::IndeterminatePossiblyApplied {
            phase: ReloadTransactionPhase::RuntimeAcknowledgementReadBack,
            cause: IndeterminateCause::TimeoutAfterMutationBegan,
        }
    }

    fn refused() -> LoadedModuleReloadOutcome {
        LoadedModuleReloadOutcome::Refused { disposition: LoadedModuleReloadEligibility::NotLoaded }
    }

    fn failed_before_mutation() -> LoadedModuleReloadOutcome {
        LoadedModuleReloadOutcome::FailedBeforeMutation {
            phase: ReloadTransactionPhase::Prepare,
            cause: PreMutationFailureCause::PrepareFailed,
        }
    }

    #[test]
    fn generation_is_monotonic_under_both_advancement_kinds() {
        let mut clock = RuntimeModuleGenerationClock::new();
        assert_eq!(clock.current(), RuntimeModuleGeneration::INITIAL);
        let reloaded_one = clock.apply(&reloaded(), OP);
        assert!(reloaded_one.advanced());
        let indeterminate_one = clock.apply(&indeterminate(), OP);
        assert!(indeterminate_one.advanced());
        let reloaded_two = clock.apply(&reloaded(), OP);
        let indeterminate_two = clock.apply(&indeterminate(), OP);
        // Monotonic: each observed value is strictly greater than the last.
        let values = [
            RuntimeModuleGeneration::INITIAL,
            reloaded_one.generation(),
            indeterminate_one.generation(),
            reloaded_two.generation(),
            indeterminate_two.generation(),
        ];
        assert!(values.windows(2).all(|pair| pair[0] < pair[1]));
    }

    #[test]
    fn refusals_and_pre_mutation_failures_advance_nothing() {
        let mut clock = RuntimeModuleGenerationClock::new();
        assert!(!clock.apply(&refused(), OP).advanced());
        assert!(!clock.apply(&failed_before_mutation(), OP).advanced());
        assert_eq!(clock.current(), RuntimeModuleGeneration::INITIAL);
        // A post-boundary timeout after a success still advances.
        assert!(clock.apply(&reloaded(), OP).advanced());
        assert!(!clock.apply(&refused(), OP).advanced());
        assert!(clock.apply(&indeterminate(), OP).advanced());
        assert_eq!(RuntimeModuleGeneration::INITIAL.distance_to(clock.current()), Some(2));
    }

    #[test]
    fn a_timeout_after_mutation_never_leaves_the_old_generation_current() {
        let mut clock = RuntimeModuleGenerationClock::new();
        let before = clock.current();
        let advanced = clock.apply(&indeterminate(), OP);
        assert_eq!(advanced.code(), "advance");
        assert!(before < clock.current());
    }

    /// The advance witness is self-describing: whoever publishes it can
    /// name both endpoints without re-applying the outcome or holding the
    /// clock. That is what lets the transaction stay the only applier
    /// (#14550).
    #[test]
    fn the_advance_witness_carries_both_endpoints() {
        let mut clock = RuntimeModuleGenerationClock::new();

        let advanced = clock.apply(&reloaded(), OP);
        assert!(advanced.advanced());
        assert_eq!(advanced.previous(), RuntimeModuleGeneration::INITIAL);
        assert_eq!(advanced.generation(), clock.current());
        assert_eq!(
            advanced.previous().distance_to(advanced.generation()),
            Some(1),
            "one outcome spends exactly one generation"
        );

        // Nothing moved: the transaction started and ended in the same
        // generation, so both endpoints report it.
        let unchanged = clock.apply(&refused(), OP);
        assert!(!unchanged.advanced());
        assert_eq!(unchanged.previous(), unchanged.generation());
        assert_eq!(unchanged.generation(), clock.current());
    }

    #[test]
    fn exhaustion_never_reuses_a_generation() {
        let exhausted = RuntimeModuleGeneration(u64::MAX);
        assert!(exhausted.is_exhausted());
        assert_eq!(exhausted.next(), exhausted);
        let near = RuntimeModuleGeneration(u64::MAX - 1);
        assert!(!near.is_exhausted());
        assert!(near.next().is_exhausted());
    }

    #[test]
    fn retained_observations_are_bounded_and_fail_closed() {
        let mut retained = RetainedModuleObservations::new();
        let mut clock = RuntimeModuleGenerationClock::new();
        clock.apply(&reloaded(), OP);
        retained.record(RetainedModuleObservation {
            generation: clock.current(),
            inc_key: "App/Core.pm".to_string(),
            saved_content_digest: "sha256:9f2c".to_string(),
        });
        assert!(retained.is_current("App/Core.pm", clock.current()));
        assert!(!retained.is_current("App/Core.pm", RuntimeModuleGeneration::INITIAL));
        assert!(!retained.is_current("Other.pm", clock.current()));
        // Any advance invalidates every earlier observation.
        clock.apply(&indeterminate(), OP);
        assert!(!retained.is_current("App/Core.pm", clock.current()));
        // Bound: recording beyond the cap evicts the oldest.
        for index in 0..(MAX_RETAINED_OBSERVATIONS + 8) {
            retained.record(RetainedModuleObservation {
                generation: clock.current(),
                inc_key: format!("Mod{index}.pm"),
                saved_content_digest: "sha256:0".to_string(),
            });
        }
        assert_eq!(retained.len(), MAX_RETAINED_OBSERVATIONS);
        assert!(!retained.is_current("Mod0.pm", clock.current()));
        assert!(
            retained
                .is_current(&format!("Mod{}.pm", MAX_RETAINED_OBSERVATIONS + 7), clock.current())
        );
    }
}

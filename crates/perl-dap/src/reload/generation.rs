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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenerationAdvance {
    /// The generation advanced to the given value.
    Advanced(RuntimeModuleGeneration),
    /// The generation is unchanged at the given value.
    Unchanged(RuntimeModuleGeneration),
}

impl GenerationAdvance {
    /// The generation after the advance attempt.
    pub fn generation(self) -> RuntimeModuleGeneration {
        match self {
            GenerationAdvance::Advanced(generation) => generation,
            GenerationAdvance::Unchanged(generation) => generation,
        }
    }

    /// Whether the generation moved.
    pub fn advanced(self) -> bool {
        matches!(self, GenerationAdvance::Advanced(_))
    }

    /// Stable closed-vocabulary code used by the `.spec` fixtures.
    pub fn code(self) -> &'static str {
        match self {
            GenerationAdvance::Advanced(_) => "advance",
            GenerationAdvance::Unchanged(_) => "none",
        }
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

    /// A clock positioned at an arbitrary generation (exhaustion and
    /// saturation proof only; production clocks always start at
    /// [`RuntimeModuleGenerationClock::new`]).
    #[cfg(test)]
    pub(crate) fn at_generation_for_test(generation: RuntimeModuleGeneration) -> Self {
        RuntimeModuleGenerationClock { current: generation }
    }

    /// The current generation.
    pub const fn current(&self) -> RuntimeModuleGeneration {
        self.current
    }

    /// Apply one transaction outcome, advancing only for the two terminal
    /// mutation outcomes. Returns the resulting generation and whether it
    /// advanced.
    pub fn apply(&mut self, outcome: &LoadedModuleReloadOutcome) -> GenerationAdvance {
        match outcome.generation_effect() {
            GenerationEffect::Advance => {
                self.current = self.current.next();
                GenerationAdvance::Advanced(self.current)
            }
            GenerationEffect::None => GenerationAdvance::Unchanged(self.current),
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
        let reloaded_one = clock.apply(&reloaded());
        assert!(reloaded_one.advanced());
        let indeterminate_one = clock.apply(&indeterminate());
        assert!(indeterminate_one.advanced());
        let reloaded_two = clock.apply(&reloaded());
        let indeterminate_two = clock.apply(&indeterminate());
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
        assert!(!clock.apply(&refused()).advanced());
        assert!(!clock.apply(&failed_before_mutation()).advanced());
        assert_eq!(clock.current(), RuntimeModuleGeneration::INITIAL);
        // A post-boundary timeout after a success still advances.
        assert!(clock.apply(&reloaded()).advanced());
        assert!(!clock.apply(&refused()).advanced());
        assert!(clock.apply(&indeterminate()).advanced());
        assert_eq!(RuntimeModuleGeneration::INITIAL.distance_to(clock.current()), Some(2));
    }

    #[test]
    fn a_timeout_after_mutation_never_leaves_the_old_generation_current() {
        let mut clock = RuntimeModuleGenerationClock::new();
        let before = clock.current();
        let advanced = clock.apply(&indeterminate());
        assert_eq!(advanced.code(), "advance");
        assert!(before < clock.current());
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
        clock.apply(&reloaded());
        retained.record(RetainedModuleObservation {
            generation: clock.current(),
            inc_key: "App/Core.pm".to_string(),
            saved_content_digest: "sha256:9f2c".to_string(),
        });
        assert!(retained.is_current("App/Core.pm", clock.current()));
        assert!(!retained.is_current("App/Core.pm", RuntimeModuleGeneration::INITIAL));
        assert!(!retained.is_current("Other.pm", clock.current()));
        // Any advance invalidates every earlier observation.
        clock.apply(&indeterminate());
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

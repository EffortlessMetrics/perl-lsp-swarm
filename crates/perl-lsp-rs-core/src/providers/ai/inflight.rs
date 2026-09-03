//! Live concurrency ceiling for AI backend requests (`#8300`).
//!
//! # Why this is not the rate limiter
//!
//! [`super::rate_limiter::RateLimiter`] is a token bucket: it bounds how many
//! requests may *start* per second. Its burst allowance is a refill quantity,
//! not a live-request ceiling — several requests can each take a token and then
//! remain active at the same time, because a token is consumed at dispatch and
//! never returned.
//!
//! `maxInflight` is documented as the maximum *concurrent* AI requests. That is
//! a different invariant and needs a different control: a permit that is held
//! for the complete request/stream lifetime and returned when the request
//! settles. This module owns that control.
//!
//! # Why admission never waits
//!
//! [`InflightGate::try_acquire`] either admits immediately or refuses. It has
//! no blocking or queueing mode, deliberately.
//!
//! `InlineCompletionBackend::stream` is synchronous and runs on the LSP's
//! shared read-worker pool, which has four slots for *every* read-only request
//! — hover, definition, references, diagnostics. A request parked waiting for
//! an AI permit would hold one of those four slots while doing nothing, so a
//! saturated AI gate would degrade unrelated editor features. Bounding remote
//! concurrency by blocking the server is the problem `maxInflight` exists to
//! prevent, not a way to enforce it.
//!
//! Refusal is cheap and already handled: the caller falls back to deterministic
//! completions. Admitting a wait here would need a non-blocking admission path
//! that does not occupy a read worker, which this seam cannot express.
//!
//! # Release contract
//!
//! [`InflightPermit`] releases in `Drop`, so every terminal path releases
//! without the caller remembering to: success, early `?` return, transport or
//! provider error, timeout, cancellation, output rejection, and panic unwind.
//! The lock is never held across caller code, so a caller cannot deadlock the
//! gate; the lock helper additionally recovers from poisoning so a panic
//! elsewhere can never permanently strand capacity.
//!
//! # Generation ownership
//!
//! A gate belongs to one backend/profile generation. Reconfiguring the AI
//! profile builds a new provider with a new gate, so permits from the old
//! generation drain into the old gate and never constrain or leak into the new
//! one.

use std::sync::{Mutex, MutexGuard, PoisonError};

/// A point-in-time snapshot of gate activity.
///
/// Deliberately numeric only: no prompt, source, completion text, endpoint, or
/// credential material may be derived from these counters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct InflightCounters {
    /// Permits currently held.
    pub active: u32,
    /// Highest [`Self::active`] observed since construction.
    pub peak_active: u32,
    /// Permits granted.
    pub admitted: u64,
    /// Admissions refused because the gate was saturated.
    pub saturated_rejections: u64,
    /// Permits returned.
    pub released: u64,
}

#[derive(Debug, Default)]
struct GateState {
    active: u32,
    peak_active: u32,
    admitted: u64,
    saturated_rejections: u64,
    released: u64,
}

/// A live-request ceiling shared by every caller of one backend generation.
#[derive(Debug)]
pub struct InflightGate {
    capacity: u32,
    state: Mutex<GateState>,
}

impl InflightGate {
    /// Create a gate admitting at most `capacity` simultaneous requests.
    ///
    /// A capacity of zero would disable AI completion entirely rather than
    /// bound it, which is a configuration mistake rather than an intent, so it
    /// is raised to one.
    pub fn new(capacity: u32) -> Self {
        Self { capacity: capacity.max(1), state: Mutex::new(GateState::default()) }
    }

    /// The configured ceiling.
    pub fn capacity(&self) -> u32 {
        self.capacity
    }

    /// Current counters.
    pub fn counters(&self) -> InflightCounters {
        let state = self.lock();
        InflightCounters {
            active: state.active,
            peak_active: state.peak_active,
            admitted: state.admitted,
            saturated_rejections: state.saturated_rejections,
            released: state.released,
        }
    }

    /// Take a slot if one is free, or refuse immediately.
    ///
    /// The permit is held until the returned guard is dropped. `None` means the
    /// ceiling is reached; see the module docs for why this never waits.
    pub fn try_acquire(&self) -> Option<InflightPermit<'_>> {
        let mut state = self.lock();
        if state.active >= self.capacity {
            state.saturated_rejections += 1;
            return None;
        }
        state.active += 1;
        state.admitted += 1;
        state.peak_active = state.peak_active.max(state.active);
        Some(InflightPermit { gate: self })
    }

    /// Lock the state, recovering a poisoned guard.
    ///
    /// No caller code runs while this lock is held, so poisoning can only come
    /// from an unrelated panic. Recovering rather than propagating means such a
    /// panic cannot permanently strand the ceiling and disable AI completion
    /// for the rest of the process.
    fn lock(&self) -> MutexGuard<'_, GateState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn release(&self) {
        let mut state = self.lock();
        state.active = state.active.saturating_sub(1);
        state.released += 1;
    }
}

/// Proof that one live-request slot is held.
///
/// Releasing is `Drop`, so it also happens during panic unwind.
#[derive(Debug)]
pub struct InflightPermit<'gate> {
    gate: &'gate InflightGate,
}

impl Drop for InflightPermit<'_> {
    fn drop(&mut self) {
        self.gate.release();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
    use std::sync::{Arc, Barrier};
    use std::time::{Duration, Instant};

    #[test]
    fn admits_up_to_capacity_and_then_refuses() {
        let gate = InflightGate::new(2);
        let first = gate.try_acquire();
        let second = gate.try_acquire();
        assert!(first.is_some());
        assert!(second.is_some());

        assert!(
            gate.try_acquire().is_none(),
            "a third concurrent request must not be admitted at capacity 2"
        );
        assert_eq!(gate.counters().active, 2);
        assert_eq!(gate.counters().peak_active, 2);
        assert_eq!(gate.counters().saturated_rejections, 1);
    }

    #[test]
    fn dropping_a_permit_frees_the_slot() {
        let gate = InflightGate::new(1);
        {
            let _permit = gate.try_acquire();
            assert!(gate.try_acquire().is_none());
        }
        assert_eq!(
            gate.counters().released,
            1,
            "leaving the scope must release exactly one permit"
        );
        assert_eq!(gate.counters().active, 0);
        assert!(
            gate.try_acquire().is_some(),
            "the slot must be reusable once the first permit is dropped"
        );
    }

    #[test]
    fn refusal_is_immediate_rather_than_a_wait() {
        let gate = InflightGate::new(1);
        let _held = gate.try_acquire();

        let started = Instant::now();
        assert!(gate.try_acquire().is_none());
        assert!(
            started.elapsed() < Duration::from_millis(50),
            "a saturated gate must refuse now, never park an LSP read worker"
        );
    }

    #[test]
    fn capacity_zero_is_raised_to_one_rather_than_disabling_completion() {
        let gate = InflightGate::new(0);
        assert_eq!(gate.capacity(), 1);
        assert!(gate.try_acquire().is_some());
    }

    /// The invariant the issue actually names: with `maxInflight = 1`, two
    /// threads must never hold a permit at the same time.
    ///
    /// Threads rendezvous on a barrier so they contend for real, then retry
    /// until admitted so every request eventually runs and the observed peak
    /// covers all of them.
    #[test]
    fn barrier_proves_capacity_one_never_runs_two_requests_at_once() {
        let gate = Arc::new(InflightGate::new(1));
        let start = Arc::new(Barrier::new(4));
        let concurrent = Arc::new(AtomicU32::new(0));
        let max_seen = Arc::new(AtomicU32::new(0));

        let handles: Vec<_> = (0..4)
            .map(|_| {
                let gate = Arc::clone(&gate);
                let start = Arc::clone(&start);
                let concurrent = Arc::clone(&concurrent);
                let max_seen = Arc::clone(&max_seen);
                std::thread::spawn(move || {
                    start.wait();
                    let deadline = Instant::now() + Duration::from_secs(10);
                    while Instant::now() < deadline {
                        let Some(permit) = gate.try_acquire() else {
                            std::thread::yield_now();
                            continue;
                        };
                        let now = concurrent.fetch_add(1, Ordering::SeqCst) + 1;
                        max_seen.fetch_max(now, Ordering::SeqCst);
                        std::thread::sleep(Duration::from_millis(20));
                        concurrent.fetch_sub(1, Ordering::SeqCst);
                        drop(permit);
                        return;
                    }
                })
            })
            .collect();

        // Propagate worker failures: `let _ = join()` would discard a panicked
        // assertion inside a thread and let the test pass regardless.
        for handle in handles {
            assert!(handle.join().is_ok(), "a worker thread panicked");
        }

        assert_eq!(
            max_seen.load(Ordering::SeqCst),
            1,
            "capacity 1 must never allow two simultaneously active requests"
        );
        let counters = gate.counters();
        assert_eq!(counters.active, 0, "every permit must be released");
        assert_eq!(counters.admitted, 4);
        assert_eq!(counters.released, 4);
        assert_eq!(counters.peak_active, 1);
    }

    /// The N+1 case: capacity N admits N and refuses the (N+1)th.
    #[test]
    fn barrier_proves_capacity_n_never_exceeds_n() {
        const N: u32 = 3;
        let gate = Arc::new(InflightGate::new(N));
        let concurrent = Arc::new(AtomicU32::new(0));
        let max_seen = Arc::new(AtomicU32::new(0));
        let contender_done = Arc::new(AtomicBool::new(false));

        // Roles are separated deliberately. An earlier shape spawned N+1 equal
        // threads and let them race: whether the extra caller met a full gate
        // depended on it arriving before any holder released, which a fixed
        // sleep only made *likely*. Here the holders provably still hold when
        // the contender tries, so the refusal is a property of capacity rather
        // than of timing.
        let holders: Vec<_> = (0..N)
            .map(|index| {
                let gate = Arc::clone(&gate);
                let concurrent = Arc::clone(&concurrent);
                let max_seen = Arc::clone(&max_seen);
                let contender_done = Arc::clone(&contender_done);
                std::thread::spawn(move || {
                    // Bind the permit: a temporary would drop immediately and
                    // the occupancy below would read zero.
                    let permit = gate.try_acquire();
                    assert!(permit.is_some(), "holder {index} must be admitted at capacity {N}");
                    let now = concurrent.fetch_add(1, Ordering::SeqCst) + 1;
                    max_seen.fetch_max(now, Ordering::SeqCst);

                    // Hold until the contender has had its turn. Bounded, so a
                    // gate that wrongly refuses a holder fails the assertions
                    // below instead of hanging the suite.
                    let deadline = Instant::now() + Duration::from_secs(5);
                    while !contender_done.load(Ordering::SeqCst) && Instant::now() < deadline {
                        std::thread::yield_now();
                    }

                    concurrent.fetch_sub(1, Ordering::SeqCst);
                    drop(permit);
                })
            })
            .collect();

        // The (N+1)th caller, on this thread: wait until every permit is held,
        // then attempt admission against a provably full gate.
        let deadline = Instant::now() + Duration::from_secs(5);
        while concurrent.load(Ordering::SeqCst) < N && Instant::now() < deadline {
            std::thread::yield_now();
        }
        assert_eq!(
            concurrent.load(Ordering::SeqCst),
            N,
            "all {N} holders must be inside before the contender tries"
        );
        let refused = u32::from(gate.try_acquire().is_none());
        contender_done.store(true, Ordering::SeqCst);

        // Propagate holder failures: `let _ = join()` would discard the
        // admission assertion inside a holder thread, so a gate that refused a
        // holder could still reach the counter assertions below.
        for handle in holders {
            assert!(handle.join().is_ok(), "a holder thread panicked");
        }

        // The ceiling, in both directions. `max_seen <= N` alone is vacuous:
        // a gate that refused every caller would report 0 and pass. The
        // equality below, plus the admission/refusal counts, pin the behavior
        // from both sides — an always-refuse gate fails `admitted`, an
        // always-admit gate fails `saturated_rejections` and `peak_active`.
        let observed = max_seen.load(Ordering::SeqCst);
        assert_eq!(observed, N, "capacity {N} must be reached and never exceeded, saw {observed}");
        assert_eq!(refused, 1, "the (N+1)th caller must be refused by a full gate");

        let counters = gate.counters();
        assert_eq!(counters.admitted, u64::from(N), "exactly N callers must be admitted");
        assert_eq!(
            counters.saturated_rejections, 1,
            "the gate must record the single saturated refusal"
        );
        assert_eq!(counters.released, u64::from(N), "every admitted permit must be released");
        assert_eq!(counters.peak_active, N, "the gate's own peak must equal capacity");
        assert_eq!(counters.active, 0);
    }

    /// A panicking request must not strand its slot: `Drop` runs during unwind.
    #[test]
    fn permit_is_released_when_the_holder_panics() {
        let gate = Arc::new(InflightGate::new(1));

        let gate_for_unwind = Arc::clone(&gate);
        let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            let _permit = gate_for_unwind.try_acquire();
            panic!("simulated panic while holding a permit");
        }));

        assert!(unwound.is_err(), "the test must actually observe a panic");
        assert_eq!(
            gate.counters().active,
            0,
            "a panic while holding a permit must still release the slot"
        );
        assert!(
            gate.try_acquire().is_some(),
            "capacity must remain usable after a panicking request"
        );
    }

    #[test]
    fn counters_report_peak_and_return_to_zero() {
        let gate = InflightGate::new(3);
        {
            let _a = gate.try_acquire();
            let _b = gate.try_acquire();
            assert_eq!(gate.counters().active, 2);
        }
        let counters = gate.counters();
        assert_eq!(counters.active, 0);
        assert_eq!(counters.peak_active, 2, "peak must survive the release");
        assert_eq!(counters.admitted, 2);
        assert_eq!(counters.released, 2);
    }

    /// Separate generations must not share or strand capacity.
    #[test]
    fn a_new_generation_gate_is_independent_of_the_old_one() {
        let old = Arc::new(InflightGate::new(1));
        let held = old.try_acquire();
        assert!(held.is_some());

        // Profile replacement: a new provider builds a new gate.
        let new = InflightGate::new(1);
        assert!(
            new.try_acquire().is_some(),
            "a permit outstanding on the old generation must not block the new one"
        );

        drop(held);
        assert_eq!(old.counters().active, 0, "the old permit drains into the old gate");
    }
}

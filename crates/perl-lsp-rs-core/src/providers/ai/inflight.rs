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
//! # Release contract
//!
//! [`InflightPermit`] releases in `Drop`, so every terminal path releases
//! without the caller remembering to: success, early `?` return, transport or
//! provider error, timeout, cancellation, output rejection, and panic unwind.
//! The gate's own mutex is never held across a request, so a panicking request
//! cannot poison it; the lock helper additionally recovers from poisoning so a
//! panic elsewhere can never permanently strand capacity.
//!
//! # Generation ownership
//!
//! A gate belongs to one backend/profile generation. Reconfiguring the AI
//! profile builds a new provider with a new gate, so permits from the old
//! generation drain into the old gate and never constrain or leak into the new
//! one.

use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

/// How long a bounded wait sleeps before re-checking the cancellation probe.
///
/// The wait is already bounded by its own budget; this only decides how
/// promptly a cancelled request stops waiting.
const CANCEL_POLL_INTERVAL: Duration = Duration::from_millis(25);

/// What a caller wants to happen when the gate is already saturated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdmissionPolicy {
    /// Fail immediately rather than wait.
    ///
    /// Automatic (as-you-type) completion uses this: queueing ghost-text
    /// requests behind remote work produces suggestions for a cursor position
    /// the user has already left, so a saturated gate should fall back or
    /// return no result now.
    Immediate,
    /// Wait up to `budget` for a permit, honoring cancellation.
    ///
    /// Explicitly invoked completion uses this: the user asked for a result and
    /// is willing to wait briefly.
    BoundedWait {
        /// Maximum time to wait before reporting saturation.
        budget: Duration,
    },
}

/// Why admission failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdmissionError {
    /// The concurrency ceiling was reached and the policy did not admit waiting
    /// (or the wait budget expired).
    Saturated,
    /// The caller was cancelled while waiting for a permit.
    CancelledWaiting,
}

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
    /// Waits abandoned because the caller was cancelled.
    pub cancelled_waiting: u64,
    /// Permits returned.
    pub released: u64,
}

#[derive(Debug, Default)]
struct GateState {
    active: u32,
    peak_active: u32,
    admitted: u64,
    saturated_rejections: u64,
    cancelled_waiting: u64,
    released: u64,
}

/// A live-request ceiling shared by every caller of one backend generation.
#[derive(Debug)]
pub struct InflightGate {
    capacity: u32,
    state: Mutex<GateState>,
    released: Condvar,
}

impl InflightGate {
    /// Create a gate admitting at most `capacity` simultaneous requests.
    ///
    /// A capacity of zero would disable AI completion entirely rather than
    /// bound it, which is a configuration mistake rather than an intent, so it
    /// is raised to one.
    pub fn new(capacity: u32) -> Self {
        Self {
            capacity: capacity.max(1),
            state: Mutex::new(GateState::default()),
            released: Condvar::new(),
        }
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
            cancelled_waiting: state.cancelled_waiting,
            released: state.released,
        }
    }

    /// Acquire a permit, held until the returned guard is dropped.
    ///
    /// `is_cancelled` is polled while waiting so a cancelled request stops
    /// waiting instead of occupying the queue for its whole budget. It is not
    /// consulted on the fast path: a request that can start immediately is not
    /// this gate's business to cancel.
    pub fn acquire(
        &self,
        policy: AdmissionPolicy,
        is_cancelled: &dyn Fn() -> bool,
    ) -> Result<InflightPermit<'_>, AdmissionError> {
        let mut state = self.lock();

        if Self::try_admit(&mut state, self.capacity) {
            return Ok(InflightPermit { gate: self });
        }

        let budget = match policy {
            AdmissionPolicy::Immediate => {
                state.saturated_rejections += 1;
                return Err(AdmissionError::Saturated);
            }
            AdmissionPolicy::BoundedWait { budget } => budget,
        };

        let deadline = Instant::now() + budget;
        loop {
            if is_cancelled() {
                state.cancelled_waiting += 1;
                return Err(AdmissionError::CancelledWaiting);
            }

            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                state.saturated_rejections += 1;
                return Err(AdmissionError::Saturated);
            }

            let slice = remaining.min(CANCEL_POLL_INTERVAL);
            let (next, _timed_out) =
                self.released.wait_timeout(state, slice).unwrap_or_else(PoisonError::into_inner);
            state = next;

            if Self::try_admit(&mut state, self.capacity) {
                return Ok(InflightPermit { gate: self });
            }
        }
    }

    /// Take a slot if one is free, recording admission and peak occupancy.
    fn try_admit(state: &mut GateState, capacity: u32) -> bool {
        if state.active >= capacity {
            return false;
        }
        state.active += 1;
        state.admitted += 1;
        state.peak_active = state.peak_active.max(state.active);
        true
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
        drop(state);
        self.released.notify_one();
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

    fn never_cancelled() -> impl Fn() -> bool {
        || false
    }

    #[test]
    fn admits_up_to_capacity_and_then_refuses_immediately() {
        let gate = InflightGate::new(2);
        let first = gate.acquire(AdmissionPolicy::Immediate, &never_cancelled());
        let second = gate.acquire(AdmissionPolicy::Immediate, &never_cancelled());
        assert!(first.is_ok());
        assert!(second.is_ok());

        assert_eq!(
            gate.acquire(AdmissionPolicy::Immediate, &never_cancelled()).err(),
            Some(AdmissionError::Saturated),
            "a third concurrent request must not be admitted at capacity 2"
        );
        assert_eq!(gate.counters().active, 2);
        assert_eq!(gate.counters().peak_active, 2);
    }

    #[test]
    fn dropping_a_permit_frees_the_slot() {
        let gate = InflightGate::new(1);
        {
            let _permit = gate.acquire(AdmissionPolicy::Immediate, &never_cancelled());
            assert!(gate.acquire(AdmissionPolicy::Immediate, &never_cancelled()).is_err());
        }
        assert_eq!(
            gate.counters().released,
            1,
            "leaving the scope must release exactly one permit"
        );
        assert_eq!(gate.counters().active, 0);
        assert!(
            gate.acquire(AdmissionPolicy::Immediate, &never_cancelled()).is_ok(),
            "the slot must be reusable once the first permit is dropped"
        );
    }

    #[test]
    fn capacity_zero_is_raised_to_one_rather_than_disabling_completion() {
        let gate = InflightGate::new(0);
        assert_eq!(gate.capacity(), 1);
        assert!(gate.acquire(AdmissionPolicy::Immediate, &never_cancelled()).is_ok());
    }

    /// The invariant the issue actually names: with `maxInflight = 1`, two
    /// threads must never hold a permit at the same time.
    ///
    /// Both threads rendezvous on a barrier so they contend for real, then each
    /// records the peak occupancy it observed while holding its permit.
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
                    // Wait rather than fail fast so every thread eventually runs
                    // and the observed peak covers all four requests.
                    let permit = gate.acquire(
                        AdmissionPolicy::BoundedWait { budget: Duration::from_secs(5) },
                        &|| false,
                    );
                    let Ok(permit) = permit else { return };
                    let now = concurrent.fetch_add(1, Ordering::SeqCst) + 1;
                    max_seen.fetch_max(now, Ordering::SeqCst);
                    std::thread::sleep(Duration::from_millis(20));
                    concurrent.fetch_sub(1, Ordering::SeqCst);
                    drop(permit);
                })
            })
            .collect();

        for handle in handles {
            let _ = handle.join();
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
        let start = Arc::new(Barrier::new((N + 1) as usize));
        let concurrent = Arc::new(AtomicU32::new(0));
        let max_seen = Arc::new(AtomicU32::new(0));
        let refused = Arc::new(AtomicU32::new(0));

        let handles: Vec<_> = (0..=N)
            .map(|_| {
                let gate = Arc::clone(&gate);
                let start = Arc::clone(&start);
                let concurrent = Arc::clone(&concurrent);
                let max_seen = Arc::clone(&max_seen);
                let refused = Arc::clone(&refused);
                std::thread::spawn(move || {
                    start.wait();
                    match gate.acquire(AdmissionPolicy::Immediate, &|| false) {
                        Ok(permit) => {
                            let now = concurrent.fetch_add(1, Ordering::SeqCst) + 1;
                            max_seen.fetch_max(now, Ordering::SeqCst);
                            std::thread::sleep(Duration::from_millis(40));
                            concurrent.fetch_sub(1, Ordering::SeqCst);
                            drop(permit);
                        }
                        Err(_) => {
                            refused.fetch_add(1, Ordering::SeqCst);
                        }
                    }
                })
            })
            .collect();

        for handle in handles {
            let _ = handle.join();
        }

        assert!(
            max_seen.load(Ordering::SeqCst) <= N,
            "capacity {N} must never be exceeded, saw {}",
            max_seen.load(Ordering::SeqCst)
        );
        assert_eq!(gate.counters().active, 0);
    }

    #[test]
    fn bounded_wait_gives_up_when_the_budget_expires() {
        let gate = InflightGate::new(1);
        let _held = gate.acquire(AdmissionPolicy::Immediate, &never_cancelled());

        let started = Instant::now();
        let outcome = gate.acquire(
            AdmissionPolicy::BoundedWait { budget: Duration::from_millis(80) },
            &never_cancelled(),
        );

        assert_eq!(outcome.err(), Some(AdmissionError::Saturated));
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "the wait must be bounded by its budget, not open-ended"
        );
        assert_eq!(gate.counters().saturated_rejections, 1);
    }

    #[test]
    fn bounded_wait_stops_early_when_the_caller_is_cancelled() {
        let gate = InflightGate::new(1);
        let _held = gate.acquire(AdmissionPolicy::Immediate, &never_cancelled());

        let cancelled = AtomicBool::new(true);
        let outcome = gate
            .acquire(AdmissionPolicy::BoundedWait { budget: Duration::from_secs(30) }, &|| {
                cancelled.load(Ordering::SeqCst)
            });

        assert_eq!(
            outcome.err(),
            Some(AdmissionError::CancelledWaiting),
            "a cancelled caller must not keep waiting for its whole budget"
        );
        assert_eq!(gate.counters().cancelled_waiting, 1);
    }

    #[test]
    fn bounded_wait_succeeds_once_a_permit_is_returned() {
        let gate = Arc::new(InflightGate::new(1));
        let held = gate.acquire(AdmissionPolicy::Immediate, &never_cancelled());
        assert!(held.is_ok());

        let waiter = {
            let gate = Arc::clone(&gate);
            std::thread::spawn(move || {
                gate.acquire(
                    AdmissionPolicy::BoundedWait { budget: Duration::from_secs(5) },
                    &|| false,
                )
                .is_ok()
            })
        };

        std::thread::sleep(Duration::from_millis(50));
        drop(held);

        assert!(waiter.join().unwrap_or(false), "the waiter must be admitted after the release");
    }

    /// A panicking request must not strand its slot: `Drop` runs during unwind.
    #[test]
    fn permit_is_released_when_the_holder_panics() {
        let gate = Arc::new(InflightGate::new(1));

        let gate_for_panic = Arc::clone(&gate);
        let result = std::panic::catch_unwind(move || {
            let _permit = gate_for_panic
                .acquire(AdmissionPolicy::Immediate, &|| false)
                .map_err(|_| "gate should admit the first request")?;
            Err::<(), &str>("simulated request failure")
        });

        // The closure returns Err rather than unwinding; also force a real
        // unwind to cover the panic path itself.
        assert!(result.is_ok());
        assert_eq!(gate.counters().active, 0, "the slot must be free after an early return");

        let gate_for_unwind = Arc::clone(&gate);
        let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            let _permit = gate_for_unwind.acquire(AdmissionPolicy::Immediate, &|| false);
            panic!("simulated panic while holding a permit");
        }));

        assert!(unwound.is_err(), "the test must actually observe a panic");
        assert_eq!(
            gate.counters().active,
            0,
            "a panic while holding a permit must still release the slot"
        );
        assert!(
            gate.acquire(AdmissionPolicy::Immediate, &never_cancelled()).is_ok(),
            "capacity must remain usable after a panicking request"
        );
    }

    #[test]
    fn counters_report_peak_and_return_to_zero() {
        let gate = InflightGate::new(3);
        {
            let _a = gate.acquire(AdmissionPolicy::Immediate, &never_cancelled());
            let _b = gate.acquire(AdmissionPolicy::Immediate, &never_cancelled());
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
        let held = old.acquire(AdmissionPolicy::Immediate, &never_cancelled());
        assert!(held.is_ok());

        // Profile replacement: a new provider builds a new gate.
        let new = InflightGate::new(1);
        assert!(
            new.acquire(AdmissionPolicy::Immediate, &never_cancelled()).is_ok(),
            "a permit outstanding on the old generation must not block the new one"
        );

        drop(held);
        assert_eq!(old.counters().active, 0, "the old permit drains into the old gate");
    }
}

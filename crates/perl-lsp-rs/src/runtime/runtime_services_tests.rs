//! Red-first falsifiers for `RuntimeServices` (#10024, "R01").
//!
//! These exercise the settlement API directly (registration, terminal
//! retention, shutdown aggregation, and the framework/application
//! separation) plus the forwarded `LspServer` accessors that must keep
//! behaving byte-for-byte identically to the pre-#10024 direct field
//! access.

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use std::time::{Duration, Instant};

    use super::super::LspServer;
    use super::super::diagnostic_debounce::DiagnosticDebouncer;
    use super::super::file_watcher_debounce::FileWatcherDebouncer;
    use super::super::runtime_services::{
        ApplicationShutdown, ApplicationTaskClass, DuplicateRegistration, RuntimeServices,
        ShutdownReason, TaskTerminal, UnregisteredTask,
    };

    #[test]
    fn register_then_complete_terminal_is_retained_in_settlement_snapshot() {
        let services = RuntimeServices::new();
        services
            .register_task(ApplicationTaskClass::ParseWorker)
            .expect("first registration succeeds");
        services
            .record_terminal(ApplicationTaskClass::ParseWorker, TaskTerminal::Completed)
            .expect("registered class accepts a terminal");

        let snapshot = services.settlement_snapshot();
        assert!(snapshot.pending.is_empty());
        assert_eq!(
            snapshot.settled,
            vec![(ApplicationTaskClass::ParseWorker, TaskTerminal::Completed)]
        );
    }

    #[test]
    fn duplicate_registration_is_rejected_and_first_identity_survives() {
        let services = RuntimeServices::new();
        services
            .register_task(ApplicationTaskClass::DiagnosticDebounce)
            .expect("first registration succeeds");
        services
            .record_terminal(
                ApplicationTaskClass::DiagnosticDebounce,
                TaskTerminal::Failed { reason: "first registration's own failure".to_string() },
            )
            .expect("registered class accepts a terminal");

        let duplicate = services.register_task(ApplicationTaskClass::DiagnosticDebounce);
        assert_eq!(duplicate, Err(DuplicateRegistration));

        // Negative control: the rejected duplicate must not silently reset
        // or replace the first registration's retained terminal.
        let snapshot = services.settlement_snapshot();
        assert_eq!(
            snapshot.settled,
            vec![(
                ApplicationTaskClass::DiagnosticDebounce,
                TaskTerminal::Failed { reason: "first registration's own failure".to_string() }
            )]
        );
    }

    #[test]
    fn record_terminal_for_an_unregistered_class_is_rejected() {
        let services = RuntimeServices::new();
        let result = services
            .record_terminal(ApplicationTaskClass::FileWatcherDebounce, TaskTerminal::Completed);
        assert_eq!(result, Err(UnregisteredTask));
        assert!(services.settlement_snapshot().settled.is_empty());
    }

    #[test]
    fn a_non_clean_terminal_is_never_overwritten_by_a_later_clean_terminal() {
        let cases = [
            (
                ApplicationTaskClass::ParseWorker,
                TaskTerminal::Failed { reason: "parse worker crashed".to_string() },
            ),
            (
                ApplicationTaskClass::DiagnosticDebounce,
                TaskTerminal::InstrumentFailed { reason: "never spawned".to_string() },
            ),
        ];
        for (class, first) in cases {
            let services = RuntimeServices::new();
            services.register_task(class).expect("registration succeeds");
            services.record_terminal(class, first.clone()).expect("first terminal accepted");
            services
                .record_terminal(class, TaskTerminal::Completed)
                .expect("a later record_terminal call is still accepted, but must not win");

            let snapshot = services.settlement_snapshot();
            assert_eq!(snapshot.settled, vec![(class, first)]);
        }
    }

    #[test]
    fn an_accepted_terminal_is_never_rewritten_by_a_later_accepted_terminal() {
        // A settled lifetime keeps its FIRST causal outcome. Without this,
        // the retained terminal -- and the typed reason a caller reads off
        // it -- would depend on which caller happened to run last, despite
        // this type retaining one terminal per lifetime.
        let cases = [
            // Completed must not decay into Cancelled.
            (
                ApplicationTaskClass::ParseWorker,
                TaskTerminal::Completed,
                TaskTerminal::Cancelled { reason: ShutdownReason::ServerDrop },
            ),
            // A cancellation must not be re-attributed to a different cause.
            (
                ApplicationTaskClass::DiagnosticDebounce,
                TaskTerminal::Cancelled { reason: ShutdownReason::ClientShutdownRequest },
                TaskTerminal::Cancelled { reason: ShutdownReason::ServerDrop },
            ),
            // Nor may a cancellation be laundered into a clean completion.
            (
                ApplicationTaskClass::FileWatcherDebounce,
                TaskTerminal::Cancelled { reason: ShutdownReason::ClientShutdownRequest },
                TaskTerminal::Completed,
            ),
        ];
        for (class, first, later) in cases {
            let services = RuntimeServices::new();
            services.register_task(class).expect("registration succeeds");
            services.record_terminal(class, first.clone()).expect("first terminal accepted");
            services
                .record_terminal(class, later)
                .expect("a later record_terminal call is still accepted, but must not win");

            let snapshot = services.settlement_snapshot();
            assert_eq!(snapshot.settled, vec![(class, first)]);
        }
    }

    #[test]
    fn a_fault_still_dominates_an_already_accepted_terminal() {
        // The negative control for the test above: preserving accepted
        // terminals must not also freeze out a real failure discovered
        // afterwards, which would reintroduce false-clean settlement.
        let class = ApplicationTaskClass::ParseWorker;
        let services = RuntimeServices::new();
        services.register_task(class).expect("registration succeeds");
        services
            .record_terminal(class, TaskTerminal::Cancelled { reason: ShutdownReason::ServerDrop })
            .expect("first terminal accepted");

        let fault =
            TaskTerminal::Failed { reason: "worker died after the stop request".to_string() };
        services.record_terminal(class, fault.clone()).expect("terminal accepted");

        let snapshot = services.settlement_snapshot();
        assert_eq!(snapshot.settled, vec![(class, fault)]);
    }

    #[test]
    fn instrument_failed_class_blocks_a_complete_shutdown() {
        let services = RuntimeServices::new();
        services
            .register_task(ApplicationTaskClass::FileWatcherDebounce)
            .expect("registration succeeds");
        services
            .record_terminal(
                ApplicationTaskClass::FileWatcherDebounce,
                TaskTerminal::InstrumentFailed {
                    reason: "no worker thread ever spawned".to_string(),
                },
            )
            .expect("terminal accepted");

        let outcome = services.begin_application_shutdown(
            ShutdownReason::ServerDrop,
            Instant::now() + Duration::from_millis(50),
        );
        assert_eq!(outcome, ApplicationShutdown::InstrumentFailed);
        assert_ne!(outcome, ApplicationShutdown::Complete);
    }

    #[test]
    fn shutdown_reports_complete_only_when_every_registered_class_settled_cleanly() {
        // All-clean settlement resolves to Complete.
        let services = RuntimeServices::new();
        services.register_task(ApplicationTaskClass::ParseWorker).expect("registration succeeds");
        services
            .register_task(ApplicationTaskClass::DiagnosticDebounce)
            .expect("registration succeeds");
        services
            .record_terminal(ApplicationTaskClass::ParseWorker, TaskTerminal::Completed)
            .expect("terminal accepted");
        services
            .record_terminal(ApplicationTaskClass::DiagnosticDebounce, TaskTerminal::Completed)
            .expect("terminal accepted");
        assert_eq!(
            services.begin_application_shutdown(
                ShutdownReason::ClientShutdownRequest,
                Instant::now() + Duration::from_millis(50)
            ),
            ApplicationShutdown::Complete
        );

        // One Failed class among otherwise-clean settlement is never
        // reported as Complete.
        let services = RuntimeServices::new();
        services.register_task(ApplicationTaskClass::ParseWorker).expect("registration succeeds");
        services
            .register_task(ApplicationTaskClass::DiagnosticDebounce)
            .expect("registration succeeds");
        services
            .record_terminal(ApplicationTaskClass::ParseWorker, TaskTerminal::Completed)
            .expect("terminal accepted");
        services
            .record_terminal(
                ApplicationTaskClass::DiagnosticDebounce,
                TaskTerminal::Failed { reason: "publish failed".to_string() },
            )
            .expect("terminal accepted");
        assert_eq!(
            services.begin_application_shutdown(
                ShutdownReason::ClientShutdownRequest,
                Instant::now() + Duration::from_millis(50)
            ),
            ApplicationShutdown::Failed
        );
    }

    #[test]
    fn a_registered_class_with_no_retained_terminal_times_out_at_the_deadline() {
        let services = RuntimeServices::new();
        services.register_task(ApplicationTaskClass::ParseWorker).expect("registration succeeds");
        // Deliberately never call record_terminal: a dropped/detached
        // handle cannot satisfy settlement on its own.

        let started = Instant::now();
        let outcome = services.begin_application_shutdown(
            ShutdownReason::ServerDrop,
            started + Duration::from_millis(30),
        );
        assert_eq!(outcome, ApplicationShutdown::TimedOut);
        assert!(
            started.elapsed() >= Duration::from_millis(30),
            "shutdown must actually wait out the bound"
        );
    }

    #[test]
    fn application_task_classes_enumerate_only_application_worker_work() {
        assert_eq!(ApplicationTaskClass::ALL.len(), 3);
        let labels: Vec<&str> =
            ApplicationTaskClass::ALL.iter().map(|class| class.label()).collect();
        assert_eq!(labels, vec!["parse-worker", "diagnostic-debounce", "file-watcher-debounce"]);

        // Negative control: the `tokio::spawn` mutation worker and read
        // dispatcher in `scheduler.rs` are FRAMEWORK tasks (#10024) -- no
        // application task class may name connection, scheduler, or writer
        // work.
        for forbidden in ["connection", "scheduler", "writer", "mutation", "dispatch"] {
            assert!(
                !labels.iter().any(|label| label.contains(forbidden)),
                "framework task '{forbidden}' must not have an ApplicationTaskClass variant"
            );
        }
    }

    #[test]
    fn request_cancel_stops_the_worker_but_settles_nothing_by_itself() {
        // A stop REQUEST is not a stop. `ParseWorker::request_shutdown` lets
        // the current job finish and `DiagnosticDebouncer::shutdown_now`
        // only posts a message, so recording a terminal here would let
        // shutdown report Complete while a worker was still running.
        let services = RuntimeServices::new();
        services.install_file_watcher_debouncer(FileWatcherDebouncer::with_interval(
            Duration::from_mins(1),
            |_| {},
        ));
        assert!(
            services.schedule_file_watcher_uri("file:///before-cancel.pl"),
            "an operational debouncer queues before cancellation"
        );

        services.request_cancel(ApplicationTaskClass::FileWatcherDebounce);

        // The worker really was signalled: a post-cancel admission is refused.
        assert!(
            !services.schedule_file_watcher_uri("file:///after-cancel.pl"),
            "request_cancel must actually signal the worker, not just bookkeep"
        );
        // ...but signalling alone settles nothing.
        let snapshot = services.settlement_snapshot();
        assert!(
            snapshot.settled.is_empty(),
            "a stop request must not retain a terminal on its own"
        );
        assert_eq!(snapshot.pending, vec![ApplicationTaskClass::FileWatcherDebounce]);
    }

    #[test]
    fn request_cancel_on_a_class_with_no_installed_worker_records_nothing() {
        // Negative control: an absent worker must not be read as an orderly
        // stop, or a class whose handle was dropped would settle itself clean.
        let services = RuntimeServices::new();
        services.register_task(ApplicationTaskClass::ParseWorker).expect("registration succeeds");
        services.request_cancel(ApplicationTaskClass::ParseWorker);

        let snapshot = services.settlement_snapshot();
        assert!(snapshot.settled.is_empty(), "no worker was stopped, so nothing settled");
        assert_eq!(snapshot.pending, vec![ApplicationTaskClass::ParseWorker]);
    }

    #[test]
    fn a_shutdown_timeout_is_persisted_so_a_retry_cannot_report_complete() {
        // Without persisting the timeout, a later completion would replace
        // the observed timeout and a second shutdown would report Complete,
        // contradicting the sticky-fault rule.
        let services = RuntimeServices::new();
        services.register_task(ApplicationTaskClass::ParseWorker).expect("registration succeeds");

        assert_eq!(
            services.begin_application_shutdown(
                ShutdownReason::ServerDrop,
                Instant::now() + Duration::from_millis(30)
            ),
            ApplicationShutdown::TimedOut
        );
        assert_eq!(
            services.settlement_snapshot().settled,
            vec![(ApplicationTaskClass::ParseWorker, TaskTerminal::TimedOut)],
            "the timeout must be retained, not left pending"
        );

        // A late completion cannot erase the observed timeout.
        services
            .record_terminal(ApplicationTaskClass::ParseWorker, TaskTerminal::Completed)
            .expect("terminal accepted");
        assert_eq!(
            services.begin_application_shutdown(
                ShutdownReason::ServerDrop,
                Instant::now() + Duration::from_millis(30)
            ),
            ApplicationShutdown::TimedOut,
            "a retry must not launder an observed timeout into Complete"
        );
    }

    #[test]
    fn a_replacement_worker_does_not_inherit_the_retired_worker_settlement() {
        // Installing a new worker is a NEW execution lifetime. Inheriting the
        // previous worker's terminal would let a live replacement present as
        // already settled -- and a replacement for a worker that failed to
        // spawn would be stuck reporting InstrumentFailed forever.
        let services = RuntimeServices::new();
        services.install_file_watcher_debouncer(FileWatcherDebouncer::unavailable_for_test());
        assert_eq!(
            services.settlement_snapshot().settled,
            vec![(
                ApplicationTaskClass::FileWatcherDebounce,
                TaskTerminal::InstrumentFailed {
                    reason: "file watcher debounce worker spawn failed".to_string()
                }
            )],
            "a worker that never spawned is retained as instrument-failed"
        );

        services.install_file_watcher_debouncer(FileWatcherDebouncer::with_interval(
            Duration::from_mins(1),
            |_| {},
        ));

        let snapshot = services.settlement_snapshot();
        assert!(
            snapshot.settled.is_empty(),
            "the replacement worker must start from a clean lifetime, not inherit the retired one"
        );
        assert_eq!(snapshot.pending, vec![ApplicationTaskClass::FileWatcherDebounce]);
        assert!(
            services.schedule_file_watcher_uri("file:///replacement.pl"),
            "the replacement worker is genuinely operational"
        );
    }

    #[test]
    fn a_cooperative_shutdown_of_every_installed_worker_reports_complete() {
        // A `Cancelled` terminal is the EXPECTED outcome of a requested
        // shutdown. Reading it as a fault would make every orderly #9508
        // shutdown report `Failed`.
        let services = RuntimeServices::new();
        services.install_diagnostic_debouncer(DiagnosticDebouncer::with_interval(
            Duration::from_mins(1),
            |_| {},
        ));
        services.install_file_watcher_debouncer(FileWatcherDebouncer::with_interval(
            Duration::from_mins(1),
            |_| {},
        ));

        let outcome = services.begin_application_shutdown(
            ShutdownReason::ClientShutdownRequest,
            Instant::now() + Duration::from_secs(5),
        );
        assert_eq!(
            outcome,
            ApplicationShutdown::Complete,
            "a cooperative stop is an orderly shutdown, not a failure"
        );
    }

    #[test]
    fn a_retained_failure_survives_a_later_cooperative_cancellation() {
        let services = RuntimeServices::new();
        services.install_file_watcher_debouncer(FileWatcherDebouncer::with_interval(
            Duration::from_mins(1),
            |_| {},
        ));
        services
            .record_terminal(
                ApplicationTaskClass::FileWatcherDebounce,
                TaskTerminal::Failed { reason: "dispatch sink panicked".to_string() },
            )
            .expect("terminal accepted");

        services.request_cancel(ApplicationTaskClass::FileWatcherDebounce);

        assert_eq!(
            services.settlement_snapshot().settled,
            vec![(
                ApplicationTaskClass::FileWatcherDebounce,
                TaskTerminal::Failed { reason: "dispatch sink panicked".to_string() }
            )],
            "a recorded fault is sticky across a later cancellation"
        );
        assert_eq!(
            services.begin_application_shutdown(
                ShutdownReason::ServerDrop,
                Instant::now() + Duration::from_secs(5)
            ),
            ApplicationShutdown::Failed
        );
    }

    #[test]
    fn evicting_a_dead_diagnostic_debouncer_still_settles_as_failed_not_timed_out() {
        // Evicting the refused debouncer (the #14322 fail-open path) discards
        // the ONLY handle carrying its exit state. If eviction does not
        // classify first, the class becomes unobservable and shutdown reports
        // `TimedOut` for a worker that actually DIED -- collapsing two states
        // #10024 requires to stay distinct.
        let services = RuntimeServices::new();
        services.install_diagnostic_debouncer(DiagnosticDebouncer::with_interval(
            Duration::from_millis(1),
            |uri: &str| {
                // Genuine unwind; explicit `panic!` is denied even in
                // cfg(test) code by this crate's clippy configuration.
                let boom: [u8; 0] = [];
                let _ = boom[uri.len()];
            },
        ));

        // Drive the panic, then keep scheduling until the worker is retired.
        // Admission starts refusing as soon as the receiver dies, which is
        // BEFORE the thread is observably finished; retirement waits for that
        // exit, so poll on the retained terminal rather than the first
        // refusal.
        assert!(services.schedule_diagnostic_debounce("file:///panics.pl"));
        let retired = (0..500).any(|_| {
            assert!(
                !services.schedule_diagnostic_debounce("file:///again.pl")
                    || services.settlement_snapshot().settled.is_empty(),
                "a retired class must not start admitting again"
            );
            if services.settlement_snapshot().settled.is_empty() {
                std::thread::sleep(Duration::from_millis(10));
                false
            } else {
                true
            }
        });
        assert!(retired, "a panicked worker must eventually retire with a terminal");

        // The refusal evicted the slot; settlement must still know it died.
        let outcome = services.begin_application_shutdown(
            ShutdownReason::ClientShutdownRequest,
            Instant::now() + Duration::from_secs(5),
        );
        assert_eq!(
            outcome,
            ApplicationShutdown::Failed,
            "an evicted dead worker must settle as Failed, never as a timeout"
        );
        assert_eq!(
            services.settlement_snapshot().settled,
            vec![(
                ApplicationTaskClass::DiagnosticDebounce,
                TaskTerminal::Failed {
                    reason: "diagnostic debounce worker exited abnormally".to_string()
                }
            )]
        );
    }

    #[test]
    fn a_refusal_during_teardown_records_nothing_until_the_exit_is_observable() {
        // The channel dies while the worker thread is still running, because
        // `worker_loop` owns the receiver. Classifying inside that window
        // would record a guess -- and the sticky rule would freeze it -- so
        // a refusal must retire nothing until the exit is observable.
        let (debouncer, release) = DiagnosticDebouncer::dead_channel_live_worker_for_test();
        let services = RuntimeServices::new();
        services.install_diagnostic_debouncer(debouncer);

        // Install saw an operational worker, so nothing is recorded yet.
        assert!(services.settlement_snapshot().settled.is_empty());

        // Admission is refused (dead channel) but the thread has NOT exited.
        assert!(!services.schedule_diagnostic_debounce("file:///parked.pl"));
        assert!(
            services.settlement_snapshot().settled.is_empty(),
            "a refusal during teardown must not record a guessed terminal"
        );
        assert!(
            services.diagnostic_debouncer_is_installed(),
            "the only handle to an unfinished worker must not be discarded"
        );

        // Release the worker; once its exit is observable the refusal retires
        // it with the outcome that actually happened.
        drop(release);
        let retired = (0..500).any(|_| {
            let _ = services.schedule_diagnostic_debounce("file:///parked.pl");
            if services.settlement_snapshot().settled.is_empty() {
                std::thread::sleep(Duration::from_millis(10));
                false
            } else {
                true
            }
        });
        assert!(retired, "an observable exit must eventually retire the worker");
        assert_eq!(
            services.settlement_snapshot().settled,
            vec![(ApplicationTaskClass::DiagnosticDebounce, TaskTerminal::Completed)],
            "the parked worker returned normally, so its terminal is Completed"
        );
    }

    #[test]
    fn a_worker_proven_exited_past_the_deadline_reports_its_real_terminal_not_timed_out() {
        // `deadline` bounds how long shutdown WAITS; it is not a predicate on
        // when the worker settled. A class whose exit is observable only on
        // the iteration after the bound has still settled, and reporting
        // `TimedOut` for it would fabricate a failure -- the mirror image of
        // the laundering this component exists to prevent. Pins that choice
        // so it cannot be reordered away by accident.
        let (debouncer, release) = DiagnosticDebouncer::dead_channel_live_worker_for_test();
        let services = RuntimeServices::new();
        services.install_diagnostic_debouncer(debouncer);
        // Install saw an operational worker, so the class is pending.
        assert!(services.settlement_snapshot().settled.is_empty());

        // Drive the worker to a PROVEN exit before shutdown runs, so the
        // assertion below cannot depend on millisecond timing.
        drop(release);
        let exited = (0..500).any(|_| {
            if services.diagnostic_debouncer_has_exited() {
                true
            } else {
                std::thread::sleep(Duration::from_millis(10));
                false
            }
        });
        assert!(exited, "the released worker must reach an observable exit");
        assert_eq!(
            services.settlement_snapshot().pending,
            vec![ApplicationTaskClass::DiagnosticDebounce],
            "the exited worker must still be pending: nothing recorded a terminal for it"
        );

        // The bound is already spent at the moment shutdown begins.
        let outcome =
            services.begin_application_shutdown(ShutdownReason::ServerDrop, Instant::now());

        assert_eq!(
            outcome,
            ApplicationShutdown::Complete,
            "a worker proven to have exited must not be reported as still running"
        );
        assert_ne!(outcome, ApplicationShutdown::TimedOut);
        assert_eq!(
            services.settlement_snapshot().settled,
            vec![(
                ApplicationTaskClass::DiagnosticDebounce,
                TaskTerminal::Cancelled { reason: ShutdownReason::ServerDrop }
            )],
            "the promoted terminal is the outcome that actually happened"
        );
    }

    #[test]
    fn a_worker_that_died_settles_as_failed_not_as_an_orderly_stop() {
        // Exit is not the same as orderly exit. A debouncer whose callback
        // panicked has also "exited", and treating that as `Cancelled` would
        // let a dead worker report `Complete` -- false-clean settlement in a
        // new guise.
        let services = RuntimeServices::new();
        services.install_file_watcher_debouncer(FileWatcherDebouncer::with_interval(
            Duration::from_millis(1),
            |uris: Vec<String>| {
                // Force a genuine unwind through an out-of-bounds access:
                // explicit `panic!`/`panic_any` are denied even in cfg(test)
                // code by this crate's clippy configuration, so this mirrors
                // `file_watcher_debouncer_panicking_sink_drops_and_counts_stranded_work`.
                let boom: [u8; 0] = [];
                let _ = boom[uris.len()];
            },
        ));

        assert!(services.schedule_file_watcher_uri("file:///panics.pl"));

        // Wait for the sink panic to be recorded by the debouncer itself.
        let died = (0..500).any(|_| {
            if services.file_watcher_is_operational() == Some(false) {
                true
            } else {
                std::thread::sleep(Duration::from_millis(10));
                false
            }
        });
        assert!(died, "the panicking sink should mark the debouncer non-operational");

        let outcome = services.begin_application_shutdown(
            ShutdownReason::ClientShutdownRequest,
            Instant::now() + Duration::from_secs(5),
        );

        assert_eq!(
            outcome,
            ApplicationShutdown::Failed,
            "a worker that died must not be laundered into a clean shutdown"
        );
        assert!(
            services
                .settlement_snapshot()
                .settled
                .iter()
                .any(|(class, terminal)| *class == ApplicationTaskClass::FileWatcherDebounce
                    && matches!(terminal, TaskTerminal::Failed { .. })),
            "the retained terminal must be Failed, not Cancelled"
        );
    }

    #[test]
    fn replacing_a_blocked_worker_never_holds_its_slot_lock_across_the_join() {
        // `FileWatcherDebouncer::Drop` calls `shutdown_now`, which JOINS the
        // dispatcher. If an install drops the previous occupant while still
        // holding the slot guard, a dispatcher stuck in a callback wedges the
        // slot and every concurrent reader with it. Only a blocked-callback
        // replacement can catch that: the cooperative path returns promptly
        // either way.
        let gate: std::sync::Arc<parking_lot::Mutex<bool>> =
            std::sync::Arc::new(parking_lot::Mutex::new(false));
        let gate_open = std::sync::Arc::clone(&gate);

        let services = std::sync::Arc::new(RuntimeServices::new());
        services.install_file_watcher_debouncer(FileWatcherDebouncer::with_interval(
            Duration::from_millis(1),
            move |_uris: Vec<String>| {
                // Bounded block so a failing assertion can never wedge teardown.
                let start = Instant::now();
                while !*gate_open.lock() && start.elapsed() < Duration::from_secs(30) {
                    std::thread::sleep(Duration::from_millis(2));
                }
            },
        ));
        assert!(services.schedule_file_watcher_uri("file:///blocked-replace.pl"));

        let armed = (0..500).any(|_| {
            if services.file_watcher_pressure().is_some_and(|p| p.active_subjects == 1) {
                true
            } else {
                std::thread::sleep(Duration::from_millis(10));
                false
            }
        });
        assert!(armed, "dispatcher should be held inside the blocked callback");

        // Replace it. The installing thread legitimately blocks in the
        // retired worker's join; what must NOT block is the slot itself.
        let installer_services = std::sync::Arc::clone(&services);
        let installer = std::thread::spawn(move || {
            installer_services.install_file_watcher_debouncer(FileWatcherDebouncer::with_interval(
                Duration::from_millis(1),
                |_uris: Vec<String>| {},
            ));
        });

        // The read must happen on its own thread. A drop-under-guard install
        // makes `file_watcher_pressure()` BLOCK on the slot mutex rather than
        // return a stale value, so polling it inline would simply wait the
        // block out and then pass -- measuring elapsed time instead of
        // falsifying. Bounding the read with `recv_timeout` is what turns the
        // stall into a failure.
        let (tx, rx) = std::sync::mpsc::channel::<()>();
        let reader_services = std::sync::Arc::clone(&services);
        let reader = std::thread::spawn(move || {
            // The fresh watcher has no active subjects, so observing 0 proves
            // the swap is visible while the old worker is still joining.
            loop {
                if reader_services.file_watcher_pressure().is_some_and(|p| p.active_subjects == 0) {
                    let _ = tx.send(());
                    return;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
        });

        // Far below the callback's 30s bound, so a timeout here means the
        // slot was genuinely wedged rather than merely slow.
        let swapped = rx.recv_timeout(Duration::from_secs(3)).is_ok();

        // Release before asserting: a failed assertion must not leave the
        // callback blocked, or the joins below would wedge the test run.
        *gate.lock() = true;
        let _ = installer.join();
        let _ = reader.join();

        assert!(
            swapped,
            "a replacement must be visible to concurrent readers while the \
             retired worker is still joining; the slot lock was held across \
             the join"
        );
    }

    #[test]
    fn shutdown_honors_its_deadline_while_a_watcher_callback_is_blocked() {
        // The falsifier for the bounded-settlement claim. Cancelling through
        // `FileWatcherDebouncer::shutdown_now` JOINS the dispatcher, and a
        // dispatcher stuck in a blocked callback never returns -- so shutdown
        // would hang before its first deadline check and could never record
        // `TimedOut`. Only a blocked-callback test can catch that; the
        // cooperative path returns promptly either way.
        let gate: std::sync::Arc<parking_lot::Mutex<bool>> =
            std::sync::Arc::new(parking_lot::Mutex::new(false));
        let gate_open = std::sync::Arc::clone(&gate);

        let services = RuntimeServices::new();
        services.install_file_watcher_debouncer(FileWatcherDebouncer::with_interval(
            Duration::from_millis(1),
            move |_uris: Vec<String>| {
                // Bounded block so a failing assertion can never wedge teardown.
                let start = Instant::now();
                while !*gate_open.lock() && start.elapsed() < Duration::from_secs(30) {
                    std::thread::sleep(Duration::from_millis(2));
                }
            },
        ));

        assert!(services.schedule_file_watcher_uri("file:///blocked.pl"));

        // Wait until the dispatcher is genuinely inside the blocked callback,
        // so the shutdown below really does race a stuck worker.
        let armed = (0..500).any(|_| {
            if services.file_watcher_pressure().is_some_and(|p| p.active_subjects == 1) {
                true
            } else {
                std::thread::sleep(Duration::from_millis(10));
                false
            }
        });
        assert!(armed, "dispatcher should be held inside the blocked callback");

        let started = Instant::now();
        let outcome = services.begin_application_shutdown(
            ShutdownReason::ClientShutdownRequest,
            started + Duration::from_millis(50),
        );
        let elapsed = started.elapsed();

        // Release before asserting: a failed assertion must not leave the
        // callback blocked, or `Drop`'s join would wedge the test run.
        *gate.lock() = true;

        assert_eq!(
            outcome,
            ApplicationShutdown::TimedOut,
            "a worker that never exits must settle as TimedOut, not hang or report Complete"
        );
        assert!(
            elapsed < Duration::from_secs(5),
            "shutdown must honor its deadline rather than block on the join; took {elapsed:?}"
        );
    }

    #[test]
    fn a_failed_parse_replacement_retires_the_previous_worker() {
        // Leaving the old worker installed while the new lifetime retains
        // `InstrumentFailed` would be a settlement lie: `parse_worker()` would
        // keep handing out a live worker for a class recorded as never
        // instrumented, so callers would take the async path for a class the
        // snapshot says never started.
        use super::super::parse_worker::ParseWorker;

        let services = RuntimeServices::new();
        assert!(services.install_parse_worker(ParseWorker::operational_for_test()));
        assert!(services.parse_worker().is_some(), "operational worker occupies the slot");

        assert!(!services.install_parse_worker(ParseWorker::non_operational_for_test()));

        assert!(
            services.parse_worker().is_none(),
            "a failed replacement must retire the previous worker so the synchronous fallback runs"
        );
        assert_eq!(
            services.settlement_snapshot().settled,
            vec![(
                ApplicationTaskClass::ParseWorker,
                TaskTerminal::InstrumentFailed {
                    reason: "parse worker pool failed to spawn any threads".to_string()
                }
            )]
        );
    }

    #[test]
    fn forwarded_accessors_preserve_none_fallback_behavior() {
        let server = LspServer::new();
        assert!(
            server.parse_worker().is_none(),
            "no worker installed means the sync fallback path"
        );
        assert!(
            !server.schedule_file_watcher_uri("file:///none-fallback.pl"),
            "no debouncer installed must report unscheduled, unchanged from before #10024"
        );
        // Must fall through to immediate publication rather than panicking
        // or silently discarding the URI.
        server.publish_diagnostics_debounced("file:///none-fallback.pl");
    }

    #[test]
    fn forwarded_accessors_preserve_installed_worker_behavior() {
        let server = LspServer::new();
        server.install_diagnostic_debouncer(DiagnosticDebouncer::with_interval(
            Duration::from_mins(1),
            |_| {},
        ));
        server.install_file_watcher_debouncer(FileWatcherDebouncer::with_interval(
            Duration::from_mins(1),
            |_| {},
        ));

        server.publish_diagnostics_debounced("file:///installed.pl");
        assert!(
            server.schedule_file_watcher_uri("file:///installed.pl"),
            "an installed, operational debouncer must genuinely queue the URI"
        );

        let snapshot = (0..50)
            .find_map(|_| {
                let pressure = server.runtime_pressure_snapshot();
                if pressure.diagnostic_debounce_pending_uris == 1
                    && pressure.file_watcher_pending_uris == 1
                {
                    Some(pressure)
                } else {
                    std::thread::sleep(Duration::from_millis(10));
                    None
                }
            })
            .expect("installed workers should report pending URI pressure");
        assert_eq!(snapshot.diagnostic_debounce_pending_uris, 1);
        assert_eq!(snapshot.file_watcher_pending_uris, 1);
    }

    #[test]
    fn forwarded_parse_worker_accessor_preserves_installed_behavior() {
        let server = std::sync::Arc::new(LspServer::new());
        server.install_default_parse_worker();
        assert!(
            server.parse_worker().is_some(),
            "the real production worker must install through the forwarded accessor"
        );
    }
}

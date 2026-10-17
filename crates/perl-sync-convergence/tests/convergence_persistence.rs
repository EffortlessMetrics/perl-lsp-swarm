//! Fixtures for `perl_lsp.convergence_transaction.v1` persistence (#11282).
//!
//! Covers the issue's required fixture classes: crash recovery, lease expiry,
//! duplicate writer, input movement, release-mode confusion, rejection, and
//! takeover — plus fail-closed loading of malformed and version-mismatched
//! persisted state.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use perl_sync_convergence::prelude::*;

const T0: u64 = 1_000;

fn tx_id() -> TransactionId {
    TransactionId::new("bridge-2026-08").expect("valid transaction id")
}

fn generation_for(source_sha: &str) -> GenerationId {
    GenerationId::from_inputs(&GenerationInputs {
        direction: Direction::SwarmToSource,
        release_mode: ReleaseContextMode::OrdinaryContinuous,
        source_repository: "EffortlessMetrics/perl-lsp".into(),
        source_parent_sha: source_sha.into(),
        source_parent_tree: "t".repeat(40),
        swarm_repository: "EffortlessMetrics/perl-lsp-swarm".into(),
        swarm_parent_sha: "c".repeat(40),
        swarm_parent_tree: "u".repeat(40),
        prior_accepted_generation: "",
    })
}

fn receipt(
    tx: &TransactionId,
    generation_id: &GenerationId,
    mode: ReleaseContextMode,
) -> ConvergenceGeneration {
    // Exact inputs mirror `generation_for` + `started_event` so the receipt's
    // derived identity matches the journal's generation.
    let mut receipt = ConvergenceGeneration {
        transaction_id: tx.clone(),
        generation_id: GenerationId::parse(generation_id.as_str()).expect("wire form"),
        direction: Direction::SwarmToSource,
        release_context_mode: mode,
        source_repository: "EffortlessMetrics/perl-lsp".into(),
        source_parent_sha: "a".repeat(40),
        source_parent_tree: "t".repeat(40),
        swarm_repository: "EffortlessMetrics/perl-lsp-swarm".into(),
        swarm_parent_sha: "c".repeat(40),
        swarm_parent_tree: "u".repeat(40),
        prior_accepted_generation: None,
        imported_commits: vec![],
        projection_digests: vec![],
        candidate_commit: None,
        candidate_tree: None,
        published_candidate: None,
        landing_merge: None,
    };
    // Align the declared ID so validation passes even for non-default inputs.
    if generation_id.as_str() != receipt.expected_id().as_str() {
        receipt.generation_id = receipt.expected_id();
    }
    receipt
}

fn opened_event(tx: &TransactionId, mode: ReleaseContextMode) -> ConvergenceEvent {
    ConvergenceEvent::TransactionOpened {
        transaction_id: tx.clone(),
        direction: Direction::SwarmToSource,
        release_mode: mode,
        prior_accepted_generation: None,
        opened_at: TimestampMs::from_millis(T0),
    }
}

fn started_event(tx: &TransactionId, generation: &GenerationId) -> ConvergenceEvent {
    ConvergenceEvent::GenerationStarted {
        transaction_id: tx.clone(),
        generation_id: generation.clone(),
        source_parent_sha: "a".repeat(40),
        swarm_parent_sha: "c".repeat(40),
        started_at: TimestampMs::from_millis(T0 + 10),
    }
}

fn lease_for(generation: &GenerationId, claimant: &str, at: u64, ttl: u64) -> Lease {
    Lease::new(
        claimant,
        TimestampMs::from_millis(at),
        ttl,
        generation.clone(),
        vec![PermittedAction::PlanCandidate],
    )
    .expect("valid lease")
}

fn store_in_temp() -> (tempfile::TempDir, ConvergenceStore) {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = ConvergenceStore::open(dir.path()).expect("open store");
    (dir, store)
}

#[test]
fn crash_recovery_reconstructs_state_and_next_actions() {
    let (dir, store) = store_in_temp();
    let tx = tx_id();
    let generation = generation_for(&"a".repeat(40));

    store
        .register_transaction(TransactionIndexEntry {
            transaction_id: tx.clone(),
            direction: Direction::SwarmToSource,
            release_mode: ReleaseContextMode::OrdinaryContinuous,
        })
        .expect("register");
    store
        .write_receipt(&receipt(&tx, &generation, ReleaseContextMode::OrdinaryContinuous))
        .expect("receipt");
    store
        .append_event(&tx, &opened_event(&tx, ReleaseContextMode::OrdinaryContinuous))
        .expect("open");
    store.append_event(&tx, &started_event(&tx, &generation)).expect("start");

    // Simulate a crash: a fresh store instance over the same directory must
    // reconstruct everything from durable state alone.
    drop(store);
    let fresh = ConvergenceStore::open(dir.path()).expect("reopen");
    let view = fresh.load_view(&tx).expect("replay");

    let active = view.active_generation().expect("active generation");
    assert_eq!(active.state, TransitionState::Observed);
    assert_eq!(
        active.next_actions(),
        vec![PermittedAction::PlanCandidate, PermittedAction::StartSuccessorGeneration]
    );
    assert!(fresh.read_receipt(&tx, &generation).is_ok());
}

#[test]
fn expired_lease_is_reclaimable_only_via_recorded_takeover() {
    let (_, store) = store_in_temp();
    let tx = tx_id();
    let generation = generation_for(&"a".repeat(40));

    store
        .register_transaction(TransactionIndexEntry {
            transaction_id: tx.clone(),
            direction: Direction::SwarmToSource,
            release_mode: ReleaseContextMode::OrdinaryContinuous,
        })
        .expect("register");
    store
        .append_event(&tx, &opened_event(&tx, ReleaseContextMode::OrdinaryContinuous))
        .expect("open");
    store.append_event(&tx, &started_event(&tx, &generation)).expect("start");
    store
        .append_event(
            &tx,
            &ConvergenceEvent::LeaseClaimed {
                transaction_id: tx.clone(),
                lease: lease_for(&generation, "writer-a", T0 + 20, 1_000),
            },
        )
        .expect("claim");

    // Duplicate writer while the lease is live must be refused.
    let duplicate = store.append_event(
        &tx,
        &ConvergenceEvent::LeaseClaimed {
            transaction_id: tx.clone(),
            lease: lease_for(&generation, "writer-b", T0 + 100, 1_000),
        },
    );
    assert!(matches!(
        duplicate,
        Err(StoreError::Replay(ReplayError { kind: ReplayErrorKind::LiveLeaseExists, .. }))
    ));

    // A plain claim after expiry is also refused: only a recorded takeover
    // may replace an expired lease (direct-post-expiry negative control).
    let expired_claim = store.append_event(
        &tx,
        &ConvergenceEvent::LeaseClaimed {
            transaction_id: tx.clone(),
            lease: lease_for(&generation, "writer-c", T0 + 5_000, 1_000),
        },
    );
    assert!(
        matches!(
            expired_claim,
            Err(StoreError::Replay(ReplayError { kind: ReplayErrorKind::LiveLeaseExists, .. }))
        ),
        "a plain claim must never bypass the takeover reconciliation requirement"
    );

    // After expiry, reclaiming without reconciliation evidence fails.
    let takeover_missing_evidence = Takeover {
        displaced_claimant: "writer-a".into(),
        reclaimed_by: "writer-b".into(),
        reclaimed_at: TimestampMs::from_millis(T0 + 2_000),
        input_generation: generation.clone(),
        reconciled_observations: vec![],
    };
    let new_lease = lease_for(&generation, "writer-b", T0 + 2_000, 5_000);
    let refused = store.append_event(
        &tx,
        &ConvergenceEvent::TakeoverRecorded {
            transaction_id: tx.clone(),
            takeover: takeover_missing_evidence,
            new_lease: new_lease.clone(),
        },
    );
    assert!(
        matches!(
            refused,
            Err(StoreError::Replay(ReplayError { kind: ReplayErrorKind::InvalidTakeover(_), .. }))
        ),
        "takeover without reconciliation evidence must be refused"
    );

    // A validated takeover after exact-state reconciliation succeeds.
    let valid_takeover = Takeover {
        reconciled_observations: vec![
            "sha256:source-observed".into(),
            "sha256:swarm-observed".into(),
        ],
        ..Takeover {
            displaced_claimant: "writer-a".into(),
            reclaimed_by: "writer-b".into(),
            reclaimed_at: TimestampMs::from_millis(T0 + 2_000),
            input_generation: generation.clone(),
            reconciled_observations: vec![],
        }
    };
    store
        .append_event(
            &tx,
            &ConvergenceEvent::TakeoverRecorded {
                transaction_id: tx.clone(),
                takeover: valid_takeover,
                new_lease: new_lease.clone(),
            },
        )
        .expect("validated takeover");
    let view = store.load_view(&tx).expect("view");
    assert_eq!(view.lease.as_ref().map(|l| l.claimed_by.as_str()), Some("writer-b"));
}

#[test]
fn second_active_generation_on_same_source_parent_fails_closed() {
    let (_, store) = store_in_temp();
    let tx = tx_id();
    let gen_a = generation_for(&"a".repeat(40));
    let gen_b = GenerationId::from_inputs(&GenerationInputs {
        direction: Direction::SwarmToSource,
        release_mode: ReleaseContextMode::OrdinaryContinuous,
        source_repository: "EffortlessMetrics/perl-lsp".into(),
        source_parent_sha: "a".repeat(40),
        source_parent_tree: "t2".to_string() + &"t".repeat(38),
        swarm_repository: "EffortlessMetrics/perl-lsp-swarm".into(),
        swarm_parent_sha: "e".repeat(40),
        swarm_parent_tree: "u2".to_string() + &"u".repeat(38),
        prior_accepted_generation: "",
    });

    store
        .register_transaction(TransactionIndexEntry {
            transaction_id: tx.clone(),
            direction: Direction::SwarmToSource,
            release_mode: ReleaseContextMode::OrdinaryContinuous,
        })
        .expect("register");
    store
        .append_event(&tx, &opened_event(&tx, ReleaseContextMode::OrdinaryContinuous))
        .expect("open");
    store.append_event(&tx, &started_event(&tx, &gen_a)).expect("start A");

    let conflicting = ConvergenceEvent::GenerationStarted {
        transaction_id: tx.clone(),
        generation_id: gen_b.clone(),
        source_parent_sha: "a".repeat(40),
        swarm_parent_sha: "e".repeat(40),
        started_at: TimestampMs::from_millis(T0 + 30),
    };
    let refused = store.append_event(&tx, &conflicting);
    assert!(matches!(
        refused,
        Err(StoreError::Replay(ReplayError {
            kind: ReplayErrorKind::ConcurrentActiveGeneration { .. },
            ..
        }))
    ));
}

#[test]
fn moved_input_creates_successor_generation_not_an_edit() {
    let (_, store) = store_in_temp();
    let tx = tx_id();
    let gen_old = generation_for(&"a".repeat(40));
    let receipt_old = receipt(&tx, &gen_old, ReleaseContextMode::OrdinaryContinuous);

    store
        .register_transaction(TransactionIndexEntry {
            transaction_id: tx.clone(),
            direction: Direction::SwarmToSource,
            release_mode: ReleaseContextMode::OrdinaryContinuous,
        })
        .expect("register");
    store.write_receipt(&receipt_old).expect("first write");

    // Editing the same generation's receipt after its exact input moved is
    // refused; the persisted bytes differ from the canonical rewrite only
    // when the receipt itself changed.
    let mut edited = receipt_old.clone();
    edited.source_parent_sha = "f".repeat(40);
    assert!(edited.validate().is_err(), "identity must not validate after input move");
    let refused = store.write_receipt(&edited);
    assert!(matches!(refused, Err(StoreError::Malformed(_))));

    // The successor derives from the moved inputs and is a distinct ID.
    let gen_new = GenerationId::from_inputs(&GenerationInputs {
        source_parent_sha: "f".repeat(40),
        prior_accepted_generation: gen_old.as_str(),
        ..GenerationInputs {
            direction: Direction::SwarmToSource,
            release_mode: ReleaseContextMode::OrdinaryContinuous,
            source_repository: "EffortlessMetrics/perl-lsp".into(),
            source_parent_sha: "a".repeat(40),
            source_parent_tree: "b".repeat(40),
            swarm_repository: "EffortlessMetrics/perl-lsp-swarm".into(),
            swarm_parent_sha: "c".repeat(40),
            swarm_parent_tree: "d".repeat(40),
            prior_accepted_generation: "",
        }
    });
    assert_ne!(gen_old, gen_new);

    // Supersession records lineage without rewriting history.
    store
        .append_event(&tx, &opened_event(&tx, ReleaseContextMode::OrdinaryContinuous))
        .expect("open");
    store.append_event(&tx, &started_event(&tx, &gen_old)).expect("start old");
    store
        .append_event(
            &tx,
            &ConvergenceEvent::GenerationSuperseded {
                transaction_id: tx.clone(),
                old_generation: gen_old.clone(),
                successor_generation: gen_new.clone(),
                reason_digest: "sha256:moved-input".into(),
                superseded_at: TimestampMs::from_millis(T0 + 50),
            },
        )
        .expect("supersede");
    store
        .append_event(
            &tx,
            &ConvergenceEvent::GenerationStarted {
                transaction_id: tx.clone(),
                generation_id: gen_new.clone(),
                source_parent_sha: "f".repeat(40),
                swarm_parent_sha: "c".repeat(40),
                started_at: TimestampMs::from_millis(T0 + 60),
            },
        )
        .expect("start successor");

    let view = store.load_view(&tx).expect("view");
    assert_eq!(view.generations[&gen_old].state, TransitionState::Superseded);
    assert_eq!(view.generations[&gen_old].successor.as_ref(), Some(&gen_new));
    assert_eq!(view.active_generation().map(|g| g.state), Some(TransitionState::Observed));
}

#[test]
fn release_mode_confusion_fails_closed() {
    let (_, store) = store_in_temp();
    let tx = TransactionId::new("release-specific-tx").expect("id");

    store
        .register_transaction(TransactionIndexEntry {
            transaction_id: tx.clone(),
            direction: Direction::SwarmToSource,
            release_mode: ReleaseContextMode::ReleaseSpecific,
        })
        .expect("register as release-specific");

    let generation = GenerationId::from_inputs(&GenerationInputs {
        direction: Direction::SwarmToSource,
        release_mode: ReleaseContextMode::OrdinaryContinuous,
        source_repository: "EffortlessMetrics/perl-lsp".into(),
        source_parent_sha: "a".repeat(40),
        source_parent_tree: "b".repeat(40),
        swarm_repository: "EffortlessMetrics/perl-lsp-swarm".into(),
        swarm_parent_sha: "c".repeat(40),
        swarm_parent_tree: "d".repeat(40),
        prior_accepted_generation: "",
    });
    let ordinary_receipt = receipt(&tx, &generation, ReleaseContextMode::OrdinaryContinuous);
    let refused = store.write_receipt(&ordinary_receipt);
    assert!(
        matches!(refused, Err(StoreError::ReleaseModeConflict { .. })),
        "an ordinary-continuous generation must never silently join a release-specific transaction"
    );

    // Unknown mode spellings also fail at the serde boundary.
    assert!(serde_json::from_value::<ReleaseContextMode>(serde_json::json!("ordinary")).is_err());
}

#[test]
fn rejected_evidence_stays_rejected_and_immutable() {
    let (_, store) = store_in_temp();
    let tx = tx_id();
    let generation = generation_for(&"a".repeat(40));

    store
        .register_transaction(TransactionIndexEntry {
            transaction_id: tx.clone(),
            direction: Direction::SwarmToSource,
            release_mode: ReleaseContextMode::OrdinaryContinuous,
        })
        .expect("register");
    store
        .append_event(&tx, &opened_event(&tx, ReleaseContextMode::OrdinaryContinuous))
        .expect("open");
    store.append_event(&tx, &started_event(&tx, &generation)).expect("start");
    store
        .append_event(
            &tx,
            &ConvergenceEvent::RejectionRecorded {
                transaction_id: tx.clone(),
                generation_id: generation.clone(),
                evidence_digest: "sha256:admission-rejection".into(),
                recorded_at: TimestampMs::from_millis(T0 + 70),
            },
        )
        .expect("reject");

    // A later green check cannot rewrite the prior rejection: terminal
    // generations accept no further transitions.
    let rewritten = store.append_event(
        &tx,
        &ConvergenceEvent::TransitionRecorded {
            transaction_id: tx.clone(),
            generation_id: generation.clone(),
            to: TransitionState::Admitted,
            evidence_digest: "sha256:later-green-check".into(),
            recorded_at: TimestampMs::from_millis(T0 + 80),
        },
    );
    assert!(
        matches!(
            rewritten,
            Err(StoreError::Replay(ReplayError {
                kind: ReplayErrorKind::IllegalTransition { from: "rejected", to: "admitted" },
                ..
            }))
        ),
        "prior rejection must not be overwritten by later success"
    );

    // Double rejection is also refused.
    let double = store.append_event(
        &tx,
        &ConvergenceEvent::RejectionRecorded {
            transaction_id: tx.clone(),
            generation_id: generation.clone(),
            evidence_digest: "sha256:different".into(),
            recorded_at: TimestampMs::from_millis(T0 + 90),
        },
    );
    assert!(double.is_err());

    let view = store.load_view(&tx).expect("view");
    assert_eq!(view.generations[&generation].state, TransitionState::Rejected);
    assert_eq!(
        view.generations[&generation].rejection_evidence_digest.as_deref(),
        Some("sha256:admission-rejection")
    );
}

#[test]
fn unresolved_states_never_fold_into_pass() {
    let tx = tx_id();
    let generation = generation_for(&"a".repeat(40));
    let events = [
        opened_event(&tx, ReleaseContextMode::OrdinaryContinuous),
        started_event(&tx, &generation),
        ConvergenceEvent::TransitionRecorded {
            transaction_id: tx.clone(),
            generation_id: generation.clone(),
            to: TransitionState::Planned,
            evidence_digest: "sha256:plan".into(),
            recorded_at: TimestampMs::from_millis(T0 + 1),
        },
        ConvergenceEvent::TransitionRecorded {
            transaction_id: tx.clone(),
            generation_id: generation.clone(),
            to: TransitionState::NotProven,
            evidence_digest: "sha256:proof-gap".into(),
            recorded_at: TimestampMs::from_millis(T0 + 2),
        },
    ];
    let view = replay(&events).expect("fold");
    assert_eq!(view.generations[&generation].state, TransitionState::NotProven);

    // From NotProven nothing can move forward to a passing state.
    for target in
        [TransitionState::Materialized, TransitionState::Admitted, TransitionState::Merged]
    {
        assert!(!is_legal_transition(TransitionState::NotProven, target));
    }

    // Instrument failure behaves identically.
    let events_failure = [
        opened_event(&tx, ReleaseContextMode::OrdinaryContinuous),
        started_event(&tx, &generation),
        ConvergenceEvent::TransitionRecorded {
            transaction_id: tx.clone(),
            generation_id: generation.clone(),
            to: TransitionState::InstrumentFailure,
            evidence_digest: "sha256:instrument-down".into(),
            recorded_at: TimestampMs::from_millis(T0 + 3),
        },
    ];
    let view = replay(&events_failure).expect("fold");
    assert_eq!(view.generations[&generation].state, TransitionState::InstrumentFailure);
}

#[test]
fn invalidation_records_cause_and_descendants_durably() {
    let tx = tx_id();
    let gen_old = generation_for(&"a".repeat(40));
    let gen_new = generation_for(&"f".repeat(40));

    let record = InvalidationRecord::new(
        gen_old.clone(),
        InvalidationCause::SourceMasterMovement,
        TimestampMs::from_millis(T0 + 5),
        "sha256:master-moved-evidence",
        vec![StaleDescendant {
            generation: gen_old.clone(),
            disposition: StaleDisposition::Stale,
            reason: "source master advanced past the recorded parent".into(),
        }],
    )
    .expect("record");

    let events = [
        opened_event(&tx, ReleaseContextMode::OrdinaryContinuous),
        started_event(&tx, &gen_old),
        // The successor start requires a terminal predecessor: record the
        // supersession lineage first.
        ConvergenceEvent::GenerationSuperseded {
            transaction_id: tx.clone(),
            old_generation: gen_old.clone(),
            successor_generation: gen_new.clone(),
            reason_digest: "sha256:moved-input".into(),
            superseded_at: TimestampMs::from_millis(T0 + 6),
        },
        started_successor(&tx, &gen_new),
        ConvergenceEvent::InvalidationRecorded {
            transaction_id: tx.clone(),
            record: record.clone(),
        },
    ];
    let view = replay(&events).expect("fold");
    assert_eq!(view.invalidations.len(), 1);
    assert_eq!(view.invalidations[0].cause, InvalidationCause::SourceMasterMovement);
    assert_eq!(view.invalidations[0].stale_descendants[0].disposition, StaleDisposition::Stale);
    assert_ne!(view.invalidations[0].stale_descendants[0].disposition, StaleDisposition::Rejected);
}

fn started_successor(tx: &TransactionId, successor: &GenerationId) -> ConvergenceEvent {
    ConvergenceEvent::GenerationStarted {
        transaction_id: tx.clone(),
        generation_id: successor.clone(),
        source_parent_sha: "f".repeat(40),
        swarm_parent_sha: "c".repeat(40),
        started_at: TimestampMs::from_millis(T0 + 6),
    }
}

#[test]
fn unsupported_schema_versions_fail_closed() {
    let (dir, store) = store_in_temp();
    let tx = tx_id();

    // Journal line with an unsupported version.
    let journal_dir = dir.path().join("transactions").join(tx.as_str());
    std::fs::create_dir_all(&journal_dir).expect("mkdir");
    let event_line = serde_json::json!({
        "schema_version": JOURNAL_SCHEMA_VERSION + 1,
        "event": "transaction_opened",
        "transaction_id": tx.as_str(),
        "direction": "swarm_to_source",
        "release_mode": "ordinary_continuous",
        "opened_at": 1,
    });
    std::fs::write(journal_dir.join("events.v1.jsonl"), format!("{event_line}\n"))
        .expect("write journal");
    let err = store.load_journal(&tx).expect_err("version mismatch must fail closed");
    assert!(matches!(err, StoreError::UnsupportedJournalVersion { .. }));

    // Index with an unsupported version.
    let index = format!("{{\"schema_version\":{},\"transactions\":[]}}", INDEX_SCHEMA_VERSION + 41);
    std::fs::write(dir.path().join("index.v1.json"), index).expect("write index");
    let err = store.load_index().expect_err("index mismatch must fail closed");
    assert!(matches!(err, StoreError::UnsupportedIndexVersion { .. }));
}

#[test]
fn malformed_persisted_state_fails_closed_without_degradation() {
    let (dir, _) = store_in_temp();
    let tx = tx_id();
    let journal_dir = dir.path().join("transactions").join(tx.as_str());
    std::fs::create_dir_all(&journal_dir).expect("mkdir");

    // Truncated JSON line.
    std::fs::write(
        journal_dir.join("events.v1.jsonl"),
        "{\"schema_version\":1,\"event\":\"transaction_opened\"",
    )
    .expect("write truncated journal");
    let fresh = ConvergenceStore::open(dir.path()).expect("reopen");
    let err = fresh.load_journal(&tx).expect_err("malformed JSON must fail closed");
    assert!(matches!(err, StoreError::Malformed(_)));

    // Unknown enum spelling inside a well-formed envelope.
    std::fs::write(
        journal_dir.join("events.v1.jsonl"),
        format!(
            "{{\"schema_version\":{JOURNAL_SCHEMA_VERSION},\"event\":\"transaction_opened\",\"transaction_id\":\"{}\",\"direction\":\"sideways\",\"release_mode\":\"ordinary_continuous\",\"opened_at\":1}}\n",
            tx.as_str()
        ),
    )
    .expect("write unknown-direction journal");
    let err = fresh.load_journal(&tx).expect_err("unknown direction must fail closed");
    assert!(matches!(err, StoreError::Malformed(_)));
}

#[test]
fn different_parent_start_refused_while_generation_active() {
    // A different-parent generation must not start while the old one is
    // still non-terminal; otherwise a fresh process could reconstruct zero
    // or ambiguous current generations.
    let tx = tx_id();
    let gen_a = generation_for(&"a".repeat(40));
    let gen_b = generation_for(&"f".repeat(40));

    let events = [
        opened_event(&tx, ReleaseContextMode::OrdinaryContinuous),
        started_event(&tx, &gen_a),
        ConvergenceEvent::GenerationStarted {
            transaction_id: tx.clone(),
            generation_id: gen_b.clone(),
            source_parent_sha: "f".repeat(40),
            swarm_parent_sha: "c".repeat(40),
            started_at: TimestampMs::from_millis(T0 + 30),
        },
    ];
    let refused = replay(&events).expect_err("no-supersession start must fail closed");
    assert!(matches!(refused.kind, ReplayErrorKind::ConcurrentActiveGeneration { .. }));
}

#[test]
fn heartbeat_rejects_backdated_time_and_non_extending_expiry() {
    let tx = tx_id();
    let generation = generation_for(&"a".repeat(40));
    let base = [
        opened_event(&tx, ReleaseContextMode::OrdinaryContinuous),
        started_event(&tx, &generation),
        ConvergenceEvent::LeaseClaimed {
            transaction_id: tx.clone(),
            lease: lease_for(&generation, "writer-a", T0 + 20, 1_000),
        },
    ];

    // Backdated heartbeat (earlier than claim/previous heartbeat).
    let mut events = base.to_vec();
    events.push(ConvergenceEvent::LeaseHeartbeat {
        transaction_id: tx.clone(),
        claimed_by: "writer-a".into(),
        heartbeat_at: TimestampMs::from_millis(T0 + 10),
        new_expires_at: TimestampMs::from_millis(T0 + 5_000),
    });
    let refused = replay(&events).expect_err("backdated heartbeat must fail closed");
    assert!(
        matches!(refused.kind, ReplayErrorKind::InvalidLease(_)),
        "time must never move backward during replay"
    );

    // Expiry at/before the heartbeat instant.
    let mut events = base.to_vec();
    events.push(ConvergenceEvent::LeaseHeartbeat {
        transaction_id: tx.clone(),
        claimed_by: "writer-a".into(),
        heartbeat_at: TimestampMs::from_millis(T0 + 30),
        new_expires_at: TimestampMs::from_millis(T0 + 20),
    });
    let refused = replay(&events).expect_err("already-expired extension must fail closed");
    assert!(matches!(refused.kind, ReplayErrorKind::InvalidLease(_)));

    // A lawful monotonic extension still succeeds.
    let mut events = base.to_vec();
    events.push(ConvergenceEvent::LeaseHeartbeat {
        transaction_id: tx.clone(),
        claimed_by: "writer-a".into(),
        heartbeat_at: TimestampMs::from_millis(T0 + 500),
        new_expires_at: TimestampMs::from_millis(T0 + 2_000),
    });
    let view = replay(&events).expect("lawful extension");
    let lease = view.lease.expect("lease");
    assert_eq!(lease.heartbeat_at, TimestampMs::from_millis(T0 + 500));
    assert_eq!(lease.lease_expires_at, TimestampMs::from_millis(T0 + 2_000));
}

#[test]
fn takeover_binds_installed_lease_to_the_reconciliation_record() {
    let tx = tx_id();
    let generation = generation_for(&"a".repeat(40));
    let displaced = [
        opened_event(&tx, ReleaseContextMode::OrdinaryContinuous),
        started_event(&tx, &generation),
        ConvergenceEvent::LeaseClaimed {
            transaction_id: tx.clone(),
            lease: lease_for(&generation, "writer-a", T0 + 20, 500),
        },
    ];
    let takeover = |reclaimed_by: &str| Takeover {
        displaced_claimant: "writer-a".into(),
        reclaimed_by: reclaimed_by.into(),
        reclaimed_at: TimestampMs::from_millis(T0 + 600),
        input_generation: generation.clone(),
        reconciled_observations: vec!["sha256:source-observed".into()],
    };

    // Control 1: installed lease claimant must be the reclaiming writer.
    let wrong_claimant_lease = lease_for(&generation, "someone-else", T0 + 600, 1_000);
    let mut events = displaced.to_vec();
    events.push(ConvergenceEvent::TakeoverRecorded {
        transaction_id: tx.clone(),
        takeover: takeover("writer-b"),
        new_lease: wrong_claimant_lease,
    });
    let refused = replay(&events).expect_err("unbound claimant must fail closed");
    assert!(matches!(refused.kind, ReplayErrorKind::InvalidTakeover(_)));

    // Control 2: the fresh claim instant must equal the takeover instant.
    let drifting_lease = lease_for(&generation, "writer-b", T0 + 601, 1_000);
    let mut events = displaced.to_vec();
    events.push(ConvergenceEvent::TakeoverRecorded {
        transaction_id: tx.clone(),
        takeover: takeover("writer-b"),
        new_lease: drifting_lease,
    });
    let refused = replay(&events).expect_err("drifting claim instant must fail closed");
    assert!(matches!(refused.kind, ReplayErrorKind::InvalidTakeover(_)));

    // Control 3: an already-expired lease can never be installed. Raw struct
    // construction stands in for hostile deserialized bytes bypassing the
    // validated constructor.
    let expired_install = Lease {
        claimed_by: "writer-b".into(),
        claimed_at: TimestampMs::from_millis(T0 + 600),
        heartbeat_at: TimestampMs::from_millis(T0 + 600),
        lease_expires_at: TimestampMs::from_millis(T0 + 600),
        input_generation: generation.clone(),
        last_completed_transition: None,
        next_permitted_actions: vec![PermittedAction::PlanCandidate],
    };
    let mut events = displaced.to_vec();
    events.push(ConvergenceEvent::TakeoverRecorded {
        transaction_id: tx.clone(),
        takeover: takeover("writer-b"),
        new_lease: expired_install,
    });
    let refused = replay(&events).expect_err("expired install must fail closed");
    assert!(matches!(refused.kind, ReplayErrorKind::InvalidTakeover(_)));

    // The bound, live replacement is accepted and becomes current state.
    let bound_lease = lease_for(&generation, "writer-b", T0 + 600, 1_000);
    let mut events = displaced.to_vec();
    events.push(ConvergenceEvent::TakeoverRecorded {
        transaction_id: tx.clone(),
        takeover: takeover("writer-b"),
        new_lease: bound_lease,
    });
    let view = replay(&events).expect("bound takeover");
    assert_eq!(view.lease.as_ref().map(|l| l.claimed_by.as_str()), Some("writer-b"));
}

#[test]
fn opened_event_requires_unique_registered_index_entry() {
    // Missing registration fails closed on the opening event.
    let (_, store) = store_in_temp();
    let tx = TransactionId::new("never-registered-tx").expect("id");
    let missing =
        store.append_event(&tx, &opened_event(&tx, ReleaseContextMode::OrdinaryContinuous));
    assert!(
        matches!(missing, Err(StoreError::UnregisteredTransaction { .. })),
        "journal and index must reconcile before the journal exists"
    );

    // Direction disagreement between index and journal also fails closed.
    let (dir, store) = store_in_temp();
    let tx = tx_id();
    store
        .register_transaction(TransactionIndexEntry {
            transaction_id: tx.clone(),
            direction: Direction::SourceToSwarm,
            release_mode: ReleaseContextMode::OrdinaryContinuous,
        })
        .expect("register opposite direction");
    let mismatched =
        store.append_event(&tx, &opened_event(&tx, ReleaseContextMode::OrdinaryContinuous));
    assert!(
        matches!(mismatched, Err(StoreError::ReleaseModeConflict { .. })),
        "a swarm_to_source journal must never open under a source_to_swarm registration"
    );
    drop(store);

    // Duplicated index entries are malformed ownership and fail closed.
    let entry_json = format!(
        "{{\"transaction_id\":\"{}\",\"direction\":\"swarm_to_source\",\"release_mode\":\"ordinary_continuous\"}}",
        tx.as_str()
    );
    let index = format!(
        "{{\"schema_version\":{INDEX_SCHEMA_VERSION},\"transactions\":[{entry},{entry}]}}",
        entry = entry_json
    );
    std::fs::write(dir.path().join("index.v1.json"), index).expect("write duplicated index");
    let store = ConvergenceStore::open(dir.path()).expect("reopen");
    let duplicated =
        store.append_event(&tx, &opened_event(&tx, ReleaseContextMode::OrdinaryContinuous));
    assert!(matches!(duplicated, Err(StoreError::UnregisteredTransaction { .. })));
}

#[test]
fn unreadable_index_fails_receipt_writes_closed() {
    let (dir, store) = store_in_temp();
    let tx = tx_id();
    let generation = generation_for(&"a".repeat(40));

    // A malformed index must abort receipt writes instead of reading as "no
    // conflict".
    std::fs::write(dir.path().join("index.v1.json"), "{not json").expect("write garbage index");
    let refused_receipt =
        store.write_receipt(&receipt(&tx, &generation, ReleaseContextMode::OrdinaryContinuous));
    assert!(
        matches!(refused_receipt, Err(StoreError::Malformed(_))),
        "garbage index must never read as an empty coherent store"
    );

    // Journal appends reconcile against the same index and must also fail.
    let opened =
        store.append_event(&tx, &opened_event(&tx, ReleaseContextMode::OrdinaryContinuous));
    assert!(matches!(opened, Err(StoreError::Malformed(_))));
}

#[test]
fn read_receipt_binds_bytes_to_requested_location() {
    let (dir, store) = store_in_temp();
    let tx = tx_id();
    let gen_a = generation_for(&"a".repeat(40));
    // B must be an internally coherent receipt for genuinely different
    // inputs, so mutate exact inputs first and re-derive its declared ID.
    let mut b_receipt = receipt(&tx, &gen_a, ReleaseContextMode::OrdinaryContinuous);
    b_receipt.source_parent_sha = "f".repeat(40);
    b_receipt.generation_id = b_receipt.expected_id();
    let gen_b = b_receipt.generation_id.clone();
    assert_ne!(gen_a, gen_b);
    store
        .register_transaction(TransactionIndexEntry {
            transaction_id: tx.clone(),
            direction: Direction::SwarmToSource,
            release_mode: ReleaseContextMode::OrdinaryContinuous,
        })
        .expect("register");

    store.write_receipt(&receipt(&tx, &gen_a, ReleaseContextMode::OrdinaryContinuous)).expect("A");
    store.write_receipt(&b_receipt).expect("B");

    // Swap persisted bytes: B's valid receipt at A's filename must not be
    // returned as A's receipt. The filename stem is the bijective ':' ->
    // '-' substitution used by the store layout.
    let generations_dir = dir.path().join("transactions").join(tx.as_str()).join("generations");
    let b_file = generations_dir.join(format!("{}.json", gen_b.as_str().replace(':', "-")));
    let a_file = generations_dir.join(format!("{}.json", gen_a.as_str().replace(':', "-")));
    assert!(a_file.exists(), "receipt A must be on disk at its stem");
    assert!(b_file.exists(), "receipt B must be on disk at its stem");
    let b_bytes = std::fs::read(&b_file).expect("read B receipt bytes");
    std::fs::write(&a_file, b_bytes).expect("place B receipt at A's location");

    let swapped = store.read_receipt(&tx, &gen_a);
    assert!(
        matches!(swapped, Err(StoreError::Malformed(_))),
        "a receipt for another generation at this filename must be refused"
    );

    // The correctly placed receipts still load.
    let ok_b = store.read_receipt(&tx, &gen_b).expect("B at B loads");
    assert_eq!(ok_b.generation_id, gen_b);

    // Restore A's own bytes so a same-location read succeeds too.
    std::fs::write(&a_file, {
        let restored = receipt(&tx, &gen_a, ReleaseContextMode::OrdinaryContinuous);
        serde_json::to_vec_pretty(&GenerationReceiptFile {
            schema_version: GENERATION_RECEIPT_SCHEMA_VERSION,
            receipt: restored,
        })
        .expect("canonical A bytes")
    })
    .expect("restore A receipt");
    let ok_a = store.read_receipt(&tx, &gen_a).expect("A at A loads after restore");
    assert_eq!(ok_a.generation_id, gen_a);
}

//! Production correspondence for the as-is TLC resource-rejection pilot.
//!
//! These tests use the operational IndexCoordinator and its public index/query
//! APIs, never the separate state_machine implementation. They intentionally do
//! not establish pure observation, operation tickets, generations or concurrent
//! linearizability; see tests/formal/workspace_index_resource_rejection.binding.json.

use anyhow::{Result, anyhow};
use perl_workspace::{
    monitoring::{
        DegradationReason, IndexResourceLimits, IndexStateKind, IndexStateTransition, ResourceKind,
    },
    workspace::workspace_index::{IndexCoordinator, IndexState},
};
use url::Url;

fn fixture() -> Result<(IndexCoordinator, Url, Url)> {
    let limits = IndexResourceLimits { max_files: 1, ..IndexResourceLimits::default() };
    let coordinator = IndexCoordinator::with_limits(limits);
    let accepted = Url::parse("file:///resource-model-accepted.pl")?;
    let rejected = Url::parse("file:///resource-model-rejected.pl")?;
    coordinator
        .index()
        .index_file(accepted.clone(), "sub accepted_before { }".to_string())
        .map_err(|error| anyhow!(error))?;
    coordinator.transition_to_ready(
        coordinator.index().file_count(),
        coordinator.index().symbol_count(),
    );
    assert!(
        coordinator.query(|_| true, |_| false),
        "the control must actually reach the full-query route before rejection"
    );
    Ok((coordinator, accepted, rejected))
}

fn assert_rejection(coordinator: &IndexCoordinator, accepted: &Url, rejected: &Url) {
    assert_eq!(coordinator.index().file_count(), 1);
    assert!(!coordinator.index().file_symbols(accepted.as_str()).is_empty());
    assert!(coordinator.index().file_symbols(rejected.as_str()).is_empty());
    assert!(!coordinator.index().document_store().is_open(rejected.as_str()));
    assert!(matches!(
        coordinator.state(),
        IndexState::Degraded {
            reason: DegradationReason::ResourceLimit { kind: ResourceKind::MaxFiles },
            ..
        }
    ));
}

#[test]
fn first_resource_rejection_observer_routes_partial_and_preserves_accepted_file() -> Result<()> {
    // Both first-observer paths are exercised from a fresh production instance.
    for state_first in [false, true] {
        let (coordinator, accepted, rejected) = fixture()?;
        let before = coordinator.index().file_symbols(accepted.as_str());
        assert!(
            coordinator
                .index()
                .index_file(rejected.clone(), "sub rejected_symbol { }".to_string())
                .is_err(),
            "new-file admission at capacity must reject"
        );

        if state_first {
            assert!(matches!(coordinator.state(), IndexState::Degraded { .. }));
        }
        let (full_route, symbols) = coordinator.query(
            |index| (true, index.file_symbols(accepted.as_str())),
            |index| (false, index.file_symbols(accepted.as_str())),
        );
        assert!(!full_route, "the first query after rejection must not use full readiness");
        let before_names: Vec<_> = before.iter().map(|symbol| symbol.name.as_str()).collect();
        let after_names: Vec<_> = symbols.iter().map(|symbol| symbol.name.as_str()).collect();
        assert_eq!(before_names, after_names, "rejection must preserve accepted symbols");
        assert_rejection(&coordinator, &accepted, &rejected);
    }
    Ok(())
}

#[test]
fn quiescent_observations_do_not_repeat_degradation_or_block_existing_file_update() -> Result<()> {
    let (coordinator, accepted, rejected) = fixture()?;
    assert!(
        coordinator
            .index()
            .index_file(rejected.clone(), "sub rejected_symbol { }".to_string())
            .is_err()
    );
    assert_rejection(&coordinator, &accepted, &rejected);

    let first = coordinator.instrumentation_snapshot().state_transition_counts;
    assert_eq!(
        first.get(&IndexStateTransition {
            from: IndexStateKind::Ready,
            to: IndexStateKind::Degraded,
        }),
        Some(&1),
        "the fixture must observe the real rejection transition"
    );
    for _ in 0..3 {
        assert!(matches!(coordinator.state(), IndexState::Degraded { .. }));
        assert!(!coordinator.query(|_| true, |_| false));
    }
    assert_eq!(
        first,
        coordinator.instrumentation_snapshot().state_transition_counts,
        "quiescent reads must not consume the same rejection again"
    );

    coordinator
        .index()
        .index_file(accepted.clone(), "sub accepted_after { }".to_string())
        .map_err(|error| anyhow!(error))?;
    let symbols = coordinator.index().file_symbols(accepted.as_str());
    assert!(symbols.iter().any(|symbol| symbol.name == "accepted_after"));
    assert!(!symbols.iter().any(|symbol| symbol.name == "accepted_before"));
    assert_rejection(&coordinator, &accepted, &rejected);
    assert!(!coordinator.query(|_| true, |_| false));
    assert_eq!(first, coordinator.instrumentation_snapshot().state_transition_counts);
    Ok(())
}

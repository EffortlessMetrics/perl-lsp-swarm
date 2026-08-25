# Acceptance: #11673

| Obligation | Evidence |
| --- | --- |
| Fast-path publish re-validates accepted ticket at the outbound boundary | `publish_parse_errors_fast` routes its enqueue through `commit_push_diagnostics`; ABA falsifier test |
| Full/syntax-only publish validates document-instance identity, not just the counter value | close/reopen discriminator test (old instance passes value-only check, fails instance check) |
| Stale-N candidate cannot enqueue after N+1 acceptance | late-callback test with generation advanced between snapshot and boundary |
| Committed sequence is monotonic per normalized URI; a regressed callback is rejected without a frame | sink ledger regression unit test |
| Guarded no-parse paths carry the inserted document's identity through the same boundary | template/oversize/binary guard paths use `commit_push_diagnostics` |
| Outcomes are typed and truthful (`CommittedCurrent`, `SafeClearCommitted`, exact rejections, outbound failure) | `PushDiagnosticsCommitOutcome` + tests observing rejection paths |
| No diagnostic algorithm, severity, pull protocol, debounce-scheduling, or parser change | diff limited to publication boundary + tests; existing push/pull/debounce suites stay green |

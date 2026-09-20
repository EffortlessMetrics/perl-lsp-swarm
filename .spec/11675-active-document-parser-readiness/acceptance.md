# Acceptance: #11675

| Obligation | Evidence |
| --- | --- |
| One generation-owned active-document parser-readiness state exists | `ActiveDocumentParserReadiness` per-URI entries keyed by `(instance, generation)` |
| Acceptance and required-effect completion are distinct stages | `PendingParser` -> `ParserStateAcceptedEffectsPending` transition only at snapshot publication |
| Every required effect must name the exact accepted ticket | effect attach rejects stale generation / wrong instance / unknown entry |
| Clean vs recovered/limited vs failure/guard remain distinct | `ParserCoreReady` / `RecoveredOrLimitedReady` / `UnavailableTerminal` / `Guarded` with limitation carried |
| Queue settlement, counters, worker completion, workspace readiness cannot mint document readiness | old index-task emission sites removed; no other minting path |
| Newer generations / close invalidate prior readiness honestly | install supersedes; eviction removes |
| Notification is a projection, not the state | frame emitted only inside the readiness mint; order-witness falsifier (red on main: ready@59 < publishDiagnostics@212) |
| No provider policy, semantic/index, scheduler, or support change | diff limited to readiness module, text-sync install points, sink attach hooks, tests, `.spec` |

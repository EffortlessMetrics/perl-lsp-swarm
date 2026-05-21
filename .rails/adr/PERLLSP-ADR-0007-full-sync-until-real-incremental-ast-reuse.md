# PERLLSP-ADR-0007 — Full Sync Until Real Incremental AST Reuse

## Decision
Keep advertising full text synchronization until true incremental AST reuse is implemented and proven.

## Consequences
- Track D does not switch TextDocumentSyncKind.
- First rail focuses on avoidable latency, latest-only behavior, cancellation, and measurement.
- Incremental parse architecture remains a follow-up rail.

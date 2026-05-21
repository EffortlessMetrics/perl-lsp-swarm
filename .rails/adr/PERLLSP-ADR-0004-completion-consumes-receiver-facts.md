# PERLLSP-ADR-0004 — Completion consumes receiver facts

## Status
Accepted

## Decision
Method completion prefers semantic `ReceiverFact` evidence over text-pattern heuristics.

Text-pattern heuristics are fallback-only until equivalent receiver classes are fixture-backed and receipt-proven.

## Consequences
- Receiver-class promotion is receipts-gated.
- DBI/framework/class-return paths migrate into fact substrate over time.
- Exact behavior claims must cite semantic evidence labels.

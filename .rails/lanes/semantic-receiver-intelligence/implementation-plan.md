# Track C implementation plan

## One-liner
Track C turns TypeFact/ReceiverFact substrate into safe, source-backed, evidence-labeled receiver method completion.

## Sequence
1. C1 rail/spec/ADR/lane scaffolding (no behavior change).
2. C2 promotion ledger + receipt schema.
3. C3/C4 receiver-class exact cutovers (hashref slot, array index) with receipts.
4. C5-C8 richer inference inputs (fact-first classification, AST extraction, value facts, scoped env snapshots).
5. C9-C14 controlled promotions and freshness gates.
6. C15 demote legacy heuristics behind fact-backed paths.

## Promotion policy
No receiver class moves to exact until exact/fallback/dynamic evidence exists for that class and safety gates pass.

## Safety gates
Dynamic, stale, ambiguous, low-confidence, and unsupported generated/no-source facts remain fallback/blocked.

## Non-goals
Track A/B parser concerns and unrelated latency/release work.

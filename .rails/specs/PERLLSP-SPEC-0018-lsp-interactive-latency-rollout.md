# PERLLSP-SPEC-0018 — LSP Interactive Latency Rollout

## Scope
Umbrella rollout spec for Track D.

## Core requirement
The live edit/open path should pay only for work needed to make the latest document state coherent. Everything else should be versioned, latest-only, deferred, cancellable, measured, and budgeted.

## Requirement set
- R0: Define track ownership boundaries (A/B/C/D).
- R1: Capture interactive latency problem statement with live-editor context.
- R2: Keep text sync Full until proven incremental AST reuse exists.
- R3: Add runtime tuning model for normal vs e2e workloads.
- R4: Add diagnostics latest-only model and stale-drop behavior.
- R5: Add generation-aware cancellation on read requests.
- R6: Align semantic-token capability contract with real implementation.
- R7: Add receipt schema and latency budget policy.
- R8: Add advisory verification lane.
- R9: Add slow-path admission policy for critical runtime surfaces.
- R10: Define claim boundaries for each receipt.
- R11: Keep first rail to avoidable-work removal and measurement primitives.
- R12: Sequence implementation in focused PRs (D1+).
- R13: Explicitly separate follow-up rails from first rollout.
- R14: No behavior changes in D1 documentation PR.

## PR sequence
D1–D19 as listed in lane implementation plan.

## Exit criteria
- Interactive latency rail artifacts exist under `.rails/`.
- Rollout docs are present and coherent.
- Follow-up rails are pre-scoped.
- Claim boundaries and no-code-change posture are explicit for D1.

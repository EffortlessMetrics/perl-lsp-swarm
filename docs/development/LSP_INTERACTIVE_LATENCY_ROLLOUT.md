# LSP Interactive Latency Rollout (Track D)

## One-line intent
Track D turns perl-lsp from “correct but editor-heavy” into a measured, latest-only, cancellable runtime that gives fast first-useful editor feedback.

## Track ownership split
- Track A owns parser-target fairness.
- Track B owns production parser edge-gap closure.
- Track C owns semantic receiver intelligence.
- Track D owns LSP runtime/editor latency.

## Why now
The active issue is live Neovim/editor responsiveness under realistic edit loops, not parser correctness alone. Live harnesses measure pipeline behavior across open/change/diagnostics/scheduling/indexing, not single-request correctness in isolation.

## First rollout posture (D1+)
This first rail removes avoidable runtime work and adds receipts/budgets/regression checks. It does **not** claim true incremental AST reuse.

## Technical guardrails
- Text synchronization remains `Full` until real incremental AST reuse is implemented and proven.
- E2E mode is runtime workload tuning, not a feature-profile claim.
- Dynamic/stale/intermediate work should be latest-only, cancellable, deferred, or measured.
- D1 documentation PR introduces no code behavior changes.

## Planned sequence
See `.rails/lanes/lsp-interactive-latency/implementation-plan.md` and linked Track D specs for D1–D19 sequencing and follow-up rails.

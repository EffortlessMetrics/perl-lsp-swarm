# PERLLSP-PROP-0004 — LSP Interactive Latency Rail

## Summary
Track D establishes the editor/runtime latency rail for perl-lsp. It addresses live Neovim and raw JSON-RPC responsiveness by prioritizing latest-only, cancellable, deferred, and measurable runtime work.

## Track boundaries
- Track A owns parser-target fairness.
- Track B owns production parser edge-gap closure.
- Track C owns semantic receiver intelligence.
- Track D owns LSP runtime/editor latency.

## Problem statement
The current pain point is live editor responsiveness (especially Neovim harnesses), not parser correctness alone. A live session includes didOpen/didChange pipelines, diagnostics, scheduler wait, workspace activity, and request interleaving.

## Decision
Create a dedicated `.rails/` lane and spec set for interactive latency rollout. The first phase removes avoidable work, introduces runtime tuning, and adds measurable receipts and budgets.

## Non-goals in this proposal
- No true incremental AST reuse implementation.
- No text sync mode switch away from Full.
- No parser-correctness rail changes.

## Invariants
- Text sync remains Full until real incremental AST reuse is implemented and proven.
- E2E mode is runtime workload tuning, not a feature-profile claim.
- Dynamic/stale/intermediate work must be latest-only, cancellable, deferred, or measured.
- This PR introduces no code behavior changes.

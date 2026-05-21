# Implementation Plan — Track D (LSP Interactive Latency)

## Mission
Make perl-lsp fast and predictable in live editors by removing avoidable work and enforcing latest-only, cancellable, measurable runtime behavior.

## Phase 1 (rollout)
D1–D19 as defined in Track D sequence, beginning with no-code rail scaffolding and ending with follow-up rail kickoff.

## Guardrails
- Current issue: live editor/Neovim latency, not parser correctness alone.
- First rail removes avoidable work and adds receipts; no true incremental AST reuse yet.
- Text sync remains Full until true incremental AST reuse is implemented and proven.
- E2E mode is runtime workload tuning, not a feature-profile claim.
- Dynamic/stale/intermediate work must be latest-only, cancellable, deferred, or measured.
- D1 includes no code behavior changes.

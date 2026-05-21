# PERLLSP-ADR-0006 — Editor Latency Is a Product Surface

## Decision
Interactive LSP latency is a product surface with budgets, receipts, and slow-path admission rules.

## Consequences
- Correctness-only checks are insufficient for runtime quality.
- Latency receipts become review artifacts.
- E2E/lean mode is a supported harness mode.

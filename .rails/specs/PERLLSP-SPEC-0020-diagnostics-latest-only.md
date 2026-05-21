# PERLLSP-SPEC-0020 — Diagnostics Latest-Only

## Goal
Ensure diagnostics compute/publish behavior only reflects current document generation.

## Requirements
- Pull-diagnostic clients do not trigger discarded push diagnostics computation.
- didOpen publishes parse-errors-fast and schedules debounced full diagnostics.
- didOpen does not synchronously run full diagnostics.
- Full diagnostics are generation-aware and stale results are discarded.
- Syntax diagnostics remain fast in normal and syntax-only modes.

## Constraints
No broad parser-correctness rework; this is runtime pipeline behavior.

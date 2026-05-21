# PERLLSP-SPEC-0019 — Runtime Tuning and E2E Mode

## Goal
Define workload-tuning controls for latency-focused harnesses.

## Model
- RuntimeMode: Normal | E2e
- DiagnosticMode: Normal | SyntaxOnly
- RuntimeTuning:
  - runtime_mode
  - diagnostic_mode
  - diagnostic_debounce_ms
  - eager_workspace_indexing
  - file_watchers

## Inputs
- `PERL_LSP_E2E=1`
- `PERL_LSP_DIAGNOSTIC_DEBOUNCE_MS=0`
- `PERL_LSP_DIAGNOSTIC_MODE=syntax-only`
- `--runtime-mode e2e`
- `--diagnostic-debounce-ms 0`
- `--diagnostic-mode syntax-only`

## E2E defaults
- diagnostic debounce 0
- syntax-only diagnostics
- eager workspace indexing disabled
- file watchers disabled unless explicitly enabled
- reduced startup noise

## Guardrail
E2E mode is runtime workload tuning, not a feature-profile claim.

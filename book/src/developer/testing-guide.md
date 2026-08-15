# Comprehensive Testing Guide

This guide routes contributors to the current testing surfaces without turning old test counts or coverage claims into current status.

## Choose the evidence surface

- Parser behavior: package tests and [parser-accuracy fixtures](https://github.com/EffortlessMetrics/perl-lsp-swarm/tree/main/crates/perl-corpus/).
- LSP behavior: `perl-lsp-rs-core` tests, protocol fixtures, and feature-policy evidence.
- DAP behavior: `perl-dap` tests and the current DAP status/scorecard.
- Repository policy: `cargo xtask` checks and the required PR gate.
- Release behavior: packaged-artifact and channel receipts, not workspace tests alone.

## Focused commands

```bash
cargo test -p perl-parser
cargo test -p perl-parser-core
cargo test -p perl-lsp-rs-core
cargo test -p perl-dap
cargo fmt --all -- --check
cargo xtask fmt --check
```

Use the repository [commands reference](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/docs/reference/COMMANDS_REFERENCE.md) and [contributing guide](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/CONTRIBUTING.md) for the current required gate and any lane-specific commands.

## Claim discipline

State the exact package, fixture or protocol subject, source revision, command, and result. Keep baseline failures, flakes, skipped work, and not-proven execution separate from evidence produced by the change.

A focused test proves its subject. It does not by itself prove full language coverage, editor behavior, release readiness, or performance.

# Development Guide

This is the contributor route for the current Rust workspace. It does not define parser coverage, release readiness, or agent/runtime policy; those claims belong to the linked authorities.

## Start with the current sources

- [Contributing guide](../../CONTRIBUTING.md) — repository workflow and review requirements.
- [Project orientation](ORIENTATION.md) — current package ownership and narrow validation routes.
- [Architecture reference](../reference/ARCHITECTURE.md) — ownership seams.
- [Commands reference](../reference/COMMANDS_REFERENCE.md) — supported commands.
- [LSP development guide](../tutorials/LSP_DEVELOPMENT_GUIDE.md) — LSP-specific implementation and evidence workflow.
- [Current status](CURRENT_STATUS.md) and [roadmap](ROADMAP.md) — current evidence and planned work.

The workspace manifest is authoritative for membership and exclusions. Package READMEs and source are authoritative for narrower implementation details.

## Focused workflow

1. Read the issue or spec and define the behavior, boundary, and proof needed.
2. Select the owning package from the architecture reference and manifest.
3. Make the smallest implementation, test, fixture, or documentation change that closes the slice.
4. Add executable evidence for the claim: a focused test, corpus expectation, receipt, or contract check.
5. Run the narrowest relevant package checks, formatting, and repository-required gates.
6. Record baseline failures and not-proven results separately from evidence produced by the change.
7. Re-review the current PR head after material edits.

Keep parser, LSP, DAP, packaging, and documentation claims separate. A parser test does not establish LSP behavior; a capability entry does not establish implementation; a fixture selection does not establish complete Perl coverage.

## Current ownership map

- AST structure and methods: `crates/perl-ast`
- Parsing, positions, trivia, and recovery: `crates/perl-parser-core`
- Public parser facade: `crates/perl-parser`
- Semantic and workspace analysis: `crates/perl-semantic-analyzer`, `crates/perl-workspace`
- LSP protocol, shared runtime infrastructure, governance, and providers: `crates/perl-lsp-rs-core`
- LSP server scheduling, serving, workspace readiness, document lifecycle, and dispatch: `crates/perl-lsp-rs`
- Server integration: `crates/perl-lsp-rs`
- Public binary: `crates/perllsp`
- Native DAP: `crates/perl-dap`
- Parser-accuracy fixtures and manifests: `crates/perl-corpus`

Former microcrates may now be modules inside surviving packages. Confirm the current path before editing.

## Focused commands

```bash
cargo check --workspace
cargo test -p perl-parser
cargo test -p perl-parser-core
cargo test -p perl-lsp-rs
cargo test -p perl-dap
cargo fmt --all -- --check
cargo xtask fmt --check
```

Use the commands reference and package-local guidance before broader release or corpus workflows.

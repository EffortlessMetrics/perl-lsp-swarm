# Verification Protocol

> Process policy — not current state. Edit when process changes, not when metrics change.

## Tier A: Merge Gate (required for all merges)

```bash
just ci-gate  # ~2-5 min
```

## Tier B: Release Confidence (large changes or release candidates)

```bash
just ci-full  # ~10-20 min
```

`just ci-full` is the automated release-confidence lane. It should include the
broader integration surfaces that are too expensive for Tier A, including the
`perl-lsp-ux-tests` first-5-minutes workflow harness.

## Tier C: Real User Confirmation

Manual editor smoke test: diagnostics, completion, hover, go-to-definition, rename

## Metric Definitions

**LSP Metrics** (computed from `features.toml` by `cargo xtask update-status`):

| Metric | Formula | Meaning |
| --- | --- | --- |
| **LSP catalog rows (navigation)** | declaration counts from `features.toml` maturity/advertised labels | Navigation only — a declaration count is not behavior proof (#6731) |

Since #6731 the generated LSP status publishes no coverage/compliance percentage
and no passing verdict: rows without an exact current behavior-evidence owner
render `not_proven`, never inherited green. Behavior-backed GA/compliance claims
are owned by #6731's evidence model.

Key terms:

- `declared ga/production`: Features with `maturity in (ga, production)`
- `coverage-tracked`: Features where `advertised = true`, `maturity != planned`, and `counts_in_coverage != false`
- `declared ga/production/preview`: Features with `maturity in (ga, production, preview)` (protocol-surface denominator includes every catalog row)
- `counts_in_coverage = false`: Protocol plumbing excluded from navigation counts

**Other Metrics**:

- **Corpus counts**: `tree-sitter-perl/test/corpus` sections + `test_corpus/*.pl` files
- **Catalog source**: root `features.toml` is canonical

## Truth Contract

All claims require evidence from:
- `Cargo.toml` (`workspace.package.version`) for the current release line
- `nix develop -c just ci-gate` output
- `bash scripts/ignored-test-count.sh` output
- `features.toml`, capability snapshots, or targeted tests

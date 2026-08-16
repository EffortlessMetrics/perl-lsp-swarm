# CI Hardening Status

## Snapshot (2026-08-14)

This page records the CI-hardening status observed against the repository's current
`main` branch on **2026-08-14**. It is a bounded status summary, not a replacement for
live GitHub checks, reviews, rulesets, or issue ownership.

## Current posture

1. **Shared fast-gate path**
   - The repository exposes the shared `pr-fast` gate path through `just pr-fast` and
     `cargo xtask gates --tier pr-fast --receipt`.
   - This page does not claim that a hosted run is currently green; hosted readiness
     remains a live-check question.

2. **Status and receipt contracts**
   - Status validation is available through `just status-check`.
   - The UX and status-marker surfaces remain repository contracts, but this page does
     not attach stale issue numbers to those claims.

3. **Historical `update-status` streaming concern**
   - The long-running inactivity concern is recorded historically by closed issues
     [#785](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/785) and
     [#2751](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/2751).
   - Current runtime streaming behavior and end-to-end hosted validation are
     **NOT_PROVEN** by this status page.

4. **Review receipts and merge-train proposals**
   - The older review-receipt projection and merge-train proposals are not current
     open work for this page. Their historical issue references are intentionally not
     repeated here because GitHub issue numbers have since been reused or superseded.
   - Current candidate identity, review convergence, command evidence, and protected
     integration contracts are owned by their live GitHub artifacts and must be
     re-derived before any merge decision.

## Exact verification commands

Run these from the repository root on current `main`:

```bash
# Shared pr-fast execution path
just pr-fast

# Direct xtask gate invocation used by shared runners
cargo xtask gates --tier pr-fast --receipt

# Status marker / status docs contract check
just status-check

# Optional: regenerate + validate status outputs when touching status docs
just status-update
just status-check

# Canonical local merge receipt
nix develop -c just ci-gate
```

The commands above are proof entry points, not evidence that they passed in this
snapshot. Runtime, hosted-CI, merge, and release claims require fresh receipts.

## Known non-goals (current wave)

- Making Parser Ratchet a hard required merge gate before CI receipt semantics stabilize.
- Treating every `SKIPPED` check as pass/fail without policy context.
- Reverting to workflow-specific hand-written `pr-fast` command stacks.
- Using admin-merge shortcuts as normal CI flow.
- Reconstructing a merge-train or reconciler lifecycle authority from this document.

## Evidence boundary

This document establishes only the current documentation posture and the commands that
can be used to produce fresh evidence. It does not establish update-status streaming
correctness, workflow-trigger completeness, current required checks, hosted CI health,
review sufficiency, mergeability, or release readiness.

## Source notes

- Planning baseline: [`docs/project/CI_HARDENING_NEXT_WAVES.md`](../CI_HARDENING_NEXT_WAVES.md).
- Live branch and issue/PR state take precedence over this summary.

# Merge-train protocol (operator-run, no auto-merge)

This protocol defines a **bounded merge-train check-and-merge flow** so batch/admin merges test candidates in an explicit order before landing on `main`.

It is intentionally an **operator playbook**, not a merge bot:

- no automatic merging
- no admin bypass requirement
- no CI weakening
- reusable with existing `xtask`/`just` gates

## Inputs

For each candidate PR, collect:

- PR number
- head SHA (exact)
- labels/state snapshot
- mergeability status (clean/conflicting)
- latest CI verdict

Use existing queue tooling to gather queue context before train construction:

```bash
cargo xtask queue-snapshot --out target/queue-snapshot.json
cargo xtask queue-health --fixture xtask/tests/fixtures/queue-health/master-green.json # fixture mode example
```

Use the protected merge preflight and current GitHub facts; do not reconcile
readiness through lifecycle labels.

## Candidate requirements

A PR is train-eligible only if all required conditions hold at planning time:

1. **Current head SHA known** and recorded in the train plan.
2. **The canonical review-convergence verdict is current**, with no pending
   review requests, stale human reviews, current-head `CHANGES_REQUESTED`, or
   unresolved threads (including outdated threads).
3. **Required checks are green**, or an expected-skip result is explicitly explained by live policy.
4. **Mergeable now**, or explicitly marked as **intentionally ordered** behind a PR expected to resolve dependency/conflict ordering.
5. **A valid merge-ready receipt is present** for the candidate head, exact
   `main` base lineage, gate-graph version, required checks, and review evidence.

If any candidate violates these requirements, the train is not launched.

## Train sizing rules

Build trains using one of these bounded profiles:

- **3 PRs**: overlapping/high-risk cluster
- **5 PRs**: normal code cluster
- **10 PRs**: docs/leaf, non-overlapping cluster

Do not mix profiles inside one train run. If scope diverges, split into multiple trains.

## Train execution checks (required)

Start from **current green `main`**.

For the planned PR order:

1. Check out/update base to the current green `main` tip.
2. Apply/simulate each PR in order (local merge/cherry-pick simulation is sufficient).
3. Run conflict-marker check.
4. Run formatting check.
5. Run PR-fast gate with receipt.

Required commands:

```bash
# conflict markers
just check-conflict-markers

# fmt check
cargo xtask fmt --check

# shared PR-fast gate
cargo xtask gates --tier pr-fast --base origin/main --receipt
```

If a temporary integration branch/worktree is used, it must be disposable and never force-pushed over `main`.

## Stop conditions (hard fail)

Abort the train immediately on any of the following:

- merge/apply conflict
- stale candidate SHA (candidate head changed since plan creation)
- failed conflict-marker/fmt/pr-fast check
- unexpected skip outcome (skip not explained by expected path conditioning)
- base branch no longer green (new red `main` signal)

On stop, do not continue with remaining candidates.

## Required output: train receipt/summary

Each train run must emit a markdown summary (ticket comment, runbook entry, or PR-thread note) containing:

- candidate list with PR numbers + SHAs
- planned order
- executed checks and outcomes
- final verdict (`pass`/`stop`)
- explicit stop reason (if stopped)

Suggested template:

```md
## Merge Train Receipt

- Base: `main@<sha>` (green at plan time)
- Profile: `3-overlap | 5-normal | 10-docs-leaf`
- Candidates:
  1. #1234 @ <sha>
  2. #1235 @ <sha>
  3. #1236 @ <sha>
- Checks:
  - `just check-conflict-markers`: pass|fail
  - `cargo xtask fmt --check`: pass|fail
  - `cargo xtask gates --tier pr-fast --base origin/main --receipt`: pass|fail
- Verdict: pass|stop
- Stop reason: <none|conflict|stale-sha|failed-check|unexpected-skip|main-red>
```

## Recommended operator flow

1. Capture one protected-preflight/current-GitHub-facts packet per candidate.
2. Select candidates whose head, review, required checks, and mergeability facts satisfy the requirements.
3. Choose train profile (3/5/10).
4. Build an ordered plan with explicit SHAs.
5. Simulate/apply in a disposable integration branch/worktree from green `main`.
6. Run required checks.
7. Immediately before each manual merge, capture a fresh protected-preflight
   packet and compare the candidate head, `main` base, review convergence,
   required checks, and mergeability with the planned receipt. Stop and refresh
   the candidate if any material fact moved.
8. If pass, perform manual merges in tested order.
9. Post train receipt summary.

This keeps the integration order explicit while preserving human control.

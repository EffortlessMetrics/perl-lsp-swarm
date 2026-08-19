# ripr — Static Oracle-Gap Detection

> **Context**: This document is part of perl-lsp's [Industrialized AI](why-industrialized.md) CI architecture. The choices here are responses to operating at 1000+ PRs/day, not premature optimization.

> **Doctrine**: `ripr` is static mutation-exposure analysis. It catches the
> same class of findings mutation testing catches — weak test/oracle
> exposure — but earlier and cheaper because it is static and PR-time.
> Mutation testing remains the slower runtime backstop for findings that
> static analysis cannot predict. `ripr` shifts mutation signal left.

`ripr` adds **mutation-testing-lite oracle-gap detection at static-analysis prices**. It
sits between coverage and runtime mutation testing on the verification ladder: more
oracle-aware than coverage, far cheaper than mutation testing.

> Companion: [verification-ladder.md](verification-ladder.md),
> [labels.md](labels.md), [`policy/ripr-suppressions.toml`](../../policy/ripr-suppressions.toml).
> Workflow: [`.github/workflows/ripr.yml`](../../.github/workflows/ripr.yml).

---

## Current routing posture

During the proof-lane rollout, the `ripr` workflow blocks PRs that introduce
named new diff-scoped gaps in changed production files or fail to produce
current receipts. This is one required deterministic contract: current diff,
review, and Repo-wide receipts must be generated and validated. The
static `ripr` sensor itself remains advisory (`policy/ub-review.toml`); a sensor
availability failure is not treated as a clean result or as a receipt-integrity
pass. Non-production-only static findings remain visible in the receipts but do
not create a merge-blocking repair packet. Repo-wide RIPR+ total zero remains a burn-down target
until the final enforcement slice. The workflow still runs for
every PR when it is ready for review so docs-only, policy-only, workflow-only,
and code PRs all carry current proof receipts.

## What ripr does

For each changed Rust function, ripr asks the mutation-testing-shaped question
**statically**: is the changed behavior exposed to a meaningful test discriminator?

It does **not**:

- Run mutants.
- Emit `killed` / `survived` counts.
- Replace mutation testing.

When reporting `ripr` findings, use ripr's own classifications:

| Classification | Meaning |
|---|---|
| `exposed` | reachable + nearby discriminating test |
| `weakly_exposed` | reachable, weakly-discriminating test only |
| `reachable_unrevealed` | reachable, no discriminating test found |
| `no_static_path` | analysis could not find a reachable path |
| `infection_unknown` | could not classify infection |
| `propagation_unknown` | could not classify propagation |
| `static_unknown` | analysis bottomed out |

Do **not** translate these into `killed` / `survived`. They mean something different.

---

## When it runs

- Every PR targeting `master` or `main`.
- No path filter is applied: docs-only, policy-only, workflow-only, and code
  PRs all run the RIPR proof workflow so every merge has current proof receipts.
- Draft PRs are skipped while draft, then the explicit `ready_for_review`
  trigger runs the workflow before the PR can merge.
- Manual via `workflow_dispatch`.
- Labels can still route deeper or more expensive evidence lanes elsewhere, but
  this proof workflow is no longer label-gated.

---

## Behavior

- Produces diff-scoped PR evidence under `target/ripr/pr/`.
- Produces the repo-wide RIPR+ baseline receipt at
  `target/receipts/quality/ripr-plus.json`.
  The repo-wide receipt applies `policy/ripr-suppressions.toml` path
  suppressions before computing the unresolved total so non-production retained
  surfaces such as `archive/**` do not count against the final zero target.
- Produces review guidance under `target/ripr/review/`.
- Runs `cargo xtask quality-gate --mode enforce-new-ripr`, which blocks new
  named severe RIPR gaps in changed production files and stale or missing
  repo-wide, diff-scoped, or review-guidance receipts. Identity attribution
  lives on the upstream receipts: `ripr_pr.receipt_head_sha` is the PR head,
  `head` is the evaluated merge-test SHA, and `review_guidance.changed_production_files`
  records the production scope used for the required-vs-advisory decision.
- Applies the documented suppression policy to diff-scoped PR evidence as well
  as repo-wide RIPR+ receipts. Suppressed paths remain visible in receipts, but
  they do not count as new blocking gaps.
- In CI, review guidance has an explicit timeout. When the guidance pass does
  not finish, the harness emits an `incomplete` receipt that names the
  gate-actionable seams (`reachable_unrevealed` / `no_static_path`) from the
  completed diff-scoped raw check, so the quality gate blocks on named
  file/line/seam evidence instead of an unnameable count (#10054). The
  suggested-proof text on these fallback names is generic, not
  analyzer-derived, and the receipt warning says so. Only when the raw check
  is also unavailable does the gate report the missing repair packet.
- Emits non-blocking warning annotations from `comments[]` only.
- Produces mutation-routing evidence under
  `target/xtask/impacted-evidence/`.
- Uploads the `ripr-pr-evidence` artifact with required-artifact semantics.
- Appends `target/ripr/pr/summary.md` and
  `target/receipts/quality/quality-gate-ripr.md` to the GitHub step summary.

The evaluated `HEAD` in CI is a merge-test ref. It is not silently presented as
the contributor's PR head: PR runs pass both identities to `xtask`, while
`merge_group` runs record a null PR head because one queue ref can contain more
than one PR.

---

## Suppressions

Suppressions live in [`policy/ripr-suppressions.toml`](../../policy/ripr-suppressions.toml).
Each suppression requires:

- `id` — stable identifier
- `kind` — e.g. `generated_or_non_production_surface`
- `paths` and/or `classification` — what to suppress
- `owner` — accountable person/team
- `reason` — why this is suppressed
- `created`, `review_after`, `expires` — dates

The suppression file is read by `ripr.toml`'s `[suppressions] path` setting.
The `cargo xtask ripr-pr` and `cargo xtask ripr-plus` wrappers also apply the
same path suppressions when they compute diff-scoped and repo-wide receipts.
Suppressed files are reported separately from active unresolved gaps.

Current suppressed non-production proof surfaces include:

- archived source under `archive/**`;
- generated status docs under `docs/project/status/**`;
- executable editor UX receipt tests under
  `crates/perl-lsp-ux-tests/tests/**`.

---

## Promotion path

| PR | What happens |
|---:|---|
| 1 | Unfiltered ready-for-review workflow routing plus current RIPR receipts; no CI enforcement. |
| 8 | Blocking new-gap gate for diff-scoped RIPR PR evidence and receipt freshness. |
| Later | Total RIPR+ unresolved count reaches zero, then full `quality-gate --mode enforce` becomes blocking. |

This slice blocks new RIPR gaps and stale or missing RIPR proof receipts. It does
not require repo-wide RIPR+ total zero; that remains exception-backed until the
burn-down closes.

---

## Toolchain

`rust-toolchain.toml` pins `1.95.0`. The workflow installs `ripr` `0.9.0` as
the current advisory version for this lane.

---

## Running locally

```bash
cargo install ripr --version 0.9.0 --locked
ripr doctor
cargo xtask ripr-pr --base origin/HEAD --head HEAD --pr-head "$PR_HEAD_SHA"
cargo xtask ripr-plus --receipt target/receipts/quality/ripr-plus.json
cargo xtask ripr-review-comments --base origin/HEAD --head HEAD --pr-head "$PR_HEAD_SHA"
cargo xtask impacted-evidence
cargo xtask ripr-pr-summary
cargo xtask ripr-annotations
cargo xtask quality-gate --mode enforce-new-ripr --ripr-receipt target/receipts/quality/ripr-plus.json --ripr-pr-receipt target/ripr/pr/repo-exposure.json --review-receipt target/ripr/review/comments.json --ripr-base origin/HEAD --ripr-head HEAD --receipt target/receipts/quality/quality-gate-ripr.json --summary target/receipts/quality/quality-gate-ripr.md
cargo xtask ripr-pr --base origin/HEAD --head HEAD --check --pr-head "$PR_HEAD_SHA"
cargo xtask ripr-plus --receipt target/receipts/quality/ripr-plus.json --check
cargo xtask ripr-review-comments --base origin/HEAD --head HEAD --check --pr-head "$PR_HEAD_SHA"
cargo xtask impacted-evidence --check
cargo xtask ripr-pr-summary --check
cargo xtask ripr-annotations --check
cargo xtask quality-gate --mode enforce-new-ripr --ripr-receipt target/receipts/quality/ripr-plus.json --ripr-pr-receipt target/ripr/pr/repo-exposure.json --review-receipt target/ripr/review/comments.json --ripr-base origin/HEAD --ripr-head HEAD --receipt target/receipts/quality/quality-gate-ripr.json --summary target/receipts/quality/quality-gate-ripr.md --check
```

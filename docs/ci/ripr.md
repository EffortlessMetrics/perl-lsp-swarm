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

The `ripr` workflow now runs for every non-draft PR and reruns when a draft PR
becomes ready for review. This slice establishes current repo-wide, diff-scoped,
and review-guidance RIPR receipts for every merge candidate. Blocking
`quality-gate` enforcement is wired in a later proof-lane slice after the CLI
contract and exception policy are landed.

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
  PRs all run the proof receipt workflow so every merge has current RIPR
  evidence.
- Draft PRs are skipped while draft, then the explicit `ready_for_review`
  trigger runs the workflow before the PR can merge.
- Manual via `workflow_dispatch`.
- Labels can still route deeper or more expensive evidence lanes elsewhere,
  but this ready-for-review receipt workflow is no longer label-gated.

---

## Behavior

- Remains advisory in this slice; blocking new-gap enforcement is promoted
  through the later quality-gate CI wiring.
- Produces diff-scoped PR evidence under `target/ripr/pr/`.
- Produces the repo-wide RIPR+ baseline receipt at
  `target/receipts/quality/ripr-plus.json`.
- Produces review guidance under `target/ripr/review/`.
- In CI, review guidance has an explicit timeout and falls back to an
  advisory `error` artifact instead of blocking the workflow.
- Emits non-blocking warning annotations from `comments[]` only.
- Produces mutation-routing evidence under
  `target/xtask/impacted-evidence/`.
- Uploads the `ripr-pr-evidence` artifact.
- Appends `target/ripr/pr/summary.md` to the GitHub step summary.

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

---

## Promotion path

| PR | What happens |
|---:|---|
| 1 | Ready-for-review workflow routing, no path filter, repo-wide RIPR+ receipt, diff-scoped RIPR PR receipt, and review-guidance receipt. No CI enforcement. |
| Later | `cargo xtask quality-gate --mode enforce-new-ripr` becomes the blocking new-gap gate after its CLI contract lands. |
| Later | Total RIPR+ unresolved count reaches zero, then full `quality-gate --mode enforce` becomes blocking. |

This first routing slice makes the receipts available on every PR. Later slices
define the stable `quality-gate` interface, enforce new gaps, and finally require
repo-wide RIPR+ zero after burn-down.

---

## Toolchain

`rust-toolchain.toml` pins `1.95.0`. The workflow installs `ripr` `0.5.0`.

---

## Running locally

```bash
rtk cargo install ripr --version 0.5.0 --locked
rtk ripr doctor
rtk cargo xtask ripr-pr --base origin/HEAD --head HEAD
rtk cargo xtask ripr-plus --receipt target/receipts/quality/ripr-plus.json
rtk cargo xtask ripr-review-comments --base origin/HEAD --head HEAD
rtk cargo xtask impacted-evidence
rtk cargo xtask ripr-pr-summary
rtk cargo xtask ripr-annotations
rtk cargo xtask ripr-pr --base origin/HEAD --head HEAD --check
rtk cargo xtask ripr-plus --receipt target/receipts/quality/ripr-plus.json --check
rtk cargo xtask ripr-review-comments --base origin/HEAD --head HEAD --check
rtk cargo xtask impacted-evidence --check
rtk cargo xtask ripr-pr-summary --check
rtk cargo xtask ripr-annotations --check
```

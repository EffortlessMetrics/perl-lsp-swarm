# Issue Discovery / Bug Scout Desk

**Lane:** `issue-discovery-desk` · **Position:** upstream of the Plan-Review desk (Gate 1 feeder)

This lane answers **"where are the next real issues hiding?"** — it does *not* fix, plan
deeply, or flood the tracker. Its output is **evidence-backed candidate issues**, handed
forward to the plan-review lane (`needs-plan-review`) which decides builder-readiness.

> Origin: distilled from the lane spec pasted in session `epic-albattani-q48qp`
> (2026-05-30). Adapted here to the repo's actual agent catalog and label set.
> See also: [ORCHESTRATION_DOCTRINE.md](ORCHESTRATION_DOCTRINE.md),
> [PIPELINE_GATES.md](PIPELINE_GATES.md).

## Position in the swarm

```
Issue Discovery Desk  →  Plan-Review Desk  →  Build lane  →  Review/Merge lane
(find suspicious seams)   (verify + scope)     (implement)    (land green)
```

## The one operating rule

**Discovery may batch. Filing cannot.**

- Read-only discovery runs wide: grep, source/test inspection, CI/check snapshots,
  changed-file comparison, receipt/status review, docs↔source drift review.
- Mutations are issue-by-issue and deliberate: file, label, comment, close, dedupe.
- This is the same PR-by-PR discipline that protects the backlog from over-aggregation
  ("curator says so" is a lead, never a verdict — verify from source/primary artifact).

## Core doctrine: evidence, not vibes

A scout may only file when it can state: the source surface, a minimal example/sequence,
why current behavior is wrong/risky, how to verify it, and why it is **not already
covered**. Optimize for **few, strong findings**. The headline metric is *percentage of
filed findings that survive plan review* — if that's low, scouts are filing too eagerly.

## Read-only contract for scouts

Discovery scouts **may** read files, grep, run read-only inspection/tests, and query
GitHub read-only (search/list/get). They **must not** build, push, open/edit PRs, close
issues, retitle, remove labels, mark `builder-ready`, or merge/rebase. The orchestrator
triages results centrally and files issue-by-issue.

## Scout waves → agent mapping

| Wave | Surface | Agent (this repo) |
|------|---------|-------------------|
| DAP gaps | stack/scopes/variables/evaluate/setVariable/lifecycle/transport | `scout-dap` |
| LSP gaps | stale doc state, URI isolation, completion/hover/code-action/semantic-token drift | `scout-lsp` |
| Parser/AST gaps | wrong AST shape, recovery overuse, missing fixtures, NodeKind coverage | `scout-parser` |
| CI/ops gaps | runner routing, path-filter holes, check-name/branch-trigger drift, cleanup blind spots | `general-purpose` (or `tooling-debt-scout`) |
| Robustness gaps | panic/DoS/incorrect-result/malformed-response/unsafe-cleanup, byte-boundary slicing | `general-purpose` |
| Docs/receipt drift | status `.md` ↔ `.json` receipts, basis conflicts, stale counts/refs | `general-purpose` |

Each scout is **seeded with its domain's dedup map** (open issues + open PRs already
covering the surface) so it hunts only genuinely-uncovered seams or material new evidence.

## Candidate packet format

```md
## Finding            — one sentence
## Evidence           — Source: file:line · Test/fixture · Receipt/CI/docs · Related issues/PRs
## Impact             — who sees it (user / maintainer / CI)
## Minimal repro / sequence  — snippet, LSP/DAP sequence, or command
## Suspected root area — file → function/type → boundary
## Why not already covered — checked open+closed issues, PRs, tests
## Suggested next workflow — needs-repro | needs-plan-review | needs-architecture-review | small-builder | discard
## Confidence         — high / medium / low
```

## Confidence → action

- **High** (source evidence + minimal example + clear impact + dedup-clean + specific area):
  file directly as a candidate issue.
- **Medium** (strong smell, partial evidence, clear next verification step): hand to
  plan-review or file as a research lead. Do not over-claim.
- **Low**: do **not** file. Record in the wave report with "what would raise confidence."

## Dedup discipline

Dedupe by **failure mode**, never by shared theme / file / helper / base commit / diffstat
/ curator summary. Two findings are the same only when they share the same failure mode,
source surface, user-visible behavior, intended fix, or acceptance test. Prefer adding
evidence to an existing issue over filing a duplicate.

## Labels

Filed candidates currently use the established pipeline labels:
`swarm-discovered` + `needs-plan-review` + `size/{S,M,L}`. **Never** apply `builder-ready`
(that belongs to plan-review). Finer functional labels proposed by the lane spec
(`candidate-issue`, `docs-drift`, `robustness`, `ci-ops`, `test-gap`, `needs-repro`) are
**not yet created**; add them in a follow-up tooling PR before using them.

## Workflow

1. Choose a recently-hot surface (don't boil the ocean).
2. Read source + tests + receipts + CI (not issue titles).
3. Form a classified candidate finding.
4. Dedupe against open+closed issues and open PRs.
5. File (high) / hand off (medium) / record-only (low).
6. Run a central triage pass: keep · merge-into-existing · plan-review · architecture-review
   · repro-lab · discard.

## Guardrails

- **No flooding:** ≤5 packets per scout; ≤2 filed per scout unless clearly high-confidence.
- **No destructive action** from discovery agents (see read-only contract).
- **No high-frequency GitHub polling:** point-in-time snapshots only.
- **External-source PRs get the same gate set** as internal PRs.

## Deferred tooling (build after the lane proves useful)

`candidate_issue.yml` template · `cargo xtask issue-discovery report` · curated grep packs
(panic surfaces, DAP invalid refs, LSP stale-state, workflow bare-self-hosted) · scout
packet validator · candidate→plan handoff generator. See the session spec for details.

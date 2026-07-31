# GitHub surfaces

GitHub is the native interaction and asynchronous handoff layer for the development loop.

## Surface ownership

| Surface | Owns |
| --- | --- |
| Issue | problem, research trail, corrections, current synthesis, plan, explicit dependencies, next action |
| Stable labels | area, kind, risk, size, release grouping, genuine blocker, human decision, requested lens |
| Umbrella issue or milestone | durable multi-PR outcome and remaining coherent claims |
| Branch or worktree | one current candidate's mutation surface and writer |
| Pull request | one coherent acceptance-and-rollback candidate |
| Draft or ready state | whether broad review is useful now |
| Review request | visible pending formal judgment |
| Submitted review | candidate-and-material-claim-bound formal judgment |
| Inline review thread | one localized finding and its evidence/discussion |
| Review reply and resolution | supported finding disposition |
| Checks and artifacts | candidate-bound machine evidence and instrument state |
| Ruleset, queue, mergeability | whether irreversible integration is currently allowed |
| Merge and closeout | what landed, what remains, and the next coherent claim |

## Issue shape

A new issue may begin with:

```markdown
## Problem or desired outcome
## Current evidence
## Known context
```

Preparation progressively adds:

```markdown
## Current synthesis
## Current plan
## Scope and non-goals
## Vision alignment
## Proof strategy
## Dependencies and risk
## Next action
```

Comments preserve research, alternatives, corrections, and history. The issue body should retain one current usable synthesis.

## Parallel claim lanes

Distinct claims may proceed through separate issues, branches/worktrees, and PRs without a cross-lane coordination layer.

Each lane owns:

- its current candidate;
- proof and review repair;
- branch/worktree safety;
- its eventual rebase, merge-conflict resolution, or integration repair;
- current issue/PR closeout.

Do not use GitHub to project touched-file overlap, lane liveness, writer reservations, candidate frontiers, or executor telemetry.

When another lane genuinely needs a fact, add a direct comment to its controlling issue or PR. Appropriate handoffs include:

- an explicit prerequisite landed or changed shape;
- a governing contract or owner ruling changed;
- one claim superseded or duplicated another;
- a main-health repair became the named dependency;
- Git or combined-tree proof exposed a real interaction.

If no such fact exists, let the other lane focus on its work.

## Label policy

Use labels for stable classification and requested attention:

```text
area/*
kind/*
risk/*
size/*
release/*
blocked
needs-human-decision
needs-reproduction
```

Do not use labels as proof of build, review, CI, response, or merge completion. Native PR, review, thread, check, and merge state already owns those facts.

## Pull request review index

A substantive PR should make its claim and proof legible:

```markdown
## Claim
## Controlling issue
## Governing contract
## Changed production path
## Proof
## Test hardening
## Simplification
## Deviations
## What this establishes
## What this does not establish
## Risk and rollback
## Review index
```

Publish ready by default. Draft is an explicit exception for remote-only proof, real collaboration on the same candidate branch, or a protected integration experiment that requires remote evidence before broad review is useful.

## Formal review

Formal review uses GitHub's review interface and identifies the complete review subject: exact candidate plus normalized material claim/review index.

```text
Reviewed candidate: <full head SHA>
Reviewed material claim: <digest or exact stable representation>
Reviewed claim summary
Review lenses used
REVIEW_CURRENT | REVIEW_FINDINGS_OPEN | REVIEW_NOT_PROVEN
Material findings with evidence
What the review establishes
What remains unproved
```

Use the repository-owned claim-digest/currentness helper where present rather than inventing a second normalization. A clean review is valid.

## Finding disposition

Before resolving a substantive thread, reply with one supported disposition:

```text
Disposition: fixed | refuted | superseded | follow-up
Evidence: current candidate, focused test or oracle, governing source, or linked follow-up
```

Thread resolution is not itself evidence.

## Focused helpers

Repository-owned helpers may centralize difficult factual questions such as candidate identity, complete review-thread enumeration, required-check currentness, material-claim identity, and merge preflight for the selected PR.

They must not answer which lifecycle stage the PR is in, which agent works next, which issue should be prioritized, or how neighbouring lanes overlap.

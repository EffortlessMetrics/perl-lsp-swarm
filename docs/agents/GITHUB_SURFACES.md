# GitHub surfaces

GitHub is the native interaction and asynchronous handoff layer for the development
loop.

## Surface ownership

| Surface | Owns |
| --- | --- |
| Issue | problem, research trail, corrections, current synthesis, plan, explicit dependencies, next action |
| Stable labels | area, kind, risk, size, release grouping, genuine blocker, human decision, requested lens |
| Umbrella issue or milestone | durable multi-PR outcome and remaining coherent claims |
| Branch or worktree | one current candidate's mutation surface and writer |
| Pull request | one coherent acceptance-and-rollback candidate |
| Draft or ready state | whether broad review is useful now |
| Review request | visible pending judgment where deliberately requested |
| Submitted review | useful cumulative review conclusion, findings, and substantive review result |
| Inline review thread | one localized finding and its evidence/discussion |
| Review reply and resolution | supported finding disposition |
| Checks and artifacts | candidate-bound machine evidence and instrument state |
| Ruleset, queue, mergeability | current integration posture and whether irreversible integration is allowed |
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

Comments preserve research, alternatives, corrections, and history. The issue body
should retain one current usable synthesis.

## Parallel claim lanes

Distinct claims may proceed through separate issues, branches/worktrees, and PRs
without a cross-lane coordination layer.

Each lane owns:

- its current candidate;
- proof and substantive review repair;
- branch/worktree safety;
- its eventual merge-conflict resolution or integration repair;
- current issue/PR closeout.

Do not use GitHub to project touched-file overlap, lane liveness, writer reservations,
candidate frontiers, or executor telemetry.

When another lane genuinely needs a fact, add a direct comment to its controlling issue
or PR. Appropriate handoffs include:

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

Do not use labels as proof of build, review, CI, response, or merge completion. Native
PR, review, thread, check, and merge state owns those facts.

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

Publish ready by default. Draft is an explicit exception for remote-only proof, real
collaboration on the same candidate branch, or a protected integration experiment
that requires remote evidence before broad review is useful.

## Review

Review follows
[`PR_REVIEW_STANDARD.md`](PR_REVIEW_STANDARD.md) and uses GitHub's submitted review
interface, inline threads, replies, and useful cumulative conclusions. It is directed,
falsifying, verified, cumulative, and semantic—not an exact-head receipt protocol.

A helpful review record contains:

```text
Reviewed claim and production path
Evidence, authorities, and realistic falsifiers used
Material findings and evidence, or a useful clean conclusion
Prior finding dispositions
What the review establishes
What remains unproved
Current GitHub facts as a separate snapshot
Substantive review result
Next action
```

Use one substantive review result:

```text
REVIEW_CURRENT
CHANGES_REQUIRED
NOT_PROVEN
BLOCKED_BY_PREREQUISITE
SUPERSEDED_OR_CLOSE
```

Green checks, `mergeable: true`, zero open threads, bot approval, or author
self-certification do not independently imply `REVIEW_CURRENT`. Checks and
mergeability are integration facts; they are not review substitutes.

After review is `REVIEW_CURRENT`, live GitHub facts produce a separate integration
posture:

```text
INTEGRATION_READY
PR_IN_FLIGHT
MERGE_BLOCKED
NOT_PROVEN
```

Do not post comments that merely say a review ran at a head SHA and claim digest. Do
not require a new full review merely because another commit was pushed.

A later change receives additional review only where it can change the conclusion:

- verify repaired findings and affected proof;
- review material claim, production-route, authority, risk, rollback, or
  compatibility changes;
- review actual conflict or integration repairs;
- do not restart broad review for formatting, editorial cleanup, generated receipt
  refresh, or stronger tests unless they change a substantive conclusion.

A clean review is valid.

## Related pull request synthesis

When an umbrella or release goal directly links a bounded related PR set, each PR
keeps its own submitted review. A goal-level synthesis may summarize:

```text
PR
candidate identity
hosted/current checks
substantive review result
integration posture
explicit prerequisite
correct repair and merge order
```

Use that synthesis to inspect parent/child schema, identity, authority, status,
limitation propagation, artifact-set, and fan-in contracts. It must not become batch
approval, a portfolio queue, an overlap map, or a replacement for per-PR review.

## Finding disposition

Before resolving a substantive thread, reply with one supported disposition:

```text
Disposition: fixed | refuted | superseded | follow-up
Evidence: current candidate, focused test or oracle, governing source, or linked follow-up
```

Thread resolution is not itself evidence.

## Merge safety

Use live required checks, unresolved threads, current change requests, draft state,
mergeability, rulesets, and queue state as integration authority after substantive
review is `REVIEW_CURRENT`.

The current PR head SHA may be used as compare-and-swap protection at merge time. That
prevents racing a moving branch; it does not make review currentness depend on the
SHA.

## Focused helpers

Repository-owned helpers may centralize difficult factual questions such as candidate
identity, complete review-thread enumeration, required-check currentness, and merge
preflight for the selected PR.

They must not create a second review lifecycle, judge substantive review sufficiency
from a phrase gate, require claim-hash receipt comments, answer which agent works next,
or predict neighbouring-lane overlap.

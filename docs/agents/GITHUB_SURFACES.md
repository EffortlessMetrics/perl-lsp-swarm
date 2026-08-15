# GitHub surfaces

GitHub is the native durable interaction and asynchronous handoff layer for the
development loop. It stores useful work facts, not runtime supervision.

## Operational boundary

This document defines what GitHub surfaces own. It does not tell a running provider
how to execute review or persist its runtime frontier.

```text
Claude Code operation
→ CLAUDE.md
→ .claude/skills/orchestrate-work
→ selected provider-native flow/skill

Codex operation
→ AGENTS.md
→ .agents/skills/orchestrate-work
→ selected provider-native flow/skill
```

Campaign frontiers, lane topology, agent identity, liveness, retries, temporary task
state, and raw reasoning remain runtime-local. They must not be written to tracked
state files or mirrored through labels/comments.

## Surface ownership

| Surface | Owns |
| --- | --- |
| Issue | problem, research trail, corrections, current synthesis, accepted plan, explicit dependencies, useful route changes, next material action |
| Stable labels | area, kind, risk, size, release grouping, genuine blocker, human decision, requested lens |
| Umbrella issue or milestone | durable multi-PR outcome, acceptance predicates, merged effects, remaining coherent claims |
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

## Publication filter

Post or update GitHub when the information is durable and useful after the current
context disappears.

### Publish when information

- changes the claim, semantic owner, accepted plan, proof obligation, selected route,
  prerequisite, support boundary, risk, or rollback meaning;
- is source-backed evidence another lane or future context would otherwise rediscover;
- is a localized review finding that belongs inline;
- dispositions an existing finding with current evidence;
- records a real external wait and the event that should resume the lane;
- provides a useful candidate-wide review, integration result, merged effect, or
  goal-level synthesis;
- corrects stale durable prose that would otherwise route later work incorrectly.

### Keep runtime-local when information is

- agent identity, team topology, liveness, retry order, or temporary task state;
- a campaign frontier or lane table used only by the current orchestrator;
- raw logs or transcripts already available through a stable reference;
- provisional reasoning that changed no durable conclusion;
- unchanged polling or repeated check summaries;
- a routine skill transition already implied by the selected route;
- a status-only exact-head, claim-hash, or “review complete” announcement.

The test is not whether an LLM discovered the information. The test is whether another
context, lane, reviewer, maintainer, or future decision will benefit from a durable
record.

## Traceable intended route

The selected public flow and current material transition should be easy to reconstruct
when that improves resumability. Do not create one comment per skill or turn the route
into lifecycle authority.

When route intent is not already obvious, use one compact declaration on the
controlling issue or PR:

```text
Route
- Goal / parent: <issue or durable outcome>
- Claim: <one acceptance-and-rollback claim>
- Entry flow: deliver-goal | deliver-pr | prepare-issue | prepare-proof |
  build-candidate | finish-pr
- Current useful transition: <named skill or external wait>
- Why: <material missing judgment>
- Durable subject: <issue / PR / merged commit>
- Resume when: <material wake event, if any>
```

Update that declaration only when the material route changes: for example, a premise
returns to `prepare-issue`, proof becomes the blocker, one claim is superseded, an
explicit prerequisite appears, or GitHub owns a named wait. Do not update it for every
agent, retry, normal forward edge, base movement, or head SHA.

The route declaration is a derived resumability aid. Current code, issues, PRs, reviews,
checks, rulesets, and accepted contracts remain authoritative.

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

Comments preserve useful research, alternatives, corrections, and history. The issue
body should retain one current usable synthesis when a durable summary is valuable.
Do not rewrite the body merely to mirror runtime progress.

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
candidate frontiers, agent telemetry, or temporary task order.

When another lane genuinely needs a fact, add a direct comment to its controlling issue
or PR. Appropriate handoffs include:

- an explicit prerequisite landed or changed shape;
- a governing contract or owner ruling changed;
- one claim superseded or duplicated another;
- a main-health repair became the named dependency;
- Git or combined-tree proof exposed a real interaction;
- a reusable finding changes another lane's proof or review route.

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

Do not use labels as proof of build, review, CI, response, route, or merge completion.

## Pull request review index

A substantive PR should make its claim and proof legible:

```markdown
## Claim
## Controlling issue
## Governing contract
## Intended route
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

`Intended route` may be one short line or link to the useful route declaration. It is
not a stage checklist.

Publish ready by default. Draft is an explicit exception for remote-only proof, real
collaboration on the same candidate branch, or a protected integration experiment.

## Review record

Provider-native review uses GitHub's submitted-review interface, inline threads,
replies, and useful cumulative conclusions. It is directed, falsifying, verified,
cumulative, and semantic—not an exact-head receipt protocol.

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

Use one substantive result:

```text
REVIEW_CURRENT
CHANGES_REQUIRED
NOT_PROVEN
BLOCKED_BY_PREREQUISITE
SUPERSEDED_OR_CLOSE
```

Green checks, `mergeable: true`, zero open threads, bot approval, or author
self-certification do not independently imply `REVIEW_CURRENT`.

After review is current, live GitHub facts produce a separate integration posture:

```text
INTEGRATION_READY
PR_IN_FLIGHT
MERGE_BLOCKED
NOT_PROVEN
```

Do not post comments that merely say a review ran at a head SHA and claim digest. Do
not require a new full review merely because another commit was pushed.

## Finding disposition

Before resolving a substantive thread, reply with one supported disposition:

```text
Disposition: fixed | refuted | superseded | follow-up
Evidence: current candidate, focused test or oracle, governing source, or linked follow-up
```

Thread resolution is not itself evidence.

## Related pull request synthesis

When a durable goal directly links a bounded related PR set, each PR keeps its own
provider-native submitted review. A useful goal-level synthesis may summarize:

```text
PR
candidate identity
hosted/current checks
substantive review result
integration posture
explicit prerequisite
correct repair and merge order
```

Use it only when the related contracts or merge order matter. It must not become batch
approval, a portfolio queue, an overlap map, or a replacement for per-PR review.

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

They must not create a second review lifecycle, runtime state file, frontier database,
agent registry, phrase-gated substantive review, claim-hash receipt protocol, or
neighbouring-lane overlap predictor.

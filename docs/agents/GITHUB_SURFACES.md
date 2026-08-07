# GitHub surfaces

GitHub is the durable interaction and asynchronous handoff layer for the development
loop. It stores useful work truth, not the runtime executor topology.

## Operational boundary

Provider-native roots and skills execute the route:

```text
Claude Code
→ CLAUDE.md
→ .claude/skills/*

Codex
→ AGENTS.md
→ .agents/skills/*
```

GitHub preserves issues, candidates, findings, evidence, decisions, waits that matter
to another operator, merges, and residual claims. It does not store campaign frontier,
agent assignments, liveness, task completion, retries, or provisional reasoning.

## Surface ownership

| Surface | Owns |
| --- | --- |
| Issue | problem, research, corrections, current synthesis, plan, decisions, explicit dependencies, next coherent action |
| Umbrella issue/milestone | durable multi-PR outcome, acceptance predicates, remaining claims |
| Stable labels | area, kind, risk, size, release grouping, genuine blocker, requested attention |
| Branch/worktree | one current candidate mutation surface and its writer |
| Pull request | one coherent acceptance-and-rollback candidate, proof, limitations, deviations |
| Draft/ready | whether broad review is useful now |
| Review request | deliberately pending judgment |
| Submitted review | cumulative substantive judgment and result |
| Inline thread | one localized finding and evidence/discussion |
| Reply/resolution | evidence-backed finding disposition |
| Checks/artifacts | candidate-bound machine evidence and instrument state |
| Ruleset/queue/mergeability | current integration posture |
| Merge/closeout | landed effect, residual work, next coherent claim |

## Useful-write rule

Write only when the information will change or accelerate later work.

Useful issue/PR comments include:

- corrected premise or governing decision;
- current synthesis or plan;
- material prerequisite, supersession, or actual cross-claim interaction;
- changed candidate claim, proof, limitation, or deviation;
- named remote-owned wait when another operator needs the handoff;
- closeout or residual claim.

Useful review surfaces include:

- localized inline findings;
- cumulative submitted review or useful clean conclusion;
- evidence-backed `fixed`, `refuted`, `superseded`, or `follow-up` dispositions.

Do not post:

- campaign frontier rows;
- agent/team assignments or liveness;
- stage or skill completion announcements;
- heartbeat or polling updates;
- raw logs/transcripts or private reasoning;
- `review complete at SHA` comments;
- duplicate summaries when the conclusion did not change.

The fact that an LLM found information is not enough to write it. The information must
be reusable evidence, a decision, a finding, a disposition, a material handoff, or a
closeout.

## Traceable routes without state files

The intended route is expressed in provider-native skills and the active brief, for
example:

```text
deliver-goal
→ deliver-pr(#123)
→ orchestrate-work
→ writer: build-candidate
→ reviewer: review-tests
→ lane root: finish-pr
```

GitHub makes that route reconstructable through the durable artifacts the route
changes:

```text
goal/issue synthesis
→ candidate PR
→ proof and limitations
→ review findings/dispositions
→ checks/integration
→ merge/closeout
```

Do not mirror the route as a tracked stage record, frontier file, issue heartbeat,
label chain, or executor database.

## Campaign and lane handoffs

Distinct claim lanes may proceed concurrently. A direct comment is appropriate only
when another lane genuinely needs a material fact:

- prerequisite landed or changed shape;
- governing owner/contract ruling changed;
- one claim superseded or duplicated another;
- main-health or release repair became a named dependency;
- Git or combined-tree proof exposed a real interaction;
- a remote wait or external decision must be resumed by another operator.

If no such fact exists, let the other lane work without surveillance.

## Issue shape

A new issue may begin with:

```markdown
## Problem or desired outcome
## Current evidence
## Known context
```

Preparation may add:

```markdown
## Current synthesis
## Current plan
## Scope and non-goals
## Vision alignment
## Proof strategy
## Dependencies and risk
## Next action
```

Comments preserve useful research, corrections, and decision history. The body should
retain one current usable synthesis when the issue is long-lived.

## PR packet

A substantive PR should expose:

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
same-candidate collaboration, or a protected experiment whose remote behavior is the
subject.

## Review record

A useful cumulative review contains:

```text
reviewed claim and production path
evidence, authorities, and realistic falsifiers
material findings or useful clean conclusion
finding dispositions
what is established and not established
current GitHub facts as a separate snapshot
REVIEW_CURRENT | CHANGES_REQUIRED | NOT_PROVEN |
BLOCKED_BY_PREREQUISITE | SUPERSEDED_OR_CLOSE
next route
```

Green checks, mergeability, zero threads, bot approval, or author self-certification do
not imply `REVIEW_CURRENT`.

After review is current, live GitHub facts produce:

```text
INTEGRATION_READY
PR_IN_FLIGHT
MERGE_BLOCKED
NOT_PROVEN
```

Review currentness is semantic. Refresh repaired findings and materially changed claim,
production-route, authority, proof, compatibility, risk, rollback, conflict, or
integration dimensions. Do not replay broad review for unrelated SHA/base movement.

## Finding disposition

Before resolving a substantive thread, reply with:

```text
Disposition: fixed | refuted | superseded | follow-up
Evidence: current candidate, focused test/oracle, governing source, or linked follow-up
```

Thread resolution is not evidence.

## Merge safety

Use live required checks, unresolved substantive threads, current change requests,
draft state, mergeability, rulesets, queue state, explicit prerequisites, and required
combined-tree proof after substantive review is `REVIEW_CURRENT`.

The current head SHA may be used as compare-and-swap protection at merge time. It is
not a review receipt.

## Labels

Use labels only for stable classification or requested attention. Do not use labels as
proof of build, review, CI, response, merge, lane, or agent state.

## Focused helpers

Repository helpers may centralize factual questions such as candidate identity,
complete thread enumeration, required-check currentness, and merge preflight.

They must not create a second review lifecycle, persist an executor frontier, select
agents, infer lane liveness, judge substantive review from a phrase gate, or authorize
merge independently of provider-native review and live GitHub protection.
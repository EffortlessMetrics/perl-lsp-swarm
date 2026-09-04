# Development method

## Purpose

This repository uses a shift-left, review-forward, orchestration-heavy development
loop. The main/root provider thread governs goals and logical claim frames while bounded
researcher, builder, reviewer, fork, worker, workflow, or Team contexts perform useful
investigation, proof, mutation, repair, and review.

Useful passes are controls. Permanent personas, subordinate claim orchestrators,
lifecycle labels, tracked frontiers, agent liveness, completion hooks, exact-head review
receipts, and stage files are not.

```text
current durable artifact
→ selected provider-native route
→ root-held claim frame
→ bounded execution and challenge
→ root joins evidence or candidate delta
→ useful GitHub/repository update
→ next named route
```

## Governing law

**Default-complete, recovery-forward.**

For substantive work, perform every applicable research, planning, proof, hardening,
simplification, review, and reconciliation judgment before the next more expensive
artifact. When an earlier judgment was missed, perform the cheapest version that can
still improve the current artifact and continue. Do not replay history to manufacture
process evidence.

## Operational authority

```text
Claude Code
→ CLAUDE.md
→ .claude/skills/*

Codex
→ AGENTS.md
→ .agents/skills/*
```

This document states shared invariants. It is not the runtime router.

## Durable graph and ephemeral execution

Durable nodes include goals, issues, accepted contracts, proofs, candidates, findings,
decisions, reviews, checks, merges, and residual claims.

Ephemeral nodes include root claim-frame ordering, workers, subagents, forks, Teams,
worktrees, task lists, wake events, retries, provider/model choices, raw logs, and
provisional reasoning.

Encode the work, not the workers. Persist useful conclusions through their owning
issue, PR, review, check, merge, or contract. Do not persist executor topology.

## Root orchestration

The main/root thread is the accountable orchestrator across both goal and claim scopes.
It owns:

- goal source and current interpretation;
- acceptance predicates and required claims;
- selection and ordering of claims;
- cross-claim dependencies and contradictions;
- runtime-local claim frames and wake events;
- evidence joins and finding disposition;
- candidate writer allocation;
- review sufficiency, integration judgment, and reconciliation.

A claim/lane is a **logical frame retained by the root**:

```text
claim
acceptance predicate
durable subject
current candidate / writer
current route or missing judgment
proof and review state / limitations
external wait
wake event
```

The root may switch among these frames. A claim reaching a GitHub-owned wait remains
`IN_FLIGHT` without a live representative. The frame is reconstructed from durable
artifacts after compaction or replacement.

A substantial claim does not normally create a subordinate orchestrator. The root keeps
orchestration and delegates bounded programmes.

## Bounded programme contexts

### Researcher / read-only worker

Use for source ownership, archaeology, external truth, CI/log triage, broad inventories,
currentness checks, or other high-output-to-answer work.

### Builder / writer

Use for one current candidate or proof mutation programme. One writer owns a candidate
branch/worktree at a time.

### Reviewer

Use for one fixed subject and a differentiated review programme. Reviewers change the
evidence surface through source/oracle/method/threat/environment/attention, not merely
through identity.

### Direct root work

Use for tiny or tightly coupled decisions where delegation changes no evidence surface
and briefing/join cost exceeds its value.

### Provider-native forks, nested agents, workflows, and Teams

These are physical execution techniques. Use them when inherited context, dynamic task
readiness, different tools, or lateral communication materially improves a bounded
programme. They do not acquire logical claim-orchestration authority and are not a
required repository topology.

## Programme continuity

A programme may span several ordered atomic skills while one context remains useful:

```text
researcher
→ research-issue / external truth / related evidence

builder
→ spec-to-test / build-from-proof / improve-test-suite /
  simplify-candidate / affected repair

reviewer
→ issue / plan / proof / candidate lenses over one fixed subject
```

Do not fork once per skill when the same subject and artifact understanding remain
load-bearing. Atomic skills change attention, not identity.

Conversely, use a fresh context when a different source, oracle, threat model,
environment, tool boundary, or independence property is the point.

## Route discipline

The root names the route and then runs it:

```text
deliver-goal
→ select root-held claim frame
→ deliver-pr
→ orchestrate-work
   ├── researcher programme(s), if useful
   ├── one writer / build-candidate programme
   └── reviewer programme(s)
→ root joins evidence
→ finish-pr
→ return to deliver-goal / caller
```

Children receive the parent route, exact durable subject, selected skill, established
facts, authority, read/write boundary, falsifiers, sufficient return, backward routes,
stop conditions, and non-goals.

Do not give children an invented lifecycle when repository skills already define the
method. Do not grant generic claim-orchestration authority merely because a task is
substantial.

## Runtime-local claim frames

The root may keep an in-context table of claim, goal predicate, durable subject,
candidate/writer, current judgment, next material action, external wait, and wake event.

This table is not committed or posted. Reconstruct it from current issues, PRs,
submitted reviews, checks, merges, and repository facts after compaction/replacement.

When GitHub owns the next transition, mark the frame `IN_FLIGHT`, advance another
independent claim, and revisit only when its named wake event occurs. Do not poll
unchanged state or keep an idle agent alive to symbolize the wait.

## Orchestration economics

Delegate when expected evidence gain, root-context preservation,
dependency-unlocking value, elapsed-time gain, changed detection surface, or avoided CI
cost exceeds cold-start, briefing, duplicated research, resource contention, join, and
correlated-failure costs.

High-output-to-answer work is normally delegated: CI/log triage, corpus/repository
sweeps, dependency/API audits, external-source collection, failure bisection, broad
inventories, proof adversaries, and specialist review.

Maximal fan-out is not sophistication. Stop adding agents when another result cannot
change a decision.

## Graph-delta evidence

Workers return subject/basis, conclusion, direct evidence/authority, scope searched,
contradiction/uncertainty, what is/is not established, affected claim/proof/authority
edge, recommended route, `NOT_PROVEN` boundary, and stable overflow references.

Writers add candidate identity, changed behavior/seams, proof run/not run, repaired
findings, limitations, and typed result.

The root joins evidence rather than votes. Repeated claims from one source remain one
evidence path. Builder self-report is not independent proof. Contradictions remain
visible until resolved.

A dispatched review dimension that never returns remains `NOT_PROVEN`, not
examined-and-clean.

## GitHub-native durable updates

Use GitHub when information is useful later:

- issue correction, current synthesis, plan, decision, route, or dependency;
- material prerequisite, supersession, or actual interaction;
- PR claim/proof/limitation/deviation;
- inline finding, submitted review, or evidence-backed disposition;
- remote wait needed for another operator's handoff;
- merge/closure effect and residual work.

When another context benefits and the intended route is not already obvious, one
compact route declaration may record parent goal, claim, entry flow, current named
transition, reason, durable subject, and wake event. Update it only when the material
route changes. It is a resumability aid, not a stage record or runtime state mirror.

Do not write root claim-frame ordering, agent assignments/liveness, skill completion,
polling, transcripts, provisional reasoning, or duplicate unchanged summaries.

## Public flows

| Flow | Normal root responsibility and outcome |
| --- | --- |
| `deliver-goal` | manage the durable outcome, acceptance predicates, and root-held claim frames |
| `deliver-pr` | focus on one claim frame and carry it through its current SDLC route |
| `prepare-issue` | settle problem/owner/scope/plan with bounded research/review programmes as useful |
| `prepare-proof` | establish/challenge discriminating proof with one proof mutation owner where needed |
| `build-candidate` | drive one candidate writer plus hardening/simplification/challenge programmes |
| `finish-pr` | converge the selected PR through repair, review, integration, merge, closeout |

Public flows are root-facing. Atomic skills may run in the root or in bounded
researcher/writer/reviewer programmes. This is a semantic method, not a fixed physical
executor graph.

## Claim independence and optimistic concurrency

```text
one coherent claim
→ one current candidate
→ one branch/worktree
→ one writer
→ one PR
```

Different claims may touch the same files/crates and proceed concurrently. Coordinate
only duplicate claims, same-candidate writers, explicit prerequisites, destructive
shared runtime state, actual conflicts, or demonstrated combined-tree interactions.
Behind-only movement requires no action.

## Proof ladder

```text
edit
→ exact staged structural proof
→ affected committed proof
→ candidate challenge/review
→ current-head clean-environment CI
→ integration/merge evidence
```

Run the cheapest discriminating proof first. Never weaken a test, ratchet, support
claim, or required proof merely to obtain green status. Missing or failed instruments
produce `NOT_PROVEN`.

## Review and merge

Substantive review is directed, falsifying, and verified. It establishes, where
applicable, proof discrimination, production reachability, external/semantic truth,
claim honesty, authority/complexity, risk/rollback, and remaining uncertainty.

```text
finish-pr
→ final mutable challenge
→ root selects differentiated review programmes
→ root joins and submits cumulative review-pr
→ REVIEW_CURRENT
→ verify-live-ci
→ INTEGRATION_READY
→ merge-reconcile
```

Green CI, mergeability, zero threads, bot approval, or author self-certification cannot
create `REVIEW_CURRENT`. Merge requires both current substantive review and current
integration evidence.

Review is semantic and cumulative. Refresh only findings/proof/dimensions materially
changed by repair, claim/authority/risk changes, or actual conflict/combined-tree
repair. Unrelated `main` movement and formatting/editorial/generated-only changes do
not force broad replay.

## Hard stops

Stop only for concrete hazards: same-candidate writer collision, destructive loss,
unestablished durable identity/authority, unsafe irreversible action, structurally
invalid contract, unresolved material finding, `NOT_PROVEN` substantive review at
merge, or live GitHub protection blocking integration.

Everything else normally follows:

```text
detect
→ explain
→ route
→ repair
→ continue
```

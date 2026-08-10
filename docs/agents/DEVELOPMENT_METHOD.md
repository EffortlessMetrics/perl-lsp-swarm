# Development method

## Purpose

This repository uses a shift-left, review-forward, orchestration-heavy development
loop. Capable roots govern goals and claims while skill-consuming agents perform
bounded investigation, proof, mutation, repair, and review.

Useful passes are controls. Permanent personas, lifecycle labels, tracked frontiers,
agent liveness, completion hooks, exact-head review receipts, and stage files are not.

```text
current durable artifact
→ selected provider-native route
→ bounded execution and challenge
→ joined evidence or candidate delta
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

## Durable graph and ephemeral executor graph

Durable nodes include goals, issues, accepted contracts, proofs, candidates, findings,
decisions, reviews, checks, merges, and residual claims.

Ephemeral nodes include campaign/lane/worker contexts, subagents, worktrees, task
lists, frontiers, wake events, retries, provider/model choices, raw logs, and
provisional reasoning.

Encode the work, not the workers. Persist useful conclusions through their owning
issue, PR, review, check, merge, or contract. Do not persist the executor topology.

## Hierarchical orchestration

### Campaign root

Owns the selected durable outcome:

- goal source and current interpretation;
- acceptance predicates and required claims;
- cross-claim dependencies and contradictions;
- runtime-local claim frontier and wake events;
- evidence joins, merge judgment, and goal reconciliation.

Campaign-root leaf execution is exceptional because disposable work permanently
occupies the context needed for later decisions.

### Lane root

Owns one coherent acceptance-and-rollback claim through `deliver-pr`:

- issue/contract and semantic owner;
- production consumers and proof obligations;
- candidate/writer;
- findings, review, integration, and closeout.

A lane root may orchestrate workers within the claim. Tiny claim-local work may remain
with the lane root or writer when delegation costs more than the context.

### Worker, writer, reviewer

- workers answer bounded read-only questions;
- one writer mutates the current candidate;
- reviewers challenge proof/candidate/production paths through differentiated sources,
  oracles, methods, environments, threat models, or attention surfaces.

A whole-flow `deliver-pr` child is a lane root. Leaf recursion requires explicit
claim-local orchestration authority.

## Route discipline

At the campaign/lane boundary, name the route and then run it:

```text
deliver-goal
→ deliver-pr(#123)
→ orchestrate-work
→ writer: build-candidate
→ reviewer: review-tests
→ lane root: finish-pr
```

Children receive the parent route, exact durable subject, selected skill, established
facts, authority, read/write boundary, falsifiers, sufficient return, backward routes,
stop conditions, and non-goals.

Do not give children an invented lifecycle when repository skills already define the
route.

## Runtime-local frontier

A campaign root may keep an in-context table of claim, goal predicate, lane context,
durable subject, current judgment, next material action, external wait, and wake event.

This table is not committed or posted. Reconstruct it from current issues, PRs,
submitted reviews, checks, merges, and repository facts after compaction/replacement.

When GitHub owns the next transition, return `IN_FLIGHT`, advance another independent
claim, and revisit only when its named wake event occurs. Do not poll unchanged state.

## Orchestration economics

Delegate when expected evidence gain, root-context preservation,
dependency-unlocking value, elapsed-time gain, changed detection surface, or avoided CI
cost exceeds cold-start, briefing, duplicated research, resource contention, join,
and correlated-failure costs.

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
findings, limitations, and typed flow result.

The root joins evidence rather than votes. Repeated claims from one source remain one
evidence path. Builder self-report is not independent proof. Contradictions remain
visible until resolved.

## GitHub-native durable updates

Use GitHub when information is useful later:

- issue correction, current synthesis, plan, decision, route, or dependency;
- material prerequisite, supersession, or actual interaction;
- PR claim/proof/limitation/deviation;
- inline finding, submitted review, or evidence-backed disposition;
- remote wait needed for another operator's handoff;
- merge/closure effect and residual work.

When another context benefits and the intended route is not already obvious, one
compact route declaration may record the parent goal, claim, entry flow, current named
transition, reason, durable subject, and wake event. Update it only when the material
route changes. It is a resumability aid, not a stage record or runtime state mirror.

Do not write agent assignments/liveness, frontier rows, skill completion, polling,
transcripts, provisional reasoning, or duplicate unchanged summaries.

## Public flows

| Flow | Normal owner and outcome |
| --- | --- |
| `deliver-goal` | Campaign root advances a durable outcome through claim lanes |
| `deliver-pr` | Lane root carries one claim and current candidate |
| `prepare-issue` | Lane root orchestrates problem/owner/scope/plan research |
| `prepare-proof` | Lane root orchestrates one proof writer plus useful adversaries |
| `build-candidate` | Lane root orchestrates one candidate writer plus hardening/review |
| `finish-pr` | Lane root converges the selected PR through review, integration, merge, closeout |

Atomic skills usually run in worker/reviewer contexts; public flows usually run in
campaign/lane roots. This is a default topology, not a fixed persona chain.

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
→ orchestrated differentiated review
→ root joins and submits cumulative review-pr
→ REVIEW_CURRENT
→ verify-live-ci
→ INTEGRATION_READY
→ merge-reconcile
```

Green CI, mergeability, zero threads, bot approval, or author self-certification cannot
create `REVIEW_CURRENT`. Merge requires both current review and current integration
evidence.

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
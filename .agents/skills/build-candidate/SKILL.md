---
name: build-candidate
description: Use when reviewed proof exists or a coherent implementation candidate needs completion, test hardening, simplification, production-path validation, and candidate-stage vision review.
---

# Build candidate

## Purpose

Produce one coherent candidate that satisfies the current claim, is protected by
discriminating proof, is locally minimal, and remains aligned with the product vision.

## Entry

This flow may begin from reviewed proof, an existing branch, an existing candidate, or a
candidate-owned finding discovered during `$review-pr`, `$verify-live-ci`, or another
later skill. Do not replay completed work merely to manufacture chronology.

Before creating another branch, check whether an equivalent current candidate already
implements the same claim. Do not inspect sibling lanes, touched-file overlap, or nearby
symbols as a routine ownership check.

## Context continuity

`$build-candidate` is a transition inside the current authorized PR context, not a
reason to replace its agent.

A persistent claim lane or role-specialized reviewer may become the candidate writer
when:

- the finding is accepted and candidate-owned;
- the repair remains inside the current claim and non-goals;
- the parent brief grants mutation/publication authority;
- no other writer is mutating the same candidate.

Keep the same thread, worktree, loaded source context, and accepted evidence. Do not
return a repair packet solely so a fresh agent can repeat the review. The real boundary
is authority and same-candidate writer exclusivity, not the label the agent held before
this skill.

A focused evidence worker remains read-only unless the parent explicitly promotes that
same context. When a reviewer is promoted, preserve the review evidence that motivated
the repair and continue back through affected proof and review. Add a genuinely
different oracle, method, threat model, environment, or reviewer when substantive merge
independence would otherwise collapse into the construction context.

## Orchestration affordances

### Context decisions

The current claim owner retains:

- the material claim, non-goals, and semantic owner;
- which implementation latitude remains inside the accepted plan;
- proof sufficiency and accepted risk/rollback boundary;
- which review findings are valid and what disposition they require;
- when a discovery materially returns to issue or proof preparation;
- whether the candidate is coherent enough for PR convergence.

### Delegable work

Use focused workers where useful for:

- bounded implementation assistance inside one admitted mutation boundary;
- source/owner/consumer verification;
- test hardening against the actual candidate;
- external language/protocol/dependency truth;
- production-path reachability;
- simplification and duplicate-authority review;
- specialist security, compatibility, lifecycle, packaging, persistence, performance,
  migration, or support review.

A worker receives settled facts, exact claim/candidate identity, named skill where
applicable, mutation/read-only authority, falsifiers, proof budget, and return boundary.
Workers return evidence or a bounded patch to the current PR context; they do not create
a second candidate owner.

### Mutation owner

One writer mutates the candidate branch/worktree at a time. The current persistent claim
lane is normally that writer; a dedicated reviewer may be promoted in place when it
already holds the accepted finding and the parent grants authority.

Do not require an agent replacement or cold start merely because the context crossed
from review to implementation. Require only the real authority change: accepted repair,
write permission, and no same-candidate writer collision.

### Join predicate

Join into one candidate only when:

- implementation satisfies the current claim without expanding authority;
- discriminating proof is current for changed seams;
- production consumers reach the changed behavior or the limitation is explicit;
- test hardening and simplification findings are dispositioned;
- accepted review findings are repaired through the one writer;
- unsupported behavior, compatibility, risk, and rollback boundaries are honest;
- no material contradiction remains hidden behind a worker verdict.

### Return packet and local proof budget

Return candidate/head identity, changed behavior and seams, current claim/non-goals,
proof run and deliberately not run, production-route evidence, findings and
dispositions, limitations, risk/rollback, current GitHub state, and typed candidate
result.

The writer runs formatting, diff hygiene, focused proof, and affected package/semantic
checks before publication when the local proof budget and host admission permit them.
Broad workspace, platform, package, or release proof remains hosted or risk-selected;
do not pay repository-wide CI cost after every edit.

A published candidate with affected proof still missing is `PR_IN_FLIGHT / NOT_PROVEN`,
not solid or review-current. Formatting and `git diff --check` are supporting evidence,
not behavioral proof.

## Procedure

1. Reuse the current authorized PR context, candidate branch/worktree, and loaded
   evidence.
2. Run `$build-from-proof` for missing implementation.
3. Run `$improve-test-suite` against the actual candidate.
4. Run `$simplify-candidate`; every changed revision returns through affected proof.
5. Run `$review-candidate`, including candidate-stage vision alignment against current
   authorities.
6. Repair ordinary findings through this same writer context and repeat affected proof
   and review.
7. Continue according to the result below; do not terminate merely because the
   implementation step completed.

## GitHub boundary

Publish when implementation changes the accepted claim/authority/route, when a reusable
production-path or external-truth finding affects later work, when a prerequisite or
support/risk boundary changes, or when the candidate-wide proof/limitation summary is
ready for PR review.

Keep agent identities, topology, task progress, temporary experiments, raw build logs,
retries, and routine local passes runtime-local. Do not post one update per edit, test,
agent, or normal skill transition.

## Valid exits

- `CANDIDATE_READY` → continue in the current PR context to publication/convergence
  through `$finish-pr`; if the PR already exists, continue to affected
  `$final-challenge` and `$review-pr`
- `CANDIDATE_FINDINGS_OPEN` → repair within this flow, then rerun affected proof and
  `$review-candidate`
- `WEAK_PROOF` → continue in this context to `$prepare-proof`, then resume this flow
- `MATERIAL_SCOPE_OR_AUTHORITY_CHANGE` → `$prepare-issue`; return to the parent only if
  the claim must split or change owner
- `NO_BUILD_SUBJECT` → return the no-build disposition to the invoking flow for
  proportional publication/review
- `WRITER_COLLISION` / `UNSAFE_WORKTREE` → resolve the same-candidate mechanical hazard
- `BLOCKED` / `NOT_PROVEN` → preserve the exact boundary and next skill or wake event

## What this establishes

A locally coherent publication candidate within the stated claim, produced without
throwing away the review context that discovered the repair.

## What this does not establish

Formal cumulative review, current GitHub checks, review-thread convergence, merge
authorization, or current-main reconciliation. Those continue through `$finish-pr` in
the current PR context.

---
name: build-candidate
description: Implement or complete one coherent candidate, harden its tests, simplify it, and challenge the actual result before candidate-bound local proof without discarding the review context that found the repair.
argument-hint: "[issue, branch, or candidate]"
---

# Build candidate

Existing coherent work enters midstream; do not replay completed chronology. This flow
may begin from reviewed proof, an existing candidate, or a candidate-owned finding
returned by `review-pr`, `verify-live-ci`, or another later skill.

Before creating another candidate, check only whether an equivalent current PR already
implements the same claim. Do not inspect sibling lanes, touched-file overlap, or nearby
symbols as a routine ownership check.

## Context continuity

`build-candidate` is a transition inside the current authorized PR context, not a reason
to replace its subagent.

A persistent claim lane or role-specialized reviewer may become the candidate writer
when:

- the finding is accepted and candidate-owned;
- the repair remains inside the current claim and non-goals;
- the parent brief grants mutation/publication authority;
- no other writer is mutating the same candidate.

Keep the same thread, worktree, loaded source context, and accepted evidence. Do not
return a repair packet solely so a fresh subagent can repeat the review. The real
boundary is authority and same-candidate writer exclusivity, not the role label the
context held before this skill.

A focused evidence worker remains read-only unless the parent explicitly promotes that
same context. When a reviewer is promoted, preserve the review evidence that motivated
the repair and continue back through affected proof and review. Add a genuinely
different oracle, method, threat model, environment, or reviewer when substantive merge
independence would otherwise collapse into the construction context.

## Orchestration affordances

### Context decisions

The current claim owner retains the material claim/non-goals and semantic owner,
accepted implementation latitude, proof sufficiency, risk/rollback boundary, finding
dispositions, material return-to-issue/proof decisions, and candidate sufficiency for
candidate-bound local proof.

### Useful execution and review contexts

Use focused subagents, context forks, an Ultracode workflow, or an Agent Team only where
useful for:

- bounded implementation assistance inside one admitted mutation boundary;
- owner/consumer verification;
- test hardening against the actual candidate;
- external language/protocol/dependency truth;
- production-path reachability;
- simplification and duplicate-authority review;
- specialist security, compatibility, lifecycle, packaging, persistence, performance,
  migration, or support review.

Give each child settled facts, exact claim/candidate identity, named skill where
applicable, mutation/read-only authority, falsifiers, proof budget, and return boundary.
Children return evidence or a bounded patch to the current PR context; they do not
create a second candidate owner.

### Mutation owner

One writer mutates the candidate branch/worktree at a time. The persistent claim lane is
normally that writer; a dedicated reviewer may be promoted in place when it already
holds the accepted finding and the parent grants authority.

Do not require a subagent replacement or cold start merely because the context crossed
from review to implementation. Require only the real authority change: accepted repair,
write permission, and no same-candidate writer collision.

### Join predicate

Join into one candidate only when implementation satisfies the current claim without
expanding authority; discriminating proof is current; production consumers reach the
behavior or the limitation is explicit; test-hardening/simplification findings and
accepted review findings are dispositioned; unsupported behavior and risk/rollback are
honest; and no contradiction is hidden behind a subagent verdict.

### Return packet and local proof budget

Return candidate/head identity, changed behavior/seams, claim/non-goals, proof run and
not run, production-route evidence, findings/dispositions, limitations, risk/rollback,
current GitHub state, and typed result.

The writer runs formatting, diff hygiene, focused proof, and affected package/semantic
checks while constructing the candidate when the local proof budget and host admission
permit them. `prove-before-push` then binds the committed candidate range to the
canonical affected-proof plan, Changie disposition, and applicable diff-scoped RIPR
result before ordinary publication.

Broad workspace/platform/package/release proof remains hosted or risk-selected; do not
pay repository-wide CI cost after every edit. Formatting and `git diff --check` are
supporting evidence, not behavioral proof.

## Flow

1. Reuse the current authorized PR context, candidate branch/worktree, and loaded
   evidence.
2. Invoke `build-from-proof` where implementation is missing.
3. Invoke `improve-test-suite` against the actual candidate.
4. Invoke `simplify-candidate`; every changed revision returns through affected proof.
5. Invoke `review-candidate` against current issue/contract/product authorities.
6. Repair through this same writer context and rerun affected proof/review.
7. Commit one coherent candidate and continue to `prove-before-push`; do not terminate
   merely because implementation completed.

## GitHub boundary

Publish durable issue/spec changes when implementation changes the accepted
claim/authority/route, or when a reusable production-path/external-truth finding affects
later work. Ordinary candidate publication occurs through `prove-before-push` and
`publish-pr`, not by posting one update per edit, test, subagent, or local transition.

Keep subagent/Team topology, task progress, temporary experiments, raw build logs,
retries, and routine local passes runtime-local.

## Routes

- `CANDIDATE_READY` → `prove-before-push` in the current PR context
- `CANDIDATE_FINDINGS_OPEN` → repair within this flow, then rerun affected proof and
  `review-candidate`
- `WEAK_PROOF` → continue in this context to `prepare-proof`, then resume this flow
- `MATERIAL_SCOPE_OR_AUTHORITY_CHANGE` → `prepare-issue`; return to the parent only if
  the claim must split or change owner
- `NO_BUILD_SUBJECT` → `prove-before-push` for proportional local disposition, then the
  invoking publication/review flow
- `WRITER_COLLISION` / `UNSAFE_WORKTREE` → resolve the same-candidate mechanical hazard
- `BLOCKED` / `NOT_PROVEN` → preserve the exact boundary and next skill or wake event

## What this establishes

A coherent local candidate within the stated claim, produced without throwing away the
review context that discovered the repair.

## What this does not establish

The candidate-bound pre-push result, PR publication, formal cumulative review, current
GitHub checks, review-thread convergence, merge authorization, or current-main
reconciliation. Those continue through `prove-before-push`, `publish-pr`, and
`finish-pr` in the current PR context.

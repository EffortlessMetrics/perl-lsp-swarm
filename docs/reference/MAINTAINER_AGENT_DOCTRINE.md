# Maintainer-agent operating contract

Status: current authority
Owner: perl-lsp maintainers
Method: [Development method](../agents/DEVELOPMENT_METHOD.md)
Review currentness: [Review and proof currentness](../agents/REVIEW_CURRENTNESS.md)
Provider routes: [`AGENTS.md`](../../AGENTS.md) and [`CLAUDE.md`](../../CLAUDE.md)

This document governs an agent acting with delegated maintainer authority over issues,
pull requests, branches, reviews, integration, and repository cleanup. It defines
judgment and safety boundaries. The provider route files and named skills define the
executable procedure.

The old fixed conveyor, permanent-agent roster, lifecycle-label state machine, and
exact-head review-receipt model are historical. Do not reconstruct them from archived
or superseded documents.

## Authority and evidence

Keep two authorities distinct:

```text
maintainer or system ruling
→ governs what the work should become

evidence, current source, and external constraints
→ govern what can be claimed and whether the requested mechanism is safe
```

Challenge while a question is open. Once a maintainer rules, execute that target unless
new evidence changes the relevant facts, safety boundary, or external constraint.

A challenge is not a reversal instruction. Correct the false premise or unsafe
mechanism while preserving the ruling's objective. Do not replace a settled preference
with a stricter or cleaner-looking policy merely because the alternative is attractive.

Examples:

| Instruction | Correct response |
| --- | --- |
| “Rebase this conflicting PR” | use rebase when it is a sound conflict-resolution strategy; preserve and refresh affected evidence |
| “Keep every PR exactly current” | reject the freshness-chasing mechanism unless live policy or a concrete integration need requires it |
| “Rebases are fine when useful; avoid CI churn” | allow concrete-purpose rebasing with no quota; reject repeated behind-only refresh |
| “Merge it” while a required check has a real failure | name the factual blocker and repair path; do not reinterpret the product objective |
| “Close duplicates” when the deltas differ | preserve the cleanup objective, correct the false duplicate premise, and harvest unique value |

When an override is necessary, record:

```text
instruction or ruling:
factual or safety conflict:
evidence checked:
preserved objective:
action taken:
remaining risk or authority needed:
```

## Current source of truth

Use the highest applicable current evidence:

1. current `origin/main`, live GitHub issue/PR/review/check/ruleset state, and actual
   repository behavior;
2. accepted current specifications, ADRs, policies, generated contracts, and
   independent proof;
3. the provider route files and named skills;
4. current shared method documents under `docs/agents/`;
5. runtime plans, workers, worktrees, memory, and conversation.

A document calling itself “north star,” “active doctrine,” or “accepted” is not current
merely because the words remain in an old file. Its local status and links must agree
with the current authority graph.

## Candidate-local execution

One coherent claim normally has:

```text
one current candidate
one branch/worktree
one mutation owner
one pull request
```

Read-only investigation may fan out. Different coherent claims may proceed in parallel,
including same-file work. Coordinate only when there is an equivalent candidate, an
explicit stack or prerequisite, one branch with multiple writers, destructive shared
runtime state, a real Git conflict, or a demonstrated combined-tree interaction.

Consequential writes are candidate-local. Finish the accounting for one branch before
mutating a different candidate through the same lane. Do not batch merges, closures,
force-pushes, title changes, or thread resolutions from a summary alone.

## Select the route from current state

Classify before acting:

- claim or authority unsettled;
- proof absent or non-discriminating;
- implementation incomplete;
- substantive finding open;
- candidate review-current;
- required checks pending or failed;
- textual conflict;
- same-seam or combined-tree interaction;
- explicit stack or prerequisite;
- duplicate or supersession candidate;
- environment, capacity, rate-limit, or instrument failure;
- merged but unreconciled.

Then enter the named provider-native route at the earliest missing useful judgment:

- `prepare-issue`;
- `prepare-proof`;
- `build-candidate`;
- `finish-pr`;
- `merge-reconcile`.

Do not force every PR through a fixed sequence of named agents or lifecycle labels.
Useful reviewers change the evidence surface; identities and labels do not create
independence by themselves.

## Review and proof currentness

Review is semantic and cumulative. A SHA identifies code and machine evidence; it is
not the human or agent review verdict.

Refresh only what later work can change:

- material claim, implementation, production route, authority, risk, compatibility,
  packaging, migration, support, or rollback change → refresh affected review and
  proof;
- focused finding repair → verify the finding, affected proof, and changed seam;
- actual conflict or combined-tree repair → review the interaction and affected seam;
- formatting, editorial cleanup, unrelated generated refresh, or other non-semantic
  movement → no broad review restart.

Green CI, mergeability, zero threads, a bot approval, or an old sign-off label cannot
create substantive review. A clean review is valid when it records scope, evidence and
falsifiers, what is established, and residual uncertainty.

## Integration and rebase

Behind-only movement requires no action.

```text
candidate remains conflict-free
+ unrelated main work lands
→ leave the candidate unchanged
```

Rebase is ordinary integration work. Its main accepted use is resolving an actual merge
conflict in the candidate lane. It is also available when refreshing the base materially
simplifies current owned work or reduces a concrete integration risk. Merge-main,
retarget, cherry-pick, reconstruction, or another bounded strategy may be better for a
particular conflict or stack.

There is no mechanical one-rebase limit. Distinct integration work may justify more
than one rebase. Repeated rebases solely to chase `main`, manufacture exact-head review,
or retrigger CI are churn.

Before branch mutation:

- establish the mutation owner;
- pin the expected current head;
- name the conflict, interaction, stack, policy, or active-work reason;
- identify proof and review that the mutation can affect;
- use `--force-with-lease` with an explicit expected SHA for a rewrite;
- stop if the remote head moved unexpectedly.

A missing required status on an unchanged candidate is a live integration fact, not
branch-mutation authority. Let the run report, request a same-head rerun where supported,
or return `NOT_PROVEN`.

## CI and failure classification

For each failed or missing gate, distinguish:

- candidate product or source defect;
- test or oracle defect;
- base-owned failure proven at the candidate's merge base;
- combined-tree interaction;
- required-policy mismatch;
- advisory finding;
- environment, runner, storage, quota, or capacity failure;
- cancellation or pending state;
- instrument failure;
- unknown or `NOT_PROVEN`.

Do not infer cause from a red check name. A cancelled run reached no verdict. A genuine
failure on an earlier candidate remains evidence until the affected seam changes or
replacement proof refutes it. Base attribution requires the same gate and failure
signature at the PR merge base, not merely some failure on current `main`.

Never weaken a test, ratchet, support claim, or required proof to obtain green status.
Start new enforcement advisory when existing repository state would create material
false blocks.

## Duplicate and supersession decisions

Never classify a candidate as duplicate from title, age, shared files, common ancestry,
diffstat, broad theme, or automated clustering alone.

Compare:

- acceptance criteria and user-visible behavior;
- production route and semantic model;
- APIs and helpers;
- tests, assertions, and negative controls;
- review findings and failure evidence;
- residual claims.

Possible outcomes:

- advance the existing candidate;
- sequence both;
- define an explicit stack;
- choose one and harvest unique value from the other;
- close as `SUPERSEDED_WITH_EVIDENCE`;
- return `NOT_PROVEN` when overlap cannot be established.

A supersession closeout names what landed or remains, why it owns the complete boundary,
what unique tests/ideas/evidence were preserved, and any follow-up owner.

## GitHub updates and waits

Publish only durable information that changes claim, authority, proof obligation,
finding, prerequisite, risk, route, accepted state, merge, or closeout.

Do not post repeated unchanged pending comments, exact-head status receipts, worker
liveness, retry logs, provisional reasoning, or runtime frontier state. When GitHub or
another remote system owns the next transition, record one useful wait and wake event,
return control, and advance another independent claim. Do not poll unchanged state.

Labels are navigation. Live issues, PRs, checks, reviews, threads, rulesets, and source
own the decision.

## Worktrees and cleanup

Use one worktree per genuine concurrent write claim, not per lifecycle pass. The main
checkout is a coordination surface, not an edit surface. Never use `git stash`; stash is
shared across worktrees.

Cleanup is evidence-gated:

- inspect dirty and untracked state;
- preserve unpushed or salvageable work;
- confirm the PR merged, closed, or was intentionally superseded;
- remove only lane-created worktrees and branches whose ownership is clear;
- never delete `.git/worktrees` metadata manually.

A squash merge means branch commit ancestry is not the landed history. Verify landed
behavior and unique deltas before deleting a branch.

## Irreversible actions

Before merge, closure, destructive cleanup, force-push, release, or publication:

- establish the exact durable subject and authority;
- verify the expected head or artifact identity;
- require current review and integration evidence;
- preserve unresolved findings and `NOT_PROVEN` boundaries;
- use the protected ordinary path without admin or policy bypass;
- verify and reconcile the result.

If two technically valid irreversible choices remain tied after evidence review, obtain
the owning maintainer decision. Otherwise make the safest reversible move and continue.

## Reporting

For each candidate, record only what another maintainer needs:

```text
subject and claim:
action taken:
proof and review currentness:
live integration state:
merge, repair, supersession, or blocker decision:
settled ruling preserved:
remaining risk or next material route:
```

Do not report that an agent is “standing by” when a safe next action is mechanically
available.

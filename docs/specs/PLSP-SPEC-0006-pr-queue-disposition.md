# PLSP-SPEC-0006: PR semantic incorporation and disposition

Status: accepted (amended 2026-08-11)
Owner: perl-lsp maintainers
Implementation issue: [#4560](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/4560)
Review currentness: [REVIEW_CURRENTNESS.md](../agents/REVIEW_CURRENTNESS.md)
Development method: [DEVELOPMENT_METHOD.md](../agents/DEVELOPMENT_METHOD.md)
Status impact: PR review, repair, integration, squash merge, supersession, and closeout

## Amendment and authority

The original accepted contract required every retained pull request to rebase onto
current `master` before proof and squash merge. It also used `needs-rebase` as a
classification for valuable work whose base had advanced.

Those requirements are superseded by this amendment.

The repository integrates a pull request's net result through squash merge. Branch age,
commit count, merge-base age, and history linearity are not acceptance criteria. A base
update is ordinary integration work selected for a concrete conflict, interaction,
stack, policy, or active-work reason. It is not a freshness ceremony.

The former link to the 0.14.0 readiness queue is historical. This specification is the
current durable disposition contract; provider-native procedures remain in `AGENTS.md`,
`CLAUDE.md`, and their named skills.

## Contract

Open pull requests are evidence, not a blind ordered queue. A maintainer or agent must
review each candidate or duplicate cluster against current repository reality, choose
an explicit semantic disposition, preserve unique value, and leave enough rationale for
a later maintainer to reconstruct the decision.

The governing rules are:

1. **Age and commit distance are metadata, not dispositions.** They may select a
   candidate for review. They do not select rebase, replacement, closure, or proof
   replay.
2. **Review the existing candidate first.** Repair it in place when it remains the best
   owner of the claim.
3. **Inspect current `main` without ceremonially mutating the candidate.** Determine
   whether the same semantic seam, prerequisite, generated authority, or accepted
   behavior changed.
4. **Preserve useful evidence.** Later work invalidates only the review and proof
   subjects it can affect.
5. **Keep candidate, integration, status, and landed identities distinct.** One cannot
   substitute for another.
6. **Prove supersession.** Similar titles, shared files, common ancestry, age, and
   diffstat do not establish duplicate value.
7. **Squash-merge only the expected current head.** Head movement rejects the merge
   attempt and requires a fresh live-state read; it does not automatically erase all
   prior semantic review.

## Evidence subjects and identities

### Semantic candidate and proof

Identity:

```text
cumulative PR change
+ claim and non-goals
+ implementation and production subjects
+ named proof subjects
```

A later candidate commit invalidates only the conclusions it can change. Formatting,
editorial cleanup, unrelated generated refreshes, and other changes that cannot affect a
completed proof subject do not erase that evidence. Material claim, implementation,
production-route, authority, risk, rollback, or tested-seam changes refresh the affected
proof and review.

### Integration

Identity:

```text
candidate combined with the current base, merge group, or synthetic integration tree
```

A real textual conflict, same-seam interaction, explicit stack dependency, changed live
policy, or demonstrated combined-tree failure selects integration work. Unrelated
movement on `main` does not automatically invalidate the candidate or require branch
mutation.

### Live required status

GitHub attaches a status or check result to the commit it evaluated. A required context
missing from the current PR head is pending or not proven for that live integration
state; an older success cannot be presented as a status GitHub recorded on the new head.

That does not authorize a rebase, empty commit, or unrelated source change. Let the
current run report, rerun or redispatch the same head where the platform supports it, or
return `NOT_PROVEN` with the missing capability.

### Merge race and landed result

The live PR head SHA is compare-and-swap protection at the irreversible merge boundary:

```bash
gh pr merge <n> --squash --match-head-commit <current-head-sha>
```

A moved head rejects that authorization. After squash merge, the landed commit on
`main` is the subject of reconciliation, issue closeout, source-truth updates, and safe
cleanup.

## Invalidation matrix

| Later event | Candidate and proof | Review | Integration and live status | Required response |
| --- | --- | --- | --- | --- |
| Unrelated `main` movement | current | current | recompute only when policy or a concrete interaction requires it | leave the candidate unchanged |
| Candidate edit cannot affect a completed proof subject | evidence remains usable | no broad refresh | current-head required statuses may still be pending separately | preserve proof; wait or rerun only required live status |
| Candidate changes a tested or reviewed seam | affected evidence is stale | refresh affected dimensions | recompute affected integration | focused proof and review |
| `main` changes the same semantic seam | prior candidate evidence remains evidence | review the interaction | integration may be required or blocked | `REVIEW_INTEGRATION_INTERACTION` |
| Textual conflict appears | candidate evidence remains useful | refresh conflict-affected conclusions after repair | blocked until resolved | `RESOLVE_CONFLICT` |
| Explicit stack prerequisite changes | child-only evidence remains useful | refresh changed child/interaction seams | reconcile the stack basis | smallest useful retarget, rebase, merge-main, cherry-pick, or reconstruction |
| Required policy changes | evidence remains evidence | reevaluate applicability | satisfy current policy or return `NOT_PROVEN` | no invented branch churn |
| PR head moves before merge | semantic review may remain current | refresh only affected dimensions | old merge authorization is rejected | reread live state and use the new expected head |

## Canonical dispositions

Every reviewed candidate receives one current disposition:

| Disposition | Meaning |
| --- | --- |
| `MERGE_EXISTING_CANDIDATE` | The current candidate is useful, correct, review-current, conflict-free, and needs no source mutation before squash merge |
| `REPAIR_EXISTING_CANDIDATE` | The candidate remains the best owner but has a concrete implementation, proof, review, or documentation defect |
| `RESOLVE_CONFLICT` | A real textual conflict exists; inspect the interaction before selecting a repair strategy |
| `REVIEW_INTEGRATION_INTERACTION` | Current `main`, a prerequisite, or another candidate changed the same semantic contract; compare models before editing |
| `RECONCILE_BASE_FOR_CONCRETE_REASON` | A base mutation or retarget is useful for one recorded integration or active-work reason |
| `SALVAGE_UNIQUE_DELTA` | Branch topology, contamination, ownership, or deletion makes the current branch unusable; preserve the bounded unique value elsewhere |
| `SUPERSEDED_WITH_EVIDENCE` | Another landed or retained implementation owns the complete acceptance boundary and the original candidate's unique value has been harvested |
| `NOT_PROVEN` | Required source, review, proof, policy, or tool evidence could not be established |
| `BLOCKED` | A product, architecture, ownership, policy, safety, or external-authority decision is required |

There is no age-driven or behind-driven `needs-rebase` disposition.

## Base reconciliation and rebase

Rebase is an ordinary integration tool. Its main accepted use is resolving an actual
merge conflict in the candidate lane. The lane may also rebase when a base refresh
materially simplifies active work or reduces a concrete integration risk. Merge-main,
retarget, cherry-pick, reconstruction, or another bounded strategy may be better for a
particular conflict or stack.

There is no mechanical one-rebase limit. Distinct integration work may justify more
than one rebase.

A `RECONCILE_BASE_FOR_CONCRETE_REASON` disposition must name at least one reason:

- an actual textual conflict;
- current `main` changed the same semantic contract and adaptation is required;
- an explicit stack prerequisite changed so the child cannot be reviewed or tested
  independently;
- live branch protection or merge-queue policy requires a current integration basis;
- selected proof cannot be interpreted without incorporating the prerequisite or
  current contract;
- refreshing the base materially simplifies current owned work or reduces a named
  integration risk.

These facts are insufficient by themselves:

- the candidate is old or inactive;
- the branch is many commits behind;
- unrelated files changed on `main`;
- the branch contains merge commits or non-linear history;
- a cleaner graph would be aesthetically preferable;
- a current status is missing and the operator wants to retrigger CI;
- a prior rebase already happened or has not happened yet.

Before rewriting a branch, establish its mutation owner, expected old head, permission
to rewrite, the evidence affected by the change, and the proof/review to refresh. Use an
explicit lease for force-pushes. Never mutate another active writer's candidate merely
to make the branch look current.

## Conflict and unknown-state semantics

Keep these observations distinct:

| Observation | Meaning | Default route |
| --- | --- | --- |
| `MERGEABLE` | GitHub reports no textual conflict | continue semantic review and live integration checks regardless of age |
| `CONFLICTING` | GitHub reports a textual conflict | inspect mechanical, semantic, stack, and generated-authority interaction before selecting a strategy |
| `UNKNOWN_NOT_PROVEN` | GitHub or the available tool cannot establish mergeability | retry boundedly or report `NOT_PROVEN`; do not mutate, merge, or close |
| `BEHIND_ONLY` | The candidate is conflict-free while `main` advanced | no required action |
| `SUPERSEDED_CANDIDATE` | A landed or retained candidate may own the same acceptance boundary | compare complete deltas and harvest unique value before closure |

`UNSTABLE`, `BLOCKED`, and similar GitHub summaries must be decomposed into required
status, advisory status, review, thread, policy, conflict, or platform facts. A summary
word is not a semantic disposition.

## Stacked candidates under squash merge

For an explicit stack:

1. record the parent PR/head and child PR/head;
2. inspect the child-only delta separately from cumulative stack behavior;
3. after the parent squash-merges, compare the child-only value with landed `main`;
4. treat duplicated parent commits as topology to reconcile, not proof that the child is
   obsolete;
5. select retarget, rebase, merge-main, cherry-pick, or reconstruction only as needed to
   preserve and make the child delta reviewable;
6. bind new machine evidence to the resulting subject;
7. preserve prior review and proof for unchanged seams.

Parent or base movement alone does not erase useful child review.

## Duplicate and supersession handling

Never classify a candidate as duplicate or superseded from:

- title similarity;
- shared base commits;
- shared files or helpers;
- similar diffstat;
- the same broad issue theme;
- age, inactivity, or branch divergence;
- automated clustering alone.

Compare the acceptance criteria, production behavior, APIs/helpers, assertions, negative
controls, review findings, and residual claims.

A `SUPERSEDED_WITH_EVIDENCE` closeout records:

```text
winning PR, commit, or current-main evidence:
acceptance criteria compared:
unique tests preserved:
unique implementation ideas preserved:
review and failure evidence preserved:
why the original candidate is no longer the best owner:
follow-up owner, if any:
```

If two candidates are independently valuable, sequence both or define an explicit stack
rather than discarding one as a vague duplicate.

## Required disposition record

A durable PR comment, review, or closeout should contain enough evidence to answer:

```text
source basis:
candidate, base, and current-main identity:
claim and acceptance criteria checked:
current disposition:
current-main or sibling interaction:
unique value:
concrete defects or blockers:
proof run and proof not run:
review currentness:
base-reconciliation reason, if any:
merge or close rationale:
follow-up:
```

Use `none` only when a field is genuinely not applicable. The packet indexes evidence;
it does not replace the diff, checks, submitted review, or current GitHub state.

## Proof and merge authorization

Proof is selected by changed behavior and risk, not branch age.

Before protected squash merge:

- substantive review is current under the semantic currentness contract;
- required GitHub checks are satisfied for the live subject GitHub protects;
- no current change request or unresolved substantive thread remains;
- deliberately requested review is complete;
- mergeability, rulesets, queue state, and applicable release/changelog policy permit
  integration;
- any selected combined-tree proof is current for its stated basis;
- the merge call names the expected current PR head.

A passing local command can preflight or diagnose. It does not replace a required hosted
status. A missing, partial, timed-out, cancelled, rate-limited, or instrument-failed
result is `NOT_PROVEN`, not success and not permission to change unrelated source.

## Examples

### Old, clean, conflict-free candidate

The candidate is three weeks old and many commits behind, but current `main` did not
change its semantic seam. Review and required checks are satisfied.

Disposition: `MERGE_EXISTING_CANDIDATE`.

### Old candidate with a real correctness defect

Age is irrelevant; the defect is concrete.

Disposition: `REPAIR_EXISTING_CANDIDATE`.

### Unrelated movement on `main`

`main` gained parser and documentation changes while the candidate changes a DAP test
seam. No conflict or interaction exists.

Disposition: leave the candidate unchanged.

### Same semantic seam changed

`main` and the candidate choose different models for the same contract.

Disposition: `REVIEW_INTEGRATION_INTERACTION`; compare before editing.

### Actual textual conflict

Disposition: `RESOLVE_CONFLICT`; rebase is one available implementation, not the name of
the problem.

### Stacked child after parent squash

Preserve the child-only delta and reconcile topology only as needed. Do not discard the
child or its unchanged review because the parent landed under a new squash commit.

### Truly superseded candidate

Compare the full acceptance boundary, harvest unique tests and evidence, then close as
`SUPERSEDED_WITH_EVIDENCE` with the winning commit cited.

### Unknown GitHub state

Disposition: `NOT_PROVEN`. Do not infer conflict, safe merge, or safe closure.

### Invalid branch-freshness argument

```text
The PR is 500 commits behind, therefore rebase it before review.
```

Invalid. Commit distance may prompt a semantic comparison, but it does not select a
mutation.

## Automation boundary

A structural checker may validate:

- required identity and rationale fields;
- allowed dispositions;
- a concrete base-reconciliation reason;
- supersession-harvest fields;
- expected-head merge protection;
- absence of age-only or behind-only mutation rationale.

It must not decide whether an implementation is correct, valuable, superseded, or safe
to merge. Current GitHub facts, repository proof, and maintainer judgment own those
decisions.

Source-aware regression tests should fail when current authority:

- mandates update/rebase merely because a candidate is behind;
- imposes a one-rebase quota;
- treats every head change as total review/proof invalidation;
- conflates a required current status with a need to mutate source;
- calls a conflict `needs rebase` without classifying the interaction;
- presents expected-head merge protection as review freshness;
- names `master` as this repository's current branch;
- leaves the superseded mandatory-rebase contract looking accepted and current.

Historical and forensic records remain evidence and are excluded from current-authority
wording checks.

## Non-goals

- Do not require every open candidate to merge.
- Do not prohibit useful rebase, merge-main, retarget, cherry-pick, reconstruction, or
  same-head workflow rerun.
- Do not weaken required checks, branch protection, review threads, or change requests.
- Do not require heavy combined-tree proof for every candidate.
- Do not define release readiness or publication approval.
- Do not create a lifecycle database, overlap map, autonomous queue scheduler, or
  product-value oracle.
- Do not define branch/worktree deletion safety beyond the ownership and evidence
  requirements above.
- Do not rewrite dated historical evidence to make it look current.

## Claim boundaries

A disposition record proves only that the named candidate and comparison basis were
reviewed through the stated evidence boundary. It does not prove broad product
correctness, release readiness, support-tier promotion, or unrelated provider behavior.

Closing a candidate as superseded means the cited replacement contains the reviewed
value and the original's unique value was preserved. It does not reject every idea in
the closed branch.

Merging a candidate means the expected head passed the required review and integration
boundary. It does not validate stale PR-body claims, unrelated checks, sibling branches,
or future changes on `main`.

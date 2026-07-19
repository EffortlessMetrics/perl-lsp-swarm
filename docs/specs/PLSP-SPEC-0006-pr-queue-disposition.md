# PLSP-SPEC-0006: PR semantic incorporation and disposition

Status: accepted (amended 2026-07-19)
Owner: perl-lsp maintainers
Linked proposal: [PLSP-PROP-0001](../proposals/PLSP-PROP-0001-real-perl-editor-trust.md)
Linked program: [#4552](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/4552)
Implementation issues: [#4553](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/4553), [#4554](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/4554), [#4556](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/4556), [#4560](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/4560)
Status impact: PR review, repair, conflict resolution, stack handling, squash merge, supersession, and close recommendations

## Amendment from the original contract

The original accepted text required every retained PR to rebase onto the current
base before proof and squash merge. That requirement is superseded by this
amendment.

The repository squash-merges PRs. The PR branch's age, commit count, merge-base
age, and history linearity are therefore not integration goals. A branch update
is a repair tool selected for a concrete conflict, semantic interaction, stack,
policy, or proof reason. It is not a ceremonial freshness step.

The original contract's valuable requirements remain in force:

- open PRs are evidence, not a blind ordered queue;
- old, large, agent-authored, behind, CI-stale, or conflict-prone work is not
  closed without semantic review;
- sibling and duplicate overlap is inspected before choosing a winner;
- unique tests, implementation ideas, and failure evidence are preserved;
- merge and close decisions carry exact rationale;
- product failures, test failures, policy failures, and instrument failures are
  classified separately.

## Contract

A maintainer or agent reviews each PR or duplicate cluster against current
repository reality, then chooses an explicit semantic disposition.

The governing rules are:

1. **Age is metadata, not a disposition.** Inactivity may select a PR for
   current review. It cannot select close, rebase, cherry-pick, or replacement.
2. **Review the existing branch first.** Repair it in place when it remains a
   usable owner of the change.
3. **Fetch current `main` for comparison; do not automatically merge it into the
   PR.** Record whether `main` changed the same semantic seam.
4. **Preserve exact-head evidence.** Unnecessary pushes invalidate current-head
   checks, review convergence, and review continuity.
5. **Separate PR-head proof from integration proof.** A changed integration
   basis may require new combined-tree proof without changing or re-reviewing an
   unchanged PR head.
6. **Supersession is proven, not inferred.** Similar titles, shared files,
   shared base commits, diff size, age, or broad theme are insufficient.
7. **The irreversible merge is a squash merge of the expected full head SHA.**
   If the head moved, authorization is stale.

## Evidence identities

Three identities are distinct and must not substitute for one another:

| Identity | Owns | Invalidated by |
| --- | --- | --- |
| PR head SHA | implementation diff, focused proof, current-head checks, review convergence, substantive-thread dispositions | any PR-head change |
| integration base plus merge-group or synthetic integration SHA | applicability to current `main`, combined-tree compile/test behavior, interaction with queued work | integration-base or candidate-set change |
| post-merge `main` SHA | landed result, issue/spec closeout, source-truth and cleanup reconciliation | a later mainline change only for claims that depend on it |

A reviewer may inspect both the exact PR head and current `main`. Each conclusion
must say which object it describes.

## Invalidation matrix

| Event | PR-head proof | Review convergence | Integration proof | Required response |
| --- | --- | --- | --- | --- |
| PR head changes | stale | stale for the resulting changed seams | stale | return to current-head review and proof |
| `main` advances in unrelated areas | current | current | stale only when tied to the older integration basis | refresh integration decision when applicable; leave the PR head unchanged |
| `main` changes the same semantic seam | remains evidence for the reviewed head, but interaction is unresolved | re-review changed/interacting seams as needed | stale or newly required | `REVIEW_SEMANTIC_INTERACTION` |
| textual conflict appears | current but not integrable | current unless repair changes the head | blocked | `RESOLVE_CONFLICTS` |
| a stacked prerequisite lands in a materially different shape | current for the old stack basis | current only for unchanged child seams | stale | compare the child-only delta and select a bounded stack repair |
| live policy or required-check set changes | remains evidence | reevaluate applicability | stale or `NOT_PROVEN` | recompute readiness from live policy |

## Required review packet

Each PR selected for convergence should produce one bounded current packet:

```yaml
schema: pr-incorporation-v1
repository: EffortlessMetrics/perl-lsp-swarm
pr: 1234
verified_at: 2026-07-19T00:00:00Z

identity:
  head_sha: <full sha>
  base_sha: <full sha>
  current_main_sha: <full sha>
  source_basis: PROVEN

mergeability:
  mergeable: true
  actual_conflicts: false

issue:
  still_valid: true
  acceptance_criteria:
    - ...

current_main_interaction:
  same_semantic_seam_changes: []
  material_interaction: false
  equivalent_implementation_landed: false
  stacked_prerequisite_changed: false

implementation:
  unique_value:
    - ...
  concrete_defects: []
  contamination: []

proof:
  red_green_present: true
  focused_tests: green
  required_checks: green
  not_proven: []

review:
  unresolved_substantive: 0
  current_head_reviewed: true

disposition: MERGE_EXISTING_HEAD
reason: ...
```

The packet is a review index, not a replacement for GitHub, the diff, test
artifacts, or current-head merge readiness.

## Canonical dispositions

| Disposition | Meaning |
| --- | --- |
| `MERGE_EXISTING_HEAD` | The existing head is useful, correct, conflict-free, reviewed, and needs no mutation before squash merge |
| `REPAIR_EXISTING_BRANCH` | The branch remains the best owner; fix concrete implementation, proof, or review defects there |
| `RESOLVE_CONFLICTS` | An actual textual conflict exists; inspect it and select the smallest correct repair |
| `REVIEW_SEMANTIC_INTERACTION` | Current `main` changed the same contract or model; compare before editing |
| `UPDATE_BASE_REQUIRED` | Base mutation is required for one recorded concrete reason |
| `SALVAGE_UNIQUE_DELTA` | Branch topology or contamination makes the branch unusable; preserve bounded unique work elsewhere |
| `SUPERSEDED_WITH_EVIDENCE` | A better implementation landed or owns the complete acceptance boundary; unique value has been harvested |
| `NOT_PROVEN` | Required source, policy, tool, or proof evidence could not be established |
| `BLOCKED` | A product, architecture, ownership, policy, or external-authority decision is required |

There is no `RETIRE_BECAUSE_OLD` disposition. `needs-rebase` is not a semantic
classification.

## Valid reasons to update a PR branch

`UPDATE_BASE_REQUIRED` must name at least one concrete reason:

- an actual textual merge conflict;
- current `main` changed the same semantic contract and the implementation must
  adapt;
- a stacked prerequisite changed so the child cannot be reviewed or tested
  independently;
- live branch protection or merge-queue policy requires a current integration
  basis;
- meaningful proof cannot be interpreted without incorporating a prerequisite
  or current contract.

These facts are insufficient by themselves:

- the PR is old or inactive;
- the branch is many commits behind;
- unrelated files changed on `main`;
- the branch contains a merge commit or non-linear history;
- a cleaner graph would look nicer;
- a reviewer feels more comfortable after a refresh without naming a semantic,
  conflict, policy, stack, or proof reason.

When a base update is required, record branch ownership, expected old head,
force-push authorization when applicable, evidence invalidated by the mutation,
and the proof/review that must rerun.

## Conflict and unknown-state semantics

Queue observations must distinguish:

| State | Meaning | Default action |
| --- | --- | --- |
| `MERGEABLE` | GitHub reports no textual conflict | Continue normal semantic review and exact-head proof regardless of age |
| `CONFLICTING` | GitHub reports an actual textual conflict | Inspect mechanical versus semantic conflict; do not assume rebase |
| `UNKNOWN_NOT_PROVEN` | GitHub cannot establish mergeability or required state | Retry boundedly or report `NOT_PROVEN`; do not mutate or close |
| `IDLE_REVIEW_NEEDED` | No qualifying activity during the observation window | Run current semantic review; do not infer abandonment or obsolescence |
| `SUPERSEDED_CANDIDATE` | A sibling or landed change may cover the same acceptance boundary | Compare full deltas and harvest unique value before closure |

`UNSTABLE` and other GitHub summaries must be decomposed into required proof,
advisory proof, review, policy, or platform state before deciding what blocks.

## Stacked PRs under squash merge

For a stack:

1. record parent PR/head and child head;
2. inspect the child-only delta and cumulative stack behavior separately;
3. after the parent squash-merges, compare the child's unique delta with landed
   `main`;
4. treat duplicated parent commits as topology to reconcile, not evidence that
   the child is obsolete;
5. retarget, merge-main, rebase, cherry-pick, or reconstruct only when needed to
   preserve and make the child delta reviewable;
6. bind new proof to the resulting child head;
7. preserve prior review evidence for unchanged seams.

## Duplicate and supersession handling

Never classify a PR as duplicate or superseded from:

- shared base commit;
- shared helper or test file;
- similar diffstat;
- same broad issue theme;
- title similarity;
- age, inactivity, or branch divergence;
- automated clustering alone.

Compare changed files, semantic behavior, helpers/APIs, assertions, negative
controls, and acceptance criteria.

A `SUPERSEDED_WITH_EVIDENCE` or duplicate closure records:

```text
winning PR/commit/current-main evidence:
acceptance criteria compared:
unique tests preserved:
unique implementation ideas preserved:
review/failure evidence preserved:
why the original branch is no longer the best owner:
follow-up owner, if any:
```

If two PRs are independently valuable, sequence both or define a stack instead
of discarding one as a vague duplicate.

## Proof and merge authorization

Proof is selected by the touched behavior and risk, not by branch age.

Before merge:

- required checks apply to the exact current head;
- current-head review convergence is satisfied;
- substantive conversations are resolved with evidence where required;
- live policy permits integration;
- applicable integration proof is current for its base/group;
- the PR remains mergeable;
- the merge call uses squash semantics and the expected full head SHA.

A local command may preflight or compute evidence, but a merge-relevant gate must
be visible through the normal GitHub-integrated review, CI, queue, or readiness
surface. Missing or broken instrumentation is `NOT_PROVEN`, not success and not
permission to change unrelated production code.

Same-head proof refresh and base integration are separate operations. Missing,
cancelled, or stale workflow evidence on an unchanged head should be rerun or
dispatched against that head when supported. It does not by itself authorize an
`update-branch`, empty commit, rebase, or force-push.

## Comment template

```text
source basis:
head/base/current-main identity:
classification:
disposition:
acceptance criteria checked:
overlap checked:
current main already contains:
material same-seam interaction:
unique value:
concrete defects:
proof run / required proof:
review convergence:
base update reason, if any:
close/merge rationale:
follow-up:
```

Use `none` only when a field is genuinely not applicable.

## Automation boundary

A structural checker may validate identity fields, allowed dispositions,
concrete base-update reasons, and supersession-harvest fields. It must not decide
whether an implementation is correct, valuable, semantically superseded, or
merge-authorized.

New enforcement starts advisory when existing repository state would produce
false blocks. Historical and archived documents remain evidence, not current
instruction.

## Invalid closure reasons

None of these is a valid close rationale by itself:

- old or inactive;
- behind `main`;
- large;
- agent-generated;
- CI-stale;
- conflict-prone but repairable;
- old checks are red or green;
- another PR title sounds similar;
- a current-main rewrite exists but the acceptance boundary was not compared.

## Non-goals

- Do not require every open PR to merge.
- Do not prohibit a necessary rebase, merge-main, update-branch, retarget,
  cherry-pick, or reconstruction.
- Do not weaken code review, security review, required checks, review
  convergence, or protected integration.
- Do not define release readiness or publication approval.
- Do not define branch/worktree deletion safety; the worktree and reconciliation
  authorities own cleanup.
- Do not create a lifecycle database, fixed agent topology, or numeric global
  merge order.

## Claim boundaries

A disposition packet proves only that the named PR/head and comparison basis
were reviewed through the stated evidence boundary. It does not prove broad
product correctness, release readiness, support-tier promotion, or unrelated
provider behavior.

Closing a PR as superseded means the cited replacement contains the specific
reviewed value and that unique value was preserved. It does not reject every idea
in the closed PR.

Merging a PR means the expected head passed the required review and proof at the
stated boundary. It does not validate stale checks, unrelated PR-body claims, or
sibling branches.

# Control-plane authority map

Status: active
Scope: PR convergence, queue observation, current-head proof, merge, and reconciliation
Owner: perl-lsp maintainers
Effective from: 2026-07-19
Program: [#4552](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/4552)

## Purpose

This page answers one question:

> Which repository artifact owns a PR/queue/merge decision, and which nearby
> documents are adapters, planned transitions, or historical evidence?

It is an index, not a workflow engine. GitHub state, the exact source objects,
review/proof artifacts, and live repository policy remain authority.

## Document status vocabulary

Operational documents should use one of these statuses in frontmatter or a
visible header:

| Status | Meaning |
| --- | --- |
| `active` | Current authority or implemented adapter for the declared scope |
| `transitional` | Still used by compatibility paths, but not final authority |
| `planned` | Accepted target/issue exists, but the operational adapter is not implemented yet |
| `superseded` | Replaced; retained only for traceability |
| `historical` | Dated evidence or forensics; never current instruction |
| `draft` | Proposal that is not yet authoritative |

A superseded document should name its successor. Historical directories may be
classified by directory policy instead of editing every file.

## Shared vocabulary

Do not infer similarly named states from neighboring prose:

- semantic PR dispositions (`MERGE_EXISTING_HEAD`,
  `REPAIR_EXISTING_BRANCH`, `UPDATE_BASE_REQUIRED`,
  `SUPERSEDED_WITH_EVIDENCE`, and related results) are defined normatively in
  [PLSP-SPEC-0006](../specs/PLSP-SPEC-0006-pr-queue-disposition.md);
- queue observations (`CONFLICTING`, `UNKNOWN_NOT_PROVEN`,
  `IDLE_REVIEW_NEEDED`, and related states) are owned by
  [#4554](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/4554) and
  [#4570](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/4570);
- current-head merge-readiness result classes are owned by the implemented M1
  contract from [#3988](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/3988)
  and its live-collector continuation
  [#4565](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/4565).

`UNKNOWN_NOT_PROVEN` is the queue-facing representation of a `NOT_PROVEN`
mergeability claim; it is not a second proof standard.

## Authority by concern

| Concern | Canonical authority | Implemented/transitional/planned adapters | Historical or superseded material |
| --- | --- | --- | --- |
| Repository development lifecycle | issue [#3949](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/3949) and its accepted linked specs | root agent guidance and named lifecycle skills (transitional where not yet linked) | session-specific lifecycle checklists |
| PR semantic disposition | [PLSP-SPEC-0006](../specs/PLSP-SPEC-0006-pr-queue-disposition.md) and issue [#4553](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/4553) | maintainer doctrine, PR-incorporation packet, queue handoff | age-driven salvage/rebase classifiers |
| Source basis for durable conclusions | issue [#4568](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/4568) | local freshness diagnostics, SHA-pinned GitHub inspection | unqualified stale-checkout status comments |
| Current issue plan | issue [#4569](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/4569) and the linked spec graph | current-plan index comment and builder view (planned) | overlapping status comments without supersession |
| Queue visibility and idle/conflict/unknown observations | issues [#4554](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/4554) and [#4570](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/4570) | current queue snapshot is transitional until the explicit-state schema lands; advisory labels and ops queue guidance are navigation only | age-to-rebase/close guidance and conflated interpretations of inactivity/conflict/unknown state |
| Current-head review convergence | issue [#3693](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/3693) and its canonical script/docs | ops and merge-readiness consumers | labels or `reviewDecision` treated as sufficient proof |
| Repository and affected proof | issues [#3985](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/3985) and [#3987](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/3987) | local pre-push helpers and GitHub checks | manual proof narratives used as final-head authority |
| Same-head proof refresh | issue [#4564](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/4564) | **planned** GitHub adapter; current guidance must return `NOT_PROVEN` when no non-mutating trigger exists | `update-branch` or empty commits used only to trigger CI |
| Combined-tree integration proof | issue [#4556](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/4556) | **planned** merge-group or synthetic squash-integration receipt | mandatory update/rebase of every PR |
| Merge readiness and irreversible merge | implemented snapshot evaluator from [#3988](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/3988) plus live continuation [#4565](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/4565) | M1 evaluator is active; live collection and expected-head merge adapter are **planned** | `merge-ready` labels or prompt-maintained check lists as proof |
| PR/CI observation | issue [#4566](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/4566) | bounded event/poll adapter is **planned** | root-driven repeated full-state polling |
| Post-merge reconciliation | issue [#3989](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/3989) | reconcile and cleanup skills | merge-as-completion session assumptions |
| Branch/worktree ownership and cleanup safety | [WORKTREE_PROTOCOL.md](WORKTREE_PROTOCOL.md) and issue [#3957](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/3957) | worktree manager and local admission/cleanup tools | age-only branch/worktree cleanup |
| Lane ownership | [LANE_BOUNDARIES.md](LANE_BOUNDARIES.md) | explicit maintainer lane comments and labels as navigation | cross-lane cleanup treated as authority |

## Core distinctions

### Labels are navigation, not proof

Labels may locate candidates or project a workflow state. They do not prove:

- the current PR head passed required checks;
- review evaluated the current head;
- substantive threads are dispositioned;
- a PR is semantically superseded;
- a branch is safe to mutate or delete;
- the current plan is accepted.

Use the canonical evidence owner for the decision.

### Read current `main`; do not ceremonially mutate the PR

Reviewers fetch or inspect current `main` to answer:

- did the same semantic seam change?
- did an equivalent implementation land?
- did a stacked prerequisite change?
- is there an actual textual conflict?

A finding of no material interaction is a valid reason to leave the PR head
unchanged. Branch mutation requires one concrete reason from PLSP-SPEC-0006.
Live policy may still require a current integration basis; that is an explicit
policy reason, not a commit-distance heuristic.

### Proof refresh is not base integration

Missing, cancelled, or stale workflow evidence for an unchanged head should be
retried or dispatched for that head when an implemented adapter supports it. If
no non-mutating trigger is available, report `NOT_PROVEN`; do not pretend the
planned #4564 adapter already exists.

Combined-tree applicability belongs to the integration-proof authority.

### Exact-head proof is not combined-tree proof

A PR head may remain reviewed and green while a later integration basis needs a
new compile or interaction check. Conversely, a green synthetic integration
result is not a substitute for review of a changed PR head.

### Parallelism is bounded by convergence capacity

File non-overlap is useful but not sufficient for unlimited writers. Shared
constraints include:

- one writer per branch;
- public API and semantic overlap;
- generated files, registries, schemas, workflows, and release authorities;
- review and CI capacity;
- integration order;
- current-main source truth.

Read-only fan-out may be broad. Writer admission is limited by the repository's
ability to review, prove, and integrate the resulting heads.

## Adapter migration state

The following active adapters are aligned and link to this map in this slice:

- `.claude/agents/green-ci.md`
- `.claude/commands/ops-check-queue.md`
- `.claude/commands/ops-merge-batch.md`
- `.claude/commands/rebase-pr.md`

These active-looking entry points remain explicit migration targets under
[#4561](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/4561); they
must not be described as aligned until they link to and follow the map:

- root `AGENTS.md` and `CLAUDE.md` orchestration sections;
- `docs/reference/MAINTAINER_AGENT_DOCTRINE.md`;
- `.claude/agents/ops.md`;
- `docs/ci/github-queue-snapshot.md` until the queue-schema slice lands;
- other current non-archived handoff/swarm-pack operational docs.

This distinction prevents a new authority map from claiming discoverability that
has not yet been implemented.

## Historical policy

These locations are evidence, not current instruction, unless an active document
explicitly promotes a statement:

- `docs/reference/archive/**`
- dated `docs/forensics/**`
- dated session records and retrospectives
- old release-specific readiness plans

Do not rewrite historical records to make them look current. When an active
document links to one, identify it as historical and link the current successor.

## Change discipline

A change to a canonical authority should:

1. name the superseded rule;
2. update active adapters or file bounded follow-ups;
3. retain historical evidence;
4. start new enforcement advisory when false-block risk is material;
5. avoid adding another label, database, or prompt-owned authority for the same
   decision.

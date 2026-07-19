# Control-plane authority map

Status: active
Scope: PR convergence, queue observation, current-head proof, merge, and reconciliation
Owner: perl-lsp maintainers
Effective from: 2026-07-19
Program: [#4552](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/4552)

## Purpose

This page answers one question:

> Which repository artifact owns a PR/queue/merge decision, and which nearby
> documents are adapters or historical evidence?

It is an index, not a workflow engine. GitHub state, the exact source objects,
review/proof artifacts, and live repository policy remain authority.

## Document status vocabulary

Operational documents should use one of these statuses in frontmatter or a
visible header:

| Status | Meaning |
| --- | --- |
| `active` | Current authority for the declared scope |
| `transitional` | Still used by compatibility paths, but not final authority |
| `superseded` | Replaced; retained only for traceability |
| `historical` | Dated evidence or forensics; never current instruction |
| `draft` | Proposal that is not yet authoritative |

A superseded document should name its successor. Historical directories may be
classified by directory policy instead of editing every file.

## Authority by concern

| Concern | Canonical authority | Active adapters / projections | Historical or superseded material |
| --- | --- | --- | --- |
| Repository development lifecycle | issue [#3949](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/3949) and its accepted linked specs | root agent guidance and named lifecycle skills | session-specific lifecycle checklists |
| PR semantic disposition | [PLSP-SPEC-0006](../specs/PLSP-SPEC-0006-pr-queue-disposition.md) and issue [#4553](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/4553) | maintainer doctrine, PR-incorporation packet, queue handoff | age-driven salvage/rebase classifiers |
| Source basis for durable conclusions | issue [#4568](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/4568) | local freshness diagnostics, SHA-pinned GitHub inspection | unqualified stale-checkout status comments |
| Current issue plan | issue [#4569](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/4569) and the linked spec graph | current-plan index comment and builder view | overlapping status comments without supersession |
| Queue visibility and idle/conflict/unknown observations | issues [#4554](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/4554) and [#4570](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/4570) | queue snapshot, advisory labels, ops queue command | `stale_or_dirty`, age-to-rebase/close guidance |
| Current-head review convergence | issue [#3693](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/3693) and its canonical script/docs | ops and merge-readiness consumers | labels or `reviewDecision` treated as sufficient proof |
| Repository and affected proof | issues [#3985](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/3985) and [#3987](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/3987) | local pre-push helpers and GitHub checks | manual proof narratives used as final-head authority |
| Same-head proof refresh | issue [#4564](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/4564) | green-CI/ops GitHub adapter | `update-branch` or empty commits used only to trigger CI |
| Combined-tree integration proof | issue [#4556](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/4556) | merge-group or synthetic squash-integration receipt | mandatory update/rebase of every PR |
| Merge readiness and irreversible merge | completed M1 from [#3988](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/3988) plus live continuation [#4565](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/4565) | ops merge command and readiness packet | `merge-ready` labels or prompt-maintained check lists as proof |
| PR/CI observation | issue [#4566](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/4566) | bounded event/poll adapter | root-driven repeated full-state polling |
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

### Proof refresh is not base integration

Missing, cancelled, or stale workflow evidence for an unchanged head should be
retried or dispatched for that head when supported. It does not by itself
justify `update-branch`, rebase, merge-main, an empty commit, or force-push.

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

## Active operational surfaces to align

The following files are active adapters and must link back to the authorities
above rather than inventing their own lifecycle truth:

- `docs/reference/MAINTAINER_AGENT_DOCTRINE.md`
- `.claude/agents/ops.md`
- `.claude/agents/green-ci.md`
- `.claude/commands/ops-check-queue.md`
- `.claude/commands/ops-merge-batch.md`
- `.claude/commands/rebase-pr.md`
- `docs/ci/github-queue-snapshot.md`
- root `AGENTS.md` and `CLAUDE.md` orchestration sections

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

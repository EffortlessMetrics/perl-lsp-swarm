# Agent and maintainer authority status

Status: current authority index
Owner: perl-lsp maintainers
Machine registry: [`authority_status.toml`](authority_status.toml)
Tracking issue: [#4555](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/4555)

This page answers one bounded question:

> When two repository documents teach different agent, review, queue, branch, or
> orchestration rules, which one is current?

Use the machine registry for the path-by-path classification. It is not yet a closed
inventory of every agent-facing document: it classifies the root provider routes, the
current method contracts, the known transitional current-main defects, and the legacy
doctrine/ADR/design graph. Ten files under `docs/agents/` remain unclassified, and
`ORCHESTRATION_ROLES.md` is the one that matters most — it routes readers to the
superseded `ORCHESTRATION_DOCTRINE.md` and `PIPELINE_GATES.md`. Classifying the
remainder is named residual work on #4555. An unclassified path does not thereby become
current; apply the reading rule below.

Internal words
such as “accepted,” “active doctrine,” “north star,” “operating contract,” or “current”
inside a document listed as `historical`, `superseded`, or `transitional` do not override
this index. Those words remain part of the historical record until local status banners
are migrated.

This index is not a scheduler, lifecycle database, or replacement for source evidence.
It classifies documentation authority only.

## Current method

The current provider-native method is defined by:

- root [`AGENTS.md`](../../AGENTS.md) for Codex;
- root [`CLAUDE.md`](../../CLAUDE.md) for Claude;
- [Development method](DEVELOPMENT_METHOD.md);
- [Review and proof currentness](REVIEW_CURRENTNESS.md);
- [GitHub surfaces](GITHUB_SURFACES.md);
- [Skill contract](SKILL_CONTRACT.md);
- [Session operations](../how-to/SESSION_OPERATIONS.md);
- [Agent contributing guide](../how-to/AGENT_CONTRIBUTING.md).

The current shape is:

```text
current durable issue, candidate, review, check, or merge
→ select the narrowest provider-native route
→ use one mutation owner for one candidate
→ challenge with differentiated evidence
→ preserve useful findings and proof
→ integrate only when live GitHub policy permits
→ reconcile the landed result
```

It does **not** require:

- a fixed seven-stage conveyor;
- a permanent named-agent roster;
- lifecycle labels as authority;
- an always-on reconciler;
- exact-head human-review receipts;
- branch refresh because `main` advanced;
- file reservations or a central scheduler.

## Transitional current-main defects

Several paths remain active-looking on `main` while replacement PRs are in review. They
are classified `transitional`, not silently treated as current:

| Path | Replacement |
| --- | --- |
| `docs/reference/MAINTAINER_AGENT_DOCTRINE.md` | #4555 / PR #6868 — current ruling, review, integration, and cleanup contract |
| `docs/reference/WORKTREE_PROTOCOL.md` | #4555 / PR #6868 — one mutation owner and concrete-purpose branch rewrite |
| `CONTRIBUTING.md` | #4555 / PR #6868 — provider-native review and retained contributor routes |
| `.github/copilot-instructions.md` | #4555 / PR #6868 — current route map and crate entrypoints |
| `scripts/ci/check-pr-review-convergence-core` | public wrapper plus #5778 — compatibility collector containment |

Three rows left this table because their replacements landed on `main` while this
candidate was open. They are reclassified rather than kept pending:

| Path | Landed | New status |
| --- | --- | --- |
| `docs/specs/PLSP-SPEC-0006-pr-queue-disposition.md` | #4560 / PR #6863 (`e9a698285f`) | `current` — the amended specification retired the mandatory-rebase contract itself and now declares that it *is* the current durable disposition contract |
| `scripts/ci/check-pr-claim-currentness` | #5778 / PR #6871 (`c5c43757ed`) | `historical` — fixture-mode reader with no live review-convergence authority |
| `scripts/reviews/claim-digest` | #5778 / PR #6871 (`c5c43757ed`) | `historical` — the CLI prints a RETIRED notice and exits |

Leaving `PLSP-SPEC-0006` classified `transitional` would have demoted a
still-authoritative document — the exact inversion this index exists to prevent. The
validator now reads each transitional document and rejects a row the document's own
content contradicts.

Until those candidates land, use the current issue rulings and provider-native method.
Do not “fix” the mismatch by repeatedly rebasing the candidates; unrelated `main`
movement is not an authority change.

## Historical and superseded design graph

The following documents remain useful evidence of prior operating eras but are not
current instruction:

- `docs/reference/ORCHESTRATION_DOCTRINE.md`;
- `docs/reference/PIPELINE_GATES.md`;
- `docs/reference/OCTOPUS_CLUSTER.md`;
- `docs/reference/GLOSSARY.md`;
- `docs/reference/LIVE_SIGNALS_VS_LABELS.md`;
- `docs/adr/0044-octopus-cluster-orchestration.md`;
- `docs/articles/PIPELINE_STATE_MACHINE.md`;
- `docs/handoff/SWARM_DESIGN.md`;
- `.spec/3988-merge-readiness/spec.md`.

Do not rewrite their historical observations merely to make the past look consistent.
Do not route current work from their fixed stages, label taxonomies, receipt rules,
permanent coordinators, or branch-refresh examples.

## Surviving principles

Supersession does not mean every idea in the old graph was rejected. The current method
retains these principles where current source supports them:

- GitHub is a durable multi-writer substrate;
- live facts beat stale label claims;
- candidate variance can be useful search;
- proof should be scoped and discriminating;
- duplicate closure must preserve unique value;
- instrument failure is not product success or failure;
- learning belongs in durable issues, reviews, contracts, and code.

The old implementation model does not survive merely because one principle does.

## Reading and retrieval rule

When search lands directly inside an old document:

1. check its path in `authority_status.toml`;
2. follow the named successor;
3. verify the current issue, PR, source, review, and live GitHub state;
4. use historical content only for the facts or rationale it actually records;
5. do not turn internal old status words into current authority.

A current document may cite a historical one. The citation does not promote the whole
historical document.

## Migration discipline

The registry is an immediate authority correction, not the final local-banner migration.
For each transitional or historical active-looking path, #4555 should eventually choose
one local disposition:

```text
current authority
historical design record
superseded by <successor>
partially retained reference with named surviving sections
```

Local migration must preserve historical content and links while removing misleading
current-status claims. The registry remains the machine-checkable inventory that prevents
a document from silently returning to current authority.

## Claim boundary

This index proves only the declared documentation status at the checked-in revision. It
does not prove that a candidate is correct, review-current, green, mergeable, or safe to
publish. Current source and live GitHub evidence own those decisions.

# Active Goals

Active goals are machine-readable current-state manifests for `perl-lsp` lanes.
They let agents identify the current objective, active work item, proof commands,
and status pointers without scraping chat or hand-maintained narrative.

| Layer | Owns | Must not do |
|---|---|---|
| Active goal | Machine-readable current work, status pointers, active work-item IDs, proof command list | Prose-only strategy, generated status content, durable design rationale |

## Manifest Contract

The active manifest should live at `.perl-lsp/goals/active.toml`. Future archived
manifests should move under `.perl-lsp/goals/archive/`.
Manifest and work-item IDs must be stable slug IDs: lowercase ASCII letters,
digits, and single hyphens only.

An active manifest should include:

- stable lane ID and title
- active/inactive status
- objective and end state
- current work items
- links to the relevant proposal, spec, ADR, plan, and status docs
- proof commands that define the current checkable boundary

## Validation

Validate the active manifest with:

```bash
rtk cargo xtask check-active-goal-manifest
```

This check verifies that `.perl-lsp/goals/active.toml` is parseable and that
declared proposal, plan, spec, ADR, status, receipt, and command-reference paths
exist. References must be repo-relative without root components or
parent-directory traversal, and path strings must not be empty or include
leading or trailing whitespace. Top-level `specs`, `adrs`, and `status_docs` arrays are
document inventories, so their entries must not include Markdown anchors or
`path::symbol` references. Top-level `proposal`, `plan`, and `previous_goal`
fields are document paths and must not include Markdown anchors or
`path::symbol` references; use `status_pointer`, work-item pointers, or receipt
fields for precise anchors. The manifest `created` field must use a valid
`YYYY-MM-DD` calendar date.
The manifest must carry a non-empty `objective`, `end_state`, and
`claim_boundaries` so agents can reason from repo artifacts. `end_state` and
`claim_boundaries` entries must be unique and must not include leading or
trailing whitespace. Top-level `title` and `owner` values must also avoid
leading or trailing whitespace. The checked manifest is the active handoff and its
top-level `status` must be `active`. Each work item must carry a non-empty proof command list,
and those commands must be unique, must not include leading or trailing
whitespace, and must use the workspace's `rtk` command prefix so they are
directly copyable by agents. Each
work item's status and proof commands must also appear in the linked
implementation-plan section so the manifest and plan cannot drift. Each work item must also carry a
plan pointer under the active manifest's top-level plan, plus a current-state
pointer, and status-doc current pointers under `docs/project/status/` must be
listed in top-level `status_docs`. The plan anchor must match the work-item ID. Work-item specs must be listed in the
manifest's top-level `specs` list and mentioned in the linked implementation-plan
section. The primary `status_pointer` may include a Markdown anchor, but its
underlying document path must be listed in `status_docs`. Top-level path arrays must not contain duplicates, and
an `active` goal must retain at least one non-completed work item. Work-item
`spec` fields are document inventory pointers and must not include Markdown
anchors or `path::symbol` references. Work-item `plan` and `current_pointer`
fields may use Markdown anchors where required or relevant, but must not use
`path::symbol`; symbol anchors belong in `receipt` or `*_receipt` fields.
Markdown anchors and `path::symbol` receipt anchors are checked when supplied. Each
linked plan section must include `Proof commands` and `Rollback` headings so it
is executable and reversible from repo artifacts, plus `Non-goals` and
`Acceptance` headings so the proof target is visible. The section must also
include `Claim boundary` and the same work-item `claim_boundary` prose from the
manifest; the validator normalizes whitespace so Markdown wrapping is allowed.
The exact work-item `current_pointer` must also appear in the linked plan
section so both artifacts route agents to the same evidence. When a work item
carries `current_status`, the same status prose must also appear in the linked
plan section with the same whitespace-normalized comparison. When a work item
carries `trigger` or `blocked_by`, the same routing prose must also appear in the
linked plan section. Any `receipt` or `*_receipt` field must also appear in the
linked plan section so closed or supporting evidence stays visible outside the
TOML. `planned` work items must explain their route with `trigger` or `current_status`, and `blocked`
work items must name `blocked_by`, so agents do not mistake parked work for an
immediate slice. `active` and `ready` work items must include `current_status`
so actionable handoffs explain why they are executable now. Work-item
`claim_boundary`, `current_status`, `trigger`, and `blocked_by` prose must not
include leading or trailing whitespace. `completed` work
items must include a `receipt` or `*_receipt` path so closed work remains
traceable to repo evidence.

The validator output reports both open work and actionable work. `active` and
`ready` work items are actionable; `planned` and `blocked` work items stay
visible as open/parked work but are not counted as an immediate next slice. If
an active manifest has open work but zero actionable work items, it must include
`next_action` so agents know whether to select a new ready item, wait for a
trigger, archive the goal, or replace it. A `0 actionable` manifest without that
handoff is not a complete source of truth. When `next_action` is present, it
must be a non-empty string. At most one work item may be marked `active`; when
one exists, `current_work_item` must point at that active item rather than a
separate `ready` item. When `next_action` and `current_work_item` are both
present, `next_action` must mention the exact current work-item ID so the prose
handoff cannot drift from the machine-readable pointer.

If an active manifest has any actionable work item, it must include
`current_work_item`, and that value must match an `active` or `ready` work-item
ID and use the same stable slug-id format. This keeps the next executable slice machine-readable; `next_action` remains
the human-readable handoff note. The success output includes the current work
item so `rtk cargo xtask check-active-goal-manifest` can serve as a compact
handoff receipt. The receipt also prints the current work item's plan pointer,
current-state pointer, current status, and claim boundary so the next
implementation slice is visible without reopening the TOML by hand. It then
prints the current work item's proof commands exactly as recorded in the
manifest.

The check proves manifest reference integrity only. It does not prove the lane
is complete, promote support tiers, refresh generated status, or validate the
behavior claimed by a receipt.

The top-level active goal `proposal`, `previous_goal`, `status_pointer`,
`objective`, `end_state`, `claim_boundaries`, `specs`, `adrs`, and
`status_docs` entries must also be mentioned in the top-level implementation plan
so both handoff artifacts expose the same lane rationale, objective, finish
line, governing contracts, current evidence owners, and archive trail.

## Status Pointers

Goal manifests point at status docs; they do not copy generated state. Preferred
current-state pointers for Real Perl Editor Trust are:

- [parser accuracy next](../../docs/project/status/parser_accuracy_next.md)
- [parser status](../../docs/project/status/parser.md)
- [provider cutover](../../docs/project/status/provider_cutover.md)
- [semantic scorecard](../../docs/project/status/semantic_scorecard.md)
- [semantic shadow compare](../../docs/project/status/semantic_shadow_compare.md)
- [UX capability dashboard](../../docs/project/status/ux_capability_dashboard.md)

## Minimal Shape

```toml
id = "plsp-real-perl-editor-trust"
title = "Real Perl editor trust"
status = "active"
owner = "codex-swarm"
created = "YYYY-MM-DD"
current_work_item = "work-item-id"
proposal = "docs/proposals/PLSP-PROP-####-short-name.md"
plan = "plans/real-perl-editor-trust/implementation-plan.md"
previous_goal = ".perl-lsp/goals/archive/YYYY-MM-DD-previous-goal.toml"
status_pointer = "docs/project/status/parser_accuracy_next.md"

specs = [
  "docs/specs/PLSP-SPEC-####-short-name.md",
]

adrs = [
  "docs/adr/PLSP-ADR-####-short-name.md",
]

status_docs = [
  "docs/project/status/parser_accuracy_next.md",
  "docs/project/status/parser.md",
]

objective = """
State the active lane objective.
"""

end_state = [
  "State a checkable lane outcome.",
]

claim_boundaries = [
  "State what this goal must not claim without proof.",
]

[[work_item]]
id = "work-item-id"
status = "active"
spec = "docs/specs/PLSP-SPEC-####-short-name.md"
plan = "plans/real-perl-editor-trust/implementation-plan.md#work-item-work-item-id"
current_pointer = "docs/project/status/parser_accuracy_next.md"
current_status = "Ready because parser status routes the current proof slice."
claim_boundary = "Fixture-only parser proof; no parser bucket reduction claim without fresh generated status."
commands = [
  "rtk cargo xtask update-status --only parser --check",
]
```

Replace placeholders with existing repo-relative paths, and make sure the linked
implementation-plan section mirrors the work-item status, current pointer,
current status, claim boundary, required headings, and proof commands.

# Editor Trust UX Closeout Implementation Plan

Status: active
Owner: perl-lsp maintainers
Linked proposal: [PLSP-PROP-0001](../../docs/proposals/PLSP-PROP-0001-real-perl-editor-trust.md)
Active goal: [active.toml](../../.perl-lsp/goals/active.toml)
Previous goal archive: [2026-05-18-real-perl-editor-trust.toml](../../.perl-lsp/goals/archive/2026-05-18-real-perl-editor-trust.toml)

## Current State

The original Real Perl Editor Trust control-plane lane is archived as complete.
Its proposal, initial specs, ADRs, generated parser status, provider confidence
matrix, support tiers, and real-workspace receipts remain the source of truth
for what can be claimed.

This plan owns the next active lane: turning that control plane into a
user-facing editor-trust product. It does not reopen broad parser bucket work
while generated parser status lists no current nonzero raw bucket, and it does
not promote provider behavior from docs or receipt-only PRs.

Goal objective:

Make the existing Real Perl Editor Trust control plane visible and useful in
normal editor workflows: explanations, receipts, workspace trust state,
preview-first refactors, receiver-aware completion, and claim-shaped CI proof.

Goal end state:

- User-facing trust and setup surfaces are documented, snapshotted, and supportable from receipts.
- Receiver-aware completion has a narrow source-backed pilot with fallback-preserving proof.
- Rename and safe-delete remain bounded by false-allow, freshness, blocker, and rollback receipts.
- Workspace trust report and setup docs are the support front door.
- Support claims, README claims, and VS Code command docs match proof tiers.
- CI routing classifies PRs by trust lane and names required proof.

Lane claim boundaries:

- Do not claim parser bucket reduction without a refreshed corpus receipt.
- Do not broaden live provider behavior from receipt PRs alone.
- Do not hand-edit generated status sections.
- Do not claim all-CPAN support or full dynamic Perl inference.
- Do not promote edit-producing providers without rollback, blocker, fallback, and no-edit proof.
- Do not turn generated, dynamic, stale, or low-confidence facts into exact source-backed claims.

Current merged contract anchors:

- [PLSP-SPEC-0002](../../docs/specs/PLSP-SPEC-0002-provider-confidence-receipts.md)
- [PLSP-SPEC-0003](../../docs/specs/PLSP-SPEC-0003-real-workspace-editor-baseline.md)
- [PLSP-SPEC-0005](../../docs/specs/PLSP-SPEC-0005-receiver-expression-facts.md)
- [PLSP-SPEC-0007](../../docs/specs/PLSP-SPEC-0007-receiver-fact-completion.md)
- [PLSP-SPEC-0008](../../docs/specs/PLSP-SPEC-0008-edit-producing-provider-safety.md)
- [PLSP-SPEC-0009](../../docs/specs/PLSP-SPEC-0009-workspace-trust-report.md)
- [PLSP-SPEC-0010](../../docs/specs/PLSP-SPEC-0010-support-claim-map.md)
- [PLSP-SPEC-0011](../../docs/specs/PLSP-SPEC-0011-trust-lane-ci-routing.md)
- [PLSP-SPEC-0012](../../docs/specs/PLSP-SPEC-0012-user-facing-trust-surfaces.md)
- [PLSP-SPEC-0013](../../docs/specs/PLSP-SPEC-0013-agent-build-storage-and-gates.md)
- [PLSP-SPEC-0014](../../docs/specs/PLSP-SPEC-0014-refactor-acceptance.md)
- [PLSP-SPEC-0015](../../docs/specs/PLSP-SPEC-0015-real-perl-editor-trust-v1-boundary.md)
- [PLSP-SPEC-0016](../../docs/specs/PLSP-SPEC-0016-provider-decision-receipt-v1.md)
- [PLSP-SPEC-0017](../../docs/specs/PLSP-SPEC-0017-fact-provenance-and-source-backing.md)
- [PLSP-SPEC-0018](../../docs/specs/PLSP-SPEC-0018-edit-authorization-contract.md)
- [PLSP-SPEC-0019](../../docs/specs/PLSP-SPEC-0019-semantic-token-class-promotion-contract.md)
- [PLSP-SPEC-0020](../../docs/specs/PLSP-SPEC-0020-workspace-symbol-generated-label-contract.md)
- [PLSP-SPEC-0021](../../docs/specs/PLSP-SPEC-0021-diagnostic-explanation-v1.md)
- [PLSP-SPEC-0022](../../docs/specs/PLSP-SPEC-0022-module-path-authority.md)
- [PLSP-SPEC-0023](../../docs/specs/PLSP-SPEC-0023-ambient-inputs.md)
- [PLSP-ADR-0001](../../docs/adr/PLSP-ADR-0001-generated-status-is-control-plane.md)
- [PLSP-ADR-0002](../../docs/adr/PLSP-ADR-0002-confidence-before-cutover.md)
- [PLSP-ADR-0003](../../docs/adr/PLSP-ADR-0003-preview-before-edit.md)

Status owners:

- [Real Perl Editor Trust dashboard](../../docs/project/status/real_perl_editor_trust_v1.md)
- [support tiers](../../docs/project/status/SUPPORT_TIERS.md)
- [provider confidence matrix](../../docs/project/status/provider_confidence_matrix.md)
- [provider cutover](../../docs/project/status/provider_cutover.md)
- [UX capability dashboard](../../docs/project/status/ux_capability_dashboard.md)
- [semantic scorecard](../../docs/project/status/semantic_scorecard.md)
- [semantic shadow compare](../../docs/project/status/semantic_shadow_compare.md)
- [receiver facts](../../docs/project/status/receiver_facts.md)
- [parser accuracy next](../../docs/project/status/parser_accuracy_next.md)
- [parser status](../../docs/project/status/parser.md)

## Work item: editor-trust-user-facing-contracts

Status: completed
Linked proposal: [PLSP-PROP-0001](../../docs/proposals/PLSP-PROP-0001-real-perl-editor-trust.md)
Linked specs: [PLSP-SPEC-0011](../../docs/specs/PLSP-SPEC-0011-trust-lane-ci-routing.md), [PLSP-SPEC-0012](../../docs/specs/PLSP-SPEC-0012-user-facing-trust-surfaces.md)
Current pointer: `docs/project/status/real_perl_editor_trust_v1.md`
Blocks: user-facing trust docs, command documentation, setup troubleshooting
Blocked by: none

Claim boundary

Spec and docs work only; no provider behavior or support-tier promotion.

Goal

Encode the remaining user-facing trust contracts in durable specs so
agents do not need chat context to know what explanations, receipts, previews,
workspace trust, support claims, and CI routing may claim.

Production delta

Spec and ADR changes only. No provider behavior, support-tier promotion, parser
runtime change, workspace scan, DAP launch, perldoc execution, or CI routing
implementation unless the PR explicitly says it is a validator/policy PR.

Non-goals

No broad roadmap. No generated status content. No current PR queue ordering.
No behavior cutover from docs-only PRs.

Acceptance

Each spec states contract, non-goals, valid PR shapes, invalid PR shapes, proof
commands, claim boundaries, and status docs that own current evidence.

Proof commands

```bash
rtk cargo xtask check-support-claims
rtk cargo xtask check-provider-confidence-matrix
rtk git diff --check
```

Rollback

Revert the affected spec PR. If a contract is wrong, leave the active
goal in place and mark the specific work item blocked until a replacement
contract lands.

## Work item: editor-trust-user-docs

Status: completed
Linked proposal: [PLSP-PROP-0001](../../docs/proposals/PLSP-PROP-0001-real-perl-editor-trust.md)
Linked spec: [PLSP-SPEC-0012](../../docs/specs/PLSP-SPEC-0012-user-facing-trust-surfaces.md)
Current pointer: `docs/project/status/SUPPORT_TIERS.md`
Blocks: README claim refresh, VS Code command docs
Blocked by: none
Receipt: [Editor Trust](../../docs/how-to/EDITOR_TRUST.md)
Supporting receipts:
[Perl Setup Troubleshooting](../../docs/how-to/PERL_SETUP_TROUBLESHOOTING.md),
[VS Code README](../../vscode-extension/README.md),
[Commands reference](../../docs/reference/COMMANDS_REFERENCE.md)

Claim boundary

Plain-language docs must link to support tiers and avoid broad CPAN/static/refactor claims.

Goal

Add user-facing docs that explain measured Perl editor trust without requiring
users to read provider matrices or dashboards.

Production delta

Add or update docs for editor trust, setup troubleshooting, command discovery,
and bug-report receipts.

Non-goals

No support-tier promotion. No broad CPAN/static/refactor claims. No duplicated
support matrix.

Acceptance

Docs explain partial-live-with-fallback, fallback reasons, safe-edit previews,
dynamic Perl boundaries, `@INC`/module resolution, workspace trust report,
provider explanations, diagnostic explanations, and copyable receipts in plain
language while linking to status/support truth sources.

Current implementation status

The user-facing guide, setup troubleshooting guide, VS Code trust-claim docs,
and command reference now cover the trust and explanation surfaces. This work
item is documentation only; it does not promote support tiers or broaden
provider behavior.

Proof commands

```bash
rtk cargo xtask check-support-claims
rtk cargo xtask check-provider-confidence-matrix
rtk git diff --check
```

Rollback

Revert the docs PR. If wording overclaims, narrow the docs before any support
claim changes.

## Work item: workspace-trust-schema-snapshots

Status: completed
Linked proposal: [PLSP-PROP-0001](../../docs/proposals/PLSP-PROP-0001-real-perl-editor-trust.md)
Linked spec: [PLSP-SPEC-0009](../../docs/specs/PLSP-SPEC-0009-workspace-trust-report.md)
Current pointer: `docs/project/status/SUPPORT_TIERS.md`
Blocks: setup troubleshooting docs, support-front-door docs
Receipt: `crates/perl-lsp-rs/tests/lsp_execute_command_tests.rs::test_execute_command_workspace_trust_report_schema_snapshot`

Claim boundary

Report existing state only; do not probe Perl, run perldoc, launch DAP, scan files, or promote tiers.

Goal

Lock the user-facing workspace trust report shape enough that setup support and
bug reports do not churn.

Production delta

Add focused snapshots or tests for the report schema and output-channel
presentation. The report remains read-only over existing server/client state.

Non-goals

No Perl probing. No perldoc execution. No DAP launch. No workspace scan. No
support-tier promotion.

Acceptance

Snapshots cover workspace roots, include path state, `PERL5LIB` policy, perldoc
contract state, DAP/perldoc runtime state when supplied, launch config
counts/classes, provider tiers, dynamic caveats, and copyable payload fields.

Proof commands

```bash
rtk cargo test -p perl-lsp-rs --test lsp_execute_command_tests test_execute_command_workspace_trust_report --profile agent --locked -- --nocapture --test-threads=1
rtk cargo xtask check-support-claims
rtk git diff --check
```

Rollback

Revert snapshots or output changes. If the report shape is unstable, mark the
work item blocked and keep docs from promising a stable payload.

## Work item: receiver-fact-completion

Status: completed
Linked proposal: [PLSP-PROP-0001](../../docs/proposals/PLSP-PROP-0001-real-perl-editor-trust.md)
Linked specs: [PLSP-SPEC-0005](../../docs/specs/PLSP-SPEC-0005-receiver-expression-facts.md), [PLSP-SPEC-0007](../../docs/specs/PLSP-SPEC-0007-receiver-fact-completion.md)
Current pointer: `docs/project/status/receiver_facts.md`
Blocks: broader receiver-form completion expansion
Blocked by: none
Receipt: [receiver facts status](../../docs/project/status/receiver_facts.md)
Supporting receipts:
[support tiers](../../docs/project/status/SUPPORT_TIERS.md),
[provider cutover](../../docs/project/status/provider_cutover.md),
[provider confidence matrix](../../docs/project/status/provider_confidence_matrix.md),
`crates/perl-lsp-rs-core/src/providers/completion/completion/tests.rs`

Claim boundary

Narrow source-backed receiver completion pilot only; unknown and dynamic receivers preserve legacy fallback.

Goal

Use receiver facts to make method completion useful for source-backed object and
package shapes while preserving fallback for unknown and dynamic receivers.

Production delta

First add receiver fact extraction or receipts. Enable a narrow live completion
pilot only after source-backed, fresh, high-confidence receiver facts and
fallback preservation are proven.

Non-goals

No completion behavior change from facts-only PRs. No generated/no-source
promotion. No dynamic hash key exactness. No suppression of legacy candidates
for unknown receivers.

Acceptance

Facts expose receiver kind, inferred package, shape fact, confidence, evidence,
freshness, dynamic boundary, source range, and fallback state. Completion
receipt proves source-backed `$self`/object/package/hashref known-slot ranking,
dynamic fallback, and unknown fallback before any pilot.

Current implementation status

Receiver facts, expression-fact substrate, completion ranking receipts, the
narrow source-backed receiver completion pilot, and the support review have
landed. Completion remains partial-live-with-fallback: only fresh,
high-confidence, source-backed receiver evidence may contribute exact method
candidates in the narrow pilot, while unknown, dynamic, generated/no-source,
stale, low-confidence, and broader workspace-wide method shapes remain
fallback, shadowed, or blocked.

Proof commands

```bash
rtk cargo test -p perl-semantic-analyzer --lib receiver_fact --profile agent --locked -- --nocapture
rtk cargo test -p perl-lsp-rs-core --lib completion --profile agent --locked -- --nocapture
rtk cargo xtask check-support-claims
rtk cargo xtask check-provider-confidence-matrix
rtk git diff --check
```

Rollback

Revert the facts, receipt, or pilot PR. If fallback changes unexpectedly, revert
the pilot first and keep facts-only evidence for follow-up.

## Work item: receiver-real-workspace-quality-receipt

Status: ready
Linked proposal: [PLSP-PROP-0001](../../docs/proposals/PLSP-PROP-0001-real-perl-editor-trust.md)
Linked specs: [PLSP-SPEC-0003](../../docs/specs/PLSP-SPEC-0003-real-workspace-editor-baseline.md), [PLSP-SPEC-0007](../../docs/specs/PLSP-SPEC-0007-receiver-fact-completion.md)
Blocks: broader receiver-form completion expansion
Blocked by: none
Current pointer: [receiver facts next implementation steps](../../docs/project/status/receiver_facts.md#next-implementation-steps) (`docs/project/status/receiver_facts.md#next-implementation-steps`)

Claim boundary

Receipt-only receiver quality proof; no completion behavior change, generated/dynamic promotion, or support-tier promotion.

Goal

Add a receipt-only receiver quality slice over real-workspace or project-shaped
completion evidence so the next receiver expansion decision is based on current
provider confidence data instead of unit fixtures alone.

Production delta

Add or refresh a focused provider-confidence receipt for exact, fallback, and
dynamic receiver cases. The receipt may extend scenario coverage or status
evidence, but it must preserve existing completion output unless the PR is
explicitly promoted later by a separate cutover row.

Non-goals

No completion behavior change. No generated/no-source or dynamic receiver
promotion. No support-tier promotion. No parser bucket claim. No broad
workspace-wide method completion cutover.

Acceptance

The receipt names the receiver form, inferred package or absence of one,
confidence, evidence source, freshness, dynamic-boundary state, fallback state,
and claim boundary. Unknown, dynamic, generated/no-source, stale, low-confidence,
and medium-confidence receiver cases remain fallback, shadowed, or blocked.

Current implementation status

Ready next because receiver facts have a narrow source-backed pilot and the
status page routes follow-up to real-workspace receiver-quality receipts before
broader completion expansion.

The active dashboard names additional real-workspace receiver-quality receipts
as the next completion proof before broader generated, dynamic, method, or
workspace-wide completion cutover. The receiver facts status page also routes
next work to real-workspace and additional receiver-form provider confidence
receipts after the landed narrow source-backed pilot.

Proof commands

```bash
rtk cargo test -p perl-lsp-ux-tests --test ux_scenario_28_mojolicious_completion_ranking --profile agent --locked -- --nocapture
rtk cargo test -p perl-semantic-analyzer --lib receiver_fact --profile agent --locked -- --nocapture
rtk cargo test -p perl-lsp-rs-core --lib completion --profile agent --locked -- --nocapture
rtk cargo xtask check-support-claims
rtk cargo xtask check-provider-confidence-matrix
rtk git diff --check
```

Rollback

Revert the receipt PR. If the receipt exposes behavior drift, keep the current
fallback behavior and route a separate fix or cutover PR through the provider
promotion ledger.

## Work item: edit-provider-safety-refresh

Status: planned
Linked proposal: [PLSP-PROP-0001](../../docs/proposals/PLSP-PROP-0001-real-perl-editor-trust.md)
Linked spec: [PLSP-SPEC-0008](../../docs/specs/PLSP-SPEC-0008-edit-producing-provider-safety.md)
Current pointer: `docs/project/status/provider_confidence_matrix.md`
Blocks: rename/safe-delete support review
Blocked by: provider fact or workspace-index changes that require refreshed edit-safety proof

Claim boundary

Rename and safe-delete remain narrow pilots until false-allow, freshness, blocker, and rollback proof justify more.

Trigger condition

Run only when rename facts, safe-delete facts, workspace-index
behavior, or edit-producing provider code changes. Docs-only changes,
unrelated refactors, receiver-completion work, parser fixtures, workspace-trust
reporting, or CI routing do not require a new edit-safety receipt by default.

Goal

Keep rename and safe-delete false-allow, edit-freshness, blocker, and rollback
receipts current as facts and workspace indexing evolve.

Production delta

Receipt/test updates only unless a separate live-cutover PR satisfies the
edit-producing provider safety contract.

Non-goals

No broader package rename. No broader safe-delete. No generated, dynamic,
imported/exported, stale, low-confidence, ambiguous, non-source-backed,
non-subroutine, or package-wide edit authorization.

Acceptance

Generated/no-source, dynamic, referenced, imported/exported, stale,
low-confidence, ambiguous, non-subroutine, and package-wide cases return no
edit or fallback with explicit reasons and copyable receipts.

Current implementation status

Current rename and safe-delete receipts are reviewed and bounded; no refresh is
required from docs-only or unrelated refactor work.

Current rename and safe-delete receipts have already been reviewed in the
provider confidence matrix, support tiers, provider cutover notes, and Real Perl
Editor Trust dashboard. Rename remains limited to same-file lexical rename and
the narrow package-local pilot; safe-delete remains limited to the exact
unreferenced source-backed subroutine pilot with current-source, workspace
reference, workspace identity, and rollback guards. This work item stays planned
as a trigger-based maintenance lane, not as the next unconditional provider
cutover.

Proof commands

```bash
rtk cargo test -p perl-lsp-rs --lib refactor_runtime_blocker_ux_safe_delete --profile agent --locked -- --nocapture --test-threads=1
rtk cargo test -p perl-lsp-rs-core --lib safe_delete_shadow --profile agent --locked -- --nocapture
rtk cargo xtask check-support-claims
rtk cargo xtask check-provider-confidence-matrix
rtk git diff --check
```

Rollback

Revert the receipt or behavior PR. If an edit-producing live path is unsafe,
revert live behavior before changing docs.

## Work item: trust-lane-ci-routing

Status: completed
Linked proposal: [PLSP-PROP-0001](../../docs/proposals/PLSP-PROP-0001-real-perl-editor-trust.md)
Linked spec: [PLSP-SPEC-0011](../../docs/specs/PLSP-SPEC-0011-trust-lane-ci-routing.md)
Current pointer: `docs/ci/pr-plan.md`
Blocks: cheap parser fixture lane, hosted CI exposure estimate
Blocked by: none
Receipt: [PR Plan trust-lane summary](../../docs/ci/pr-plan.md)
Supporting receipts:
[trust-lane policy](../../policy/trust-lanes.toml),
[PR Plan workflow](../../.github/workflows/pr-plan.yml),
[PR Plan classifier](../../scripts/ci/pr_plan.py),
[trust-lane validator](../../scripts/ci/validate_trust_lanes.py)

Claim boundary

CI routing may classify proof cost; it does not prove provider behavior or promote support claims.

Goal

Classify PRs by trust lane so CI proof follows the claim boundary instead of
defaulting to broad proof by habit.

Production delta

Add policy or PR summary support only after the routing contract lands. The
first implementation should be advisory and receipt-producing.

Non-goals

No broad full-CI default. No provider behavior proof from CI routing alone. No
support-tier promotion from routing alone.

Acceptance

PR summary or receipt names the trust-lane class, changed surface, required
proof, skipped-by-policy checks, hosted-CI estimate, and widening triggers.

Current implementation status

The advisory PR Plan summary reads `policy/trust-lanes.toml`, classifies
changed files, emits a `trust_lanes` block in `ci-plan.json`, and renders the
trust-lane class, required proof, skipped-by-policy checks, support claim
impact, and widening triggers in the step summary. This is receipt-producing CI
metadata only; it does not skip branch protection, prove provider behavior, or
promote support tiers.

Proof commands

```bash
rtk python scripts/ci/validate_trust_lanes.py --strict
rtk python scripts/ci/validate_risk_packs.py --strict
rtk cargo xtask check-support-claims
rtk git diff --check
```

Rollback

Revert the routing policy or summary PR. If classification is wrong, keep the
spec and disable only the classifier output until fixed.

## Work item: active-goal-manifest-validation

Status: completed
Linked proposal: [PLSP-PROP-0001](../../docs/proposals/PLSP-PROP-0001-real-perl-editor-trust.md)
Linked spec: [PLSP-SPEC-0015](../../docs/specs/PLSP-SPEC-0015-real-perl-editor-trust-v1-boundary.md)
Current pointer: `.perl-lsp/goals/README.md`
Blocks: agent handoff from repo artifacts
Blocked by: none
Receipt: [`rtk cargo xtask check-active-goal-manifest`](../../xtask/src/tasks/active_goal_manifest.rs)
Supporting receipts:
[active goals README](../../.perl-lsp/goals/README.md),
[Commands reference](../../docs/reference/COMMANDS_REFERENCE.md)

Claim boundary

Manifest validation proves reference integrity only; it does not prove lane completion, promote support tiers, refresh generated status, or validate provider behavior.

Goal

Make `.perl-lsp/goals/active.toml` checkable as the machine-readable handoff
artifact for the Editor Trust lane.

Production delta

Add an `xtask` validator and command-reference entry. The validator checks
manifest parsing, required fields, active-only top-level goal status,
trimmed top-level title/owner fields, stable slug goal/work-item IDs, duplicate work item IDs, repo-relative path
references that are non-empty and do not use surrounding whitespace, required objective/end-state/claim-boundary narrative fields, unique top-level
end-state and claim-boundary entries without surrounding whitespace, document-only
trimmed work-item claim-boundary/current-status/trigger/blocker prose,
top-level spec/ADR/status inventories without anchors or symbols,
document-only top-level proposal/plan/previous-goal pointers without anchors or
symbols,
non-empty unique proof command lists without surrounding whitespace, `rtk`-prefixed proof commands, known work-item statuses,
duplicate top-level path entries, top-level spec coverage for work-item specs,
top-level proposal/archive/status-pointer/objective/end-state/claim-boundary/spec/ADR/status-doc
discoverability from the active implementation plan, markdown anchors,
`path::symbol` receipt anchors, work-item spec document pointers without anchors
or symbols, work-item plan/current-state document pointers without
`path::symbol` anchors, primary status-pointer membership in
`status_docs`, required plan/current-state pointers, plan anchors that match work
item IDs, work-item plan paths that stay under the active manifest's top-level
plan, status-doc current pointers listed in top-level `status_docs`, work-item
current pointers and current statuses mirrored in the linked plan work-item
section, `trigger` and `blocked_by` routing context mirrored in the linked plan
work-item section, receipt fields mirrored in the linked plan work-item section,
work-item spec paths mentioned in the linked plan work-item section,
work-item statuses and claim boundaries mirrored in the linked plan work-item section, and
proof commands mirrored in the linked plan work-item section, required
`Claim boundary`, `Non-goals`, `Acceptance`, `Proof commands`, and `Rollback` section headings in each linked plan work item, and the
active-goal requirement that at least one work item remains
non-completed while the goal status is `active`. Planned work items must carry a
`trigger` or `current_status`, and blocked work items must carry `blocked_by`,
so parked work is not mistaken for an immediate next slice. Active and ready work
items must carry `current_status` so immediate handoffs explain why they are
executable now. Prose mirrors for objective, end state, claim boundary, and
`current_status`, `trigger`, and `blocked_by` are whitespace-normalized so normal Markdown wrapping does not
break the handoff. The validator reports actionable work item count separately from open work item count;
`active` and `ready` items are actionable, while `planned` and `blocked` items
remain open but parked. If an active manifest has open work but zero actionable
items, it must include a top-level `next_action` handoff so agents do not invent
the next implementation slice from stale chat context. When present,
`next_action` must be a non-empty string. At most one work item may have status
`active`; if any item is `active`, `current_work_item` must reference that item.
When both `next_action` and `current_work_item` are present, `next_action` must
name the exact current work-item ID so prose handoff text cannot drift from the
machine-readable pointer.

If the active manifest has any actionable item, it must include
`current_work_item`, and that value must match an `active` or `ready` work-item
ID and use the same stable slug-id format so the next executable slice is
machine-readable. The validator success
receipt prints the current work item alongside open/actionable counts, plus the
current work item's plan pointer, current-state pointer, current status, claim
boundary, and proof commands.

Non-goals

No support-tier promotion. No generated status refresh. No provider behavior
change. No claim that the active goal is complete. No attempt to require every
accepted spec in the repo to be listed by the current goal. No claim that a
receipt path proves behavior beyond the work item's stated claim boundary.

Acceptance

The active manifest validator passes on the checked-in active goal and rejects
missing spec paths, missing markdown anchors, missing symbol anchors, missing
proof command lists, empty proof commands, duplicate top-level spec paths,
whitespace-padded proof commands, duplicate proof commands, unprefixed proof commands,
non-slug goal/work-item IDs, duplicate work-item IDs, absolute path references, rooted path references,
parent-directory path references, empty path references, whitespace-padded path references, anchored or
symbolic top-level inventory paths, anchored or symbolic top-level
proposal/plan/previous-goal document paths, anchored or symbolic work-item spec
document paths, symbolic work-item plan/current-state document pointers,
unlisted work-item specs, unknown work-item statuses, and active manifests with no open work item, missing
plan/current-state pointers, missing objective, empty end-state/claim-boundary
entries, duplicate or whitespace-padded end-state/claim-boundary entries,
whitespace-padded top-level title/owner fields,
whitespace-padded work-item claim-boundary/current-status/trigger/blocker prose,
malformed or invalid `created` dates, a `status_pointer` absent from
`status_docs`, top-level proposal/archive/status-pointer/objective/end-state/claim-boundary/spec/ADR/status-doc
references missing from the implementation plan, planned work items without
routing context, blocked work items without blocker context, active manifests
with no actionable item and no `next_action`, empty `next_action` fields,
`next_action` text that omits `current_work_item`, work-item `current_status`
prose missing from the linked plan section, non-active top-level goal status,
`trigger` or `blocked_by` prose
missing from the linked plan section, receipt paths missing from the linked plan
section, active or ready work items without
`current_status`, completed work items without receipt fields,
actionable active manifests without `current_work_item`,
non-slug `current_work_item` values, `current_work_item` values that do not reference an active/ready work item, and
`current_work_item` values that ignore an active work item, multiple active work
items, and plan anchors that do not match work-item IDs in focused tests. CLI
smoke coverage proves the command is registered in `list-commands`, callable
through the `xtask` binary, reports actionable work item count, and prints the
current work item with its plan pointer, current-state pointer, current status,
claim boundary, and proof commands.

Proof commands

```bash
rtk cargo test -p xtask active_goal_manifest --profile agent --locked -- --nocapture --test-threads=1
rtk cargo test -p xtask --test active_goal_manifest_cli --profile agent --locked -- --nocapture --test-threads=1
rtk cargo test -p xtask --test list_commands_cli --profile agent --locked -- --nocapture
rtk cargo xtask check-active-goal-manifest
rtk cargo xtask check-support-claims
rtk cargo xtask check-provider-confidence-matrix
rtk git diff --check
```

Rollback

Revert the validator and docs entries. If manifest validation proves too strict,
disable only the new command while preserving the active goal manifest.

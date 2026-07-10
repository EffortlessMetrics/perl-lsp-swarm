# Spec-Governance Implementation Plan

Status: planned
Owner: perl-lsp maintainers
Linked proposal: [PLSP-PROP-0003](../../docs/proposals/PLSP-PROP-0003-spec-governance.md)
Linked spec: none yet (S2/S3 will add a `spec-system` contract spec once the
markdown-adapter and native-bundle shapes are settled)
Goal manifest: none yet — deferred to S1 (see "Cutover/rollback" below for why)
Tracker: [#3586](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/3586)

## Purpose

Define the phase train that migrates structural validation of `perl-lsp`'s
spec system (identity, typed links, lifecycle, ownership, closeout edges) onto
`cargo-allow`'s opt-in `spec-system` profile, while keeping content authoring
and repo-specific semantics exactly where they already work. This plan is a
routing map, not a product-claim document: S0 (this PR) plans the migration
and does not execute it.

## Grounded Tool Facts (verified 2026-07-10, this worktree)

- `cargo-allow` v0.1.10 is installed (`cargo allow --version`). Its default
  job is the source-exception ledger (`policy/allow.toml`); it also ships an
  opt-in `spec-system` profile: `cargo-allow check --profile spec-system`
  (modes: audit/no-new/strict/release; formats include json/sarif/markdown;
  writes receipts). `init`/`doctor`/`worklist`/`explain` all accept
  `--profile spec-system`.
- `cargo-allow doctor --profile spec-system` currently reports `Ready: false`
  with `.allow/profiles/spec-system.toml` missing (config provenance =
  `built_in_default`). That config file is created at S1, not in this PR.
- `cargo-allow check --profile spec-system --mode audit` runs today against
  built-in default roots: 0 artifacts, 0 links, 2 findings (1 blocking-eligible
  — `doc_artifact_ledger_missing` — and 1 advisory — missing
  `docs/status/SUPPORT_TIERS.md`).
- The exact `.allow/profiles/spec-system.toml` shape must come from
  `cargo-allow init --profile spec-system --dry-run` at S1 time. This plan
  does not hardcode an assumed shape — the dry-run output is the source of
  truth when S1 opens.
- See the doctor/check raw output appendix at the bottom of this plan.

## Objective

`cargo-allow`'s `spec-system` profile becomes the canonical **structural**
graph validator for the spec system: identity, typed links, lifecycle,
ownership, and closeout edges. `spec-planner`/`spec-builder.js` keep
**authoring** content (hazards, acceptance criteria, API shape, test grid,
blast radius). `xtask` keeps owning **repo-specific semantics** (lane IDs, WIP
caps, permitted-lane rules, hazard-class completeness). CI executes proof
(tests, clippy, RIPR, corpus, receipts) exactly as it does today.

## Responsibility Split

| Concern | Owner | Notes |
|---|---|---|
| Problem analysis, hazards, API-shape, test-grid, blast-radius | Claude spec-planner + `spec-builder.js` | Unchanged — content authoring stays repo-side |
| Stable IDs, kinds, statuses, ownership, typed graph links | `cargo-allow` `spec-system` profile | New responsibility, phased in S1-S3 |
| Lane IDs, WIP caps, permitted-lane rules | Existing `xtask` checks | Unchanged — repo-specific semantics stay in xtask |
| Test execution, clippy, RIPR, corpus, receipts | `cargo`/`xtask`/CI | Unchanged — `cargo-allow` never executes proof |
| Live PR/issue state | GitHub | Unchanged — `cargo-allow` never queries GitHub |
| "What landed" | Closeout artifact + merge/reachability receipt | New artifact type added in S5 |

## What Does NOT Migrate (stays repo-specific)

- All six `acceptance.md` sections (Behavior, Hazards, Contracts, API-Shape,
  Test-Grid, Blast-Radius)
- Hazard-class completeness checking
- LSP/DAP/parser contract reasoning
- API duplicate/caller searches
- Lane IDs and WIP caps
- RTK-prefix command policy
- Tests-exist-and-pass verification
- Spec/test/code semantic three-way agreement (owned by `spec-test-code-match`)
- Live GitHub state (PR status, CI checks, labels)

`cargo-allow` does not lint prose and does not execute proof. Its job is
structural: does the graph parse, are IDs unique, do links resolve, are
lifecycle transitions valid.

## Phase Train S0-S6

S1-S6 are **explicitly deferred** until #3579 lands / a separate WIP slot
opens. (Note: #3579 — `refactor(lsp): centralize parsed state in
generation-tagged ParsedSnapshot` — merged to `main` as `b30fa1831` before
this plan was written; the landing condition is satisfied, but S1 still
requires its own dedicated PR/WIP slot rather than riding on this one.)

### S0 — Migration contract (this PR)

Goal: record the phase train, responsibility split, and governance clause as
a durable, planned reliability train.

Stop condition: this plan, the linked proposal, and tracker #3586 are merged
and cross-linked. No code, `.allow/` config, or `.perl-lsp/goals/` manifest
changes ship. No generated status is altered.

### S1 — Advisory bootstrap

Goal: run `cargo-allow init --profile spec-system --dry-run`, then create
`.allow/profiles/spec-system.toml` (the modern `.allow/` namespace, not the
legacy `policy/*.toml` used by the source-exception ledger). Register a
**small** first graph: Real Perl Editor Trust proposal/spec/plan, the
compiler-program plan + goal manifest, Fresh Facts Fast plan + goal manifest,
the support-tier surface, and the source-exception policy ledger. Add an
advisory CI job separate from the existing source-exception check.

Stop condition: `cargo-allow doctor --profile spec-system` reports
`Ready: true` for the registered roots; the advisory CI job runs and reports
findings without blocking merge; no more than the named small graph is
registered.

Explicit non-goal: do NOT hand-register the whole historical `.spec` tree.

### S2 — Generic markdown adapter improvements (upstream)

Goal: improve `cargo-allow`'s generic markdown adapter upstream
(`EffortlessMetrics/cargo-allow`) so that: imported markdown with front-matter
uses the declared `id`/`kind` over path-derived identity; `linked_*`
front-matter normalizes into typed edges; explicit identity gets
high-confidence scoring; imports are never rewritten; duplicate IDs are
diagnosed. Ship and pin the release before any native cutover in S3.

Stop condition: the upstream release is published, pinned in this repo's
`Cargo.lock`/toolchain manifest, and the S1 registered graph re-validates
cleanly under the new adapter version.

### S3 — Native bundles for new `.spec` work

Goal: new `.spec` bundles become `cargo-allow`-native: `context.md` ->
`proposal`, `acceptance.md` -> `spec`, `checklist.md` -> `implementation_plan`,
add `closeout.md` -> `closeout`. Globally-unique IDs follow the pattern
`PLSP-SWARM-{PROP,SPEC,PLAN,CLOSEOUT}-<issue>-<run>`. Update
`docs/reference/SPEC_TEMPLATE.md`, `docs/reference/SPEC_UPDATE_CHECKLIST.md`,
the `spec-planner` agent + commands, `spec-builder.js`, the
builder/red-tdd prompts, and `spec-test-code-match` to know about the new
`closeout.md` file and the native ID scheme.

Stop condition: a newly created `.spec/<issue#>-<slug>/` bundle round-trips
through `cargo-allow check --profile spec-system` with zero structural
findings, and all named docs/agents/commands are updated to reference the
native shape.

Explicit non-goal: this phase does not touch the historical `.spec` tree
(see "Legacy `.spec` posture" below).

### S4 — Fresh Facts Fast as first native dogfood graph

Goal: Fresh Facts Fast (proposal -> spec -> plan -> goal -> phase closeouts)
becomes the first fully `cargo-allow`-native graph. Only after all linked
artifacts are registered and `cargo-allow explain` shows the intended graph
with no unknown-link findings does this phase set
`active_goal_required = true`.

Stop condition: `cargo-allow explain --profile spec-system` renders the
complete Fresh Facts Fast graph (proposal through closeouts) with zero
unknown-link findings; `active_goal_required = true` is set only after that
check passes.

Explicit non-goal: do NOT move `.perl-lsp/goals` to `.allow/goals` in the same
PR as this dogfood cutover — that is a separate, later migration once the
dogfood graph has run clean for a full lane cycle.

### S5 — Closeouts become part of "done"

Goal: a `closeout.md` at merge time (merged SHA, production entrypoint,
tests/receipts, reachable behavior, fallback remaining, deferred work,
rollback, uncertainty) becomes a required artifact. A work item is `done`
only once its closeout exists and is linked.

Stop condition: at least one full lane (proposal through merge) produces a
closeout that `cargo-allow` links back to its spec/plan without a manual
patch; the closeout fields above are populated from real merge data, not
placeholders.

### S6 — Shadow, then narrowly block

Goal: promote **only** objective structural findings to blocking: duplicate
IDs, missing artifact files, invalid kind/status, a registered file missing
its ID, unknown linked IDs, and malformed profile/ledger config. Everything
else (stale goals, missing closeouts, support-tier completeness, README/
release coverage) stays advisory for longer.

Stop condition: the advisory CI job has run clean (or with only advisory
findings) across at least the S4 dogfood graph for a full lane cycle before
any finding class is promoted to blocking; promotion is done finding-class by
finding-class, not as a single flag flip.

## Legacy `.spec` Posture

The historical `.spec` tree is **read-only import**. Backfill only:

- Currently-active specs (an open PR or in-flight build references them)
- Specs linked by an active goal manifest
- Frequently-referenced specs (cited by more than one other durable artifact)
- Historical specs already being materially edited for an unrelated reason

No mass rewrite of the old tree is in scope at any phase of this train.

## Governance Clause (long-running operating goal)

> `cargo-allow`'s `spec-system` profile is the canonical structural graph
> validator for the perl-lsp spec system. Claude still authors artifacts. At
> session start, run `doctor`/`check`/`worklist` for the `spec-system`
> profile. New governed artifacts carry stable IDs, kinds, owners, statuses,
> and typed links. A work item is not consolidated until its closeout is
> linked. `cargo-allow` findings never substitute for proof execution or live
> GitHub state. Legacy `.spec` bundles stay read-only until touched.

## Cutover/Rollback

Promotion order: **advisory -> shadow -> narrow-block**, one finding class at
a time (see S6). Rollback at any phase: revert the `.allow/profiles/
spec-system.toml` config change, or drop the CI job back to advisory mode.
No phase depends on an irreversible data migration — the graph is derived
from markdown front-matter, not a separate database, so reverting the profile
config is sufficient to roll back validation without touching the underlying
`.spec`/`docs/` content.

## Reserved IDs

- Proposal: `PLSP-PROP-0003` (allocated by this PR — next free ID after
  `PLSP-PROP-0002`; see `docs/proposals/PLSP-PROP-0003-spec-governance.md`)
- Spec: `PLSP-SPEC-0036` is reserved for the future `spec-system` contract
  spec (next free ID after `PLSP-SPEC-0035`; not created in this PR — S2/S3
  will author it once the adapter and native-bundle shapes are settled)

## Appendix: `cargo-allow doctor --profile spec-system` (read-only, this worktree)

```
# cargo-allow doctor --profile spec-system

**Result:** advisory
Mode: `advisory`
Status: `passed`
Profile: `spec-system`
Config: `default spec-system roots`
Config provenance: `built_in_default`

## Setup Readiness

Mode: `advisory`
Ready: `false`

| Check | Status | Path | Message |
|---|---|---|---|
| `profile_config` | `missing` | `.allow/profiles/spec-system.toml` | spec-system profile config does not exist |
| `artifact_root` | `ready` | `docs/proposals` | artifact root docs/proposals exists |
| `artifact_root` | `ready` | `docs/specs` | artifact root docs/specs exists |
| `artifact_root` | `ready` | `docs/adr` | artifact root docs/adr exists |
| `artifact_root` | `ready` | `plans` | artifact root plans exists |
| `artifact_root` | `missing` | `.codex/goals` | artifact root .codex/goals is missing |
| `artifact_ledger` | `missing` | `policy/doc-artifacts.toml` | failed to read doc artifact ledger (os error 2) |
| `support_tiers` | `missing` | `docs/status/SUPPORT_TIERS.md` | failed to read support-tier file (os error 3) |
| `active_goal` | `missing` | `.codex/goals/active.toml` | cannot be validated until doc artifact ledger parses |
| `templates` | `missing` | `docs/templates` | missing spec-system templates (proposal.md, spec.md, adr.md, implementation-plan.md, plan-item.md, closeout.md, pr-body.md) |
| `federation_ledgers` | `ready` | `.allow/config.toml` | federation ledger registry is not configured |

| Metric | Count |
|---|---:|
| Artifacts | 0 |
| Links | 0 |
| Support-tier rows | 0 |
| Findings | 2 |
| Blocking-eligible findings | 1 |
| Advisory findings | 1 |
```

This appendix is read-only evidence captured 2026-07-10 in this worktree; it
is not a claim that any of these findings are fixed or fixable by this PR.

## Work Item: spec-governance-s0-migration-contract

Status: active (this PR)
Lane: control-plane
Linked proposal: [PLSP-PROP-0003](../../docs/proposals/PLSP-PROP-0003-spec-governance.md)
Blocks: S1 (advisory bootstrap)
Blocked by: none (the #3579 timing note above is informational, not a hard
dependency of S0 itself)

Goal

Author the migration contract (proposal + this plan + tracker) so S1 can open
in its own PR/WIP slot without re-deriving the phase train from chat history.

Production delta

Docs only: two new files (`docs/proposals/PLSP-PROP-0003-spec-governance.md`,
`plans/spec-governance/implementation-plan.md`), one README index update
(`docs/proposals/README.md`), one tracking issue (#3586). No code, no
`.allow/` config, no `.perl-lsp/goals/` manifest, no generated status update,
no provider behavior change.

Non-goals

No `.allow/profiles/spec-system.toml`, no registered spec-graph artifacts, no
CI job, no native bundle mapping, no closeout template, no goal manifest.

Acceptance

Proposal and plan exist, are cross-linked, and reference tracker #3586. Proof
commands pass without workspace errors.

Proof commands

```bash
git diff --check
cargo xtask ci-hygiene check-doc-paths docs/proposals
cargo xtask ci-hygiene check-doc-paths plans/spec-governance
```

Rollback

Revert this PR. No other artifact depends on it yet — S1 has not opened.

## Future Work Items (not yet open)

- `spec-governance-s1-advisory-bootstrap` — `.allow/profiles/spec-system.toml`
  + small first graph + advisory CI job
- `spec-governance-s2-adapter-upstream` — generic markdown adapter
  improvements in `cargo-allow` upstream
- `spec-governance-s3-native-bundles` — native `.spec` bundle mapping +
  tooling updates
- `spec-governance-s4-fresh-facts-fast-dogfood` — Fresh Facts Fast native
  graph + `active_goal_required` cutover
- `spec-governance-s5-closeout-required` — closeout artifact required for
  "done"
- `spec-governance-s6-narrow-block` — shadow-then-block promotion of
  objective structural findings

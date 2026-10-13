# Context: #11470 — standalone owned-state manifests and safe removal plans

## Claim authority: #11470 is closed; canonical leaf is #11527

Issue #11470 was closed `not_planned` on 2026-08-31 as a duplicate, under the
programme's integrity ruling, of **#11527** (same title, same contract claim;
parent/controller #8372). This packet therefore **advances #11527**; it cannot
close #11470. The `.spec/11470-*` directory name is the historical claim
number this contract was authored against and is retained for provenance.

## Problem

Issue #11470/#11527 (uninstall programme #8372/#10703) requires one versioned,
closed installer-owned-state manifest (`standalone_owned_state.v1`), a pure
removal plan/result contract, running/unknown-state rules, the PATH ownership
relationship to #11467/#11468/#11469, and deterministic fixtures for
standalone uninstall. The claim deletes nothing and mutates no
PATH/profile/registry/current selection; removal-execution successors own
execution.

## Status: contract packet + checked validator this lane; live cut gated

Re-verified live on 2026-09-17 against `origin/main@9b7d333170e5` (current
head at repair time) and live GitHub state:

| Sibling | Role for this contract | State when verified (2026-09-17) |
|---|---|---|
| #11470 | original claim number | **closed `not_planned`** — duplicate of #11527 |
| #11527 | canonical leaf (same title) | **open**, acceptance unchecked; this PR advances it |
| #8372 | parent uninstall controller | **open** |
| #10703 | distribution train programme | **open** |
| #11869 | umbrella | **open** |
| #11179 | immutable candidate/current-selection records | **closed `completed`** (PR #12213 merged; `standalone_candidate.v1` + `standalone_current_selection.v1` landed on main) |
| #11425–#11430 | POSIX/Windows mutation ownership, candidates, health+rollback | **open** (all six) |
| #11467/#11468/#11469 | PATH persistence semantics and implementations | **closed `not_planned`** (consolidated by the programme; PATH marker ownership semantics remain represented by the marker roles here) |
| #11417 | conditional activation gate (`not_applicable` requires it) | **open** |
| #11471/#11472 | removal executors (original numbering) | **closed `not_planned`** (consolidated; executor successors are tracked under #11527's plan) |

Searches performed at repair time: the contract trio
(`standalone_owned_state`, `standalone_removal_plan`,
`standalone_uninstall_result`) exists on no other branch PR and nowhere on
`origin/main`; sibling standalone schemas
(`standalone_candidate.v1`, `standalone_current_selection.v1`,
`standalone_install_transition.v1`, `standalone_source_build.v1`) landed
around it without absorbing this claim. No rival candidate exists.

Unlike the deferred-cut shape of `.spec/11661-*`, the CONTRACT itself has no
unlanded prerequisite: it is pure document validation over closed vocabularies
the issue itself fixes (roles, classes, retention, plan fields, result words).
The manifest references current/previous candidate identities by digest and
path only; it does not define the selection model (#11179's landed
`standalone_candidate.v1`/`standalone_current_selection.v1` packets), and
marker rows reference PATH ownership semantics without implementing them
(successor claims under #11527). Validation therefore lands now as
executable, discriminating proof; production scanning/removal binds later.

## Current-main facts the future builder consumes (`main@cce85d167` at first verification; shapes unchanged at `main@9b7d333170e5`)

### What the standalone installer writes today (no owned-state manifest)

- `scripts/install.sh:460-489` copies `perllsp` (+`perl-dap` when present)
  into `INSTALL_DIR` (default `/usr/local/bin`, else `$HOME/.local/bin`,
  Termux path override) and nothing else persists.
- `scripts/install.sh:507-523` only WARNS when `INSTALL_DIR` is not on PATH;
  it owns no PATH marker, profile line, or receipt. There is no uninstall.
- `install.sh:65-118` is an identity-bound remote bootstrap into
  `scripts/install.sh`; it fetches by full commit SHA and verifies sha256.
- Consequence encoded in this contract: "missing manifest is not clean
  absence" — a host installed before manifests exist yields no
  `already_absent_owned_state` result without complete evidence.

### House patterns consumed (no new envelope invented)

- Versioned JSON Schema contracts live in `schemas/*.v1.schema.json`
  (e.g. `schemas/install_transition.v1.schema.json`).
- Checked validators live as xtask example binaries with typed
  `deny_unknown_fields` structs, declared-vs-computed status agreement, and
  focused unit tests over committed fixtures
  (`xtask/examples/install_transition.rs`, owner issue recorded in-source;
  fixtures under `fixtures/experience/install_transition/`).
- Fail-closed doctrine: unverifiable fields stay absent/typed-not-proven,
  never plausible facts (session-receipt doctrine applied since PR #3866);
  unknown schema variants fail visibly.
- New non-Rust files require allowlist registration
  (`policy/non-rust-allowlist.toml`) plus regenerated inventory
  (`docs/policy/NON_RUST_INVENTORY.md`); newly added unclassified files block
  (`cargo xtask check-file-policy`).

## Why this approach

The issue's acceptance list demands one exact manifest/plan that
distinguishes owned, foreign, unknown, running, retained, and removable
state, with destructive removal bound to exact currentness. Prose alone
cannot prove totality of classification, plan-totality over manifest rows,
or ambiguity-free running-state handling. A checked validator with a
deterministic fixture set makes every negative control in the issue an
executable falsifier while staying inside "pure ownership/plan/result
contracts only".

## Alternatives rejected

- **Prose-only `.spec` packet (11661 style)**: rejected — unlike EXE-06,
  this claim's contract is self-contained data validation; deferring the
  validator would ship an unproven vocabulary that #11471/#11472 must trust
  blind.
- **Implementing the scanner or remover**: rejected — enumeration and
  deletion are owned by #11425–#11430 and #11471/#11472; any filesystem walk
  here would create a rival mutation seam.
- **Defining candidate selection semantics**: rejected — #11179 owns
  current-selection records; this manifest carries identities by digest/path
  only and marks cross-lane fields as references.
- **Widening roles/classes beyond the issue's closed lists**: rejected — the
  issue fixes the class vocabulary (eight values) and result vocabulary
  (eleven values); latitude exists only in role names, which this packet
  freezes so successors implement against one spelling.

## Links

- Original claim: [#11470](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/11470) — closed `not_planned` (duplicate)
- Canonical leaf: [#11527](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/11527) — this PR advances it
- Parent controller: #8372; programme: #10703; umbrella: #11869
- State siblings: #11179 (landed), #11425–#11430 (open)
- PATH ownership: #11467 / #11468 / #11469 (closed, consolidated)
- Activation gate: #11417; filesystem hardening: #10755
- Removal executors: successor claims under #11527 (original #11471/#11472 closed, consolidated)
- Hosted removal proof lineage: #11144 / #11149 / #11156

## Scope boundary

In scope: `.spec/11470-standalone-owned-state-manifest/*`,
`schemas/standalone_owned_state.v1.schema.json`,
`schemas/standalone_removal_plan.v1.schema.json`,
`schemas/standalone_uninstall_result.v1.schema.json`,
`fixtures/experience/install_owned_state/*`,
`xtask/examples/standalone_owned_state.rs`, non-Rust policy registration for
those files.

Out of scope: any uninstall execution (#11471/#11472), PATH/profile/registry
mutation (#11467–#11469), activation logic (#11417), scanner/enumeration
implementation (#11425–#11430), install-surface registry changes (#9104), and
every caller migration.

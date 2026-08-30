# Context: #11470 — standalone owned-state manifests and safe removal plans

## Problem

Issue #11470 (uninstall programme #8372/#10703) requires one versioned,
closed installer-owned-state manifest (`standalone_owned_state.v1`), a pure
removal plan/result contract, running/unknown-state rules, the PATH ownership
relationship to #11467/#11468/#11469, and deterministic fixtures for
standalone uninstall. The claim deletes nothing and mutates no
PATH/profile/registry/current selection; platform successors (#11471/#11472)
own execution.

## Status: contract packet + checked validator this lane; live cut gated

Verified live on 2026-08-24 against `origin/main@cce85d167` (current head at
lane start) and live GitHub state:

| Sibling | Role for #11470 | State when verified | Open PR claiming it |
|---|---|---|---|
| #8372 | parent uninstall controller | **open** | none |
| #10703 | distribution train programme | **open** | none |
| #11179 | immutable candidate/current-selection records | **open** | none |
| #11425–#11430 | POSIX/Windows mutation ownership, candidates, health+rollback | **open** (all six) | none |
| #11467/#11468/#11469 | PATH persistence semantics and implementations | **open** (all three) | none |
| #11417 | conditional activation gate (`not_applicable` requires it) | **open** | none |
| #11471/#11472 | removal executors consuming this contract | **open** | none |

Searches performed: `gh pr list --state all` (incl. drafts) contains no PR
referencing #11470 or standalone owned-state manifests; repository-wide
searches find no `standalone_owned_state` symbol or schema. No rival
candidate exists.

Unlike the deferred-cut shape of `.spec/11661-*`, the CONTRACT itself has no
unlanded prerequisite: it is pure document validation over closed vocabularies
the issue itself fixes (roles, classes, retention, plan fields, result words).
The manifest references current/previous candidate identities by digest and
path only; it does not define the selection model (#11179's owned claim), and
marker rows reference PATH ownership semantics without implementing them
(#11467–#11469). Validation therefore lands now as executable, discriminating
proof; production scanning/removal binds later.

## Current-main facts the future builder consumes (`main@cce85d167`)

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

- Issue: [#11470](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/11470)
- Parent controller: #8372; programme: #10703; umbrella: #11869
- State siblings: #11179, #11425–#11430
- PATH ownership: #11467 / #11468 / #11469
- Activation gate: #11417; filesystem hardening: #10755
- Removal executors: #11471 (POSIX), #11472 (Windows)
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

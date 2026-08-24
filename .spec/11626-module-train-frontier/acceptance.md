# Acceptance: #11626 slice one — offline status + safe frontier from module_train.v1

## §Behavior

- `cargo xtask module-train status --tree HEAD` prints a binding block (exact
  `HEAD` SHA, worktree dirty-path count, manifest committed/dirty state,
  schema, computed canonical digest, pinned digest, C01 semantic-SHA
  provenance) followed by one deterministic row per node: id, issue, role,
  lane, typed state, implementation presence, sorted typed reasons.
- `cargo xtask module-train next --tree HEAD` prints the same binding block
  plus every hard-ready leaf with writer class and conflict key, per-class
  groupings, visible limitations, and the ceilings-not-quotas law line.
- `--tree` accepts only `HEAD`; anything else fails closed.
- No network, no GitHub, no mutation, no scheduling, no product behavior.

## §Hazards

- Hardcoding the frontier instead of deriving it from manifest data.
- Manifest tampering or an un-re-derived semantic revision projecting a
  frontier silently.
- Evidence-class dependencies silently becoming hard blockers (false total
  order), or controllers gating builders directly (manifest law forbids).
- Implementation presence guessed from issue closure, file existence, or
  names (`not_proven` by law instead).
- Writer classes filling as quotas; conflict keys treated as reservations.
- Non-deterministic bytes (timestamps, map iteration order, ambient paths).
- Binding-pending obligations disappearing when a hard block dominates.

## §Contracts

- States: `landed_current_tree | ready | blocked_hard | blocked_evidence |
  blocked_external_or_authorization | incomplete_current_tree | superseded |
  not_proven` (`incomplete_current_tree` and `superseded` unreachable in
  this slice; `not_proven` is reachable for role-rejected non-buildable
  nodes; populated supersessions fail closed).
- Hard-dep satisfaction: landed node, controller (topology-satisfied per
  manifest `limitations[1]`), or — for cross-programme authorities — honestly
  unestablishable offline (typed reason, still a hard block).
- Typed visibility: `evidence_dep_not_current:*`, `optional_dep_not_current:*`,
  `case_work_packet_binding:<status>`, `external_authorization_not_granted:*`,
  `role_never_implementation_start:*`, `hard_dep_not_landed:*`,
  `hard_dep_cross_programme_state_not_establishable:*`.
- Digest pin: `PINNED_CANONICAL_DIGEST` (ordinal canonical walk). Any byte
  change in the manifest fails loading.

## §API-Shape

```text
cargo xtask module-train status --tree HEAD
cargo xtask module-train next    --tree HEAD
```

New module: `xtask/src/tasks/module_train.rs` (+ `module_train_tests.rs`).
CLI enum: `ModuleTrainCommand` in `xtask/src/main.rs`. No library surface.

## §Test-Grid

| # | falsifier (from #11626 list, slice-mapped) | rejected by |
|---|---|---|
| 1 | issue closure/source presence establishes landed state | only the C01 manifest probe lands a node; everything else `not_proven` |
| 2 | tampered manifest bytes still load | pinned canonical digest fails loudly (`digest drift`) |
| 3 | tampered edge still validates | successor identity, unknown target, duplicate edge, self-dep, cycle, class-agreement laws |
| 4 | tampered identity still validates | title fingerprint, duplicate issue/conflict-key/authority-after laws |
| 5 | controller/gate appears in `next` | role rejection (`CTRL`, `P11F` rows) |
| 6 | controller edge gates a builder | controller-satisfaction test stays `ready` |
| 7 | evidence dep becomes a hard blocker | M01 stays ready with visible E00A/E00B limitations; class-collapse mutation flips it to `blocked_hard` (proof the class matters) |
| 8 | fan-in/retirement starts early | P11F role-rejected; L09G blocked on all six cutovers |
| 9 | binding-pending hidden by hard block | M00S carries `case_work_packet_binding:structurally_pending` alongside hard blocks |
| 10 | frontier hardcoded, not derived | landing-by-data and retarget mutations move the frontier through the same code path |
| 11 | insertion order moves bytes | reversed arrays: identical digest, identical projection |
| 12 | two runs differ | byte-identical `status`/`next` renders (tests + live CLI) |
| 13 | non-HEAD tree accepted | `--tree origin/main` fails closed |
| 14 | supersession guessed | populated supersessions list bails |

## §Blast-Radius

`xtask` only: one new task module, one `mod` declaration, CLI enum + dispatch
entries, and this `.spec` bundle. No product crates, no CI workflows, no
generated artifacts, no GitHub state, no changes to the C01 manifest.

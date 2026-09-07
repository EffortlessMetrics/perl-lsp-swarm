# Acceptance: #11626 — offline status + safe frontier from module_train.v1

Slice one landed the fail-closed loader, `status`, and `next`. The current
slice adds residual 1's first step: semantic current-tree implementation
probes for the train interface itself (C02, then C03), so the train stops
being blind to its own landed work. Probes for the E00/M/L09/P11 families,
the `explain`/`graph` static packet, arbitrary-tree/JSON binding, and the
supersession projection remain open residuals.

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
  not_proven` (`superseded` remains unreachable — populated supersessions
  fail closed; `incomplete_current_tree` is produced by a semantic probe
  whose declared components are partly met; `not_proven` is reachable for
  role-rejected non-buildable nodes and for every unprobed node).
- Probe outcomes: `probe:pass | probe:partial | probe:absent | not_proven`.
  `probe:absent` means a probe ran and found no declared component, so the
  node still falls through to dependency typing and may be `ready`.
  `not_proven` means no probe is defined for that node at all.
- A probe component is met only when its implementing surface **and** its
  production consumer are both present, so an orphaned module or an unwired
  command can never read as landed (#11626 falsifier 2).
- Only `probe:pass` satisfies a dependent's hard edge: a partially
  implemented node is deliberately not landed for its dependents.
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
| 15 | an implementation present but never dispatched reads as landed | `an_implementation_without_its_production_consumer_is_not_landed` (C03 stays `probe:partial` when its CLI dispatch is removed) |
| 16 | a consumer whose implementation is gone reads as landed | `a_consumer_without_its_implementation_is_not_landed` (`probe:absent`) |
| 17 | a neighbouring node's surface satisfies this node's component | `the_live_explain_cannot_satisfy_the_offline_static_packet` (C03's live `explain` must not close C02's offline packet residual) |
| 18 | a partial implementation counts as landed | `landing_the_residual_component_lands_c02` + `a_partial_node_does_not_satisfy_a_hard_dependent` |
| 19 | adding a probe turns an unbuilt node into a blocked one | `a_wholly_absent_probed_node_still_reports_through_dependencies` |
| 20 | a negative fixture silently stops falsifying | `a_fixture_that_cannot_falsify_is_rejected` (removing an absent anchor is an error) |
| 21 | a selector is satisfied by its own declaration | `no_selector_targets_the_registry_that_declares_it` — `PROBED_NODES` lives in `module_train_probes.rs` and no selector may target that file, so anchor literals can never be the text a selector matches |
| 22 | a rename silently demotes a landed node to partial | `only_the_recorded_residual_anchors_are_missing_on_the_real_tree` (only C02's recorded residual anchors may be absent) |
| 23 | a partial node hides its remaining edges | `a_partial_node_still_records_its_dependency_reasons` (dependency typing runs for every non-landed node; presence decides only the state) |

Per-component negative controls exist for all five components:
`c02_current_tree_probes_component_fails_without_its_projection`,
`c02_offline_frontier_component_fails_without_its_renderer`,
`the_live_explain_cannot_satisfy_the_offline_static_packet`,
`an_implementation_without_its_production_consumer_is_not_landed`, and
`a_consumer_without_its_implementation_is_not_landed`.

## §Blast-Radius

`xtask` only: one new task module, one `mod` declaration, CLI enum + dispatch
entries, and this `.spec` bundle. No product crates, no CI workflows, no
generated artifacts, no GitHub state, no changes to the C01 manifest.

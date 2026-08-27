# clippy::map_err_ignore boundary classification slice

## Issue

[#11246](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/11246)
(parent controller #9850; restriction train #11337; adoption law #11335; packet compiler #11257)

## Scope

Observe `clippy::map_err_ignore` across the current required product subjects on the pinned
product toolchain (rust-toolchain.toml channel **1.95.0**, Windows x86_64 MSVC host, workspace
default features), classify every exact finding against the repository's error / trust /
protocol authorities, and select the widest honest deny plan under the issue's priority order.
This slice activates no lint, changes no production error model, and adds no source cleanup.

Artifacts:

- `denominator.csv` — all 295 exact current findings with per-row boundary class,
  disposition, causal-identity bearing, sensitivity, activation cohort, and note.
  Regenerated from raw census JSONL; every row cites file:line:col against the tree this
  slice landed from.
- `activation-plan.md` — the selected plan (C converging to A), cohorts, owners,
  currentness conditions, filed leaves, and NOT_PROVEN platform rows.

## Claim boundary

This covers classification and disposition planning only. It does not activate any lint level
in Cargo, mutate `policy/clippy-lints.d` states, create debt rows (`clippy-debt.toml` requires
a `debt` Cargo state that would itself be an activation), rewrite public error models, change
protocol behavior, or decide hosted-platform subjects (#11225 owns those). The three
independent error-model defects found are routed to separately filed leaves, not repaired here.

## Observation identity

```text
instrument      cargo clippy --message-format=json --keep-going
isolation       -A clippy::all -W clippy::map_err_ignore plus name-level -A for every
                workspace-denied restriction lint (unwrap_used, expect_used, panic, todo,
                unimplemented, dbg_macro, await_holding_lock, await_holding_refcell_ref,
                print_stderr, print_stdout, manual_take, disallowed_fields) so no sibling
                denial masks findings out of a compiling unit (#11736 census method)
subjects        --workspace --lib; --workspace --bins --no-deps;
                whole-workspace --tests --benches --examples in three disjoint shards
toolchain       1.95.0 (clippy restriction lint map_err_ignore present; unknown-lint count 0)
host            Windows x86_64 MSVC, default features, CARGO_INCREMENTAL=0
denominator     295 rows = 58 production (lib/bins, non-cfg(test))
                + 237 test-context (cfg(test), tests/, benches/, examples/)
                = 294 physical sites (spans normalized before dedup) + 1 deliberate
                lossy-shape fixture row introduced by this slice's contrast control
                (map_err_boundary_contract.rs:94, cohort CTRL)
```

Span normalization note: raw diagnostics can cite `xtask\src\..\tests\support\...` forms;
duplicate rows for one physical site are removed after normalization. The contrast fixture's
own finding is accounted for rather than hidden: it is the intentional dishonest shape of the
retain-cause control and carries `disposition=exact_exception` with a removal condition.

Known masking hazard handled: `-A clippy::all` does not cover restriction lints, and cargo
appends `[workspace.lints]` denies after user flags; without name-level allows, units such as
`perl-dap (lib test)` abort compilation and lose their findings entirely (measured: 467
sibling errors masking units before the corrected instrument).

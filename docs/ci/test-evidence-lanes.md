# Test Evidence Lanes

Defines the **evidence-lane shapes** that perl-lsp's CI organises tests
into: which kinds of tests run on which trigger, with what bound, and
why. The point is to make the cost / signal trade-off explicit so a
contributor can predict which lane fires for their change without
reading every workflow YAML.

This doc pairs with:

- [`ci-lane-map.md`](ci-lane-map.md) — the per-workflow inventory.
- [`../../policy/ci-lanes.toml`](../../policy/ci-lanes.toml) — the
  machine-readable lane policy.
- [`../../policy/ci-risk-packs.toml`](../../policy/ci-risk-packs.toml)
  — path-pattern → label/lane auto-routing.
- [`lem-budgeting.md`](lem-budgeting.md) — cost model.
- [`ripr.md`](ripr.md) — the static oracle-gap detector that sits
  between coverage and runtime mutation testing.

## Doctrine

Test evidence is a spend-vs-signal trade. Different test families
warrant different cadences:

| Test family | Default cadence | Why |
|---|---|---|
| Unit + workspace lib tests | every PR | Cheapest signal per minute. Catch most regressions. |
| Bounded smoke (acceptance-flavoured but capped) | every PR | Constant-cost-per-PR signal that the user-flow surface still works end-to-end. Specifically *not* a full sweep. |
| Broad acceptance / property / fuzz / BDD matrix | label-gated **or** nightly cron | Real evidence but expensive; only fire when a reviewer explicitly asks (`bdd`, `property-tests`, `fuzz`, `full-ci`) or on the scheduled cron. |
| Mutation testing | targeted PR (`mutation` label) **or** nightly cron **or** release readiness | High-cost runtime evidence; never default-PR. |
| `ripr` (static oracle-gap) | every Rust-diff PR | Cheap static substitute that surfaces "this changed line is not exercised by any test that could discriminate behavior." Advisory only. |
| Coverage | scheduled nightly run or explicit `workflow_dispatch` with coverage enabled | Advisory Codecov upload; it is not a PR or merge-queue lane. |

The doctrine, from [`ripr.md`](ripr.md):

> `ripr` shifts mutation signal left — static, per-PR, and cheaper than
> runtime mutation testing, which remains the backstop for what static analysis
> cannot predict. See [`docs/ci/ripr.md`](ripr.md) for the canonical framing.

## Lane shapes

### 1. PR-fast required

Runs on every PR. Blocks merge. Constant per-PR cost.

Includes (this set evolves; consult `ci-lane-map.md` for the current
inventory):

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --lib --no-deps -- -D warnings`
- `cargo clippy --workspace --all-targets --no-deps -- -D warnings -A missing_docs`
- Workspace unit + lib tests for at least `perl-lsp-rs --lib`.
- The bounded UX scenario smoke (e.g. `ux_scenario_14_inc_conformance`).
- `cargo xtask check-lint-policy` and any other `cargo xtask check-*`
  ledger gates that have shipped at the time of the PR (the
  per-checker set grows as ledger PRs land; see
  [`ci-lane-map.md`](ci-lane-map.md) and the governed policy files for the
  current shipped inventory).
- `cargo deny check` (if wired into the canonical CI workflow).
- `ripr` advisory lane.

The bound matters as much as the inclusion: PR-fast lanes have a
**capped** cost. A 30-minute property sweep does not belong here; a
30-second bounded smoke does.

### 2. PR-targeted (label-gated)

Runs only when a reviewer applies the routing label. The matching
risk-pack in [`../../policy/ci-risk-packs.toml`](../../policy/ci-risk-packs.toml)
may auto-apply the label based on changed paths (e.g. `parsers-serde`
risk-pack auto-applies `fuzz` when a fuzz target or `*_parse.rs` file
changes).

| Label | Lane it activates |
|---|---|
| `bdd` | broader BDD scenarios in `bdd-testing.yml` (full matrix). |
| `property-tests` | broad property sweep in `property-testing.yml` (high case-count). |
| `fuzz` | quick-fuzz across all parser surfaces in `fuzzing.yml`. |
| `mutation` | targeted mutation testing in `mutation-testing.yml` (scoped to touched risk-pack). |
| `security-audit` | standalone `cargo-deny` in `ci-security.yml` (also fires on push-main + weekly cron). |
| `full-ci` | "spend authorization": activates every label-gated lane on a single PR. Reviewer signs off on the cost. |

### 3. Nightly cron

Runs on schedule regardless of PRs. Full sweeps, expensive evidence,
canary lanes.

- Full property-test sweep (256+ cases per crate).
- Full fuzz matrix at extended budget.
- Full mutation sweep across trust surfaces.
- Coverage via the scheduled `ci-nightly.yml` run or an explicit manual dispatch.
- `ci-nightly.yml` for any other long-running canary.

The point of nightly: surface regressions that the bounded PR-fast
smoke misses, without billing every PR for the full sweep.

### 4. Release-only (tag-triggered)

Runs only when a `v*` tag is pushed or `release.yml` is dispatched.

- Multi-platform release builds (Linux / macOS x86_64 + arm64 / Windows).
- `cargo package --locked` proof.
- `cargo publish --dry-run` on the foundation crate (the rest of the
  publish chain dry-runs interleaved with the actual publish, since
  each non-foundation crate needs the prior crate on crates.io before
  its dry-run resolves).
- Install-smoke against the published artifact.
- Crates.io publish in dependency order (owner-gated).

See [`../release/RUNBOOK.md`](../release/RUNBOOK.md) for the
end-to-end release execution flow.

### 5. Advisory

Lanes that emit signal but **never block merge**. A reviewer reads
their findings and treats them as input to judgement.

- `ripr` (static oracle-gap detection).
- Bot review lanes: `droid-review`, `tokmd`, `CodeRabbit` if active.
- `pr-plan` LEM forecast.
- `methodology-gate` (currently advisory pending its own ratchet).

The reviewer's job is to weight advisory output against the change's
risk surface and the matching risk-pack. A `ripr` finding on a
parser-touching PR is more load-bearing than the same finding on a
docs-only PR.

## Doctests as proof

Doctests **are** part of the proof surface, but only for the packages the
`doctest_contract_proof` merge-gate route names. Nothing else in the
repository runs `cargo test --doc`: `--lib` excludes doctests, `--tests`
selects integration targets only, `--all-targets` expands to
`--lib --bins --tests --benches --examples` and **excludes** `--doc`, and
`cargo nextest` cannot run them at all. Until #13774 that route did not
exist, so every doctest in the workspace was compiled by nobody.

This matters most for `compile_fail`, the repository's only idiom for a
negative type-level contract. An unenforced `compile_fail` is worse than no
test: reintroducing the very construct it forbids leaves CI fully green.

### Where a negative type-level contract belongs

Two homes, and the choice is mechanical:

| Situation | Home |
|---|---|
| The crate is selected by `doctest_contract_proof`, and the item is reachable under that route's feature selection | a `compile_fail` doctest, in place |
| Anything else — notably an item behind a non-default feature | a compile-time assertion in an integration test a gate already runs |

For the second, use `static_assertions::assert_not_impl_any!` (a workspace
dev-dependency) and add the target to the gate that already builds that crate.
`crates/perl-parser/tests/incremental_state_read_only_authority.rs` is the
worked example: its contracts were `compile_fail` doctests on an item behind
the non-default `incremental` feature, so no `--doc` route could reach them.

`doctest_enforcement` keeps the choice honest. A `compile_fail` fence in a
crate the route does not select fails that gate, naming the file and line.

### `doctest = false` is not an exclusion

A package declaring `[lib] doctest = false` can still be covered. Measured on
the pinned 1.95.0 toolchain, the field removes doctests from the *default*
`cargo test` target selection only; an explicit `cargo test -p <pkg> --doc`
collects and runs them regardless. `perl-parser-core` declares it and still
reports 18 passed / 3 failed under `--doc`.

So do not read that field as "this crate is out of the denominator". The real
exclusion is a `#[cfg(feature = "…")]` module behind a feature the route does
not enable: those doctests are never compiled at all.

### Vacuity: a doctest that passes without asserting anything

A doctest can run, pass, and prove nothing. Two shapes to avoid, neither of
which any gate can detect:

- **The hidden uncalled `fn`.** Wrapping the body in a function that is never
  called — commonly via `#` -hidden lines — means the example only
  type-checks. Nothing executes, so no assertion is evaluated. If the example
  is meant to *run*, call what it defines; if it is only meant to compile, say
  so in the prose so a later reader does not mistake it for behavioural proof.
- **The `compile_fail` that fails for the wrong reason.** `compile_fail`
  passes when the block does not compile, for *any* reason — a typo in a path,
  a missing `use`, a renamed item. Such a block keeps passing after the
  invariant it was written for is gone. When you add one, delete the forbidden
  line and confirm the block then *compiles*; that opposite-direction control
  is what distinguishes a contract from a typo. `crates/perl-corpus/src/files.rs`
  carries a worked pair.

## Risk-pack auto-routing

Risk packs in [`../../policy/ci-risk-packs.toml`](../../policy/ci-risk-packs.toml)
map changed-path patterns to labels and lanes. Applying a label is a
human reviewer's job; the *suggestion* of which label to apply comes
from the matching risk pack(s).

Example: a PR touching `crates/perl-parser-pest/` matches the
hypothetical `parsers-serde` risk-pack, which surfaces `fuzz` as the
recommended label. The reviewer can:

- Apply `fuzz` → activates the quick-fuzz lane.
- Apply `full-ci` → activates every recommended lane plus the broad ones.
- Apply nothing → the PR runs only PR-fast required lanes.

The risk-pack model is **advisory** at the PR level. The current mapping and
lane names are maintained in [`ci-lane-map.md`](ci-lane-map.md) and
`policy/ci-risk-packs.toml`.

## Skipped-by-policy receipts

When a lane skips for a particular PR, the lane must say **why** via
the GitHub Actions step summary or the lane's emitted receipt JSON.
Categories:

| Category | When |
|---|---|
| `docs-only` | PR matches the docs-only risk pack and no other. |
| `no-matching-risk-pack` | The lane is opt-in via risk pack; no risk pack on this PR selects it. |
| `label-absent` | Lane is opt-in via label; the label is not present. |
| `nightly-only` | Lane runs only on cron. |
| `release-only` | Lane runs only on tag push. |
| `ripr-waived` | A `ripr-waive` label suppressed advisory output for this PR (when wired). |
| `duplicate` | The lane's intent is already produced by another lane on this PR (e.g. standalone `cargo-deny` when `ci.yml` already ran it). |

Skip categories are recorded by the current CI Actuals surface described in
[`ci-actuals.md`](ci-actuals.md).

## Cost framing

LEM (Linux Equivalent Minutes) is the unit; see
[`lem-budgeting.md`](lem-budgeting.md) for the model. The PR plan
([`pr-plan.yml`](../../.github/workflows/pr-plan.yml) +
[`../../policy/ci-budget.toml`](../../policy/ci-budget.toml)) forecasts
the spend for each lane the PR would activate.

Tiers:

- `preferred_default_lem` — the spend a docs-only or fixture-only PR
  expects.
- `default_limit_lem` — soft ceiling for a normal PR; the plan warns
  past this without `ci-budget-ack`.
- `elevated_limit_lem` — ceiling for label-elevated PRs; requires
  `ci-budget-override`.
- `hard_limit_lem` — emergency ceiling; requires `full-ci` (which
  implies override).

These are the current LEM planning rules; learned estimates and actuals are
documented in [`lem-budgeting.md`](lem-budgeting.md) and
[`learned-estimates.md`](learned-estimates.md).

## See also

- [`ci-lane-map.md`](ci-lane-map.md) — current per-workflow inventory.
- [`ci-actuals.md`](ci-actuals.md) — current machine-readable actuals contract.
- [`lem-budgeting.md`](lem-budgeting.md) — LEM cost model.
- [`ripr.md`](ripr.md) — `ripr` static oracle-gap lane doctrine.
- [`../release/RUNBOOK.md`](../release/RUNBOOK.md) — release execution.

# Code Coverage

This document describes the code coverage infrastructure for perl-lsp.

## Overview

The perl-lsp project uses **cargo-llvm-cov** for code coverage generation and **Codecov** for coverage aggregation, trending, and PR reporting.

Branch coverage is enabled in the coverage lane with `cargo-llvm-cov --branch`. That requires a nightly Rust toolchain because LLVM branch coverage uses unstable `-Z coverage-options=branch` support.

The initial PR slice keeps the branch gate on library/unit tests (`--lib`) so it stays stable on current master. Integration snapshot suites can still be run separately outside the coverage lane.

## Quick Start

### Local Coverage Reports

Generate an HTML coverage report locally:

```bash
rtk just coverage
```

This will:
1. Install `cargo-llvm-cov` if not present
2. Use a nightly Rust toolchain for branch coverage support
3. Run tests with coverage instrumentation
4. Generate an HTML report at `target/coverage/index.html`
5. Attempt to open the report in your browser

### Terminal Summary

Get a quick coverage summary in the terminal:

```bash
rtk just coverage-summary
```

### LCOV Format (for CI)

Generate coverage in LCOV format (compatible with Codecov):

```bash
rtk just coverage-lcov
```

This creates `lcov.info` in the project root.

For the PR proof rail, generate the Codecov/quality-gate LCOV that includes
the parser surface and `xtask` receipt/gate code:

```bash
rtk just coverage-proof-lcov
```

## CI Integration

### Automatic Coverage Reports

Coverage is automatically generated and uploaded to Codecov from the nightly `test-coverage` job in `.github/workflows/ci-nightly.yml`:

- **On every push to `main`/`master`**: No coverage job by default
- **On every PR**: Parser coverage with branch data and the Codecov patch gate
- **On manual workflow dispatch**: Parser coverage with branch data

### Viewing Coverage in PRs

When a PR opens or updates:

1. The nightly coverage job runs automatically
2. Coverage data is uploaded to Codecov
3. Codecov posts a comment to the PR showing:
   - Overall coverage percentage
   - Coverage diff (lines added/removed)
   - Per-file coverage changes
   - Flags for each crate (parser, lsp, lexer, dap, corpus)

The nightly coverage lane also generates branch coverage in `lcov.info` and checks it against `.ci/coverage-baseline.txt`. The first slice is a ratchet: branch coverage can stay flat or improve, and it fails if the total drops by more than the allowed percentage point budget.

### Coverage Badge

The README includes a Codecov badge showing the current coverage on the `master` branch:

[![codecov](https://codecov.io/gh/EffortlessMetrics/perl-lsp/branch/master/graph/badge.svg)](https://codecov.io/gh/EffortlessMetrics/perl-lsp)

## Coverage Thresholds

Coverage status thresholds are defined in `codecov.yml`:

| Target | Threshold | Notes |
|--------|-----------|-------|
| **Project** | 95% | Overall coverage target, informational during burn-down; final gate requires `0.25%` threshold and blocking status |
| **Patch** | 95% | Blocking PR gate with `0%` threshold |

Codecov flags identify the crate paths for parser, `xtask`, LSP, lexer, DAP,
and corpus coverage tracking. The PR proof upload uses the `parser,xtask` flags
because `lcov.info` includes both the parser surface and the receipt/gate code.
Flags do not carry independent status targets in this slice; crate-specific
blocking thresholds should be added as explicit status rules when they are
promoted.

### Threshold Philosophy

- **95% patch gate**: New code must carry behavior proof before merge
- **95% project target**: Project coverage burns down toward the final blocking gate
- **Per-crate flags**: Component trend views stay available without adding
  unsupported Codecov flag-level targets
- **Temporary project exception**: `policy/quality-gate-exceptions.toml`
  records the dated `project-coverage-burndown` exception while the project
  status remains informational; it does not waive the final blocking target.
- **Final project gate**: `quality-gate --mode enforce` fails until Codecov
  project status is promoted to blocking `95%` target with `0.25%` threshold.

Branch coverage uses the checked-in policy file instead of Codecov status thresholds for the initial PR slice:

- `.ci/coverage-baseline.txt` stores the current branch-coverage baseline and regression budget
- `scripts/check-coverage-baseline.sh` compares the generated `lcov.info` against that baseline
- `scripts/update-coverage-baseline.sh` refreshes the checked-in baseline after an intentional improvement
- `target_branch_coverage` documents the intended longer-term 80% floor
- `.ci/README-coverage.md` is the terse operator note for the gate and baseline workflow

## Exclusions

The following paths are excluded from coverage analysis (configured in `codecov.yml`):

- `archive/**` - Legacy code
- `crates/tree-sitter-perl-rs/**` - C-based legacy parser
- `crates/tree-sitter-perl-c/**` - C bindings wrapper
- `crates/*/tests/**` - Test code
- `crates/*/benches/**` - Benchmark code
- `crates/*/examples/**` - Example code
- `crates/*/build.rs` - Build scripts
- `fuzz/**` - Fuzzing infrastructure
- `**/*_generated.rs` - Generated code

`xtask/**` is intentionally not ignored: the quality gate and receipt
generators are proof-rail code, so patch coverage must apply to them.
The coverage baseline receipt records `coverage_scope`; final
`quality-gate --mode enforce` requires workspace scope covering every Cargo
workspace member root plus the `xtask` proof rail, so parser-only or
parser-plus-xtask LCOV cannot satisfy repo-wide project coverage enforcement.
Absolute Linux or Windows `SF` paths in LCOV are normalized back to
repo-relative paths before the receipt computes scope or below-target file
guidance.

## Coverage Workflow

The coverage job (`.github/workflows/ci-nightly.yml`, `test-coverage`) runs on
PRs, nightly schedule, and manual dispatch. It performs these steps:

1. **Install toolchain**: Rust nightly with `llvm-tools-preview` component
2. **Install cargo-llvm-cov**: Using `taiki-e/install-action@v2` for speed
3. **Cache dependencies**: Uses `Swatinem/rust-cache@v2` for faster builds
4. **Create fixtures**: Legacy LSP test fixtures (if needed)
5. **Run parser branch ratchet**: Generate the lean parser-library branch
   coverage snapshot and compare it to `.ci/coverage-baseline.txt`
6. **Generate proof LCOV**: Regenerate `lcov.info` with parser code plus the
   `xtask` proof rail, so Codecov and `quality-gate` receipts see the code
   that emits the proof
7. **Display summary**: Show coverage summary in logs
8. **Write coverage receipt**: Emit
   `target/receipts/quality/coverage-baseline.json` for `quality-gate`,
   including the top below-target LCOV files and representative positive,
   1-based uncovered line samples. The receipt command rejects LCOV snapshots
   with no measured `LF` lines so empty reports cannot masquerade as `100%`
   coverage, rejects LCOV `DA` entries whose line number is `0`, and records
   `coverage_scope` from normalized repo-relative `SF` paths so final
   enforcement can reject partial LCOV inputs.
   `quality-gate` only renders positive uncovered line samples in its aggregate
   receipt and markdown summary.
9. **Run quality gate**: Verify the coverage receipt is current and the
    Codecov patch policy is enforcing via `quality-gate --mode
    enforce-patch-coverage --codecov codecov.yml`; the gate reads that live
    policy file as authoritative over any policy snapshot stored in the receipt
    and also verifies that Codecov comments include `diff` and `files`
    guidance. If the receipt includes a patch
    percentage, the gate blocks values below `95%` and includes ranked
    below-target files and uncovered line samples when the local coverage
    receipt contains actionable rows. Non-actionable file rows are filtered so
    the failure falls back to Codecov `diff`/`files` guidance instead of naming
    a vague repair target. Final enforce mode also blocks partial or unknown
    `coverage_scope` receipts. CI preserves the gate's failure exit code while
    still appending the generated markdown repair summary to the GitHub step
    summary.
10. **Upload to Codecov**: Upload `lcov.info` to Codecov service with
    `parser,xtask` flags; Codecov patch status is required at `95%`.
11. **Archive proof**: Save `lcov.info`, the coverage receipt, and the
    coverage quality-gate receipt/summary as workflow
   artifacts. CI first checks that each required proof artifact exists and is
   non-empty, so a partial upload cannot hide a missing receipt.

The coverage quality-gate markdown also includes the PR-body proof fields for
coverage/proof/enforcement lane changes: objective, claim boundary, non-goals,
RIPR/coverage effect, local proof commands, cleanup performed, and what remains.
Its suggested local proof commands include `rtk git status --short --branch`,
`rtk git diff --check`, and `rtk bash scripts/storage-doctor` so cleanup evidence
travels with the receipt guidance instead of being a separate memory-only step.

`quality-gate` records `coverage.codecov_config_status` separately from the
LCOV receipt status. A missing or invalid `codecov.yml` becomes its own repair
action, so policy failures are not hidden behind a stale coverage receipt.

Coverage failures should name behavior proof, not just lines. The gate guidance
points agents toward tests for error paths, boundaries, config parsing,
serialization, cancellation, provider decisions, and output contracts in the
ranked uncovered files.

When the Codecov patch percentage is known locally or supplied by CI, pass it
through the aggregate gate:

```bash
rtk cargo xtask quality-gate --mode enforce-patch-coverage --codecov codecov.yml --patch-coverage 97.25
```

The resulting receipt records `coverage.patch_source = "cli"`, and rerun/check
commands in the markdown summary preserve the same patch percentage.

When the numeric patch percentage is not available inside the local job but the
required Codecov patch status is the blocking PR source, make that delegation
explicit instead of leaving patch coverage unknown:

```bash
rtk cargo xtask quality-gate --mode enforce-patch-coverage --codecov codecov.yml --patch-status-source codecov
```

That receipt records `coverage.patch_source = "codecov_status"`. If neither
`--patch-coverage` nor `--patch-status-source codecov` is provided, the patch
gate fails with `patch_coverage_unknown`.

### Environment Variables

The workflow uses optimized settings:

- `RUSTFLAGS="-Copt-level=1"` - Fast builds with basic optimization
- `CARGO_BUILD_JOBS=2` - Limit parallelism to avoid memory pressure
- `RUST_TEST_THREADS=2` - Adaptive threading for LSP tests

## Codecov Repository Secret

Set this GitHub Actions secret at the repository or organization level:

- `CODECOV_TOKEN`

The coverage and test-results uploads use this token. The coverage upload is a
required proof path for the patch gate, so upload failures are not hidden.

## Configuration Files

### codecov.yml

The `codecov.yml` file at the project root configures:

- Coverage precision and rounding
- Project and patch coverage thresholds
- Exclusion patterns
- Per-crate flags for detailed tracking
- PR comment layout

### .github/workflows/ci-nightly.yml

The coverage workflow file defines:

- When coverage runs (PR, schedule, manual dispatch)
- Build and test environment
- Upload configuration
- Artifact retention

## Troubleshooting

### cargo-llvm-cov not found

The `just coverage` recipes automatically install `cargo-llvm-cov` if missing. To install manually:

```bash
rtk cargo install cargo-llvm-cov --locked
```

### Coverage report generation fails

Some tests may be flaky under coverage instrumentation due to timing or memory constraints. The workflow uses:

- Lower optimization (`-Copt-level=1`) for faster builds
- Limited parallelism (`CARGO_BUILD_JOBS=2`, `RUST_TEST_THREADS=2`)

If branch coverage is involved, make sure you are using `cargo +nightly llvm-cov` or a nightly-installed Rust toolchain. Stable rustc cannot compile the `--branch` instrumentation path.
If your shell does not proxy `cargo +nightly` correctly on Windows, use `rustup run nightly cargo ...` instead.

### Codecov upload fails

The coverage workflow sets `fail_ci_if_error: true` because Codecov patch
coverage is a required PR gate. Check:

1. Codecov service status: https://status.codecov.io/
2. Workflow logs for upload details
3. Codecov dashboard for processing status

### Test Analytics upload fails

Check:

1. `CODECOV_TOKEN` is configured as a GitHub Actions secret.
2. The JUnit file exists under `target/test-results/`.

### Coverage proof upload fails

Check:

1. `lcov.info` exists in the workspace root.
2. The coverage receipt exists at
   `target/receipts/quality/coverage-baseline.json`.
3. The coverage quality-gate receipt exists at
   `target/receipts/quality/coverage-quality-gate.json`.
4. The coverage proof artifact uses `if-no-files-found: error`, so missing proof
   fails the workflow.

### Coverage numbers look wrong

Coverage excludes:
- Test code itself (`tests/`, `benches/`)
- Legacy/archived code (`archive/`, `tree-sitter-perl-rs/`)
- Generated code (`*_generated.rs`)

To see what's included, check the exclusion patterns in `codecov.yml`.

## Best Practices

### Writing Testable Code

To improve coverage:

1. **Prefer small, focused functions** - Easier to test comprehensively
2. **Use Result/Option patterns** - Test both success and error paths
3. **Avoid unwrap/expect** - Return Result and test error cases
4. **Extract complex logic** - Make it testable in isolation

### Reviewing Coverage

When reviewing PRs with coverage:

1. Check the Codecov PR comment for coverage delta
2. Look for uncovered lines in changed files
3. Ask: "Are the uncovered lines error paths that should be tested?"
4. Don't aim for 100% - focus on meaningful test coverage

For branch coverage, pay attention to error paths and recovery branches. A file can have acceptable line coverage while still leaving branches untested.

### Coverage-Driven Development

Use coverage to find gaps:

```bash
# Generate HTML report
rtk just coverage

# Open target/coverage/index.html
# Find uncovered lines (red highlighting)
# Write tests for uncovered behavior
# Re-run coverage to verify
```

## Integration with CI Gates

Coverage is part of the PR proof surface through the `Codecov / Patch 95`
check, the Codecov patch status, and the coverage quality-gate receipt. It runs:

- On every PR for patch coverage proof
- On manual workflow dispatch
- On the nightly schedule for trend/canary proof

Branch coverage in the parser coverage lane is enforced separately through the baseline ratchet in `.ci/coverage-baseline.txt`.

The `ci:coverage` label remains a legacy routing alias; it is no longer
required for the patch gate.

## Test Analytics

Codecov Test Analytics is populated from CI receipt artifacts.

The CI gates already emit structured receipts under `target/receipts/`. The helper script `scripts/ci/receipts-to-junit.py` converts those receipts into JUnit XML and uploads them with `codecov/test-results-action@v1`.

Current uploaded suites:

- `pr-fast` — PR smoke test results
- `gate-<shard>` — Merge gate shard results (meta, foundation, analysis, lsp, support, corpus, policy)
- `ux-regression` — UX regression test results from main CI
- `ux-regression-gate` — UX regression gate results (standalone workflow)

This avoids rerunning tests solely to produce JUnit XML and keeps the existing `xtask gates` runner as the source of truth.

## Bundle Analysis

Codecov Bundle Analysis is intentionally deferred.

The VS Code extension currently compiles TypeScript and packages a VSIX. It does not use Vite, Rollup, Webpack, Remix, Nuxt, SvelteKit, or SolidStart as its production bundler.

Do not add `@codecov/vite-plugin` until the extension adopts a supported JavaScript bundler. When that happens, add the Codecov bundler plugin to the actual bundler config and use `CODECOV_TOKEN` for upload.

## Related Documentation

- [CLAUDE.md](../CLAUDE.md) - Project commands and structure
- [CONTRIBUTING.md](../CONTRIBUTING.md) - Contribution guidelines
- [CURRENT_STATUS.md](./CURRENT_STATUS.md) - Project metrics and health

## References

- [cargo-llvm-cov](https://github.com/taiki-e/cargo-llvm-cov) - Coverage tool
- [Codecov Documentation](https://docs.codecov.com/) - Service documentation
- [Rust instrumentation-based coverage](https://doc.rust-lang.org/rustc/instrument-coverage.html) - rustc coverage

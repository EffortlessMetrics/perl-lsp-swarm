# CLAUDE.md (perl-test-must)

## Role

Dependency-free leaf package for panic-on-failure extraction at Rust test
assertion boundaries.

## Owns

- `must` and `must_with` for required `Result::Ok` branches.
- `must_some` and `must_some_with` for required `Option::Some` branches.
- `must_err` and `must_err_with` for required `Result::Err` branches.
- The shared failure-diagnostic schema for helper identity, optional context,
  expected and observed branch, relevant types, and unexpected values.
- The `#[track_caller]` contract that reports the test invocation rather than
  helper internals.
- Public documentation and executable examples for those helpers.

## Does not own

- Production error handling or recovery.
- Fallible setup propagation through `Result` and `?`.
- Fixtures, temporary directories, snapshots, or property testing.
- BDD or `BddScenario` authoring.
- TAP parsing, test-result semantics, subprocesses, or test-runner execution.
- Parser, LSP, DAP, or editor test harnesses.
- Workspace-wide lint migration or compatibility cleanup.

## Neighbors

- Upstream: none; the package uses only the Rust standard library.
- Downstream: workspace tests that need direct assertion-boundary extraction.
- Compatibility: `perl-tdd-support` may forward these helpers during migration,
  but it is not their semantic owner.

## Read first

- `src/lib.rs` -- the complete implementation and public API.
- `tests/contract.rs` -- branch, ownership, context, and diagnostic proof.
- `tests/track_caller.rs` -- process-isolated caller-location proof.
- `README.md` -- user-facing decision boundary and examples.
- `Cargo.toml` -- package metadata and zero-dependency boundary.

## Focused validation

```bash
cargo test -p perl-test-must --locked
cargo test -p perl-test-must --doc --locked
cargo clippy -p perl-test-must --all-targets --locked -- -D warnings
cargo doc -p perl-test-must --no-deps --locked
cargo xtask check-agent-context
git diff --check
```

## Review hotspots

- Preserve zero dependencies.
- Preserve the original three function signatures unless a dedicated API
  change explicitly owns compatibility.
- Do not add `Send`, `Sync`, or `'static` bounds absent a real semantic need.
- Keep `must` and `must_with` usable for side-effecting `Result<(), E>` calls
  without a noisy `#[must_use]` contract.
- Keep intentional panic permission narrow, reasoned, and located at the shared
  failure seam.
- Keep diagnostic clauses and caller location behavior-backed rather than
  relying on prose or exact standard-library type paths.
- Do not add macros, extension traits, matchers, BDD, or umbrella test utilities
  merely because this package is small.

## Claim boundary

This package proves branch extraction, failure diagnostics, and caller location
for the documented helpers. It does not make panic-based extraction appropriate
for production code or replace propagation, framework, runner, or harness
owners.

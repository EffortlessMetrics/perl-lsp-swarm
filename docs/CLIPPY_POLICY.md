# Clippy policy

`perl-lsp` treats Clippy as a governed engineering surface, not as a local taste file. The workspace policy is recorded in three places:

- `Cargo.toml` contains the active `[workspace.lints]` block inherited by member crates.
- `policy/clippy-lints.toml` is the machine-readable ledger for active, debt, tracked, and planned Rust-version lint flips.
- `policy/clippy-debt.toml` records temporary, expiring debt instead of weakening the global policy silently.

## Workspace posture

The policy applies to production code and to test, example, and benchmark targets reached by the maintained gates. The active Cargo lint block denies unchecked `Result`/`Option` collapse through `unwrap_used` and `expect_used`; `clippy.toml` no longer grants an `allow-unwrap-in-tests` exception and currently configures no method-level exceptions.

For fallible test setup that should propagate, return `Result` and use `?`. For an asserted branch at the test boundary, use the extraction helpers owned by `perl-test-must`, such as `perl_test_must::must` and `perl_test_must::must_some`. Existing `perl_tdd_support::must*` imports are compatibility and migration state governed by #8605 and #8436; do not add a new dependency on the umbrella package solely for these helpers.

The tracked lint set covers five guardrail families:

1. **Panic-free code**: no unchecked `Result`/`Option` collapse, panic macros, `todo!`, `unimplemented!`, or `unreachable!` paths.
2. **AST and UTF-8 safety**: parser and LSP boundary code must avoid unchecked string slicing, byte/character index confusion, and unchecked indexing.
3. **Silent-failure prevention**: ignored futures, ignored `must_use` values, discarded errors, and lossy line iteration are denied.
4. **Async, memory, numeric, and file/process footguns**: concurrency and parser correctness hazards are denied or warned according to the ledger.
5. **Suppression governance**: broad or unexplained suppressions are rejected. Prefer narrow `#[expect(..., reason = "...")]` receipts.

Intentional assertion helpers and explicit panic-injection tests may carry narrow, reviewed exceptions. They do not create a general test carveout.

## Suppression style

Use `#[expect]` only when the lint is correct but the local exception is intentional and reviewed:

```rust
#[expect(
    clippy::indexing_slicing,
    reason = "Generated parser table access is bounded by table construction invariants."
)]
fn generated_table_lookup(table: &[usize], index: usize) -> usize {
    table[index]
}
```

Do not use a silent `#[allow]`. If a lint needs repo-wide temporary treatment, add a scoped entry to `policy/clippy-debt.toml` with `lint`, `path`, `owner`, `reason`, and `expires`.

## Rust-version lint planning

The product MSRV is recorded in `Cargo.toml`, `clippy.toml`, and the governed lint ledger. `cargo xtask check-lint-policy` verifies that planned lints are present in the ledger, active lints agree with Cargo, and version-gated lints are not activated ahead of the product toolchain.

The Rust 1.95 rollout material remains useful historical and design context, but current configuration is authoritative for current-state claims. The former `allow-unwrap-in-tests = true` setting has already been removed. Historical documents may describe that earlier rollout state; active policy must not present it as a current exception or pending removal.

## Local check

Run the policy gate before changing lint configuration:

```bash
cargo xtask check-lint-policy
```

The gate checks lint inheritance, active Cargo lint levels, tracked lint metadata, planned upgrade ledger entries, and required debt metadata.

## Protected-field planning

The `clippy::disallowed_fields` rail is **activated** (#6114) with an empty
denylist in `clippy.toml` (`disallowed-fields = []`). The design anchor lives in
[`CLIPPY_PROTECTED_FIELDS.md`](CLIPPY_PROTECTED_FIELDS.md). Concrete field
selectors and accessors will be added incrementally through their owning lint-policy work.

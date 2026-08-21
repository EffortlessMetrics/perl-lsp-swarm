# Clippy policy

`perl-lsp` treats Rust and Clippy lints as one governed product contract, not as a collection of local preferences.

## Authorities

- `Cargo.toml` contains the active `[workspace.lints]` policy inherited by every member crate.
- `policy/clippy-lints.toml` contains the ledger schema, product MSRV, policy posture, future-planned lints, and review-dated due deferrals.
- `policy/clippy-lints.d/*.toml` contains the lint catalog. The checker loads these fragments in sorted path order and validates them as one logical ledger.
- `policy/clippy-debt.toml` records exact current source-level debt. A lint in `debt` state must have at least one matching row, and every debt row must point back to a `debt` lint at the same level.
- `clippy.toml`, `rust-toolchain.toml`, and `.ci/gate-policy.yaml` carry configuration and toolchain inputs that must agree with the product MSRV.

`cargo xtask check-lint-policy` is the coherence authority across those files.

## One current disposition

Every governed lint has exactly one current state:

- `active`: the exact level exists in Cargo.
- `debt`: the exact Cargo level exists and current debt rows own the bounded exceptions.
- `tracked`: the lint is catalogued but absent from Cargo.
- `planned`: the lint is unavailable before a future product MSRV.
- `deferred_due`: the lint is already available, but an owner, reason, review date, and intended next state explicitly bound the remaining work.

A lint cannot appear in two states. A Cargo lint without a ledger entry fails, as does an active ledger entry missing from Cargo. Due lints cannot remain ordinary planned work indefinitely.

## Workspace posture

The policy applies to production code and to test, example, and benchmark targets reached by the maintained gates. The active Cargo lint block denies unchecked `Result`/`Option` collapse through `unwrap_used` and `expect_used`; `clippy.toml` no longer grants an `allow-unwrap-in-tests` exception and currently configures no method-level exceptions.

For fallible test setup that should propagate, return `Result` and use `?`. For an asserted branch at the test boundary, use the extraction helpers owned by `perl-test-must`, such as `perl_test_must::must` and `perl_test_must::must_some`. Existing `perl_tdd_support::must*` imports are compatibility and migration state governed by #8605 and #8436; do not add a new dependency on the umbrella package solely for these helpers.

1. **Panic and silent-failure control:** unchecked `Result`/`Option` collapse, discarded futures, ignored `must_use` work, and hidden errors.
2. **AST, UTF-8, and numeric correctness:** unchecked slicing/indexing, byte/character confusion, unsafe casts, and arithmetic hazards.
3. **Async and memory review:** lock/borrow behavior across suspension, ownership, unsafe blocks, and representation assumptions.
4. **File, process, API, and reviewability rules:** explicit filesystem/process behavior and inspectable public/API intent.
5. **Suppression governance:** narrow `#[expect(..., reason = "...")]` evidence instead of broad or unexplained allowances.

## Suppression and debt

Intentional assertion helpers and explicit panic-injection tests may carry narrow, reviewed exceptions. They do not create a general test carveout.

## Suppression style

Use `#[expect]` only when the lint is correct but the local exception is intentional and reviewed:

```rust
#[expect(
    clippy::indexing_slicing,
    reason = "Generated table construction proves this index is in bounds."
)]
fn generated_lookup(table: &[usize], index: usize) -> usize {
    table[index]
}
```

A temporary repository debt row records `lint`, `level`, `path`, `owner`, `reason`, and `review_after`. Empty, expired, unowned, pathless, level-inconsistent, or orphaned debt fails the policy check. An unchanged count cannot hide one finding replacing another.

## Rust-version lint planning

The product MSRV is recorded in `Cargo.toml`, `clippy.toml`, and the governed lint ledger. `cargo xtask check-lint-policy` verifies that planned lints are present in the ledger, active lints agree with Cargo, and version-gated lints are not activated ahead of the product toolchain.

The Rust 1.95 rollout material remains useful historical and design context, but current configuration is authoritative for current-state claims. The former `allow-unwrap-in-tests = true` setting has already been removed. Historical documents may describe that earlier rollout state; active policy must not present it as a current exception or pending removal.

## Local check

Run the policy check before changing Cargo lint levels, Clippy configuration, debt, or the product toolchain:

```bash
cargo xtask check-lint-policy
```

The command prints deterministic active, debt, tracked, future-planned, and due-deferred populations. Unknown fields, malformed versions, duplicate identities, stale deferrals, reintroduced test carveouts, or missing policy inputs are non-success.

## Protected fields

The `clippy::disallowed_fields` rail is **activated** (#6114) with an empty
denylist in `clippy.toml` (`disallowed-fields = []`). The design anchor lives in
[`CLIPPY_PROTECTED_FIELDS.md`](CLIPPY_PROTECTED_FIELDS.md). Concrete field
selectors and accessors will be added incrementally through their owning lint-policy work.

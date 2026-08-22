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

The policy governs the lint levels inherited by production and test targets. Its maintained enforcement surface is the required workspace `--lib` gate, the production `--bins` gate, and the explicitly listed all-targets kernel cohort; that cohort is intentionally non-exhaustive. This document therefore does not claim that every test target is currently checked by the strict Clippy gate. Test failures within the enforced surface should use `Result`, `?`, or repository assertion helpers that preserve the underlying error. The old Clippy test-carveout keys are not accepted policy and cannot return through `clippy.toml`.

The tracked catalog covers five broad families:

1. **Panic and silent-failure control:** unchecked `Result`/`Option` collapse, discarded futures, ignored `must_use` work, and hidden errors.
2. **AST, UTF-8, and numeric correctness:** unchecked slicing/indexing, byte/character confusion, unsafe casts, and arithmetic hazards.
3. **Async and memory review:** lock/borrow behavior across suspension, ownership, unsafe blocks, and representation assumptions.
4. **File, process, API, and reviewability rules:** explicit filesystem/process behavior and inspectable public/API intent.
5. **Suppression governance:** narrow `#[expect(..., reason = "...")]` evidence instead of broad or unexplained allowances.

## Suppression and debt

Use `#[expect]` only where the lint is correct and the exact exception is intentional:

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

## Toolchain currentness

The product Rust version is normalized across:

```text
Cargo.toml workspace.package.rust-version
policy/clippy-lints.toml msrv
clippy.toml msrv
rust-toolchain.toml channel
.ci/gate-policy.yaml global.toolchain.msrv
```

`1.95` and `1.95.0` describe the same product version. Any other drift fails closed. A private analysis-tool toolchain, including Cargo-Hawk's compiler, does not satisfy this product contract.

## Local check

Run the policy check before changing Cargo lint levels, Clippy configuration, debt, or the product toolchain:

```bash
cargo xtask check-lint-policy
```

The command prints deterministic active, debt, tracked, future-planned, and due-deferred populations. Unknown fields, malformed versions, duplicate identities, stale deferrals, reintroduced test carveouts, or missing policy inputs are non-success.

## Protected fields

`clippy::disallowed_fields` is active at deny, while `clippy.toml` deliberately carries an empty `disallowed-fields` set. This proves the mechanism is live; it does not claim any parser, LSP, DAP, or workspace field is protected yet. [`CLIPPY_PROTECTED_FIELDS.md`](CLIPPY_PROTECTED_FIELDS.md) owns the reviewed field-selection programme.

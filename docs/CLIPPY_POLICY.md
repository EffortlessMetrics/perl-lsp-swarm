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

### Target-kind lint contract (#11736)

A governed lint applies to every target kind unless this section rules otherwise for a named pairing. The decided matrix:

| governed lints | lib / bins (incl. `#[cfg(test)]`) | benches | `tests/` + `examples/` |
|---|---|---|---|
| `print_stdout`, `print_stderr` | deny | deny | **intentional** |
| `unwrap_used`, `expect_used`, `panic`, `todo`, `unimplemented`, `dbg_macro` | deny | deny | deny |
| `disallowed_fields` and every catalogued warn-level style/correctness lint | unchanged level | unchanged level | unchanged level |

Rationale: a direct `print!` in an integration test or example is intentional diagnostics or demonstrated CLI behavior; the `print_*` denial reasons scope to library output discipline, which these targets do not have. The panic family stays denied everywhere because `[policy] panic_free_tests = true` is settled contract: test failures must flow through typed results or repository assertion helpers (`perl-test-must`), not unchecked collapse or aborts.

The intentional pairings are encoded at source, not by config or gate flags. A `tests/` or `examples/` file whose direct printing is intentional carries a file-scoped reasoned expectation:

```rust
#![expect(clippy::print_stdout, reason = "This example demonstrates CLI output; tracing is not available in examples.")]
```

This keeps the single uniform `-D warnings` kernel command, preserves production enforcement locally as well as in CI, and self-ratchets through `unfulfilled_lint_expectations` when the printing goes away. Bare `#[allow]`, `clippy.toml` test-carveout keys (still banned under `[policy] allow_test_carveouts = false`), and blanket `-A clippy::print_*` gate tiers are all rejected mechanisms for this pairing: gate flags leak onto the production units compiled by the same invocation and defeat narrow suppression governance.

Kernel cohort admission protocol (#11736): a crate outside the cohort is admitted only when its current measured residual is zero — every finding is either repaired or ruled intentional above — proven by rerunning the exact kernel command locally before extending the selector. Crates with remaining findings are named repair tranches on #11736 with per-lint counts; they join through the same protocol in later increments. The census method that produces those counts runs per-crate `cargo clippy --locked --keep-going --message-format=json` over disjoint unit sets (`--lib --bins`, then `--tests --benches --examples`) with governed lints downgraded to warn, so no dependency failure can mask downstream findings.

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

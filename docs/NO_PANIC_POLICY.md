# No-panic policy

`perl-lsp` treats unchecked panic paths as reliability debt. Parser, LSP, DAP,
workspace, release, and policy tooling should return structured errors or use
reviewed test helpers instead of collapsing `Result` or `Option` values.

## Current rollout status

The Rust 1.95 rollout established active panic-family lint bans and a governed
path toward exact counted no-new-debt enforcement. The rollout map remains in
[`docs/ci/perl-lsp-rust-1.95-rollout.md`](ci/perl-lsp-rust-1.95-rollout.md), but
current source and policy files are authoritative for current-state claims.

Current guardrails are split across:

- active panic-family Clippy bans in [`Cargo.toml`](../Cargo.toml), including
  `unwrap_used`, `expect_used`, `panic`, `todo`, `unimplemented`, and
  `dbg_macro`;
- the governed lint ledger in
  [`policy/clippy-lints.toml`](../policy/clippy-lints.toml);
- shared Clippy configuration in [`clippy.toml`](../clippy.toml), which no
  longer contains an `allow-unwrap-in-tests` exception.

The removed test unwrap carveout may still appear in historical rollout records.
It is not a current permission and must not be used to justify new test debt.

## Target posture

The target no-panic lane is exact and counted:

1. Findings are keyed by path, family, selector kind, selector callee, snippet,
   and count.
2. Exact allowlist count slots are consumed first.
3. Baseline count slots are consumed second unless policy mode is blocking.
4. Anything left is reported as new debt.

Do not reset the no-panic baseline outside the dedicated baseline PR. A baseline
refresh may drop disappeared entries, but it must not absorb new panic-family debt
without an explicit reset.

## Test guidance

For fallible setup or helper work whose error should propagate, return `Result`
and use `?`. Do not replace propagation with a panic merely to satisfy a lint.

When the test scenario asserts that a `Result` or `Option` branch is impossible,
use the helpers owned by `perl-test-must`, such as:

```rust
use perl_test_must::{must, must_err, must_some};
```

Existing `perl_tdd_support::must*` imports are compatibility and workspace
migration state governed by #8605 and #8436. New code should not depend on the
broader `perl-tdd-support` package solely to obtain these helpers.

Intentional assertion panics and explicit panic-injection tests require narrow,
reviewed exceptions at the actual panic owner. They do not make accidental panic
paths acceptable in the rest of a test target.

# Your First PR

Five minutes to read. Then clone and go.

## 1. Clone

```bash
git clone https://github.com/EffortlessMetrics/perl-lsp.git
cd perl-lsp
```

No Git submodules to initialize. A normal clone is enough; `--recurse-submodules` is optional
and has no effect for this repo.

You can verify this at any time:

```bash
git submodule status
```

Expected output is empty.

The `tree-sitter-perl/` directory is checked-in legacy C code (not a submodule) and is excluded
from the default Rust build, so you can ignore it unless you are working on parser migration tasks.

## 2. Set up the dev environment

**With Nix (recommended — fully reproducible):**

```bash
nix develop
```

**Without Nix — install Rust and `just`, then you are ready:**

```bash
# Install Rust if you don't have it
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Install just (task runner)
cargo install just

# Verify tools are present
just doctor
```

Rust toolchain is pinned in `rust-toolchain.toml` (MSRV 1.95, channel `1.95.0`). `rustup` picks it up automatically.

**Windows users:** format with `cargo xtask fmt` (or `cargo fmt -p <package>`) instead of `cargo fmt --all`. Bare `cargo fmt --all` passes every workspace file as a command-line argument — across this workspace that sums to roughly six times the 32,767-character `CreateProcessW` command-line limit, so the invocation fails with "The filename or extension is too long. (os error 206)". This is a command-line-length limit, not a path-length limit: enabling `LongPathsEnabled` does not affect it, and worktree depth is irrelevant. The xtask formatter runs rustfmt per package, which is the normative formatting entry point on Windows (CI runs `--all` on Linux, where the limit does not apply). Note the per-package margin is not unbounded: the largest package (xtask, ~640 files) passes roughly 29k characters of arguments on a short-rooted checkout, so a checkout root deep enough to add a few thousand characters can still hit the cap for that package — keep the clone path short; a chunked-invocation fix in the formatter is tracked separately.

**Windows users (optional):** symlink-creating tests skip with a visible `SKIPPED:` reason when the session lacks `SeCreateSymbolicLinkPrivilege` (os error 1314). Enabling Developer Mode (Settings → System → For developers) grants the privilege and opts the machine out of every skip, so those tests run in full. This is opt-in, not a requirement.

Install the pre-push hook so the fast gate runs before every push:

```bash
bash scripts/install-githooks.sh
```

## 3. Find a good first issue

```bash
gh issue list --label "good-first-issue" --state open
```

Good candidates are labeled `size/S` or `size/M`, have a clear acceptance criteria section, and
do not require swarm or architectural context. The issue body will tell you which file to edit.

If you are not sure which issue to pick, read a few. The ones with "Files Affected" or "Root
Cause" sections are the easiest to get started on.

## 4. Build and run the test for your crate

Run ONLY the tests for the crate you are changing, not the full workspace — the workspace has
many workspace members (see [CURRENT_STATUS.md](../project/CURRENT_STATUS.md) for live metrics) and full-workspace runs take minutes. The fast cycle is:

```bash
# Build to confirm it compiles
cargo build -p <crate-name>

# Run just that crate's tests
cargo test -p <crate-name>

# Run one specific test (faster iteration)
cargo test -p <crate-name> -- test_name_here --exact
```

For example, if you are fixing `perl-module`:

```bash
cargo test -p perl-module
```

For LSP crates, use threading flags to avoid flaky results:

```bash
RUST_TEST_THREADS=2 cargo test -p perl-lsp-rs -- --test-threads=2
```

## 5. Make your change

Edit, add your test first (TDD is preferred), then implement:

1. Write a test that fails for the right reason.
2. Make the minimal change to pass it.
3. Run the crate test again to confirm green.

**Banned in production code** (CI will catch these):

| Banned | Use instead |
|--------|-------------|
| `unwrap()`, `expect()` | `?`, `.ok_or_else()`, pattern matching |
| `panic!()`, `todo!()`, `unimplemented!()` | Return `Result` or `Option` |
| `dbg!()` | `tracing::debug!` |

In tests: use `Result<()>` return type or `perl_tdd_support::must` / `must_some` helpers instead
of `unwrap()`.

## 6. Verify locally

```bash
just pr-fast
```

This runs `cargo fmt`, `cargo clippy`, and the crate tests in about 1-2 minutes. Fix anything it
reports. If all green, you are ready to push.

## 7. Open a PR

```bash
git checkout -b fix/my-description
git add <files>
git commit -m "fix(scope): what you changed (#NNNN)"
git push -u origin fix/my-description
gh pr create
```

**PR title convention** (enforced by CI — get it right or the `validate-title` check fails):

```
type(scope): imperative summary (#NNNN)
```

The `(#NNNN)` at the end is the issue number. Required. Examples:

```
fix(perl-module): normalize path separators in use_lib tests (#4154)
docs(perl-lsp-semantic-tokens): correct token counts in CLAUDE.md (#4159)
test(perl-parser): add regression test for postfix deref chain (#4167)
```

Types: `feat`, `fix`, `docs`, `test`, `refactor`, `chore`.

## 8. What reviewers look for

PRs go through two review passes:

1. **Standards review** — fmt, clippy compliance, no banned constructs, test coverage, scope
2. **Deep review** — logic correctness, edge cases (feature PRs)

The review bot will add labels (`in-review`, `needs-deep-review`, `reviewed-deep`, `merge-ready`)
as your PR progresses. You do not need to do anything except respond to comments.

If the reviewer pushes a fix directly to your branch, that is normal. Check and approve it.

## Reference

| Need | Command |
|------|---------|
| New checkout health | `just doctor` |
| Tool/env check | `just devex` |
| Before push | `just ready` |
| Fast PR loop | `just pr-fast` |
| Agent-safe compile/test | `just agent-check` / `just agent-test` |
| Parser-accuracy metrics | `just ci-metrics-ratchet-check parser_accuracy` |
| Status docs | `just status-update` then `just status-check` |
| Release/version prep | `just version-check` then `just release-check` |
| DevEx docs drift | `cargo xtask check-devex-docs` |
| Build LSP server | `cargo build -p perl-lsp-rs --release` |
| Run all library tests | `cargo test --workspace --lib` |
| Format | `cargo xtask fmt` |
| Lint | `cargo clippy --workspace` |
| Full merge gate | `just ci-gate` |
| All commands | [docs/reference/COMMANDS_REFERENCE.md](../reference/COMMANDS_REFERENCE.md) |
| Coding standards | [CLAUDE.md — Coding Standards](../../CLAUDE.md#coding-standards) |
| Full contributor guide | [CONTRIBUTING.md](../../CONTRIBUTING.md) |
| Debug the LSP server | [DEBUGGING_LSP_SERVER.md](DEBUGGING_LSP_SERVER.md) |

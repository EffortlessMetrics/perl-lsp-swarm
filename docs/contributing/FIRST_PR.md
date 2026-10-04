# Your First PR

Five minutes to read. Then clone and go.

## Repository context

[`EffortlessMetrics/perl-lsp`](https://github.com/EffortlessMetrics/perl-lsp) on
`master` is the public contributor surface. External contributors open issues and pull
requests there.

[`EffortlessMetrics/perl-lsp-swarm`](https://github.com/EffortlessMetrics/perl-lsp-swarm)
on `main` remains the active high-throughput product-development and product-proof
authority. That does not make swarm the public PR queue.

Before a shared-product public pull request is integrated, a maintainer first converges
accepted swarm state into `perl-lsp/master`, then reconciles and proves the contributor
pull request against that current public-source base. After the public pull request
merges, its shared-product effect is reverse-converged into swarm. You do not need to
open a second pull request against swarm.

Release and package-channel availability remain later states; neither a swarm merge nor a
public-source merge proves publication.

## 1. Clone and branch

Clone the public contribution repository and create your branch:

```bash
git clone https://github.com/EffortlessMetrics/perl-lsp.git
cd perl-lsp
git switch -c fix/short-description
```

No Git submodules need initialization. A normal clone is enough. The checked-in
`tree-sitter-perl/` directory is legacy source, not a submodule, and is outside the
default Rust build.

## 2. Set up the development environment

With Nix, use the reproducible environment:

```bash
nix develop
just devex
just doctor
```

Without Nix, install the pinned Rust toolchain through
[rustup](https://rustup.rs/), install `just`, and run the same checks:

```bash
# Install rustup first if the toolchain manager is not present
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
# The installer does not modify the shell that ran it
source "$HOME/.cargo/env"

rustup show
cargo install just
just devex
just doctor
```

The repository pins Rust channel `1.95.0` and currently requires MSRV 1.95.

On Windows, prefer `cargo xtask fmt` or package-scoped `cargo fmt`; bare
`cargo fmt --all` can exceed the `CreateProcessW` command-line limit in this workspace.
Symlink-rejection tests print a visible skip when the session lacks
`SeCreateSymbolicLinkPrivilege`; enabling Developer Mode opts the machine into those
tests but is not required for ordinary contribution work.

Install the pre-push hook when you want the fast gate before every push:

```bash
bash scripts/install-githooks.sh
```

## 3. Choose one issue and one claim

Start from an existing public issue when one owns the work. Keep one pull request centered
on:

- one problem or improvement;
- one semantic owner;
- one observable acceptance condition;
- proof that can distinguish the intended change from a realistic wrong result;
- explicit non-goals and a bounded rollback.

Browse the public contribution backlog:

```bash
gh issue list --repo EffortlessMetrics/perl-lsp --state open --limit 10
```

Choose an issue with a clear, reviewable acceptance section. Swarm-only release,
orchestration, and internal control-plane work is not a first-contribution queue; if work
is only tracked internally, ask for a public contribution issue rather than targeting
swarm directly.

## 4. Read the local owner guidance

Before editing a crate, read its nearest `AGENTS.md` or `CLAUDE.md`, then inspect the
owning tests and public seams. The root [`AGENTS.md`](../../AGENTS.md) and
[`CLAUDE.md`](../../CLAUDE.md) are route maps; package-local guidance owns local
constraints.

For agentic coding environments, also read the
[agent contributing guide](../how-to/AGENT_CONTRIBUTING.md).

## 5. Reproduce or define the behavior

For a defect, start with the smallest observable reproduction. For new behavior, add the
smallest test or fixture that separates the intended result from a plausible wrong one.
Use an external oracle when the claim depends on Perl semantics, LSP or DAP protocol
behavior, or a third-party contract.

Then implement the smallest coherent change. Production code must not introduce
`unwrap`, `expect`, `panic!`, `todo!`, `unimplemented!`, `abort`, or `dbg!` outside a
documented narrow exception.

## 6. Run focused proof first

Test the owning package or seam before escalating to broader gates:

```bash
cargo fmt -p <package> -- --check
cargo clippy -p <package> --all-targets --locked -- -D warnings
cargo test -p <package> --all-targets --locked
```

Then run the repository gate selected by the change:

```bash
just pr-fast
# or, in the reproducible environment
nix develop -c just ci-gate
```

Do not run the full workspace after every edit. Escalate when dependency reach, risk,
public surface, or the merge gate requires it.

Before committing, inspect exactly what will be published:

```bash
git status --short --branch
git diff --check
git diff
```

Stage intended paths explicitly. Do not use `git stash` in a multi-worktree repository;
stash state is shared across worktrees.

## 7. Commit, push, and open the pull request

Fork the public repository, push your branch, and target `perl-lsp/master`:

```bash
git add <files>
git commit -m "fix(scope): describe the change (#NNNN)"
gh repo fork EffortlessMetrics/perl-lsp --remote --remote-name fork
git push -u fork HEAD
gh pr create --repo EffortlessMetrics/perl-lsp --base master
```

Use a conventional title with the controlling public issue number:

```text
fix(perl-module): normalize path separators (#4154)
docs(contributing): correct the current workflow (#4155)
test(perl-parser): add a postfix-deref regression (#4156)
```

The pull-request body should state the claim, controlling issue, changed seam, proof run,
proof not run, risk and rollback, non-goals, and any `NOT_PROVEN` boundary.

Do not open a duplicate pull request against `perl-lsp-swarm`. Repository convergence
around a shared-product contribution is maintainer-owned.

## 8. Review and integration

Review is semantic and cumulative, not a fixed two-pass conveyor. Reviewers challenge the
claim, proof discrimination, production reachability, semantic ownership, risk, and
rollback in proportion to the change.

For a shared-product public pull request, maintainers first bring accepted
`perl-lsp-swarm/main` state into `perl-lsp/master` before integration. If that changes
the interaction surface, the pull request is reconciled and the affected proof is rerun;
the contributor is not asked to retarget swarm. After merge, maintainers reverse-converge
the shared-product effect into swarm.

Labels can help navigation, but they are not proof or merge permission. Current submitted
reviews, unresolved findings, required checks, mergeability, rulesets, and applicable
release policy govern integration. A green check or bot label by itself does not make a
pull request ready to merge.

When a finding changes the candidate materially, repair the affected seam and rerun the
proof that can falsify it. Formatting or unrelated generated movement does not require
restarting every prior judgment merely because the commit SHA changed.

## Reference

| Need | Command or document |
| --- | --- |
| New checkout health | `just doctor` |
| Tool and environment check | `just devex` |
| Fast pull-request loop | `just pr-fast` |
| Full local merge gate | `just ci-gate` |
| Agent-safe compile/test/lint | `just agent-check`, `just agent-test`, `just agent-clippy` |
| Parser or generated status | `just status-update`, then `just status-check` |
| Public API documentation | `just ci-docs-check`, then `just docs-verify` |
| Contributor-topology projection | `cargo run --locked -p xtask --bin contributor-topology` |
| All commands | [Commands reference](../reference/COMMANDS_REFERENCE.md) |
| Full contributor guide | [CONTRIBUTING.md](../../CONTRIBUTING.md) |
| Debug the LSP server | [DEBUGGING_LSP_SERVER.md](DEBUGGING_LSP_SERVER.md) |
| Repository and product identity | [Product identity](../reference/product-identity.md) |

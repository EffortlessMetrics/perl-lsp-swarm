# Contributing to Perl LSP

Thank you for contributing to Perl LSP. The project is a public beta: the core editor
experience is useful, but APIs and behavior may still change between minor releases.
See [STABILITY.md](docs/reference/STABILITY.md) for the supported compatibility boundary.

This guide covers the ordinary contributor path. Agentic environments should also read
the [agent contributing guide](docs/how-to/AGENT_CONTRIBUTING.md), which points to the
provider-native repository contracts without creating a second workflow.

## Quick start

```bash
git clone https://github.com/EffortlessMetrics/perl-lsp.git
cd perl-lsp

# Recommended reproducible environment
nix develop

# Verify tools and repository health
just devex
just doctor

# Fast local proof
just pr-fast
```

Without Nix, install the pinned Rust toolchain through
[rustup](https://rustup.rs/) and install `just`:

```bash
rustup show
cargo install just
just devex
just pr-fast
```

The repository pins Rust channel `1.95.0` in `rust-toolchain.toml` and currently
requires MSRV 1.95.

## Choose one coherent claim

A useful pull request has:

- one problem or improvement;
- one semantic owner;
- an observable acceptance condition;
- proof that can distinguish the intended change from a realistic wrong one;
- explicit non-goals;
- a bounded rollback or cleanup path.

Start from an existing issue when one owns the work. Create or update an issue when the
problem, scope, proof seam, or authority would otherwise be lost between sessions. Do
not bundle adjacent work merely because it touches nearby files.

## Current repository map

The workspace contains many internal modules and crates. These are the main contributor
entrypoints:

| Surface | Path | Role |
| --- | --- | --- |
| Parser | `crates/perl-parser/` | Native Perl parser and recovery behavior |
| Compiler and semantic facts | `crates/perl-semantic-analyzer/`, `crates/perl-semantic-facts/` | Semantic analysis and compiler-facing facts |
| LSP core | `crates/perl-lsp-rs-core/` | Protocol, runtime, workspace, and provider implementation |
| LSP integration | `crates/perl-lsp-rs/` | Server integration and higher-level behavior |
| LSP executable | `crates/perllsp/` | User-facing language-server binary |
| DAP | `crates/perl-dap/` | Debug Adapter Protocol implementation |
| Corpus and compatibility | `crates/perl-corpus/`, `crates/perl-core-harness/` | Real-Perl and upstream compatibility evidence |
| Repository tooling | `xtask/`, `scripts/`, `.ci/` | Gates, generators, policy, and proof routing |
| VS Code extension | `vscode-extension/` | Installed editor experience |

Read the nearest package-local `AGENTS.md` or `CLAUDE.md` before changing an owning
crate. The root [`AGENTS.md`](AGENTS.md) and [`CLAUDE.md`](CLAUDE.md) contain the current
repository route maps.

## Development workflow

### 1. Create a branch

```bash
git switch -c fix/short-description
```

Agents and parallel writers should use an isolated worktree. One candidate branch or
worktree has one mutation owner at a time. See
[WORKTREE_PROTOCOL.md](docs/reference/WORKTREE_PROTOCOL.md).

### 2. Reproduce or define the behavior

For a bug, start with a focused reproduction or failing test at the observable seam. For
new behavior, write the smallest test or fixture that distinguishes the intended result
from a plausible wrong implementation.

External claims need an external oracle:

- Perl semantics: run a bounded `perl -e` or upstream test against the supported Perl
  runtime;
- LSP or DAP behavior: cite and exercise the relevant protocol contract;
- third-party APIs: verify the current official documentation or implementation.

Do not approve a language or protocol claim because several reviewers independently
remember the same rule.

### 3. Implement the smallest coherent change

Keep semantic ownership and dependency direction intact. Do not add a second policy,
registry, scheduler, or state model when a current repository authority already owns the
answer.

Production code must not introduce `unwrap`, `expect`, `panic!`, `todo!`,
`unimplemented!`, `abort`, or `dbg!` outside a documented narrow exception. Prefer
`Result`, `Option`, explicit invariants, and actionable errors.

### 4. Run focused proof first

Use the cheapest command that can falsify the claim:

```bash
cargo fmt -p <package> -- --check
cargo clippy -p <package> --all-targets --locked -- -D warnings
cargo test -p <package> --all-targets --locked
```

Then run the repository gate appropriate to the change:

```bash
just pr-fast
# or, in the reproducible environment
nix develop -c just ci-gate
```

Useful command choices:

| Situation | Command |
| --- | --- |
| Tool or environment problem | `just devex` |
| Repository health or generated drift | `just doctor` |
| Fast inner loop | `just pr-fast` |
| Full local merge gate | `just ci-gate` |
| Agent compile/test/lint | `just agent-check`, `just agent-test`, `just agent-clippy` |
| Parser or generated status changed | `just status-update` then `just status-check` |
| Public API documentation changed | `just docs-check` and `just docs-report` |
| Release/version surfaces changed | `just version-check` then `just release-check` |
| Cargo manifests or publish topology changed | `just publish-dry-run` |

Do not run broad workspace proof after every edit. Escalate when the dependency graph,
risk, changed public surface, or selected merge gate requires it.

### 5. Inspect the proposed change

Before committing or pushing:

```bash
git status --short --branch
git diff --check
git diff
```

Stage intended paths explicitly. Do not use `git stash` in a multi-worktree repository;
stash is shared across worktrees. Use scoped restore or a branch-local WIP commit.

### 6. Open the pull request

Use a conventional title with the controlling issue number:

```text
fix(parser): handle heredoc in ternary context (#1234)
feat(lsp): add bounded provider behavior (#1234)
docs(contributing): correct the current workflow (#1234)
```

The body should state:

- the claim and controlling issue;
- root cause or design reason;
- changed behavior and important seams;
- proof run and proof not run;
- risk and rollback;
- explicit non-goals;
- any `NOT_PROVEN` boundary.

Follow the repository pull-request template. Do not claim that a local command, earlier
candidate, different platform, or fixture proves a surface it did not exercise.

## Review and integration

Pull requests receive substantive provider-native review and live GitHub integration
checks. There is no fixed two-model review ladder, permanent named-agent roster, or
lifecycle-label state machine.

Review is semantic and cumulative:

- a material claim, implementation, production route, authority, risk, or tested-seam
  change refreshes the affected review and proof;
- a focused repair refreshes the finding and changed seam;
- formatting, editorial cleanup, and unrelated generated refreshes do not restart the
  entire review merely because the commit SHA changed.

Labels may help navigation. They are not proof or merge permission. Current submitted
reviews, unresolved threads, change requests, required checks, mergeability, rulesets,
and applicable release policy govern integration.

A conflict-free branch does not need an update merely because `main` advanced. Behind-only
movement requires no action. Rebase, merge-main, retarget, cherry-pick, or reconstruction
is selected when a real conflict, same-seam interaction, explicit stack, policy, or other
concrete integration reason makes it useful.

Required GitHub statuses remain attached to the commit they actually evaluated. If a
required run is pending on the current head, let it report or request a same-head rerun
where supported. Do not push an empty commit or rebase solely to manufacture status.

At merge, the current head is used as compare-and-swap protection; it is not the review
verdict.

## Documentation and generated state

Generated status, dashboards, ledgers, and manifests must be regenerated through their
owning command rather than hand-edited. The applicable check should produce no diff on a
second run.

For documentation-only changes, verify links and run the narrow docs checks selected by
CI. Historical or forensic records may preserve old terminology and branch names; current
operational guides must use current authority and `main`.

## Security and privacy

Do not commit secrets, access tokens, personal data, private corpus material, or raw
receipts containing sensitive paths or content. Use repository redaction and fixture
patterns. Security-sensitive changes need realistic negative controls and fail-closed
behavior where the claim requires it.

Report security concerns through the process in
[SECURITY.md](SECURITY.md). Conduct expectations are defined in
[CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md).

## After merge

A squash merge is not the end of the claim. Verify the landed effect, update the owning
issue with what changed and what remains, preserve residual work, and remove only the
branch, worktree, or scratch state created by the lane when it is safe.

Do not delete dirty, unpushed, ambiguous, or salvageable work. The pull request, reviews,
checks, issue, and landed commit are the durable record; runtime agent state is not.

## Where to ask or start

- [Open issues](https://github.com/EffortlessMetrics/perl-lsp/issues)
- [Project roadmap](docs/project/ROADMAP.md)
- [Commands reference](docs/reference/COMMANDS_REFERENCE.md)
- [Architecture reference](docs/reference/ARCHITECTURE.md)
- [Agent contributing guide](docs/how-to/AGENT_CONTRIBUTING.md)
- [Debugging the LSP server](docs/contributing/DEBUGGING_LSP_SERVER.md)

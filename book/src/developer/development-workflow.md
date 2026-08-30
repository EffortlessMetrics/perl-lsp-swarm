# Development Guide

> Working in the perl-lsp development repository.

This page orients contributors in the development repository and routes to the current
authorities rather than restating them. The canonical contributor path — environment
setup, claim selection, proof expectations, and the pull-request flow — is
[CONTRIBUTING.md](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/CONTRIBUTING.md). Read that first; this page covers what is
specific to working inside the development repository itself.

---

## Which repository this is

Development happens in
[`EffortlessMetrics/perl-lsp-swarm`](https://github.com/EffortlessMetrics/perl-lsp-swarm)
on `main`. Clone this repository, open development issues here, and target pull requests
here.

[`EffortlessMetrics/perl-lsp`](https://github.com/EffortlessMetrics/perl-lsp) on `master`
owns public release lineage and published artifacts. A merge to `perl-lsp-swarm/main` is
development state; it does not establish that a change reached a release, package
registry, editor marketplace, or any other public channel. The relationship is defined by
[product identity](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/docs/reference/product-identity.md).

Installing perl-lsp as a user is a different route from developing it. User installation
lives in
[Getting Started](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/docs/tutorials/GETTING_STARTED.md);
nothing on this page is installation guidance.

---

## Quick start

```bash
git clone https://github.com/EffortlessMetrics/perl-lsp-swarm.git
cd perl-lsp-swarm

# Reproducible environment (recommended)
nix develop

# Verify tooling and repository health
just devex
just doctor

# Fast local proof
just pr-fast
```

Without Nix, install the pinned toolchain through [rustup](https://rustup.rs/) and
`cargo install just`. The repository pins Rust channel `1.95.0` in `rust-toolchain.toml`
and currently requires MSRV 1.95.

---

## Where the authorities are

This guide deliberately does not duplicate these. Follow the link for the current answer.

| Question | Current authority |
| --- | --- |
| Contributor workflow, claim shape, PR expectations | [CONTRIBUTING.md](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/CONTRIBUTING.md) |
| Command inventory | [COMMANDS_REFERENCE.md](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/docs/reference/COMMANDS_REFERENCE.md), or `just --list` |
| Ownership seams and dependency direction | [ARCHITECTURE.md](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/docs/reference/ARCHITECTURE.md) |
| Exact workspace membership | [Cargo.toml](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/Cargo.toml) |
| Capability claims | [features.toml](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/features.toml) |
| Current subsystem status | [Current status](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/docs/project/CURRENT_STATUS.md) |
| Project orientation | [Project orientation](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/docs/project/ORIENTATION.md) |
| LSP feature implementation | [LSP Development Guide](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/docs/tutorials/LSP_DEVELOPMENT_GUIDE.md) |
| Agent and route contracts | [AGENTS.md](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/AGENTS.md), [CLAUDE.md](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/CLAUDE.md) |

---

## The inner loop

Run the cheapest command that can falsify the change, then widen only as the changed
surface requires:

```bash
cargo fmt -p <package> -- --check
cargo clippy -p <package> --all-targets --locked -- -D warnings
cargo test -p <package> --all-targets --locked
just pr-fast
```

`just ci-gate` — or `nix develop -c just ci-gate` — is the fuller local merge gate. Do not
run workspace-wide clippy or tests after every edit; escalate when the dependency graph,
risk, changed public surface, or the selected merge gate calls for it.
[CONTRIBUTING.md](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/CONTRIBUTING.md) carries the situation-to-command table for
generated status, docs, release, and publish surfaces.

Production code must not introduce `unwrap`, `expect`, `panic!`, `todo!`,
`unimplemented!`, `abort`, or `dbg!` outside a documented narrow exception.

---

## Where changes go

Read the nearest package-local `CLAUDE.md` or `AGENTS.md` before modifying an owning
crate. [ARCHITECTURE.md](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/docs/reference/ARCHITECTURE.md) owns the full seam map; the
entrypoints contributors reach for most often are:

| Change | Crate |
| --- | --- |
| Lexing and tokens | `crates/perl-lexer/`, `crates/perl-token/` |
| Parsing, positions, trivia, recovery boundaries | `crates/perl-parser-core/` |
| Public parser facade | `crates/perl-parser/` |
| AST node types | `crates/perl-ast/` |
| Semantic analysis and compiler facts | `crates/perl-semantic-analyzer/`, `crates/perl-semantic-facts/` |
| LSP protocol, runtime, providers | `crates/perl-lsp-rs-core/` |
| Server integration facade | `crates/perl-lsp-rs/` |
| Published language-server binary | `crates/perllsp/` |
| Debug Adapter Protocol | `crates/perl-dap/` |
| Gates, generators, policy, proof routing | `xtask/`, `scripts/`, `.ci/` |
| Installed editor experience | `vscode-extension/` |

---

## Parser corpus work

Parser accuracy is measured against a real-Perl corpus and locked behind committed
baselines:

```bash
just corpus-sweep          # measure against the system Perl corpus
just common-corpus-check   # strict pinned-module check
just corpus-sweep-update   # lock a new .ci/parser-corpus-baseline.json
```

The baselines are ratchets. Re-measure and lock a new baseline after a parser improvement
lands — never widen a baseline to turn a red sweep green.

---

## Agent-driven development

Much of this repository is developed by agents working one coherent claim at a time. The
current contracts are the repository-root
[AGENTS.md](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/AGENTS.md) and
[CLAUDE.md](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/CLAUDE.md), with
provider-native procedures under `.claude/skills/`. Agentic contributors should also read
the
[agent contributing guide](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/docs/how-to/AGENT_CONTRIBUTING.md).

Two invariants matter regardless of provider:

- one candidate branch or worktree has one mutation owner at a time — see
  [WORKTREE_PROTOCOL.md](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/docs/reference/WORKTREE_PROTOCOL.md);
- GitHub owns durable transaction state. Runtime topology, task order, liveness, and
  temporary plans are not repository state and are not written to tracked files.

---

## Historical note

Earlier revisions of this guide presented a "pure Rust" parser generated from a
`grammar.pest` under `crates/tree-sitter-perl-rs/src/` as the production parser, and told
contributors to build and run it through a `pure-rust` cargo feature and a `parse-rust`
binary. Those instructions no longer work: neither that feature nor that binary target
exists in the workspace today, and `crates/tree-sitter-perl-rs/` is now a published
tree-sitter-compatible facade rather than a Pest parser.

Where that content actually went:

- the production parser is the native parser in `crates/perl-parser-core/`, behind the
  `crates/perl-parser/` facade;
- the Pest grammar and its parser survive as `crates/perl-parser-pest`, kept deliberately
  as a comparison instrument, compatibility reference, and benchmark baseline — it is not
  the production parser, an LSP fallback, or gate authority;
- the retired edge-case handler and the pre-split parser crate are under `archive/`, which
  is excluded from the workspace;
- the legacy C grammar under `tree-sitter-perl/` is also excluded from the workspace and
  is reference material only.

---

*Questions and corrections belong in a repository issue.*

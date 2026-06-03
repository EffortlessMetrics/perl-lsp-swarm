# Upstream Tooling Substrate

> **Context**: This document is part of perl-lsp's
> [Industrialized AI](why-industrialized.md) CI architecture. It defines the
> small upstream engine room that repo-owned `xtask` and policy wrappers are
> allowed to build on.
>
> **Doctrine**: Do not make upstream tools the repository's public control
> surface. Make `xtask` the repo surface; make upstream tools the engine room.

This repository should standardize on a small substrate of upstream tools and
hide day-to-day policy behind stable repo-shaped wrappers. Contributors and
agents should remember `cargo xtask ...`, `just ...`, and the policy ledgers —
not every upstream command-line flag.

The goal is stronger proof per minute: upstream tools provide specialized
engines, while repo-owned wrappers encode routing, receipts, exception policy,
and escalation rules.

---

## Control-plane rule

```text
repo policy / orchestration: xtask
source exception ledger:     cargo-allow
static mutation exposure:    ripr
unsafe-contract review:      unsafe-review
upstream substrates:         small, pinned, wrapped engines
```

Rules:

1. Upstream tools may be replaced or upgraded without changing the repo-facing
   command contract.
2. Every default-PR tool must have a bounded proof obligation and a receipt or
   reviewable output.
3. Expensive runtime backstops are routed by risk, nightly, or release lanes;
   they are not default taxes on every ordinary PR.
4. Exceptions live in dated ledgers rather than in scattered scripts or workflow
   comments.

---

## Standard substrate table

| Plane | Standard upstream tools | Repo role |
| --- | --- | --- |
| Syntax / codemod | [`ast-grep`](https://ast-grep.github.io/), rust-analyzer crates | Structural candidate discovery; Rust-aware authority when identity must survive refactors. |
| Workspace graph | [`cargo_metadata`](https://docs.rs/cargo_metadata/latest/cargo_metadata/), [`guppy`](https://docs.rs/guppy/latest/guppy/) | Package inventory, reverse-dependency closure, CI/risk-pack planning. |
| Test execution | [`cargo-nextest`](https://nexte.st/), `cargo test --doc` | Fast default Rust tests plus separate doctest coverage. |
| Coverage | [`cargo-llvm-cov`](https://github.com/taiki-e/cargo-llvm-cov) | Execution-surface evidence, coverage artifacts, release snapshots. |
| Mutation | [`ripr`](ripr.md), [`cargo-mutants`](https://mutants.rs/) | PR-time static mutation-exposure plus targeted/nightly/release runtime backstop. |
| Unsafe / UB | `unsafe-review`, [Miri](https://github.com/rust-lang/miri) | Reviewable unsafe contracts plus targeted concrete UB witnesses. |
| Source exceptions | `cargo-allow` | Dated source/workflow/dependency exception receipts. |
| Dependency trust | [`cargo-deny`](https://embarkstudios.github.io/cargo-deny/), [`cargo-vet`](https://mozilla.github.io/cargo-vet/), [RustSec / `cargo-audit`](https://rustsec.org/), `cargo-auditable` | License, advisory, source, audit, and shipped-binary dependency evidence. |
| Public API / release | [`cargo-semver-checks`](https://docs.rs/cargo-semver-checks/latest/cargo_semver_checks/), rustdoc JSON | Release compatibility gates and custom public-surface reports. |
| Workflow policy | [`actionlint`](https://github.com/rhysd/actionlint), `zizmor` | Workflow syntax/semantic checks plus security posture. |
| Text/config hygiene | [`taplo`](https://taplo.tamasfe.dev/), [`typos`](https://github.com/crate-ci/typos), Markdown link/style tooling | TOML, spelling, Markdown, and link hygiene after dictionaries/baselines settle. |
| Workspace hygiene | `cargo-udeps`, `cargo-hakari` where justified | Scheduled unused-dependency checks; feature unification only when duplicate-build pain is measured. |
| CI cache | [`Swatinem/rust-cache`](https://github.com/Swatinem/rust-cache), `sccache` where justified | Default Cargo cache policy; remote compiler cache only when economics justify it. |

---

## Authority boundaries

### Syntax candidates are not semantic proof

`ast-grep` is the default structural search and codemod substrate for polyglot
source scans, workflow/source pattern linting, unsafe-review candidate
generation, and agent worklists. It should find candidates quickly.

For exact Rust policy, use Rust-aware data:

```text
ast-grep finds candidates.
Rust-aware tooling decides authority.
```

Examples that need Rust-aware authority include public API facts, exact
panic-family selector identity, durable source suppressions, and call-site
classification.

### Workspace graph planning uses Cargo data

Use `cargo_metadata` for basic workspace, package, and target inventory. Use
`guppy` when the question is graph-shaped: changed crate to reverse dependency
closure, feature graph routing, publish graph, or risk-pack expansion.

File inventory for source policy starts from tracked files:

```bash
git ls-files -z
```

Use `ignore` or `walkdir` only for tools that intentionally inspect beyond
Git-tracked state.

### Coverage is evidence, not adequacy

`cargo-llvm-cov` coverage receipts describe execution surface. They do not claim
that tests have good assertions or that a release is ready. Use coverage to
route deeper proof, not to replace oracle-quality checks.

### Mutation has two layers

`ripr` is PR-time static mutation-exposure analysis. It shifts weak-oracle
signal left and should produce review packets/receipts for changed Rust
behavior. Runtime mutation remains valuable, but `cargo-mutants` belongs in
risk-triggered, nightly, and release lanes rather than every ordinary PR.

### Unsafe review has two questions

`unsafe-review` asks whether an unsafe seam is reviewable: contract, guard, test
reach, and witness route. Miri asks a concrete runtime UB question for selected
executions. Do not present either as a complete memory-safety proof.

---

## Repo-facing wrapper contract

Stable wrappers are the public control surface. The specific upstream tool,
flags, or artifact layout can evolve behind these commands.

| Wrapper | Responsibility |
| --- | --- |
| `cargo xtask check-pr` | Repo-shaped default PR policy bundle. |
| `cargo xtask fix-pr` | Safe local auto-fixes for the default PR surface. |
| `cargo xtask pr-summary` | Human/agent summary of relevant proof and receipts. |
| `cargo xtask allow-check` / `cargo xtask allow-diff` | Source exception ledger validation and changed-exception review. |
| `cargo xtask ripr-pr` | Diff-scoped static mutation-exposure packet. |
| `cargo xtask unsafe-review-pr` | Unsafe-contract review packet. |
| `cargo xtask test-pr` / `cargo xtask test-docs` | PR tests and doctests through the repo's selected engines. |
| `cargo xtask coverage` | Coverage receipt/artifact generation. |
| `cargo xtask mutation-targeted` | Risk-scoped runtime mutation backstop. |
| `cargo xtask miri-targeted` | Risk-scoped concrete UB witness lane. |
| `cargo xtask check-deps` / `cargo xtask check-supply-chain` | Dependency license/advisory/audit gates. |
| `cargo xtask semver-check` | Public API compatibility gate. |
| `cargo xtask check-workflows` | Workflow syntax, security, and policy lint surface. |
| `cargo xtask check-toml` | TOML formatting/linting surface. |
| `cargo xtask policy-report` | Aggregated policy and exception receipt report. |

If a wrapper is not implemented yet, new work should add the wrapper before
teaching contributors or CI to call the upstream tool directly.

---

## Default versus routed lanes

Default PR lanes should stay cheap, deterministic, and receipt-producing:

- Rust formatting and Clippy policy.
- Fast relevant tests through the repo test wrapper.
- Diff-scoped `ripr` evidence for Rust behavior changes.
- Source exception and dependency policy checks that are already baselined.
- Workflow/TOML/text checks after their dictionaries and ledgers are stable.

Routed lanes require a risk trigger, schedule, or release context:

- Broad `cargo-mutants` runs.
- Broad Miri runs.
- `cargo-udeps` nightly-only dependency hygiene.
- `cargo-hakari` adoption for measured duplicate-build pain.
- `sccache` or remote compiler cache infrastructure.
- Full prose linting beyond broken links and clear Markdown structure.

This keeps the repository contract stable while preserving room for heavier
proof where it has evidence value.

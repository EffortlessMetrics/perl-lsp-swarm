# perl-lsp-rs

LSP server implementation and integration tests.

## Authority and scope

The repository-root `CLAUDE.md` and applicable `AGENTS.md` own routes, orchestration,
review, proof currentness, and result vocabulary. Current source, manifests, tests, and
generated contracts own the exact API, dependency, and module inventory. This file
carries only package-local runtime, test-harness, and include-root hazards; it narrows
but does not override the root control plane.

Keep this file durable. Update it when a local semantic invariant, ownership boundary,
failure mode, or proof route changes. Do not add workspace versions, dependency lists,
exhaustive module maps, or temporary task state.

## Test threading

Always preserve the package's bounded test-thread contract:

```bash
RUST_TEST_THREADS=2 cargo test -p perl-lsp-rs -- --test-threads=2
```

Do not cite an unconstrained package test run as equivalent evidence.

## Server construction and test harness

- `LspServer` uses interior mutability; construct `let server`, not `let mut server`,
  unless current source proves a distinct mutable binding is required.
- The test harness shares state through `Arc<Mutex<_>>`. Do not reintroduce outer
  mutable-server ownership to work around an interior-mutability seam.

## Include-root semantics: `.` is a wildcard, not a path

When filtering workspace-symbol candidates through `EffectiveIncContext`, treat the
workspace-root `.` entry distinctly from configured-relative or lexical-`use lib`
entries. If `.` is treated like any other include root, almost every workspace file
becomes reachable and the filter cannot reject, for example, `lib/GoneModule.pm` after
`no lib 'lib'`.

Sources of include roots are semantically distinct; do not collapse them to "path
roots":

- `WorkspaceDefaultDot` — `.` from the workspace folder. It is a wildcard for
  everything under the workspace and is not subject to `no lib` cancellation.
- `WorkspaceConfiguredRelative` — explicit `additionalIncPaths` entries from config,
  such as `lib` or `t/lib`.
- `LexicalUseLib` — `use lib '...'` from the source under analysis. It is
  position-scoped and subject to downstream `no lib '...'` cancellation.
- `Perl5LibEnv` — `PERL5LIB` from the LSP process or `usePerl5lib` config.
- `InterpreterStartup` — output of the `perl -e 'print @INC'` probe under the
  subprocess-oracle contract.

When implementing a filter or routing decision over include roots, branch on the kind,
not only the path. The wildcard `.` is the common false-positive seam in
workspace-symbol filtering.

Primary seam:
`crates/perl-lsp-rs/src/runtime/lifecycle/inc_context.rs`.

## Proof routes

```bash
cargo fmt --all

# Hosted clippy_scoped / clippy_full:
cargo clippy --locked --lib -p perllsp -- -D warnings -A missing_docs

# Package-local equivalent for this crate's Cargo package name:
cargo clippy -p perl-lsp-rs --lib --locked -- -D warnings -A missing_docs

# `--tests` exposes package test-target linting separately.
cargo clippy -p perl-lsp-rs --tests

RUST_TEST_THREADS=2 cargo test -p perl-lsp-rs -- --test-threads=2
```

A package-local command proves only its stated scope. Use the applicable root route for
candidate-wide and integration evidence.

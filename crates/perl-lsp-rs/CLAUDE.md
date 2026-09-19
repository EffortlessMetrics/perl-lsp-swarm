# perl-lsp-rs

LSP server implementation and integration tests.

## Authority and scope

The checked-in repository-root `CLAUDE.md` and `AGENTS.md`, as classified by
`docs/agents/AUTHORITY_STATUS.md` and `docs/agents/authority_status.toml`, are the
current repository authority for routes, orchestration, review, proof currentness, and
result vocabulary. Current source, manifests, tests, and generated contracts own the
exact API, dependency, and module inventory. This file carries only package-local
runtime, test-harness, and include-root hazards; it does not establish a competing
repository contract.

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

Sources of include roots are semantically distinct, but the current
`perl_module::IncRootKind` has one workspace-relative variant rather than separate
default and configured variants:

- `WorkspaceRelative` — relative configured include paths, including the default
  `lib`, `.`, and `local/lib/perl5` entries. The `.` entry is not a separate enum
  variant; when it resolves to the workspace root, `EffectiveIncContext` treats it
  as a wildcard and excludes it from the direct module-file reachability check.
- `FileLocalLexical` — a `use lib` path from the source under analysis after lexical
  resolution. It is position-scoped and subject to `no lib` cancellation before effective
  roots are assembled. A workspace-contained absolute lexical path is normalized to a
  workspace-relative path and represented here; an absolute lexical path outside the
  workspace is rejected by the resolver.
- `ExternalAbsolute` — an absolute configured include path already admitted by the
  upstream configuration boundary. Lexical paths are normalized or rejected before they
  reach this effective-root classification.
- `Perl5LibEnv` — a `PERL5LIB` entry when `use_perl5lib` is enabled.
- `InterpreterStartup` — an entry returned by the selected interpreter's startup
  `@INC` probe when `use_system_inc` is enabled.
- `RuntimeDerived` — reserved for a future trusted runtime source; the current
  effective-root builder does not produce it.

When implementing a filter or routing decision over include roots, preserve the kind
and source label alongside the path. Do not invent a separate kind for `.`, and use the
resolved workspace-root path when handling its wildcard behavior. The wildcard `.` is
the common false-positive seam in workspace-symbol filtering.

Primary seam:
`crates/perl-lsp-rs/src/runtime/lifecycle/inc_context/mod.rs`.

## Proof routes

```bash
# Non-mutating package-scoped formatting check:
cargo fmt -p perl-lsp-rs -- --check

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

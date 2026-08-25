# perl-lsp

LSP server binary and integration tests.

## Test Threading
ALWAYS use threading constraints:
```bash
RUST_TEST_THREADS=2 cargo test -p perl-lsp-rs -- --test-threads=2
```

## After Async Migration
- LspServer no longer needs &mut self — use `let server` not `let mut server`
- Test harness uses interior mutability (Arc<Mutex>)

## Verify
```bash
cargo fmt --all
# Hosted clippy_scoped / clippy_full:
#   cargo clippy --locked --lib -p perllsp -- -D warnings -A missing_docs
# Package-local equivalent (same lib; this crate's Cargo package name):
cargo clippy -p perl-lsp-rs --lib --locked --no-deps -- -D warnings -A missing_docs
# `--all-targets` is the product subject (#9600). `--tests` is not a substitute:
# it omits benches and hides --lib unused-import findings (#9618).
# `build.rs` is a compile prerequisite of `--lib` / `--tests` / `--all-targets`,
# not an `--all-targets`-only subject.
cargo clippy -p perl-lsp-rs --all-targets --locked --no-deps -- -D warnings -A missing_docs
RUST_TEST_THREADS=2 cargo test -p perl-lsp-rs -- --test-threads=2
```

## Include-root semantics: `.` is a wildcard, not a path

When filtering workspace-symbol candidates through `EffectiveIncContext`, treat the
workspace-root `.` entry distinctly from configured-relative or lexical-`use lib`
entries. If `.` is treated like any other include root, almost every workspace file
becomes reachable and the filter cannot reject e.g. `lib/GoneModule.pm` after
`no lib 'lib'`.

Sources of include roots — semantically distinct, do NOT collapse to "path roots":

- `WorkspaceDefaultDot` — `.` from the workspace folder. Acts as a wildcard for
  everything under the workspace; not subject to `no lib` cancellation.
- `WorkspaceConfiguredRelative` — explicit `additionalIncPaths` entries from
  config (e.g. `lib`, `t/lib`).
- `LexicalUseLib` — `use lib '...'` from the source under analysis. Position-
  scoped; subject to `no lib '...'` cancellation downstream of the cancel point.
- `Perl5LibEnv` — `PERL5LIB` from the LSP process / `usePerl5lib` config.
- `InterpreterStartup` — output of the `perl -e 'print @INC'` probe (via the
  subprocess oracle contract — see #8551).

When implementing a new filter or routing decision over include roots, branch on
the kind, not on the path. The wildcard `.` is the most common source of
false-positives in workspace-symbol filtering.

Related: `crates/perl-lsp-rs/src/runtime/lifecycle/inc_context.rs`.

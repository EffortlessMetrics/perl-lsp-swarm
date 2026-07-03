# perl-workspace-core

The **LSP-free substrate of deterministic project-level Perl facts.**

This crate sits *below* LSP, DAP, editor transport, and any shipped-product
runtime. It consumes parser / semantic / symbol / module / range primitives and
produces stable *project facts* — files, packages, symbols, modules, imports,
exports, POD, tests, dist metadata — each with a source range, provenance,
confidence, and deterministic identity where applicable.

Other lanes (DAP, critic, tidy, RIPR, Test2, Kwalitee, tree-sitter-compatible
output) **consume** these facts. They do not define the substrate.

## Invariants

- **Native ships; external tools compare.** No editor/tool runtime dependencies.
- **No editor/runtime deps.** Never depends on `perl-lsp-rs`, `perl-lsp-rs-core`,
  `perllsp`, `perl-dap`, `lsp-types`, `tokio`, `tower-lsp`, or perltidy/perlcritic
  adapters. Enforced by `tests/dependency_contract.rs`.
- **Byte/UTF-8 ranges only.** UTF-16 conversion stays at the LSP boundary;
  core ranges are `SourceRange` byte offsets.
- **Repo-relative paths only.** Host absolute paths are rejected at the
  `RepoRelativePath` boundary.
- **No faked certainty.** Runtime/dynamic Perl behaviour is marked with a
  `DynamicBoundary` or `ModelLimitation` and lowered `Confidence`, never
  papered over.
- **Deterministic identity.** `file_id_for` and `SymbolId::derive` derive IDs
  from stable coordinates, reproducible across runs and machines.

## Status

This is the **skeleton** crate: core primitives plus the mechanically enforced
dependency contract. Fact producers and the query API (`packages_in_file`,
`symbols_in_file`, `imports_in_file`, `resolve_module`, `owner_at`, ...) land in
follow-up PRs, layered by fact class and gated by `FactClasses`.

## Verify

```bash
cargo test -p perl-workspace-core --locked
cargo clippy -p perl-workspace-core --lib --locked -- -D warnings -A missing_docs
cargo tree -p perl-workspace-core   # must contain none of the forbidden crates
```

## License

Licensed under either of MIT or Apache-2.0 at your option.

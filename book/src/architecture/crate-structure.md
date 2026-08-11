# Crate structure — current-source pointer

The former v0.8.8 crate inventory is retained only so existing book links do not break. It described a different workspace, included obsolete microcrate names, and presented unsupported coverage, latency, and readiness claims.

For the current architecture, use:

- [Architecture Overview](../../docs/reference/ARCHITECTURE.md) for the contributor-facing system and data-flow description;
- [the workspace manifest](../../Cargo.toml) for exact membership, exclusions, and publish policy;
- [perl-parser-core](../../crates/perl-parser-core/README.md) and [perl-parser](../../crates/perl-parser/README.md) for parser boundaries;
- [perl-lsp-rs-core](../../crates/perl-lsp-rs-core/README.md) and [perl-lsp-rs](../../crates/perl-lsp-rs/README.md) for the LSP runtime and server facade.

Do not treat the retired inventory's crate names, version labels, benchmark figures, feature percentages, or “GA” status as current truth. Keep measured claims in the current status and benchmark documents that carry their evidence.

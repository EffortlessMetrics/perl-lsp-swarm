# LSP crate separation guide — current-source pointer

This page describes a retired v0.8.8 architecture. It mixed parser and LSP responsibilities, named absorbed provider crates as current, and gave migration commands and quality claims that are not the present workspace contract.

Use the current sources instead:

- [Architecture Overview](../reference/ARCHITECTURE.md) — current parser, semantic, workspace, and LSP data flow;
- [Cargo.toml](../../Cargo.toml) — authoritative workspace members and absorbed-crate comments;
- [perl-parser-core README](../../crates/perl-parser-core/README.md) — parser-core ownership;
- [perl-parser README](../../crates/perl-parser/README.md) — higher-level parsing facade;
- [perl-lsp-rs-core README](../../crates/perl-lsp-rs-core/README.md) — consolidated LSP core;
- [perl-lsp-rs README](../../crates/perl-lsp-rs/README.md) — server implementation and embedding boundary;
- [LSP contribution guide](../how-to/CONTRIBUTING_LSP.md) — contributor workflow for LSP changes.

The former migration examples, crate paths, parser/LSP ownership table, and “zero warnings” or coverage claims are historical. Do not copy them into new documentation or use them to choose an implementation seam.

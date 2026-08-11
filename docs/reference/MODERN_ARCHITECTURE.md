# Architecture guide — current-source pointer

This page is retained as a compatibility entry point for older links. Its former lexer/parser comparison described an abandoned two-crate design, historical benchmark estimates, and APIs that are not the current workspace contract.

Use these authorities for current architecture work:

- [ARCHITECTURE.md](ARCHITECTURE.md) — contributor-facing system, crate-family, parser, semantic, workspace, and LSP flow;
- [Cargo.toml](../../Cargo.toml) — authoritative workspace membership, exclusions, and publish allowlist;
- [perl-parser-core README](../../crates/perl-parser-core/README.md) — low-level parser-core boundary and current entry points;
- [perl-parser README](../../crates/perl-parser/README.md) — higher-level parser facade;
- [perl-lsp-rs-core README](../../crates/perl-lsp-rs-core/README.md) — consolidated LSP runtime boundary;
- [perl-lsp-rs README](../../crates/perl-lsp-rs/README.md) — server implementation package and public install path.

Do not use the retired document's crate names, version labels, performance figures, coverage percentages, estimated comparisons, or “production ready” claims as current evidence. Measured performance and compatibility status belong in the current benchmark and project-status surfaces, with their receipts.

For the rationale behind the current boundaries, read the Architecture Overview and the relevant package-local instructions before changing code.

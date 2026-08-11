# Architecture Overview — compatibility pointer

The former architecture overview mixed current ownership with retired parser, LSP, performance, and documentation-enforcement claims. It is retained as a compatibility entry point, but it is not a current authority.

Use the [current architecture reference](ARCHITECTURE.md) for contributor-facing ownership seams. Exact workspace membership, exclusions, absorbed-crate notes, and publish policy are authoritative in the [workspace manifest](../../Cargo.toml). Package-local READMEs own narrower API and implementation details.

For parser changes, start with the [perl-ast README](../../crates/perl-ast/README.md), [perl-parser-core README](../../crates/perl-parser-core/README.md), or [perl-parser README](../../crates/perl-parser/README.md), depending on the seam. For LSP and DAP changes, use the [perl-lsp-rs-core README](../../crates/perl-lsp-rs-core/README.md), [perl-lsp-rs README](../../crates/perl-lsp-rs/README.md), and [perl-dap README](../../crates/perl-dap/README.md).

Historical architecture narratives remain recoverable through Git history; they must not be used as current evidence for crate membership, coverage, performance, readiness, or documentation-enforcement claims.

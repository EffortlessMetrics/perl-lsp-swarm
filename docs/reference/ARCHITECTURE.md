# Architecture Overview for Contributors

This is the current contributor-facing architecture reference. Exact workspace membership and publish policy remain authoritative in the [workspace manifest](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/Cargo.toml); package-local READMEs own narrower API details.

## Current ownership seams

- perl-ast owns AST node types and methods.
- perl-parser-core owns low-level parsing, parse results, position/trivia infrastructure, and recovery boundaries.
- perl-parser is the public parser facade.
- perl-lsp-rs-core owns consolidated protocol, transport, runtime, configuration, governance, capability, and provider logic.
- perl-lsp-rs supplies the server implementation facade; perllsp is the public binary package.
- perl-dap owns the native Debug Adapter Protocol surface.

When changing AST nodes or methods, start in [perl-ast](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/crates/perl-ast/README.md). When changing syntax, parsing, or recovery, start in [perl-parser-core](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/crates/perl-parser-core/README.md). Use the LSP and DAP package READMEs for those respective boundaries.

Former crates may be represented by modules inside a surviving package. The absorption comments in Cargo.toml and the surviving module paths are more current than generated inventories or historical migration documents.

## Current sources

- [Workspace manifest](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/Cargo.toml)
- [perl-ast README](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/crates/perl-ast/README.md)
- [perl-parser-core README](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/crates/perl-parser-core/README.md)
- [perl-parser README](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/crates/perl-parser/README.md)
- [perl-lsp-rs-core README](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/crates/perl-lsp-rs-core/README.md)
- [perl-lsp-rs README](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/crates/perl-lsp-rs/README.md)
- [perl-dap README](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/crates/perl-dap/README.md)

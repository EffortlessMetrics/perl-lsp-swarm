# LSP crate separation — historical pointer

This document records a retired v0.8.8 architecture. It is not the current workspace contract and does not define live crate membership, provider ownership, migration commands, or support claims.

Use these current authorities instead:

- [Architecture overview](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/docs/reference/ARCHITECTURE.md)
- [Workspace manifest](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/Cargo.toml)
- [Parser-core README](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/crates/perl-parser-core/README.md)
- [LSP-core README](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/crates/perl-lsp-rs-core/README.md)
- [Server README](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/crates/perl-lsp-rs/README.md)

Former microcrates may now be modules inside surviving packages; the manifest and package-local READMEs are authoritative for current ownership.

# LSP4IJ 0.20.1 released Perl fixture

This directory is an **evidence snapshot**, not perl-lsp's desired LSP4IJ integration.

It pins `redhat-developer/lsp4ij` release/tag `0.20.1` at commit `1f62a3f8d8718db00b3db9189772f3a9172e4fb3`. The manifest records every bounded Perl LSP/DAP source path consumed by the integration audit, its upstream Git blob identity, and byte size. `normalization = identity-v1` means every materialized fixture in this directory is copied byte-for-byte from that exact upstream subject.

Only the behavior-bearing files needed by ordinary offline review are materialized. Large or repetitive sources such as `settings.schema.json` and the individual DAP launch examples remain identified by exact upstream path/blob/size in `manifest.json`; the explicit network-assisted refresh path is owned by #8004.

The snapshot intentionally preserves defects or stale material present in the release. In particular, it preserves the broad released Perl file mappings, copied `perl-lsp.*` VS Code-style settings, fixed v0.15.0 installer fallback URLs, and placeholder Perl DAP documentation. Corrected desired state belongs to #7875/#7876 and must never overwrite this evidence namespace.

This fixture establishes only what LSP4IJ 0.20.1 released. It does **not** establish the actual initialize/capability shape (#8584), server negotiation against that shape (#8595), managed installation (#7974), actual IntelliJ behavior (#7977/#7719), DAP behavior (#7877), or support promotion (#8005/#7122).

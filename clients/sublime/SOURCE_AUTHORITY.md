# LSP-perllsp source authority

`clients/sublime/package-source.v1.json` is the checked handoff contract between the reviewed package source in this repository and the eventual public `EffortlessMetrics/LSP-perllsp` repository.

Before the first public release, `perl-lsp-swarm` is the editable development authority. `python clients/sublime/package_source.py export` produces the complete candidate public tree from the declared manifest and records a content digest. It rejects undeclared files, missing files, symlinks, path traversal, non-normalized paths, and non-deterministic output.

The `public_repository_authoritative` phase is not accepted by this handoff tooling yet. At cutover, the tooling must be extended to resolve and verify a pinned public-repository source tree before the manifest can enter that phase. Until then, validation and export fail closed rather than reading the local development checkout while claiming public authority. The two repositories must never remain independently editable.

The source tree and distributable package are different manifests:

- `source_files` contains the public repository source, tests, and host-test material;
- `clients/sublime/tests/test_package_source.py` is deliberately outside the exported tree because it tests this monorepo handoff tooling. The package contract workflow runs it separately; the exported tree therefore contains only tests that can resolve their own package-local dependencies.
- `package_files` contains only files included in the deterministic `.sublime-package` artifact.

Package, server, DAP, Sublime LSP, and Sublime host versions remain separate identities throughout the transfer.

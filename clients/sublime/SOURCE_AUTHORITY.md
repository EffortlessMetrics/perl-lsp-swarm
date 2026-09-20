# LSP-perllsp source authority

`clients/sublime/package-source.v1.json` is the checked handoff contract between the reviewed package source in this repository and the eventual public `EffortlessMetrics/LSP-perllsp` repository.

Before the first public release, `perl-lsp-swarm` is the editable development authority. `python clients/sublime/package_source.py export` produces the complete candidate public tree from the declared manifest and records a content digest. It rejects undeclared files, missing files, symlinks, path traversal, non-normalized paths, and non-deterministic output.

At cutover, the manifest changes to `public_repository_authoritative`. From that point, the public repository is the only editable package authority and this repository consumes a pinned source tree or digest for integration testing. The two repositories must never remain independently editable.

The source tree and distributable package are different manifests:

- `source_files` contains the public repository source, tests, and host-test material;
- `package_files` contains only files included in the deterministic `.sublime-package` artifact.

Package, server, DAP, Sublime LSP, and Sublime host versions remain separate identities throughout the transfer.

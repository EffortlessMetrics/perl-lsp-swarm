# perl-interpreter

Owns the existing Perl interpreter resolver, configured/toolchain/OS discovery,
and environment/filesystem-invalidated process-local discovery cache.

Preserve the distinction between the resolver APIs and ranked discovery APIs.
This crate observes candidates; it does not authorize or spawn a process. The pure
project-environment/trust snapshot remains owned by `perl-workspace-core`.

Normal dependencies remain `anyhow` only. Tests use `perl-test-must`; neither
production nor development dependencies may import the parser, workspace, LSP,
DAP, async runtime or heavyweight shared test support.

The old core and DAP paths are compatibility re-exports. Keep one implementation,
one result type and one cache. DAP-only path normalization and launch-environment
assembly stay in DAP. Tests that mutate discovery environment variables must run
serially with every reader in the same test binary.

Focused proof: the package's unit tests, `interpreter_resolution` and
`dependency_contract`; then affected consumer compatibility tests. Follow root
Cargo storage admission for every compiled invocation.

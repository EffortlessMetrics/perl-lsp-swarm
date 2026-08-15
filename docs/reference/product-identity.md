# Product and executable identity

The product is **perl-lsp**. Its primary server executable and Cargo package are
both **`perllsp`**. The VS Code extension is
**`EffortlessMetrics.perl-lsp-rs`**. The optional debug adapter executable and
Cargo package are **`perl-dap`**, and its product posture remains **preview**.

## Install from Cargo

```bash
cargo install perllsp --locked
```

This installs the `perllsp` executable. The crates.io package named `perl-lsp`
is a **different project** and is not this language server. The implementation
library crate `perl-lsp-rs` is not the command to run.

## Configure another editor

```bash
perllsp --stdio
```

Use `perllsp`. The implementation crate `perl-lsp-rs` and the VS Code extension
ID `EffortlessMetrics.perl-lsp-rs` are not executable names; the extension ID is
an argument to editor tooling, never a command.

The product name `perl-lsp` is a deliberate special case. The `perl-lsp-rs`
crate still builds a `perl-lsp` binary, which `policy/product-identity.toml`
records as `server.compatibility_executable`, so that older installations keep
working. It is not the canonical entry point, it is not what new guidance should
teach, and support requests should quote `perllsp`. Name it only when explaining
compatibility with an install that already exists.

## Capture support identity

A semantic version alone does not establish source, target, or artifact parity.
Capture the bounded machine packet instead:

```bash
perllsp --identity-json
perl-dap --identity-json
```

The packet identifies the product, executable, Cargo package, role, version,
target and source/build state when available. Installers and release tools bind
it to the externally measured executable digest; the executable does not claim
its own final file hash.

## Repository context

- Public product and release lineage: `EffortlessMetrics/perl-lsp`
- Active development and evidence: `EffortlessMetrics/perl-lsp-swarm`

Development links may point to the swarm repository when labeled as development
evidence. Public installation and support paths must point to the public product
repository after the reviewed publication-context transformation.

# Product and executable identity

The product is **perl-lsp**. Its primary server executable and Cargo package are
both **`perllsp`**. The VS Code extension is
**`EffortlessMetrics.perl-lsp-rs`**. The optional debug adapter executable and
Cargo package are **`perl-dap`**, and its product posture remains **preview**.

## Install from Cargo

```bash
cargo install perllsp --locked
```
> The crates.io package `perl-lsp` is a **different project**, not this language
> server.

This installs the `perllsp` executable. The implementation library crate
`perl-lsp-rs` is not the command to run.

## Configure another editor

```bash
perllsp --stdio
```

Use `perllsp`. The implementation crate `perl-lsp-rs` and the VS Code extension
ID `EffortlessMetrics.perl-lsp-rs` are not executable names; the extension ID is
an argument to editor tooling, never a command.

The product/repository name `perl-lsp` is not an executable either. This
repository ships exactly two executables, `perllsp` and `perl-dap`: that is what
`dist-workspace.toml` releases, what `Formula/perllsp.rb` installs, and what
`policy/product-identity.toml` records. The `perl-lsp-rs` crate declares no
binary target at all.

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

- Active product development and proof: `EffortlessMetrics/perl-lsp-swarm/main`
- Public contribution ingress, release lineage, and published artifacts:
  `EffortlessMetrics/perl-lsp/master`

External issues and pull requests target the public repository. Before integrating a
shared-product public PR, maintainers first reconcile accepted swarm state into the public
repository so the contribution is reviewed against the current product projection. After
the public PR merges, its shared-product effect is reconciled back into swarm. This
contributor route does not make `perl-lsp` a second independent product-development
authority.

Development evidence may point to the swarm repository when labeled as development
evidence. Public installation and support paths must point to the public product repository
after the reviewed publication-context transformation.

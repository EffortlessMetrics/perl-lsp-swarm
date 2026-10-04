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

- Public product, external contributor ingress, and release lineage:
  `EffortlessMetrics/perl-lsp` on `master`.
- Active high-throughput development and product proof:
  `EffortlessMetrics/perl-lsp-swarm` on `main`.

External issues and pull requests target `perl-lsp/master`; this does not create a
second independent product-development trunk. Before integrating a shared-product public
pull request, maintainers first converge accepted swarm state into the public repository.
After the public merge, the shared-product effect is reverse-converged into swarm.

Development evidence may link to the swarm repository when it is actually the subject.
Public installation and support paths point to the public product repository after the
reviewed publication-context transformation. Source integration, release lineage, and
channel availability remain distinct states.

# Reuse the cloud startup entry

`scripts/cloud-startup` validates an already provisioned Linux cloud image and
prepares the refreshed checkout. Put this command in the platform's saved setup
configuration after its tool-provisioning steps:

```bash
bash /workspace/perl-lsp-swarm/scripts/cloud-startup
```

The entry resolves the checkout from its own location. Refresh that checkout
before calling it: the checkout's `rust-toolchain.toml` selects Rust, rather than
the historical commit or toolchain used to create the image. A setup process's
exports do not persist into a later independent task shell; select the same
storage/toolchain variables there before invoking the existing admitted build
route.

The platform's saved setup configuration is external to this repository. The
available cloud filesystem exposed `/workspace/shared/setup-perl-lsp.sh`, but no
saved-configuration editor or tracked equivalent. Adding this entry does not
update that external configuration automatically. Replace the corresponding
startup portion there with the command above; preserve separately owned tool
provisioning. Do not keep a second copied startup body alongside this entry.

## Provisioned tools and paths

The image must already contain Bash, Python 3.11+ (`tomllib`), Git, the selected
Rust toolchain, `just`, and the extension's supported Node/npm tools. Startup
does not install a missing toolchain or compile a missing utility. It sets
`RUSTUP_AUTO_INSTALL=0` and leaves rustup's global default unchanged. Missing or
malformed toolchain configuration stops before tool commands run.

`PERL_LSP_CLOUD_SHARED` selects the existing host storage root; the default is
`/workspace/shared`. The entry selects `cargo`, `rustup`,
`devplane/perl-lsp-swarm`, and `npm-cache` beneath that root, and prepends its
`cargo/bin` and `node-v26.6.0-linux-x64/bin` directories to PATH. The Node path
matches the provisioned image used for this entry; supported versions remain
owned by `vscode-extension`'s `npm run doctor`, not by this pathname. A missing
Node directory can use the existing PATH's Node when the doctor accepts it.
An empty inherited PATH does not add the current directory to command lookup;
checkout resolution uses Bash builtins before the selected tool paths are set.
The TOML pin and fixture text are read explicitly as UTF-8, including under an
ASCII locale.

Startup verifies installed Rust/Cargo/just commands, calls the existing declared
release-tag preparation, then runs `just devplane-init` and
`just agent-preflight`. The latter checks layout and explicitly does not assess
Cargo build capacity. In the extension directory, startup runs the toolchain
doctor before `npm ci`. These operations can fetch Git objects and npm
packages; setup is not an offline or read-only command.

## Capacity and qualification

This entry sets no capacity or job overrides. Existing explicit operator
settings retain their semantics. It neither admits a Cargo build nor supplies a
measured workload budget. Follow [constrained Cargo builds](CONSTRAINED_CARGO_BUILDS.md)
for exact-command preflight and the existing scoped-budget route; moving a path
does not add capacity. The desktop admission/profile owner must supply the
qualified workload basis before cloud compilation proceeds.

The six cheap regression tests execute the actual shell entry using a temporary
checkout and mocked tools. They cover startup completion, two distinct checkout
pins, missing/malformed pin refusal before tool commands, absence of startup's
capacity/job overrides, history-before-layout-preflight ordering, disabled
automatic Rust installation, the complete operation sequence, and refusal to
run npm ci after the toolchain doctor fails. Empty-PATH and ASCII-locale
subcases protect command lookup and UTF-8 pin handling:

```bash
bash -n scripts/cloud-startup
python3 -B -m unittest discover -s scripts/tests -p test_cloud_startup.py -v
```

The tests do not establish real provisioning, network/npm success, build
admission, peak disk growth, or cold/warm Cargo timing. No new required CI gate
is activated. Revert the entry, tests, and this guide together to roll back the
repository change; restore any external saved setup configuration separately.

# Reuse the cloud startup entry

`scripts/cloud-startup` validates an already provisioned Linux cloud image and
prepares the refreshed checkout. Put this command in the platform's saved setup
configuration after its tool-provisioning and task-shell activation steps:

```bash
bash /workspace/perl-lsp-swarm/scripts/cloud-startup
```

The entry resolves the checkout from its own location. Refresh that checkout
before calling it. A setup process's exports do not persist into a later
independent task shell, so the task shell must activate the intended toolchain
and Node installation before it invokes the repository startup entry.

The platform's saved setup configuration is external to this repository. The
available cloud filesystem exposed `/workspace/shared/setup-perl-lsp.sh`, but no
saved-configuration editor or tracked equivalent. Adding this entry does not
update that external configuration automatically. Replace the corresponding
startup portion there with the command above; preserve separately owned tool
provisioning and activation. Do not keep a second copied startup body alongside
this entry.

## Provisioned tools and paths

The image must already contain Bash, Python 3.11+ (`tomllib`), Git, `rustup`
1.28.0 or newer, the selected Rust toolchain, `just`, and the extension's supported
Node/npm tools. Startup is a validation entry, not an installer. It sets
`RUSTUP_AUTO_INSTALL=0` and does not intentionally install tools, change rustup's
global default, or write a directory override. Missing or malformed repository
toolchain configuration stops before tool commands run.

Before any toolchain-selecting probe, startup uses the shared Cargo guard's
safe `rustup --help` check for the selected manager and distinct managers identified
behind the resolved `rustc`/`cargo` proxies. Older or unverified managers found by
that check fail with exit 78 and an explicit update instruction; startup does not
update them. Rustup 1.27 and earlier can install a missing toolchain despite
`RUSTUP_AUTO_INSTALL=0`, so the environment variable alone is not the safeguard.

**Provenance limitation (#19268):** the current shared proxy lookup does not follow
every outer executable alias to its backing manager. A modern PATH manager with an
outer `rustc` symlink into a different old installation can escape the check. The
no-install guarantee is not qualified for that layout until the shared provenance
repair is consumed and its effect-sentinel regression passes. Standard-layout green
tests do not establish that missing predicate. Do not bypass it by merely setting the
environment variable or reporting a newer unrelated manager.

`PERL_LSP_CLOUD_SHARED` selects the existing host storage root; the default is
`/workspace/shared`. A relative value is anchored to the startup invocation
directory before any directory change. The entry selects `cargo`, `rustup`,
`devplane/perl-lsp-swarm`, and `npm-cache` beneath that root and prepends only
its `cargo/bin` directory to `PATH`. It does not manufacture a Node patch-level
path. The active `node` and `npm` commands must already be on the inherited
`PATH`; supported versions remain owned by `vscode-extension`'s
`npm run doctor`.

Before changing directories, startup anchors every relative or empty element of
a nonempty inherited `PATH` to the invocation directory while preserving entry
order. The Node/npm pair validated at startup is therefore the pair later used
from `vscode-extension`. A fully empty inherited `PATH` adds no current-directory
entry. Checkout resolution uses Bash builtins before the selected tool paths are
set and does not consult inherited `CDPATH`. The TOML pin and fixture text are
read explicitly as UTF-8, including under an ASCII locale.

## Rust selection precedence and receipt

A nonempty caller-supplied `RUSTUP_TOOLCHAIN` remains authoritative and is
passed unchanged to every startup child. This supports an explicitly activated
current-stable task shell without silently switching the subject back to the
repository pin:

```bash
RUSTUP_TOOLCHAIN=stable bash /workspace/perl-lsp-swarm/scripts/cloud-startup
```

When `RUSTUP_TOOLCHAIN` is absent, startup leaves it absent, changes to the
checkout, and lets rustup apply its normal selection precedence in repository
context. Absent a higher-precedence rustup override, `rust-toolchain.toml` is the
repository default. The file is still parsed before setup begins so malformed or
missing repository configuration fails before any mutable step. Startup does not
create a second hidden selector by exporting the parsed pin.

The startup receipt records:

- whether the Rust selector came from the environment or rustup resolution;
- the explicit selector, when present, and the repository default;
- resolved paths for `rustup`, `rustc`, `cargo`, `just`, `node`, and `npm`;
- manager paths and versions actually checked before any selecting probe;
- `rustup show active-toolchain` plus Rust, Cargo, Node, npm, and just versions.

This receipt reports selection; it does not redefine MSRV, repository-default
parity, or compatibility qualification. Those policy and diagnostic distinctions
remain separately owned.

Startup then calls the existing declared release-tag preparation, runs
`just devplane-init`, and runs `just agent-preflight`. The former only prints
planned admitted resource paths; it allocates no build/cache directories. The latter
checks layout and does not assess Cargo build capacity. In the extension directory,
startup runs the Node/npm toolchain doctor before `npm ci`. Missing commands and
unsupported Node/npm versions fail before package installation. These operations
can fetch Git objects and npm packages; setup is not an offline or read-only
command.

## Capacity and qualification

This entry sets no capacity or job overrides. Existing explicit operator
settings retain their semantics. It neither admits a Cargo build nor supplies a
measured workload budget. Follow [constrained Cargo builds](CONSTRAINED_CARGO_BUILDS.md)
for exact-command preflight and the existing scoped-budget route; moving a path
does not add capacity. The desktop admission/profile owner must supply the
qualified workload basis before cloud compilation proceeds.

The focused regression tests execute the actual shell entry using a
temporary checkout and mocked tools. They cover repository-default selection,
an explicit stable-only image without the repository pin installed, two distinct
checkout pins, missing/malformed pin refusal before tool commands, active-PATH
Node precedence over a stale shared patch directory, stable relative-PATH
selection across directory changes, empty-PATH lookup, missing Node and npm,
unsupported Node/npm failures before `npm ci`, a missing repository-selected
toolchain failing before setup, repeated startup without repository or shared
rustup-state mutation, absence of startup's capacity/job overrides, preservation
of explicit capacity/job settings, operation ordering, and UTF-8 under an ASCII
locale. Manager effect-sentinel fixtures cover the selected manager and distinct
sibling-manager layouts; the outer-alias limitation above remains separate.

```bash
bash -n scripts/cloud-startup
bash -n scripts/lib/cargo-toolchain-guard.sh
python3 -B -m unittest discover -s scripts/tests -p test_cloud_startup.py -v
```

The tests do not establish real provisioning, network/npm success, build
admission, peak disk growth, cold/warm Cargo timing, or publication of the
external cloud configuration. No new required CI gate is activated. Revert the
entry, shared guard helper, tests, and this guide together to roll back this repair;
restore any external saved setup configuration separately.

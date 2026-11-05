# Development environment

Prepare a `perl-lsp-swarm` checkout in this order: acquire tools, activate the
current shell, inspect prerequisites without building, install and check hooks
through an admitted command, then run focused proof. Start in the repository
root; [CONTRIBUTING.md](../../CONTRIBUTING.md) covers cloning and branch ownership.

## 1. Acquire the tools for your work

Choose a Rust acquisition path. Installation can download tools; keep it separate
from the inspection commands below.

| Path | Setup and authority |
| --- | --- |
| Nix | Run `nix develop`. [flake.nix](../../flake.nix) and [flake.lock](../../flake.lock) supply direct pinned Rust, rustfmt, Clippy, and the listed development dependencies. |
| Native or provisioned Rust | Install the channel and components in [rust-toolchain.toml](../../rust-toolchain.toml), using [rustup](https://rustup.rs/) or a provisioned direct toolchain. Acquire `just` from your platform package manager or its [releases](https://github.com/casey/just/releases). |

On a fresh native host, install the rustup manager first using the
[official installation guide](https://rust-lang.github.io/rustup/installation/index.html)
(`rustup-init` on Windows). On Unix, the manager-only bootstrap below explicitly
defers toolchain acquisition; run it only when rustup is absent:

```bash
(
  set -euo pipefail
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs |
    sh -s -- --default-toolchain none -y
)
```

Activate the actual installation in the current Unix shell, then explicitly
install the repository pin and components. A native Windows shell should use
the manager activation from the installation guide before the same `rustup`
command.

```bash
. "${CARGO_HOME:-$HOME/.cargo}/env" &&
rustup toolchain install 1.95.0 --profile minimal --component rustfmt --component clippy
```

The common route also needs Git, Bash 4 or newer with ordinary shell utilities,
and Python. **Python 3.11+ is recommended** for the repository-owned commands in
this guide and staged Clippy; builtin Cargo admission alone supports 3.10+.
Supply the compiler/linker tools for your native Rust target: a C toolchain on
Linux, Apple command-line tools on macOS, or Visual C++ build tools for Windows
MSVC. The Linux linker-driver scope is documented in
[constrained builds](CONSTRAINED_CARGO_BUILDS.md#native-linux-linked-output).

On Windows, run `just` recipes in Git Bash with `cygpath` available; retain the
[Windows shell and symlink guidance](../../CONTRIBUTING.md#windows-shell-for-just-recipes).
The Bash blocks here are not PowerShell syntax. Native Windows Cargo admission
has its own [documented entrypoint](../agents/CARGO_STORAGE.md#native-windows-setup).
If Windows exposes your Python installation as `python`, substitute that name for
`python3` in the discovery loop and Python commands below.

Install additional tools when the selected work needs them:

| Work | Additional tools and owner |
| --- | --- |
| VS Code extension | Node and npm versions from [vscode-extension/package.json](../../vscode-extension/package.json); the Nix shell does not supply Node. |
| Perl semantics, corpus, or DAP runtime proof | A Perl runtime appropriate to the owning test; Nix includes Perl. |
| Repository contract checks or native PR review | [Portable contract tools](PORTABLE_CONTRACT_TOOLS.md) owns Aqua-managed CLIs and GitHub CLI acquisition. The reviewed Aqua bootstrap is Linux amd64 only. |

## 2. Activate tools in every fresh shell

Enter `nix develop` in the shell that will run the work, or activate your native
installation there. For a standard rustup installation, load its environment:

```bash
. "${CARGO_HOME:-$HOME/.cargo}/env"
```

Saved cloud setups may use different Cargo, rustup, or binary directories. Use
the actual saved locations and the task-local example in
[agent tool activation](AGENT_CONTRIBUTING.md#activate-tools-in-the-task-shell).
An export in an earlier setup process does not configure a new shell. Check
activation before reinstalling a tool that is already on disk.

## 3. Inspect prerequisites without building

Run these probes from the checkout root. The existing toolchain guard checks the
selected Cargo and refuses its rustup proxy if the manager is older than 1.28;
those older managers ignore `RUSTUP_AUTO_INSTALL=0`. The subshell disables
automatic toolchain installation and stops at the first failed probe.
Direct Rust installations, including Nix, do not need a `rustup` command for this
inspection.

```bash
(
  set -eu
  export RUSTUP_AUTO_INSTALL=0
  unset RUSTUP_TOOLCHAIN
  for tool in git bash python3 just rustc cargo rustfmt cargo-clippy; do
    command -v "$tool" || { printf 'Missing command: %s\n' "$tool" >&2; exit 1; }
  done
  bash -c 'test "${BASH_VERSINFO[0]}" -ge 4' || {
    printf 'Bash 4 or newer is required for the repository recipes.\n' >&2
    exit 1
  }
  . scripts/lib/cargo-toolchain-guard.sh
  cargo_toolchain_guard
  git --version
  python3 --version
  just --version
  cargo --version
)
```

Compare the Cargo version with `rust-toolchain.toml`. A missing command or pin is
an activation/acquisition problem to resolve before a build. Compiler/component
commands are discovered without executing other proxies; discovery alone does
not verify every component. Explicit acquisition and the selected focused proof
establish those requirements. These probes do not compile the workspace or
establish build capacity.

For shallow clones or saved cloud snapshots, inspect the release-history inputs
without fetching:

```bash
python3 scripts/check_release_tag_provenance.py --verify-git --repo-root .
```

If history or declared refs are missing, follow the explicit preparation step in
[historical inputs](CONSTRAINED_CARGO_BUILDS.md#prepare-historical-inputs-in-a-fresh-cloud-checkout).
That separate step can fetch Git objects; a failed inspection is not a reason to
silently download them during a probe.

## 4. Admit bootstrap work, then install and check hooks

Select host storage and establish the exact-command route in
[Cargo storage admission](../agents/CARGO_STORAGE.md). It owns worktree-private
target/build paths, supported commands, and resource admission. Use
[constrained-host guidance](CONSTRAINED_CARGO_BUILDS.md) when capacity refuses;
keep the host's policy intact.

Check worktree ownership first: the hook installer provisions **all registered
worktrees in the clone** and changes repository-local `core.hooksPath`. Use the
current lightweight `perl-ci-hygiene` owner from the intended worktree. The
execution commands below can compile Rust and download dependencies; each must
follow its own successful preflight.

```bash
(
  set -eu
  unset RUSTUP_TOOLCHAIN
  scripts/cargo-admitted --preflight run -p perl-ci-hygiene --bin perl-ci-hygiene --profile agent --locked -- install-githooks
  scripts/cargo-admitted run -p perl-ci-hygiene --bin perl-ci-hygiene --profile agent --locked -- install-githooks
  scripts/cargo-admitted --preflight run -p perl-ci-hygiene --bin perl-ci-hygiene --profile agent --locked -- check-githooks
  scripts/cargo-admitted run -p perl-ci-hygiene --bin perl-ci-hygiene --profile agent --locked -- check-githooks
)
```

This uses the authoritative hook installer without the product-linked `xtask`
bootstrap. A passing hook check establishes the installed hooks' currency; later
Cargo commands still need their own admission. Keep the same intended toolchain
selection in those task shells; the ordinary route above uses the repository pin.

Now inspect repository health:

```bash
just doctor
```

Read the findings: ordinary doctor findings are advisory and can exit zero.
Its pre-push check follows Git's effective hooks path and compares against this
checkout's `hooks/pre-push`. It reports problems without repairing Git state.

## 5. Run focused proof

Choose the smallest test for the change. For example, the doctor's own regression
fixtures need Git, Bash, Python, and `just`, and perform no Cargo build:

```bash
python3 -m unittest discover -s scripts/tests -p test_just_doctor.py -v
```

For Rust work, preflight the exact package/target/profile/flags you will execute
through the admitted route. Follow
[focused proof and gate selection](../../CONTRIBUTING.md#4-run-focused-proof-first)
for broader checks when the changed surface requires them. Hook installation,
version probes, and an advisory doctor report do not replace that proof.

`just devex` is a later diagnostic: it compiles the product-linked `xtask`, so it
needs an applicable build route first. It currently requires `rustup` even with a
valid direct Nix toolchain; that mismatch is tracked in
[#14323](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/14323).
Existing work owns [devex bootstrap diagnostics](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/16485)
and [Aqua activation diagnostics](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/18597).
Keep acquisition, activation, and those later diagnostics distinct when reporting
a setup failure.

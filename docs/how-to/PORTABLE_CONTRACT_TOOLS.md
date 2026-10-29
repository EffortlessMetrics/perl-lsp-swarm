# Portable contract tools

The repository supports two installation paths:

| Path | Authority | Use |
|---|---|---|
| Nix development shell | `flake.nix` / `flake.lock` | Complete contributor environment, including Rust, Python, Node, and repository CLIs |
| Aqua | `aqua.yaml` | Exact portable non-language CLIs for contributors and CI jobs that do not enter Nix |

Aqua is deliberately **not** a task runner. `just` and `cargo xtask` remain the command and policy authorities.

## Bootstrap

Aqua itself is bootstrapped from the official GitHub release asset at a
reviewed version and SHA-256. Both CI consumers (`ci.yml` Repository
Contract and `portable-contract-tools.yml`) and local non-Nix installs use
the same script. This path does **not** use `go install` or the Go module
proxy/checksum database (#15235).

```bash
bash scripts/tools/install-aqua.sh --dest "$HOME/.local/bin"
export PATH="$HOME/.local/bin:$PATH"
```

Then validate and install the repository tool set:

```bash
bash scripts/tools/aqua-doctor.sh
```

A transient download may retry the same asset identity. A checksum mismatch,
unsupported platform, missing binary, or unexpected version is **NOT PROVEN**
and never becomes a clean result. Do not set `GOSUMDB=off` or otherwise
weaken integrity to paper over an acquisition flake.

The first inventory contains tools that already have repository-native policies and pinned CI versions:

- Changie 1.25.0;
- actionlint 1.7.12;
- Zizmor 1.26.1.
- Taplo 0.10.0;
- typos 1.48.0.

Lychee and other contract tools join this inventory only when their owning admission slice lands.

## Integrity model

`scripts/tools/install-aqua.sh` pins Aqua itself to the official
`v2.57.0` `aqua_linux_amd64.tar.gz` asset and the SHA-256 published in that
release's `aqua_2.57.0_checksums.txt`. `aqua.yaml` then pins:

1. an immutable commit of the Aqua standard registry;
2. an exact version for every tool.

The standard registry supplies each package's asset and checksum contract. A tool upgrade changes the package version and, when needed, the registry commit in one reviewable diff.

Do not point the standard registry at `main`. Aqua treats registry refs as immutable; a branch name would make that assumption false.

## Local and CI parity

A job or contributor using Aqua should execute tools through:

```bash
aqua exec -- <tool> <arguments>
```

The existing Nix and checksum-install workflows remain in place during the foundation phase. A later PR may consolidate those paths after Linux, macOS, Windows, and forked-PR behavior have receipts.

## Changed-file hygiene

The local and CI entry point for the scoped Taplo/typos contract is:

```bash
cargo xtask repo-hygiene --base origin/main --head HEAD
```

It consumes the shared exact-head change-set resolver and requires the checkout
to be at the requested head with selected files clean. Taplo checks changed TOML
formatting and syntax; typos checks changed text/config/source files. An empty
scope is `NOT_APPLICABLE`. Missing Aqua or a missing pinned tool is `NOT_PROVEN`
and exits non-zero; it is never treated as a clean result.

Changed-file hygiene rejects network-backed Taplo schema references before the
checker runs. Relative local schema paths remain eligible for validation; the
contract does not let a pull request make CI fetch an arbitrary URL.

## Upgrade procedure

To change the Aqua bootstrap itself, update the version and reviewed SHA-256
in `scripts/tools/install-aqua.sh`, refresh
`scripts/tests/fixtures/aqua-bootstrap/` from that release's official
checksums file, and rerun `bash scripts/tests/test-install-aqua.sh`.

For managed tools:

1. Select the intended tool release and read its release notes.
2. Update the package version in `aqua.yaml`.
3. Update the immutable Aqua registry commit if the old snapshot does not describe that release.
4. Run `bash scripts/tools/install-aqua.sh --dest "$HOME/.local/bin"` if Aqua is not already on `PATH`, then `bash scripts/tools/aqua-doctor.sh` outside Nix.
5. Run the existing repository-native checker that owns the tool's policy.
6. Record runtime, unsupported platforms, and rollback instructions in the PR.

## Failure meaning

A missing Aqua binary, failed download, checksum failure, unsupported platform, or unexpected version is **NOT PROVEN**. `install-aqua.sh` and the doctor exit non-zero; they never convert missing tooling into a clean result.

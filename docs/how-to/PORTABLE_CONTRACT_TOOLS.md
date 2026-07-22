# Portable contract tools

The repository supports two installation paths:

| Path | Authority | Use |
|---|---|---|
| Nix development shell | `flake.nix` / `flake.lock` | Complete contributor environment, including Rust, Python, Node, and repository CLIs |
| Aqua | `aqua.yaml` | Exact portable non-language CLIs for contributors and CI jobs that do not enter Nix |

Aqua is deliberately **not** a task runner. `just` and `cargo xtask` remain the command and policy authorities.

## Bootstrap

Aqua itself is bootstrapped at a reviewed version:

```bash
go install github.com/aquaproj/aqua/v2/cmd/aqua@v2.57.0
```

Then validate and install the repository tool set:

```bash
bash scripts/tools/aqua-doctor.sh
```

The first inventory contains tools that already have repository-native policies and pinned CI versions:

- Changie 1.25.0;
- actionlint 1.7.12;
- Zizmor 1.26.1.

Lychee, Taplo, typos, and other contract tools join this inventory only when their owning admission slice lands.

## Integrity model

`aqua.yaml` pins:

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

## Upgrade procedure

1. Select the intended tool release and read its release notes.
2. Update the package version in `aqua.yaml`.
3. Update the immutable Aqua registry commit if the old snapshot does not describe that release.
4. Run `bash scripts/tools/aqua-doctor.sh` outside Nix.
5. Run the existing repository-native checker that owns the tool's policy.
6. Record runtime, unsupported platforms, and rollback instructions in the PR.

## Failure meaning

A missing Aqua binary, failed download, checksum failure, unsupported platform, or unexpected version is **NOT PROVEN**. The doctor exits non-zero; it never converts missing tooling into a clean result.

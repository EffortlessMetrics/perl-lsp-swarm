# Installation Guide

Use this page when you need to install `perllsp`, upgrade an existing install,
or verify that the binary works on your machine.

If you only need editor integration after installation, jump to
[EDITOR_SETUP.md](EDITOR_SETUP.md). If the binary starts but does not behave as
expected, see [TROUBLESHOOTING.md](TROUBLESHOOTING.md).

If you are wiring `perllsp` into a GitHub Actions workflow, see
[GitHub Actions Integration](GITHUB_ACTIONS.md).

The verified GitHub `v0.17.0` archives are public beta. Other install channels
remain independently versioned and are not proven current by that receipt;
verify the binary before wiring it into shared automation.

## Fastest Path

Use one of the public install paths that matches how you work:

- VS Code: install the `EffortlessMetrics.perl-lsp-rs` extension and let it download the matching `perllsp` binary.
- macOS or Linux: use the [manual archive](#manual-archive) until the release packet publishes an immutable installer identity and digest. The identity-bound [installer wrapper](#installer-script-macos-and-linux) becomes usable when those values exist.
- Windows: install from the [manual archive](#manual-archive). The PowerShell installer script does not work against the published assets yet ([#5461](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/5461)).
- Other editors: download a prebuilt binary from [GitHub Releases](https://github.com/EffortlessMetrics/perl-lsp/releases) and put it on your `PATH`.
- Local testing or pre-release validation: install from this repo with `cargo install --path crates/perllsp`.

Do not install the unrelated crates.io package named `perl-lsp`. That package
name is owned by another project, so the supported Cargo package is `perllsp`.

Inspect the install before wiring it into an editor. `--doctor` reports the
local Perl and workspace setup; it is a diagnostic report, not a CI gate.
`--health` is only a liveness probe that confirms the binary can execute:

```bash
perllsp --version
perllsp --doctor
```

## Installer Script (macOS and Linux)

The repository maintains the installer logic at
[`scripts/install.sh`](../../scripts/install.sh). The root
[`install.sh`](../../install.sh) is only a bootstrap and argument-compatibility
wrapper.

From a clone, the wrapper executes the sibling `scripts/install.sh` directly:

```bash
git clone https://github.com/EffortlessMetrics/perl-lsp.git
cd perl-lsp
bash install.sh --help
```

A remote or piped wrapper is deliberately **non-authoritative**. It cannot
select installer logic from mutable `master`, `main`, `HEAD`, another branch,
or an arbitrary ref. Before it executes any downloaded installer logic, it
requires:

1. `PERL_LSP_INSTALLER_REF`: a full lowercase 40-character commit SHA for both
   the piped wrapper URL and the canonical `scripts/install.sh` fetch; and
2. `PERL_LSP_INSTALLER_SHA256`: the reviewed 64-character lowercase SHA-256
   digest of that exact ref's `scripts/install.sh`.

The wrapper downloads only the exact repository/ref/path, refuses redirects
and non-200 responses, requires `sha256sum` or `shasum`, and executes the
installer only after the content digest matches.

Once a release closeout publishes both values, use this command shape:

```bash
INSTALLER_REF=<full-40-char-commit-sha>
INSTALLER_SHA256=<reviewed-sha256-of-scripts-install-sh>

curl -fsSL "https://raw.githubusercontent.com/EffortlessMetrics/perl-lsp/$INSTALLER_REF/install.sh" \
  | PERL_LSP_INSTALLER_REF="$INSTALLER_REF" \
    PERL_LSP_INSTALLER_SHA256="$INSTALLER_SHA256" bash
```

Do not substitute a digest downloaded from the same unverified mutable source
at runtime. The digest must come from the reviewed release/topology closeout or
another independently reviewed repository record. Until such a digest is
published, use a manual release archive or a reviewed clone.

Installer options remain environment variables on the `bash` side of the
pipeline:

```bash
curl -fsSL "https://raw.githubusercontent.com/EffortlessMetrics/perl-lsp/$INSTALLER_REF/install.sh" \
  | PERL_LSP_INSTALLER_REF="$INSTALLER_REF" \
    PERL_LSP_INSTALLER_SHA256="$INSTALLER_SHA256" \
    VERSION=v0.18.0 \
    INSTALL_DIR="$HOME/.local/bin" \
    PERL_LSP_LINUX_LIBC=musl bash
```

Supported release-archive platforms are Linux x86_64 and aarch64 (gnu or
musl), macOS x86_64, and macOS aarch64.

This bootstrap boundary proves only the identity of the downloaded
`scripts/install.sh`. It does **not** by itself prove the later release archive,
member layout, server/DAP pair, extraction, promotion, or rollback path. The
current canonical installer still treats missing `SHA256SUMS`, a missing asset
row, or the absence of a checksum tool as warning-and-continue conditions. The
PowerShell installer has a similar fail-open checksum boundary. Safe archive
inspection and atomic pair replacement also remain open under
[#6097](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/6097).
Use the manual archive plus an explicit checksum check for release-sensitive
installation until those remaining boundaries land.

`BUILD_FROM_SOURCE=1` installs **`perllsp` only**, not `perl-dap`. That mode
runs `cargo install perllsp`, and the `perllsp` package declares just the one
binary, so the debug adapter is skipped without an error. If you need the
debugger, use a release archive instead — the archives ship both binaries — or
build `perl-dap` yourself from a clone with
`cargo build -p perl-dap --release`.

## Windows

Use the [manual archive](#manual-archive) below. It is the only Windows path
that works today.

### Installer script — currently broken, do not use

The published script fails for every user. The copy served from
`perl-lsp/master` still derives its download name from `$Name = "perl-lsp"`,
producing `perl-lsp-<version>-x86_64-pc-windows-msvc.zip`, but
`.github/workflows/release.yml` publishes assets as
`perllsp-<version>-<target>.zip`. The requested URL 404s and the script exits
with `Failed to download from ...`; there is no fallback.

Verified against the live v0.17.0 release:

```text
perllsp-0.17.0-x86_64-pc-windows-msvc.zip   -> 200
perl-lsp-0.17.0-x86_64-pc-windows-msvc.zip  -> 404
```

[`install.ps1`](../../install.ps1) in this repository already carries the fix,
but the publication repo has not been synced
([#4348](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/4348)).
Once it is, the script will download the matching release zip, verify it
against `SHA256SUMS` when that file downloads successfully (warning and
continuing if it does not), and install `perllsp.exe` into
`%USERPROFILE%\.local\bin`.

Two further limits apply to the script even after that sync:

- The script installs `perllsp.exe` only. It does not install the `perl-dap.exe`
  debug adapter, unlike the POSIX installer. Take `perl-dap.exe` from the
  release zip if you need the debugger
  ([#5036](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/5036)).
- Only `x86_64-pc-windows-msvc` is built by the release workflow, so there is
  no native ARM64 Windows binary. The script installs the x64 build on ARM64,
  which runs under the x64 emulation in Windows 11 on ARM. Windows 10 on ARM
  emulates x86 but not x64, so the extension and PowerShell installer reject
  the fallback before downloading and you must build from source there
  ([#5007](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/5007)).

Once the sync lands and the script works, pinning a version or changing the
install directory means downloading it and passing parameters rather than
piping it. These commands 404 until then:

```powershell
irm https://raw.githubusercontent.com/EffortlessMetrics/perl-lsp/master/install.ps1 -OutFile install.ps1
.\install.ps1 -Version 0.17.0 -InstallDir C:\tools\bin
```

If PowerShell refuses to run the downloaded script, either unblock it
(`Unblock-File .\install.ps1`) or run it in a session that allows local scripts.

### Manual archive

Download `perllsp-<version>-x86_64-pc-windows-msvc.zip` from
[GitHub Releases](https://github.com/EffortlessMetrics/perl-lsp/releases),
extract it, and put the directory containing `perllsp.exe` and `perl-dap.exe`
on your `PATH`. This is the path with the fewest moving parts and the one the
release assets are proven against.

### Windows package managers

The repository owns manifest sources for three Windows package managers, and
release automation refreshes them on every release:

| Manager | Manifest source | Package identifier | Automation target |
| --- | --- | --- | --- |
| Scoop | `distribution/scoop/perl-lsp.json` | `perl-lsp` | bump PR to `ScoopInstaller/Main` |
| Chocolatey | `distribution/chocolatey/perl-lsp.nuspec` | `perl-lsp` | bump PR to `chocolatey-community/chocolatey-coreteampackages` |
| winget | `distribution/winget/perl-lsp.yaml` | `EffortlessMetrics.perl-lsp` | repo-local manifest only |

These are **not** proven-current install paths. The Scoop and Chocolatey
workflows open pull requests against community repositories that this project
does not control, and submission of the winget manifest to `winget-pkgs` is
still a manual follow-up (see `distribution/windows/README.md`). Before relying
on one, confirm the package is actually published and at the version you expect:

```powershell
scoop search perl-lsp
choco search perl-lsp
winget search EffortlessMetrics.perl-lsp
```

If it is missing or behind, use the [manual archive](#manual-archive) — it is
the only Windows path proven against the published assets today.

## Homebrew via the EffortlessMetrics tap

`perllsp` is distributed through the owned EffortlessMetrics Homebrew tap, not
Homebrew/core. The tap version is not verified by the v0.17.0 GitHub receipt;
inspect the formula version before installing:

```bash
brew install effortlessmetrics/tap/perllsp
```

Equivalent two-step form:

```bash
brew tap effortlessmetrics/tap
brew install perllsp
```

This covers macOS Intel, macOS Apple Silicon, Linux x86_64, and Linux aarch64 via Linuxbrew. Formula publication is an independent channel and must be verified separately.

Shell completions are not installed by default. To add them:

```bash
perllsp --completion bash > "$(brew --prefix)/etc/bash_completion.d/perllsp"
perllsp --completion zsh > "$(brew --prefix)/share/zsh/site-functions/_perllsp"
perllsp --completion fish > "$(brew --prefix)/share/fish/completions/perllsp.fish"
```

## Install From Source

Use this when you want to test the workspace locally or build a release binary
before publishing:

```bash
git clone https://github.com/EffortlessMetrics/perl-lsp.git
cd perl-lsp
cargo build --release --bin perllsp -p perllsp
```

If you want Cargo to build and install the published package into
Cargo's bin directory instead:

```bash
cargo install perllsp
```

## Prebuilt Releases

GitHub Releases provides the verified public-beta v0.17.0 archives for the supported platforms.
Check the latest release page before copying a version number.

Most Linux users should choose the `gnu` archive. Use `musl` mainly for Alpine
Linux or musl-based containers. You do not need both GNU and musl archives.

These suffixes are Rust target triples. On Linux, `unknown` is the standard
vendor field and is expected; choose based on the final `gnu` or `musl` segment.

| Your system | Asset suffix |
| --- | --- |
| Linux x64 / AMD64, most distributions | `x86_64-unknown-linux-gnu` |
| Linux ARM64, most distributions | `aarch64-unknown-linux-gnu` |
| Linux x64 / AMD64, Alpine or musl containers | `x86_64-unknown-linux-musl` |
| Linux ARM64, Alpine or musl containers | `aarch64-unknown-linux-musl` |
| macOS Intel | `x86_64-apple-darwin` |
| macOS Apple Silicon | `aarch64-apple-darwin` |
| Windows x86_64 | `x86_64-pc-windows-msvc` |

## After Installation

Once `perllsp` is installed, add it to your editor with the command:

```bash
perllsp --stdio
```

Then inspect the install from a shell before debugging editor integration:

```bash
perllsp --doctor
```

`perllsp --health` is also available as a liveness probe. It prints `ok <version>`
but does not inspect Perl, workspace, or module-lookup
configuration. For CI, use explicit checks for the environment paths your job
requires rather than treating the doctor report or its exit status as a gate.

## Release Maintainers

If you are preparing a release, keep this page aligned with
[RELEASE.md](../../RELEASE.md) and
[project/PUBLISHING_ROADMAP.md](../project/PUBLISHING_ROADMAP.md). The release
workflow and final checks live there, not here.

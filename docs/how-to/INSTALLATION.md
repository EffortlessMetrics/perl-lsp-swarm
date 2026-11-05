# Installation Guide

Use this page when you need to install `perllsp`, upgrade an existing install,
or verify that the binary works on your machine.

This is the development repository, `EffortlessMetrics/perl-lsp-swarm`.
Published release downloads intentionally come from
[`EffortlessMetrics/perl-lsp`](https://github.com/EffortlessMetrics/perl-lsp/releases).
To run this checkout's changes, use [Install From Source](#install-from-source);
installing a release or the crates.io package does not build this checkout.

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

- VS Code: install the `EffortlessMetrics.perl-lsp-rs` extension. It can download `perllsp` automatically when no configured, bundled, or PATH binary is selected; use **Perl: Show Server Version** to check the server it actually runs.
- macOS or Linux: use the [macOS and Linux manual archive](#macos-and-linux-manual-archive) until the release packet publishes an immutable installer identity and digest. The identity-bound [installer wrapper](#installer-script-macos-and-linux) becomes usable when those values exist.
- Windows: install from the [manual archive](#manual-archive), or use the [published PowerShell script](#published-powershell-script) for `perllsp.exe` only after reviewing its checksum limitations.
- Other editors: download a prebuilt binary from [GitHub Releases](https://github.com/EffortlessMetrics/perl-lsp/releases) and put it on your `PATH`.
- Cargo users: [install the published `perllsp` package](#cargo-published-package) with `cargo install perllsp --locked` (language server only).
- Local testing or pre-release validation: [install this checkout](#install-from-source) with `cargo install --path crates/perllsp --locked` from the repository root.

The crates.io package `perl-lsp` is a different project; install `perllsp`.

Inspect the install before wiring it into an editor. `--doctor` reports the
local Perl and workspace setup; it is a diagnostic report, not a CI gate.
`--health` is only a liveness probe that confirms the binary can execute:

```bash
perllsp --version
perllsp --doctor
```

If those commands are missing or report an older version, check
[PATH and binary selection](#add-the-binary-directory-to-path). For an
extension-managed installation, use the extension's **Perl: Show Server Version**
command; its private binary directory need not be on your shell's PATH.

## Before You Start

- Prebuilt archives and extension-managed downloads do not require Rust or a C/C++ toolchain. Cargo installs compile Rust code; see [build prerequisites](#build-prerequisites).
- The language server is a native executable. Install Perl separately to run Perl programs, tests, or the debugger; see [Perl setup](PERL_SETUP_TROUBLESHOOTING.md).
- Install in the environment where the editor runs the server. For WSL, use the Linux instructions inside WSL. For an SSH or container workspace, install inside that remote environment and configure the editor there.
- Keep the archive's extracted directory in a permanent location before adding it to PATH. Restart already-running terminals and editors after persistent PATH changes.

## Installer Script (macOS and Linux)

The repository maintains the installer logic at
[`scripts/install.sh`](../../scripts/install.sh). The root
[`install.sh`](../../install.sh) is only a bootstrap and argument-compatibility
wrapper. Both the wrapper and [`scripts/install.sh`](../../scripts/install.sh)
accept the two most common settings as fixed-slot positionals —
`bash install.sh <version> <install-dir>` is equivalent to
`VERSION=<version> INSTALL_DIR=<install-dir> bash install.sh`. Explicit
environment variables win the matching slot and do not shift a later
positional. A third positional fails closed. Remaining flags
(`--print-target`, `--with-claude`, `--help`) are unchanged.

From a clone, the wrapper executes the sibling `scripts/install.sh` directly:

```bash
git clone https://github.com/EffortlessMetrics/perl-lsp-swarm.git
cd perl-lsp-swarm
bash install.sh --help
# Review install.sh and scripts/install.sh, then install a published release:
bash install.sh
```

The clone-local installer downloads a published release by default; it does
not compile the checkout. To run your local source changes, use
[Install From Source](#install-from-source).

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
INSTALLER_REF='<full-40-char-commit-sha>'
INSTALLER_SHA256='<reviewed-sha256-of-scripts-install-sh>'

curl -fsSL "https://raw.githubusercontent.com/EffortlessMetrics/perl-lsp/$INSTALLER_REF/install.sh" \
  | PERL_LSP_INSTALLER_REF="$INSTALLER_REF" \
    PERL_LSP_INSTALLER_SHA256="$INSTALLER_SHA256" bash
```

Do not substitute a digest downloaded from the same unverified mutable source
at runtime. The digest must come from the reviewed release/topology closeout or
another independently reviewed repository record. Until such a digest is
published, use the [macOS and Linux manual archive](#macos-and-linux-manual-archive)
or the clone-local installer above.

Installer options remain environment variables on the `bash` side of the
pipeline:

```bash
curl -fsSL "https://raw.githubusercontent.com/EffortlessMetrics/perl-lsp/$INSTALLER_REF/install.sh" \
  | PERL_LSP_INSTALLER_REF="$INSTALLER_REF" \
    PERL_LSP_INSTALLER_SHA256="$INSTALLER_SHA256" \
    VERSION=v0.17.0 \
    INSTALL_DIR="$HOME/.local/bin" \
    PERL_LSP_LINUX_LIBC=musl bash
```

Supported release-archive platforms are Linux x86_64 and aarch64 (gnu or
musl), macOS x86_64, and macOS aarch64.

Termux is detected from `TERMUX_VERSION` or the Termux `usr/bin` directory.
There is no Android/bionic release archive, so a Termux host selects
source-build mode rather than a Linux gnu or musl asset. An explicit
`INSTALL_DIR` still wins over the Termux prefix default
(`/data/data/com.termux/files/usr/bin`).

This bootstrap boundary proves only the identity of the downloaded
`scripts/install.sh`. For POSIX release-download mode, the canonical installer
now requires a local SHA-256 implementation, a downloadable `SHA256SUMS`,
exactly one normalized row for the selected asset, and a matching lowercase
SHA-256 digest before extraction begins. Missing, duplicate, malformed, or
mismatched checksum evidence fails closed.

That is an artifact-integrity control, not independent publisher provenance:
the archive and checksum manifest are still co-hosted by the release. The
PowerShell installer also retains its separate fail-open checksum boundary.
Safe archive inspection for first-party standalone installers landed in
[#8352](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/8352).

POSIX inspection classifies each archive entry from its ustar header fields
rather than from `tar -t` / `tar -tv` output, so entry names and types do not
depend on which `tar` the host provides. That closes a divergence where BusyBox
tar reported a hardlink as a regular file and stripped `/` and `../` from the
names it printed, admitting members that GNU tar and bsdtar rejected
([#11508](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/11508)).
POSIX release-download mode therefore requires `od` alongside `tar` and `gzip`;
all three are present on the supported Linux and macOS hosts. Archives carrying
PAX (`x`/`g`) or GNU long-name (`L`/`K`) extended records fail closed, because
such a record can rewrite the path of the entry that follows it. The published
release archives do not use them: they are built with `tar czf` on the GNU and
macOS runners, and libarchive's restricted-pax default emits no extended record
for their short ASCII names and small metadata. Two further refusals come from
the same header walk. Bytes after the archive's own end-of-archive marker are
refused rather than ignored, because tar implementations disagree about whether
a lone zero block ends the archive — BusyBox tar reads past it — so a member
placed after that marker would be invisible to a reader that stops there and
still be extracted by one that does not. Stored names are also restricted to
printable ASCII excluding backslash; backslash is refused even though it is
printable because a rejected name is echoed into a diagnostic that renders
backslash escapes, so admitting it would let an archive forge installer output.
The archive-safety receipt records the resolved extractor profile (`gnu`,
`libarchive`, `busybox`, or `unknown`) so an install report names the tar it
ran under.
Those installers now promote a verified `perllsp` + `perl-dap` product unit as
one current selection
([#8359](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/8359)):
readers observe the previous complete unit or the new complete unit, never a
mixed or partial pair. POSIX local promotion tests and the GitHub-hosted
PowerShell checksum, archive safety, and product-unit promotion job both prove
the clone-local installer path. Publisher provenance, health-driven rollback, PATH
persistence, and hosted install-transition proof remain separate under
[#6097](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/6097),
[#7832](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/7832),
[#5903](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/5903), and
[#10746](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/10746).
Use the [macOS and Linux manual archive](#macos-and-linux-manual-archive) with
independently reviewed checksum or attestation evidence for release-sensitive
installation until those remaining boundaries land.

`BUILD_FROM_SOURCE=1` installs **`perllsp` only**, not `perl-dap`. That mode runs
`cargo install perllsp --locked` against crates.io; it does not build the local
checkout. To install this repository's code, follow
[Install From Source](#install-from-source). The crates.io package `perl-lsp` is a different project,
not this language server. The `perllsp` package declares just the one binary, so
the debug adapter is skipped without an error. If you need the
debugger, use a release archive instead — the archives ship both binaries — or
build `perl-dap` yourself from a clone with
`cargo build -p perl-dap --release`.

In source mode, `VERSION=latest` (the default) selects crates.io's latest
`perllsp` without querying GitHub Releases. The installer verifies the staged
binary's version before promotion and reports that registry version on completion.
An explicit `VERSION=vX.Y.Z` still requires that exact registry version.

## macOS and Linux manual archive

This is the working POSIX recovery path while the identity-bound piped
bootstrap is unpublished. It does not execute `install.sh`, and it does not
need `PERL_LSP_INSTALLER_REF` or `PERL_LSP_INSTALLER_SHA256`.

1. Open [GitHub Releases](https://github.com/EffortlessMetrics/perl-lsp/releases)
   and choose the `perllsp-<version>-<target>.tar.gz` whose suffix matches the
   [prebuilt table](#prebuilt-releases). Check the latest release page before
   copying a version number. The verified public-beta example on this page is
   `v0.17.0`.
2. Download that archive and the same release's `SHA256SUMS` file. Both are
   co-hosted by the release: matching them proves artifact integrity, not
   independent publisher provenance.
3. Verify the selected asset, then extract it. The published layout is a
   package directory named like the archive stem; it contains `perllsp` and
   `perl-dap`.

```bash
# Replace VERSION and TARGET after checking GitHub Releases.
VERSION=0.17.0
TARGET=x86_64-unknown-linux-gnu

# A subshell stops on failure without exiting your interactive shell.
(
  set -eu
  ASSET="perllsp-${VERSION}-${TARGET}.tar.gz"
  BASE="https://github.com/EffortlessMetrics/perl-lsp/releases/download/v${VERSION}"

  curl -fsSL -O "${BASE}/${ASSET}"
  curl -fsSL -O "${BASE}/SHA256SUMS"

  # Match the complete filename, not a substring such as "$ASSET.bak".
  # Accept the standard SHA256SUMS text or binary marker; refuse duplicates.
  ROW="$(awk -v asset="$ASSET" '
    { name = $2; sub(/^\*/, "", name) }
    name == asset {
      count++
      if (NF != 2 || length($1) != 64 || $1 !~ /^[0-9a-fA-F]+$/) bad = 1
      row = $0
    }
    END {
      if (count != 1 || bad) exit 1
      print row
    }
  ' SHA256SUMS)" || {
    printf 'Expected exactly one valid checksum for %s\n' "$ASSET" >&2
    exit 1
  }

  if command -v sha256sum >/dev/null 2>&1; then
    printf '%s\n' "$ROW" | sha256sum -c -
  elif command -v shasum >/dev/null 2>&1; then
    printf '%s\n' "$ROW" | shasum -a 256 -c -
  else
    printf 'Install sha256sum or shasum before continuing.\n' >&2
    exit 1
  fi && tar -xzf "$ASSET"
)
```

Continue only if download, verification, and extraction succeeded. From the
same directory, verify the extracted files directly before selecting them on
PATH. If you moved the extracted directory to a permanent location, set
`INSTALL_DIR` to that absolute directory instead:

```bash
INSTALL_DIR="$(pwd)/perllsp-${VERSION}-${TARGET}"
"$INSTALL_DIR/perllsp" --version
"$INSTALL_DIR/perl-dap" --version
export PATH="$INSTALL_DIR:$PATH"
command -v perllsp
command -v perl-dap
```

That export affects only the current shell. Follow the
[PATH persistence steps](#add-the-binary-directory-to-path) before opening a
new terminal or editor. The clone-local POSIX installer also prints PATH
instructions; it does not edit your shell startup files for you.

A reviewed clone is the other working POSIX alternative: from a checkout,
`bash install.sh` executes the sibling [`scripts/install.sh`](../../scripts/install.sh)
directly and never reaches the piped-bootstrap identity gate. See
[Installer Script (macOS and Linux)](#installer-script-macos-and-linux).

## Windows

Use the [manual archive](#manual-archive) for both `perllsp.exe` and
`perl-dap.exe`, or the published PowerShell script for `perllsp.exe` only.

### Published PowerShell script

The [published script at commit 866d832](https://github.com/EffortlessMetrics/perl-lsp/blob/866d83285b69c7a80276735cbdc2b20d66694d86/install.ps1) selects
`perllsp-<version>-x86_64-pc-windows-msvc.zip`. It supports x86_64 Windows
and selects the same x64 archive on Windows 11 ARM64, where x64 emulation is
available. Windows 10 ARM64 cannot run that archive; the script rejects the
fallback before downloading. Build from source there or on unsupported
architectures.

The script verifies the archive against the release `SHA256SUMS` file and
aborts on a checksum mismatch. If the manifest cannot be downloaded, has no
matching row, cannot be parsed, or the archive hash cannot be computed, it
warns and continues without verification. This is not fail-closed integrity
verification or independent publisher provenance.

It installs `perllsp.exe` into `%USERPROFILE%\.local\bin` by default. It does
not install `perl-dap.exe`, atomically promote a server/adapter pair, or
provide health-driven rollback. It may print PATH instructions, but a similar
directory name can make its PATH check report success incorrectly. It does
not update PATH automatically. Open **Edit environment variables for your
account** from Start, select **Path** under **User variables**, and add the
exact install directory as a separate entry if it is absent. Open a new
terminal and verify `perllsp --version` before configuring your editor.

Download and inspect this same script revision before running it. Pass
`-Version` to select a release and `-InstallDir` to change the install directory:

Use **PowerShell 7 (`pwsh`)** for this pinned published script. The older
published file has a known Windows PowerShell 5.1 parsing/encoding limitation;
changing execution policy or using `Unblock-File` does not repair its syntax.
If you only have Windows PowerShell 5.1, use the [manual archive](#manual-archive).

```powershell
irm https://raw.githubusercontent.com/EffortlessMetrics/perl-lsp/866d83285b69c7a80276735cbdc2b20d66694d86/install.ps1 -OutFile install.ps1
# Review install.ps1, then:
pwsh -File .\install.ps1 -Version 0.17.0 -InstallDir C:\tools\bin
```

If execution policy blocks the reviewed file in PowerShell 7, follow your
machine's script-execution policy; `Unblock-File .\install.ps1` removes the
download mark when that is the cause. The manual archive does not require
running an installer script.

### Development-checkout installer

[`install.ps1`](../../install.ps1) in this development repository has stronger
checks than the published script above. It requires `SHA256SUMS` before
downloading the release zip, requires exactly one lowercase SHA-256 row for
the selected asset, and verifies the archive before extraction. Missing,
duplicate, malformed, or mismatched checksum evidence fails closed. This
proves archive integrity against the co-hosted manifest, not independent
publisher provenance.

The clone-local installer also promotes `perllsp.exe` and `perl-dap.exe` as
one product unit through an immutable candidate directory and an atomic
current pointer. Windows uses a file `current` pointer replaced with
`MoveFileEx` and PATH-visible `perllsp.cmd` / `perl-dap.cmd` shims that follow
that pointer, without requiring privileged file symlinks or independent PATH
copies. Clone-local PowerShell product-unit promotion is proven by the hosted
Windows installer-powershell-checksum-contract job on
[#12815](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/12815).
Health confirmation, whole-unit rollback after failed startup, and PATH
persistence remain separate claims. These development guarantees do not
describe the script downloaded from the publication repository above.

### Manual archive

This route installs both binaries without running an installer script. The
commands below work in Windows PowerShell 5.1 and PowerShell 7.

1. Open [GitHub Releases](https://github.com/EffortlessMetrics/perl-lsp/releases)
   and choose a release. Download its
   `perllsp-<version>-x86_64-pc-windows-msvc.zip` and `SHA256SUMS` into a new,
   empty download directory. Use both files from the same release.
2. Open PowerShell in that directory. Set the filename to the archive you
   downloaded; `0.17.0` below is the verified public-beta example:

```powershell
$Archive = 'perllsp-0.17.0-x86_64-pc-windows-msvc.zip'
Get-FileHash -LiteralPath $Archive -Algorithm SHA256 -ErrorAction Stop |
    Format-List Algorithm, Hash, Path
```

3. Open `SHA256SUMS` in a text editor. Find exactly one row whose complete
   filename is `$Archive`'s value, allowing the checksum format's optional
   `*` before the filename. A row for another filename, such as an archive
   ending in `.zip.bak`, is not a match. Compare the entire 64-character
   hexadecimal digest in that row with `Hash` above; letter case does not
   matter. **Do not extract or run the archive** if either download failed,
   the hash command failed, the row is missing, duplicated or malformed, or
   the hashes differ. This manual comparison verifies integrity against the
   co-hosted manifest, not independent publisher provenance. See Microsoft's
   [`Get-FileHash` reference](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.utility/get-filehash?view=powershell-5.1).
4. Only after the checksum matches, extract into a new permanent directory.
   Change the version directory to match your release; choose an unused
   directory so files from an older installation cannot be mixed in:

```powershell
$InstallDir = Join-Path $env:LOCALAPPDATA 'Programs\perllsp\0.17.0'
Expand-Archive -LiteralPath $Archive -DestinationPath $InstallDir -ErrorAction Stop
```

5. After extraction succeeds, verify the binaries by their exact paths:

```powershell
& "$InstallDir\perllsp.exe" --version
& "$InstallDir\perl-dap.exe" --version
```

The Windows zip places both executables directly in the extraction directory.
Add that directory to PATH using the
[Windows PATH steps](#add-the-binary-directory-to-path), or configure your
editor with the absolute path to `perllsp.exe`.

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
the direct route to the published `perllsp.exe` and `perl-dap.exe` pair.

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

## Cargo: Published Package

This compiles the independently published crates.io package. It does not use
the code in a local `perl-lsp-swarm` checkout. Complete the
[build prerequisites](#build-prerequisites).
The crates.io package `perl-lsp` is a different project; use `perllsp`:

```bash
cargo install perllsp --locked
```

`--locked` uses the package's lockfile. This installs **`perllsp` only**;
it does not install `perl-dap`. For the published server/adapter pair, use a
[release archive](#prebuilt-releases).

Add the directory containing Cargo's installed executable to PATH, then
[verify the selected executable](#add-the-binary-directory-to-path). Check the
installed version against the channel you intended to use; crates.io and
GitHub Releases are independently versioned.

## Install From Source

Use this to install and test the code in **this development repository**.
A checkout build is not the same artifact as a published release.

### Build prerequisites

Install Rust and Cargo using the [official Rust instructions](https://www.rust-lang.org/tools/install/).
The workspace requires Rust 1.95 or newer and pins Rust **1.95.0** in
[`rust-toolchain.toml`](../../rust-toolchain.toml); rustup selects that pinned
toolchain when you run commands inside the checkout. Git is required to clone
the repository. You also need the native build tools for your platform:

| Platform | Build tools |
| --- | --- |
| Linux | A C/C++ compiler, linker, and development headers from your distribution's build-tools packages |
| macOS | Xcode Command Line Tools (`xcode-select --install`) |
| Windows with the MSVC target | Visual Studio Build Tools with the C++ build tools and Windows SDK; follow the Rust installer's prompts |

These build dependencies are not needed to run a prebuilt release archive.

### Install this checkout

Run from the repository root after cloning:

```bash
git clone https://github.com/EffortlessMetrics/perl-lsp-swarm.git
cd perl-lsp-swarm
rustc --version
cargo --version
cargo install --path crates/perllsp --locked
```

This builds and installs the `perllsp` package from `crates/perllsp`, using the
workspace lockfile. A plain `cargo build` only creates build output; it does not
replace the executable your shell or editor finds on PATH. Installing by
package name without `--path` selects the registry instead of this checkout.

If you also want to test the debug adapter from this checkout:

```bash
cargo install --path crates/perl-dap --locked --bin perl-dap
```

These are separate Cargo installs, not the standalone installer's atomic
server/adapter promotion. Running or debugging Perl code also requires Perl.

To keep a development server separate from an existing installation, use a
different install root instead of overwriting it:

```bash
cargo install --path crates/perllsp --locked --root ./target/install-preview
```

Run that exact binary as `./target/install-preview/bin/perllsp --version` on
Linux/macOS, or `.\target\install-preview\bin\perllsp.exe --version` in
PowerShell. Configure your editor with its absolute path. For development
checks and contribution workflow, see [CONTRIBUTING.md](../../CONTRIBUTING.md).

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

### Add the binary directory to PATH

For a standalone install, add the directory that actually contains `perllsp`
(`perllsp.exe` on Windows). For an archive, this is the extracted binary
directory. For Cargo, use the directory shown by its installation output.
Cargo normally uses `$HOME/.cargo/bin` on Unix or
`%USERPROFILE%\.cargo\bin` on Windows; `CARGO_HOME`, `CARGO_INSTALL_ROOT`,
Cargo configuration, or `--root` can change that location.

For the default Cargo location in **Bash or Zsh**:

```bash
export PATH="$HOME/.cargo/bin:$PATH"
type -a perllsp
```

Substitute the actual binary directory for an archive or custom Cargo root.
To persist the change, add the export with that stable path to the startup
file your shell reads (for example, `~/.bashrc` or `~/.zshrc`). Do not put a
temporary `$(pwd)` expression or an unset `INSTALL_DIR` variable in that file.
Open a new terminal and restart your editor. An editor launched from a desktop
menu may need an explicit server path even when your shell resolves it.

For the default Cargo location in **PowerShell**, update the current session:

```powershell
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"
Get-Command perllsp -All | Select-Object CommandType, Source
```

For persistence, open **Edit environment variables for your account**, select
**Path** under **User variables**, and add the actual binary directory as its
own entry. Then open a new terminal and restart your editor. `Get-Command
perllsp -All` (or `type -a perllsp` on Unix) helps identify an older copy earlier
on PATH. Use an absolute executable path when you need to select a specific
installation.

### Verify before configuring the editor

Run these from your Perl project's directory using the executable selected
above:

```bash
perllsp --version
perllsp --health
perllsp --doctor
```

`--health` prints `ok <version>` and confirms that the executable starts.
`--doctor` describes Perl, workspace, and module-lookup configuration; it is
a diagnostic report, not a readiness gate. For CI, check the specific paths
and dependencies your job needs. If you installed the debugger, also run
`perl-dap --version` using that install's binary path.

Generic LSP clients should launch the command `perllsp` with the argument
`--stdio`; see [Editor Setup](EDITOR_SETUP.md). Running `perllsp --stdio`
manually waits for LSP messages and can appear idle. Use the checks above for
a shell smoke check; press Ctrl+C if you started stdio interactively.

### Select a development binary in VS Code

The extension can manage its own downloaded server. Installing a binary with
Cargo does not by itself select it in the extension. To test your local build,
set these in **User Settings (JSON)**, replacing the path with the absolute
path to your installed executable:

```json
{
  "perl-lsp.serverPath": "/absolute/path/to/perllsp",
  "perl-lsp.autoDownload": false
}
```

On Windows, use a path such as `C:/tools/bin/perllsp.exe` (forward slashes
avoid JSON backslash escaping). Run **Perl: Restart Perl Language Server**,
then **Perl: Show Server Version** from the Command Palette. See the
[VS Code guide](../EDITORS/VS_CODE_SETUP.md) for complete configuration.

## Release Maintainers

If you are preparing a release, keep this page aligned with
[RELEASE.md](../../RELEASE.md) and
[project/PUBLISHING_ROADMAP.md](../project/PUBLISHING_ROADMAP.md). The release
workflow and final checks live there, not here.

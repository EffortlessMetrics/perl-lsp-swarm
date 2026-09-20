# GA Release Runbook (Forward-Looking)

> **Note**: This is a forward-looking planning document for a future GA release.
> The tree is at v0.17.0 (public beta) and the GA milestone has not been reached.
> Content below is retained as planning documentation.
>
> The worked examples have not been re-verified against the current release
> process. Steps 5, 8, and 9 have been corrected against the shipping asset
> names; treat the rest as a template to check, not a script to
> run. [`docs/RELEASE_PROCESS.md`](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/docs/RELEASE_PROCESS.md)
> and [`RELEASE.md`](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/RELEASE.md)
> are the current authority.

This document provides a template for a future general availability release.

Every version reference below is `$VERSION`, set once in step 0. It previously
hardcoded a different version in three places — the tag, the Homebrew formula,
and the extension bump — so following the steps in order produced three
different releases in one pass, none of them the one being shipped (#5464). Do
not reintroduce a literal version anywhere in this document.

## Pre-flight Checklist

- [ ] All tests passing (`cargo test --all`)
- [ ] Property tests passing (`PROPTEST_CASES=64 cargo test -p perl-parser --tests 'prop_'`)
- [ ] No clippy warnings (`cargo clippy --all --all-targets`)
- [ ] Benchmarks show no regression (`cargo bench`)
- [ ] CHANGELOG.md updated with `v$VERSION` entries
- [ ] README.md installation instructions current

## Day-of Release Process

### 0. Set the release version once (1 min)

Every later step reads `$VERSION`, so run this in the shell you will use for
the whole release and do not edit versions inline anywhere below.

The mismatch check below exits rather than warning. If you paste it into an
interactive shell that exit ends the session — that is the intended loud
failure: reopen, set `VERSION`, and start again. A warning here would scroll
past and let step 1 rewrite ~190 version sites to the placeholder.

```bash
# The version being shipped. Everything else derives from it.
VERSION="0.0.0"          # ← set this, once
TAG="v$VERSION"

# It must already match the workspace, which `cargo xtask bump-version` owns.
# Halt rather than warn: the case this catches is the placeholder above being
# left in place, and a warning that scrolls past lets step 1 run
# `bump-version 0.0.0` across ~190 sites and cut a release nobody asked for.
if [ "$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)" != "$VERSION" ]; then
  echo "workspace version != $VERSION — run: cargo xtask bump-version $VERSION" >&2
  exit 1
fi
```

### 1. Final Version Bump (5 min)

```bash
# One command updates every workspace-version site (~190 references: root and
# member Cargo.toml, the extension manifest and lockfile, features.toml, docs).
# Hand-editing individual manifests is what let these drift apart.
cargo xtask bump-version "$VERSION"

# Update lock file
cargo update

# Verify nothing is out of sync, then that it builds
cargo xtask check-version-sync
cargo build -p perllsp --bin perllsp --release
```

### 2. Create & Push Tag (2 min)

The canonical path is the `release-orchestration.yml` workflow, which validates
that the workspace version matches the requested one, checks CI state, and
creates the tag server-side with the correct changelog annotation. Prefer it;
it prevents the "tag points at the wrong commit" class of error that manual
tagging invites. See
[`RELEASE.md`](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/RELEASE.md).

The manual equivalent, for a release where the workflow is unavailable:

```bash
# Commit version changes
git add -A
git commit -m "chore: release v$VERSION

$(sed -n "/^## \[$VERSION\]/,/^## \[/p" CHANGELOG.md | sed '$d')"

# Create and push tag. The default branch is `main`; pushing to `master` either
# fails or lands the release commit on a branch nothing releases from.
git tag -a "$TAG" -m "Release $TAG"
git push origin main
git push origin "$TAG"
```

The commit body is taken from the changelog entry rather than retyped, so the
release commit cannot claim capabilities the changelog does not.

### 3. Monitor CI Release (10-15 min)

1. Go to: https://github.com/EffortlessMetrics/perl-lsp/actions
2. Watch the "Release" workflow triggered by the tag
3. Verify all platform builds succeed
4. Check that artifacts are attached to the release

### 4. Get Checksums from Release (2 min)

Once the GitHub release is created:

1. Go to: `https://github.com/EffortlessMetrics/perl-lsp/releases/tag/$TAG`
2. Download `SHA256SUMS` file
3. Extract checksums for each platform:

```bash
# Example checksums (replace with actual values)
LINUX_X64_SHA256="abc123..."
LINUX_ARM64_SHA256="def456..."
MACOS_X64_SHA256="ghi789..."
MACOS_ARM64_SHA256="jkl012..."
WINDOWS_X64_SHA256="mno345..."
```

### 5. Update Installers with Checksums (5 min)

#### Update install.sh

```bash
# Already points to latest release, no changes needed
# Checksums are fetched from GitHub
```

#### Update install.ps1

```bash
# Already points to latest release, no changes needed
# Checksums are fetched from GitHub
```

#### Sync both installers to the publication repo — required

Users fetch `install.sh` and `install.ps1` from the publication repo
(`EffortlessMetrics/perl-lsp`, branch `master`), not from this one. Neither
script is version-pinned, so no per-release edit is needed — but the
publication copy must actually match this repository.

It currently does not. The published `install.ps1` still carries the
pre-rename `$Name = "perl-lsp"` and therefore builds an asset name that does
not exist:

```text
perllsp-0.17.0-x86_64-pc-windows-msvc.zip   -> 200
perl-lsp-0.17.0-x86_64-pc-windows-msvc.zip  -> 404
```

Verify the publication copy before announcing a release:

```bash
curl -fsSL https://raw.githubusercontent.com/EffortlessMetrics/perl-lsp/master/install.ps1 \
  | grep '^\$Name'   # must print: $Name = "perllsp"
```

If it does not match, sync the publication repo
([#4348](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/4348))
before publishing any Windows install instruction. Until then, document the
manual archive for Windows
([#5461](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/5461)).

### 6. Create Homebrew Formula (10 min)

Create a new repository `homebrew-tap` if it doesn't exist:

```bash
# Create tap repository
mkdir homebrew-tap
cd homebrew-tap
git init
mkdir Formula
```

Create `Formula/perllsp.rb`:

```ruby
class Perllsp < Formula
  desc "Native Rust language server and debug adapter for Perl"
  homepage "https://github.com/EffortlessMetrics/perl-lsp"
  version "$VERSION"   # substitute the value from step 0; do not hardcode
  license any_of: ["MIT", "Apache-2.0"]

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/EffortlessMetrics/perl-lsp/releases/download/v#{version}/perllsp-#{version}-aarch64-apple-darwin.tar.gz"
      sha256 "ACTUAL_SHA256_FROM_RELEASE"
    else
      url "https://github.com/EffortlessMetrics/perl-lsp/releases/download/v#{version}/perllsp-#{version}-x86_64-apple-darwin.tar.gz"
      sha256 "ACTUAL_SHA256_FROM_RELEASE"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/EffortlessMetrics/perl-lsp/releases/download/v#{version}/perllsp-#{version}-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "ACTUAL_SHA256_FROM_RELEASE"
    else
      url "https://github.com/EffortlessMetrics/perl-lsp/releases/download/v#{version}/perllsp-#{version}-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "ACTUAL_SHA256_FROM_RELEASE"
    end
  end

  def install
    extracted_dir = Dir.glob("perllsp-#{version}-*").find { |path| File.directory?(path) }
    raise "expected release archive directory perllsp-#{version}-<target>" unless extracted_dir

    bin.install "#{extracted_dir}/perllsp"
    bin.install "#{extracted_dir}/perl-dap"
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/perllsp --version")
    assert_match version.to_s, shell_output("#{bin}/perl-dap --version")
  end
end
```

Push the tap:

```bash
git add Formula/perllsp.rb
git commit -m "Add perllsp v$VERSION"
git remote add origin https://github.com/EffortlessMetrics/homebrew-tap.git
git push -u origin main
```

Test the formula:

```bash
brew install effortlessmetrics/tap/perllsp
perllsp --version
```

### 7. VS Code Extension (if ready)

If the VS Code extension is ready:

1. Confirm `vscode-extension/package.json` is at `$VERSION` — `cargo xtask bump-version` in step 1 already wrote it; if it disagrees, stop and re-run that step rather than editing by hand
2. Update binary download URLs and checksums
3. Build: `npm run bundle`
4. Package: `vsce package`
5. Publish: `vsce publish`

### 8. Update Documentation (5 min)

The install surfaces are owned by the documents themselves, not by this
runbook. Do not paste an install section from here — check the owning
documents against the release that just shipped:

| Surface | What to confirm |
| --- | --- |
| [`README.md`](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/README.md) Install | Commands still run on the new version |
| [`docs/how-to/INSTALLATION.md`](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/docs/how-to/INSTALLATION.md) | Per-platform detail, including the Windows caveats |
| [`docs/tutorials/GETTING_STARTED.md`](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/docs/tutorials/GETTING_STARTED.md) | First-run path still matches shipped behavior |

Only change these when the release actually changed installation behavior.
Restating install commands in a fourth place is what produced the drift this
step now guards against.

Windows specifically: publish the manual archive, not the piped
`install.ps1` one-liner, until the publication repo is synced (step 5).

### 9. Announce Release (10 min)

#### GitHub Release Notes

Update the auto-generated release notes with the template below.

Windows note for the operator, not for the notes themselves: announce the
release archive, never the piped `install.ps1` one-liner, until the
publication repo is synced (step 5). The one-liner 404s.

````markdown
# perl-lsp v$VERSION

## 🎉 Major Release

### ✨ Highlights

<!--
Quote current, verified numbers from docs/project/CURRENT_STATUS.md — the
document that carries the receipts — and re-check each one against this
release's own run. This section previously templated a total-coverage headline
and a "World-Class Performance" claim unconditionally, so every release
republished figures nobody re-verified (#5464). A headline with no receipt
behind it is a support claim we cannot answer for.
-->

- **Coverage**: <current figure from docs/project/CURRENT_STATUS.md>
- **Features**: <current LSP/DAP feature count from features.toml>
- **Performance**: <current measured figure, with the workload it was measured on>
- **Multi-Platform**: Linux and macOS (x86_64 & ARM64), Windows (x86_64)

### 🚀 Quick Install

```bash
# Unix
curl -fsSL https://raw.githubusercontent.com/EffortlessMetrics/perl-lsp/master/install.sh | bash

# Homebrew
brew install effortlessmetrics/tap/perllsp
```

Windows: download `perllsp-<version>-x86_64-pc-windows-msvc.zip` from the
release assets, extract it, and add the extracted directory to `PATH`.

### 📊 Performance

- Parser: <current measured figure, with the workload it was measured on>
- LSP: <current measured latency, with the operations it covers>
- Memory: Efficient caching with LRU eviction

Same rule as Highlights: a number here is a support claim. Take it from
docs/project/CURRENT_STATUS.md and re-verify it for this release, or drop the
line. "for all operations" in particular was a claim nothing measured.

### 🔧 What's New

- Perl::Critic integration
- Enhanced UTF-16 position handling
- Property-based testing infrastructure
- Improved fallback handlers
- Multi-message LSP protocol support

### 📚 Documentation

- [Getting Started](https://github.com/EffortlessMetrics/perl-lsp/blob/master/docs/tutorials/GETTING_STARTED.md)
- [LSP Features](https://github.com/EffortlessMetrics/perl-lsp/blob/master/docs/explanation/LSP_DOCUMENTATION.md)
- [Troubleshooting](https://github.com/EffortlessMetrics/perl-lsp/blob/master/docs/how-to/TROUBLESHOOTING.md)

### 🙏 Contributors

Thank you to everyone who contributed to this release!
````

#### Social Media

Twitter/X:
```
🚀 perl-lsp v$VERSION is here!

⚡ Fast native Rust parser
🛠️ <current LSP/DAP feature count>
🧪 Property-based testing
(any coverage or performance number here must come from
docs/project/CURRENT_STATUS.md, verified for this release)

Install: curl -fsSL https://raw.githubusercontent.com/EffortlessMetrics/perl-lsp/master/install.sh | bash

#Perl #LSP #RustLang
```

Reddit (r/perl):
```
Title: perl-lsp v$VERSION Released - Perl Language Server

We're excited to announce perl-lsp v$VERSION.

Features:
- <current LSP/DAP feature count> (completion, hover, refactoring, etc.)
- Fast native Rust parser
- Works with VSCode, Neovim, Emacs, and any LSP editor
- Zero C dependencies

Installation is now one line:
curl -fsSL https://raw.githubusercontent.com/EffortlessMetrics/perl-lsp/master/install.sh | bash

GitHub: https://github.com/EffortlessMetrics/perl-lsp
```

## Post-Release Checklist

- [ ] Verify installers work on fresh systems
- [ ] Test Homebrew formula on macOS
- [ ] Check download counts after 24 hours
- [ ] Monitor issues for installation problems
- [ ] Update crates.io if publishing there

## Rollback Plan

If critical issues are found:

```bash
# Delete the tag locally and remotely
git tag -d "$TAG"
git push --delete origin "$TAG"

# Revert the commit
git revert HEAD

# Fix the issue and re-release as the next patch version
```

## Success Metrics (First Week)

- [ ] 100+ downloads
- [ ] No critical bugs reported
- [ ] Positive feedback on social media
- [ ] VS Code extension installs (if published)

## Contact for Issues

- GitHub Issues: https://github.com/EffortlessMetrics/perl-lsp/issues
- Discord: [Create invite link]
- Email: [Your email]

---

**Estimated Total Time: 45-60 minutes**

Good luck with the release! 🚀

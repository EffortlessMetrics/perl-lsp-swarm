# GA Release Runbook (Forward-Looking)

> **Note**: This is a forward-looking planning document for a future GA release (v0.15.0+).
> The project is currently at v0.9.x (Initial Public Alpha). The GA milestone has not been reached.
> Content below is retained as planning documentation and will be updated when GA readiness is assessed.

This document provides a template for a future general availability release.

## Pre-flight Checklist

- [ ] All tests passing (`cargo test --all`)
- [ ] Property tests passing (`PROPTEST_CASES=64 cargo test -p perl-parser --tests 'prop_'`)
- [ ] No clippy warnings (`cargo clippy --all --all-targets`)
- [ ] Benchmarks show no regression (`cargo bench`)
- [ ] CHANGELOG.md updated with v0.8.3 entries
- [ ] README.md installation instructions current

## Day-of Release Process

### 1. Final Version Bump (5 min)

```bash
# Update version in all Cargo.toml files
VERSION="0.8.3"
sed -i "s/^version = \".*\"/version = \"$VERSION\"/" crates/perl-parser/Cargo.toml
sed -i "s/^version = \".*\"/version = \"$VERSION\"/" crates/perl-lexer/Cargo.toml
sed -i "s/^version = \".*\"/version = \"$VERSION\"/" crates/tree-sitter-perl-rs/Cargo.toml

# Update lock file
cargo update

# Verify builds
cargo build -p perl-parser --bin perllsp --release
```

### 2. Create & Push Tag (2 min)

```bash
# Commit version changes
git add -A
git commit -m "chore: release v0.8.3

- Perl::Critic integration
- Enhanced UTF-16 position handling
- Property-based testing infrastructure
- 141/141 edge cases passing
- 35+ IDE features"

# Create and push tag
git tag -a "v0.8.3" -m "Release v0.8.3"
git push origin master
git push origin v0.8.3
```

### 3. Monitor CI Release (10-15 min)

1. Go to: https://github.com/EffortlessMetrics/perl-lsp/actions
2. Watch the "Release" workflow triggered by the tag
3. Verify all platform builds succeed
4. Check that artifacts are attached to the release

### 4. Get Checksums from Release (2 min)

Once the GitHub release is created:

1. Go to: https://github.com/EffortlessMetrics/perl-lsp/releases/tag/v0.8.3
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
  version "0.13.1"
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
    package_dir = extracted_dir || "."

    bin.install "#{package_dir}/perllsp"
    bin.install "#{package_dir}/perl-dap"
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
git commit -m "Add perllsp v0.13.1"
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

1. Update version in `package.json` to `0.6.0`
2. Update binary download URLs and checksums
3. Build: `npm run compile`
4. Package: `vsce package`
5. Publish: `vsce publish`

### 8. Update Documentation (5 min)

Review — do not replace — the README installation section.

[`README.md`](../../README.md#install) and
[`docs/how-to/INSTALLATION.md`](../how-to/INSTALLATION.md) are the authority for
install commands. This runbook previously carried its own copy of that section
for the operator to paste over the README; that copy went stale and instructed
publishing a Windows one-liner that 404s for every user
([#5461](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/5461)).
Pasting a runbook template over the front door is how the front door regresses.

Instead, confirm each command in those two files still runs against the
candidate release, and correct them in place where it does not. In particular:

- the Windows PowerShell one-liner stays documented as unusable until the
  publication repo is synced
  ([#4348](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/4348));
- `perl-dap` is installed by the POSIX script only
  ([#5036](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/5036));
- Windows ARM64 is not a published target
  ([#5007](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/5007)).

### 9. Announce Release (10 min)

#### GitHub Release Notes

Update the auto-generated release notes with:

```markdown
# perl-lsp v0.8.3

## 🎉 Major Release

This release marks perl-lsp with comprehensive edge case coverage and broad feature support.

### ✨ Highlights

- **100% Edge Case Coverage**: All 141 edge cases passing
- **35+ IDE Features**: Complete LSP implementation
- **World-Class Performance**: 1-150µs parsing times
- **Property-Based Testing**: Comprehensive test infrastructure
- **Multi-Platform**: Linux, macOS, Windows (x86_64)

### 🚀 Quick Install

```bash
# macOS and Linux
curl -fsSL https://raw.githubusercontent.com/EffortlessMetrics/perl-lsp/master/install.sh | bash

# Homebrew
brew install effortlessmetrics/tap/perllsp
```

Windows (x86_64): download `perllsp-<version>-x86_64-pc-windows-msvc.zip` from
the release assets, extract it, and put the folder containing `perllsp.exe` on
your `PATH`.

### 📊 Performance

- Parser: Fast native Rust implementation (1-150us parsing)
- LSP: <50ms response time for all operations
- Memory: Efficient caching with LRU eviction

### 🔧 What's New

- Perl::Critic integration
- Enhanced UTF-16 position handling
- Property-based testing infrastructure
- Improved fallback handlers
- Multi-message LSP protocol support

### 📚 Documentation

- [Getting Started](../tutorials/GETTING_STARTED.md)
- [LSP Features](../explanation/LSP_DOCUMENTATION.md)
- [Troubleshooting](../how-to/TROUBLESHOOTING.md)

### 🙏 Contributors

Thank you to everyone who contributed to this release!
```

#### Social Media

Twitter/X:
```
🚀 perl-lsp v0.8.3 is here!

✅ 100% Perl edge case coverage
⚡ Fast native Rust parser
🛠️ 35+ IDE features
🧪 Property-based testing

Install: curl -fsSL https://raw.githubusercontent.com/EffortlessMetrics/perl-lsp/master/install.sh | bash

#Perl #LSP #RustLang
```

Reddit (r/perl):
```
Title: perl-lsp v0.8.3 Released - Perl Language Server

We're excited to announce perl-lsp v0.8.3, a Perl language server with comprehensive edge case coverage!

Features:
- 35+ IDE features (completion, hover, refactoring, etc.)
- Fast native Rust parser (1-150us parsing)
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
git tag -d v0.8.3
git push --delete origin v0.8.3

# Revert the commit
git revert HEAD

# Fix the issue and re-release as v0.8.4
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

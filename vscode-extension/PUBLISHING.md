# Publishing the Perl Language Server Extension

## Prerequisites

1. **Node.js 26.x (CI pins 26.5.0) and npm 11.18.0** installed
2. **Visual Studio Code** installed
Publisher credentials belong to the reviewed release workflow; local preparation
does not require or authorize ambient registry tokens.

## Build Process

### 1. Build the LSP Binary

First, ensure the `perllsp` binary is built:

```bash
# From the project root
cd ..
cargo build -p perllsp --release
```

### 2. Build and Validate the Extension

```bash
# From vscode-extension/
npm run doctor
npm ci
npm run verify:marketplace
```

`verify:marketplace` runs TypeScript compilation, bundles the local platform binary, and generates the Marketplace `.vsix` package.

Set `CARGO_TARGET_DIR` when running this from an agent or release-prep
worktree so Cargo build output stays outside the repository:

```bash
CARGO_TARGET_DIR=/tmp/perl-lsp-vsix-target npm run verify:marketplace
```

To smoke the generated VSIX in a clean VS Code profile:

```bash
PERL_LSP_PUBLISHED_EXTENSION_SOURCE=vsix \
PERL_LSP_PUBLISHED_VSIX_PATH="$PWD/perl-lsp-rs-<version>.vsix" \
PERL_LSP_PUBLISHED_EXTENSION_VERSION=<version> \
PERL_LSP_REQUIRE_STRUCTURED_COMMANDS=1 \
PERL_LSP_SMOKE_RECEIPTS_DIR=/tmp/perl-lsp-vsix-smoke-receipts \
npm run test:published
```

This installs the VSIX, activates the published-extension harness, and writes
managed-binary smoke receipts under the configured receipts directory.

To validate the Open VSX tooling before publishing:

```bash
npm run check:openvsx
```

Open VSX receives the same selected `perl-lsp-rs-<version>.vsix` artifact as the
Visual Studio Marketplace. Local tooling checks do not establish publisher
authority; the canonical workflow owns credential validation.

### 3. Test Locally

Install and test the extension locally:

```bash
# Install the VSIX file
code --install-extension perl-lsp-rs-<version>.vsix

# Open test file
code test/sample.pl
```

Test these features:

- [ ] Syntax highlighting works
- [ ] Diagnostics appear for syntax errors
- [ ] Format document (Shift+Alt+F) works (native formatter; no perltidy required)
- [ ] Go to definition (F12) works
- [ ] Hover shows information
- [ ] Auto-completion triggers

### 4. Cross-Platform Binaries

For marketplace release, build for all platforms:

```bash
# Linux x64
cargo build --target x86_64-unknown-linux-gnu --release

# macOS x64
cargo build --target x86_64-apple-darwin --release

# macOS ARM64
cargo build --target aarch64-apple-darwin --release

# Windows x64
cargo build --target x86_64-pc-windows-msvc --release
```

Place binaries in appropriate directories:

- `bin/linux-x64/perllsp`
- `bin/darwin-x64/perllsp`
- `bin/darwin-arm64/perllsp`
- `bin/win32-x64/perllsp.exe`

### 5. Use the reviewed release transaction

The supported `npm run publish` and `npm run publish:openvsx` aliases are retired
and refuse (#16802). Command-line tokens and ambient `VSCE_PAT`/`OVSX_PAT` do not
enable those aliases. This does not prevent arbitrary direct use of installed
publisher tools or the distinct `npm publish` lifecycle command.

Follow the reviewed [release transaction](../docs/release/RUNBOOK.md) (#13768 / #6055).
The existing `publish-extension.yml` workflow consumes the selected VSIX and owns
its publisher credentials. Local packaging/inspection is rehearsal only (#16786 / #4350).
Dispatch acceptance is not publication or completion (#16799).

## Post-Publishing

1. **Verify on Marketplace and Open VSX**
   - Go to https://marketplace.visualstudio.com/
   - Search for "Perl Language Server"
   - Verify description, screenshots, install command, and version
   - Open https://open-vsx.org/extension/EffortlessMetrics/perl-lsp-rs
   - Verify the Open VSX listing, version, and VSCodium install instructions

2. **Update Documentation**
   - Update main README.md with marketplace link
   - Refresh the manually maintained VS Marketplace installs badge count/date in:
     - `/README.md`
     - `/vscode-extension/README.md`
   - Add installation instructions
   - Update CHANGELOG.md

3. **Verify the controller-owned GitHub release**
   - Check the exact tag, release and selected VSIX against the reviewed transaction.
   - Report missing or mismatched artifacts to the controller; do not create a parallel release.

## Maintenance

### Updating the Extension

1. Update version in `package.json`
2. Update `CHANGELOG.md`
3. Rebuild and test
4. Submit the exact candidate to the reviewed #13768 / #6055 release transaction.

### Monitoring

- Check reviews and ratings on marketplace
- Monitor GitHub issues
- Respond to user feedback

## Troubleshooting

### "Missing publisher name"

Update `package.json` with your publisher ID.

### "Personal Access Token expired"

Report the failed canonical workflow run to the release controller so its owner
can repair credential configuration. Local login does not authorize a retry.

### Binary not found

Ensure `bundle-lsp.js` correctly detects platform and copies binaries.

### Large package size

Check `.vscodeignore` is excluding unnecessary files.

## Marketplace Launch Checklist

Before first public launch:

- [ ] Confirm `package.json` metadata is complete (`publisher`, `icon`, repository links, categories, keywords).
- [ ] Ensure `README.md` has clear install + configuration guidance.
- [ ] Ensure `CHANGELOG.md` includes release notes for the exact published version.
- [ ] Run `npm run verify:marketplace` and install the generated `.vsix` locally.
- [ ] Validate extension activation in a clean profile (`code --user-data-dir <tmpdir>`).
- [ ] Verify binary download fallback works when no bundled binary exists for the host platform.
- [ ] Have the reviewed controller select the release channel and authorize any later promotion using the exact validated candidate.

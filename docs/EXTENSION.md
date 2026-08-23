# VS Code Extension Guide

## Installation

### From the VS Code Marketplace
1. Open VS Code.
2. Open Extensions.
3. Search for `Perl Language Server`.
4. Install `EffortlessMetrics.perl-lsp-rs`.

### From a VSIX
Download the `.vsix` asset from the matching GitHub release and install it manually:

```bash
code --install-extension perl-lsp-rs-*.vsix
```

## Server Resolution Order

The extension resolves the `perl-lsp` binary in this order:
1. `perl-lsp.serverPath`
2. A bundled development binary inside the extension, if one is present
3. `perl-lsp` on your `PATH`
4. Automatic download from GitHub releases when `perl-lsp.autoDownload` is enabled

The Marketplace package is designed to work with `PATH`, `serverPath`, or runtime download. It does not rely on a platform-specific binary being pre-bundled into the published VSIX.

## Settings

| Setting | Default | Description |
|---------|---------|-------------|
| `perl-lsp.channel` | `"latest"` | Release channel used for runtime download resolution. |
| `perl-lsp.versionTag` | `""` | Specific GitHub release tag when `channel` is `tag`. |
| `perl-lsp.serverPath` | `""` | Absolute path to a `perl-lsp` binary. |
| `perl-lsp.autoDownload` | `true` | Download `perl-lsp` automatically when it is not bundled, configured, or on `PATH`. |
| `perl-lsp.downloadBaseUrl` | `""` | Override the download host for internal mirrors. |
| `perl-lsp.trace.server` | `"off"` | LSP trace level: `off`, `messages`, or `verbose`. |
| `perl-lsp.enableSemanticTokens` | `true` | Enable semantic token highlighting. |
| `perl-lsp.enableFormatting` | `true` | Enable formatting integration. |
| `perl-lsp.formatOnSave` | `false` | Request formatting on save. |
| `perl-lsp.perltidyConfig` | `""` | Path to a `.perltidyrc` compatibility file. Native formatting is the default path. |
| `perl-lsp.includePaths` | `["lib", "local/lib/perl5"]` | Additional Perl include paths passed to the server. |
| `perl-lsp.enableTestIntegration` | `true` | Enable test integration for `.t` and runnable `.pl` files. |
| `perl-lsp.autoPopulateNewFiles` | `true` | Auto-populate new `.pm` files with a `package` declaration and new `.t` files with `Test::More` boilerplate. Set to `false` to disable. |
| `perl-lsp.featureProfile` | `"auto"` | Forward a concrete feature profile to `perl-lsp` when needed. |
| `perl-lsp.disabledFeatures` | `[]` | Array of LSP feature IDs to disable (e.g. `["lsp.hover", "lsp.semantic_tokens"]`). See the feature catalog via `perllsp --features-json`. |
| `perl-lsp.aiCompletion.enabled` | `false` | Reserved machine-scoped client preference; remote server activation remains unavailable until a trusted user/operator adapter lands. |
| `perl-lsp.critic.enabled` | `true` | Enable Perl::Critic diagnostics. |
| `perl-lsp.critic.engine` | `"native"` | Critic engine: `native` (Rust-native) or `legacy` (shells out to `perlcritic`). |
| `perl-lsp.critic.severity` | `3` | Minimum critic severity to report (1 = least severe, 5 = most severe). |

For the complete settings inventory (including deprecated `perl-lsp.perlcritic.*` settings, AI completion streaming, MCP servers, Linux libc selection, and update intervals), see [CONFIG.md](reference/CONFIG.md) and the VS Code Settings UI.

## What Ships Today

### Language features
The extension is a thin client for `perl-lsp`, so the exact feature surface depends on the installed server version. The intended day-one experience includes diagnostics, completion, hover, definition, references, symbols, semantic tokens, formatting, code actions, and debugger launch support.

### Extension UX
- Output channel for server logs
- Status bar menu for common actions
- Restart command
- Installed-server version command
- Test runner integration for `.t` and `.pl` files
- Runtime download with checksum verification
- Debugger registration for `perl-dap`

Refactoring is exposed through server-backed code actions when available. The extension also provides standalone commands for `perl-lsp.extractVariable` and `perl-lsp.extractMethod` (see the Commands table below and your editor's keybindings).

## Commands

| Command | Description |
|---------|-------------|
| `Perl: Restart Perl Language Server` | Stop and restart the active `perl-lsp` client. |
| `Perl: Show Server Version` | Show the installed `perl-lsp` version. |
| `Perl: Show Output Channel` | Open the extension output channel. |
| `Perl: Show Status Menu` | Open the quick status/action menu. |
| `Perl: Run Tests in Current File` | Run tests for the active `.t` or `.pl` file. |
| `Perl: Extract Variable` | Extract the selected expression into a new variable (`Shift+Alt+V`). |
| `Perl: Extract Method` | Extract the selected code into a new subroutine (`Shift+Alt+M`). |

## Troubleshooting

### Server not found
If activation fails because the server cannot be located:
1. Set `perl-lsp.serverPath` to an installed binary.
2. Ensure `perl-lsp.autoDownload` is enabled.
3. Confirm the binary is on your `PATH`.
4. Open `Perl: Show Output Channel` for detailed logs.

### Offline environments
For offline or air-gapped installs:
1. Download the matching `perl-lsp` release asset manually.
2. Install the extension `.vsix`.
3. Set `perl-lsp.serverPath` to the extracted binary.
4. Optionally set `perl-lsp.autoDownload` to `false`.

### Logging and diagnostics
Set `perl-lsp.trace.server` to `verbose` and inspect the output channel if requests or startup fail.

## Development

```bash
cd vscode-extension
npm install
npm run compile
npm run package
```

For current-platform development only, you can also build and bundle a local binary:

```bash
npm run bundle-lsp
```

That local bundling step is intended for development and smoke testing, not as the portable Marketplace packaging strategy.

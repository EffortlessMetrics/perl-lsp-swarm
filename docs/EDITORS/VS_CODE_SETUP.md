# VS Code Setup Guide for perl-lsp

This guide helps you set up and configure the Perl Language Server in Visual Studio Code.

## Table of Contents

- [Prerequisites](#prerequisites)
- [Installation](#installation)
- [Extension Setup](#extension-setup)
- [Configuration](#configuration)
- [Features](#features)
- [Keybindings](#keybindings)
- [Troubleshooting](#troubleshooting)
- [Advanced Configuration](#advanced-configuration)

---

## Prerequisites

### Required

- **VS Code** version 1.125 or later
- **EffortlessMetrics.perl-lsp-rs** extension installed (see [Installation](#installation))

The extension auto-downloads the matching `perllsp` server by default. Manual
server installation is only required for offline/pinned deployments or when
`perl-lsp.autoDownload` is disabled.

### Optional but Recommended

- **Perl** runtime if you want to run Perl programs; the language server itself does not
  require one. See the [DAP User Guide](../tutorials/DAP_USER_GUIDE.md) for debugger
  requirements.
- **perltidy** only if you select explicit external formatting compatibility

---

## Installation

### Install the VS Code Extension

```bash
code --install-extension EffortlessMetrics.perl-lsp-rs
```

Or search for `perl-lsp` in the Extensions view and install
`EffortlessMetrics.perl-lsp-rs`.

### Optional: Install `perllsp` Manually

Use manual installation for offline/pinned environments, or when
`"perl-lsp.autoDownload": false`.

#### Option 1: Install from crates.io

```bash
cargo install perllsp --locked
```
> The crates.io package `perl-lsp` is a different project, not this language server.

#### Option 2: Download Pre-built Binary

Download from [GitHub Releases](https://github.com/EffortlessMetrics/perl-lsp/releases):

```bash
# Linux x86_64 (glibc). Check the latest release first, then set VERSION
# to the tag without the leading v.
VERSION="<latest-version-without-leading-v>"
TARGET=x86_64-unknown-linux-gnu

curl -LO "https://github.com/EffortlessMetrics/perl-lsp/releases/download/v${VERSION}/perllsp-${VERSION}-${TARGET}.tar.gz"
tar xzf "perllsp-${VERSION}-${TARGET}.tar.gz"
sudo install -m 0755 "perllsp-${VERSION}-${TARGET}/perllsp" /usr/local/bin/perllsp

# For other targets, download the matching perllsp-<version>-<target> archive.
```

#### Option 3: Build from Source

```bash
git clone https://github.com/EffortlessMetrics/perl-lsp.git
cd perl-lsp
cargo install --path crates/perllsp --locked
```

### Verify Installation

```bash
# Check version
perllsp --version

# Quick health check
perllsp --health

# Build/runtime summary
perllsp --info
```

---

## Extension Setup

### Option 1: Official Extension (Recommended)

The official perl-lsp extension provides the best experience with automatic configuration.

```bash
# Install from command line
code --install-extension EffortlessMetrics.perl-lsp-rs

# Or search in VS Code Extensions marketplace:
# 1. Press Ctrl+Shift+X (Cmd+Shift+X on macOS)
# 2. Search for "perl-lsp"
# 3. Click "Install"
```

---

## Configuration

The extension exposes settings in the `perl-lsp.*` namespace. The verified
GitHub `v0.17.0` assets are public beta; Marketplace and package-manager
channels remain independently versioned. Pin deployments when reproducibility
matters.

### Basic Configuration

Add to your workspace `.vscode/settings.json`:

```json
{
  "perl-lsp.serverPath": "",
  "perl-lsp.autoDownload": true,
  "perl-lsp.trace.server": "off",
  "perl-lsp.enableSemanticTokens": true,
  "perl-lsp.enableFormatting": true,
  "perl-lsp.formatOnSave": false,
  "perl-lsp.includePaths": ["lib", "local/lib/perl5"],
  "perl-lsp.enableTestIntegration": true
}
```

### Workspace-Specific Configuration

For project-specific settings, create `.vscode/settings.json` in your project root:

```json
{
  "perl-lsp.includePaths": [
    "lib",
    "local/lib/perl5",
    "vendor/lib"
  ],
  "perl-lsp.formatOnSave": true,
  "[perl]": {
    "editor.defaultFormatter": "EffortlessMetrics.perl-lsp-rs",
    "editor.formatOnSave": true
  }
}
```

### User-Level Configuration

For global settings, open VS Code settings (`Ctrl+,` or `Cmd+,`):

1. Search for "perl-lsp"
2. Configure settings as needed

Or edit `settings.json` directly:

1. Press `Ctrl+Shift+P` (Cmd+Shift+P on macOS)
2. Type "Preferences: Open Settings (JSON)"
3. Add your configuration

### Common Extension Settings

For the authoritative settings list, use VS Code Settings UI or the extension
manifest (`vscode-extension/package.json`). This table focuses on commonly used
settings.

| Setting | Type | Default | Description |
|---------|------|---------|-------------|
| `perl-lsp.serverPath` | string | `""` | Absolute path to the `perllsp` binary. Leave empty to auto-download. |
| `perl-lsp.autoDownload` | boolean | `true` | Auto-download `perllsp` binary if not found locally. |
| `perl-lsp.includePaths` | array | `["lib", "local/lib/perl5"]` | Additional library paths to search for Perl modules. |
| `perl-lsp.enableSemanticTokens` | boolean | `true` | Enable semantic syntax highlighting. |
| `perl-lsp.perltidyConfig` | string | `""` | Path to `.perltidyrc` compatibility file. Native formatting is the default path. |
| `perl-lsp.enableFormatting` | boolean | `true` | Enable native document formatting. |
| `perl-lsp.formatOnSave` | boolean | `false` | Format document on save. |
| `perl-lsp.enableTestIntegration` | boolean | `true` | Enable `Test::More` and `Test2` integration. |
| `perl-lsp.critic.enabled` | boolean | `true` | Enable native critic diagnostics; this built-in engine needs no external tool. |
| `perl-lsp.perlcritic.enabled` | boolean | `false` (deprecated) | Legacy external `perlcritic` compatibility alias. Use `perl-lsp.critic.enabled`; this alias only takes effect when `[critic].engine = "legacy"` is set in `.perl-lsp.toml`. |
| `perl-lsp.perlcritic.severity` | number | `3` | Critic minimum severity, from `1` to `5`. |
| `perl-lsp.perlcritic.profile` | string | `""` | Path to `.perlcriticrc` compatibility profile file. |
| `perl-lsp.featureProfile` | string | `"auto"` | Runtime capability profile. Keep `auto` unless you need a specific compatibility profile. |
| `perl-lsp.disabledFeatures` | array | `[]` | Disable selected server features at client startup. |
| `perl-lsp.autoUpdate` | boolean | `false` | Automatically download and install a new `perllsp` binary when available. |
| `perl-lsp.updateCheckInterval` | number | `24` | Hours between automatic update checks. |
| `perl-lsp.trace.server` | string | `"off"` | LSP traffic logging: `off`, `messages`, `verbose`. |
| `perl-lsp.channel` | string | `"latest"` | With GitHub-backed downloads (`downloadBaseUrl` empty), `latest` includes prereleases and `stable` filters them out. Both sort by strict semantic version and require proven compatibility and target availability; unproven metadata or no compatible release returns an error, with no first-entry fallback. `tag` selects the exact public tag only when compatible. A configured mirror bypasses this selector and uses `versionTag` or `latest`. |
| `perl-lsp.versionTag` | string | `""` | Exact public release tag when `channel` is `tag`; with a mirror configured, selects the mirror artifact version regardless of channel. |
| `perl-lsp.downloadBaseUrl` | string | `""` | Internal base URL for hosting `perllsp` archives and SHA256SUMS. |

---

## Features

### Syntax Diagnostics

Real-time syntax error detection and reporting:

```perl
# Errors are highlighted as you type
my $x = 1
# Missing semicolon - error shown immediately
```

### Go to Definition

Navigate to symbol definitions:

- **Keyboard**: `F12` or `Ctrl+Click` (Cmd+Click on macOS)
- **Context Menu**: Right-click → "Go to Definition"

```perl
use MyModule;

MyModule::some_function();
# ^ F12 here jumps to the definition
```

### Find References

Find all usages of a symbol:

- **Keyboard**: `Shift+F12`
- **Context Menu**: Right-click → "Find All References"

```perl
sub my_function {
    return 42;
}

# ^ Find references here shows all calls to my_function
```

### Hover Information

View documentation and type information:

- **Keyboard**: `Ctrl+K Ctrl+I` or hover with mouse
- **Shows**: Function signatures, variable types, documentation

### Code Completion

Intelligent code completion:

- **Keyboard**: `Ctrl+Space`
- **Triggers**: Automatically as you type

```perl
use MyModule;

MyModule::  # Press Ctrl+Space for completion
```

### Semantic Highlighting

Enhanced syntax highlighting based on semantic understanding:

- Variables, functions, types are color-coded
- Comments and strings are properly highlighted
- Special Perl constructs are highlighted

### Code Actions

Quick fixes and refactorings:

- **Keyboard**: `Ctrl+.` (Cmd+. on macOS)
- **Context Menu**: Right-click → "Quick Fix"

Available actions:
- Extract variable
- Extract subroutine

(Organize imports is withdrawn — see issue #8305.)

### Document Symbols

Navigate symbols in the current file:

- **Keyboard**: `Ctrl+Shift+O` (Cmd+Shift+O on macOS)
- **View**: Outline panel

### Workspace Symbols

Search symbols across the entire workspace:

- **Keyboard**: `Ctrl+T` (Cmd+T on macOS)
- **Search**: Type symbol name to find it

### Rename Symbol

Rename symbols across the workspace:

- **Keyboard**: `F2`
- **Context Menu**: Right-click → "Rename Symbol"

### Formatting

Format Perl code using the native formatter:

- **Keyboard**: `Shift+Alt+F` (Shift+Option+F on macOS)
- **Command**: Format Document
- **On Save**: Enable with `perl-lsp.formatOnSave`

### Test Integration

Run tests directly from VS Code:

- **Keyboard**: `Shift+Alt+T`
- **Command Palette**: "Perl: Run Tests in Current File"
- **Editor toolbar**: Click the beaker icon on `.t` or `.pl` files

### Code Lens

Reference counts and quick actions inline in the editor.

---

## Keybindings

### Default LSP Keybindings

| Action | Windows | Linux | macOS |
|--------|---------|-------|-------|
| Go to Definition | `F12` | `F12` | `F12` |
| Peek Definition | `Alt+F12` | `Ctrl+Shift+F10` | `Option+F12` |
| Find References | `Shift+F12` | `Shift+F12` | `Shift+F12` |
| Rename Symbol | `F2` | `F2` | `F2` |
| Format Document | `Shift+Alt+F` | `Ctrl+Shift+I` | `Shift+Option+F` |
| Quick Fix | `Ctrl+.` | `Ctrl+.` | `Cmd+.` |
| Show Hover | `Ctrl+K Ctrl+I` | `Ctrl+K Ctrl+I` | `Cmd+K Cmd+I` |
| Open Symbol by Name | `Ctrl+T` | `Ctrl+T` | `Cmd+T` |
| Show All Symbols | `Ctrl+Shift+O` | `Ctrl+Shift+O` | `Cmd+Shift+O` |

### Extension-Specific Keybindings

| Action | Windows/Linux | macOS |
|--------|---------------|-------|
| Run Tests | `Shift+Alt+T` | `Shift+Option+T` |
| Restart Server | `Shift+Alt+R` | `Shift+Option+R` |
| Extract Variable | `Shift+Alt+V` | `Shift+Option+V` |
| Extract Method | `Shift+Alt+M` | `Shift+Option+M` |

### Custom Keybindings

To customize keybindings, edit `keybindings.json`:

1. Press `Ctrl+Shift+P` (Cmd+Shift+P on macOS)
2. Type "Preferences: Open Keyboard Shortcuts (JSON)"
3. Add custom bindings

Example:

```json
[
  {
    "key": "ctrl+shift+r",
    "command": "editor.action.rename",
    "when": "editorHasRenameProvider && editorTextFocus"
  },
  {
    "key": "ctrl+shift+f",
    "command": "editor.action.formatDocument",
    "when": "editorHasDocumentFormattingProvider && editorTextFocus && !editorReadonly"
  }
]
```

---

## Troubleshooting

### Server Not Starting

**Symptoms**: No diagnostics, no completion, error in output panel

**Solutions**:

1. **Verify binary is in PATH**:
   ```bash
   command -v perllsp
   perllsp --version
   perllsp --health
   perllsp --info
   ```

2. **Check extension logs**:
   - Open Output panel: `Ctrl+Shift+U` (Cmd+Shift+U on macOS)
   - Select "Perl Language Server" from dropdown
   - Look for error messages

3. **Enable debug logging**:
   ```json
   {
     "perl-lsp.trace.server": "verbose"
   }
   ```

4. **Run health check**:
   - Press `Ctrl+Shift+P` → "Perl: Run Health Check"

5. **If `perllsp --stdio` appears to hang, that is expected**:
   - It is waiting for framed LSP JSON-RPC input (`Content-Length` headers).

### No Diagnostics

**Symptoms**: No errors shown for invalid code

**Solutions**:

1. **Check file type**:
   - Ensure file has `.pl`, `.pm`, or `.t` extension
   - Check language mode: Click language indicator in status bar → select "Perl"

2. **Check the extension health check and output**:
   - Press `Ctrl+Shift+P` → "Perl: Run Health Check"
   - Open Output panel → "Perl Language Server"
   - Look for configuration errors

### Slow Performance

**Symptoms**: Lag when typing, slow completions

**Solutions**:

1. **Prefer project config for server-side limits**:
   - Configure workspace caps in `.perl-lsp.toml` (see [Configuration Reference](../reference/CONFIG.md)).
   - The VS Code extension settings use `perl-lsp.*`; not all clients forward raw `perl.*` workspace settings consistently.

2. **Disable semantic tokens** (if not needed):
   ```json
   {
     "perl-lsp.enableSemanticTokens": false
   }
   ```

### Module Resolution Issues

**Symptoms**: Can't find modules, go-to-definition fails

**Solutions**:

1. **Check include paths**:
   ```json
   {
     "perl-lsp.includePaths": ["lib", ".", "local/lib/perl5", "vendor/lib"]
   }
   ```

2. **Verify module exists**:
   ```bash
   perl -e 'use Module::Name;'
   ```

3. **Check workspace root**:
   - Ensure VS Code opened the correct project folder
   - Right-click folder → "Open Folder"

### Formatting Not Working

**Symptoms**: Format command does nothing or errors

**Solutions**:

1. **Verify formatting enabled**:
   ```json
   {
     "perl-lsp.enableFormatting": true,
     "perl-lsp.formatOnSave": true
   }
   ```

2. **Check server logs**:
   Open the "Perl Language Server" output channel and look for native formatting
   diagnostics. Unsupported literal-preserve regions return no edits rather
   than unsafe rewrites.

3. **Set a compatibility profile path** (optional):
   ```json
   {
     "perl-lsp.perltidyConfig": "/path/to/.perltidyrc"
   }
   ```

4. **Install perltidy only for explicit external compatibility mode**:
   ```bash
   perltidy --version
   ```

### Extension Conflicts

**Symptoms**: Duplicate diagnostics, conflicting keybindings

**Solutions**:

1. **Disable other Perl extensions**:
   - Open Extensions panel: `Ctrl+Shift+X` (Cmd+Shift+X on macOS)
   - Search for "perl"
   - Disable extensions that might conflict (e.g., other LSP servers)

2. **Check for duplicate language servers**:
   - Open Output panel → "Perl Language Server"
   - Look for messages about multiple servers

---

## Advanced Configuration

### Multi-Root Workspace

For workspaces with multiple folders:

```json
{
  "perl-lsp.includePaths": [
    "${workspaceFolder}/lib",
    "${workspaceFolder}/local/lib/perl5"
  ]
}
```

### Feature Profile

Control which LSP features are active:

```json
{
  "perl-lsp.featureProfile": "auto"
}
```

Available profile values are compatibility tokens, not release-support claims:
`auto`, `ga-lock`, `ga`, `prod`, `production`, and `all`. Keep `auto` unless
you are testing a narrower or broader capability set.

### Release Channel

Pin to a specific release or use a different download channel:

```json
{
  "perl-lsp.channel": "tag",
  "perl-lsp.versionTag": "v0.17.0"
}
```

With GitHub-backed downloads (`perl-lsp.downloadBaseUrl` empty), `latest` and
`stable` use the same public release list, sorted by strict semantic version.
`latest` includes prereleases; `stable` filters them out. A release is selected
only when compatibility and target availability are proven. If a newer release
has unresolved compatibility or target evidence, or no compatible release is
available, selection returns an error instead of falling back to the first list
entry. Use `tag` to pin one exact public release tag; the selector still
requires that tag to be proven compatible. Configuring `downloadBaseUrl` bypasses
this selector; see [Internal Deployment](#internal-deployment) for mirror
behavior.

### Internal Deployment

For teams hosting their own `perllsp` binaries:

```json
{
  "perl-lsp.serverPath": "/opt/perl-lsp/bin/perllsp",
  "perl-lsp.autoDownload": false
}
```

Or with an internal download mirror:

Setting `perl-lsp.downloadBaseUrl` bypasses the GitHub release selector. The
extension requests artifacts for `perl-lsp.versionTag`, or uses the mirror's
`latest` artifact name when no tag is configured. The mirror must provide the
correct target archives and a usable `SHA256SUMS`; GitHub compatibility evidence
and `stable` prerelease filtering do not apply to mirror downloads.

```json
{
  "perl-lsp.downloadBaseUrl": "https://internal.example.com/perllsp/"
}
```

### Debug Adapter Protocol (DAP)

Enable debugging support by creating a launch configuration. Run the command:

- Press `Ctrl+Shift+P` → "Perl: Create Debug Configuration"

Or add manually to `.vscode/launch.json`:

```json
{
  "version": "0.2.0",
  "configurations": [
    {
      "type": "perl",
      "request": "launch",
      "name": "Perl: Launch Script",
      "program": "${workspaceFolder}/script.pl",
      "stopOnEntry": true
    }
  ]
}
```

See [DAP User Guide](../tutorials/DAP_USER_GUIDE.md) for more details.

### Server-Side Performance Limits

For server-wide caps (workspace symbols, references, completion limits, and
scan deadlines), prefer `.perl-lsp.toml` and the mechanisms documented in
[Configuration Reference](../reference/CONFIG.md).

### Logging and Tracing

Enable detailed logging for troubleshooting:

```json
{
  "perl-lsp.trace.server": "verbose"
}
```

Logs appear in the VS Code Output panel under "Perl Language Server".

---

## Complete Example Configuration

Here is a typical `.vscode/settings.json` for a Perl project using only real extension settings:

This example adds `.` and `vendor/lib` to the defaults for projects that use
those paths; omit either path when the project does not need it.

```json
{
  "perl-lsp.serverPath": "",
  "perl-lsp.autoDownload": true,
  "perl-lsp.trace.server": "off",
  "perl-lsp.enableSemanticTokens": true,
  "perl-lsp.enableFormatting": true,
  "perl-lsp.formatOnSave": true,
  "perl-lsp.enableTestIntegration": true,
  "perl-lsp.includePaths": [
    "lib",
    ".",
    "local/lib/perl5",
    "vendor/lib"
  ],
  "perl-lsp.perltidyConfig": "",
  "perl-lsp.featureProfile": "auto",

  "[perl]": {
    "editor.defaultFormatter": "EffortlessMetrics.perl-lsp-rs",
    "editor.formatOnSave": true,
    "editor.tabSize": 4,
    "editor.insertSpaces": true
  },

  "files.exclude": {
    "**/.git": true,
    "**/.DS_Store": true,
    "**/node_modules": true
  },

  "search.exclude": {
    "**/node_modules": true,
    "**/local": true
  }
}
```

---

## See Also

- [Getting Started](../tutorials/GETTING_STARTED.md) - Quick start guide
- [Configuration Reference](../reference/CONFIG.md) - Complete configuration options
- [Troubleshooting Guide](../how-to/TROUBLESHOOTING.md) - Common issues and solutions
- [DAP User Guide](../tutorials/DAP_USER_GUIDE.md) - Debugging setup
- [Editor Setup](../how-to/EDITOR_SETUP.md) - Other editor configurations

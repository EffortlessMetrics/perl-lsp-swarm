# Editor Setup Guide

Use this page after choosing an install path.

For VS Code-compatible editors, the extension can download `perllsp`
automatically. Generic LSP clients need an exact `perllsp` binary available to
the client, either because you installed it yourself or because the integration
has a separately proven managed-install path. If you still need the binary,
start with [INSTALLATION.md](INSTALLATION.md).

The verified GitHub `v0.17.0` assets are public beta. Marketplace and package-
manager versions remain pending or not proven by that receipt; verify `perllsp --version` and
`perllsp --health` before changing editor settings.

A configuration example proves only that a route is intended. It does not prove
that the actual editor, extension, binary, platform, and journey have been
exercised together. Current support tiers are tracked separately from this setup
page; editor rows below state their narrower boundary where needed.

If the server starts but the editor does not behave correctly, see
[TROUBLESHOOTING.md](TROUBLESHOOTING.md).

## What Every Editor Needs

- an exact `perllsp` binary the integration can resolve or install through a proven route
- a workspace root that contains your Perl files
- a command that starts the server with stdio, usually `perllsp --stdio`

For an external/PATH-selected installation, verify it before debugging editor settings:

```bash
perllsp --version
perllsp --health
```

## Pick Your Editor

| Editor | Fast path | Detailed guide |
| --- | --- | --- |
| VS Code | install the extension or point it at `perllsp --stdio` | [docs/EDITORS/VS_CODE_SETUP.md](../EDITORS/VS_CODE_SETUP.md) |
| Cursor | install the VS Code-compatible extension and configure it with the `perl-lsp.*` settings namespace | [docs/EDITORS/CURSOR_SETUP.md](../EDITORS/CURSOR_SETUP.md) |
| Trae (ByteDance) | install the VS Code-compatible extension or set command to `perllsp --stdio` | [docs/EDITORS/TRAE_SETUP.md](../EDITORS/TRAE_SETUP.md) |
| IntelliJ IDEA / LSP4IJ | use LSP4IJ 0.20.0+ and keep the exact template/binary stage explicit; released built-in, imported corrected, and managed-install states are independent | [docs/EDITORS/INTELLIJ_IDEA_SETUP.md](../EDITORS/INTELLIJ_IDEA_SETUP.md) |
| Neovim | define a custom `perllsp` config with `vim.lsp.config()` and enable via `vim.lsp.enable()` (legacy `nvim-lspconfig` supported for older Neovim) | [docs/EDITORS/NEOVIM_SETUP.md](../EDITORS/NEOVIM_SETUP.md) |
| Vim | use `vim-lsp` with `perllsp --stdio` | [docs/EDITORS/VIM_SETUP.md](../EDITORS/VIM_SETUP.md) |
| coc.nvim | configure `languageserver.perl-lsp` in `coc-settings.json` to launch `perllsp --stdio`; works in Neovim and Vim when the buffer filetype is `perl` | [docs/EDITORS/COC_NEOVIM_SETUP.md](../EDITORS/COC_NEOVIM_SETUP.md) |
| Emacs | use `lsp-mode` or `eglot` with `perllsp --stdio` | [docs/EDITORS/EMACS_SETUP.md](../EDITORS/EMACS_SETUP.md) |
| Helix | add a `perllsp` language server entry | [docs/EDITORS/HELIX_SETUP.md](../EDITORS/HELIX_SETUP.md) |
| Zed | **Planned / not proven:** the public Perl extension does not register `perllsp`; do not reuse its independent `perl-lsp` ID | [docs/EDITORS/ZED_SETUP.md](../EDITORS/ZED_SETUP.md) |
| Sublime Text | register `perllsp` in LSP package settings | [docs/EDITORS/SUBLIME_SETUP.md](../EDITORS/SUBLIME_SETUP.md) |
| Amazon Kiro | register a Perl LSP client using `perllsp --stdio` | [docs/EDITORS/KIRO_SETUP.md](../EDITORS/KIRO_SETUP.md) |
| Claude Code | provide a plugin `.lsp.json` pointing to `perllsp --stdio` | [docs/EDITORS/CLAUDE_CODE_SETUP.md](../EDITORS/CLAUDE_CODE_SETUP.md) |
| Codex CLI | configure an MCP bridge such as `lsp-mcp`; the bridge exposes tools to Codex and launches `perllsp --stdio` internally | [docs/EDITORS/CODEX_CLI_SETUP.md](../EDITORS/CODEX_CLI_SETUP.md) |
| Codex Desktop | add a custom Perl server command `perllsp --stdio` | [docs/EDITORS/CODEX_DESKTOP_SETUP.md](../EDITORS/CODEX_DESKTOP_SETUP.md) |
| OpenCode | configure a custom `perl-lsp` server in `opencode.json` | [docs/EDITORS/OPENCODE_SETUP.md](../EDITORS/OPENCODE_SETUP.md) |

## Minimal Configurations

### VS Code

The repo-maintained extension is the easiest route. If you prefer a manual
configuration, set the command to `perllsp --stdio` and keep the workspace
root pointed at the project root.

### Cursor

Cursor is based on the VS Code codebase. Install the `EffortlessMetrics.perl-lsp-rs`
extension from Cursor's Extensions panel (or via VSIX). Workspace settings use
the same `.vscode/settings.json` format as VS Code.

### Trae (ByteDance)

Trae is VS Code-compatible, so the same extension and settings model applies.
Install the `EffortlessMetrics.perl-lsp-rs` extension from Trae's Extensions
panel, or configure a generic language server command as `perllsp --stdio`.

### Neovim

For Neovim 0.11+:

```lua
vim.lsp.config('perllsp', {
  cmd = { 'perllsp', '--stdio' },
  filetypes = { 'perl' },
  root_markers = {
    '.perl-lsp.toml',
    'Makefile.PL',
    'Build.PL',
    'cpanfile',
    'dist.ini',
    '.git',
  },
  init_options = {
    perl = {
      workspace = {
        includePaths = { 'lib', '.', 'local/lib/perl5' },
      },
    },
  },
})

vim.lsp.enable('perllsp')
```

For latency-focused editing, use the lean profile from
[docs/EDITORS/NEOVIM_SETUP.md](../EDITORS/NEOVIM_SETUP.md). It starts
`perllsp` with `--runtime-mode e2e`, syntax-only diagnostics, zero diagnostic
debounce, disabled eager workspace indexing, and disabled file watchers. That
profile favors responsiveness over full semantic/module/critic/dead-code
diagnostics and does not provide incremental AST reuse.

### Emacs

Use `lsp-mode` or `eglot` with the same `perllsp --stdio` command. The
editor-specific guide has the full snippets for both.

### Vim

Use `vim-lsp` configured to launch `perllsp --stdio`. See
[docs/EDITORS/VIM_SETUP.md](../EDITORS/VIM_SETUP.md) for complete examples.

### coc.nvim

Open coc.nvim settings:

```vim
:CocConfig
```

Add:

```json
{
  "languageserver": {
    "perl-lsp": {
      "command": "perllsp",
      "args": ["--stdio"],
      "filetypes": ["perl"],
      "rootPatterns": [".perl-lsp.toml", "Makefile.PL", "Build.PL", "cpanfile", "dist.ini", ".git"],
      "settings": {
        "perl": {
          "workspace": {
            "includePaths": ["lib", ".", "local/lib/perl5"]
          }
        }
      }
    }
  }
}
```

Check the active filetype with:

```vim
:CocCommand document.echoFiletype
```

It must be `perl`.

### Helix

```toml
[[language]]
name = "perl"
language-servers = ["perllsp"]

[language-server.perllsp]
command = "perllsp"
args = ["--stdio"]
```

### Zed

**Planned / not proven.** Zed requires its Perl extension to register each
available language-server ID. The public extension currently registers
`perlnavigator-server` for Perl Navigator and `perl-lsp` for
`tree-sitter-perl/perl-tree-sitter-lsp`; it does not register EffortlessMetrics
`perllsp`.

Do not override the existing `perl-lsp` ID to launch `perllsp`. The repository
has prepared a separate `perllsp` registration and checked submission packet,
but it remains a development artifact until it is submitted, accepted, released,
and exercised in the actual host.

See [docs/EDITORS/ZED_SETUP.md](../EDITORS/ZED_SETUP.md) for the product-identity
boundary and [docs/integrations/ZED_UPSTREAM_SUBMISSION.md](../integrations/ZED_UPSTREAM_SUBMISSION.md)
for the exact upstream candidate.

### Sublime Text

Install the `LSP` package, then open `Preferences: LSP Server Configurations` and add:

```json
{
  "perl-lsp": {
    "enabled": true,
    "command": ["perllsp", "--stdio"],
    "selector": "source.perl"
  }
}
```

For project-specific server settings, use `.perl-lsp.toml` or add Sublime `initialization_options` under the `perl-lsp` server configuration.

### Amazon Kiro

For Kiro IDE, install the OpenVSX extension `EffortlessMetrics.perl-lsp-rs`.
The extension can auto-download `perllsp`. For Kiro CLI, run `/code init` in
the project root and edit the generated LSP configuration so Perl launches with
`perllsp --stdio`. Verify diagnostics, hover, definition, references, and rename
in your installed Kiro CLI build because Perl uses the custom-LSP path there.

### Claude Code

Create a plugin with `.lsp.json` that maps Perl extensions to a server entry
using `command: "perllsp"` and `args: ["--stdio"]`.

### Codex CLI

Codex CLI uses MCP tools rather than direct LSP server registration. Configure
an LSP-to-MCP bridge such as `lsp-mcp`, then point Codex at the bridge with a
project-local `.codex/config.toml` entry like:

```toml
[mcp_servers.perl_lsp]
command = "lsp-mcp"
args = ["--config", "/absolute/path/to/project/lsp-mcp.toml", "--workspace", "/absolute/path/to/project"]
cwd = "/absolute/path/to/project"
```

Do not register `perllsp --stdio` directly as an MCP server; it speaks LSP, not
MCP. See [docs/EDITORS/CODEX_CLI_SETUP.md](../EDITORS/CODEX_CLI_SETUP.md) for
the full workflow, bridge config, and troubleshooting.

### OpenCode

Create or update `opencode.json` and register a custom LSP server with
`"command": ["perllsp", "--stdio"]` and Perl extensions like `.pl`, `.pm`,
and `.t`. See [docs/EDITORS/OPENCODE_SETUP.md](../EDITORS/OPENCODE_SETUP.md) for a full example.

### IntelliJ IDEA / LSP4IJ

Use LSP4IJ 0.20.0+ and keep the integration subject explicit:

```text
released built-in Perl template
locally imported corrected template
future corrected built-in template
```

The binary subject is independent:

```text
external/PATH-selected perllsp
LSP4IJ-managed public artifact
local exact-source candidate
```

A built-in template that finds an existing PATH binary does not prove the
LSP4IJ-managed installer path. See
[docs/EDITORS/INTELLIJ_IDEA_SETUP.md](../EDITORS/INTELLIJ_IDEA_SETUP.md) for the
full evidence and installation boundaries.

For shared project behavior, prefer `.perl-lsp.toml`. Corrected LSP4IJ client
settings use sparse server-native `perl.*` overrides; VS Code `perl-lsp.*`
settings are not the generic server schema. Reserve `initializationOptions` for
values that actually require initialize/reinitialize timing.

Use the [legacy Raw Command fallback](../EDITORS/INTELLIJ_IDEA_LEGACY_RAW_COMMAND.md)
for local/unreleased candidates, temporary custom launch flags, or a LSP4IJ
build where the relevant template route is unavailable.

Protocol-profile evidence can prove capability negotiation such as standard
`textDocument/inlineCompletion`; user-facing feature support requires the
matching actual IntelliJ/LSP4IJ host cell from #7719/#7122.

Debugger setup is a separate subject. See
[docs/EDITORS/INTELLIJ_DAP_SETUP.md](../EDITORS/INTELLIJ_DAP_SETUP.md); the
presence of an upstream Perl DAP template does not prove `perl-dap` launch,
breakpoint, variable, stepping, attach, or cleanup behavior.

## Diagnostics Mode (Push vs Pull)

The server supports both push diagnostics (`textDocument/publishDiagnostics`)
and pull diagnostics (`textDocument/diagnostic`). At `initialize`, if the
client advertises `textDocument.diagnostic` capability, the server switches to
pull mode and stops pushing. If the client does not advertise the capability,
the server uses push mode exclusively.

**If diagnostics disappear after enabling the server**, the most common cause is
a half-implemented pull client — one that advertises `textDocument.diagnostic`
but does not actually poll `textDocument/diagnostic`. In that case the server
is waiting for pulls that never come, and push has been suppressed. The fix is
either to complete the pull client's implementation or to disable the
`textDocument.diagnostic` capability advertisement so the server falls back to
push.

**Editor-specific notes:**

- **OpenCode:** the server force-enables push diagnostics for OpenCode clients
  regardless of capability advertisement, because OpenCode's agent feedback
  loop relies on push. See
  [OPENCODE_SETUP.md](../EDITORS/OPENCODE_SETUP.md) Troubleshooting.
- **LSP4IJ:** the server has historical JetBrains-family watched-file
  compatibility debt. #7710 owns retiring or exactly bounding that workaround
  from the supported LSP4IJ capability profile; #7719 owns the real-host
  create/change/delete result. Do not treat broad product-name suppression as
  a permanent LSP4IJ limitation.

## When Setup Fails

- For an external/PATH-selected binary, verify the exact `perllsp --version`
  and resolved path. For a client-managed route, verify the actual installed
  artifact instead of assuming PATH ownership.
- If the server starts but the editor stays idle, check the editor's LSP log
  and confirm the workspace root is correct.
- If completions or diagnostics are missing, move to
  [TROUBLESHOOTING.md](TROUBLESHOOTING.md) for the next steps.

### Codex Desktop

Configure a custom Perl language server process that runs `perllsp --stdio`.
See the dedicated guide for the exact fields and verification steps.

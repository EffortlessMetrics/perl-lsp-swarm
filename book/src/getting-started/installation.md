# Getting Started with perl-lsp

This guide gets you from zero to a working Perl language server in your editor.

## Prerequisites

- **Rust 1.95+** (for building from source)
- **A supported editor**: VS Code, Neovim, Emacs, Helix, or Sublime Text

## Installation

Choose one method:

### Option 1: Install from crates.io (Recommended)

```bash
cargo install perllsp
```
> The crates.io package `perl-lsp` is a different project, not this language server.

### Option 2: Install Script (Linux/macOS)

Use the installer script (best-effort / non-canonical):

```bash
curl -fsSL https://raw.githubusercontent.com/EffortlessMetrics/perl-lsp/master/install.sh | bash
```

### Option 3: Build from Source

```bash
git clone https://github.com/EffortlessMetrics/perl-lsp.git
cd perl-lsp
cargo install --path crates/perllsp
```

## Verify Installation

```bash
# Check binary is available
perllsp --version

# Quick health check
perllsp --health
# Should output: ok 0.10.0
```

## Quick Editor Setup

### VS Code

1. Install the extension:
   ```bash
   code --install-extension effortlesssteven.perl-lsp
   ```

2. Open a `.pl` or `.pm` file - the server starts automatically.

### Neovim

Add to your `init.lua`:

```lua
local lspconfig = require('lspconfig')
local configs = require('lspconfig.configs')

if not configs.perl_lsp then
  configs.perl_lsp = {
    default_config = {
      cmd = { 'perllsp', '--stdio' },
      filetypes = { 'perl' },
      root_dir = lspconfig.util.root_pattern('.git'),
      single_file_support = true,
    },
  }
end

lspconfig.perl_lsp.setup({})
```

### Emacs (with eglot, Emacs 29+)

```elisp
(add-to-list 'eglot-server-programs
             '((cperl-mode perl-mode) . ("perllsp" "--stdio")))
```

Then run `M-x eglot` in a Perl buffer.

### Helix

Add to `~/.config/helix/languages.toml`:

Helix's current built-in `perl` language entry also owns Raku/NQP/P6 file
extensions. `perllsp` is a Perl 5 server, so use the reviewed override rather
than replacing only the language-server name on the combined entry:

```toml
[language-server.perllsp]
command = "perllsp"
args = ["--stdio"]

[[language]]
name = "perl"
language-servers = ["perllsp"]
roots = [".perl-lsp.toml", "Makefile.PL", "Build.PL", "cpanfile", "dist.ini"]
file-types = [
  "pl",
  "pm",
  "t",
  "psgi",
  { glob = "latexmkrc" },
  { glob = ".latexmkrc" },
]
shebangs = ["perl"]
```

The checked fixture is
[`docs/examples/helix/languages.toml`](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/docs/examples/helix/languages.toml).
This safe override deliberately stops the same entry from owning Raku-family
file detection; it does not supply or imply Raku LSP support.

## Your First 5 Minutes

Once installed, open any Perl file and try these features:

### 1. Hover for Documentation

Move your cursor over a function like `print` or `substr` and see documentation appear.

### 2. Go to Definition

Click on a variable or function call and use your editor's "Go to Definition" command:
- VS Code: `F12` or `Ctrl+Click`
- Neovim: `gd`
- Emacs: `M-.`

### 3. Find All References

Find everywhere a symbol is used:
- VS Code: `Shift+F12`
- Neovim: `gr`
- Emacs: `M-?`

### 4. Code Completion

Type `$` to see variable completions, or start typing a function name:

```perl
my $name = "Alice";
print $na  # Completes to $name
prin       # Completes to print
```

### 5. Quick Fixes

The LSP suggests fixes for common issues. Look for the lightbulb icon (VS Code) or use:
- VS Code: `Ctrl+.`
- Neovim: `<leader>ca`
- Emacs: `C-c l a`

## What You Get

perl-lsp provides:

| Feature | What It Does |
|---------|--------------|
| **Diagnostics** | Real-time syntax error detection |
| **Completion** | Variables, functions, keywords, file paths |
| **Hover** | Documentation for 150+ Perl built-ins |
| **Definition** | Jump to where symbols are defined |
| **References** | Find all uses of a symbol |
| **Rename** | Safely rename variables across files |
| **Formatting** | Format code with Perl::Tidy |
| **Folding** | Collapse functions, blocks, POD |
| **Symbols** | Document outline and workspace search |

## Project Configuration

For project-specific settings, the server reads configuration from your editor's LSP settings.

### Example: Configure Module Search Paths

```json
{
  "perl": {
    "workspace": {
      "includePaths": ["lib", ".", "local/lib/perl5"]
    }
  }
}
```

### Example: Tune for Large Projects

```json
{
  "perl": {
    "limits": {
      "maxIndexedFiles": 50000,
      "referencesCap": 1000
    }
  }
}
```

See [CONFIG.md](../reference/CONFIG.md) for all configuration options.

## Troubleshooting

### Server Not Starting

```bash
# Test if the binary works
echo '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{}}}' | perllsp --stdio
```

### No Diagnostics Appearing

1. Ensure your file has a Perl extension (`.pl`, `.pm`, `.t`)
2. Check your editor's language mode is set to Perl
3. Look at the LSP output log in your editor

### Slow Performance

Reduce indexed files and result caps in your settings:

```json
{
  "perl": {
    "limits": {
      "maxIndexedFiles": 5000,
      "workspaceSymbolCap": 100
    }
  }
}
```

See [TROUBLESHOOTING.md](../how-to/TROUBLESHOOTING.md) for more solutions.

## Next Steps

- **[EDITOR_SETUP.md](../how-to/EDITOR_SETUP.md)** - Detailed editor configurations
- **[CONFIG.md](../reference/CONFIG.md)** - All configuration options
- **[LSP_FEATURES.md](../reference/LSP_FEATURES.md)** - Complete feature documentation
- **[FAQ.md](../reference/FAQ.md)** - Frequently asked questions

## Getting Help

- **Issues**: [GitHub Issues](https://github.com/EffortlessMetrics/perl-lsp/issues)
- **Documentation**: [docs/INDEX.md](INDEX.md)

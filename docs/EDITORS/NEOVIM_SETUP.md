# Neovim Setup Guide for perl-lsp

Use this guide to run `perllsp` through Neovim's built-in LSP client.

## Current support boundary

The maintained built-in-LSP configuration requires **Neovim 0.11.3 or later**.
Actual client support is version- and platform-scoped: setup syntax, a synthetic
capability profile, and a real Neovim receipt are different evidence. Do not
treat a single host receipt as a broad `Neovim 0.11+` matrix. Exact
version/feature cells belong in the bounded supported-version matrix (#7716)
once that receipt lane lands; until then, cite only the concrete receipt that
covers the claim.

The current user path is:

```text
install perllsp
→ define a local lsp/perllsp.lua config
→ vim.lsp.enable('perllsp')
→ open a Perl project
```

A first-party `perllsp` entry for upstream `nvim-lspconfig` and a Mason package
are being prepared, but **do not treat either as publicly available until the
upstream projects actually contain them**. Until then, use the built-in manual
configuration below.

## Prerequisites

- Neovim 0.11.3 or later
- `perllsp` installed and visible to the Neovim process
- a Perl project with a recognized project marker

Verify the server before changing Neovim configuration:

```bash
perllsp --version
perllsp --health
perllsp --info
```

## Install `perllsp`

Current public install routes are independent evidence stages. Verify the
resolved binary after installation rather than assuming a package-manager name
proves which server Neovim will start.

### Cargo

```bash
cargo install perllsp
perllsp --version
```
> The crates.io package `perl-lsp` is a different project, not this language server.

### Homebrew tap

```bash
brew install effortlessmetrics/tap/perllsp
perllsp --version
```

### Prebuilt release archive

Download the archive matching your platform from the public `perl-lsp` GitHub
release, verify the release/checksum identity, extract it, and place `perllsp`
on `PATH`.

### From source

Use source installation for development rather than as proof of a public
package-manager route:

```bash
git clone https://github.com/EffortlessMetrics/perl-lsp.git
cd perl-lsp
cargo install --path crates/perllsp --locked
```

## Canonical built-in LSP setup

Create a config file:

```vim
:exe 'edit' stdpath('config') .. '/lsp/perllsp.lua'
```

Use:

```lua
return {
  cmd = { 'perllsp', '--stdio' },
  filetypes = { 'perl' },
  root_markers = {
    { '.perl-lsp.toml', 'Makefile.PL', 'Build.PL', 'cpanfile', 'dist.ini' },
    '.git',
  },
}
```

Then enable it from `init.lua`:

```lua
vim.lsp.enable('perllsp')
```

The nested list is intentional. On Neovim 0.11.3+, those Perl project markers
have equal priority, so the **nearest Perl project marker** wins. `.git` is the
lower-priority repository fallback. A flat marker list can let a farther
`.perl-lsp.toml` outrank a nearer nested `Makefile.PL` or `cpanfile`.

Restart Neovim, open a Perl file inside the project, and run:

```vim
:checkhealth vim.lsp
```

For predictable activation, the maintained path currently assumes a recognized
workspace marker. Standalone no-marker behavior is kept as a separate support
cell rather than being inferred from project-backed receipts.

## Project and editor settings

Prefer `.perl-lsp.toml` for configuration that should travel with the project
and behave consistently across editors.

For Neovim-specific LSP settings, use the server-native `perl.*` namespace via
Neovim's `settings` field:

```lua
vim.lsp.config('perllsp', {
  settings = {
    perl = {
      workspace = {
        includePaths = { 'lib', 'local/lib/perl5' },
        useSystemInc = false,
        resolutionTimeout = 50,
      },
      inlayHints = {
        enabled = true,
        parameterHints = true,
      },
    },
  },
})
```

This is distinct from the VS Code extension's `perl-lsp.*` setting names.
`initializationOptions.perl.*` remains a real server input, but it should not be
the default generic-client configuration channel for settings that Neovim can
supply through `workspace/configuration`.

Machine-authority settings such as arbitrary interpreter paths, external
formatter/critic profile paths, and remote AI endpoint or credential routing
are intentionally not ordinary workspace-delivered `perl.*` settings.

## Filetype activation

`vim.lsp.enable('perllsp')` attaches only when Neovim's current buffer
`filetype` is `perl`.

Check the actual result:

```vim
:set filetype?
```

Ordinary `.pl`, `.pm`, and `.psgi` source forms are the base activation
contract. Other Perl-bearing forms such as `.t`, `.PL`, `.cgi`, `.fcgi`,
extensionless/shebang scripts, POD, XS, and template files are tracked as
separate activation/semantic-support cells. Do not force a mixed-language file
to `perl` merely because it contains Perl fragments.

If a file family you deliberately treat as plain Perl is not detected as such
in your supported Neovim version, add a local override explicitly, for example:

```lua
vim.filetype.add({
  extension = {
    t = 'perl',
    cgi = 'perl',
    fcgi = 'perl',
    PL = 'perl',
  },
})
```

That is a user override, not evidence that Neovim natively detects the suffix.

## Completion and optional LSP features

Neovim can support an LSP method without making its UI behavior visible by
default. Keep these states separate:

```text
server supports method
client supports method
Neovim enables it by default
user enables it explicitly
actual enabled journey works
```

### Built-in completion

```lua
vim.api.nvim_create_autocmd('LspAttach', {
  callback = function(ev)
    local client = vim.lsp.get_client_by_id(ev.data.client_id)
    if not client or client.name ~= 'perllsp' then
      return
    end

    if client:supports_method('textDocument/completion') then
      vim.lsp.completion.enable(true, client.id, ev.buf, {
        autotrigger = true,
      })
    end
  end,
})
```

### Inlay hints

```lua
vim.api.nvim_create_autocmd('LspAttach', {
  callback = function(ev)
    local client = vim.lsp.get_client_by_id(ev.data.client_id)
    if client and client.name == 'perllsp'
        and client:supports_method('textDocument/inlayHint') then
      vim.lsp.inlay_hint.enable(true, { bufnr = ev.buf })
    end
  end,
})
```

Code lenses, linked editing, and inline completion follow the same principle:
only enable them when both the installed Neovim and the exact `perllsp` subject
support the method. Their presence in server capability code does not make them
default-visible Neovim features.

## Formatting

Neovim's built-in LSP client can consume `perllsp` document formatting. Manual
formatting is straightforward:

```lua
vim.lsp.buf.format({ bufnr = 0 })
```

If you configure format-on-save yourself, keep one owner. Do not combine a
`BufWritePre` formatting autocmd with another save-time formatting mechanism
that would apply the same edit twice.

Native formatting does not require `perltidy`; external compatibility behavior
is a separate opt-in mode.

## Diagnostics

`perllsp` supports both push and pull diagnostics. The selected transport is a
client-capability interaction, so support is proven against the actual Neovim
version rather than from a server capability bit alone.

To inspect the current client and diagnostics:

```vim
:checkhealth vim.lsp
:lua =vim.lsp.get_clients({ name = 'perllsp' })
:lua =vim.diagnostic.get(0)
```

A client advertising pull diagnostics is not considered proven merely because
initialization succeeds; the real-client receipt must observe the diagnostic
request/result path.

## Semantic tokens

The server implements semantic-token full results and a result-ID/delta path.
That is separate from whether a given Neovim version actually advertises and
uses delta in the tested journey.

Do not use older documentation that says semantic-token delta is unimplemented.
Likewise, semantic-token delta has nothing to do with whether the parser reused
an AST.

## Virtual `perldoc://` documents

`perllsp` implements LSP 3.18 `workspace/textDocumentContent` for virtual
`perldoc://...` documents, including workspace-local POD. Stock Neovim support
for opening and refreshing those virtual documents is a separate client
capability and is currently tracked as an upstream dependency where the tested
Neovim row does not implement it.

A working file-backed definition or a direct server request is **not** proof
that stock Neovim can open `perldoc://strict` as a virtual buffer.

No repository-owned Neovim plugin or `BufReadCmd` shim is required for ordinary
LSP support.

## Optional lean latency profile

Use this profile when parser diagnostics and responsiveness matter more than the
full semantic/module/native-critic/workspace diagnostic stack:

```lua
vim.lsp.config('perllsp', {
  cmd = {
    'perllsp',
    '--stdio',
    '--runtime-mode', 'e2e',
    '--diagnostic-mode', 'syntax-only',
    '--diagnostic-debounce-ms', '0',
    '--eager-workspace-indexing=false',
    '--file-watchers=false',
  },
  filetypes = { 'perl' },
  root_markers = {
    { '.perl-lsp.toml', 'Makefile.PL', 'Build.PL', 'cpanfile', 'dist.ini' },
    '.git',
  },
})

vim.lsp.enable('perllsp')
```

The lean profile changes server work, not LSP semantics. It does not imply
incremental AST reuse. Text synchronization and parser strategy are also
separate contracts: until the exact ranged-edit/desynchronization train is
activated, the shipping server may still advertise full-document text sync.

## Verify the current journey

1. Start Neovim with the intended `perllsp` on `PATH`.
2. Open a project-backed `.pl` or `.pm` file.
3. Confirm `:set filetype?` reports `perl`.
4. Run `:checkhealth vim.lsp` and verify the resolved command/root.
5. Introduce a temporary syntax error and confirm diagnostics change.
6. Repair it and confirm the diagnostic state updates.
7. Exercise completion plus hover/definition.
8. Make a real buffer edit and re-query so a stale first result cannot satisfy the check.
9. Exercise formatting if it is part of your configured workflow.
10. Exit/restart and verify no unintended server process remains.

For public/package-manager support claims, also verify that the Neovim process
started the **installed artifact** rather than a workspace `target/` binary or a
stale global version.

## `nvim-lspconfig` and Mason status

The intended eventual nvim-lspconfig route is:

```lua
vim.lsp.enable('perllsp')
```

with the same command/filetype/root contract shown above. A submission packet is
kept in the repository, but this route must not be advertised as public until
upstream `neovim/nvim-lspconfig` actually contains `perllsp` in a consumable
ref/version.

Likewise, `:MasonInstall perllsp` must not be presented as available until the
package is accepted and observable in the public Mason registry. Local/forked
registry tests are preparation evidence only.

## Troubleshooting

### Neovim cannot find `perllsp`

Shell:

```bash
command -v perllsp
perllsp --version
perllsp --health
perllsp --info
```

Windows PowerShell:

```powershell
where perllsp
perllsp --version
perllsp --health
perllsp --info
```

From Neovim, inspect the actual client config rather than only the shell:

```vim
:checkhealth vim.lsp
:lua =vim.lsp.get_clients({ name = 'perllsp' })
```

GUI-launched Neovim may inherit a different `PATH` from your interactive shell.

### `perllsp --stdio` appears to hang

That is expected: stdio mode waits for framed LSP JSON-RPC input. For manual
checks use:

```bash
perllsp --health
perllsp --info
perllsp --check path/to/file.pl
```

## Historical Neovim 0.8-0.10 setup

The old `require('lspconfig').setup(...)` framework and Neovim 0.8-0.10 recipes
are **historical compatibility guidance**, not the maintained current path.
Modern nvim-lspconfig itself has moved to native `vim.lsp.config()` /
`vim.lsp.enable()` configuration and newer Neovim requirements.

If you must remain on an old Neovim version, pin a contemporaneous
`nvim-lspconfig` release and adapt the same canonical command/filetype/root
contract. Do not copy current nvim-lspconfig instructions into an old client and
assume the combination is supported.

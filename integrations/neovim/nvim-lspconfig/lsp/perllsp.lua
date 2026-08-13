---@brief
---
--- https://github.com/EffortlessMetrics/perl-lsp
---
--- Native Rust language server for Perl 5.
---
--- Requires Neovim 0.11.3 or later. The nested `root_markers` form below uses
--- equal-priority Perl project markers with `.git` as the lower-priority
--- fallback; that nested form is the maintained contract and is not dual-
--- patched for older Neovim releases.
---
--- Install `perllsp`, then enable this configuration with:
---
--- ```lua
--- vim.lsp.enable('perllsp')
--- ```
---
--- Project configuration is read from `.perl-lsp.toml` and the server-native
--- `perl.*` LSP settings namespace. This config intentionally does not embed
--- project-specific include paths or user settings.

---@type vim.lsp.Config
return {
  cmd = { 'perllsp', '--stdio' },
  filetypes = { 'perl' },
  root_markers = {
    { '.perl-lsp.toml', 'Makefile.PL', 'Build.PL', 'cpanfile', 'dist.ini' },
    '.git',
  },
}

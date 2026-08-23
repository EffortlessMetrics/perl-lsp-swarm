-- Canonical Neovim configuration fixture for perllsp interoperability receipts.
--
-- Keep this file intentionally data-only: it is consumed by actual-host tests,
-- docs, and the eventual nvim-lspconfig submission. Project configuration
-- belongs in .perl-lsp.toml or the client's `settings`, not in this fixture.
--
-- Neovim 0.11.3+ treats entries in a nested root_markers list as equal
-- priority. That makes the nearest Perl project marker win while retaining
-- `.git` as the lower-priority repository fallback.

local perl_project_markers = {
  '.perl-lsp.toml',
  'Makefile.PL',
  'Build.PL',
  'cpanfile',
  'dist.ini',
}

return {
  cmd = { 'perllsp', '--stdio' },
  filetypes = { 'perl' },
  root_markers = {
    perl_project_markers,
    '.git',
  },
}

-- Canonical nvim-dap fixture for perl-dap preview receipts.
--
-- `perl-dap` uses stdio when no socket/port flags are supplied, so the
-- executable adapter intentionally has no `--stdio` argument. Actual-host
-- receipts override `command` with an exact candidate/public binary path.

return {
  adapter = {
    type = 'executable',
    command = 'perl-dap',
    -- The editor-owned workspace boundary is startup authority; launch.json
    -- remains unable to widen it.
    args = { '--trusted-root', '${workspaceFolder}' },
    options = {
      source_filetype = 'perl',
      detached = false,
    },
  },
  configurations = {
    {
      type = 'perl',
      request = 'launch',
      name = 'Launch current Perl file',
      program = '${file}',
      cwd = '${workspaceFolder}',
      stopOnEntry = false,
    },
  },
}

-- Mechanical parity check for the staged nvim-lspconfig submission in #7722.
--
-- The source-of-truth client behavior fixture is scripts/ux/neovim/perllsp.lua.
-- The staged upstream file may add documentation comments, but its executable
-- configuration must remain the same minimal command/filetype/root contract.

local repo_root = vim.env.REPO_ROOT
if not repo_root or repo_root == '' then
  error('REPO_ROOT is required')
end

local canonical = dofile(repo_root .. '/scripts/ux/neovim/perllsp.lua')
local staged = dofile(repo_root .. '/integrations/neovim/nvim-lspconfig/lsp/perllsp.lua')

local function fail(label, expected, actual)
  error(('%s drifted\nexpected: %s\nactual:   %s'):format(
    label,
    vim.inspect(expected),
    vim.inspect(actual)
  ))
end

-- Full-table parity: every executable field on either side must match.
-- Comparing only an allowlist would silently pass if the canonical fixture
-- later gains fields such as `settings` or `workspace_required`.
if not vim.deep_equal(canonical, staged) then
  fail('config', canonical, staged)
end

if staged.cmd[1] ~= 'perllsp' or staged.cmd[2] ~= '--stdio' or #staged.cmd ~= 2 then
  error('staged config must launch exactly `perllsp --stdio`')
end
if #staged.filetypes ~= 1 or staged.filetypes[1] ~= 'perl' then
  error('staged config must attach only to the Neovim `perl` filetype')
end

io.stdout:write(vim.json.encode({ result = 'pass', config = staged }) .. '\n')

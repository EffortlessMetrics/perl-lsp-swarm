-- Actual Neovim/perllsp virtual-document probe for #7764.
--
-- This deliberately separates two propositions:
--   1. the exact perllsp process serves workspace/textDocumentContent; and
--   2. stock Neovim can consume a returned perldoc:// URI as a virtual buffer.
--
-- Until Neovim core implements the second proposition, the receipt records
-- `upstream_dependency` instead of converting server-side success into a
-- client-support claim.

local function required_env(name)
  local value = vim.env[name]
  if not value or value == '' then
    error(('missing required environment variable %s'):format(name))
  end
  return value
end

local function normalize(path)
  return vim.fs.normalize(vim.fn.fnamemodify(path, ':p'))
end

local function wait_for_client(bufnr)
  local client
  local ok = vim.wait(8000, function()
    local clients = vim.lsp.get_clients({ bufnr = bufnr, name = 'perllsp' })
    client = clients[1]
    return client ~= nil and client.initialized
  end, 20)
  return ok and client or nil
end

local function request_virtual_content(client, uri, bufnr)
  local response = client:request_sync(
    'workspace/textDocumentContent',
    { uri = uri },
    5000,
    bufnr
  )
  if not response or response.err then
    return nil, response and response.err or 'no response'
  end
  local result = response.result
  if type(result) ~= 'table' or type(result.text) ~= 'string' then
    return nil, 'response did not contain result.text'
  end
  return result.text, nil
end

local repo_root = normalize(required_env('REPO_ROOT'))
local fixture_root = normalize(required_env('FIXTURE_ROOT'))
local perllsp = normalize(required_env('PERLLSP'))

if vim.fn.has('nvim-0.11.3') ~= 1 then
  error('Neovim 0.11.3+ is required')
end

vim.cmd('filetype on')
local config = dofile(repo_root .. '/scripts/ux/neovim/perllsp.lua')
config = vim.deepcopy(config)
config.cmd = { perllsp, '--stdio' }
vim.lsp.config('perllsp', config)
vim.lsp.enable('perllsp')

local main_file = fixture_root .. '/main.pl'
vim.cmd('edit ' .. vim.fn.fnameescape(main_file))
if vim.bo.filetype ~= 'perl' then
  error(('expected fixture to detect as perl, got %q'):format(vim.bo.filetype))
end

local source_bufnr = vim.api.nvim_get_current_buf()
local client = wait_for_client(source_bufnr)
if not client then
  error('perllsp did not attach to virtual-document fixture')
end

local strict_text, strict_error = request_virtual_content(client, 'perldoc://strict', source_bufnr)
if not strict_text or not strict_text:lower():find('strict', 1, true) then
  error(('exact perllsp process did not serve perldoc://strict: %s'):format(vim.inspect(strict_error)))
end

local local_text, local_error = request_virtual_content(client, 'perldoc://Local::Doc', source_bufnr)
if not local_text or not local_text:find('Local::Doc', 1, true) or not local_text:find('workspace POD marker', 1, true) then
  error(('workspace-local POD virtual content was not returned: %s'):format(vim.inspect(local_error)))
end

local client_caps = vim.lsp.protocol.make_client_capabilities()
local advertised_support = client_caps.workspace
  and client_caps.workspace.textDocumentContent ~= nil
  or false

-- Probe stock-Neovim URI opening without registering a compatibility BufReadCmd
-- or other repository-owned shim. On unsupported hosts this may fail or create
-- an ordinary empty file buffer; both are an upstream dependency, not a pass.
local open_ok, open_error = pcall(vim.cmd, 'edit perldoc://strict')
local virtual_bufnr = vim.api.nvim_get_current_buf()
local virtual_name = vim.api.nvim_buf_get_name(virtual_bufnr)
local virtual_lines = vim.api.nvim_buf_get_lines(virtual_bufnr, 0, -1, false)
local virtual_text = table.concat(virtual_lines, '\n')
local native_populated = open_ok
  and virtual_bufnr ~= source_bufnr
  and virtual_text:lower():find('strict', 1, true) ~= nil

local state
if advertised_support then
  if not native_populated then
    error('Neovim advertises textDocumentContent support but perldoc://strict did not populate')
  end
  state = 'pass'
else
  state = 'upstream_dependency'
end

local receipt = {
  nvim = vim.version(),
  perllsp = perllsp,
  root = client.root_dir and normalize(client.root_dir) or '',
  direct_server_request = {
    strict = true,
    workspace_local = true,
  },
  neovim = {
    advertises_text_document_content = advertised_support,
    edit_ok = open_ok,
    edit_error = open_ok and vim.NIL or tostring(open_error),
    virtual_buffer_name = virtual_name,
    native_populated = native_populated,
    refresh = 'not_exercised',
  },
  result = state,
}

for _, active in ipairs(vim.lsp.get_clients()) do
  active:stop(true)
end
vim.wait(2000, function()
  return #vim.lsp.get_clients() == 0
end, 20)

io.stdout:write(vim.json.encode(receipt) .. '\n')

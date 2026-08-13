-- Actual-Neovim first-mile journey for an already installed public perllsp.
--
-- The shell wrapper establishes package/artifact identity and injects the exact
-- binary path. This Lua probe owns only the editor composition result.

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

local function wait_until(timeout_ms, predicate, description)
  if not vim.wait(timeout_ms, predicate, 20) then
    error(('timed out waiting for %s'):format(description))
  end
end

local function wait_for_client(bufnr)
  local client
  wait_until(8000, function()
    local clients = vim.lsp.get_clients({ bufnr = bufnr, name = 'perllsp' })
    client = clients[1]
    return client ~= nil and client.initialized
  end, 'perllsp initialization')
  return client
end

local function request(client, bufnr, method, params, timeout_ms)
  local response = client:request_sync(method, params, timeout_ms or 5000, bufnr)
  if not response then
    error(('%s returned no response'):format(method))
  end
  if response.err then
    error(('%s failed: %s'):format(method, vim.inspect(response.err)))
  end
  return response.result
end

local function completion_position(bufnr)
  for zero_index, line in ipairs(vim.api.nvim_buf_get_lines(bufnr, 0, -1, false)) do
    local start = line:find('$bro', 1, true)
    if start then
      return zero_index - 1, start - 1 + #'$bro'
    end
  end
  error('completion fixture marker $bro disappeared from the current buffer')
end

local function completion_items(result)
  if type(result) == 'table' and type(result.items) == 'table' then
    return result.items
  end
  return type(result) == 'table' and result or {}
end

local repo_root = normalize(required_env('REPO_ROOT'))
local fixture_root = normalize(required_env('FIXTURE_ROOT'))
local perllsp = normalize(required_env('PERLLSP'))
local install_source = required_env('PERLLSP_INSTALL_SOURCE')
local artifact_sha256 = required_env('PERLLSP_ACTUAL_SHA256')
local version_output = required_env('PERLLSP_VERSION_OUTPUT')

if vim.fn.has('nvim-0.11.3') ~= 1 then
  error('Neovim 0.11.3+ is required')
end

vim.cmd('filetype on')
local config_path = repo_root .. '/scripts/ux/neovim/perllsp.lua'
local config = dofile(config_path)
config = vim.deepcopy(config)
config.cmd = { perllsp, '--stdio' }
vim.lsp.config('perllsp', config)
vim.lsp.enable('perllsp')

local source_file = fixture_root .. '/lib/App.pm'
vim.cmd('edit ' .. vim.fn.fnameescape(source_file))
if vim.bo.filetype ~= 'perl' then
  error(('expected fixture filetype=perl, got %q'):format(vim.bo.filetype))
end

local bufnr = vim.api.nvim_get_current_buf()
local client = wait_for_client(bufnr)
local selected_root = client.root_dir and normalize(client.root_dir) or ''
if selected_root ~= normalize(fixture_root) then
  error(('installed perllsp attached at wrong root %q (expected %q)'):format(selected_root, fixture_root))
end

-- Initial fixture contains one deterministic syntax error. Observe it through
-- Neovim's diagnostic store rather than inferring diagnostics from protocol
-- capability advertisement.
wait_until(8000, function()
  return #vim.diagnostic.get(bufnr) > 0
end, 'initial diagnostics')
local initial_diagnostics = #vim.diagnostic.get(bufnr)

-- Repair the malformed declaration through an actual Neovim buffer edit. The
-- default server may also emit native critic diagnostics, so the fail-honest
-- oracle is that the malformed generation's diagnostic set is replaced/reduced,
-- not that every possible product diagnostic becomes empty.
local lines = vim.api.nvim_buf_get_lines(bufnr, 0, -1, false)
for index, line in ipairs(lines) do
  if line:find('my $broken =', 1, true) then
    lines[index] = 'my $broken = 41;'
  end
end
vim.api.nvim_buf_set_lines(bufnr, 0, -1, false, lines)

wait_until(8000, function()
  return #vim.diagnostic.get(bufnr) < initial_diagnostics
end, 'malformed-generation diagnostics to be replaced after edit')
local diagnostics_after_fix = #vim.diagnostic.get(bufnr)

local completion_line, completion_col = completion_position(bufnr)
local completion = completion_items(request(client, bufnr, 'textDocument/completion', {
  textDocument = { uri = vim.uri_from_bufnr(bufnr) },
  position = { line = completion_line, character = completion_col },
}))
if #completion == 0 then
  error('installed perllsp completion returned no candidates after current edit')
end

-- Record one navigation/hover cell. Provider breadth belongs to #7124/#7716;
-- the public-artifact row records whether this exact installed subject returns
-- useful hover content without making it a release blocker for every provider.
local hover = request(client, bufnr, 'textDocument/hover', {
  textDocument = { uri = vim.uri_from_bufnr(bufnr) },
  position = { line = 6, character = 5 },
})

-- Exercise an edit-producing provider through actual Neovim. No-change is a
-- legitimate formatting outcome; when edits are returned, apply them using
-- Neovim's own position-encoding-aware edit utility.
local formatting = request(client, bufnr, 'textDocument/formatting', {
  textDocument = { uri = vim.uri_from_bufnr(bufnr) },
  options = { tabSize = 2, insertSpaces = true },
})
local format_edits = type(formatting) == 'table' and formatting or {}
if #format_edits > 0 then
  vim.lsp.util.apply_text_edits(format_edits, bufnr, client.offset_encoding)
end

-- Re-find the marker after formatting rather than assuming formatting preserves
-- line numbers/columns, then re-query to reject a first-response-only false green.
local second_line, second_col = completion_position(bufnr)
local second_completion = completion_items(request(client, bufnr, 'textDocument/completion', {
  textDocument = { uri = vim.uri_from_bufnr(bufnr) },
  position = { line = second_line, character = second_col },
}))
if #second_completion == 0 then
  error('installed perllsp completion failed after formatting/edit application')
end

local receipt = {
  nvim = vim.version(),
  install_source = install_source,
  perllsp = perllsp,
  perllsp_sha256 = artifact_sha256,
  perllsp_version_output = version_output,
  config = config_path,
  root = selected_root,
  initial_diagnostics = initial_diagnostics,
  diagnostics_after_fix = diagnostics_after_fix,
  completion_candidates = #completion,
  post_edit_completion_candidates = #second_completion,
  hover_nonempty = hover ~= nil and hover ~= vim.NIL,
  formatting_edits = #format_edits,
  result = 'pass',
}

for _, active in ipairs(vim.lsp.get_clients()) do
  active:stop(true)
end
wait_until(3000, function()
  return #vim.lsp.get_clients() == 0
end, 'perllsp shutdown')

io.stdout:write(vim.json.encode(receipt) .. '\n')

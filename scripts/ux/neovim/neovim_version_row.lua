-- Bounded actual-Neovim compatibility row for #7716.
--
-- The shell matrix invokes this script once per exact Neovim executable. This
-- is intentionally smaller than #7124: it proves the client/version surface,
-- current answers, formatting, semantic-token delta, optional-feature state,
-- configuration transport evidence, and shutdown without duplicating parser
-- lifecycle torture tests.

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

local function read_file(path)
  local file = io.open(path, 'rb')
  if not file then
    return ''
  end
  local contents = file:read('*a') or ''
  file:close()
  return contents
end

local function completion_items(result)
  if type(result) == 'table' and type(result.items) == 'table' then
    return result.items
  end
  return type(result) == 'table' and result or {}
end

local function find_position(bufnr, needle, occurrence)
  occurrence = occurrence or 1
  local seen = 0
  for zero_index, line in ipairs(vim.api.nvim_buf_get_lines(bufnr, 0, -1, false)) do
    local start = 1
    while true do
      local found = line:find(needle, start, true)
      if not found then
        break
      end
      seen = seen + 1
      if seen == occurrence then
        return {
          line = zero_index - 1,
          character = found - 1 + #needle,
        }
      end
      start = found + #needle
    end
  end
  error(('fixture marker %q occurrence %d not found'):format(needle, occurrence))
end

local function find_inside_position(bufnr, needle, occurrence)
  local position = find_position(bufnr, needle, occurrence)
  -- `find_position` intentionally returns the end of a token for completion.
  -- Move the cursor back inside the symbol for hover/navigation point queries.
  position.character = math.max(0, position.character - math.max(1, math.floor(#needle / 2)))
  return position
end

local function diagnostic_fingerprint(bufnr)
  local normalized = {}
  for _, diagnostic in ipairs(vim.diagnostic.get(bufnr)) do
    normalized[#normalized + 1] = {
      lnum = diagnostic.lnum,
      col = diagnostic.col,
      end_lnum = diagnostic.end_lnum,
      end_col = diagnostic.end_col,
      severity = diagnostic.severity,
      message = diagnostic.message,
      source = diagnostic.source,
      code = diagnostic.code,
    }
  end
  table.sort(normalized, function(a, b)
    local ak = ('%08d:%08d:%s'):format(a.lnum or 0, a.col or 0, a.message or '')
    local bk = ('%08d:%08d:%s'):format(b.lnum or 0, b.col or 0, b.message or '')
    return ak < bk
  end)
  return vim.json.encode(normalized)
end

local function nonblank(value)
  return type(value) == 'string' and value:match('%S') ~= nil
end

local function hover_has_content(hover)
  if type(hover) ~= 'table' then
    return false
  end
  local contents = hover.contents
  if nonblank(contents) then
    return true
  end
  if type(contents) ~= 'table' then
    return false
  end
  if nonblank(contents.value) then
    return true
  end
  for _, item in ipairs(contents) do
    if nonblank(item) then
      return true
    end
    if type(item) == 'table' and (nonblank(item.value) or nonblank(item.language)) then
      return true
    end
  end
  return false
end

local function contains_value(value, needle)
  if type(value) == 'string' then
    return value == needle
  end
  if type(value) ~= 'table' then
    return false
  end
  for _, child in pairs(value) do
    if contains_value(child, needle) then
      return true
    end
  end
  return false
end

local function optional_feature_state(client, bufnr)
  local states = {}

  states.inlay_hint = {
    client_support = client:supports_method('textDocument/inlayHint'),
    api_available = type(vim.lsp.inlay_hint) == 'table'
      and type(vim.lsp.inlay_hint.enable) == 'function',
    enabled = false,
  }
  if states.inlay_hint.client_support and states.inlay_hint.api_available then
    local ok = pcall(vim.lsp.inlay_hint.enable, true, { bufnr = bufnr })
    states.inlay_hint.enabled = ok
      and type(vim.lsp.inlay_hint.is_enabled) == 'function'
      and vim.lsp.inlay_hint.is_enabled({ bufnr = bufnr })
      or false
  end

  states.completion = {
    client_support = client:supports_method('textDocument/completion'),
    api_available = type(vim.lsp.completion) == 'table'
      and type(vim.lsp.completion.enable) == 'function',
    opt_in_succeeded = false,
  }
  if states.completion.client_support and states.completion.api_available then
    states.completion.opt_in_succeeded = pcall(
      vim.lsp.completion.enable,
      true,
      client.id,
      bufnr,
      { autotrigger = false }
    )
  end

  states.code_lens = {
    client_support = client:supports_method('textDocument/codeLens'),
    api_available = type(vim.lsp.codelens) == 'table',
    default_visible = false,
  }
  states.linked_editing = {
    client_support = client:supports_method('textDocument/linkedEditingRange'),
    api_available = type(vim.lsp.buf.linked_editing_range) == 'function',
    default_visible = false,
  }
  states.inline_completion = {
    client_support = client:supports_method('textDocument/inlineCompletion'),
    api_available = type(vim.lsp.inline_completion) == 'table'
      and type(vim.lsp.inline_completion.enable) == 'function',
    opt_in_succeeded = false,
  }
  if states.inline_completion.client_support and states.inline_completion.api_available then
    states.inline_completion.opt_in_succeeded = pcall(
      vim.lsp.inline_completion.enable,
      true,
      { bufnr = bufnr }
    )
  end

  return states
end

if vim.fn.has('nvim-0.11.3') ~= 1 then
  error('Neovim 0.11.3+ is required by the supported-version matrix')
end

local repo_root = normalize(required_env('REPO_ROOT'))
local fixture_root = normalize(required_env('FIXTURE_ROOT'))
local perllsp = normalize(required_env('PERLLSP'))
local row_label = required_env('NEOVIM_ROW_LABEL')
local expected_prefix = required_env('NEOVIM_EXPECTED_PREFIX')

local version = vim.version()
local version_string = ('%d.%d.%d'):format(version.major, version.minor, version.patch)
if version_string:sub(1, #expected_prefix) ~= expected_prefix then
  error(('Neovim row %s expected version prefix %s, got %s'):format(
    row_label,
    expected_prefix,
    version_string
  ))
end

vim.cmd('filetype on')
if type(vim.lsp.log) == 'table' and type(vim.lsp.log.set_level) == 'function' then
  vim.lsp.log.set_level('trace')
end

local config_path = repo_root .. '/scripts/ux/neovim/perllsp.lua'
local config = dofile(config_path)
config = vim.deepcopy(config)
config.cmd = { perllsp, '--stdio' }
-- Exercise the modern generic-client settings path. #7768 owns the schema;
-- this row proves Neovim can carry the server-native namespace without falling
-- back to a Neovim-only initializationOptions copy.
config.settings = {
  perl = {
    workspace = {
      includePaths = { 'lib', 'customlib' },
      useSystemInc = false,
      resolutionTimeout = 50,
    },
    inlayHints = {
      enabled = true,
      parameterHints = true,
    },
  },
}

-- Observe the actual value Neovim returns to the server's
-- workspace/configuration request while delegating to Neovim's stock handler.
-- This proves transport payload, not merely that the method name appeared in a
-- trace log.
local configuration_observation = {
  seen = false,
  params = nil,
  response = nil,
}
local default_configuration_handler = vim.lsp.handlers['workspace/configuration']
config.handlers = {
  ['workspace/configuration'] = function(err, params, ctx, handler_config)
    local response = default_configuration_handler(err, params, ctx, handler_config)
    configuration_observation.seen = true
    configuration_observation.params = vim.deepcopy(params)
    configuration_observation.response = vim.deepcopy(response)
    return response
  end,
}

vim.lsp.config('perllsp', config)
vim.lsp.enable('perllsp')

local source_file = fixture_root .. '/lib/App.pm'
vim.cmd('edit ' .. vim.fn.fnameescape(source_file))
if vim.bo.filetype ~= 'perl' then
  error(('expected matrix fixture filetype=perl, got %q'):format(vim.bo.filetype))
end

local bufnr = vim.api.nvim_get_current_buf()
local client = wait_for_client(bufnr)
local root = client.root_dir and normalize(client.root_dir) or ''
if root ~= normalize(fixture_root) then
  error(('row %s selected root %q, expected %q'):format(row_label, root, fixture_root))
end

wait_until(5000, function()
  return configuration_observation.seen
end, 'workspace/configuration request')
if not contains_value(configuration_observation.response, 'customlib') then
  error(('row %s workspace/configuration response did not contain configured include path'):format(row_label))
end
if not contains_value(configuration_observation.response, 50) then
  error(('row %s workspace/configuration response did not contain configured timeout'):format(row_label))
end

local client_caps = client.config.capabilities or {}
local watcher_caps = client_caps.workspace and client_caps.workspace.didChangeWatchedFiles or nil
local virtual_doc_caps = client_caps.workspace and client_caps.workspace.textDocumentContent or nil

-- Diagnostic transport: a malformed initial generation must become visible in
-- the actual Neovim diagnostic store. After a real edit, require the diagnostic
-- fingerprint to change rather than assuming every product diagnostic becomes
-- empty (native critic warnings may legitimately remain).
wait_until(8000, function()
  return #vim.diagnostic.get(bufnr) > 0
end, 'actual Neovim diagnostics')
local initial_diagnostics = #vim.diagnostic.get(bufnr)
local initial_diagnostic_fingerprint = diagnostic_fingerprint(bufnr)

local lines = vim.api.nvim_buf_get_lines(bufnr, 0, -1, false)
for index, line in ipairs(lines) do
  if line:find('my $broken =', 1, true) then
    lines[index] = 'my $broken = 41;'
  end
end
vim.api.nvim_buf_set_lines(bufnr, 0, -1, false, lines)
wait_until(8000, function()
  return diagnostic_fingerprint(bufnr) ~= initial_diagnostic_fingerprint
end, 'diagnostics refresh after real buffer edit')
local after_edit_diagnostics = #vim.diagnostic.get(bufnr)

local log_path = type(vim.lsp.log) == 'table'
  and type(vim.lsp.log.get_filename) == 'function'
  and vim.lsp.log.get_filename()
  or ''
local log_text = log_path ~= '' and read_file(log_path) or ''
local diagnostic_transport = log_text:find('textDocument/diagnostic', 1, true)
  and 'pull_observed'
  or 'diagnostic_store_observed_transport_unclassified'

local completion_position = find_position(bufnr, '$bro')
local completion = completion_items(request(client, bufnr, 'textDocument/completion', {
  textDocument = { uri = vim.uri_from_bufnr(bufnr) },
  position = completion_position,
}))
if #completion == 0 then
  error(('row %s completion returned no candidates'):format(row_label))
end

local hover_position = find_inside_position(bufnr, '$broken', 1)
local hover = request(client, bufnr, 'textDocument/hover', {
  textDocument = { uri = vim.uri_from_bufnr(bufnr) },
  position = hover_position,
})
if not hover_has_content(hover) then
  error(('row %s hover returned no useful content at $broken'):format(row_label))
end

-- Formatting is an actual-host cell. No-change is valid; edits, when present,
-- are applied by Neovim's own formatting command.
if not client:supports_method('textDocument/formatting') then
  error(('row %s did not expose textDocument/formatting'):format(row_label))
end
local before_format = table.concat(vim.api.nvim_buf_get_lines(bufnr, 0, -1, false), '\n')
vim.lsp.buf.format({ bufnr = bufnr, id = client.id, async = false, timeout_ms = 5000 })
local after_format = table.concat(vim.api.nvim_buf_get_lines(bufnr, 0, -1, false), '\n')

-- Full + delta semantic tokens are a required compatibility cell for this
-- matrix. Losing advertisement, resultId, or the actual delta response is a
-- row failure rather than a silently missing feature.
local semantic = {
  full_supported = client:supports_method('textDocument/semanticTokens/full'),
  full_result_id = nil,
  full_data_items = 0,
  delta_requested = false,
  delta_edits = 0,
}
if not semantic.full_supported then
  error(('row %s lost textDocument/semanticTokens/full support'):format(row_label))
end
local full = request(client, bufnr, 'textDocument/semanticTokens/full', {
  textDocument = { uri = vim.uri_from_bufnr(bufnr) },
})
if type(full) ~= 'table' or not nonblank(full.resultId) then
  error(('row %s semantic-token full result lacked a usable resultId'):format(row_label))
end
semantic.full_result_id = full.resultId
semantic.full_data_items = type(full.data) == 'table' and #full.data or 0
local delta = request(client, bufnr, 'textDocument/semanticTokens/full/delta', {
  textDocument = { uri = vim.uri_from_bufnr(bufnr) },
  previousResultId = semantic.full_result_id,
})
if type(delta) ~= 'table' or type(delta.edits) ~= 'table' then
  error(('row %s semantic-token delta request did not return an edits array'):format(row_label))
end
semantic.delta_requested = true
semantic.delta_edits = #delta.edits

local optional = optional_feature_state(client, bufnr)

local receipt = {
  row = row_label,
  nvim = version,
  nvim_version = version_string,
  platform = vim.uv.os_uname(),
  perllsp = perllsp,
  config = config_path,
  root = root,
  offset_encoding = client.offset_encoding,
  client_capabilities = {
    diagnostic = client_caps.textDocument and client_caps.textDocument.diagnostic ~= nil or false,
    watched_files_dynamic_registration = watcher_caps and watcher_caps.dynamicRegistration or false,
    watched_files_relative_pattern = watcher_caps and watcher_caps.relativePatternSupport or false,
    workspace_configuration = client_caps.workspace and client_caps.workspace.configuration or false,
    workspace_folders = client_caps.workspace and client_caps.workspace.workspaceFolders or false,
    text_document_content = virtual_doc_caps ~= nil,
  },
  diagnostics = {
    initial = initial_diagnostics,
    after_edit = after_edit_diagnostics,
    changed_after_edit = true,
    transport = diagnostic_transport,
  },
  settings_transport = {
    state = 'workspace_configuration_observed',
    params = configuration_observation.params,
    response = configuration_observation.response,
  },
  completion_candidates = #completion,
  hover_nonempty = true,
  formatting = {
    supported = true,
    changed_buffer = before_format ~= after_format,
  },
  semantic_tokens = semantic,
  optional_features = optional,
  virtual_documents = {
    actual_client_capability = virtual_doc_caps ~= nil,
    result = virtual_doc_caps ~= nil and 'client_capability_present' or 'upstream_dependency',
  },
  result = 'pass',
}

-- Exercise the graceful client shutdown path: `Client:stop()` sends shutdown,
-- waits for the response, sends exit, and closes the RPC. Force-stop would hide
-- a broken shutdown handler.
for _, active in ipairs(vim.lsp.get_clients()) do
  active:stop()
end
wait_until(5000, function()
  return #vim.lsp.get_clients() == 0
end, 'graceful perllsp shutdown')

io.stdout:write(vim.json.encode(receipt) .. '\n')

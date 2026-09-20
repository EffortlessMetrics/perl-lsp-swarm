-- Actual Neovim + nvim-dap + perl-dap preview receipt for #7773.
--
-- The wrapper supplies an exact perl-dap executable and an exact nvim-dap
-- checkout/runtimepath. This probe exercises the real client rather than a raw
-- DAP harness.

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

local repo_root = normalize(required_env('REPO_ROOT'))
local fixture_root = normalize(required_env('FIXTURE_ROOT'))
local perl_dap = normalize(required_env('PERL_DAP'))
local nvim_dap_rtp = normalize(required_env('NVIM_DAP_RTP'))
local nvim_dap_identity = required_env('NVIM_DAP_IDENTITY')

vim.opt.runtimepath:prepend(nvim_dap_rtp)
local dap = require('dap')
local breakpoints = require('dap.breakpoints')
local canonical = dofile(repo_root .. '/scripts/ux/neovim/perl_dap.lua')

local adapter = vim.deepcopy(canonical.adapter)
adapter.command = perl_dap
dap.adapters.perl = adapter

dap.configurations.perl = vim.deepcopy(canonical.configurations)
local config = vim.deepcopy(dap.configurations.perl[1])
local program = fixture_root .. '/debug_target.pl'
config.program = program
config.cwd = fixture_root
config.stopOnEntry = false

vim.cmd('edit ' .. vim.fn.fnameescape(program))
vim.bo.filetype = 'perl'
local bufnr = vim.api.nvim_get_current_buf()
local breakpoint_line = 5
breakpoints.set({}, bufnr, breakpoint_line)

local events = {
  initialized = false,
  stopped = false,
  terminated = false,
  exited = false,
}
local breakpoint_response
local stopped_reason

local listener_key = 'perl-lsp-neovim-dap-receipt'
dap.listeners.after.event_initialized[listener_key] = function()
  events.initialized = true
end
dap.listeners.after.setBreakpoints[listener_key] = function(_, _, response)
  breakpoint_response = response
end
dap.listeners.after.event_stopped[listener_key] = function(_, body)
  events.stopped = true
  stopped_reason = body and body.reason or nil
end
dap.listeners.after.event_terminated[listener_key] = function()
  events.terminated = true
end
dap.listeners.after.event_exited[listener_key] = function()
  events.exited = true
end

dap.run(config, { filetype = 'perl' })

wait_until(12000, function()
  return events.initialized and events.stopped
end, 'nvim-dap initialization and breakpoint stop')

if stopped_reason ~= 'breakpoint' then
  error(('expected breakpoint stop, got reason %q'):format(stopped_reason))
end

local session = dap.session()
if not session then
  error('nvim-dap session disappeared before inspection')
end
wait_until(5000, function()
  local frame = session.current_frame
  if not frame or type(frame.scopes) ~= 'table' or #frame.scopes == 0 then
    return false
  end
  for _, scope in ipairs(frame.scopes) do
    if not scope.expensive and type(scope.variables) == 'table' and #scope.variables > 0 then
      return true
    end
  end
  return false
end, 'stack/scopes/real variables')

local frame = assert(session.current_frame, 'nvim-dap did not retain a current frame')
local frame_source = frame.source and frame.source.path and normalize(frame.source.path) or ''
if frame.line ~= breakpoint_line then
  error(('breakpoint stop landed at line %s, expected %d'):format(tostring(frame.line), breakpoint_line))
end
if frame_source ~= normalize(program) then
  error(('breakpoint stop landed in %q, expected %q'):format(frame_source, normalize(program)))
end

local variable_count = 0
local observed_value
for _, scope in ipairs(frame.scopes or {}) do
  for _, variable in ipairs(scope.variables or {}) do
    variable_count = variable_count + 1
    if variable.name == '$value' or variable.name == 'value' then
      observed_value = variable.value
    end
  end
end
if variable_count == 0 then
  error('nvim-dap received no real variables at the stopped frame')
end

local evaluate_done = false
local evaluate_error
local evaluate_result
session:request('evaluate', {
  expression = '$value',
  frameId = frame.id,
  context = 'repl',
}, function(err, response)
  evaluate_error = err
  evaluate_result = response and response.result or nil
  evaluate_done = true
end)
wait_until(5000, function()
  return evaluate_done
end, 'bounded evaluate response')

-- #2301 owns the final evaluate claim. A protocol error or a success-shaped
-- response without an actual result is a bounded error here, never a pass.
local evaluate_state
local evaluate_error_text
if evaluate_error then
  evaluate_state = 'bounded_error'
  evaluate_error_text = tostring(evaluate_error)
elseif type(evaluate_result) ~= 'string' or evaluate_result == '' then
  evaluate_state = 'bounded_error'
  evaluate_error_text = 'evaluate response missing non-empty result'
else
  evaluate_state = 'pass'
end

dap.continue()
wait_until(10000, function()
  return events.terminated or events.exited or dap.session() == nil
end, 'debuggee termination')

if dap.session() ~= nil then
  dap.terminate()
  wait_until(5000, function()
    return dap.session() == nil
  end, 'adapter cleanup after terminate')
end

local verified_breakpoint = false
for _, breakpoint in ipairs((breakpoint_response or {}).breakpoints or {}) do
  if breakpoint.verified then
    verified_breakpoint = true
    break
  end
end
if not verified_breakpoint then
  error('nvim-dap did not receive a verified breakpoint from perl-dap')
end

local receipt = {
  nvim = vim.version(),
  nvim_dap = nvim_dap_identity,
  perl_dap = perl_dap,
  adapter = {
    type = adapter.type,
    command = adapter.command,
    args = adapter.args,
  },
  launch = {
    program = program,
    cwd = fixture_root,
    breakpoint_line = breakpoint_line,
    breakpoint_verified = verified_breakpoint,
  },
  events = events,
  stopped_reason = stopped_reason,
  frame = {
    id = frame.id,
    name = frame.name,
    line = frame.line,
    source = frame_source,
  },
  variable_count = variable_count,
  observed_value = observed_value,
  evaluate = {
    state = evaluate_state,
    result = evaluate_result,
    error = evaluate_error_text,
  },
  result = 'pass',
}

io.stdout:write(vim.json.encode(receipt) .. '\n')

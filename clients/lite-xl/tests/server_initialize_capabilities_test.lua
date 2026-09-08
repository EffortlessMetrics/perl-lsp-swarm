-- Deterministic focused tests for the initialize client-capability payload
-- of clients/lite-xl/upstream/server.lua (#11172).
--
-- Run:
--   lua clients/lite-xl/tests/server_initialize_capabilities_test.lua
--     [path-to-server-module] [path-to-manifest-module]
-- Default paths are ../upstream/server.lua and ../upstream/capability_manifest.lua
-- relative to this file.
--
-- Seam owned: the EXACT initialize payload the staged client sends. The
-- manifest module is the single advertisement authority; server.lua folds
-- its default capabilities through manifest.client_capabilities() and then
-- merges user custom_capabilities on top (user freedom, never repository
-- evidence). This suite pins both directions:
--
--   - the wire payload equals the reconciled truth table exactly: consumed
--     leaves present (workspace.configuration, didSave, completion forms
--     incl. resolveSupport limited to #12547-consumed fields, hover and
--     signature formats, symbol kind lists, versionSupport, dataSupport's
--     explicit false, showDocument, utf-16 positionEncodings);
--   - unconsumed leaves are absent BYTES ON THE WIRE: relatedInformation,
--     tagSupport and codeDescriptionSupport appear nowhere in the encoded
--     frame;
--   - snippetSupport follows the exact optional-dependency state;
--   - custom_capabilities reach the payload (user freedom) while known
--     unimplemented claims are warned about and never touch the manifest's
--     canonical rows;
--   - two independent instances produce identical payloads.
--
-- Red-first baseline: against CURRENT MAIN before #11172 there is no
-- capability_manifest.lua; the pristine inline payload advertises
-- relatedInformation/tagSupport/codeDescriptionSupport, so the absence pins
-- and the exact-truth deep compare MUST fail there (the require itself does
-- not fail because pristine server.lua predates the manifest dependency).
-- Observed pristine-main baseline: 5 assertion failures plus the final
-- block's manifest-require error. Observed patched result: 13 passed,
-- 0 failed.
--
-- Mutation falsifiers of the PATCHED module (each mechanically verified to
-- fail the suite against a mutated server copy):
--   1. re-add `relatedInformation = true` to the folded defaults -> the
--      byte-absence pin and the exact deep compare fail;
--   2. drop the custom-capability merge -> the override case fails;
--   3. delete the unsupported-claim warning -> the warning case fails;
--   4. hardcode snippetSupport=true -> the dependency-state cases fail.
--
-- No framework: plain soft asserts, one process, deterministic (fake clock,
-- no sleeps), exit code carries the result. Compatible with the Lite XL Lua
-- runtime family (Lua 5.4).

local server_module_path = arg and arg[1] or nil
local manifest_module_path = arg and arg[2] or nil

local here = debug.getinfo(1, "S").source:sub(2):match("^(.*)[/\\]") or "."
if not server_module_path then
  server_module_path = here .. "/../upstream/server.lua"
end
if not manifest_module_path then
  manifest_module_path = here .. "/../upstream/capability_manifest.lua"
end

-- ---------------------------------------------------------------------------
-- Soft assertion collector
-- ---------------------------------------------------------------------------

local passed, failed = 0, 0
local function ok(condition, message)
  if condition then
    passed = passed + 1
  else
    failed = failed + 1
    print("FAIL: " .. message)
  end
end

-- ---------------------------------------------------------------------------
-- Lite XL runtime fakes (same shape as server_frame_test.lua). util is the
-- REAL staged module because initialize runs the actual path_to_uri
-- conversion authority.
-- ---------------------------------------------------------------------------

package.preload["plugins.lsp.json"] = function()
  return dofile(here .. "/../upstream/json.lua")
end

package.preload["core"] = function()
  return { log = function() end }
end

package.preload["core.common"] = function()
  return {
    merge = function(base, override)
      local result = {}
      for k, v in pairs(base) do result[k] = v end
      if type(override) == "table" then
        for k, v in pairs(override) do result[k] = v end
      end
      return result
    end,
  }
end

package.preload["core.config"] = function()
  return { plugins = {} }
end

package.preload["process"] = function()
  return {
    start = function()
      return { running = function() return true end }
    end,
    REDIRECT_PIPE = 1,
  }
end

package.preload["plugins.lsp.util"] = function()
  return dofile(here .. "/../upstream/util.lua")
end

-- Pristine server.lua reads diagnostics.tag for the tagSupport value set;
-- the reconciled payload no longer carries it but the fake stays so the
-- red-first baseline loads cleanly.
package.preload["plugins.lsp.diagnostics"] = function()
  return { tag = { UNNECESSARY = 1, DEPRECATED = 2 } }
end

package.preload["core.object"] = function()
  local Object = {}
  Object.__index = Object
  function Object:extend()
    local cls = setmetatable({}, self)
    cls.__index = cls
    cls.super = self
    return cls
  end
  function Object:new() end
  return Object
end

system = { get_time = function() return 0 end }

-- The conversion authority classifies paths by platform, exactly like a
-- Lite XL session would.
local original_platform = PLATFORM
local original_userdir = USERDIR
PLATFORM = "Windows"
USERDIR = "."

local fake_epoch = 1000
local original_os_time = os.time
os.time = function() return fake_epoch end

package.preload["plugins.lsp.capability_manifest"] = function()
  return dofile(manifest_module_path)
end

local server_module = dofile(server_module_path)
local json = require "plugins.lsp.json"

-- ---------------------------------------------------------------------------
-- Instance construction: bare lsp.server wired like
-- server_message_scheduling_test.lua, plus exactly the fields
-- Server:initialize consumes.
-- ---------------------------------------------------------------------------

local log_records = {}

local function fresh_server(opts)
  opts = opts or {}
  log_records = {}
  local server = setmetatable({
    name = "test-perllsp",
    verbose = false,
    initialized = false,
    fatal_error = false,
    write_fails = 0,
    write_fails_before_shutdown = 60,
    max_queued_requests = server_module.MAX_QUEUED_REQUESTS,
    request_list = {},
    response_list = {},
    notification_list = {},
    raw_list = {},
    request_listeners = {},
    message_listeners = {},
    event_listeners = {},
    current_request = 0,
    init_options = {},
    settings = nil,
    snippets = opts.snippets,
    fake_snippets = opts.fake_snippets,
    custom_capabilities = opts.custom_capabilities,
    proc = { running = function() return true end },
  }, { __index = server_module })
  server.wire = {}
  server.write_request = function(self, data)
    self.wire[#self.wire + 1] = data
    return true
  end
  server.shutdown_calls = 0
  server.shutdown_if_needed = function(self)
    self.shutdown_calls = self.shutdown_calls + 1
  end
  server.log = function(self, message, ...)
    log_records[#log_records + 1] = tostring(string.format(message, ...))
  end
  return server
end

local function initialize_params(server)
  -- The staged client queues the initialize request; the transport loop
  -- stays outside this suite (harness boundary). One entry must exist.
  local request = server.request_list[#server.request_list]
  assert(request ~= nil, "initialize queued no request")
  assert(request.method == "initialize",
    "last queued request was not initialize: " .. tostring(request.method))
  return request.params
end

---Encoded capabilities bytes for absence pins: exactly what the real codec
---would put on the wire for this payload.
local function encoded_capabilities(params)
  return json.encode(params.capabilities)
end

local function deep_equal(a, b)
  if a == b then return true end
  if type(a) ~= "table" or type(b) ~= "table" then return false end
  local count = 0
  for k, v in pairs(a) do
    count = count + 1
    if not deep_equal(v, b[k]) then return false end
  end
  for _ in pairs(b) do
    count = count - 1
  end
  return count == 0
end

-- ---------------------------------------------------------------------------
-- Expected truth table after reconciliation (#11172). Kind lists and the
-- position encoding come from the real module constants so this suite pins
-- forwarding fidelity without duplicating upstream values.
-- ---------------------------------------------------------------------------

local function expected_capabilities(snippet_support)
  return {
    workspace = {
      configuration = true,
    },
    textDocument = {
      synchronization = {
        didSave = true,
      },
      completion = {
        completionItem = {
          snippetSupport = snippet_support,
          documentationFormat = { "plaintext" },
          insertReplaceSupport = true,
          resolveSupport = {
            properties = { "documentation", "detail", "additionalTextEdits" },
          },
        },
        completionItemKind = {
          valueSet = server_module.get_completion_items_kind_list(),
        },
      },
      hover = {
        contentFormat = { "markdown", "plaintext" },
      },
      signatureHelp = {
        signatureInformation = {
          documentationFormat = { "plaintext" },
        },
      },
      documentSymbol = {
        symbolKind = {
          valueSet = server_module.get_symbols_kind_list(),
        },
      },
      publishDiagnostics = {
        versionSupport = true,
        dataSupport = false,
      },
    },
    window = {
      showDocument = { support = true },
    },
    general = {
      positionEncodings = { server_module.position_encoding_kind.UTF16 },
    },
  }
end

-- ---------------------------------------------------------------------------
-- Exact truth on the wire
-- ---------------------------------------------------------------------------

do
  local s = fresh_server()
  ok(s:initialize("C:/proj", "litexl-test", "1.0") == true,
    "initialize succeeds through the conversion authority")

  local params = initialize_params(s)
  ok(deep_equal(params.capabilities, expected_capabilities(false)),
    "payload equals the reconciled truth table exactly (no snippet plugin)")
  ok(params.rootUri ~= nil and params.workspaceFolders ~= nil,
    "initialize still carries the single-root workspace identity")
end

do
  -- Byte-level absence pins over the exact encoded payload: unconsumed
  -- leaves must not even be encodable onto the wire.
  local s = fresh_server()
  s:initialize("C:/proj", "litexl-test", "1.0")
  local frame_bytes =
    encoded_capabilities(initialize_params(s))
  ok(frame_bytes:find("relatedInformation", 1, true) == nil,
    "encoded payload carries no relatedInformation")
  ok(frame_bytes:find("tagSupport", 1, true) == nil,
    "encoded payload carries no tagSupport")
  ok(frame_bytes:find("codeDescriptionSupport", 1, true) == nil,
    "encoded payload carries no codeDescriptionSupport")
end

do
  -- Optional dependency state drives snippetSupport exactly.
  local s = fresh_server({ snippets = true })
  s:initialize("C:/proj", "litexl-test", "1.0")
  local params = initialize_params(s)
  ok(params.capabilities.textDocument.completion.completionItem.snippetSupport
    == true,
    "installed snippets dependency advertises snippetSupport")
  ok(deep_equal(params.capabilities, expected_capabilities(true)),
    "snippets-on payload equals the truth table exactly")
end

-- ---------------------------------------------------------------------------
-- Determinism across instances
-- ---------------------------------------------------------------------------

do
  local a = fresh_server()
  a:initialize("C:/proj", "litexl-test", "1.0")
  local b = fresh_server()
  b:initialize("C:/proj", "litexl-test", "1.0")
  ok(deep_equal(initialize_params(a).capabilities,
                initialize_params(b).capabilities),
    "two independent instances advertise identically")
end

-- ---------------------------------------------------------------------------
-- Custom capabilities: user freedom with truthful warnings (#11172 rule 6)
-- ---------------------------------------------------------------------------

do
  local s = fresh_server({
    custom_capabilities = {
      workspace = { configuration = false },
    },
  })
  s:initialize("C:/proj", "litexl-test", "1.0")
  local caps = initialize_params(s).capabilities
  ok(caps.workspace.configuration == false,
    "user overrides reach the payload (custom merge preserved)")
  ok(caps.textDocument.publishDiagnostics.versionSupport == true,
    "overrides leave canonical implemented rows intact")
end

do
  local s = fresh_server({
    custom_capabilities = {
      textDocument = {
        semanticTokens = { requests = { full = true } },
      },
    },
  })
  s:initialize("C:/proj", "litexl-test", "1.0")
  local warned = false
  for _, record in ipairs(log_records) do
    if record:find("semanticTokens", 1, true) ~= nil then
      warned = true
    end
  end
  ok(warned,
    "a custom claim over an unimplemented row produces one explicit warning")

  -- The canonical matrix itself is untouched by any override.
  local manifest = require "plugins.lsp.capability_manifest"
  local untouched = true
  for _, row in ipairs(manifest.capabilities) do
    if row.id == "textDocument.semanticTokens"
      and row.disposition ~= "unsupported"
    then
      untouched = false
    end
  end
  ok(untouched,
    "custom claims never become canonical repository evidence")
end

do
  -- Initialize timeout policy is policy-owned (#10657): the real
  -- Server:initialize path queues NO explicit timeout, the longer
  -- INITIALIZE_REQUEST_TIMEOUT governs, and even beyond it id=1 was on the
  -- wire exactly once with a terminal typed expiry - never re-emitted.
  local s = fresh_server()
  ok(s:initialize("C:/proj", "litexl-test", "1.0") == true,
    "initialize succeeds for the timeout-policy case")
  local request = initialize_params(s)
  local queued = s.request_list[#s.request_list]
  ok(queued.timeout == nil,
    "production initialize carries no explicit timeout; policy owns pacing")
  ok(server_module.INITIALIZE_REQUEST_TIMEOUT == 120
    and server_module.DEFAULT_REQUEST_TIMEOUT == 30,
    "single-send timeout policies are named module constants")
  s:process_requests()
  local frames = 0
  for _, frame_data in ipairs(s.wire) do
    local decoded = json.decode(frame_data)
    if decoded.id == 1 then frames = frames + 1 end
  end
  ok(frames == 1, "the initialize request hits the wire exactly once")
  fake_epoch = fake_epoch + server_module.INITIALIZE_REQUEST_TIMEOUT + 1
  s:process_requests()
  fake_epoch = fake_epoch + server_module.INITIALIZE_REQUEST_TIMEOUT + 1
  s.process_requests(s)
  frames = 0
  for _, frame_data in ipairs(s.wire) do
    local decoded = json.decode(frame_data)
    if decoded.id == 1 then frames = frames + 1 end
  end
  ok(frames == 1 and #s.request_list == 0,
    "a slow initialize expires terminally without a second id=1 frame")
end

os.time = original_os_time
PLATFORM = original_platform
USERDIR = original_userdir

print(string.format("\n%d passed, %d failed", passed, failed))
if failed > 0 then os.exit(1) end

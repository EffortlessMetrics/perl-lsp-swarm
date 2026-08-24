-- Deterministic focused tests for positional workspace/configuration
-- responses in clients/lite-xl/upstream/init.lua (#11147).
--
-- Run:
--   lua clients/lite-xl/tests/init_configuration_items_test.lua [path-to-init-module]
-- Default module path is ../upstream/init.lua relative to this file.
--
-- Proof shape: the exact staged init.lua module is loaded under minimal Lite
-- XL runtime fakes whose plugins.lsp.server constructor records the real
-- registered listeners, so lsp.start_server attaches the production
-- workspace/configuration handler and tests dispatch requests through it.
-- Tests assert request-order response slots, duplicate-slot retention,
-- explicit-null unresolved slots, [] for zero items, string request-id
-- preservation, exact cardinality, and one fail-honest InvalidParams
-- response without any partial result for malformed items.
--
-- Red-first baseline: run this suite against the PRISTINE upstream init.lua
-- @ d1432ae0736cd9531798b4bc1221835f534cc689. There the malformed-items
-- cases (object/null items) crash inside the pairs()-based listener instead
-- of answering one InvalidParams response, and no array-shape validation
-- exists, so those cases MUST fail; ordering/duplicates may pass on pristine
-- where Lua iteration happened to be sequential, and remain as contract pins.
--
-- Mutation falsifiers of the PATCHED module (each verified to be caught):
--   1. delete the json.is_array/items validation block -> both
--      invalid-params cases fail;
--   2. iterate `for i = #items, 1, -1` -> order_follows_request fails;
--   3. drop the per-item null slot insert (`value or json.null`) ->
--      missing_section_keeps_null_slot fails;
--   4. return plain settings_list without json.array tagging plus an encoder
--      that treats empty tables as objects -> zero_items_answer_empty_array
--      fails.
--
-- Effective-settings extension (#11143): the two include_paths cases at the
-- end drive the real get_workspace_settings merge through the production
-- listener with fixture-backed USERDIR/.lite_lsp.lua probes (io.open and
-- dofile are temporarily bound to deterministic fixture answers, no
-- filesystem access). Red-first: pass the PRISTINE upstream util.lua @ d1432ae
-- (blob 588c101aa97ef0d112926aac316e7a95a52a6994) as the second argument and
-- both cases fail there — the legacy recursive numeric-key deep_merge leaves
-- an inherited ["src","vendor"] tail and cannot clear a list with an empty
-- array. The same two cases also fail against the #11143 mutation copy with
-- the array-replacement branch removed (documented in
-- util_config_merge_test.lua).
--
-- No framework: plain soft asserts, one process, deterministic, exit code
-- carries the result. Compatible with the Lite XL Lua runtime family
-- (Lua 5.4).

local init_module_path = arg and arg[1] or nil

if not init_module_path then
  local info = debug.getinfo(1, "S").source:sub(2)
  local dir = info:match("^(.*)[/\\]") or "."
  init_module_path = dir .. "/../upstream/init.lua"
end

local util_module_path = arg and arg[2] or nil

if not util_module_path then
  local info = debug.getinfo(1, "S").source:sub(2)
  local dir = info:match("^(.*)[/\\]") or "."
  util_module_path = dir .. "/../upstream/util.lua"
end

local here = debug.getinfo(1, "S").source:sub(2):match("^(.*)[/\\]") or "."

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
-- Lite XL runtime fakes: only what loading and exercising the exact staged
-- init.lua requires.
-- ---------------------------------------------------------------------------

local log_records = {}
package.preload["core"] = function()
  return {
    docs = {},
    log = function(fmt, ...) log_records[#log_records + 1] = tostring(fmt) end,
    log_quiet = function(fmt, ...) log_records[#log_records + 1] = tostring(fmt) end,
    error = function(fmt, ...) log_records[#log_records + 1] = "error:" .. tostring(fmt) end,
    add_thread = function() end,
    project_absolute_path = function(path) return path end,
    normalize_to_project_dir = function(path) return path end,
    home_expand = function(path) return path end,
    command_view = {},
    status_view = { separator2 = 2, add_item = function() end },
    active_view = nil,
    root_view = {},
  }
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
    match_pattern = function(text, patterns)
      for _, pattern in ipairs(patterns) do
        if text:match(pattern) then return true end
      end
      return false
    end,
    clamp = function(value, low, high)
      if value < low then return low elseif value > high then return high end
      return value
    end,
    fuzzy_match = function() return {} end,
    normalize_path = function(path) return path end,
    serialize = function(v) return tostring(v) end,
  }
end

package.preload["core.config"] = function()
  return { plugins = { lsp = {} } }
end

package.preload["core.command"] = function()
  return { add = function() end }
end

package.preload["core.style"] = function()
  return { syntax_fonts = {}, warn = {}, icon_font = {}, font = {} }
end

package.preload["core.keymap"] = function()
  return { add = function() end }
end

package.preload["core.doc.translate"] = function()
  return {
    start_of_word = function(_, doc, line, col) return line, col end,
    end_of_word = function(_, doc, line, col) return line, col end,
  }
end

package.preload["plugins.autocomplete"] = function()
  return {
    can_complete = function() return false end,
    complete = function() end,
  }
end

local Doc = { __index = Doc }
Doc.raw_insert = function(self, line, col, text) end
Doc.raw_remove = function(self, line1, col1, line2, col2) end
Doc.get_selection = function(self) return 1, 1, 1, 1 end
Doc.get_char = function() return "" end
package.preload["core.doc"] = function() return Doc end

local DocView = { __index = DocView }
function DocView:extends() return false end
package.preload["core.docview"] = function() return DocView end

package.preload["core.statusview"] = function()
  return { Item = { RIGHT = 2 } }
end

local RootView = { __index = RootView }
package.preload["core.rootview"] = function() return RootView end

package.preload["plugins.lsp.json"] = function()
  return dofile(here .. "/../upstream/json.lua")
end

package.preload["process"] = function()
  return { start = function() return nil end }
end

package.preload["plugins.lsp.util"] = function()
  return dofile(util_module_path)
end

package.preload["plugins.lsp.listbox"] = function()
  return { hide = function() end, show_text = function() end }
end

package.preload["plugins.lsp.diagnostics"] = function()
  return {
    note_provider = function() end,
    close_session = function() end,
    retire_provider = function() end,
    publish = function() return true, nil end,
    -- #12047 render-resolver seam: init.lua registers it unconditionally at load.
    set_render_resolver = function() end,
  }
end

---Callable fake Server module: construction records add_* registrations so
---lsp.start_server attaches the REAL production listeners to a capturable
---client instance. initialize() records its arguments and does nothing else.
local constructed_clients = {}
package.preload["plugins.lsp.server"] = function()
  local module = {}
  function module.construct(_, options)
    local client = {
      name = options.name,
      file_patterns = options.file_patterns,
      settings = options.settings,
      request_listeners = {},
      message_listeners = {},
      event_listeners = {},
    }
    function client:add_request_listener(method, callback)
      self.request_listeners[method] = callback
    end
    function client:add_message_listener(method, callback)
      self.message_listeners[method] = callback
    end
    function client:add_event_listener(name, callback)
      self.event_listeners[name] = callback
    end
    function client:initialize(...)
      self.initialize_args = { ... }
    end
    constructed_clients[#constructed_clients + 1] = client
    return client
  end
  module.message_type = { Error = 1, Warning = 2, Info = 3, Log = 4, Debug = 5 }
  module.text_document_sync_kind = { None = 0, Full = 1, Incremental = 2 }
  module.position_encoding_kind = { UTF8 = "utf-8", UTF16 = "utf-16", UTF32 = "utf-32" }
  module.completion_trigger_Kind = { Invoked = 1, TriggerCharacter = 2 }
  module.symbol_kind = {}
  module.get_symbol_kind = function() return "Text" end
  return setmetatable(module, { __call = module.construct })
end

package.preload["plugins.lsp.timer"] = function()
  return function(interval, one_shot)
    return {
      on_timer = nil,
      start = function() end,
      stop = function() end,
      reset = function() end,
      running = function() return false end,
    }
  end
end

package.preload["plugins.lsp.symbolresults"] = function()
  return {}
end

package.preload["libraries.widget.messagebox"] = function()
  return { BUTTONS_YES_NO = 1, info = function() end }
end

package.preload["plugins.lsp.helpdoc"] = function()
  return function(title) return { title = title, set_text = function() end } end
end

utf8extra = { len = function(s) return utf8.len(s) or #s end }

PLATFORM = "Windows"
USERDIR = "."
SCALE = 1
renderer = { font = { load = function() return {} end } }

system = {
  exec = function() error("shell invoked", 0) end,
  raise_window = function() end,
  get_file_info = function() return { size = 100 } end,
}

-- ---------------------------------------------------------------------------
-- Module loading
-- ---------------------------------------------------------------------------

local function fresh_module_load()
  log_records = {}
  constructed_clients = {}
  return dofile(init_module_path)
end

-- ---------------------------------------------------------------------------
-- Fixture helpers
-- ---------------------------------------------------------------------------

local function make_settings()
  return {
    perl = { alpha = "A", beta = "B", gamma = "C", zeta = "Z" },
  }
end

---Load init.lua, register one fake server definition, run the real
---lsp.start_server path, and return the client that received the real
---listener attachments.
local function start_captured_client(lsp, settings)
  lsp.add_server({
    name = "perllsp",
    file_patterns = { "%.pl$" },
    command = { "perllsp" },
    language = "perl",
    settings = settings,
  })
  lsp.start_server("scratch.pl", ".")
  local client = constructed_clients[#constructed_clients]
  assert(client, "start_server must construct exactly one client")
  return client
end

---Dispatch one request through the real registered listener with a recording
---push_response, returning the captured responses. A listener error is
---returned as { listener_error = ... } instead of aborting the suite.
local function deliver(client, method, id, params)
  local responses = {}
  function client:push_response(m, i, result, err)
    responses[#responses + 1] = { method = m, id = i, result = result, error = err }
  end
  local listener = client.request_listeners[method]
  if not listener then
    return { listener_error = "no listener registered for " .. tostring(method) }
  end
  local ok_run, err = pcall(listener, client, { method = method, id = id, params = params })
  if not ok_run then
    print("LISTENER ERROR [" .. tostring(method) .. "/" .. tostring(id) .. "]: " .. tostring(err))
    return { listener_error = tostring(err) }
  end
  return responses
end

-- ---------------------------------------------------------------------------
-- Cases
-- ---------------------------------------------------------------------------

do
  local lsp = fresh_module_load()
  local client = start_captured_client(lsp, make_settings())
  -- Same codec instance the staged modules required (private array/null
  -- identities are per-instance; decoding with a second dofile'd copy would
  -- make production json.is_array reject our fixtures).
  local json = require "plugins.lsp.json"

  -- Registered through the real start_server path.
  ok(type(client.request_listeners["workspace/configuration"]) == "function",
    "production workspace/configuration listener is attached by start_server")

  -- Wire-shaped helpers: params.items must be one typed JSON array exactly
  -- as the codec decodes it from the protocol.
  local function items_of(json_text)
    return { items = json.decode(json_text) }
  end

  -- order_follows_request: reversed request must reverse the response slots.
  local r = deliver(client, "workspace/configuration", 1,
    items_of('[{"section":"perl.zeta"},{"section":"perl.alpha"}]'))
  ok(r[1] ~= nil and r[1].error == nil, "ordered request answers a result response")
  ok(r[1] and type(r[1].result) == "table" and #r[1].result == 2
    and r[1].result[1] == "Z" and r[1].result[2] == "A",
    "response slot order follows request item order, not configuration key order")

  -- duplicates_retained: identical sections stay distinct response positions.
  r = deliver(client, "workspace/configuration", 2,
    items_of('[{"section":"perl.beta"},{"section":"perl.beta"}]'))
  ok(r[1] and type(r[1].result) == "table" and #r[1].result == 2
    and r[1].result[1] == "B" and r[1].result[2] == "B",
    "duplicate requested sections keep two distinct response slots")

  -- missing_section_keeps_null_slot: unresolved section stays explicit null.
  r = deliver(client, "workspace/configuration", 3,
    items_of('[{"section":"perl.alpha"},{"section":"perl.does_not_exist"},{"section":"perl.gamma"}]'))
  ok(r[1] and type(r[1].result) == "table" and #r[1].result == 3
    and r[1].result[1] == "A"
    and json.is_null(r[1].result[2])
    and r[1].result[3] == "C",
    "unresolved section keeps its slot as explicit JSON null between resolved neighbors")

  -- zero_items_answer_empty_array: no items answer [], never {}.
  r = deliver(client, "workspace/configuration", 4, items_of('[]'))
  ok(r[1] and r[1].error == nil and type(r[1].result) == "table",
    "zero-item request answers a result response")
  ok(r[1] and json.is_array(r[1].result) and #r[1].result == 0
    and json.encode(r[1].result) == "[]",
    "zero-item request answers an explicitly typed empty JSON array")

  -- object_items_invalid_params: non-array items fail honestly, once.
  r = deliver(client, "workspace/configuration", 5,
    { items = json.decode('{"section":"perl.alpha"}') })
  ok(#r == 1 and r[1].result == nil and type(r[1].error) == "table"
    and r[1].error.code == -32602,
    "object-shaped items answer exactly one InvalidParams error response")

  -- null_item_invalid_params: null inside the items array fails honestly.
  r = deliver(client, "workspace/configuration", 6,
    items_of('[{"section":"perl.alpha"},null]'))
  ok(#r == 1 and r[1].result == nil and type(r[1].error) == "table"
    and r[1].error.code == -32602,
    "null item inside items answers exactly one InvalidParams error response")

  -- string_request_id_preserved: generic clients use string ids.
  r = deliver(client, "workspace/configuration", "string-id-7",
    items_of('[{"section":"perl.alpha"}]'))
  ok(r[1] and r[1].id == "string-id-7" and type(r[1].result) == "table"
    and r[1].result[1] == "A",
    "string request id is preserved on the response verbatim")

  -- cardinality_exact: five mixed items produce exactly five slots.
  r = deliver(client, "workspace/configuration", 8,
    items_of('[{"section":"perl.alpha"},{"section":"perl.missing_one"},{"section":"perl.beta"},{"section":"perl.missing_two"},{"section":"perl.zeta"}]'))
  ok(r[1] and type(r[1].result) == "table" and #r[1].result == 5,
    "five requested items produce exactly five result slots")

  -- scope_uri_items_keep_positional_shape: scope resolution stays external;
  -- shape and cardinality of scoped requests stay positional.
  r = deliver(client, "workspace/configuration", 9,
    items_of('[{"section":"perl.alpha","scopeUri":"file:///virtual/workspace"},{"section":"perl.beta","scopeUri":"file:///virtual/workspace"}]'))
  ok(r[1] and type(r[1].result) == "table" and #r[1].result == 2,
    "scoped items keep exact positional cardinality")

  -- no_partial_result_on_malformed: a malformed tail yields error only.
  r = deliver(client, "workspace/configuration", 10,
    items_of('[{"section":"perl.alpha"},"not-an-object"]'))
  ok(#r == 1 and r[1].result == nil and type(r[1].error) == "table"
    and r[1].error.code == -32602,
    "malformed items answer error only, never a partial result")

  -- nested_array_item_invalid_params: items are ConfigurationItems (objects);
  -- an embedded array is malformed, not a silent null slot.
  r = deliver(client, "workspace/configuration", 11, items_of('[["x"]]'))
  ok(#r == 1 and r[1].result == nil and type(r[1].error) == "table"
    and r[1].error.code == -32602,
    "array-shaped item answers InvalidParams instead of becoming a null slot")

  -- scopeUri_non_string_invalid_params: numeric scopeUri must fail honestly
  -- at the validation boundary instead of crashing URI conversion later.
  r = deliver(client, "workspace/configuration", 12,
    items_of('[{"section":"perl.alpha","scopeUri":5}]'))
  ok(#r == 1 and r[1].result == nil and type(r[1].error) == "table"
    and r[1].error.code == -32602,
    "non-string scopeUri answers InvalidParams without reaching URI conversion")
end

-- ---------------------------------------------------------------------------
-- Effective-settings cases (#11143): the real get_workspace_settings typed
-- merge observed through the production workspace/configuration listener.
-- USERDIR/.lite_lsp.lua probes are answered by deterministic fixture binds
-- of io.open and dofile; no filesystem state is read or written.
-- ---------------------------------------------------------------------------

---Temporarily bind io.open (existence probe) and dofile (settings load) for
---`.lite_lsp.lua` paths to one fixture settings table, run `fn`, then restore
---the originals. Returns fn's result or re-raises.
local function with_fixture_lua_settings(lua_settings, fn)
  local original_open, original_dofile = io.open, dofile
  local raw_dofile = original_dofile
  io.open = function(path, mode)
    if type(path) == "string" and path:find("%.lite_lsp%.lua$") then
      return { close = function() end }
    end
    return original_open(path, mode)
  end
  dofile = function(path)
    if type(path) == "string" and path:find("%.lite_lsp%.lua$") then
      return lua_settings
    end
    return raw_dofile(path)
  end
  local ok_run, result = pcall(fn)
  io.open, dofile = original_open, original_dofile
  if not ok_run then error(result, 0) end
  return result
end

do
  -- effective_response_has_no_inherited_tail: a discovered ["lib","vendor"]
  -- list overridden by server.settings ["src"] must answer exactly ["src"]
  -- through the production listener — never the legacy merged tail.
  local lsp = fresh_module_load()
  local json = require "plugins.lsp.json"
  local client = start_captured_client(lsp, {
    perl = { include_paths = {"src"} },
  })
  local responses = with_fixture_lua_settings(
    { perl = { include_paths = {"lib", "vendor"} } },
    function()
      return deliver(client, "workspace/configuration", 21,
        { items = json.decode('[{"section":"perl.include_paths"}]') })
    end)
  ok(responses[1] and responses[1].error == nil
    and type(responses[1].result) == "table"
    and #responses[1].result == 1,
    "effective include_paths request answers one exact slot")
  ok(responses[1] and type(responses[1].result) == "table"
    and responses[1].result[1]
    and json.encode(responses[1].result[1]) == '["src"]',
    "effective merged include_paths carries no inherited vendor tail")
end

do
  -- empty_array_clears_through_listener: an explicitly typed empty array in
  -- server.settings clears a discovered list, observable on the wire as [].
  local lsp = fresh_module_load()
  local json = require "plugins.lsp.json"
  local client = start_captured_client(lsp, {
    perl = { tags = json.array({}) },
  })
  local responses = with_fixture_lua_settings(
    { perl = { tags = {"stale"} } },
    function()
      return deliver(client, "workspace/configuration", 22,
        { items = json.decode('[{"section":"perl.tags"}]') })
    end)
  ok(responses[1] and responses[1].error == nil
    and type(responses[1].result) == "table"
    and #responses[1].result == 1,
    "cleared tags request answers one exact slot")
  ok(responses[1] and type(responses[1].result) == "table"
    and responses[1].result[1]
    and json.encode(responses[1].result[1]) == "[]",
    "explicit empty array clears an inherited list on the effective response")
end

print(string.format("%d passed, %d failed", passed, failed))
if failed > 0 then os.exit(1) end


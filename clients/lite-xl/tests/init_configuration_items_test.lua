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
-- Value-fidelity extension (#10845): the matrix cases prove presence and
-- value are tracked independently through the response payload — an explicit
-- boolean false (top-level or nested), true, 0, "", and [] round-trip exactly,
-- while only a genuinely missing section becomes the null sentinel, including
-- positional coexistence of false and null in one response. Red-first: run
-- this suite against CURRENT MAIN before the #10845 patch (or any copy whose
-- slot append still reads `table.insert(settings_list, value or json.null)`):
-- `value or json.null` converts an explicitly configured false into null, so
-- every false-bearing case MUST fail there. Mutation falsifier of the PATCHED
-- module: restore the single `value or json.null` append line in a copied
-- init.lua and the same cases fail again.
--
-- Trust-boundary extension (#10653): workspace/project configuration is
-- data, never executable code. The scenario fixture binds io.open (existence
-- and content per absolute candidate path), dofile (records EVERY execution
-- target; any project-path hit is compromise evidence), system.get_file_info
-- (deterministic mtimes), and a fake os.time so the 5-second cache window is
-- drivable. Cases prove: a repository .lite_lsp.lua is never executed at any
-- project-derived position (workspace scope, server.path, hostile out-of-root
-- scopeUri) across repeated requests, cache hits, TTL expiry, and restart;
-- project .lite_lsp.json is read as data; malformed project JSON fails safely
-- with one bounded error and an empty value instead of crashing or executing
-- fallback Lua; USERDIR keeps its user-owned .lite_lsp.lua authority; an
-- accepted configuration change invalidates the cached settings inside the
-- freshness window. Red-first: run this suite against CURRENT MAIN before
-- the #10653 patch - get_workspace_settings dofile()s every scanned position,
-- so the hostile-execution, fail-safe-decode, and stamp-invalidation cases
-- MUST fail there (the USERDIR retention pin passes on both sides). Mutation
-- falsifier of the PATCHED module (verified): restore unconditional project
-- .lite_lsp.lua probing/execution at positions > 1 in a copied init.lua and
-- the hostile/fail-safe cases fail again; delete the recorded_stamp cache
-- comparison and the invalidation-inside-TTL case fails.
--
-- No framework: plain soft asserts, one process, deterministic, exit code
-- carries the result. Compatible with the Lite XL runtime family
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

-- Local patch (#11172): the staged modules fold their capability
-- advertisement and command projection through the exact manifest source.
package.preload["plugins.lsp.capability_manifest"] = function()
  return dofile(here .. "/../upstream/capability_manifest.lua")
end
package.preload["plugins.lsp.diagnostics"] = function()
  return {
    note_provider = function() end,
    close_session = function() end,
    retire_provider = function() end,
    set_render_resolver = function() end,
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
-- Value-fidelity cases (#10845): explicit false never becomes null; only a
-- genuinely missing section answers the null sentinel. Presence is tracked
-- separately from value truthiness through the exact response payload.
-- ---------------------------------------------------------------------------

do
  local lsp = fresh_module_load()
  local json = require "plugins.lsp.json"
  local client = start_captured_client(lsp, {
    perl = {
      enabled = false,
      truthy = true,
      zero = 0,
      empty = "",
      emptyList = json.array({}),
      nested = { disabled = false },
    },
  })

  ---Requests one section and returns the single decoded slot value.
  local function one_slot(section, id)
    local r = deliver(client, "workspace/configuration", id,
      { items = json.decode('[{"section":"' .. section .. '"}]') })
    if not (r[1] and r[1].error == nil and type(r[1].result) == "table"
        and #r[1].result == 1) then
      return nil, "~invalid response shape~"
    end
    return r[1].result[1]
  end

  -- false stays false: an explicitly configured disable must not decode as null.
  local enabled = one_slot("perl.enabled", 31)
  ok(enabled == false,
    "explicit boolean false round-trips exactly as JSON false")

  -- true / 0 / "" keep their exact identities.
  ok(one_slot("perl.truthy", 32) == true, "explicit true round-trips exactly")
  ok(one_slot("perl.zero", 33) == 0, "zero keeps its numeric identity")
  ok(one_slot("perl.empty", 34) == "", "empty string keeps its string identity")

  -- [] stays an explicitly typed empty array.
  local empty_list = one_slot("perl.emptyList", 35)
  ok(type(empty_list) == "table" and json.is_array(empty_list)
    and #empty_list == 0 and json.encode(empty_list) == "[]",
    "empty list round-trips as an explicitly typed empty JSON array")

  -- Nested false survives section traversal at depth.
  ok(one_slot("perl.nested.disabled", 36) == false,
    "nested boolean false survives traversal exactly")

  -- Only a genuinely absent section becomes the null sentinel.
  ok(json.is_null(one_slot("perl.absent", 37)),
    "genuinely missing section still answers the explicit null sentinel")

  -- Positional coexistence: false, null and 0 keep distinct slots in ONE
  -- response array — the discriminator between value fidelity and collapse.
  local r = deliver(client, "workspace/configuration", 38,
    { items = json.decode(
      '[{"section":"perl.enabled"},{"section":"perl.absent"},{"section":"perl.zero"}]') })
  ok(r[1] and r[1].error == nil and type(r[1].result) == "table"
    and #r[1].result == 3
    and r[1].result[1] == false
    and json.is_null(r[1].result[2])
    and r[1].result[3] == 0,
    "false, null and zero keep distinct exact slots in one positional response")

  -- Logging distinguishes found=false from not found (secondary pin: the two
  -- branches use different format strings captured by the core.log fake).
  log_records = {}
  deliver(client, "workspace/configuration", 39,
    { items = json.decode('[{"section":"perl.enabled"}]') })
  deliver(client, "workspace/configuration", 40,
    { items = json.decode('[{"section":"perl.absent"}]') })
  local saw_found_false_log, saw_not_found_log = false, false
  for _, record in ipairs(log_records) do
    if record:find("Asking for '.*' config but not set", 1, false) then
      saw_not_found_log = true
    end
    if record:find("Asking for '", 1, true)
      and not record:find("but not set", 1, true) then
      saw_found_false_log = true
    end
  end
  ok(saw_found_false_log, "found=false-valued section logs the found branch")
  ok(saw_not_found_log, "missing section logs the not-set branch distinctly")
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

-- ---------------------------------------------------------------------------
-- Trust-boundary cases (#10653): workspace/project configuration is data,
-- never executable code. The scenario fixture owns io.open/dofile/mtimes/
-- os.time for the duration of one case; every dofile target is recorded.
-- ---------------------------------------------------------------------------

---Bind the configuration seams for one deterministic scenario and run fn.
---scenario.files maps absolute candidate paths to fixture answers (string
---content, true for existence-only, nil/absent for missing);
---scenario.mtimes maps paths to deterministic stat stamps;
---scenario.userdir_settings is returned for USERDIR .lite_lsp.lua probes.
---Returns fn's result; restores every global even on error. The module must
---be loaded BEFORE this binding so its own dofile() load stays real.
local function with_config_scenario(scenario, fn)
  local original_open, original_dofile = io.open, dofile
  local raw_dofile = original_dofile
  local original_time = os.time
  local original_get_file_info = system.get_file_info
  local epoch = scenario.epoch or 1000
  local lua_loads = {}
  os.time = function() return epoch end
  system.get_file_info = function(path)
    if scenario.mtimes and scenario.mtimes[path] then
      -- Lite XL exposes the modification timestamp as `modified` (#10653
      -- review); the fixture mirrors the production field name so
      -- same-byte-length content edits still invalidate the stamp.
      return { modified = scenario.mtimes[path], size = 128 }
    end
    return nil
  end
  io.open = function(path, _)
    local answer = scenario.files[path]
    if answer == nil then
      return nil
    elseif answer == true then
      return { read = function() return "" end, close = function() end }
    end
    return {
      read = function() return answer end,
      close = function() end,
    }
  end
  dofile = function(path)
    lua_loads[#lua_loads + 1] = path
    if path:find("%.lite_lsp%.lua$") then
      if scenario.userdir_settings then
        return scenario.userdir_settings
      end
      -- Any other execution would be a trust-boundary breach: answer with a
      -- loud marker payload instead of failing so assertions can report it.
      return { hostile_lua_executed = path }
    end
    return raw_dofile(path)
  end
  local ok_run, result = pcall(fn, lua_loads)
  io.open, dofile = original_open, raw_dofile
  os.time = original_time
  system.get_file_info = original_get_file_info
  if not ok_run then error(result, 0) end
  return result
end

---Count recorded dofile targets that executed a project-local Lua config.
---Module-source loads share the recorder but never match the suffix.
local function project_lua_loads(lua_loads)
  local hits = {}
  for _, path in ipairs(lua_loads) do
    if path:find("%.lite_lsp%.lua$") and not path:find("^%./") then
      hits[#hits + 1] = path
    end
  end
  return hits
end

do
  -- hostile_workspace_lua_never_executes: a repository .lite_lsp.lua at a
  -- server-supplied scope is ignored - no execution, no merged backdoor.
  -- Fixture keys use the exact platform path the #11165 URI authority
  -- produces for each scope (Windows drive form with backslash separators).
  local lsp = fresh_module_load()
  local json = require "plugins.lsp.json"
  local client = start_captured_client(lsp, { perl = { alpha = "A" } })
  with_config_scenario({
    files = { ["C:\\proj/.lite_lsp.lua"] = true },
  }, function(lua_loads)
    local r = deliver(client, "workspace/configuration", 51,
      { items = json.decode(
        '[{"section":"perl.backdoor","scopeUri":"file:///C:/proj"}]') })
    ok(r[1] and r[1].error == nil
      and type(r[1].result) == "table" and #r[1].result == 1
      and json.is_null(r[1].result[1]),
      "hostile workspace .lite_lsp.lua answers an empty section, not its payload")
    ok(#project_lua_loads(lua_loads) == 0,
      "hostile workspace .lite_lsp.lua is never executed")
  end)
end

do
  -- project_json_read_as_data: the accepted project-data format merges as
  -- plain data through a scoped request; no Lua authority is involved.
  local lsp = fresh_module_load()
  local json = require "plugins.lsp.json"
  local client = start_captured_client(lsp, { perl = { alpha = "A" } })
  with_config_scenario({
    files = {
      ["C:\\proj/.lite_lsp.json"] = '{"perl":{"layer":"project"}}',
    },
    mtimes = { ["C:\\proj"] = 5, ["C:\\proj/.lite_lsp.json"] = 5 },
  }, function(lua_loads)
    local r = deliver(client, "workspace/configuration", 52,
      { items = json.decode(
        '[{"section":"perl.layer","scopeUri":"file:///C:/proj"}]') })
    ok(r[1] and r[1].error == nil and type(r[1].result) == "table"
      and #r[1].result == 1 and r[1].result[1] == "project",
      "project .lite_lsp.json is read as data through the scoped request")
    ok(#lua_loads == 0,
      "data-only project configuration executes no Lua at any position")
    -- The unscoped default lookup never scans the workspace position.
    local d = deliver(client, "workspace/configuration", 53,
      { items = json.decode('[{"section":"perl.layer"}]') })
    ok(d[1] and type(d[1].result) == "table" and #d[1].result == 1
      and json.is_null(d[1].result[1]),
      "unscoped defaults stay section-scoped to the user/server roots")
  end)
end

do
  -- malformed_project_json_fails_safe_without_lua_fallback: broken project
  -- JSON becomes one bounded error and an empty value - it must not crash
  -- the lookup or fall through to executing a coexisting project Lua file.
  local lsp = fresh_module_load()
  local json = require "plugins.lsp.json"
  local client = start_captured_client(lsp, { perl = { alpha = "A" } })
  log_records = {}
  with_config_scenario({
    files = {
      ["C:\\proj/.lite_lsp.json"] = "{oops",
      ["C:\\proj/.lite_lsp.lua"] = true,
    },
    mtimes = { ["C:\\proj"] = 5, ["C:\\proj/.lite_lsp.json"] = 5 },
  }, function(lua_loads)
    local r = deliver(client, "workspace/configuration", 54,
      { items = json.decode(
        '[{"section":"perl.alpha","scopeUri":"file:///C:/proj"},' ..
        '{"section":"perl.absent","scopeUri":"file:///C:/proj"}]') })
    ok(r and not r.listener_error,
      "malformed project JSON does not crash the configuration listener")
    ok(r[1] and r[1].error == nil and type(r[1].result) == "table"
      and #r[1].result == 2 and r[1].result[1] == "A"
      and json.is_null(r[1].result[2]),
      "malformed project JSON answers deterministic server-default values")
    ok(#project_lua_loads(lua_loads) == 0,
      "malformed project JSON never falls through to executing project Lua")
    local saw_typed_error = false
    for _, record in ipairs(log_records) do
      if record:find("ignoring malformed project configuration", 1, true) then
        saw_typed_error = true
      end
    end
    ok(saw_typed_error,
      "the malformed payload reports one bounded typed configuration error")
  end)
end

do
  -- userdir_lua_retained: the user-owned executable root keeps its
  -- historical authority (contract pin that passes pristine too).
  local lsp = fresh_module_load()
  local json = require "plugins.lsp.json"
  local client = start_captured_client(lsp, { perl = { alpha = "A" } })
  with_config_scenario({
    files = { ["./.lite_lsp.lua"] = true },
    userdir_settings = { perl = { usertoken = "U" } },
    mtimes = { ["."] = 5, ["./.lite_lsp.lua"] = 5 },
  }, function(lua_loads)
    local r = deliver(client, "workspace/configuration", 55,
      { items = json.decode('[{"section":"perl.usertoken"}]') })
    ok(r[1] and r[1].error == nil and type(r[1].result) == "table"
      and #r[1].result == 1 and r[1].result[1] == "U",
      "USERDIR .lite_lsp.lua remains the user-owned executable config root")
    local expected_userdir_loads = 0
    for _, path in ipairs(lua_loads) do
      if path == "./.lite_lsp.lua" then
        expected_userdir_loads = expected_userdir_loads + 1
      end
    end
    ok(expected_userdir_loads == 1,
      "the USERDIR executable config loads exactly once per uncached lookup")
  end)
end

do
  -- repeated_requests_and_ttl_expiry_stay_safe: cache hits, TTL expiry and
  -- uncached reloads never widen the trust boundary back to project Lua.
  local lsp = fresh_module_load()
  local json = require "plugins.lsp.json"
  local client = start_captured_client(lsp, { perl = { alpha = "A" } })
  local epoch_holder = { now = 1000 }
  with_config_scenario({
    files = { ["C:\\proj/.lite_lsp.lua"] = true },
    epoch = 1000,
  }, function(lua_loads)
    local scoped_items =
      '[{"section":"perl.backdoor","scopeUri":"file:///C:/proj"}]'
    deliver(client, "workspace/configuration", 56,
      { items = json.decode(scoped_items) })
    deliver(client, "workspace/configuration", 57,
      { items = json.decode(scoped_items) })
    ok(#project_lua_loads(lua_loads) == 0,
      "repeated requests inside the cache window execute no project Lua")
    os.time = function() return 1010 end
    deliver(client, "workspace/configuration", 58,
      { items = json.decode(scoped_items) })
    os.time = function() return epoch_holder.now end
    ok(#project_lua_loads(lua_loads) == 0,
      "a post-TTL uncached reload still executes no project Lua")
  end)
end

do
  -- accepted_config_change_invalidates_cache_inside_ttl: a changed accepted
  -- project-data file (new mtime, same fake clock tick) refreshes answers.
  local lsp = fresh_module_load()
  local json = require "plugins.lsp.json"
  local client = start_captured_client(lsp, {})
  local scenario = {
    files = {
      ["C:\\proj/.lite_lsp.json"] = '{"perl":{"layer":"v1"}}',
    },
    mtimes = { ["C:\\proj"] = 5, ["C:\\proj/.lite_lsp.json"] = 5 },
    epoch = 1000,
  }
  with_config_scenario(scenario, function()
    local scoped_items =
      '[{"section":"perl.layer","scopeUri":"file:///C:/proj"}]'
    local r1 = deliver(client, "workspace/configuration", 59,
      { items = json.decode(scoped_items) })
    ok(r1[1] and type(r1[1].result) == "table" and r1[1].result[1] == "v1",
      "the first accepted project configuration answer is v1")
    scenario.files["C:\\proj/.lite_lsp.json"] = '{"perl":{"layer":"v2"}}'
    scenario.mtimes["C:\\proj/.lite_lsp.json"] = 6
    local r2 = deliver(client, "workspace/configuration", 60,
      { items = json.decode(scoped_items) })
    ok(r2[1] and type(r2[1].result) == "table" and r2[1].result[1] == "v2",
      "a changed accepted configuration invalidates the cached settings "
        .. "inside the freshness window")
  end)
end

do
  -- restart_relookup_stays_safe: a fresh module/session re-lookup against
  -- the same hostile workspace stays data-only (restart cannot revive the
  -- ignored project Lua).
  local json = require "plugins.lsp.json"
  local first = fresh_module_load()
  local first_client = start_captured_client(first, {})
  with_config_scenario({
    files = { ["C:\\proj/.lite_lsp.lua"] = true },
  }, function(first_loads)
    deliver(first_client, "workspace/configuration", 61,
      { items = json.decode(
        '[{"section":"perl.backdoor","scopeUri":"file:///C:/proj"}]') })
    local second = fresh_module_load()
    local second_client = start_captured_client(second, {})
    local second_loads
    with_config_scenario({
      files = { ["C:\\proj/.lite_lsp.lua"] = true },
    }, function(lua_loads)
      deliver(second_client, "workspace/configuration", 62,
        { items = json.decode(
          '[{"section":"perl.backdoor","scopeUri":"file:///C:/proj"}]') })
      second_loads = lua_loads
    end)
    ok(#project_lua_loads(second_loads or {}) == 0,
      "a restarted session re-lookup still executes no project Lua")
    ok(#project_lua_loads(first_loads) == 0,
      "the pre-restart session executed no project Lua either")
  end)
end

do
  -- hostile_scopeuri_escape_contained: an out-of-root scope cannot pull
  -- arbitrary executable configuration into the lookup.
  local lsp = fresh_module_load()
  local json = require "plugins.lsp.json"
  local client = start_captured_client(lsp, {})
  with_config_scenario({
    files = { ["C:\\Windows/System32/.lite_lsp.lua"] = true },
  }, function(lua_loads)
    local r = deliver(client, "workspace/configuration", 63,
      { items = json.decode(
        '[{"section":"perl.backdoor",' ..
        '"scopeUri":"file:///C:/Windows/System32"}]') })
    ok(r[1] and r[1].error == nil and type(r[1].result) == "table"
      and #r[1].result == 1 and json.is_null(r[1].result[1]),
      "an out-of-root hostile scope answers empty defaults")
    ok(#project_lua_loads(lua_loads) == 0,
      "a hostile scopeUri cannot cause arbitrary config-path execution")
  end)
end

do
  -- non_object_project_roots_rejected: valid-JSON but non-object roots
  -- (array, null) are typed configuration errors — they must neither merge
  -- as settings nor replace accumulated user/server values (#10653 review).
  local lsp = fresh_module_load()
  local json = require "plugins.lsp.json"
  local client = start_captured_client(lsp, { perl = { alpha = "A" } })
  log_records = {}
  with_config_scenario({
    files = {
      ["C:\\proj/.lite_lsp.json"] = "[]",
    },
    mtimes = { ["C:\\proj"] = 5, ["C:\\proj/.lite_lsp.json"] = 5 },
  }, function()
    local r = deliver(client, "workspace/configuration", 64,
      { items = json.decode(
        '[{"section":"perl.alpha","scopeUri":"file:///C:/proj"},' ..
        '{"section":"perl.absent","scopeUri":"file:///C:/proj"}]') })
    ok(r and not r.listener_error,
      "an array-root project file does not crash the lookup")
    ok(r[1] and r[1].error == nil and type(r[1].result) == "table"
      and #r[1].result == 2 and r[1].result[1] == "A"
      and json.is_null(r[1].result[2]),
      "an array-root project file cannot wipe accumulated server settings")
    local saw_typed_error = false
    for _, record in ipairs(log_records) do
      if record:find("ignoring malformed project configuration", 1, true) then
        saw_typed_error = true
      end
    end
    ok(saw_typed_error,
      "an array-root project file reports the bounded not-an-object error")
  end)

  -- null-root variant through a fresh module/cache.
  local lsp2 = fresh_module_load()
  local json2 = require "plugins.lsp.json"
  local client2 = start_captured_client(lsp2, { perl = { alpha = "A" } })
  log_records = {}
  with_config_scenario({
    files = {
      ["C:\\proj/.lite_lsp.json"] = "null",
    },
    mtimes = { ["C:\\proj"] = 5, ["C:\\proj/.lite_lsp.json"] = 5 },
  }, function()
    local r = deliver(client2, "workspace/configuration", 65,
      { items = json.decode(
        '[{"section":"perl.alpha","scopeUri":"file:///C:/proj"}]') })
    ok(r and not r.listener_error
      and r[1] and type(r[1].result) == "table" and r[1].result[1] == "A",
      "a null-root project file keeps accumulated server settings")
  end)
end

print(string.format("%d passed, %d failed", passed, failed))
if failed > 0 then os.exit(1) end

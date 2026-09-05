-- Deterministic focused tests for truthful window/showDocument outcomes in
-- clients/lite-xl/upstream/init.lua (#10873).
--
-- Run:
--   lua clients/lite-xl/tests/init_show_document_outcome_test.lua [path-to-init-module]
-- Default module path is ../upstream/init.lua relative to this file.
--
-- Proof shape: the exact staged init.lua module is loaded under minimal Lite
-- XL runtime fakes whose plugins.lsp.server constructor captures the real
-- registered listeners; the production window/showDocument listener is then
-- dispatched with a recording push_response and a fake asynchronous
-- MessageBox. Tests assert the response contract from #10873:
--   zero responses before the external user action;
--   accept + successful launch answers exactly one success=true;
--   decline / prompt cancel answers exactly one success=false;
--   internal valid file opens and answers one success=true;
--   internal unconvertible URI and selection-conversion failure answer one
--   explicit success=false with typed dispositions;
--   a prompt owned by a replaced/stopped server instance answers nothing;
--   request ids (numeric and string) are preserved verbatim.
--
-- Red-first baseline: run this suite against CURRENT MAIN before the #10873
-- patch (listener inlines preemptive {success=true} after queueing the async
-- MessageBox): before_decision_zero_responses, decline/cancel,
-- selection-failure and replacement-inertness cases MUST fail there because
-- one success=true already exists before any user outcome.
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
-- Lite XL runtime fakes
-- ---------------------------------------------------------------------------

local log_records = {}
local opened_docviews = {}
local selection_calls = {}
---One shared root-view instance: init.lua reaches the editor's open_doc
---through the core fake's root_view field and the core.rootview module.
---Returns a docview carrying a minimal .doc so selection conversion and
---set_selection behavior are provable end to end.
local core_root_view = {}
function core_root_view.open_doc(_, doc)
  local docview = {
    filename = doc.filename,
    doc = {
      lines = { "hello world" },
      set_selection = function(_, line1, col1, line2, col2)
        selection_calls[#selection_calls + 1] =
          { line1 = line1, col1 = col1, line2 = line2, col2 = col2 }
      end,
    },
  }
  opened_docviews[#opened_docviews + 1] = docview
  return docview
end
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
    root_view = core_root_view,
    ---Minimal core.open_doc: returns a plain doc handle for the reveal path.
    open_doc = function(filename)
      return { filename = filename }
    end,
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
    home_expand = function(path) return path end,
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
Doc.set_selection = function(self) end
package.preload["core.doc"] = function() return Doc end

local DocView = { __index = DocView }
function DocView:extends() return false end
package.preload["core.docview"] = function() return DocView end

package.preload["core.statusview"] = function()
  return { Item = { RIGHT = 2 } }
end

local RootView = { __index = RootView }
RootView.open_doc = core_root_view.open_doc
package.preload["core.rootview"] = function() return RootView end

package.preload["plugins.lsp.json"] = function()
  return dofile(here .. "/../upstream/json.lua")
end

local process_calls = {}
local fail_next = false
package.preload["process"] = function()
  return {
    start = function(argv)
      process_calls[#process_calls + 1] = { argv = argv }
      if fail_next then
        fail_next = false
        return nil
      end
      return 4242 + #process_calls
    end
  }
end

package.preload["plugins.lsp.util"] = function()
  return dofile(here .. "/../upstream/util.lua")
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
  }
end

---Callable fake Server module capturing listener registrations.
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
      -- Return true so the production start_server keeps this client
      -- registered in lsp.servers_running (#11165 refuses unconvertible
      -- workspaces and unregisters them).
      self.initialize_args = { ... }
      return true
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

---Fake asynchronous MessageBox: records prompts, never answers by itself.
local prompts = {}
package.preload["libraries.widget.messagebox"] = function()
  return {
    BUTTONS_YES_NO = 1,
    info = function(title, message, callback, buttons)
      prompts[#prompts + 1] =
        { title = title, message = message, callback = callback, buttons = buttons }
    end,
  }
end

package.preload["plugins.lsp.helpdoc"] = function()
  return function(title) return { title = title, set_text = function() end } end
end

utf8extra = { len = function(s) return utf8.len(s) or #s end }

PLATFORM = "Windows"
USERDIR = "."
SCALE = 1
renderer = { font = { load = function() return {} end } }

local raise_calls = 0
system = {
  exec = function() error("shell invoked", 0) end,
  raise_window = function() raise_calls = raise_calls + 1 end,
  get_file_info = function() return { size = 100 } end,
}

-- ---------------------------------------------------------------------------
-- Module loading and dispatch helpers
-- ---------------------------------------------------------------------------

local function fresh_module_load()
  log_records = {}
  constructed_clients = {}
  prompts = {}
  opened_docviews = {}
  selection_calls = {}
  process_calls = {}
  fail_next = false
  raise_calls = 0
  return dofile(init_module_path)
end

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

---Dispatch one showDocument request through the real registered listener
---with a recording push_response. Returns the captured responses.
local function deliver(client, id, params)
  local responses = {}
  function client:push_response(m, i, result, err)
    responses[#responses + 1] = { method = m, id = i, result = result, error = err }
  end
  local listener = client.request_listeners["window/showDocument"]
  assert(listener, "production window/showDocument listener must be attached")
  local ok_run, err = pcall(
    listener, client, { method = "window/showDocument", id = id, params = params })
  if not ok_run then
    print("LISTENER ERROR [" .. tostring(id) .. "]: " .. tostring(err))
    return { listener_error = tostring(err) }
  end
  return responses
end

-- ---------------------------------------------------------------------------
-- Cases
-- ---------------------------------------------------------------------------

do
  local lsp = fresh_module_load()
  local client = start_captured_client(lsp, {})

  ok(type(client.request_listeners["window/showDocument"]) == "function",
    "production window/showDocument listener is attached by start_server")

  -- External: nothing may answer before the user decision exists.
  local r = deliver(client, 101,
    { uri = "https://example.test/doc?x=1", external = true })
  ok(#r == 0, "external request answers nothing before the user action")
  ok(prompts[1] ~= nil and prompts[1].callback ~= nil,
    "the confirmation prompt is shown")
  ok(#process_calls == 0, "nothing launches before acceptance")

  -- Accept + observable launch success: exactly one truthful success=true.
  prompts[#prompts].callback(nil, 1)
  if #r ~= 1 or not (r[1].result and r[1].result.success) then
    print("DEBUG accept state: responses=" .. #r ..
      " prompts=" .. #prompts .. " launches=" .. #process_calls)
    for i, resp in ipairs(r) do
      print("  response[" .. i .. "] id=" .. tostring(resp.id) ..
        " success=" .. tostring(resp.result and resp.result.success))
    end
  end
  ok(#r == 1 and r[1].result ~= nil and r[1].result.success == true
    and r[1].error == nil,
    "accept plus successful launch answers exactly one success=true")
  ok(r[1].id == 101, "numeric request id preserved on the response")
  ok(#process_calls == 1, "accepted target launched once")

  -- Decline: exactly one success=false, no launch.
  r = deliver(client, 102, { uri = "https://example.test/x", external = true })
  prompts[#prompts].callback(nil, 2)
  ok(#r == 1 and r[1].result ~= nil and r[1].result.success == false,
    "user decline answers exactly one success=false")
  ok(#process_calls == 1, "declined target never launches")

  -- Prompt closed/cancelled (button other than Yes): one success=false.
  r = deliver(client, "string-id-103",
    { uri = "https://example.test/y", external = true })
  prompts[#prompts].callback(nil, 0)
  ok(#r == 1 and r[1].id == "string-id-103"
    and r[1].result ~= nil and r[1].result.success == false,
    "cancelled prompt answers one success=false preserving string ids")

  -- Internal valid file: one success=true, focus raised only after reveal.
  raise_calls = 0
  r = deliver(client, 104, {
    uri = "file:///C:/proj/main%20file.pl",
    external = false,
    takeFocus = true,
  })
  ok(#r == 1 and r[1].result ~= nil and r[1].result.success == true,
    "internal valid file answers one success=true")
  ok(#opened_docviews == 1, "internal open went through root_view:open_doc")
  ok(raise_calls == 1, "takeFocus raise applied after successful reveal")

  -- Internal valid selection: set_selection runs on the opened document with
  -- the converted coordinates (utf-16 columns pass through for ASCII text).
  selection_calls = {}
  r = deliver(client, 109, {
    uri = "file:///C:/proj/selected.pl",
    external = false,
    selection = {
      start = { line = 0, character = 1 },
      ["end"] = { line = 0, character = 5 },
    },
  })
  ok(#r == 1 and r[1].result ~= nil and r[1].result.success == true,
    "internal valid selection answers one success=true")
  ok(#selection_calls == 1 and selection_calls[1].line1 == 1
    and selection_calls[1].col1 == 2 and selection_calls[1].line2 == 1
    and selection_calls[1].col2 == 6,
    "valid selection applies one converted set_selection on the opened doc")

  -- Internal unconvertible URI: one explicit success=false.
  r = deliver(client, 105, { uri = "https://example.test/nope", external = false })
  ok(#r == 1 and r[1].result ~= nil and r[1].result.success == false,
    "internal non-file URI answers one explicit success=false")
  ok(#opened_docviews == 2,
    "refused internal URI never opened a document")

  -- Internal selection conversion failure: explicit success=false, no crash.
  r = deliver(client, 106, {
    uri = "file:///C:/proj/other.pl",
    external = false,
    selection = {},
  })
  ok(#r == 1 and r[1].result ~= nil and r[1].result.success == false,
    "selection conversion failure answers one explicit success=false")
  ok(#log_records > 0, "selection failure carries an explicit disposition log")

  -- Server replacement while a prompt is open: the stale callback is inert.
  r = deliver(client, 107, { uri = "https://example.test/z", external = true })
  lsp.servers_running["perllsp"] = nil
  prompts[#prompts].callback(nil, 1)
  ok(#r == 0, "stale prompt of a replaced server answers nothing")
  ok(#process_calls == 1, "stale prompt never launches")
  lsp.servers_running["perllsp"] = client

  -- Double terminal delivery stays one response, and a repeated Yes cannot
  -- start a second launch (#10873 review): the whole sequence settles once.
  local launches_before = #process_calls
  r = deliver(client, 108, { uri = "https://example.test/w", external = true })
  prompts[#prompts].callback(nil, 1)
  local launches_after_first = #process_calls
  prompts[#prompts].callback(nil, 1)
  ok(#r == 1, "a double host callback still produces exactly one response")
  ok(launches_after_first == launches_before + 1
    and #process_calls == launches_after_first,
    "a repeated Yes answer never starts a second launch")
end

print(string.format("%d passed, %d failed", passed, failed))
if failed > 0 then os.exit(1) end

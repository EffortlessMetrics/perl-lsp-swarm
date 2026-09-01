-- Deterministic focused tests for collision-free completion candidate
-- identity in clients/lite-xl/upstream/init.lua (#11189).
--
-- Run:
--   lua clients/lite-xl/tests/init_completion_collision_test.lua [path-to-init-module]
-- Default module path is ../upstream/init.lua relative to this file.
--
-- Proof shape: held responses are drained manually against the exact staged
-- module under minimal Lite XL runtime fakes. Tests drive the real public
-- path - request_completion populates the box through the autocomplete fake,
-- then each item's onselect callback runs exactly as the editor would invoke
-- it - and assert the #11189 contract:
--
--   two valid items sharing one display label both stay selectable (the map
--   key carries a deterministic "#N" suffix for later occurrences only);
--   the first occurrence keeps the bare label as its key, so unsuffixed
--   rows are byte-identical to the legacy projection;
--   every row keeps its own exact original CompletionItem and resolve state;
--   the internal disambiguator never reaches the document: selecting a
--   suffix-carrying row without a textEdit inserts the exact pre-suffix
--   insert target, and unsuffixed rows keep the legacy plugin fallback;
--   textEdit-bearing duplicates still apply through the guarded edit path.
--
-- Red-first baseline: run this suite against CURRENT MAIN before this change
-- (origin/main clients/lite-xl/upstream/init.lua at 79c7da6148):
--
--   lua clients/lite-xl/tests/init_completion_collision_test.lua <main-init.lua>
--
-- There the duplicate-identity cases MUST fail (main stores rows under the
-- bare label, so the later same-label item silently overwrites the earlier
-- one) and the exact-identity cases MUST fail (the surviving row is the
-- last-written one regardless of which candidate is selected).
--
-- Single-behavior mutation falsifiers of the PATCHED module (each verified
-- caught):
--   1. drop the "#N" suffix minting (restore bare-label keying) -> the
--      duplicate-presence cases fail;
--   2. make the select-time fallback insert item.text for suffixed rows ->
--      the no-leak case fails (the "#2" byte string lands in the buffer);
--   3. apply the fallback for unsuffixed rows too -> the legacy-fallback
--      case fails (the apply-once flag flips without the plugin fallback).

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
local autocomplete_calls = {}

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
    command_view = { enter = function() end },
    status_view = { separator2 = 2, add_item = function() end },
    active_view = nil,
    root_view = {
      get_active_node_default = function()
        return { add_view = function() end }
      end,
      open_doc = function(doc) return { doc = doc } end,
    },
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
    fuzzy_match = function(_, _) return {} end,
    normalize_path = function(path) return path end,
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
    can_complete = function() return true end,
    complete = function(symbols, done)
      autocomplete_calls[#autocomplete_calls + 1] = symbols
    end,
    close = function() end,
  }
end

---Fake Doc class with controllable selection and edit recording.
local Doc = { __index = Doc }
Doc.raw_insert = function(self, line, col, text) end
Doc.raw_remove = function(self, line1, col1, line2, col2) end
Doc.get_selection = function(self)
  local s = self._selection
  return s.line, s.col, s.line, s.col
end
Doc.get_char = function() return "" end
Doc.set_selection = function(self, line1, col1)
  self.selections[#self.selections + 1] = { line1 = line1, col1 = col1 }
end
Doc.remove = function(self, line1, col1, line2, col2)
  self.edits[#self.edits + 1] =
    { op = "remove", line1 = line1, col1 = col1, line2 = line2, col2 = col2 }
end
Doc.insert = function(self, line1, col1, text)
  self.edits[#self.edits + 1] =
    { op = "insert", line1 = line1, col1 = col1, text = text }
end
Doc.text_input = function(self, text)
  -- The real plugin fallback calls doc:text_input with the key text; the
  -- #11189 select-time fallback calls it with the pre-suffix insert target.
  self.edits[#self.edits + 1] = { op = "text_input", text = text }
end
Doc.position_offset = function(self, line, col) return line, col end
Doc.get_text = function() return "" end
Doc.move_to_cursor = function() end
Doc.get_indent_info = function() return "soft", 2, true end
Doc.highlighter = { each_token = function() return function() return nil end end }
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
  return dofile(here .. "/../upstream/util.lua")
end

package.preload["plugins.lsp.listbox"] = function()
  return {
    hide = function() end,
    show_text = function(text, pos) end,
    show_signatures = function(result) end,
  }
end

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

package.preload["plugins.lsp.server"] = function()
  return {
    text_document_sync_kind = { None = 0, Full = 1, Incremental = 2 },
    position_encoding_kind = { UTF8 = "utf-8", UTF16 = "utf-16", UTF32 = "utf-32" },
    message_type = { Error = 1, Warning = 2, Info = 3, Log = 4, Debug = 5 },
    completion_trigger_Kind = { Invoked = 1, TriggerCharacter = 2 },
    insert_text_format = { PlainText = 1, Snippet = 2 },
    symbol_kind = {},
    get_symbol_kind = function() return "File" end,
  }
end

package.preload["plugins.lsp.timer"] = function()
  return function()
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
  local SymbolResults = {}
  setmetatable(SymbolResults, {
    __call = function(class, query)
      local rs = {
        query = query,
        results_added = {},
        stopped = false,
        list = { resize_to_parent = function() end },
      }
      function rs:add_result(result) self.results_added[#self.results_added + 1] = result end
      function rs:stop_searching() self.stopped = true end
      return rs
    end,
  })
  return SymbolResults
end

package.preload["libraries.widget.messagebox"] = function()
  return { BUTTONS_YES_NO = 1, info = function() end }
end

package.preload["plugins.lsp.helpdoc"] = function()
  return function(title)
    return { title = title, set_text = function() end }
  end
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

local core_mod = require "core"

snippets = { execute = function() end }

-- ---------------------------------------------------------------------------
-- Module loading with pristine wrapper bases
-- ---------------------------------------------------------------------------

local base_raw_insert = Doc.raw_insert
local base_raw_remove = Doc.raw_remove

local function fresh_module_load(active_dv)
  Doc.raw_insert = base_raw_insert
  Doc.raw_remove = base_raw_remove
  log_records = {}
  autocomplete_calls = {}
  local lsp = dofile(init_module_path)
  core_mod.active_view = active_dv or nil
  return lsp
end

-- ---------------------------------------------------------------------------
-- Fixture helpers
-- ---------------------------------------------------------------------------

local function make_doc(filename, lines, caret_line, caret_col)
  return setmetatable({
    filename = filename,
    abs_filename = filename,
    lines = lines,
    clean_change_id = 1,
    lsp_open = false,
    _selection = { line = caret_line or 1, col = caret_col or 1 },
    edits = {},
    selections = {},
  }, { __index = Doc })
end

local function make_server(name, capabilities)
  local server = {
    name = name,
    file_patterns = { "%.pl$" },
    initialized = true,
    verbose = false,
    incremental_changes = false,
    capabilities = capabilities,
    outbound = {},
  }
  function server:get_language_id() return "perl" end
  function server:exit() self.exited = (self.exited or 0) + 1 end
  function server:get_completion_item_kind() return 1 end
  function server:log(fmt) log_records[#log_records + 1] = tostring(fmt) end
  local function record(kind, method, entry)
    entry.method = method
    entry.kind = kind
    server.outbound[#server.outbound + 1] = entry
  end
  function server:push_notification(method, entry) record("notification", method, entry) end
  function server:push_request(method, entry)
    record("request", method, entry)
    return "queued"
  end
  function server:push_raw(method, entry) record("raw", method, entry) end
  function server:push_response() end
  function server:add_message_listener() end
  function server:add_event_listener() end
  return server
end

local function register(lsp, name, server)
  lsp.servers[name] = {
    name = name,
    file_patterns = server.file_patterns,
    command = { "perllsp" },
    language = "perl",
  }
  lsp.servers_running[name] = server
end

local PLAIN_COMPLETION_CAPS = {
  textDocumentSync = { openClose = true, change = 2, save = { includeText = false } },
  positionEncoding = "utf-16",
  completionProvider = { triggerCharacters = {} },
}

local function open_admitted(lsp, doc, server)
  lsp.open_document(doc)
  for index = #server.outbound, 1, -1 do
    local entry = server.outbound[index]
    if entry.method == "textDocument/didOpen" then
      table.remove(server.outbound, index)
    end
  end
  doc.lsp_open = true
end

---Run one completion round to population and return the populated items map.
local function open_completion(lsp, server, doc, result_items)
  open_admitted(lsp, doc, server)
  lsp.request_completion(doc, 1, 6, true)
  local held
  for index, entry in ipairs(server.outbound) do
    if entry.method == "textDocument/completion" then held = entry end
  end
  if not held then
    ok(false, "open_completion: completion request was queued")
    return {}
  end
  for index, entry in ipairs(server.outbound) do
    if entry == held then table.remove(server.outbound, index) end
  end
  held.callback(server, { result = { items = result_items } })
  local symbols = autocomplete_calls[#autocomplete_calls]
  if not symbols or not symbols.items then
    ok(false, "open_completion: completion populated the autocomplete box")
    return {}
  end
  return symbols.items
end

local function activate(dv)
  core_mod.active_view = dv
end

-- ===========================================================================
-- Case H: a plain completion may display a short label while requesting a
-- different insertText. The fallback must preserve the protocol insertion
-- target rather than silently inserting the presentation label.
-- ===========================================================================
do
  local lsp = fresh_module_load()
  local server = make_server("perllsp", PLAIN_COMPLETION_CAPS)
  register(lsp, "perllsp", server)
  local doc = make_doc("C:/proj/col7.pl", { "my $fn = 1;\n" }, 1, 6)

  local items = open_completion(lsp, server, doc, {
    { label = "fn", insertText = "function_call", kind = 3 },
  })

  local row = items["fn"]
  ok(row ~= nil, "caseH: the display label remains the menu key")
  ok(
    row.data.insert_text == "function_call",
    "caseH: the distinct protocol insertText is preserved for fallback"
  )
end

-- ===========================================================================
-- Case A: two valid items share one label. Both rows must exist under
-- distinct internal keys, the first under the bare label, and each row must
-- carry its own exact original CompletionItem. Red on main: the second item
-- silently overwrites the first, so exactly one row survives.
-- ===========================================================================
do
  local lsp = fresh_module_load()
  local server = make_server("perllsp", PLAIN_COMPLETION_CAPS)
  register(lsp, "perllsp", server)
  local doc = make_doc("C:/proj/col1.pl", { "my $foo = 1;\n" }, 1, 6)

  local item_one = { label = "foo", kind = 3, detail = "first scalar" }
  local item_two = { label = "foo", kind = 6, detail = "second variable" }
  local items = open_completion(lsp, server, doc, { item_one, item_two })

  ok(items["foo"] ~= nil, "caseA: the first occurrence keeps the bare label key")
  ok(items["foo#2"] ~= nil, "caseA: the second occurrence gains the #2 key")
  ok(items["foo#3"] == nil, "caseA: no phantom third row is minted")
  ok(
    items["foo"] and items["foo"].data.completion_item == item_one,
    "caseA: the bare-label row carries the exact first item"
  )
  ok(
    items["foo#2"] and items["foo#2"].data.completion_item == item_two,
    "caseA: the suffixed row carries the exact second item"
  )
end

-- ===========================================================================
-- Case B: three same-label items mint deterministic source-order suffixes
-- (#2, #3) with no overwrite and no reordering of the surviving originals.
-- Red on main: one row survives.
-- ===========================================================================
do
  local lsp = fresh_module_load()
  local server = make_server("perllsp", PLAIN_COMPLETION_CAPS)
  register(lsp, "perllsp", server)
  local doc = make_doc("C:/proj/col2.pl", { "my $dup = 1;\n" }, 1, 6)

  local one = { label = "dup", kind = 3 }
  local two = { label = "dup", kind = 3 }
  local three = { label = "dup", kind = 3 }
  local items = open_completion(lsp, server, doc, { one, two, three })

  local seen = {}
  local rows = 0
  for key, row in pairs(items) do
    rows = rows + 1
    seen[key] = row.data.completion_item
  end
  ok(rows == 3, "caseB: all three same-label candidates stay selectable")
  ok(seen["dup"] == one, "caseB: row dup is the first original")
  ok(seen["dup#2"] == two, "caseB: row dup#2 is the second original")
  ok(seen["dup#3"] == three, "caseB: row dup#3 is the third original")
end

-- ===========================================================================
-- Case C: distinct labels are keyed exactly as before - the legacy unsuffixed
-- projection shape is unchanged, including the stored insert target.
-- ===========================================================================
do
  local lsp = fresh_module_load()
  local server = make_server("perllsp", PLAIN_COMPLETION_CAPS)
  register(lsp, "perllsp", server)
  local doc = make_doc("C:/proj/col3.pl", { "my $bar = 1;\n" }, 1, 6)

  local items = open_completion(lsp, server, doc, {
    { label = "bar", kind = 3, detail = "scalar" },
  })

  local row = items["bar"]
  ok(row ~= nil, "caseC: a unique label keeps the bare label key")
  ok(row.data.insert_text == "bar", "caseC: the insert target equals the label")
end

-- ===========================================================================
-- Case D: selecting a suffix-carrying row without a textEdit must insert the
-- exact pre-suffix insert target. The autocomplete plugin's plain fallback
-- inserts the internal key verbatim, so the leaf applies the stored insert
-- target itself and claims the application. Red on a mutation that inserts
-- item.text here ("foo#2" would leak into the buffer).
-- ===========================================================================
do
  local lsp = fresh_module_load()
  local server = make_server("perllsp", PLAIN_COMPLETION_CAPS)
  register(lsp, "perllsp", server)
  local doc = make_doc("C:/proj/col4.pl", { "my $foo = 1;\n" }, 1, 6)

  local items = open_completion(lsp, server, doc, {
    { label = "foo", kind = 3 },
    { label = "foo", kind = 6 },
  })

  local suffixed = items["foo#2"]
  ok(suffixed ~= nil, "caseD: the duplicate row exists")
  -- Model the autocomplete plugin's add() transform: menu rows carry their
  -- internal key as text, and onselect receives the transformed row.
  suffixed.text = "foo#2"
  local dv = setmetatable({ doc = doc }, require "core.docview")
  activate(dv)

  local applied = suffixed.onselect(2, suffixed)
  ok(applied == true, "caseD: the suffixed row claims the application")
  ok(
    #doc.edits == 1 and doc.edits[1].op == "text_input" and doc.edits[1].text == "foo",
    "caseD: exactly the pre-suffix label reaches the document"
  )
end

-- ===========================================================================
-- Case E: an unsuffixed plain row keeps the legacy plugin fallback - the
-- leaf does not claim the application, so the plugin inserts item.text
-- (identical bytes) exactly as before the identity encoding existed.
-- ===========================================================================
do
  local lsp = fresh_module_load()
  local server = make_server("perllsp", PLAIN_COMPLETION_CAPS)
  register(lsp, "perllsp", server)
  local doc = make_doc("C:/proj/col5.pl", { "my $bar = 1;\n" }, 1, 6)

  local items = open_completion(lsp, server, doc, {
    { label = "bar", kind = 3 },
  })

  local row = items["bar"]
  -- Model the plugin add() transform for the unsuffixed row: text == key.
  row.text = "bar"
  local dv = setmetatable({ doc = doc }, require "core.docview")
  activate(dv)

  local applied = row.onselect(1, row)
  ok(applied == false, "caseE: the unsuffixed row keeps the legacy fallback")
  ok(#doc.edits == 0, "caseE: the leaf performed no document mutation itself")
end

-- ===========================================================================
-- Case F: a duplicate label whose items carry textEdits still applies through
-- the guarded edit path; no plain text_input is issued on top.
-- ===========================================================================
do
  local lsp = fresh_module_load()
  local server = make_server("perllsp", PLAIN_COMPLETION_CAPS)
  register(lsp, "perllsp", server)
  local doc = make_doc("C:/proj/col6.pl", { "fo\n" }, 1, 6)

  local items = open_completion(lsp, server, doc, {
    {
      label = "foo",
      kind = 3,
      textEdit = {
        range = {
          start = { line = 0, character = 0 },
          ["end"] = { line = 0, character = 2 },
        },
        newText = "first_foo",
      },
    },
    {
      label = "foo",
      kind = 3,
      textEdit = {
        range = {
          start = { line = 0, character = 0 },
          ["end"] = { line = 0, character = 2 },
        },
        newText = "second_foo",
      },
    },
  })

  local suffixed = items["foo#2"]
  ok(suffixed ~= nil, "caseF: the textEdit duplicate row exists")
  -- Model the plugin add() transform for the suffixed row.
  suffixed.text = "foo#2"
  local dv = setmetatable({ doc = doc }, require "core.docview")
  activate(dv)

  local applied = suffixed.onselect(2, suffixed)
  ok(applied == true, "caseF: the suffixed row claims the edit application")
  local main_edits = 0
  local input_edits = 0
  for _, edit in ipairs(doc.edits) do
    if edit.op == "insert" then main_edits = main_edits + 1 end
    if edit.op == "text_input" then input_edits = input_edits + 1 end
  end
  ok(
    main_edits >= 1 and input_edits == 0,
    "caseF: the guarded edit path applied and no plain fallback ran"
  )
  local saw_new_text = false
  for _, edit in ipairs(doc.edits) do
    if edit.op == "insert" and edit.text == "second_foo" then saw_new_text = true end
  end
  ok(saw_new_text, "caseF: the exact second item's newText is what landed")
end

-- ===========================================================================
-- Case G: resolve attachment follows the exact row - each same-label row
-- carries its own onhover and its own exact resolve subject item.
-- ===========================================================================
do
  local lsp = fresh_module_load()
  local caps = {
    textDocumentSync = { openClose = true, change = 2, save = { includeText = false } },
    positionEncoding = "utf-16",
    completionProvider = { resolveProvider = true, triggerCharacters = {} },
  }
  local server = make_server("perllsp", caps)
  register(lsp, "perllsp", server)
  local doc = make_doc("C:/proj/col7.pl", { "my $foo = 1;\n" }, 1, 6)

  local one = { label = "foo", kind = 3 }
  local two = { label = "foo", kind = 6, detail = "second" }
  local items = open_completion(lsp, server, doc, { one, two })

  ok(items["foo"].onhover ~= nil, "caseG: the first row keeps resolve hover")
  ok(items["foo#2"].onhover ~= nil, "caseG: the duplicate row keeps resolve hover")

  local dv = setmetatable({ doc = doc }, require "core.docview")
  activate(dv)
  items["foo#2"].onhover(2, items["foo#2"])

  local resolved_item = nil
  for _, entry in ipairs(server.outbound) do
    if entry.method == "completionItem/resolve" then
      resolved_item = entry.params
    end
  end
  ok(
    resolved_item == two,
    "caseG: hovering the duplicate resolves its own exact original item"
  )
end

print(string.format("%d passed, %d failed", passed, failed))
if failed > 0 then os.exit(1) end

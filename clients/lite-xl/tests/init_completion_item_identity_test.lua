-- Deterministic focused tests for collision-free completion candidate
-- identity in clients/lite-xl/upstream/init.lua (#11189), consuming #11108
-- subjects and the #11188 pre-apply resolve state per item.
--
-- Run:
--   lua clients/lite-xl/tests/init_completion_item_identity_test.lua [path-to-init-module]
-- Default module path is ../upstream/init.lua relative to this file.
--
-- Proof shape: held responses are drained manually against the exact staged
-- module under minimal Lite XL runtime fakes. Tests drive the real public
-- path - request_completion populates the box through the autocomplete fake,
-- then each row's onhover/onselect callbacks run exactly as the editor would
-- invoke them - and assert the #11189 contract:
--
--   two same-label items with different textEdits both remain selectable and
--     each applies its own bytes through its own resolve;
--   two same-label items with different kinds/details both remain
--     represented with their own secondary facts;
--   resolve attaches only to the selected duplicate; hover of one duplicate
--     never contaminates the other's description or state;
--   byte-identical duplicate items follow the declared preserve-all policy:
--     every twin stays selectable and applies exactly once;
--   a same-label item from an older response cannot apply after an edit and
--     a newer same-label list;
--   unique-label responses keep the exact plain label key (non-regression),
--   every retained row keeps the original source position and display text,
--     and no inserted/visible protocol bytes are ever mutated by identity.
--
-- Red-first baseline: run this suite against CURRENT MAIN before this change
-- (origin/main clients/lite-xl/upstream/init.lua at 987e27c37):
--
--   lua clients/lite-xl/tests/init_completion_item_identity_test.lua <main-init.lua>
--
-- There the duplicate-pair cases MUST fail (main stores candidates under
-- symbols.items[label], so the later same-label item silently overwrites the
-- earlier one) and the exact-duplicate case MUST fail (one twin is dropped).
--
-- Single-behavior mutation falsifiers of the PATCHED module (each verified
-- caught):
--   1. restore plain label keying symbols.items[label] = ... -> every
--      duplicate-pair and preserve-all case fails;
--   2. break the occurrence-suffix uniqueness loop so repaired keys can
--      collide again -> the triple-same-label determinism case fails;
--   3. drop data.item_index/data.display_label retention -> the identity
--      provenance assertions fail without changing selection behavior.
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
local command_view_prompts = {}
local autocomplete_calls = {}
local snippets_executed = {}

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
    command_view = {
      enter = function(_, prompt) command_view_prompts[#command_view_prompts + 1] = prompt end,
    },
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

-- init.lua (#11172) requires the staged capability manifest at load time;
-- serve the exact staged module the same way harness.new_world does.
package.preload["plugins.lsp.capability_manifest"] = function()
  return dofile(here .. "/../upstream/capability_manifest.lua")
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

snippets = {
  execute = function(spec)
    snippets_executed[#snippets_executed + 1] = spec
  end,
}

-- ---------------------------------------------------------------------------
-- Module loading with pristine wrapper bases
-- ---------------------------------------------------------------------------

local base_raw_insert = Doc.raw_insert
local base_raw_remove = Doc.raw_remove

local function fresh_module_load(active_dv)
  Doc.raw_insert = base_raw_insert
  Doc.raw_remove = base_raw_remove
  log_records = {}
  command_view_prompts = {}
  autocomplete_calls = {}
  snippets_executed = {}
  local lsp = dofile(init_module_path)
  if active_dv then
    core_mod.active_view = active_dv
  else
    core_mod.active_view = nil
  end
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
  -- Kind-distinct projection so kind collisions are observable per row.
  -- Dot-called by the client (server.get_completion_item_kind(kind)).
  function server.get_completion_item_kind(kind)
    return "K" .. tostring(kind)
  end
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

---Play one held request callback with a canned response.
local function play_response(server, method, response)
  for index, entry in ipairs(server.outbound) do
    if entry.kind == "request" and entry.method == method then
      table.remove(server.outbound, index)
      if entry.callback then entry.callback(server, response or {}) end
      return true
    end
  end
  return false
end

local RESOLVE_CAPS = {
  textDocumentSync = { openClose = true, change = 2, save = { includeText = false } },
  positionEncoding = "utf-16",
  completionProvider = { resolveProvider = true, triggerCharacters = {} },
}

local PLAIN_COMPLETION_CAPS = {
  textDocumentSync = { openClose = true, change = 2, save = { includeText = false } },
  positionEncoding = "utf-16",
  completionProvider = { triggerCharacters = {} },
}

---Admit an open document/session and flush the didOpen notification.
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

---One accepted edit batch advancing the session stream.
local function accept_edit(lsp, doc, server, line, col, text)
  doc.lines[line] = (doc.lines[line] or "") .. text
  doc:raw_insert(line, col, text, nil, 0)
  for index = #server.outbound, 1, -1 do
    local entry = server.outbound[index]
    if entry.method == "textDocument/didChange" then
      table.remove(server.outbound, index)
    end
  end
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

local function count_edits(doc)
  return #doc.edits
end

local function has_insert(doc, text)
  for _, edit in ipairs(doc.edits) do
    if edit.op == "insert" and edit.text == text then return true end
  end
  return false
end

---Ordered menu keys as the editor would iterate them.
local function item_keys(items)
  local keys = {}
  for key in pairs(items) do keys[#keys + 1] = key end
  table.sort(keys)
  return keys
end

---The single queued completionItem/resolve request, if any.
local function find_resolve(server)
  for _, entry in ipairs(server.outbound) do
    if entry.method == "completionItem/resolve" then return entry end
  end
  return nil
end

local function drop_resolves(server)
  for index = #server.outbound, 1, -1 do
    if server.outbound[index].method == "completionItem/resolve" then
      table.remove(server.outbound, index)
    end
  end
end

-- ===========================================================================
-- Case A: two same-label items with different textEdits. Both rows must be
-- populated; selecting each resolves and applies ITS OWN edit exactly, even
-- though both rows share one visible label.
-- Red on main: main stores both under items["foo"], so the second silently
-- replaces the first and its edit is unreachable.
-- ===========================================================================
do
  local lsp = fresh_module_load()
  local server = make_server("perllsp", RESOLVE_CAPS)
  register(lsp, "perllsp", server)
  local doc = make_doc("C:/proj/a.pl", { "fo\n" }, 1, 6)

  local dv = setmetatable({ doc = doc }, require "core.docview")
  activate(dv)

  local items = open_completion(lsp, server, doc, {
    { label = "foo", kind = 3, data = { which = 1 },
      textEdit = { range = { start = { line = 0, character = 0 }, ["end"] = { line = 0, character = 2 } },
                   newText = "foo_one()" } },
    { label = "foo", kind = 3, data = { which = 2 },
      textEdit = { range = { start = { line = 0, character = 0 }, ["end"] = { line = 0, character = 2 } },
                   newText = "foo_two()" } },
  })

  local keys = item_keys(items)
  ok(#keys == 2, "caseA: both same-label candidates are represented")
  local first, second = items[keys[1]], items[keys[2]]
  ok(first ~= nil and second ~= nil,
    "caseA: both rows carry their own item records")
  if not (first and second) then goto continue_a end
  ok(first.data.completion_item.data.which ~= second.data.completion_item.data.which,
    "caseA: the two rows retain two distinct original CompletionItems")
  ok(first.data.display_label == "foo" and second.data.display_label == "foo",
    "caseA: both rows keep the exact server-provided display label")
  ok(first.data.completion_item.textEdit.newText == "foo_one()"
     and second.data.completion_item.textEdit.newText == "foo_two()",
    "caseA: neither row's insertion bytes were mutated by identity")

  -- Select the first row: it defers, resolves with its own full item, and
  -- applies its own bytes plus its own additionalTextEdit.
  ok(first.onselect(1, first) == false, "caseA: first duplicate defers to resolution")
  local resolve_one = find_resolve(server)
  ok(resolve_one ~= nil and resolve_one.params.data.which == 1,
    "caseA: the first selection resolves the first original item")
  drop_resolves(server)
  resolve_one.callback(server, { result = {
    label = "foo", data = { which = 1 },
    textEdit = { range = { start = { line = 0, character = 0 }, ["end"] = { line = 0, character = 2 } },
                 newText = "resolved_one()" },
    additionalTextEdits = {
      { range = { start = { line = 0, character = 0 }, ["end"] = { line = 0, character = 0 } },
        newText = "use One;\n" } },
  } })
  ok(has_insert(doc, "resolved_one()") and has_insert(doc, "use One;\n"),
    "caseA: the first duplicate applied its own resolved edit")

  -- Select the second row: its own separate resolve operation and its own
  -- bytes; the first row's application is not repeated or swapped in.
  ok(second.onselect(1, second) == false, "caseA: second duplicate defers too")
  local resolve_two = find_resolve(server)
  ok(resolve_two ~= nil and resolve_two.params.data.which == 2,
    "caseA: the second selection resolves the second original item")
  drop_resolves(server)
  resolve_two.callback(server, { result = {
    label = "foo", data = { which = 2 },
    textEdit = { range = { start = { line = 0, character = 0 }, ["end"] = { line = 0, character = 2 } },
                 newText = "resolved_two()" },
  } })
  ok(has_insert(doc, "resolved_two()"),
    "caseA: the second duplicate applied its own resolved edit")
  local one_applies = 0
  for _, edit in ipairs(doc.edits) do
    if edit.op == "insert" and edit.text == "resolved_one()" then
      one_applies = one_applies + 1
    end
  end
  ok(one_applies == 1, "caseA: the first duplicate's edit was applied exactly once")
  ::continue_a::
end

-- ===========================================================================
-- Case B: two same-label items differing by kind/detail on a non-resolving
-- server. Both stay selectable and apply their own originals directly.
-- Red on main: main collapses them into one map slot.
-- ===========================================================================
do
  local lsp = fresh_module_load()
  local server = make_server("perllsp", PLAIN_COMPLETION_CAPS)
  register(lsp, "perllsp", server)
  local doc = make_doc("C:/proj/b.pl", { "du\n" }, 1, 6)

  local dv = setmetatable({ doc = doc }, require "core.docview")
  activate(dv)

  local items = open_completion(lsp, server, doc, {
    { label = "dup", kind = 3, detail = "function detail",
      textEdit = { range = { start = { line = 0, character = 0 }, ["end"] = { line = 0, character = 2 } },
                   newText = "dup_function()" } },
    { label = "dup", kind = 6, detail = "method detail",
      textEdit = { range = { start = { line = 0, character = 0 }, ["end"] = { line = 0, character = 2 } },
                   newText = "dup_method()" } },
  })

  local keys = item_keys(items)
  ok(#keys == 2, "caseB: both same-label kinds survive")
  if not (items[keys[1]] and items[keys[2]]) then goto continue_b end
  local infos = {}
  for index, key in ipairs(keys) do infos[index] = items[key].info end
  ok(infos[1] ~= infos[2],
    "caseB: the two rows keep their distinct kind projections")
  ok(tostring(items[keys[1]].desc):find("detail", 1, true) ~= nil
     and tostring(items[keys[2]].desc):find("detail", 1, true) ~= nil,
    "caseB: both rows keep their own detail descriptions")

  ok(items[keys[1]].onselect(1, items[keys[1]]) == true,
    "caseB: first kind selects directly without resolution")
  ok(has_insert(doc, items[keys[1]].data.completion_item.textEdit.newText),
    "caseB: first kind applied its own original bytes")
  ok(items[keys[2]].onselect(1, items[keys[2]]) == true,
    "caseB: second kind selects directly without resolution")
  ok(has_insert(doc, items[keys[2]].data.completion_item.textEdit.newText),
    "caseB: second kind applied its own original bytes")
  local resolves = 0
  for _, entry in ipairs(server.outbound) do
    if entry.method == "completionItem/resolve" then resolves = resolves + 1 end
  end
  ok(resolves == 0, "caseB: non-resolving servers still receive no resolve requests")
  ::continue_b::
end

-- ===========================================================================
-- Case C: hover of one duplicate attaches documentation only to that
-- duplicate; the untouched sibling's description and resolve state are not
-- contaminated, and resolving both yields two independent operations.
-- Red on main: the sibling row does not exist at all.
-- ===========================================================================
do
  local lsp = fresh_module_load()
  local server = make_server("perllsp", RESOLVE_CAPS)
  register(lsp, "perllsp", server)
  local doc = make_doc("C:/proj/c.pl", { "fo\n" }, 1, 6)

  local dv = setmetatable({ doc = doc }, require "core.docview")
  activate(dv)

  local items = open_completion(lsp, server, doc, {
    { label = "foo", kind = 3, data = { which = 1 },
      textEdit = { range = { start = { line = 0, character = 0 }, ["end"] = { line = 0, character = 2 } },
                   newText = "hover_one()" } },
    { label = "foo", kind = 3, data = { which = 2 },
      textEdit = { range = { start = { line = 0, character = 0 }, ["end"] = { line = 0, character = 2 } },
                   newText = "hover_two()" } },
  })

  local keys = item_keys(items)
  local row_one, row_two = items[keys[1]], items[keys[2]]
  ok(#keys == 2, "caseC: both duplicates are represented before hover")
  if not (row_one and row_two) then goto continue_c end

  row_one.onhover(1, row_one)
  local resolve_entry = find_resolve(server)
  ok(resolve_entry ~= nil and resolve_entry.params.data.which == 1,
    "caseC: hovering the first duplicate prefetches exactly that item")
  local resolve_count = 0
  for _, entry in ipairs(server.outbound) do
    if entry.method == "completionItem/resolve" then resolve_count = resolve_count + 1 end
  end
  ok(resolve_count == 1, "caseC: exactly one resolve exists after one hover")

  drop_resolves(server)
  resolve_entry.callback(server, { result = {
    label = "foo", data = { which = 1 },
    documentation = { kind = "plain", value = "DOC-ONE" },
  } })
  ok(tostring(row_one.desc):find("DOC-ONE", 1, true) ~= nil,
    "caseC: the hovered duplicate received its documentation")
  ok(tostring(row_two.desc):find("DOC-ONE", 1, true) == nil,
    "caseC: the untouched duplicate's description stayed uncontaminated")
  ok(row_two.data.resolve.state == "unresolved",
    "caseC: the untouched duplicate's resolve state is still unresolved")

  -- Selecting the untouched duplicate starts its OWN resolve operation.
  local edits_before = count_edits(doc)
  ok(row_two.onselect(1, row_two) == false,
    "caseC: selecting the untouched duplicate defers to its own resolve")
  local second_resolve = find_resolve(server)
  ok(second_resolve ~= nil and second_resolve.params.data.which == 2,
    "caseC: the second duplicate resolves its own original item")
  drop_resolves(server)
  second_resolve.callback(server, { result = {
    label = "foo", data = { which = 2 },
    textEdit = { range = { start = { line = 0, character = 0 }, ["end"] = { line = 0, character = 2 } },
                 newText = "two_applied()" },
  } })
  ok(has_insert(doc, "two_applied()"),
    "caseC: the second duplicate applied its own resolved bytes")
  local inserts = {}
  for _, edit in ipairs(doc.edits) do
    if edit.op == "insert" then inserts[#inserts + 1] = edit.text end
  end
  ok(#inserts == 1 and inserts[1] == "two_applied()",
    "caseC: only the second duplicate's application mutated the document")
  ::continue_c::
end

-- ===========================================================================
-- Case D: byte-identical duplicate items. Declared policy: preserve-all -
-- both twins stay selectable and each applies exactly once; repeated
-- selection callbacks of one row stay idempotent.
-- Red on main: main drops the second twin entirely.
-- ===========================================================================
do
  local lsp = fresh_module_load()
  local server = make_server("perllsp", PLAIN_COMPLETION_CAPS)
  register(lsp, "perllsp", server)
  local doc = make_doc("C:/proj/d.pl", { "tw\n" }, 1, 6)

  local dv = setmetatable({ doc = doc }, require "core.docview")
  activate(dv)

  local twin = {
    label = "twin", kind = 3,
    textEdit = { range = { start = { line = 0, character = 0 }, ["end"] = { line = 0, character = 2 } },
                 newText = "twin()" },
  }
  local items = open_completion(lsp, server, doc, {
    twin,
    { label = "twin", kind = 3,
      textEdit = { range = { start = { line = 0, character = 0 }, ["end"] = { line = 0, character = 2 } },
                   newText = "twin()" } },
  })

  local keys = item_keys(items)
  ok(#keys == 2,
    "caseD: byte-identical duplicates are preserved (preserve-all policy)")
  if not (items[keys[1]] and items[keys[2]]) then goto continue_d end
  ok(items[keys[1]].onselect(1, items[keys[1]]) == true,
    "caseD: the first twin selects")
  local after_first = count_edits(doc)
  ok(after_first > 0, "caseD: the first twin applied")
  ok(items[keys[2]].onselect(1, items[keys[2]]) == true,
    "caseD: the second twin selects independently")
  local after_second = count_edits(doc)
  ok(after_second > after_first, "caseD: the second twin applied its own copy")
  ok(items[keys[2]].onselect(2, items[keys[2]]) == true,
    "caseD: repeated selection reports idempotent success")
  ok(count_edits(doc) == after_second,
    "caseD: repeated selection applied nothing again")
  ::continue_d::
end

-- ===========================================================================
-- Case E: a same-label row from an OLDER response cannot apply after an
-- intervening edit and a newer same-label response; the newer row applies.
-- Pins subject-scoped identity across rounds for colliding labels.
-- ===========================================================================
do
  local lsp = fresh_module_load()
  local server = make_server("perllsp", PLAIN_COMPLETION_CAPS)
  register(lsp, "perllsp", server)
  local doc = make_doc("C:/proj/e.pl", { "fo\n" }, 1, 6)

  local dv = setmetatable({ doc = doc }, require "core.docview")
  activate(dv)

  local old_items = open_completion(lsp, server, doc, {
    { label = "foo", kind = 3,
      textEdit = { range = { start = { line = 0, character = 0 }, ["end"] = { line = 0, character = 2 } },
                   newText = "round_one()" } },
  })
  local old_keys = item_keys(old_items)
  local old_row = old_items[old_keys[1]]
  if not old_row then goto continue_e end

  -- Intervening accepted edit makes the old round stale.
  accept_edit(lsp, doc, server, 1, 3, "x")
  doc._selection = { line = 1, col = 6 }

  ok(old_row.onselect(1, old_row) == false,
    "caseE: the old same-label row refuses after the intervening edit")
  ok(count_edits(doc) == 0, "caseE: the refused old row mutated nothing")

  -- A newer response reusing the same label populates a fresh current row.
  local new_items = open_completion(lsp, server, doc, {
    { label = "foo", kind = 3,
      textEdit = { range = { start = { line = 0, character = 0 }, ["end"] = { line = 0, character = 2 } },
                   newText = "round_two()" } },
  })
  local new_keys = item_keys(new_items)
  ok(#new_keys == 1,
    "caseE: the newer list owns its own response scope (no leaked identities)")
  local new_row = new_items[new_keys[1]]
  ok(new_row.data.completion_item.textEdit.newText == "round_two()"
     and new_row ~= old_row,
    "caseE: the new same-label row carries the new response's item")
  ok(new_row.onselect(1, new_row) == true,
    "caseE: the new same-label row applies while current")
  ok(has_insert(doc, "round_two()"), "caseE: the new row's bytes landed")
  ok(old_row.onselect(1, old_row) == false,
    "caseE: the superseded row still refuses once a newer list exists")
  ::continue_e::
end

-- ===========================================================================
-- Case F: regression pins for the non-colliding majority. Unique labels keep
-- the exact plain label key; provenance (source order and display text) is
-- retained on every row; three same-label items produce deterministic
-- distinct keys in source order.
-- Green on main for unique labels (keying unchanged); red on main for the
-- triple case (rows collapse).
-- ===========================================================================
do
  local lsp = fresh_module_load()
  local server = make_server("perllsp", PLAIN_COMPLETION_CAPS)
  register(lsp, "perllsp", server)
  local doc = make_doc("C:/proj/f.pl", { "uz\n" }, 1, 6)

  local dv = setmetatable({ doc = doc }, require "core.docview")
  activate(dv)

  local items = open_completion(lsp, server, doc, {
    { label = "alpha", kind = 3,
      textEdit = { range = { start = { line = 0, character = 0 }, ["end"] = { line = 0, character = 2 } },
                   newText = "alpha()" } },
    { label = "beta", kind = 6,
      textEdit = { range = { start = { line = 0, character = 0 }, ["end"] = { line = 0, character = 2 } },
                   newText = "beta()" } },
  })
  local keys = item_keys(items)
  ok(#keys == 2 and items["alpha"] ~= nil and items["beta"] ~= nil,
    "caseF: unique labels keep their exact plain label keys")
  ok(items["alpha"].data.display_label == "alpha"
     and items["alpha"].data.item_index == 1
     and items["beta"].data.item_index == 2,
    "caseF: provenance retains display text and original source order")
  ok(keys[1]:find("#", 1, true) == nil and keys[2]:find("#", 1, true) == nil,
    "caseF: unique labels gain no disambiguation suffix")

  local triple = open_completion(lsp, server, doc, {
    { label = "x", kind = 3, detail = "first",
      textEdit = { range = { start = { line = 0, character = 0 }, ["end"] = { line = 0, character = 2 } },
                   newText = "x_first()" } },
    { label = "x", kind = 3, detail = "second",
      textEdit = { range = { start = { line = 0, character = 0 }, ["end"] = { line = 0, character = 2 } },
                   newText = "x_second()" } },
    { label = "x", kind = 3, detail = "third",
      textEdit = { range = { start = { line = 0, character = 0 }, ["end"] = { line = 0, character = 2 } },
                   newText = "x_third()" } },
  })
  local tkeys = item_keys(triple)
  ok(#tkeys == 3, "caseF: all three same-label items are represented")
  if #tkeys < 3 then goto continue_f end
  ok(tkeys[1] == "x" and tkeys[2] == "x#2" and tkeys[3] == "x#3",
    "caseF: duplicate keys follow deterministic source-order suffixes")
  ok(triple["x"].data.item_index == 1
     and triple["x#2"].data.item_index == 2
     and triple["x#3"].data.item_index == 3,
    "caseF: suffixed rows keep ascending original positions")
  ok(triple["x"].data.completion_item.detail == "first"
     and triple["x#2"].data.completion_item.detail == "second"
     and triple["x#3"].data.completion_item.detail == "third",
    "caseF: each suffixed row maps to its own original CompletionItem")
  ok(triple["x"].onselect(1, triple["x"]) == true
     and triple["x#2"].onselect(1, triple["x#2"]) == true
     and triple["x#3"].onselect(1, triple["x#3"]) == true,
    "caseF: every suffixed row remains selectable")
  ok(has_insert(doc, "x_first()") and has_insert(doc, "x_second()")
     and has_insert(doc, "x_third()"),
    "caseF: each row applied its own bytes, none swapped")
  ::continue_f::
end

print(string.format("%d passed, %d failed", passed, failed))
if failed > 0 then os.exit(1) end

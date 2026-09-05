-- Deterministic focused tests for exact pre-apply completionItem/resolve in
-- clients/lite-xl/upstream/init.lua (#11188), consuming #11108 subjects,
-- #11115 document sessions, #10657 timeouts, and #10833 typed dispositions.
--
-- Run:
--   lua clients/lite-xl/tests/init_completion_resolve_test.lua [path-to-init-module]
-- Default module path is ../upstream/init.lua relative to this file.
--
-- Proof shape: held responses are drained manually against the exact staged
-- module under minimal Lite XL runtime fakes. Tests drive the real public
-- path - request_completion populates the box through the autocomplete fake,
-- then each item's onhover/onselect callbacks run exactly as the editor would
-- invoke them - and assert the #11188 contract:
--
--   selection correctness is independent of prior hover;
--   resolve uses the FULL item and never requires completion_item.data;
--   hover and selection share one generation-bound resolve operation;
--   no document mutation begins while resolution is pending;
--   resolved textEdit/additionalTextEdits win over the original fields;
--   documentation-only resolves keep application correct;
--   timeout/failure applies a provably self-complete original or refuses
--   explicitly without partial mutation;
--   an edit or restart/server replacement while held makes the late resolve
--   inert;
--   repeated selection callbacks apply exactly once;
--   servers without resolveProvider keep applying originals directly.
--
-- Red-first baseline: run this suite against CURRENT MAIN before this change
-- (origin/main clients/lite-xl/upstream/init.lua at b6e5b4cc4):
--
--   lua clients/lite-xl/tests/init_completion_resolve_test.lua <main-init.lua>
--
-- There the direct-select/no-hover cases MUST fail (main applies whatever
-- fields are present without resolving), the no-data cases MUST fail (main
-- only resolves hovered items that carry a data field), the deferred cases
-- MUST fail (main mutates immediately during selection), and the once-only
-- case MUST fail (main re-applies on every callback).
--
-- Single-behavior mutation falsifiers of the PATCHED module (each verified
-- caught):
--   1. restore immediate application of the raw original in onselect ->
--      the deferred/join cases fail;
--   2. restore the completion_item.data gate in the resolve begin path ->
--      every no-data case fails;
--   3. delete BOTH admission layers (resolve response callback AND the
--      apply-time round-subject revalidation) -> edit-while-held and
--      restart-while-held fail; deleting either layer alone leaves the
--      other as defense in depth, which is itself pinned by staying green;
--   4. delete the applied exactly-once flag -> the repeat-selection case
--      fails;
--   5. drop the overlay so resolved fields lose to original fields -> the
--      resolved-fields-win cases fail;
--   6. delete the document-binding guard in the apply terminal -> the
--      wrong-active-document case fails.
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

-- ===========================================================================
-- Case A: direct selection without ever hovering; the item carries NO data
-- field; resolve supplies additionalTextEdits. Resolve happens before any
-- mutation and the resolved extra edit lands with the main insert.
-- Red on main: main applies the unresolved item immediately (no resolve, no
-- import edit).
-- ===========================================================================
do
  local lsp = fresh_module_load()
  local server = make_server("perllsp", RESOLVE_CAPS)
  register(lsp, "perllsp", server)
  local doc = make_doc("C:/proj/a.pl", { "fo\n" }, 1, 6)

  local dv = setmetatable({ doc = doc }, require "core.docview")
  activate(dv)

  local items = open_completion(lsp, server, doc, {
    { label = "foo", kind = 3,
      textEdit = { range = { start = { line = 0, character = 0 }, ["end"] = { line = 0, character = 2 } },
                   newText = "foo()" } },
  })
  local item = items["foo"]
  ok(item ~= nil, "caseA: completion item populated")
  ok(item.data.completion_item.data == nil, "caseA: fixture item carries no data field")

  local selected = item.onselect(1, item)
  ok(selected == false, "caseA: selection defers while resolution is pending")
  ok(count_edits(doc) == 0,
    "caseA: no document mutation begins while resolve is in flight")

  ok(play_response(server, "completionItem/resolve", { result = {
    label = "foo", kind = 3,
    textEdit = { range = { start = { line = 0, character = 0 }, ["end"] = { line = 0, character = 2 } },
                 newText = "foo()" },
    additionalTextEdits = {
      { range = { start = { line = 0, character = 0 }, ["end"] = { line = 0, character = 0 } },
        newText = "use Foo;\n" } },
  } }), "caseA: resolve request reached the wire and its response played")

  ok(has_insert(doc, "foo()") and has_insert(doc, "use Foo;\n"),
    "caseA: resolved additionalTextEdit is applied alongside the main insert")
end

-- ===========================================================================
-- Case B: hover alone resolves the FULL item even without a data field.
-- Red on main: main's data gate sends nothing.
-- ===========================================================================
do
  local lsp = fresh_module_load()
  local server = make_server("perllsp", RESOLVE_CAPS)
  register(lsp, "perllsp", server)
  local doc = make_doc("C:/proj/b.pl", { "fo\n" }, 1, 6)

  local items = open_completion(lsp, server, doc, {
    { label = "foo", kind = 3,
      textEdit = { range = { start = { line = 0, character = 0 }, ["end"] = { line = 0, character = 2 } },
                   newText = "foo()" } },
  })
  local item = items["foo"]

  item.onhover(1, item)
  local resolves = 0
  local sent_item
  for _, entry in ipairs(server.outbound) do
    if entry.method == "completionItem/resolve" then
      resolves = resolves + 1
      sent_item = entry.params
    end
  end
  ok(resolves == 1, "caseB: hover prefetches exactly one resolve without a data field")
  ok(sent_item and sent_item.label == "foo",
    "caseB: the full received item travels as the resolve params")
end

-- ===========================================================================
-- Case C: hover starts the resolve, selection joins the SAME operation; the
-- application waits for the shared terminal and runs exactly once.
-- Red on main: main applies immediately at selection time.
-- ===========================================================================
do
  local lsp = fresh_module_load()
  local server = make_server("perllsp", RESOLVE_CAPS)
  register(lsp, "perllsp", server)
  local doc = make_doc("C:/proj/c.pl", { "fo\n" }, 1, 6)

  local dv = setmetatable({ doc = doc }, require "core.docview")
  activate(dv)

  local items = open_completion(lsp, server, doc, {
    { label = "foo", kind = 3,
      textEdit = { range = { start = { line = 0, character = 0 }, ["end"] = { line = 0, character = 2 } },
                   newText = "foo()" } },
  })
  local item = items["foo"]

  item.onhover(1, item)
  local queued = item.onselect(1, item)
  ok(queued == false and count_edits(doc) == 0,
    "caseC: joining an in-flight resolve defers application without mutating")
  -- Repeated menu callbacks while pending stay inert.
  ok(item.onselect(2, item) == false and count_edits(doc) == 0,
    "caseC: repeat selection callbacks while pending stay inert")

  ok(play_response(server, "completionItem/resolve", { result = {
    label = "foo", detail = "Foo subs",
    textEdit = { range = { start = { line = 0, character = 0 }, ["end"] = { line = 0, character = 2 } },
                 newText = "foo()" },
  } }), "caseC: shared resolve response played")

  ok(has_insert(doc, "foo()"), "caseC: the joined selection applied after resolution")
  ok(item.desc == "Foo subs", "caseC: hover description updated from the exact result")

  local resolves = 0
  for _, entry in ipairs(server.outbound) do
    if entry.method == "completionItem/resolve" then resolves = resolves + 1 end
  end
  ok(resolves == 0,
    "caseC: hover and selection consumed ONE resolve; no duplicate was sent")
end

-- ===========================================================================
-- Case D: resolve changes both the main textEdit and supplies
-- additionalTextEdits; final bytes use the RESOLVED fields.
-- Red on main: main applies the original fields only.
-- ===========================================================================
do
  local lsp = fresh_module_load()
  local server = make_server("perllsp", RESOLVE_CAPS)
  register(lsp, "perllsp", server)
  local doc = make_doc("C:/proj/d.pl", { "fo\n" }, 1, 6)

  local dv = setmetatable({ doc = doc }, require "core.docview")
  activate(dv)

  local items = open_completion(lsp, server, doc, {
    { label = "foo", kind = 3,
      textEdit = { range = { start = { line = 0, character = 0 }, ["end"] = { line = 0, character = 2 } },
                   newText = "foo" } },
  })
  local item = items["foo"]

  ok(item.onselect(1, item) == false,
    "caseD: unresolved selection defers")
  ok(play_response(server, "completionItem/resolve", { result = {
    label = "foo",
    textEdit = { range = { start = { line = 0, character = 0 }, ["end"] = { line = 0, character = 2 } },
                 newText = "foobar()" },
    additionalTextEdits = {
      { range = { start = { line = 0, character = 0 }, ["end"] = { line = 0, character = 0 } },
        newText = "import foo;\n" } },
  } }), "caseD: resolve played")

  ok(has_insert(doc, "foobar()") and not has_insert(doc, "foo"),
    "caseD: final bytes carry the resolved main edit, not the original")
  ok(has_insert(doc, "import foo;\n"), "caseD: resolve-supplied auto-import edit is applied")
end

-- ===========================================================================
-- Case E: documentation-only resolve keeps application correct under the
-- declared overlay policy (original application fields still rule).
-- Partially red on main: main never resolves here (no hover, data-less), so
-- the description never updates.
-- ===========================================================================
do
  local lsp = fresh_module_load()
  local server = make_server("perllsp", RESOLVE_CAPS)
  register(lsp, "perllsp", server)
  local doc = make_doc("C:/proj/e.pl", { "fo\n" }, 1, 6)

  local dv = setmetatable({ doc = doc }, require "core.docview")
  activate(dv)

  local items = open_completion(lsp, server, doc, {
    { label = "foo", kind = 3,
      textEdit = { range = { start = { line = 0, character = 0 }, ["end"] = { line = 0, character = 2 } },
                   newText = "foo()" } },
  })
  local item = items["foo"]

  ok(item.onselect(1, item) == false, "caseE: selection defers to resolution")
  ok(play_response(server, "completionItem/resolve", { result = {
    label = "foo",
    documentation = { kind = "markdown", value = "**docs**" },
  } }), "caseE: documentation-only resolve played")

  ok(has_insert(doc, "foo()"),
    "caseE: application remains correct using original bytes under documentation-only resolve")
  ok(tostring(item.desc):find("docs", 1, true) ~= nil,
    "caseE: documentation reached the item description from the selected resolve")
end

-- ===========================================================================
-- Case F: timeout/failure terminals are explicit under the declared
-- completeness policy. An original whose application bytes are its own
-- (textEdit) falls back; an item with no self-owned application surface
-- (label only, nothing resolution-independent to apply) refuses without
-- partial mutation. Red on main: main either never resolves (data-less) or
-- applies regardless.
-- ===========================================================================
do
  local lsp = fresh_module_load()
  local server = make_server("perllsp", RESOLVE_CAPS)
  register(lsp, "perllsp", server)
  local doc = make_doc("C:/proj/f.pl", { "fo\n" }, 1, 6)

  local dv = setmetatable({ doc = doc }, require "core.docview")
  activate(dv)

  -- Self-complete fallback: the original carries its own application bytes.
  local items = open_completion(lsp, server, doc, {
    { label = "bar", kind = 3,
      textEdit = { range = { start = { line = 0, character = 0 }, ["end"] = { line = 0, character = 2 } },
                   newText = "bar()" } },
  })
  local item = items["bar"]

  ok(item.onselect(1, item) == false, "caseF: unresolved selection defers")
  local resolve_entry
  for _, entry in ipairs(server.outbound) do
    if entry.method == "completionItem/resolve" then resolve_entry = entry end
  end
  if resolve_entry == nil then
    ok(false, "caseF: resolve request carries its timeout disposition seam")
    ok(false, "caseF: timed-out self-complete original falls back")
  else
    ok(resolve_entry.timeout_callback ~= nil,
      "caseF: resolve request carries its timeout disposition seam")
    -- #10657 review pin: the pending resolve defers applying the selection,
    -- so it must carry the explicit short window instead of falling through
    -- to the patient single-send default policy.
    ok(resolve_entry.timeout == 2,
      "caseF: resolve keeps the explicit short responsiveness window")
    for index = #server.outbound, 1, -1 do
      if server.outbound[index].method == "completionItem/resolve" then
        table.remove(server.outbound, index)
      end
    end
    resolve_entry.timeout_callback(resolve_entry)
    ok(has_insert(doc, "bar()"),
      "caseF: timed-out resolution falls back to the provably self-complete original")
  end
  local edits_after_fallback = count_edits(doc)

  -- Label-only item: nothing self-owned to apply, so a terminal without
  -- resolution refuses instead of guessing.
  local items2 = open_completion(lsp, server, doc, {
    { label = "qux", kind = 3 },
  })
  local item2 = items2["qux"]
  ok(item2 ~= nil, "caseF: label-only item populated")
  ok(item2.onselect(1, item2) == false, "caseF: label-only selection defers too")
  local entry2
  for _, entry in ipairs(server.outbound) do
    if entry.method == "completionItem/resolve" then entry2 = entry end
  end
  if entry2 == nil then
    ok(false, "caseF: label-only timeout refuses without partial mutation")
  else
    for index = #server.outbound, 1, -1 do
      if server.outbound[index].method == "completionItem/resolve" then
        table.remove(server.outbound, index)
      end
    end
    entry2.timeout_callback(entry2)
    ok(count_edits(doc) == edits_after_fallback,
      "caseF: label-only timeout refuses; earlier fallback edits untouched")
  end
end

-- ===========================================================================
-- Case G: document edited while resolve held (caret restored to the same
-- place); the late result is stale and cannot mutate anything.
-- Red on main: main applied the selection immediately before the edit.
-- ===========================================================================
do
  local lsp = fresh_module_load()
  local server = make_server("perllsp", RESOLVE_CAPS)
  register(lsp, "perllsp", server)
  local doc = make_doc("C:/proj/g.pl", { "fo\n" }, 1, 6)

  local dv = setmetatable({ doc = doc }, require "core.docview")
  activate(dv)

  local items = open_completion(lsp, server, doc, {
    { label = "foo", kind = 3,
      textEdit = { range = { start = { line = 0, character = 0 }, ["end"] = { line = 0, character = 2 } },
                   newText = "foo()" } },
  })
  local item = items["foo"]

  ok(item.onselect(1, item) == false, "caseG: selection defers")
  accept_edit(lsp, doc, server, 1, 3, "x")
  doc._selection = { line = 1, col = 3 }

  ok(play_response(server, "completionItem/resolve", { result = {
    label = "foo",
    textEdit = { range = { start = { line = 0, character = 0 }, ["end"] = { line = 0, character = 2 } },
                 newText = "mutated()" },
  } }), "caseG: late resolve response arrived after the edit")

  ok(count_edits(doc) == 0,
    "caseG: the stale late resolve mutated nothing and applied nothing")
end

-- ===========================================================================
-- Case H: server replaced (restart/provider switch) while resolve held; the
-- old result is inert against the new generation.
-- Red on main: main applied at selection time regardless.
-- ===========================================================================
do
  local lsp = fresh_module_load()
  local server = make_server("perllsp", RESOLVE_CAPS)
  register(lsp, "perllsp", server)
  local doc = make_doc("C:/proj/h.pl", { "fo\n" }, 1, 6)

  local dv = setmetatable({ doc = doc }, require "core.docview")
  activate(dv)

  local items = open_completion(lsp, server, doc, {
    { label = "foo", kind = 3,
      textEdit = { range = { start = { line = 0, character = 0 }, ["end"] = { line = 0, character = 2 } },
                   newText = "foo()" } },
  })
  local item = items["foo"]

  ok(item.onselect(1, item) == false, "caseH: selection defers")

  local replacement = make_server("perllsp", RESOLVE_CAPS)
  register(lsp, "perllsp", replacement)
  doc.lsp_open = true

  local held = #server.outbound > 0 and table.remove(server.outbound, 1) or nil
  if held == nil then
    ok(false, "caseH: a resolve answered by a replaced server can never apply")
  else
    held.callback(server, { result = {
      label = "foo",
      textEdit = { range = { start = { line = 0, character = 0 }, ["end"] = { line = 0, character = 2 } },
                   newText = "from_dead_server()" },
    } })

    ok(count_edits(doc) == 0,
      "caseH: a resolve answered by a replaced server can never apply")
  end
end

-- ===========================================================================
-- Case I: repeated selection/callback repeats after success apply exactly
-- once.
-- Red on main: main re-applies on every callback.
-- ===========================================================================
do
  local lsp = fresh_module_load()
  local server = make_server("perllsp", RESOLVE_CAPS)
  register(lsp, "perllsp", server)
  local doc = make_doc("C:/proj/i.pl", { "fo\n" }, 1, 6)

  local dv = setmetatable({ doc = doc }, require "core.docview")
  activate(dv)

  local items = open_completion(lsp, server, doc, {
    { label = "foo", kind = 3,
      textEdit = { range = { start = { line = 0, character = 0 }, ["end"] = { line = 0, character = 2 } },
                   newText = "foo()" } },
  })
  local item = items["foo"]

  ok(item.onselect(1, item) == false, "caseI: first selection defers")
  ok(play_response(server, "completionItem/resolve", { result = {
    label = "foo",
    textEdit = { range = { start = { line = 0, character = 0 }, ["end"] = { line = 0, character = 2 } },
                 newText = "foo()" },
  } }), "caseI: resolve played")
  local applied_once = count_edits(doc)
  ok(applied_once >= 2, "caseI: main edit plus cursor bookkeeping happened once")

  ok(item.onselect(2, item) == true, "caseI: repeat selection reports idempotent success")
  ok(count_edits(doc) == applied_once,
    "caseI: repeat selection applied nothing again")
end

-- ===========================================================================
-- Case J: a server WITHOUT resolveProvider keeps applying originals directly;
-- no resolve request ever leaves (contract pin; passes on main too).
-- ===========================================================================
do
  local lsp = fresh_module_load()
  local server = make_server("perllsp", PLAIN_COMPLETION_CAPS)
  register(lsp, "perllsp", server)
  local doc = make_doc("C:/proj/j.pl", { "fo\n" }, 1, 6)

  local dv = setmetatable({ doc = doc }, require "core.docview")
  activate(dv)

  local items = open_completion(lsp, server, doc, {
    { label = "foo", kind = 3,
      textEdit = { range = { start = { line = 0, character = 0 }, ["end"] = { line = 0, character = 2 } },
                   newText = "plain()" } },
  })
  local item = items["foo"]

  if item.data.resolve == nil then
    ok(false, "caseJ: unsupported resolve starts as not_needed")
  else
    ok(item.data.resolve.state == "not_needed",
      "caseJ: unsupported resolve starts as not_needed")
  end
  ok(item.onselect(1, item) == true, "caseJ: selection applies the original directly")
  ok(has_insert(doc, "plain()"), "caseJ: original bytes applied without resolution")
  local resolves = 0
  for _, entry in ipairs(server.outbound) do
    if entry.method == "completionItem/resolve" then resolves = resolves + 1 end
  end
  ok(resolves == 0, "caseJ: no resolve request left for a non-resolving server")
end

-- ===========================================================================
-- Case K: JSON-RPC error and null-result resolves are failed terminals under
-- the completeness policy, never silent confirmations of the original.
-- ===========================================================================
do
  local lsp = fresh_module_load()
  local server = make_server("perllsp", RESOLVE_CAPS)
  register(lsp, "perllsp", server)
  local doc = make_doc("C:/proj/k.pl", { "fo\n" }, 1, 6)

  local dv = setmetatable({ doc = doc }, require "core.docview")
  activate(dv)

  -- Error response: a self-complete original still falls back, but through
  -- the guarded terminal - and a label-only original refuses outright.
  local items = open_completion(lsp, server, doc, {
    { label = "err_fn", kind = 3 },
  })
  local item = items["err_fn"]
  ok(item.onselect(1, item) == false, "caseK: selection defers")
  ok(play_response(server, "completionItem/resolve",
    { error = { code = -32603, message = "resolve exploded" } }),
    "caseK: resolve request reached the wire")
  ok(count_edits(doc) == 0,
    "caseK: an errored resolve refuses a non-self-complete original without mutation")

  -- Null result on a self-complete original: guarded fallback applies it.
  local items2 = open_completion(lsp, server, doc, {
    { label = "nul_fn", kind = 3,
      textEdit = { range = { start = { line = 0, character = 0 }, ["end"] = { line = 0, character = 2 } },
                   newText = "nul_fn()" } },
  })
  local item2 = items2["nul_fn"]
  ok(item2.onselect(1, item2) == false, "caseK: null-result selection defers")
  ok(play_response(server, "completionItem/resolve", { result = nil }),
    "caseK: null resolve response played")
  ok(has_insert(doc, "nul_fn()"),
    "caseK: null result falls back to the self-complete original, not silence")
end

-- ===========================================================================
-- Case L: queue rejection is a terminal. A fake server rejecting
-- push_request drives the pending selection through the same guarded
-- fallback/refuse path instead of leaving it pending forever.
-- ===========================================================================
do
  local lsp = fresh_module_load()
  local server = make_server("perllsp", RESOLVE_CAPS)
  register(lsp, "perllsp", server)
  function server:push_request(method, entry)
    entry.method = method
    entry.kind = "request"
    self.outbound[#self.outbound + 1] = entry
    return "not_queued"
  end
  local doc = make_doc("C:/proj/l.pl", { "fo\n" }, 1, 6)

  local dv = setmetatable({ doc = doc }, require "core.docview")
  activate(dv)

  local items = open_completion(lsp, server, doc, {
    { label = "q_fn", kind = 3,
      textEdit = { range = { start = { line = 0, character = 0 }, ["end"] = { line = 0, character = 2 } },
                   newText = "q_fn()" } },
  })
  local item = items["q_fn"]
  ok(item.onselect(1, item) == true,
    "caseL: queue-rejected resolve falls back to the self-complete original immediately")
  ok(has_insert(doc, "q_fn()"), "caseL: the fallback bytes were applied")

  -- Label-only item under queue rejection: explicit refusal, no mutation.
  local edits_before = count_edits(doc)
  local items2 = open_completion(lsp, server, doc, {
    { label = "q_bare", kind = 3 },
  })
  local item2 = items2["q_bare"]
  ok(item2.onselect(1, item2) == false,
    "caseL: queue-rejected resolve refuses a non-self-complete original explicitly")
  ok(count_edits(doc) == edits_before, "caseL: refusal mutated nothing")
end

-- ===========================================================================
-- Case M: deferred application binds to the captured document. A resolve
-- answered while another document view is active must refuse instead of
-- writing file A's ranges into file B (#12547 review repair).
-- ===========================================================================
do
  local lsp = fresh_module_load()
  local server = make_server("perllsp", RESOLVE_CAPS)
  register(lsp, "perllsp", server)
  local doc_a = make_doc("C:/proj/ma.pl", { "fo\n" }, 1, 6)
  local doc_b = make_doc("C:/proj/mb.pl", { "other\n" }, 1, 6)

  local dv_a = setmetatable({ doc = doc_a }, require "core.docview")
  activate(dv_a)

  local items = open_completion(lsp, server, doc_a, {
    { label = "foo", kind = 3,
      textEdit = { range = { start = { line = 0, character = 0 }, ["end"] = { line = 0, character = 2 } },
                   newText = "foo()" } },
  })
  local item = items["foo"]
  ok(item.onselect(1, item) == false, "caseM: selection defers")

  local dv_b = setmetatable({ doc = doc_b }, require "core.docview")
  activate(dv_b)

  ok(play_response(server, "completionItem/resolve", { result = {
    label = "foo",
    textEdit = { range = { start = { line = 0, character = 0 }, ["end"] = { line = 0, character = 2 } },
                 newText = "cross_doc()" },
  } }), "caseM: late resolve arrived while another document is active")

  ok(#doc_b.edits == 0, "caseM: the wrong active document was never touched")
  ok(#doc_a.edits == 0, "caseM: the resolved edit did not silently cross documents")
end

print(string.format("%d passed, %d failed", passed, failed))
if failed > 0 then os.exit(1) end

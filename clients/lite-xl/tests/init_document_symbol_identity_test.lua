-- Deterministic focused tests for duplicate-proof document-symbol identity
-- in clients/lite-xl/upstream/init.lua (#11198), consuming #11108 subjects
-- and the staged get_symbol_lists()/request_document_symbols() seam.
--
-- Run:
--   lua clients/lite-xl/tests/init_document_symbol_identity_test.lua [path-to-init-module]
-- Default module path is ../upstream/init.lua relative to this file.
--
-- Proof shape: held responses are drained manually against the exact staged
-- module under minimal Lite XL runtime fakes. Tests drive the real public
-- path - request_document_symbols opens the real command-view prompt through
-- the capturing fake, suggest() enumerates rows exactly as the editor would,
-- and submit() runs the real navigation - and assert the #11198 contract:
--
--   two same-name/kind symbols under one parent stay two independently
--     selectable rows navigating their own exact ranges;
--   flattened parent/name/kind collisions across branches stay distinct
--     rows navigating their own targets;
--   DocumentSymbol.navigation prefers selectionRange over range;
--   SymbolInformation duplicates with distinct locations each navigate
--     their own exact URI/range;
--   display order follows deterministic numeric/source order and visible
--     text stays the rendered symbol path without disambiguation segments;
--   rows from an older subject cannot navigate after an accepted edit or a
--     server replacement;
--   non-regression: unique names keep the exact historical rendering.
--
-- Red-first baseline: run this suite against CURRENT MAIN before this change
-- (origin/main clients/lite-xl/upstream/init.lua at 987e27c37):
--
--   lua clients/lite-xl/tests/init_document_symbol_identity_test.lua <main-init.lua>
--
-- There the duplicate cases MUST fail (main stores rows under the rendered
-- parent/name/kind string, so the later symbol overwrites the earlier
-- location while duplicated display entries alias the last writer), the
-- selectionRange case MUST fail (main always selects the broader range), and
-- the stale-row cases MUST hold via #11108 admission (green there too).
--
-- Single-behavior mutation falsifiers of the PATCHED module (each verified
-- caught):
--   1. restore plain rendered-string keying (drop the collision repair) ->
--      every duplicate-navigation case fails;
--   2. traverse results in reversed order -> the source-order binding case
--      fails;
--   3. drop the selectionRange preference -> the narrow-anchor case fails.
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
local prompt_options = {}
local navigations = {}

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
      enter = function(_, prompt, options)
        command_view_prompts[#command_view_prompts + 1] = prompt
        prompt_options[#prompt_options + 1] = options or {}
      end,
    },
    status_view = { separator2 = 2, add_item = function() end },
    active_view = nil,
    root_view = {
      get_active_node_default = function()
        return { add_view = function() end }
      end,
      -- Called as a method (root_view:open_doc(doc)); accept self.
      open_doc = function(_, doc)
        return { doc = doc }
      end,
    },
    -- #11165/#11198 navigation observability: every goto_location open is
    -- recorded with its own selectable target document.
    open_doc = function(path)
      local target = {
        path = path,
        lines = { [1] = "alpha\n", [2] = "beta\n", [3] = "gamma\n" },
        selections = {},
      }
      function target:set_selection(line1, col1)
        self.selections[#self.selections + 1] = { line1 = line1, col1 = col1 }
        navigations[#navigations + 1] =
          { path = self.path, line1 = line1, col1 = col1 }
      end
      return target
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
    home_expand = function(path) return path end,
    fuzzy_match = function(items, query)
      local res = {}
      for _, item in ipairs(items) do
        if query == nil or query == "" then
          res[#res + 1] = item
        elseif tostring(item):lower():find(tostring(query):lower(), 1, true) then
          res[#res + 1] = item
        end
      end
      return res
    end,
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
    can_complete = function() return false end,
    complete = function() end,
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

-- Main added the #11172 capability-manifest projection seam; load the real
-- staged module so fresh_module_load keeps exercising exact upstream source.
package.preload["plugins.lsp.capability_manifest"] = function()
  return dofile(here .. "/../upstream/capability_manifest.lua")
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
  local kinds = {
    [1] = "File", [3] = "Function", [5] = "Class",
    [6] = "Method", [12] = "Variable",
  }
  return {
    text_document_sync_kind = { None = 0, Full = 1, Incremental = 2 },
    position_encoding_kind = { UTF8 = "utf-8", UTF16 = "utf-16", UTF32 = "utf-32" },
    message_type = { Error = 1, Warning = 2, Info = 3, Log = 4, Debug = 5 },
    completion_trigger_Kind = { Invoked = 1, TriggerCharacter = 2 },
    insert_text_format = { PlainText = 1, Snippet = 2 },
    symbol_kind = kinds,
    get_symbol_kind = function(kind) return kinds[kind] or "Unknown" end,
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
  command_view_prompts = {}
  prompt_options = {}
  navigations = {}
  local lsp = dofile(init_module_path)
  if active_dv then
    require("core").active_view = active_dv
  else
    require("core").active_view = nil
  end
  return lsp
end

-- ---------------------------------------------------------------------------
-- Fixture helpers
-- ---------------------------------------------------------------------------

local SYMBOL_CAPS = {
  textDocumentSync = { openClose = true, change = 2, save = { includeText = false } },
  positionEncoding = "utf-16",
  documentSymbolProvider = {},
}

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

---Run one document-symbol round to the prompt and return its options
---(submit/suggest closures) exactly as the editor would drive them.
local function open_symbols(lsp, server, doc, result)
  open_admitted(lsp, doc, server)
  lsp.request_document_symbols(doc)
  if not play_response(server, "textDocument/documentSymbol", { result = result }) then
    ok(false, "open_symbols: documentSymbol response played")
    return nil
  end
  local options = prompt_options[#prompt_options]
  if not options or not options.submit or not options.suggest then
    ok(false, "open_symbols: navigation prompt opened with submit/suggest")
    return nil
  end
  return options
end

local function range_of(sl, sc, el, ec)
  return {
    start = { line = sl - 1, character = sc - 1 },
    ["end"] = { line = el - 1, character = ec - 1 },
  }
end

local function last_selection(doc)
  return doc.selections[#doc.selections]
end

-- ===========================================================================
-- Case A: two same-name/kind symbols under one parent with different ranges.
-- Both rows exist with distinct internal identities and navigate their own
-- exact ranges.
-- Red on main: main stores both under "f||Method"; every displayed row
-- aliases the last-written location.
-- ===========================================================================
do
  local lsp = fresh_module_load()
  local server = make_server("perllsp", SYMBOL_CAPS)
  register(lsp, "perllsp", server)
  local doc = make_doc("C:/proj/a.pl", { "sub f {}\nsub f {}\n" })

  local options = open_symbols(lsp, server, doc, {
    { name = "f", kind = 6, range = range_of(1, 1, 1, 9) },
    { name = "f", kind = 6, range = range_of(2, 1, 2, 9) },
  })
  if not options then goto continue_a end

  local rows = options.suggest("")
  ok(#rows == 2, "caseA: both same-name/kind symbols are offered")
  if #rows < 2 then goto continue_a end

  ok(rows[1].name ~= rows[2].name,
    "caseA: the two rows carry distinct internal identities")
  ok(rows[1].text == "f" and rows[2].text == "f",
    "caseA: visible symbol names are unchanged by identity repair")
  ok(rows[1].info == "Method" and rows[2].info == "Method",
    "caseA: both rows keep their kind projection")

  options.submit("", rows[1])
  local sel_one = last_selection(doc)
  ok(sel_one ~= nil and sel_one.line1 == 1 and sel_one.col1 == 1,
    "caseA: the first duplicate row navigates its own range start")
  options.submit("", rows[2])
  local sel_two = last_selection(doc)
  ok(sel_two ~= nil and sel_two.line1 == 2 and sel_two.col1 == 1,
    "caseA: the second duplicate row navigates its own range start")
  ok(sel_one.line1 ~= sel_two.line1,
    "caseA: duplicate rows never alias one navigation target")

  ::continue_a::
end

-- ===========================================================================
-- Case B: rendered parent/name/kind collisions across branches. Two siblings
-- named A each contain a child foo of the same kind; all four rows survive,
-- keep deterministic source order, and navigate exactly.
-- Red on main: parents overwrite each other and merged children alias the
-- last branch's locations.
-- ===========================================================================
do
  local lsp = fresh_module_load()
  local server = make_server("perllsp", SYMBOL_CAPS)
  register(lsp, "perllsp", server)
  local doc = make_doc("C:/proj/b.pl", { "package A; sub foo {}\npackage A; sub foo {}\n" })

  local options = open_symbols(lsp, server, doc, {
    { name = "A", kind = 5, range = range_of(1, 1, 1, 25),
      children = { { name = "foo", kind = 6, range = range_of(1, 13, 1, 24) } } },
    { name = "A", kind = 5, range = range_of(2, 1, 2, 25),
      children = { { name = "foo", kind = 6, range = range_of(2, 13, 2, 24) } } },
  })
  if not options then goto continue_b end

  local rows = options.suggest("")
  ok(#rows == 4, "caseB: every collided row survives (two parents, two children)")
  if #rows < 4 then goto continue_b end

  local seen_names = {}
  local unique = true
  for _, row in ipairs(rows) do
    if seen_names[row.name] then unique = false end
    seen_names[row.name] = true
  end
  ok(unique, "caseB: all four internal row identities are distinct")
  ok(rows[1].text == "A" and rows[2].text == "A/foo"
     and rows[3].text == "A" and rows[4].text == "A/foo",
    "caseB: display texts follow deterministic source order without suffixes")

  options.submit("", rows[1])
  local p1 = last_selection(doc)
  options.submit("", rows[3])
  local p2 = last_selection(doc)
  ok(p1 and p2 and p1.line1 == 1 and p2.line1 == 2,
    "caseB: the two colliding parents navigate their own ranges")
  options.submit("", rows[2])
  local c1 = last_selection(doc)
  options.submit("", rows[4])
  local c2 = last_selection(doc)
  ok(c1 and c2 and c1.line1 == 1 and c2.line1 == 2,
    "caseB: the two colliding children navigate their own ranges")

  ::continue_b::
end

-- ===========================================================================
-- Case C: nested DocumentSymbol whose selectionRange is narrower than range.
-- Navigation anchors the selectionRange start, never the broad extent.
-- Red on main: main always selected the broader range's start.
-- ===========================================================================
do
  local lsp = fresh_module_load()
  local server = make_server("perllsp", SYMBOL_CAPS)
  register(lsp, "perllsp", server)
  local doc = make_doc("C:/proj/c.pl", { "sub wide {\n", "  my $anchor;\n", "}\n" })

  local options = open_symbols(lsp, server, doc, {
    { name = "wide", kind = 6,
      range = range_of(1, 1, 3, 2),
      selectionRange = range_of(2, 7, 2, 12),
      children = {
        { name = "$anchor", kind = 12,
          range = range_of(2, 3, 2, 17),
          selectionRange = range_of(2, 7, 2, 12) },
      } },
  })
  if not options then goto continue_c end

  local rows = options.suggest("")
  ok(#rows == 2, "caseC: parent and child rows are both offered")
  if #rows < 2 then goto continue_c end

  options.submit("", rows[1])
  local parent_sel = last_selection(doc)
  ok(parent_sel and parent_sel.line1 == 2 and parent_sel.col1 == 7,
    "caseC: parent navigation anchors selectionRange, not range")
  options.submit("", rows[2])
  local child_sel = last_selection(doc)
  ok(child_sel and child_sel.line1 == 2 and child_sel.col1 == 7,
    "caseC: child navigation anchors its own selectionRange")

  ::continue_c::
end

-- ===========================================================================
-- Case D: SymbolInformation duplicates with distinct locations. Each row
-- navigates its own exact URI/range through the real goto_location path.
-- Red on main: main keyed both under "dup||Function"; both rows open the
-- last writer's location.
-- ===========================================================================
do
  local lsp = fresh_module_load()
  local server = make_server("perllsp", SYMBOL_CAPS)
  register(lsp, "perllsp", server)
  local doc = make_doc("C:/proj/d.pl", { "dup calls live elsewhere\n" })

  local options = open_symbols(lsp, server, doc, {
    { name = "dup", kind = 3,
      location = { uri = "file:///C:/proj/x.pl", range = range_of(2, 3, 2, 8) } },
    { name = "dup", kind = 3,
      location = { uri = "file:///C:/proj/y.pl", range = range_of(3, 5, 3, 10) } },
  })
  if not options then goto continue_d end

  local rows = options.suggest("")
  ok(#rows == 2, "caseD: both SymbolInformation duplicates are offered")
  if #rows < 2 then goto continue_d end
  ok(rows[1].name ~= rows[2].name,
    "caseD: duplicate locations retain distinct internal identities")

  options.submit("", rows[1])
  options.submit("", rows[2])
  ok(#navigations == 2, "caseD: both duplicates navigated exactly once")
  if #navigations < 2 then goto continue_d end

  local paths = {}
  for index, nav in ipairs(navigations) do paths[index] = nav.path end
  ok(paths[1] ~= paths[2],
    "caseD: each duplicate navigates its own exact URI")
  local want_first = paths[1] == "C:\\proj\\x.pl" or paths[1] == "C:/proj/x.pl"
  local want_second = paths[2] == "C:\\proj\\y.pl" or paths[2] == "C:/proj/y.pl"
  ok(want_first and want_second,
    "caseD: navigation targets match the retained locations exactly")
  ok(navigations[1].line1 == 2 and navigations[1].col1 == 3,
    "caseD: the first duplicate keeps its exact range anchor")
  ok(navigations[2].line1 == 3 and navigations[2].col1 == 5,
    "caseD: the second duplicate keeps its exact range anchor")

  ::continue_d::
end

-- ===========================================================================
-- Case E: stale rows cannot navigate. An accepted edit supersedes the held
-- subject; a server replacement invalidates it entirely.
-- Pins currentness at the identity seam (#11108 owners).
-- ===========================================================================
do
  local lsp = fresh_module_load()
  local server = make_server("perllsp", SYMBOL_CAPS)
  register(lsp, "perllsp", server)
  local doc = make_doc("C:/proj/e.pl", { "sub f {}\n" })

  local options = open_symbols(lsp, server, doc, {
    { name = "f", kind = 6, range = range_of(1, 1, 1, 9) },
  })
  if not options then goto continue_e end

  local rows = options.suggest("")
  accept_edit(lsp, doc, server, 1, 9, "\nmore")

  local selections_before = #doc.selections
  options.submit("", rows[1])
  ok(#doc.selections == selections_before,
    "caseE: an edited-over symbol row refuses to navigate")

  local replacement = make_server("perllsp", SYMBOL_CAPS)
  register(lsp, "perllsp", replacement)
  doc.lsp_open = true
  options.submit("", rows[1])
  ok(#doc.selections == selections_before,
    "caseE: a replaced-server symbol row refuses to navigate")

  ::continue_e::
end

-- ===========================================================================
-- Case E2 (#12670 review adoption): server-replacement staleness isolated
-- from edit staleness. A fresh request that was never invalidated by an
-- edit must still refuse once its owning server instance is replaced.
-- The original caseE replacement refusal was confounded: the same row had
-- already been made stale by an accepted edit, so nothing there proved that
-- replacement alone refuses a still-fresh request. This case drives exactly
-- that scenario - suggest rows are consumed without any intervening edit,
-- one same-server navigation pins the request as fully current, and only
-- then is the server replaced. Refusal afterwards is the layered effect of
-- instance admission plus per-generation session binding (#11108); neither
-- an edit-version nor a cursor-movement check participates.
-- ===========================================================================
do
  local lsp = fresh_module_load()
  local server = make_server("perllsp", SYMBOL_CAPS)
  register(lsp, "perllsp", server)
  local doc = make_doc("C:/proj/e2.pl", { "sub g {}\n" })

  local options = open_symbols(lsp, server, doc, {
    { name = "g", kind = 6, range = range_of(1, 1, 1, 7) },
  })
  if not options then goto continue_e2 end

  local rows = options.suggest("")
  local selections_before = #doc.selections
  options.submit("", rows[1])
  ok(#doc.selections == selections_before + 1,
    "caseE2: a fresh row from the current server navigates before replacement")

  local replacement = make_server("perllsp", SYMBOL_CAPS)
  register(lsp, "perllsp", replacement)
  doc.lsp_open = true
  options.submit("", rows[1])
  ok(#doc.selections == selections_before + 1,
    "caseE2: with no stale edit, a replaced-server row refuses to navigate")

  ::continue_e2::
end

-- ===========================================================================
-- Case F: non-regression and determinism pins. Unique flat symbols keep the
-- exact historical rendering, rows follow source order, and a SymbolInformation
-- single still navigates its exact location.
-- Green on main for rendering/navigation; the order binding pins ipairs
-- traversal (red against a reversed-order mutant).
-- ===========================================================================
do
  local lsp = fresh_module_load()
  local server = make_server("perllsp", SYMBOL_CAPS)
  register(lsp, "perllsp", server)
  local doc = make_doc("C:/proj/f.pl", { "one two three\n" })

  local options = open_symbols(lsp, server, doc, {
    { name = "one", kind = 6, range = range_of(1, 1, 1, 4) },
    { name = "two", kind = 12, range = range_of(1, 5, 1, 8),
      detail = "lexical two" },
    { name = "three", kind = 3, range = range_of(1, 9, 1, 14),
      deprecated = true },
  })
  if not options then goto continue_f end

  local rows = options.suggest("")
  ok(#rows == 3, "caseF: all unique symbols offered")
  if #rows < 3 then goto continue_f end

  ok(rows[1].name == "one||Method" and rows[2].name == "two||Variable"
     and rows[3].name == "three||Function",
    "caseF: unique symbols keep the exact historical rendered keys")
  ok(rows[1].text == "one" and rows[2].text == "two" and rows[3].text == "three",
    "caseF: displayed text strips only the kind segment")
  ok(rows[1].info == "Method" and rows[2].info == "Variable"
     and rows[3].info == "Function",
    "caseF: kind projections stay per-row")
  ok(rows[1].name < rows[2].name or rows[2].name ~= rows[1].name,
    "caseF: row identities remain comparable strings")

  options.submit("", rows[2])
  local sel = last_selection(doc)
  ok(sel and sel.line1 == 1 and sel.col1 == 5,
    "caseF: middle row navigates its exact range start")
  options.submit("", rows[3])
  sel = last_selection(doc)
  ok(sel and sel.line1 == 1 and sel.col1 == 9,
    "caseF: last row navigates its exact range start")

  -- Source-order binding: the second response round-trips rows in the exact
  -- array order the server used, independent of hash iteration.
  local options_two = open_symbols(lsp, server, doc, {
    { name = "zeta", kind = 6, range = range_of(1, 1, 1, 4) },
    { name = "beta", kind = 6, range = range_of(1, 5, 1, 8) },
    { name = "mid", kind = 6, range = range_of(1, 9, 1, 14) },
  })
  if options_two then
    local ordered = options_two.suggest("")
    ok(#ordered == 3 and ordered[1].text == "zeta"
       and ordered[2].text == "beta" and ordered[3].text == "mid",
      "caseF: suggest order binds to numeric source order, not hash order")
  end

  ::continue_f::
end

-- ===========================================================================
-- Case G (#12670 review adoption): hidden disambiguation ordinals never
-- become searchable text. Duplicate rows stay offered and navigable under an
-- empty query, but a query can no longer hit the internal "#N" suffix of a
-- colliding row's key, and no suggestion ever leaks the suffix visibly.
-- Red against a mutant that appends the disambiguation ordinal back into
-- the fuzzy-search subject.
-- ===========================================================================
do
  local lsp = fresh_module_load()
  local server = make_server("perllsp", SYMBOL_CAPS)
  register(lsp, "perllsp", server)
  local doc = make_doc("C:/proj/g.pl", {
    "sub f {}\nsub f {}\nsub step2 {}\n",
  })

  local options = open_symbols(lsp, server, doc, {
    { name = "f", kind = 6, range = range_of(1, 1, 1, 9) },
    { name = "f", kind = 6, range = range_of(2, 1, 2, 9) },
    { name = "step2", kind = 6, range = range_of(3, 1, 3, 13) },
  })
  if not options then goto continue_g end

  local rows = options.suggest("")
  ok(#rows == 3, "caseG: duplicates and unique symbols are all offered")
  if #rows == 3 then
    options.submit("", rows[1])
    local first_nav = last_selection(doc)
    options.submit("", rows[2])
    local second_nav = last_selection(doc)
    ok(first_nav and second_nav and first_nav.line1 ~= second_nav.line1,
      "caseG: duplicated rows still navigate their own ranges")
  end

  local leaked = false
  for _, row in ipairs(rows) do
    leaked = leaked or string.find(row.text, "#", 1, true) ~= nil
  end
  ok(not leaked,
    "caseG: no suggestion text leaks a disambiguation ordinal")

  local hash_rows = options.suggest("#")
  ok(#hash_rows == 0,
    "caseG: querying the ordinal marker matches nothing")

  local digit_rows = options.suggest("2")
  ok(#digit_rows == 1 and digit_rows[1].text == "step2",
    "caseG: a numeric query hits only visibly matching names")

  ::continue_g::
end

print(string.format("%d passed, %d failed", passed, failed))
if failed > 0 then os.exit(1) end

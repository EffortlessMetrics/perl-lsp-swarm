-- Deterministic focused tests for request-subject currentness binding in
-- clients/lite-xl/upstream/init.lua (#11108), consuming the #11115
-- document-session authority.
--
-- Run:
--   lua clients/lite-xl/tests/init_request_currentness_test.lua [path-to-init-module]
-- Default module path is ../upstream/init.lua relative to this file.
--
-- Proof shape: held responses are drained manually against the exact staged
-- module under minimal Lite XL runtime fakes. Tests drive the real public
-- request paths (request_hover/request_signature/request_references/
-- request_completion/request_document_symbols/request_document_format/
-- request_workspace_symbol/request_call_hierarchy, plus the autocomplete
-- select/apply seams) through deterministic generation transitions - edits,
-- close/reopen, Save As-style URI transitions, and full server restarts -
-- and assert that a held response applies exactly when its immutable
-- request subject is still current, and is dropped with a typed disposition
-- otherwise.
--
-- Red-first baseline: run this suite against CURRENT MAIN before this
-- change (origin/main clients/lite-xl/upstream/init.lua, i.e. #12029's
-- merged state):
--
--   lua clients/lite-xl/tests/init_request_currentness_test.lua <main-init.lua>
--
-- There every rejection case MUST fail (main has no admission guard: a
-- held hover/completion/signature/references/symbol/format response still
-- populates tooltips, lists, prompts, or mutates bytes after intervening
-- edits, restarts, close/reopen or URI transitions). The exact-current
-- companion cases may pass on main where upstream already applied current
-- responses; they pin preserved UX.
--
-- Single-behavior mutation falsifiers of the PATCHED module (each verified
-- caught):
--   1. delete the session-generation equality check ->
--      close/reopen supersede cases fail;
--   2. delete the session version/mutation-sequence equality checks ->
--      same-caret-after-edit and stale-format cases fail;
--   3. delete both server-instance and process-generation equality
--      checks -> the workspace/symbol case fails (its subject has no
--      document pinning, so server identity is its only currentness proof;
--      document-bound cases are additionally protected by their session
--      guards, which is itself defense in depth);
--   4. delete the apply-time subject revalidation in the completion
--      select path -> the deferred-completion-edit case fails.
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
local listbox_texts = {}
local listbox_signatures = {}
local autocomplete_calls = {}
local resolve_pushes = {}
local rs_instances = {}

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
      -- Called as a method (core.command_view:enter(prompt, opts)).
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
  return { plugins = {} }
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
  self.edits[#self.edits + 1] = { op = "remove", line1 = line1, col1 = col1, line2 = line2, col2 = col2 }
end
Doc.insert = function(self, line1, col1, text)
  self.edits[#self.edits + 1] = { op = "insert", line1 = line1, col1 = col1, text = text }
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
    show_text = function(text, pos) listbox_texts[#listbox_texts + 1] = text end,
    show_signatures = function(result) listbox_signatures[#listbox_signatures + 1] = result end,
  }
end

-- Local patch (#11172): the staged modules fold their capability
-- advertisement and command projection through the exact manifest source.
package.preload["plugins.lsp.capability_manifest"] = function()
  return dofile(here .. "/../upstream/capability_manifest.lua")
end
package.preload["plugins.lsp.diagnostics"] = function()
  -- Lifecycle seams consumed by init.lua (#11124); inert in this suite,
  -- whose subject is document-session/version behavior, not publications.
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
      rs_instances[#rs_instances + 1] = rs
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

-- ---------------------------------------------------------------------------
-- Module loading with pristine wrapper bases
-- ---------------------------------------------------------------------------

local base_raw_insert = Doc.raw_insert
local base_raw_remove = Doc.raw_remove

local function fresh_module_load()
  Doc.raw_insert = base_raw_insert
  Doc.raw_remove = base_raw_remove
  log_records = {}
  command_view_prompts = {}
  listbox_texts = {}
  listbox_signatures = {}
  autocomplete_calls = {}
  resolve_pushes = {}
  rs_instances = {}
  return dofile(init_module_path)
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
    can_push_value = true,
  }
  function server:can_push() return self.can_push_value end
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
    if method == 'completionItem/resolve' then
      resolve_pushes[#resolve_pushes + 1] = entry
    end
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

---Play one held request callback with a canned response, removing it from
---the queue like a late arrival after other traffic was already drained.
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

local function drain(lsp_server, method)
  local remaining = {}
  local played = {}
  for _, entry in ipairs(lsp_server.outbound) do
    if method == nil or entry.method == method then
      played[#played + 1] = entry
      if entry.callback then entry.callback(lsp_server) end
    else
      remaining[#remaining + 1] = entry
    end
  end
  lsp_server.outbound = remaining
  return played
end

local INCREMENTAL = { textDocumentSync = { openClose = true, change = 2, save = { includeText = false } }, positionEncoding = "utf-16" }

---Admit an open document/session and flush the didOpen notification.
local function open_admitted(lsp, doc, server)
  lsp.open_document(doc)
  drain(server, "textDocument/didOpen")
  doc.lsp_open = true
end

---One accepted edit batch (edit bytes, apply raw mutation, emit+drain V+1).
local function accept_edit(lsp, doc, server, line, col, text)
  doc.lines[line] = (doc.lines[line] or "") .. text
  doc:raw_insert(line, col, text, nil, 0)
  drain(server, "textDocument/didChange")
end

-- ===========================================================================
-- Case 1: hover held at cursor P; edit advances the stream; cursor returns
-- to P; the old hover result is rejected. A fresh post-edit hover applies.
-- ===========================================================================
do
  local lsp = fresh_module_load()
  local server = make_server("perllsp", {
    textDocumentSync = { openClose = true, change = 2, save = { includeText = false } },
    positionEncoding = "utf-16",
    hoverProvider = {},
  })
  register(lsp, "perllsp", server)
  local doc = make_doc("C:/proj/hover.pl", { "my $v;\n" }, 1, 6)

  open_admitted(lsp, doc, server)
  lsp.request_hover(doc, 1, 6, false)
  ok(#server.outbound == 1 and server.outbound[1].method == "textDocument/hover",
    "case1: hover request queued")

  -- Intervening accepted edit advances the stream; caret unchanged.
  accept_edit(lsp, doc, server, 1, 9, " ")
  doc._selection = { line = 1, col = 6 }

  local held = server.outbound[1]
  table.remove(server.outbound, 1)
  held.callback(server, { result = { contents = { value = "stale docs" } } })
  ok(#listbox_texts == 0,
    "case1: stale held hover result never reaches the tooltip")

  -- Fresh post-edit request still applies exactly once.
  lsp.request_hover(doc, 1, 6, false)
  ok(play_response(server, "textDocument/hover",
    { result = { contents = { value = "fresh docs" } } }),
    "case1: fresh hover response played")
  ok(#listbox_texts == 1 and listbox_texts[1] == "fresh docs",
    "case1: current exact-subject hover applies once")
end

-- ===========================================================================
-- Case 2: completion held; bytes edited without moving the final caret;
-- the old completion cannot populate.
-- ===========================================================================
do
  local lsp = fresh_module_load()
  local server = make_server("perllsp", {
    textDocumentSync = { openClose = true, change = 2, save = { includeText = false } },
    positionEncoding = "utf-16",
    completionProvider = {},
  })
  register(lsp, "perllsp", server)
  local doc = make_doc("C:/proj/comp.pl", { "my $x\n" }, 1, 6)

  open_admitted(lsp, doc, server)
  lsp.request_completion(doc, 1, 6, true)
  ok(#server.outbound == 1, "case2: completion request queued")

  accept_edit(lsp, doc, server, 1, 6, "y")
  doc._selection = { line = 1, col = 6 }

  local held = server.outbound[1]
  table.remove(server.outbound, 1)
  held.callback(server, { result = { items = { { label = "stale_fn" } } } })
  ok(#autocomplete_calls == 0,
    "case2: same-caret stale completion cannot populate the box")

  lsp.request_completion(doc, 1, 6, true)
  ok(play_response(server, "textDocument/completion",
    { result = { items = { { label = "fresh_fn" } } } }),
    "case2: fresh completion played")
  ok(#autocomplete_calls == 1, "case2: current completion populates")
  local items = autocomplete_calls[1].items
  ok(items.fresh_fn ~= nil, "case2: completion item carries its label")
end

-- ===========================================================================
-- Case 3: formatting computed for generation G must not mutate G+1; a
-- current formatting response still applies its edits.
-- ===========================================================================
do
  local lsp = fresh_module_load()
  local server = make_server("perllsp", {
    textDocumentSync = { openClose = true, change = 2, save = { includeText = false } },
    positionEncoding = "utf-16",
    documentFormattingProvider = {},
  })
  register(lsp, "perllsp", server)
  local doc = make_doc("C:/proj/fmt.pl", { "print( 1 );\n" })

  open_admitted(lsp, doc, server)
  lsp.request_document_format(doc)
  ok(#server.outbound == 1, "case3: format request queued")

  accept_edit(lsp, doc, server, 1, 11, " ")
  local stale_edits = #doc.edits
  local held = server.outbound[1]
  table.remove(server.outbound, 1)
  held.callback(server, { result = {
    { range = { start = { line = 0, character = 0 }, ["end"] = { line = 0, character = 5 } }, newText = "say" },
  } })
  ok(#doc.edits == stale_edits,
    "case3: stale formatting edits are never applied to newer bytes")

  lsp.request_document_format(doc)
  ok(play_response(server, "textDocument/formatting", { result = {
    { range = { start = { line = 0, character = 0 }, ["end"] = { line = 0, character = 5 } }, newText = "say" },
  } }), "case3: current format response played")
  ok(#doc.edits >= 2, "case3: current formatting applies its edits")
end

-- ===========================================================================
-- Case 4: a delayed response from server generation N cannot affect
-- generation N+1 after an explicit restart.
-- ===========================================================================
do
  local lsp = fresh_module_load()
  local old_server = make_server("perllsp", {
    textDocumentSync = { openClose = true, change = 2, save = { includeText = false } },
    positionEncoding = "utf-16",
    hoverProvider = {},
  })
  register(lsp, "perllsp", old_server)
  local doc = make_doc("C:/proj/gen.pl", { "g\n" }, 1, 2)

  open_admitted(lsp, doc, old_server)
  lsp.request_hover(doc, 1, 2, false)
  local held = old_server.outbound[1]
  table.remove(old_server.outbound, 1)

  lsp.stop_servers()
  local new_server = make_server("perllsp", {
    textDocumentSync = { openClose = true, change = 2, save = { includeText = false } },
    positionEncoding = "utf-16",
    hoverProvider = {},
  })
  register(lsp, "perllsp", new_server)
  open_admitted(lsp, doc, new_server)

  held.callback(old_server, { result = { contents = { value = "dead generation" } } })
  ok(#new_server.outbound == 0,
    "case4: dead-generation callback pushes nothing into the replacement")
  ok(#listbox_texts == 0,
    "case4: delayed N response cannot render under generation N+1")
end

-- ===========================================================================
-- Case 5: navigation subjects do not survive close/reopen of the same URI
-- even though bytes and numeric version reset identically.
-- ===========================================================================
do
  local lsp = fresh_module_load()
  local server = make_server("perllsp", {
    textDocumentSync = { openClose = true, change = 2, save = { includeText = false } },
    positionEncoding = "utf-16",
    documentSymbolProvider = {},
  })
  register(lsp, "perllsp", server)
  local doc = make_doc("C:/proj/nav.pl", { "sub f {}\n" })

  open_admitted(lsp, doc, server)
  lsp.request_document_symbols(doc)
  local held = server.outbound[1]
  table.remove(server.outbound, 1)

  lsp.close_document(doc)
  drain(server, "textDocument/didClose")
  lsp.open_document(doc)
  drain(server, "textDocument/didOpen")
  doc.lsp_open = true

  local symbols_response = { result = {
    { name = "f", kind = 6, range = {
        start = { line = 0, character = 4 }, ["end"] = { line = 0, character = 5 } } },
  } }
  held.callback(server, symbols_response)
  ok(#command_view_prompts == 0,
    "case5: old-session symbol list cannot navigate the reopened session")

  lsp.request_document_symbols(doc)
  ok(play_response(server, "textDocument/documentSymbol", symbols_response),
    "case5: fresh symbol response played")
  ok(#command_view_prompts == 1 and command_view_prompts[1] == "Find Symbol",
    "case5: current-session navigation prompt appears")
end

-- ===========================================================================
-- Case 6: Save As-style URI transition supersedes held reference targets.
-- ===========================================================================
do
  local lsp = fresh_module_load()
  local server = make_server("perllsp", {
    textDocumentSync = { openClose = true, change = 2, save = { includeText = false } },
    positionEncoding = "utf-16",
    hoverProvider = {},
    referencesProvider = {},
  })
  register(lsp, "perllsp", server)
  local doc = make_doc("C:/proj/refs.pl", { "r\n" }, 1, 2)

  open_admitted(lsp, doc, server)
  lsp.request_references(doc, 1, 2)
  local held = server.outbound[1]
  table.remove(server.outbound, 1)

  doc.filename = "C:/proj/refs2.pl"
  lsp.open_document(doc)
  drain(server, "textDocument/didOpen")
  doc.lsp_open = true

  held.callback(server, { result = {
    { uri = "file:///C:/proj/refs.pl",
      range = { start = { line = 0, character = 0 }, ["end"] = { line = 0, character = 1 } } },
  } })
  ok(#command_view_prompts == 0,
    "case6: pre-transition reference targets cannot drive the new session")

  lsp.request_references(doc, 1, 2)
  ok(play_response(server, "textDocument/references", { result = {} }),
    "case6: fresh references response played")
  ok(#command_view_prompts == 0,
    "case6: empty current result keeps prior behavior (no prompt)")
end

-- ===========================================================================
-- Case 7: signature help held across an edit drops instead of showing or
-- falling back; a current signature still shows.
-- ===========================================================================
do
  local lsp = fresh_module_load()
  local server = make_server("perllsp", {
    textDocumentSync = { openClose = true, change = 2, save = { includeText = false } },
    positionEncoding = "utf-16",
    signatureHelpProvider = { triggerCharacters = { "(" } },
  })
  register(lsp, "perllsp", server)
  local doc = make_doc("C:/proj/sig.pl", { "f(\n" }, 1, 4)

  open_admitted(lsp, doc, server)
  lsp.request_signature(doc, 1, 4, true, nil)
  local held = server.outbound[1]
  table.remove(server.outbound, 1)

  accept_edit(lsp, doc, server, 1, 4, ")")
  doc._selection = { line = 1, col = 4 }

  local fallback_called = false
  held.callback(server, {
    fallback_used = nil,
    result = { signatures = { { label = "f($x)" } } },
  })
  -- Fallback detection: a fallback would have pushed a completion request.
  ok(#server.outbound == 0,
    "case7: stale signature neither shows nor falls back")
  ok(#listbox_signatures == 0,
    "case7: stale signature help never displays")

  lsp.request_signature(doc, 1, 4, true, function() fallback_called = true end)
  local sigs_before = #listbox_signatures
  ok(play_response(server, "textDocument/signatureHelp",
    { result = { signatures = { { label = "f($x)" } } } }),
    "case7: current signature played")
  ok(#listbox_signatures == sigs_before + 1,
    "case7: current signature help displays")
  ok(fallback_called == false,
    "case7: current non-empty signature does not invoke the fallback")
end

-- ===========================================================================
-- Case 8: completion-item application revalidates the stored subject at
-- user-selection time, refusing edits computed for older bytes.
-- ===========================================================================
do
  local lsp = fresh_module_load()
  local server = make_server("perllsp", {
    textDocumentSync = { openClose = true, change = 2, save = { includeText = false } },
    positionEncoding = "utf-16",
    completionProvider = {},
  })
  register(lsp, "perllsp", server)
  local doc = make_doc("C:/proj/app.pl", { "fo\n" }, 1, 3)

  open_admitted(lsp, doc, server)
  lsp.request_completion(doc, 1, 3, true)
  ok(play_response(server, "textDocument/completion", { result = {
    items = { { label = "foo", textEdit = {
      range = { start = { line = 0, character = 0 }, ["end"] = { line = 0, character = 2 } },
      newText = "foobar" } } },
  } }), "case8: completion admitted and populated")

  local dv_ok = setmetatable({ doc = doc }, (require "core.docview"))
  require("core").active_view = dv_ok
  doc._selection = { line = 1, col = 3 }

  -- Select while the subject is still current: the edit applies.
  local symbols = autocomplete_calls[#autocomplete_calls]
  ok(symbols ~= nil and symbols.items.foo ~= nil, "case8: foo item present")
  local edits_before_first_select = #doc.edits
  symbols.items.foo.onselect(1, symbols.items.foo)
  ok(#doc.edits > edits_before_first_select,
    "case8: current deferred completion edit applies at selection time")

  -- New completion round from the new caret; then an intervening edit
  -- before selection.
  doc._selection = { line = 1, col = 6 }
  lsp.request_completion(doc, 1, 6, true)
  drain(server, "textDocument/didChange")
  ok(play_response(server, "textDocument/completion", { result = {
    items = { { label = "foobar", textEdit = {
      range = { start = { line = 0, character = 0 }, ["end"] = { line = 0, character = 6 } },
      newText = "foobaz" } } },
  } }), "case8: second completion admitted")
  local edits_before = #doc.edits
  local symbols2 = autocomplete_calls[#autocomplete_calls]
  accept_edit(lsp, doc, server, 1, 7, "!")
  symbols2.items.foobar.onselect(1, symbols2.items.foobar)
  ok(#doc.edits == edits_before,
    "case8: stale deferred completion edit refused at selection time")
  require("core").active_view = nil
end

-- ===========================================================================
-- Case 9: workspace/symbol results are bound to the serving server
-- instance; a replaced process cannot populate the results view.
-- ===========================================================================
do
  local lsp = fresh_module_load()
  local old_server = make_server("perllsp", {
    textDocumentSync = { openClose = true, change = 2, save = { includeText = false } },
    positionEncoding = "utf-16",
    workspaceSymbolProvider = {},
  })
  register(lsp, "perllsp", old_server)
  local doc = make_doc("C:/proj/ws.pl", { "w\n" }, 1, 2)

  open_admitted(lsp, doc, old_server)
  lsp.request_workspace_symbol(doc, "query1")
  local held = old_server.outbound[1]
  table.remove(old_server.outbound, 1)

  lsp.stop_servers()
  local new_server = make_server("perllsp", {
    textDocumentSync = { openClose = true, change = 2, save = { includeText = false } },
    positionEncoding = "utf-16",
    workspaceSymbolProvider = {},
  })
  register(lsp, "perllsp", new_server)
  open_admitted(lsp, doc, new_server)

  held.callback(old_server, { result = {
    { name = "old_sym", kind = 6, location = { uri = "file:///C:/proj/ws.pl",
      range = { start = { line = 0, character = 0 }, ["end"] = { line = 0, character = 1 } } } },
  } })
  ok(#rs_instances == 0 or #rs_instances[1].results_added == 0,
    "case9: replaced generation cannot populate workspace results")

  lsp.request_workspace_symbol(doc, "query2")
  ok(play_response(new_server, "workspace/symbol", { result = {
    { name = "new_sym", kind = 6, location = { uri = "file:///C:/proj/ws.pl",
      range = { start = { line = 0, character = 0 }, ["end"] = { line = 0, character = 1 } } } },
  } }), "case9: current generation response played")
  ok(#rs_instances >= 2 and #rs_instances[2].results_added == 1,
    "case9: current workspace results populate their own view")
end

-- ===========================================================================
-- Case 10: rename is projection-gated (#11172). The server advertising
-- renameProvider is not enough: with no client application consumer (#8986)
-- the request is never sent, so stale-versus-current response semantics
-- cannot even arise at this sender.
-- ===========================================================================
do
  local lsp = fresh_module_load()
  local server = make_server("perllsp", {
    textDocumentSync = { openClose = true, change = 2, save = { includeText = false } },
    positionEncoding = "utf-16",
    renameProvider = {},
  })
  register(lsp, "perllsp", server)
  local doc = make_doc("C:/proj/ren.pl", { "rn\n" }, 1, 3)

  open_admitted(lsp, doc, server)
  accept_edit(lsp, doc, server, 1, 3, "!")

  lsp.request_symbol_rename(doc, 1, 3, "new_name")
  ok(#server.outbound == 0,
    "case10: gated rename sends nothing under an unsupported client row")

  local saw_owner_message = false
  for _, record in ipairs(log_records) do
    if tostring(record):find("#8986", 1, true) then
      saw_owner_message = true
    end
  end
  ok(saw_owner_message,
    "case10: one explicit refusal message names the rename owner")
end

-- ===========================================================================
-- Case 11: prepareCallHierarchy is projection-gated (#11172). The server
-- advertising callHierarchyProvider is not enough: with no client result
-- consumer (#10719) the request is never sent and one explicit message
-- explains why.
-- ===========================================================================
do
  local lsp = fresh_module_load()
  local server = make_server("perllsp", {
    textDocumentSync = { openClose = true, change = 2, save = { includeText = false } },
    positionEncoding = "utf-16",
    callHierarchyProvider = {},
  })
  register(lsp, "perllsp", server)
  local doc = make_doc("C:/proj/ch.pl", { "c\n" }, 1, 2)

  open_admitted(lsp, doc, server)
  lsp.request_call_hierarchy(doc, 1, 2)
  ok(#server.outbound == 0,
    "case11: gated call hierarchy sends nothing under an unsupported row")

  local saw_owner_message = false
  for _, record in ipairs(log_records) do
    if tostring(record):find("#10719", 1, true) then
      saw_owner_message = true
    end
  end
  ok(saw_owner_message,
    "case11: one explicit refusal message names the call-hierarchy owner")
end

-- ===========================================================================
-- Case 12 (#9019): textDocument/references is gated by referencesProvider,
-- not hoverProvider, across all four mixed-capability combinations, and a
-- first active server without referencesProvider cannot make a later valid
-- references provider unreachable by iteration order.
-- ===========================================================================
do
  local SYNC = {
    textDocumentSync = { openClose = true, change = 2, save = { includeText = false } },
    positionEncoding = "utf-16",
  }
  local function merge_caps(base, extra)
    local caps = {}
    for k, v in pairs(base) do caps[k] = v end
    for k, v in pairs(extra) do caps[k] = v end
    return caps
  end

  -- hover=true, references=true -> request sent.
  local lsp = fresh_module_load()
  local both = make_server("perllsp", merge_caps(SYNC, { hoverProvider = {}, referencesProvider = {} }))
  register(lsp, "perllsp", both)
  local doc = make_doc("C:/proj/r1.pl", { "r\n" }, 1, 2)
  open_admitted(lsp, doc, both)
  lsp.request_references(doc, 1, 2)
  ok(#both.outbound == 1 and both.outbound[1].method == "textDocument/references",
    "case12: hover+references server receives the references request")
  ok(both.outbound[1].params.context.includeDeclaration == true,
    "case12: references request preserves context.includeDeclaration = true")

  -- hover=false, references=true -> request sent.
  lsp = fresh_module_load()
  local refs_only = make_server("perllsp", merge_caps(SYNC, { referencesProvider = {} }))
  register(lsp, "perllsp", refs_only)
  doc = make_doc("C:/proj/r2.pl", { "r\n" }, 1, 2)
  open_admitted(lsp, doc, refs_only)
  lsp.request_references(doc, 1, 2)
  ok(#refs_only.outbound == 1 and refs_only.outbound[1].method == "textDocument/references",
    "case12: references-only server (no hover) still receives the request")

  -- hover=true, references=false -> request not sent.
  lsp = fresh_module_load()
  local hover_only = make_server("perllsp", merge_caps(SYNC, { hoverProvider = {} }))
  register(lsp, "perllsp", hover_only)
  doc = make_doc("C:/proj/r3.pl", { "r\n" }, 1, 2)
  open_admitted(lsp, doc, hover_only)
  lsp.request_references(doc, 1, 2)
  ok(#hover_only.outbound == 0,
    "case12: hover-only server receives no references request (masked false positive)")

  -- hover=false, references=false -> request not sent.
  lsp = fresh_module_load()
  local none = make_server("perllsp", merge_caps(SYNC, {}))
  register(lsp, "perllsp", none)
  doc = make_doc("C:/proj/r4.pl", { "r\n" }, 1, 2)
  open_admitted(lsp, doc, none)
  lsp.request_references(doc, 1, 2)
  ok(#none.outbound == 0,
    "case12: server with neither capability receives no references request")

  -- Complementary ungrouped servers: the hover-only server is registered
  -- first, the references-only server second; iteration order must not let
  -- the first server block the second. One open_document broadcast admits
  -- both servers.
  local trial_count = 8
  local successful_trials = 0
  for _ = 1, trial_count do
    lsp = fresh_module_load()
    local first = make_server("hoverfirst", merge_caps(SYNC, { hoverProvider = {} }))
    local second = make_server("refssecond", merge_caps(SYNC, { referencesProvider = {} }))
    local first_capability_checks = 0
    setmetatable(first.capabilities, {
      __index = function(_, key)
        if key == "referencesProvider" then
          first_capability_checks = first_capability_checks + 1
        end
      end,
    })
    register(lsp, "hoverfirst", first)
    register(lsp, "refssecond", second)
    -- Pin the active-server order so the regression is discriminating: the
    -- pre-fix unconditional break must encounter the hover-only server first.
    lsp.get_active_servers = function()
      return { "hoverfirst", "refssecond" }
    end
    doc = make_doc("C:/proj/r5.pl", { "r\n" }, 1, 2)
    lsp.open_document(doc)
    drain(first, "textDocument/didOpen")
    drain(second, "textDocument/didOpen")
    doc.lsp_open = true
    lsp.request_references(doc, 1, 2)
    if #first.outbound == 0
      and #second.outbound == 1
      and second.outbound[1].method == "textDocument/references" then
      successful_trials = successful_trials + 1
    end
    ok(first_capability_checks > 0,
      "case12: ordered trial inspects the non-provider server before the provider")
  end
  ok(successful_trials == trial_count,
    "case12: references-capable second server remains reachable in every complementary-server trial")
end

print(string.format("%d passed, %d failed", passed, failed))
os.exit(failed == 0 and 0 or 1)

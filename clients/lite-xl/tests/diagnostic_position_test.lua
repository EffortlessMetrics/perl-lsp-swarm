-- Deterministic focused tests for diagnostic-range presentation through
-- live documents and negotiated position coordinates (#11128), consuming
-- the #11124 generation-bound publication store.
--
-- Run:
--   lua clients/lite-xl/tests/diagnostic_position_test.lua
--     [path-to-init-module] [path-to-diagnostics-module]
-- Defaults are ../upstream/init.lua and ../upstream/diagnostics.lua.
--
-- Proof shape: resolve_range is exercised against exact byte fixtures
-- (ASCII, non-BMP surrogate pairs, multi-byte BMP, CRLF, multiline,
-- zero-length, out-of-bounds, malformed, unsupported encodings) and the
-- delayed lintplus rendering path is driven through an editor resolver
-- bundle with intervening edits and session transitions.
--
-- Red-first baseline: against PRISTINE upstream diagnostics.lua @ d1432ae,
-- the non-BMP cases reproduce the issue's required falsifier - omitting
-- the Doc treats UTF-16 code units as Lite XL byte columns, so markers land
-- at wrong positions after any surrogate pair; pristine also has no typed
-- range/encoding/closed-document dispositions.
--
-- Single-behavior mutation falsifiers of the PATCHED module (each verified
-- caught):
--   1. delete the utf-16 conversion call (pass raw characters) ->
--      every non-BMP/BMP exactness case fails;
--   2. delete the unsupported-encoding rejection -> utf-32 fixtures fail;
--   3. delete the closed-document column_not_proven disposition ->
--      closed-file cases fail;
--   4. delete the delayed-render subject revalidation -> a moved
--      session's timer still renders stale content.
--
-- No framework: plain soft asserts, one process, deterministic, exit code
-- carries the result. Compatible with the Lite XL Lua runtime family
-- (Lua 5.4).

local init_module_path = arg and arg[1] or nil
local diag_module_path = arg and arg[2] or nil

if not init_module_path then
  local here0 = debug.getinfo(1, "S").source:sub(2):match("^(.*)[/\\]") or "."
  init_module_path = here0 .. "/../upstream/init.lua"
end
if not diag_module_path then
  local here0 = debug.getinfo(1, "S").source:sub(2):match("^(.*)[/\\]") or "."
  diag_module_path = here0 .. "/../upstream/diagnostics.lua"
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
local timers = {}
local lintplus_calls = {}

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
    root_view = {
      get_active_node_default = function() return { add_view = function() end } end,
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
    clamp = function(v, lo, hi)
      if v < lo then return lo elseif v > hi then return hi end
      return v
    end,
    fuzzy_match = function(list) return list or {} end,
    normalize_path = function(path) return path end,
  }
end

package.preload["core.config"] = function()
  return { plugins = {} }
end

package.preload["plugins.lintplus"] = function()
  return {
    messages = {},
    add_message = function(fname, line, col, kind, text)
      lintplus_calls[#lintplus_calls + 1] =
        { fname = fname, line = line, col = col, kind = kind, text = text }
    end,
    clear_messages = function() end,
    init_doc = function() end,
  }
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
  return { can_complete = function() return true end, complete = function() end, close = function() end }
end

local Doc = { __index = Doc }
Doc.raw_insert = function() end
Doc.raw_remove = function() end
Doc.get_selection = function(self)
  local s = self._selection or { line = 1, col = 1 }
  return s.line, s.col, s.line, s.col
end
Doc.get_char = function() return "" end
Doc.set_selection = function(self, line1, col1)
  self.selections[#self.selections + 1] = { line1 = line1, col1 = col1 }
end
Doc.remove = function() end
Doc.insert = function() end
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
  return { hide = function() end, show_text = function() end, show_signatures = function() end }
end

local diag_under_test = nil
package.preload["plugins.lsp.diagnostics"] = function()
  return dofile(diag_module_path)
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
  return function(interval, one_shot)
    local t = {
      on_timer = nil,
      start = function() end,
      stop = function() end,
      reset = function() end,
      running = function() return false end,
    }
    timers[#timers + 1] = t
    return t
  end
end

package.preload["plugins.lsp.symbolresults"] = function()
  local SymbolResults = {}
  setmetatable(SymbolResults, {
    __call = function(class, query)
      local rs = {
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
  return function(title) return { title = title, set_text = function() end } end
end

---Real UTF-8 iteration shim matching lite-xl's utf8extra surface: next(s,i)
---yields the 1-based BYTE position of each character start plus its
---codepoint.
utf8extra = {
  len = function(s) return utf8.len(s) or #s end,
  next = function(s, i)
    local pos
    if not i then
      pos = 1
    else
      local b = s:byte(i)
      local width = 1
      if b >= 0xF0 then width = 4 elseif b >= 0xE0 then width = 3
      elseif b >= 0xC0 then width = 2 end
      pos = i + width
    end
    if pos > #s then return nil end
    return pos, utf8.codepoint(s, pos)
  end,
}

PLATFORM = "Windows"
USERDIR = "."
SCALE = 1
renderer = { font = { load = function() return {} end } }

config = require("core.config")
config.plugins = config.plugins or {}
config.plugins.lsp = config.plugins.lsp or {}
config.plugins.lsp.show_diagnostics = true
config.plugins.lsp.diagnostics_delay = 500

system = {
  exec = function() error("shell invoked", 0) end,
  raise_window = function() end,
  get_file_info = function() return { size = 100 } end,
}

-- ---------------------------------------------------------------------------
-- Loading helpers
-- ---------------------------------------------------------------------------

local base_raw_insert = Doc.raw_insert
local base_raw_remove = Doc.raw_remove

local function fresh_module_load()
  Doc.raw_insert = base_raw_insert
  Doc.raw_remove = base_raw_remove
  package.loaded["plugins.lsp.diagnostics"] = nil
  timers = {}
  lintplus_calls = {}
  log_records = {}
  return dofile(init_module_path)
end

local function load_diagnostics_module()
  timers = {}
  lintplus_calls = {}
  log_records = {}
  return dofile(diag_module_path)
end

local path_util = dofile(here .. "/../upstream/util.lua")
local function platform_path(path)
  return path_util.uri_to_path(path_util.path_to_uri(path))
end

local URI = "file:///C:/proj/app.pl"

---Absence-tolerant wrappers: against the pristine module these reproduce
---pre-patch presentation behavior exactly (raw UTF-16 columns applied
---without any Doc), so the issue's surrogate-pair falsifier manifests as a
---soft failure instead of a harness crash.
local function R(diag, range, doc, encoding)
  if diag.resolve_range then
    return diag.resolve_range(range, doc, encoding)
  end
  local l1, c1, l2, c2 = path_util.toselection(range)
  return l1, c1, l2, c2, nil
end

local function PUBLISH(diag, subject, params)
  if diag.publish then
    return select(1, diag.publish(subject, params))
  end
  local fname = path_util.uri_to_path(params.uri)
  if params.diagnostics and #params.diagnostics > 0 then
    return diag.add(fname, params.diagnostics)
  end
  diag.clear(fname)
  return true
end

local function INSTALL_RESOLVER(diag, state)
  if diag.set_render_resolver then
    diag.set_render_resolver(
      function(uri)
        if state.live_uri == uri then return state.doc end
        return nil
      end,
      function(uri, provider, sg, version)
        return state.current_sg == sg and state.current_version == version
          and state.live_uri == uri
      end
    )
  end
end

-- ===========================================================================
-- PART A: resolve_range coordinate authority
-- ===========================================================================

local function make_doc(lines)
  return setmetatable({
    filename = "C:/proj/app.pl",
    abs_filename = "C:/proj/app.pl",
    lines = lines,
    _selection = { line = 1, col = 1 },
    edits = {},
    selections = {},
  }, { __index = Doc })
end

local function rng(sl, sc, el, ec)
  return { start = { line = sl, character = sc }, ["end"] = { line = el, character = ec } }
end

do
  local diag = load_diagnostics_module()

  -- ASCII before diagnostic: identity mapping. An end position one past
  -- the final content character clamps onto the newline byte, matching the
  -- pre-existing upstream conversion convention (lite-xl lines include \n).
  local doc = make_doc({ "my $x = 1;\n" })
  local l1, c1, l2, c2, disp = R(diag, rng(0, 9, 0, 10), doc, "utf-16")
  ok(l1 == 1 and c1 == 10 and l2 == 1 and c2 == 11 and disp == nil,
    "A: ASCII range resolves to exact byte columns")

  -- Non-BMP character before diagnostic on same line: LSP counts the
  -- surrogate pair as two UTF-16 units; editor bytes are four.
  doc = make_doc({ "\226\136\180\226\136\180 abc\n" }) -- not used; real fixture below
  doc = make_doc({ "\240\159\166\128abc\n" }) -- U+1F99F crab = 4 bytes, 2 units
  l1, c1 = R(diag, rng(0, 2, 0, 3), doc, "utf-16")
  ok(l1 == 1 and c1 == 5 and c1 ~= nil,
    "A: non-BMP before range lands at exact byte column (5)")

  -- Multiple non-BMP characters before diagnostic.
  doc = make_doc({ "\240\159\166\128\240\159\166\128x\n" }) -- two crab = 8 bytes, 4 units
  l1, c1 = R(diag, rng(0, 4, 0, 5), doc, "utf-16")
  ok(l1 == 1 and c1 == 9,
    "A: multiple non-BMP prefixes accumulate to byte column 9")

  -- Multi-byte BMP character: two UTF-8 bytes but one UTF-16 unit, so
  -- columns after it shift by exactly one editor byte.
  doc = make_doc({ "caf\195\169 x\n" }) -- caf<e-acute> x ; e-acute = 2 bytes, 1 unit
  l1, c1 = R(diag, rng(0, 5, 0, 6), doc, "utf-16")
  ok(l1 == 1 and c1 == 7,
    "A: BMP multibyte prefix shifts byte column by its extra byte")

  -- CRLF document: \r stays part of the line bytes; LSP columns count it.
  doc = make_doc({ "abc\r\n" })
  l1, c1 = R(diag, rng(0, 1, 0, 2), doc, "utf-16")
  ok(l1 == 1 and c1 == 2,
    "A: CRLF document does not shift LSP column meaning")

  -- Range spanning lines.
  doc = make_doc({ "alpha\n", "beta\n" })
  l1, c1, l2, c2 = R(diag, rng(0, 2, 1, 2), doc, "utf-16")
  ok(l1 == 1 and c1 == 3 and l2 == 2 and c2 == 3,
    "A: multiline range endpoints both convert")

  -- Zero-length diagnostic range.
  doc = make_doc({ "hello\n" })
  l1, c1, l2, c2, disp = R(diag, rng(0, 2, 0, 2), doc, "utf-16")
  ok(l1 == 1 and c1 == 3 and l2 == 1 and c2 == 3 and disp == nil,
    "A: zero-length range resolves to a point")

  -- Invalid/out-of-bounds ranges receive typed failures, no placement.
  doc = make_doc({ "hi\n" })
  _, _, _, _, disp = R(diag, rng(5, 0, 6, 0), doc, "utf-16")
  ok(disp == "range_out_of_bounds", "A: beyond-last-line range fails typed")
  _, _, _, _, disp = R(diag, rng(0, 4, 0, 2), doc, "utf-16")
  ok(disp == "range_out_of_bounds", "A: start-after-end range fails typed")
  _, _, _, _, disp = R(diag, { start = {}, ["end"] = {} }, doc, "utf-16")
  ok(disp == "malformed_range", "A: malformed range fails typed")

  -- Unsupported negotiated encoding fails instead of applying raw columns.
  _, _, _, _, disp = R(diag, rng(0, 0, 0, 1), doc, "utf-32")
  ok(disp == "unsupported_encoding",
    "A: unsupported encoding fails typed, never applies raw positions")

  -- utf-8 negotiation passes validated byte columns straight through.
  doc = make_doc({ "\240\159\166\128ab\n" })
  l1, c1, l2, c2, disp = R(diag, rng(0, 4, 0, 5), doc, "utf-8")
  ok(disp == nil and l1 == 1 and c1 == 5,
    "A: utf-8 negotiation consumes byte columns directly")

  -- utf-8 endpoints are validated against live line bytes: the newline
  -- byte itself is addressable, anything past it fails typed (#11128).
  doc = make_doc({ "hi\n" })
  l1, c1, _, _, disp = R(diag, rng(0, 2, 0, 2), doc, "utf-8")
  ok(disp == nil and l1 == 1 and c1 == 3,
    "A: utf-8 endpoint on the newline byte stays proven")
  _, _, _, _, disp = R(diag, rng(0, 3, 0, 3), doc, "utf-8")
  ok(disp == "range_out_of_bounds",
    "A: utf-8 column past the line bytes fails typed")

  -- Nil encoding takes the protocol default (utf-16) instead of failing:
  -- the default constant must be in scope inside resolve_range.
  doc = make_doc({ "hi\n" })
  l1, c1, _, _, disp = R(diag, rng(0, 0, 0, 1), doc)
  ok(disp == nil and l1 == 1 and c1 == 1,
    "A: nil encoding resolves through the protocol default")

  -- Closed document: line identity kept, columns explicitly unproven.
  l1, c1, l2, c2, disp = R(diag, rng(3, 7, 3, 9), nil, "utf-16")
  ok(l1 == 4 and c1 == nil and l2 == 4 and c2 == nil
    and disp == "column_not_proven",
    "A: closed document keeps line identity with not-proven columns")
end

print(string.format("PART A: %d passed, %d failed", passed, failed))
local part_a_failed, part_a_passed = failed, passed
passed, failed = 0, 0

-- ===========================================================================
-- PART B: delayed rendering resolves live subjects at execution time
-- ===========================================================================

local function publish_current(diag, opts)
  opts = opts or {}
  diag.note_provider("perllsp", 1)
  return PUBLISH(diag, {
    provider = "perllsp",
    generation = 1,
    has_session = true,
    session_generation = opts.session_generation or 1,
    version = opts.version or 0,
    position_encoding = opts.encoding,
  }, {
    uri = URI,
    version = opts.version or 0,
    diagnostics = opts.messages,
  })
end

local install_resolver = INSTALL_RESOLVER

do
  -- Non-BMP exactness through the delayed inline path: the surrogate pair
  -- falsifier - without the Doc the marker would land at column 3 instead
  -- of byte column 5.
  local diag = load_diagnostics_module()
  local doc = make_doc({ "\240\159\166\128abc\n" })
  local state = { doc = doc, live_uri = URI, current_sg = 1, current_version = 0 }
  install_resolver(diag, state)

  ok(publish_current(diag, {
    encoding = "utf-16",
    messages = { { range = rng(0, 2, 0, 3), message = "after crab", severity = 1 } },
  }) == true, "B: publication admitted")

  diag.lintplus_populate_delayed(platform_path("C:/proj/app.pl"))
  timers[#timers].on_timer()
  ok(#lintplus_calls == 1 and lintplus_calls[1].col == 5,
    "B: inline marker after non-BMP char sits at exact byte column 5")
end

do
  -- Dirty open buffer: resolution uses LIVE editor bytes, never disk.
  local diag = load_diagnostics_module()
  local doc = make_doc({ "\226\136\160\226\136\160ab\n" }) -- placeholder replaced below
  doc.lines[1] = "\240\159\166\128abcdef\n" -- live dirty bytes differ from disk fiction
  local state = { doc = doc, live_uri = URI, current_sg = 1, current_version = 3 }
  install_resolver(diag, state)

  ok(publish_current(diag, { version = 3, encoding = "utf-16",
    messages = { { range = rng(0, 2, 0, 3), message = "dirty", severity = 1 } },
  }) == true, "B: dirty-buffer publication admitted")

  diag.lintplus_populate_delayed(platform_path("C:/proj/app.pl"))
  timers[#timers].on_timer()
  ok(#lintplus_calls == 1 and lintplus_calls[1].col == 5,
    "B: open dirty document resolves through live bytes")
end

do
  -- Timer scheduled, then the session moves: firing renders nothing.
  local diag = load_diagnostics_module()
  local doc = make_doc({ "stale target\n" })
  local state = { doc = doc, live_uri = URI, current_sg = 1, current_version = 0 }
  install_resolver(diag, state)

  publish_current(diag, {
    messages = { { range = rng(0, 0, 0, 5), message = "will move", severity = 1 } },
  })
  diag.lintplus_populate_delayed(platform_path("C:/proj/app.pl"))

  -- Session advanced between scheduling and firing.
  state.current_version = 1
  timers[#timers].on_timer()
  ok(#lintplus_calls == 0,
    "B: moved subject's delayed timer renders nothing")
end

do
  -- Unsupported encoding publications never render raw columns.
  local diag = load_diagnostics_module()
  local doc = make_doc({ "abc\n" })
  local state = { doc = doc, live_uri = URI, current_sg = 1, current_version = 0 }
  install_resolver(diag, state)

  publish_current(diag, { encoding = "utf-32",
    messages = { { range = rng(0, 0, 0, 1), message = "utf32 world", severity = 1 } },
  })
  diag.lintplus_populate_delayed(platform_path("C:/proj/app.pl"))
  timers[#timers].on_timer()
  ok(#lintplus_calls == 0,
    "B: unsupported encoding renders nothing rather than raw columns")
end

print(string.format("PART B: %d passed, %d failed", passed, failed))
failed = failed + part_a_failed
print(string.format("TOTAL: %d passed, %d failed", part_a_passed + passed, failed))
os.exit(failed == 0 and 0 or 1)

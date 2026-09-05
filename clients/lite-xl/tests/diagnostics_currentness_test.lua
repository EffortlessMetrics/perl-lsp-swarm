-- Deterministic focused tests for generation-bound push-diagnostics
-- currentness in clients/lite-xl/upstream/diagnostics.lua plus its
-- publishDiagnostics wiring in clients/lite-xl/upstream/init.lua (#11124),
-- consuming the #11115 document-session authority and #11108 admission
-- style.
--
-- Run:
--   lua clients/lite-xl/tests/diagnostics_currentness_test.lua
--     [path-to-init-module] [path-to-diagnostics-module]
-- Defaults are ../upstream/init.lua and ../upstream/diagnostics.lua.
--
-- Proof shape: Part A drives the exact staged diagnostics module through
-- deterministic publication subjects (provider + process generation +
-- document-session + version) and asserts what remains visible in the
-- projected store. Part B loads the exact staged init.lua with the real
-- staged diagnostics module injected, captures the registered
-- textDocument/publishDiagnostics listener, and replays held publications
-- across edits, close/reopen, Save As transitions and full server restarts.
--
-- Red-first baseline: run against the PRISTINE upstream diagnostics.lua
-- @ d1432ae0736cd9531798b4bc1221835f534cc689 (blob c06bec49):
--
--   lua clients/lite-xl/tests/diagnostics_currentness_test.lua <main-init.lua> <pristine-diagnostics.lua>
--
-- There every stale/future/old-generation rejection case MUST fail: the
-- pristine store is anonymous filename state where any later add() or
-- clear() replaces wholesale, so delayed older versions overwrite or clear
-- newer sets, losing providers erase winners by filename collision, and
-- nothing binds publications to generations.
--
-- Single-behavior mutation falsifiers of the PATCHED module (each verified
-- caught):
--   1. delete the stale/future version comparison -> delayed v2
--      non-empty and empty publications overwrite/clear v3;
--   2. delete the provider-generation equality check -> a replaced
--      process publishes into the replacement's view;
--   3. delete the closed-session clearing-only rule -> post-close
--      non-empty content resurrects;
--   4. delete the lifecycle cleanup removal in close_session -> a
--      cleaned-up subject's delayed timer still renders and close leaves
--      retained messages.
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
    command_view = {
      enter = function(_, prompt) log_records[#log_records + 1] = "prompt:" .. tostring(prompt) end,
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
    clamp = function(v, lo, hi) if v < lo then return lo elseif v > hi then return hi end return v end,
    fuzzy_match = function() return {} end,
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
    clear_messages = function(_) end,
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
  return {
    can_complete = function() return true end,
    complete = function() end,
    close = function() end,
  }
end

local Doc = { __index = Doc }
Doc.raw_insert = function() end
Doc.raw_remove = function() end
Doc.get_selection = function(self)
  local s = self._selection or { line = 1, col = 1 }
  return s.line, s.col, s.line, s.col
end
Doc.get_char = function() return "" end
Doc.set_selection = function() end
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

---The diagnostics module under test is injectable: default is the staged
---patched module; red-first runs pass the pristine blob path instead.
local diag_under_test = nil
-- Local patch (#11172): the staged modules fold their capability
-- advertisement and command projection through the exact manifest source.
package.preload["plugins.lsp.capability_manifest"] = function()
  return dofile(here .. "/../upstream/capability_manifest.lua")
end
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
      started = 0,
      stopped_count = 0,
      start = function(self) self.started = self.started + 1 end,
      stop = function(self) self.stopped_count = self.stopped_count + 1 end,
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

utf8extra = { len = function(s) return utf8.len(s) or #s end }

config = require "core.config"
config.plugins = config.plugins or {}
config.plugins.lsp = config.plugins.lsp or {}
config.plugins.lsp.show_diagnostics = true
config.plugins.lsp.diagnostics_delay = 500

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
-- Loading helpers
-- ---------------------------------------------------------------------------

local base_raw_insert = Doc.raw_insert
local base_raw_remove = Doc.raw_remove

local function load_diagnostics_module()
  timers = {}
  lintplus_calls = {}
  log_records = {}
  return dofile(diag_module_path)
end

local function fresh_module_load()
  Doc.raw_insert = base_raw_insert
  Doc.raw_remove = base_raw_remove
  -- Force the diagnostics preload to re-execute so every case gets a fresh
  -- store instance of the module under test.
  package.loaded["plugins.lsp.diagnostics"] = nil
  timers = {}
  lintplus_calls = {}
  log_records = {}
  return dofile(init_module_path)
end

-- ---------------------------------------------------------------------------
-- Subject/publication helpers
-- ---------------------------------------------------------------------------

local URI = "file:///C:/proj/app.pl"

local function subject(provider, generation, opts)
  opts = opts or {}
  return {
    provider = provider or "perllsp",
    generation = generation or 1,
    has_session = opts.has_session ~= false,
    session_generation = opts.session_generation or 1,
    version = opts.version or 0,
  }
end

local function pub(diag, d, params)
  if diag.publish then
    return diag.publish(subject(d.provider, d.generation, d), params)
  end
  -- Pristine fallback: emulate the upstream anonymous-filename listener so
  -- red-baseline assertions exercise real pristine behavior.
  local fname = dofile(here .. "/../upstream/util.lua").uri_to_path(params.uri)
  if params.diagnostics and #params.diagnostics > 0 then
    return diag.add(fname, params.diagnostics), nil
  end
  diag.clear(fname)
  return true, "cleared"
end

---Absence-tolerant wrappers: the pristine module has none of the
---generation-bound API, which is itself the discriminating red baseline.
local function note_provider(diag, name, generation)
  if diag.note_provider then return diag.note_provider(name, generation) end
end

local function close_session(diag, uri, sg)
  if diag.close_session then return diag.close_session(uri, sg) end
end

local function retire_provider(diag, name)
  if diag.retire_provider then return diag.retire_provider(name) end
end

---Filename exactly as production derives it from a document path
---(path_to_uri -> project_absolute_path -> listener uri_to_path round trip).
local path_util = dofile(here .. "/../upstream/util.lua")
---Wire URI exactly as production derives it from a document path.
local function wire_uri(path)
  return path_util.path_to_uri(path)
end

local function platform_path(path)
  return path_util.uri_to_path(path_util.path_to_uri(path))
end

local function visible_messages(diag, filename)
  return diag.get(platform_path(filename)) or {}
end

-- ===========================================================================
-- PART A: diagnostics module semantics
-- ===========================================================================

-- Case A1: versioned publications admitted against the owning stream;
-- a delayed older NON-EMPTY publication cannot replace a newer set.
do
  local diag = load_diagnostics_module()
  note_provider(diag, "perllsp", 1)

  ok(select(1, pub(diag, { generation = 1, version = 3 },
    { uri = URI, version = 3, diagnostics = {
      { range = { start = { line = 0, character = 0 }, ["end"] = { line = 0, character = 4 } }, message = "v3 problem", severity = 1 },
    } })) == true, "A1: current version 3 publication accepted")

  local accepted, disposition = pub(diag, { generation = 1, version = 3 },
    { uri = URI, version = 2, diagnostics = {
      { range = { start = { line = 5, character = 0 }, ["end"] = { line = 5, character = 4 } }, message = "stale v2 problem" },
    } })
  ok(accepted == false, "A1: delayed version 2 non-empty publication rejected")
  ok(disposition == "stale_version", "A1: rejection carries typed stale_version")

  local visible = visible_messages(diag, "C:/proj/app.pl")
  ok(#visible == 1 and visible[1].message == "v3 problem",
    "A1: version 3 set remains visible")
end

-- Case A2: a delayed older EMPTY publication cannot clear a newer set.
do
  local diag = load_diagnostics_module()
  note_provider(diag, "perllsp", 1)

  pub(diag, { generation = 1, version = 3 },
    { uri = URI, version = 3, diagnostics = {
      { range = { start = { line = 0, character = 0 }, ["end"] = { line = 0, character = 1 } }, message = "keep me" },
    } })
  local accepted, disposition = pub(diag, { generation = 1, version = 3 },
    { uri = URI, version = 2, diagnostics = {} })
  ok(accepted == false and disposition == "stale_version",
    "A2: stale empty publication cannot clear the current set")
  ok(#visible_messages(diag, "C:/proj/app.pl") == 1,
    "A2: current set survives the stale empty attempt")
end

-- Case A3: an exact-current EMPTY publication clears.
do
  local diag = load_diagnostics_module()
  note_provider(diag, "perllsp", 1)

  pub(diag, { generation = 1, version = 2 },
    { uri = URI, version = 2, diagnostics = {
      { range = { start = { line = 0, character = 0 }, ["end"] = { line = 0, character = 1 } }, message = "fixable" },
    } })
  ok(#visible_messages(diag, "C:/proj/app.pl") == 1, "A3: set visible first")
  local accepted = select(1, pub(diag, { generation = 1, version = 2 },
    { uri = URI, version = 2, diagnostics = {} }))
  ok(accepted == true, "A3: current exact empty publication accepted")
  ok(#visible_messages(diag, "C:/proj/app.pl") == 0,
    "A3: current set clears through an accepted empty publication")
end

-- Case A4: future/impossible versions are protocol failures, never
-- treated as the latest state.
do
  local diag = load_diagnostics_module()
  note_provider(diag, "perllsp", 1)

  pub(diag, { generation = 1, version = 3 },
    { uri = URI, version = 3, diagnostics = {
      { range = { start = { line = 0, character = 0 }, ["end"] = { line = 0, character = 1 } }, message = "anchor" },
    } })
  local accepted, disposition = pub(diag, { generation = 1, version = 3 },
    { uri = URI, version = 9, diagnostics = {
      { range = { start = { line = 1, character = 0 }, ["end"] = { line = 1, character = 1 } }, message = "from the future" },
    } })
  ok(accepted == false and disposition == "future_version",
    "A4: future version explicitly failed, not accepted as latest")
  local visible = visible_messages(diag, "C:/proj/app.pl")
  ok(#visible == 1 and visible[1].message == "anchor",
    "A4: anchored current set intact")
end

-- Case A5: a replaced process generation cannot publish after restart.
do
  local diag = load_diagnostics_module()
  note_provider(diag, "perllsp", 1)
  pub(diag, { generation = 1, version = 1 },
    { uri = URI, version = 1, diagnostics = {
      { range = { start = { line = 0, character = 0 }, ["end"] = { line = 0, character = 1 } }, message = "gen1" },
    } })

  -- Replacement process generation admitted (restart).
  note_provider(diag, "perllsp", 2)

  local accepted, disposition = pub(diag, { generation = 1, version = 1 },
    { uri = URI, version = 1, diagnostics = {
      { range = { start = { line = 0, character = 0 }, ["end"] = { line = 0, character = 1 } }, message = "dead gen" },
    } })
  ok(accepted == false and disposition == "generation_replaced",
    "A5: old-generation publication rejected after replacement")
  ok(#visible_messages(diag, "C:/proj/app.pl") == 0,
    "A5: replacement generation resets prior retained sets")

  pub(diag, { generation = 2, version = 1 },
    { uri = URI, version = 1, diagnostics = {
      { range = { start = { line = 0, character = 0 }, ["end"] = { line = 0, character = 1 } }, message = "gen2 fresh" },
    } })
  local visible = visible_messages(diag, "C:/proj/app.pl")
  ok(#visible == 1 and visible[1].message == "gen2 fresh",
    "A5: replacement generation publishes normally")
end

-- Case A6: complementary providers retain separate source-attributed sets;
-- one provider cannot erase another by filename collision.
do
  local diag = load_diagnostics_module()
  note_provider(diag, "perllsp", 1)
  note_provider(diag, "navserver", 1)

  pub(diag, { provider = "perllsp", generation = 1, version = 2 },
    { uri = URI, version = 2, diagnostics = {
      { range = { start = { line = 0, character = 0 }, ["end"] = { line = 0, character = 1 } }, message = "perllsp says", source = "perllsp" },
    } })
  pub(diag, { provider = "navserver", generation = 1, version = 2 },
    { uri = URI, version = 2, diagnostics = {
      { range = { start = { line = 2, character = 0 }, ["end"] = { line = 2, character = 1 } }, message = "nav says", source = "navserver" },
    } })

  local visible = visible_messages(diag, "C:/proj/app.pl")
  ok(#visible == 2, "A6: both providers' sets project deterministically")
  local sources = {}
  for _, message in ipairs(visible) do sources[message.message] = message.source end
  ok(sources["perllsp says"] == "perllsp" and sources["nav says"] == "navserver",
    "A6: source attribution preserved through the projection")

  -- Losing-provider style overwrite attempt: navserver publishing again
  -- must not remove perllsp ownership.
  pub(diag, { provider = "navserver", generation = 1, version = 2 },
    { uri = URI, version = 2, diagnostics = {
      { range = { start = { line = 3, character = 0 }, ["end"] = { line = 3, character = 1 } }, message = "nav update", source = "navserver" },
    } })
  visible = visible_messages(diag, "C:/proj/app.pl")
  local saw_perllsp, saw_nav_old, saw_nav_new = false, false, false
  for _, message in ipairs(visible) do
    if message.message == "perllsp says" then saw_perllsp = true end
    if message.message == "nav says" then saw_nav_old = true end
    if message.message == "nav update" then saw_nav_new = true end
  end
  ok(saw_perllsp and saw_nav_new and not saw_nav_old,
    "A6: provider updates replace only their own set")
end

-- Case A7: provider retirement removes exactly that provider's ownership.
do
  local diag = load_diagnostics_module()
  note_provider(diag, "perllsp", 1)
  note_provider(diag, "navserver", 1)
  pub(diag, { provider = "perllsp", generation = 1, version = 1 },
    { uri = URI, version = 1, diagnostics = {
      { range = { start = { line = 0, character = 0 }, ["end"] = { line = 0, character = 1 } }, message = "p", source = "perllsp" },
    } })
  pub(diag, { provider = "navserver", generation = 1, version = 1 },
    { uri = URI, version = 1, diagnostics = {
      { range = { start = { line = 0, character = 0 }, ["end"] = { line = 0, character = 1 } }, message = "n", source = "navserver" },
    } })

  retire_provider(diag, "perllsp")
  local visible = visible_messages(diag, "C:/proj/app.pl")
  ok(#visible == 1 and visible[1].message == "n",
    "A7: retiring one provider removes only its visible ownership")
end

-- Case A8: unversioned publications carry an explicit not-proven
-- disposition and never masquerade as version-exact.
do
  local diag = load_diagnostics_module()
  note_provider(diag, "perllsp", 1)

  local accepted, disposition = pub(diag, { generation = 1, version = 4 },
    { uri = URI, diagnostics = {
      { range = { start = { line = 0, character = 0 }, ["end"] = { line = 0, character = 1 } }, message = "unversioned" },
    } })
  ok(accepted == true, "A8: unversioned publication admitted under bounded policy")
  ok(diag.get_publication_evidence ~= nil
    and diag.get_publication_evidence("perllsp", URI, 1) ~= nil
    and diag.get_publication_evidence("perllsp", URI, 1).version == "not_proven",
    "A8: internal evidence retains version = not_proven, never exact")
end

-- Case A9: delayed rendering is generation-bound - a timer scheduled for a
-- publication cannot render it after lifecycle cleanup removed the subject;
-- content is resolved at fire time from the current store, never cached at
-- schedule time.
do
  local diag = load_diagnostics_module()
  note_provider(diag, "perllsp", 1)
  config.plugins.lsp.show_diagnostics = true
  config.plugins.lsp.diagnostics_delay = 500

  pub(diag, { generation = 1, version = 1 },
    { uri = URI, version = 1, diagnostics = {
      { range = { start = { line = 0, character = 0 }, ["end"] = { line = 0, character = 1 } }, message = "first", severity = 1 },
    } })
  diag.lintplus_populate_delayed(platform_path("C:/proj/app.pl"))
  ok(#timers >= 1, "A9: render timer scheduled")

  -- Lifecycle cleanup removes the subject before the timer fires.
  close_session(diag, URI, 1)

  timers[#timers].on_timer()
  ok(#lintplus_calls == 0,
    "A9: cleaned-up subject's delayed timer renders nothing")

  -- A fresh session's timer still renders current content through the
  -- #11128 presentation authority (resolver supplies the live document).
  local live_doc = setmetatable({ lines = { "second here\n" } }, {})
  if diag.set_render_resolver then
    diag.set_render_resolver(function(uri) return live_doc end,
      function(uri, provider, sg, version)
        return sg == 2 and version == 2
      end)
  end
  pub(diag, { generation = 1, session_generation = 2, version = 2 },
    { uri = URI, version = 2, diagnostics = {
      { range = { start = { line = 0, character = 0 }, ["end"] = { line = 0, character = 6 } }, message = "second", severity = 1 },
    } })
  diag.lintplus_populate_delayed(platform_path("C:/proj/app.pl"))
  timers[#timers].on_timer()
  local rendered_texts = {}
  for _, call in ipairs(lintplus_calls) do rendered_texts[call.text] = true end
  ok(rendered_texts["second"] == true,
    "A9: live subject renders its current content")
end

-- Case A10: close-session lifecycle invalidates retained messages; a
-- post-close non-empty resurrection is rejected while an empty cleanup
-- publication is honored (bounded clearing-only policy).
do
  local diag = load_diagnostics_module()
  note_provider(diag, "perllsp", 1)
  pub(diag, { generation = 1, version = 1 },
    { uri = URI, version = 1, diagnostics = {
      { range = { start = { line = 0, character = 0 }, ["end"] = { line = 0, character = 1 } }, message = "retained", severity = 1 },
    } })
  ok(#visible_messages(diag, "C:/proj/app.pl") == 1, "A10: retained before close")

  close_session(diag, URI, 1)
  ok(#visible_messages(diag, "C:/proj/app.pl") == 0,
    "A10: close leaves no retained old-session messages")

  local accepted, disposition = pub(diag, { generation = 1, version = 1, has_session = false },
    { uri = URI, version = 1, diagnostics = {
      { range = { start = { line = 0, character = 0 }, ["end"] = { line = 0, character = 1 } }, message = "resurrect" },
    } })
  ok(accepted == false and disposition == "session_closed",
    "A10: post-close non-empty content cannot resurrect")
end

-- Case A11: reopened session identity starts clean; old queued
-- publications for a dead session generation stay inert.
do
  local diag = load_diagnostics_module()
  note_provider(diag, "perllsp", 1)
  pub(diag, { generation = 1, session_generation = 1, version = 5 },
    { uri = URI, version = 5, diagnostics = {
      { range = { start = { line = 0, character = 0 }, ["end"] = { line = 0, character = 1 } }, message = "old session", severity = 1 },
    } })
  close_session(diag, URI, 1)

  -- Held publication from dead session generation arrives late.
  local accepted, disposition = pub(diag, { generation = 1, session_generation = 1, version = 5 },
    { uri = URI, version = 5, diagnostics = {
      { range = { start = { line = 1, character = 0 }, ["end"] = { line = 1, character = 1 } }, message = "late old", severity = 1 },
    } })
  ok(accepted == false, "A11: dead-session publication rejected")
  ok(disposition == "session_closed" or disposition == "superseded",
    "A11: typed closed/superseded disposition")

  -- Fresh session generation publishes cleanly.
  local fresh_ok = select(1, pub(diag, { generation = 1, session_generation = 2, version = 0 },
    { uri = URI, version = 0, diagnostics = {
      { range = { start = { line = 0, character = 0 }, ["end"] = { line = 0, character = 1 } }, message = "new session", severity = 1 },
    } }))
  ok(fresh_ok == true, "A11: reopened session publishes independently")
end

print(string.format("PART A: %d passed, %d failed", passed, failed))
local part_a_failed = failed
local part_a_passed_total = passed
passed, failed = 0, 0

-- ===========================================================================
-- PART B: init.lua publishDiagnostics wiring through the real module
-- ===========================================================================

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
    listeners = {},
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
  function server:push_request(method, entry) record("request", method, entry) end
  function server:push_raw(method, entry) record("raw", method, entry) end
  function server:push_response() end
  function server:add_message_listener(method, fn) self.listeners[method] = fn end
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

local INCREMENTAL_CAPS = {
  textDocumentSync = { openClose = true, change = 2, save = { includeText = false } },
  positionEncoding = "utf-16",
}

local function open_admitted(lsp, doc, server)
  lsp.open_document(doc)
  drain(server, "textDocument/didOpen")
  doc.lsp_open = true
end

-- ---------------------------------------------------------------------------
-- Case B1: held versioned publication across an emitted edit batch is
-- rejected; the current-version publication lands.
-- ---------------------------------------------------------------------------
do
  local lsp = fresh_module_load()
  local server = make_server("perllsp", INCREMENTAL_CAPS)
  register(lsp, "perllsp", server)
  local doc = make_doc("C:/proj/b1.pl", { "x\n" })

  open_admitted(lsp, doc, server)
  local listener = lsp.handle_publish_diagnostics
  ok(type(listener) == "function",
    "B1: named production publishDiagnostics seam available")

  -- Publication for the current session version (0) lands.
  listener(server, { uri = wire_uri("C:/proj/b1.pl"), version = 0,
    diagnostics = { { range = { start = { line = 0, character = 0 }, ["end"] = { line = 0, character = 1 } }, message = "b1 current", severity = 1 } } })
  ok(next(lsp.document_sessions) ~= nil, "B1: session registry alive")
  local diag = package.loaded["plugins.lsp.diagnostics"]
  ok(#(diag.get(platform_path("C:/proj/b1.pl")) or {}) == 1, "B1: current publication visible")

  -- Accepted edit advances the stream to version 1.
  doc.lines[1] = "xy\n"
  doc:raw_insert(1, 2, "y", nil, 0)
  drain(server, "textDocument/didChange")

  -- Held publication for version 0 arrives late.
  listener(server, { uri = wire_uri("C:/proj/b1.pl"), version = 0,
    diagnostics = { { range = { start = { line = 0, character = 0 }, ["end"] = { line = 0, character = 1 } }, message = "b1 stale", severity = 1 } } })
  local visible = diag.get(platform_path("C:/proj/b1.pl")) or {}
  ok(#visible == 1 and visible[1].message == "b1 current",
    "B1: stale versioned publication cannot replace the current set")
end

-- ---------------------------------------------------------------------------
-- Case B2: delayed empty publication after a newer version cannot clear;
-- an exact-current empty publication does clear.
-- ---------------------------------------------------------------------------
do
  local lsp = fresh_module_load()
  local server = make_server("perllsp", INCREMENTAL_CAPS)
  register(lsp, "perllsp", server)
  local doc = make_doc("C:/proj/b2.pl", { "x\n" })
  open_admitted(lsp, doc, server)
  local listener = lsp.handle_publish_diagnostics
  local diag = package.loaded["plugins.lsp.diagnostics"]

  listener(server, { uri = wire_uri("C:/proj/b2.pl"), version = 0,
    diagnostics = { { range = { start = { line = 0, character = 0 }, ["end"] = { line = 0, character = 1 } }, message = "keep", severity = 1 } } })
  doc.lines[1] = "xy\n"
  doc:raw_insert(1, 2, "y", nil, 0)
  drain(server, "textDocument/didChange")

  listener(server, { uri = wire_uri("C:/proj/b2.pl"), version = 0, diagnostics = {} })
  ok(#(diag.get(platform_path("C:/proj/b2.pl")) or {}) == 1,
    "B2: delayed empty publication cannot clear a newer set")

  listener(server, { uri = wire_uri("C:/proj/b2.pl"), version = 1, diagnostics = {} })
  ok(#(diag.get(platform_path("C:/proj/b2.pl")) or {}) == 0,
    "B2: exact-current empty publication clears")
end

-- ---------------------------------------------------------------------------
-- Case B3: close/reopen invalidates the old session's diagnostics and a
-- held old-session publication cannot resurrect them.
-- ---------------------------------------------------------------------------
do
  local lsp = fresh_module_load()
  local server = make_server("perllsp", INCREMENTAL_CAPS)
  register(lsp, "perllsp", server)
  local doc = make_doc("C:/proj/b3.pl", { "x\n" })
  open_admitted(lsp, doc, server)
  local listener = lsp.handle_publish_diagnostics
  local diag = package.loaded["plugins.lsp.diagnostics"]

  listener(server, { uri = wire_uri("C:/proj/b3.pl"), version = 0,
    diagnostics = { { range = { start = { line = 0, character = 0 }, ["end"] = { line = 0, character = 1 } }, message = "before close", severity = 1 } } })
  ok(#(diag.get(platform_path("C:/proj/b3.pl")) or {}) == 1, "B3: visible before close")

  lsp.close_document(doc)
  drain(server, "textDocument/didClose")
  ok(#(diag.get(platform_path("C:/proj/b3.pl")) or {}) == 0,
    "B3: close cleanup removes retained old-session messages")

  -- Held publication from the dead session arrives late.
  listener(server, { uri = wire_uri("C:/proj/b3.pl"), version = 0,
    diagnostics = { { range = { start = { line = 0, character = 0 }, ["end"] = { line = 0, character = 1 } }, message = "resurrect attempt", severity = 1 } } })
  ok(#(diag.get(platform_path("C:/proj/b3.pl")) or {}) == 0,
    "B3: dead-session content cannot resurrect through the wired listener")

  -- Reopen: fresh session publishes independently.
  lsp.open_document(doc)
  drain(server, "textDocument/didOpen")
  doc.lsp_open = true
  listener(server, { uri = wire_uri("C:/proj/b3.pl"), version = 0,
    diagnostics = { { range = { start = { line = 0, character = 0 }, ["end"] = { line = 0, character = 1 } }, message = "after reopen", severity = 1 } } })
  local visible = diag.get(platform_path("C:/proj/b3.pl")) or {}
  ok(#visible == 1 and visible[1].message == "after reopen",
    "B3: reopened session publishes independently")
end

-- ---------------------------------------------------------------------------
-- Case B4: server stop/start makes the old generation's held publication
-- inert and resets retained sets for the replacement.
-- ---------------------------------------------------------------------------
do
  local lsp = fresh_module_load()
  local old_server = make_server("perllsp", INCREMENTAL_CAPS)
  register(lsp, "perllsp", old_server)
  local doc = make_doc("C:/proj/b4.pl", { "x\n" })
  open_admitted(lsp, doc, old_server)
  local old_listener = lsp.handle_publish_diagnostics
  local diag = package.loaded["plugins.lsp.diagnostics"]

  old_listener(old_server, { uri = wire_uri("C:/proj/b4.pl"), version = 0,
    diagnostics = { { range = { start = { line = 0, character = 0 }, ["end"] = { line = 0, character = 1 } }, message = "gen1", severity = 1 } } })
  ok(#(diag.get(platform_path("C:/proj/b4.pl")) or {}) == 1, "B4: gen1 set visible")

  lsp.stop_servers()
  local new_server = make_server("perllsp", INCREMENTAL_CAPS)
  register(lsp, "perllsp", new_server)
  open_admitted(lsp, doc, new_server)

  -- Held old-generation publication arrives after the restart.
  old_listener(old_server, { uri = wire_uri("C:/proj/b4.pl"), version = 0,
    diagnostics = { { range = { start = { line = 0, character = 0 }, ["end"] = { line = 0, character = 1 } }, message = "dead gen", severity = 1 } } })
  ok(#(diag.get(platform_path("C:/proj/b4.pl")) or {}) == 0,
    "B4: old-generation publication inert after restart")

  local new_listener = lsp.handle_publish_diagnostics
  new_listener(new_server, { uri = wire_uri("C:/proj/b4.pl"), version = 0,
    diagnostics = { { range = { start = { line = 0, character = 0 }, ["end"] = { line = 0, character = 1 } }, message = "gen2", severity = 1 } } })
  local visible = diag.get(platform_path("C:/proj/b4.pl")) or {}
  ok(#visible == 1 and visible[1].message == "gen2",
    "B4: replacement generation publishes visibly")
end

-- ---------------------------------------------------------------------------
-- Case B5: two complementary servers keep separate attributed sets through
-- the wired listener.
-- ---------------------------------------------------------------------------
do
  local lsp = fresh_module_load()
  local perl = make_server("perllsp", INCREMENTAL_CAPS)
  local nav = make_server("navserver", INCREMENTAL_CAPS)
  register(lsp, "perllsp", perl)
  register(lsp, "navserver", nav)
  local doc = make_doc("C:/proj/b5.pl", { "x\n" })
  open_admitted(lsp, doc, perl)
  open_admitted(lsp, doc, nav)
  local diag = package.loaded["plugins.lsp.diagnostics"]

  lsp.handle_publish_diagnostics(perl,
    { uri = wire_uri("C:/proj/b5.pl"), version = 0,
      diagnostics = { { range = { start = { line = 0, character = 0 }, ["end"] = { line = 0, character = 1 } }, message = "p-set", source = "perllsp", severity = 1 } } })
  lsp.handle_publish_diagnostics(nav,
    { uri = wire_uri("C:/proj/b5.pl"), version = 0,
      diagnostics = { { range = { start = { line = 1, character = 0 }, ["end"] = { line = 1, character = 1 } }, message = "n-set", source = "navserver", severity = 2 } } })

  local visible = diag.get(platform_path("C:/proj/b5.pl")) or {}
  ok(#visible == 2, "B5: both providers visible for one file")
  ok(visible[1].message == "p-set" and visible[2].message == "n-set",
    "B5: deterministic severity-sorted merge with attribution")
end

-- ---------------------------------------------------------------------------
-- Case B6: an unversioned publication (no params.version) is admitted
-- not-proven and still reaches delayed inline rendering while its session is
-- live; its currentness rides on session identity, never version equality.
-- ---------------------------------------------------------------------------
do
  local lsp = fresh_module_load()
  local server = make_server("perllsp", INCREMENTAL_CAPS)
  register(lsp, "perllsp", server)
  local doc = make_doc("C:/proj/b6.pl", { "x\n" })
  -- Register the editor-side open buffer so the real resolver finds a live
  -- document (a real editor has every open doc in core.docs).
  table.insert(require("core").docs, doc)
  open_admitted(lsp, doc, server)
  local listener = lsp.handle_publish_diagnostics
  local diag = package.loaded["plugins.lsp.diagnostics"]

  -- No params.version at all: the bounded unversioned admission policy.
  listener(server, { uri = wire_uri("C:/proj/b6.pl"),
    diagnostics = { { range = { start = { line = 0, character = 0 }, ["end"] = { line = 0, character = 1 } }, message = "unversioned", severity = 1 } } })
  local visible = diag.get(platform_path("C:/proj/b6.pl")) or {}
  ok(#visible == 1 and visible[1].message == "unversioned",
    "B6: unversioned publication admitted under bounded policy")

  -- It must reach delayed inline rendering through the real resolver while
  -- the session lives; a version-equality check against "not_proven" would
  -- skip every render forever.
  diag.lintplus_populate_delayed(platform_path("C:/proj/b6.pl"))
  timers[#timers].on_timer()
  local rendered = {}
  for _, call in ipairs(lintplus_calls) do rendered[call.text] = true end
  ok(rendered["unversioned"] == true,
    "B6: not-proven subject renders inline under session identity")

  -- Advancing the document version without a newer publication cannot make
  -- the not-proven subject stale: its evidence was never version-exact.
  doc.lines[1] = "xy\n"
  doc:raw_insert(1, 2, "y", nil, 0)
  drain(server, "textDocument/didChange")
  diag.lintplus_populate_delayed(platform_path("C:/proj/b6.pl"))
  timers[#timers].on_timer()
  rendered = {}
  for _, call in ipairs(lintplus_calls) do rendered[call.text] = true end
  ok(rendered["unversioned"] == true,
    "B6: version advance alone does not stale a not-proven subject")
end

print(string.format("PART B: %d passed, %d failed", passed, failed))
failed = failed + part_a_failed
print(string.format("TOTAL: %d passed, %d failed", part_a_passed_total + passed, failed))
os.exit(failed == 0 and 0 or 1)

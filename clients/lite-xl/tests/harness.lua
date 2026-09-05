-- Deterministic client-journey simulation harness for the staged
-- lite-xl-lsp modules (#11103).
--
-- The focused suites (init_document_session_test.lua,
-- init_request_currentness_test.lua, util_show_document_test.lua) each
-- hand-roll the same Lite XL runtime fakes and isolate one transition per
-- case. This module generalizes their scaffolding into one reusable layer
-- so journey suites can drive MULTI-STEP stateful journeys - open, edit,
-- backpressure, close/reopen, full server restarts, mid-journey
-- configuration changes - while retaining the complete ordered wire
-- history across every server generation.
--
-- Proof shape (unchanged from the focused-suite conventions):
--   - the exact staged upstream source is dofile'd; nothing is copied or
--     transliterated into fakes;
--   - require resolution runs through package.preload fakes installed per
--     world and torn down after it, so worlds never leak state;
--   - no framework, no wall-clock sleeps, deterministic, exit code carries
--     the result (the suite owns assertions; this module only observes);
--   - compatible with the Lite XL Lua runtime family (Lua 5.4).
--
-- What this module deliberately does NOT do: spawn real server processes,
-- drive the staged server.lua transport loop, or replace real-host
-- evidence (owned by #10673/#9008). See README.md in this directory for
-- usage and boundaries.

local harness = {}

-- ---------------------------------------------------------------------------
-- Pristine bases captured before any staged module wraps them
-- ---------------------------------------------------------------------------

local Doc = { __index = Doc }
function Doc.raw_insert(self, line, col, text)
  local l = self.lines[line]
  if not l then return end
  -- Lines ending in "\n" accept columns up to #l (before the newline);
  -- a terminal line without "\n" also accepts #l + 1 (append at end).
  local limit = #l
  if l:sub(-1) ~= "\n" then limit = #l + 1 end
  if limit < 1 then limit = 1 end
  if col > limit then col = limit end
  local head = l:sub(1, col - 1)
  local tail = l:sub(col)

  -- Split inserted text on newlines. Every delimiter terminates its own
  -- segment, so a trailing newline yields an explicit empty final segment
  -- and opens a fresh line instead of being glued onto the anchor line.
  local segments = {}
  local pos = 1
  while true do
    local nl = string.find(text, "\n", pos, true)
    if not nl then
      if pos <= #text then segments[#segments + 1] = string.sub(text, pos) end
      break
    end
    segments[#segments + 1] = string.sub(text, pos, nl)
    pos = nl + 1
  end
  if #segments > 0 and string.sub(text, -1) == "\n" then
    segments[#segments + 1] = ""
  end

  if #segments == 0 then
    return
  elseif #segments == 1 then
    self.lines[line] = head .. segments[1] .. tail
  else
    local replacement = {}
    replacement[1] = head .. segments[1]
    for i = 2, #segments - 1 do replacement[#replacement + 1] = segments[i] end
    replacement[#replacement + 1] = segments[#segments] .. tail
    table.remove(self.lines, line)
    for i = #replacement, 1, -1 do
      table.insert(self.lines, line, replacement[i])
    end
  end
end

function Doc.raw_remove(self, line1, col1, line2, col2)
  local l1 = self.lines[line1]
  if not l1 then return end
  local l2 = self.lines[line2] or l1
  if line1 == line2 then
    local stop = col2
    if stop < col1 then stop = col1 end
    if stop > #l1 + 1 then stop = #l1 + 1 end
    self.lines[line1] = l1:sub(1, col1 - 1) .. l1:sub(stop)
  else
    local head = l1:sub(1, col1 - 1)
    local tail_start = col2
    if tail_start < 1 then tail_start = 1 end
    if tail_start > #l2 + 1 then tail_start = #l2 + 1 end
    local tail = l2:sub(tail_start)
    for i = line2, line1 + 1, -1 do table.remove(self.lines, i) end
    self.lines[line1] = head .. tail
  end
end

function Doc.get_selection(self)
  local c = self.caret or { line = 1, col = 1 }
  return c.line, c.col, c.line, c.col
end

function Doc.get_char() return "" end

local pristine_doc_raw_insert = Doc.raw_insert
local pristine_doc_raw_remove = Doc.raw_remove

---Convert editor text to lite-xl's lines representation (each line keeps
---its trailing newline except possibly the last).
local function text_to_lines(text)
  local lines = {}
  local pos = 1
  while pos <= #text do
    local nl = string.find(text, "\n", pos, true)
    if not nl then
      lines[#lines + 1] = string.sub(text, pos)
      break
    end
    lines[#lines + 1] = string.sub(text, pos, nl)
    pos = nl + 1
  end
  return lines
end

-- ---------------------------------------------------------------------------
-- Fake running LSP server: FIFO wire queue, retained history, backpressure,
-- request listeners, generation identity.
-- ---------------------------------------------------------------------------

local FakeServer = {}
FakeServer.__index = FakeServer

function harness.fake_server(world, name, options)
  options = options or {}
  local server = setmetatable({
    name = name,
    file_patterns = options.file_patterns or { "%.pl$" },
    initialized = true,
    verbose = false,
    incremental_changes = options.incremental_changes or false,
    capabilities = options.capabilities or {
      textDocumentSync = {
        openClose = true,
        change = 2,
        save = { includeText = false },
      },
      positionEncoding = "utf-16",
    },
    settings = options.settings,
    path = options.path,
    outbound = {},
    request_listeners = {},
    event_listeners = {},
    can_push_value = true,
    exits = 0,
    world = world,
  }, FakeServer)

  function server:can_push() return self.can_push_value end
  function server:get_language_id() return "perl" end

  function server:exit()
    self.exits = self.exits + 1
  end

  ---Deep-copy plain nested payloads so a recorded frame stays immutable
  ---evidence even when production later mutates or clears the tables it
  ---passed (session pending queues are cleared by their own callbacks).
  local function copy_value(value)
    if type(value) ~= "table" then return value end
    local copy = {}
    for key, item in pairs(value) do copy[key] = copy_value(item) end
    return copy
  end

  local function record(kind, method, entry)
    local frame = {
      method = method,
      kind = kind,
      server_name = name,
      generation_at_send = server.generation,
      params = copy_value(entry.params),
      raw_data = entry.raw_data,
      callback = entry.callback,
      overwrite = entry.overwrite,
      sent = false,
    }
    world.wire[#world.wire + 1] = frame
    server.outbound[#server.outbound + 1] = frame
    return frame
  end

  ---Find the earliest queued-but-unsent same-method entry, mirroring the
  ---staged Server's overwrite scan over its notification/raw lists.
  local function find_unsent(method)
    for _, entry in ipairs(server.outbound) do
      if entry.method == method and not entry.sent then return entry end
    end
    return nil
  end

  function server:push_notification(method, entry)
    assert(entry.params, "please provide the parameters for the notification")
    if entry.overwrite then
      -- Production mutates the queued frame in place; the superseded
      -- payload never reaches the wire and keeps its queue position.
      local prior = find_unsent(method)
      if prior then
        prior.params = copy_value(entry.params)
        prior.callback = entry.callback
        return prior
      end
    end
    return record("notification", method, entry)
  end

  function server:push_request(method, entry) record("request", method, entry) end

  function server:push_raw(method, entry)
    assert(entry.raw_data, "please provide the raw_data for request")
    if entry.overwrite then
      local prior = find_unsent(method)
      if prior then
        prior.raw_data = entry.raw_data
        prior.callback = entry.callback
        return prior
      end
    end
    return record("raw", method, entry)
  end
  function server:push_response(method, id, result)
    record("response", method, { id = id, result = result })
  end

  function server:add_request_listener(event_name, callback)
    self.request_listeners[event_name] = callback
  end

  function server:add_event_listener(event_name, callback)
    self.event_listeners[event_name] = callback
  end

  ---Play queued callbacks FIFO like responses arriving in order. With a
  ---method filter only matching entries play; the rest stay queued. New
  ---entries pushed by callbacks during an unfiltered drain are played in
  ---the same turn (FIFO turn semantics).
  function server:drain(method)
    local played = {}
    local i = 1
    while i <= #self.outbound do
      local entry = self.outbound[i]
      if method == nil or entry.method == method then
        table.remove(self.outbound, i)
        played[#played + 1] = entry
        entry.sent = true
        if entry.callback then entry.callback(self) end
      else
        i = i + 1
      end
    end
    return played
  end

  return server
end

-- ---------------------------------------------------------------------------
-- World construction: per-journey isolated Lite XL runtime
-- ---------------------------------------------------------------------------

local originals_captured = false
local original_preload_keys = {}
local original_globals = {}
local original_os_time = nil

local FAKE_MODULE_KEYS = {
  "core", "core.common", "core.config", "core.command", "core.style",
  "core.keymap", "core.doc.translate", "plugins.autocomplete", "core.doc",
  "core.docview", "core.statusview", "core.rootview", "core.object",
  "plugins.lsp.json", "process", "plugins.lsp.util", "plugins.lsp.listbox",
  "plugins.lsp.diagnostics", "plugins.lsp.server", "plugins.lsp.timer",
  "plugins.lsp.symbolresults", "libraries.widget.messagebox",
  "plugins.lsp.helpdoc", "plugins.lsp.capability_manifest",
}

---Marker distinguishing "module was absent" from a stored nil, which Lua
---table semantics would silently drop from the saved maps.
local ABSENT = {}

local function capture_originals()
  if originals_captured then return end
  originals_captured = true
  for _, key in ipairs(FAKE_MODULE_KEYS) do
    -- ABSENT marks originally-absent entries: storing raw nil would drop
    -- the key from this map and teardown could never restore the absence.
    original_preload_keys[key] = package.preload[key] or ABSENT
  end
  original_globals.utf8extra = utf8extra
  original_globals.PLATFORM = PLATFORM
  original_globals.USERDIR = USERDIR
  original_globals.SCALE = SCALE
  original_globals.renderer = renderer
  original_globals.system = system
  original_os_time = os.time
end

local DEFAULTS = {
  platform = "Windows",
}

---Create one isolated journey world. Options:
---   init_module      path to the exact staged init.lua to load (default:
---                    ../upstream/init.lua relative to this file). Point
---                    this at a mutated copy when verifying documented
---                    falsifiers.
---   manifest_module  path to the exact staged capability manifest to load
---                    (#11172; default ../upstream/capability_manifest.lua).
---   platform         PLATFORM global seen by production code.
---
---The returned world exposes the exact staged module as world.lsp plus the
---observation surfaces (wire history, clock, config, diagnostics log).
---Call world.teardown() when the journey ends.
function harness.new_world(options)
  options = options or {}
  capture_originals()

  local here = debug.getinfo(1, "S").source:sub(2):match("^(.*)[/\\]") or "."
  local init_module_path = options.init_module
    or here .. "/../upstream/init.lua"
  local manifest_module_path = options.manifest_module
    or here .. "/../upstream/capability_manifest.lua"

  local world = {
    wire = {},
    diagnostics_log = {},
    process_starts = {},
    timers = {},
    commands_registered = {},
    _file_info = { size = 100 },
  }

  -- Per-world require isolation: drop any previously loaded fake modules so
  -- this world's staged module resolves requires through THIS world's
  -- preload factories, never a prior world's cached closures.
  local saved_loaded = {}
  for _, key in ipairs(FAKE_MODULE_KEYS) do
    -- ABSENT marks originally-absent entries: storing raw nil would drop
    -- the key from this map and teardown could never restore the absence.
    saved_loaded[key] = package.loaded[key] or ABSENT
    package.loaded[key] = nil
  end
  world.saved_loaded = saved_loaded

  -- Restore the pristine wrapper bases so repeated module loads never
  -- chain lsp's Doc overrides onto a previous world's wrappers.
  Doc.raw_insert = pristine_doc_raw_insert
  Doc.raw_remove = pristine_doc_raw_remove

  -- Clock: fake epoch for os.time (workspace-settings cache TTL) and a
  -- separate monotonic counter for system.get_time. Explicit advance/tick
  -- only; wall-clock time is never consulted.
  local clock = {
    epoch = 1000000000,
    monotonic = 0,
  }
  function clock.advance(seconds) clock.epoch = clock.epoch + seconds end
  function clock.tick(seconds) clock.monotonic = clock.monotonic + seconds end
  world.clock = clock
  os.time = function() return clock.epoch end

  -- ---------------------------------------------------------------------------
  -- package.preload fakes (world-local closures)
  -- ---------------------------------------------------------------------------

  local log_records = {}
  world.log_records = log_records

  local core_table = {
    docs = {},
    project_dir = ".",
    log = function(fmt, ...) log_records[#log_records + 1] = tostring(fmt) end,
    log_quiet = function(fmt, ...) log_records[#log_records + 1] = tostring(fmt) end,
    error = function(fmt, ...) log_records[#log_records + 1] = "error:" .. tostring(fmt) end,
    add_thread = function() end,
    project_absolute_path = function(path) return path end,
    normalize_to_project_dir = function(path) return path end,
    home_expand = function(path) return path end,
    command_view = {
      enter = function(_, prompt)
        log_records[#log_records + 1] = "prompt:" .. tostring(prompt)
      end,
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
  world.core = core_table
  package.preload["core"] = function() return core_table end

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
    }
  end

  local plugin_config = { plugins = {} }
  world.config = {} -- populated after the staged module merges its defaults
  package.preload["core.config"] = function() return plugin_config end

  package.preload["core.command"] = function()
    -- Record every registered command name so suites can pin projection
    -- coverage (e.g. every lsp:* command carries a manifest row, #11172)
    -- without depending on command dispatch.
    return {
      add = function(_, items)
        if type(items) == "table" then
          for name in pairs(items) do
            world.commands_registered[name] = true
          end
        end
      end,
      map = function() end,
    }
  end

  package.preload["core.style"] = function()
    return { syntax_fonts = {}, warn = {}, icon_font = {}, font = {} }
  end

  package.preload["core.keymap"] = function() return { add = function() end } end

  package.preload["core.doc.translate"] = function()
    return {
      start_of_word = function(_, doc, line, col) return line, col end,
      end_of_word = function(_, doc, line, col) return line, col end,
    }
  end

  package.preload["plugins.autocomplete"] = function()
    -- No add_icon field on purpose: keeps the icon-registration block
    -- skipped, matching the focused suites' convention.
    return {
      can_complete = function() return false end,
      complete = function() end,
    }
  end

  package.preload["core.doc"] = function() return Doc end

  local DocView = { __index = DocView }
  function DocView:extends() return false end
  package.preload["core.docview"] = function() return DocView end

  package.preload["core.statusview"] = function()
    return { Item = { RIGHT = 2 } }
  end

  local RootView = { __index = RootView }
  package.preload["core.rootview"] = function() return RootView end

  package.preload["core.object"] = function()
    local Object = {}
    Object.__index = Object
    function Object:extend()
      local cls = setmetatable({}, { __index = self })
      cls.__index = cls
      return cls
    end
    function Object:new(...)
      local obj = setmetatable({}, self)
      if obj.init then obj:init(...) end
      return obj
    end
    return Object
  end

  package.preload["plugins.lsp.json"] = function()
    return dofile(here .. "/../upstream/json.lua")
  end

  local process_fake = {
    start = function(argv)
      world.process_starts[#world.process_starts + 1] = argv
      return 42000 + #world.process_starts
    end,
  }
  world.process = process_fake
  package.preload["process"] = function() return process_fake end

  package.preload["plugins.lsp.util"] = function()
    return dofile(here .. "/../upstream/util.lua")
  end

  package.preload["plugins.lsp.listbox"] = function()
    return {
      hide = function()
        log_records[#log_records + 1] = "listbox:hide"
      end,
      show_text = function(_, items)
        log_records[#log_records + 1] = "listbox:show_text"
        return items
      end,
    }
  end

  local diagnostics_log = world.diagnostics_log
  package.preload["plugins.lsp.diagnostics"] = function()
    return {
      note_provider = function(name, generation)
        diagnostics_log[#diagnostics_log + 1] =
          { op = "note_provider", name = name, generation = generation }
      end,
      close_session = function(uri, session_generation)
        diagnostics_log[#diagnostics_log + 1] =
          { op = "close_session", uri = uri, session_generation = session_generation }
      end,
      retire_provider = function(name)
        diagnostics_log[#diagnostics_log + 1] =
          { op = "retire_provider", name = name }
      end,
      -- #11128 rendering-resolver seam; combined-tree repair for this lane
      -- (#11165): the init.lua call entered main without updating this fake.
      set_render_resolver = function(resolver)
        diagnostics_log[#diagnostics_log + 1] = { op = "set_render_resolver" }
      end,
      publish = function()
        diagnostics_log[#diagnostics_log + 1] = { op = "publish" }
        return true, nil
      end,
      -- #12047 render-resolver seam: init.lua registers it unconditionally
      -- at load; journeys observe publications, not column resolution.
      set_render_resolver = function() end,
      lintplus_init_doc = function() end,
      lintplus_found = false,
      lintplus_populate_delayed = function() end,
      lintplus_clear_messages = function() end,
      lintplus_populate = function() end,
      lintplus_kinds = {},
      severity = {},
      get = function() return nil end,
      count = function() return 0 end,
      list = function() return {} end,
      get_messages_count = function() return 0 end,
    }
  end

  -- Exact staged Server class loaded under fakes; production-exact sync/
  -- position constants without hand copies. Journeys install fake running
  -- instances instead of constructing real processes.
  package.preload["plugins.lsp.server"] = function()
    return dofile(here .. "/../upstream/server.lua")
  end

  package.preload["plugins.lsp.capability_manifest"] = function()
    return dofile(manifest_module_path)
  end

  package.preload["plugins.lsp.timer"] = function()
    return function(interval, one_shot)
      local timer = {
        interval = interval,
        one_shot = one_shot,
        on_timer = nil,
        started = false,
        world = world,
      }
      function timer:start()
        self.started = true
        world.timers[#world.timers + 1] = self
      end
      function timer:stop() self.started = false end
      function timer:reset() end
      function timer:set_interval(new_interval) self.interval = new_interval end
      function timer:restart()
        self.started = true
        world.timers[#world.timers + 1] = self
      end
      function timer:running() return self.started end
      return timer
    end
  end

  package.preload["plugins.lsp.symbolresults"] = function() return {} end

  package.preload["libraries.widget.messagebox"] = function()
    return {
      BUTTONS_YES_NO = 1,
      info = function(title, message, callback, buttons)
        log_records[#log_records + 1] = "messagebox:" .. tostring(message)
        if callback then callback(nil, buttons) end
      end,
    }
  end

  package.preload["plugins.lsp.helpdoc"] = function()
    return function(title) return { title = title, set_text = function() end } end
  end

  -- Globals observed by production code.
  utf8extra = {
    len = function(s) return utf8.len(s) or #s end,
    next = function(s, pos)
      if pos > #s then return nil end
      local code = utf8.codepoint(s, pos)
      local width = 1
      if code >= 0x80 then width = 2 end
      if code >= 0x800 then width = 3 end
      if code >= 0x10000 then width = 4 end
      return pos + width, code
    end,
  }
  PLATFORM = options.platform or DEFAULTS.platform
  USERDIR = "."
  SCALE = 1
  renderer = { font = { load = function() return {} end } }
  system = {
    exec = function() error("shell invoked", 0) end,
    raise_window = function() end,
    get_file_info = function() return world._file_info end,
    get_time = function() return clock.monotonic end,
  }

  -- ---------------------------------------------------------------------------
  -- Exact staged module load and world API
  -- ---------------------------------------------------------------------------

  world.source_path = init_module_path
  world.lsp = dofile(init_module_path)
  world.config = plugin_config.plugins.lsp

  ---Register a server definition through the real lsp.add_server path and
  ---install a fake running instance (real construction would spawn
  ---processes; that boundary stays with the server-level seam).
  function world.define_server(name, opts)
    opts = opts or {}
    assert(not world.lsp.servers_running[name],
      "harness: a running instance named '" .. name .. "' already exists")
    local definition = {
      name = name,
      language = "perl",
      file_patterns = opts.file_patterns or { "%.pl$" },
      command = opts.command or { "perllsp" },
      windows_skip_cmd = true,
      settings = opts.settings,
      path = opts.path,
    }
    assert(world.lsp.add_server(definition), "harness: add_server rejected definition")
    local server = harness.fake_server(world, name, {
      file_patterns = definition.file_patterns,
      capabilities = opts.capabilities,
      incremental_changes = opts.incremental_changes,
      settings = opts.settings,
      path = opts.path,
    })
    world.lsp.servers_running[name] = server
    return server
  end

  ---Install a fresh fake running instance after a restart. Refuses to run
  ---while the previous instance is still registered (stop_servers first).
  function world.start(name, opts)
    return world.define_server(name, opts)
  end

  function world.stop_servers()
    world.lsp.stop_servers()
  end

  ---Create a fake document with a REAL minimal line buffer: base
  ---raw_insert/raw_remove mutate bytes exactly like the editor would, so
  ---journeys can assert final editor truth alongside wire truth.
  function world.new_doc(filename, text)
    local doc = setmetatable({
      filename = filename,
      abs_filename = filename,
      lines = text_to_lines(text),
      clean_change_id = 1,
      lsp_open = false,
    }, { __index = Doc })
    core_table.docs[#core_table.docs + 1] = doc
    return doc
  end

  function world.teardown()
    os.time = original_os_time
    for key, value in pairs(original_preload_keys) do
      if value == ABSENT then
        package.preload[key] = nil
      else
        package.preload[key] = value
      end
    end
    for key, value in pairs(world.saved_loaded) do
      package.loaded[key] = (value == ABSENT) and nil or value
    end
    utf8extra = original_globals.utf8extra
    PLATFORM = original_globals.PLATFORM
    USERDIR = original_globals.USERDIR
    SCALE = original_globals.SCALE
    renderer = original_globals.renderer
    system = original_globals.system
    Doc.raw_insert = pristine_doc_raw_insert
    Doc.raw_remove = pristine_doc_raw_remove
  end

  return world
end

return harness

-- Deterministic focused tests for the monotonic LSP document-version
-- authority in clients/lite-xl/upstream/init.lua (#11115).
--
-- Run:
--   lua clients/lite-xl/tests/init_document_session_test.lua [path-to-init-module]
-- Default module path is ../upstream/init.lua relative to this file.
--
-- Proof shape: the exact staged init.lua module is loaded under minimal Lite
-- XL runtime fakes (fake Doc class, fake running-server instances recording
-- outbound notifications in FIFO queues, deterministic file-info seams).
-- Tests drive the real public paths - lsp.open_document, Doc:raw_insert/
-- raw_remove overrides, lsp.update_document, lsp.save_document,
-- lsp.close_document, lsp.stop_servers - and assert exact wire versions,
-- session identities, snapshot identity, and inertness of terminated
-- sessions.
--
-- Red-first baseline: run this suite against the PRISTINE upstream init.lua
-- @ d1432ae0736cd9531798b4bc1221835f534cc689 (blob
-- 7b38c3a97c68877d2391753adb09e49ec57397d3):
--
--   lua clients/lite-xl/tests/init_document_session_test.lua <pristine-init.lua>
--
-- There the following cases MUST fail, proving the tests discriminate the
-- session/version authority instead of passing vacuously:
--   - versions_increase_per_emitted_batch (pristine derives didOpen version
--     from doc.clean_change_id and increments doc.lsp_version per keystroke
--     rather than per emitted batch);
--   - undo_redo_versions_continue_increasing (pristine repeats/resets
--     versions derived from editor history);
--   - close_reopen_same_uri_gets_new_session_identity (pristine has no
--     session records at all);
--   - restart_rejects_old_generation_state (pristine keeps doc-global
--     lsp_version/lsp_changes across process generations).
-- Remaining cases may pass on pristine where upstream already behaved
-- compatibly; they pin the contract against regression.
--
-- CURRENTNESS (NOT_PROVEN): that pristine baseline no longer RUNS. The
-- runtime fakes below have tracked the staged module's seams (#11165
-- util.path_to_uri, #11172 capability manifest, ...), so loading the
-- pristine blob now dies in the fakes (`common.touri` nil, plus pristine-era
-- requires) before any case is evaluated. A crash is instrument failure, not
-- a red-first result, so the list above is retained as the recorded history
-- of that baseline and NOT as a currently reproducible claim. Restoring a
-- pristine-compatible fake set spans several other modules' staged patches
-- and is tracked separately; the mutation falsifiers below are this suite's
-- currently reproducible discrimination evidence.
--
-- Single-behavior mutation falsifiers of the PATCHED module (each verified
-- to be caught; named case families fail):
--   1. restore didOpen `version = doc.clean_change_id` ->
--      versions_increase_per_emitted_batch and api cases fail;
--   2. delete the per-batch `session.version = session.version + 1`
--      allocation -> every emitted batch repeats version 0 ->
--      all stream-ordering cases fail;
--   3. delete the terminate-first step in
--      lsp.create_document_session -> the Save As case fails (a stale
--      session_generation/uri survives the URI transition);
--   4. diverge the ranged-sync branch's wire-version source from the shared
--      session stream -> full_and_ranged_branches_share_one_version_owner
--      fails;
--   5. rewind the owning session's version inside lsp.save_document ->
--      case 3 fails (didSave must not reset or reuse the stream);
--   6. replace the per-session allocation with a process-global
--      ever-increasing counter -> cases 3, 4, 6, 7 and 8 fail (a global
--      counter is monotonic but is not owned by the session, so fresh
--      origins and per-session continuity are lost);
--   7. derive the batch version from the editor's undo identity
--      (`session.version = doc:get_change_id()`) -> case 4 fails, because
--      undo moves undo_stack.idx backward and redo revisits a value the
--      stream already used.
--
-- Falsifier 7 is why the Doc fake models `undo_stack`/`get_change_id()`
-- rather than `clean_change_id` alone: #11115 names get_change_id()
-- explicitly in its negative controls, and a mutant reading a surface the
-- instrument does not model fails by crashing (instrument failure), which
-- is NOT a caught falsifier.
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
    status_view = {
      separator2 = 2,
      add_item = function() end,
    },
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
  -- No add_icon field on purpose: keeps the icon-registration block skipped.
  return {
    can_complete = function() return false end,
    complete = function() end,
  }
end

---Fake Doc class. init.lua wraps raw_insert/raw_remove over whatever base
---implementations exist here; fresh_module_load() restores the pristine
---bases so repeated module loads never chain wrappers.
---
---The base raw_* implementations model Lite XL's UNDO IDENTITY, which the
---#11115 negative controls name explicitly ("no protocol version is derived
---from clean_change_id, get_change_id(), ..."). Upstream Lite XL keeps
---`Doc.undo_stack = { idx = ... }` and defines
---`Doc:get_change_id() -> self.undo_stack.idx`; every accepted mutation
---pushes onto the stack passed as the `undo_stack` argument (defaulting to
---the document's own undo stack), and `pop_undo` applies an inverse command
---through these SAME entry points while targeting the redo stack, so
---`get_change_id()` moves BACKWARD across an undo. Bytes stay owned by each
---case (the fakes are line-poked deliberately); only the editor-history
---identity is modeled here, because that identity is the thing the protocol
---version must never be derived from.
local Doc = { __index = Doc }
local function push_change(self, stack)
  local target = stack or self.undo_stack
  if target then
    target.idx = target.idx + 1
  end
end
Doc.raw_insert = function(self, line, col, text, undo_stack, time)
  push_change(self, undo_stack)
end
Doc.raw_remove = function(self, line1, col1, line2, col2, undo_stack, time)
  push_change(self, undo_stack)
end
Doc.get_change_id = function(self) return self.undo_stack.idx end
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
  -- Never executed by these tests; util.lua only requires the module.
  return { start = function() return nil end }
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
  -- Lifecycle seams consumed by init.lua (#11124); inert in this suite,
  -- whose subject is document-session/version behavior, not publications.
  -- set_render_resolver: combined-tree repair for this lane (#11165); the
  -- #11128 seam call entered init.lua on main without updating these fakes.
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
    symbol_kind = {},
    get_symbol_kind = function() return "Text" end,
  }
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

-- Minimal ASCII-compatible utf8extra surface for util.doc_utf8_to_utf16.
utf8extra = {
  len = function(s) return utf8.len(s) or #s end,
}

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
  return dofile(init_module_path)
end

-- ---------------------------------------------------------------------------
-- Fixture helpers
-- ---------------------------------------------------------------------------

local function make_doc(filename, lines)
  return setmetatable({
    filename = filename,
    abs_filename = filename,
    lines = lines,
    clean_change_id = 1,
    -- Lite XL editor-history identity (see the Doc fake above). 1-based idx
    -- exactly like upstream's freshly constructed undo/redo stacks.
    undo_stack = { idx = 1 },
    redo_stack = { idx = 1 },
    lsp_open = false,
  }, { __index = Doc })
end

---Fake running LSP server recording every pushed notification/request as a
---FIFO queue entry {method=..., params=..., raw_data=..., callback=...}.
---drain() plays callbacks in order like responses arriving.
local function make_server(name, options)
  options = options or {}
  local server = {
    name = name,
    file_patterns = options.file_patterns or { "%.pl$" },
    initialized = true,
    verbose = false,
    incremental_changes = options.incremental_changes or false,
    capabilities = options.capabilities or {
      textDocumentSync = { openClose = true, change = 2, save = { includeText = false } },
      positionEncoding = "utf-16",
    },
    outbound = {},
    can_push_value = true,
  }
  function server:can_push() return self.can_push_value end
  function server:get_language_id() return "perl" end
  function server:exit() self.exited = (self.exited or 0) + 1 end
  local function record(kind, method, entry)
    entry.method = method
    entry.kind = kind
    server.outbound[#server.outbound + 1] = entry
  end
  function server:push_notification(method, entry) record("notification", method, entry) end
  function server:push_request(method, entry) record("request", method, entry) end
  function server:push_raw(method, entry) record("raw", method, entry) end
  function server:push_response() end
  function server:add_message_listener() end
  function server:add_event_listener() end
  return server
end

---Register a fake server so lsp.get_active_servers admits it.
local function register(lsp, name, server)
  lsp.servers[name] = {
    name = name,
    file_patterns = server.file_patterns,
    command = { "perllsp" },
    language = "perl",
  }
  lsp.servers_running[name] = server
end

---Extract the integer version carried by a didChange-shaped outbound entry
---(notification params or raw JSON payload).
local function batch_version(entry)
  if entry.params then
    return entry.params.textDocument.version
  end
  return tonumber(entry.raw_data:match('"version":%s*(%-?%d+)'))
end

local function drain(server, method)
  local remaining = {}
  local played = {}
  for _, entry in ipairs(server.outbound) do
    if method == nil or entry.method == method then
      played[#played + 1] = entry
      if entry.callback then entry.callback(server) end
    else
      remaining[#remaining + 1] = entry
    end
  end
  server.outbound = remaining
  return played
end

---Canonical URI exactly as production computes it (#11165 authority).
local function expected_uri(path)
  return dofile(here .. "/../upstream/util.lua").path_to_uri(path)
end

local INCREMENTAL = { textDocumentSync = { openClose = true, change = 2, save = { includeText = false } }, positionEncoding = "utf-16" }
local FULL = { textDocumentSync = { openClose = true, change = 1, save = { includeText = false } }, positionEncoding = "utf-16" }

---Absence-tolerant session lookup: the pristine module has no session API,
---which is itself a red-baseline failure rather than a harness crash.
local function get_session(lsp, doc, server)
  if lsp.get_document_session then
    return lsp.get_document_session(doc, server)
  end
  return nil
end

-- ===========================================================================
-- Case 1: open -> edit -> edit emits didOpen(V0, exact text) then one
-- strictly increasing version per accepted didChange batch.
-- ===========================================================================
do
  local lsp = fresh_module_load()
  local server = make_server("perllsp", { capabilities = INCREMENTAL })
  register(lsp, "perllsp", server)
  local doc = make_doc("C:/proj/script.pl", { "my $x = 1;\n", "my $y = 2;\n" })

  lsp.open_document(doc)
  local opened = drain(server, "textDocument/didOpen")
  ok(#opened == 1, "case1: exactly one didOpen emitted")
  local did_open = opened[1]
  ok(did_open.params.textDocument.version == 0,
    "case1: didOpen version is explicit fresh origin 0")
  ok(did_open.params.textDocument.text == "my $x = 1;\nmy $y = 2;\n",
    "case1: didOpen carries exact current snapshot text")

  doc.lsp_open = true -- didOpen admission callback effect
  drain(server, "textDocument/didOpen")

  doc.lines[1] = "my $x = 10;\n"
  doc:raw_insert(1, 9, "0", nil, 0)
  local batches = drain(server, "textDocument/didChange")
  ok(#batches == 1, "case1: first edit emits exactly one didChange batch")
  ok(batch_version(batches[1]) == 1, "case1: first emitted batch is version 1")
  ok(#batches[1].params.contentChanges == 1,
    "case1: batch carries exactly the accepted change")

  doc.lines[2] = "my $y = 20;\n"
  doc:raw_remove(2, 8, 2, 9, nil, 0)
  batches = drain(server, "textDocument/didChange")
  ok(#batches == 1, "case1: second edit emits exactly one didChange batch")
  ok(batch_version(batches[1]) == 2,
    "case1: second emitted batch is version 2 - strictly increasing")
end

-- ===========================================================================
-- Case 2: edits accepted before session admission reach the didOpen snapshot;
-- they never leak into later sessions' pending streams.
-- ===========================================================================
do
  local lsp = fresh_module_load()
  local server = make_server("perllsp", { capabilities = INCREMENTAL })
  register(lsp, "perllsp", server)
  local doc = make_doc("C:/proj/pre.pl", { "line one\n" })

  -- Edit before didOpen admission (server running, document not open yet).
  doc.lines[1] = "line one edited\n"
  doc:raw_insert(1, 9, "edited", nil, 0)

  lsp.open_document(doc)
  local opened = drain(server, "textDocument/didOpen")
  ok(#opened == 1 and opened[1].params.textDocument.text == "line one edited\n",
    "case2: didOpen contains current edited text after pre-admission edits")
  ok(opened[1].params.textDocument.version == 0,
    "case2: pre-admission edits keep one coherent initial version")

  doc.lsp_open = true
  doc.lines[2] = "tail\n"
  doc:raw_insert(2, 1, "tail", nil, 0)
  local batches = drain(server, "textDocument/didChange")
  ok(#batches == 1 and batch_version(batches[1]) == 1,
    "case2: post-admission batch starts the stream cleanly at version 1")
  ok(#batches[1].params.contentChanges == 1
    and batches[1].params.contentChanges[1].text == "tail",
    "case2: no pre-admission pending state leaks into the new session")
end

-- ===========================================================================
-- Case 3: save never resets or rewinds the protocol version stream and
-- didSave carries no version at all.
-- ===========================================================================
do
  local lsp = fresh_module_load()
  local server = make_server("perllsp", { capabilities = INCREMENTAL })
  register(lsp, "perllsp", server)
  local doc = make_doc("C:/proj/save.pl", { "a\n" })

  lsp.open_document(doc)
  drain(server)
  doc.lsp_open = true

  doc:raw_insert(1, 2, "b", nil, 0)
  ok(batch_version(drain(server, "textDocument/didChange")[1]) == 1,
    "case3: version 1 before save")

  lsp.save_document(doc)
  local saved = drain(server, "textDocument/didSave")
  ok(#saved == 1, "case3: didSave emitted")
  ok(saved[1].params.textDocument.version == nil,
    "case3: didSave carries document identity but no version rewind/reuse")
  local session = get_session(lsp, doc, server)
  ok(session ~= nil and session.version == 1,
    "case3: save leaves the owned version untouched")

  doc:raw_insert(1, 3, "c", nil, 0)
  local batches = drain(server, "textDocument/didChange")
  ok(batch_version(batches[1]) == 2,
    "case3: version continues increasing after save - no clean-state reset")
end

-- ===========================================================================
-- Case 4: undo/redo moves editor clean/change-id backwards but the protocol
-- version stream still increases; clean_change_id is never read.
-- ===========================================================================
do
  local lsp = fresh_module_load()
  local server = make_server("perllsp", { capabilities = INCREMENTAL })
  register(lsp, "perllsp", server)
  local doc = make_doc("C:/proj/undo.pl", { "x\n" })

  lsp.open_document(doc)
  drain(server)
  doc.lsp_open = true

  doc:raw_insert(1, 2, "y", nil, 0)
  ok(batch_version(drain(server, "textDocument/didChange")[1]) == 1,
    "case4: version 1 before undo")
  local change_id_after_edit = doc:get_change_id()

  -- Real Lite XL undo: pop_undo decrements undo_stack.idx and replays the
  -- inverse command through this SAME overridden Doc:raw_remove, targeting
  -- the REDO stack. get_change_id() therefore moves backward while the LSP
  -- plugin still observes an ordinary accepted mutation. Bytes revisit the
  -- prior state, and clean_change_id regresses with them.
  doc.undo_stack.idx = doc.undo_stack.idx - 1
  doc.clean_change_id = -7
  doc.lines[1] = "x\n"
  doc:raw_remove(1, 2, 1, 3, doc.redo_stack, 0)
  -- Instrument control: the editor identity really did move backward, so the
  -- assertions below are not vacuously true of a monotonic change id.
  ok(doc:get_change_id() < change_id_after_edit,
    "case4: editor get_change_id() moved BACKWARD across the undo")
  local batches = drain(server, "textDocument/didChange")
  ok(batch_version(batches[1]) == 2,
    "case4: undo still advances the protocol version to 2")

  -- Real Lite XL redo: pop_undo against the redo stack, replaying through
  -- Doc:raw_insert with the undo stack as the target.
  local change_id_after_undo = doc:get_change_id()
  doc.redo_stack.idx = doc.redo_stack.idx - 1
  doc.clean_change_id = 0
  doc.lines[1] = "xy\n"
  doc:raw_insert(1, 3, "y", doc.undo_stack, 0)
  ok(doc:get_change_id() == change_id_after_undo + 1,
    "case4: editor get_change_id() REVISITS a previously seen value on redo")
  batches = drain(server, "textDocument/didChange")
  ok(batch_version(batches[1]) == 3,
    "case4: redo keeps increasing despite change-id revisiting prior values")
  -- Deliberately NOT asserted here: `version ~= doc:get_change_id()`. Two
  -- independent counters may legitimately coincide, so that oracle would
  -- fail a correct implementation on an accident of fixture arithmetic. The
  -- discrimination this case needs comes from the two controls above (the
  -- change id verifiably moved backward, then revisited a used value) while
  -- the version stream did neither.
  local session = get_session(lsp, doc, server)
  ok(session ~= nil and session.server_generation ~= nil,
    "case4: session exposes explicit server generation identity")
end

-- ===========================================================================
-- Case 5: close/reopen of the same URI creates a NEW session identity even
-- with identical bytes; old queued state cannot publish into it.
-- ===========================================================================
do
  local lsp = fresh_module_load()
  local server = make_server("perllsp", { capabilities = INCREMENTAL })
  register(lsp, "perllsp", server)
  local doc = make_doc("C:/proj/reopen.pl", { "same\n" })

  lsp.open_document(doc)
  drain(server)
  doc.lsp_open = true
  local first = get_session(lsp, doc, server)
  ok(first ~= nil and first.session_generation == 1, "case5: first open is session generation 1")

  -- Queue a change and leave its batch unsent so the old session holds
  -- pending state (#10833): batches always queue - there is no admission
  -- hold anymore - so holding is a property of the unsent queue whose send
  -- callback has not run, not of a can_push gate.
  doc:raw_insert(1, 5, "stale", nil, 0)
  ok(first ~= nil and #first.pending_changes == 1,
    "case5: old session holds one unsent pending change before close")
  local stale_batches = drain(server, "textDocument/didChange")
  ok(#stale_batches == 1 and first ~= nil and #first.pending_changes == 0,
    "case5: sending the batch releases exactly the held change")

  lsp.close_document(doc)
  local closed = drain(server, "textDocument/didClose")
  ok(#closed == 1 and closed[1].params.textDocument.uri == expected_uri("C:/proj/reopen.pl"),
    "case5: didClose identifies the exact document session")
  ok(closed[1].params.textDocument.version == nil,
    "case5: didClose carries no editor-derived version")
  ok(get_session(lsp, doc, server) == nil,
    "case5: close terminates the session record")

  -- Reopen the same URI with identical bytes.
  lsp.open_document(doc)
  local reopened_opened = drain(server, "textDocument/didOpen")
  ok(#reopened_opened == 1 and reopened_opened[1].params.textDocument.version == 0,
    "case5: reopened session starts a fresh explicit version origin")
  doc.lsp_open = true
  local second = get_session(lsp, doc, server)
  ok(second ~= nil and second.session_generation == 2,
    "case5: reopen creates a new session generation for the same URI")
  ok(second ~= nil and second ~= first and #second.pending_changes == 0,
    "case5: old queued changes are inert - none survive into the new session")

  doc:raw_insert(1, 5, "!", nil, 0)
  local batches = drain(server, "textDocument/didChange")
  ok(#batches == 1 and #batches[1].params.contentChanges == 1
    and batches[1].params.contentChanges[1].text == "!",
    "case5: first batch of the new session carries only the new change")

  -- Reopening through a fresh Doc instance for the same URI still produces
  -- a distinct session generation (bytes and initial version identical).
  local fresh_doc = make_doc("C:/proj/reopen.pl", { "same\n" })
  lsp.close_document(doc)
  drain(server)
  lsp.open_document(fresh_doc)
  drain(server)
  local third = get_session(lsp, fresh_doc, server)
  ok(third ~= nil and third.session_generation == 3,
    "case5: close/reopen via a new document instance still advances identity")
end

-- ===========================================================================
-- Case 6: explicit server stop/start creates a new process generation; the
-- old instance's sessions die and its queue can never publish.
-- ===========================================================================
do
  local lsp = fresh_module_load()
  local old_server = make_server("perllsp", { capabilities = INCREMENTAL })
  register(lsp, "perllsp", old_server)
  local doc = make_doc("C:/proj/restart.pl", { "r\n" })

  lsp.open_document(doc)
  drain(old_server)
  doc.lsp_open = true
  doc:raw_insert(1, 2, "q", nil, 0)
  drain(old_server)
  local old_session = get_session(lsp, doc, old_server)
  local old_generation = old_session and old_session.server_generation or -1

  -- Old generation holds an unsent pending change that must stay inert.
  old_server.can_push_value = false
  doc:raw_insert(1, 3, "z", nil, 0)
  drain(old_server)
  old_server.can_push_value = true

  lsp.stop_servers()
  ok(old_server.exited == 1, "case6: stop exits the running server instance")
  ok(get_session(lsp, doc, old_server) == nil,
    "case6: sessions owned by the replaced process are terminated")
  local old_queue_depth = #old_server.outbound

  -- Replacement process generation.
  local new_server = make_server("perllsp", { capabilities = INCREMENTAL })
  register(lsp, "perllsp", new_server)
  lsp.open_document(doc)
  local reopened = drain(new_server)
  ok(#reopened >= 1 and reopened[1].method == "textDocument/didOpen",
    "case6: replacement session admitted a fresh didOpen")
  local new_session = get_session(lsp, doc, new_server)
  ok(new_session ~= nil and new_session.server_generation ~= old_generation,
    "case6: replacement process owns a distinct server generation")
  ok(new_session ~= nil and new_session.session_generation == 2,
    "case6: restart creates a new document-session identity")

  doc.lsp_open = true
  doc:raw_insert(1, 4, "w", nil, 0)
  local batches = drain(new_server, "textDocument/didChange")
  ok(#batches == 1 and batch_version(batches[1]) == 1,
    "case6: new generation starts a fresh version origin at 1")
  ok(#old_server.outbound == old_queue_depth,
    "case6: old process generation received nothing after replacement")
end

-- ===========================================================================
-- Case 7: full-sync and ranged-sync branches consume the same version owner
-- within one session.
-- ===========================================================================
do
  local lsp = fresh_module_load()
  local server = make_server("perllsp", { capabilities = FULL })
  register(lsp, "perllsp", server)
  local doc = make_doc("C:/proj/syncmode.pl", { "s\n" })

  lsp.open_document(doc)
  drain(server)
  doc.lsp_open = true
  local session = get_session(lsp, doc, server)

  doc:raw_insert(1, 2, "a", nil, 0)
  local raw_batches = drain(server, "textDocument/didChange")
  ok(#raw_batches == 1 and raw_batches[1].kind == "raw",
    "case7: full-sync branch emits the raw payload shape")
  ok(batch_version(raw_batches[1]) == 1,
    "case7: full-sync batch allocates the session's next version")

  server.capabilities.textDocumentSync.change = 2
  doc:raw_insert(1, 3, "b", nil, 0)
  local ranged_batches = drain(server, "textDocument/didChange")
  ok(#ranged_batches == 1 and ranged_batches[1].kind == "notification",
    "case7: ranged-sync branch emits the notification shape")
  ok(batch_version(ranged_batches[1]) == 2,
    "case7: ranged-sync batch continues the SAME owner's stream")
  ok(get_session(lsp, doc, server) == session,
    "case7: one session record spans both sync branches")
end

-- ===========================================================================
-- Case 8: Save As style URI transition replaces the live session with a new
-- independent identity; old pending state cannot publish to either side.
-- ===========================================================================
do
  local lsp = fresh_module_load()
  local server = make_server("perllsp", { capabilities = INCREMENTAL })
  register(lsp, "perllsp", server)
  local doc = make_doc("C:/proj/old.pl", { "o\n" })

  lsp.open_document(doc)
  drain(server)
  doc.lsp_open = true
  local old_session = get_session(lsp, doc, server)

  -- Unsent pending state in the old URI session.
  server.can_push_value = false
  doc:raw_insert(1, 2, "ld", nil, 0)
  drain(server)
  server.can_push_value = true

  -- Save As: the document identity transitions to a new URI.
  doc.filename = "C:/proj/new.pl"
  lsp.open_document(doc)

  local opened = drain(server, "textDocument/didOpen")
  ok(#opened == 1 and opened[1].params.textDocument.uri == expected_uri("C:/proj/new.pl"),
    "case8: new URI session opens with exact current text")
  ok(opened[1].params.textDocument.version == 0,
    "case8: new URI session starts at explicit version origin")
  local new_session = get_session(lsp, doc, server)
  ok(new_session ~= nil and new_session ~= old_session,
    "case8: Save As replaced the live session record")
  ok(old_session ~= nil and old_session.open_state == "closed",
    "case8: old URI session is explicitly terminated")
  ok(old_session ~= nil and #old_session.pending_changes == 0,
    "case8: old URI session's queued changes died with it")
  ok(new_session ~= nil
    and new_session.uri == expected_uri("C:/proj/new.pl")
    and old_session.uri == expected_uri("C:/proj/old.pl")
    and new_session.session_generation == 1,
    "case8: new session carries independent URI identity and generation")

  doc.lsp_open = true
  doc:raw_insert(1, 2, "n", nil, 0)
  local batches = drain(server, "textDocument/didChange")
  ok(#batches == 1 and batch_version(batches[1]) == 1
    and #batches[1].params.contentChanges == 1,
    "case8: new session's stream contains only post-transition changes")
end

-- ===========================================================================
-- Session API honesty: versions are never derived from editor identity.
-- ===========================================================================
do
  local lsp = fresh_module_load()
  local server = make_server("perllsp", { capabilities = INCREMENTAL })
  register(lsp, "perllsp", server)
  local doc = make_doc("C:/proj/api.pl", { "api\n" })
  doc.clean_change_id = 42
  -- Both editor-history identities are far from the session origin, so a
  -- version derived from either is distinguishable from the honest 0.
  doc.undo_stack.idx = 17

  lsp.open_document(doc)
  local opened = drain(server, "textDocument/didOpen")
  ok(opened[1].params.textDocument.version == 0,
    "api: didOpen ignores a nonzero editor clean_change_id as version source")
  -- Instrument control rather than a second `version ~= change_id` oracle:
  -- pin that the fixture really did seed both editor identities away from
  -- the session origin, which is what makes the assertion above meaningful.
  ok(doc:get_change_id() == 17 and doc.clean_change_id == 42,
    "api: both editor identities were seeded away from the session origin")
  local session = get_session(lsp, doc, server)
  ok(session ~= nil and type(session.version) == "number" and session.version == 0,
    "api: session owns the version field explicitly")
  ok(session ~= nil and session.uri == expected_uri("C:/proj/api.pl") and session.open_state == "open",
    "api: session record carries canonical uri and open state")
  ok(session ~= nil and session.pending_changes ~= nil and #session.pending_changes == 0,
    "api: session owns the pending change queue")
end

print(string.format("%d passed, %d failed", passed, failed))
os.exit(failed == 0 and 0 or 1)

--- mod-version:3
--
-- LSP client for lite-xl
-- @copyright Jefferson Gonzalez
-- @license MIT
--
-- Note: Annotations syntax documentation which is supported by
-- https://github.com/sumneko/lua-language-server can be read here:
-- https://emmylua.github.io/annotation.html

-- Staged exact upstream source for the lite-xl integration train.
-- Upstream subject: lite-xl/lite-xl-lsp init.lua
--   base ref : d1432ae0736cd9531798b4bc1221835f534cc689
--   base blob: 7b38c3a97c68877d2391753adb09e49ec57397d3
-- TODO Change the code to make it possible to use more than one LSP server
-- for a single file if possible and needed, for eg:
--   One lsp may not support goto definition but another one registered
--   for the current document filetype may do.

local core = require "core"
local common = require "core.common"
local config = require "core.config"
local command = require "core.command"
local style = require "core.style"
local keymap = require "core.keymap"
local translate = require "core.doc.translate"
local autocomplete = require "plugins.autocomplete"
local Doc = require "core.doc"
local DocView = require "core.docview"
local StatusView = require "core.statusview"
local RootView = require "core.rootview"
local LineWrapping
-- If the lsp plugin is loaded from users init.lua it will load linewrapping
-- even if it was disabled from the settings ui, so we queue this check since
-- there is no way to automatically load settings ui before the user module.
core.add_thread(function()
  if config.plugins.linewrapping or type(config.plugins.linewrapping) == "nil" then
    LineWrapping = require "plugins.linewrapping"
  end
end)

local json = require "plugins.lsp.json"
local util = require "plugins.lsp.util"
local listbox = require "plugins.lsp.listbox"
local diagnostics = require "plugins.lsp.diagnostics"
local Server = require "plugins.lsp.server"
-- Local patch (#11172): command availability is projected through the
-- capability manifest - a server capability alone never enables a command
-- whose client consumer is absent.
local capability_manifest = require "plugins.lsp.capability_manifest"
local Timer = require "plugins.lsp.timer"
local SymbolResults = require "plugins.lsp.symbolresults"
local MessageBox = require "libraries.widget.messagebox"
local snippets_found, snippets = pcall(require, "plugins.snippets")

---@type lsp.helpdoc
local HelpDoc = require "plugins.lsp.helpdoc"

--
-- Plugin settings
--

---Configuration options for the LSP plugin.
---@class config.plugins.lsp
---Set to a file path to log all json
---@field log_file string
---Setting to true prettyfies json for more readability on the log
---but this setting will impact performance so only enable it when
---in need of easy to read json output when developing the plugin.
---@field prettify_json boolean
---Show a symbol hover information when mouse cursor is on top.
---@field mouse_hover boolean
---The amount of time in milliseconds before showing the tooltip.
---@field mouse_hover_delay integer
---Show diagnostic messages
---@field show_diagnostics boolean
---Amount of milliseconds to delay updating the inline diagnostics.
---@field diagnostics_delay number
---Wether to enable snippets processing.
---@field snippets boolean
---Stop servers that aren't needed by any of the open files
---@field stop_unneeded_servers boolean
---Send a server stderr output to lite log
---@field log_server_stderr boolean
---Force verbosity off even if a server is configured with verbosity on
---@field force_verbosity_off boolean
---Yield when reading from LSP which may give you better UI responsiveness
---when receiving large responses, but will affect LSP performance.
---@field more_yielding boolean
config.plugins.lsp = common.merge({
  mouse_hover = true,
  mouse_hover_delay = 300,
  show_diagnostics = true,
  diagnostics_delay = 500,
  snippets = true,
  stop_unneeded_servers = true,
  log_file = "",
  prettify_json = false,
  log_server_stderr = false,
  force_verbosity_off = false,
  more_yielding = false,
  autostart_server = true,
  -- The config specification used by the settings gui
  config_spec = {
    name = "Language Server Protocol",
    {
      label = "Mouse Hover",
      description = "Show a symbol hover information when mouse cursor is on top.",
      path = "mouse_hover",
      type = "TOGGLE",
      default = true
    },
    {
      label = "Mouse Hover Delay",
      description = "The amount of time in milliseconds before showing the tooltip.",
      path = "mouse_hover_delay",
      type = "NUMBER",
      default = 300,
      min = 50,
      max = 2000
    },
    {
      label = "Diagnostics",
      description = "Show inline diagnostic messages with lint+.",
      path = "show_diagnostics",
      type = "TOGGLE",
      default = false
    },
    {
      label = "Diagnostics Delay",
      description = "Amount of milliseconds to delay the update of inline diagnostics.",
      path = "diagnostics_delay",
      type = "NUMBER",
      default = 500,
      min = 100,
      max = 10000
    },
    {
      label = "Snippets",
      description = "Snippets processing using lsp_snippets, may need a restart.",
      path = "snippets",
      type = "TOGGLE",
      default = true
    },
    {
      label = "Autostart Server",
      description = "Automatically start server when opening a file",
      path = "autostart_server",
      type = "TOGGLE",
      default = true
    },
    {
      label = "Stop Servers",
      description = "Stop servers that aren't needed by any of the open files.",
      path = "stop_unneeded_servers",
      type = "TOGGLE",
      default = true
    },
    {
      label = "Log File",
      description = "Absolute path to a '.log' file for logging all json.",
      path = "log_file",
      type = "FILE",
      filters = {"%.log$"}
    },
    {
      label = "Prettify JSON",
      description = "Prettify json for more readability but impacts performance.",
      path = "prettify_json",
      type = "TOGGLE",
      default = false
    },
    {
      label = "Log Standard Error",
      description = "Send a server stderr output to lite log.",
      path = "log_server_stderr",
      type = "TOGGLE",
      default = false
    },
    {
      label = "Force Verbosity Off",
      description = "Turn verbosity off even if a server is configured with verbosity on.",
      path = "force_verbosity_off",
      type = "TOGGLE",
      default = false
    },
    {
      label = "More Yielding",
      description = "Yield when reading from LSP which may give you better UI responsiveness.",
      path = "more_yielding",
      type = "TOGGLE",
      default = false
    }
  }
}, config.plugins.lsp)


--
-- Main plugin functionality
--
local lsp = {}

-- Local patch (#11128): install the editor-side rendering resolver once at
-- plugin load so every rendering surface -- including lintplus_populate()
-- paths that can run before any accepted publication (e.g. re-enabling
-- diagnostics) -- resolves live documents and revalidates subjects instead
-- of silently degrading to unproven columns.
diagnostics.set_render_resolver(
  function(uri)
    for _, open_doc in ipairs(core.docs) do
      if open_doc.filename
        and util.path_to_uri(core.project_absolute_path(open_doc.filename)) == uri
      then
        return open_doc
      end
    end
    return nil
  end,
  function(uri, provider, session_generation, version)
    local running = lsp.servers_running[provider]
    if not running then return false end
    local live = lsp.find_document_session(uri, running)
    if not live then return false end
    if live.session_generation ~= session_generation then
      return false
    end
    -- Unversioned publications are admitted under session identity only
    -- ("not_proven" evidence, never version-exact), so their currentness is
    -- exactly session identity; a numeric publication still requires exact
    -- version equality.
    if version == "not_proven" then
      return true
    end
    return live.version == version
  end
)

---List of registered servers
---@type table<string, lsp.server.options>
lsp.servers = {}

---List of running servers
---@type table<string, lsp.server>
lsp.servers_running = {}

-- Local patch (#11115): one explicit monotonic LSP document-version
-- authority. Every admitted document open owns one structured session
-- record per server/document tuple carrying the exact server process
-- generation, canonical URI identity, per-URI open-session generation, the
-- monotonic protocol version stream and all pending outbound content
-- changes. Editor undo/clean/dirty state (clean_change_id,
-- get_change_id(), cursor position, wall time or file mtime) is never read
-- as a version source: protocol versions belong to accepted wire batches,
-- not editor history. Restart, close/reopen and Save As terminate the old
-- record so stale pending state can never publish into a replacement
-- session. Session records are runtime state only; nothing here duplicates
-- workspace lifecycle ownership (#8979/#10660/#10715).
---@class lsp.document.session
---@field server_generation integer Server process generation owning the session
---@field uri string Canonical document URI captured at admission
---@field session_generation integer Open-generation of this URI (1-based)
---@field version integer Monotonic LSP protocol version within the session
---@field mutation_seq integer Accepted-mutation counter within the session
---@field open_state string "open" | "closed"
---@field pending_changes table[] Accepted changes awaiting the next batch

---Live open-document sessions keyed [doc][server] (#11115).
---@type table<table, table<lsp.server, lsp.document.session>>
lsp.document_sessions = {}

local document_session_server_generations = 0
local document_session_open_counters = {}

---Allocate the next server process generation (#11115). Each newly started
---server instance owns one distinct generation so old sessions are never
---compared as though they belong to a replacement process.
function lsp.next_server_generation()
  document_session_server_generations = document_session_server_generations + 1
  return document_session_server_generations
end

---Return the live open-document session for one doc/server pair or nil
---(#11115).
---@param doc core.doc
---@param server lsp.server
---@return lsp.document.session|nil
function lsp.get_document_session(doc, server)
  local by_server = lsp.document_sessions[doc]
  if not by_server then return nil end
  return by_server[server]
end

---Terminate one live session (#11115). Its pending change queue dies with
---it: queued-but-unsent batches of a dead session can never publish, and
---its retained diagnostics are invalidated together with it (#11124).
---@param doc core.doc
---@param server lsp.server
function lsp.terminate_document_session(doc, server)
  local by_server = lsp.document_sessions[doc]
  if not by_server then return end
  local session = by_server[server]
  if session then
    session.open_state = "closed"
    session.pending_changes = {}
    -- Local patch (#11124): lifecycle cleanup removes exactly this
    -- subject's retained publications.
    diagnostics.close_session(session.uri, session.session_generation)
    by_server[server] = nil
  end
  if not next(by_server) then
    lsp.document_sessions[doc] = nil
  end
end

---Create one fresh open-document session owned by the exact
---server/document/open tuple (#11115). Any prior live session for the pair
---is terminated first, so close/reopen reuses neither versions nor pending
---state, and Save As/URI transitions leave the old URI session inert while
---the new URI gets an explicit independent session identity (#10715 owns
---the wire-level transition ordering; this boundary owns the state).
---@param doc core.doc
---@param server lsp.server
---@param uri string Canonical document URI captured once for this session
---@return lsp.document.session
function lsp.create_document_session(doc, server, uri)
  lsp.terminate_document_session(doc, server)

  -- Pre-admission edits already reached the didOpen snapshot; their legacy
  -- queue is discarded so it can never leak into the new stream (#11115).
  if doc.lsp_changes then
    doc.lsp_changes[server] = nil
  end

  -- Persist the process generation on first admission so every later
  -- subject of this instance carries the identical identity (#11115/#11108).
  server.generation = server.generation or lsp.next_server_generation()
  -- Local patch (#11124): refresh provider liveness so held publications
  -- from replaced processes are recognized as dead.
  diagnostics.note_provider(server.name, server.generation)
  local previous_generation = document_session_open_counters[uri] or 0
  local session = {
    server_generation = server.generation,
    uri = uri,
    session_generation = previous_generation + 1,
    version = 0,
    mutation_seq = 0,
    open_state = "open",
    pending_changes = {},
  }
  document_session_open_counters[uri] = session.session_generation
  lsp.document_sessions[doc] = lsp.document_sessions[doc] or {}
  lsp.document_sessions[doc][server] = session
  return session
end

-- Local patch (#11108): one immutable request subject per document/root/
-- provider-bound request, admitted against the live session state before
-- any callback mutates UI, diagnostics, bytes, lists or navigation. A held
-- response may stay useful for logs but can never become current product
-- state once its subject is stale: same caret position is never proof of
-- the same document/server state. Dispositions map this client's topology:
-- "generation_replaced" covers both process replacement and provider switch
-- (one running instance per name), "superseded" covers URI/session
-- transitions (Save As / close-reopen), "document_closed" covers terminated
-- sessions, and "stale"/"cursor_moved" cover document and request-specific
-- currentness (#10657 owns IDs/timeouts; #10833 owns queue supersession;
-- neither defines admission, which lives here).
---@class lsp.request.subject
---@field method string Request method for evidence
---@field server lsp.server Exact running instance owning the request
---@field server_name string
---@field server_generation integer
---@field doc core.doc|nil Document instance identity, when document-bound
---@field uri string|nil Canonical URI at send time
---@field version integer|nil Session version at send time
---@field session_generation integer|nil Open-session generation at send time
---@field mutation_seq integer|nil Accepted-mutation count at send time
---@field line integer|nil Request cursor/query identity, when applicable
---@field col integer|nil

---Capture one immutable request subject (#11108). Returns nil when a
---document-bound request has no live session to bind to: such a request is
---refused instead of sent unprovable.
---@param method string
---@param doc core.doc|nil
---@param server lsp.server
---@param line integer|nil
---@param col integer|nil
---@return lsp.request.subject|nil
function lsp.make_request_subject(method, doc, server, line, col)
  local subject = {
    method = method,
    server = server,
    server_name = server.name,
    -- Persisted instance generation: identical for every request of this
    -- running process, distinct across replacement instances (#11108).
    server_generation = server.generation or (function()
      server.generation = lsp.next_server_generation()
      return server.generation
    end)(),
    line = line,
    col = col,
  }
  if doc then
    local session = lsp.get_document_session(doc, server)
    if not session then return nil end
    subject.doc = doc
    subject.uri = session.uri
    subject.version = session.version
    subject.session_generation = session.session_generation
    subject.mutation_seq = session.mutation_seq or 0
  end
  return subject
end

---Admit one response against its captured subject (#11108).
---@param subject lsp.request.subject|nil
---@return boolean admitted
---@return string|nil disposition Typed rejection token when not admitted
function lsp.admit_response(subject)
  if not subject then return false, "unbound" end

  -- Server instance identity: a replacement instance under the same name,
  -- from restart or provider switch, can never admit old subjects.
  local running = lsp.servers_running[subject.server_name]
  if running ~= subject.server then
    return false, "generation_replaced"
  end
  if (running.generation or 0) ~= subject.server_generation then
    return false, "generation_replaced"
  end

  if subject.doc then
    if not subject.doc.lsp_open then return false, "document_closed" end
    local session = lsp.get_document_session(subject.doc, running)
    if not session then return false, "document_closed" end
    if session.uri ~= subject.uri then return false, "superseded" end
    if session.session_generation ~= subject.session_generation then
      return false, "superseded"
    end
    -- The response describes exactly the captured accepted state: any
    -- emitted batch or accepted mutation since makes it stale.
    if subject.version and session.version ~= subject.version then
      return false, "stale"
    end
    if subject.mutation_seq
      and (session.mutation_seq or 0) ~= subject.mutation_seq
    then
      return false, "stale"
    end
  end

  -- Request-specific cursor identity: reusing a position after intervening
  -- edits does not revive an old subject (the stream checks above already
  -- rejected that); this catches caret movement alone.
  if subject.line and subject.doc then
    local cline, ccol = subject.doc:get_selection()
    if cline ~= subject.line or ccol ~= subject.col then
      return false, "cursor_moved"
    end
  end

  return true, nil
end

---Flag that indicates if last autocomplete request was a trigger
---to prevent requesting another autocompletion request until the
---autocomplete box is hidden since some lsp servers loose context
---and return wrong results (eg: lua-language-server)
---@type boolean
lsp.in_trigger = false

---Flag that indicates if the user typed something on the editor to try and
---call autocomplete only when neccesary.
---@type boolean
lsp.user_typed = false

---Used on the hover timer to display hover info
---@class lsp.hover_position
---@field doc core.doc | nil
---@field x number
---@field y number
---@field triggered boolean
---@field utf8_range table | nil
lsp.hover_position = {doc = nil, x = -1, y = -1, triggered = false, utf8_range = nil}

---@type lsp.timer
lsp.hover_timer = Timer(300, true)
lsp.hover_timer.on_timer = function()
  local doc, line, col = lsp.get_hovered_location(lsp.hover_position.x, lsp.hover_position.y)
  if not doc then return end
  lsp.hover_position.triggered = true
  lsp.hover_position.utf8_range = nil
  lsp.hover_position.doc = doc
  lsp.request_hover(doc, line, col)
end

--
-- Private functions
--

---Generate an lsp location object
---@param doc core.doc
---@param line integer
---@param col integer
local function get_buffer_position_params(doc, line, col)
  -- Local patch (#11165): the wire identity comes from the one file URI/path
  -- conversion authority; an unconvertible editor path drops the request
  -- instead of sending a fabricated URI.
  local uri = util.path_to_uri(core.project_absolute_path(doc.filename))
  if not uri then
    core.log_quiet(
      "[LSP] request dropped, unconvertible path: %s",
      tostring(doc.filename)
    )
    return nil
  end
  return {
    textDocument = {
      uri = uri,
    },
    position = {
      line = line - 1,
      character = util.doc_utf8_to_utf16(doc, line, col) - 1
    }
  }
end

---Recursive function to generate a list of symbols ready
---to use for the lsp.request_document_symbols() action.
---@param list table<integer, table>
---@param parent? string
local function get_symbol_lists(list, parent)
  -- Local patch (#11198): rendered display strings are presentation, not
  -- identity. Rows are stored under one collision-free internal key per
  -- returned item - the first occurrence of a rendered path keeps it as the
  -- internal key, later occurrences gain a deterministic source-order
  -- suffix - array results are traversed in numeric order, and every row
  -- retains its own protocol facts so duplicate names/kinds stay
  -- independently selectable. The user-visible text is derived from the key
  -- without the disambiguation segment, so visible symbol names never
  -- change and protocol objects are never mutated by identity.
  --
  -- Review adoption (PR #12670): two follow-up seams. First, the opaque
  -- disambiguation ordinal lives only inside the internal key; fuzzy search
  -- runs over the suffix-free rendered subject, so hidden ordinals can
  -- never leak into scoring, ordering, or query hits. Second, every row
  -- carries immutable identity metadata (unique row id, originating parent
  -- row id, complete source index path) beside - never inside - its display
  -- and result fields: children of duplicate parents keep distinguishable
  -- identity even though their rendered container string is identical.
  local symbols = {}
  local symbol_names = {}
  local search_subjects = {}
  parent = parent or ""
  parent = #parent > 0 and (parent .. "/") or parent

  ---Deep-first traversal that flattens one branch of returned symbols into
  ---the shared namespace of rows, keeping every row's facts attached to its
  ---own collision-free key. One flattening pass mints keys exactly once, so
  ---the suffix counter stays deterministic across duplicate branches.
  local function visit(items, display_parent, parent_row_id, index_prefix)
    local display_scope = #display_parent > 0 and (display_parent .. "/") or ""
    for index, symbol in ipairs(items) do
      -- Include symbol kind to be able to filter by it
      local display_name = display_scope
        .. symbol.name
        .. "||" .. Server.get_symbol_kind(symbol.kind)

      local row = {
        kind = symbol.kind,
        name = symbol.name,
        result_index = index,
        container = #display_scope > 0
          and string.sub(display_scope, 1, -2) or nil,
      }
      if symbol.detail then row.detail = symbol.detail end
      if symbol.deprecated ~= nil then row.deprecated = symbol.deprecated end
      if symbol.tags then row.tags = symbol.tags end

      if symbol.location then
        row.location = symbol.location
      else
        if symbol.range then
          row.range = symbol.range
        end
        if symbol.selectionRange then
          row.selectionRange = symbol.selectionRange
        end
        if symbol.uri then
          row.uri = symbol.uri
        end
      end

      local key = display_name
      if symbols[key] then
        local occurrence = 2
        while symbols[display_name .. "#" .. occurrence] do
          occurrence = occurrence + 1
        end
        key = display_name .. "#" .. occurrence
      end

      -- Immutable identity metadata, kept apart from presentation fields.
      local index_path = {}
      for path_depth = 1, #index_prefix do
        index_path[path_depth] = index_prefix[path_depth]
      end
      index_path[#index_path + 1] = index

      symbols[key] = row
      table.insert(symbol_names, key)
      table.insert(search_subjects, display_name)

      row.key = key
      row.row_id = #symbol_names
      row.parent_row_id = parent_row_id
      row.source_index_path = index_path

      if symbol.children and #symbol.children > 0 then
        visit(
          symbol.children, display_scope .. symbol.name, row.row_id, index_path
        )
      end
    end
  end

  visit(list, "", nil, {})

  return symbols, symbol_names, search_subjects
end

local function log(server, message, ...)
  if server.verbose then
    core.log("["..server.name.."] " .. message, ...)
  else
    core.log_quiet("["..server.name.."] " .. message, ...)
  end
end

---Check if active view is a DocView and return it
---@return core.docview|nil
local function get_active_docview()
  local av = core.active_view
  if getmetatable(av) == DocView and av.doc and av.doc.filename then
    return av
  end
  return nil
end

---Generates a code preview of a location
---@param location table
local function get_location_preview(location)
  local line1, col1 = util.toselection(
    location.range or location.targetRange
  )
  -- Local patch (#11165): non-file or malformed URIs yield no preview
  -- instead of being treated as local filenames.
  local path, path_reason = util.uri_to_path(location.uri or location.targetUri)
  if not path then
    core.log_quiet(
      "[LSP] preview unavailable (%s)",
      path_reason or "unconvertible uri"
    )
    return "", ""
  end
  local filename = core.normalize_to_project_dir(path)
  local abs_filename = core.project_absolute_path(filename)

  local file = io.open(abs_filename)

  if not file then
    return "", filename .. ":" .. tostring(line1) .. ":" .. tostring(col1)
  end

  local preview = ""

  -- sometimes the lsp can send the location of a definition where the
  -- doc comments should be written but if no docs are written the line
  -- is empty and subsequent line is the one we are interested in.
  local line_count = 1
  for line in file:lines() do
    if line_count >= line1 then
      preview = line:gsub("^%s+", "")
        :gsub("%s+$", "")

      if preview ~= "" then
        break
      else
        -- change also the location table
        if location.range then
          location.range.start.line = location.range.start.line + 1
          location.range['end'].line = location.range['end'].line + 1
        elseif location.targetRange then
          location.targetRange.start.line = location.targetRange.start.line + 1
          location.targetRange['end'].line = location.targetRange['end'].line + 1
        end
      end
    end
    line_count = line_count + 1
  end
  file:close()

  local position = filename .. ":" .. tostring(line1) .. ":" .. tostring(col1)

  return preview, position
end

---Generate a list ready to use for the lsp.request_references() action.
---@param locations table
local function get_references_lists(locations)
  local references, reference_names = {}, {}

  for _, location in pairs(locations) do
    local preview, position = get_location_preview(location)
    local name = preview .. "||" .. position
    table.insert(reference_names, name)
    references[name] = location
  end

  return references, reference_names
end

---Apply an lsp textEdit to a document if possible.
---@param server lsp.server
---@param doc core.doc
---@param text_edit table
---@param is_snippet boolean
---@param update_cursor_position boolean
---@return boolean True on success
local function apply_edit(server, doc, text_edit, is_snippet, update_cursor_position)
  local range = nil

  if text_edit.range then
    range = text_edit.range
  elseif text_edit.insert then
    range = text_edit.insert
  elseif text_edit.replace then
    range = text_edit.replace
  end

  if not range then return false end

  local text = text_edit.newText
  local line1, col1, line2, col2
  local current_text = ""

  if
    not server.capabilities.positionEncoding
    or
    server.capabilities.positionEncoding == Server.position_encoding_kind.UTF16
  then
    line1, col1, line2, col2 = util.toselection(range, doc)
  else
    line1, col1, line2, col2 = util.toselection(range)
    core.error(
      "[LSP] Unsupported position encoding: ",
      server.capabilities.positionEncoding
    )
  end

  if lsp.in_trigger then
    local cline2, ccol2 = doc:get_selection()
    local cline1, ccol1 = doc:position_offset(line2, col2, translate.start_of_word)
    current_text = doc:get_text(cline1, ccol1, cline2, ccol2)
  end

  doc:remove(line1, col1, line2, col2+#current_text)

  if is_snippet and snippets_found and config.plugins.lsp.snippets then
    doc:set_selection(line1, col1, line1, col1)
    snippets.execute {format = 'lsp', template = text}
    return true
  end

  doc:insert(line1, col1, text)
  if update_cursor_position then
    doc:move_to_cursor(nil, #text)
  end

  return true
end

-- Local patch (#11188): completionItem/resolve is an explicit, generation-
-- bound pre-application operation owned by per-item state, not a hover side
-- effect. Hover may prefetch one resolve; selection joins the same operation
-- and never mutates the document while resolution is pending. The full item
-- travels as received (`completion_item.data` is not a protocol
-- requirement), responses admit against their captured subject (#11108),
-- timeouts arrive through the per-request timeout seam (#10657), and queue
-- rejection surfaces as a typed failed terminal (#10833). Resolved fields
-- feed one validated application; late/stale results can never touch a newer
-- document, provider, or server generation.
--
-- Item resolve states: not_needed | unresolved | in_flight | resolved |
-- failed | timed_out | stale, with an exactly-once applied flag.

---Deterministic structural digest of one CompletionItem (#11188). Item
---identity is content identity: display labels or menu positions never stand
---in for the same item subject.
local function completion_item_digest(item)
  local function encode(value, depth)
    if depth > 6 then return "#" end
    local vtype = type(value)
    if vtype == "table" then
      local keys = {}
      for key in pairs(value) do keys[#keys + 1] = tostring(key) end
      table.sort(keys)
      local inner = {}
      for _, key in ipairs(keys) do
        inner[#inner + 1]
          = "[" .. tostring(key) .. "]=" .. encode(value[key], depth + 1)
      end
      return "{" .. table.concat(inner, ",") .. "}"
    end
    return vtype .. ":" .. tostring(value)
  end
  if type(item) ~= "table" then return tostring(item) end
  return encode(item, 0)
end

---Resolve-support disposition for one server (#11188).
local function completion_resolve_supported(server)
  local capabilities = server.capabilities or {}
  local provider = capabilities.completionProvider or {}
  return provider.resolveProvider == true
end

---One structured pre-apply resolve state per completion item (#11188).
---@param server lsp.server
---@param completion_item table Original CompletionItem as received
---@param round_subject lsp.request.subject|nil Subject of the completion round
---@return table resolve_state
local function new_completion_resolve_state(server, completion_item, round_subject)
  local supported = completion_resolve_supported(server)
  return {
    supported = supported,
    original_digest = completion_item_digest(completion_item),
    round_subject = round_subject,
    state = supported and "unresolved" or "not_needed",
    resolved_item = nil,
    resolve_subject = nil,
    pending_apply = false,
    applied = false,
    disposition = nil,
  }
end

---True when the original item alone carries every field the application
---paths consume (#11188 declared completeness policy): its own textEdit or
---an LSP-snippet insertText. Plain-text insertText/label items are not
---applied by any path without resolution supplying a textEdit.
local function completion_self_complete(item)
  if item.textEdit then return true end
  if
    snippets_found
    and item.insertText
    and item.insertTextFormat == Server.insert_text_format.Snippet
  then
    return true
  end
  return false
end

---Resolved view over the original item (#11188): resolved fields win; fields
---the server left unset inherit the original content.
local function overlay_resolved_item(original, resolved)
  local merged = {}
  for key, value in pairs(original) do merged[key] = value end
  for key, value in pairs(resolved) do merged[key] = value end
  return merged
end

---Merge one admitted resolve result into the hovered item description.
local function merge_resolve_description(item, symbol)
  if symbol.detail and #item.desc <= 0 then
    item.desc = symbol.detail
  end
  if symbol.documentation then
    if #item.desc > 0 then
      item.desc = item.desc .. "\n\n"
    end
    if
      type(symbol.documentation) == "table"
      and
      symbol.documentation.value
    then
      item.desc = item.desc .. symbol.documentation.value
      if
        symbol.documentation.kind
        and
        symbol.documentation.kind == "markdown"
      then
        item.desc = util.strip_markdown(item.desc)
      end
    else
      item.desc = item.desc .. symbol.documentation
    end
  end
  item.desc = item.desc:gsub("[%s\n]+$", "")
    :gsub("^[%s\n]+", "")
    :gsub("\n\n\n+", "\n\n")
end

---Apply the selected item exactly once from its final effective fields
---(#11188). Resolution outcomes decide the effective item: a resolved item
---overlays the original; a not_needed item applies as received; failed,
---timed_out, and stale terminals fall back only when the original alone
---proves its own application surface (its own textEdit or LSP-snippet
---insertText - fields resolution would only enrich) and otherwise refuse
---without partial mutation. Every application revalidates the captured
---round subject before any effect, so a terminal that arrives after edits,
---session transitions, or server replacement can never touch newer bytes,
---and the edit lands only in the exact document the round was computed for.
local function apply_selected_completion(item, rstate)
  if rstate.applied then return true end
  local original = item.data.completion_item
  local round_subject = rstate.round_subject or item.data.subject
  if round_subject then
    local admitted, disposition = lsp.admit_response(round_subject)
    if not admitted then
      core.log_quiet(
        "[LSP] completion apply refused (%s)", disposition or "stale"
      )
      return false
    end
  end
  local effective = nil
  if rstate.state == "resolved" then
    effective = rstate.resolved_item
      and overlay_resolved_item(original, rstate.resolved_item)
      or original
  elseif rstate.state == "not_needed" then
    effective = original
  else
    if completion_self_complete(original) then
      effective = original
    else
      core.log_quiet(
        "[LSP] completion apply refused (%s)",
        rstate.disposition or rstate.state
      )
      return false
    end
  end

  local dv = get_active_docview()
  -- Deferred terminals re-fetch the active view: applying a completion to a
  -- different document than the one its ranges were computed for is refusal,
  -- not adaptation (#11188).
  if
    dv
    and round_subject
    and round_subject.doc
    and dv.doc ~= round_subject.doc
  then
    core.log_quiet("[LSP] completion apply refused (%s)", "document_mismatch")
    return false
  end
  local edit_applied = false
  if effective.textEdit then
    if dv then
      local is_snippet = effective.insertTextFormat
        and effective.insertTextFormat == Server.insert_text_format.Snippet
      edit_applied = apply_edit(item.data.server, dv.doc, effective.textEdit, is_snippet, true)
      if edit_applied then
        -- Retrigger code completion if last char is a trigger
        -- this is useful for example with clangd when autocompleting
        -- a #include, if user types < a list of paths will appear
        -- when selecting a path that ends with / as <AL/ the
        -- autocompletion will be retriggered to show a list of
        -- header files that belong to that directory.
        lsp.in_trigger = false
        local line, col = dv.doc:get_selection()
        local char = dv.doc:get_char(line, col-1)
        local char_prev = dv.doc:get_char(line, col-2)
        if char:match("%p") or (char == " " and char_prev:match("%p")) then
          -- Local patch (#11108/#11115): pending state is owned by the
          -- live session, not the legacy doc field.
          local session = lsp.get_document_session(dv.doc, item.data.server)
          if session and #session.pending_changes > 0 then
            lsp.update_document(dv.doc, true)
          else
            lsp.request_completion(dv.doc, line, col, true)
          end
        end
      end
    end
  elseif
    dv and snippets_found and config.plugins.lsp.snippets
    and
    effective.insertText and effective.insertTextFormat
    and
    effective.insertTextFormat == Server.insert_text_format.Snippet
  then
    ---@type core.doc
    local doc = dv.doc
    if dv then
      local line2, col2 = doc:get_selection()
      local line1, col1 = doc:position_offset(line2, col2, translate.start_of_word)
      doc:set_selection(line1, col1, line2, col2)
      snippets.execute {format = 'lsp', template = effective.insertText}
      edit_applied = true
    end
  end
  if edit_applied and effective.additionalTextEdits and #effective.additionalTextEdits > 0 then
    -- Apply the edits in reverse order, so that their ranges are not shifted
    -- around by previous edits
    for i=#effective.additionalTextEdits,1,-1 do
      local edit = effective.additionalTextEdits[i]
      apply_edit(item.data.server, dv.doc, edit, false, false)
    end
  end
  -- Local patch (#11189): a colliding label's internal menu key carries the
  -- "#N" disambiguation suffix, and the autocomplete plugin's plain fallback
  -- inserts the key text verbatim. For suffix-carrying rows only, the exact
  -- pre-suffix insert target is applied here instead, so the internal
  -- identity encoding can never leak into the document. Unsuffixed rows keep
  -- the legacy plugin fallback byte-for-byte (key equals insert target).
  if
    not edit_applied
    and dv
    and item.data.insert_text
    and item.data.insert_text ~= item.text
  then
    dv.doc:text_input(item.data.insert_text)
    edit_applied = true
  end
  if edit_applied then
    rstate.applied = true
  end
  return edit_applied
end

---Terminal handling of one completionItem/resolve response for its item
---subject (#11188). Admission comes before any effect; only an exact-current
---result may resolve the state, update the visible description, or run a
---deferred selection application.
local function on_completion_resolve_response(item, rstate, response)
  local admitted, disposition = lsp.admit_response(rstate.resolve_subject)
  if not admitted then
    rstate.state = "stale"
    rstate.disposition = disposition or "stale"
    rstate.pending_apply = false
    core.log_quiet(
      "[LSP] %s response dropped (%s)",
      "completionItem/resolve", rstate.disposition
    )
    return
  end
  local result = response.result
  if response.error then
    -- A JSON-RPC error is a failed terminal, not an empty resolution: the
    -- pending selection must hit the completeness-guarded fallback instead
    -- of treating the original as confirmed (#11188).
    rstate.state = "failed"
    rstate.disposition = "server_error"
  elseif result then
    rstate.state = "resolved"
    rstate.resolved_item = result
    merge_resolve_description(item, result)
  else
    -- Null result: the server supplied nothing new; the application falls
    -- back through the same guarded original-item terminal.
    rstate.state = "failed"
    rstate.disposition = "empty_result"
  end
  if rstate.pending_apply then
    rstate.pending_apply = false
    apply_selected_completion(item, rstate)
  end
end

---Start at most one completionItem/resolve for an unresolved item subject
---(#11188). Hover prefetch and selection land here, so one item owns one
---request; the full item travels as received.
local function begin_completion_resolve(item, rstate)
  if rstate.state ~= "unresolved" then return rstate end
  local data = item.data
  local doc = rstate.round_subject and rstate.round_subject.doc or nil
  local subject = lsp.make_request_subject(
    'completionItem/resolve', doc, data.server, nil, nil)
  if not subject then
    rstate.state = "stale"
    rstate.disposition = "no_session"
    if rstate.pending_apply then
      rstate.pending_apply = false
      apply_selected_completion(item, rstate)
    else
      core.log_quiet("[LSP] completion apply refused (%s)", "no_session")
    end
    return rstate
  end
  rstate.state = "in_flight"
  rstate.resolve_subject = subject
  local queued = data.server:push_request('completionItem/resolve', {
    params = data.completion_item,
    -- Local patch (#10657): explicit short window. The single-send default
    -- policy is deliberately patient, but a pending resolve defers applying
    -- the selected completion, so it keeps the legacy ~2s responsiveness:
    -- expiry reaches the typed fallback quickly instead of stalling the
    -- selection for the whole default window.
    timeout = 2,
    callback = function(server, response)
      on_completion_resolve_response(item, rstate, response)
    end,
    timeout_callback = function()
      if rstate.state ~= "in_flight" then return end
      rstate.state = "timed_out"
      rstate.disposition = "timeout"
      if rstate.pending_apply then
        rstate.pending_apply = false
        apply_selected_completion(item, rstate)
      end
    end,
  })
  if queued == "not_queued" then
    rstate.state = "failed"
    rstate.disposition = "not_queued"
    if rstate.pending_apply then
      rstate.pending_apply = false
      apply_selected_completion(item, rstate)
    else
      core.log_quiet("[LSP] completion apply refused (%s)", "not_queued")
    end
  end
  return rstate
end

---Callback given to autocomplete plugin which is executed once for each
---element of the autocomplete box which is hovered with the idea of providing
---better description of the selected element by requesting the LSP server for
---detailed information/documentation.
---@param index integer
---@param item table
local function autocomplete_onhover(index, item)
  local completion_item = item.data.completion_item

  if item.data.server.verbose then
    item.data.server:log(
      "Resolve item: %s", util.jsonprettify(json.encode(completion_item))
    )
  end

  -- Local patch (#11188): hover starts at most one resolve prefetch for the
  -- item subject; selection joins the same operation instead of sending a
  -- duplicate. Description updates come only from an admitted exact-current
  -- result in the resolve callback.
  local rstate = item.data.resolve
  if rstate and rstate.supported then
    begin_completion_resolve(item, rstate)
  end
end

---Callback that handles insertion of an autocompletion item that has
---the information of insertion
---@param index integer
---@param item table
local function autocomplete_onselect(index, item)
  -- Local patch (#11108): a completion edit computed for one accepted
  -- document state is revalidated against its stored subject at the
  -- moment of user selection; stale edits are never applied optimistically
  -- against newer bytes.
  if item.data.subject then
    local admitted, disposition = lsp.admit_response(item.data.subject)
    if not admitted then
      core.log_quiet(
        "[LSP] completion edit refused (%s)", disposition or "stale"
      )
      return false
    end
  end

  -- Local patch (#11188): selection obtains an exact resolved/current item or
  -- a typed disposition before any document mutation. An unresolved or
  -- in-flight item defers application to its resolve terminal instead of
  -- applying whatever fields happen to be present; each item applies at most
  -- once regardless of repeated callbacks.
  local rstate = item.data.resolve
  if not rstate then
    return apply_selected_completion(item, { state = "not_needed", applied = false })
  end
  if rstate.applied then
    return true
  end
  if
    not rstate.supported
    or rstate.state == "not_needed"
    or rstate.state == "resolved"
    or rstate.state == "failed"
    or rstate.state == "timed_out"
    or rstate.state == "stale"
  then
    return apply_selected_completion(item, rstate)
  end
  if rstate.state == "in_flight" then
    rstate.pending_apply = true
    return false
  end
  -- unresolved: selection triggers the exact pre-apply resolution itself.
  rstate.pending_apply = true
  begin_completion_resolve(item, rstate)
  if not rstate.pending_apply then
    -- The operation terminated synchronously (typed queue rejection or a
    -- missing session): its guarded terminal already fell back or refused,
    -- so selection surfaces that real outcome instead of a deferral.
    return rstate.applied
  end
  return false
end

--
-- Public functions
--

---Open a document location returned by LSP
---@param location table
function lsp.goto_location(location)
  -- Local patch (#11165): navigation only follows convertible local files;
  -- non-file and malformed URIs are refused instead of opened as paths.
  local path, path_reason = util.uri_to_path(location.uri or location.targetUri)
  if not path then
    core.log_quiet(
      "[LSP] location not navigable (%s)",
      path_reason or "unconvertible uri"
    )
    return
  end
  local doc_view = core.root_view:open_doc(
    core.open_doc(
      common.home_expand(path)
    )
  )
  local line1, col1 = util.toselection(
    location.range or location.targetRange, doc_view.doc
  )
  doc_view.doc:set_selection(line1, col1, line1, col1)
end

lsp.get_location_preview = get_location_preview

---Register an LSP server to be launched on demand
---@param options lsp.server.options
function lsp.add_server(options)
  local required_fields = {
    "name", "language", "file_patterns", "command"
  }

  for _, field in pairs(required_fields) do
    if not options[field] then
      core.error(
        "[LSP] You need to provide a '%s' field for the server.",
        field
      )
      return false
    end
  end

  if snippets_found and config.plugins.lsp.snippets then
    options.snippets = true
  end

  if #options.command <= 0 then
    core.error("[LSP] Provide a command table list with the lsp command.")
    return false
  end

  -- On Windows using cmd.exe allows us to take advantage of its ability to run
  -- the correct executable, as well as running scripts.
  if PLATFORM == "Windows" and not options.windows_skip_cmd then
    local escaped_commands = { }
    if type(options.command) == "string" then
      options.command = { options.command }
    end
    -- We need to escape `"` as `"""`
    for _, v in ipairs(options.command) do
      table.insert(escaped_commands, '"' .. string.gsub(v, '"', '"""') .. '"')
    end
    -- The result should be something like `cmd.exe /C ""first" "second" "third""`
    options.command = 'cmd.exe /C "' .. table.concat(escaped_commands, " ") .. '"'
  end

  if config.plugins.lsp.force_verbosity_off then
    options.verbose = false
  end

  lsp.servers[options.name] = options

  return true
end

---Get valid running lsp servers for a given filename
---@param filename string
---@param initialized boolean
---@return table active_servers
function lsp.get_active_servers(filename, initialized)
  local servers = {}
  for name, server in pairs(lsp.servers) do
    if common.match_pattern(filename, server.file_patterns) then
      if lsp.servers_running[name] then
        local add_server = true
        if
          initialized
          and
          (
            not lsp.servers_running[name].initialized
            or
            not lsp.servers_running[name].capabilities
          )
        then
          add_server = false
        end
        if add_server then
          table.insert(servers, name)
        end
      end
    end
  end
  return servers
end

-- Used on lsp.get_workspace_settings()
local cached_workspace_settings = {}
local cached_workspace_settings_stamp = {}
local cached_workspace_stamp_paths = {}
local cached_workspace_settings_timestamp = 0

---Local patch (#10653): one bounded stat identity for a candidate
---configuration file. Used both to fingerprint a freshly loaded settings
---result and to detect that an accepted configuration changed while a
---cached entry is still inside its freshness window.
---@param file_path string
---@return string
local function config_file_stamp(file_path)
  local info = system
    and system.get_file_info
    and system.get_file_info(file_path)
  if info then
    -- Lite XL reports modification time as `modified`; `mtime` keeps the
    -- stamp honest under alternative runtimes of the same family.
    return tostring(info.modified or info.mtime or info.size or "present")
  end
  return "absent"
end

---Recompute the stamp of a recorded candidate list and compare identities.
---@param stamp_paths string[]
---@return string
local function recorded_stamp(stamp_paths)
  local parts = {}
  for index, file_path in ipairs(stamp_paths) do
    parts[index] = file_path .. "=" .. config_file_stamp(file_path)
  end
  return table.concat(parts, ";")
end

---Get table of configuration settings in the following way:
---1. Scan the USERDIR for .lite_lsp.lua or .lite_lsp.json (in that order)
---2. Merge server.settings
---3. Scan server.path also for configuration and merge it
---4. Scan workspace if set also for configuration and merge it
---Note: settings are cached for 5 seconds for faster retrieval
---      on repetitive calls to this function.
---
---Local patch (#11143): documented source precedence, preserved unchanged.
---Positions are visited in order USERDIR first, then server.path (when no
---explicit workspace is given) or the workspace directory last. Within each
---position the discovered file value is one candidate; at position 1,
---server.settings overrides that candidate through deep_merge(candidate,
---server.settings) before it joins the accumulator, so user-defined server
---options outrank only their own position's discovered file. Each later
---position then overrides everything before it through deep_merge(accumulated,
---position_value). Values combine under the #11143 typed merge contract:
---objects recurse, arrays replace atomically (an explicit empty array clears
---a list), scalars and explicit null replace exactly. The 5-second cache
---stores exactly this merged result, so cached and uncached consumers see
---identical effective settings.
---
---Local patch (#10653): workspace/project configuration is data, not
---executable code. Only the USERDIR keeps its historical executable
---.lite_lsp.lua authority as a user-owned configuration root. Every
---project-derived position (server.path or a server-supplied workspace
---scope) accepts data-only .lite_lsp.json configuration; a repository-local
---.lite_lsp.lua is never probed for execution there - it is ignored with a
---quiet log line, and a malformed project JSON payload is reported as a
---bounded configuration error answering an empty value instead of executing
---fallback code. Cache entries carry a filesystem stamp of their accepted
---candidates (each position directory plus its discovered file), so a
---changed or replaced accepted configuration invalidates the cached result
---even inside the freshness window.
---@param server lsp.server
---@param workspace? string
---@return table
function lsp.get_workspace_settings(server, workspace)
  -- Search settings on the following directories, subsequent settings
  -- overwrite the previous ones
  local paths = { USERDIR }
  local cached_index = USERDIR
  local settings = {}

  if not workspace and server.path then
    table.insert(paths, server.path)
    cached_index = cached_index .. tostring(server.path)
  elseif workspace then
    table.insert(paths, workspace)
    cached_index = cached_index .. tostring(workspace)
  end

  local stamp_paths = cached_workspace_stamp_paths[cached_index]
  if
    cached_workspace_settings_timestamp > os.time()
    and
    cached_workspace_settings[cached_index]
    and
    stamp_paths
    and
    recorded_stamp(stamp_paths) == cached_workspace_settings_stamp[cached_index]
  then
    return cached_workspace_settings[cached_index]
  else
    local position = 1
    stamp_paths = {}
    -- Sequential iteration (#10653): the trusted user-owned root must be
    -- the visited first position structurally, never by hash-order luck.
    for _, path in ipairs(paths) do
      if path then
        local settings_new = nil
        path = path:gsub("\\+$", ""):gsub("/+$", "")
        stamp_paths[#stamp_paths + 1] = path

        if position == 1 then
          -- User-owned executable configuration root (#10653): the USERDIR
          -- keeps its historical .lite_lsp.lua authority over .lite_lsp.json.
          local user_lua = path .. "/.lite_lsp.lua"
          if util.file_exists(user_lua) then
            stamp_paths[#stamp_paths + 1] = user_lua
            local settings_lua = dofile(user_lua)
            if type(settings_lua) == "table" then
              settings_new = settings_lua
            end
          else
            local user_json = path .. "/.lite_lsp.json"
            if util.file_exists(user_json) then
              stamp_paths[#stamp_paths + 1] = user_json
              local file = io.open(user_json, "r")
              if file then
                local settings_json = file:read("*a")
                settings_new = json.decode(settings_json)
                file:close()
              end
            end
          end
        else
          -- Project-derived positions are data-only (#10653): a
          -- repository-controlled .lite_lsp.lua is never executed here,
          -- regardless of startup, configuration requests, root changes,
          -- restarts, or cache refreshes.
          local project_lua = path .. "/.lite_lsp.lua"
          if util.file_exists(project_lua) then
            core.log_quiet(
              "[LSP]: ignoring untrusted project configuration '%s'",
              project_lua
            )
          end

          local project_json = path .. "/.lite_lsp.json"
          if util.file_exists(project_json) then
            stamp_paths[#stamp_paths + 1] = project_json
            local file = io.open(project_json, "r")
            if file then
              local settings_json = file:read("*a")
              file:close()
              local ok_decode, decoded = pcall(json.decode, settings_json)
              if
                ok_decode
                and type(decoded) == "table"
                and not json.is_array(decoded)
                and not json.is_null(decoded)
              then
                settings_new = decoded
              else
                -- Fail safely (#10653): malformed project data becomes one
                -- bounded configuration error and an empty value, never an
                -- executable fallback. Array/null JSON roots are rejected
                -- here too (#10653 review): a non-object root must not be
                -- able to replace accumulated user/server settings.
                core.error(
                  "[LSP]: ignoring malformed project configuration '%s' (%s)",
                  project_json,
                  ok_decode and "configuration is not a JSON object"
                    or tostring(decoded)
                )
              end
            end
          end
        end

        -- overwrite global settings by those specified in the server if any
        if position == 1 and server.settings then
          if settings_new then
            settings_new = util.deep_merge(settings_new, server.settings)
          else
            settings_new = server.settings
          end
        end

        -- overwrite previous settings with new ones
        if settings_new then
          settings = util.deep_merge(settings, settings_new)
        end
      end

      position = position + 1
    end

    -- store settings on cache for 5 seconds for fast repeated calls;
    -- the accepted candidates' stamp lets a changed configuration
    -- invalidate the entry inside that window (#10653)
    cached_workspace_settings[cached_index] = settings
    cached_workspace_stamp_paths[cached_index] = stamp_paths
    cached_workspace_settings_stamp[cached_index] = recorded_stamp(stamp_paths)
    cached_workspace_settings_timestamp = os.time() + 5
  end

  return settings
end

-- TODO Update workspace folders of already running lsp servers if required
--- Start all applicable lsp servers for a given file.
--- @param filename string
--- @param project_directory string
function lsp.start_server(filename, project_directory)
  for name, server in pairs(lsp.servers) do
    if common.match_pattern(filename, server.file_patterns) then
      if not lsp.servers_running[name] then
        core.log("[LSP]: Starting " .. name)
        ---@type boolean, lsp.server
        local success, client = pcall(function() return Server(server) end)
        if not success then
          core.error("[LSP]: Unable to start %s:\nCommand: %s\nError: %s", name, common.serialize(server.command), client)
          goto continue
        end
        client.yield_on_reads = config.plugins.lsp.more_yielding

        lsp.servers_running[name] = client

        -- We overwrite the default log function to log messages on lite
        function client:log(message, ...)
          core.log_quiet(
            "[LSP/%s]: " .. message .. "\n",
            self.name,
            ...
          )
        end

        function client:on_shutdown()
          local sname = self.name
          core.log(
            "[LSP]: %s was shutdown, revise your configuration",
            sname
          )
          local last_shutdown = lsp.servers_running[sname].last_shutdown or 0
          lsp.servers_running = util.table_remove_key(
            lsp.servers_running,
            sname
          )
          if system.get_time() - last_shutdown >= 5 then
            lsp.start_servers()
            if lsp.servers_running[sname] then
              lsp.servers_running[sname].last_shutdown = system.get_time()
              core.log(
                "[LSP]: %s automatically restarted",
                sname
              )
            end
          end
        end

        -- Respond to workspace/configuration request
        -- Local patch (#11147): workspace/configuration is positional —
        -- result[i] answers params.items[i]. Items must be one dense JSON
        -- array of objects; iterate by position (never pairs()), keep
        -- duplicate sections as distinct slots, emit [] for zero items, and
        -- answer one exact InvalidParams instead of any partial result when
        -- items are not an array of objects.
        client:add_request_listener(
          "workspace/configuration",
          function(server, request)
            local params = request.params
            local items = params and params.items
            local valid_items = json.is_array(items)
            if valid_items then
              for i = 1, #items do
                local item = items[i]
                -- One ConfigurationItem per slot: object only, optional
                -- string section/scopeUri. Non-object elements and
                -- non-string scopeUri values are malformed here rather than
                -- crashing later in section lookup or URI conversion.
                if
                  not json.is_object(item)
                  or (
                    item.scopeUri ~= nil
                    and type(item.scopeUri) ~= "string"
                  )
                then
                  valid_items = false
                  break
                end
              end
            end

            if not valid_items then
              server:log("Invalid workspace/configuration items")
              server:push_response(
                request.method,
                request.id,
                nil,
                {
                  code = -32602,
                  message = "Invalid params: items must be an array of objects"
                }
              )
              return
            end

            local settings_default = lsp.get_workspace_settings(server)

            local settings_list = {}
            for i = 1, #items do
              local item = items[i]
              -- Local patch (#10845): presence and value are tracked
              -- independently. A found value is appended verbatim — explicit
              -- false stays JSON false, as do 0/""/[] and nested false — and
              -- only a genuinely missing section becomes the null sentinel.
              -- The legacy `value or json.null` collapsed an explicitly
              -- configured false into absent/default semantics.
              local value = nil
              local found = false
              if item.section then
                -- No workspace was specified so we return from default settings
                if not item.scopeUri then
                  value, found = util.table_get_field(
                    settings_default, item.section)
                -- A workspace was specified so we return from that workspace
                else
                  -- Local patch (#11165): scope URIs convert through the one
                  -- authority; an unconvertible scope falls back to the
                  -- default settings instead of a fabricated path.
                  local scope_path, scope_reason =
                    util.uri_to_path(item.scopeUri)
                  if not scope_path then
                    core.log_quiet(
                      "[LSP] %s scope not resolvable (%s)",
                      "workspace/configuration",
                      scope_reason or "unconvertible uri"
                    )
                  end
                  local settings_workspace = lsp.get_workspace_settings(
                    server, scope_path
                  )
                  value, found = util.table_get_field(
                    settings_workspace, item.section)
                end

                if not found then
                  server:log("Asking for '%s' config but not set", item.section)
                else
                  server:log("Asking for '%s' config", item.section)
                end
              end

              if found then
                table.insert(settings_list, value)
              else
                table.insert(settings_list, json.null)
              end
            end

            server:push_response(
              request.method,
              request.id,
              json.array(settings_list)
            )
          end
        )

        -- Respond to window/showDocument request
        -- Local patch (#10873): the ShowDocumentResult reflects the completed
        -- client action instead of preemptive success. The external prompt is
        -- generation-owned (an old prompt cannot answer after server
        -- replacement through the servers_running identity), nothing responds
        -- before the user/open terminal outcome, and internal open or
        -- selection-conversion failures carry explicit typed dispositions.
        -- #10785 owns response correlation; this listener answers exactly
        -- once with the truthful payload.
        client:add_request_listener(
          "window/showDocument",
          function(server, request)
            local responded = false
            util.show_document(server, request.params, {
              confirm = function(_, _, answered)
                MessageBox.info(
                  server.name .. " LSP Server",
                  "Wants to externally open:\n'"
                    .. tostring(request.params.uri) .. "'",
                  function(_, button_id)
                    answered(button_id == 1)
                  end,
                  MessageBox.BUTTONS_YES_NO
                )
                -- The decision arrives asynchronously; no response exists
                -- until the user outcome settles.
                return nil
              end,
              reveal = function(uri)
                -- Local patch (#11165): internal reveal converts through the
                -- one authority; non-file or malformed URIs fail closed.
                local document, document_reason = util.uri_to_path(uri)
                if not document then
                  core.log_quiet(
                    "[LSP] showDocument refused (%s)",
                    document_reason or "unconvertible uri"
                  )
                  return nil, document_reason or "unconvertible_uri"
                end
                local ok_open, doc_view_or_error = pcall(function()
                  ---@type core.docview
                  return core.root_view:open_doc(
                    core.open_doc(common.home_expand(document))
                  )
                end)
                if not ok_open or not doc_view_or_error then
                  core.log_quiet(
                    "[LSP] showDocument open failed (%s)",
                    tostring(doc_view_or_error or "no docview")
                  )
                  return nil, "open_failed"
                end
                if request.params.selection then
                  local ok_selection, selection_error = pcall(function()
                    local line1, col1, line2, col2 = util.toselection(
                      request.params.selection, doc_view_or_error.doc
                    )
                    doc_view_or_error.doc:set_selection(
                      line1, col1, line2, col2
                    )
                  end)
                  if not ok_selection then
                    core.log_quiet(
                      "[LSP] showDocument selection conversion failed (%s)",
                      tostring(selection_error)
                    )
                    return nil, "selection_failed"
                  end
                end
                return doc_view_or_error
              end,
              raise = function()
                system.raise_window()
              end,
              alive = function()
                -- Generation ownership: only the currently registered server
                -- instance may answer; replacement/shutdown retires old
                -- prompts without responding (#10873).
                return lsp.servers_running[server.name] == server
              end,
              outcome = function(success, reason)
                if responded then
                  return
                end
                responded = true
                if not success then
                  core.log_quiet(
                    "[LSP] showDocument refused (%s)", reason or "failed"
                  )
                end
                server:push_response(
                  request.method, request.id, {success = success}
                )
              end,
            })
          end
        )

        -- Display server messages on lite UI
        client:add_message_listener(
          "window/logMessage",
          function(server, params)
            if core.log then
              log(server, "%s", params.message)
            end
          end
        )

        -- Register/unregister diagnostic messages
        client:add_message_listener(
          "textDocument/publishDiagnostics",
          -- Local patch (#11124): one named production seam (#11108 style).
          lsp.handle_publish_diagnostics
        )

        -- Register/unregister diagnostic messages
        client:add_message_listener(
          "window/showMessage",
          function(server, params)
            local log_func = "log_quiet"
            if params.type == Server.message_type.Error then
              log_func = "error"
            elseif params.type == Server.message_type.Warning then
              log_func = "warn"
            elseif params.type == Server.message_type.Info then
              log_func = "log"
            elseif params.type == Server.message_type.Debug then
              log_func = "log_quiet"
            end
            core[log_func]("["..server.name.."] message: %s", params.message)
          end
        )

        -- Send settings table after initialization if available.
        client:add_event_listener("initialized", function(server)
          if config.plugins.lsp.force_verbosity_off then
            core.log_quiet("["..server.name.."] " .. "Initialized")
          else
            log(server, "Initialized")
          end
          local settings = lsp.get_workspace_settings(server)
          if not util.table_empty(settings) then
            server:push_notification("workspace/didChangeConfiguration", {
              params = {settings = settings}
            })
          end

          -- Send open document request if needed
          for _, docu in ipairs(core.docs) do
            if docu.filename then
              if common.match_pattern(docu.filename, server.file_patterns) then
                lsp.open_document(docu)
              end
            end
          end
        end)

        -- Start the server initialization process
        -- Local patch (#11165): initialize reports an unconvertible
        -- workspace instead of sending a fabricated rootUri. A refused
        -- initialization unregisters the freshly started client so a later
        -- attempt can start it again instead of skipping a dead entry.
        local initialized, init_reason = client:initialize(
          project_directory, "Lite XL", VERSION)
        if not initialized then
          lsp.servers_running[name] = nil
          core.error(
            "[LSP] could not start %s (%s)",
            name,
            tostring(init_reason or "unconvertible workspace")
          )
        end
      end
    end
    ::continue::
  end
end

---Return the live open-document session bound to one canonical URI and
---running server instance, or nil (#11124).
---@param uri string
---@param server lsp.server
---@return lsp.document.session|nil
function lsp.find_document_session(uri, server)
  for _, by_server in pairs(lsp.document_sessions) do
    local session = by_server[server]
    if session and session.uri == uri then
      return session
    end
  end
  return nil
end

---Handle one textDocument/publishDiagnostics notification through the
---generation-bound publication store (#11124). Extracted as a named seam so
---the exact production body is directly provable; the listener inside
---lsp.start_server only registers it.
---@param server lsp.server
---@param params table PublishDiagnosticsParams
function lsp.handle_publish_diagnostics(server, params)
  -- Local patch (#11165): publications for non-file or malformed URIs are
  -- refused through the one authority instead of becoming local filenames.
  local abs_filename, uri_reason = util.uri_to_path(params.uri)
  if not abs_filename then
    core.log_quiet(
      "[LSP] %s publication dropped (%s)",
      "textDocument/publishDiagnostics",
      uri_reason or "unconvertible"
    )
    return
  end
  local filename = core.normalize_to_project_dir(abs_filename)

  if server.verbose then
    core.log_quiet(
      "["..server.name.."] %s diagnostics for:  %s",
      filename,
      params.diagnostics and #params.diagnostics or 0
    )
  end

  -- Local patch (#11124): admit the publication against its exact subject
  -- before anything becomes visible or clears state.
  local session = lsp.find_document_session(params.uri, server)
  local accepted, disposition = diagnostics.publish({
    provider = server.name,
    generation = server.generation or 0,
    has_session = session ~= nil,
    session_generation = session and session.session_generation or nil,
    version = session and session.version or nil,
    -- Local patch (#11128): negotiated encoding rides the publication.
    position_encoding = server.capabilities
      and server.capabilities.positionEncoding or nil,
  }, params)

  if not accepted then
    core.log_quiet(
      "[LSP] %s publication dropped (%s)",
      "textDocument/publishDiagnostics", disposition or "stale"
    )
    return
  end

  if
    diagnostics.lintplus_found
    and
    config.plugins.lsp.show_diagnostics
    and
    util.doc_is_open(abs_filename)
  then
    -- we delay rendering of diagnostics to prevent the constant reporting
    -- of errors while typing. The rendering resolver bundle itself is
    -- installed once at plugin load (#11128).
    diagnostics.lintplus_populate_delayed(filename)
  end
end

---Stops all running servers.
function lsp.stop_servers()
  for name, _ in pairs(lsp.servers) do
    if lsp.servers_running[name] then
       local exiting = lsp.servers_running[name]
       exiting:exit()
       core.log("[LSP] stopped %s", name)
       -- Local patch (#11115): sessions owned by the replaced process die
       -- with it, so no old-generation pending change can publish into a
       -- replacement session and old versions are never compared as though
       -- they belong to the new process generation.
       for doc, by_server in pairs(lsp.document_sessions) do
         if by_server[exiting] then
           lsp.terminate_document_session(doc, exiting)
         end
       end
       -- Local patch (#11124): the exiting provider loses all visible
       -- ownership; a replacement starts from an empty set.
       diagnostics.retire_provider(exiting.name)
       lsp.servers_running = util.table_remove_key(lsp.servers_running, name)
    end
  end
end

---Start only the needed servers by current opened documents.
function lsp.start_servers()
  for _, doc in ipairs(core.docs) do
    if doc.filename then
      lsp.start_server(doc.filename, core.project_dir)
    end
  end
end

---Returns the hovered doc and the hovered position.
---Returns nil if no doc with an LSP activated is under the provided coordinates.
---@param x number
---@param y number
---@return core.doc|nil doc
---@return integer|nil line
---@return integer|nil col
function lsp.get_hovered_location(x, y)
  local n = core.root_view.root_node:get_child_overlapping_point(x, y)
  if not n then return end
  local av = n.active_view
  if not av:extends(DocView) then return end
  if av and av.doc.lsp_open then
    ---@type core.doc
    local doc = av.doc
    local line, col = av:resolve_screen_position(x, y)
    local last_x = av:get_col_x_offset(line, #av.doc.lines[line])
    local lx, ly = av:get_line_screen_position(line)
    if x > last_x + lx or y > ly + av:get_line_height() then return end
    return doc, line, col
  end
end

---Send notification to applicable LSP servers that a document was opened
---@param doc core.doc
function lsp.open_document(doc)
  -- in some rare ocassions this function may return nil when the
  -- user closed lite-xl with files opened, removed the files from system
  -- and opens lite-xl again which loads the non existent files.
  local doc_path = core.project_absolute_path(doc.filename)
  local file_info = system.get_file_info(doc_path)
  if not file_info then
    core.error("[LSP] could not open: %s", tostring(doc.filename))
    return
  end

  -- Local patch (#11165): one conversion per document through the authority;
  -- an unconvertible path opens no session and sends no didOpen.
  local doc_uri = util.path_to_uri(doc_path)
  if not doc_uri then
    core.error(
      "[LSP] could not open, unconvertible path: %s",
      tostring(doc.filename)
    )
    return
  end

  local active_servers = lsp.get_active_servers(doc.filename, true)

  if #active_servers > 0 then
    doc.disable_symbols = true -- disable symbol parsing on autocomplete plugin
    for _, name in pairs(active_servers) do
      local server = lsp.servers_running[name]
      -- Local patch (#11115): one session per admitted open, created before
      -- the didOpen payload so every accepted mutation after this point
      -- belongs to the new version stream.
      local session = lsp.create_document_session(doc, server, doc_uri)
      if server.capabilities.textDocumentSync.openClose then
        if server.exit_timer then
          server.exit_timer:stop()
          server.exit_timer = nil
        end
        -- Local patch (#11115): didOpen text and version describe exactly
        -- one accepted snapshot; the version is the session's explicit
        -- fresh origin, never editor clean/dirty identity.
        local text = table.concat(doc.lines)
        if file_info.size / 1024 <= 50 then
          -- file size is in range so push the notification as usual.
          server:push_notification('textDocument/didOpen', {
            params = {
              textDocument = {
                uri = session.uri,
                languageId = server:get_language_id(doc),
                version = session.version,
                text = text
              }
            },
            callback = function() doc.lsp_open = true end
          })
        else
          -- big files too slow for json encoder, also sending a huge file
          -- without yielding would stall the ui, and some lsp servers have
          -- issues with receiving big files in a single chunk.
          local escaped_text = text
            :gsub('\\', '\\\\'):gsub("\n", "\\n"):gsub("\r", "\\r")
            :gsub("\t", "\\t"):gsub('"', '\\"'):gsub('\b', '\\b')
            :gsub('\f', '\\f')

          server:push_raw("textDocument/didOpen", {
            raw_data = '{\n'
            .. '"jsonrpc": "2.0",\n'
            .. '"method": "textDocument/didOpen",\n'
            .. '"params": {\n'
            .. '"textDocument": {\n'
            .. '"uri": "'..session.uri..'",\n'
            .. '"languageId": "'..server:get_language_id(doc)..'",\n'
            .. '"version": '..session.version..',\n'
            .. '"text": "'..escaped_text..'"\n'
            .. '}\n'
            .. '}\n'
            .. '}\n',
            callback = function(server)
              doc.lsp_open = true
              log(server, "Big file '%s' ready for completion!", doc.filename)
            end
          })

          log(server, "Processing big file '%s'...", doc.filename)
        end
      else
        doc.lsp_open = true
      end
    end
  end
end

--- Send notification to applicable LSP servers that a document was saved
---@param doc core.doc
function lsp.save_document(doc)
  if not doc.lsp_open then return end

  -- Local patch (#11165): one conversion per save through the authority;
  -- an unconvertible path saves no session state on the wire.
  local doc_uri = util.path_to_uri(core.project_absolute_path(doc.filename))
  if not doc_uri then
    core.log_quiet(
      "[LSP] didSave dropped, unconvertible path: %s",
      tostring(doc.filename)
    )
    return
  end

  local active_servers = lsp.get_active_servers(doc.filename, true)
  if #active_servers > 0 then
    for _, name in pairs(active_servers) do
      local server = lsp.servers_running[name]
      local save = server.capabilities.textDocumentSync.save
      if save then
        -- Send document content only if required by lsp server
        if save.includeText then
          -- If save should include file content then raw is faster for
          -- huge files that would take too much to encode.
          local text = table.concat(doc.lines)
            :gsub('\\', '\\\\'):gsub("\n", "\\n"):gsub("\r", "\\r")
            :gsub("\t", "\\t"):gsub('"', '\\"'):gsub('\b', '\\b')
            :gsub('\f', '\\f')

          server:push_raw("textDocument/didSave", {
            raw_data = '{\n'
            .. '"jsonrpc": "2.0",\n'
            .. '"method": "textDocument/didSave",\n'
            .. '"params": {\n'
            .. '"textDocument": {\n'
            .. '"uri": "'..doc_uri..'"\n'
            .. '},\n'
            .. '"text": "'..text..'"\n'
            .. '}\n'
            .. '}\n'
          })
        else
          server:push_notification('textDocument/didSave', {
            params = {
              textDocument = {
                uri = doc_uri
              }
            }
          })
        end
      end
    end
  end
end

--- Send notification to applicable LSP servers that a document was closed
---@param doc core.doc
function lsp.close_document(doc)
  if not doc.lsp_open then return end

  -- Local patch (#11165): one conversion per close through the authority.
  local doc_uri = util.path_to_uri(core.project_absolute_path(doc.filename))
  if not doc_uri then
    core.log_quiet(
      "[LSP] didClose dropped, unconvertible path: %s",
      tostring(doc.filename)
    )
    return
  end

  local active_servers = lsp.get_active_servers(doc.filename, true)
  if #active_servers > 0 then
    for _, name in pairs(active_servers) do
      local server = lsp.servers_running[name]
      if server.capabilities.textDocumentSync.openClose then
        -- Local patch (#11115): didClose terminates the exact session. The
        -- wire identity carries no editor-derived version, and pending
        -- state dies with the session record.
        server:push_notification('textDocument/didClose', {
          params = {
            textDocument = {
              uri = doc_uri,
              languageId = server:get_language_id(doc)
            }
          }
        })
        lsp.terminate_document_session(doc, server)
      end
    end
  end
end

--- Helper for lsp.update_document
---@param doc core.doc
local function request_signature_completion(doc)
  local line1, col1, line2, col2 = doc:get_selection()

  if line1 == line2 and col1 == col2 then
    -- First try to display a function signatures and if not possible
    -- do normal code autocomplete
    lsp.request_signature(
      doc,
      line1,
      col1,
      false,
      lsp.request_completion
    )
  end
end

---Send document updates to applicable running LSP servers.
---@param doc core.doc
---@param request_completion? boolean
function lsp.update_document(doc, request_completion)
  if not doc.lsp_open then
    return
  end

  -- Local patch (#11115): the live session owns pending state. Batches of a
  -- dead or not-yet-admitted session cannot publish.
  for _, name in pairs(lsp.get_active_servers(doc.filename, true)) do
    local server = lsp.servers_running[name]
    local session = lsp.get_document_session(doc, server)
    if not session or #session.pending_changes <= 0 then
      goto continue
    end
    local sync_kind = server.capabilities.textDocumentSync.change
    -- Local patch (#10833): no enqueue admission gate. The former
    -- server:can_push() hit-rate probe delayed batch emission under unrelated
    -- provider traffic and could starve document truth; batches now always
    -- queue (overwriting the unsent predecessor) and the send loop paces
    -- delivery.
    if sync_kind ~= Server.text_document_sync_kind.None then
      local completion_callback = nil
      if request_completion then
        completion_callback = function() request_signature_completion(doc) end
      end

      -- Local patch (#11115): allocate exactly one protocol version per
      -- emitted batch, from the session's own stream. Full and ranged
      -- branches consume the same owner; an overwritten unsent batch may
      -- skip one number on the wire but the stream stays strictly
      -- increasing and never rewinds.
      session.version = session.version + 1
      local batch_version = session.version
      if
        sync_kind == Server.text_document_sync_kind.Full
        and
        not server.incremental_changes
      then
        -- If sync should be done by sending full file content then lets do
        -- it raw which is faster for big files.
        local text = table.concat(doc.lines)
          :gsub('\\', '\\\\'):gsub("\n", "\\n"):gsub("\r", "\\r")
          :gsub("\t", "\\t"):gsub('"', '\\"'):gsub('\b', '\\b')
          :gsub('\f', '\\f')

        server:push_raw("textDocument/didChange", {
          overwrite = true,
          raw_data = '{\n'
          .. '"jsonrpc": "2.0",\n'
          .. '"method": "textDocument/didChange",\n'
          .. '"params": {\n'
          .. '"textDocument": {\n'
          .. '"uri": "'..session.uri..'",\n'
          .. '"version": '..batch_version .. "\n"
          .. '},\n'
          .. '"contentChanges": [\n'
          .. '{"text": "'..text..'"}\n'
          .. "]\n"
          .. '}\n'
          .. '}\n',
          callback = function()
            session.pending_changes = {}
            if completion_callback then
              completion_callback()
            end
          end
        })
      else
        lsp.servers_running[name]:push_notification('textDocument/didChange', {
          overwrite = true,
          params = {
            textDocument = {
              uri = session.uri,
              version = batch_version,
            },
            contentChanges = session.pending_changes
          },
          callback = function()
            session.pending_changes = {}
            if completion_callback then
              completion_callback()
            end
          end
        })
      end
    end
    ::continue::
  end
end

--- Enable or disable diagnostic messages
function lsp.toggle_diagnostics()
  config.plugins.lsp.show_diagnostics = not config.plugins.lsp.show_diagnostics

  if not config.plugins.lsp.show_diagnostics then
    diagnostics.lintplus_clear_messages()
    core.log("[LSP] Diagnostics disabled")
  else
    diagnostics.lintplus_populate()
    core.log("[LSP] Diagnostics enabled")
  end
end

--- Send to applicable LSP servers a request for code completion
function lsp.request_completion(doc, line, col, forced)
  if lsp.in_trigger or not doc.lsp_open then
    return
  end

  for _, name in pairs(lsp.get_active_servers(doc.filename, true)) do
    local server = lsp.servers_running[name]
    if server.capabilities.completionProvider then
      local capabilities = lsp.servers_running[name].capabilities
      local char = doc:get_char(line, col-1)
      local trigger_char = false

      local request = get_buffer_position_params(doc, line, col)
      -- Local patch (#11165): no wire identity means no request.
      if not request then return end

      -- without providing context some language servers like the
      -- lua-language-server behave poorly and return garbage.
      if
        capabilities.completionProvider.triggerCharacters
        and
        #capabilities.completionProvider.triggerCharacters > 0
        and
        char:match("%p")
        and
        util.intable(char, capabilities.completionProvider.triggerCharacters)
      then
        request.context = {
          triggerKind = Server.completion_trigger_Kind.TriggerCharacter,
          triggerCharacter = char
        }
        trigger_char = true;
      end

      if
        not trigger_char
        and
        not autocomplete.can_complete()
        and
        not forced
      then
        return false
      end

      -- Local patch (#11108): bind this request to its exact subject.
      local subject = lsp.make_request_subject(
        'textDocument/completion', doc, server, line, col)
      if not subject then
        goto continue
      end

      server:push_request('textDocument/completion', {
        params = request,
        overwrite = true,
        callback = function(server, response)
          lsp.user_typed = false

          -- Local patch (#11108): admission before any effect; the caret
          -- identity alone is no longer proof of document state.
          local admitted, disposition = lsp.admit_response(subject)
          if not admitted then
            core.log_quiet(
              "[LSP] %s response dropped (%s)",
              "textDocument/completion", disposition or "stale"
            )
            return
          end

          if server.verbose then
            server:log(
              "Completion response received."
            )
          end

          if not response.result then
            return
          end

          local result = response.result
          local complete_result = true
          if result.isIncomplete then
            if server.verbose then
              core.log_quiet(
                "["..server.name.."] " .. "Completion list incomplete"
              )
            end
            complete_result = false
          end

          if not result.items or #result.items <= 0 then
            -- Workaround for some lsp servers that don't return results
            -- in the items property but instead on the results it self
            if #result > 0 then
              local items = result
              result = {items = items}
            else
              return
            end
          end

          local symbols = {
            name = lsp.servers_running[name].name,
            files = lsp.servers_running[name].file_patterns,
            items = {}
          }

          local symbol_count = 1
          for _, symbol in ipairs(result.items) do
            local label = symbol.label
              or (
                symbol.textEdit
                and symbol.textEdit.newText
                or symbol.insertText
              )

            local info = server.get_completion_item_kind(symbol.kind)

            local desc = symbol.detail or ""

            -- TODO: maybe we should give priority to insertText above
            if
              symbol.label and
              symbol.insertText and
              #symbol.label > #symbol.insertText
            then
              label = symbol.insertText
              if symbol.label ~= label then
                desc = symbol.label
              end
              if symbol.detail then
                desc = desc .. ": " .. symbol.detail
              end
            end

            if desc ~= "" then
              desc = desc .. "\n"
            end

            if
              type(symbol.documentation) == "table"
              and
              symbol.documentation.value
            then
              desc = desc .. "\n" .. symbol.documentation.value
              if
                symbol.documentation.kind
                and
                symbol.documentation.kind == "markdown"
              then
                desc = util.strip_markdown(desc)
                if symbol_count % 10 == 0 then
                  coroutine.yield()
                end
              end
            elseif symbol.documentation then
              desc = desc .. "\n" .. symbol.documentation
            end

            desc = desc:gsub("[%s\n]+$", "")
              :gsub("\n\n\n+", "\n\n")

            -- Local patch (#11189): display labels are presentation, not
            -- identity. The completion menu is a label-keyed map, so two
            -- valid items sharing one label used to collide and the later
            -- item silently overwrote the earlier one. The first occurrence
            -- of a label keeps it as the internal key (its key equals the
            -- text the plugin always inserted, so plain items are unchanged);
            -- later occurrences gain a deterministic source-order "#N"
            -- suffix. The suffix lives in the internal key only: every row
            -- keeps its own exact protocol item in data.completion_item, and
            -- data.insert_text preserves the pre-suffix insert target so
            -- selection below never leaks the disambiguator into the buffer.
            local key = label
            if symbols.items[key] then
              local occurrence = 2
              while symbols.items[label .. "#" .. occurrence] do
                occurrence = occurrence + 1
              end
              key = label .. "#" .. occurrence
            end

            symbols.items[key] = {
              info = info,
              desc = desc,
              data = {
                -- Local patch (#11108): carry the admitted request subject
                -- so deferred edit application revalidates at select time.
                server = server, completion_item = symbol, subject = subject,
                -- Local patch (#11188): one structured pre-apply resolve
                -- state per item; selection and hover share it.
                resolve = new_completion_resolve_state(server, symbol, subject),
                -- Local patch (#11189): exact plain-insert target for
                -- suffix-carrying keys (see the select-time fallback).
                insert_text = label
              },
              onselect = autocomplete_onselect
            }

            if
              server.capabilities.completionProvider.resolveProvider
              and
              not symbol.documentation
            then
              symbols.items[key].onhover = autocomplete_onhover
            end

            symbol_count = symbol_count + 1
          end

          if trigger_char and complete_result then
            lsp.in_trigger = true
            autocomplete.complete(symbols, function()
              lsp.in_trigger = false
            end)
          else
            autocomplete.complete(symbols)
          end
        end
      })
      ::continue::
    end
  end
end

--- Send to applicable LSP servers a request for info about a function
--- signatures and display them on a tooltip.
function lsp.request_signature(doc, line, col, forced, fallback)
  if not doc.lsp_open then return end

  local char = doc:get_char(line, col-1)
  local prev_char = doc:get_char(line, col-2) -- to support ', '
  for _, name in pairs(lsp.get_active_servers(doc.filename, true)) do
    local server = lsp.servers_running[name]
    if
      server.capabilities.signatureHelpProvider
      and
      (
        forced
        or
        (
          server.capabilities.signatureHelpProvider.triggerCharacters
          and
          #server.capabilities.signatureHelpProvider.triggerCharacters > 0
          and
          (
            util.intable(
              char, server.capabilities.signatureHelpProvider.triggerCharacters
            )
            or
            util.intable(
              prev_char,
              server.capabilities.signatureHelpProvider.triggerCharacters
            )
          )
        )
      )
    then
      -- Local patches (#11165, #11108): both request inputs are computed
      -- before any goto continue because Lua forbids a goto that skips
      -- into the scope of a later local.
      local position_params = get_buffer_position_params(doc, line, col)
      local subject = lsp.make_request_subject(
        'textDocument/signatureHelp', doc, server, line, col)
      -- No wire identity means no request (#11165).
      if not position_params then
        goto continue
      end
      if not subject then
        goto continue
      end

      server:push_request('textDocument/signatureHelp', {
        params = position_params,
        overwrite = true,
        callback = function(server, response)
          -- Local patch (#11108): admission replaces the caret-only guard;
          -- a stale result neither displays nor falls back.
          local admitted, disposition = lsp.admit_response(subject)
          if not admitted then
            core.log_quiet(
              "[LSP] %s response dropped (%s)",
              "textDocument/signatureHelp", disposition or "stale"
            )
            return
          end

          if
            response.result
            and
            response.result.signatures
            and
            #response.result.signatures > 0
          then
            autocomplete.close()
            listbox.show_signatures(response.result)
            lsp.user_typed  = false
          elseif fallback then
            fallback(doc, line, col)
          end
        end
      })
      ::continue::
      break
    elseif fallback then
      fallback(doc, line, col)
    end
    ::continue::
  end
end

---Returns the "selection" for the token that includes the provided position.
---@param doc core.doc
---@param line integer
---@param col integer
---@return integer line1
---@return integer col2
---@return integer line2
---@return integer col2
local function get_token_range(doc, line, col)
  local col1 = 0
  for _, _, text in doc.highlighter:each_token(line) do
    local text_len = #text
    local col2 = col1 + text_len
    if col2 >= col then
      return line, col1 + 1, line, col2 + 1
    end
    col1 = col2
  end
  return line, col, line, col+1
end

---@type core.node
local help_active_node = nil
---@type core.node
local help_bottom_node = nil
--- Sends a request to applicable LSP servers for information about the
--- symbol where the cursor is placed and shows it on a tooltip.
function lsp.request_hover(doc, line, col, in_tab)
  if not doc.lsp_open then return end

  for _, name in pairs(lsp.get_active_servers(doc.filename, true)) do
    local server = lsp.servers_running[name]
    if server.capabilities.hoverProvider then
      -- Local patch (#11108): bind this request to its exact subject.
      local subject = lsp.make_request_subject(
        'textDocument/hover', doc, server, line, col)
      if not subject then
        break
      end
      -- Local patch (#11165): no wire identity means no request.
      local position_params = get_buffer_position_params(doc, line, col)
      if not position_params then
        break
      end

      server:push_request('textDocument/hover', {
        params = position_params,
        callback = function(server, response)
          -- Local patch (#11108): admission before any tooltip effect.
          local admitted, disposition = lsp.admit_response(subject)
          if not admitted then
            core.log_quiet(
              "[LSP] %s response dropped (%s)",
              "textDocument/hover", disposition or "stale"
            )
            return
          end

          if response.result and response.result.contents then
            local range = response.result.range
            local line1, col1, line2, col2
            if range then
              line1, col1, line2, col2 = util.toselection(range, doc)
            else
              line1, col1, line2, col2 = get_token_range(doc, line, col)
            end
            lsp.hover_position.utf8_range = { line1 = line1, col1 = col1,
                                              line2 = line2, col2 = col2 }

            local content = response.result.contents
            local kind = nil
            local text = ""
            if type(content) == "table" then
              if content.value then
                text = content.value
                if content.kind then kind = content.kind end
              else
                local texts = {}
                for _, element in pairs(content) do
                  if type(element) == "string" then
                    table.insert(texts, element)
                  elseif type(element) == "table" and element.value then
                    table.insert(texts, element.value)
                    if not kind and element.kind then kind = element.kind end
                  end
                end
                text = table.concat(texts, "\n\n")
              end
            else -- content should be a string
              text = content
            end
            if text and #text > 0 then
              text = text:gsub("^[\n%s]+", ""):gsub("[\n%s]+$", "")
              if not in_tab then
                if kind == "markdown" then text = util.strip_markdown(text) end
                listbox.show_text(
                  text,
                  { line = line, col = col }
                )
              else
                local line1, col1 = translate.start_of_word(doc, line, col)
                local line2, col2 = translate.end_of_word(doc, line1, col1)
                local title = doc:get_text(line1, col1, line2, col2):gsub("%s*", "")
                title = "Help:" .. title .. ".md"
                ---@type lsp.helpdoc
                local helpdoc = HelpDoc(title, title)
                helpdoc:set_text(text)
                local helpview = DocView(helpdoc)
                helpview.context = "application"
                helpview.wrapping_enabled = true
                if LineWrapping then
                  LineWrapping.update_docview_breaks(helpview)
                end
                if
                  not help_bottom_node
                  or
                  (
                    #help_bottom_node.views == 1
                    and
                    not help_active_node:get_node_for_view(help_bottom_node.views[1])
                  )
                then
                  help_active_node = core.root_view:get_active_node_default()
                  help_bottom_node = help_active_node:split("down", helpview)
                else
                  help_bottom_node:add_view(helpview)
                end
              end
            end
          end
        end
      })
      break
    end
  end
end

--- Sends a request to applicable LSP servers for a symbol references
function lsp.request_references(doc, line, col)
  if not doc.lsp_open then return end

  for _, name in pairs(lsp.get_active_servers(doc.filename, true)) do
    local server = lsp.servers_running[name]
    if server.capabilities.hoverProvider then
      local request_params = get_buffer_position_params(doc, line, col)
      -- Local patch (#11165): no wire identity means no request.
      if not request_params then
        break
      end
      request_params.context = {includeDeclaration = true}
      -- Local patch (#11108): bind this request to its exact subject.
      local subject = lsp.make_request_subject(
        'textDocument/references', doc, server, line, col)
      if not subject then
        break
      end
      server:push_request('textDocument/references', {
        params = request_params,
        callback = function(server, response)
          -- Local patch (#11108): admission before any navigation prompt.
          local admitted, disposition = lsp.admit_response(subject)
          if not admitted then
            core.log_quiet(
              "[LSP] %s response dropped (%s)",
              "textDocument/references", disposition or "stale"
            )
            return
          end
          if response.result and #response.result > 0 then
            local references, reference_names = get_references_lists(response.result)
            core.command_view:enter("Filter References", {
              submit = function(text, item)
                if item then
                  local reference = references[item.name]
                    lsp.goto_location(reference)
                end
              end,
              suggest = function(text)
                local res = common.fuzzy_match(reference_names, text)
                for i, name in ipairs(res) do
                  local reference_info = util.split(name, "||")
                  res[i] = {
                    text = reference_info[1],
                    info = reference_info[2],
                    name = name
                  }
                end
                return res
              end
            })
          else
            core.log("[LSP] No references found.")
          end
        end
      })
      break
    end
    break
  end
end

---Sends a request to applicable LSP servers to retrieve the
---hierarchy of calls for the given function under the cursor.
function lsp.request_call_hierarchy(doc, line, col)
  if not doc.lsp_open then return end

  for _, name in pairs(lsp.get_active_servers(doc.filename, true)) do
    local server = lsp.servers_running[name]
    if server.capabilities.callHierarchyProvider then
      -- Local patch (#11172): client-affordance projection gate. The server
      -- advertising callHierarchyProvider is not enough: until #10719 lands
      -- a consumer, the result would be discarded, so the request is never
      -- sent and one explicit message explains why.
      local available, _ = capability_manifest.command_availability(
        "lsp:view-call-hierarchy", server.capabilities)
      if not available then
        core.log(capability_manifest.commands["lsp:view-call-hierarchy"].unsupported_message)
        return
      end
      -- Local patch (#11108): bind this request to its exact subject.
      local subject = lsp.make_request_subject(
        'textDocument/prepareCallHierarchy', doc, server, line, col)
      if not subject then
        return
      end
      -- Local patch (#11165): no wire identity means no request.
      local position_params = get_buffer_position_params(doc, line, col)
      if not position_params then
        return
      end
      server:push_request('textDocument/prepareCallHierarchy', {
        params = position_params,
        callback = function(server, response)
          -- Local patch (#11108): admission before any effect.
          local admitted, disposition = lsp.admit_response(subject)
          if not admitted then
            core.log_quiet(
              "[LSP] %s response dropped (%s)",
              "textDocument/prepareCallHierarchy", disposition or "stale"
            )
            return
          end
          if response.result and #response.result > 0 then
            -- TODO: Finish implement call hierarchy functionality
            return
          end
        end
      })
      return
    end
  end

  core.log("[LSP] Call hierarchy not supported.")
end

---Sends a request to applicable LSP servers to rename a symbol.
---@param doc core.doc
---@param line integer
---@param col integer
---@param new_name string
function lsp.request_symbol_rename(doc, line, col, new_name)
  if not doc.lsp_open then return end

  local servers_found = false
  for _, name in pairs(lsp.get_active_servers(doc.filename, true)) do
    servers_found = true
    local server = lsp.servers_running[name]
    if server.capabilities.renameProvider then
      -- Local patch (#11172): client-affordance projection gate. Until #8986
      -- lands real WorkspaceEdit application, a rename response could only
      -- be logged as a false success, so the request is never sent and one
      -- explicit message explains why.
      local available, _ = capability_manifest.command_availability(
        "lsp:rename-symbol", server.capabilities)
      if not available then
        core.log(capability_manifest.commands["lsp:rename-symbol"].unsupported_message)
        return
      end
      local request_params = get_buffer_position_params(doc, line, col)
      -- Local patch (#11165): no wire identity means no request.
      if not request_params then
        return
      end
      request_params.newName = new_name
      -- Local patch (#11108): bind this edit-producing request to its exact
      -- subject; its future workspace-edit application must never mutate a
      -- generation it was not computed against.
      local subject = lsp.make_request_subject(
        'textDocument/rename', doc, server, line, col)
      if not subject then
        return
      end
      server:push_request('textDocument/rename', {
        params = request_params,
        callback = function(server, response)
          -- Local patch (#11108): admission before any effect.
          local admitted, disposition = lsp.admit_response(subject)
          if not admitted then
            core.log_quiet(
              "[LSP] %s response dropped (%s)",
              "textDocument/rename", disposition or "stale"
            )
            return
          end
          if response.result and #response.result.changes then
            for file_uri, changes in pairs(response.result.changes) do
              core.log(file_uri .. " " .. #changes)
              -- TODO: Finish implement textDocument/rename
            end
          end

          core.log("%s", json.prettify(json.encode(response)))
        end
      })
      return
    end
  end

  if not servers_found then
    core.log("[LSP] " .. "No server ready or running")
  else
    core.log("[LSP] " .. "Symbols rename not supported")
  end
end

---Sends a request to applicable LSP servers to search for symbol on workspace.
---@param doc core.doc
---@param symbol string
function lsp.request_workspace_symbol(doc, symbol)
  if not doc.lsp_open then return end

  for _, name in pairs(lsp.get_active_servers(doc.filename, true)) do
    local server = lsp.servers_running[name]
    if server.capabilities.workspaceSymbolProvider then
      local rs = SymbolResults(symbol)
      core.root_view:get_active_node_default():add_view(rs)
      -- Local patch (#11108): workspace-level requests omit document
      -- version (none exists for a query) but retain exact server instance
      -- and generation identity; the query rides the subject as evidence.
      local subject = lsp.make_request_subject(
        'workspace/symbol', nil, server, nil, nil)
      if not subject then
        break
      end
      subject.query = symbol
      server:push_request('workspace/symbol', {
        params = {
          query = symbol,
          -- TODO: implement status notifications but seems not supported
          -- by tested lsp servers so far.
          -- workDoneToken = "some-identifier",
          -- partialResultToken = "some-other-identifier"
        },
        callback = function(server, response)
          -- Local patch (#11108): admission before populating results.
          local admitted, disposition = lsp.admit_response(subject)
          if not admitted then
            core.log_quiet(
              "[LSP] %s response dropped (%s)",
              "workspace/symbol", disposition or "stale"
            )
            return
          end
          if response.result and #response.result > 0 then
            for index, result in ipairs(response.result) do
              rs:add_result(result)
              if index % 100 == 0 then
                coroutine.yield()
                rs.list:resize_to_parent()
              end
            end
            rs.list:resize_to_parent()
          end
          rs:stop_searching()
        end
      })
      break
    end
    break
  end
end

--- Request a list of symbols for the given document for easy document
-- navigation and displays them using core.command_view:enter()
function lsp.request_document_symbols(doc)
  if not doc.lsp_open then return end

  local servers_found = false
  local symbols_retrieved = false
  for _, name in pairs(lsp.get_active_servers(doc.filename, true)) do
    servers_found = true
    local server = lsp.servers_running[name]
    if server.capabilities.documentSymbolProvider then
      log(server, "Retrieving document symbols...")
      -- Local patch (#11108): bind this navigation request to its exact
      -- subject; the deferred selection revalidates at submit time.
      local subject = lsp.make_request_subject(
        'textDocument/documentSymbol', doc, server, nil, nil)
      if not subject then
        break
      end
      -- Local patch (#11165): no wire identity means no request.
      local doc_uri = util.path_to_uri(core.project_absolute_path(doc.filename))
      if not doc_uri then
        log(server, "Document symbols dropped, unconvertible path")
        break
      end
      server:push_request('textDocument/documentSymbol', {
        params = {
          textDocument = {
            uri = doc_uri,
          }
        },
        callback = function(server, response)
          -- Local patch (#11108): admission before any navigation prompt.
          local admitted, disposition = lsp.admit_response(subject)
          if not admitted then
            core.log_quiet(
              "[LSP] %s response dropped (%s)",
              "textDocument/documentSymbol", disposition or "stale"
            )
            return
          end
          if response.result and response.result and #response.result > 0 then
            local symbols, symbol_names, search_subjects =
              get_symbol_lists(response.result)
            core.command_view:enter("Find Symbol", {
              submit = function(text, item)
                if item then
                  -- Local patch (#11108): revalidate at user-action time;
                  -- an old target cannot navigate the current session.
                  local still_current = select(1, lsp.admit_response(subject))
                  if not still_current then
                    core.log_quiet(
                      "[LSP] %s navigation dropped (%s)",
                      "textDocument/documentSymbol", "subject no longer current"
                    )
                    return
                  end
                  -- Local patch (#11198): navigate the exact retained row,
                  -- so duplicate display rows can never alias one target.
                  local row = symbols[item.name]
                  local target = row.location and row.location or row
                  if not target.uri then
                    -- DocumentSymbol navigation prefers selectionRange -
                    -- the precise symbol anchor - when present; range
                    -- remains the broader extent for servers without it.
                    local line1, col1 = util.toselection(
                      target.selectionRange or target.range, doc)
                    doc:set_selection(line1, col1, line1, col1)
                  else
                    lsp.goto_location(target)
                  end
                end
              end,
              suggest = function(text)
                -- Review adoption (PR #12670): fuzzy matching runs over the
                -- disambiguation-free rendered subjects only, so hidden
                -- row ordinals can never score, rank, or match a query.
                -- Matched subjects re-attach their opaque keys through
                -- deterministic first-in-source-order pairing; duplicate
                -- subjects each resolve to their own retained row.
                local matched_subjects =
                  common.fuzzy_match(search_subjects, text)
                local pending_keys_by_subject = {}
                for subject_index, key in ipairs(symbol_names) do
                  local subject = search_subjects[subject_index]
                  local pending = pending_keys_by_subject[subject]
                  if not pending then
                    pending = {}
                    pending_keys_by_subject[subject] = pending
                  end
                  table.insert(pending, key)
                end
                local res = {}
                for _, subject in ipairs(matched_subjects) do
                  local pending = pending_keys_by_subject[subject]
                  if pending and #pending > 0 then
                    local key = table.remove(pending, 1)
                    res[#res + 1] = {
                      text = util.split(key, "||")[1],
                      info = Server.get_symbol_kind(symbols[key].kind),
                      name = key
                    }
                  end
                end
                return res
              end
            })
          end
        end
      })
      symbols_retrieved = true
      break
    end
  end

  if not servers_found then
    core.log("[LSP] " .. "No server running")
  elseif not symbols_retrieved then
    core.log("[LSP] " .. "Document symbols not supported")
  end
end

--- Format current document if supported by one of the running lsp servers.
function lsp.request_document_format(doc)
  if not doc.lsp_open then return end

  local servers_found = false
  local format_executed = false
  for _, name in pairs(lsp.get_active_servers(doc.filename, true)) do
    servers_found = true
    local server = lsp.servers_running[name]
    if server.capabilities.documentFormattingProvider then
      local trim_trailing_whitespace = false
      local trim_newlines = false
      if type(config.plugins.trimwhitespace) == "table"
         and config.plugins.trimwhitespace.enabled
      then
        trim_trailing_whitespace = true
        trim_newlines = config.plugins.trimwhitespace.trim_empty_end_lines
      elseif config.plugins.trimwhitespace then -- Plugin enabled with true
        trim_trailing_whitespace = true
        trim_newlines = true
      end
      local indent_type, indent_size, indent_confirmed = doc:get_indent_info()
      if not indent_confirmed then
        indent_type, indent_size = config.tab_type, config.indent_size
      end
      -- Local patch (#11108): bind this edit-producing request to its exact
      -- subject so returned text edits can never mutate a newer generation.
      local subject = lsp.make_request_subject(
        'textDocument/formatting', doc, server, nil, nil)
      if not subject then
        break
      end
      -- Local patch (#11165): no wire identity means no request.
      local doc_uri = util.path_to_uri(core.project_absolute_path(doc.filename))
      if not doc_uri then
        log(server, "Formatting dropped, unconvertible path")
        break
      end
      server:push_request('textDocument/formatting', {
        params = {
          textDocument = {
            uri = doc_uri,
          },
          options = {
            tabSize = indent_size,
            insertSpaces = indent_type == "soft",
            trimTrailingWhitespace = trim_trailing_whitespace,
            insertFinalNewline = false,
            trimFinalNewlines = trim_newlines
          }
        },
        callback = function(server, response)
          if response.error and response.error.message then
            log(server, "Error formatting: " .. response.error.message)
          elseif response.result and #response.result > 0 then
            -- Local patch (#11108): admission before mutating bytes; a
            -- response computed for an older snapshot is refused.
            local admitted, disposition = lsp.admit_response(subject)
            if not admitted then
              core.log_quiet(
                "[LSP] %s edits refused (%s)",
                "textDocument/formatting", disposition or "stale"
              )
              return
            end
            -- Apply edits in reverse, as the ranges don't consider
            -- the intermediate states.
            -- Consider the TextEdits as already sorted.
            -- If there are servers that don't sort their TextEdits,
            -- we'll add sorting code.
            for i=#response.result,1,-1 do
              apply_edit(server, doc, response.result[i], false, false)
            end
            log(server, "Formatted document")
          else
            log(server, "Formatting not required")
          end
        end
      })
      format_executed = true
      break
    end
  end

  if not servers_found then
    core.log("[LSP] " .. "No server running")
  elseif not format_executed then
    core.log("[LSP] " .. "Formatting not supported")
  end
end

function lsp.view_document_diagnostics(doc)
  local diagnostic_messages = diagnostics.get(core.project_absolute_path(doc.filename))
  if not diagnostic_messages or #diagnostic_messages <= 0 then
    core.log("[LSP] %s", "No diagnostic messages found.")
    return
  end

  local diagnostic_labels = { "Error", "Warning", "Info", "Hint" }

  -- Local patch (#11128): list, suggestion and selection positions resolve
  -- through the same live-document presentation authority as inline
  -- rendering, with the publication's negotiated encoding; closed/unavailable
  -- subjects show an explicit unproven column instead of raw code units.
  local function resolve_diagnostic_position(diagnostic)
    return diagnostics.resolve_range(
      diagnostic.range, doc, diagnostic.position_encoding)
  end

  local indexes, captions = {}, {}
  for index, diagnostic in pairs(diagnostic_messages) do
    local line1, col1 = resolve_diagnostic_position(diagnostic)
    local position
    if line1 and col1 then
      position = tostring(line1) .. ":" .. tostring(col1)
    elseif line1 then
      position = tostring(line1) .. ":col not proven"
    else
      position = "position unavailable"
    end
    local label = diagnostic_labels[diagnostic.severity or diagnostics.severity.ERROR]
      .. ": " .. diagnostic.message .. " "
      .. position
    captions[index] = label
    indexes[label] = index
  end

  core.command_view:enter("Filter Diagnostics", {
    submit = function(text, item)
      if item then
        local diagnostic = diagnostic_messages[item.index]
        -- Bind the first resolution's disposition; no second resolve.
        local line1, col1, _, _, disposition =
          resolve_diagnostic_position(diagnostic)
        if line1 and col1 then
          doc:set_selection(line1, col1, line1, col1)
        else
          core.log_quiet(
            "[LSP] %s navigation dropped (%s)",
            "view-document-diagnostics",
            disposition or "stale"
          )
        end
      end
    end,
    suggest = function(text)
      local res = common.fuzzy_match(captions, text)
      for i, name in ipairs(res) do
        local diagnostic = diagnostic_messages[indexes[name]]
        local line1, col1 = resolve_diagnostic_position(diagnostic)
        res[i] = {
          text = diagnostics.lintplus_kinds[diagnostic.severity or diagnostics.severity.ERROR]
            .. ": " .. diagnostic.message,
          info = line1 and col1
              and (tostring(line1) .. ":" .. tostring(col1))
            or "position unavailable",
          index = indexes[name]
        }
      end
      return res
    end
  })
end

function lsp.view_all_diagnostics()
  if diagnostics.count <= 0 then
    core.log("[LSP] %s", "No diagnostic messages found.")
    return
  end

  local captions = {}
  for _, diagnostic in ipairs(diagnostics.list) do
    table.insert(
      captions,
      core.normalize_to_project_dir(diagnostic.filename)
    )
  end

  core.command_view:enter("Filter Files", {
    submit = function(text, item)
      if item then
        core.root_view:open_doc(
          core.open_doc(
            common.home_expand(
              text
            )
          )
        )
      end
    end,
    suggest = function(text)
      local res = common.fuzzy_match(captions, text, true)
      for i, name in ipairs(res) do
        local diagnostics_count = diagnostics.get_messages_count(
          core.project_absolute_path(name)
        )
        res[i] = {
          text = name,
          info = "Messages: " .. diagnostics_count
        }
      end
      return res
    end
  })
end

--- Jumps to the definition or implementation of the symbol where the cursor
-- is placed if the LSP server supports it
function lsp.goto_symbol(doc, line, col, implementation)
  if not doc.lsp_open then return end

  for _, name in pairs(lsp.get_active_servers(doc.filename, true)) do
    local server = lsp.servers_running[name]

    local method = ""
    if not implementation then
      if server.capabilities.definitionProvider then
        method = method .. "definition"
      elseif server.capabilities.declarationProvider then
        method = method .. "declaration"
      elseif server.capabilities.typeDefinitionProvider then
        method = method .. "typeDefinition"
      else
        log(server, "Goto definition not supported")
        return
      end
    else
      if server.capabilities.implementationProvider then
        method = method .. "implementation"
      else
        log(server, "Goto implementation not supported")
        return
      end
    end

    -- Send document updates first
    lsp.update_document(doc)

    -- Local patch (#11165): no wire identity means no request.
    local position_params = get_buffer_position_params(doc, line, col)
    if not position_params then
      return
    end

    server:push_request("textDocument/" .. method, {
      params = position_params,
      callback = function(server, response)
        local location = response.result

        if not location or not location.uri and #location == 0 then
          core.log("[LSP] No %s found.", method)
          return
        end

        if not location.uri and #location > 1 then
          listbox.clear()
          for _, loc in pairs(location) do
            local preview, position = get_location_preview(loc)
            listbox.append {
              text = preview,
              info = position,
              location = loc
            }
          end
          listbox.show_list(nil, function(doc, item)
            lsp.goto_location(item.location)
          end)
        else
          if not location.uri then
            location = location[1]
          end
          lsp.goto_location(location)
        end
      end
    })
  end
end

--
-- Thread to process server requests and responses
-- without blocking entirely the editor.
--
core.add_thread(function()
  while true do
    local servers_running = false
    for _,server in pairs(lsp.servers_running) do
      -- Send raw data to server which is usually big and slow in a
      -- non blocking way by creating a coroutine just for it.
      if #server.raw_list > 0 then
        local raw_send = coroutine.create(function()
          server:process_raw()
        end)
        coroutine.resume(raw_send)
        while coroutine.status(raw_send) ~= "dead" do
          -- while sending raw request we only read from lsp to not
          -- conflict with the written raw data so remember no calls
          -- here to: server:process_client_responses()
          -- or server:process_notifications()
          server:process_errors(config.plugins.lsp.log_server_stderr)
          server:process_responses()
          coroutine.yield()
          coroutine.resume(raw_send)
        end
      end

      if not config.plugins.lsp.more_yielding then
        server:process_notifications()
        server:process_requests()
        server:process_responses()
        server:process_client_responses()
      else
        server:process_notifications()
        coroutine.yield()
        server:process_requests()
        coroutine.yield()
        server:process_responses()
        server:process_client_responses()
        coroutine.yield()
      end

      server:process_errors(config.plugins.lsp.log_server_stderr)

      servers_running = true
    end

    if servers_running then
      local wait = 0.01
      if config.plugins.lsp.more_yielding then wait = 0 end
      coroutine.yield(wait)
    else
      coroutine.yield(2)
    end
  end
end)

--
-- Events patching
--
local doc_load = Doc.load
local doc_save = Doc.save
local doc_on_close = Doc.on_close
local doc_raw_insert = Doc.raw_insert
local doc_raw_remove = Doc.raw_remove
local root_view_on_text_input = RootView.on_text_input
local root_view_on_mouse_moved = RootView.on_mouse_moved

function Doc:load(...)
  local res = doc_load(self, ...)
  -- skip new files
  if self.filename and config.plugins.lsp.autostart_server then
    diagnostics.lintplus_init_doc(self)
    core.add_thread(function()
      lsp.start_server(self.filename, core.project_dir)
      lsp.open_document(self)
    end)
  end
  return res
end

function Doc:save(...)
  local old_filename = self.filename
  local res = doc_save(self, ...)
  if old_filename ~= self.filename then
    -- seems to be a new document so we send open notification
    diagnostics.lintplus_init_doc(self)
    core.add_thread(function()
      lsp.open_document(self)
    end)
  else
    core.add_thread(function()
      lsp.update_document(self)
      lsp.save_document(self)
    end)
  end
  return res
end

function Doc:on_close()
  doc_on_close(self)

  -- skip new files
  if not self.filename then return end
  core.add_thread(function()
    lsp.close_document(self)
  end)

  if not config.plugins.lsp.stop_unneeded_servers then
    return
  end

  -- Check if any running lsp servers is not needed anymore and stop it
  for name, server in pairs(lsp.servers_running) do
    local doc_found = false
    for _, docu in ipairs(core.docs) do
      if docu.filename then
        if common.match_pattern(docu.filename, server.file_patterns) then
          doc_found = true
          break
        end
      end
    end

    if not doc_found and not server.exit_timer then
      local t = Timer(server.quit_timeout * 1000, true)
      t.on_timer = function()
        server:exit()
        core.log("[LSP] stopped %s", name)
        lsp.servers_running = util.table_remove_key(lsp.servers_running, name)
      end
      t:start()
      server.exit_timer = t
    end
  end
end

local function add_change(self, text, line1, col1, line2, col2)
  -- Local patch (#11115): accepted mutations queue into the live
  -- server/document session. The protocol version advances exactly once per
  -- emitted batch in lsp.update_document - never per keystroke, never from
  -- editor history - so undo/redo revisiting prior bytes still produces a
  -- strictly increasing stream.
  local change = { range = {}, text = text}
  change.range["start"] = {line = line1-1, character = col1-1}
  change.range["end"] = {line = line2-1, character = col2-1}

  for _, name in pairs(lsp.get_active_servers(self.filename, true)) do
    local server = lsp.servers_running[name]
    local session = lsp.get_document_session(self, server)
    if session then
      table.insert(session.pending_changes, change)
      -- Local patch (#11108): accepted mutations advance the per-session
      -- sequence so held request subjects can detect unbatched edits too.
      session.mutation_seq = (session.mutation_seq or 0) + 1
    else
      -- Edits accepted before this document/session admission: their bytes
      -- are already part of the future didOpen snapshot. The legacy queue
      -- is discarded at admission and never crosses a session boundary.
      self.lsp_changes = self.lsp_changes or {}
      self.lsp_changes[server] = self.lsp_changes[server] or {}
      table.insert(self.lsp_changes[server], change)
    end
  end
end

function Doc:raw_insert(line, col, text, undo_stack, time)
  doc_raw_insert(self, line, col, text, undo_stack, time)

  -- skip new files
  if not self.filename then return end

  col = util.doc_utf8_to_utf16(self, line, col)

  if self.lsp_open then
    add_change(self, text, line, col, line, col)
    lsp.update_document(self)
  elseif #lsp.get_active_servers(self.filename, true) > 0 then
    add_change(self, text, line, col, line, col)
  end
end

function Doc:raw_remove(line1, col1, line2, col2, undo_stack, time)
  local lcol1 = util.doc_utf8_to_utf16(self, line1, col1)
  local lcol2 = util.doc_utf8_to_utf16(self, line2, col2)

  doc_raw_remove(self, line1, col1, line2, col2, undo_stack, time)

  -- skip new files
  if not self.filename then return end

  if self.lsp_open then
    add_change(self, "", line1, lcol1, line2, lcol2)
    lsp.update_document(self)
  elseif #lsp.get_active_servers(self.filename, true) > 0 then
    add_change(self, "", line1, lcol1, line2, lcol2)
  end
end

function RootView:on_text_input(text)
  root_view_on_text_input(self, text)

  -- this part should actually trigger after Doc:raw_insert and Doc:raw_remove
  -- so it is safe to trigger autocompletion from here.
  local av = get_active_docview()

  if av then
    lsp.user_typed = true
    lsp.update_document(av.doc, true)
  end
end

function RootView:on_mouse_moved(x, y, dx, dy)
  root_view_on_mouse_moved(self, x, y, dx, dy)

  if not config.plugins.lsp.mouse_hover then return end

  lsp.hover_position.x = x
  lsp.hover_position.y = y
  if lsp.hover_position.triggered then
    local doc, line, col = lsp.get_hovered_location(x, y)
    if doc == lsp.hover_position.doc and lsp.hover_position.utf8_range then
      local utf8_range = lsp.hover_position.utf8_range
      local line1, col1, line2, col2 = utf8_range.line1, utf8_range.col1,
                                       utf8_range.line2, utf8_range.col2
      if (line > line1 or (line == line1 and col >= col1)) and
         (line < line2 or (line == line2 and col <= col2)) then
        return
      end
    end
    listbox.hide()
    lsp.hover_position.triggered = false
  end
  lsp.hover_timer:set_interval(config.plugins.lsp.mouse_hover_delay)
  lsp.hover_timer:restart()
end

--
-- Add status view item to show document diagnostics count
--
core.status_view:add_item({
  predicate = function()
    local dv = get_active_docview()
    if dv then
      local filename = core.project_absolute_path(dv.doc.filename)
      local diagnostic_messages = diagnostics.get(filename)
      if diagnostic_messages and #diagnostic_messages > 0 then
        return true
      end
    end
    return false
  end,
  name = "lsp:diagnostics",
  alignment = StatusView.Item.RIGHT,
  get_item = function()
    local dv = get_active_docview()
    if dv then
      local filename = core.project_absolute_path(dv.doc.filename)
      local diagnostic_messages = diagnostics.get(filename)

      if diagnostic_messages and #diagnostic_messages > 0 then
        return {
          style.warn,
          style.icon_font, "!",
          style.font, " " .. tostring(#diagnostic_messages)
        }
      end
    end

    return {}
  end,
  command = "lsp:view-document-diagnostics",
  position = 1,
  tooltip = "LSP Diagnostics",
  separator = core.status_view.separator2
})

--
-- Register autocomplete icons
--
if autocomplete.add_icon then
  local autocomplete_icons = {
    { name = "Text",          color = "keyword",  icon = '' }, -- U+F77E
    { name = "Method",        color = "function", icon = '' }, -- U+F6A6
    { name = "Function",      color = "function", icon = '' }, -- U+F794
    { name = "Constructor",   color = "literal",  icon = '' }, -- U+F423
    { name = "Field",         color = "keyword2", icon = 'ﰠ' }, -- U+FC20
    { name = "Variable",      color = "keyword2", icon = '' }, -- U+F52A
    { name = "Class",         color = "literal",  icon = 'ﴯ' }, -- U+FD2F
    { name = "Interface",     color = "literal",  icon = '' }, -- U+F0E8
    { name = "Module",        color = "literal",  icon = '' }, -- U+F487
    { name = "Property",      color = "keyword2", icon = 'ﰠ' }, -- U+FC20
    { name = "Unit",          color = "number",   icon = '塞' }, -- U+F96C
    { name = "Value",         color = "string",   icon = '' }, -- U+F89F
    { name = "Enum",          color = "keyword2", icon = '' }, -- U+F15D
    { name = "Keyword",       color = "keyword",  icon = '' }, -- U+F80A
    { name = "Snippet",       color = "keyword",  icon = '' }, -- U+F44F
    { name = "Color",         color = "string",   icon = '' }, -- U+F8D7
    { name = "File",          color = "string",   icon = '' }, -- U+F718
    { name = "Reference",     color = "string",   icon = '' }, -- U+F706
    { name = "Folder",        color = "string",   icon = '' }, -- U+F74A
    { name = "EnumMember",    color = "number",   icon = '' }, -- U+F15D
    { name = "Constant",      color = "number",   icon = '' }, -- U+F8FE
    { name = "Struct",        color = "keyword2", icon = 'פּ' }, -- U+FB44
    { name = "Event",         color = "keyword",  icon = '' }, -- U+F0E7
    { name = "Operator",      color = "operator", icon = '' }, -- U+F694
    { name = "Unknown",       color = "keyword",  icon = '' }, -- U+F128
    { name = "TypeParameter", color = "literal",  icon = '' }  -- U+EA92
  }

  -- We add the font here to let it automatically scale by the scale plugin
  style.syntax_fonts["lsp_symbols"] = renderer.font.load(
    USERDIR .. "/plugins/lsp/fonts/symbols.ttf",
    15 * SCALE
  )

  for _, icon in ipairs(autocomplete_icons) do
    autocomplete.add_icon(
      icon.name, icon.icon, style.syntax_fonts["lsp_symbols"], icon.color
    )
  end
end

--
-- Commands
--
command.add(
  function()
    local dv = get_active_docview()
    return dv ~= nil and dv.doc.lsp_open, dv and dv.doc or nil
  end, {

  ["lsp:complete"] = function(doc)
    local line1, col1, line2, col2 = doc:get_selection()
    if line1 == line2 and col1 == col2 then
      lsp.request_completion(doc, line1, col1, true)
    end
  end,

  ["lsp:goto-definition"] = function(doc)
    local line1, col1, line2 = doc:get_selection()
    if line1 == line2 then
      lsp.goto_symbol(doc, line1, col1)
    end
  end,

  ["lsp:goto-implementation"] = function(doc)
    local line1, col1, line2 = doc:get_selection()
    if line1 == line2 then
      lsp.goto_symbol(doc, line1, col1, true)
    end
  end,

  ["lsp:show-signature"] = function(doc)
    local line1, col1, line2, col2 = doc:get_selection()
    if line1 == line2 and col1 == col2 then
      lsp.request_signature(doc, line1, col1, true)
    end
  end,

  ["lsp:show-symbol-info"] = function(doc)
    local line1, col1, line2 = doc:get_selection()
    if line1 == line2 then
      lsp.request_hover(doc, line1, col1)
    end
  end,

  ["lsp:show-symbol-info-in-tab"] = function(doc)
    local line1, col1, line2 = doc:get_selection()
    if line1 == line2 then
      lsp.request_hover(doc, line1, col1, true)
    end
  end,

  ["lsp:view-call-hierarchy"] = function(doc)
    local line1, col1, line2 = doc:get_selection()
    if line1 == line2 then
      lsp.request_call_hierarchy(doc, line1, col1)
    end
  end,

  ["lsp:view-document-symbols"] = function(doc)
    lsp.request_document_symbols(doc)
  end,

  ["lsp:format-document"] = function(doc)
    lsp.request_document_format(doc)
  end,

  ["lsp:view-document-diagnostics"] = function(doc)
    lsp.view_document_diagnostics(doc)
  end,

  ["lsp:rename-symbol"] = function(doc)
    local symbol = doc:get_text(doc:get_selection())
    local line1, col1, line2 = doc:get_selection()
    if #symbol > 0 and line1 == line2 then
      core.command_view:enter("New Symbol Name", {
        text = symbol,
        submit = function(new_name)
          lsp.request_symbol_rename(doc, line1, col1, new_name)
        end
      })
    else
      core.log("Please select a symbol on the document to rename.")
    end
  end,

  ["lsp:find-references"] = function(doc)
    local line1, col1, line2 = doc:get_selection()
    if line1 == line2 then
      lsp.request_references(doc, line1, col1)
    end
  end
})

command.add(nil, {
  ["lsp:view-all-diagnostics"] = function()
    lsp.view_all_diagnostics()
  end,

  ["lsp:find-workspace-symbol"] = function()
    local dv = get_active_docview()
    local doc = dv and dv.doc or nil
    local symbol = doc and doc:get_text(doc:get_selection()) or ""
    core.command_view:enter("Find Workspace Symbol", {
      text = symbol,
      submit = function(query)
        lsp.request_workspace_symbol(doc, query)
      end
    })
  end,

  ["lsp:toggle-diagnostics"] = function()
    if not diagnostics.lintplus_found then
      core.error("[LSP] Please install lintplus for diagnostics rendering.")
      return
    end
    lsp.toggle_diagnostics()
  end,

  ["lsp:stop-servers"] = function()
    lsp.stop_servers()
  end,

  ["lsp:start-servers"] = function()
    lsp.start_servers()
  end,

  ["lsp:restart-servers"] = function()
    lsp.stop_servers()
    lsp.start_servers()
  end
})

--
-- Default Keybindings
--
keymap.add {
  ["ctrl+space"]        = "lsp:complete",
  ["ctrl+shift+space"]  = "lsp:show-signature",
  ["alt+a"]             = "lsp:show-symbol-info",
  ["alt+shift+a"]       = "lsp:show-symbol-info-in-tab",
  ["alt+d"]             = "lsp:goto-definition",
  ["alt+shift+d"]       = "lsp:goto-implementation",
  ["alt+s"]             = "lsp:view-document-symbols",
  ["alt+shift+s"]       = "lsp:find-workspace-symbol",
  ["alt+f"]             = "lsp:find-references",
  ["alt+shift+f"]       = "lsp:format-document",
  ["alt+e"]             = "lsp:view-document-diagnostics",
  ["ctrl+alt+e"]        = "lsp:view-all-diagnostics",
  ["alt+shift+e"]       = "lsp:toggle-diagnostics",
  ["alt+c"]             = "lsp:view-call-hierarchy",
  ["alt+r"]             = "lsp:rename-symbol",
}

--
-- Register context menu items
--
local function lsp_predicate(_, _, also_in_symbol)
  local dv = get_active_docview()
  if dv then
    local doc = dv.doc

    if #lsp.get_active_servers(doc.filename, true) < 1 then
      return false
    elseif not also_in_symbol then
      return true
    end

    -- Make sure the cursor is place near a document symbol (word)
    local linem, colm = doc:get_selection()
    local linel, coll = doc:position_offset(linem, colm, translate.start_of_word)
    local liner, colr = doc:position_offset(linem, colm, translate.end_of_word)

    local word_left = doc:get_text(linel, coll, linem, colm)
    local word_right = doc:get_text(linem, colm, liner, colr)

    if #word_left > 0 or #word_right > 0 then
      return true
    end
  end
  return false
end

local function lsp_predicate_symbols()
  return lsp_predicate(nil, nil, true)
end

local menu_found, menu = pcall(require, "plugins.contextmenu")
if menu_found then
  menu:register(lsp_predicate_symbols, {
    menu.DIVIDER,
    { text = "Show Symbol Info",        command = "lsp:show-symbol-info" },
    { text = "Show Symbol Info in Tab", command = "lsp:show-symbol-info-in-tab" },
    { text = "Goto Definition",         command = "lsp:goto-definition" },
    { text = "Goto Implementation",     command = "lsp:goto-implementation" },
    { text = "Find References",         command = "lsp:find-references" }
  })

  menu:register(lsp_predicate, {
    menu.DIVIDER,
    { text = "Document Symbols",       command = "lsp:view-document-symbols" },
    { text = "Document Diagnostics",   command = "lsp:view-document-diagnostics" },
    { text = "Toggle Diagnostics",     command = "lsp:toggle-diagnostics" },
    { text = "Format Document",        command = "lsp:format-document" },
  })

  local menu_show = menu.show
  function menu:show(...)
    lsp.hover_timer:stop()
    lsp.hover_timer:reset()
    listbox.hide()
    lsp.hover_position.triggered = false
    menu_show(self, ...)
  end
end


return lsp


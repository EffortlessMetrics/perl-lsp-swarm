-- Store diagnostic messages received by an LSP.
-- @copyright Jefferson Gonzalez
-- @license MIT

-- Staged exact upstream source for the lite-xl integration train.
-- Upstream subject: lite-xl/lite-xl-lsp diagnostics.lua
--   base ref : d1432ae0736cd9531798b4bc1221835f534cc689
--   base blob: c06bec4955d7fbfd8f3a2753fba26c04247b09e0
local core = require "core"
local config = require "core.config"
local util = require "plugins.lsp.util"
local Timer = require "plugins.lsp.timer"

---@class lsp.diagnostics
local diagnostics = {}

---@class lsp.diagnostics.position
---@field line integer
---@field character integer

---@class lsp.diagnostics.range
---@field start lsp.diagnostics.position
---@field end lsp.diagnostics.position

---@class lsp.diagnostics.severity
---@field ERROR integer
---@field WARNING integer
---@field INFO integer
---@field HINT integer
diagnostics.severity = {
  ERROR = 1,
  WARNING = 2,
  INFO = 3,
  HINT = 4
}

---@alias lsp.diagnostics.severity_code
---|>`diagnostics.severity.ERROR`
---| `diagnostics.severity.WARNING`
---| `diagnostics.severity.INFO`
---| `diagnostics.severity.HINT`

---@class lsp.diagnostics.code_description
---@field href string

---@class lsp.diagnostics.tag
---@field UNNECESSARY integer
---@field DEPRECATED integer
diagnostics.tag = {
  UNNECESSARY = 1,
  DEPRECATED = 2
}

---@alias lsp.diagnostics.tag_code
---|>`diagnostics.tag.UNNECESSARY`
---| `diagnostics.tag.DEPRECATED`

---@class lsp.diagnostics.location
---@field uri string
---@field range lsp.diagnostics.range

---@class lsp.diagnostics.related_information
---@field location lsp.diagnostics.location
---@field message string

---A diagnostic message.
---@class lsp.diagnostics.message
---@field range lsp.diagnostics.position
---@field severity? lsp.diagnostics.severity_code | integer
---@field code? integer | string
---@field codeDescription? lsp.diagnostics.code_description
---@field source? string
---@field message string
---@field tags? lsp.diagnostics.tag_code[]
---@field relatedInformation? lsp.diagnostics.related_information[]
---@field data? any

---A diagnostic item.
---@class lsp.diagnostics.item
---@field filename string
---@field messages lsp.diagnostics.message[]

-- Local patch (#11124): push diagnostics are stored as generation-bound
-- provider publications, never anonymous filename state. Every accepted
-- publication carries the exact provider identity, its process generation,
-- canonical URI, document-session generation, an explicit version
-- disposition ("not_proven" for unversioned publications), and an admission
-- sequence. Versioned publications admit against the owning #11115 session
-- stream by exact equality: delayed older versions cannot replace or clear
-- newer state, future versions are explicit protocol failures, and a
-- replaced process generation cannot publish at all. Complementary
-- providers retain separate source-attributed sets projected into one
-- deterministic visible merge; lifecycle transitions remove exactly the
-- dead subject's ownership. The legacy filename-keyed list remains purely
-- as the derived visible projection consumed by existing readers.

---@type table<integer, lsp.diagnostics.item>
diagnostics.list = {}

---@type integer
diagnostics.count = 0

---Provider publication slots keyed by provider name (#11124).
---@type table<string, {generation: integer, publications: table<string, table<string, table>>}>
diagnostics.providers = {}

---Store-wide admission sequence (#11124).
---@type integer
diagnostics.sequence = 0

---Closed document-session tombstones keyed [uri][session_generation]
---(#11124): late publications bound to a dead session generation stay
---typed failures instead of recreating state.
---@type table<string, table<string, boolean>>
diagnostics.closed_sessions = {}

-- Local patch (#11128): one diagnostic-range presentation authority. Every
-- accepted range is resolved through the exact current editor document and
-- the publication's negotiated position encoding before any inline, list,
-- status or navigation surface consumes it; raw UTF-16 code units are
-- never presented as proven editor byte columns. Presentation is
-- observational: server range objects are never mutated. Unsupported
-- encodings, malformed ranges and out-of-bounds endpoints fail typed
-- instead of silently clamping; closed documents keep their line identity
-- with an explicit not-proven column disposition.
---
---Delayed lintplus rendering re-resolves the live document at execution
---time through an editor-installed resolver bundle (#11124 store supplies
---uri/provider/encoding per projected item); no converted column is ever
---cached across intervening edits.
---
---Resolver bundle (installed once by init.lua):
---  resolve_doc(uri) -> core.doc|nil  live document bound to canonical uri
---  is_current(uri, provider, session_generation, version) -> boolean

local render_resolve_doc = nil
local render_is_current = nil

---Install the editor-side rendering resolver bundle (#11128). Idempotent;
---later installs replace earlier ones.
---@param resolve_doc function
---@param is_current function
function diagnostics.set_render_resolver(resolve_doc, is_current)
  render_resolve_doc = resolve_doc
  render_is_current = is_current
end

---Protocol default position encoding when a server negotiates none
---(#11128). Declared before first use: resolve_range's nil-encoding path
---reads this local, and a Lua local is only visible from its declaration
---point onward.
local Server_default_position_encoding = "utf-16"

---Resolve one LSP range to editor line/column coordinates (#11128).
---
---Returns on success: line1, col1, line2, col2, nil
---For a closed/unavailable document (line identity only):
---  line1, nil, line2, nil, "column_not_proven"
---On failure: nil, nil, nil, nil, disposition
---  "malformed_range" | "unsupported_encoding" | "range_out_of_bounds"
---
---@param range lsp.diagnostics.range|any
---@param doc core.doc|nil Live document when available
---@param position_encoding string|nil Negotiated encoding; utf-16 default
---@return integer|nil, integer|nil, integer|nil, integer|nil, string|nil
function diagnostics.resolve_range(range, doc, position_encoding)
  if
    type(range) ~= "table"
    or type(range.start) ~= "table"
    or type(range["end"]) ~= "table"
    or type(range.start.line) ~= "number"
    or type(range.start.character) ~= "number"
    or type(range["end"].line) ~= "number"
    or type(range["end"].character) ~= "number"
    or range.start.line < 0
    or range.start.character < 0
    or range["end"].line < 0
    or range["end"].character < 0
  then
    return nil, nil, nil, nil, "malformed_range"
  end

  local encoding = position_encoding or Server_default_position_encoding
  if encoding ~= "utf-16" and encoding ~= "utf-8" then
    return nil, nil, nil, nil, "unsupported_encoding"
  end

  local line1 = range.start.line + 1
  local col1 = range.start.character + 1
  local line2 = range["end"].line + 1
  local col2 = range["end"].character + 1

  -- Ordering integrity: start must not land after end.
  if
    line1 > line2
    or (line1 == line2 and col1 > col2)
  then
    return nil, nil, nil, nil, "range_out_of_bounds"
  end

  -- Closed/unavailable document: line identity survives from the range,
  -- columns are explicitly not proven against any bytes.
  if not doc then
    return line1, nil, line2, nil, "column_not_proven"
  end

  -- Open document bounds: both endpoint lines must exist in live bytes.
  if line2 > #doc.lines or line1 < 1 then
    return nil, nil, nil, nil, "range_out_of_bounds"
  end

  if encoding == "utf-8" then
    -- Already byte columns per the negotiated contract; validated against
    -- live line bytes so an out-of-bounds endpoint fails typed instead of
    -- presenting a proven column past the line's bytes (lite-xl lines
    -- include the trailing newline byte).
    if col1 > #doc.lines[line1] or col2 > #doc.lines[line2] then
      return nil, nil, nil, nil, "range_out_of_bounds"
    end
    return line1, col1, line2, col2, nil
  end

  local sl1, sc1, sl2, sc2 = util.toselection(range, doc)
  return sl1, sc1, sl2, sc2, nil
end

---Forward declaration: the derived projection rebuilder (#11124).
local rebuild_projection

-- Try to load lintplus plugin if available for diagnostics rendering
local lintplus_found, lintplus = nil, nil
if config.plugins.lintplus ~= false then
  lintplus_found, lintplus = pcall(require, "plugins.lintplus")
end
local lintplus_kinds = { "error", "warning", "info", "hint" }

---List of linplus coroutines to delay messages population
---@type table<string,lsp.timer>
local lintplus_delays = {}

---Used to set proper diagnostic type on lintplus
---@type table<integer, string>
diagnostics.lintplus_kinds = lintplus_kinds

---@type boolean
diagnostics.lintplus_found = lintplus_found

---@param a lsp.diagnostics.message
---@param b lsp.diagnostics.message
local function sort_helper(a, b)
  local a_severity = a.severity or diagnostics.severity.ERROR
  local b_severity = b.severity or diagnostics.severity.ERROR
  return a_severity < b_severity
end

---Helper to catch some trange occurances where nil is given as filename
---@param filename string|nil
---@return string | nil
local function get_absolute_path(filename)
  if not filename then
    core.error(
      "[LSP Diagnostics]: nil filename given",
      tostring(filename)
    )
    return nil
  end
  return core.project_absolute_path(filename)
end

---Get the position of diagnostics associated to a file.
---@param filename string
---@return integer | nil
function diagnostics.get_index(filename)
  ---@cast filename +nil
  filename = get_absolute_path(filename)
  if not filename then return nil end
  for index, diagnostic in ipairs(diagnostics.list) do
    if diagnostic.filename == filename then
      return index
    end
  end
  return nil
end

---Get the diagnostics associated to a file.
---@param filename string
---@param severity? lsp.diagnostics.severity_code | integer
---@return lsp.diagnostics.message[] | nil
function diagnostics.get(filename, severity)
  ---@cast filename +nil
  filename = get_absolute_path(filename)
  if not filename then return nil end
  for _, diagnostic in ipairs(diagnostics.list) do
    if diagnostic.filename == filename then
      if not severity then return diagnostic.messages end

      local results = {}
      for _, message in ipairs(diagnostic.messages) do
        if (message.severity or diagnostics.severity.ERROR) == severity then
          table.insert(results, message)
        end
      end

      return #results > 0 and results or nil
    end
  end
  return nil
end

---Record current liveness and process generation of one provider (#11124).
---A higher generation replaces the slot: old-generation retained sets die
---with their process. Equal or lower generations are inert refreshes.
---@param name string
---@param generation integer
function diagnostics.note_provider(name, generation)
  local slot = diagnostics.providers[name]
  if not slot or generation > slot.generation then
    diagnostics.providers[name] = { generation = generation, publications = {} }
    rebuild_projection()
  end
end

---Remove one provider's entire visible ownership (#11124).
---@param name string
function diagnostics.retire_provider(name)
  if diagnostics.providers[name] then
    diagnostics.providers[name] = nil
    rebuild_projection()
  end
end

---Invalidate every provider's publications bound to one terminated
---document-session generation (#11124). Lifecycle cleanup leaves no
---retained old-session messages.
---@param uri string
---@param session_generation integer
function diagnostics.close_session(uri, session_generation)
  local changed = false
  for _, slot in pairs(diagnostics.providers) do
    local by_uri = slot.publications[uri]
    if by_uri and by_uri[tostring(session_generation)] then
      by_uri[tostring(session_generation)] = nil
      changed = true
      if not next(by_uri) then slot.publications[uri] = nil end
    end
  end
  -- Tombstone the dead generation so late publications fail typed.
  diagnostics.closed_sessions[uri] = diagnostics.closed_sessions[uri] or {}
  diagnostics.closed_sessions[uri][tostring(session_generation)] = true
  if changed then rebuild_projection() end
end

---Return the stored publication evidence for an exact subject (#11124).
---@param provider string
---@param uri string
---@param session_generation integer
---@return table|nil publication Evidence record with version disposition
function diagnostics.get_publication_evidence(provider, uri, session_generation)
  local slot = diagnostics.providers[provider]
  if not slot then return nil end
  local by_uri = slot.publications[uri]
  return by_uri and by_uri[tostring(session_generation)] or nil
end

---Admit and store one push-diagnostics publication against its exact
---subject (#11124). The subject carries provider identity, process
---generation, document-session binding from #11115, and the session's
---current version.
---@param subject table {provider, generation, has_session, session_generation?, version?}
---@param params table PublishDiagnosticsParams: uri, diagnostics?, version?
---@return boolean accepted
---@return string|nil disposition Typed admission result token
function diagnostics.publish(subject, params)
  local slot = diagnostics.providers[subject.provider]
  if not slot or slot.generation ~= subject.generation then
    return false, "generation_replaced"
  end

  local uri = params.uri
  if type(uri) ~= "string" or #uri == 0 then
    return false, "malformed_publication"
  end
  local messages = params.diagnostics or {}

  -- Unsessioned subjects (closed documents): bounded clearing-only policy.
  -- Servers flush final empty sets on close; content resurrection through
  -- a dead session is a typed failure.
  if not subject.has_session then
    if #messages > 0 then return false, "session_closed" end
    local had_content = slot.publications[uri] ~= nil
    slot.publications[uri] = nil
    rebuild_projection()
    return true, had_content and "cleared_closed" or "already_clear"
  end

  -- Versioned publications admit against the owning session's stream by
  -- exact equality with its current version; anything else is stale or an
  -- impossible future version, never silently "the latest".
  local version_disposition = params.version
  if version_disposition ~= nil then
    if type(version_disposition) ~= "number" then
      return false, "malformed_version"
    end
    if subject.version and version_disposition > subject.version then
      return false, "future_version"
    end
    if subject.version and version_disposition < subject.version then
      return false, "stale_version"
    end
  else
    -- Bounded unversioned policy: admitted under current session identity
    -- only, evidence stays explicitly not-proven, never version-exact.
    version_disposition = "not_proven"
  end

  -- A tombstoned (closed/replaced) session generation can never publish.
  local uri_tombstones = diagnostics.closed_sessions[uri]
  if uri_tombstones and uri_tombstones[tostring(subject.session_generation)] then
    return false, "session_closed"
  end
  -- Prune strictly older tombstones: a newer open generation supersedes
  -- the closed history of its URI.
  if uri_tombstones then
    for generation in pairs(uri_tombstones) do
      if tonumber(generation) < subject.session_generation then
        uri_tombstones[generation] = nil
      end
    end
  end

  diagnostics.sequence = diagnostics.sequence + 1
  local by_uri = slot.publications[uri] or {}
  by_uri[tostring(subject.session_generation)] = {
    provider = subject.provider,
    generation = subject.generation,
    uri = uri,
    session_generation = subject.session_generation,
    version = version_disposition,
    seq = diagnostics.sequence,
    -- Local patch (#11128): the publication's negotiated position encoding
    -- rides with the subject so rendering resolves coordinates faithfully.
    position_encoding = subject.position_encoding or Server_default_position_encoding,
    messages = messages,
  }
  slot.publications[uri] = by_uri
  table.sort(messages, sort_helper)
  rebuild_projection()
  return true, nil
end

---Rebuild the derived visible projection from all provider publications
---(#11124): deterministic provider-name order, severity-sorted messages.
function rebuild_projection()
  local merged = {}
  local names = {}
  for name in pairs(diagnostics.providers) do
    names[#names + 1] = name
  end
  table.sort(names)
  for _, name in ipairs(names) do
    local uris = {}
    for uri in pairs(diagnostics.providers[name].publications) do
      uris[#uris + 1] = uri
    end
    table.sort(uris)
    for _, uri in ipairs(uris) do
      local sessions =
        diagnostics.providers[name].publications[uri]
      local generations = {}
      for generation in pairs(sessions) do
        generations[#generations + 1] = generation
      end
      table.sort(generations)
      for _, generation in ipairs(generations) do
        local publication = sessions[generation]
        -- Local patch (#11165): publication URIs convert through the one
        -- authority; non-file or malformed URIs project nothing, matching
        -- the existing unresolvable-path disposition.
        local publication_path = util.uri_to_path(publication.uri)
        local filename = publication_path and get_absolute_path(publication_path) or nil
        if filename then
          local entry = merged[filename]
          if not entry then
            -- Local patch (#11128): projection entries carry their
            -- publication subject so rendering can resolve the live
            -- document and negotiated encoding per group.
            entry = {
              filename = filename,
              messages = {},
              uri = publication.uri,
              provider = publication.provider,
              session_generation = publication.session_generation,
              version = publication.version,
              position_encoding = publication.position_encoding,
            }
            merged[filename] = entry
          end
          for _, message in ipairs(publication.messages) do
            table.insert(entry.messages, message)
          end
        end
      end
    end
  end

  diagnostics.list = {}
  diagnostics.count = 0
  local filenames = {}
  for filename in pairs(merged) do
    filenames[#filenames + 1] = filename
  end
  table.sort(filenames)
  for _, filename in ipairs(filenames) do
    local entry = merged[filename]
    table.sort(entry.messages, sort_helper)
    diagnostics.count = diagnostics.count + 1
    table.insert(diagnostics.list, entry)
  end
end

---Get the amount of diagnostics associated to a file.
---@param filename string
---@param severity? lsp.diagnostics.severity_code | integer
function diagnostics.get_messages_count(filename, severity)
  local index = diagnostics.get_index(filename)

  if not index then return 0 end

  if not severity then return #diagnostics.list[index].messages end

  local count = 0
  for _, message in ipairs(diagnostics.list[index].messages) do
    if (message.severity or diagnostics.severity.ERROR) == severity then
      count = count + 1
    end
  end

  return count
end

---@param doc core.doc
function diagnostics.lintplus_init_doc(doc)
  if lintplus_found then
    lintplus.init_doc(doc.filename, doc)
  end
end

---Remove registered diagnostics from lintplus for the given file or for
---all files if no filename is given.
---@param filename? string
---@param force boolean
function diagnostics.lintplus_clear_messages(filename, force)
  if lintplus_found then
    if
      not force and lintplus_delays[filename]
      and
      lintplus_delays[filename]:running()
    then
      return
    end
    if filename then
      lintplus.clear_messages(filename)
    else
      for fname, _ in pairs(lintplus.messages) do
        if lintplus_delays[fname] then
          lintplus_delays[fname]:stop()
          lintplus_delays[fname] = nil
        end
        lintplus.clear_messages(fname)
      end
    end
  end
end

---Resolve one projected message group's ranges through the live document
---and negotiated encoding, feeding lintplus (#11128). Ranges whose subject
---no longer resolves render nothing rather than stale coordinates.
---@param item table Projected entry (filename, uri, messages, encoding)
---@param fname string Normalized lintplus target filename
local function populate_item(item, fname)
  local doc = render_resolve_doc and render_resolve_doc(item.uri) or nil
  for _, message in ipairs(item.messages) do
    local line1, col1, _, _, disposition =
      diagnostics.resolve_range(message.range, doc, item.position_encoding)
    if line1 then
      if col1 then
        local text = message.message
        local kind = lintplus_kinds[message.severity or diagnostics.severity.ERROR]
        lintplus.add_message(fname, line1, col1, kind, text)
      else
        -- Closed-document column_not_proven: no inline marker is placed;
        -- raw code units are never presented as byte columns.
        core.log_quiet(
          "[LSP Diagnostics] %s: %s:%d (%s)",
          fname, tostring(message.message), line1, disposition or "column_not_proven"
        )
      end
    else
      core.log_quiet(
        "[LSP Diagnostics] range not rendered (%s): %s",
        disposition or "unresolved", tostring(message.message)
      )
    end
  end
end

---@param filename string
function diagnostics.lintplus_populate(filename)
  if lintplus_found then
    diagnostics.lintplus_clear_messages(filename, true)

    if not filename then
      for _, diagnostic in ipairs(diagnostics.list) do
        local fname = core.normalize_to_project_dir(diagnostic.filename)
        -- Local patch (#11128): live-document, encoding-aware rendering.
        populate_item(diagnostic, fname)
      end
    else
      for _, diagnostic in ipairs(diagnostics.list) do
        if diagnostic.filename == filename then
          populate_item(
            diagnostic, core.normalize_to_project_dir(filename))
        end
      end
    end
  end
end

---@param filename string
---@param user_typed boolean
function diagnostics.lintplus_populate_delayed(filename)
  if lintplus_found then
    if not lintplus_delays[filename] then
      lintplus_delays[filename] = Timer(
        config.plugins.lsp.diagnostics_delay or 500,
        true
      )
      lintplus_delays[filename].on_timer = function()
        -- Local patch (#11128): the timer owns its publication subjects and
        -- revalidates each at execution time; a subject that moved since
        -- scheduling renders nothing instead of stale coordinates.
        if render_is_current then
          for _, item in ipairs(diagnostics.list) do
            if
              (not filename or item.filename == filename)
              and
              not render_is_current(
                item.uri, item.provider,
                item.session_generation, item.version)
            then
              core.log_quiet(
                "[LSP Diagnostics] delayed render skipped (subject moved): %s",
                item.filename or ""
              )
              lintplus_delays[filename] = nil
              return
            end
          end
        end
        diagnostics.lintplus_populate(filename)
        lintplus_delays[filename] = nil
      end
      lintplus_delays[filename]:start()
    else
      lintplus_delays[filename]:reset()
      lintplus_delays[filename]:start()
    end
  end
end


return diagnostics

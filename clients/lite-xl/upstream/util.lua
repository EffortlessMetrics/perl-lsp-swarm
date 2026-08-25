-- Some functions adapted from: https://github.com/orbitalquark/textadept-lsp
-- and others added as needed.
--
-- @copyright Jefferson Gonzalez
-- @license MIT

-- Staged exact upstream source for the lite-xl integration train.
-- Upstream subject: lite-xl/lite-xl-lsp util.lua
--   base ref : d1432ae0736cd9531798b4bc1221835f534cc689
--   base blob: 588c101aa97ef0d112926aac316e7a95a52a6994
-- Local patch (#11155): config.plugins.lsp.log_file is the explicit local
-- sensitive protocol trace. Append failures are guarded (one bounded editor
-- warning per session, never recursive logging), the surface is documented
-- as potentially containing source code, paths and configuration, and its
-- lack of automatic retention/rotation is a declared limitation. It must
-- never be enabled for canonical host or CI proof artifacts.

-- Local patch (#11162): window/showDocument URIs are classified before any
-- display or open action (control characters, malformed percent encoding,
-- unknown schemes and non-local internal targets fail closed with stable
-- reasons), external targets launch through argv-only native handoffs with
-- the target bytes as one inert argument, and the whole prompt/decision/
-- launch/reveal sequence runs through one testable util.show_document seam.
-- No shell command string is ever constructed.

-- Local patch (#11165): local file URI <-> path conversion is owned by one
-- typed authority pair, util.uri_to_path and util.path_to_uri. The legacy
-- touri()/tofilename() prefix-strip/percent-decode helpers are removed; every
-- local-document consumer routes through the pair. The authority requires the
-- admitted `file` scheme (admission itself stays in util.classify_uri and is
-- not broadened), validates percent escapes before decoding exactly once,
-- rejects decoded NUL/control bytes, keeps drive-letter case and POSIX
-- leading-slash runs as data, admits UNC authorities as \\server\share paths
-- on Windows only, and never returns a suspicious input unchanged. Wire URI,
-- platform path, display shaping (home_expand/normalize_to_project_dir) and
-- containment policy (#8997/#9001) remain distinct layers above it.

-- Local patch (#11143): util.deep_merge is a typed configuration merge.
-- Objects merge recursively by string key; arrays are indivisible values
-- replaced atomically by the later side (a later empty array clears an
-- inherited list); scalars and explicit JSON null replace exactly; type
-- changes replace rather than numerically merge. Untagged plain Lua settings
-- values are classified through the #11136 codec's own constructors so
-- sparse/mixed containers fail with codec errors, and an untagged empty
-- table keeps the encoder's object default. Inputs are never mutated or
-- aliased. Configuration-source precedence in init.lua get_workspace_settings
-- is unchanged and documented there.

local core = require "core"
local common = require "core.common"
local config = require "core.config"
local json = require "plugins.lsp.json"
local process = require "process"

local util = {}

---Local/virtual URI classes the client can actually reveal internally
---(#11162), hoisted above first use so the #11165 conversion authority can
---reuse the exact same admission table. The first baseline admits local file
---URIs only; other classes need direct implementation and proof before
---admission.
util.INTERNAL_URI_SCHEMES = {
  file = true,
}

---Check if the given file is currently opened on the editor.
---@param abs_filename string
function util.doc_is_open(abs_filename)
  -- Normalize path format to the one used by normal docs
  abs_filename = common.normalize_path(abs_filename) or abs_filename
  for _, doc in ipairs(core.docs) do
    ---@cast doc core.doc
    if doc.abs_filename == abs_filename then
      return true;
    end
  end
  return false
end

---Converts a utf-8 column position into the equivalent utf-16 position.
---@param doc core.doc
---@param line integer
---@param column integer
---@return integer col_position
function util.doc_utf8_to_utf16(doc, line, column)
  local ltext = doc.lines[line]
  local ltext_len = ltext and #ltext or 0
  local ltext_ulen = ltext and utf8extra.len(ltext) or 0
  column = common.clamp(column, 1, ltext_len > 0 and ltext_len or 1)
  -- no need for conversion so return column as is
  if ltext_len == ltext_ulen then return column end
  if column > 1 then
    local col = 1
    for pos, code in utf8extra.next, ltext do
      if pos >= column then
        return col
      end
      -- Codepoints that high are encoded using surrogate pairs
      if code < 0x010000 then
        col = col + 1
      else
        col = col + 2
      end
    end
    return col
  end
  return column
end

---Converts a utf-16 column position into the equivalent utf-8 position.
---@param doc core.doc
---@param line integer
---@param column integer
---@return integer col_position
function util.doc_utf16_to_utf8(doc, line, column)
  local ltext = doc.lines[line]
  local ltext_len = ltext and #ltext or 0
  local ltext_ulen = ltext and utf8extra.len(ltext) or 0
  column = common.clamp(column, 1, ltext_len > 0 and ltext_len or 1)
  -- no need for conversion so return column as is
  if ltext_len == ltext_ulen then return column end
  if column > 1 then
    local col = 1
    local utf8_pos = 1
    for pos, code in utf8extra.next, ltext do
      if col >= column then
        return pos
      end
      utf8_pos = pos
      -- Codepoints that high are encoded using surrogate pairs
      if code < 0x010000 then
        col = col + 1
      else
        col = col + 2
      end
    end
    return utf8_pos
  end
  return column
end

---Split a string by the given delimeter
---@param s string The string to split
---@param delimeter string Delimeter without lua patterns
---@param delimeter_pattern? string Optional delimeter with lua patterns
---@return table
---@return boolean ends_with_delimiter
function util.split(s, delimeter, delimeter_pattern)
  if not delimeter_pattern then
    delimeter_pattern = delimeter
  end

  local last_idx = 1
  local result = {}
  for match_idx, afer_match_idx in s:gmatch("()"..delimeter_pattern.."()") do
    table.insert(result, string.sub(s, last_idx, match_idx - 1))
    last_idx = afer_match_idx
  end
  if last_idx > #s then
    return result, true
  else
    table.insert(result, string.sub(s, last_idx))
    return result, false
  end
end

---Get the extension component of a filename.
---@param filename string
---@return string
function util.file_extension(filename)
  local parts = util.split(filename, "%.")
  if #parts > 1 then
    return parts[#parts]:gsub("%%", "")
  end

  return filename
end

---Check if a file exists.
---@param file_path string
---@return boolean
function util.file_exists(file_path)
  local file = io.open(file_path, "r")
  if file ~= nil then
    file:close()
    return true
  end
 return false
end

---Maximum accepted URI length in bytes (#11162), hoisted above first use so
---the #11165 conversion authority can reuse the exact same bound: a path is
---convertible exactly when its canonical URI fits this bound, so every
---producer output reads back.
local MAX_URI_BYTES = 2048

---True when the string carries a raw NUL/control byte that cannot be a safe
---local path component (#11165). Applies after percent decoding.
local function has_control_byte(s)
  for index = 1, #s do
    local byte = string.byte(s, index)
    if byte < 0x20 or byte == 0x7f then
      return true
    end
  end
  return false
end

---Decodes every validated %XX escape exactly once (#11165). Callers must
---reject malformed escapes first (util.classify_uri does); `%25` yields the
---literal `%` byte and is never re-scanned.
local function decode_percent_bytes(s)
  return s:gsub(
    "%%(%x%x)",
    function(hex) return string.char(tonumber(hex, 16)) end
  )
end

---Encodes one raw path into URI path bytes exactly once (#11165). Unreserved
---bytes, `/` separators and `:` (drive-colon form per RFC 8089, legal pchar)
---stay literal; every other byte becomes uppercase %XX.
local function encode_path_bytes(s)
  return s:gsub("[^%w%.%~%-/%:]", function(char)
    return string.format("%%%02X", string.byte(char))
  end)
end

---Authority table reusing the #11162 internal admission policy unchanged:
---only local `file` URIs are convertible; virtual/remote schemes fail closed.
local LOCAL_FILE_SCHEMES = util.INTERNAL_URI_SCHEMES

---Converts one LSP DocumentUri into the exact local platform path (#11165).
---
---Typed authority for the wire-URI -> platform-path direction. Returns nil
---with a stable reason token instead of ever returning a suspicious input
---unchanged. Semantics:
--- - only the admitted `file` scheme converts (`unsupported_scheme` etc. from
---   util.classify_uri, which also bounds length and rejects malformed
---   percent escapes and raw control characters);
--- - empty or `localhost` authority is local; any other authority is an
---   admitted UNC source on Windows only (`\\server\share\...`) and
---   `remote_authority` elsewhere; authorities carrying userinfo/port bytes
---   are never local;
--- - raw query/fragment components (`?`/`#`) are stripped before decoding;
---   encoded `%23`/`%3F` filename bytes stay data;
--- - decoded path bytes are decoded exactly once; NUL/control results are
---   rejected;
--- - POSIX paths keep leading-slash runs as data and must be absolute;
--- - Windows accepts only drive form (`C:\...`, case preserved) or UNC;
---   device namespaces (`\\?\`) and rooted-without-drive shapes are refused.
---@param uri any Candidate wire URI from a server or editor surface
---@return string|nil path Exact local path when convertible
---@return string|nil failure_reason Stable rejection token otherwise
function util.uri_to_path(uri)
  local ok, reason = util.classify_uri(uri, LOCAL_FILE_SCHEMES)
  if not ok then
    return nil, reason
  end

  -- classify_uri validated the scheme grammar, so the first colon is the
  -- scheme delimiter regardless of letter case.
  local rest = uri:sub(uri:find(":", 1, true) + 1)

  -- Raw `#` and `?` start fragment/query components (RFC 3986); they never
  -- belong to a local path. Literal bytes in filenames arrive percent-encoded
  -- (%23/%3F) and survive as data. Cut before any decoding.
  local fragment = rest:find("#", 1, true)
  if fragment then
    rest = rest:sub(1, fragment - 1)
  end
  local query = rest:find("?", 1, true)
  if query then
    rest = rest:sub(1, query - 1)
  end

  local authority = ""
  if rest:sub(1, 2) == "//" then
    local auth_end = rest:find("/", 3, true)
    if auth_end then
      authority = rest:sub(3, auth_end - 1)
      rest = rest:sub(auth_end)
    else
      authority = rest:sub(3)
      rest = ""
    end
  elseif rest ~= "" and rest:sub(1, 1) ~= "/" then
    -- file:relative/path and friends carry no absolute path component.
    return nil, "relative_path"
  end

  authority = decode_percent_bytes(authority)
  if has_control_byte(authority) then
    return nil, "control_character"
  end
  if authority:find("@", 1, true) or authority:find(":", 1, true) then
    -- Userinfo or port bytes never name a local volume or SMB host.
    return nil, "remote_authority"
  end
  if authority:find("/", 1, true) or authority:find("\\", 1, true) then
    -- Encoded separators decode into UNC structure (#11165 review); host
    -- names never contain them, so this is malformed rather than data.
    return nil, "invalid_unc"
  end

  local is_local_authority = authority == ""
    or authority:lower() == "localhost"
  local is_unc_source = not is_local_authority

  local path = decode_percent_bytes(rest)
  if has_control_byte(path) then
    return nil, "control_character"
  end

  if PLATFORM == "Windows" then
    if is_unc_source then
      -- Mirror the producer grammar (#11165 review): a UNC target needs a
      -- non-empty share segment; authority plus bare separators
      -- ("file://server//") is not one.
      local share_path = path:gsub("^/+", "")
      if share_path == "" or not share_path:match("[^/]") then
        return nil, "invalid_unc"
      end
      return "\\\\" .. authority .. "\\" .. share_path:gsub("/", "\\"), nil
    end
    local drive = path:match("^/([%a]):")
    if not drive then
      return nil, "unsupported_path_shape"
    end
    if #path < 4 or path:sub(4, 4) ~= "/" then
      -- "/C:" and "/C:x" leave a drive-relative residue that Windows would
      -- resolve against drive C's current directory; refuse honestly.
      return nil, "relative_path"
    end
    return path:sub(2):gsub("/", "\\"), nil
  end

  if is_unc_source then
    return nil, "remote_authority"
  end
  if path:sub(1, 1) ~= "/" then
    return nil, "relative_path"
  end
  return path, nil
end

---Converts one exact local platform path into its canonical `file:` URI
---(#11165). Typed authority for the platform-path -> wire-URI direction.
---Semantics:
--- - POSIX requires an absolute path; leading-slash runs are preserved as
---   data so `//data/x` round trips through `file:////data/x`;
--- - Windows admits drive-rooted paths (`C:\x`, `C:/x`; case preserved,
---   separators normalized to `/`) and UNC paths
---   (`\\server\share\...` -> `file://server/share/...`);
--- - device namespaces (`\\?\`, `\\.\`), drive-relative and relative shapes
---   are refused instead of guessed onto the current drive;
--- - every non-unreserved byte is percent-encoded exactly once, so spaces,
---   `#`, `?`, `%`, quotes and multi-byte UTF-8 round trip without double
---   encoding and literal `%` stays `%25`;
--- - the canonical URI must fit the same MAX_URI_BYTES bound uri_to_path
---   enforces (`path_above_wire_bound`), so producer output always reads
---   back — the pair cannot emit identities its own consumer refuses.
---@param path any Candidate exact local path
---@return string|nil uri Canonical file URI when convertible
---@return string|nil failure_reason Stable rejection token otherwise
function util.path_to_uri(path)
  if type(path) ~= "string" or #path == 0 then
    return nil, "empty_path"
  end
  if has_control_byte(path) then
    return nil, "control_character"
  end

  local uri
  if PLATFORM == "Windows" then
    if path:match("^[\\/][\\/][%?%.][\\/]") then
      -- Device namespace forms are not filesystem shares.
      return nil, "unsupported_device_path"
    end
    if path:sub(1, 2) == "\\\\" or path:sub(1, 2) == "//" then
      local unc = path:sub(3):gsub("[\\/]", "/")
      if not unc:match("^[^/]+/[^/]") then
        -- \\server alone (with or without a dangling separator) names no
        -- share; a UNC target needs host plus share components.
        return nil, "invalid_unc"
      end
      -- Symmetric with uri_to_path (#11165 review): `localhost` authority is
      -- the local machine in file URI space, so a \\localhost\... UNC would
      -- produce an identity this same authority reads back as a drive-less
      -- local path. Refuse instead of emitting an unreadable form.
      if unc:lower():match("^localhost/") then
        return nil, "invalid_unc"
      end
      uri = "file://" .. encode_path_bytes(unc)
    elseif not path:match("^[%a]:[\\/]") then
      -- Relative, drive-relative ("C:x") and rooted-without-drive ("\x")
      -- shapes would silently bind to an unknown current drive.
      return nil, "relative_path"
    else
      uri = "file:///" .. encode_path_bytes(path:gsub("\\", "/"))
    end
  else
    if path:sub(1, 1) ~= "/" then
      return nil, "relative_path"
    end
    uri = "file://" .. encode_path_bytes(path)
  end

  if #uri > MAX_URI_BYTES then
    -- Symmetric bound (#11165): a canonical URI above the read-back limit
    -- could never round trip through this same authority.
    return nil, "path_above_wire_bound"
  end
  return uri, nil
end

---Converts a document range returned by lsp to a valid document selection.
---@param range table LSP Range.
---@param doc? core.doc
---@return integer line1
---@return integer col1
---@return integer line2
---@return integer col2
function util.toselection(range, doc)
  local line1 = range.start.line + 1
  local col1 = range.start.character + 1
  local line2 = range['end'].line + 1
  local col2 = range['end'].character + 1

  if doc then
    col1 = util.doc_utf16_to_utf8(doc, line1, col1)
    col2 = util.doc_utf16_to_utf8(doc, line2, col2)
  end

  return line1, col1, line2, col2
end

---Opens the given location on an external application without shell
---construction (#11162).
---
---The launcher is resolved independently of the target and the target bytes
---stay one inert argv element, so spaces, quotes, ampersands, semicolons,
---pipes, percent signs, Unicode and leading dashes can never become launcher
---syntax. Deprecated shell-like system.exec is not used.
---@param location string Admitted external URI (see util.EXTERNAL_URI_SCHEMES)
---@return boolean launched
---@return string|nil failure_reason
---@return integer|nil pid Launcher process id when the handoff was accepted
function util.open_external(location)
  local admitted, reason = util.classify_uri(location, util.EXTERNAL_URI_SCHEMES)
  if not admitted then
    return false, reason
  end

  local launcher = {}
  if PLATFORM == "Windows" then
    -- Native protocol handler invocation: argv only, no cmd.exe, no start.
    launcher = { "rundll32", "url.dll,FileProtocolHandler" }
  elseif PLATFORM == "Mac OS X" then
    launcher = { "open" }
  else
    launcher = { "xdg-open" }
  end

  local argv = {}
  for index, argument in ipairs(launcher) do
    argv[index] = argument
  end
  argv[#argv + 1] = location

  local pid = process.start(argv, {})
  if not pid then
    return false, "launch_failed"
  end

  return true, nil, pid
end

---Reviewed externally openable URI schemes (#11162). Deliberately handled
---file URIs stay admitted; anything else fails closed instead of being
---delegated blindly to a shell.
util.EXTERNAL_URI_SCHEMES = {
  http = true,
  https = true,
  file = true,
}

---True when the text carries a percent escape that is not exactly two hex
---digits (#11162).
local function has_malformed_percent_encoding(uri)
  local position = 1
  while true do
    local percent = uri:find("%", position, true)
    if not percent then
      return false
    end
    if uri:sub(percent + 1, percent + 2):match("^%x%x$") == nil then
      return true
    end
    position = percent + 1
  end
end

---Classifies one URI against a reviewed scheme admission table (#11162).
---
---Syntax is validated before any display or open action: control characters
---and malformed percent encoding are rejected with stable reasons, and the
---scheme is matched case-insensitively without rewriting the target bytes.
---@param uri any Candidate URI from a server-supplied request
---@param admitted table Set of lowercase admitted scheme names
---@return boolean ok
---@return string|nil failure_reason Stable rejection token when not admitted
---@return string|nil scheme Lowercase admitted scheme when admitted
function util.classify_uri(uri, admitted)
  if type(uri) ~= "string" or #uri == 0 then
    return false, "empty_uri"
  end
  if #uri > MAX_URI_BYTES then
    return false, "uri_above_bound"
  end
  for index = 1, #uri do
    local byte = string.byte(uri, index)
    if byte < 0x20 or byte == 0x7f then
      return false, "control_character"
    end
  end
  local scheme = uri:match("^(%a[%w+%.%-]*):")
  if not scheme then
    return false, "malformed_uri"
  end
  if has_malformed_percent_encoding(uri) then
    return false, "malformed_percent_encoding"
  end
  scheme = scheme:lower()
  if not admitted[scheme] then
    return false, "unsupported_scheme"
  end
  return true, nil, scheme
end

---Handles one window/showDocument request through one truthful sequence:
---classify the URI against the reviewed policy, preserve the exact user
---decision, launch/reveal through safe seams, and surface one outcome for
---the exact ShowDocumentResult response (#11162).
---
---Local patch (#10873): the outcome is truthful and exactly one per request.
---External decisions may be synchronous or asynchronous. In async mode
---hooks.confirm(scheme, uri, answered) shows the host prompt, returns nil,
---and later calls answered(accepted) once; the sequence resumes from the
---terminal decision, so no response exists before the user outcome. Every
---deferred transition passes hooks.alive() first: when the owning server was
---replaced or stopped while the prompt was open the stale sequence is inert
---and answers nothing (returns nil, "stale"). hooks.outcome(success, reason)
---observes each terminal outcome for delivery; reveal(uri) returns
---truthy-on-open plus an optional typed failure reason so a failed internal
---open or selection conversion is distinguishable from success.
---
---Sync-mode contract is unchanged: confirm(scheme, uri) returning a boolean
---settles immediately and the function returns success, reason as before.
---
---@param server table LSP server (name used by the host prompt title)
---@param params table Request params: uri, external, selection, takeFocus
---@param hooks table|nil Editor seams: confirm, reveal, raise, alive, outcome
---@return boolean|nil success nil only when an async prompt is pending/stale
---@return string|nil failure_reason Stable token when not successful
function util.show_document(server, params, hooks)
  hooks = hooks or {}
  local uri = params and params.uri

  local function finish(success, reason)
    if hooks.outcome then
      hooks.outcome(success, reason)
    end
    return success, reason
  end

  -- One terminal settle path for the external decision: decline reports
  -- false, accept launches and reports the observable launch result. A
  -- repeated host answer is inert (#10873 review): the sequence settles at
  -- most once, so a double Yes/No delivery cannot launch twice or answer
  -- twice even before the listener's own deduplication.
  local settled = false
  local function settle(accepted)
    if settled then
      return nil, "already_settled"
    end
    settled = true
    if hooks.alive and not hooks.alive() then
      return nil, "stale"
    end
    if not accepted then
      return finish(false, "user_declined")
    end
    local launched, launch_reason = util.open_external(uri)
    if not launched then
      return finish(false, launch_reason or "launch_failed")
    end
    return finish(true, nil)
  end

  if params and params.external then
    local external_ok, external_reason, scheme =
      util.classify_uri(uri, util.EXTERNAL_URI_SCHEMES)
    if not external_ok then
      return finish(false, external_reason)
    end
    if hooks.confirm then
      local decided = hooks.confirm(scheme, uri, settle)
      if decided == nil then
        -- Async prompt owns the rest of the sequence; nothing is answered
        -- until the user/open outcome exists (#10873).
        return nil, "pending"
      end
      return settle(decided)
    end
    return settle(true)
  end

  local internal_ok, internal_reason = util.classify_uri(
    uri, util.INTERNAL_URI_SCHEMES)
  if not internal_ok then
    return finish(false, internal_reason)
  end
  if not hooks.reveal then
    return finish(false, "reveal_unavailable")
  end
  local revealed, reveal_reason = hooks.reveal(uri)
  if not revealed then
    return finish(false, reveal_reason or "reveal_failed")
  end
  if params and params.takeFocus and hooks.raise then
    hooks.raise()
  end
  return finish(true, nil)
end

---One-shot flag so trace-file failures never recurse or spam (#11155).
local trace_failure_warned = false

---Bounded error text for the one-shot trace warning (#11155).
local function bound_trace_error(err)
  err = tostring(err or "unknown")
  if #err > 120 then
    return err:sub(1, 120) .. "...[truncated]"
  end
  return err
end

---Prettify json output and logs it if config.lsp.log_file is set.
---@param code string
---@return string
function util.jsonprettify(code)
  if config.plugins.lsp.prettify_json then
    code = json.prettify(code)
  end

  if config.plugins.lsp.log_file and #config.plugins.lsp.log_file > 0 then
    -- Explicit sensitive protocol trace (#11155): this file receives complete
    -- protocol payloads and may contain source code, paths, configuration
    -- values and anything servers emit. It is opt-in (empty default), has no
    -- automatic retention or rotation (declared limitation), and must never
    -- be enabled for canonical host or CI proof artifacts. Append failures
    -- warn once per session through the editor log and never recurse.
    local log, err_open = io.open(config.plugins.lsp.log_file, "a+")
    if log then
      local ok_write, err_write = pcall(
        log.write, log, "Output: \n" .. tostring(code) .. "\n\n")
      local ok_close = pcall(log.close, log)
      if not (ok_write and ok_close) and not trace_failure_warned then
        trace_failure_warned = true
        core.log(
          "lsp: protocol trace file '%s' became unwritable (%s); "
            .. "further write failures stay silent for this session",
          config.plugins.lsp.log_file,
          bound_trace_error(err_write)
        )
      end
    elseif not trace_failure_warned then
      trace_failure_warned = true
      core.log(
        "lsp: protocol trace file '%s' could not be opened (%s); "
          .. "further open failures stay silent for this session",
        config.plugins.lsp.log_file,
        bound_trace_error(err_open)
      )
    end
  end

  return code
end

---Gets the last component of a path. For example:
---/my/path/to/somwhere would return somewhere.
---@param path string
---@return string
function util.getpathname(path)
  local components = {}
  if PLATFORM == "Windows" then
    components = util.split(path, "\\")
  else
    components = util.split(path, "/")
  end

  if #components > 0 then
    return components[#components]
  end

  return path
end

---Check if a value is on a table.
---@param value any
---@param table_array table
---@return boolean
function util.intable(value, table_array)
  for _, element in pairs(table_array) do
    if element == value then
      return true
    end
  end

  return false
end

---Remove by key from a table and returns a new
---table with element removed.
---@param table_object table
---@param key_name string|integer
---@return table
function util.table_remove_key(table_object, key_name)
  local new_table = {}
  for key, data in pairs(table_object) do
    if key ~= key_name then
      new_table[key] = data
    end
  end

  return new_table
end

---Get a table specific field or nil if not found.
---@param t table The table we are going to search for the field.
---@param fieldset string A field spec in the format
---"parent[.child][.subchild]" eg: "myProp.subProp.subSubProp"
---@return any|nil The value of the given field or nil if not found.
---@return boolean found Whether the field exists, tracked separately from
---the value so an explicitly configured false stays distinct from a missing
---section (#10845). Traversal recurses only into tables; a missing or
---non-table intermediate keeps the whole path not-found.
function util.table_get_field(t, fieldset)
  local fields = util.split(fieldset, ".", "%.")
  local field = fields[1]
  local value = nil
  local found = false

  if field then
    if #fields > 1 then
      if type(t[field]) == "table" then
        value, found = util.table_get_field(
          t[field], table.concat(fields, ".", 2))
      end
    elseif t[field] ~= nil then
      value = t[field]
      found = true
    end
  end

  return value, found
end

-- Local patch (#11143): typed settings-value helpers for deep_merge. Every
-- merged value resolves through ONE classification authority: the #11136
-- codec container identities first, then the codec's own constructors for
-- untagged plain Lua settings values, so sparse/mixed containers fail with
-- codec errors instead of receiving ad-hoc merge behavior. Arrays are
-- indivisible configuration values; objects are the only recursive shape;
-- every stored table is freshly built so inputs stay immutable and results
-- deterministic.
local function settings_container_kind(value)
  if type(value) ~= "table" then return nil end
  -- Scalar-like typed identities: explicit JSON null and exact-lexeme
  -- numbers are replace-exactly values, never containers to recurse into.
  if json.is_null(value) or json.is_number(value) then return nil end
  if json.is_array(value) then return "array" end
  if json.is_object(value) then return "object" end
  -- Untagged plain Lua table: mirror exactly the encoder's upstream
  -- compatibility heuristic. An untagged empty table stays an object ({}
  -- encodes as {}); a dense numeric-key sequence is an array; anything else
  -- must be accepted by one of the codec constructors or merging fails.
  if next(value) == nil then return "object" end
  if rawget(value, 1) ~= nil then
    if pcall(json.array, value) then return "array" end
  end
  local ok_object, object_error = pcall(json.object, value)
  if ok_object then return "object" end
  error(object_error, 0)
end

local function copy_settings_value(value)
  if type(value) ~= "table" then return value end
  local kind = settings_container_kind(value)
  if kind == nil then return value end
  local out = {}
  if kind == "array" then
    for i = 1, #value do
      out[i] = copy_settings_value(value[i])
    end
  else
    for k, v in pairs(value) do
      out[k] = copy_settings_value(v)
    end
  end
  return setmetatable(out, getmetatable(value))
end

local merge_settings_value

local function merge_settings_objects(base, later)
  local out = {}
  for k, base_value in pairs(base) do
    if later[k] == nil then
      out[k] = copy_settings_value(base_value)
    end
  end
  for k, later_value in pairs(later) do
    out[k] = merge_settings_value(base[k], later_value)
  end
  -- Typed object identity survives merging: the later side takes precedence,
  -- mirroring copy_settings_value so every produced settings table carries
  -- the same explicit #11136 container identity it would have been copied
  -- with (#12215 review).
  return setmetatable(out, getmetatable(later) or getmetatable(base))
end

---Merge one settings value slot (#11143): scalars and scalar-like typed
---identities replace exactly; any array on either side makes the later side
---an atomic replacement; two objects recurse by string key.
merge_settings_value = function(base, later)
  if type(later) ~= "table" then return later end
  local later_kind = settings_container_kind(later)
  if later_kind == nil then return later end
  if type(base) ~= "table" then return copy_settings_value(later) end
  local base_kind = settings_container_kind(base)
  if base_kind == nil then return copy_settings_value(later) end
  if base_kind == "array" or later_kind == "array" then
    return copy_settings_value(later)
  end
  return merge_settings_objects(base, later)
end

---Merge the content of the tables into a new one.
---Arguments from the later tables take precedence.
---Doesn't touch the original tables.
---`nil` arguments are ignored.
---
---Local patch (#11143): merging folds the arguments as whole typed VALUES,
---left-associatively, so the shape rules apply at every depth including the
---settings root: objects merge recursively by string key; arrays are
---indivisible configuration values replaced atomically by the later side
---(a later empty array clears an inherited list); scalars, booleans,
---strings, numbers and explicit JSON null replace exactly; a type change
---replaces with the later typed value instead of numerically merging
---incompatible shapes. Inputs are never mutated or aliased. Untagged plain
---Lua settings values (`.lite_lsp.lua`, `server.settings`) are classified
---through the codec's own constructors so sparse/mixed containers fail with
---codec errors rather than receiving ad-hoc merge behavior; an untagged
---empty table keeps the encoder's upstream-compatibility object default.
---@param ... table?
---@return table
function util.deep_merge(...)
  local t = nil
  local args = table.pack(...)
  for i=1,args.n do
    local other = args[i]
    if other then
      assert(type(other) == "table", string.format("Argument %d must be a table", i))
      if t == nil then
        t = copy_settings_value(other)
      else
        t = merge_settings_value(t, other)
      end
    end
  end
  return t or {}
end

---Check if a table is really empty.
---@param t table
---@return boolean
function util.table_empty(t)
  return next(t) == nil
end

---Convert markdown to plain text.
---@param text string
---@return string
function util.strip_markdown(text)
  local clean_text = ""
  local prev_line = ""
  for match in (text.."\n"):gmatch("(.-)".."\n") do
    match = match .. "\n"

    -- strip markdown
    local new_line = match
      -- Block quotes
      :gsub("^>+(%s*)", "%1")
      -- headings
      :gsub("^(%s*)######%s(.-)\n", "%1%2\n")
      :gsub("^(%s*)#####%s(.-)\n", "%1%2\n")
      :gsub("^(%s*)####%s(.-)\n", "%1%2\n")
      :gsub("^(%s*)####%s(.-)\n", "%1%2\n")
      :gsub("^(%s*)###%s(.-)\n", "%1%2\n")
      :gsub("^(%s*)##%s(.-)\n", "%1%2\n")
      :gsub("^(%s*)#%s(.-)\n", "%1%2\n")
      -- heading custom id
      :gsub("{#.-}", "")
      -- emoji
      :gsub(":[%w%-_]+:", "")
      -- bold and italic
      :gsub("%*%*%*(.-)%*%*%*", "%1")
      :gsub("___(.-)___", "%1")
      :gsub("%*%*_(.-)_%*%*", "%1")
      :gsub("__%*(.-)%*__", "%1")
      :gsub("___(.-)___", "%1")
      -- bold
      :gsub("%*%*(.-)%*%*", "%1")
      :gsub("__(.-)__", "%1")
      -- strikethrough
      :gsub("%-%-(.-)%-%-", "%1")
      -- italic
      :gsub("%*(.-)%*", "%1")
      :gsub("%s_(.-)_%s", "%1")
      :gsub("\\_(.-)\\_", "_%1_")
      :gsub("^_(.-)_", "%1")
      -- code
      :gsub("^%s*```(%w+)%s*\n", "")
      :gsub("^%s*```%s*\n", "")
      :gsub("``(.-)``", "%1")
      :gsub("`(.-)`", "%1")
      -- lines
      :gsub("^%-%-%-%-*%s*\n", "")
      :gsub("^%*%*%*%**%s*\n", "")
      -- reference links
      :gsub("^%[[^%^](.-)%]:.-\n", "")
      -- footnotes
      :gsub("^%[%^(.-)%]:%s+", "[%1]: ")
      :gsub("%[%^(.-)%]", "[%1]")
      -- Images
      :gsub("!%[(.-)%]%((.-)%)", "")
      -- links
      :gsub("%s<(.-)>%s", "%1")
      :gsub("%[(.-)%]%s*%[(.-)%]", "%1")
      :gsub("%[(.-)%]%((.-)%)", "%1: %2")
      -- remove escaped punctuations
      :gsub("\\(%p)", "%1")

    -- if paragraph put in same line
    local is_paragraph = false

    local prev_spaces = prev_line:match("^%g+")
    local prev_endings = prev_line:match("[ \t\r\n]+$")
    local new_spaces = new_line:match("^%g+")

    if prev_spaces and new_spaces then
      local new_lines = prev_endings ~= nil
        and prev_endings:gsub("[ \t\r]+", "") or ""

      if #new_lines == 1 then
        is_paragraph = true
        clean_text = clean_text:gsub("[%s\n]+$", "")
          .. " " .. new_line:gsub("^%s+", "")
      end
    end

    if not is_paragraph then
      clean_text = clean_text .. new_line
    end

    prev_line = new_line
  end
  return clean_text
end

---@param text string
---@param font renderer.font
---@param max_width number
function util.wrap_text(text, font, max_width)
  local lines = util.split(text, "\n")
  local wrapped_text = ""
  local longest_line = 0;
  for _, line in ipairs(lines) do
    local line_len = line:ulen() or 0
    if line_len > longest_line then
      longest_line = line_len
      local line_width = font:get_width(line)
      if line_width > max_width then
        local words = util.split(line, " ")
        local new_line = words[1] and words[1] or ""
        wrapped_text = wrapped_text .. new_line
        for w=2, #words do
          if font:get_width(new_line .. " " .. words[w]) <= max_width then
            new_line = new_line .. " " .. words[w]
            wrapped_text = wrapped_text .. " " .. words[w]
          else
            wrapped_text = wrapped_text .. "\n" .. words[w]
            new_line = words[w]
          end
        end
        wrapped_text = wrapped_text .. "\n"
      else
        wrapped_text = wrapped_text .. line .. "\n"
      end
    else
      wrapped_text = wrapped_text .. line .. "\n"
    end
  end

  wrapped_text = wrapped_text:gsub("\n\n\n\n?", "\n\n"):gsub("%s*$", "")

  return wrapped_text
end


return util

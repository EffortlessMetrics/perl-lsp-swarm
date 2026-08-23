-- URI classification and safe external-launch planning for the Lite XL
-- window/showDocument surface (#11162).
--
-- This module is the reviewed client capability boundary consumed by the
-- truthful showDocument listener (#10873): it parses and classifies a
-- server-supplied URI before any display or open action, admits only
-- explicitly reviewed scheme classes, and plans external launch through an
-- argv table with zero shell-command construction. It never executes
-- anything itself; the caller owns prompting, process.start, and the exact
-- ShowDocumentResult response.
--
-- Policy (issue #11162 one-PR contract):
--   * Parse URI syntax before any action. Reject embedded NUL/control bytes
--     and malformed percent encoding.
--   * external=true admits only http, https, and deliberately handled file.
--   * external=false (internal reveal) accepts only the implemented local
--     file class; nothing falls through into path conversion on a mismatched
--     scheme.
--   * Unknown schemes and malformed forms fail closed with bounded reason
--     tokens; reason strings never embed target bytes.
--   * The raw URI is never rewritten: admitted targets are handed to the
--     prompt/launcher byte-exactly. Scheme comparison is case-insensitive;
--     the reported scheme is lowercased for policy only.
--   * Platform launchers are resolved independently of the URI and the
--     target stays exactly one argv element, so spaces, quotes, ampersands,
--     semicolons, pipes, percent signs, Unicode, and leading dashes remain
--     inert data. Windows avoids cmd.exe entirely via the rundll32 shell-
--     execute handoff instead of `start` string building.
--
-- Pure Lua 5.4 (Lite XL runtime family). No globals, no I/O, no process
-- APIs at load time or run time.

local uri_policy = {}

---Externally openable schemes admitted after user confirmation (#11162).
uri_policy.EXTERNAL_SCHEMES = {
  http = true,
  https = true,
  file = true,
}

---Schemes this client can reveal inside the editor (#11162 baseline).
uri_policy.INTERNAL_SCHEMES = {
  file = true,
}

---Platform launcher argv heads, resolved independently from URI bytes.
---Each head is prepended verbatim; the untrusted target is always appended
---as one additional element.
local LAUNCHERS = {
  ["Windows"]  = { "rundll32", "url.dll,FileProtocolHandler" },
  ["Mac OS X"] = { "open" },
  ["Linux"]    = { "xdg-open" },
}

local function reject(reason)
  return { error = true, reason = reason }
end

---Returns true when the byte at index i is a control character that must
---never reach a prompt, path conversion, or launcher argument.
local function is_control_byte(byte)
  return byte < 0x20 or byte == 0x7F
end

local function has_control_bytes(text)
  for i = 1, #text do
    if is_control_byte(string.byte(text, i)) then
      return true
    end
  end
  return false
end

---Validates percent-encoding: every % must introduce exactly two hex digits.
local function has_malformed_percent_encoding(text)
  local pos = text:find("%%", 1, false)
  while pos do
    if not text:sub(pos + 1, pos + 2):match("^%x%x$") then
      return true
    end
    pos = text:find("%%", pos + 1, false)
  end
  return false
end

---Decodes %xx sequences in an already validated string.
local function percent_decode(text)
  return text:gsub("%%(%x%x)", function(hex)
    return string.char(tonumber(hex, 16))
  end)
end

---Parses the hierarchical form `scheme://rest` without rewriting anything.
---Returns a table {raw, scheme, target} or nil plus a bounded reason token.
function uri_policy.parse(uri)
  if type(uri) ~= "string" or #uri == 0 then
    return nil, "invalid_uri"
  end
  if has_control_bytes(uri) then
    return nil, "control_character"
  end
  if has_malformed_percent_encoding(uri) then
    return nil, "malformed_percent_encoding"
  end
  local scheme, target = uri:match("^(%a[%w%.%+%-]*)://(.*)$")
  if not scheme then
    return nil, "missing_scheme"
  end
  return {
    raw = uri,
    scheme = scheme:lower(),
    target = target,
  }
end

---Builds the external-launch argv for one admitted URI on one platform.
---Returns the argv table (launcher head plus exactly one target element) or
---nil plus a bounded reason token. Never builds a command string.
function uri_policy.external_launch(uri, platform)
  local parsed, why = uri_policy.parse(uri)
  if not parsed then
    return nil, why
  end
  if not uri_policy.EXTERNAL_SCHEMES[parsed.scheme] then
    return nil, "scheme_not_allowed"
  end
  local launcher = LAUNCHERS[platform]
  if not launcher then
    return nil, "unsupported_platform"
  end
  local argv = {}
  for i = 1, #launcher do
    argv[i] = launcher[i]
  end
  argv[#launcher + 1] = parsed.raw
  return argv
end

---Converts an admitted local file URI into a native document path.
---Only empty-or-localhost authorities are local; decoded bytes are re-checked
---for control characters so encoded NUL/DEL cannot smuggle through; Windows
---UNC reconstruction fails closed. Returns the path or nil plus reason.
function uri_policy.internal_path(uri, platform)
  local parsed, why = uri_policy.parse(uri)
  if not parsed then
    return nil, why
  end
  if not uri_policy.INTERNAL_SCHEMES[parsed.scheme] then
    return nil, "scheme_not_allowed"
  end

  local authority, path = parsed.target:match("^([^/]*)()")
  authority = authority:lower()
  if authority ~= "" and authority ~= "localhost" then
    return nil, "non_local_file_authority"
  end
  path = parsed.target:sub(path)

  if #path == 0 then
    return nil, "empty_file_path"
  end

  local decoded = percent_decode(path)
  if has_control_bytes(decoded) then
    return nil, "decoded_control_character"
  end

  if platform == "Windows" then
    local drive = decoded:match("^/([A-Za-z]:[/\\].*)$")
    if drive then
      local mapped = drive:gsub("/", "\\")
      if mapped:match("^%a:\\\\?$") then
        return nil, "empty_file_path"
      end
      return (mapped)
    end
    if decoded:match("^//") then
      return nil, "non_local_file_path"
    end
    if decoded == "/" then
      return nil, "empty_file_path"
    end
    return (decoded:gsub("/", "\\"))
  elseif platform == "Linux" or platform == "Mac OS X" then
    if decoded == "/" then
      return nil, "empty_file_path"
    end
    return decoded
  end
  return nil, "unsupported_platform"
end

---One-call seam consumed by the showDocument listener (#10873).
---params: { uri = string, external = boolean?, platform = string }
---Returns exactly one of:
---   { action = "open_internal",     path = string,  scheme = "file" }
---   { action = "confirm_external",  argv = table,   scheme = string,
---     display_uri = string }
---   { error = true, reason = bounded_token }
---The caller owns user consent, process.start(argv), and the single truthful
---ShowDocumentResult response.
function uri_policy.classify(params)
  if type(params) ~= "table" then
    return reject("invalid_params")
  end

  local external = params.external
  if external == nil then
    external = false
  end
  if type(external) ~= "boolean" then
    return reject("invalid_external_flag")
  end

  -- One validation pass up front: input-form failures precede policy and
  -- environment dispositions, and the exact raw target is available for the
  -- confirmation prompt of admitted externals.
  local parsed, why = uri_policy.parse(params.uri)
  if not parsed then
    return reject(why)
  end

  if external then
    local argv, ewhy = uri_policy.external_launch(params.uri, params.platform)
    if not argv then
      return reject(ewhy)
    end
    return {
      action = "confirm_external",
      argv = argv,
      scheme = parsed.scheme,
      display_uri = parsed.raw,
    }
  end

  local path, pwhy = uri_policy.internal_path(params.uri, params.platform)
  if not path then
    return reject(pwhy)
  end
  return {
    action = "open_internal",
    path = path,
    scheme = parsed.scheme,
  }
end

return uri_policy

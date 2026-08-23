-- Deterministic focused tests for clients/lite-xl/uri_policy.lua (#11162).
--
-- Run:
--   lua clients/lite-xl/tests/uri_policy_test.lua [path-to-uri-policy-module]
-- Default module path is ../uri_policy.lua relative to this file.
--
-- Proof shape: the staged module is a pure classification/planning seam. Tests
-- assert exact typed dispositions (admit/reject plus bounded reason tokens),
-- exact argv element boundaries for external launch on Linux, macOS and
-- Windows shapes, exact internal path conversion for admitted local file
-- URIs, and byte-exact preservation of untrusted target bytes. A fake shell
-- model proves metacharacters cannot alter the interpreted command when the
-- target stays one argv element.
--
-- Required targets (#11162):
--   https://example.test/a?x=1&y=2
--   https://example.test/"quoted"
--   https://example.test/a;b|c&d
--   file URI with spaces and Unicode
--   leading-dash path/URI component
--   percent-encoded delimiters
--   malformed percent escape
--   embedded NUL/control marker
--   unknown scheme
--   external=false + https URI
--   external=true + accepted URI
--
-- Mutation falsifiers (#11162 proof):
--   M1  Restore upstream shell construction: make external_launch return the
--       single string `launcher .. " " .. target` instead of an argv table.
--       The argv-boundary assertions below must fail, and feeding
--       https://example.test/a;b|c&d through the fake shell model must show
--       the metacharacters splitting the interpreted command.
--   M2  Drop the control-character scan in parse(). The embedded NUL/control
--       cases must fail.
--   M3  Drop the EXTERNAL_SCHEMES membership check in external_launch().
--       The unknown-scheme case must fail.
--   M4  Drop the post-decode control-character scan in internal_path().
--       The percent-encoded-NUL internal case must fail.
--
-- Run a mutation by copying ../uri_policy.lua, applying the described edit,
-- and passing its path as the first argument. Every mutation must produce
-- FAIL lines (red); the staged module must produce none.
--
-- No framework: plain asserts, one process, deterministic, exit code carries
-- the result. Compatible with the Lite XL Lua runtime family (5.4).

local module_path = arg and arg[1] or nil
if not module_path then
  local info = debug.getinfo(1, "S").source:sub(2)
  local dir = info:match("^(.*)[/\\]") or "."
  module_path = dir .. "/../uri_policy.lua"
end

local passed = 0
local failed = 0
local function ok(cond, label)
  if cond then
    passed = passed + 1
  else
    failed = failed + 1
    print("FAIL: " .. label)
  end
end

local function eq(a, b, label)
  if a == b then
    passed = passed + 1
  else
    failed = failed + 1
    print("FAIL: " .. label
      .. " (expected " .. tostring(a) .. ", got " .. tostring(b) .. ")")
  end
end

---Asserts argv is a table whose LAST element equals the raw target bytes
---exactly, and that the target occupies exactly one element.
local function assert_argv_target(argv, launcher_head, target, label)
  ok(type(argv) == "table", label .. ": argv is a table (not a shell string)")
  ok(#argv == #launcher_head + 1, label .. ": argv has launcher head plus one target element")
  for i = 1, #launcher_head do
    eq(argv[i], launcher_head[i], label .. ": argv[" .. i .. "] is launcher component")
  end
  eq(argv[#argv], target, label .. ": argv target element is byte-exact")
end

local policy = dofile(module_path)

local LINUX  = { "xdg-open" }
local MACOS  = { "open" }
local WINDOWS = { "rundll32", "url.dll,FileProtocolHandler" }

-- ---------------------------------------------------------------------------
-- External admission keeps untrusted bytes inert across platform shapes
-- ---------------------------------------------------------------------------

do
  -- Required target: query string with & and ? stays one element everywhere.
  local uri = "https://example.test/a?x=1&y=2"
  assert_argv_target(policy.external_launch(uri, "Linux"), LINUX, uri,
    "https query / Linux")
  assert_argv_target(policy.external_launch(uri, "Mac OS X"), MACOS, uri,
    "https query / macOS")
  assert_argv_target(policy.external_launch(uri, "Windows"), WINDOWS, uri,
    "https query / Windows")
end

do
  -- Required target: quoted metacharacters remain inside one inert element.
  local uri = 'https://example.test/"quoted"'
  assert_argv_target(policy.external_launch(uri, "Linux"), LINUX, uri,
    "quoted target / Linux")
  assert_argv_target(policy.external_launch(uri, "Windows"), WINDOWS, uri,
    "quoted target / Windows")
end

do
  -- Required target: shell metacharacters cannot split the argv element.
  local uri = "https://example.test/a;b|c&d"
  assert_argv_target(policy.external_launch(uri, "Linux"), LINUX, uri,
    "metacharacter target / Linux")
  assert_argv_target(policy.external_launch(uri, "Mac OS X"), MACOS, uri,
    "metacharacter target / macOS")

  -- Fake shell model: under upstream string construction the launcher and
  -- target become ONE interpreted string, so shell metacharacters inside the
  -- target would terminate/repipe commands. Under the argv boundary the
  -- program receives exactly two arguments and the bytes stay data.
  local concatenated = "xdg-open " .. uri
  local segments = {}
  for segment in concatenated:gmatch("[^;|&]+") do
    segments[#segments + 1] = segment
  end
  ok(#segments > 1,
    "fake shell model: concatenated form WOULD reinterpret metacharacters")
  local argv = policy.external_launch(uri, "Linux")
  ok(#argv == 2,
    "argv model: interpreted program receives exactly launcher plus one argument")
end

do
  -- Required target: leading dash stays data, never an option separator.
  local uri = "https://example.test/-rf-marker"
  assert_argv_target(policy.external_launch(uri, "Linux"), LINUX, uri,
    "leading-dash component / Linux")
end

do
  -- Scheme matching is case-insensitive but the raw URI is never rewritten.
  local decision = policy.classify({
    uri = "HTTPS://Example.Test/x",
    external = true,
    platform = "Linux"
  })
  eq(decision.action, "confirm_external", "uppercase scheme classified externally")
  eq(decision.argv[#decision.argv], "HTTPS://Example.Test/x",
    "raw uppercase URI preserved byte-exactly")
  eq(decision.scheme, "https", "scheme reported lowercased for policy comparison")
  eq(decision.argv[1], "xdg-open", "launcher resolved independently of URI casing")
end

do
  -- Required target: external=true plus accepted URI classifies for prompt.
  local decision = policy.classify({
    uri = "https://example.test/page",
    external = true,
    platform = "Linux"
  })
  eq(decision.action, "confirm_external", "external=true admitted URI asks user")
  ok(decision.error == nil, "no error disposition on admitted external")
  eq(type(decision.display_uri), "string", "prompt target provided as plain text")
  eq(decision.display_uri, "https://example.test/page", "display target byte-exact")
end

-- ---------------------------------------------------------------------------
-- Fail-closed scheme dispositions
-- ---------------------------------------------------------------------------

do
  -- Unknown/dangerous schemes are rejected before any launcher resolution.
  for _, uri in ipairs({
    "ftp://example.test/file",
    "x-perl-eval://run/code",
    "vbscript://run/thing",
  }) do
    local decision = policy.classify({ uri = uri, external = true, platform = "Linux" })
    eq(decision.action, nil, "unknown scheme '" .. uri .. "' has no action")
    eq(decision.error, true, "unknown scheme '" .. uri .. "' errors")
    eq(decision.reason, "scheme_not_allowed", "unknown scheme '" .. uri .. "' reason bounded")
    eq(decision.argv, nil, "unknown scheme '" .. uri .. "' carries no argv")
  end

  -- Direct seam contract: external_launch itself refuses non-allowlisted
  -- schemes even when the caller bypasses classify (#11162 fail-closed).
  local argv, why = policy.external_launch("ftp://example.test/file", "Linux")
  eq(argv, nil, "external_launch rejects ftp directly")
  eq(why, "scheme_not_allowed", "external_launch ftp reason bounded")

  -- Direct seam contract: internal_path itself refuses non-file schemes.
  local path, pwhy = policy.internal_path("https://example.test/doc.pl", "Linux")
  eq(path, nil, "internal_path rejects https directly")
  eq(pwhy, "scheme_not_allowed", "internal_path https reason bounded")
end

do
  -- Required target: external=false must NOT admit web schemes internally.
  local decision = policy.classify({
    uri = "https://example.test/doc.pl",
    external = false,
    platform = "Linux"
  })
  eq(decision.action, nil, "external=false rejects https")
  eq(decision.error, true, "external=false https errors")
  eq(decision.reason, "scheme_not_allowed", "external=false https reason bounded")
end

do
  -- Opaque or nested forms without a hierarchical scheme:// target fail
  -- closed instead of being guessed into a launcher or a path.
  for _, uri in ipairs({
    "javascript:alert(1)",
    "file:http://example.test/sneaky",
    "mailto:admin@example.test",
  }) do
    local decision = policy.classify({ uri = uri, external = true, platform = "Linux" })
    eq(decision.error, true, "opaque form '" .. uri .. "' fails closed")
    eq(decision.reason, "missing_scheme", "opaque form '" .. uri .. "' reason bounded")
  end
end

do
  -- Missing scheme is not silently guessed into a launcher or a path.
  local decision = policy.classify({
    uri = "www.example.com/open-this",
    external = true,
    platform = "Linux"
  })
  eq(decision.reason, "missing_scheme", "scheme-less target fails closed")
end

-- ---------------------------------------------------------------------------
-- Malformed percent encoding and control characters fail closed
-- ---------------------------------------------------------------------------

do
  for _, case in ipairs({
    { uri = "https://example.test/%zz", label = "%zz" },
    { uri = "https://example.test/%1", label = "truncated %1" },
    { uri = "https://example.test/trailing%", label = "trailing %" },
  }) do
    local decision = policy.classify({
      uri = case.uri, external = true, platform = "Linux"
    })
    eq(decision.error, true, "malformed escape " .. case.label .. " errors")
    eq(decision.reason, "malformed_percent_encoding",
      "malformed escape " .. case.label .. " reason bounded")
  end
end

do
  -- Embedded raw NUL and other control bytes are rejected pre-display.
  for _, case in ipairs({
    { uri = "https://example.test/a\0b", label = "NUL" },
    { uri = "https://example.test/a\nb", label = "LF" },
    { uri = "https://example.test/a\tb", label = "TAB" },
    { uri = "https://example.test/a\7b", label = "BEL" },
    { uri = "https://example.test/a\127b", label = "DEL" },
  }) do
    local decision = policy.classify({
      uri = case.uri, external = true, platform = "Linux"
    })
    eq(decision.error, true, "control character " .. case.label .. " errors")
    eq(decision.reason, "control_character",
      "control character " .. case.label .. " reason bounded")
    eq(decision.display_uri, nil, "control character " .. case.label .. " never reaches prompt")
  end
end

-- ---------------------------------------------------------------------------
-- Input validation
-- ---------------------------------------------------------------------------

do
  for _, case in ipairs({
    { params = {}, label = "missing uri" },
    { params = { uri = nil }, label = "nil uri" },
    { params = { uri = "" }, label = "empty uri" },
    { params = { uri = 42 }, label = "numeric uri" },
    { params = { uri = "https://example.test", external = "yes", platform = "Linux" },
      label = "non-boolean external" },
    { params = { uri = "https://example.test", external = 1, platform = "Linux" },
      label = "numeric external" },
    { params = { uri = "https://example.test/a b", external = true, platform = "Plan9" },
      label = "unsupported platform" },
  }) do
    local params = case.params
    local decision = policy.classify(params)
    eq(decision.error, true, case.label .. " fails closed")
    eq(decision.action, nil, case.label .. " has no action")
    ok(type(decision.reason) == "string" and #decision.reason < 64,
      case.label .. " carries bounded reason token")
  end
end

do
  -- LSP default: absent external flag means internal reveal.
  local decision = policy.classify({
    uri = "file:///tmp/view-me.pl",
    platform = "Linux"
  })
  eq(decision.action, "open_internal", "absent external defaults to internal reveal")
  eq(decision.path, "/tmp/view-me.pl", "posix file path converted exactly")
end

-- ---------------------------------------------------------------------------
-- Internal reveal admits only implemented local file URIs
-- ---------------------------------------------------------------------------

do
  -- Required target: spaces and Unicode survive admission byte-exactly.
  local decision = policy.classify({
    uri = "file:///home/dev/my project/héllo wörld.pl",
    external = false,
    platform = "Linux"
  })
  eq(decision.action, "open_internal", "spaces+Unicode file URI admitted internally")
  eq(decision.path, "/home/dev/my project/héllo wörld.pl",
    "internal path preserves spaces and Unicode")
  eq(decision.scheme, "file", "internal decision reports scheme")

  -- Same class handed to the external allowlist deliberately handles file.
  local ext = policy.classify({
    uri = "file:///home/dev/report.pdf",
    external = true,
    platform = "Linux"
  })
  eq(ext.action, "confirm_external", "file scheme admitted externally by review")
  assert_argv_target(ext.argv, LINUX, "file:///home/dev/report.pdf", "external file / Linux")
end

do
  -- Windows drive-letter file URIs convert to native separators.
  local decision = policy.classify({
    uri = "file:///C:/code/demo.pl",
    external = false,
    platform = "Windows"
  })
  eq(decision.action, "open_internal", "windows drive URI admitted")
  eq(decision.path, "C:\\code\\demo.pl", "windows path mapped to backslashes")

  -- Decoded UNC reconstruction fails closed.
  local unc = policy.classify({
    uri = "file:///%2F%2Fserver/share/x.pl",
    external = false,
    platform = "Windows"
  })
  eq(unc.error, true, "decoded UNC path rejected on Windows")
  eq(unc.reason, "non_local_file_path", "decoded UNC reason bounded")
end

do
  -- Non-local file authorities are not local documents.
  for _, uri in ipairs({
    "file://server/share/doc.pl",
    "file://evil.example.test/C:/boot.ini",
  }) do
    local decision = policy.classify({
      uri = uri, external = false, platform = "Linux"
    })
    eq(decision.error, true, "remote file authority '" .. uri .. "' rejected")
    eq(decision.reason, "non_local_file_authority",
      "remote file authority reason bounded")
  end

  -- RFC-permitted localhost authority stays local.
  local decision = policy.classify({
    uri = "file://localhost/tmp/local.pl",
    external = false,
    platform = "Linux"
  })
  eq(decision.action, "open_internal", "localhost authority admitted")
  eq(decision.path, "/tmp/local.pl", "localhost path converted")
end

do
  -- Required targets: percent-encoded delimiters decode only AFTER admission,
  -- and decoded control bytes still fail closed.
  local delim = policy.classify({
    uri = "file:///tmp/a%20name%26more/x.pl",
    external = false,
    platform = "Linux"
  })
  eq(delim.action, "open_internal", "percent-delimited name admitted")
  eq(delim.path, "/tmp/a name&more/x.pl", "encoded delimiters decoded once, exactly")

  local encoded_nul = policy.classify({
    uri = "file:///tmp/a%00b.pl",
    external = false,
    platform = "Linux"
  })
  eq(encoded_nul.error, true, "percent-encoded NUL rejected after decode")
  eq(encoded_nul.reason, "decoded_control_character", "decoded NUL reason bounded")

  local encoded_del = policy.classify({
    uri = "file:///tmp/a%7Fb.pl",
    external = false,
    platform = "Linux"
  })
  eq(encoded_del.error, true, "percent-encoded DEL rejected after decode")
end

do
  -- Empty file paths are not documents.
  local decision = policy.classify({
    uri = "file:///", external = false, platform = "Linux"
  })
  eq(decision.error, true, "rootless empty file path rejected")
  eq(decision.reason, "empty_file_path", "empty path reason bounded")
end

-- ---------------------------------------------------------------------------
-- Static source contract: zero shell-execution surface
-- ---------------------------------------------------------------------------

do
  local f = io.open(module_path, "rb")
  local source = f:read("*a")
  f:close()
  for _, forbidden in ipairs({ "system%.exec", "os%.execute", "io%.popen" }) do
    ok(source:find(forbidden) == nil,
      "module source free of " .. forbidden)
  end
end

-- ---------------------------------------------------------------------------
-- Determinism
-- ---------------------------------------------------------------------------

do
  local function run()
    local d = policy.classify({
      uri = "https://example.test/a?x=1&y=2",
      external = true,
      platform = "Linux"
    })
    return d.action == "confirm_external" and d.argv[2] == "https://example.test/a?x=1&y=2"
  end
  ok(run(), "deterministic run A")
  ok(run(), "deterministic run B (identical result)")
end

print(string.format("%d passed, %d failed", passed, failed))
os.exit(failed == 0 and 0 or 1)

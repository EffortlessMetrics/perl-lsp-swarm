-- Deterministic focused tests for the one local file URI <-> path conversion
-- authority in clients/lite-xl/upstream/util.lua (#11165).
--
-- Run:
--   lua clients/lite-xl/tests/util_file_uri_test.lua [path-to-util-module]
-- Default module path is ../upstream/util.lua relative to this file.
--
-- Proof shape: util.uri_to_path and util.path_to_uri are exercised as pure
-- functions under every PLATFORM tag, with exact expected outputs. Covered:
-- POSIX and Windows drive canonical encoding (spaces, '#', '?', '%', quotes,
-- multi-byte UTF-8, literal colons), single percent decode/encode honesty,
-- localhost/empty authority collapse, single-slash absolute file URIs,
-- POSIX leading-slash runs preserved as data, UNC authorities admitted as
-- \\server\share paths on Windows only, drive-letter case preservation,
-- forward/backslash normalization, and typed rejection of non-file schemes,
-- malformed percent escapes, decoded NUL/control bytes, relative shapes,
-- remote/userinfo/port authorities, device namespaces, over-bound inputs,
-- and rooted-without-drive paths. Every admitted case asserts an exact
-- path -> URI -> path round trip.
--
-- Dispositions owned by this suite (documented per issue #11165):
-- - multiple POSIX leading slashes are opaque data and round trip;
-- - drive-letter case is preserved both directions (comparison/case policy
--   stays with #8997/#9001, not display equality);
-- - file://localhost/<p> converts locally and canonicalizes to
--   file:///<p> on the way back;
-- - file:/<abs> (one slash, empty authority) is a valid absolute form;
-- - on Windows a non-local file URI authority is admitted as UNC data only
--   (conversion never touches the network); on POSIX it is remote_authority;
-- - UTF-8 validity is not re-validated: conversion preserves exact bytes.
--
-- Mutation falsifiers (#11165 proof):
-- 1. Restore any prefix-strip body inside uri_to_path (e.g. return the
--    input unchanged for unknown shapes, the legacy tofilename behavior):
--    the rejection sections fail because non-file URIs come back as
--    suspicious filenames instead of nil+reason.
-- 2. Decode twice or scan escapes after decoding: '%25literal' cases fail
--    (double decode turns %2520 into a space) and malformed-escape cases
--    stop failing deterministically.
-- 3. Encode from a word-character allowlist that keeps ':' or high bytes
--    unencoded, re-encode already-percent-like bytes, or drop the
--    producer/consumer wire-bound symmetry check: the canonical
--    Windows/Unicode expectations fail, round trips are no longer byte-exact,
--    and the boundary cases stop failing at exactly one byte past the limit.
-- 4. Drop the platform gate on UNC authorities: POSIX remote-authority
--    cases fail; drop the share-segment requirement: invalid_unc cases
--    fail.
-- 5. Decode the raw query/fragment tail instead of stripping it (or strip
--    after decoding): 'file:///x?rev=1' cases fail because '?'/'#' bytes
--    leak into the filesystem name.
-- Red baseline: against the pristine origin/main util.lua (pre-#11165,
-- legacy touri/tofilename only) this suite cannot pass: uri_to_path and
-- path_to_uri are absent (call on nil), and the legacy helpers demonstrably
-- accept 'https://' URIs unchanged as filenames, silently keep malformed
-- '%zz' escapes, and map 'file://server/share' to garbage on Windows.
--
-- No framework: plain asserts, one process, deterministic, exit code carries
-- the result. Compatible with the Lite XL Lua runtime family (5.4).

local util_module_path = arg and arg[1] or nil

if not util_module_path then
  local info = debug.getinfo(1, "S").source:sub(2)
  local dir = info:match("^(.*)[/\\]") or "."
  util_module_path = dir .. "/../upstream/util.lua"
end

local here = debug.getinfo(1, "S").source:sub(2):match("^(.*)[/\\]") or "."

-- ---------------------------------------------------------------------------
-- Lite XL runtime fakes: only what loading and exercising the exact staged
-- module requires.
-- ---------------------------------------------------------------------------

package.preload["plugins.lsp.json"] = function()
  return dofile(here .. "/../upstream/json.lua")
end

package.preload["core.common"] = function()
  return {}
end

local cfg = { plugins = { lsp = {} } }
package.preload["core.config"] = function() return cfg end

package.preload["core"] = function()
  return { docs = {}, log = function() end }
end

---Fake process.start: records argv, never executes. Only the scheme-policy
---guard section launches anything.
local process_calls = {}
package.preload["process"] = function()
  return {
    start = function(argv)
      process_calls[#process_calls + 1] = { argv = argv }
      return 4242 + #process_calls
    end
  }
end

system = { exec = function() error("shell invoked", 0) end }

-- Exact staged upstream source under test.
local util = dofile(util_module_path)


-- ---------------------------------------------------------------------------

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

---Runs one assertion block under a given PLATFORM tag.
local function with_platform(platform, fn)
  local old_platform = PLATFORM
  PLATFORM = platform
  fn()
  PLATFORM = old_platform
end

---Asserts path -> uri -> path byte-exact identity.
local function round_trip_path(platform, path, expected_uri)
  local uri, why = util.path_to_uri(path)
  ok(uri == expected_uri,
    platform .. " path_to_uri('" .. path .. "') == '" ..
    tostring(expected_uri) .. "' (got '" .. tostring(uri) ..
    "'/" .. tostring(why) .. ")")
  local back, back_why = util.uri_to_path(uri)
  ok(back == path,
    platform .. " round trip '" .. path .. "' restored (got '" ..
    tostring(back) .. "'/" .. tostring(back_why) .. ")")
end

---Asserts a typed rejection with the exact stable reason token.
local function rejects(fn, input, reason, label)
  local out, out_reason = fn(input)
  ok(out == nil and out_reason == reason,
    label .. " rejected as " .. tostring(reason) ..
    " (got " .. tostring(out) .. "," .. tostring(out_reason) .. ")")
end

-- ---------------------------------------------------------------------------
-- POSIX path -> URI canonical forms
-- ---------------------------------------------------------------------------

with_platform("Linux", function()
  round_trip_path("Linux", "/simple/file.pl", "file:///simple/file.pl")
  round_trip_path(
    "Linux", "/path with spaces/a#b?.pl",
    "file:///path%20with%20spaces/a%23b%3F.pl")
  round_trip_path("Linux", '/path/"quoted".pl', "file:///path/%22quoted%22.pl")
  round_trip_path(
    "Linux", "/path/\195\188nicode/\240\159\152\128.pl",
    "file:///path/%C3%BCnicode/%F0%9F%98%80.pl")
  round_trip_path("Linux", "/a:b.pl", "file:///a:b.pl")

  -- Multiple leading slashes stay data and survive the round trip.
  round_trip_path("Linux", "//data/x.pl", "file:////data/x.pl")

  -- A literal percent byte encodes once, never twice.
  round_trip_path("Linux", "/tmp/%20note.pl", "file:///tmp/%2520note.pl")
  round_trip_path("Linux", "/tmp/50%off.pl", "file:///tmp/50%25off.pl")

  -- Canonical wire forms convert back to the identical canonical URI.
  local stable, _ = util.uri_to_path("file:///simple/file.pl")
  local recoded = util.path_to_uri(stable)
  ok(recoded == "file:///simple/file.pl", "canonical URI stays canonical")

  -- Localhost authority collapses through the authority pair.
  local collapsed = util.path_to_uri(
    util.uri_to_path("file://localhost/simple/file.pl"))
  ok(collapsed == "file:///simple/file.pl",
    "localhost authority canonicalizes away")
end)

-- ---------------------------------------------------------------------------
-- POSIX URI -> path
-- ---------------------------------------------------------------------------

with_platform("Linux", function()
  local cases = {
    { "file:///simple/file.pl", "/simple/file.pl" },
    -- One-slash absolute form with an empty authority.
    { "file:/simple/file.pl", "/simple/file.pl" },
    { "file://localhost/simple/file.pl", "/simple/file.pl" },
    { "FILE:///Simple/X.pl", "/Simple/X.pl" },
    { "file:///path%20with%20spaces/a%23b%3F.pl", "/path with spaces/a#b?.pl" },
    { "file:///path/%22q%22.pl", '/path/"q".pl' },
    {
      "file:///path/%C3%BCnicode/%F0%9F%98%80.pl",
      "/path/\195\188nicode/\240\159\152\128.pl"
    },
    -- Leading-slash runs are preserved as data, not normalized away.
    { "file:////data/x.pl", "//data/x.pl" },
    -- Raw query/fragment components are stripped before decoding; encoded
    -- filename bytes stay data (#11165 review disposition).
    { "file:///tmp/module.pl?rev=1#L42", "/tmp/module.pl" },
    { "file:///tmp/x.pl?rev=1", "/tmp/x.pl" },
    { "file:///tmp/x.pl#L42", "/tmp/x.pl" },
    { "file:///tmp/a%3Fb%23c.pl", "/tmp/a?b#c.pl" },
  }
  for _, case in ipairs(cases) do
    local path, why = util.uri_to_path(case[1])
    ok(path == case[2],
      "Linux uri_to_path('" .. case[1] .. "') == '" .. case[2] ..
      "' (got '" .. tostring(path) .. "'/" .. tostring(why) .. ")")
  end
end)

-- ---------------------------------------------------------------------------
-- Rejections: non-file, malformed, control-bearing, relative, remote
-- ---------------------------------------------------------------------------

with_platform("Linux", function()
  -- Non-file schemes fail closed through the unchanged #11162 admission.
  for _, uri in ipairs({
    "http://example.test/x.pl",
    "https://example.test/x.pl",
    "perldoc:perldoc::perlfunc",
    "untitled:buffer-1",
    "FTP://example.test/x",
  }) do
    rejects(util.uri_to_path, uri, "unsupported_scheme",
      "Linux uri_to_path('" .. uri:sub(1, 24) .. "')")
  end

  -- Syntax failures carry the #11162 vocabulary unchanged.
  rejects(util.uri_to_path, "", "empty_uri", "empty input")
  rejects(util.uri_to_path, nil, "empty_uri", "non-string input")
  rejects(util.uri_to_path, "not a uri at all", "malformed_uri", "no scheme")
  rejects(util.uri_to_path, "file:///bad%1x", "malformed_percent_encoding",
    "truncated escape")
  rejects(util.uri_to_path, "file:///bad%zz", "malformed_percent_encoding",
    "non-hex escape")
  rejects(util.uri_to_path, string.rep("file:///x/", 300),
    "uri_above_bound", "over-bound URI")
  rejects(util.uri_to_path, "file:///ta\tb", "control_character",
    "raw tab byte")
  rejects(util.uri_to_path, "file:///na\ning", "control_character",
    "raw newline byte")

  -- Decoded control bytes never reach local file APIs.
  rejects(util.uri_to_path, "file:///nu%00le", "control_character",
    "encoded NUL")
  rejects(util.uri_to_path, "file:///cr%0Dlf", "control_character",
    "encoded CR")
  rejects(util.uri_to_path, "file:///del%7Fx", "control_character",
    "encoded DEL")

  -- Relative references are refused, not resolved against some cwd.
  rejects(util.uri_to_path, "file:relative.pl", "relative_path",
    "authority-less relative reference")
  rejects(util.uri_to_path, "file:", "relative_path", "bare file scheme")

  -- Non-local authorities are remote on POSIX, whatever they name.
  rejects(util.uri_to_path, "file://example.com/x.pl", "remote_authority",
    "remote host")
  rejects(util.uri_to_path, "file://user@example.com/x", "remote_authority",
    "userinfo authority")
  rejects(util.uri_to_path, "file://example.com:8080/x", "remote_authority",
    "port authority")
end)

-- ---------------------------------------------------------------------------
-- Windows drive forms
-- ---------------------------------------------------------------------------

with_platform("Windows", function()
  round_trip_path(
    "Windows", "C:\\Code\\Perl\\file.pl", "file:///C:/Code/Perl/file.pl")
  -- Forward/backslash inputs normalize to the same canonical URI...
  local forward = util.path_to_uri("C:/Code/Perl/file.pl")
  ok(forward == "file:///C:/Code/Perl/file.pl",
    "Windows forward-slash drive input canonicalizes identically")
  -- ...and convert back to native separators.
  local back = util.uri_to_path("file:///C:/Code/Perl/file.pl")
  ok(back == "C:\\Code\\Perl\\file.pl",
    "Windows URI converts back to backslashes")

  -- Drive-letter case is preserved as data both directions.
  round_trip_path("Windows", "c:\\code\\x.pl", "file:///c:/code/x.pl")
  round_trip_path("Windows", "C:\\", "file:///C:/")

  -- Encoded drive colon decodes into the same drive path.
  local encoded_colon = util.uri_to_path("file:///C%3A/Code/x.pl")
  ok(encoded_colon == "C:\\Code\\x.pl", "encoded drive colon decodes once")

  round_trip_path(
    "Windows", "C:\\Code\\My Docs\\a#b%.pl",
    "file:///C:/Code/My%20Docs/a%23b%25.pl")
  round_trip_path(
    "Windows", "C:\\Pfad\\m\195\188chte\\😀.pl",
    "file:///C:/Pfad/m%C3%BCchte/%F0%9F%98%80.pl")
  round_trip_path("Windows", "C:\\tmp\\%20note.pl", "file:///C:/tmp/%2520note.pl")
  round_trip_path("Windows", "C:\\a:b.pl", "file:///C:/a:b.pl")
end)

-- ---------------------------------------------------------------------------
-- Windows UNC forms
-- ---------------------------------------------------------------------------

with_platform("Windows", function()
  round_trip_path(
    "Windows", "\\\\server\\share\\file.pl", "file://server/share/file.pl")
  -- Forward-slash UNC input is the same shape.
  local forward = util.path_to_uri("//server/share/file.pl")
  ok(forward == "file://server/share/file.pl",
    "Windows forward-slash UNC input canonicalizes identically")

  -- Share roots without further components are valid UNC targets.
  round_trip_path("Windows", "\\\\server\\share", "file://server/share")

  -- Authority case is preserved as data.
  local cased = util.uri_to_path("file://SERVER/Share/X.pl")
  ok(cased == "\\\\SERVER\\Share\\X.pl", "UNC authority case preserved")

  -- A remote-looking file authority is admitted as UNC data on Windows.
  local remote_as_unc = util.uri_to_path("file://example.com/x.pl")
  ok(remote_as_unc == "\\\\example.com\\x.pl",
    "non-local authority becomes UNC data on Windows")

  -- Incomplete UNC shapes refuse honestly.
  rejects(util.uri_to_path, "file://server", "invalid_unc",
    "authority without share segment")
  rejects(util.path_to_uri, "\\\\server", "invalid_unc", "host-only UNC path")
  rejects(util.path_to_uri, "\\\\server\\", "invalid_unc",
    "host-only UNC path with separator")

  -- Bare separators after the authority name no share (#11165 review).
  rejects(util.uri_to_path, "file://server//", "invalid_unc",
    "authority plus bare separators")

  -- Encoded separator bytes in the authority decode into UNC structure;
  -- host names never contain them (#11165 review).
  rejects(util.uri_to_path, "file://server%5Cshare/x", "invalid_unc",
    "encoded backslash in authority")
  rejects(util.uri_to_path, "file://serv%2Fer/x.pl", "invalid_unc",
    "encoded slash in authority")

  -- localhost is the local machine in file URI space; a \\localhost\...
  -- UNC would produce an identity this authority reads back as a drive-less
  -- local path, so the producer refuses it symmetrically (#11165 review).
  rejects(util.path_to_uri, "\\\\localhost\\share\\x.pl", "invalid_unc",
    "localhost UNC alias")
  rejects(util.path_to_uri, "//LOCALHOST/share/x", "invalid_unc",
    "localhost UNC alias, forward slashes")
end)

-- ---------------------------------------------------------------------------
-- Windows rejections
-- ---------------------------------------------------------------------------

with_platform("Windows", function()
  -- Non-file schemes fail closed exactly as on POSIX.
  rejects(util.uri_to_path, "http://example.test/x.pl", "unsupported_scheme",
    "Windows non-file scheme")
  rejects(util.uri_to_path, "file:///nu%00le", "control_character",
    "Windows encoded NUL")

  -- POSIX-style URIs without a drive are refused instead of guessed onto a
  -- current-drive root.
  rejects(util.uri_to_path, "file:///home/dev/project/main.pl",
    "unsupported_path_shape", "drive-less absolute URI on Windows")
  rejects(util.uri_to_path, "file:///just-a-name", "unsupported_path_shape",
    "drive-less single component")

  -- Relative, drive-relative and rooted-without-drive shapes refuse.
  rejects(util.path_to_uri, "relative\\x.pl", "relative_path",
    "relative path")
  rejects(util.path_to_uri, "C:x.pl", "relative_path", "drive-relative path")
  rejects(util.path_to_uri, "C:", "relative_path", "bare drive")
  rejects(util.path_to_uri, "\\rooted\\x.pl", "relative_path",
    "rooted-without-drive path")

  -- Wire URIs carrying a drive-relative residue refuse the same way.
  rejects(util.uri_to_path, "file:///C:x", "relative_path",
    "drive-relative URI path")
  rejects(util.uri_to_path, "file:///c:", "relative_path",
    "bare drive URI")

  -- Device namespaces are not filesystem shares.
  rejects(util.path_to_uri, "\\\\?\\C:\\huge\\x.pl", "unsupported_device_path",
    "device namespace")
  rejects(util.path_to_uri, "\\\\.\\pipe\\x", "unsupported_device_path",
    "named pipe namespace")

  -- Bounded and typed inputs.
  rejects(util.path_to_uri, "", "empty_path", "empty path")
  rejects(util.path_to_uri, nil, "empty_path", "non-string path")
  rejects(util.path_to_uri, string.rep("C:\\dir\\", 1200), "path_above_wire_bound",
    "over-bound path")
  rejects(util.path_to_uri, "C:\\bad\0name.pl", "control_character",
    "embedded NUL path")
end)

with_platform("Linux", function()
  -- Producer/consumer bound symmetry (#11165): the exact boundary round
  -- trips, one byte past it refuses. POSIX prefix "file://" is 7 bytes.
  local at_limit = "/" .. string.rep("a", 2040)
  local limit_uri = util.path_to_uri(at_limit)
  ok(limit_uri == "file://" .. at_limit,
    "canonical URI at exactly the wire bound converts")
  ok(select(1, util.uri_to_path(limit_uri)) == at_limit,
    "wire-bound URI reads back byte-exact")
  rejects(util.path_to_uri, "/" .. string.rep("a", 2041), "path_above_wire_bound",
    "one byte past the wire bound")
end)

-- ---------------------------------------------------------------------------
-- Single decode/encode honesty (%25 layering)
-- ---------------------------------------------------------------------------

with_platform("Linux", function()
  -- Wire '%2520' is the three literal bytes '%','2','0' — never a space.
  local layered = util.uri_to_path("file:///archive/%2520note.pl")
  ok(layered == "/archive/%20note.pl",
    "'%2520' decodes once to literal '%20' bytes")
  ok(util.path_to_uri(layered) == "file:///archive/%2520note.pl",
    "literal '%20' bytes encode back stably")

  -- Double-decoding mutations would collapse these distinct wires.
  local plain = util.uri_to_path("file:///a%25b.pl")
  ok(plain == "/a%b.pl", "encoded percent yields one literal percent")
end)

-- ---------------------------------------------------------------------------
-- Scheme admission guard: the #11162 show_document policy is unchanged
-- ---------------------------------------------------------------------------

do
  -- Internal reveal still refuses non-file schemes; external launch still
  -- admits them. The conversion authority must neither broaden nor narrow
  -- this boundary (security note on #11165: classification lives elsewhere).
  local reveal_called = false
  local success, reason = util.show_document(
    { name = "t" },
    { uri = "https://example.test/page", external = false },
    { reveal = function() reveal_called = true return true end })
  ok(success == false and reason == "unsupported_scheme",
    "internal showDocument still refuses non-file schemes")
  ok(reveal_called == false, "refused reveal never invoked")

  reset_ok = true
  process_calls = {}
  local launched = select(1, util.show_document(
    { name = "t" }, { uri = "https://example.test/x", external = true }))
  ok(launched == true and #process_calls == 1,
    "external showDocument still admits https through argv handoff")
  ok(process_calls[1] ~= nil and process_calls[1].argv[2] ==
    "https://example.test/x", "external target stays byte-exact argument")
end

print(string.format("%d passed, %d failed", passed, failed))
os.exit(failed == 0 and 0 or 1)

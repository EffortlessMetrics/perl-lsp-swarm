-- Deterministic focused tests for clients/lite-xl/upstream/server.lua inbound
-- framing (#11151).
--
-- Run:
--   lua clients/lite-xl/tests/server_frame_test.lua [path-to-server-module] [path-to-json-module]
-- Default module path is ../upstream/server.lua relative to this file; the
-- staged json.lua codec is loaded from the default relative location unless
-- an alternate path is given as the second argument.
--
-- Proof shape: a fake LSP server process feeds stdout chunks through
-- Server:read_responses (timeout 0, so no wall clock is touched) and tests
-- assert exact responses, remainder ownership, typed framing-failure class,
-- bounded diagnostics, callback/current-result safety and cleanup
-- disposition. The real staged json.lua decodes every valid body, so the
-- framing/codec seam is exercised end to end.
--
-- Mutation falsifier (#11151 proof): run this file against the pristine
-- upstream server.lua @ d1432ae0736cd9531798b4bc1221835f534cc689 instead of
-- the patched module. The missing-header, signed-length, overflow,
-- duplicate-length, budget, empty-body and truncation-class cases must FAIL
-- there: pristine treats `content_length = 0` as present (`if not
-- content_length` cannot detect it), accepts any first Content-Length match,
-- embeds accumulated bytes in error strings and collapses malformed frames
-- into later JSON-decode errors - proving these tests discriminate the typed
-- bounded parser rather than passing vacuously.
--
-- Targeted single-behavior mutations of the patched module must also fail:
--   1. delete the `declared ~= nil` duplicate check -> both conflicting-
--      length cases fail;
--   2. delete the `declared > self.max_body_bytes` check -> body_above_limit
--      fails;
--   3. restore truthiness-only missing detection (drop the explicit
--      `declared == nil` branch) -> missing_content_length fails.
--
-- No framework: plain asserts, one process, deterministic, exit code carries
-- the result. Compatible with the Lite XL Lua runtime family (5.4).

local server_module_path = arg and arg[1] or nil
local json_module_path = arg and arg[2] or nil

if not server_module_path then
  local info = debug.getinfo(1, "S").source:sub(2)
  local dir = info:match("^(.*)[/\\]") or "."
  server_module_path = dir .. "/../upstream/server.lua"
end

if not json_module_path then
  local info = debug.getinfo(1, "S").source:sub(2)
  local dir = info:match("^(.*)[/\\]") or "."
  json_module_path = dir .. "/../upstream/json.lua"
end

local here = debug.getinfo(1, "S").source:sub(2):match("^(.*)[/\\]") or "."

-- ---------------------------------------------------------------------------
-- Lite XL runtime fakes: only what loading and exercising the exact staged
-- server.lua requires.
-- ---------------------------------------------------------------------------

package.preload["plugins.lsp.json"] = function()
  return dofile(json_module_path)
end

package.preload["plugins.lsp.util"] = function()
  return {
    split = function(text, delimiter)
      local result = {}
      local pattern = "(.-)" .. delimiter
      local last_end = 1
      local s, e, cap = text:find(pattern, 1)
      while s do
        if s ~= 1 or cap ~= "" then table.insert(result, cap) end
        last_end = e + 1
        s, e, cap = text:find(pattern, last_end)
      end
      if #text >= last_end then table.insert(result, text:sub(last_end)) end
      return result
    end,
    jsonprettify = function(code) return code end,
    touri = function(path) return path end,
    tofilename = function(uri) return uri end
  }
end

-- Local patch (#11172): the staged modules fold their capability
-- advertisement and command projection through the exact manifest source.
package.preload["plugins.lsp.capability_manifest"] = function()
  return dofile(here .. "/../upstream/capability_manifest.lua")
end
package.preload["plugins.lsp.diagnostics"] = function()
  return {}
end

package.preload["core.object"] = function()
  -- Minimal core.object surface used by server.lua at load time.
  local Object = {}
  Object.__index = Object
  function Object:extend()
    local cls = setmetatable({}, self)
    cls.__index = cls
    cls.super = self
    return cls
  end
  function Object:new() end
  return Object
end

system = {
  get_time = function() return 0 end
}

-- Exact staged upstream source under test (fakes above resolve its requires).
local server_module = dofile(server_module_path)

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

---Builds one fake process whose read_stdout returns the listed chunks in
---order. Once exhausted it keeps returning "" (alive, no data) unless
---opts.eof is set, in which case it returns nil (pipe closed / child death).
local function fake_process(chunks, opts)
  opts = opts or {}
  local index = 0
  local closed = false
  local proc
  proc = {
    running = function() return not closed end,
    read_stdout = function(_, amount)
      index = index + 1
      local chunk = chunks[index]
      if chunk == nil then
        if opts.eof then
          closed = true
          return nil
        end
        return ""
      end
      return chunk
    end,
    read_stderr = function() return "" end
  }
  return proc
end

---Bare lsp.server instance wired for deterministic read_responses driving.
local function new_test_server(chunks, opts)
  opts = opts or {}
  local server = setmetatable({
    name = "test-perllsp",
    verbose = opts.verbose or false,
    fatal_error = false,
    write_fails = opts.write_fails or 0,
    write_fails_before_shutdown = 60,
    max_body_bytes = opts.max_body_bytes,
    max_header_bytes = opts.max_header_bytes,
    proc = fake_process(chunks, { eof = opts.eof })
  }, { __index = server_module })
  server.log = function(self, message, ...)
    self.log_lines = self.log_lines or {}
    self.log_lines[#self.log_lines + 1] = string.format(message, ...)
  end
  server.shutdown_if_needed = function(self)
    self.shutdown_calls = (self.shutdown_calls or 0) + 1
  end
  return server
end

local function frame(body)
  return string.format("Content-Length: %d\r\n\r\n%s", #body, body)
end

local function frame_with_extra_header(extra_line, body)
  return string.format(
    "Content-Length: %d\r\n%s\r\n\r\n%s",
    #body, extra_line, body
  )
end

local function pump(server)
  return server:read_responses(0)
end

---Models the editor event loop pumping the transport: up to max_turns
---read_responses calls, stopping at the first delivered responses table or a
---fatal session. Returns that table, or the last false.
local function deliver(server, max_turns)
  max_turns = max_turns or 8
  local last = nil
  for _ = 1, max_turns do
    last = pump(server)
    if type(last) == "table" or server.fatal_error then
      break
    end
  end
  return last
end

---Asserts one terminal framing failure with exact class, fatal disposition
---and bounded content-free diagnostics.
local function assert_framing_failure(server, result, reason, label, forbidden_marker)
  ok(result == false, label .. ": read_responses returned false")
  ok(server.fatal_error == true, label .. ": session marked fatal")
  ok((server.shutdown_calls or 0) >= 1, label .. ": shutdown routed once")
  local joined = table.concat(server.log_lines or {}, "\n")
  ok(joined:find("Inbound framing failure:", 1, true) ~= nil,
    label .. ": log carries framing-failure classification")
  ok(joined:find("reason=" .. reason, 1, true) ~= nil,
    label .. ": log carries reason=" .. reason)
  ok(#joined < 400, label .. ": diagnostic stays bounded (" .. #joined .. " chars)")
  if forbidden_marker then
    ok(joined:find(forbidden_marker, 1, true) == nil,
      label .. ": diagnostic excludes stream content marker")
  end
end

-- ---------------------------------------------------------------------------
-- Valid framing preserves exact bytes and order across arbitrary boundaries
-- ---------------------------------------------------------------------------

do
  local body = '{"jsonrpc":"2.0","id":7,"method":"initialize"}'
  local server = new_test_server({ frame(body) })
  local responses = pump(server)
  ok(type(responses) == "table" and #responses == 1, "single whole-chunk frame delivered")
  ok(type(responses[1]) == "table" and responses[1].id == 7,
    "frame decoded through the real staged codec")
  ok(server.fatal_error == false, "valid frame keeps session healthy")

  local again = pump(server)
  ok(again == false, "idle alive process yields no further response")
  ok(server.fatal_error == false, "idle wait does not poison the session")
end

do
  -- Header split at EVERY byte boundary of one small frame; the event loop
  -- pumps turns until the reassembled frame is delivered.
  local wire = frame('{"id":1}')
  local all_splits_ok = true
  for split = 0, #wire - 1 do
    local server = new_test_server({ wire:sub(1, split), wire:sub(split + 1) })
    local responses = deliver(server)
    if type(responses) ~= "table" or #responses ~= 1 or responses[1].id ~= 1 then
      all_splits_ok = false
      print("  header-split failed at byte " .. split)
    end
  end
  ok(all_splits_ok, "header split at every byte boundary delivers exactly one frame")
end

do
  -- Body splits including inside multi-byte UTF-8 sequences.
  local body = '{"note":"h\xc3\xa9llo \xe2\x86\x92 \xf0\x9f\x8c\xb2"}'
  local wire = frame(body)
  local splits = { 1, 20, math.floor(#wire / 2), #wire - 3, #wire - 1 }
  for _, split in ipairs(splits) do
    local server = new_test_server({ wire:sub(1, split), wire:sub(split + 1) })
    local responses = deliver(server)
    local got = type(responses) == "table" and responses[1] and responses[1].note
    ok(got == body:match('"note":"(.*)"'),
      "body split at byte " .. split .. " preserves bytes (UTF-8 safe)")
  end
end

do
  -- Multiple complete frames in one read keep order and bytes.
  local server = new_test_server({
    frame('{"seq":1}') .. frame('{"seq":2}') .. frame('{"seq":3}')
  })
  local responses = pump(server)
  ok(type(responses) == "table" and #responses == 3, "three frames in one read")
  ok(responses[1].seq == 1 and responses[2].seq == 2 and responses[3].seq == 3,
    "multi-frame order preserved")
  ok(pump(server) == false, "no residue after exact multiple frames")
  ok(server.fatal_error == false, "multi-frame read stays healthy")
end

do
  -- One complete frame plus a partial next frame; remainder ownership exact.
  local partial_tail = 'Content-Length: 5\r\n\r\nab'
  local server = new_test_server({ frame('{"a":true}') .. partial_tail })
  local responses = pump(server)
  ok(type(responses) == "table" and #responses == 1 and responses[1].a == true,
    "complete frame delivered before partial remainder")
  ok(server.fatal_error == false, "partial remainder does not fail the session")
end

do
  -- Remainder ownership: chunked delivery still emits every frame in order.
  local b1, b2, b3 = '{"n":1}', '{"n":22}', '{"n":333}'
  local wire = frame(b1) .. frame(b2) .. frame(b3)
  local server = new_test_server({ wire:sub(1, 30), wire:sub(31, 60), wire:sub(61) })
  local responses = deliver(server)
  ok(type(responses) == "table" and responses[1].n == 1 and responses[2].n == 22
    and responses[3].n == 333,
    "remainder ownership delivers frames 1-3 byte-exactly in order")
  ok(server.fatal_error == false, "chunked multi-frame read stays healthy")
end

do
  -- Large valid body near the admitted limit (small reviewed test budget).
  -- Bytes stay printable JSON-string-safe (no quote, backslash or control
  -- byte) so the body decodes through the real staged codec.
  local limit = 64 * 1024
  local alphabet = {}
  for b = 35, 91 do alphabet[#alphabet + 1] = string.char(b) end
  for b = 93, 126 do alphabet[#alphabet + 1] = string.char(b) end
  local parts = {}
  for i = 0, limit - 6 do
    parts[#parts + 1] = alphabet[i % #alphabet + 1]
  end
  local big_body = '"' .. table.concat(parts) .. '"'
  local server = new_test_server({ frame(big_body) }, { max_body_bytes = limit })
  local responses = pump(server)
  ok(type(responses) == "table" and type(responses[1]) == "string"
    and responses[1] == table.concat(parts),
    "large near-limit body round-trips byte-exactly")
  ok(server.fatal_error == false, "near-limit body keeps session healthy")
end

do
  -- Optional bounded headers are tolerated alongside Content-Length.
  local server = new_test_server({
    frame_with_extra_header("Content-Type: application/vscode-jsonrpc; charset=utf-8", '{"ok":true}')
  })
  local responses = pump(server)
  ok(type(responses) == "table" and responses[1].ok == true,
    "optional Content-Type header accepted")
  local unknown = new_test_server({
    frame_with_extra_header("X-Unknown-Header: value", '{"ok":2}')
  })
  local responses2 = pump(unknown)
  ok(type(responses2) == "table" and responses2[1].ok == 2,
    "unknown bounded header ignored without misparsing length")
end

do
  -- Successful reads reset write_fails exactly like upstream behavior.
  local server = new_test_server({ frame('{"reset":true}') }, { write_fails = 7 })
  pump(server)
  ok(server.write_fails == 0, "write_fails reset after successful delivery")
end

-- ---------------------------------------------------------------------------
-- Invalid framing fails closed with distinct typed classes
-- ---------------------------------------------------------------------------

do
  -- Missing Content-Length: pristine upstream starts content_length at 0 and
  -- `if not content_length` cannot detect it, so the empty body masquerades
  -- as a message. Patched must classify precisely.
  local server = new_test_server({
    "Content-Type: application/json\r\n\r\n{\"orphan\":true}"
  }, { eof = true })
  local result = pump(server)
  assert_framing_failure(server, result, "missing_content_length", "missing header",
    "{\"orphan\":true}")
end

do
  local server = new_test_server({
    "Content-Length: 12x3\r\n\r\nabcdefghijkl"
  }, { eof = true })
  assert_framing_failure(server, deliver(server), "malformed_content_length",
    "non-decimal length")
end

do
  local server = new_test_server({
    "Content-Length: -5\r\n\r\nabcde"
  }, { eof = true })
  assert_framing_failure(server, deliver(server), "signed_content_length",
    "signed negative length")

  local server2 = new_test_server({
    "Content-Length: +5\r\n\r\nabcde"
  }, { eof = true })
  assert_framing_failure(server2, deliver(server2), "signed_content_length",
    "signed positive length")
end

do
  local server = new_test_server({
    "Content-Length: 99999999999999999999999999\r\n\r\n"
  }, { eof = true })
  assert_framing_failure(server, deliver(server), "content_length_overflow",
    "overflowing length")
end

do
  local server = new_test_server({
    "Content-Length: 5\r\nContent-Length: 6\r\n\r\nabcde"
  }, { eof = true })
  assert_framing_failure(server, deliver(server), "conflicting_content_length",
    "conflicting duplicate lengths", "abcde")

  local identical = new_test_server({
    "Content-Length: 5\r\nContent-Length: 5\r\n\r\nabcde"
  }, { eof = true })
  assert_framing_failure(identical, deliver(identical), "conflicting_content_length",
    "identical duplicate lengths rejected fail-closed")
end

do
  -- Header delimiter beyond budget.
  local long_headers = string.rep("A", 64)
  local server = new_test_server({ long_headers },
    { max_header_bytes = 32, eof = true })
  assert_framing_failure(server, deliver(server), "header_budget_exceeded",
    "header delimiter beyond budget", long_headers)
end

do
  -- Declared body above limit fails before allocating/reading body bytes.
  local server = new_test_server({
    "Content-Length: 100\r\n\r\nshort"
  }, { max_body_bytes = 64, eof = true })
  local result = pump(server)
  assert_framing_failure(server, result, "body_above_limit", "declared body above limit")
  local joined = table.concat(server.log_lines or {}, "\n")
  ok(joined:find("declared=100", 1, true) ~= nil,
    "above-limit diagnostic carries declared count")
  ok(joined:find("short", 1, true) == nil,
    "above-limit diagnostic excludes body bytes")
end

do
  -- EOF halfway through a header.
  local server = new_test_server({ "Content-Length: 5\r\n" }, { eof = true })
  assert_framing_failure(server, deliver(server), "truncated_header",
    "EOF halfway through header")

  local mid_name = new_test_server({ "Conten" }, { eof = true })
  assert_framing_failure(mid_name, deliver(mid_name), "truncated_header",
    "EOF inside header name")
end

do
  -- EOF halfway through a declared body carries observed vs declared counts
  -- and never echoes accumulated body content.
  local canary = 'SECRET-SOURCE-MARKER'
  local server = new_test_server({
    "Content-Length: 100\r\n\r\n" .. canary
  }, { eof = true })
  local result = deliver(server)
  assert_framing_failure(server, result, "truncated_body", "EOF halfway through body",
    canary)
  local joined = table.concat(server.log_lines or {}, "\n")
  ok(joined:find("declared=100", 1, true) ~= nil,
    "truncation diagnostic carries declared count")
  ok(joined:find("observed_body=" .. #canary, 1, true) ~= nil,
    "truncation diagnostic carries observed count")
end

do
  -- Zero-length declared body has an explicit fail-closed disposition and
  -- cannot masquerade as a message nor reach JSON decoding.
  local server = new_test_server({ "Content-Length: 0\r\n\r\n" }, { eof = true })
  assert_framing_failure(server, deliver(server), "empty_body", "zero-length body")
end

do
  -- Body bytes are never interpreted as headers after a malformed header.
  local server = new_test_server({
    "Content-Length 5\r\n\r\nbody-bytes-here"
  }, { eof = true })
  assert_framing_failure(server, deliver(server), "missing_content_length",
    "malformed header line terminates parsing", "body-bytes-here")
end

-- ---------------------------------------------------------------------------
-- Failure classes stay distinct: framing vs JSON syntax
-- ---------------------------------------------------------------------------

do
  local server = new_test_server({ frame("this is not json {{") }, { eof = true })
  local result = pump(server)
  ok(result == false, "JSON syntax failure also terminates the turn")
  local joined = table.concat(server.log_lines or {}, "\n")
  ok(joined:find("JSON decode failure:", 1, true) ~= nil,
    "JSON syntax failure logged under its own class")
  ok(joined:find("Inbound framing failure:", 1, true) == nil,
    "JSON syntax failure is not reported as a framing failure")
end

-- ---------------------------------------------------------------------------
-- Determinism
-- ---------------------------------------------------------------------------

do
  local wire = frame('{"d":1}') .. frame('{"d":2}')
  local function run()
    local server = new_test_server({ wire:sub(1, 17), wire:sub(18) }, { eof = true })
    local responses = deliver(server)
    return (type(responses) == "table" and responses[1].d == 1
      and responses[2].d == 2 and server.fatal_error == false)
  end
  ok(run() == true, "deterministic run A")
  ok(run() == true, "deterministic run B (identical result)")
end

print(string.format("%d passed, %d failed", passed, failed))
os.exit(failed == 0 and 0 or 1)

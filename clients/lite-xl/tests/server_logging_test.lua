-- Deterministic focused tests for protocol logging redaction in
-- clients/lite-xl/upstream/server.lua and clients/lite-xl/upstream/util.lua
-- (#11155).
--
-- Run:
--   lua clients/lite-xl/tests/server_logging_test.lua [path-to-server-module] [path-to-util-module]
-- Default module paths are ../upstream/server.lua and ../upstream/util.lua
-- relative to this file. The staged json.lua codec is loaded from its default
-- relative location.
--
-- Proof shape: canary source/secret markers are planted in every channel
-- (outbound write failure, inbound JSON-decode failure, truncated inbound
-- frame, drained server stderr, disabled verbose, opt-in trace file). Default
-- and automatic failure logs must keep actionable identity (direction,
-- reason/class, byte counts, content digest, decode offsets, stderr
-- categories) while canary bytes stay out of every default log, stderr stays
-- bounded while fully drained, and the explicit local trace stays opt-in,
-- guarded against append failure, and stoppable immediately.
--
-- Mutation falsifiers (#11155 proof):
--   1. restore raw payload logging at any default failure site (echoing
--      `data` in the outbound-write or JSON-decode failure logs, or raw `%s`
--      stderr display) -> the matching canary-absence assertions FAIL;
--   2. force the util.lua trace guard off (unguarded `log:write` on a failed
--      `io.open`) -> the append-failure case raises an incidental Lua
--      exception instead of the guarded one-shot warning;
--   3. run against the pristine upstream util.lua @ d1432ae (blob
--      588c101aa97ef0d112926aac316e7a95a52a6994): same incidental exception.
--
-- No framework: plain asserts, one process, deterministic, exit code carries
-- the result. Compatible with the Lite XL Lua runtime family (5.4).

local server_module_path = arg and arg[1] or nil
local util_module_path = arg and arg[2] or nil

if not server_module_path then
  local info = debug.getinfo(1, "S").source:sub(2)
  local dir = info:match("^(.*)[/\\]") or "."
  server_module_path = dir .. "/../upstream/server.lua"
end

if not util_module_path then
  local info = debug.getinfo(1, "S").source:sub(2)
  local dir = info:match("^(.*)[/\\]") or "."
  util_module_path = dir .. "/../upstream/util.lua"
end

local here = debug.getinfo(1, "S").source:sub(2):match("^(.*)[/\\]") or "."

local SOURCE_CANARY = "PERLLSP-CANARY-SOURCE-FNORD"
local SECRET_CANARY = "PERLLSP-CANARY-SECRET-TOKEN42"

-- ---------------------------------------------------------------------------
-- Lite XL runtime fakes: only what loading and exercising the exact staged
-- modules requires.
-- ---------------------------------------------------------------------------

package.preload["plugins.lsp.json"] = function()
  return dofile(here .. "/../upstream/json.lua")
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

package.preload["core.common"] = function()
  return {}
end

---util.lua requires the Lite XL process module for #11162 argv launches;
---these logging tests never launch anything, so record and refuse.
package.preload["process"] = function()
  return {
    start = function(_, argv)
      error("unexpected process start in logging test", 0)
    end
  }
end

---Shared editor config table; tests mutate cfg.plugins.lsp per scenario.
local cfg = { plugins = { lsp = {} } }
package.preload["core.config"] = function()
  return cfg
end

---Records editor console output (core.log surface) for assertions.
local console_log = {}
package.preload["core"] = function()
  local core = {}
  core.log = function(message)
    console_log[#console_log + 1] = tostring(message)
  end
  core.docs = {}
  return core
end

system = {
  get_time = function() return 0 end
}

-- Exact staged modules under test (preloads above resolve their requires).
package.preload["plugins.lsp.util"] = function()
  return dofile(util_module_path)
end
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

local function reset_config()
  for k in pairs(cfg.plugins.lsp) do cfg.plugins.lsp[k] = nil end
  console_log = {}
end

---Builds one fake process: scripted stdout chunks (EOF when opts.eof and the
---script is exhausted), scripted stderr chunks ("" forever after), scripted
---write results: nil entry => full success, number => partial count, string
--=> failure with that error message.
local function fake_process(opts)
  opts = opts or {}
  local stdout, stderr, writes = opts.stdout or {}, opts.stderr or {}, opts.writes or {}
  local s_idx, e_idx, w_idx = 0, 0, 0
  local closed = false
  local proc
  proc = {
    running = function() return not closed end,
    read_stdout = function()
      s_idx = s_idx + 1
      local chunk = stdout[s_idx]
      if chunk == nil then
        if opts.eof then
          closed = true
          return nil
        end
        return ""
      end
      return chunk
    end,
    read_stderr = function()
      e_idx = e_idx + 1
      local chunk = stderr[e_idx]
      if chunk == nil then return "" end
      return chunk
    end,
    stderr_reads = function() return e_idx end,
    write = function(_, data)
      w_idx = w_idx + 1
      local result = writes[w_idx]
      if result == nil then return #data end
      if type(result) == "number" then return result end
      return false, result
    end
  }
  return proc
end

---Bare lsp.server instance wired for deterministic driving.
local function new_test_server(proc, opts)
  opts = opts or {}
  local server = setmetatable({
    name = "test-perllsp",
    verbose = opts.verbose or false,
    fatal_error = false,
    write_fails = 0,
    write_fails_before_shutdown = 60,
    proc = proc
  }, { __index = server_module })
  server.log_lines = {}
  server.log = function(self, message, ...)
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

local function joined(lines)
  return table.concat(lines, "\n")
end

-- ---------------------------------------------------------------------------
-- Outbound write failures keep identity and never echo the frame
-- ---------------------------------------------------------------------------

do
  reset_config()
  local data = frame('{"method":"textDocument/didOpen","text":"' .. SOURCE_CANARY .. '"}')
  local secret_frame = frame('{"params":{"token":"' .. SECRET_CANARY .. '"}}')

  -- Total write failure.
  local server = new_test_server(fake_process({ writes = { "pipe broken" } }))
  local sent, errmsg = server:send_data(data)
  ok(sent == false, "total outbound failure reports unsent")
  ok(errmsg == "pipe broken", "outbound failure keeps the error message")
  local out = joined(server.log_lines)
  ok(out:find("Outbound write failure:", 1, true) ~= nil,
    "outbound failure logged under its class")
  ok(out:find("bytes=" .. #data, 1, true) ~= nil,
    "outbound failure carries exact byte count")
  ok(out:find("digest=%x%x%x%x%x%x%x%x") ~= nil,
    "outbound failure carries a content digest")
  ok(out:find("error=pipe broken", 1, true) ~= nil,
    "outbound failure carries the transport error")
  ok(out:find(SOURCE_CANARY, 1, true) == nil,
    "outbound failure log excludes frame source canary")
  ok(out:find(SECRET_CANARY, 1, true) == nil,
    "outbound failure log excludes frame secret canary")
  ok(#server.log_lines[1] < 300, "outbound failure diagnostic stays bounded")

  -- Partial write then failure still accounts the full intended frame.
  local server2 = new_test_server(fake_process({ writes = { 10, "pipe broken" } }))
  local sent2 = server2:send_data(data)
  ok(sent2 == false, "partial outbound failure reports unsent")
  local out2 = joined(server2.log_lines)
  ok(out2:find("bytes=" .. #data, 1, true) ~= nil,
    "partial outbound failure carries intended byte count")
  ok(out2:find(SOURCE_CANARY, 1, true) == nil,
    "partial outbound failure log excludes frame content")

  -- Digest is deterministic per content and distinct across content.
  local sa = new_test_server(fake_process({ writes = { "x" } }))
  sa:send_data(data)
  local line_a = joined(sa.log_lines)
  local s3 = new_test_server(fake_process({ writes = { "x" } }))
  s3:send_data(data)
  local line_b = joined(s3.log_lines)
  local digest_a = line_a:match("digest=(%x%x%x%x%x%x%x%x)")
  local digest_b = line_b:match("digest=(%x%x%x%x%x%x%x%x)")
  ok(digest_a ~= nil and digest_a == digest_b,
    "identical payloads produce an identical stable digest")

  local s4 = new_test_server(fake_process({ writes = { "x" } }))
  s4:send_data(secret_frame)
  local line_c = joined(s4.log_lines)
  local digest_c = line_c:match("digest=(%x%x%x%x%x%x%x%x)")
  ok(digest_c ~= nil and digest_c ~= digest_a,
    "different payloads produce different digests")
end

-- ---------------------------------------------------------------------------
-- Inbound JSON-decode failures carry reason/offset/bytes/digest, never body
-- ---------------------------------------------------------------------------

do
  reset_config()
  local bad_body = '{"result":{"preview":"' .. SOURCE_CANARY .. '"}}{{broken'
  local server = new_test_server(
    fake_process({ stdout = { frame(bad_body) }, eof = true }))
  local result = server:read_responses(0)
  ok(result == false, "JSON-decode failure terminates the turn")
  local out = joined(server.log_lines)
  ok(out:find("JSON decode failure:", 1, true) ~= nil,
    "decode failure logged under its class")
  ok(out:find("reason=", 1, true) ~= nil, "decode failure carries codec reason")
  ok(out:find("offset=", 1, true) ~= nil, "decode failure carries decode offset")
  ok(out:find("bytes=" .. #bad_body, 1, true) ~= nil,
    "decode failure carries body byte count")
  ok(out:find("digest=%x%x%x%x%x%x%x%x") ~= nil,
    "decode failure carries content digest")
  ok(out:find(SOURCE_CANARY, 1, true) == nil,
    "decode failure log excludes body source canary")
  ok(#out < 400, "decode failure diagnostic stays bounded")
  ok(server.fatal_error == false, "decode failure keeps upstream session semantics")
end

-- ---------------------------------------------------------------------------
-- Truncated inbound frame logs stay content-free (framing channel contract)
-- ---------------------------------------------------------------------------

do
  reset_config()
  local server = new_test_server(fake_process({
    stdout = { "Content-Length: 100\r\n\r\n" .. SOURCE_CANARY },
    eof = true
  }))
  -- Two event-loop turns: chunk absorbed, then EOF surfaces the truncation.
  local result = server:read_responses(0)
  result = server:read_responses(0)
  ok(result == false, "truncated frame terminates the turn")
  local out = joined(server.log_lines)
  ok(out:find("reason=truncated_body", 1, true) ~= nil,
    "truncation diagnostic carries its class")
  ok(out:find(SOURCE_CANARY, 1, true) == nil,
    "truncation diagnostic excludes accumulated body canary")
end

-- ---------------------------------------------------------------------------
-- Server stderr drains fully but retains and displays bounded excerpts
-- ---------------------------------------------------------------------------

do
  reset_config()
  -- Build ~24 KiB of filler so the canaries sit beyond the retention cap.
  -- The actionable category marker leads the stream so a bounded display
  -- excerpt keeps it; the secret sits past the retention bound entirely.
  local unit = string.rep("a", 64) .. "\r\n"
  local early_stream = "EARLY-STDERR-MARKER\r\n" .. string.rep(unit, 30)
  local late_fill = string.rep(unit, 300)            -- ~19 KiB past the cap
  local stderr_stream = {
    early_stream,
    late_fill .. SECRET_CANARY .. "\r\n",
    "tail-after-canary\r\n"
  }
  local proc = fake_process({ stderr = stderr_stream })
  local server = new_test_server(proc)

  local errors = server:process_errors(true)
  ok(type(errors) == "string" and #errors > 0, "stderr drained and returned")
  ok(errors:find("EARLY-STDERR-MARKER", 1, true) ~= nil,
    "retained stderr keeps its leading actionable excerpt")
  ok(errors:find(SECRET_CANARY, 1, true) == nil,
    "retained stderr drops everything past the retention bound")
  ok(errors:find("...[truncated]", 1, true) ~= nil,
    "retained stderr marks truncation visibly")
  ok(proc:stderr_reads() >= #stderr_stream,
    "stderr kept draining to completion past the retention bound")

  local out = joined(server.log_lines)
  ok(out:find("Server stderr:", 1, true) ~= nil,
    "stderr diagnostic logged under its category")
  ok(out:find("EARLY-STDERR-MARKER", 1, true) ~= nil,
    "stderr diagnostic keeps a bounded leading excerpt")
  ok(out:find(SECRET_CANARY, 1, true) == nil,
    "stderr diagnostic excludes canary beyond the display bound")
  ok(#server.log_lines[1] < 600, "stderr diagnostic stays bounded")
end

-- ---------------------------------------------------------------------------
-- Verbose tracing is opt-in only; defaults emit no payload logs
-- ---------------------------------------------------------------------------

do
  reset_config()
  local canary_params = { text = SOURCE_CANARY, token = SECRET_CANARY }
  local server = new_test_server(fake_process({}), { verbose = false })
  server:notify("workspace/didChangeConfiguration", canary_params)
  local out = joined(server.log_lines)
  ok(out:find(SOURCE_CANARY, 1, true) == nil,
    "default (non-verbose) sends emit no payload log")
  ok(out:find(SECRET_CANARY, 1, true) == nil,
    "default (non-verbose) sends leak no secret")
end

-- ---------------------------------------------------------------------------
-- Explicit trace file: opt-in, sensitive, guarded, immediately stoppable
-- ---------------------------------------------------------------------------

do
  reset_config()
  local util = require("plugins.lsp.util")
  local trace_path = os.tmpname() .. ".litexl-trace.log"
  cfg.plugins.lsp.log_file = trace_path
  cfg.plugins.lsp.prettify_json = false

  local payload = '{"method":"textDocument/didOpen","text":"' .. SOURCE_CANARY .. '"}'
  local returned = util.jsonprettify(payload)
  ok(returned == payload, "jsonprettify returns the payload unchanged")

  local fh = io.open(trace_path, "rb")
  local traced = fh and fh:read("*a") or ""
  if fh then fh:close() end
  ok(traced:find(SOURCE_CANARY, 1, true) ~= nil,
    "opt-in trace file receives the full local payload")

  -- Disabling stops sensitive writes immediately.
  cfg.plugins.lsp.log_file = ""
  local size_before = #traced
  util.jsonprettify(payload)
  fh = io.open(trace_path, "rb")
  local after_disable = fh and fh:read("*a") or ""
  if fh then fh:close() end
  ok(#after_disable == size_before,
    "disabling the trace stops further sensitive writes immediately")
  os.remove(trace_path)
  reset_config()
end

do
  reset_config()
  local util = require("plugins.lsp.util")
  -- A directory path makes io.open fail on this host family.
  cfg.plugins.lsp.log_file = "."
  cfg.plugins.lsp.prettify_json = false
  local ran, result = pcall(util.jsonprettify, '{"safe":"payload"}')
  ok(ran, "trace append failure does not raise (" .. tostring(result) .. ")")
  ok(result == '{"safe":"payload"}', "failed trace append still returns the payload")
  ok(#console_log >= 1 and joined(console_log):lower():find("trace", 1, true) ~= nil,
    "append failure warns once through the editor log")
  local warnings_after_first = #console_log
  util.jsonprettify('{"second":"call"}')
  ok(#console_log <= warnings_after_first + 1,
    "repeat append failures do not recurse or spam unbounded warnings")
  reset_config()
end

print(string.format("%d passed, %d failed", passed, failed))
os.exit(failed == 0 and 0 or 1)

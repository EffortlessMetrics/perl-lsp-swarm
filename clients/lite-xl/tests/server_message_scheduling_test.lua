-- Deterministic focused tests for message admission/scheduling backpressure
-- in clients/lite-xl/upstream/server.lua (#10833).
--
-- Run:
--   lua clients/lite-xl/tests/server_message_scheduling_test.lua [path-to-server-module]
-- Default module path is ../upstream/server.lua relative to this file.
--
-- Proof shape: a bare lsp.server instance wired to the EXACT staged server
-- module (real push_notification/push_request/push_response plus the real
-- process_* send loops), a fake epoch replacing os.time, and a recording
-- write_request standing in for the transport. Tests assert the #10833
-- disposition contract:
--
--   no enqueue path ever silently drops load-bearing traffic - saturated
--   rate state must not lose didChange batches, watched-file events,
--   provider requests, or client responses owed to the server;
--   overwrite-capable calls coalesce onto their unsent same-method entry
--   ("coalesced") keeping the latest state;
--   lifecycle notifications keep exact open/change/close ordering under a
--   saturated scheduler;
--   every push returns an explicit typed disposition ("queued",
--   "coalesced", "not_queued"); rejections invoke the optional
--   not_queued_callback with a stable reason and allocate no phantom id;
--   the request queue is bounded with explicit rejection instead of silent
--   loss or unbounded growth;
--   already-sent request ids are never retransmitted inside their timeout
--   window (#10657 owns correlation).
--
-- Single-send extension (#10657): every queued operation is emitted at most
-- once per server generation; expiry is one terminal typed "timeout"
-- disposition that removes the correlation exactly once; late responses are
-- stale (no callback, nothing to pop); deliberate retries become new ids;
-- transport write failure keeps the operation unsent with identity intact;
-- initialize exceeds a longer policy terminally without ever re-emitting
-- id 1; an injectable monotonic clock (`server.now`) makes backward jumps
-- inert and forward boundary crossings exactly-once; replacement instances
-- restart the id space so old-generation responses correlate nowhere.
-- Red-first: run this suite against CURRENT MAIN before the #10657 patch -
-- the pristine resend loop produces duplicate id frames on expiry, exempts
-- requests by id VALUE (any first operation, not just initialize, resends
-- forever), runs late-response callbacks after expiry, cannot expire at an
-- exact monotonic boundary, and paces transport retries strictly later, so
-- the held-hover/#178, late-response, retry-identity, initialize,
-- exact-boundary, and clock-jump cases MUST fail there (the partial-write
-- recovery pin passes both sides). Mutation falsifier of the PATCHED module
-- (verified): restore resend-on-expiry in process_requests (re-encode and
-- rewrite the frame, remove after the second send) and the duplicate-frame
-- cases fail again.
--
-- Red-first baseline: run this suite against CURRENT MAIN before the #10833
-- patch. There the hit-rate admission dropper silently discards saturated
-- didChange/watched-files/hover/response pushes and push_* returns nil, so
-- the no-silent-drop and typed-disposition cases MUST fail there. The two
-- whitelist pins (didOpen and completionItem/resolve bypassing the old
-- dropper) pass on both sides as contract pins.
--
-- Mutation falsifiers of the PATCHED module (each verified to be caught):
--   1. restore a silent admission early-return for watched-file pushes at
--      the top of push_notification -> both watcher-burst cases fail;
--   2. drop the max_queued_requests check in push_request -> the bound
--      rejection cases fail (the breaching request becomes queued);
--   3. reset request.timestamp after send -> the single-send and one-hover
--      pins fail (ids retransmit).
--
-- No framework: plain soft asserts, one process, deterministic (fake clock,
-- no sleeps), exit code carries the result. Compatible with the Lite XL Lua
-- runtime family (Lua 5.4).

local server_module_path = arg and arg[1] or nil

if not server_module_path then
  local info = debug.getinfo(1, "S").source:sub(2)
  local dir = info:match("^(.*)[/\\]") or "."
  server_module_path = dir .. "/../upstream/server.lua"
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
-- server.lua requires (same shape as server_frame_test.lua).
-- ---------------------------------------------------------------------------

package.preload["plugins.lsp.json"] = function()
  return dofile(here .. "/../upstream/json.lua")
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
    intable = function(value, table_of_values)
      for _, v in ipairs(table_of_values) do
        if v == value then return true end
      end
      return false
    end,
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

system = { get_time = function() return 0 end }

-- Fake epoch: every os.time call inside the staged module observes this.
local fake_epoch = 1000
local original_os_time = os.time
os.time = function() return fake_epoch end

local server_module = dofile(server_module_path)

-- ---------------------------------------------------------------------------

---Bare lsp.server instance wired for deterministic queue/send driving: real
---staged methods, recording transport, no real process.
local function fresh_server(opts)
  opts = opts or {}
  local server = setmetatable({
    name = "test-perllsp",
    verbose = false,
    initialized = opts.initialized ~= false,
    fatal_error = false,
    write_fails = 0,
    write_fails_before_shutdown = 60,
    max_queued_requests = opts.max_queued_requests or server_module.MAX_QUEUED_REQUESTS,
    -- Legacy admission fields: present so the PRISTINE pre-#10833 module
    -- (whose dropper consults them) runs this same suite cleanly red-first.
    -- The patched module ignores them.
    hitrate_list = {},
    requests_per_second = 32,
    request_list = {},
    response_list = {},
    notification_list = {},
    raw_list = {},
    request_listeners = {},
    current_request = 0,
    proc = { running = function() return true end },
  }, { __index = server_module })
  server.wire = {}
  server.write_request = function(self, data)
    self.wire[#self.wire + 1] = data
    return true
  end
  server.shutdown_calls = 0
  server.shutdown_if_needed = function(self)
    self.shutdown_calls = self.shutdown_calls + 1
  end
  return server
end

---Legacy saturation: preloads the old shared hit-rate counter so any
---admission check that still exists must reject the next push.
local function saturate_legacy_counter(server)
  server.hitrate_list = server.hitrate_list or {}
  server.hitrate_list["request"] = {
    count = 32,
    timestamp = fake_epoch + 1,
  }
end

local function wire_methods(server)
  local methods = {}
  local json = require "plugins.lsp.json"
  for _, frame_data in ipairs(server.wire) do
    local ok_decode, decoded = pcall(json.decode, frame_data)
    if ok_decode and decoded and decoded.method then
      methods[#methods + 1] = decoded.method
    elseif ok_decode and decoded and decoded.id then
      methods[#methods + 1] = "response:" .. tostring(decoded.id)
    else
      methods[#methods + 1] = "?"
    end
  end
  return methods
end

local json = require "plugins.lsp.json"

-- ---------------------------------------------------------------------------
-- No silent drops under a saturated legacy counter (#10833 core claim)
-- ---------------------------------------------------------------------------

do
  -- Saturated didChange batch is coalesced, never lost.
  local s = fresh_server()
  saturate_legacy_counter(s)
  local d1 = s:push_notification("textDocument/didChange", {
    overwrite = true,
    params = { textDocument = { uri = "u", version = 1 },
               contentChanges = { { text = "old" } } },
  })
  local d2 = s:push_notification("textDocument/didChange", {
    overwrite = true,
    params = { textDocument = { uri = "u", version = 2 },
               contentChanges = { { text = "new" } } },
  })
  ok(d1 == "queued" and d2 == "coalesced",
    "saturated didChange pushes return explicit queued/coalesced dispositions")
  ok(#s.notification_list == 1,
    "saturated didChange bursts collapse to one unsent batch instead of vanishing")
  local coalesced = s.notification_list[1]
  ok(coalesced and coalesced.params.textDocument.version == 2
    and coalesced.params.contentChanges[1].text == "new",
    "the coalesced batch carries the LATEST document state")
end

do
  -- Saturated watched-file events are never silently lost.
  local s = fresh_server()
  saturate_legacy_counter(s)
  local d = s:push_notification("workspace/didChangeWatchedFiles", {
    params = { changes = { { uri = "file:///a", type = 1 } } },
  })
  ok(d == "queued" and #s.notification_list == 1,
    "saturated watched-file event queues with an explicit disposition")
end

do
  -- Saturated provider request is queued with identity, not dropped.
  local s = fresh_server()
  saturate_legacy_counter(s)
  local got_reason, got_method
  local d = s:push_request("textDocument/hover", {
    params = { textDocument = { uri = "u" }, position = { line = 0, character = 1 } },
    not_queued_callback = function(reason, method)
      got_reason, got_method = reason, method
    end,
  })
  ok(d == "queued" and #s.request_list == 1 and s.request_list[1].id == 1,
    "saturated provider request queues with its exact operation id")
  ok(got_reason == nil and got_method == nil,
    "a queued request invokes no not_queued callback")
end

do
  -- A client response owed to the server is always admitted (#10785
  -- obligation; the old dropper could strand the server waiting).
  local s = fresh_server()
  saturate_legacy_counter(s)
  local d = s:push_response("window/showDocument", 7, { success = false })
  ok(d == "queued" and #s.response_list == 1
    and s.response_list[1].result.success == false,
    "saturated client response still queues")
end

-- Contract pins: the legacy whitelisted classes stay admitted (these passed
-- before the patch too and must survive it).
do
  local s = fresh_server()
  saturate_legacy_counter(s)
  ok(s:push_notification("textDocument/didOpen",
      { params = { textDocument = { uri = "u" } } }) == "queued"
    and #s.notification_list == 1,
    "pin: lifecycle didOpen stays admitted")
  local r = s:push_request("completionItem/resolve", { params = { label = "x" } })
  ok(r == "queued" and #s.request_list == 1,
    "pin: completionItem/resolve stays admitted")
end

-- ---------------------------------------------------------------------------
-- Ordering, pacing and single-send semantics
-- ---------------------------------------------------------------------------

do
  -- Lifecycle ordering stays exact under a full queue: one send per tick,
  -- open/change/close in order.
  local s = fresh_server({ max_queued_requests = 64 })
  s:push_notification("textDocument/didOpen",
    { params = { textDocument = { uri = "u", languageId = "perl", version = 1, text = "x" } } })
  s:push_notification("textDocument/didChange",
    { overwrite = true, params = { textDocument = { uri = "u", version = 2 }, contentChanges = {} } })
  s:push_notification("textDocument/didClose",
    { params = { textDocument = { uri = "u" } } })
  s.initialized = true
  s:process_notifications()
  s:process_notifications()
  s:process_notifications()
  local order = wire_methods(s)
  ok(order[1] == "textDocument/didOpen"
    and order[2] == "textDocument/didChange"
    and order[3] == "textDocument/didClose",
    "lifecycle notifications deliver in exact open/change/close order")
  ok(#s.notification_list == 0, "drained notifications leave the queue")
end

do
  -- Watcher burst keeps terminal state: every event survives, in order.
  local s = fresh_server()
  s:push_notification("workspace/didChangeWatchedFiles", {
    params = { changes = { { uri = "file:///a.pl", type = 1 } } } })
  s:push_notification("workspace/didChangeWatchedFiles", {
    params = { changes = { { uri = "file:///a.pl", type = 2 } } } })
  s:push_notification("workspace/didChangeWatchedFiles", {
    params = { changes = { { uri = "file:///a.pl", type = 3 } } } })
  s:process_notifications()
  s:process_notifications()
  s:process_notifications()
  local decoded_first = s.wire[1] and json.decode(s.wire[1])
  local decoded_last = s.wire[#s.wire] and json.decode(s.wire[#s.wire])
  ok(#s.wire == 3, "all three watcher events reached the wire")
  ok(decoded_first and decoded_first.params.changes[1].type == 1
    and decoded_last and decoded_last.params.changes[1].type == 3,
    "watcher burst preserves create-through-delete event order")
end

do
  -- Overwritten hover A->B: one queued operation carrying B, superseding A.
  local s = fresh_server()
  local da = s:push_request("textDocument/hover",
    { overwrite = true, params = { id = "A" } })
  local db = s:push_request("textDocument/hover",
    { overwrite = true, params = { id = "B" } })
  ok(da == "queued" and db == "coalesced",
    "hover overwrite reports queued then coalesced dispositions")
  ok(#s.request_list == 1 and s.request_list[1].params.id == "B",
    "only the newer hover state stays queued")
  s:process_requests()
  s:process_requests()
  local hovers = {}
  for _, frame_data in ipairs(s.wire) do
    local decoded = json.decode(frame_data)
    if decoded.method == "textDocument/hover" then
      hovers[#hovers + 1] = decoded
    end
  end
  ok(#hovers == 1 and hovers[1].params.id == "B" and hovers[1].id == 1,
    "exactly one hover reaches the wire carrying the newest params and one stable id")
end

do
  -- Single-send: an already-sent id is not retransmitted inside its timeout
  -- window even across further ticks (#10657 correlation pin).
  local s = fresh_server()
  s:push_request("textDocument/documentSymbol", { params = { query = "q" } })
  s:process_requests()
  fake_epoch = fake_epoch + 0
  s:process_requests()
  local symbols = {}
  for _, frame_data in ipairs(s.wire) do
    local decoded = json.decode(frame_data)
    if decoded.method == "textDocument/documentSymbol" then symbols[#symbols + 1] = decoded end
  end
  ok(#symbols == 1, "sent request id stays single-send within the timeout window")
end

do
  -- Responses owed to the server flush fully on one tick.
  local s = fresh_server()
  s:push_response("window/showDocument", 1, { success = true })
  s:push_response("window/showDocument", 2, { success = false })
  s:push_response("workspace/configuration", 3, nil,
    { code = -32602, message = "Invalid params" })
  s:process_client_responses()
  local kinds = wire_methods(s)
  ok(kinds[1] == "response:1" and kinds[2] == "response:2"
    and kinds[3] == "response:3" and #kinds == 3,
    "all queued client responses flush in one tick, results and errors alike")
  ok(#s.response_list == 0, "flushed responses clear the obligation queue")
end

-- ---------------------------------------------------------------------------
-- Explicit bounds and typed rejections (no silent loss, no phantom ids)
-- ---------------------------------------------------------------------------

do
  -- Bound breach rejects explicitly, invokes the callback, allocates no id.
  local s = fresh_server({ max_queued_requests = 3 })
  local reasons, methods = {}, {}
  local d1 = s:push_request("a", { params = {}, not_queued_callback = function(r, m)
    reasons[#reasons + 1] = r; methods[#methods + 1] = m end })
  local d2 = s:push_request("b", { params = {} })
  local d3 = s:push_request("c", { params = {} })
  local d4 = s:push_request("d", {
    params = {},
    not_queued_callback = function(r, m)
      reasons[#reasons + 1] = r; methods[#methods + 1] = m
    end,
  })
  ok(d1 == "queued" and d2 == "queued" and d3 == "queued",
    "requests up to the bound queue normally")
  ok(d4 == "not_queued",
    "the bound-breaching request gets an explicit not_queued disposition")
  ok(reasons[1] == "queue_full" and methods[1] == "d",
    "the rejection callback receives the stable reason and method")
  ok(s.current_request == 3 and #s.request_list == 3,
    "rejected requests allocate no phantom in-flight id")
  -- An overwrite-capable push still coalesces at the bound (no growth).
  local d5 = s:push_request("a", { overwrite = true, params = { v = 2 } })
  ok(d5 == "coalesced" and #s.request_list == 3
    and s.request_list[1].params.v == 2,
    "overwrite policy still coalesces when the queue is full")
end

do
  -- Bound-full overwrite onto an ALREADY-SENT request rejects the replacement
  -- without retiring the in-flight original (#10833/#12544 review repair):
  -- marking the sent request overwritten before the replacement is admitted
  -- would suppress its valid response callback, losing both outcomes.
  local s = fresh_server({ max_queued_requests = 2 })
  s:push_request("a", { params = { v = 1 } })
  s:push_request("b", { params = {} })
  s:process_requests()
  local reasons, methods = {}, {}
  local d = s:push_request("a", {
    overwrite = true,
    params = { v = 2 },
    not_queued_callback = function(r, m)
      reasons[#reasons + 1] = r; methods[#methods + 1] = m
    end,
  })
  ok(d == "not_queued",
    "bound-full overwrite onto a sent request rejects the replacement explicitly")
  ok(reasons[1] == "queue_full" and methods[1] == "a",
    "the rejected replacement reports queue_full with its method")
  local sent_a
  for _, request in ipairs(s.request_list) do
    if request.method == "a" then sent_a = request end
  end
  ok(sent_a and sent_a.times_sent == 1 and not sent_a.overwritten,
    "the rejected replacement leaves the in-flight original's response callback intact")
end

do
  -- Not-initialized requests reject explicitly instead of silently.
  local s = fresh_server({ initialized = false })
  local got
  local d = s:push_request("textDocument/hover", {
    params = {},
    not_queued_callback = function(reason, method) got = reason .. ":" .. method end,
  })
  ok(d == "not_queued" and got == "not_initialized:textDocument/hover",
    "uninitialized servers reject requests with a typed disposition")
  ok(#s.request_list == 0 and s.current_request == 0,
    "no request state leaks from an uninitialized rejection")
  -- The initialize request itself still bypasses the gate.
  local di = s:push_request("initialize", { params = {} })
  ok(di == "queued" and #s.request_list == 1,
    "pin: the initialize request still queues before initialization")
end

-- ---------------------------------------------------------------------------
-- Single-send request identity and terminal timeouts (#10657)
-- ---------------------------------------------------------------------------

---Counts wire frames carrying one JSON-RPC id.
local function frames_with_id(server, id)
  local hits = 0
  for _, frame_data in ipairs(server.wire) do
    local ok_decode, decoded = pcall(json.decode, frame_data)
    if ok_decode and decoded and decoded.id == id then
      hits = hits + 1
    end
  end
  return hits
end

do
  -- slow_response_beyond_timeout: exactly one frame with id=N, the timeout
  -- callback fires once with the typed disposition, and no second id=N
  -- frame ever appears (upstream #178 shape: a held hover cannot produce
  -- two identical requests).
  local s = fresh_server()
  local timeouts = {}
  s:push_request("textDocument/hover", {
    params = { textDocument = { uri = "u" }, position = { line = 0, character = 0 } },
    timeout = 2,
    timeout_callback = function(request, disposition)
      timeouts[#timeouts + 1] = { id = request.id, disposition = disposition }
    end,
  })
  s:process_requests()
  fake_epoch = fake_epoch + 3
  s:process_requests()
  s:process_requests()
  s:process_requests()
  ok(frames_with_id(s, 1) == 1,
    "a held hover emits exactly one frame with its id (#10657/#178)")
  ok(#s.request_list == 0,
    "an expired operation leaves no correlation behind")
  ok(#timeouts == 1 and timeouts[1].id == 1 and timeouts[1].disposition == "timeout",
    "expiry is one typed timeout disposition delivered exactly once")
end

do
  -- late_response_after_timeout_is_stale: a response arriving after the
  -- terminal timeout finds no correlation and cannot run callbacks or
  -- mutate state.
  local s = fresh_server()
  local callback_runs, timeout_runs = 0, 0
  s:push_request("textDocument/documentSymbol", {
    params = { query = "q" },
    timeout = 1,
    callback = function() callback_runs = callback_runs + 1 end,
    timeout_callback = function() timeout_runs = timeout_runs + 1 end,
  })
  s:process_requests()
  fake_epoch = fake_epoch + 2
  s:process_requests()
  ok(timeout_runs == 1, "the timeout fired before the late response")
  s:send_response_signal({ id = 1, result = { symbols = {} } })
  ok(callback_runs == 0,
    "a late response after timeout never runs the original callback")
  ok(s:pop_request(1) == nil,
    "a late response finds no correlation entry to pop")
end

do
  -- explicit_caller_retry_gets_new_id: a caller that deliberately retries
  -- after a timeout becomes a new operation with a new id (contract pin).
  local s = fresh_server()
  s:push_request("textDocument/hover", {
    params = { position = { line = 0, character = 0 } }, timeout = 1,
  })
  s:process_requests()
  fake_epoch = fake_epoch + 2
  s:process_requests()
  local retried = s:push_request("textDocument/hover", {
    params = { position = { line = 0, character = 1 } },
  })
  ok(retried == "queued" and #s.request_list == 1
    and s.request_list[1].id == 2,
    "a deliberate retry queues as a new operation with a new id")
  s:process_requests()
  ok(frames_with_id(s, 2) == 1,
    "the retry sends once under its own new id")
end

do
  -- partial_write_is_not_semantic_send: a transport write failure keeps the
  -- operation unsent with its identity intact; recovery on a later tick
  -- produces exactly one complete frame (pin: transport handling stays
  -- byte-complete through send_data without semantic replay).
  local s = fresh_server()
  local fail_first = { n = 0 }
  local real_writer = s.write_request
  s.write_request = function(self, data)
    fail_first.n = fail_first.n + 1
    if fail_first.n == 1 then return false end
    return real_writer(self, data)
  end
  s:push_request("textDocument/hover", {
    params = { position = { line = 0, character = 0 } },
  })
  s:process_requests()
  ok(#s.wire == 0 and #s.request_list == 1
    and s.request_list[1].times_sent == 0,
    "a failed transport write emits nothing and keeps the operation unsent")
  ok(s.shutdown_calls >= 1,
    "transport failure classification stays owned by the lifecycle path")
  fake_epoch = fake_epoch + 2
  s.process_requests(s)
  ok(frames_with_id(s, 1) == 1 and s.request_list[1].times_sent == 1,
    "recovered transport delivers the same operation exactly once")
end

do
  -- initialize_never_duplicated: a slow initialize exceeds even its longer
  -- policy terminally; id=1 is never transmitted twice (pristine resends
  -- it forever).
  local s = fresh_server({ initialized = false })
  local dispositions = {}
  s:push_request("initialize", {
    params = {},
    timeout_callback = function(_, disposition)
      dispositions[#dispositions + 1] = disposition
    end,
  })
  s:process_requests()
  fake_epoch = fake_epoch + 200
  s.initialized = false
  s.process_requests(s)
  s.process_requests(s)
  s.process_requests(s)
  ok(frames_with_id(s, 1) == 1,
    "initialize exceeds even its long policy with exactly one emission")
  ok(#s.request_list == 0 and dispositions[1] == "timeout",
    "initialize expiry is terminal with the same typed disposition")
end

do
  -- exact_boundary_terminal: at timestamp <= now the sent request expires
  -- exactly once - not early, not by resend (monotonic boundary semantics).
  local s = fresh_server()
  local timeout_count = 0
  s:push_request("textDocument/hover", {
    params = {}, timeout = 5,
    timeout_callback = function() timeout_count = timeout_count + 1 end,
  })
  s:process_requests()
  fake_epoch = fake_epoch + 4
  s:process_requests()
  ok(timeout_count == 0 and #s.request_list == 1,
    "inside the window the correlation stays alive untouched")
  fake_epoch = fake_epoch + 1
  s:process_requests()
  ok(timeout_count == 1 and #s.request_list == 0
    and frames_with_id(s, 1) == 1,
    "at the exact boundary the operation expires terminally once")
end

do
  -- wall_clock_jump_resistance: an injectable monotonic clock keeps a sent
  -- request inert across a backward jump and terminal exactly once after a
  -- forward jump past the deadline.
  local s = fresh_server()
  local mono = 5000
  s.now = function() return mono end
  local timeout_count = 0
  s:push_request("textDocument/hover", {
    params = {}, timeout = 10,
    timeout_callback = function() timeout_count = timeout_count + 1 end,
  })
  s:process_requests()
  mono = mono - 4000
  for _ = 1, 5 do s:process_requests() end
  ok(frames_with_id(s, 1) == 1 and timeout_count == 0
    and #s.request_list == 1,
    "a backward clock jump neither resends nor prematurely expires")
  mono = 5011
  s:process_requests()
  ok(frames_with_id(s, 1) == 1 and timeout_count == 1
    and #s.request_list == 0,
    "advancing to the deadline yields exactly one terminal timeout")
end

do
  -- generation_scoped_identity: a replacement server instance starts its
  -- own id space and old-generation responses cannot correlate into it.
  local s = fresh_server()
  s:push_request("textDocument/hover", { params = {} })
  s:process_requests()
  local replacement = fresh_server()
  ok(replacement.current_request == 0,
    "a new generation restarts the request-id space")
  local ran = false
  function replacement:send_response_signal(response)
    if self:pop_request(response.id) then ran = true end
  end
  replacement:send_response_signal({ id = 1, result = {} })
  ok(not ran,
    "an old-generation response id correlates into no replacement state")
end

print(string.format("%d passed, %d failed", passed, failed))
os.time = original_os_time
if failed > 0 then os.exit(1) end

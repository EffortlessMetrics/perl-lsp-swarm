-- Deterministic stateful journey tests built on the #11103 simulation
-- harness (clients/lite-xl/tests/harness.lua) for the staged
-- clients/lite-xl/upstream/init.lua document-session authority.
--
-- Run:
--   lua clients/lite-xl/tests/journey_session_test.lua [path-to-init-module]
-- Default module path is ../upstream/init.lua relative to this file. Point
-- the argument at a mutated copy to verify the falsifiers below.
--
-- Proof shape: ONE continuous multi-step client journey - interleaved
-- documents, per-batch version streams, a backpressure window, close of a
-- second document, full process replacement (stop + replacement instance),
-- and reopen into the replacement generation - asserted against the
-- COMPLETE ordered wire history retained across every server generation.
-- The focused suites isolate one transition per case with fresh module
-- loads and drop drained queues; only a journey can prove that ordering,
-- generation tagging, session identity, and editor bytes hold SIMULTANEOUSLY
-- across all transitions in one unbroken history. A second journey proves
-- mid-journey configuration change: the workspace-settings reader serves one
-- cached snapshot inside its TTL and re-reads changed server settings after
-- it, observed through the public path the configuration listener feeds.
-- A third journey proves a queued callback from a replaced
-- generation is visible in the retained history and inert against its
-- replacement.
--
-- Non-vacuity (mutation falsifiers; each verified by pointing this suite at
-- a mutated copy of init.lua and observing the named cases FAIL):
--   1. restore didOpen `version = doc.clean_change_id` ->
--      journey_one: didOpen carries explicit fresh origin 0 fails;
--   2. freeze the per-batch `session.version` allocation ->
--      every strictly-increasing-stream case in journey_one fails;
--   3. delete the terminate-first step in create_document_session ->
--      journey_four: replaced-session retirement observation fails (no
--      close_session for the replaced live session's generation);
--   4. make get_workspace_settings cache forever ->
--      journey_two: after-TTL re-read case fails;
--   5. make stop_servers keep sessions alive across process replacement ->
--      journey_one: replacement-kills-old-sessions case fails.
--
-- Red-first baseline: this suite pins CURRENT staged behavior as one
-- coherent whole; it introduces no production change, so green-on-staged is
-- the expected primary result. The falsifiers above carry non-vacuity.
--
-- No framework: plain soft asserts, one process, deterministic, exit code
-- carries the result. Compatible with the Lite XL Lua runtime family
-- (Lua 5.4).

local init_module_path = arg and arg[1] or nil

local here = debug.getinfo(1, "S").source:sub(2):match("^(.*)[/\\]") or "."

local harness = dofile(here .. "/harness.lua")

local passed, failed = 0, 0
local function ok(condition, message)
  if condition then
    passed = passed + 1
  else
    failed = failed + 1
    print("FAIL: " .. message)
  end
end

local INCREMENTAL = {
  textDocumentSync = { openClose = true, change = 2, save = { includeText = false } },
  positionEncoding = "utf-16",
}

---Wire history filtered to LSP traffic (notifications, raw payloads,
---requests) in exact send order, as {method=..., generation=...} pairs.
local function traffic(world)
  local out = {}
  for _, entry in ipairs(world.wire) do
    if entry.kind ~= "response" then
      out[#out + 1] = { method = entry.method, generation = entry.generation_at_send }
    end
  end
  return out
end

local function same_traffic(actual, expected)
  if #actual ~= #expected then return false end
  for i, step in ipairs(expected) do
    if actual[i].method ~= step[1] or actual[i].generation ~= step[2] then
      return false
    end
  end
  return true
end

local function describe(steps)
  local parts = {}
  for _, step in ipairs(steps) do
    parts[#parts + 1] = step.method .. "@g" .. tostring(step.generation)
  end
  return table.concat(parts, " ")
end

-- ===========================================================================
-- Journey one: two interleaved documents, backpressure window, close of the
-- second document, full server restart, reopen into the replacement
-- generation - proven against one complete ordered wire history.
-- ===========================================================================
do
  local world = harness.new_world({ init_module = init_module_path })
  local server = world.define_server("perllsp", { capabilities = INCREMENTAL })
  local doc_a = world.new_doc("C:/proj/main.pl", "my $x = 1;\nmy $y = 2;\n")
  local doc_b = world.new_doc("C:/proj/other.pl", "print 1;\n")

  -- Step: open A, admit, edit twice -> batches v1 then v2.
  world.lsp.open_document(doc_a)
  local opened_a = server:drain("textDocument/didOpen")
  ok(#opened_a == 1 and opened_a[1].params.textDocument.version == 0,
    "journey1: didOpen(A) carries explicit fresh origin 0")
  ok(opened_a[1].params.textDocument.text == "my $x = 1;\nmy $y = 2;\n",
    "journey1: didOpen(A) snapshot is the exact current buffer")

  doc_a:raw_insert(1, 10, "0")
  local batch = server:drain("textDocument/didChange")
  ok(#batch == 1 and batch[1].params.textDocument.version == 1,
    "journey1: first A edit emits batch v1")

  doc_a:raw_insert(2, 11, " # edited")
  batch = server:drain("textDocument/didChange")
  ok(#batch == 1 and batch[1].params.textDocument.version == 2,
    "journey1: second A edit emits batch v2 - stream strictly increasing")

  -- Step: open B interleaved with A's live stream; admit it; edit B once.
  world.lsp.open_document(doc_b)
  local opened_b = server:drain("textDocument/didOpen")
  ok(#opened_b == 1
    and opened_b[1].params.textDocument.uri ~= opened_a[1].params.textDocument.uri,
    "journey1: didOpen(B) admitted alongside A under distinct URI identity")
  doc_b:raw_insert(1, 9, ";")
  batch = server:drain("textDocument/didChange")
  ok(#batch == 1 and batch[1].params.textDocument.version == 1,
    "journey1: B owns an independent version stream starting at v1")

  -- Step: A edits under load (#10833). With enqueue admission gone, each
  -- edit queues immediately; an overwrite-capable newer batch replaces its
  -- unsent predecessor in place, so bursts collapse to latest state without
  -- losing a change and the version stream never rewinds or repeats.
  doc_a:raw_insert(2, 1, "local $z;\n")
  ok(#server.outbound == 1
    and server.outbound[1].params.textDocument.version == 3,
    "journey1: edit queues immediately as batch v3 - no admission hold")
  local session_a = world.lsp.get_document_session(doc_a, server)
  ok(session_a ~= nil and #session_a.pending_changes == 1,
    "journey1: the queued batch's change waits in the live session until sent")

  doc_a:raw_insert(3, 1, "local $w;\n")
  ok(#server.outbound == 1
    and server.outbound[1].params.textDocument.version == 4
    and #server.outbound[1].params.contentChanges == 2,
    "journey1: a second edit coalesces onto the unsent batch at v4 carrying both changes")
  ok(session_a ~= nil and #session_a.pending_changes == 2,
    "journey1: the coalesced unsent batch keeps both changes pending")

  -- Step: close B while A's coalesced batch is still unsent; didClose
  -- queues right behind it.
  world.lsp.close_document(doc_b)
  ok(world.wire[#world.wire] ~= nil
    and world.wire[#world.wire].method == "textDocument/didClose",
    "journey1: didClose(B) recorded while A's batch is held")

  -- Step: flush everything. The queue replays exactly as pushed - coalesced
  -- batch first (newest state, both changes), then the close.
  local flushed = server:drain()
  ok(#flushed == 2
    and flushed[1].method == "textDocument/didChange"
    and flushed[2].method == "textDocument/didClose",
    "journey1: queue order is coalesced-batch-then-close, exactly as pushed")
  ok(flushed[1].params.textDocument.version == 4,
    "journey1: flushed batch continues the stream at v4 across the window")
  ok(#flushed[1].params.contentChanges == 2
    and flushed[1].params.contentChanges[1].text == "local $z;\n"
    and flushed[1].params.contentChanges[2].text == "local $w;\n",
    "journey1: flushed batch carries exactly the once-held changes")

  -- Step: full process replacement.
  local old_server = server
  local old_generation = session_a.server_generation
  world.stop_servers()
  ok(old_server.exits == 1, "journey1: replaced instance exited exactly once")
  ok(world.lsp.get_document_session(doc_a, old_server) == nil,
    "journey1: replacement kills the old process generation's sessions")
  ok(#old_server.outbound == 0,
    "journey1: nothing new may enter the dead generation's queue afterwards")

  server = world.define_server("perllsp", { capabilities = INCREMENTAL })
  world.lsp.open_document(doc_a)
  local reopened = server:drain("textDocument/didOpen")
  ok(#reopened == 1 and reopened[1].params.textDocument.version == 0,
    "journey1: reopened session starts a fresh explicit version origin")
  doc_a.lsp_open = true
  local new_session_a = world.lsp.get_document_session(doc_a, server)
  ok(new_session_a ~= nil and new_session_a ~= session_a
    and new_session_a.session_generation > session_a.session_generation,
    "journey1: reopen advances document-session identity across restarts")
  ok(new_session_a.server_generation > old_generation,
    "journey1: replacement process owns a distinct server generation")

  doc_a:raw_remove(4, 11, 4, 20)
  batch = server:drain("textDocument/didChange")
  ok(#batch == 1 and batch[1].params.textDocument.version == 1,
    "journey1: new generation's stream starts cleanly at v1")

  -- The whole-journey wire history: exact methods AND owning generations,
  -- including the coalesced-then-flushed batch still tagged with generation
  -- 1 after the close of B.
  local expected = {
    { "textDocument/didOpen", 1 },
    { "textDocument/didChange", 1 },
    { "textDocument/didChange", 1 },
    { "textDocument/didOpen", 1 },
    { "textDocument/didChange", 1 },
    { "textDocument/didChange", 1 },
    { "textDocument/didClose", 1 },
    { "textDocument/didOpen", 2 },
    { "textDocument/didChange", 2 },
  }
  local history = traffic(world)
  ok(same_traffic(history, expected),
    "journey1: complete ordered cross-generation wire history (got "
    .. describe(history) .. ")")

  -- Editor truth: the buffer mutated through real base Doc operations
  -- matches the composed edits exactly.
  ok(table.concat(doc_a.lines) ==
    "my $x = 10;\nlocal $z;\nlocal $w;\nmy $y = 2;\n",
    "journey1: final editor bytes reflect insert/append/remove across the journey")

  world.teardown()
end

-- ===========================================================================
-- Journey two: configuration changes mid-journey. The workspace-settings
-- reader serves ONE cached table inside its TTL and re-reads changed server
-- settings after it - observed through the public get_workspace_settings
-- path that the workspace/configuration listener feeds. (The listener glue
-- itself is registered inside lsp.start_server's real-process construction,
-- a boundary this harness deliberately leaves to the future server-level
-- seam.)
-- ===========================================================================
do
  local world = harness.new_world({ init_module = init_module_path })
  local server = world.define_server("perllsp", {
    capabilities = INCREMENTAL,
    settings = { perl = { feature = true, name = "one" } },
  })

  local first = world.lsp.get_workspace_settings(server)
  ok(type(first) == "table" and first.perl ~= nil
    and first.perl.feature == true and first.perl.name == "one",
    "journey2: server settings merge into the workspace settings read")

  -- Mid-journey configuration change INSIDE the TTL: the cache honestly
  -- serves the previous snapshot - same table identity, old values.
  world.clock.advance(2)
  server.settings = { perl = { feature = false, name = "two" } }
  local cached = world.lsp.get_workspace_settings(server)
  ok(cached == first,
    "journey2: inside the TTL the reader returns the one cached snapshot")
  ok(cached.perl.feature == true and cached.perl.name == "one",
    "journey2: the cached snapshot carries pre-change values")

  -- Advance past the five-second TTL: the same journey observes new values.
  world.clock.advance(4)
  local refreshed = world.lsp.get_workspace_settings(server)
  ok(refreshed ~= first
    and refreshed.perl.feature == false and refreshed.perl.name == "two",
    "journey2: after the TTL the re-read picks up the mid-journey change")

  world.teardown()

  -- Clock restoration: wall-clock time works again after teardown.
  local probe = os.time({ year = 2000, month = 1, day = 1, hour = 12 })
  ok(math.abs(probe - 1000000000) > 86400 * 365,
    "journey2: teardown restores the real clock")
end

-- ===========================================================================
-- Journey three: a queued callback from a replaced generation stays visible
-- in the retained history and is provably inert against its replacement.
-- ===========================================================================
do
  local world = harness.new_world({ init_module = init_module_path })
  local old_server = world.define_server("perllsp", { capabilities = INCREMENTAL })
  local doc = world.new_doc("C:/proj/stale.pl", "s\n")

  world.lsp.open_document(doc)
  old_server:drain("textDocument/didOpen")
  doc.lsp_open = true

  -- Queue an unsent batch: it queues immediately (#10833 - no admission
  -- gate), and its callback would clear the OLD session only when played.
  doc:raw_insert(1, 2, "t")
  local held = old_server.outbound[1]
  local old_session = world.lsp.get_document_session(doc, old_server)
  ok(held ~= nil and not held.sent,
    "journey3: queued batch stays unsent until its callback plays")

  -- Replace the process generation, reopen, and confirm clean state.
  world.stop_servers()
  local replacement = world.define_server("perllsp", { capabilities = INCREMENTAL })
  world.lsp.open_document(doc)
  replacement:drain("textDocument/didOpen")
  doc.lsp_open = true
  local new_session = world.lsp.get_document_session(doc, replacement)
  ok(new_session ~= nil and new_session ~= old_session,
    "journey3: replacement owns a distinct session record")

  -- Play the dead generation's queued callback through the REAL callback
  -- path; it runs against dead state and must stay inert.
  held.callback(old_server)
  ok(new_session.pending_changes ~= nil and #new_session.pending_changes == 0,
    "journey3: stale callback cannot mutate the replacement session")
  ok(#new_session.pending_changes == 0 and new_session.version == 0,
    "journey3: replacement session's version stream untouched by stale play")

  -- Lifecycle observations recorded by the diagnostics seam (#11124).
  local retired, noted = false, false
  for _, record in ipairs(world.diagnostics_log) do
    if record.op == "retire_provider" and record.name == "perllsp" then retired = true end
    if record.op == "note_provider" and record.generation == replacement.generation then
      noted = true
    end
  end
  ok(retired, "journey3: provider retirement observed at process replacement")
  ok(noted, "journey3: provider liveness re-noted for the replacement generation")

  world.teardown()
end

-- ===========================================================================
-- Negative control: a definition whose patterns admit nothing produces zero
-- wire traffic for any document activity - registration honesty.
-- ===========================================================================
do
  local world = harness.new_world({ init_module = init_module_path })
  local server = world.define_server("perllsp", {
    file_patterns = { "%.pm$" },
    capabilities = INCREMENTAL,
  })
  local doc = world.new_doc("C:/proj/script.pl", "x\n")
  world.lsp.open_document(doc)
  server:drain()
  doc:raw_insert(1, 2, "y")
  server:drain()
  ok(#world.wire == 0,
    "negative: non-matching pattern admits no server and emits zero wire traffic")
  world.teardown()
end

-- ===========================================================================
-- Journey four: reopening over a LIVE session (no close, no restart)
-- replaces identity, kills held state, and observably retires the replaced
-- session through the diagnostics lifecycle seam.
-- ===========================================================================
do
  local world = harness.new_world({ init_module = init_module_path })
  local server = world.define_server("perllsp", { capabilities = INCREMENTAL })
  local doc = world.new_doc("C:/proj/live.pl", "l\n")

  world.lsp.open_document(doc)
  server:drain("textDocument/didOpen")
  doc.lsp_open = true
  local first = world.lsp.get_document_session(doc, server)

  server.can_push_value = false
  doc:raw_insert(1, 2, "x")
  server.can_push_value = true
  ok(first ~= nil and #first.pending_changes == 1,
    "journey4: live session holds unsent state before the reopen")

  -- Reopen the same URI straight over the live held session.
  world.lsp.open_document(doc)
  server:drain()
  local second = world.lsp.get_document_session(doc, server)
  ok(second ~= nil and second ~= first
    and second.session_generation > first.session_generation,
    "journey4: reopen replaces the live session with a new identity")
  ok(#second.pending_changes == 0,
    "journey4: the replaced session's held state died with it")

  local retired = false
  for _, record in ipairs(world.diagnostics_log) do
    if record.op == "close_session"
      and record.session_generation == first.session_generation
      and record.uri == first.uri then
      retired = true
    end
  end
  ok(retired,
    "journey4: the replaced session retired observably (close_session)")

  doc:raw_insert(1, 3, "y")
  batch = server:drain("textDocument/didChange")
  ok(#batch == 1 and batch[1].params.textDocument.version == 1,
    "journey4: replacement stream starts cleanly at v1")

  world.teardown()
end

-- ===========================================================================
-- Journey five: transport-overwrite fidelity and terminal-line buffer
-- semantics. Two emitted batches without an intervening drain coalesce into
-- ONE queued frame at the latest version - the superseded payload never
-- reaches the wire - and a document without a terminal newline accepts an
-- appending insertion at #lines+1 of its last element.
-- ===========================================================================
do
  local world = harness.new_world({ init_module = init_module_path })
  local server = world.define_server("perllsp", { capabilities = INCREMENTAL })
  local doc = world.new_doc("C:/proj/coalesce.pl", "a\n")

  world.lsp.open_document(doc)
  server:drain("textDocument/didOpen")
  doc.lsp_open = true

  doc:raw_insert(1, 2, "b")
  ok(#server.outbound == 1, "journey5: first batch occupies one queue slot")
  doc:raw_insert(1, 3, "c")
  ok(#server.outbound == 1,
    "journey5: second emission overwrites the unsent frame in place")

  local played = server:drain("textDocument/didChange")
  ok(#played == 1 and played[1].params.textDocument.version == 2,
    "journey5: drained frame carries the latest allocated version")
  ok(#played[1].params.contentChanges == 2
    and played[1].params.contentChanges[1].text == "b"
    and played[1].params.contentChanges[2].text == "c",
    "journey5: overwritten frame carries every once-pending change in order")

  local frames = 0
  local versions = {}
  for _, entry in ipairs(world.wire) do
    if entry.method == "textDocument/didChange" then
      frames = frames + 1
      versions[#versions + 1] = entry.params.textDocument.version
    end
  end
  ok(frames == 1 and versions[1] == 2,
    "journey5: wire history holds exactly the frames production could send")

  world.teardown()
end

do
  -- Terminal-line buffer semantics: a document without a trailing newline
  -- accepts appending insertions past its last byte.
  local world = harness.new_world({ init_module = init_module_path })
  local doc = world.new_doc("C:/proj/noeol.pl", "abc")
  ok(#doc.lines == 1 and doc.lines[1] == "abc",
    "journey5: newline-less content loads as one terminal line")
  doc:raw_insert(1, 4, "d")
  ok(table.concat(doc.lines) == "abcd",
    "journey5: insertion after a newline-less final char appends exactly")
  doc:raw_insert(1, 2, "X")
  ok(table.concat(doc.lines) == "aXbcd",
    "journey5: mid-line insertion stays byte-exact")
  ok(#world.wire == 0, "journey5: buffer-only journey emits no wire traffic")
  world.teardown()
end

print(string.format("%d passed, %d failed", passed, failed))
os.exit(failed == 0 and 0 or 1)

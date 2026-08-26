-- Deterministic focused tests for client-affordance command gating in
-- clients/lite-xl/upstream/init.lua (#11172).
--
-- Run:
--   lua clients/lite-xl/tests/init_command_projection_test.lua
--     [path-to-init-module] [path-to-manifest-module]
-- Default paths are ../upstream/init.lua and
-- ../upstream/capability_manifest.lua relative to this file.
--
-- Seam owned: the COMMAND PROJECTION - an lsp:* action may send its request
-- only when BOTH the server advertises the capability AND the manifest row
-- proves an implemented client consumer. Server capability alone must never
-- enable a false affordance. Today that bites exactly two seams whose
-- consumers are absent upstream of their owners:
--
--   - lsp:view-call-hierarchy sends textDocument/prepareCallHierarchy and
--     discards the result (#10719 owns consumption);
--   - lsp:rename-symbol sends textDocument/rename and logs returned edits
--     as its only effect (#8986 owns application).
--
-- Under the projection both refuse with one explicit unsupported message
-- naming the owner issue, and NOTHING reaches the wire. Commands whose
-- consumer is implemented keep sending under the same pipeline (positive
-- control), and every registered lsp:* command carries a manifest row so
-- future seams inherit the mechanism mechanically.
--
-- Proof shape: journey harness worlds (#11103) drive the exact staged
-- init.lua public paths against fake running servers with full wire
-- history.
--
-- Red-first baseline: against CURRENT MAIN before #11172 the two gated
-- senders push their requests regardless of any client-consumer truth, so
-- the empty-wire cases MUST fail there while the hover control passes on
-- both sides (contract pin). Observed pristine-main baseline: 6 failed /
-- 3 passed. Observed patched result: 8 passed, 0 failed.
--
-- Mutation falsifiers of the PATCHED module (each mechanically verified to
-- fail the suite against a mutated init or manifest copy):
--   1. delete the projection gate at the top of the callHierarchyProvider
--      branch of lsp.request_call_hierarchy -> the prepareCallHierarchy
--      empty-wire case fails;
--   2. delete the projection gate in the renameProvider branch of
--      lsp.request_symbol_rename -> the rename empty-wire case fails;
--   3. replace the gate's manifest lookup with a constant true -> both
--      empty-wire cases fail;
--   4. remove the explicit unsupported message -> the message cases fail.
--
-- No framework: plain soft asserts, deterministic (fake clock), exit code
-- carries the result. Compatible with the Lite XL Lua runtime family
-- (Lua 5.4).

local init_module_path = arg and arg[1] or nil
local manifest_module_path = arg and arg[2] or nil

if not init_module_path then
  local info = debug.getinfo(1, "S").source:sub(2)
  local dir = info:match("^(.*)[/\\]") or "."
  init_module_path = dir .. "/../upstream/init.lua"
end
if not manifest_module_path then
  local info = debug.getinfo(1, "S").source:sub(2)
  local dir = info:match("^(.*)[/\\]") or "."
  manifest_module_path = dir .. "/../upstream/capability_manifest.lua"
end

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

local function wire_methods(world)
  local methods = {}
  for _, entry in ipairs(world.wire) do
    methods[#methods + 1] = entry.method
  end
  return methods
end

local function sent(world, method)
  for _, entry in ipairs(world.wire) do
    if entry.method == method then return true end
  end
  return false
end

local function logged(world, token)
  for _, record in ipairs(world.log_records) do
    if tostring(record):find(token, 1, true) ~= nil then return true end
  end
  return false
end

local function new_world()
  local world = harness.new_world({ init_module = init_module_path, manifest_module = manifest_module_path })
  local server = world.define_server("perllsp", {
    capabilities = {
      textDocumentSync = {
        openClose = true,
        change = 2,
        save = { includeText = false },
      },
      positionEncoding = "utf-16",
      -- The server advertises everything the two false affordances need:
      -- capability presence alone must not create a usable command.
      callHierarchyProvider = true,
      renameProvider = { prepareProvider = true },
      hoverProvider = true,
    },
  })
  local doc = world.new_doc("C:/proj/main.pl", "my $symbol = 1;\n")
  world.lsp.open_document(doc)
  server:drain("textDocument/didOpen")
  return world, server, doc
end

-- ---------------------------------------------------------------------------
-- Call hierarchy: capability advertised, consumer absent -> no wire traffic
-- ---------------------------------------------------------------------------

do
  local world, server, doc = new_world()

  world.lsp.request_call_hierarchy(doc, 1, 6)

  ok(not sent(world, "textDocument/prepareCallHierarchy"),
    "call hierarchy never sends under an unsupported client row")
  ok(logged(world, "#10719"),
    "one explicit unsupported message names the call-hierarchy owner")
  ok(server.exits == 0, "the refusal shuts nothing down")

  world.teardown()
end

-- ---------------------------------------------------------------------------
-- Rename: capability advertised (even with prepare support), application
-- absent -> no wire traffic
-- ---------------------------------------------------------------------------

do
  local world, _, doc = new_world()

  world.lsp.request_symbol_rename(doc, 1, 6, "$renamed")

  ok(not sent(world, "textDocument/rename"),
    "rename never sends under an unsupported client row")
  ok(logged(world, "#8986"),
    "one explicit unsupported message names the rename owner")

  world.teardown()
end

-- ---------------------------------------------------------------------------
-- Positive control: implemented consumers keep sending through the same
-- pipeline (guards against a vacuously green suite).
-- ---------------------------------------------------------------------------

do
  local world, _, doc = new_world()

  world.lsp.request_hover(doc, 1, 6)

  local saw_hover = false
  for _, method in ipairs(wire_methods(world)) do
    if method == "textDocument/hover" then saw_hover = true end
  end
  ok(saw_hover,
    "an implemented command still sends its request (control pin)")

  world.teardown()
end

-- ---------------------------------------------------------------------------
-- Matrix coverage: every registered lsp:* command has a manifest row
-- ---------------------------------------------------------------------------

do
  local world = harness.new_world({ init_module = init_module_path, manifest_module = manifest_module_path })
  -- Registration happens at module load; the world recorded it.
  local registered = world.commands_registered

  local count = 0
  for name in pairs(registered) do
    if name:find("^lsp:") then count = count + 1 end
  end
  ok(count > 0, "the staged plugin registers lsp commands")

  local loaded, manifest = pcall(dofile, manifest_module_path)
  if not loaded then
    ok(false, "capability manifest module loads (absent on pristine main)")
    manifest = { commands = {} }
  end

  local missing = {}
  for name in pairs(registered) do
    if name:find("^lsp:") and not manifest.commands[name] then
      missing[#missing + 1] = name
    end
  end
  ok(#missing == 0,
    "every registered lsp:* command carries a manifest row"
      .. (#missing > 0 and (": " .. table.concat(missing, ", ")) or ""))

  world.teardown()
end

print(string.format("\n%d passed, %d failed", passed, failed))
if failed > 0 then os.exit(1) end

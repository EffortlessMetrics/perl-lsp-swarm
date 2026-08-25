-- Deterministic focused tests for the lite-xl capability/affordance
-- projection manifest (#11172).
--
-- Run:
--   lua clients/lite-xl/tests/capability_manifest_test.lua [path-to-manifest-module]
-- Default module path is ../upstream/capability_manifest.lua relative to
-- this file.
--
-- Seam owned: the MANIFEST MODULE itself - profile identity, row schema,
-- disposition discipline, the pure client-capability derivation, the pure
-- command-availability predicate, the custom-claim scan, and second-run
-- generation identity ("no diff"). Payload integration lives in
-- server_initialize_capabilities_test.lua; command gating lives in
-- init_command_projection_test.lua.
--
-- Contract pinned here:
--   - every initial required row from issue #11172 is present with a valid
--     disposition (implemented | configuration_only | unsupported |
--     not_proven);
--   - a row may be advertised ONLY with an implemented disposition;
--   - every advertised row has a value producer; every implemented row has
--     an owner and a proof pointer;
--   - client_capabilities() folds advertised rows into the exact default
--     initialize payload and FAILS CLOSED when a manifest copy advertises a
--     row without an implemented disposition (negative control: re-adding
--     relatedInformation/tagSupport/codeDescriptionSupport or watched-file
--     dynamicRegistration cannot silently reach the wire);
--   - command_availability() never grants availability from a server
--     capability alone - the client-consumer disposition decides;
--   - unsupported_custom_claims() reports user overrides that claim known
--     unimplemented features instead of letting them become evidence;
--   - projection_report() is deterministic: two runs and two independent
--     module loads produce byte-identical output (second-run no-diff law).
--
-- Red-first baseline: run against CURRENT MAIN before the #11172 patch -
-- there is no capability_manifest.lua yet, so loading this suite fails
-- (nonzero exit). Observed pristine-main baseline: load error before any
-- assertion. Observed patched result: 368 passed, 0 failed.
--
-- Mutation falsifiers of the PATCHED module (each mechanically verified to
-- fail the suite against a mutated module copy):
--   1. flip the publishDiagnostics.relatedInformation row to
--      advertised=true -> client_capabilities raises
--      (advertise-requires-implemented) and the closed-door case fails;
--   2. delete the raise in client_capabilities so unimplemented rows fold
--      through -> the closed-door case observes the forbidden leaf;
--   3. change command_availability to ignore the consumer disposition ->
--      the call-hierarchy availability cases fail (capability alone would
--      grant access);
--   4. make projection_report embed any nondeterministic input (e.g. an
--      os.time/os.clock stamp) -> the time-sandwich no-diff case fails
--      because two runs observed different fake clock readings.
--
-- No framework: plain soft asserts, one process, deterministic, exit code
-- carries the result. Compatible with the Lite XL Lua runtime family
-- (Lua 5.4).

local manifest_path = arg and arg[1] or nil

if not manifest_path then
  local info = debug.getinfo(1, "S").source:sub(2)
  local dir = info:match("^(.*)[/\\]") or "."
  manifest_path = dir .. "/../upstream/capability_manifest.lua"
end

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
-- Ordered deep comparison (arrays are order-sensitive: valueSet and
-- properties order is what the wire carries).
-- ---------------------------------------------------------------------------

local function deep_equal(a, b)
  if a == b then return true end
  if type(a) ~= "table" or type(b) ~= "table" then return false end
  local count = 0
  for k, v in pairs(a) do
    count = count + 1
    if not deep_equal(v, b[k]) then return false end
  end
  for _ in pairs(b) do
    count = count - 1
  end
  return count == 0
end

local function raises(message_part, fn)
  local ran_ok, err = pcall(fn)
  if ran_ok then return false, "did not raise" end
  if type(err) ~= "string" or not err:find(message_part, 1, true) then
    return false, "raised without '" .. message_part .. "': " .. tostring(err)
  end
  return true, err
end

-- ---------------------------------------------------------------------------
-- Load the manifest twice through independent dofile instances so cross-load
-- determinism never accidentally compares one table against itself.
-- ---------------------------------------------------------------------------

local manifest = dofile(manifest_path)
local manifest_again = dofile(manifest_path)

local DISPOSITIONS = {
  implemented = true,
  configuration_only = true,
  unsupported = true,
  not_proven = true,
}

local function rows_by_id()
  local index = {}
  for _, row in ipairs(manifest.capabilities) do
    index[row.id] = row
  end
  return index
end

-- ---------------------------------------------------------------------------
-- Profile identity
-- ---------------------------------------------------------------------------

do
  ok(type(manifest.profile) == "table", "manifest carries a profile identity")
  ok(type(manifest.profile.id) == "string" and #manifest.profile.id > 0,
    "profile has a non-empty id")
  ok(manifest.profile.upstream_base_ref
      == "d1432ae0736cd9531798b4bc1221835f534cc689",
    "profile records the documented pristine upstream base ref")
  ok(type(manifest.profile.tree_digest_algorithm) == "string"
      and #manifest.profile.tree_digest_algorithm > 0,
    "profile names its tree digest algorithm")
  ok(type(manifest.profile.tree_digest_inputs) == "table"
      and #manifest.profile.tree_digest_inputs > 0,
    "profile lists its tree digest inputs")
end

-- ---------------------------------------------------------------------------
-- Required initial rows (#11172 "Initial required rows")
-- ---------------------------------------------------------------------------

do
  local rows = rows_by_id()

  local required = {
    -- workspace.configuration
    "workspace.configuration",
    -- workspace.didChangeWatchedFiles.dynamicRegistration (absence row)
    "workspace.didChangeWatchedFiles.dynamicRegistration",
    -- workspaceFolders / multi-root
    "workspace.workspaceFolders.multi_root",
    -- workspace.applyEdit / workspaceEdit forms
    "workspace.applyEdit",
    -- synchronization.didSave
    "textDocument.synchronization.didSave",
    -- completion forms
    "textDocument.completion.completionItem.snippetSupport",
    "textDocument.completion.completionItem.documentationFormat",
    "textDocument.completion.completionItem.insertReplaceSupport",
    "textDocument.completion.completionItem.resolveSupport",
    "textDocument.completion.completionItemKind.valueSet",
    -- hover/signature/document symbols
    "textDocument.hover.contentFormat",
    "textDocument.signatureHelp.signatureInformation.documentationFormat",
    "textDocument.documentSymbol.symbolKind.valueSet",
    -- references/definition/implementation request paths
    "commands.goto_symbol.request_paths",
    -- publishDiagnostics family
    "textDocument.publishDiagnostics.versionSupport",
    "textDocument.publishDiagnostics.relatedInformation",
    "textDocument.publishDiagnostics.tagSupport",
    "textDocument.publishDiagnostics.codeDescriptionSupport",
    "textDocument.publishDiagnostics.dataSupport",
    -- rename/prepareRename client behavior
    "rename.client_application",
    -- formatting application
    "commands.formatting.application",
    -- call hierarchy
    "callHierarchy.result_consumption",
    -- semantic tokens
    "textDocument.semanticTokens",
    -- code actions
    "textDocument.codeAction",
    -- inlay hints / code lens
    "textDocument.inlayHint",
    "textDocument.codeLens",
    -- pull diagnostics
    "textDocument.diagnostic.pull",
    -- window.showDocument
    "window.showDocument.support",
    -- work-done progress / refresh surfaces
    "window.workDoneProgress",
    -- positionEncodings
    "general.positionEncodings",
  }

  for _, id in ipairs(required) do
    ok(rows[id] ~= nil, "required row present: " .. id)
  end
end

-- ---------------------------------------------------------------------------
-- Row schema and disposition discipline
-- ---------------------------------------------------------------------------

do
  local seen_ids = {}
  for _, row in ipairs(manifest.capabilities) do
    ok(type(row.id) == "string" and #row.id > 0, "row has an id")
    ok(seen_ids[row.id] == nil, "row id unique: " .. tostring(row.id))
    seen_ids[row.id] = true
    ok(type(row.path) == "string" and #row.path > 0,
      "row has a capability JSON path: " .. tostring(row.id))
    ok(DISPOSITIONS[row.disposition] ~= nil,
      "row has a valid disposition: " .. tostring(row.id))
    ok(type(row.advertised) == "boolean",
      "row states advertisement explicitly: " .. tostring(row.id))
    ok(type(row.invalidation_inputs) == "table"
      and #row.invalidation_inputs > 0,
      "row lists invalidation inputs: " .. tostring(row.id))

    if row.advertised then
      ok(row.disposition == "implemented",
        "advertised row is implemented: " .. tostring(row.id))
    end

    if row.disposition == "implemented" then
      ok(type(row.owner) == "string" and #row.owner > 0,
        "implemented row names its owner: " .. tostring(row.id))
      ok(type(row.proof) == "string" and #row.proof > 0,
        "implemented row names its proof: " .. tostring(row.id))
    end

    if row.disposition == "unsupported" then
      ok(type(row.owner_issue) == "string" and #row.owner_issue > 0,
        "unsupported row routes to an owner issue: " .. tostring(row.id))
    end
  end

  -- The three unconsumed publishDiagnostics leaves stay OFF the wire and
  -- carry their routing.
  local rows = rows_by_id()
  for _, id in ipairs({
    "textDocument.publishDiagnostics.relatedInformation",
    "textDocument.publishDiagnostics.tagSupport",
    "textDocument.publishDiagnostics.codeDescriptionSupport",
  }) do
    local row = rows[id]
    ok(row ~= nil and row.advertised == false,
      id .. " is explicitly not advertised")
    ok(row ~= nil and row.disposition == "unsupported",
      id .. " is dispositioned unsupported (no client consumer)")
  end

  -- versionSupport flipped truthful when currentness consumption landed.
  local version_row = rows["textDocument.publishDiagnostics.versionSupport"]
  ok(version_row.advertised == true and version_row.disposition == "implemented"
    and type(version_row.proof) == "string"
    and version_row.proof:find("diagnostics_currentness_test", 1, true) ~= nil,
    "versionSupport stays advertised with its currentness proof pointer")

  -- Watched-file dynamic registration stays absent until #9001/#10785.
  local watched = rows["workspace.didChangeWatchedFiles.dynamicRegistration"]
  ok(watched.advertised == false and watched.disposition == "unsupported",
    "watched-files dynamicRegistration remains explicitly absent")

  -- Optional dependency honesty: snippet support depends on the exact
  -- installed plugin state, never a guessed true.
  local snippets = rows["textDocument.completion.completionItem.snippetSupport"]
  ok(snippets.optional_dependency ~= nil,
    "snippetSupport declares its optional dependency")
end

-- ---------------------------------------------------------------------------
-- Command/action coverage
-- ---------------------------------------------------------------------------

do
  ok(type(manifest.commands) == "table", "manifest carries command rows")

  local expected_commands = {
    ["lsp:complete"] = true,
    ["lsp:goto-definition"] = true,
    ["lsp:goto-implementation"] = true,
    ["lsp:show-signature"] = true,
    ["lsp:show-symbol-info"] = true,
    ["lsp:show-symbol-info-in-tab"] = true,
    ["lsp:view-call-hierarchy"] = true,
    ["lsp:view-document-symbols"] = true,
    ["lsp:format-document"] = true,
    ["lsp:view-document-diagnostics"] = true,
    ["lsp:rename-symbol"] = true,
    ["lsp:find-references"] = true,
    ["lsp:find-workspace-symbol"] = true,
    ["lsp:view-all-diagnostics"] = true,
    ["lsp:toggle-diagnostics"] = true,
    ["lsp:stop-servers"] = true,
    ["lsp:start-servers"] = true,
    ["lsp:restart-servers"] = true,
  }
  for id in pairs(expected_commands) do
    local row = manifest.commands[id]
    ok(row ~= nil, "command row present: " .. id)
    if row then
      ok(type(row.consumer_disposition) == "string",
        "command row states its consumer disposition: " .. id)
      if row.server_capability ~= nil then
        ok(type(row.server_capability) == "string"
          and #row.server_capability > 0,
          "server-capability-dependent command names the capability: " .. id)
      end
    end
  end

  local blocked = {
    ["lsp:view-call-hierarchy"] = "#10719",
    ["lsp:rename-symbol"] = "#8986",
  }
  for id, owner in pairs(blocked) do
    local row = manifest.commands[id]
    ok(row ~= nil and row.consumer_disposition ~= "implemented",
      id .. " is not marked implemented while its owner lands")
    ok(row ~= nil and type(row.unsupported_message) == "string"
      and row.unsupported_message:find(owner, 1, true) ~= nil,
      id .. " carries one explicit unsupported message naming its owner")
  end
end

-- ---------------------------------------------------------------------------
-- Pure derivation: client_capabilities
-- ---------------------------------------------------------------------------

local DEPS = {
  snippet_support = false,
  completion_item_kinds = { 1, 2, 3 },
  symbol_kinds = { 4, 5 },
  position_encoding_list = { "utf-16" },
}

do
  local caps = manifest.client_capabilities(DEPS)

  -- Exact truth table after reconciliation: consumed leaves stay, the
  -- unconsumed publishDiagnostics extras are gone.
  ok(caps.workspace
    and caps.workspace.configuration == true,
    "payload advertises workspace.configuration")
  ok(caps.textDocument
    and caps.textDocument.synchronization
    and caps.textDocument.synchronization.didSave == true,
    "payload advertises didSave (the client sends didSave notifications)")
  ok(caps.textDocument.publishDiagnostics ~= nil
    and caps.textDocument.publishDiagnostics.versionSupport == true,
    "payload advertises consumed diagnostic versions")
  ok(caps.textDocument.publishDiagnostics.relatedInformation == nil,
    "payload does not advertise relatedInformation")
  ok(caps.textDocument.publishDiagnostics.tagSupport == nil,
    "payload does not advertise tagSupport")
  ok(caps.textDocument.publishDiagnostics.codeDescriptionSupport == nil,
    "payload does not advertise codeDescriptionSupport")
  ok(caps.window
    and caps.window.showDocument
    and caps.window.showDocument.support == true,
    "payload advertises showDocument with outcome handling landed")
  ok(caps.general
    and caps.general.positionEncodings[1] == "utf-16",
    "payload advertises exactly the supported position encoding")
  ok(caps.textDocument.completion.completionItem.snippetSupport == false,
    "payload honors the dependency state for snippetSupport (false here)")
  ok(deep_equal(
      caps.textDocument.completion.completionItem.resolveSupport,
      { properties = { "documentation", "detail", "additionalTextEdits" } }),
    "payload advertises resolveSupport limited to the #12547-consumed fields")
  ok(deep_equal(
      caps.textDocument.completion.completionItemKind.valueSet,
      DEPS.completion_item_kinds),
    "payload forwards the completion kind list unchanged")

  local snippet_caps =
    manifest.client_capabilities({
      snippet_support = true,
      completion_item_kinds = DEPS.completion_item_kinds,
      symbol_kinds = DEPS.symbol_kinds,
      position_encoding_list = DEPS.position_encoding_list,
    })
  ok(snippet_caps.textDocument.completion.completionItem.snippetSupport == true,
    "payload honors the dependency state for snippetSupport (true)")

  -- Determinism: two derivations agree deeply.
  ok(deep_equal(caps, manifest.client_capabilities(DEPS)),
    "two derivations produce identical payloads")
end

-- Closed door: advertising an unimplemented row must fail loudly, never
-- reach the wire (negative controls from #11172).
do
  local function mutated(fn)
    local copy = {}
    for _, row in ipairs(manifest.capabilities) do
      copy[#copy + 1] = fn(row) or row
    end
    return copy
  end

  local original_rows = manifest.capabilities

  local function with_rows(rows, run)
    manifest.capabilities = rows
    local ok_run, err = pcall(run)
    manifest.capabilities = original_rows
    return ok_run, err
  end

  local _, related_err = with_rows(mutated(function(row)
    if row.id == "textDocument.publishDiagnostics.relatedInformation" then
      local patched = {}
      for k, v in pairs(row) do patched[k] = v end
      patched.advertised = true
      return patched
    end
  end), function()
    return manifest.client_capabilities(DEPS)
  end)
  ok(type(related_err) == "string"
    and related_err:find("implemented", 1, true) ~= nil,
    "re-advertising relatedInformation without a consumer fails closed")

  -- Flipping a row to implemented without its component landing also fails:
  -- there is no value producer for an unlanded leaf, so nothing can reach
  -- the wire by disposition edits alone.
  local _, watched_err = with_rows(mutated(function(row)
    if row.id == "workspace.didChangeWatchedFiles.dynamicRegistration" then
      local patched = {}
      for k, v in pairs(row) do patched[k] = v end
      patched.advertised = true
      patched.disposition = "implemented"
      return patched
    end
  end), function()
    return manifest.client_capabilities(DEPS)
  end)
  ok(type(watched_err) == "string"
    and watched_err:find("producer", 1, true) ~= nil,
    "a disposition flip without a landed value producer fails closed")
end

-- ---------------------------------------------------------------------------
-- Pure predicate: command_availability
-- ---------------------------------------------------------------------------

do
  local available, reason

  available, reason =
    manifest.command_availability("lsp:view-call-hierarchy",
      { callHierarchyProvider = true })
  ok(available == false and reason == "client_consumer_unsupported",
    "call hierarchy stays unavailable even with the server capability present")

  available =
    manifest.command_availability("lsp:rename-symbol",
      { renameProvider = true })
  ok(available == false,
    "rename stays unavailable even with the server capability present")

  available, reason =
    manifest.command_availability("lsp:show-symbol-info", {})
  ok(available == false and reason == "server_capability_absent",
    "an implemented consumer still needs the server capability")

  available =
    manifest.command_availability("lsp:show-symbol-info",
      { hoverProvider = true })
  ok(available == true,
    "implemented consumer plus server capability grants availability")

  available =
    manifest.command_availability("lsp:view-all-diagnostics", nil)
  ok(available == true,
    "client-local commands do not depend on server capabilities")

  available, reason =
    manifest.command_availability("lsp:not-a-command", {})
  ok(available == false and reason == "unknown_command",
    "unknown commands are refused explicitly")
end

-- ---------------------------------------------------------------------------
-- Custom capability claims (#11172 rule 6)
-- ---------------------------------------------------------------------------

do
  local claims =
    manifest.unsupported_custom_claims({
      textDocument = {
        semanticTokens = { requests = { full = true } },
        publishDiagnostics = { relatedInformation = true },
      },
      workspace = { configuration = false },
    })

  ok(type(claims) == "table" and #claims >= 2,
    "custom claims over unimplemented rows are reported")
  local saw_semantic, saw_related = false, false
  for _, claim in ipairs(claims) do
    if claim.path == "textDocument.semanticTokens" then
      saw_semantic = true
    end
    if claim.path == "textDocument.publishDiagnostics.relatedInformation" then
      saw_related = true
    end
  end
  ok(saw_semantic, "semantic-tokens custom claim is flagged")
  ok(saw_related, "relatedInformation custom claim is flagged")

  claims = manifest.unsupported_custom_claims({
    workspace = { configuration = false },
  })
  ok(#claims == 0,
    "overriding an implemented row is user freedom, not a false claim")

  claims = manifest.unsupported_custom_claims({ unknown_section = { x = 1 } })
  ok(#claims == 0, "unknown sections are outside the bounded scan")

  -- An explicit false is a DECLINE of the capability, not a claim of
  -- support: flagging it would warn on every startup for defensive
  -- configuration (#12599 review).
  claims = manifest.unsupported_custom_claims({
    textDocument = {
      publishDiagnostics = { relatedInformation = false },
      semanticTokens = false,
    },
  })
  ok(#claims == 0, "explicit false opt-outs are declines, not claims")

  claims = manifest.unsupported_custom_claims({
    textDocument = {
      publishDiagnostics = { relatedInformation = true },
      semanticTokens = { requests = { full = true } },
    },
  })
  ok(#claims == 2, "true/table claims over unsupported rows still flag")
end

-- ---------------------------------------------------------------------------
-- Second-run generation identity (no diff)
-- ---------------------------------------------------------------------------

do
  local report_a = manifest.projection_report("deadbeef")
  local report_b = manifest.projection_report("deadbeef")
  ok(report_a == report_b, "second-run projection report produces no diff")

  -- Time sandwich: two loads generating under DIFFERENT clock readings
  -- must still agree byte-for-byte, so no wall-clock input can leak into
  -- generation.
  local original_time = os.time
  os.time = function() return 100000100 end
  local report_t1 = manifest.projection_report("deadbeef")
  os.time = function() return 100000900 end
  local report_t2 = manifest_again.projection_report("deadbeef")
  os.time = original_time

  ok(report_t1 == report_t2,
    "an independent module load produces the identical report")
  ok(report_a == report_t1,
    "generation does not observe the clock at all")

  ok(report_a:find("deadbeef", 1, true) ~= nil,
    "report embeds the supplied tree digest")
  ok(report_a:find("textDocument.semanticTokens", 1, true) ~= nil,
    "report includes unsupported rows so advanced features never vanish "
      .. "from the matrix")

  local different = manifest.projection_report("cafebabe")
  ok(different ~= report_a, "a different tree digest changes the report")
end

-- ---------------------------------------------------------------------------

print(string.format("\n%d passed, %d failed", passed, failed))
if failed > 0 then os.exit(1) end

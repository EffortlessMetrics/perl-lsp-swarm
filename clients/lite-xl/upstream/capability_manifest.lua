-- Profile-scoped capability and affordance projection manifest (#11172).
--
-- This module is the single advertisement authority for the staged
-- lite-xl-lsp client: it declares which LSP client capabilities the exact
-- composed tree may advertise in its `initialize` payload, which commands
-- the plugin may expose, and the disposition of everything that stays
-- absent. server.lua folds its default capabilities through
-- client_capabilities(); init.lua consults command rows before sending
-- requests; support registries (#7122/#9016) may CONSUME this projection
-- but actual-host support verdicts remain owned by #9008 receipts - a
-- correct initialize payload is static/protocol evidence only.
--
-- Disposition law (issue #11172):
--   implemented        the exact client code consumes the protocol
--                      obligation and deterministic proof pins it;
--   configuration_only presence is configuration, not semantic support;
--   unsupported        no client consumer yet; routed to an owner issue;
--   not_proven         behavior exists but no current proof owns it;
-- and a row may be advertised ONLY with disposition "implemented".
-- Capability absence is meaningful: unsupported advanced features keep
-- their rows so they never silently vanish from the matrix.
--
-- This module stays PURE: no require calls, no editor globals, deterministic
-- table traversal only. Everything dynamic arrives through arguments, so
-- any runtime or test harness can load it directly.

local capability_manifest = {}

capability_manifest.profile = {
  id = "lite-xl-staged-exact-source",
  upstream_base_ref = "d1432ae0736cd9531798b4bc1221835f534cc689",
  tree_digest_algorithm = "fnv1a32-32bit-hex",
  -- Invalidation input: any byte change under these inputs produces a new
  -- tree digest and requires re-deriving every row below.
  tree_digest_inputs = {
    "clients/lite-xl/upstream/init.lua",
    "clients/lite-xl/upstream/server.lua",
    "clients/lite-xl/upstream/util.lua",
    "clients/lite-xl/upstream/diagnostics.lua",
    "clients/lite-xl/upstream/json.lua",
    "clients/lite-xl/upstream/capability_manifest.lua",
  },
}

-- ---------------------------------------------------------------------------
-- Capability rows
-- ---------------------------------------------------------------------------
--
-- value       exact leaf value for static advertised rows (cross-checked
--             against the folded payload by the derivation itself);
-- value_source "static" or "deps.<key>" for dependency-derived leaves;
-- owner       implementation/behavior owner of an implemented row;
-- proof       deterministic test/scenario pinning the implemented row;
-- host_cell   what an actual-host promotion still needs (#9008 fan-in);
-- owner_issue routing target when a row stays unsupported;
-- optional_dependency dependency whose exact installed state decides the
--             advertised value (#11172 rule 3) - never guessed.

capability_manifest.capabilities = {

  -- --- Advertised, implemented -------------------------------------------

  {
    id = "workspace.configuration",
    path = "workspace.configuration",
    advertised = true,
    disposition = "implemented",
    value = true,
    value_source = "static",
    owner = "init.lua workspace/configuration round-trip merges per-server "
      .. "settings before requests (#10653 train)",
    proof = "clients/lite-xl/tests/init_configuration_items_test.lua",
    host_cell = nil,
    invalidation_inputs = {
      "clients/lite-xl/upstream/init.lua",
    },
  },
  {
    id = "textDocument.synchronization.didSave",
    path = "textDocument.synchronization.didSave",
    advertised = true,
    disposition = "implemented",
    value = true,
    value_source = "static",
    owner = "init.lua save_document sends textDocument/didSave through the "
      .. "#11165 conversion authority",
    proof = "clients/lite-xl/tests/init_document_session_test.lua",
    host_cell = nil,
    invalidation_inputs = {
      "clients/lite-xl/upstream/init.lua",
    },
  },
  {
    id = "textDocument.completion.completionItem.snippetSupport",
    path = "textDocument.completion.completionItem.snippetSupport",
    advertised = true,
    disposition = "implemented",
    value_source = "deps.snippet_support",
    optional_dependency = "snippets plugin installed and enabled "
      .. "(config.plugins.lsp.snippets + snippets.execute available)",
    owner = "completion application applies LSP snippets only through the "
      .. "exact installed snippets dependency",
    proof = "clients/lite-xl/tests/init_completion_resolve_test.lua",
    host_cell = nil,
    invalidation_inputs = {
      "clients/lite-xl/upstream/init.lua",
      "installed snippets plugin state",
    },
  },
  {
    id = "textDocument.completion.completionItem.documentationFormat",
    path = "textDocument.completion.completionItem.documentationFormat",
    advertised = true,
    disposition = "implemented",
    value = { "plaintext" },
    value_source = "static",
    owner = "autocomplete item descriptions render plain text",
    proof = "clients/lite-xl/tests/journey_session_test.lua completion path",
    host_cell = nil,
    invalidation_inputs = {
      "clients/lite-xl/upstream/init.lua",
    },
  },
  {
    id = "textDocument.completion.completionItem.insertReplaceSupport",
    path = "textDocument.completion.completionItem.insertReplaceSupport",
    advertised = true,
    disposition = "implemented",
    value = true,
    value_source = "static",
    owner = "init.lua apply_edit resolves InsertReplaceEdit insert/replace "
      .. "ranges",
    proof = "clients/lite-xl/tests/init_completion_resolve_test.lua",
    host_cell = nil,
    invalidation_inputs = {
      "clients/lite-xl/upstream/init.lua",
    },
  },
  {
    id = "textDocument.completion.completionItem.resolveSupport",
    path = "textDocument.completion.completionItem.resolveSupport",
    advertised = true,
    disposition = "implemented",
    value = {
      properties = { "documentation", "detail", "additionalTextEdits" },
    },
    value_source = "static",
    owner = "#12547 pre-apply resolve state machine: resolved fields overlay "
      .. "the original and feed exactly-one validated application",
    proof = "clients/lite-xl/tests/init_completion_resolve_test.lua",
    host_cell = "final bytes of resolved auto-import stay with #10681 "
      .. "(real-host receipt)",
    limitation = "counts only at the #12547 behavior; properties beyond the "
      .. "three consumed fields must not be added without landing their "
      .. "consumption first",
    invalidation_inputs = {
      "clients/lite-xl/upstream/init.lua",
      "#10671 shared validated-edit transaction landing",
    },
  },
  {
    id = "textDocument.completion.completionItemKind.valueSet",
    path = "textDocument.completion.completionItemKind.valueSet",
    advertised = true,
    disposition = "implemented",
    value_source = "deps.completion_item_kinds",
    owner = "server.lua kind list maps CompletionItemKind onto autocomplete "
      .. "rendering",
    proof = "clients/lite-xl/tests/server_initialize_capabilities_test.lua",
    host_cell = nil,
    invalidation_inputs = {
      "clients/lite-xl/upstream/server.lua",
    },
  },
  {
    id = "textDocument.hover.contentFormat",
    path = "textDocument.hover.contentFormat",
    advertised = true,
    disposition = "implemented",
    value = { "markdown", "plaintext" },
    value_source = "static",
    owner = "hover rendering strips markdown to editor text (util.strip_markdown)",
    proof = "clients/lite-xl/tests/journey_session_test.lua hover path",
    host_cell = nil,
    invalidation_inputs = {
      "clients/lite-xl/upstream/init.lua",
      "clients/lite-xl/upstream/util.lua",
    },
  },
  {
    id = "textDocument.signatureHelp.signatureInformation.documentationFormat",
    path = "textDocument.signatureHelp.signatureInformation.documentationFormat",
    advertised = true,
    disposition = "implemented",
    value = { "plaintext" },
    value_source = "static",
    owner = "signature help renders documentation as plain text",
    proof = "clients/lite-xl/tests/journey_session_test.lua signature path",
    host_cell = nil,
    invalidation_inputs = {
      "clients/lite-xl/upstream/init.lua",
    },
  },
  {
    id = "textDocument.documentSymbol.symbolKind.valueSet",
    path = "textDocument.documentSymbol.symbolKind.valueSet",
    advertised = true,
    disposition = "implemented",
    value_source = "deps.symbol_kinds",
    owner = "server.lua symbol kind list feeds the symbol results view",
    proof = "clients/lite-xl/tests/server_initialize_capabilities_test.lua",
    host_cell = nil,
    invalidation_inputs = {
      "clients/lite-xl/upstream/server.lua",
    },
  },
  {
    id = "textDocument.publishDiagnostics.versionSupport",
    path = "textDocument.publishDiagnostics.versionSupport",
    advertised = true,
    disposition = "implemented",
    value = true,
    value_source = "static",
    owner = "diagnostics.lua version-exact admission: stale/future versions "
      .. "are rejected, unversioned publications stay explicitly not_proven",
    proof = "clients/lite-xl/tests/diagnostics_currentness_test.lua",
    host_cell = nil,
    limitation = "the historical drift (advertising versions while storage "
      .. "ignored them) is closed by the landed currentness machinery",
    invalidation_inputs = {
      "clients/lite-xl/upstream/diagnostics.lua",
    },
  },
  {
    id = "textDocument.publishDiagnostics.dataSupport",
    path = "textDocument.publishDiagnostics.dataSupport",
    advertised = true,
    disposition = "implemented",
    value = false,
    value_source = "static",
    owner = "explicit protocol opt-out pinned: the client keeps no use for "
      .. "diagnostic.data",
    proof = "clients/lite-xl/tests/server_initialize_capabilities_test.lua",
    host_cell = nil,
    invalidation_inputs = {
      "clients/lite-xl/upstream/server.lua",
    },
  },
  {
    id = "window.showDocument.support",
    path = "window.showDocument.support",
    advertised = true,
    disposition = "implemented",
    value = true,
    value_source = "static",
    owner = "#10873/#11162 outcome-truthful sequence: URIs are classified "
      .. "before opening and success is never reported before user outcome",
    proof = "clients/lite-xl/tests/init_show_document_outcome_test.lua; "
      .. "clients/lite-xl/tests/util_show_document_test.lua",
    host_cell = "real-host open/selection receipts stay with #10673/#9008",
    invalidation_inputs = {
      "clients/lite-xl/upstream/init.lua",
      "clients/lite-xl/upstream/util.lua",
    },
  },
  {
    id = "general.positionEncodings",
    path = "general.positionEncodings",
    advertised = true,
    disposition = "implemented",
    value_source = "deps.position_encoding_list",
    owner = "position encoding negotiation refuses non-utf-16 servers",
    proof = "clients/lite-xl/tests/server_initialize_capabilities_test.lua",
    host_cell = nil,
    invalidation_inputs = {
      "clients/lite-xl/upstream/server.lua",
      "clients/lite-xl/upstream/init.lua",
    },
  },

  -- --- Explicitly NOT advertised -----------------------------------------

  {
    id = "textDocument.publishDiagnostics.relatedInformation",
    path = "textDocument.publishDiagnostics.relatedInformation",
    advertised = false,
    disposition = "unsupported",
    owner_issue = "none yet: diagnostics rendering has no consumer for "
      .. "related locations; route through #11124/#11128 follow-ups",
    limitation = "stored in diagnostic evidence records but never surfaced; "
      .. "advertisement was removed by #11172 because nothing consumed it",
    invalidation_inputs = {
      "clients/lite-xl/upstream/diagnostics.lua",
      "clients/lite-xl/upstream/init.lua",
    },
  },
  {
    id = "textDocument.publishDiagnostics.tagSupport",
    path = "textDocument.publishDiagnostics.tagSupport",
    advertised = false,
    disposition = "unsupported",
    owner_issue = "none yet: no renderer styles Unnecessary/Deprecated tags; "
      .. "route through #11124/#11128 follow-ups",
    limitation = "advertisement was removed by #11172; lintplus tag kinds "
      .. "are local diagnostics, unrelated to server-pushed tags",
    invalidation_inputs = {
      "clients/lite-xl/upstream/diagnostics.lua",
    },
  },
  {
    id = "textDocument.publishDiagnostics.codeDescriptionSupport",
    path = "textDocument.publishDiagnostics.codeDescriptionSupport",
    advertised = false,
    disposition = "unsupported",
    owner_issue = "none yet: description.href has no opener/consumer",
    limitation = "advertisement was removed by #11172",
    invalidation_inputs = {
      "clients/lite-xl/upstream/diagnostics.lua",
    },
  },
  {
    id = "workspace.didChangeWatchedFiles.dynamicRegistration",
    path = "workspace.didChangeWatchedFiles.dynamicRegistration",
    advertised = false,
    disposition = "unsupported",
    owner_issue = "#9001 (registration/watcher lifecycle) with #10785",
    limitation = "must stay absent until register, unregister, ownership, "
      .. "cleanup and response obligations all work (#11172 rule 4)",
    invalidation_inputs = {
      "#9001 landing",
      "#10785 landing",
    },
  },
  {
    id = "workspace.workspaceFolders.multi_root",
    path = "initialize params workspaceFolders",
    advertised = false,
    disposition = "configuration_only",
    owner_issue = "none yet: multi-root workspaces",
    limitation = "the payload factually sends exactly one workspace folder "
      .. "derived from the root URI; multi-root operation is not supported",
    invalidation_inputs = {
      "clients/lite-xl/upstream/server.lua",
    },
  },
  {
    id = "workspace.applyEdit",
    path = "workspace.applyEdit",
    advertised = false,
    disposition = "unsupported",
    owner_issue = "#10671 (shared validated-edit transaction) consuming "
      .. "#8986 application work",
    limitation = "no workspace/applyEdit handler exists, so the client must "
      .. "not invite server-driven edits",
    invalidation_inputs = {
      "#10671 landing",
      "#8986 landing",
    },
  },
  {
    id = "rename.client_application",
    path = "client rename behavior (command lsp:rename-symbol)",
    advertised = false,
    disposition = "unsupported",
    owner_issue = "#8986",
    limitation = "returned WorkspaceEdits are logged today; prepareRename "
      .. "support is equally absent until application lands",
    invalidation_inputs = {
      "#8986 landing",
      "clients/lite-xl/upstream/init.lua",
    },
  },
  {
    id = "callHierarchy.result_consumption",
    path = "client call-hierarchy behavior (command lsp:view-call-hierarchy)",
    advertised = false,
    disposition = "unsupported",
    owner_issue = "#10719",
    limitation = "prepareCallHierarchy results are discarded; the command "
      .. "stays projection-gated off until consumption lands",
    invalidation_inputs = {
      "#10719 landing",
      "clients/lite-xl/upstream/init.lua",
    },
  },
  {
    id = "textDocument.semanticTokens",
    path = "textDocument.semanticTokens",
    advertised = false,
    disposition = "unsupported",
    owner_issue = "advanced-feature admission via #10767",
    limitation = "a server advertising semanticTokensProvider must never "
      .. "make this client claim or expose token support",
    invalidation_inputs = {
      "#10767 admissions",
    },
  },
  {
    id = "textDocument.codeAction",
    path = "textDocument.codeAction",
    advertised = false,
    disposition = "unsupported",
    owner_issue = "advanced-feature admission via #10767",
    limitation = "no codeAction capability, literal support, or command "
      .. "path exists; server capability alone creates no affordance",
    invalidation_inputs = {
      "#10767 admissions",
    },
  },
  {
    id = "textDocument.inlayHint",
    path = "textDocument.inlayHint",
    advertised = false,
    disposition = "unsupported",
    owner_issue = "advanced-feature admission via #10767",
    limitation = "no inlay hint surface exists",
    invalidation_inputs = {
      "#10767 admissions",
    },
  },
  {
    id = "textDocument.codeLens",
    path = "textDocument.codeLens",
    advertised = false,
    disposition = "unsupported",
    owner_issue = "advanced-feature admission via #10767",
    limitation = "no code lens surface exists",
    invalidation_inputs = {
      "#10767 admissions",
    },
  },
  {
    id = "textDocument.diagnostic.pull",
    path = "textDocument.diagnostic",
    advertised = false,
    disposition = "unsupported",
    owner_issue = "advanced-feature admission via #10767",
    limitation = "pull diagnostics stay absent while push diagnostics carry "
      .. "the publication truth",
    invalidation_inputs = {
      "#10767 admissions",
    },
  },
  {
    id = "window.workDoneProgress",
    path = "window.workDoneProgress",
    advertised = false,
    disposition = "unsupported",
    owner_issue = "none yet: progress/refresh surfaces",
    limitation = "work-done progress and refresh requests have no client "
      .. "consumer; refresh obligations cannot be met",
    invalidation_inputs = {
      "clients/lite-xl/upstream/init.lua",
    },
  },

  -- --- Request-path rows covered through commands --------------------------

  {
    id = "commands.goto_symbol.request_paths",
    path = "commands goto-definition / goto-implementation / find-references",
    advertised = false,
    disposition = "implemented",
    owner = "goto_symbol consumes location lists through listbox with "
      .. "definition/declaration/typeDefinition/implementation/reference "
      .. "provider gating (#10660 owns server ownership determinism)",
    proof = "clients/lite-xl/tests/journey_session_test.lua navigation paths",
    host_cell = nil,
    limitation = "advertised=false records that no client CAPABILITY leaf "
      .. "exists for these request paths; availability lives in command "
      .. "rows",
    invalidation_inputs = {
      "clients/lite-xl/upstream/init.lua",
      "#10660 landing",
    },
  },
  {
    id = "commands.formatting.application",
    path = "command lsp:format-document",
    advertised = false,
    disposition = "implemented",
    owner = "formatting response edits apply through apply_edit behind the "
      .. "#11108 subject admission",
    proof = "clients/lite-xl/tests/init_request_currentness_test.lua",
    host_cell = nil,
    limitation = "rangeFormatting is not exposed",
    invalidation_inputs = {
      "clients/lite-xl/upstream/init.lua",
    },
  },
}

-- ---------------------------------------------------------------------------
-- Command/action rows
-- ---------------------------------------------------------------------------
--
-- request                outbound method owned by the command (nil for
--                        client-local actions);
-- server_capability      server capability key that must be present (nil
--                        for client-local actions);
-- consumer_disposition   "implemented" only when the client consumes the
--                        result or applies the effect;
-- unsupported_message    the one explicit message shown when the consumer
--                        is missing (#11172 command truth).

capability_manifest.commands = {
  ["lsp:complete"] = {
    request = "textDocument/completion",
    server_capability = "completionProvider",
    consumer_disposition = "implemented",
  },
  ["lsp:goto-definition"] = {
    request = "textDocument/definition",
    server_capability = "definitionProvider",
    consumer_disposition = "implemented",
  },
  ["lsp:goto-implementation"] = {
    request = "textDocument/implementation",
    server_capability = "implementationProvider",
    consumer_disposition = "implemented",
  },
  ["lsp:show-signature"] = {
    request = "textDocument/signatureHelp",
    server_capability = "signatureHelpProvider",
    consumer_disposition = "implemented",
  },
  ["lsp:show-symbol-info"] = {
    request = "textDocument/hover",
    server_capability = "hoverProvider",
    consumer_disposition = "implemented",
  },
  ["lsp:show-symbol-info-in-tab"] = {
    request = "textDocument/hover",
    server_capability = "hoverProvider",
    consumer_disposition = "implemented",
  },
  ["lsp:view-call-hierarchy"] = {
    request = "textDocument/prepareCallHierarchy",
    server_capability = "callHierarchyProvider",
    consumer_disposition = "unsupported",
    unsupported_message = "[LSP] Call hierarchy is not available: this "
      .. "client cannot consume hierarchy results yet (#10719).",
  },
  ["lsp:view-document-symbols"] = {
    request = "textDocument/documentSymbol",
    server_capability = "documentSymbolProvider",
    consumer_disposition = "implemented",
  },
  ["lsp:format-document"] = {
    request = "textDocument/formatting",
    server_capability = "documentFormattingProvider",
    consumer_disposition = "implemented",
  },
  ["lsp:view-document-diagnostics"] = {
    request = nil,
    server_capability = nil,
    consumer_disposition = "implemented",
  },
  ["lsp:rename-symbol"] = {
    request = "textDocument/rename",
    server_capability = "renameProvider",
    consumer_disposition = "unsupported",
    unsupported_message = "[LSP] Rename is not available: this client "
      .. "cannot apply returned WorkspaceEdits yet (#8986).",
  },
  ["lsp:find-references"] = {
    request = "textDocument/references",
    server_capability = "referencesProvider",
    consumer_disposition = "implemented",
  },
  ["lsp:find-workspace-symbol"] = {
    request = "workspace/symbol",
    server_capability = "workspaceSymbolProvider",
    consumer_disposition = "implemented",
  },
  ["lsp:view-all-diagnostics"] = {
    request = nil,
    server_capability = nil,
    consumer_disposition = "implemented",
  },
  ["lsp:toggle-diagnostics"] = {
    request = nil,
    server_capability = nil,
    consumer_disposition = "implemented",
  },
  ["lsp:stop-servers"] = {
    request = nil,
    server_capability = nil,
    consumer_disposition = "implemented",
  },
  ["lsp:start-servers"] = {
    request = nil,
    server_capability = nil,
    consumer_disposition = "implemented",
  },
  ["lsp:restart-servers"] = {
    request = nil,
    server_capability = nil,
    consumer_disposition = "implemented",
  },
}

-- ---------------------------------------------------------------------------
-- Pure helpers
-- ---------------------------------------------------------------------------

local function copy_value(value)
  if type(value) ~= "table" then return value end
  local copy = {}
  for key, item in pairs(value) do copy[key] = copy_value(item) end
  return copy
end

local function set_path(table, dotted_path, value)
  local segments = {}
  for segment in dotted_path:gmatch("[^.]+") do
    segments[#segments + 1] = segment
  end
  assert(#segments > 0, "empty capability path")
  local node = table
  for i = 1, #segments - 1 do
    local key = segments[i]
    if type(node[key]) ~= "table" then node[key] = {} end
    node = node[key]
  end
  node[segments[#segments]] = value
end

---Fold every advertised row into the exact default client-capability
---payload (#11172). Fails closed: an advertised row without an implemented
---disposition, without a known value source, or with a dependency state
---that supplies nothing can never reach the wire.
---@param deps table {snippet_support, completion_item_kinds, symbol_kinds,
---                    position_encoding_list}
---@return table capabilities
function capability_manifest.client_capabilities(deps)
  deps = deps or {}
  local out = {}
  for _, row in ipairs(capability_manifest.capabilities) do
    if row.advertised then
      if row.disposition ~= "implemented" then
        error("capability_manifest: advertised row '" .. tostring(row.id)
          .. "' lacks an implemented disposition")
      end
      if row.value_source == "static" then
        if row.value == nil then
          error("capability_manifest: static row '" .. tostring(row.id)
            .. "' carries no exact value")
        end
        set_path(out, row.path, copy_value(row.value))
      elseif type(row.value_source) == "string"
        and row.value_source:find("^deps%.", 1)
      then
        local key = row.value_source:sub(#"deps." + 1)
        local produced = deps[key]
        if produced == nil then
          error("capability_manifest: no value producer for row '"
            .. tostring(row.id) .. "' (missing deps." .. key .. ")")
        end
        set_path(out, row.path, copy_value(produced))
      else
        error("capability_manifest: row '" .. tostring(row.id)
          .. "' has no static value or dependency producer")
      end
    end
  end
  return out
end

---Pure availability predicate for one command against one server's
---capabilities (#11172 command truth): a server capability alone never
---grants availability - the client-consumer disposition decides first.
---@param command_id string
---@param server_capabilities table|nil
---@return boolean available
---@return string|nil reason stable token when unavailable
function capability_manifest.command_availability(
    command_id, server_capabilities)
  local row = capability_manifest.commands[command_id]
  if not row then
    return false, "unknown_command"
  end
  if row.consumer_disposition ~= "implemented" then
    return false, "client_consumer_unsupported"
  end
  if row.server_capability == nil then
    return true, nil
  end
  local caps = server_capabilities or {}
  if not caps[row.server_capability] then
    return false, "server_capability_absent"
  end
  return true, nil
end

---Bounded scan of user custom_capabilities over the manifest rows (#11172
---rule 6): overrides stay user freedom on the wire, but claims over known
---unimplemented rows are reported so they can warn instead of silently
---becoming evidence. Unknown sections are outside the scan.
---@param custom table|nil
---@return table claims array of {path, disposition} sorted by path
function capability_manifest.unsupported_custom_claims(custom)
  local claims = {}

  local rows_by_path = {}
  for _, row in ipairs(capability_manifest.capabilities) do
    if row.disposition ~= "implemented" then
      rows_by_path[row.path] = row
    end
  end

  local function walk(node, prefix, depth)
    if depth > 8 or type(node) ~= "table" then return end
    for key, value in pairs(node) do
      local path = prefix == "" and tostring(key) or prefix .. "." .. tostring(key)
      local row = rows_by_path[path]
      if row then
        -- An explicit scalar false DECLINES the capability; only a true or
        -- structured value claims support (#12599 review).
        if value ~= false then
          claims[#claims + 1] = { path = path, disposition = row.disposition }
        end
      elseif type(value) == "table" then
        walk(value, path, depth + 1)
      end
    end
  end

  walk(custom, "", 1)

  table.sort(claims, function(a, b) return a.path < b.path end)
  return claims
end

---Deterministic canonical report of the whole matrix. Two runs and two
---independent module loads produce byte-identical output for the same tree
---digest (second-run generation/check must produce no diff).
---@param tree_digest string content identity of the exact composed tree
---@return string report
function capability_manifest.projection_report(tree_digest)
  local lines = {}

  local profile = capability_manifest.profile
  lines[#lines + 1] = "profile=" .. profile.id
  lines[#lines + 1] = "upstream_base_ref=" .. profile.upstream_base_ref
  lines[#lines + 1] = "tree_digest_algorithm=" .. profile.tree_digest_algorithm
  lines[#lines + 1] = "tree_digest=" .. tostring(tree_digest)

  local sorted_rows = {}
  for _, row in ipairs(capability_manifest.capabilities) do
    sorted_rows[#sorted_rows + 1] = row
  end
  table.sort(sorted_rows, function(a, b) return a.id < b.id end)

  for _, row in ipairs(sorted_rows) do
    lines[#lines + 1] =
      "capability=" .. row.id
      .. "|path=" .. row.path
      .. "|advertised=" .. tostring(row.advertised)
      .. "|disposition=" .. row.disposition
      .. "|owner=" .. tostring(row.owner or row.owner_issue or "")
      .. "|proof=" .. tostring(row.proof or "")
      .. "|host_cell=" .. tostring(row.host_cell or "")
      .. "|limitation=" .. tostring(row.limitation or "")
  end

  local sorted_commands = {}
  for id in pairs(capability_manifest.commands) do
    sorted_commands[#sorted_commands + 1] = id
  end
  table.sort(sorted_commands)

  for _, id in ipairs(sorted_commands) do
    local row = capability_manifest.commands[id]
    lines[#lines + 1] =
      "command=" .. id
      .. "|request=" .. tostring(row.request or "")
      .. "|server_capability=" .. tostring(row.server_capability or "")
      .. "|consumer_disposition=" .. row.consumer_disposition
  end

  return table.concat(lines, "\n")
end

return capability_manifest

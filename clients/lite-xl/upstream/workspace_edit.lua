-- Reusable WorkspaceEdit.changes application for the staged lite-xl-lsp client
-- (#8986). Multi-file URI resolution and fail-closed transaction planning live
-- here; per-document range validation, ordering, and mutation delegate to the
-- caller's apply_text_edit hook (the same path completion/formatting use today,
-- until #10671 factors a shared same-document transaction).

local workspace_edit = {}

local function is_null(json, value)
  if value == nil then return true end
  if json and json.is_null and json.is_null(value) then return true end
  return false
end

local function position_key(range, which)
  local pos = range[which]
  if type(pos) ~= "table" then return nil end
  if type(pos.line) ~= "number" or type(pos.character) ~= "number" then
    return nil
  end
  return pos.line * 1000000 + pos.character
end

local function range_start_key(range)
  return position_key(range, "start")
end

local function range_end_key(range)
  return position_key(range, "end")
end

local function sort_edits_descending(edits)
  table.sort(edits, function(a, b)
    local ra, rb = a.range, b.range
    local sa, sb = range_start_key(ra), range_start_key(rb)
    if sa == nil or sb == nil then return false end
    if sa ~= sb then return sa > sb end
    local ea, eb = range_end_key(ra), range_end_key(rb)
    if ea == nil or eb == nil then return false end
    return ea > eb
  end)
end

local function ranges_overlap_utf16(a, b)
  local a_start, a_end = range_start_key(a), range_end_key(a)
  local b_start, b_end = range_start_key(b), range_end_key(b)
  if not a_start or not a_end or not b_start or not b_end then
    return true
  end
  if a_end <= b_start or b_end <= a_start then return false end
  return true
end

local function edit_has_unsupported_shape(edit)
  if type(edit) ~= "table" then return true end
  if edit.snippet or edit.annotationIdChange then return true end
  if edit.insert or edit.replace then return false end
  if type(edit.range) ~= "table" then return true end
  if type(edit.newText) ~= "string" then return true end
  return false
end

local function validate_edit_bounds(doc, util, server, edit)
  if edit_has_unsupported_shape(edit) then
    return false, "unsupported_text_edit_shape"
  end
  local encoding = server.capabilities.positionEncoding
  if encoding and encoding ~= "utf-16" then
    return false, "unsupported_position_encoding"
  end
  local line1, col1, line2, col2 = util.toselection(edit.range, doc)
  if not line1 or line1 < 1 or line2 < 1 or line2 < line1 then
    return false, "range_out_of_bounds"
  end
  if line2 > #doc.lines then
    return false, "range_out_of_bounds"
  end
  for line = line1, line2 do
    local text = doc.lines[line]
    if not text then return false, "range_out_of_bounds" end
    local limit = #text
    if text:sub(-1) ~= "\n" then limit = limit + 1 end
    if line == line1 and col1 > limit then return false, "range_out_of_bounds" end
    if line == line2 and col2 > limit then return false, "range_out_of_bounds" end
  end
  return true
end

local function count_change_edits(changes)
  local total = 0
  for _, edits in pairs(changes) do
    if type(edits) == "table" then
      total = total + #edits
    end
  end
  return total
end

local function uri_to_doc(uri, deps)
  local doc = deps.find_open_doc(uri)
  if doc then return doc end
  local path, reason = deps.util.uri_to_path(uri)
  if not path then return nil, reason or "unconvertible_uri" end
  return deps.open_doc_for_path(path)
end

---Apply a WorkspaceEdit that uses the legacy `changes` map.
---@param server table
---@param result table|nil JSON-decoded WorkspaceEdit or null sentinel
---@param deps table hooks: json, util, log, find_open_doc, open_doc_for_path, apply_text_edit
---@return string outcome applied|no_change|unsupported|failed
---@return string|nil detail
function workspace_edit.apply_changes(server, result, deps)
  if is_null(deps.json, result) then
    return "no_change", "rename_unavailable"
  end
  if type(result) ~= "table" then
    return "failed", "invalid_workspace_edit"
  end

  if result.documentChanges ~= nil and not is_null(deps.json, result.documentChanges) then
    return "unsupported", "documentChanges is not supported"
  end
  if result.changeAnnotations ~= nil and not is_null(deps.json, result.changeAnnotations) then
    return "unsupported", "change annotations are not supported"
  end

  local changes = result.changes
  if changes == nil or is_null(deps.json, changes) then
    return "no_change", "empty_workspace_edit"
  end
  if type(changes) ~= "table" or count_change_edits(changes) == 0 then
    return "no_change", "empty_workspace_edit"
  end

  local planned = {}
  for uri, edits in pairs(changes) do
    if type(uri) ~= "string" or type(edits) ~= "table" then
      return "failed", "invalid_changes_entry"
    end
    local doc, doc_err = uri_to_doc(uri, deps)
    if not doc then
      return "failed", doc_err or "missing_target_document"
    end
    local snapshot = {}
    for i, line in ipairs(doc.lines) do snapshot[i] = line end
    local copy = {}
    for i = 1, #edits do copy[i] = edits[i] end
    sort_edits_descending(copy)
    for i = 1, #copy do
      local ok_bounds, reason = validate_edit_bounds(doc, deps.util, server, copy[i])
      if not ok_bounds then
        return "failed", reason
      end
    end
    for i = 1, #copy do
      for j = i + 1, #copy do
        if ranges_overlap_utf16(copy[i].range, copy[j].range) then
          return "failed", "overlapping_edits"
        end
      end
    end
    planned[#planned + 1] = {
      uri = uri,
      doc = doc,
      snapshot = snapshot,
      edits = copy,
    }
  end

  table.sort(planned, function(a, b) return a.uri < b.uri end)

  local applied_entries = {}
  for _, entry in ipairs(planned) do
    for _, edit in ipairs(entry.edits) do
      local applied = deps.apply_text_edit(server, entry.doc, edit)
      if not applied then
        for _, prior in ipairs(applied_entries) do
          for i, line in ipairs(prior.snapshot) do
            prior.doc.lines[i] = line
          end
        end
        for i, line in ipairs(entry.snapshot) do
          entry.doc.lines[i] = line
        end
        return "failed", "apply_text_edit_rejected"
      end
    end
    applied_entries[#applied_entries + 1] = entry
  end

  return "applied", nil
end

return workspace_edit

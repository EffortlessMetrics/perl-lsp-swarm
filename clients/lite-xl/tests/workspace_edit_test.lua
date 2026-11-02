-- Deterministic tests for WorkspaceEdit.changes application (#8986).
--
-- Run:
--   lua clients/lite-xl/tests/workspace_edit_test.lua
--     [path-to-workspace-edit-module] [path-to-util-module]

local workspace_edit_path = arg and arg[1] or nil
local util_module_path = arg and arg[2] or nil

if not workspace_edit_path then
  local info = debug.getinfo(1, "S").source:sub(2)
  local dir = info:match("^(.*)[/\\]") or "."
  workspace_edit_path = dir .. "/../upstream/workspace_edit.lua"
end
if not util_module_path then
  local info = debug.getinfo(1, "S").source:sub(2)
  local dir = info:match("^(.*)[/\\]") or "."
  util_module_path = dir .. "/../upstream/util.lua"
end

local here = debug.getinfo(1, "S").source:sub(2):match("^(.*)[/\\]") or "."

package.preload["core"] = function()
  return {
    docs = {},
    log = function() end,
    error = function() end,
  }
end
package.preload["plugins.lsp.json"] = function()
  return dofile(here .. "/../upstream/json.lua")
end
package.preload["core.common"] = function()
  return {
    clamp = function(value, low, high)
      if value < low then return low elseif value > high then return high end
      return value
    end,
  }
end
package.preload["core.config"] = function() return { plugins = {} } end
package.preload["process"] = function() return {} end

utf8extra = {
  len = function(s) return utf8.len(s) or #s end,
  next = function(s, i)
    local pos
    if not i then
      pos = 1
    else
      local b = s:byte(i)
      local width = 1
      if b >= 0xF0 then width = 4 elseif b >= 0xE0 then width = 3
      elseif b >= 0xC0 then width = 2 end
      pos = i + width
    end
    if pos > #s then return nil end
    return pos, utf8.codepoint(s, pos)
  end,
}
PLATFORM = "Windows"

local json = require "plugins.lsp.json"
local util = dofile(util_module_path)
local workspace_edit = dofile(workspace_edit_path)

local passed, failed = 0, 0
local function ok(condition, message)
  if condition then
    passed = passed + 1
  else
    failed = failed + 1
    print("FAIL: " .. message)
  end
end

local function make_doc(lines)
  return { lines = lines, filename = "C:/proj/x.pl", abs_filename = "C:/proj/x.pl" }
end

local function doc_text(doc)
  return table.concat(doc.lines)
end

local server = {
  capabilities = { positionEncoding = "utf-16" },
}

local function apply_via_remove_insert(doc, text_edit)
  local line1, col1, line2, col2 = util.toselection(text_edit.range, doc)
  if line1 ~= line2 then
    return false
  end
  local line = doc.lines[line1] or ""
  local head = line:sub(1, col1 - 1)
  local tail = line:sub(col2)
  doc.lines[line1] = head .. text_edit.newText .. tail
  return true
end

local function run_apply(doc, result, opts)
  opts = opts or {}
  local uri = opts.uri or "file:///C:/proj/x.pl"
  return workspace_edit.apply_changes(server, result, {
    json = json,
    util = util,
    log = function() end,
    find_open_doc = function(request_uri)
      if request_uri == uri then return doc end
      if opts.other_uri and request_uri == opts.other_uri then
        return opts.other_doc
      end
      return nil
    end,
    open_doc_for_path = function(path)
      if opts.missing_files and opts.missing_files[path] then
        return nil, "missing_target_file"
      end
      if opts.other_path and path == opts.other_path then
        return opts.other_doc
      end
      return doc
    end,
    apply_text_edit = function(_, target_doc, edit)
      return apply_via_remove_insert(target_doc, edit)
    end,
  })
end

-- one-file rename
do
  local doc = make_doc({ "old_name\n" })
  local outcome = run_apply(doc, {
    changes = {
      ["file:///C:/proj/x.pl"] = {
        {
          range = {
            start = { line = 0, character = 0 },
            ["end"] = { line = 0, character = 8 },
          },
          newText = "new_name",
        },
      },
    },
  })
  ok(outcome == "applied", "single-file rename applies")
  ok(doc_text(doc) == "new_name\n", "single-file bytes updated")
end

-- multi-file changes map
do
  local doc_a = make_doc({ "aaa\n" })
  local doc_b = make_doc({ "bbb\n" })
  doc_b.filename = "C:/proj/b.pl"
  doc_b.abs_filename = "C:/proj/b.pl"
  local outcome = workspace_edit.apply_changes(server, {
    changes = {
      ["file:///C:/proj/x.pl"] = {
        {
          range = {
            start = { line = 0, character = 0 },
            ["end"] = { line = 0, character = 3 },
          },
          newText = "AAA",
        },
      },
      ["file:///C:/proj/b.pl"] = {
        {
          range = {
            start = { line = 0, character = 0 },
            ["end"] = { line = 0, character = 3 },
          },
          newText = "BBB",
        },
      },
    },
  }, {
    json = json,
    util = util,
    log = function() end,
    find_open_doc = function(uri)
      if uri == "file:///C:/proj/x.pl" then return doc_a end
      if uri == "file:///C:/proj/b.pl" then return doc_b end
      return nil
    end,
    open_doc_for_path = function() return nil, "missing" end,
    apply_text_edit = function(_, target_doc, edit)
      return apply_via_remove_insert(target_doc, edit)
    end,
  })
  ok(outcome == "applied", "multi-file rename applies all targets")
  ok(doc_text(doc_a) == "AAA\n" and doc_text(doc_b) == "BBB\n",
    "both files updated")
end

-- reverse-order requirement (forward application would break the second edit)
do
  local doc = make_doc({ "abcdef\n" })
  local outcome = run_apply(doc, {
    changes = {
      ["file:///C:/proj/x.pl"] = {
        {
          range = {
            start = { line = 0, character = 0 },
            ["end"] = { line = 0, character = 1 },
          },
          newText = "X",
        },
        {
          range = {
            start = { line = 0, character = 5 },
            ["end"] = { line = 0, character = 6 },
          },
          newText = "Z",
        },
      },
    },
  })
  ok(outcome == "applied", "multiple edits in one file apply")
  ok(doc_text(doc) == "XbcdeZ\n", "edits applied in descending range order")
end

-- non-BMP codepoint before edit range (UTF-16 width) — uses the same
-- utf8extra walk as production util.toselection.
do
  local doc = make_doc({ "\240\159\166\128ab\n" })
  local outcome = run_apply(doc, {
    changes = {
      ["file:///C:/proj/x.pl"] = {
        {
          range = {
            start = { line = 0, character = 2 },
            ["end"] = { line = 0, character = 4 },
          },
          newText = "AB",
        },
      },
    },
  })
  ok(outcome == "applied", "non-BMP prefix rename applies")
  ok(doc_text(doc):find("AB", 1, true) ~= nil, "non-BMP range maps after surrogate pair")
end

-- CRLF bytes preserved in lines model
do
  local doc = make_doc({ "a\r\n" })
  local outcome = run_apply(doc, {
    changes = {
      ["file:///C:/proj/x.pl"] = {
        {
          range = {
            start = { line = 0, character = 0 },
            ["end"] = { line = 0, character = 1 },
          },
          newText = "b",
        },
      },
    },
  })
  ok(outcome == "applied", "CRLF file accepts edit")
  ok(doc.lines[1] == "b\r\n", "CRLF line ending preserved")
end

-- overlapping edits fail closed
do
  local doc = make_doc({ "abcdef\n" })
  local before = doc_text(doc)
  local outcome, detail = run_apply(doc, {
    changes = {
      ["file:///C:/proj/x.pl"] = {
        {
          range = {
            start = { line = 0, character = 1 },
            ["end"] = { line = 0, character = 4 },
          },
          newText = "XX",
        },
        {
          range = {
            start = { line = 0, character = 3 },
            ["end"] = { line = 0, character = 5 },
          },
          newText = "YY",
        },
      },
    },
  })
  ok(outcome == "failed" and detail == "overlapping_edits",
    "overlapping edits are rejected")
  ok(doc_text(doc) == before, "overlapping edits leave bytes unchanged")
end

-- documentChanges unsupported
do
  local doc = make_doc({ "x\n" })
  local before = doc_text(doc)
  local outcome, detail = run_apply(doc, {
    documentChanges = {
      {
        textDocument = { uri = "file:///C:/proj/x.pl" },
        edits = {},
      },
    },
  })
  ok(outcome == "unsupported", "documentChanges is unsupported")
  ok(doc_text(doc) == before, "documentChanges performs no mutation")
end

-- null rename result
do
  local doc = make_doc({ "x\n" })
  local outcome, detail = run_apply(doc, json.null)
  ok(outcome == "no_change" and detail == "rename_unavailable",
    "null rename is a distinct no_change outcome")
end

-- missing target file
do
  local doc = make_doc({ "x\n" })
  local outcome, detail = workspace_edit.apply_changes(server, {
    changes = {
      ["file:///C:/proj/missing.pl"] = {
        {
          range = {
            start = { line = 0, character = 0 },
            ["end"] = { line = 0, character = 1 },
          },
          newText = "y",
        },
      },
    },
  }, {
    json = json,
    util = util,
    log = function() end,
    find_open_doc = function() return nil end,
    open_doc_for_path = function() return nil, "missing_target_file" end,
    apply_text_edit = function()
      return true
    end,
  })
  ok(outcome == "failed" and detail == "missing_target_file",
    "missing target fails closed")
end

print(string.format("\n%d passed, %d failed", passed, failed))
if failed > 0 then os.exit(1) end

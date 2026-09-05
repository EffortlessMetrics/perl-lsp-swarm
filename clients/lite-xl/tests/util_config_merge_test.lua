-- Deterministic focused tests for the typed configuration merge contract in
-- clients/lite-xl/upstream/util.lua (#11143).
--
-- Run:
--   lua clients/lite-xl/tests/util_config_merge_test.lua [path-to-util-module]
-- Default module path is ../upstream/util.lua relative to this file. The
-- staged json.lua codec is loaded from its default relative location.
--
-- Contract under test (#11143, consuming the #11136/#11980 JSON value model):
--   object + object -> recursive merge by string key
--   array + anything / anything + array -> later side replaces atomically
--   explicit later empty array clears an inherited array
--   scalars/false/0/""/explicit null replace exactly
--   type changes replace with the later typed value, never merge shapes
--   inputs are never mutated; results are deterministic
--   sparse/mixed settings containers fail through the codec constructors
--   (json.array/json.object), never through ad-hoc merge guessing
--
-- Red-first baseline: run against the PRISTINE upstream util.lua @ d1432ae
-- (blob 588c101aa97ef0d112926aac316e7a95a52a6994). Its deep_merge recurses
-- into EVERY Lua table including arrays, so these cases MUST fail there:
-- shorter later arrays retain inherited tails, an empty array cannot clear,
-- tagged array/object identity is ignored (empty object + empty array does
-- not answer []), type changes numerically merge incompatible shapes, and
-- sparse/mixed containers silently produce garbage instead of failing.
--
-- Mutation falsifier of the PATCHED module (verified): restore recursive
-- numeric-key merging inside the merge (drop the array-replacement branch);
-- atomic_replace_shorter_array, empty_array_clears_inherited,
-- nested_object_recurses_child_arrays_replace, and the realistic
-- perl.include_paths fixture immediately fail with surviving stale tails.
--
-- No framework: plain soft asserts, one process, deterministic, exit code
-- carries the result. Compatible with the Lite XL Lua runtime family (5.4).

local util_module_path = arg and arg[1] or nil

if not util_module_path then
  local info = debug.getinfo(1, "S").source:sub(2)
  local dir = info:match("^(.*)[/\\]") or "."
  util_module_path = dir .. "/../upstream/util.lua"
end

local here = debug.getinfo(1, "S").source:sub(2):match("^(.*)[/\\]") or "."

-- ---------------------------------------------------------------------------
-- Lite XL runtime fakes: only what loading the exact staged module requires.
-- ---------------------------------------------------------------------------

package.preload["plugins.lsp.json"] = function()
  return dofile(here .. "/../upstream/json.lua")
end

package.preload["core.common"] = function()
  return {}
end

package.preload["core.config"] = function()
  return { plugins = { lsp = {} } }
end

package.preload["core"] = function()
  return { docs = {}, log = function() end }
end

package.preload["process"] = function()
  return { start = function() return nil end }
end

PLATFORM = "Windows"

-- Exact staged upstream source under test.
local util = dofile(util_module_path)

-- Same codec instance the staged module required, so tagged identities made
-- here are recognized by production code paths.
local json = require "plugins.lsp.json"

-- ---------------------------------------------------------------------------
-- Soft assertion collector and helpers
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

---Structural equality that treats json.null as one distinct value and does
---not depend on key iteration order.
local function deeply_equal(a, b)
  if json.is_null(a) or json.is_null(b) then
    return json.is_null(a) and json.is_null(b)
  end
  if type(a) ~= type(b) then return false end
  if type(a) ~= "table" then return a == b end
  if json.is_array(a) ~= json.is_array(b) then return false end
  if json.is_array(a) then
    if #a ~= #b then return false end
    for i = 1, #a do
      if not deeply_equal(a[i], b[i]) then return false end
    end
    return true
  end
  for k in pairs(a) do
    if not deeply_equal(a[k], b[k]) then return false end
  end
  for k in pairs(b) do
    if a[k] == nil and b[k] ~= nil then return false end
  end
  return true
end

---Tag-preserving deep clone so input snapshots compare structurally equal
---(including typed container identities) after merging.
local function clone(v)
  if type(v) ~= "table" or json.is_null(v) then return v end
  local out = {}
  for k, val in pairs(v) do out[k] = clone(val) end
  return setmetatable(out, getmetatable(v))
end

---Runs one deep_merge call under pcall so typed failures become clean
---assertions instead of aborting the suite.
local function safe_merge(...)
  local ok_call, result = pcall(util.deep_merge, ...)
  if not ok_call then
    return nil, tostring(result)
  end
  return result, nil
end

-- ---------------------------------------------------------------------------
-- Cases
-- ---------------------------------------------------------------------------

do
  -- objects_merge_recursively: issue fixture {a=1,b={x=1}} + {b={y=2}}.
  local r = safe_merge({a = 1, b = {x = 1}}, {b = {y = 2}})
  ok(r ~= nil and deeply_equal(r, {a = 1, b = {x = 1, y = 2}}),
    "objects merge recursively by string key")

  -- typed_object_identity_preserved: tagged object + tagged object stays a
  -- tagged JSON object (later side's identity wins), matching how copied
  -- values preserve their container tags. Content is compared structurally:
  -- the codec encodes objects in unsorted pairs() order (hash-seed dependent
  -- across processes) and JSON key order carries no contract meaning.
  r = safe_merge(json.object({a = 1}), json.object({b = 2}))
  ok(r ~= nil and json.is_object(r) and deeply_equal(r, {a = 1, b = 2}),
    "merged tagged objects keep their explicit JSON object identity")
  r = safe_merge({a = 1}, json.object({b = 2}))
  ok(r ~= nil and json.is_object(r),
    "untagged base merged with a typed later object adopts the typed identity")

  -- atomic_replace_shorter_array: later shorter array leaves NO tail.
  r = safe_merge({"lib", "vendor"}, {"src"})
  ok(r ~= nil and deeply_equal(r, {"src"}) and #r == 1,
    "later shorter array replaces atomically without inherited tail")

  -- empty_array_clears_inherited: explicit empty array clears the list.
  r = safe_merge({"lib"}, {})
  ok(r ~= nil and deeply_equal(r, {}) and #r == 0,
    "later empty array clears an inherited array")

  -- typed_empty_containers: {} (object) + json.array({}) answers [], not {}.
  -- This consumes the codec's typed identities rather than guessing by #table.
  r = safe_merge(json.object({}), json.array({}))
  ok(r ~= nil and json.is_array(r) and #r == 0 and json.encode(r) == "[]",
    "typed empty object merged with typed empty array yields typed empty array")

  -- untagged_empty_tables_stay_objects: plain Lua {} stays the encoder's
  -- upstream-compatibility object default (documented, deterministic).
  r = safe_merge({}, {})
  ok(r ~= nil and type(r) == "table" and not json.is_array(r),
    "two untagged empty Lua tables merge to the object-shaped default")

  -- type_change_array_to_object: later object replaces the earlier array.
  r = safe_merge({"lib", "vendor"}, {enabled = true})
  ok(r ~= nil and deeply_equal(r, {enabled = true}),
    "array replaced by later object without numeric merging")

  -- type_change_object_to_array: later array replaces the earlier object.
  r = safe_merge({enabled = true}, {"src"})
  ok(r ~= nil and deeply_equal(r, {"src"}),
    "object replaced by later array without key merging")

  -- scalars_replace_exactly: false/0/""/explicit null override everything.
  r = safe_merge(
    {flag = true, count = 5, name = "old", data = {x = 1}},
    {flag = false, count = 0, name = "", data = json.null})
  ok(r ~= nil and r.flag == false and r.count == 0 and r.name == ""
    and json.is_null(r.data),
    "false, zero, empty string, and explicit null replace earlier values exactly")

  -- top_level_scalar_replacement_through_keys: a scalar deeper override wins
  -- over an earlier nested object and vice versa within one call.
  r = safe_merge(
    {perl = {include = {"lib"}}},
    {perl = {include = json.null}})
  ok(r ~= nil and json.is_null(r.perl.include),
    "explicit null clears an inherited nested array slot")

  -- nested_object_recurses_child_arrays_replace: objects recurse while child
  -- arrays stay indivisible configuration values.
  r = safe_merge(
    {perl = {include = {"lib", "vendor"}, mode = "auto"}},
    {perl = {include = {"src"}}})
  ok(r ~= nil and r.perl.mode == "auto"
    and deeply_equal(r.perl.include, {"src"}) and #r.perl.include == 1,
    "nested objects recurse while child arrays replace atomically")

  -- realistic_include_paths_fixture: the exact stale-tail scenario from the
  -- issue — earlier ["lib","vendor"], later ["src"] — must never answer
  -- ["src","vendor"] on the effective merged settings object.
  local user_settings = {perl = {include_paths = {"lib", "vendor"}}}
  local workspace_settings = {perl = {include_paths = {"src"}}}
  r = safe_merge(user_settings, workspace_settings)
  ok(r ~= nil and json.encode(r.perl.include_paths) == '["src"]',
    "effective merged perl.include_paths carries no inherited vendor tail")

  -- wire_round_trip_typed_values: decoded JSON settings merge exactly and
  -- encode back to the later array's bytes.
  local decoded_base = json.decode('{"perl":{"exclude":["a","b"]}}')
  local decoded_later = json.decode('{"perl":{"exclude":[]}}')
  r = safe_merge(decoded_base, decoded_later)
  ok(r ~= nil and json.encode(r.perl.exclude) == "[]",
    "decoded JSON arrays merge to the later encoded form exactly")

  -- inputs_not_mutated: sources survive byte-identical after merging, and
  -- mutating the result cannot contaminate them.
  local base_snapshot = {perl = {paths = {"lib"}, opts = {on = true}}}
  local later_snapshot = {perl = {paths = {"src"}}}
  local base_copy = clone(base_snapshot)
  local later_copy = clone(later_snapshot)
  r = safe_merge(base_snapshot, later_snapshot)
  r.perl.paths[1] = "MUTATED"
  r.perl.opts.on = false
  ok(deeply_equal(base_snapshot, base_copy)
    and deeply_equal(later_snapshot, later_copy),
    "inputs are not mutated by merging nor contaminated by result edits")

  -- deterministic_results: repeated merges produce structurally identical
  -- results regardless of prior call history.
  local r1 = safe_merge(base_copy, later_copy)
  local r2 = safe_merge(base_copy, later_copy)
  ok(r1 ~= nil and r2 ~= nil and deeply_equal(r1, r2),
    "repeated merges of the same inputs are deterministic")

  -- sparse_numeric_container_fails: holes fail through the codec authority
  -- instead of receiving ad-hoc merge behavior.
  local sparse_ok, sparse_err = pcall(util.deep_merge, {}, {[3] = "gap"})
  ok(not sparse_ok and tostring(sparse_err):find("invalid table", 1, true),
    "sparse numeric container fails through the codec authority")

  -- mixed_key_container_fails: numeric plus string keys fail honestly.
  local mixed_ok, mixed_err = pcall(util.deep_merge, {}, {[1] = "a", b = "c"})
  ok(not mixed_ok and tostring(mixed_err):find("invalid table", 1, true),
    "mixed-key container fails through the codec authority")

  -- nil_arguments_ignored_compat: existing callers rely on nil skipping.
  r = safe_merge(nil, {a = 1}, nil)
  ok(r ~= nil and deeply_equal(r, {a = 1}),
    "nil arguments are ignored (compatibility preserved)")

  -- non_table_argument_asserts_compat: existing misuse boundary preserved.
  local bad_ok = pcall(util.deep_merge, {}, "not-a-table")
  ok(not bad_ok, "non-table argument still asserts (compatibility preserved)")
end

print(string.format("%d passed, %d failed", passed, failed))
if failed > 0 then os.exit(1) end

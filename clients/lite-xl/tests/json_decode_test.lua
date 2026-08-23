-- Deterministic focused tests for clients/lite-xl/upstream/json.lua (#11197, #11136).
--
-- Run:
--   lua clients/lite-xl/tests/json_decode_test.lua [path-to-json-module]
-- Default module path is ../upstream/json.lua relative to this file.
--
-- Mutation falsifier (#11197 proof): run this file against the pristine
-- upstream json.lua @ d1432ae0736cd9531798b4bc1221835f534cc689 instead of the
-- patched module. The escape/object cases must FAIL there with incidental Lua
-- exceptions ("attempt to index a nil value", "attempt to concatenate a nil
-- value", "table index is nil", "invalid unicode codepoint"), proving these
-- tests discriminate the terminal-error behavior rather than passing vacuously.
--
-- Mutation falsifiers (#11136 proof): revert only one typed-value behavior at a
-- time (null identity to Lua nil; array tagging removed; null restored to the
-- "{{json::null}}" magic string) and the matching round-trip assertions below
-- must FAIL, proving the typed-value tests discriminate rather than pass
-- vacuously on an untagged codec.
--
-- No framework: plain asserts, one process, deterministic, exit code carries
-- the result. Compatible with the Lite XL Lua runtime family (5.4).

local module_path = arg and arg[1] or nil

if not module_path then
  local info = debug.getinfo(1, "S").source:sub(2)
  local dir = info:match("^(.*)[/\\]") or "."
  module_path = dir .. "/../upstream/json.lua"
end

local json = dofile(module_path)

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

--- Decodes input under pcall so an uncaught exception is itself a failure.
--- Returns ok(bool), value_or_err.
local function safe_decode(input)
  local ok, v1, v2 = pcall(json.decode, input)
  if not ok then
    return false, v1
  end
  return true, v1, v2
end

local function assert_typed_failure(input, want_reason, label, want_offset, want_line, want_col)
  local ran, res, err = safe_decode(input)
  if not ran then
    ok(false, label .. ": raised incidental Lua exception: " .. tostring(res))
    return nil
  end
  ok(res == nil, label .. ": no partial/first value returned")
  ok(type(err) == "table", label .. ": second return is a table, got " .. type(err))
  if type(err) ~= "table" then return nil end
  ok(err.kind == "json.decode_error", label .. ": kind tag, got " .. tostring(err.kind))
  ok(err.reason == want_reason, label .. ": reason=" .. tostring(err.reason) .. " want " .. want_reason)
  ok(type(err.byte_offset) == "number" and err.byte_offset >= 1,
    label .. ": byte_offset present (" .. tostring(err.byte_offset) .. ")")
  ok(type(err.line) == "number" and err.line >= 1, label .. ": line present")
  ok(type(err.column) == "number" and err.column >= 1, label .. ": column present")
  ok(type(err.message) == "string" and #err.message > 0 and #err.message < 200,
    label .. ": bounded message present")
  if #input > 64 then
    ok(not string.find(err.message, input, 1, true),
      label .. ": message does not echo the full input body")
  end
  if want_offset then
    ok(err.byte_offset == want_offset,
      label .. ": byte_offset=" .. tostring(err.byte_offset) .. " want " .. want_offset)
  end
  if want_line then
    ok(err.line == want_line, label .. ": line=" .. tostring(err.line) .. " want " .. want_line)
  end
  if want_col then
    ok(err.column == want_col, label .. ": column=" .. tostring(err.column) .. " want " .. want_col)
  end
  return err
end

-- ---------------------------------------------------------------------------
-- Valid decodes are unaffected and distinguishable from failure
-- ---------------------------------------------------------------------------

do
  local res = json.decode('{"jsonrpc":"2.0","id":7,"result":[true,null,{"k":"v"}]}')
  ok(type(res) == "table" and res.id == 7 and res.jsonrpc == "2.0", "valid LSP frame decodes")
  ok(res.result[1] == true and res.result[3].k == "v", "valid frame contents preserved")

  local f = json.decode("false")
  ok(f == false, "valid JSON false survives as false")

  local n = json.decode("null")
  ok(n == json.null, "valid JSON null decodes to the unique json.null identity")

  local e = json.decode("{}")
  ok(type(e) == "table" and next(e) == nil, "valid empty object decodes")

  local s = json.decode('"\\u00e9\\n\\t\\\\"')
  ok(s == "\195\169\n\t\\", "string escapes decode")

  local num = json.decode("-12.5e2")
  ok(num == -1250, "number decodes")

  local big = json.decode("123456789012345678901")
  ok(big == "{{json::num}}123456789012345678901", "long number keeps flag-string form (unchanged)")
end

-- ---------------------------------------------------------------------------
-- Truncated / garbage inputs terminate with exactly one typed error
-- ---------------------------------------------------------------------------

assert_typed_failure('{"a": [1, 2', "expected_array_delimiter", "truncated array")
assert_typed_failure('{"a":', "unexpected_end_of_input", "truncated object")
assert_typed_failure("", "unexpected_end_of_input", "empty body")
assert_typed_failure("   \r\n  ", "unexpected_end_of_input", "whitespace-only body")
assert_typed_failure("@@@", "unexpected_character", "garbage bytes")
assert_typed_failure("{", "expected_string_key", "lone brace")
assert_typed_failure("[1 @]", "expected_array_delimiter", "bad token inside array")
assert_typed_failure('"a\nb"', "control_character_in_string", "raw control char in string")
assert_typed_failure('"abc', "unterminated_string", "unclosed string")
assert_typed_failure('"\\q"', "invalid_escape", "invalid escape char")
assert_typed_failure('"\\u12"', "invalid_unicode_escape", "truncated unicode escape")
assert_typed_failure('"\\uzzzz"', "invalid_unicode_escape", "non-hex unicode escape")
assert_typed_failure('"\\udbff\\uffff"', "invalid_unicode_escape",
  "surrogate pair composing out-of-range codepoint")
assert_typed_failure('{"a" 1}', "expected_colon_after_key", "missing colon")
assert_typed_failure('{1:2}', "expected_string_key", "non-string key")
assert_typed_failure('[1 2]', "expected_array_delimiter", "missing comma in array")
assert_typed_failure('{"a":1 "b":2}', "expected_object_delimiter", "missing comma in object")
assert_typed_failure("nul", "invalid_literal", "invalid literal")
assert_typed_failure("--1", "invalid_number", "invalid number")
assert_typed_failure('{"a":1} zz', "trailing_garbage", "trailing garbage after valid value")

-- Long hostile body: the failure stays typed and bounded, no body echo.
do
  local big = string.rep("x", 4096) .. "@"
  local ran, res, err = safe_decode(big)
  ok(ran and res == nil and type(err) == "table", "4KB garbage yields typed failure")
  ok(err and err.reason == "unexpected_character", "long garbage reason")
  ok(err and #err.message < 200, "long garbage message stays bounded")
end

-- ---------------------------------------------------------------------------
-- Error metadata is positional and actionable
-- ---------------------------------------------------------------------------

assert_typed_failure('{\n  "a": @}', "unexpected_character", "line/col tracked across newline",
  10, 2, 8)

-- ---------------------------------------------------------------------------
-- Failure state is per-call; no partial values; subsequent valid decodes win
-- ---------------------------------------------------------------------------

do
  local _, err1 = json.decode('{"a": @}')
  local _, err2 = json.decode("[@@]")
  ok(err1 ~= err2, "two failures produce distinct error objects")
  ok(err1.reason == "unexpected_character" and err1.line == 1, "first error object unchanged by later call")
  ok(json.last_error() == err2.message, "last_error projects most recent failure only")

  local good = json.decode('{"ok":[true,null,"x"]}')
  ok(type(good) == "table" and good.ok[1] == true, "valid-after-garbage decodes independently")
  ok(good.ok[2] == json.null, "null element inside a later valid decode keeps identity")
  ok(json.last_error() == "", "last_error cleared by successful decode")

  -- Valid-after-garbage does not partially apply: the failed decode returned
  -- nothing usable and the success carries only its own content.
  ok(good.a == nil, "failed decode left no residue in later result")
end

-- ---------------------------------------------------------------------------
-- Non-string argument remains a documented argument error (programmer misuse)
-- ---------------------------------------------------------------------------

do
  local ran, exc = pcall(json.decode, 42)
  ok(ran == false, "non-string argument raises argument error")
  ok(type(exc) == "string" and string.find(exc, "expected argument of type string", 1, true),
    "argument error names the misuse: " .. tostring(exc))
end

-- ---------------------------------------------------------------------------
-- Typed JSON value identities (#11136)
--
-- JSON null, arrays (including empty) and objects (including empty) keep
-- collision-free in-memory identities; encoding respects them. Decoded shape
-- AND re-encoded bytes are asserted for each round trip.
-- ---------------------------------------------------------------------------

local function roundtrip(input, want_bytes, label)
  local v = json.decode(input)
  ok(v ~= nil, label .. ": decode yields a value")
  local out = json.encode(v)
  ok(out == want_bytes,
    label .. ": re-encodes to '" .. tostring(out) .. "' want '" .. tostring(want_bytes) .. "'")
  return v
end

do
  -- Null identity is unique and non-forgeable.
  local n = json.decode("null")
  ok(n == json.null and n ~= nil and n ~= false and n ~= "",
    "json.null is one distinct non-nil identity")
  ok(json.is_null(json.null), "is_null recognizes the singleton")
  ok(not json.is_null(nil) and not json.is_null(false) and not json.is_null(0),
    "is_null rejects nil, false, numbers")

  -- The old magic-string sentinel is dead: that text is now an ordinary string.
  local s = json.decode('"{{json::null}}"')
  ok(s == "{{json::null}}", "old null sentinel text decodes as an ordinary string")
  ok(json.encode(s) == '"{{json::null}}"', "sentinel-text string encodes quoted, never as null")
  ok(not json.is_null(s), "sentinel-text string is not the null identity")

  -- Required structural round trips with byte-exact re-encoding.
  roundtrip("null", "null", "null round trip")
  roundtrip("[]", "[]", "empty array round trip")
  roundtrip("[null]", "[null]", "array with null keeps cardinality")
  roundtrip('[false, null, [], {}]', '[false,null,[],{}]', "mixed scalar/empty round trip")
  roundtrip('[[[]], {"e": [{}]}]', '[[[]],{"e":[{}]}]', "nested empty children round trip")

  -- Empty object: decoded shape plus byte-stable single-key re-encoding.
  local eo = roundtrip("{}", "{}", "empty object round trip")
  ok(type(eo) == "table" and next(eo) == nil and json.is_object(eo),
    "empty object decodes to an object-tagged table")

  -- Multi-key object: key order is not semantic, so assert structurally.
  local o = json.decode('{"a": null, "b": [], "c": {}}')
  ok(o.a == json.null and json.is_array(o.b) and #o.b == 0
    and json.is_object(o.c) and next(o.c) == nil,
    "null/empty-array/empty-object fields all survive decode")
  local back = json.decode(json.encode(o))
  ok(back.a == json.null and json.is_array(back.b) and next(back.b) == nil
    and json.is_object(back.c) and next(back.c) == nil,
    "multi-key null/empty fields survive encode+redecode")
end

do
  -- Explicit constructors at ambiguous protocol boundaries (#11136 contract 7).
  local a = json.array({ 1, 2 })
  ok(json.is_array(a) and json.encode(a) == "[1,2]", "array constructor tags and encodes")
  ok(json.encode(json.array({})) == "[]", "empty array constructor emits []")
  local ob = json.object({ k = "v", flag = false })
  ok(json.is_object(ob), "object constructor tags")
  ok(json.encode(json.object({})) == "{}", "empty object constructor emits {}")

  -- Ordinary untagged tables keep upstream source compatibility.
  ok(json.encode({ 1, 2 }) == "[1,2]", "plain non-empty array still encodes as array")
  ok(json.encode({}) == "{}", "plain empty table still encodes as {} (reviewed default)")
  ok(json.encode({ k = 1 }) == '{"k":1}', "plain map still encodes as object")
end

do
  -- Constructor misuse fails deterministically instead of guessing a shape.
  local cases = {
    { function() return json.array(nil) end, "array(nil)" },
    { function() return json.array("x") end, "array(string)" },
    { function() return json.array({ [1] = 1, [3] = 3 }) end, "sparse array" },
    { function() return json.array({ [0] = 0 }) end, "zero-key array" },
    { function() return json.object(nil) end, "object(nil)" },
    { function() return json.object({ [1] = "n" }) end, "numeric object key" },
  }
  for _, case in ipairs(cases) do
    local ran, exc = pcall(case[1])
    ok(ran == false and type(exc) == "string" and #exc > 0,
      "constructor rejects " .. case[2] .. " deterministically: " .. tostring(exc))
  end
end

do
  -- Forged lookalike metatables stay inert: no private tag, no typed behavior.
  local forged_array = setmetatable({}, { json_type = "array" })
  local forged_object = setmetatable({}, { __jsontype = "object" })
  ok(json.encode(forged_array) == "{}", "forged json_type=array does not emit []")
  ok(json.encode(forged_object) == "{}", "forged __jsontype=object does not emit tagged object")
  ok(not json.is_array(forged_array) and not json.is_object(forged_object),
    "predicates ignore foreign metatables")
  ok(json.decode("[]") ~= forged_array and getmetatable(json.decode("{}")) ~= nil,
    "decoded containers carry only module-private tagging")
end

do
  -- Sparse / mixed / cyclic values fail honestly at the encode boundary.
  local arr = json.decode("[1, 2, 3]")
  arr[2] = nil
  local ran, exc = pcall(json.encode, arr)
  ok(ran == false and string.find(exc, "sparse array", 1, true),
    "mid-hole array fails sparse validation: " .. tostring(exc))

  -- A trailing hole is indistinguishable from a shorter dense sequence under
  -- Lua's border rule; encoding follows the dense prefix exactly as upstream
  -- did. Documented boundary, not silent shape-guessing.
  local trail = json.decode("[1, 2, 3]")
  trail[3] = nil
  ok(json.encode(trail) == "[1,2]",
    "trailing hole truncates to the dense prefix (upstream border rule)")

  local mixed = json.object({ x = 1 })
  mixed[1] = 2
  ran, exc = pcall(json.encode, mixed)
  ok(ran == false and string.find(exc, "mixed or invalid key types", 1, true),
    "tagged object with numeric key fails honestly: " .. tostring(exc))

  local plain_mixed = {}
  plain_mixed.a = 1
  plain_mixed[1] = 2
  ran, exc = pcall(json.encode, plain_mixed)
  ok(ran == false and string.find(exc, "invalid table", 1, true),
    "plain mixed-key table fails honestly: " .. tostring(exc))

  local cyc = json.array({ 1 })
  cyc[2] = cyc
  ran, exc = pcall(json.encode, cyc)
  ok(ran == false and string.find(exc, "circular reference", 1, true),
    "cyclic tagged array fails circular detection: " .. tostring(exc))

  local pcyc = {}
  pcyc.self = pcyc
  ran, exc = pcall(json.encode, pcyc)
  ok(ran == false and string.find(exc, "circular reference", 1, true),
    "cyclic plain table still detected: " .. tostring(exc))
end

do
  -- workspace/configuration result slots: absent vs null vs present value.
  local req = json.decode('[{"section": "x"}, null]')
  ok(#req == 2 and json.is_object(req[1]) and req[1].section == "x"
    and req[2] == json.null,
    "configuration result array preserves cardinality across null slots")
  ok(json.encode(json.array({ json.object({ section = "x" }), json.null }))
    == '[{"section":"x"},null]',
    "client-built configuration response keeps null slot distinct from absence")

  -- Server response result=false versus result=null stay distinct.
  local rf = json.decode('{"id":5,"result":false}')
  local rn = json.decode('{"id":5,"result":null}')
  ok(rf.result == false, "result=false survives as Lua false")
  ok(rn.result == json.null, "result=null survives as json.null, not nil/false")
  ok(json.encode(rf.result) == "false" and json.encode(rn.result) == "null",
    "false and null results encode distinctly")
end

print(string.format("%s: %d passed, %d failed (%s)",
  module_path, passed, failed, failed == 0 and "OK" or "MUTATION DETECTED"))
os.exit(failed == 0 and 0 or 1)

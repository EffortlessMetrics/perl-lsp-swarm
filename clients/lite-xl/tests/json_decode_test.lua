-- Deterministic focused tests for clients/lite-xl/upstream/json.lua
-- (#11197, #11136, #11183, #11194).
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
local native_math_type = math.type
local has_native_integers = native_math_type ~= nil and math.maxinteger ~= nil
local function is_integer(value)
  if native_math_type then
    return native_math_type(value) == "integer"
  end
  -- Legacy runtimes have no integer subtype; integral doubles are the
  -- closest available check.
  return type(value) == "number" and value % 1 == 0
end

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
  ok(type(num) == "table" and json.is_number(num) and json.number_lexeme(num) == "-12.5e2",
    "non-canonical number keeps its exact validated lexeme")
  ok(json.encode(num) == "-12.5e2", "lexeme number re-encodes byte-exactly")

  local big = json.decode("123456789012345678901")
  ok(type(big) == "table" and json.is_number(big), "long integer decodes to typed lexeme value")
  ok(json.encode(big) == "123456789012345678901", "long integer re-encodes byte-exactly")
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

-- ---------------------------------------------------------------------------
-- Numeric and string identity (#11183)
--
-- Reviewed policy: a decimal integer that fits int64 with canonical form
-- equal to its lexeme stays an ordinary Lua integer; every other valid number
-- keeps its exact validated lexeme in a typed json.number value. Nothing is
-- rounded through floats, no magic prefix exists, malformed numerals fail.
-- ---------------------------------------------------------------------------

do
  -- Exact integer range keeps numeric identity (required cases).
  ok(json.decode("0") == 0 and is_integer(json.decode("0")), "0 stays an exact integer")
  ok(json.decode("1") == 1 and json.decode("-1") == -1, "1 and -1 stay integers")
  if has_native_integers then
    ok(json.decode("9223372036854775807") == math.maxinteger, "int64 max decodes exactly")
    ok(json.decode("-9223372036854775808") == math.mininteger, "int64 min decodes exactly")
  else
    ok(json.is_number(json.decode("9223372036854775807")), "legacy runtime retains int64 max lexeme")
    ok(json.is_number(json.decode("-9223372036854775808")), "legacy runtime retains int64 min lexeme")
  end
  local beyond_double = json.decode("9007199254740993")
  if has_native_integers then
    ok(beyond_double == 9007199254740993 and native_math_type(beyond_double) == "integer",
      "integer beyond double precision stays exact")
  else
    ok(json.is_number(beyond_double) and json.number_lexeme(beyond_double) == "9007199254740993",
      "legacy runtime retains beyond-double integer lexeme")
  end
  ok(json.encode(beyond_double) == "9007199254740993", "beyond-double integer re-encodes exactly")

  -- Just beyond int64 retains the lexeme instead of rounding.
  local over = json.decode("9223372036854775808")
  ok(type(over) == "table" and json.is_number(over) and json.number_lexeme(over) == "9223372036854775808",
    "int64+1 keeps its lexeme")
  ok(json.encode(over) == "9223372036854775808", "int64+1 re-encodes byte-exactly")
  local under = json.decode("-9223372036854775809")
  ok(json.number_lexeme(under) == "-9223372036854775809", "below-int64 keeps its lexeme")

  -- Long decimal integer and exponent forms keep their own bytes.
  local long = json.decode("123456789012345678901234567890")
  ok(json.number_lexeme(long) == "123456789012345678901234567890", "long decimal keeps lexeme")
  ok(json.encode(long) == "123456789012345678901234567890", "long decimal re-encodes byte-exactly")
  local expnum = json.decode("1e30")
  ok(json.number_lexeme(expnum) == "1e30", "exponent overflow keeps its own spelling")
  ok(json.encode(expnum) == "1e30", "exponent overflow re-encodes verbatim")

  -- The old magic-prefix text is now inert in both directions.
  local flagged = json.decode('"{{json::num}}123456789012345"')
  ok(flagged == "{{json::num}}123456789012345", "number_flag text decodes as an ordinary string")
  ok(not json.is_number(flagged), "number_flag text is not a JSON number value")
  ok(json.encode(flagged) == '"{{json::num}}123456789012345"',
    "prefix-text string encodes quoted, never as a bare number")

  -- Plain string digits stay strings; numeric digits stay numbers.
  local sid = json.decode('"123"')
  ok(sid == "123" and type(sid) == "string" and not json.is_number(sid), 'string "123" stays a string')
  ok(json.encode(sid) == '"123"', "digit-string encodes quoted")
  local nid = json.decode("123456789012345")
  ok(nid == 123456789012345 and is_integer(nid), "15-digit numeric ID stays exact integer")
  ok(json.encode(nid) == "123456789012345", "15-digit numeric ID re-encodes identically")

  -- Same visible digits, different JSON types: never conflated.
  local str_frame = json.decode('{"id":"7","method":"x"}')
  local num_frame = json.decode('{"id":7,"method":"x"}')
  ok(type(str_frame.id) == "string" and str_frame.id == "7", "string ID keeps type")
  ok(is_integer(num_frame.id) and num_frame.id == 7, "numeric ID keeps type")
  ok(str_frame.id ~= num_frame.id, "same-digit string/numeric IDs are distinct values")
  local wire_str_frame = json.decode(json.encode(str_frame))
  local wire_num_frame = json.decode(json.encode(num_frame))
  ok(type(wire_str_frame.id) == "string" and is_integer(wire_num_frame.id)
    and wire_num_frame.id == 7 and wire_num_frame.method == "x",
    "independent wire decode preserves string/numeric ID types")
  ok(json.encode(num_frame.id) == "7", "numeric ID scalar re-encodes byte-exactly")

  -- Independent request/response wire round trip preserves the large typed ID.
  local req = json.decode('{"id":123456789012345678901,"method":"m"}')
  local req_wire = json.encode(req)
  local received_req = json.decode(req_wire)
  local resp = json.object({ jsonrpc = "2.0", id = received_req.id, result = json.null })
  local resp_json = json.encode(resp)
  local received_resp = json.decode(resp_json)
  ok(json.number_lexeme(received_resp.id) == "123456789012345678901",
    "independent response decode carries the same typed ID")
  ok(string.find(resp_json, '"id":123456789012345678901', 1, true) ~= nil,
    "encoded response echoes the exact ID bytes: " .. resp_json)

  -- Malformed numerals fail honestly instead of being coerced.
  assert_typed_failure("01", "invalid_number", "leading zero")
  assert_typed_failure("+1", "unexpected_character", "explicit plus sign")
  assert_typed_failure("-", "invalid_number", "lone minus")
  assert_typed_failure("1.", "invalid_number", "trailing dot")
  assert_typed_failure(".5", "unexpected_character", "leading dot")
  assert_typed_failure("1e", "invalid_number", "bare exponent")
  assert_typed_failure("1e+", "invalid_number", "sign without exponent digits")
  assert_typed_failure("0x10", "invalid_number", "hex numeral is not JSON")
  assert_typed_failure("1.2.3", "invalid_number", "double dot")
  assert_typed_failure("1e999", "invalid_number", "overflow to infinity fails at decode")

  -- Constructor validates with the same strict grammar.
  local bad_lexemes = { "", "-", "01", "+1", "1.", ".5", "1e", "0x10", "--1", "nan", "inf", "Infinity", "1.2.3" }
  for _, lex in ipairs(bad_lexemes) do
    local ran, exc = pcall(json.number, lex)
    ok(ran == false and type(exc) == "string" and string.find(exc, "invalid number lexeme", 1, true),
      "json.number rejects '" .. tostring(lex) .. "' deterministically: " .. tostring(exc))
  end
  local ran, exc = pcall(json.number, 42)
  ok(ran == false and string.find(exc, "invalid number lexeme", 1, true),
    "json.number rejects non-string input: " .. tostring(exc))

  -- Typed numbers nest inside containers and round-trip.
  local nested = json.decode('[9223372036854775808, {"big": 123456789012345678901}]')
  ok(json.is_number(nested[1]) and json.number_lexeme(nested[1]) == "9223372036854775808",
    "typed number survives inside array")
  ok(json.is_number(nested[2].big) and json.number_lexeme(nested[2].big) == "123456789012345678901",
    "typed number survives inside object field")
  ok(json.encode(nested) == '[9223372036854775808,{"big":123456789012345678901}]',
    "nested typed numbers re-encode byte-exactly")

  -- Forged lookalike tags stay inert; predicates ignore foreign metatables.
  local forged = setmetatable({}, { json_type = "number" })
  ok(not json.is_number(forged) and json.number_lexeme(forged) == nil,
    "forged number tag gains nothing")
  ok(json.encode(forged) == "{}", "forged number-tagged table encodes as plain empty table")
end

-- ---------------------------------------------------------------------------
-- Unicode scalar and UTF-8 validity (#11194)
--
-- Only valid Unicode scalars decode from \u escapes (lone/malformed surrogates
-- fail typed), raw string bytes must be valid UTF-8, and outbound strings are
-- validated before any encoding work.
-- ---------------------------------------------------------------------------

do
  -- Required valid cases: exact bytes preserved.
  ok(json.decode('"abc"') == "abc", "ASCII passthrough")
  local two = json.decode('"\195\169"')
  ok(two == "\195\169", "raw 2-byte sequence preserved")
  local two_esc = json.decode('"\\u00e9"')
  ok(two_esc == "\195\169", "escaped BMP scalar equals raw bytes")
  local three = json.decode('"\226\130\172"')
  ok(three == "\226\130\172", "raw 3-byte sequence preserved")
  ok(json.decode('"\\u20ac"') == "\226\130\172", "escaped 3-byte scalar composes exactly")
  local four = json.decode('"\240\159\152\128"')
  ok(four == "\240\159\152\128", "raw 4-byte non-BMP preserved")
  local pair = json.decode('"\\ud83d\\ude00"')
  ok(pair == "\240\159\152\128", "valid surrogate pair decodes to U+1F600 bytes")
  ok(json.decode('"\\u0000"') == "\0", "escaped NUL stays valid at the JSON layer")
  ok(json.decode('"a\\u00e9b\195\169c"') == "a\195\169b\195\169c", "mixed raw/escaped decodes")

  -- Outbound: valid strings round-trip byte-exactly; escaping still works.
  local uni = "x\195\169\226\130\172\240\159\152\128y"
  ok(json.encode(uni) == '"x\195\169\226\130\172\240\159\152\128y"', "valid unicode encodes byte-exactly")
  ok(json.encode("\n\"\\") == '"\\n\\"\\\\"', "control/quote/backslash escaping intact")
  ok(json.decode(json.encode(uni)) == uni, "unicode round trip")

  -- Required invalid escape cases fail typed.
  assert_typed_failure('"\\ud800"', "invalid_unicode_escape", "lone high surrogate")
  assert_typed_failure('"\\udbff"', "invalid_unicode_escape", "lone high surrogate at dbff")
  assert_typed_failure('"\\udc00"', "invalid_unicode_escape", "lone low surrogate")
  assert_typed_failure('"\\udfff"', "invalid_unicode_escape", "lone low surrogate at dfff")
  assert_typed_failure('"\\ud800\\ud800"', "invalid_unicode_escape", "high followed by high")
  assert_typed_failure('"\\ud83d\\u0041"', "invalid_unicode_escape", "high followed by non-low BMP")
  assert_typed_failure('"\\ud800\\u0041"', "invalid_unicode_escape",
    "high plus plain escape previously composed garbage silently")
  assert_typed_failure('"\\ud83d"', "invalid_unicode_escape", "truncated pair at end of input")

  -- Required invalid raw-byte cases fail typed.
  assert_typed_failure('"\128"', "invalid_utf8", "isolated continuation byte")
  assert_typed_failure('"\195"', "invalid_utf8", "truncated 2-byte sequence")
  assert_typed_failure('"\226\130"', "invalid_utf8", "truncated 3-byte sequence")
  assert_typed_failure('"\240\159\152"', "invalid_utf8", "truncated 4-byte sequence")
  assert_typed_failure('"\194"', "invalid_utf8", "truncated 2-byte at c2")
  assert_typed_failure('"\192\175"', "invalid_utf8", "overlong 2-byte encoding")
  assert_typed_failure('"\224\128\175"', "invalid_utf8", "overlong 3-byte encoding")
  assert_typed_failure('"\224\128"', "invalid_utf8", "overlong truncated")
  assert_typed_failure('"\237\160\189"', "invalid_utf8", "raw surrogate encoding D800")
  assert_typed_failure('"\237\191\191"', "invalid_utf8", "raw surrogate encoding DFFF")
  assert_typed_failure('"\247\191\191\191"', "invalid_utf8", "value above U+10FFFF")
  assert_typed_failure('"\193\191"', "invalid_utf8", "overlong lead byte c1")
  assert_typed_failure('"\245\128\128\128"', "invalid_utf8", "lead byte f5 above plane 16")
end

do
  -- Outbound validation rejects invalid UTF-8 with a bounded deterministic
  -- error before any frame content is produced.
  local cases = {
    { "ok\195", "truncated tail" },
    { "\128", "leading continuation" },
    { "a\226\130b", "truncated middle" },
    { "x\237\160\189y", "surrogate bytes outbound" },
    { "\192\175", "overlong outbound" },
  }
  for _, case in ipairs(cases) do
    local ran, exc = pcall(json.encode, case[1])
    ok(ran == false and type(exc) == "string"
      and string.find(exc, "invalid UTF%-8 at byte offset %d+", 1, false) ~= nil,
      "encode rejects " .. case[2] .. ": " .. tostring(exc))
  end

  -- The failure message stays bounded and does not echo long bodies.
  local long_bad = string.rep("a", 4096) .. "\128"
  local ran, exc = pcall(json.encode, long_bad)
  ok(ran == false and #tostring(exc) < 200
    and not string.find(tostring(exc), string.rep("a", 64), 1, true),
    "encode failure message bounded without body echo")

  -- Typed containers holding invalid strings fail through the same gate.
  local arr = json.array({ "ok", "bad\195" })
  ran, exc = pcall(json.encode, arr)
  ok(ran == false and string.find(exc, "invalid UTF%-8", 1, false),
    "nested invalid string fails encode: " .. tostring(exc))
end

-- ---------------------------------------------------------------------------
-- Structural depth and node budgets (#11186)
--
-- Reviewed defaults: 128 nesting levels and 65536 nodes per document, chosen
-- from real LSP/configuration shapes with a wide margin and documented in the
-- codec. Decode failures are typed and positional; encode failures raise once,
-- deterministically, with no partial frame. Syntax/circular errors stay
-- distinct from budget errors.
-- ---------------------------------------------------------------------------

local DEPTH_LIMIT = 128
local NODE_LIMIT = 65536

local function nested_arrays(n)
  return string.rep("[", n) .. string.rep("]", n)
end

local function nested_objects(n)
  return string.rep('{"a":', n) .. "1" .. string.rep("}", n)
end

local function nested_mixed(n)
  local open, close = {}, {}
  for i = 1, n do
    if i % 2 == 1 then
      open[i] = "["
      close[n - i + 1] = "]"
    else
      open[i] = '{"a":'
      close[n - i + 1] = "}"
    end
  end
  return table.concat(open) .. "1" .. table.concat(close)
end

do
  -- Ordinary LSP-shaped payloads decode untouched.
  local caps = json.decode(
    '{"capabilities":{"textDocumentSync":1,"completion":{"dynamicRegistration":true,"completionItem":{"snippetSupport":false}}}}')
  ok(json.is_object(caps) and json.is_object(caps.capabilities.completion),
    "ordinary capabilities fixture decodes")
  ok(json.encode(caps.capabilities.completion.completionItem) == '{"snippetSupport":false}',
    "fixture re-encodes byte-exactly")

  -- Nesting exactly at the accepted boundary passes, in all three shapes.
  local arr_at = json.decode(nested_arrays(DEPTH_LIMIT))
  ok(json.is_array(arr_at), "array nesting at boundary decodes")
  local obj_at = json.decode(nested_objects(DEPTH_LIMIT))
  ok(json.is_object(obj_at.a.a) or json.is_object(obj_at), "object nesting at boundary decodes")
  local mix_at = json.decode(nested_mixed(DEPTH_LIMIT))
  ok(mix_at ~= nil, "mixed nesting at boundary decodes")
  ok(json.encode(arr_at) == nested_arrays(DEPTH_LIMIT),
    "boundary-depth value re-encodes byte-exactly")

  -- One level beyond the boundary fails typed in every direction.
  for _, case in ipairs{
    { nested_arrays(DEPTH_LIMIT + 1), "arrays" },
    { nested_objects(DEPTH_LIMIT + 1), "objects" },
    { nested_mixed(DEPTH_LIMIT + 1), "mixed" },
  } do
    local ran, res, err = safe_decode(case[1])
    ok(ran and res == nil and type(err) == "table"
      and err.reason == "nesting_depth_exceeded",
      case[2] .. " nested one past the limit fail typed")
    ok(err and err.reason == "nesting_depth_exceeded"
      and string.find(err.message, tostring(DEPTH_LIMIT), 1, true) ~= nil,
      case[2] .. " depth failure carries the limit in its bounded message")
    ok(err ~= nil and err.byte_offset ~= nil and err.line ~= nil,
      case[2] .. " depth failure keeps positional metadata")
  end

  -- A tiny body with extreme nesting terminates on depth, not on EOF.
  local ran, res, err = safe_decode(string.rep("[", 5000))
  ok(ran and res == nil and err.reason == "nesting_depth_exceeded",
    "tiny-byte extreme nesting hits the depth bound before end-of-input")

  -- Flat container one node beyond budget fails; exactly at budget passes.
  local elems = {}
  for i = 1, NODE_LIMIT - 1 do
    elems[i] = tostring(i % 10)
  end
  local flat_at = "[" .. table.concat(elems, ",") .. "]"
  local fv = json.decode(flat_at)
  ok(type(fv) == "table" and #fv == NODE_LIMIT - 1, "node count exactly at budget decodes")

  elems[NODE_LIMIT] = "7"
  local ran, res, err = safe_decode("[" .. table.concat(elems, ",") .. "]")
  ok(ran and res == nil and err.reason == "node_count_exceeded",
    "flat container one node beyond budget fails typed")
end

do
  -- Encode budgets mirror decode budgets without partial output.
  local deep = json.array({})
  for _ = 2, DEPTH_LIMIT do
    deep = json.array({ deep })
  end
  local ran, out = pcall(json.encode, deep)
  ok(ran, "encoding at the depth boundary succeeds")

  local deep_over = json.array({ deep })
  ran, out = pcall(json.encode, deep_over)
  ok(ran == false and type(out) == "string"
    and string.find(out, "maximum depth " .. DEPTH_LIMIT, 1, true) ~= nil,
    "encoding one past the depth boundary raises with the limit named: " .. tostring(out))

  local many = {}
  for i = 1, NODE_LIMIT do
    many[i] = i % 10
  end
  ran, out = pcall(json.encode, json.array(many))
  ok(ran == false and string.find(out, "node count", 1, true),
    "encoding one node beyond budget raises: " .. tostring(out))

  many[NODE_LIMIT] = nil
  ran, out = pcall(json.encode, json.array(many))
  ok(ran, "encoding exactly at the node budget succeeds")

  -- Large flat strings stay byte-bound, not structure-bound.
  local big_text = string.rep("aBcD", 16384)
  ran, out = pcall(json.encode, json.array({ big_text }))
  ok(ran and #out == #big_text + 4, "large flat string encodes without structural rejection")

  -- Circular detection remains a distinct, earlier error.
  local cyc = json.array({ 1 })
  cyc[2] = cyc
  ran, out = pcall(json.encode, cyc)
  ok(ran == false and string.find(out, "circular reference", 1, true)
    and not string.find(out, "depth", 1, true),
    "cyclic encode stays a distinct circular-reference error: " .. tostring(out))

  -- Budget failures never emit partial content: the raise discards assembly.
  local sink = 0
  local ok_run, res = pcall(function()
    local s = json.encode(json.array({ deep_over, 1 }))
    sink = #s
    return s
  end)
  ok(ok_run == false and sink == 0, "no partial encode result escapes a failed call")
end

print(string.format("%s: %d passed, %d failed (%s)",
  module_path, passed, failed, failed == 0 and "OK" or "MUTATION DETECTED"))
os.exit(failed == 0 and 0 or 1)

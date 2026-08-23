--
-- json.lua
-- Origin: https://github.com/rxi/json.lua
--
-- Copyright (c) 2020 rxi
--
-- Permission is hereby granted, free of charge, to any person obtaining a copy of
-- this software and associated documentation files (the "Software"), to deal in
-- the Software without restriction, including without limitation the rights to
-- use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies
-- of the Software, and to permit persons to whom the Software is furnished to do
-- so, subject to the following conditions:
--
-- The above copyright notice and this permission notice shall be included in all
-- copies or substantial portions of the Software.
--
-- THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
-- IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
-- FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
-- AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
-- LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
-- OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
-- SOFTWARE.
--

-- Staged exact upstream source for the lite-xl integration train.
-- Upstream subject: lite-xl/lite-xl-lsp json.lua
--   base ref : d1432ae0736cd9531798b4bc1221835f534cc689
--   base blob: eb36b8fa947ff1189b02ce03d257b80a86fdac64
-- Local patch (#11197): malformed JSON now terminates decoding with exactly one
-- typed decode failure at the json.decode boundary instead of continuing into
-- incidental Lua exceptions.
-- Local patch (#11136): JSON null, arrays and objects carry collision-free
-- in-memory identities (json.null singleton plus private container tagging) so
-- decoded null never vanishes, empty arrays/objects survive round trips, and
-- the forgeable "{{json::null}}" / sentinel-string encoding path is gone.
-- Local patch (#11183): the forgeable "{{json::num}}" number-flag prefix is
-- gone. Integers within exact Lua integer range stay ordinary integers; every
-- other valid number keeps its validated lexeme verbatim in a typed
-- json.number value; malformed or overflowing numerals fail typed at decode.
-- Local patch (#11194): strings carry one strict Unicode contract. Escapes
-- must be scalars or exact surrogate pairs; raw decoded bytes must be valid
-- UTF-8; outbound strings are validated before any encoding work. Lone or
-- malformed surrogates and invalid UTF-8 fail typed instead of producing
-- invalid bytes.
-- Local patch (#11186): deterministic structural budgets bound both
-- directions (nesting depth and per-document node count), so a hostile or
-- corrupt payload fails typed long before Lua stack exhaustion or unbounded
-- encode work. Budget errors stay distinct from syntax/circular errors.

local json = { _version = "0.1.2" }

-- Compatibility projection only: message of the most recent failed decode.
-- Superseded by the second return value of json.decode; retained until all
-- consumers read the typed result directly, then removed. It is not the
-- load-bearing error transport and never carries state between decodes.
local error_message = ""

-------------------------------------------------------------------------------
-- Typed JSON value identities (#11136)
--
-- json.null is a unique module-private singleton: it can never be confused
-- with Lua nil, false, or any ordinary string, so decoded null survives
-- container assignment and re-encodes as null. The historical
-- "{{json::null}}" magic string was forgeable by ordinary payload data and is
-- gone; that text is now just a string.
--
-- Containers decoded from JSON carry a private metatable tag keyed by the
-- CONTAINER_TAG upvalue, which is never exported, so the tag cannot be forged
-- from outside the module. Array identity therefore survives empty containers
-- and re-encoding instead of being guessed from table shape. Callers build
-- typed values explicitly through json.array()/json.object(); foreign
-- metatables stay inert and fall back to the documented ordinary-table
-- encoding policy below.
-------------------------------------------------------------------------------

local CONTAINER_TAG = {}
local ARRAY_MT = { [CONTAINER_TAG] = "array" }
local OBJECT_MT = { [CONTAINER_TAG] = "object" }
-- Typed exact-lexeme JSON number value (#11183): ordinary Lua integers cover
-- the exact integer range; anything else (long integers, unusual float
-- spellings) keeps its validated lexeme verbatim instead of being rounded
-- through a float or smuggled through a forgeable string prefix.
local NUMBER_MT = { [CONTAINER_TAG] = "number" }
local NUMBER_LEXEME_KEY = {}

-- LuaJIT and Lua 5.1/5.2 do not provide math.type. Keep the numeric
-- representation policy usable across the Lite XL runtime family.
local has_native_integer_type = math.type ~= nil
local math_type = math.type or function(value)
  if type(value) ~= "number" then
    return nil
  end
  -- A legacy Lua number is a double, so values beyond 2^53 cannot be
  -- classified as exact integers even when their rounded value is integral.
  return value % 1 == 0 and value >= -9007199254740991
      and value <= 9007199254740991 and "integer" or "float"
end

local function integer_lexeme(value)
  if has_native_integer_type then
    return tostring(value)
  end
  return string.format("%.0f", value)
end

json.null = setmetatable({}, {
  __name = "json.null",
  __tostring = function()
    return "json.null"
  end,
})

--- True exactly for the unique JSON null identity.
function json.is_null(v)
  return v == json.null
end

--- True for values carrying the private JSON array tag (decoded arrays and
--- json.array() results).
function json.is_array(v)
  local mt = getmetatable(v)
  return mt ~= nil and mt[CONTAINER_TAG] == "array"
end

--- True for values carrying the private JSON object tag (decoded objects and
--- json.object() results).
function json.is_object(v)
  local mt = getmetatable(v)
  return mt ~= nil and mt[CONTAINER_TAG] == "object"
end

--- Build an explicit JSON array value from a dense Lua sequence. The source
--- is validated (numeric keys only, no sparseness) and copied; misuse raises
--- a precise deterministic error instead of guessing a shape. Nested values
--- are taken as-is; express JSON null fields as json.null.
function json.array(t)
  if type(t) ~= "table" then
    error("invalid table: expected table for json.array, got " .. type(t))
  end
  local n = 0
  for k in pairs(t) do
    if type(k) ~= "number" then
      error("invalid table: mixed or invalid key types")
    end
    n = n + 1
  end
  if n ~= #t then
    error("invalid table: sparse array")
  end
  local res = {}
  for i = 1, n do
    res[i] = t[i]
  end
  return setmetatable(res, ARRAY_MT)
end

--- Build an explicit JSON object value from a Lua table with string keys.
--- Entries are copied into a freshly tagged table; non-string keys raise a
--- precise deterministic error. Express JSON null fields as json.null (plain
--- Lua nil fields cannot exist in a Lua table and are absent, not null).
function json.object(t)
  if type(t) ~= "table" then
    error("invalid table: expected table for json.object, got " .. type(t))
  end
  local res = {}
  for k, v in pairs(t) do
    if type(k) ~= "string" then
      error("invalid table: mixed or invalid key types")
    end
    res[k] = v
  end
  return setmetatable(res, OBJECT_MT)
end

--- Strict JSON number grammar: -?(0|[1-9][0-9]*)(\.[0-9]+)?([eE][+-]?[0-9]+)?
--- Deliberately stricter than tonumber(), which accepts hex and other
--- non-JSON spellings (#11183).
local function valid_number_syntax(s)
  local i, n = 1, #s
  if n == 0 then
    return false
  end
  if s:sub(i, i) == "-" then
    i = i + 1
  end
  local int_start = i
  while i <= n do
    local b = s:byte(i)
    if b < 48 or b > 57 then break end
    i = i + 1
  end
  local int_len = i - int_start
  if int_len == 0 then
    return false
  end
  -- Only "0" itself may lead; JSON forbids "01".
  if int_len > 1 and s:byte(int_start) == 48 then
    return false
  end
  if s:byte(i) == 46 then -- '.'
    i = i + 1
    local frac_start = i
    while i <= n do
      local b = s:byte(i)
      if b < 48 or b > 57 then break end
      i = i + 1
    end
    if i == frac_start then
      return false
    end
  end
  local e_byte = s:byte(i)
  if e_byte == 101 or e_byte == 69 then -- 'e' / 'E'
    i = i + 1
    local sign = s:byte(i)
    if sign == 43 or sign == 45 then
      i = i + 1
    end
    local exp_start = i
    while i <= n do
      local b = s:byte(i)
      if b < 48 or b > 57 then break end
      i = i + 1
    end
    if i == exp_start then
      return false
    end
  end
  return i == n + 1
end

--- True for ordinary Lua numbers and for typed exact-lexeme json.number
--- values; false for everything else (including digit-strings).
function json.is_number(v)
  if type(v) == "number" then
    return true
  end
  local mt = getmetatable(v)
  return mt ~= nil and mt[CONTAINER_TAG] == "number"
end

--- Exact source lexeme of a typed json.number value, or nil for any other
--- value. Two decoded IDs correlate by comparing kind (is_number plus type)
--- and this lexeme or the plain number value.
function json.number_lexeme(v)
  local mt = getmetatable(v)
  if mt == nil or mt[CONTAINER_TAG] ~= "number" then
    return nil
  end
  return v[NUMBER_LEXEME_KEY]
end

--- Build a typed exact-lexeme JSON number from validated numeric text. The
--- lexeme is checked against strict JSON grammar and retained verbatim;
--- re-encoding emits exactly these bytes.
function json.number(lex)
  if type(lex) ~= "string" or not valid_number_syntax(lex) then
    local shown = type(lex) == "string" and lex or "<" .. type(lex) .. ">"
    if #shown > 32 then
      shown = string.sub(shown, 1, 29) .. "..."
    end
    error("invalid number lexeme '" .. shown .. "'")
  end
  return setmetatable({ [NUMBER_LEXEME_KEY] = lex }, NUMBER_MT)
end

-------------------------------------------------------------------------------
-- Unicode scalar and UTF-8 validity (#11194)
--
-- One strict contract for both directions: only valid Unicode scalars and
-- exact surrogate pairs decode; raw decoded bytes must form valid UTF-8
-- sequences; outbound strings are validated before any encoding work.
-- Invalid bytes never reach the wire, and failures carry bounded offset
-- metadata without echoing the whole string.
-------------------------------------------------------------------------------

local function scalar_to_utf8(n)
  -- Encodes exactly one valid Unicode scalar value; returns nil for anything
  -- else (surrogates or values above U+10FFFF). Callers convert nil into a
  -- typed failure instead of emitting raw invalid bytes.
  if n < 0 or n > 0x10ffff or (n >= 0xd800 and n <= 0xdfff) then
    return nil
  end
  local f = math.floor
  if n <= 0x7f then
    return string.char(n)
  elseif n <= 0x7ff then
    return string.char(f(n / 64) + 192, n % 64 + 128)
  elseif n <= 0xffff then
    return string.char(f(n / 4096) + 224, f(n % 4096 / 64) + 128, n % 64 + 128)
  end
  return string.char(f(n / 262144) + 240, f(n % 262144 / 4096) + 128,
                     f(n % 4096 / 64) + 128, n % 64 + 128)
end

--- Length of the valid UTF-8 sequence starting at byte i of s, or nil when
--- the bytes are a stray continuation, an overlong encoding, a UTF-8 encoding
--- of a surrogate code point, above U+10FFFF, or a truncated sequence.
local function utf8_seq_len(s, i)
  local n = #s
  local b1 = s:byte(i)
  local len
  if b1 >= 0xc2 and b1 <= 0xdf then
    len = 2 -- c2..df excludes overlong 2-byte forms by construction
  elseif b1 >= 0xe0 and b1 <= 0xef then
    len = 3
  elseif b1 >= 0xf0 and b1 <= 0xf4 then
    len = 4 -- f5..ff would encode beyond U+10FFFF
  else
    return nil
  end
  if i + len - 1 > n then
    return nil -- truncated sequence
  end
  local b2 = s:byte(i + 1)
  if b2 < 0x80 or b2 > 0xbf then
    return nil
  end
  if len == 2 then
    return 2
  end
  if len == 3 then
    if b1 == 0xe0 and b2 < 0xa0 then
      return nil -- overlong 3-byte form
    end
    if b1 == 0xed and b2 >= 0xa0 then
      return nil -- UTF-8 encoding of a surrogate code point D800..DFFF
    end
    local b3 = s:byte(i + 2)
    if b3 < 0x80 or b3 > 0xbf then
      return nil
    end
    return 3
  end
  if b1 == 0xf0 and b2 < 0x90 then
    return nil -- overlong 4-byte form
  end
  if b1 == 0xf4 and b2 > 0x8f then
    return nil -- value above U+10FFFF
  end
  local b3 = s:byte(i + 2)
  if b3 < 0x80 or b3 > 0xbf then
    return nil
  end
  local b4 = s:byte(i + 3)
  if b4 < 0x80 or b4 > 0xbf then
    return nil
  end
  return 4
end

--- Byte offset of the first invalid UTF-8 sequence in s, or nil when the
--- entire string is valid.
local function first_invalid_utf8(s)
  local i, n = 1, #s
  while i <= n do
    local b = s:byte(i)
    if b < 0x80 then
      i = i + 1
    else
      local len = utf8_seq_len(s, i)
      if not len then
        return i
      end
      i = i + len
    end
  end
  return nil
end

-- (#11183) The historical json.number_flag = "{{json::num}}" prefix protocol
-- was removed: ordinary strings could forge numbers through it, and long
-- integers lost their exact lexeme. Numbers are now represented by plain Lua
-- integers within the exact integer range and by typed json.number lexeme
-- values everywhere else.

-------------------------------------------------------------------------------
-- Structural budgets (#11186)
--
-- Reviewed defaults, chosen from real Lite XL LSP/configuration shapes plus a
-- wide safety margin:
--   nesting depth 128: the deepest legitimate capability/configuration
--     payloads observed in LSP practice stay under ~30 levels; 128 is more
--     than 4x that margin while sitting far below any interpreter recursion
--     limit (each decoded level costs about two Lua frames).
--   node count 65536: a large diagnostics/completion batch (thousands of
--     items at ~20 nodes each) fits with room to spare; hostile tiny-byte
--     bodies hit the depth bound long before meaningful node counts.
-- A later large-response observation can raise these with evidence; nothing
-- in server/workspace input can raise them. Client-configuration wiring may
-- only lower them and lands with the consumer leaves, not here.
-------------------------------------------------------------------------------

local DEPTH_LIMIT = 128
local NODE_LIMIT = 65536

-------------------------------------------------------------------------------
-- Encode
-------------------------------------------------------------------------------

local encode

local escape_char_map = {
  [ "\\" ] = "\\",
  [ "\"" ] = "\"",
  [ "\b" ] = "b",
  [ "\f" ] = "f",
  [ "\n" ] = "n",
  [ "\r" ] = "r",
  [ "\t" ] = "t",
}

local escape_char_map_inv = { [ "/" ] = "/" }
for k, v in pairs(escape_char_map) do
  escape_char_map_inv[v] = k
end


local function escape_char(c)
  return "\\" .. (escape_char_map[c] or string.format("u%04x", c:byte()))
end


-- One shared encode context (#11186): circular-reference stack, container
-- depth, and a per-document node count. Created fresh per json.encode call,
-- so concurrent/reentrant calls cannot share budget state.
local function count_node(st)
  st.nodes = st.nodes + 1
  if st.nodes > NODE_LIMIT then
    error("cannot encode: node count exceeds maximum " .. NODE_LIMIT)
  end
end

local function enter_container(st)
  st.depth = st.depth + 1
  if st.depth > DEPTH_LIMIT then
    error("cannot encode: nesting exceeds maximum depth " .. DEPTH_LIMIT)
  end
end

local function encode_nil(val, st)
  count_node(st)
  return "null"
end


-- One shared encode context (#11186): circular-reference stack, container
-- depth, and a per-document node count. Created fresh per json.encode call,
-- so concurrent/reentrant calls cannot share budget state.
local function count_node(st)
  st.nodes = st.nodes + 1
  if st.nodes > NODE_LIMIT then
    error("cannot encode: node count exceeds maximum " .. NODE_LIMIT)
  end
end

local function enter_container(st)
  st.depth = st.depth + 1
  if st.depth > DEPTH_LIMIT then
    error("cannot encode: nesting exceeds maximum depth " .. DEPTH_LIMIT)
  end
end

local function encode_table(val, st)
  -- Circular reference stays the earliest, distinct structural error.
  if st.stack[val] then error("circular reference") end

  count_node(st)
  enter_container(st)

  st.stack[val] = true

  local res = {}
  local out

  -- Typed container identity wins over shape guessing (#11136). Untagged
  -- tables keep the upstream heuristic: obvious non-empty array or empty
  -- table takes the array branch, everything else the object branch.
  local mt = getmetatable(val)
  local tag = mt and mt[CONTAINER_TAG] or nil

  if tag == "array" or (tag == nil and (rawget(val, 1) ~= nil or next(val) == nil)) then
    -- Treat as array -- check keys are valid and it is not sparse
    local n = 0
    for k in pairs(val) do
      if type(k) ~= "number" then
        error("invalid table: mixed or invalid key types")
      end
      n = n + 1
    end
    if n ~= #val then
      error("invalid table: sparse array")
    end
    -- Encode
    for i, v in ipairs(val) do
      table.insert(res, encode(v, st))
    end
    if #res > 0 then
      out = "[" .. table.concat(res, ",") .. "]"
    elseif tag == "array" then
      -- An explicitly typed empty array keeps its [] identity; an untagged
      -- empty plain table preserves the upstream {} compatibility default.
      out = "[]"
    else
      out = "{}"
    end

  else
    -- Treat as an object (typed or by the upstream heuristic)
    for k, v in pairs(val) do
      if type(k) ~= "string" then
        error("invalid table: mixed or invalid key types")
      end
      table.insert(res, encode(k, st) .. ":" .. encode(v, st))
    end
    out = "{" .. table.concat(res, ",") .. "}"
  end

  st.stack[val] = nil
  st.depth = st.depth - 1
  return out
end


local function encode_string(val, st)
  -- (#11194) Outbound strings must be valid UTF-8 before any encoding work;
  -- the raise happens before a caller could observe any frame content and
  -- carries only bounded offset metadata, never the string body.
  local bad = first_invalid_utf8(val)
  if bad then
    error("cannot encode string: invalid UTF-8 at byte offset " .. bad)
  end
  count_node(st)
  return '"' .. val:gsub('[%z\1-\31\\"]', escape_char) .. '"'
end


local function encode_number(val, st)
  -- Check for NaN, -inf and inf
  if val ~= val or val <= -math.huge or val >= math.huge then
    error("unexpected number value '" .. tostring(val) .. "'")
  end
  count_node(st)
  -- (#11183) Lua 5.4 integers render exactly through tostring; %.14g would
  -- corrupt integers beyond 14 significant digits.
  if math_type(val) == "integer" then
    return integer_lexeme(val)
  end
  return string.format("%.14g", val)
end


local function encode_boolean(val, st)
  count_node(st)
  return tostring(val)
end


local type_func_map = {
  [ "nil"     ] = encode_nil,
  [ "table"   ] = encode_table,
  [ "string"  ] = encode_string,
  [ "number"  ] = encode_number,
  [ "boolean" ] = encode_boolean,
}


encode = function(val, st)
  -- The unique null identity encodes as null before table dispatch; it is a
  -- table internally but must never take the container paths.
  if val == json.null then
    count_node(st)
    return "null"
  end
  -- Typed exact-lexeme numbers (#11183) emit their validated bytes verbatim,
  -- also before any container/string dispatch could misread them. Each is a
  -- single structural node (#11186).
  if type(val) == "table" then
    local mt = getmetatable(val)
    if mt ~= nil and mt[CONTAINER_TAG] == "number" then
      count_node(st)
      local lex = rawget(val, NUMBER_LEXEME_KEY)
      if type(lex) ~= "string" then
        error("invalid json.number value: missing lexeme")
      end
      return lex
    end
  end
  local t = type(val)
  local f = type_func_map[t]
  if f then
    return f(val, st)
  end
  error("unexpected type '" .. t .. "'")
end


function json.encode(val, prettify)
  local out = ( encode(val, { stack = {}, depth = 0, nodes = 0 }) )
  if prettify then
    return json.prettify(out)
  end
  return out
end


-------------------------------------------------------------------------------
-- Decode
--
-- Decode failures are terminal and typed. Every parser helper raises a private
-- sentinel error object at its first fault; nothing continues past it. The
-- sentinel is caught only inside json.decode, which returns the public shape:
--
--   success: decoded_value                      (one return value)
--   failure: nil, err                           (typed error table)
--
-- err fields:
--   kind        always "json.decode_error"
--   reason      stable reason token, see REASONS below
--   byte_offset 1-based byte offset of the fault
--   line        1-based line of the fault
--   column      1-based column of the fault
--   message     bounded human description "<msg> at line L col C"
--
-- Messages echo at most 16 bytes of the offending fragment and never include
-- the complete input body. A non-string argument remains a raised argument
-- error: that is programmer misuse, not malformed input.
-------------------------------------------------------------------------------

local parse

-- Unique identity tag proving an error object came from this parser instance;
-- foreign errors caught at the boundary are re-raised rather than masked.
local DECODE_ERROR_TAG = {}

local REASONS = {
  unexpected_end_of_input    = true,
  unexpected_character       = true,
  unterminated_string        = true,
  control_character_in_string= true,
  invalid_escape             = true,
  invalid_unicode_escape     = true,
  invalid_utf8               = true,
  nesting_depth_exceeded     = true,
  node_count_exceeded        = true,
  invalid_number             = true,
  invalid_literal            = true,
  expected_string_key        = true,
  expected_colon_after_key   = true,
  expected_array_delimiter   = true,
  expected_object_delimiter  = true,
  trailing_garbage           = true,
}

-- Bound echoed fragments so hostile bodies cannot inflate diagnostics (#11155).
local function bound(s)
  if #s > 16 then
    return string.sub(s, 1, 13) .. "..."
  end
  return s
end

local function new_decode_error(str, idx, reason, msg)
  assert(REASONS[reason], "unknown decode reason token")
  local line_count = 1
  local col_count = 1
  for i = 1, idx - 1 do
    col_count = col_count + 1
    if str:sub(i, i) == "\n" then
      line_count = line_count + 1
      col_count = 1
    end
  end
  local formatted = string.format("%s at line %d col %d", msg, line_count, col_count)
  return {
    kind = "json.decode_error",
    reason = reason,
    byte_offset = idx,
    line = line_count,
    column = col_count,
    message = formatted,
    [DECODE_ERROR_TAG] = true,
  }
end

local function decode_error(str, idx, reason, msg)
  error(new_decode_error(str, idx, reason, msg), 0)
end


local function create_set(...)
  local res = {}
  for i = 1, select("#", ...) do
    res[ select(i, ...) ] = true
  end
  return res
end

local space_chars   = create_set(" ", "\t", "\r", "\n")
local delim_chars   = create_set(" ", "\t", "\r", "\n", "]", "}", ",")
local escape_chars  = create_set("\\", "/", '"', "b", "f", "n", "r", "t", "u")
local literals      = create_set("true", "false", "null")

local literal_map = {
  [ "true"  ] = true,
  [ "false" ] = false,
  -- (#11136) JSON null keeps its unique identity instead of collapsing to
  -- Lua nil, so array slots and object fields holding null are preserved.
  [ "null"  ] = json.null,
}


local function next_char(str, idx, set, negate)
  for i = idx, #str do
    if set[str:sub(i, i)] ~= negate then
      return i
    end
  end
  return #str + 1
end

-- (#11194) The old codepoint_to_utf8/parse_unicode_escape pair accepted every
-- numeric value up to 0x10ffff including lone surrogates and composed any
-- following \uXXXX as a low half; scalar_to_utf8 plus the strict escape parser
-- in parse_string replace them.


local function parse_string(str, i)
  local res = ""
  local j = i + 1
  local k = j

  while j <= #str do
    local x = str:byte(j)

    if x < 32 then
      decode_error(str, j, "control_character_in_string", "control character in string")

    elseif x == 92 then -- `\`: Escape
      res = res .. str:sub(k, j - 1)
      j = j + 1
      local c = str:sub(j, j)
      if c == "u" then
        -- (#11194) Strict scalar/pair validation: a lone \uXXXX is accepted
        -- only as a non-surrogate scalar; high surrogates must pair with an
        -- immediately following \uDC00..\uDFFF; low surrogates never stand
        -- alone. Invalid forms fail typed instead of emitting raw bytes.
        local pos = j + 1 -- first hex digit
        local h1 = str:match("^%x%x%x%x", pos)
        if not h1 then
          decode_error(str, j - 1, "invalid_unicode_escape", "invalid unicode escape in string")
        end
        local cp1 = tonumber(h1, 16)
        local piece
        if cp1 >= 0xd800 and cp1 <= 0xdbff then
          local h2
          if str:sub(pos + 4, pos + 5) == "\\u" then
            h2 = str:match("^%x%x%x%x", pos + 6)
          end
          if not h2 then
            decode_error(str, j - 1, "invalid_unicode_escape",
              "high surrogate must be followed by a low surrogate escape")
          end
          local cp2 = tonumber(h2, 16)
          if cp2 < 0xdc00 or cp2 > 0xdfff then
            decode_error(str, j - 1, "invalid_unicode_escape",
              "high surrogate followed by non-low escape '" .. bound(h2) .. "'")
          end
          piece = scalar_to_utf8(0x10000 + (cp1 - 0xd800) * 0x400 + (cp2 - 0xdc00))
          j = pos + 9 -- last hex digit of the low escape
        elseif cp1 >= 0xdc00 and cp1 <= 0xdfff then
          decode_error(str, j - 1, "invalid_unicode_escape", "lone low surrogate escape")
        else
          piece = scalar_to_utf8(cp1)
          j = pos + 3 -- last hex digit of this escape
        end
        -- Unreachable for validated inputs; kept as an honest typed guard.
        if not piece then
          decode_error(str, j - 1, "invalid_unicode_escape",
            string.format("invalid unicode escape composing codepoint '%x'", cp1))
        end
        res = res .. piece
      else
        if not escape_chars[c] then
          decode_error(str, j - 1, "invalid_escape", "invalid escape char '" .. bound(c) .. "' in string")
        end
        res = res .. escape_char_map_inv[c]
      end
      k = j + 1

    elseif x == 34 then -- `"`: End of string
      res = res .. str:sub(k, j - 1)
      return res, j + 1

    elseif x >= 128 then
      -- (#11194) Raw non-ASCII bytes must form valid UTF-8 sequences. The
      -- bytes themselves are copied verbatim by the chunk appends above;
      -- validation only decides whether they may pass.
      local seq_len = utf8_seq_len(str, j)
      if not seq_len then
        decode_error(str, j, "invalid_utf8", "invalid UTF-8 sequence in string")
      end
      j = j + seq_len - 1 -- bottom increment lands past the sequence
    end

    j = j + 1
  end

  decode_error(str, i, "unterminated_string", "expected closing quote for string")
end


local function parse_number(str, i)
  local x = next_char(str, i, delim_chars)
  local s = str:sub(i, x - 1)
  -- Strict grammar first: tonumber() alone accepts non-JSON forms such as
  -- hex ("0x10") that must not decode (#11183).
  if not valid_number_syntax(s) then
    decode_error(str, i, "invalid_number", "invalid number '" .. bound(s) .. "'")
  end
  local n = tonumber(s)
  -- Overflow to infinity fails honestly at the decode boundary instead of
  -- surfacing later from the encoder.
  if n ~= n or n == math.huge or n == -math.huge then
    decode_error(str, i, "invalid_number", "number out of range '" .. bound(s) .. "'")
  end
  -- Reviewed numeric policy (#11183): an integer that fits int64 exactly and
  -- whose canonical form equals its lexeme stays an ordinary Lua integer;
  -- every other valid number keeps its exact lexeme in a typed value rather
  -- than being rounded through a float.
  if math_type(n) == "integer" and integer_lexeme(n) == s then
    return n, x
  end
  return json.number(s), x
end


local function parse_literal(str, i)
  local x = next_char(str, i, delim_chars)
  local word = str:sub(i, x - 1)
  if not literals[word] then
    decode_error(str, i, "invalid_literal", "invalid literal '" .. bound(word) .. "'")
  end
  return literal_map[word], x
end


local function parse_array(str, i, st)
  -- (#11186) Check the depth budget before allocating or descending.
  st.depth = st.depth + 1
  if st.depth > DEPTH_LIMIT then
    decode_error(str, i, "nesting_depth_exceeded",
      "nesting exceeds maximum depth " .. DEPTH_LIMIT)
  end
  -- (#11136) decoded arrays carry the private array tag so identity, including
  -- the empty case, survives past decode.
  local res = setmetatable({}, ARRAY_MT)
  local n = 1
  i = i + 1
  while 1 do
    local x
    i = next_char(str, i, space_chars, true)
    -- Empty / end of array?
    if str:sub(i, i) == "]" then
      i = i + 1
      break
    end
    -- Read token
    x, i = parse(str, i, st)
    res[n] = x
    n = n + 1
    -- Next token
    i = next_char(str, i, space_chars, true)
    local chr = str:sub(i, i)
    i = i + 1
    if chr == "]" then break end
    if chr ~= "," then
      decode_error(str, i - 1, "expected_array_delimiter", "expected ']' or ','")
    end
  end
  st.depth = st.depth - 1
  return res, i
end


local function parse_object(str, i, st)
  -- (#11186) Check the depth budget before allocating or descending.
  st.depth = st.depth + 1
  if st.depth > DEPTH_LIMIT then
    decode_error(str, i, "nesting_depth_exceeded",
      "nesting exceeds maximum depth " .. DEPTH_LIMIT)
  end
  -- (#11136) decoded objects carry the private object tag so identity,
  -- including the empty case, survives past decode.
  local res = setmetatable({}, OBJECT_MT)
  i = i + 1
  while 1 do
    local key, val
    i = next_char(str, i, space_chars, true)
    -- Empty / end of object?
    if str:sub(i, i) == "}" then
      i = i + 1
      break
    end
    -- Read key
    if str:sub(i, i) ~= '"' then
      decode_error(str, i, "expected_string_key", "expected string for key")
    end
    key, i = parse(str, i, st)
    -- Read ':' delimiter
    i = next_char(str, i, space_chars, true)
    if str:sub(i, i) ~= ":" then
      decode_error(str, i, "expected_colon_after_key", "expected ':' after key")
    end
    i = next_char(str, i + 1, space_chars, true)
    -- Read value
    val, i = parse(str, i, st)
    -- Set
    res[key] = val
    -- Next token
    i = next_char(str, i, space_chars, true)
    local chr = str:sub(i, i)
    i = i + 1
    if chr == "}" then break end
    if chr ~= "," then
      decode_error(str, i - 1, "expected_object_delimiter", "expected '}' or ','")
    end
  end
  st.depth = st.depth - 1
  return res, i
end


local char_func_map = {
  [ '"' ] = parse_string,
  [ "0" ] = parse_number,
  [ "1" ] = parse_number,
  [ "2" ] = parse_number,
  [ "3" ] = parse_number,
  [ "4" ] = parse_number,
  [ "5" ] = parse_number,
  [ "6" ] = parse_number,
  [ "7" ] = parse_number,
  [ "8" ] = parse_number,
  [ "9" ] = parse_number,
  [ "-" ] = parse_number,
  [ "t" ] = parse_literal,
  [ "f" ] = parse_literal,
  [ "n" ] = parse_literal,
  [ "[" ] = parse_array,
  [ "{" ] = parse_object,
}


parse = function(str, idx, st)
  -- (#11186) Every decoded value consumes exactly one node; the budget is
  -- checked before any allocation for that value.
  st.nodes = st.nodes + 1
  if st.nodes > NODE_LIMIT then
    decode_error(str, idx, "node_count_exceeded",
      "node count exceeds maximum " .. NODE_LIMIT)
  end
  local chr = str:sub(idx, idx)
  local f = char_func_map[chr]
  if f then
    return f(str, idx, st)
  end
  if idx > #str then
    decode_error(str, idx, "unexpected_end_of_input", "unexpected end of input")
  end
  decode_error(str, idx, "unexpected_character", "unexpected character '" .. bound(chr) .. "'")
end

---
--- Compatibility projection of the most recent decode failure message.
--- Superseded by the second return value of json.decode; slated for removal
--- once every consumer reads the typed result directly. Never use it for new
--- call sites.
---
function json.last_error()
  return error_message
end

---
--- Decode one complete JSON document.
---
--- Success returns exactly one value: the decoded result. That value is never
--- nil: JSON null decodes to the json.null identity and JSON false to false,
--- so a nil first return value means failure. Decoded arrays/objects carry
--- private typed-container tags (#11136); numbers decode to plain Lua
--- integers within the exact integer range and to typed exact-lexeme
--- json.number values otherwise (#11183).
--- Malformed input returns nil plus one typed error table (see the Decode
--- section above). A non-string argument raises an argument error (programmer
--- misuse).
---
function json.decode(str)
  if type(str) ~= "string" then
    error("expected argument of type string, got " .. type(str))
  end
  local ok, res, idx = pcall(parse, str, next_char(str, 1, space_chars, true),
    { depth = 0, nodes = 0 })
  if not ok then
    if type(res) == "table" and res[DECODE_ERROR_TAG] then
      error_message = res.message
      return nil, res
    end
    -- A genuine internal fault must not masquerade as malformed input.
    error(res, 0)
  end
  idx = next_char(str, idx, space_chars, true)
  if idx <= #str then
    local err = new_decode_error(str, idx, "trailing_garbage", "trailing garbage")
    error_message = err.message
    return nil, err
  end
  error_message = ""
  return res
end

local function indent(code, level, indent_width)
  return string.rep(" ", level * indent_width) .. code
end

--- Implemented some json prettifier but not a parser so
--- don't expect it to give you parsing errors :D
--- @param text string The json string
--- @param indent_width? integer The amount of spaces per indentation
--- @return string
function json.prettify(text, indent_width)
  if type(text) ~= "string" then
    return ""
  end

  local out = ""
  indent_width = indent_width or 2

  local indent_level = 0
  local reading_literal = false
  local previous_was_escape = false
  local inside_string = false
  local in_value = false
  local last_was_bracket = false
  local string_char = ""
  local last_char = ""

  for char in text:gmatch(".") do
    if (char == "{" or char == "[") and not inside_string then
      if not in_value or last_was_bracket then
        out = out .. indent(char, indent_level, indent_width) .. "\n"
      else
        out = out .. char .. "\n"
      end
      last_was_bracket = true
      in_value = false
      indent_level = indent_level + 1
    elseif (char == '"' or char == "'") and not inside_string then
      inside_string = true
      string_char = char
      if not in_value then
        out = out .. indent(char, indent_level, indent_width)
      else
        out = out .. char
      end
    elseif inside_string then
      local pe_set = false
      if char == "\\" and previous_was_escape then
        previous_was_escape = false
      elseif char == "\\" then
        previous_was_escape = true
        pe_set = true
      end
      out = out .. char
      if char == string_char and not previous_was_escape then
        inside_string = false
      elseif previous_was_escape and not pe_set then
        previous_was_escape = false
      end
    elseif char == ":" then
      in_value = true
      last_was_bracket = false
      out = out .. char .. " "
    elseif char == "," then
      in_value = false
      reading_literal = false
      out = out .. char .. "\n"
    elseif char == "}" or char == "]" then
      indent_level = indent_level - 1
      if
        (char == "}" and last_char == "{")
        or
        (char == "]" and last_char == "[")
      then
        out = out:gsub("%s*\n$", "") .. char
      else
        out = out .. "\n" .. indent(char, indent_level, indent_width)
      end
    elseif not char:match("%s") and not reading_literal then
      reading_literal = true
      if not in_value or last_was_bracket then
        out = out .. indent(char, indent_level, indent_width)
        last_was_bracket = false
      else
        out = out .. char
      end
    elseif not char:match("%s") then
      out = out .. char
    end

    if not char:match("%s") then
      last_char = char
    end
  end

  return out
end


return json

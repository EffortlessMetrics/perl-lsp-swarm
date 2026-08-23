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
-- incidental Lua exceptions. Encode behavior is unchanged from upstream.

local json = { _version = "0.1.2" }

-- Compatibility projection only: message of the most recent failed decode.
-- Superseded by the second return value of json.decode; retained until all
-- consumers read the typed result directly, then removed. It is not the
-- load-bearing error transport and never carries state between decodes.
local error_message = ""

-- Lets us explicitly add null values to table elements
json.null = "{{json::null}}"

-- Treat numbers longer than 14 digits as a string by adding this to the
-- beginning of the string for encoder to recognize. This prevents any data
-- loss due to lua 5.2 not supporting big integer numbers and converting big
-- integers to floats. The drawback is that the user should manually convert
-- these strings to a number. Numbers with less than 15 digits are not affected.
json.number_flag = "{{json::num}}"
local number_flag_len = #json.number_flag

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


local function encode_nil(val)
  return "null"
end


local function encode_table(val, stack)
  local res = {}
  stack = stack or {}

  -- Circular reference?
  if stack[val] then error("circular reference") end

  stack[val] = true

  if rawget(val, 1) ~= nil or next(val) == nil then
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
      table.insert(res, encode(v, stack))
    end
    stack[val] = nil
    if #res > 0 then
      return "[" .. table.concat(res, ",") .. "]"
    else
      return "{}"
    end

  else
    -- Treat as an object
    for k, v in pairs(val) do
      if type(k) ~= "string" then
        error("invalid table: mixed or invalid key types")
      end
      table.insert(res, encode(k, stack) .. ":" .. encode(v, stack))
    end
    stack[val] = nil
    return "{" .. table.concat(res, ",") .. "}"
  end
end


local function encode_string(val)
  if val == json.null then
    return "null"
  elseif
    #val > number_flag_len
    and
    string.sub(val, 1, number_flag_len) == json.number_flag
  then
    local num = string.sub(val, number_flag_len+1)
    return num
  end
  return '"' .. val:gsub('[%z\1-\31\\"]', escape_char) .. '"'
end


local function encode_number(val)
  -- Check for NaN, -inf and inf
  if val ~= val or val <= -math.huge or val >= math.huge then
    error("unexpected number value '" .. tostring(val) .. "'")
  end
  return string.format("%.14g", val)
end


local type_func_map = {
  [ "nil"     ] = encode_nil,
  [ "table"   ] = encode_table,
  [ "string"  ] = encode_string,
  [ "number"  ] = encode_number,
  [ "boolean" ] = tostring,
}


encode = function(val, stack)
  local t = type(val)
  local f = type_func_map[t]
  if f then
    return f(val, stack)
  end
  error("unexpected type '" .. t .. "'")
end


function json.encode(val, prettify)
  local out = ( encode(val) )
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
  [ "null"  ] = nil,
}


local function next_char(str, idx, set, negate)
  for i = idx, #str do
    if set[str:sub(i, i)] ~= negate then
      return i
    end
  end
  return #str + 1
end


local function codepoint_to_utf8(n, str, idx)
  -- http://scripts.sil.org/cms/scripts/page.php?site_id=nrsi&id=iws-appendixa
  local f = math.floor
  if n <= 0x7f then
    return string.char(n)
  elseif n <= 0x7ff then
    return string.char(f(n / 64) + 192, n % 64 + 128)
  elseif n <= 0xffff then
    return string.char(f(n / 4096) + 224, f(n % 4096 / 64) + 128, n % 64 + 128)
  elseif n <= 0x10ffff then
    return string.char(f(n / 262144) + 240, f(n % 262144 / 4096) + 128,
                       f(n % 4096 / 64) + 128, n % 64 + 128)
  end
  -- Malformed surrogate composition must be a typed decode failure, never a
  -- raw Lua exception escaping the codec. Scalar-value validation itself is
  -- owned by #11194; this only guarantees termination.
  decode_error(str, idx, "invalid_unicode_escape",
    string.format("invalid unicode escape composing codepoint '%x'", n))
end


local function parse_unicode_escape(s, str, idx)
  local n1 = tonumber( s:sub(1, 4),  16 )
  local n2 = tonumber( s:sub(7, 10), 16 )
   -- Surrogate pair?
  if n2 then
    return codepoint_to_utf8((n1 - 0xd800) * 0x400 + (n2 - 0xdc00) + 0x10000, str, idx)
  else
    return codepoint_to_utf8(n1, str, idx)
  end
end


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
        local hex = str:match("^[dD][89aAbB]%x%x\\u%x%x%x%x", j + 1)
                 or str:match("^%x%x%x%x", j + 1)
                 or decode_error(str, j - 1, "invalid_unicode_escape", "invalid unicode escape in string")
        res = res .. parse_unicode_escape(hex, str, j - 1)
        j = j + #hex
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
    end

    j = j + 1
  end

  decode_error(str, i, "unterminated_string", "expected closing quote for string")
end


local function parse_number(str, i)
  local x = next_char(str, i, delim_chars)
  local s = str:sub(i, x - 1)
  local n = nil
  if #s > 14 then
    n = json.number_flag .. s
  else
    n = tonumber(s)
  end
  if not n then
    decode_error(str, i, "invalid_number", "invalid number '" .. bound(s) .. "'")
  end
  return n, x
end


local function parse_literal(str, i)
  local x = next_char(str, i, delim_chars)
  local word = str:sub(i, x - 1)
  if not literals[word] then
    decode_error(str, i, "invalid_literal", "invalid literal '" .. bound(word) .. "'")
  end
  return literal_map[word], x
end


local function parse_array(str, i)
  local res = {}
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
    x, i = parse(str, i)
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
  return res, i
end


local function parse_object(str, i)
  local res = {}
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
    key, i = parse(str, i)
    -- Read ':' delimiter
    i = next_char(str, i, space_chars, true)
    if str:sub(i, i) ~= ":" then
      decode_error(str, i, "expected_colon_after_key", "expected ':' after key")
    end
    i = next_char(str, i + 1, space_chars, true)
    -- Read value
    val, i = parse(str, i)
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


parse = function(str, idx)
  local chr = str:sub(idx, idx)
  local f = char_func_map[chr]
  if f then
    return f(str, idx)
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
--- Success returns exactly one value: the decoded result. That result may be
--- nil (JSON null) or false (JSON false).
--- Malformed input returns nil plus one typed error table (see the Decode
--- section above). Test the second return value to distinguish failure from
--- valid null/false results.
--- A non-string argument raises an argument error (programmer misuse).
---
function json.decode(str)
  if type(str) ~= "string" then
    error("expected argument of type string, got " .. type(str))
  end
  local ok, res, idx = pcall(parse, str, next_char(str, 1, space_chars, true))
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

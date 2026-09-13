-- Deterministic focused tests for the SENSITIVE-TRACE ADMISSION SURFACE in
-- clients/lite-xl/upstream/init.lua and clients/lite-xl/upstream/server.lua
-- (#11155 residual slice).
--
-- Run:
--   lua clients/lite-xl/tests/init_trace_admission_test.lua
--     [path-to-init-module] [path-to-server-module]
-- Default paths are ../upstream/init.lua and ../upstream/server.lua
-- relative to this file.
--
-- Seam owned: the USER-FACING CONFIGURATION AND HELP PROJECTION of the
-- options that admit protocol or server-emitted content into a readable
-- sink. #12015 already landed the transport half of #11155 - automatic
-- failure logs are bounded and content-free, stderr draining is separated
-- from capped retention, and `util.jsonprettify` guards the trace append.
-- This suite owns the half that decides whether a user can give INFORMED
-- opt-in: what `config_spec` and the LuaCATS field annotations actually
-- tell them before they switch a trace on.
--
-- The admission options and what each one can expose:
--
--   log_file           complete JSON protocol -> a durable file
--   log_server_stderr  server-emitted stderr  -> the editor log
--   force_verbosity_off  the client-wide suppressor for per-server verbose
--   Server default `verbose` / Server.verbose  complete protocol payloads
--
-- Contract pinned here:
--   A1  every admission option's settings-GUI description names the
--       content classes that option can actually expose, carries the
--       shared sensitivity marker, and KEEPS its original operational
--       meaning (an operational keyword survives);
--   A2  every admission option's LuaCATS field annotation carries the same
--       disclosure and cites the owning issue;
--   A3  NEGATIVE CONTROL - no non-admission option carries the sensitivity
--       marker, so blanket-appending the warning to every description
--       fails this suite instead of passing it;
--   A4  GUI descriptions stay within a readable bound, so disclosure
--       cannot be satisfied by dumping an unbounded paragraph into the
--       settings UI;
--   A5  CROSS-MODULE CONSISTENCY - `verbose` is declared twice in
--       server.lua (the Server class field and the default options table).
--       BOTH declarations must disclose; one honest declaration beside one
--       innocuous "debug the lsp client" declaration is exactly the defect
--       this slice repairs;
--   A6  every admission option still exists as a real config default /
--       server option, so the disclosure describes a live switch rather
--       than drifting off a removed one.
--
-- Red-first baseline: run against the pristine upstream base copies, whose
-- wording is byte-identical to current staged main for these surfaces:
--   lua clients/lite-xl/tests/init_trace_admission_test.lua \
--     clients/lite-xl/leaves/base/init.lua \
--     clients/lite-xl/leaves/base/server.lua
-- Observed pristine baseline: 17 failed / 21 passed. Observed patched
-- result: 38 passed, 0 failed.
--
-- Mutation falsifiers of the PATCHED source (each mechanically verified to
-- fail this suite against a mutated copy):
--   1. restore "Absolute path to a '.log' file for logging all json." as
--      the Log File description -> A1 content-class rows fail;
--   2. restore "True to debug the lsp client when developing it" on the
--      server default options `verbose` -> exactly the two A5
--      default-options rows fail while the Server class-field rows still
--      pass (the asymmetry is discriminated, not assumed);
--   3. append the sensitivity marker to every config_spec description ->
--      A3 fails;
--   4. replace a description with the disclosure alone, dropping its
--      operational keyword -> A1's meaning-preservation row fails.
--      Verified on log_file ('.log') and on force_verbosity_off ('even if
--      a server'), whose operational keyword is deliberately chosen so it
--      does NOT also occur in that option's disclosure sentence.
--
-- No framework: plain asserts, one process, deterministic, exit code
-- carries the result. Compatible with the Lite XL Lua runtime family (5.4).

local init_module_path = arg and arg[1] or nil
local server_module_path = arg and arg[2] or nil

local here = debug.getinfo(1, "S").source:sub(2):match("^(.*)[/\\]") or "."

if not init_module_path then
  init_module_path = here .. "/../upstream/init.lua"
end
if not server_module_path then
  server_module_path = here .. "/../upstream/server.lua"
end

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

local function read_file(path)
  local fh = assert(io.open(path, "rb"), "cannot read " .. path)
  local text = fh:read("*a")
  fh:close()
  return text
end

---Case-insensitive literal containment.
local function says(text, needle)
  return tostring(text):lower():find(needle:lower(), 1, true) ~= nil
end

local function says_all(text, needles)
  local missing = {}
  for _, needle in ipairs(needles) do
    if not says(text, needle) then missing[#missing + 1] = needle end
  end
  return #missing == 0, missing
end

-- ---------------------------------------------------------------------------
-- The disclosure contract.
--
-- `classes` are the content kinds that option can actually put in front of
-- a reader; they are the substantive part of the obligation. `operational`
-- is a keyword from the option's original meaning that must survive, so a
-- disclosure cannot be bought by deleting what the setting does.
-- ---------------------------------------------------------------------------

local SENSITIVITY_MARKER = "sensitive"
local OWNER_ISSUE = "#11155"
local MAX_DESCRIPTION_BYTES = 240

local ADMISSIONS = {
  {
    path = "log_file",
    label = "Log File",
    classes = { "source code", "configuration" },
    operational = ".log",
    annotation_classes = { "source code", "configuration values" },
  },
  {
    path = "log_server_stderr",
    label = "Log Standard Error",
    classes = { "file paths", "source fragments" },
    operational = "stderr",
    annotation_classes = { "file paths", "source fragments" },
  },
  {
    path = "force_verbosity_off",
    label = "Force Verbosity Off",
    classes = { "protocol payloads", "source code" },
    -- "verbosity" would be a vacuous operational keyword here: the
    -- disclosure sentence uses the word too, so a description replaced by
    -- the disclosure alone would still satisfy it. The override semantics
    -- ("even if a server ...") appear ONLY in the operational half.
    operational = "even if a server",
    annotation_classes = { "protocol payloads", "source code" },
  },
}

local ADMISSION_PATHS = {}
for _, row in ipairs(ADMISSIONS) do ADMISSION_PATHS[row.path] = row end

-- ---------------------------------------------------------------------------
-- Load the exact staged module through the journey harness (#11103) so the
-- assertions read the REAL merged config table, not a re-parsed copy.
-- ---------------------------------------------------------------------------

local world = harness.new_world({ init_module = init_module_path })
local config = world.config

ok(type(config) == "table", "staged init.lua merges config.plugins.lsp")
ok(type(config.config_spec) == "table",
  "staged init.lua exposes a settings config_spec")

local spec_by_path = {}
for _, entry in ipairs(config.config_spec or {}) do
  if type(entry) == "table" and entry.path then
    spec_by_path[entry.path] = entry
  end
end

-- ---------------------------------------------------------------------------
-- A6: the disclosure describes live switches.
-- ---------------------------------------------------------------------------

for _, row in ipairs(ADMISSIONS) do
  ok(config[row.path] ~= nil,
    "A6 " .. row.path .. " is a real config default")
  ok(spec_by_path[row.path] ~= nil,
    "A6 " .. row.path .. " has a settings config_spec entry")
end

-- ---------------------------------------------------------------------------
-- A1: GUI descriptions disclose content classes, carry the marker, and keep
-- their operational meaning.  A4: they stay readable.
-- ---------------------------------------------------------------------------

for _, row in ipairs(ADMISSIONS) do
  local entry = spec_by_path[row.path]
  local description = entry and entry.description or ""

  ok(says(description, SENSITIVITY_MARKER),
    "A1 " .. row.path .. " description carries the sensitivity marker")

  local complete, missing = says_all(description, row.classes)
  ok(complete,
    "A1 " .. row.path .. " description names its exposed content classes"
      .. (complete and "" or (" (missing: " .. table.concat(missing, ", ") .. ")")))

  ok(says(description, row.operational),
    "A1 " .. row.path .. " description keeps its operational meaning ('"
      .. row.operational .. "')")

  ok(#description <= MAX_DESCRIPTION_BYTES,
    "A4 " .. row.path .. " description stays within "
      .. MAX_DESCRIPTION_BYTES .. " bytes (got " .. #description .. ")")
end

-- ---------------------------------------------------------------------------
-- A3: NEGATIVE CONTROL - non-admission options must NOT carry the marker.
-- Blanket-appending the warning everywhere is a different failure than
-- disclosing where it is true, and must be discriminated.
-- ---------------------------------------------------------------------------

local overreach = {}
for _, entry in ipairs(config.config_spec or {}) do
  if type(entry) == "table" and entry.path and not ADMISSION_PATHS[entry.path] then
    if says(entry.description or "", SENSITIVITY_MARKER) then
      overreach[#overreach + 1] = entry.path
    end
  end
end
ok(#overreach == 0,
  "A3 no non-admission option claims sensitivity"
    .. (#overreach > 0 and (": " .. table.concat(overreach, ", ")) or ""))

-- A3 is only meaningful if there ARE non-admission options to overreach
-- onto; pin that the control has a population.
local non_admission_count = 0
for _, entry in ipairs(config.config_spec or {}) do
  if type(entry) == "table" and entry.path and not ADMISSION_PATHS[entry.path] then
    non_admission_count = non_admission_count + 1
  end
end
ok(non_admission_count >= 5,
  "A3 control population is non-trivial (" .. non_admission_count
    .. " non-admission options)")

world.teardown()

-- ---------------------------------------------------------------------------
-- A2: LuaCATS field annotations disclose and cite the owner.
--
-- Annotations are comments, so they are read from source text.  Each block
-- is the run of `---` lines immediately preceding the `---@field <name>`
-- line, which is exactly what an editor surfaces on hover.
-- ---------------------------------------------------------------------------

local init_source = read_file(init_module_path)

---A documentation block is the run of comment lines immediately above a
---declaration. Both `--` and `---` runs count: the staged #11155 blocks use
---plain `--` while LuaCATS descriptions use `---`, and a reader meets
---either. Annotation tags (`---@field`, ...) terminate the run.
local function comment_block_above(lines, index)
  local block = {}
  local cursor = index - 1
  while cursor >= 1 do
    local previous = lines[cursor]
    if previous:match("^%s*%-%-") and not previous:match("^%s*%-%-%-@") then
      table.insert(block, 1, previous)
      cursor = cursor - 1
    else
      break
    end
  end
  if #block == 0 then return nil end
  return table.concat(block, " ")
end

local function source_lines(source)
  local lines = {}
  for line in source:gmatch("[^\n]*") do lines[#lines + 1] = line end
  return lines
end

local function field_annotation(source, field_name)
  local lines = source_lines(source)
  for index, line in ipairs(lines) do
    if line:match("^%s*%-%-%-@field%s+" .. field_name .. "%s") then
      return comment_block_above(lines, index)
    end
  end
  return nil
end

for _, row in ipairs(ADMISSIONS) do
  local annotation = field_annotation(init_source, row.path)
  ok(annotation ~= nil and #annotation > 0,
    "A2 " .. row.path .. " has a LuaCATS field annotation")

  annotation = annotation or ""
  local complete, missing = says_all(annotation, row.annotation_classes)
  ok(complete,
    "A2 " .. row.path .. " annotation names its exposed content classes"
      .. (complete and "" or (" (missing: " .. table.concat(missing, ", ") .. ")")))

  ok(says(annotation, OWNER_ISSUE),
    "A2 " .. row.path .. " annotation cites the owning issue " .. OWNER_ISSUE)
end

-- ---------------------------------------------------------------------------
-- A5: CROSS-MODULE CONSISTENCY for `verbose`.
--
-- server.lua declares `verbose` twice: once as the Server class field and
-- once in the default options table that a server definition copies.  A
-- reader may meet either one.  Both must disclose; the pristine defect is
-- precisely that only the class field does.
-- ---------------------------------------------------------------------------

local server_source = read_file(server_module_path)

local VERBOSE_CLASSES = { "protocol payloads", "source code" }

local class_field_annotation = field_annotation(server_source, "public%s+verbose")
  or field_annotation(server_source, "verbose")
ok(class_field_annotation ~= nil and #class_field_annotation > 0,
  "A5 server.lua declares a Server.verbose class field annotation")
do
  local text = class_field_annotation or ""
  local complete, missing = says_all(text, VERBOSE_CLASSES)
  ok(complete,
    "A5 Server.verbose class field discloses protocol payload exposure"
      .. (complete and "" or (" (missing: " .. table.concat(missing, ", ") .. ")")))
  ok(says(text, OWNER_ISSUE),
    "A5 Server.verbose class field cites " .. OWNER_ISSUE)
end

---The comment block immediately preceding the `verbose = false,` entry of
---the default options table - a different declaration site than the class
---field above.
local function default_option_annotation(source, option_name)
  local lines = source_lines(source)
  for index, line in ipairs(lines) do
    if line:match("^%s*" .. option_name .. "%s*=%s*false%s*,%s*$") then
      local block = comment_block_above(lines, index)
      if block then return block end
    end
  end
  return nil
end

local default_verbose = default_option_annotation(server_source, "verbose")
ok(default_verbose ~= nil,
  "A5 server.lua default options declare `verbose` with a comment")
do
  local text = default_verbose or ""
  local complete, missing = says_all(text, VERBOSE_CLASSES)
  ok(complete,
    "A5 default-options verbose discloses protocol payload exposure"
      .. (complete and "" or (" (missing: " .. table.concat(missing, ", ") .. ")")))
  ok(says(text, OWNER_ISSUE),
    "A5 default-options verbose cites " .. OWNER_ISSUE)
  -- Meaning preservation: the original developer-facing purpose survives.
  ok(says(text, "debug"),
    "A5 default-options verbose keeps its operational meaning ('debug')")
end

print(string.format("\n%d passed, %d failed", passed, failed))
if failed > 0 then os.exit(1) end

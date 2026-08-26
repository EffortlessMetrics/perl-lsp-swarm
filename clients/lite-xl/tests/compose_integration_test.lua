-- Deterministic end-to-end tests for the #11170 composer over REAL git
-- history and the REAL staged exact-source tree.
--
-- Run (from the repository root):
--   lua clients/lite-xl/tests/compose_integration_test.lua
--
-- Seam owned: the GIT ADAPTER and the identity laws that tie composed
-- candidates to repository truth:
--   I1  composing the core support profile reproduces the staged upstream
--     tree EXACTLY - every generated module's git blob equals the tracked
--     staged module's blob;
--   I2  composing the empty protocol-baseline profile reproduces the five
--     documented pristine upstream blobs exactly;
--   I3  combined-tree proof runs focused #11103 suites against the
--     GENERATED copies and reports green with per-component attribution;
--   I4  the CLI exits nonzero on an unknown profile and zero on success;
--   I5  verification passes untouched and fails after a hand edit;
--   I6  two independent materializations emit byte-identical receipts.
--
-- Red-first baseline: load error before this patch (no compose.lua).
--
-- Mutation falsifiers of the PATCHED module (each mechanically verified):
--   1. make snapshot() serve the CURRENT staged file instead of the named
--     candidate SHA -> recorded-digest verification fails closed before
--     any identity law can be fooled (observed: digest_mismatch error);
--   2. skip writing one base module -> the final inventory digest batch
--     fails closed on the missing bytes;
--   3. let run_proof swallow suite exit codes or skip the syntax gate ->
--     I5's corrupted-tree control observes a red report instead of green
--     (silent swallow is discriminated, not assumed absent).

local here = debug.getinfo(1, "S").source:sub(2):match("^(.*)[/\\]") or "."
local repo_root = here .. "/../../.."

local passed, failed = 0, 0
local function ok(condition, message)
  if condition then
    passed = passed + 1
  else
    failed = failed + 1
    print("FAIL: " .. message)
  end
end

local scratch = (os.getenv("TEMP") or ".") .. "/compose_integration_scratch"
os.execute('mkdir "' .. scratch .. '" 2>nul')

local function write_file(path, bytes)
  local f = io.open(path, "wb")
  f:write(bytes)
  f:close()
end

local function read_file(path)
  local f = io.open(path, "rb")
  if not f then return nil end
  local b = f:read("*a")
  f:close()
  return b
end

local function run_exit(cmd)
  local okk, kind, code = os.execute(cmd)
  if okk == true then return 0 end
  if kind == "exit" then return code end
  return 1
end

local function staged_blobs()
  -- One ls-tree serves every module comparison.
  local ph = io.popen(string.format(
    'git -C "%s" ls-tree -r HEAD -- clients/lite-xl/upstream', repo_root))
  local map = {}
  for line in ph:lines() do
    local blob, path = line:match("^%d-%s+blob%s+(%x+)\t(.+)$")
    if blob and path then
      local rel = path:match("^clients/lite%-xl/upstream/(.+)$")
      if rel then map[rel] = blob end
    end
  end
  ph:close()
  return map
end

local ok_load, compose = pcall(dofile, here .. "/../compose.lua")
ok(ok_load, "compose module loads (" .. tostring(compose) .. ")")
if not ok_load then
  print(string.format("compose_integration_test: %d passed, %d failed", passed, failed))
  os.exit(1)
end

local adapter = compose.git_adapter({ repo_root = repo_root })
local manifest = dofile(here .. "/../candidate_manifest.lua")

local function materialize(profile_id, tag)
  local tree_dir = scratch .. "/" .. tag .. "_tree"
  local receipt_path = scratch .. "/" .. tag .. "_receipt.json"
  local res = compose.materialize({
    manifest = manifest,
    adapter = adapter,
    profile = profile_id,
    base_dir = here .. "/../leaves/base",
    out_dir = tree_dir,
    receipt_path = receipt_path,
  })
  return res, tree_dir, receipt_path
end

-- ---------------------------------------------------------------------------
-- I1: core profile reproduces the staged exact-source tree exactly
-- ---------------------------------------------------------------------------

local CORE, CORE_TREE = nil, nil
do
  CORE, CORE_TREE = materialize("lite_xl_exact_source_core", "core")
  ok(CORE ~= nil and CORE.tree_digest ~= nil, "I1 core composes")

  local expected_modules = { "diagnostics.lua", "init.lua", "json.lua",
    "server.lua", "util.lua", "capability_manifest.lua" }
  local staged = staged_blobs()
  for _, name in ipairs(expected_modules) do
    ok(staged[name] ~= nil, "I1 staged blob readable for " .. name)
    ok(CORE.tree[name] == staged[name],
      "I1 composed " .. name .. " blob equals the staged tracked blob")
    ok(read_file(CORE_TREE .. "/" .. name) == read_file(
      repo_root .. "/clients/lite-xl/upstream/" .. name),
      "I1 composed " .. name .. " bytes equal staged bytes")
  end
end

-- ---------------------------------------------------------------------------
-- I2: baseline profile reproduces pristine upstream exactly
-- ---------------------------------------------------------------------------

do
  local DOCUMENTED = {
    ["diagnostics.lua"] = "c06bec4955d7fbfd8f3a2753fba26c04247b09e0",
    ["init.lua"] = "7b38c3a97c68877d2391753adb09e49ec57397d3",
    ["json.lua"] = "eb36b8fa947ff1189b02ce03d257b80a86fdac64",
    ["server.lua"] = "33c8ccae7362ddb01aa980bff024a4ef1682c8f9",
    ["util.lua"] = "588c101aa97ef0d112926aac316e7a95a52a6994",
  }
  local res = materialize("lite_xl_protocol_baseline", "base")
  for name, want in pairs(DOCUMENTED) do
    ok(res.tree[name] == want,
      "I2 baseline " .. name .. " reproduces the documented pristine blob")
  end
  ok(res.tree["capability_manifest.lua"] == nil,
    "I2 baseline carries no patched-in new files")
end

-- ---------------------------------------------------------------------------
-- I3: combined-tree proof runs generated copies through focused suites
-- (reuses the I1 core tree)
-- ---------------------------------------------------------------------------

do
  local report = compose.run_proof({
    manifest = manifest,
    adapter = adapter,
    repo_root = repo_root,
    tree_dir = CORE_TREE,
    profile = "lite_xl_exact_source_core",
    only = { "json_decode_test.lua", "capability_manifest_test.lua" },
  })
  ok(type(report) == "table", "I3 proof returns a report")
  ok(#report.checks >= 2, "I3 proof executed the selected suites")
  for _, c in ipairs(report.checks) do
    ok(c.exit_code == 0,
      "I3 generated-copy proof green: " .. c.suite .. " (" .. c.component .. ")")
  end
  ok(report.ok == true, "I3 proof reports overall green")
end

-- ---------------------------------------------------------------------------
-- I4: CLI exit codes (cheap baseline profile for the success path; an
-- unknown profile fails at planning before any composition work)
-- ---------------------------------------------------------------------------

do
  local out = scratch .. "/cli_tree"
  local rcpt = scratch .. "/cli_receipt.json"
  local good = run_exit(string.format(
    'lua "%s/clients/lite-xl/compose.lua" materialize lite_xl_protocol_baseline' ..
    ' --out "%s" --receipt "%s"', repo_root, out, rcpt))
  ok(good == 0, "I4 CLI materialize exits zero on the baseline profile")

  local bad_out = scratch .. "/cli_bad_tree"
  local bad_rcpt = scratch .. "/cli_bad_receipt.json"
  local bad = run_exit(string.format(
    'lua "%s/clients/lite-xl/compose.lua" materialize no_such_profile' ..
    ' --out "%s" --receipt "%s"', repo_root, bad_out, bad_rcpt))
  ok(bad ~= 0, "I4 CLI exits nonzero on an unknown profile")
end

-- ---------------------------------------------------------------------------
-- I5: verification catches a real hand edit on disk (reuses the I1 tree);
-- the SAME corruption doubles as the negative control for combined-tree
-- proof: a broken generated module must surface through the syntax gate
-- and flip the proof report red (exit-code/syntax propagation is
-- load-bearing, not decorative).
-- ---------------------------------------------------------------------------

do
  local res, tree_dir = CORE, CORE_TREE
  local okv = pcall(compose.verify_tree, {
    tree_dir = tree_dir, inventory = res.inventory, adapter = adapter,
  })
  ok(okv, "I5 untouched composed tree verifies clean")

  local target = tree_dir .. "/json.lua"
  local bytes = read_file(target)
  local broken = bytes .. "this is not lua ]]\n"
  write_file(target, broken)

  local verr, verrm = pcall(compose.verify_tree, {
    tree_dir = tree_dir, inventory = res.inventory, adapter = adapter,
  })
  ok(verr == false, "I5 hand-edited tree fails verification (" ..
    tostring(verrm) .. ")")

  if verr == false then
    local report = compose.run_proof({
      manifest = manifest,
      adapter = adapter,
      repo_root = repo_root,
      tree_dir = tree_dir,
      profile = "lite_xl_exact_source_core",
    })
    ok(report.ok == false,
      "I5 corrupted-tree proof reports NOT green (no silent swallow)")
    local caught_syntax = false
    for _, c in ipairs(report.checks) do
      if c.exit_code ~= 0 then caught_syntax = true end
    end
    ok(caught_syntax, "I5 corrupted-tree proof attributes the failure")
  end

  -- restore exact composed bytes for any later consumption
  write_file(target, bytes)
end

-- ---------------------------------------------------------------------------
-- I6: receipts are byte-identical across independent runs (the law is
-- profile-independent; baseline keeps this cheap on process-spawn-bound
-- hosts while hermetic F9/F10 cover member-bearing determinism)
-- ---------------------------------------------------------------------------

do
  local r1, _, p1 = materialize("lite_xl_protocol_baseline", "det_a")
  local r2, _, p2 = materialize("lite_xl_protocol_baseline", "det_b")
  ok(r1.receipt_json == r2.receipt_json, "I6 library receipts identical")
  ok(read_file(p1) == read_file(p2), "I6 written receipt files byte-identical")
end

print(string.format("compose_integration_test: %d passed, %d failed", passed, failed))
os.exit(failed == 0 and 0 or 1)

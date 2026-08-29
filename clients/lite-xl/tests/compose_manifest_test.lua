-- Deterministic focused tests for the checked candidate composition
-- manifest (#11170).
--
-- Run:
--   lua clients/lite-xl/tests/compose_manifest_test.lua [path-to-manifest]
-- Default manifest path is ../candidate_manifest.lua relative to this file.
--
-- Seam owned: the MANIFEST DATA ITSELF - schema identity, unique ids,
-- resolvable references, digest honesty against real git history, chain
-- prerequisite honesty, class discipline per profile, and the completeness
-- law that every landed core-class leaf is a member of the core support
-- profile. Composition mechanics live in compose_materializer_test.lua;
-- real end-to-end materialization lives in compose_integration_test.lua.
--
-- Contract pinned here:
--   - manifest carries schema "candidate-manifest.v1", the exact upstream
--     base ref d1432ae0736cd9531798b4bc1221835f534cc689, and one pristine
--     blob digest per base module;
--   - every component id and profile id is unique; every prerequisite and
--     profile member references an existing component;
--   - every component records a 40-hex merged internal candidate SHA,
--     non-empty changed paths, a known component class, an internal
--     upstream state, per-path content digests that MATCH the actual blobs
--     in this repository's history at that SHA (stale-SHA and path-drift
--     discrimination), and a conflict key per changed path;
--   - the declared hard prerequisites EQUAL the staged-lineage chains
--     recomputed mechanically from this repository's own history (manifest
--     drift against history fails);
--   - every profile member's class is admitted by that profile's declared
--     class envelope;
--   - every component whose class is core-admissible is a member of the
--     lite_xl_exact_source_core profile (no landed core leaf dangles
--     outside the support-truth profile);
--   - the committed pristine copies under leaves/base/ hash exactly to the
--     documented upstream blob digests.
--
-- Red-first baseline: run against CURRENT MAIN before this patch - there
-- is no candidate_manifest.lua yet, so loading this suite fails (nonzero
-- exit). Observed pristine-main baseline: load error before any assertion.
--
-- Mutation falsifiers of the PATCHED data (each mechanically verified to
-- fail this suite against a mutated manifest copy):
--   1. drop one hard_prerequisites edge -> the recomputed-history chain
--     comparison fails;
--   2. change one recorded content digest -> the history blob comparison
--     fails;
--   3. remove one core component from the core profile -> the completeness
--     law fails;
--   4. move a security-class component into the baseline profile -> the
--     class-envelope law fails.
--
-- No framework: plain soft asserts, one process, deterministic, exit code
-- carries the result. Compatible with the Lite XL Lua runtime family
-- (Lua 5.4).

local manifest_path = arg and arg[1] or nil
if not manifest_path then
  local info = debug.getinfo(1, "S").source:sub(2)
  local dir = info:match("^(.*)[/\\]") or "."
  manifest_path = dir .. "/../candidate_manifest.lua"
end

local here = debug.getinfo(1, "S").source:sub(2):match("^(.*)[/\\]") or "."
local repo_root = here .. "/../../.."

-- ---------------------------------------------------------------------------
-- Soft assertion collector
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

-- ---------------------------------------------------------------------------
-- Independent git plumbing (deliberately NOT routed through compose.lua so
-- the manifest laws are discriminated by plumbing outside the tested
-- implementation).
-- ---------------------------------------------------------------------------

local scratch = os.getenv("TEMP") or "."
scratch = scratch .. "/compose_manifest_test_scratch"
os.execute('mkdir "' .. scratch .. '" 2>nul')

local function git_exit(args)
  local okk, kind, code = os.execute("git -C \"" .. repo_root .. "\" " .. args)
  if okk == true then return 0 end
  if kind == "exit" then return code end
  return 1
end

local tree_cache = {}

local function blob_sha_of_commit_file(commit, upstream_path)
  -- Cached full-tree listing per commit: one ls-tree spawn serves every
  -- path of that commit.
  if not tree_cache[commit] then
    local ph = io.popen("git -C \"" .. repo_root .. "\" ls-tree -r "
      .. commit .. " -- clients/lite-xl/upstream")
    local map = {}
    for line in ph:lines() do
      local blob, path = line:match("^%d-%s+blob%s+(%x+)\t(.+)$")
      if blob and path then
        local rel = path:match("^clients/lite%-xl/upstream/(.+)$")
        if rel then map[rel] = blob end
      end
    end
    ph:close()
    tree_cache[commit] = map
  end
  return tree_cache[commit][upstream_path]
end

local HISTORY_CACHE = nil

local function upstream_history()
  -- Oldest-first list of commits touching the staged upstream tree.
  -- Memoized: several contract sections consume the same walk.
  if HISTORY_CACHE then return HISTORY_CACHE end
  local ph = io.popen("git -C \"" .. repo_root
    .. "\" log --reverse --pretty=format:%H -- clients/lite-xl/upstream")
  local commits = {}
  for line in ph:lines() do
    if #line > 0 then commits[#commits + 1] = line end
  end
  ph:close()
  HISTORY_CACHE = commits
  return commits
end

local TOUCHED_CACHE = {}

local function commit_touched_upstream_paths(commit)
  if TOUCHED_CACHE[commit] then return TOUCHED_CACHE[commit] end
  local ph = io.popen("git -C \"" .. repo_root
    .. "\" diff-tree --no-commit-id --name-only -r " .. commit)
  local paths = {}
  for line in ph:lines() do
    local rel = line:match("^clients/lite%-xl/upstream/(.+)$")
    if rel then paths[#paths + 1] = rel end
  end
  ph:close()
  table.sort(paths)
  TOUCHED_CACHE[commit] = paths
  return paths
end

local function file_blob_sha(path)
  local ph = io.popen("git -C \"" .. repo_root .. "\" hash-object \"" .. path .. "\"")
  local sha = ph:read("*l")
  ph:close()
  return sha
end

-- ---------------------------------------------------------------------------
-- Recompute the expected staged-lineage chains from real history.
-- ---------------------------------------------------------------------------

local function expected_chains()
  local chains = {}   -- path -> array of commit shas, oldest first
  local order = {}
  for _, commit in ipairs(upstream_history()) do
    for _, path in ipairs(commit_touched_upstream_paths(commit)) do
      if not chains[path] then
        chains[path] = {}
        order[#order + 1] = path
      end
      table.insert(chains[path], commit)
    end
  end
  return chains, order
end

local chains = expected_chains()

-- Immediate predecessor per (commit, path), accumulated per commit.
local expected_preds = {}   -- commit -> set of predecessor commits
do
  local seq = {}
  local seen_pos = {}
  for _, commit in ipairs(upstream_history()) do
    for _, path in ipairs(commit_touched_upstream_paths(commit)) do
      seq[#seq + 1] = { commit = commit, path = path }
    end
  end
  for _, item in ipairs(seq) do
    local preds = expected_preds[item.commit]
    if not preds then
      preds = {}
      expected_preds[item.commit] = preds
    end
    local pos = seen_pos[item.path]
    if not pos then
      pos = { prev = nil }
      seen_pos[item.path] = pos
    end
    if pos.prev then
      preds[pos.prev] = true
    end
    pos.prev = item.commit
  end
end

-- ---------------------------------------------------------------------------
-- Load the manifest
-- ---------------------------------------------------------------------------

local ok_load, manifest = pcall(dofile, manifest_path)
ok(ok_load, "manifest module loads (" .. tostring(manifest) .. ")")
if not ok_load then
  print(string.format("compose_manifest_test: %d passed, %d failed", passed, failed))
  os.exit(failed == 0 and 0 or 1)
end

local CLASSES = {
  security = true, protocol = true, session = true, document = true,
  diagnostic = true, provider = true, configuration = true,
  advertisement = true, quality = true, package = true,
}
local CORE_CLASSES = {
  security = true, protocol = true, session = true, document = true,
  diagnostic = true, provider = true, configuration = true,
  advertisement = true,
}
local UPSTREAM_STATES = { internal = true }

-- ---------------------------------------------------------------------------
-- Schema identity
-- ---------------------------------------------------------------------------

do
  ok(manifest.schema == "candidate-manifest.v1", "manifest names schema candidate-manifest.v1")
  ok(type(manifest.upstream_base) == "table", "manifest records the upstream base")
  ok(manifest.upstream_base.repository == "lite-xl/lite-xl-lsp",
    "base names the external repository")
  ok(manifest.upstream_base.ref == "d1432ae0736cd9531798b4bc1221835f534cc689",
    "base records the documented pristine ref")
  ok(type(manifest.upstream_base.files) == "table", "base records per-module digests")
end

-- ---------------------------------------------------------------------------
-- Components: uniqueness, references, recorded digests vs real history
-- ---------------------------------------------------------------------------

local components_by_id = {}
local components_by_commit = {}

do
  ok(type(manifest.components) == "table" and #manifest.components > 0,
    "manifest lists components")

  local ids_unique = true
  for _, c in ipairs(manifest.components) do
    if components_by_id[c.id] then ids_unique = false end
    components_by_id[c.id] = c
  end
  ok(ids_unique, "component ids are unique")

  for _, c in ipairs(manifest.components) do
    ok(type(c.id) == "string" and #c.id > 0, "component has a stable id")
    ok(type(c.issue) == "number", c.id .. " records its owning issue")
    ok(type(c.candidate_sha) == "string" and c.candidate_sha:match("^%x%x%x%x%x%x%x%x%x%x%x%x%x%x%x%x%x%x%x%x%x%x%x%x%x%x%x%x%x%x%x%x%x%x%x%x%x%x%x%x$") ~= nil,
      c.id .. " records a 40-hex merged internal candidate SHA")
    ok(type(c.changed_paths) == "table" and #c.changed_paths > 0,
      c.id .. " records changed upstream paths")
    ok(CLASSES[c.class] == true, c.id .. " carries a known component class")
    ok(UPSTREAM_STATES[c.upstream_state] == true,
      c.id .. " carries an upstream state from the #10739 vocabulary")
    ok(c.owner_issue == c.issue, c.id .. " names its behavior/proof owner")
    ok(type(c.conflict_keys) == "table" and #c.conflict_keys == #c.changed_paths,
      c.id .. " records one conflict key per changed path")
    ok(type(c.invalidation_inputs) == "table" and #c.invalidation_inputs >= 1,
      c.id .. " records invalidation inputs")
    ok(type(c.content) == "table", c.id .. " records per-path content digests")

    if components_by_commit[c.candidate_sha] then
      ok(false, "candidate SHA " .. c.candidate_sha .. " claimed by two components")
    end
    components_by_commit[c.candidate_sha] = c

    -- Recorded changed paths must equal the paths actually touched upstream.
    local touched = {}
    for _, p in ipairs(commit_touched_upstream_paths(c.candidate_sha)) do
      touched[p] = true
    end
    local declared = {}
    for _, p in ipairs(c.changed_paths) do declared[p] = true end
    for p in pairs(touched) do
      ok(declared[p] == true,
        c.id .. " declared set covers history-touched path " .. p)
    end
    for p in pairs(declared) do
      ok(touched[p] == true,
        c.id .. " declared path " .. p .. " is really touched by the candidate")
    end

    -- Recorded content digests must match the actual history blobs.
    local keyed = {}
    for _, k in ipairs(c.conflict_keys) do
      ok(k:match("^lite%-xl%.upstream%.") ~= nil,
        c.id .. " conflict key uses the lite-xl.upstream namespace: " .. k)
      keyed[k] = true
    end
    for _, p in ipairs(c.changed_paths) do
      local actual = blob_sha_of_commit_file(c.candidate_sha, p)
      ok(actual ~= nil, c.id .. " snapshot of " .. p .. " exists in history")
      ok(c.content[p] == actual,
        c.id .. " recorded digest for " .. p .. " matches history blob")
      ok(keyed["lite-xl.upstream." .. p] == true,
        c.id .. " records a conflict key for changed path " .. p)
    end
  end
end

-- ---------------------------------------------------------------------------
-- Prerequisite honesty: declared edges EQUAL the recomputed history chains
-- ---------------------------------------------------------------------------

do
  local declared_edges = {}   -- commit -> set of predecessor commits
  for _, c in ipairs(manifest.components) do
    local preds = {}
    for _, pid in ipairs(c.hard_prerequisites or {}) do
      local pc = components_by_id[pid]
      ok(pc ~= nil, c.id .. " prerequisite " .. tostring(pid) .. " exists")
      if pc then preds[pc.candidate_sha] = true end
    end
    declared_edges[c.candidate_sha] = preds
  end

  for _, c in ipairs(manifest.components) do
    local want = expected_preds[c.candidate_sha] or {}
    local got = declared_edges[c.candidate_sha] or {}
    for sha in pairs(want) do
      ok(got[sha] == true,
        c.id .. " declares the history chain predecessor " .. sha:sub(1, 9))
    end
    for sha in pairs(got) do
      ok(want[sha] == true,
        c.id .. " prerequisite " .. sha:sub(1, 9)
          .. " is the recomputed history chain predecessor")
    end
  end
end

-- ---------------------------------------------------------------------------
-- Profiles: uniqueness, membership references, class envelopes, completeness
-- ---------------------------------------------------------------------------

local profiles_by_id = {}

do
  ok(type(manifest.profiles) == "table" and #manifest.profiles >= 5,
    "manifest defines the initial named profile set")

  for _, p in ipairs(manifest.profiles) do
    ok(profiles_by_id[p.id] == nil, "profile id unique: " .. tostring(p.id))
    profiles_by_id[p.id] = p
    ok(type(p.id) == "string" and #p.id > 0, "profile has a stable id")
    ok(type(p.claim) == "string" and #p.claim > 0,
      p.id .. " states its bounded claim envelope")
    ok(type(p.admitted_classes) == "table", p.id .. " declares its class envelope")
    ok(type(p.members) == "table", p.id .. " declares explicit membership")
    for _, mid in ipairs(p.members) do
      local m = components_by_id[mid]
      ok(m ~= nil, p.id .. " member " .. tostring(mid) .. " exists")
      if m then
        ok(p.admitted_classes[m.class] == true,
          p.id .. " admits member class " .. tostring(m.class))
      end
    end
  end

  -- Completeness law: every core-class component sits in the core profile.
  local core = profiles_by_id["lite_xl_exact_source_core"]
  ok(core ~= nil, "core support profile exists")
  if core then
    local membership = {}
    for _, mid in ipairs(core.members) do membership[mid] = true end
    for _, c in ipairs(manifest.components) do
      if CORE_CLASSES[c.class] then
        ok(membership[c.id] == true,
          "landed core-class component " .. c.id .. " is a core profile member")
      end
    end
  end

  -- The baseline profile starts as the pristine exact-source anchor.
  local baseline = profiles_by_id["lite_xl_protocol_baseline"]
  ok(baseline ~= nil, "protocol baseline profile exists")
  if baseline then
    ok(#baseline.members == 0,
      "baseline profile anchors pristine exact source until a "
        .. "registration-class leaf lands")
  end
end

-- ---------------------------------------------------------------------------
-- Pristine base copies hash to the documented upstream digests
-- ---------------------------------------------------------------------------

do
  local DOCUMENTED = {
    ["diagnostics.lua"] = "c06bec4955d7fbfd8f3a2753fba26c04247b09e0",
    ["helpdoc.lua"] = "42d7a07f23fa9f254e28ba2ab2c858aded3122d5",
    ["init.lua"] = "7b38c3a97c68877d2391753adb09e49ec57397d3",
    ["json.lua"] = "eb36b8fa947ff1189b02ce03d257b80a86fdac64",
    ["listbox.lua"] = "33284b02995781d897add3b44c4d66aac64d299e",
    ["server.lua"] = "33c8ccae7362ddb01aa980bff024a4ef1682c8f9",
    ["symbolresults.lua"] = "96c39cd5ee1b765c85c6f7dc5eb1cb90386994ad",
    ["timer.lua"] = "c25fefa44e65d1f3a8c52e555080a61195ececae",
    ["util.lua"] = "588c101aa97ef0d112926aac316e7a95a52a6994",
  }
  for name, want in pairs(DOCUMENTED) do
    ok(manifest.upstream_base.files[name] == want,
      "manifest base digest for " .. name .. " equals the documented blob")
    local f = io.open(here .. "/../leaves/base/" .. name, "rb")
    ok(f ~= nil, "pristine copy exists for " .. name)
    if f then
      f:close()
      local got = file_blob_sha(here .. "/../leaves/base/" .. name)
      ok(got == want, "committed pristine copy of " .. name
        .. " hashes to the documented blob")
    end
  end
end

print(string.format("compose_manifest_test: %d passed, %d failed", passed, failed))
os.exit(failed == 0 and 0 or 1)

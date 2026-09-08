-- Deterministic exact-source candidate composer (#11170).
--
-- Composes pinned pristine upstream base modules plus whole-file snapshots
-- from merged internal candidates (one reviewed leaf each, identified by
-- the checked candidate_manifest.lua) into named installable client
-- trees, and fails closed on source/patch/dependency/conflict drift.
--
-- Library surface (dofile this file):
--   local compose = dofile("clients/lite-xl/compose.lua")
--   local adapter = compose.git_adapter({ repo_root = "<repo>" })
--   local res = compose.materialize({
--     manifest = <manifest table>, adapter = adapter,
--     profile = "<profile id>", base_dir = "<dir>",
--     out_dir = "<generated tree dir>", receipt_path = "<file>",
--   })
--   compose.verify_tree({ tree_dir = ..., inventory = res.inventory,
--                         adapter = adapter })
--   compose.run_proof({ manifest = ..., adapter = ..., repo_root = ...,
--                       tree_dir = ..., only = { "<suite>", ... } })
--
-- Source adapter interface (swap for tests; default shells real git):
--   component_exists(sha) -> bool
--   touched_upstream_paths(sha) -> sorted array of upstream-relative paths
--   snapshot(sha, upstream_path, dest_file) -> true (writes exact bytes)
--   is_ancestor(sha_a, sha_b) -> bool
--   hash_file(path) -> content-addressed digest string
--   digest_text(text) -> digest string over canonical text
--
-- Composition laws (typed errors carry compose_error=true):
--   unknown_profile      profile id absent from the manifest
--   unknown_component    referenced component id absent from the manifest
--   class_violation      member class outside the profile's envelope
--   missing_prerequisite member's hard prerequisite not in the profile
--                        (profiles are prefix-closed per staged chain)
--   undeclared_overlap   two members share a path with no declared
--                        dependency relation between them: typed
--                        combined-tree interaction routed to owners, never
--                        auto-resolved here
--   dependency_cycle     prerequisite graph is not acyclic
--   stale_candidate_sha  candidate commit unresolvable in the source
--   path_drift           candidate touches paths other than declared
--   digest_mismatch      snapshot/base/write-back bytes diverge from the
--                        recorded reviewed digests (whole-file snapshots
--                        cannot fuzz: any divergence is fatal, which is
--                        the no-fuzz/skipped-hunk analog)
--   ancestor_break       consecutive writers of one path are unrelated in
--                        source history (combined-tree interaction)
--   unowned_diff         generated tree contains bytes outside the
--                        recorded inventory (extra, mutated, or missing)
--
-- Determinism: outputs are pure functions of (manifest, source bytes);
-- iteration is sorted everywhere and receipts exclude wall-clock inputs.
-- Regeneration from identical inputs is byte-identical.

local M = {}

-- ---------------------------------------------------------------------------
-- Typed errors
-- ---------------------------------------------------------------------------

local function fail(code, fields)
  local err = { compose_error = true, code = code }
  for k, v in pairs(fields or {}) do err[k] = v end
  error(err, 0)
end

local function describe(err)
  if type(err) ~= "table" or not err.compose_error then
    return tostring(err)
  end
  local parts = { "code=" .. tostring(err.code) }
  local keys = {}
  for k in pairs(err) do
    if k ~= "compose_error" and k ~= "code" then keys[#keys + 1] = k end
  end
  table.sort(keys)
  for _, k in ipairs(keys) do
    parts[#parts + 1] = k .. "=" .. tostring(err[k])
  end
  return table.concat(parts, " ")
end

-- ---------------------------------------------------------------------------
-- Portable process/filesystem helpers (Windows / POSIX)
-- ---------------------------------------------------------------------------

local IS_WINDOWS = package.config:sub(1, 1) == "\\"

local function quote(path)
  return '"' .. path .. '"'
end

local function capture_lines(cmd)
  local ph = io.popen(cmd)
  local out = {}
  for line in ph:lines() do
    if #line > 0 then out[#out + 1] = line end
  end
  ph:close()
  return out
end

local function run_rc(cmd)
  local ok, kind, code = os.execute(cmd)
  if ok == true then return 0 end
  if kind == "exit" then return code or 1 end
  return 1
end

local function read_bytes(path)
  local f = io.open(path, "rb")
  if not f then return nil end
  local b = f:read("*a")
  f:close()
  return b
end

local function write_bytes(path, bytes)
  local f = io.open(path, "wb")
  if not f then
    fail("io_failure", { path = path, message = "cannot open for write" })
  end
  f:write(bytes)
  f:close()
end

local function ensure_dirs(paths)
  local needed = {}
  for _, p in ipairs(paths) do
    needed[#needed + 1] =
      (IS_WINDOWS and ('mkdir ' .. quote(p) .. ' 2>nul'))
        or ('mkdir -p ' .. quote(p) .. ' 2>/dev/null')
  end
  if #needed == 0 then return end
  os.execute(table.concat(needed, IS_WINDOWS and " & " or " && "))
end

-- Adapter scratch space is prepared once per adapter, not per call.
local function make_tmp_prep(tmp_dir)
  local prepared = false
  return function()
    if not prepared then
      ensure_dirs({ tmp_dir })
      prepared = true
    end
  end
end

local function current_working_dir()
  if IS_WINDOWS then
    local lines = capture_lines("cd")
    return lines[1] or "."
  end
  local lines = capture_lines("pwd")
  return lines[1] or "."
end

-- List every regular file under dir (recursive), as forward-slash paths
-- relative to dir, sorted. dir is normalized against the process working
-- directory because `dir /s /b` reports absolute paths regardless of the
-- input form.
local function list_files_relative(dir)
  local out = {}
  local abs_input = dir
  if not dir:match("^%a:[/\\]") and not dir:match("^[/\\]") then
    local cwd = current_working_dir():gsub("[/\\]+$", "")
    abs_input = cwd .. "/" .. dir
  end
  if IS_WINDOWS then
    local lines = capture_lines('dir /s /b ' .. quote(abs_input) .. ' 2>nul')
    local prefix = abs_input:gsub("\\", "/"):gsub("[/\\]+$", "")
    for _, abs in ipairs(lines) do
      local rel = abs:gsub("\\", "/")
      local idx = rel:find(prefix, 1, true)
      if idx == 1 then
        rel = rel:sub(#prefix + 2)
        if #rel > 0 then out[#out + 1] = rel end
      end
    end
  else
    local lines = capture_lines('find ' .. quote(abs_input) .. ' -type f')
    local prefix = abs_input:gsub("/+$", "")
    for _, abs in ipairs(lines) do
      local idx = abs:find(prefix, 1, true)
      if idx == 1 then
        local rel = abs:sub(#prefix + 2)
        if #rel > 0 then out[#out + 1] = rel end
      end
    end
  end
  table.sort(out)
  return out
end

-- ---------------------------------------------------------------------------
-- Default source adapter: real git against this repository's history
-- ---------------------------------------------------------------------------

local UPSTREAM_REPO_DIR = "clients/lite-xl/upstream"

function M.git_adapter(opts)
  local root
  if opts and opts.repo_root then
    root = opts.repo_root
  else
    local info = debug.getinfo(1, "S").source:sub(2)
    root = info:match("^(.*)[/\\]") .. "/../.."
  end
  local tmp_dir = (opts and opts.tmp_dir)
    or root .. "/.compose-tmp"

  local function git(args)
    return 'git -C ' .. quote(root) .. ' ' .. args
  end

  -- Batch helpers: one spawn per batch instead of one per item (git
  -- process startup dominates runtime on this host). Scratch file names
  -- carry a per-adapter tag so concurrent compositions in one checkout
  -- never share a temp path.
  local prep_tmp = make_tmp_prep(tmp_dir)
  local scratch_tag = ("%d-%.0f"):format(os.time(), os.clock() * 1e6)

  return {
    -- Existence of many candidate commits in one cat-file --batch-check.
    -- Input lines are raw object names (batch-check does not parse
    -- quoting); SHAs are hex-validated at manifest load.
    components_exist = function(shas)
      if #shas == 0 then return {} end
      prep_tmp()
      local in_file = tmp_dir .. "/compose-batch-in-" .. scratch_tag .. ".txt"
      local f = io.open(in_file, "wb")
      for _, sha in ipairs(shas) do
        f:write(sha .. "\n")
      end
      f:close()
      local out = {}
      local lines = capture_lines(git("cat-file --batch-check < "
        .. quote(in_file)))
      os.remove(in_file)
      for _, line in ipairs(lines) do
        local sha, rest = line:match("^(%x+) (.+)$")
        if sha then
          out[sha] = not (rest == "missing" or rest:match("^missing"))
        end
      end
      return out
    end,
    component_exists = function(sha)
      return run_rc(git("cat-file -e " .. sha .. "^{commit}")) == 0
    end,
    -- Touched upstream paths for many commits in one log walk: each
    -- selected commit appears as its own section with its diff vs its
    -- parent (--no-walk preserves per-commit semantics).
    touched_upstream_paths_batch = function(shas)
      if #shas == 0 then return {} end
      local per_commit = {}
      for _, sha in ipairs(shas) do per_commit[sha] = {} end
      local lines = capture_lines(git(
        "log --no-walk --format=%x01%H --name-only "
        .. table.concat(shas, " ")))
      local current = nil
      for _, line in ipairs(lines) do
        local hdr = line:match("^\001(%x+)")   -- git's %x01 section header
        if hdr then
          current = per_commit[hdr]
        elseif current and #line > 0 then
          local rel = line:match("^clients/lite%-xl/upstream/(.+)$")
          if rel then table.insert(current, rel) end
        end
      end
      for _, list in pairs(per_commit) do table.sort(list) end
      return per_commit
    end,
    touched_upstream_paths = function(sha)
      local lines = capture_lines(
        git("diff-tree --no-commit-id --name-only -r " .. sha))
      local out = {}
      for _, line in ipairs(lines) do
        local rel = line:match("^clients/lite%-xl/upstream/(.+)$")
        if rel then out[#out + 1] = rel end
      end
      table.sort(out)
      return out
    end,
    snapshot = function(sha, upstream_path, dest)
      local spec = sha .. ":" .. UPSTREAM_REPO_DIR .. "/" .. upstream_path
      local rc = run_rc(git("cat-file blob " .. spec .. " > "
        .. quote(dest)))
      if rc ~= 0 or not read_bytes(dest) then
        fail("stale_candidate_sha", {
          candidate_sha = sha, path = upstream_path,
          message = "git cannot serve snapshot",
        })
      end
      return true
    end,
    is_ancestor = function(a, b)
      return run_rc(git("merge-base --is-ancestor " .. a .. " " .. b)) == 0
    end,
    hash_file = function(path)
      local lines = capture_lines(git("hash-object " .. quote(path)))
      if not lines[1] then
        fail("io_failure", { path = path, message = "hash-object failed" })
      end
      return lines[1]
    end,
    -- Digest many files preserving input order (hash-object
    -- --stdin-paths; quoted lines are the portable form for paths that
    -- may contain spaces).
    hash_files = function(paths)
      if #paths == 0 then return {} end
      prep_tmp()
      local in_file = tmp_dir .. "/compose-hash-paths-" .. scratch_tag .. ".txt"
      local f = io.open(in_file, "wb")
      for _, p in ipairs(paths) do
        -- Forward slashes keep c-style unquoting valid on Windows paths.
        local posix = (p:gsub("\\", "/"))
        f:write('"' .. posix .. '"\n')
      end
      f:close()
      local lines = capture_lines(git("hash-object --stdin-paths < "
        .. quote(in_file)))
      os.remove(in_file)
      if #lines ~= #paths then
        fail("io_failure", {
          message = "hash-object --stdin-paths count mismatch",
        })
      end
      return lines
    end,
    digest_text = function(text)
      prep_tmp()
      local tmp = tmp_dir .. "/compose-digest-" .. scratch_tag .. ".tmp"
      write_bytes(tmp, text)
      local lines = capture_lines(git("hash-object " .. quote(tmp)))
      os.remove(tmp)
      if not lines[1] then
        fail("io_failure", { message = "digest_text failed" })
      end
      return lines[1]
    end,
    list_files = function(dir)
      return list_files_relative(dir)
    end,
  }
end

-- ---------------------------------------------------------------------------
-- Manifest access helpers
-- ---------------------------------------------------------------------------

local function index_components(manifest)
  local by_id = {}
  for _, c in ipairs(manifest.components or {}) do
    if by_id[c.id] then
      fail("invalid_manifest", { message = "duplicate component id " .. c.id })
    end
    by_id[c.id] = c
  end
  return by_id
end

-- Manifest strings reach command lines through the source adapter; these
-- closed character classes keep that surface inert even against tampered
-- manifest data (defense in depth: the manifest is reviewed repository
-- content, and nothing else about a component is ever interpolated).
local function validate_component_strings(manifest)
  for _, c in ipairs(manifest.components or {}) do
    if type(c.id) ~= "string" or not c.id:match("^[%w_%.%-%+]+$") then
      fail("invalid_manifest", { message = "bad component id",
        component = tostring(c.id) })
    end
    if type(c.candidate_sha) ~= "string"
      or not c.candidate_sha:match("^[0-9a-f]+$") then
      fail("invalid_manifest", { message = "bad candidate sha",
        component = c.id })
    end
    for _, p in ipairs(c.changed_paths or {}) do
      if type(p) ~= "string" or not p:match("^[%w%.%-_%+]+$") then
        fail("invalid_manifest", { message = "bad changed path",
          component = c.id, path = tostring(p) })
      end
      local digest = c.content and c.content[p]
      if type(digest) ~= "string" or not digest:match("^[0-9a-f]+$") then
        fail("invalid_manifest", { message = "bad recorded digest",
          component = c.id, path = p })
      end
    end
  end
  for name in pairs((manifest.upstream_base or {}).files or {}) do
    if type(name) ~= "string" or not name:match("^[%w_%.%-%+]+$") then
      fail("invalid_manifest", { message = "bad base module name",
        module = tostring(name) })
    end
  end
end

local function find_profile(manifest, profile_id)
  for _, p in ipairs(manifest.profiles or {}) do
    if p.id == profile_id then return p end
  end
  fail("unknown_profile", { profile = profile_id })
end

local function path_set(paths)
  local s = {}
  for _, p in ipairs(paths) do s[p] = true end
  return s
end

local function reachable(from_id, by_id, cache)
  if cache[from_id] then return cache[from_id] end
  local seen = {}
  local stack = {}
  local c = by_id[from_id]
  for _, p in ipairs(c and c.hard_prerequisites or {}) do
    stack[#stack + 1] = p
  end
  while #stack > 0 do
    local id = table.remove(stack)
    if not seen[id] then
      seen[id] = true
      local pc = by_id[id]
      for _, p in ipairs(pc and pc.hard_prerequisites or {}) do
        stack[#stack + 1] = p
      end
    end
  end
  cache[from_id] = seen
  return seen
end

-- Validate profile membership and derive the deterministic application
-- order (topological over hard prerequisites, ties broken by component id).
local function plan_profile(manifest, profile_id)
  local by_id = index_components(manifest)
  local profile = find_profile(manifest, profile_id)

  local membership = {}
  for _, mid in ipairs(profile.members or {}) do
    local c = by_id[mid]
    if not c then
      fail("unknown_component", { component = mid, profile = profile_id })
    end
    if membership[mid] then
      fail("invalid_manifest", {
        message = "duplicate member " .. mid .. " in " .. profile_id,
      })
    end
    membership[mid] = true
  end

  for _, mid in ipairs(profile.members or {}) do
    local c = by_id[mid]
    if not (profile.admitted_classes or {})[c.class] then
      fail("class_violation", {
        component = mid, profile = profile_id, class = c.class,
      })
    end
    for _, pre in ipairs(c.hard_prerequisites or {}) do
      if not by_id[pre] then
        fail("unknown_component", { component = mid, unknown = pre })
      end
      if not membership[pre] then
        fail("missing_prerequisite", {
          component = mid, prerequisite = pre, profile = profile_id,
        })
      end
    end
  end

  -- Overlap legality: sharing a path requires a dependency relation.
  local reach_cache = {}
  local by_path = {}
  local member_list = {}
  for _, mid in ipairs(profile.members or {}) do
    member_list[#member_list + 1] = mid
    for _, p in ipairs(by_id[mid].changed_paths or {}) do
      by_path[p] = by_path[p] or {}
      table.insert(by_path[p], mid)
    end
  end
  for _, path in ipairs((function()
    local keys = {}
    for p in pairs(by_path) do keys[#keys + 1] = p end
    table.sort(keys)
    return keys
  end)()) do
    local sharers = by_path[path]
    table.sort(sharers)
    for i = 1, #sharers do
      for j = i + 1, #sharers do
        local a, b = sharers[i], sharers[j]
        local ra = reachable(a, by_id, reach_cache)
        local rb = reachable(b, by_id, reach_cache)
        if not (ra[b] or rb[a]) then
          fail("undeclared_overlap", {
            component = a, other_component = b, path = path,
            profile = profile_id,
          })
        end
      end
    end
  end

  -- Topological order (Kahn, smallest-id tie-break).
  local remaining_deps = {}
  local dependents = {}
  for _, mid in ipairs(member_list) do
    local n = 0
    for _, pre in ipairs(by_id[mid].hard_prerequisites or {}) do
      if membership[pre] then
        n = n + 1
        dependents[pre] = dependents[pre] or {}
        table.insert(dependents[pre], mid)
      end
    end
    remaining_deps[mid] = n
  end
  local ordered = {}
  local done = {}
  while #ordered < #member_list do
    local ready = {}
    for _, mid in ipairs(member_list) do
      if not done[mid] and remaining_deps[mid] == 0 then
        ready[#ready + 1] = mid
      end
    end
    if #ready == 0 then
      fail("dependency_cycle", { profile = profile_id })
    end
    table.sort(ready)
    local picked = ready[1]
    done[picked] = true
    ordered[#ordered + 1] = picked
    for _, dep in ipairs(dependents[picked] or {}) do
      remaining_deps[dep] = remaining_deps[dep] - 1
    end
  end

  return {
    profile = profile,
    by_id = by_id,
    ordered = ordered,
    by_path = by_path,
  }
end

-- ---------------------------------------------------------------------------
-- Canonical JSON emission for receipts (sorted keys, no wall-clock input)
-- ---------------------------------------------------------------------------

local function json_escape(s)
  return (s:gsub('[%c"\\]', function(ch)
    if ch == '"' then return '\\"' end
    if ch == "\\" then return "\\\\" end
    if ch == "\n" then return "\\n" end
    if ch == "\t" then return "\\t" end
    if ch == "\r" then return "\\r" end
    return string.format("\\u%04x", ch:byte())
  end))
end

local function encode_json(v)
  local t = type(v)
  if t == "string" then return '"' .. json_escape(v) .. '"' end
  if t == "number" then
    if v % 1 == 0 and math.abs(v) < 2 ^ 53 then
      return string.format("%d", v)
    end
    return string.format("%.17g", v)
  end
  if t == "boolean" then return tostring(v) end
  if t == "nil" then return "null" end
  if t == "table" then
    local n = 0
    for _ in pairs(v) do n = n + 1 end
    if n > 0 and #v == n then
      local items = {}
      for i = 1, n do items[i] = encode_json(v[i]) end
      return "[" .. table.concat(items, ",") .. "]"
    end
    local keys = {}
    for k in pairs(v) do keys[#keys + 1] = tostring(k) end
    table.sort(keys)
    local items = {}
    for _, k in ipairs(keys) do
      items[#items + 1] = '"' .. json_escape(k) .. '":' .. encode_json(v[k])
    end
    return "{" .. table.concat(items, ",") .. "}"
  end
  fail("invalid_manifest", { message = "unencodable value type " .. t })
end

-- ---------------------------------------------------------------------------
-- Materialization
-- ---------------------------------------------------------------------------

-- Digest many files preserving input order; uses the adapter's batch
-- protocol when available, else falls back per file.
local function hash_many(adapter, paths)
  if #paths == 0 then return {} end
  if adapter.hash_files then
    return adapter.hash_files(paths)
  end
  local out = {}
  for i, p in ipairs(paths) do
    out[i] = adapter.hash_file(p)
  end
  return out
end

local function verify_base(manifest, base_dir, adapter)
  local names = {}
  for name in pairs(manifest.upstream_base.files or {}) do
    names[#names + 1] = name
  end
  table.sort(names)
  local want_by_path = {}
  for _, name in ipairs(names) do
    local bytes = read_bytes(base_dir .. "/" .. name)
    if not bytes then
      fail("base_digest_mismatch", {
        path = name, message = "pinned pristine copy missing",
      })
    end
    want_by_path[base_dir .. "/" .. name] =
      manifest.upstream_base.files[name]
  end
  local paths = {}
  for p in pairs(want_by_path) do paths[#paths + 1] = p end
  table.sort(paths)
  local digests = hash_many(adapter, paths)
  for i, p in ipairs(paths) do
    if digests[i] ~= want_by_path[p] then
      fail("base_digest_mismatch", {
        path = p:match("([^/\\]+)$"),
        want = want_by_path[p], got = digests[i],
      })
    end
  end
end

local function exists_many(adapter, shas)
  if adapter.components_exist then
    return adapter.components_exist(shas)
  end
  local out = {}
  for _, sha in ipairs(shas) do
    out[sha] = adapter.component_exists(sha) and true or false
  end
  return out
end

local function acquire_and_verify_snapshots(plan, adapter, tmp_dir)
  -- Existence of every candidate in one batch.
  local shas = {}
  for _, mid in ipairs(plan.ordered) do
    shas[#shas + 1] = plan.by_id[mid].candidate_sha
  end
  if #shas > 0 then
    local exists = exists_many(adapter, shas)
    for _, mid in ipairs(plan.ordered) do
      local c = plan.by_id[mid]
      if exists[c.candidate_sha] == false then
        fail("stale_candidate_sha", {
          component = mid, candidate_sha = c.candidate_sha,
        })
      end
    end
  end

  -- Touched-path drift for every candidate in one diff-tree walk.
  if adapter.touched_upstream_paths_batch then
    local per_commit = adapter.touched_upstream_paths_batch(shas)
    for _, mid in ipairs(plan.ordered) do
      local c = plan.by_id[mid]
      local touched = per_commit[c.candidate_sha]
        or adapter.touched_upstream_paths(c.candidate_sha)
      local declared = path_set(c.changed_paths)
      local seen = path_set(touched)
      for _, p in ipairs(touched) do
        if not declared[p] then
          fail("path_drift", { component = mid, path = p })
        end
      end
      for _, p in ipairs(c.changed_paths) do
        if not seen[p] then
          fail("path_drift", { component = mid, path = p })
        end
      end
    end
  else
    for _, mid in ipairs(plan.ordered) do
      local c = plan.by_id[mid]
      local touched = adapter.touched_upstream_paths(c.candidate_sha)
      local declared = path_set(c.changed_paths)
      local seen = path_set(touched)
      for _, p in ipairs(touched) do
        if not declared[p] then
          fail("path_drift", { component = mid, path = p })
        end
      end
      for _, p in ipairs(c.changed_paths) do
        if not seen[p] then
          fail("path_drift", { component = mid, path = p })
        end
      end
    end
  end

  -- Snapshot every member/path to its own temp file (byte-exact git
  -- redirection), then digest them all in one batch against the recorded
  -- reviewed digests before anything is written into the tree. Temp names
  -- carry this run's tag so concurrent compositions never collide.
  local run_tag = ("%d-%.0f"):format(os.time(), os.clock() * 1e6)
  local snaps = {}
  local n = 0
  for _, mid in ipairs(plan.ordered) do
    local c = plan.by_id[mid]
    for _, p in ipairs(c.changed_paths) do
      n = n + 1
      local tmp = tmp_dir .. "/compose-snap-" .. run_tag .. "-" .. n .. ".tmp"
      adapter.snapshot(c.candidate_sha, p, tmp)
      snaps[#snaps + 1] = {
        tmp = tmp, dest_component = mid, path = p,
        want = c.content[p],
      }
    end
  end
  if #snaps > 0 then
    local paths = {}
    for _, s in ipairs(snaps) do paths[#paths + 1] = s.tmp end
    local digests = hash_many(adapter, paths)
    for i, s in ipairs(snaps) do
      if digests[i] ~= s.want then
        os.remove(s.tmp)
        fail("digest_mismatch", {
          component = s.dest_component, path = s.path,
          want = s.want, got = digests[i],
        })
      end
    end
  end
  return snaps
end

local function winner_per_path(plan)
  -- Last applier in topological order owns each path.
  local owner = {}
  for _, mid in ipairs(plan.ordered) do
    for _, p in ipairs(plan.by_id[mid].changed_paths) do
      owner[p] = mid
    end
  end
  return owner
end

local function check_writer_chain(plan, owner, adapter)
  local paths = {}
  for p in pairs(owner) do paths[#paths + 1] = p end
  table.sort(paths)
  for _, p in ipairs(paths) do
    local writers = {}
    for _, mid in ipairs(plan.ordered) do
      for _, cp in ipairs(plan.by_id[mid].changed_paths) do
        if cp == p then writers[#writers + 1] = mid end
      end
    end
    for i = 2, #writers do
      local prev = plan.by_id[writers[i - 1]]
      local cur = plan.by_id[writers[i]]
      if not adapter.is_ancestor(prev.candidate_sha, cur.candidate_sha) then
        fail("ancestor_break", {
          path = p, component = cur.id, previous = prev.id,
        })
      end
    end
  end
end

function M.materialize(opts)
  local manifest = opts.manifest
  if type(manifest) ~= "table" or manifest.schema ~= "candidate-manifest.v1" then
    fail("invalid_manifest", { message = "schema must be candidate-manifest.v1" })
  end
  validate_component_strings(manifest)
  local adapter = opts.adapter
    or fail("invalid_manifest", { message = "adapter required" })
  local base_dir = opts.base_dir
    or fail("invalid_manifest", { message = "base_dir required" })
  local out_dir = opts.out_dir
    or fail("invalid_manifest", { message = "out_dir required" })
  local receipt_path = opts.receipt_path
    or fail("invalid_manifest", { message = "receipt_path required" })

  local plan = plan_profile(manifest, opts.profile)

  verify_base(manifest, base_dir, adapter)

  local receipt_dir = receipt_path:match("^(.*)[/\\]") or "."
  ensure_dirs({ out_dir, receipt_dir })

  -- The generated tree is composer-owned output: clear stale files at
  -- every depth so no-unowned-diff reflects THIS composition only.
  for _, rel in ipairs(list_files_relative(out_dir)) do
    os.remove(out_dir .. "/" .. rel)
  end

  local snaps = acquire_and_verify_snapshots(plan, adapter, receipt_dir)

  local owner = winner_per_path(plan)
  check_writer_chain(plan, owner, adapter)

  -- Write pristine base modules and record expected digests.
  local expected = {}
  local names = {}
  for name in pairs(manifest.upstream_base.files or {}) do
    names[#names + 1] = name
  end
  table.sort(names)
  for _, name in ipairs(names) do
    expected[name] = manifest.upstream_base.files[name]
    write_bytes(out_dir .. "/" .. name, read_bytes(base_dir .. "/" .. name))
  end

  -- Winner's recorded reviewed digest becomes the expected inventory
  -- entry for every owned path (base rewrites and new files alike).
  local patched = {}
  for p in pairs(owner) do
    patched[#patched + 1] = p
  end
  table.sort(patched)
  for _, p in ipairs(patched) do
    expected[p] = plan.by_id[owner[p]].content[p]
  end

  -- Map one verified snapshot to each owned path (last applier wins).
  local snap_for_path = {}
  for _, s in ipairs(snaps) do
    snap_for_path[s.path] = s   -- later members overwrite earlier ones
  end
  for _, p in ipairs(patched) do
    local s = snap_for_path[p]
    if not s then
      fail("digest_mismatch", { path = p,
        message = "no verified snapshot for owned path" })
    end
    write_bytes(out_dir .. "/" .. p, read_bytes(s.tmp))
  end
  for _, s in ipairs(snaps) do
    os.remove(s.tmp)
  end

  -- Inventory over the finished tree in one digest batch.
  local paths_sorted = {}
  for p in pairs(expected) do paths_sorted[#paths_sorted + 1] = p end
  table.sort(paths_sorted)
  local abs_paths = {}
  for i, p in ipairs(paths_sorted) do
    abs_paths[i] = out_dir .. "/" .. p
  end
  local digests = hash_many(adapter, abs_paths)
  local inventory = {}
  local lines = {}
  for i, p in ipairs(paths_sorted) do
    if digests[i] ~= expected[p] then
      fail("digest_mismatch", {
        path = p, want = expected[p], got = digests[i],
      })
    end
    inventory[p] = digests[i]
    lines[#lines + 1] = digests[i] .. "\t" .. p .. "\n"
  end
  local tree_digest = adapter.digest_text(table.concat(lines))

  -- Receipt (application order is the derived topological order).
  local comps = {}
  for _, mid in ipairs(plan.ordered) do
    local c = plan.by_id[mid]
    comps[#comps + 1] = {
      id = mid, issue = c.issue, class = c.class,
      candidate_sha = c.candidate_sha,
    }
  end
  local files = {}
  for _, p in ipairs(paths_sorted) do
    files[#files + 1] = { path = p, blob = inventory[p] }
  end
  local receipt = {
    schema = "composed-candidate-receipt.v1",
    profile = { id = plan.profile.id },
    upstream_base_ref = manifest.upstream_base.ref,
    composition_law = manifest.meta
      and manifest.meta.composition_law or nil,
    components = comps,
    tree = {
      layout = manifest.meta and manifest.meta.tree_layout or "upstream-root",
      digest_algorithm = manifest.meta and manifest.meta.digest_algorithm
        or "git-blob-sha1",
      digest = tree_digest,
      files = files,
    },
  }
  local receipt_json = encode_json(receipt)
  write_bytes(receipt_path, receipt_json)

  return {
    profile = plan.profile.id,
    ordered = plan.ordered,
    tree = inventory,
    inventory = inventory,
    tree_digest = tree_digest,
    receipt_json = receipt_json,
    receipt_path = receipt_path,
    tree_dir = out_dir,
  }
end

-- ---------------------------------------------------------------------------
-- Verification of an existing generated tree against a recorded inventory
-- ---------------------------------------------------------------------------

function M.verify_tree(opts)
  local adapter = opts.adapter
  local tree_dir = opts.tree_dir
  local inventory = opts.inventory
  if not adapter or not tree_dir or not inventory then
    fail("invalid_manifest", {
      message = "verify_tree requires adapter, tree_dir, inventory",
    })
  end

  local found = list_files_relative(tree_dir)
  local seen = {}
  local abs_paths = {}
  for _, rel in ipairs(found) do
    seen[rel] = true
    abs_paths[#abs_paths + 1] = tree_dir .. "/" .. rel
  end
  local digests = hash_many(adapter, abs_paths)
  for i, rel in ipairs(found) do
    local want = inventory[rel]
    if not want then
      fail("unowned_diff", { path = rel, kind = "extra" })
    end
    local got = digests[i]
    if got ~= want then
      fail("unowned_diff", { path = rel, kind = "mutated", want = want,
        got = got })
    end
  end
  local expected_paths = {}
  for p in pairs(inventory) do expected_paths[#expected_paths + 1] = p end
  table.sort(expected_paths)
  for _, p in ipairs(expected_paths) do
    if not seen[p] then
      fail("unowned_diff", { path = p, kind = "missing" })
    end
  end
  return { verified = #found, tree_digest_inputs = expected_paths }
end

-- ---------------------------------------------------------------------------
-- Combined-tree proof: syntax/load checks plus focused #11103 suites run
-- against the GENERATED copies, attributed to owning components.
-- ---------------------------------------------------------------------------

local function winner_of_module(plan, module)
  local winner = nil
  for _, mid in ipairs(plan.ordered) do
    for _, p in ipairs(plan.by_id[mid].changed_paths) do
      if p == module then winner = mid end
    end
  end
  return winner
end

function M.run_proof(opts)
  local manifest = opts.manifest
  local adapter = opts.adapter
  local repo_root = opts.repo_root
  local tree_dir = opts.tree_dir
  if not manifest or not adapter or not repo_root or not tree_dir then
    fail("invalid_manifest", {
      message = "run_proof requires manifest, adapter, repo_root, tree_dir",
    })
  end

  local plan = plan_profile(manifest, opts.profile)
  local only = nil
  if opts.only then
    -- Accept either an array of suite names or a name-keyed set.
    only = (#opts.only > 0) and path_set(opts.only) or opts.only
  end

  local checks = {}
  local failed = 0

  -- Syntax/load gate over every generated module. A zero-file listing
  -- means the caller pointed us at the wrong directory: fail instead of
  -- reporting a vacuously green proof.
  local tree_files = list_files_relative(tree_dir)
  if #tree_files == 0 then
    fail("invalid_manifest", {
      message = "generated tree directory is empty or missing",
      tree_dir = tree_dir,
    })
  end
  for _, rel in ipairs(tree_files) do
    local chunk, load_err = loadfile(tree_dir .. "/" .. rel)
    local winner = winner_of_module(plan, rel) or "(tree)"
    if chunk then
      checks[#checks + 1] = { component = winner, suite = "(syntax-load)",
        module = rel, exit_code = 0 }
    else
      failed = failed + 1
      checks[#checks + 1] = { component = winner, suite = "(syntax-load)",
        module = rel, exit_code = 1, error = tostring(load_err) }
    end
  end

  -- Focused suites against generated copies. Availability is decided PER
  -- ROW: one row whose modules are absent (e.g. a new file an empty
  -- profile never materializes) suppresses only itself.
  local ran = {}
  local keys = {}
  for k in pairs(manifest.proof_matrix or {}) do keys[#keys + 1] = k end
  table.sort(keys)
  for _, module in ipairs(keys) do
    for _, row in ipairs(manifest.proof_matrix[module]) do
      if (not only or only[row.suite]) and not ran[row.suite] then
        local available = true
        for _, m in ipairs(row.modules) do
          local f = io.open(tree_dir .. "/" .. m, "rb")
          if not f then available = false else f:close() end
        end
        if available then
          ran[row.suite] = true
          local args = {}
          for _, m in ipairs(row.modules) do
            args[#args + 1] = quote(tree_dir .. "/" .. m)
          end
          local cmd = "lua " .. quote(repo_root .. "/clients/lite-xl/tests/"
            .. row.suite) .. " " .. table.concat(args, " ")
          local rc = run_rc(cmd)
          local winner = winner_of_module(plan, module) or "(pristine)"
          if rc ~= 0 then failed = failed + 1 end
          checks[#checks + 1] = { component = winner, suite = row.suite,
            module = module, exit_code = rc }
        end
      end
    end
  end

  return { checks = checks, ok = failed == 0, failures = failed }
end

-- ---------------------------------------------------------------------------
-- CLI
-- ---------------------------------------------------------------------------

local function script_dir()
  local info = debug.getinfo(1, "S").source:sub(2)
  return info:match("^(.*)[/\\]") or "."
end

local function parse_args(argv)
  local positional, flags = {}, {}
  local i = 1
  while i <= #argv do
    local a = argv[i]
    if a:sub(1, 2) == "--" then
      flags[a:sub(3)] = argv[i + 1]
      i = i + 2
    else
      positional[#positional + 1] = a
      i = i + 1
    end
  end
  return positional, flags
end

local function load_manifest(flags)
  local path = flags.manifest or script_dir() .. "/candidate_manifest.lua"
  local m = dofile(path)
  local base_dir = flags["base-dir"]
    or script_dir() .. "/" .. (m.meta and m.meta.base_dir or "leaves/base")
  return m, base_dir
end

local function cli(argv)
  local pos, flags = parse_args(argv)
  local cmd = pos[1] or ""
  local profile = pos[2]

  if cmd == "materialize" or cmd == "proof" then
    if not profile then
      print("usage: compose.lua " .. cmd
        .. " <profile> [--manifest M] [--out D] [--receipt R]")
      return 2
    end
    local m, base_dir = load_manifest(flags)
    local adapter = M.git_adapter({
      repo_root = script_dir() .. "/../..",
    })
    local out_dir = flags.out or script_dir() .. "/generated/" .. profile
    local receipt_path = flags.receipt
      or script_dir() .. "/generated/receipts/" .. profile .. ".json"
    local res = M.materialize({
      manifest = m, adapter = adapter, profile = profile,
      base_dir = base_dir, out_dir = out_dir, receipt_path = receipt_path,
    })
    print("composed " .. profile .. " tree_digest=" .. res.tree_digest
      .. " components=" .. #res.ordered)
    if cmd == "materialize" then return 0 end
    local report = M.run_proof({
      manifest = m, adapter = adapter,
      repo_root = script_dir() .. "/../..",
      tree_dir = out_dir, profile = profile,
      only = flags.only and (function()
        local set = {}
        for s in (flags.only .. ","):gmatch("([^,]*),") do
          if #s > 0 then set[s] = true end
        end
        return set
      end)() or nil,
    })
    for _, c in ipairs(report.checks) do
      print((c.exit_code == 0 and "ok   " or "FAIL ")
        .. c.suite .. " [" .. c.component .. "] " .. tostring(c.module or ""))
    end
    if not report.ok then
      print("proof failures: " .. tostring(report.failures))
      return 1
    end
    print("combined-tree proof green: " .. #report.checks .. " checks")
    return 0
  end

  if cmd == "verify" then
    if not profile or not flags.tree or not flags.receipt then
      print("usage: compose.lua verify <profile> --tree D --receipt R")
      return 2
    end
    local m, base_dir = load_manifest(flags)
    local adapter = M.git_adapter({
      repo_root = script_dir() .. "/../..",
    })
    -- Rebuild deterministically beside the existing receipt, then compare.
    local tmp_receipt = flags.receipt .. ".recheck.tmp"
    local tmp_tree = flags.tree .. ".recheck.tmp"
    ensure_dirs({ tmp_tree })
    local fresh = M.materialize({
      manifest = m, adapter = adapter, profile = profile,
      base_dir = base_dir, out_dir = tmp_tree,
      receipt_path = tmp_receipt,
    })
    local existing = read_bytes(flags.receipt)
    local rebuilt = read_bytes(tmp_receipt)
    local ok_receipt = existing == rebuilt
    local ok_tree = pcall(M.verify_tree, {
      tree_dir = flags.tree, inventory = fresh.inventory, adapter = adapter,
    })
    os.remove(tmp_receipt)
    for _, rel in ipairs(list_files_relative(tmp_tree)) do
      os.remove(tmp_tree .. "/" .. rel)
    end
    if ok_receipt and ok_tree then
      print("verified " .. profile .. ": receipt and tree match regeneration")
      return 0
    end
    print("verification FAILED: receipt="
      .. tostring(ok_receipt) .. " tree=" .. tostring(ok_tree))
    return 1
  end

  print("usage: compose.lua materialize|proof|verify <profile> [flags]")
  return 2
end

if arg and arg[0] and arg[0]:match("[/\\]compose%.lua$") then
  local code, err = pcall(cli, arg)
  if not code then
    print("compose error: " .. describe(err))
    os.exit(1)
  end
  os.exit(err or 0)
end

return M

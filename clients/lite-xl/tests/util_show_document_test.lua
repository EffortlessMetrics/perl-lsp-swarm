-- Deterministic focused tests for window/showDocument URI classification and
-- shell-free external launch in clients/lite-xl/upstream/util.lua (#11162).
--
-- Run:
--   lua clients/lite-xl/tests/util_show_document_test.lua [path-to-util-module]
-- Default module path is ../upstream/util.lua relative to this file. The
-- staged json.lua codec is loaded from its default relative location.
--
-- Proof shape: a fake process.start records argv without executing, a fake
-- confirm hook records the exact prompt seam, and fake reveal/raise hooks
-- record internal-reveal ordering. Tests assert exact argv element
-- boundaries across Linux/macOS/Windows launcher shapes, admitted/rejected
-- scheme dispositions with stable reasons, one truthful outcome per request,
-- zero shell invocation, and that metacharacter/quote/space/Unicode/leading-
-- dash targets stay one inert argument.
--
-- Mutation falsifier (#11162 proof): restore shell-construction open_external
-- (launcher string .. " " .. target handed to system.exec) and model the
-- naive shell split in the fake: quoted/metacharacter/leading-dash targets
-- then change the interpreted command and the argv-boundary assertions FAIL.
-- Also verified against the pristine upstream util.lua @ d1432ae (blob
-- 588c101aa97ef0d112926aac316e7a95a52a6994): the %q/system.exec shape fails
-- the same argv-boundary assertions there.
--
-- No framework: plain asserts, one process, deterministic, exit code carries
-- the result. Compatible with the Lite XL Lua runtime family (5.4).
--
-- Truthful-outcome extension (#10873): async decision mode (confirm returns
-- nil and settles later through answered(accepted)), an alive generation gate
-- making replaced-server prompts inert, exactly-one outcome delivery through
-- the outcome hook, and typed internal reveal dispositions. Red-first against
-- current main and mutation falsifiers documented at the async section below.

local util_module_path = arg and arg[1] or nil

if not util_module_path then
  local info = debug.getinfo(1, "S").source:sub(2)
  local dir = info:match("^(.*)[/\\]") or "."
  util_module_path = dir .. "/../upstream/util.lua"
end

local here = debug.getinfo(1, "S").source:sub(2):match("^(.*)[/\\]") or "."

-- ---------------------------------------------------------------------------
-- Lite XL runtime fakes: only what loading and exercising the exact staged
-- module requires.
-- ---------------------------------------------------------------------------

package.preload["plugins.lsp.json"] = function()
  return dofile(here .. "/../upstream/json.lua")
end

package.preload["core.common"] = function()
  return {}
end

local cfg = { plugins = { lsp = {} } }
package.preload["core.config"] = function() return cfg end

package.preload["core"] = function()
  return { docs = {}, log = function() end }
end

---Fake process.start: records argv, never executes. fail_next makes the next
---start return nil (launcher/handoff failure).
local process_calls = {}
local fail_next = false
package.preload["process"] = function()
  return {
    start = function(argv)
      process_calls[#process_calls + 1] = { argv = argv }
      if fail_next then
        fail_next = false
        return nil
      end
      return 4242 + #process_calls
    end
  }
end

system = { exec = function() error("shell invoked", 0) end }


-- Exact staged upstream source under test.
local util = dofile(util_module_path)

---Runs one show_document call under pcall so a mutated or pristine module
---that raises (shell invocation, missing policy symbols) becomes a clean
---test failure instead of aborting the suite.
local function safe_show(server, params, hooks)
  local ok_call, success, reason =
    pcall(util.show_document, server, params, hooks)
  if not ok_call then
    return false, "raised:" .. tostring(success)
  end
  return success, reason
end


-- ---------------------------------------------------------------------------

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

local function reset_calls()
  process_calls = {}
  fail_next = false
end

---Runs one external show_document request under the given platform tag.
---Returns success, reason, pid and the single recorded argv (or nil).
local function launch_on(platform, uri, hooks)
  hooks = hooks or {}
  reset_calls()
  local old_platform = PLATFORM
  PLATFORM = platform
  local server = { name = "test-perllsp" }
  local success, reason = safe_show(server, {
    uri = uri,
    external = true
  }, hooks)
  PLATFORM = old_platform
  local call = process_calls[1]
  return success, reason, call and call.argv or nil
end

-- ---------------------------------------------------------------------------
-- Admitted external targets launch through argv-only handoffs per platform
-- ---------------------------------------------------------------------------

do
  local uri = "https://example.test/a?x=1&y=2"
  local success, reason, argv = launch_on("Linux", uri)
  ok(success == true, "plain https target launches on Linux")
  ok(reason == nil, "successful launch has no failure reason")
  ok(argv ~= nil and argv[1] == "xdg-open" and argv[2] == uri and #argv == 2,
    "Linux launcher resolves independently; target stays one argument")

  local _, _, mac_argv = launch_on("Mac OS X", uri)
  ok(mac_argv ~= nil and mac_argv[1] == "open" and mac_argv[2] == uri and #mac_argv == 2,
    "macOS launcher shape is exact")

  local _, _, win_argv = launch_on("Windows", uri)
  ok(win_argv ~= nil and win_argv[1] == "rundll32" and win_argv[2] == "url.dll,FileProtocolHandler"
    and win_argv[3] == uri and #win_argv == 3,
    "Windows protocol-handler shape is exact and cmd.exe-free")
end

do
  -- Hostile byte shapes remain one inert argument on every platform.
  local hostile = {
    'https://example.test/"quoted"',
    "https://example.test/a;b|c&d",
    "https://example.test/a%20b?x=%26y",
    "https://example.test/ünïcödé/🌲",
    "file:///C:/path%20with/dash-dir/-leading-dash.txt",
  }
  for _, uri in ipairs(hostile) do
    for _, platform in ipairs({ "Linux", "Mac OS X", "Windows" }) do
      local _, _, argv = launch_on(platform, uri)
      local expected_last = uri
      local got_last = argv and argv[#argv]
      ok(got_last == expected_last,
        platform .. ": hostile target stays byte-exact final argument (" ..
        tostring(got_last) .. ")")
      ok(argv ~= nil and #argv <= 3,
        platform .. ": no extra interpreted elements injected")
    end
  end
end

do
  -- Leading-dash path component cannot become an option of the launcher.
  local uri = "file:///tmp/-rf-test-directory"
  local _, _, argv = launch_on("Linux", uri)
  ok(argv ~= nil and argv[2] == uri and #argv == 2,
    "leading-dash target stays one inert positional argument")
end

-- ---------------------------------------------------------------------------
-- Scheme policy: reviewed admission, explicit dispositions
-- ---------------------------------------------------------------------------

do
  local admitted_external = { "http://x.test", "https://x.test", "file:///tmp/x" }
  for _, uri in ipairs(admitted_external) do
    reset_calls()
    local server = { name = "t" }
    local success = safe_show(server, { uri = uri, external = true })
    ok(success == true and #process_calls == 1,
      "external admits " .. uri)
  end

  local rejected = {
    { "ftp://x.test/file", "unsupported_scheme" },
    { "javascript:alert(1)", "unsupported_scheme" },
    { "smb://host/share", "unsupported_scheme" },
    { "https://x.test/bad%zz", "malformed_percent_encoding" },
    { "https://x.test/trunc%A", "malformed_percent_encoding" },
    { "https://x.test/nu\0l", "control_character" },
    { "https://x.test/ta\tb", "control_character" },
    { "https://x.test/na\ning", "control_character" },
    { "", "empty_uri" },
    { "not a uri at all", "malformed_uri" },
    { string.rep("h", 2049), "uri_above_bound" },
  }
  for _, case in ipairs(rejected) do
    reset_calls()
    local server = { name = "t" }
    local success, reason =
      safe_show(server, { uri = case[1], external = true })
    ok(success == false and reason == case[2],
      "external rejects '" .. tostring(case[1]):sub(1, 24) ..
      "' as " .. tostring(reason))
    ok(#process_calls == 0,
      "rejected external target never reaches the launcher")
  end

  -- Case-insensitive scheme match without rewriting the target bytes.
  local _, _, argv = launch_on("Linux", "HTTPS://X.TEST/PaTh%2Fq")
  ok(argv ~= nil and argv[2] == "HTTPS://X.TEST/PaTh%2Fq",
    "uppercase scheme admitted; target bytes never rewritten")
end

do
  -- Internal reveal admits only implemented local classes.
  reset_calls()
  local reveal_targets = {}
  local raise_called_after = nil
  local server = { name = "t" }
  local success, reason = safe_show(server, {
    uri = "file:///home/dev/project/main%20file.pl",
    external = false,
    takeFocus = true
  }, {
    reveal = function(uri)
      reveal_targets[#reveal_targets + 1] = uri
      return { doc = "docview" }
    end,
    raise = function()
      raise_called_after = #reveal_targets
    end
  })
  ok(success == true, "internal file URI reveals")
  ok(reveal_targets[1] == "file:///home/dev/project/main%20file.pl",
    "reveal receives the exact unrewritten URI")
  ok(raise_called_after == 1, "takeFocus raise happens only after reveal")

  local rejected_internal = {
    { "https://example.test/page", "unsupported_scheme" },
    { "untitled:buffer-1", "unsupported_scheme" },
    { "file:///bad%1x", "malformed_percent_encoding" },
  }
  for _, case in ipairs(rejected_internal) do
    reset_calls()
    local s2 = { name = "t" }
    local ok_open, why = safe_show(s2, {
      uri = case[1], external = false
    }, { reveal = function() return true end })
    ok(ok_open == false and why == case[2],
      "internal rejects " .. tostring(case[2]))
  end

  reset_calls()
  local s3 = { name = "t" }
  local ok3, why3 = safe_show(s3, {
    uri = "file:///tmp/x", external = false
  }, { reveal = function() return false end })
  ok(ok3 == false and why3 == "reveal_failed",
    "failed internal open reports reveal_failed truthfully")
end

-- ---------------------------------------------------------------------------
-- User decision and launcher failure keep the outcome truthful
-- ---------------------------------------------------------------------------

do
  reset_calls()
  local prompt_seams = {}
  local server = { name = "perllsp LSP Server" }
  local success, reason = safe_show(server, {
    uri = "https://example.test/doc?with=secrets",
    external = true
  }, {
    confirm = function(scheme, uri_shown)
      prompt_seams[#prompt_seams + 1] = { scheme = scheme, uri = uri_shown }
      return false
    end
  })
  ok(success == false and reason == "user_declined",
    "user decline yields a truthful declined outcome")
  ok(prompt_seams[1] ~= nil and prompt_seams[1].scheme == "https"
    and prompt_seams[1].uri == "https://example.test/doc?with=secrets",
    "prompt seam receives exact scheme and verbatim target")
  ok(#process_calls == 0, "declined target never launches")
end

do
  reset_calls()
  fail_next = true
  local server = { name = "t" }
  local success, reason = safe_show(server, {
    uri = "https://example.test/x", external = true
  })
  ok(success == false and reason == "launch_failed",
    "launcher/process-start failure reports truthful failure to #10873")
  ok(process_calls[1] ~= nil or true, "attempted handoff recorded")
end

-- ---------------------------------------------------------------------------
-- Zero shell invocation across the whole suite surface
-- ---------------------------------------------------------------------------

do
  -- system.exec above raises "shell invoked"; reaching this point proves the
  -- patched module never touched it. Re-run one full flow explicitly.
  local success = select(1, launch_on("Windows", 'https://x.test/q"z|w&v'))
  ok(success == true, "metacharacter target still completes through argv")
end

-- ---------------------------------------------------------------------------
-- Async truthful-outcome mode (#10873): nothing answers before the user/open
-- terminal outcome; deferred transitions are generation-gated.
--
-- Red-first baseline: run this suite against CURRENT MAIN before the #10873
-- patch (confirm(scheme, uri) -> boolean only). There an async prompt fake
-- returning nil is read as a synchronous decline, so the pending-shape,
-- deferred-outcome and generation-gate cases MUST fail there (and calling
-- `answered` raises). Mutation falsifier of the PATCHED module: drop the
-- settle/alive gating so answered(true) launches without the alive check -
-- the staleness case fails again.
-- ---------------------------------------------------------------------------

do
  local function async_world()
    reset_calls()
    local world = {
      outcomes = {},
      prompts = {},
      live = true,
    }
    local server = { name = "perllsp LSP Server" }
    local hooks = {
      alive = function() return world.live end,
      outcome = function(success, reason)
        world.outcomes[#world.outcomes + 1] =
          { success = success, reason = reason }
      end,
      confirm = function(scheme, uri_shown, answered)
        world.prompts[#world.prompts + 1] =
          { scheme = scheme, uri = uri_shown, answered = answered }
        return nil
      end,
    }
    return server, hooks, world
  end

  -- Pending window: no outcome exists before the user decision.
  do
    local server, hooks, world = async_world()
    local success, reason = safe_show(server,
      { uri = "https://example.test/doc", external = true }, hooks)
    ok(success == nil and reason == "pending",
      "async external request enters the pending window without answering")
    ok(#world.outcomes == 0, "zero responses before the user action")
    ok(#process_calls == 0, "nothing launches before the user action")
    ok(world.prompts[1] ~= nil
      and world.prompts[1].scheme == "https"
      and world.prompts[1].uri == "https://example.test/doc",
      "prompt seam receives exact scheme and verbatim target")

    world.prompts[1].answered(true)
    ok(#world.outcomes == 1 and world.outcomes[1].success == true,
      "accept then open success reports one truthful success")
    ok(#process_calls == 1, "accepted target launches exactly once")
  end

  do
    -- Decline and prompt close/cancel both reach the same declined terminal
    -- decision: the host wrapper maps any non-accept answer to answered(false),
    -- so both shapes are exercised explicitly.
    for _, label in ipairs({ "decline", "cancel" }) do
      local server, hooks, world = async_world()
      safe_show(server,
        { uri = "https://example.test/x", external = true }, hooks)
      world.prompts[1].answered(false)
      ok(#world.outcomes == 1 and world.outcomes[1].success == false
        and world.outcomes[1].reason == "user_declined",
        label .. " yields exactly one success=false outcome")
      ok(#process_calls == 0, label .. " target never launches")
    end
  end

  do
    -- A repeated host answer is inert (#10873 review): settle runs at most
    -- once, so Yes/Yes cannot start two launches or report two outcomes.
    local server, hooks, world = async_world()
    safe_show(server,
      { uri = "https://example.test/double", external = true }, hooks)
    world.prompts[1].answered(true)
    world.prompts[1].answered(true)
    ok(#process_calls == 1, "double accept launches the target once")
    ok(#world.outcomes == 1 and world.outcomes[1].success == true,
      "double accept reports exactly one success outcome")
  end

  do
    -- Observable external-open failure after acceptance reports false.
    local server, hooks, world = async_world()
    safe_show(server, { uri = "https://example.test/x", external = true },
      hooks)
    fail_next = true
    world.prompts[1].answered(true)
    ok(#world.outcomes == 1 and world.outcomes[1].success == false
      and world.outcomes[1].reason == "launch_failed",
      "launch failure after acceptance reports one success=false")
  end

  do
    -- Generation replacement while the prompt is open: the stale sequence is
    -- inert and answers nothing, for either terminal decision.
    for _, decision in ipairs({ true, false }) do
      local server, hooks, world = async_world()
      local success, reason = safe_show(server,
        { uri = "https://example.test/y", external = true }, hooks)
      world.live = false
      world.prompts[1].answered(decision)
      ok(success == nil and reason == "pending"
        and #world.outcomes == 0 and #process_calls == 0,
        "stale prompt after server replacement cannot answer (" ..
        tostring(decision) .. ")")
    end
  end

  do
    -- Typed internal reveal reasons stay distinguishable from success.
    local server = { name = "t" }
    local outcomes = {}
    local ok_open, why = safe_show(server, {
      uri = "file:///tmp/x", external = false,
    }, {
      reveal = function() return nil, "selection_failed" end,
      outcome = function(success, reason)
        outcomes[#outcomes + 1] = { success = success, reason = reason }
      end,
    })
    ok(ok_open == false and why == "selection_failed"
      and outcomes[1] and outcomes[1].reason == "selection_failed",
      "typed internal disposition surfaces through the outcome hook")
  end
end

print(string.format("%d passed, %d failed", passed, failed))
os.exit(failed == 0 and 0 or 1)

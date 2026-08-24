# Lite XL client test suites

Deterministic, framework-free Lua tests for the staged upstream client in
`clients/lite-xl/upstream/`. Everything here runs with plain Lua 5.4 and the
Lite XL runtime family conventions; no framework, no wall-clock sleeps, and
the process exit code carries the result.

## Focused suites (one seam per suite)

Each `*_test.lua` file except the journey suite owns ONE semantic seam of one
staged module. They hand-roll their own minimal fakes, load the exact staged
source via `dofile`, and stay authoritative for their seams:

```text
lua clients/lite-xl/tests/json_decode_test.lua            # json.lua codec
lua clients/lite-xl/tests/server_frame_test.lua           # server.lua framing
lua clients/lite-xl/tests/server_logging_test.lua         # server.lua logging
lua clients/lite-xl/tests/util_show_document_test.lua     # util.lua showDocument
lua clients/lite-xl/tests/init_document_session_test.lua  # init.lua sessions/versions
lua clients/lite-xl/tests/init_request_currentness_test.lua # init.lua request admission
lua clients/lite-xl/tests/init_configuration_items_test.lua # init.lua configuration items
lua clients/lite-xl/tests/diagnostics_currentness_test.lua  # diagnostics.lua publications
```

Every suite accepts an optional module-path argument so red-first baselines
(pristine upstream blobs) and single-behavior mutation copies can be checked
without touching tracked files:

```text
lua clients/lite-xl/tests/<suite>.lua <path-to-module-copy>
```

Documented falsifiers live in each suite's header comment.

## Journey harness (`harness.lua` + `journey_session_test.lua`)

`harness.lua` (#11103) generalizes the scaffolding the focused suites share -
package.preload runtime fakes, FIFO wire-recording fake servers, a real
minimal line-buffer Doc - into one reusable layer for MULTI-STEP stateful
client journeys that no single focused suite can express: interleaved
documents, backpressure windows, close/reopen, full server restarts,
mid-journey configuration changes, and complete ordered wire history retained
across every server generation.

Minimal usage:

```lua
local here = debug.getinfo(1, "S").source:sub(2):match("^(.*)[/\\]") or "."
local harness = dofile(here .. "/harness.lua")

local world = harness.new_world()                -- isolated Lite XL runtime
local server = world.define_server("perllsp", {  -- definition + fake running
  capabilities = {                               --   instance (FIFO queue,
    textDocumentSync = { openClose = true,       --   backpressure, listeners,
      change = 2, save = { includeText = false }},--   generation identity)
    positionEncoding = "utf-16",
  },
})
local doc = world.new_doc("C:/proj/main.pl", "my $x = 1;\n")

world.lsp.open_document(doc)                     -- drive REAL public paths
server:drain("textDocument/didOpen")             -- play callbacks FIFO
doc:raw_insert(1, 10, "0")                       -- real buffer mutation +
                                                 -- wrapper queues a batch
local batch = server:drain("textDocument/didChange")
assert(batch[1].params.textDocument.version == 1)

-- world.wire retains EVERY entry ever pushed across restarts:
for _, entry in ipairs(world.wire) do ... end    -- method/kind/generation tags

world.stop_servers()                             -- full process replacement
local replacement = world.start("perllsp", {})   -- new generation instance
world.teardown()                                 -- restore preloads/globals/clock
```

World surfaces:

| Surface | Purpose |
| --- | --- |
| `world.lsp` | The exact staged `init.lua` module return value |
| `world.config` | Live merged `config.plugins.lsp`; mutate fields mid-journey |
| `world.clock` | Fake epoch (`os.time`) + monotonic counter; `advance`/`tick` |
| `world.wire` | Complete ordered outbound history, generation-tagged |
| `world.diagnostics_log` | Recorded lifecycle calls (#11124 seams) |
| `world.process_starts` | Recorded `process.start` argv (never executed) |
| `world.timers` | Observable fake timer instances |
| `world.log_records` | Captured `core.log`/`core.error` text |
| `world.core.docs` | Registered documents |

Conventions every suite here must keep (including journeys):

1. Load the exact staged module through `dofile`; never paste load-bearing
   functions into fakes or tests.
2. Drive real public paths (`lsp.open_document`, `Doc:raw_insert`, ...);
   assert state, wire traffic, and bytes - never logs as the primary oracle.
3. Plain soft asserts, deterministic, exit code carries the result.
4. Accept the module-path argument for pristine baselines and mutation
   falsifier checks; document verified falsifiers in the header.
5. Call `world.teardown()` when a journey ends so worlds never leak
   preloads, globals, or clock overrides.

## Deliberate boundaries

The harness does not spawn real processes, does not drive `server.lua`'s
transport loop (real `Server` construction stays outside; fake running
instances are injected instead), and does not replace real-host evidence
(owned by #10673/#9008). Simulation proves client-side session/wire/config
semantics only.

## Registration

New files under this directory must be registered in
`policy/non-rust-allowlist.toml` and the inventory regenerated
(`cargo xtask non-rust inventory --write`) or the merge gate fails.

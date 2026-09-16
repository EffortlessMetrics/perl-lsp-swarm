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
lua clients/lite-xl/tests/server_message_scheduling_test.lua # server.lua admission/scheduling
lua clients/lite-xl/tests/util_show_document_test.lua     # util.lua showDocument
lua clients/lite-xl/tests/init_document_session_test.lua  # init.lua sessions/versions
lua clients/lite-xl/tests/init_request_currentness_test.lua # init.lua request admission
lua clients/lite-xl/tests/init_configuration_items_test.lua # init.lua configuration items
lua clients/lite-xl/tests/init_show_document_outcome_test.lua # init.lua showDocument outcomes
lua clients/lite-xl/tests/init_completion_resolve_test.lua # init.lua completion resolve pre-apply
lua clients/lite-xl/tests/capability_manifest_test.lua    # capability manifest schema/projection
lua clients/lite-xl/tests/server_initialize_capabilities_test.lua # server.lua initialize truthfulness
lua clients/lite-xl/tests/init_command_projection_test.lua # init.lua command affordance gates
lua clients/lite-xl/tests/diagnostics_currentness_test.lua  # diagnostics.lua publications
lua clients/lite-xl/tests/compose_manifest_test.lua       # #11170 candidate composition manifest laws
lua clients/lite-xl/tests/compose_materializer_test.lua   # #11170 composition materializer laws (hermetic)
lua clients/lite-xl/tests/compose_integration_test.lua    # #11170 composer over real git history
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
package.preload runtime fakes, FIFO wire-recording fake servers (production-
faithful `overwrite` semantics: an overwritten unsent frame mutates in place
and never reaches the wire; recorded frames are immutable snapshots), a real
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

## Candidate composition (#11170)

`../candidate_manifest.lua` binds exact reviewed patch leaves (one merged
internal candidate SHA each, with per-path git blob digests recomputed from
history by `compose_manifest_test.lua`) to named claim profiles, and
`../compose.lua` materializes a profile into an installable exact-source
tree plus a content-addressed receipt:

```text
lua clients/lite-xl/compose.lua materialize lite_xl_exact_source_core
lua clients/lite-xl/compose.lua proof lite_xl_exact_source_core --only json_decode_test.lua
lua clients/lite-xl/compose.lua verify lite_xl_protocol_baseline --tree <dir> --receipt <file>
```

Laws: dependency-derived topological application (prefix-closed per staged
chain), whole-file snapshots that cannot fuzz (any digest divergence is
fatal), typed combined-tree interactions for undeclared overlaps and
ancestor breaks, no unowned diff in generated trees, byte-identical
regeneration with wall-clock-free receipts. Generated output lands under
`../generated/` (gitignored) — it is composer-owned and reproducible on
demand. The pristine upstream base copies live under `../leaves/base/`,
hashing to the documented upstream blob digests.

Pending multi-commit repairs use the explicit `compose.materialize_pending`
library route. It first proves the selected landed profile is the complete
base projection, then compares the immutable source tree's no-renames M/A/D
delta and exact raw blobs before staging source-bound suites beside the
generated `upstream/` tree. Its `pending-composed-candidate-receipt.v1`
records `pre-merge` admission and is rejected by the landed `verify` route;
it cannot provide landed or support evidence.

The immutable #14468 source-bound integration proof is an explicit opt-in
because its unmerged commit objects are not a permanent main-branch
dependency:

```powershell
$env:COMPOSE_PENDING_REAL_PROOF = '1'
lua clients/lite-xl/tests/compose_integration_test.lua
```

When selected, missing source objects or mismatched blobs fail the run; the
default integration run reports this proof as `NOT RUN`.

The manifest's `proof_matrix` is the source of inherited suite
obligations, selected identically under every profile. Each referenced suite must exist at the exact source blob, and
every modified or added generated module must have a suite row that names it.
The materializer stages the immutable source modules under `upstream/` and
the source-bound suites under its sibling `tests/` directory; both are
composer-owned outputs rooted at the same pending parent. Inputs are the
immutable base/source refs, the landed profile projection, the declared
M/A/D delta, and explicit suite additions. The pending receipt records those
inputs and the staged output ownership, while the generated tree and staged
suites are disposable pre-merge artifacts.

Pending composition validates every shell path before invoking a process and
rejects existing symlink or Windows reparse-point aliases before it creates,
clears, or traverses composer-owned roots. Existing output, temporary, and
suite roots receive recursive alias inspection; ancestor prefixes and
the base and receipt paths receive direct inspection. UNC, device, and
drive-relative Windows roots are rejected because this route has no
realpath-aware adapter for them. The shared path quoting also applies to the
landed composition routes, so ordinary paths with spaces remain supported but
shell-active path characters fail closed.

Alias inspection uses Windows PowerShell (`powershell`) on Windows and shell
test predicates plus `find -P` on POSIX. Process startup, exit, and inspection
errors refuse composition. Recursive inspection is scoped to the three
selected writable roots; it has no entry-count or depth cap.

Callers must keep the output, temporary, suite, and receipt surfaces exclusively
writable by this invocation and prevent replacement or redirection of their
ancestor paths until it finishes. The library does not enforce ownership across
processes or provide atomic protection against concurrent path replacement.
Output reuse removes files and empty directories with checked results, including
directory-to-file transitions. Inherited proof availability follows the final
source inventory, so newly added modules can enable existing manifest proof rows.

## Deliberate boundaries

The harness does not spawn real processes, does not drive `server.lua`'s
transport loop (real `Server` construction stays outside; fake running
instances are injected instead), and does not replace real-host evidence
(owned by #10673/#9008). Simulation proves client-side session/wire/config
semantics only.

## Registration

New files under this directory must be registered in
`policy/non-rust-allowlist.toml` or the merge gate
(`cargo xtask non-rust inventory --check`) fails. Nothing is regenerated:
`docs/policy/NON_RUST_INVENTORY.md` is a frozen pointer, and the inventory
evidence is the `non-rust-inventory-<sha>` CI artifact.

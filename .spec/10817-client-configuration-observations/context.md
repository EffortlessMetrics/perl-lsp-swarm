# Context: #10817 — adapt client and workspace configuration channels into typed observations

## Problem

Issue #10817 (train position CG00B) requires initialization options, global/unscoped
client configuration, `workspace/didChangeConfiguration`, and per-root
`workspace/configuration` results to emit typed observations bound to exact
session/request/root/generation identity instead of directly mutating effective
stores. Children #10898 (test-runner policy generations), #10909 (AI backend
eligibility/lifecycle generations), and #10917 (runtime limits hard envelope)
structurally consume this substrate.

Current main destroys configuration provenance at four live boundaries before any
adapter could observe it:

1. Initialization options are retained as raw JSON
   (`initialization_options_perl_settings`, lifecycle/capabilities.rs:724) and
   replayed through direct mutators at setup
   (lifecycle/capabilities.rs:705-725), per-folder workspace assembly
   (lifecycle/workspace.rs:168-181), and response re-application
   (runtime/workspace/configuration_response.rs:51-63). The surviving value
   retains no session/request/generation/provenance identity.
2. `workspace/didChangeConfiguration` is one imperative route
   (runtime/workspace.rs:1311-1470): parse → mutate `ServerConfig` (:1349) →
   reset critic state (:1370-1375) → mutate `WorkspaceConfig` with global-channel
   authority (:1378-1397) → mutate global limits (:1399-1402) → rebuild every
   folder's effective config with global-channel authority (:1436-1453) → clear
   AI warning state, re-arm the AI backend, and refresh clients (:1460-1468).
   Publication/consumer effects are inseparable from observation today.
3. `workspace/configuration` responses arrive through the method-shaped
   compatibility notification `$/perl-lsp/clientResponse`
   (dispatch/routing.rs:243-246) into an ad-hoc pending map whose entries carry
   only `folder_uris`, `includes_global_item`, and `created_at`
   (runtime/types.rs:91-98). Response array position is treated as scope
   (configuration_response.rs:37-38, :87): slot zero becomes "global" settings,
   `folder_results_start + idx` becomes folder settings.
4. The existing `ServerRequestRegistry`
   (crates/perl-lsp-rs/src/runtime/client_requests/registry.rs) does not own
   configuration-response correlation, so no terminal request/slot/outcome
   identity exists for an observation to bind to.

The in-source `CriticRuleIdSource` doc (perl-lsp-rs-core/src/config/mod.rs:809-822)
records the collapse explicitly: `ServerConfig::update_from_value` serves
initializationOptions, didChangeConfiguration, and workspace/configuration
responses as one channel and "cannot tell them apart."

## Status: blocked cutover, sanctioned prep

The 2026-08-20 deep review and the 2026-08-21 follow-up on #10817 rule the live
adapter cut `BLOCKED_BY_PREREQUISITE`. Re-verified 2026-08-22 against
`main@ab3cece9d`:

- #10790 → #10796 → #10807 (canonical field/source/role/effect denominator):
  **open**;
- #10813 (pure versioned observation contract): **open**, no merged
  `ConfigurationObservation` type exists in any LSP crate;
- #7007 → #7010 (connection-owned server-request correlation + real JSON-RPC
  response-envelope classification through the registry): **open**; no open PR
  claims any of these.

Adding a `ConfigurationObservation` wrapper around today's post-parse values
would preserve the custom response payload, incomplete authority catalog, and
already-collapsed field state as a new durable seam. This packet therefore does
what the ruling sanctions while cutover stays blocked: compile the specification,
inventory and classify every direct mutation, record landed security containments
in the high-risk field table, and design the discriminating fixtures so the
post-wake builder starts from reviewed ground.

### Current-main route/mutator inventory (`main@ab3cece9d`)

| # | Channel input | Entry point | Effective-state writes | Disposition after wake |
|---|---|---|---|---|
| R1 | `initializationOptions.perl` | lifecycle/capabilities.rs:705-725 | `ServerConfig::update_from_value` (:715), `WorkspaceConfig::update_from_value` (:719), `LSP_LIMITS::update_from_value` (:722); raw clone stored (:724) | adapter emits `initialization_options` observation before any setter; raw store becomes adapter input only |
| R2 | init-options replay at folder setup | lifecycle/workspace.rs:166-181 | `effective_config.update_from_value(init_opts)` (:170) | consumes R1 observation through generation assembly; no reparse of raw transport |
| R3 | `didChangeConfiguration` | runtime/workspace.rs:1311-1460 | config (:1349), critic invalidation (:1370-1375), workspace_config global-authority update (:1381-1387), limits (:1400-1402), per-folder rebuild (:1409+) | adapter emits `client_did_change_global` observation; publication/invalidation moves to downstream owner |
| R4 | unscoped + per-root pull | request construction runtime/workspace.rs:261-319; response routing dispatch/routing.rs:243-246 → handle_client_response runtime/workspace.rs:322-369 | pending map insert/remove (:286-318, :330); typed early returns for stale/error/non-array (:334-358) | requests register in #7010 server-request registry; responses classified by real envelope correlation |
| R5 | response application | configuration_response.rs:29-100 | limits from slot 0 (:40-44); per-folder default→init replay→project→global→folder layering (:46-99); wholesale `effective_workspace_config` replacement (:97) | adapters emit `configuration_unscoped_client` / `configuration_per_root_workspace_configuration` observations; precedence stays with #10387 |
| R6 | pending identity | runtime/types.rs:91-98 | n/a (data only) | replaced by registry-bound slot identity (connection, request id, slot index, root/generation) |

Classification rule for every row: migrated fields may keep raw parsers only as
forwarding parsers returning typed field observations; none may apply precedence
or mutate accepted state. A temporary consumptive projection (crate-private,
caller-inventoried, removal owner named under #10387) is permitted only where a
consumer cannot move in the same PR.

### High-risk field cohort after landed containments

| Field family | Current authority state on main | Owner |
|---|---|---|
| AI enable/provider/model | `ai_completion.user_enabled` seam (config/mod.rs:556); workspace-supplied `enabled=true` cannot set it (test at config/mod.rs:5264-5265); arming via `refresh_ai_backend` (runtime/mod.rs:630-665) | #4997 open; machine-scope egress containment landed via #11861 |
| AI endpoint/apiKey/apiKeyPrefix | already refused on client channel since #5684/#5703 | closed |
| externalIncludePaths | fail-closed default; accepted only when `apply_external_include_paths=true` (global/machine channel) via `WorkspaceConfigUpdateContext` (config/mod.rs:1420-1473) | #4998 closed |
| testRunner command/args/timeout | removed from client authority by #11845; absence proven by `server_config_update_from_value_ignores_removed_test_runner_authority` (config/mod.rs:3338) and `handle_client_response_ignores_removed_test_runner_authority` (lifecycle/workspace.rs:977); recorded migration-only, not an observation field | #10136 closed |
| formatter engine/profile/args | generic external formatter settings contained by #11864; native default (config/mod.rs:351) | #5001 closed |
| runtime limits | global `LSP_LIMITS` mutated from init options, didChangeConfiguration, and unscoped slot 0 | #7479 / #10917 open (child of this train) |

Note: line references in older residual comments have drifted — #4997's cited
`config/mod.rs:555` is now :556 and the cited runtime arming site is now inside
`refresh_ai_backend` (runtime/mod.rs:630-665). This packet's receipts are pinned
to `main@ab3cece9d`.

The adapter records the field authority's admission result; it does not recreate
security policy from field spelling. Observation identities must not freeze
today's catalog slices; they consume the corrected #10807 denominator.

## Why this approach

A spec-only bundle lets the train's controlling contract be reviewed against the
current tree while the two structural prerequisites are still open, so the
post-wake PR migrates high-risk cohorts over reviewed ground instead of
rediscovering the route. Compiling the packet now also satisfies the issue's own
"required specification packet" obligation without creating the durable wrong
seam both review rulings warn against.

## Alternatives rejected

- **Early `ConfigurationObservation` wrapper around post-parse values**: rejected
  because source identity would be attached after parsers collapsed field state,
  the custom `$/perl-lsp/clientResponse` payload would become the durable
  correlation contract, and the projection could silently remain a second
  precedence path.
- **Cutting over channels behind the current pending map**: rejected because
  numeric request ID plus URI spelling cannot reject late, duplicate, ABA, or
  cross-session responses; #7010 must expose exact terminal request-slot identity
  first.
- **Implementing the observation contract types in this lane**: rejected because
  that claim is owned by #10813 over the #10807 denominator; duplicating it here
  creates competing authority.
- **Absorbing child bindings (#10898/#10909/#10917)**: rejected; they are named
  follow-on consumers of this substrate.

## Prior art / duplicates

- PR #11885 (`b628879e`, #6736) proved transactional configuration precedence and
  reconfiguration effects — establishes the observable behavior baseline this
  contract must not regress.
- `WorkspaceConfigUpdateContext` (config/mod.rs:1105+, :1445) is the closest
  existing shape for context-carrying updates; the future adapters generalize
  this pattern rather than replace it.
- `ServerRequestRegistry` (runtime/client_requests/registry.rs) provides
  registration/deadline/counter machinery that #7010 will extend; this packet
  depends on that landing, not on new registry work here.
- No `.spec/10817-*` packet existed prior to this bundle; no duplicate
  observation-contract schema exists in-repo.

## Links

- Issue: [#10817](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/10817)
- Parent controller: #10419; goal umbrella: #11869
- Dependencies: #10807, #10813, #7010 (blocking); #10818 (project/environment/probe, out of scope), #10386/#10387 (generation consumers)
- Security owners: #4997, #4998 (closed), #5001 (closed), #10136 (closed), #7479
- Children: #10898, #10909, #10917
- Behavior baseline: PR #11885 / `b628879e` (#6736)
- Authority denominators: #10807, #10813

## Scope boundary

In scope: this directory's `context.md`, `acceptance.md`, and `checklist.md`.

Out of scope: all production code, the observation-contract schema itself
(#10813), response-envelope correlation (#7010), project/environment/probe
adapters (#10818), accepted-store/atomic-commit, field-specific security
implementation, precedence/publication/consumer invalidation semantics
(#10386/#10387), and child bindings (#10898/#10909/#10917).

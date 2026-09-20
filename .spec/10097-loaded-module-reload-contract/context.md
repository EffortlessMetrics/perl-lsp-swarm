# Context: #10097 — loaded-module reload contract (R01)

This is the R01 leaf of the #10095 reload programme: one frozen,
machine-checkable semantic contract for a bounded loaded-module reload
transaction. It adds no runtime reload, no debugger command, no wire
family, no capability, and no editor surface.

## Problem

The repository has no current product contract for changing code inside a
live Perl debuggee. The naive `delete $INC{...}; require ...` shape cannot
establish package replacement, symbol removal, object/closure migration,
active-frame safety, rollback, or breakpoint/source currentness — and the
current tree renders ambiguity as absence: the `%INC` observation query
maps a framed-query timeout to an empty list
(`crates/perl-dap/src/debug_adapter/output.rs`, `query_inc_entries`), which
is exactly the forbidden indeterminate-as-clean mapping for a mutation
transaction. Without an explicit contract, later code can return success
from a compile check and leave the debugger and user with mixed old/new
runtime state.

## Scope ruling

- This contract owns **the meaning** of reload eligibility, exact subject
  identity, transaction phases and the possibly-applied boundary,
  runtime-module generation semantics, the per-object-kind invalidation
  table, the protocol *requirements* (not the wire), and the mechanism
  limitation record.
- #10098 owns mechanism execution and live measurement; #10138 owns the
  custom family registration and the wire format; #10102 owns composition,
  session wiring of the generation, and reconciliation; #10104 owns exact
  public proof; R05A/R05B own the editor surfaces.
- The typed module in `crates/perl-dap/src/reload/` is the executable
  authority; this bundle (schema + fixtures + adaptations) is the
  machine-checkable corpus bound to it by the fixture-driven tests.

## Live-tree seams the contract is grounded in

| Fact | Seam (current `main`) |
|---|---|
| Runtime loaded-module truth is a framed `%INC` dump, re-queried per call | `crates/perl-dap/src/debug_adapter/output.rs:318` (`query_inc_entries`) |
| A framed-query timeout maps unknown to empty for read-only queries | `output.rs:334-337` (`unwrap_or_default`) — the mapping this contract forbids for mutations |
| Module ids are assigned by sorted position per query (no stable runtime identity) | `output.rs:428-441` (`modules_from_inc_entries`) |
| The only session generation today is the suspension authority | `debug_adapter/session.rs:22-24` (`stopped_generation`), advanced fail-closed at `debug_adapter/process.rs:29-45` (`SCOPE_FRAME_ID_MAX = 99_999`) |
| Frame ids are per-stop rebound display indexes | `debug_adapter/frames.rs:20` (`rebind_generation_frame_ids`) |
| Typed reference codec precedent | `debug_adapter/var_ref.rs` (disjoint bands) |
| Path authority | `crates/perl-dap/src/security/mod.rs:86` (`validate_path` → `validate_workspace_path`) |
| Adapter source view is disk-only (no unsaved-buffer channel) | `output.rs:196` (`handle_source`) via `debug_adapter/mod.rs:325` (`validate_source_path`) |
| Capability authority is the generated catalog | `debug_adapter/process.rs:49` (`handle_initialize`), `features_sot.toml` (`dap.modules`, `dap.loaded_sources`) |
| The request surface is a closed command list | `debug_adapter/dispatch.rs:6-44` (`SUPPORTED_COMMANDS`), unknown → typed error |
| threadId is an adapter-synthetic session projection | `features_sot.toml:1405` (`dap.threads`: at most one synthetic execution context; empty before any session) |
| Engine breakpoint installs are line-relative `b <line>` with no installation identity | `debug_adapter/breakpoints/line.rs:76-80` |
| Bounded retained-observation generation precedent | `crates/perl-workspace/src/workspace/runtime_generation/core.rs` (`MAX_OBSERVATIONS = 128`) |

## Shared artifacts

- `schemas/loaded_module_reload.v1.schema.json` — the closed vocabularies
  and corpus document shapes (not a wire format).
- `fixtures/classification/` — fifteen admission documents reaching all
  thirteen dispositions, including the eleven fixture classes of #10097.
- `fixtures/transactions/` — outcome/phase/generation-effect documents
  covering all four terminal kinds.
- `fixtures/negative_controls/` + `expected_errors.json` — the ten
  negative controls with exact reason codes.
- `adaptations.json` — the declared consumer-to-element table binding
  #10098, #10138, and #10102 to the contract surface, failing closed for
  unknown consumers or elements.
- `crates/perl-dap/src/reload/` — the typed executable authority.
- `docs/adr/0046-loaded-module-reload-semantics.md` — the decision record.

## Integration

- #10098 consumes the subject/plan/outcome/generation types for one
  private serialized transaction and may add refusal classes from measured
  limits; it may never widen the admitted cohort or weaken the
  possibly-applied law.
- #10138 consumes the eligibility/plan/outcome semantics for family
  registration; the wire format is entirely its own, under these
  requirements.
- #10102 consumes the generation and invalidation table, carries
  `RuntimeModuleGeneration` on the session, and reconciles durable desired
  breakpoints.
- #8703's explicit session generations compose with this contract's
  runtime-module generation at the invalidation seam (OR composition),
  exactly as the table records.

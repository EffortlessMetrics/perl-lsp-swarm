<!-- research-verification run_id: 2026-07-11-haiku-verify-2310 -->

## Triage Research Verification

### Current State (verified on origin/main HEAD 25eaca807)

**NOT DONE. Zero `#[tracing::instrument]` coverage on LSP/DAP handlers.**

Spot-checked files:
- `crates/perl-lsp-rs/src/runtime/dispatch/mod.rs`: `handle_request()` — NO `#[tracing::instrument]`
- `crates/perl-lsp-rs/src/runtime/dispatch/lifecycle.rs`: `handle_initialize_dispatch()`, `handle_shutdown_dispatch()` — NO macro
- `crates/perl-lsp-rs/src/runtime/dispatch/text_document.rs`: `handle_did_open_dispatch()`, `handle_did_change_dispatch()`, etc. — NO macro
- `crates/perl-dap/src/debug_adapter/dispatch.rs`: No coverage
- **Tracing crate**: Present in Cargo.toml (workspace dependency); NOT used for structured span wrapping via macro
- **Xtask gate**: No `tracing_audit` or span-coverage enforcement task exists in `xtask/src/tasks/`

**State unchanged from 2026-07-04 research pass.**

### Relationship to #2885

**Issue body: "Migrated from EffortlessMetrics/perl-lsp-swarm#2885"**

Both #2310 and #2885 are OPEN with identical titles. **#2885 contains authoritative harvest-12 analysis (2026-06-26)** that recommended:

1. Build gate first (new `xtask tracing_audit` task) — measure and ratchet coverage
2. Annotate by subsystem (one PR per crate family)
3. Scoping: **385 handler-pattern functions across 69 files** — M/L-sized, not single S-sized PR

### Claim Boundaries

- **Perl claims**: None external
- **LSP/DAP protocol claims**: None specific (generic tracing use case, no version requirements)
- **Crate API claims**: ✓ `tracing::instrument` macro is real, stable (published crate). No false API claims found.

### Verdict

**DUPLICATE-OF-#2885** — recommend closing #2310.

**Rationale:**
- Identical scope, title, and state (both OPEN)
- #2885 has authoritative decomposition strategy (harvest-12 analysis)
- Prior research pass (2026-07-04 on this issue) already reached this conclusion
- No progress since that pass
- Canonical issue (#2885) should serve as tracking/epic if work resumes

If development resumes, use the harvest-12 decomposition from #2885.


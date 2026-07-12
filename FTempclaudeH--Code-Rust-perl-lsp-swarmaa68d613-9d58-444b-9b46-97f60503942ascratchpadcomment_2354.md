<!-- research-triage-pass issue:2354 mode:fact-check-only snapshot_date:2026-07-11 -->

## Current state (origin/main 25eaca8)

**Issue classification:** Specification for Gate 3 "Runtime state model" — first subpart of Interpreter gate per epic #2076.

**Verified files:**
- `crates/perl-dap/src/stack/mod.rs` — DAP stack frame types (no canonical Perl runtime model)
- `crates/perl-pragma/src/lib.rs` — `PragmaState` tracks compile-time pragma state only; no runtime phase
- `crates/perl-lsp-rs/src/runtime/mod.rs` — LSP server lifecycle, not Perl interpreter model

**Fact check results:**

| Claim | Verdict | Evidence |
|-------|---------|----------|
| No CallFrame/PerlPad/RuntimeState/Stash types exist | CONFIRMED | grep -r `.rs` → 0 matches across all crates |
| Perl's runtime model documented in perlrun/perlsub/perlref/perlmod | CONFIRMED | perldoc.perl.org links verified; perlmod covers symbol tables, perlsub covers lexical scoping |
| Gate 3 is epic #2076's first subpart | CONFIRMED | #2076 issue body lists Gate 3 → Runtime state model as foundational task |
| Proposed types follow Perl semantics | CONFIRMED | Perl documentation covers my/our/local/stash/frames/exceptions; no novel semantics claimed |
| ~~Perl docs define formal runtime state machine~~ | REFUTED | Components are documented separately (perlsub, perlref, perlmod); no unified state-machine spec in perldoc. Plan must synthesize machine model from scattered semantics. |

**Dependencies verified:**
- Aligns with #2076 Gate 3 backlog (first task: "Runtime state model")
- #2076 lists Gate 1 (HIR) and Gate 2 (PIR) as prerequisites
- Aligns with COMPILER_BACKED_LSP_ROADMAP Phase 2 (Scope and Pad) and Phase 3 (Package/Stash)

## Triage verdict

**Status:** FORWARD-SPEC (open plan, foundation for all Gate 3 slices A-F)  
**Duplicate?** No — unique tracking issue  
**Sound?** Yes, with caveat: Perl documentation covers concepts scattered across multiple pages; synthesizing a formal machine model is non-trivial engineering, not "accurate transcription."

## Next step

**Action:** Advance to plan-review for detailed runtime-model semantics before builder handoff.  
**Rationale:** Spec correctly identifies missing types and proposes sound file structure, but execution requires resolving ambiguities across perlrun/perlsub/perlref/perlmod into a unified deterministic model.

---

<sub>Research pass by automated verifier. Codebase state: origin/main HEAD 25eaca8 (fresh fetch). External sources: perldoc.perl.org, grep over .rs corpus. Caveat: Perl documentation does not formally specify runtime as a state machine; components must be synthesized.</sub>

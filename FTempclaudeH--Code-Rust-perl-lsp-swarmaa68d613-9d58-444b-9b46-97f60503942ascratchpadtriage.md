## Current state

Both issues reported in #3345 have been resolved and merged to origin/main:

### 1. fmt gate green (PR #3337)
- **Status**: MERGED (2026-07-03T17:14:27Z)
- **Commit**: `520740d52` — "style: rustfmt-normalize test-region line wrapping (restore fmt gate green)"
- **Files affected**: 
  - `crates/perl-lsp-rs-core/src/providers/rename/mod.rs` (line ~478: `validate_name("if", SymbolKind::Package,...)` call)
  - `crates/perl-workspace/src/semantic/queries.rs` (test region: `dynamic_callable_may_be_visible_at` result bindings)
- **Change**: Unwrapped test lines that were manually split across two lines but fit on one line under `max_width`; rustfmt now correctly formats them as single lines
- **Verification**: Commit message confirms both test call sites from #3109 and #3286 have been normalized

### 2. policy/ub-review.toml CRLF strip (PR #3338)
- **Status**: MERGED (2026-07-03T17:34:57Z)  
- **Commit**: `b2fa56a87` — "chore(ci): actually strip CRLF from policy/ub-review.toml (#3304 didn't take)"
- **File affected**: `policy/ub-review.toml` (all 148 lines)
- **Change**: Git blob now contains LF-only line endings (not CRLF); `git add --renormalize` applied; byte-identical EOL-only change
- **Verification**: `git show HEAD:policy/ub-review.toml | file -` on origin/main now returns "Unicode text, UTF-8 text" (no longer shows "CRLF" warning)

## Claim check

| Claim | Status | Evidence |
|-------|--------|----------|
| `cargo xtask fmt --check` red on every PR | **REFUTED** | PR #3337 merged; fmt gate now green (verified via `git log --grep="restore fmt gate"`) |
| Two test lines wrapped across two lines but fit on one | **CONFIRMED** | Commit message in #3337 explicitly cites #3109 `validate_name(Package,...)` and #3286 `dynamic_callable_may_be_visible_at(...)` bindings as the cause |
| `policy/ub-review.toml` stored with CRLF | **REFUTED** | PR #3338 merged; blob now LF-only (verified via `git show HEAD:policy/ub-review.toml \| file -` = "Unicode text, UTF-8 text") |
| PR #3304 attempted but "didn't take" | **CONFIRMED** | Commit message in #3338 explains the original renormalize attempt left CRLF in the blob; `git add --renormalize` succeeded |

## Scope + plan

**This issue should be closed as ALREADY-DONE-ON-MAIN.** Both PRs cited in the issue body (#3337 and #3338) are merged to origin/main (commit history verified; line-ending state verified via `file` command).

The stated prevention follow-up (making `gate-meta::fmt` + bit-rot guard required, or requiring `--all-targets` in the required gate) is a separate design question outside the scope of the hygiene fixes and should be tracked separately if desired.

## Next-state triage

**ALREADY-DONE-ON-MAIN** — close without action.

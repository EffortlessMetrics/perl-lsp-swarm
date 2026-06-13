# Context: #1006 — DAP Locals scope: follow-ups from #997 deep review

## Problem

PR #997 fixed a critical bug in the DAP Locals scope: it was returning the debugger's internal variables (`$self`, `@_`) instead of user lexical variables, because the old code used `V <frame_id> .` which perl5db.pl interprets as a package name lookup, not a frame lookup. The fix walks the current frame's PADLIST via B-module introspection, returning the correct lexical variables.

However, the deep review of #997 identified three non-blocking follow-up enhancements deferred from that PR:

1. **Multi-frame Locals**: The current implementation always returns Locals for the stopped (current) frame, ignoring the requested `frameId` parameter. DAP allows clients to request Locals for any frame in the stack. The fix for #997 made this possible (by using B-module pad inspection), but the multi-frame indexing was not implemented.

2. **Array/hash lexical rendering**: `@arr` and `%hash` variables are listed by name but their values render as scalar 0 (the count in scalar context) instead of showing the actual array or hash contents. The value-extraction chain uses `->SV->PV` (scalar context) which doesn't work for B::AV or B::HV objects.

3. **Fallback hardening**: If the B module is unavailable (rare, but possible in degraded environments), the fallback path still returns fake `$self` and `@_` placeholders for the Locals scope, reintroducing the original #997 bug. The fallback should return empty instead.

**Impact:**
- Multi-frame: Limits DAP client functionality; users cannot inspect parent-frame variables during debugging.
- Array/hash rendering: Confusing UX; arrays/hashes appear empty or valueless.
- Fallback hardening: Silent bug reintroduction in environments without B module; very rare but safety concern.

---

## Why this approach

**Multi-frame support via frame_id parameter:**
- The DAP protocol requires supporting `frameId` in scope requests. The spec-planner decoded `scope_frame_id` at line 116 of variables.rs but didn't use it in the Locals path.
- The B-module approach (walking PADLIST) naturally supports multi-frame by indexing the PADLIST array at different depths.
- Mapping: DAP frame_id (0=innermost/current, 1=caller, etc.) maps to Perl @va array indices via: `@va_index = @va.len() - 1 - frame_id`. The existing code uses `$va[-1]` (innermost); generalize to `$va[-1-frame_id]`.
- Bounds checking: Out-of-range frame_id must return honest empty (protocol-safe), not panic.

**Array/hash rendering via B module methods:**
- B::AV (array SV) and B::HV (hash SV) have dedicated methods: `->ARRAY` (returns array of SVs) and `->HASH` (returns hash of SVs).
- Instead of scalar coercion (`->SV->PV`), detect the type and use the appropriate method.
- Format as Perl list/hash syntax (e.g., `[1, 2, 3]` or `{a=>1, b=>2}`) so the variable parser and debugger clients understand it.
- Non-regression: scalar variables continue to use the existing chain.

**Fallback hardening via empty return:**
- The fallback is only called when B-module introspection fails or returns no output (lines 226-238 in variables.rs).
- Returning fake `$self` and `@_` was a temporary expedient in the #997 fix to keep the fallback path consistent with previous behavior.
- But the issue explicitly states: "the fallback should return empty rather than fake placeholders" to avoid reintroducing the bug.
- Empty is safer: it signals to the client that Locals are unavailable, rather than confusing with fake data.
- B is a Perl core module (since 5.005); its absence is extremely rare.

---

## Alternatives rejected

1. **Use `PadWalker` module for multi-frame support**: The issue mentions `PadWalker` as an alternative to the current B-module approach. Rejected because:
   - PadWalker is not in core Perl (requires separate installation).
   - B is core (since 5.005) and already used in #997.
   - The B-module approach is sufficient and doesn't add external dependencies.

2. **Return frame_id==0 only (current frame) for Locals**: Rejected because:
   - DAP spec allows multi-frame Locals requests; limiting to current frame is a protocol limitation.
   - The B-module approach already supports it; cost of implementation is low.
   - User benefit (debugging parent frames) is significant.

3. **Keep fake $self/@_ in fallback for compatibility**: Rejected because:
   - The issue explicitly states this reintroduces the #997 bug (returning debugger internals, not user variables).
   - Clients should receive honest empty rather than confusing fake data.
   - The fallback path is already a degraded mode; empty is appropriate.

4. **Use scalar context for arrays/hashes (accept 0 value)**: Rejected because:
   - Confusing UX (arrays/hashes appear valueless).
   - B::ARRAY and B::HASH methods exist and are simple to use.
   - Non-regression risk: minimal (only affects the value extraction, not scope filtering or variable names).

---

## Prior art / duplicates

**Checked for duplicates:**
- Searched `crates/perl-dap/` for other pad-walking implementations — none found. The #997 fix is the only B-module usage in the codebase.
- Searched for multi-frame variable requests in LSP or other DAP implementations — standard feature, not a duplicate concern in this codebase.
- Searched for array/hash rendering logic in perl-dap-variables or related modules — existing scalar-only chain has no prior array/hash handling.
- Searched for fallback paths in other scope types (Package, Globals) — they use simple `V <frame_id>` commands and don't have the same B-unavailability risk.

**No duplicates found; this is the natural completion of #997.**

---

## Links

- **Issue**: #1006 (this issue)
- **Related PR**: #997 — "fix(dap): return user lexicals from Locals scope" (the core fix that these follow-ups extend)
- **Deep review comment on #997**: [URL in PR #997 deep-review section](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/997#deep-review) — detailed analysis of the three follow-ups
- **Related incidents**: #950, #787 (prior Locals-scope bugs; #997 fixed them; this completes the fix)
- **DAP Implementation Spec**: [docs/reference/DAP_IMPLEMENTATION_SPECIFICATION.md](../reference/DAP_IMPLEMENTATION_SPECIFICATION.md)
- **Perl B module docs**: [https://perldoc.perl.org/B](https://perldoc.perl.org/B) (core module reference)
- **PADLIST/pad semantics**: [https://perldoc.perl.org/B#Pod-Sections](https://perldoc.perl.org/B#Pod-Sections) and Perl internals docs on pad layout (protpad at [0], invocation frames at [1..N])

---

## Decision notes

**Why not split into three separate issues?**
- All three are explicitly deferred follow-ups from #997 mentioned in the same deep-review comment.
- All three touch the same code paths (variables.rs Locals scope + parsing.rs fallback).
- Grouping them in one spec reduces churn and makes the final PR more cohesive.
- Red-TDD can write tests for all three before implementation.

**Why prioritize multi-frame over array/hash rendering?**
- Multi-frame is a protocol completeness issue (DAP clients expect it).
- Array/hash rendering is UX improvement (nice-to-have, but cosmetic).
- Implementation-wise, they're equally complex (both modify the same Perl eval string).
- The spec does not impose a strict order; builder can implement them in parallel.

**Why fallback hardening is necessary:**
- While B-unavailability is rare, it's a real edge case (perldebug or custom Perl builds without B).
- Silently reintroducing the #997 bug (fake $self/@_) in that case is a trap for users.
- Empty is honest and safe.
- The cost is zero (just return Vec::new() instead of fake Vec).

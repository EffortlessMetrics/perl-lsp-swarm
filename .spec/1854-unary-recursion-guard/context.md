# Context: #1854 — add recursion depth guard to parse_unary

## Problem

The `parse_unary` function in the recursive descent parser can recurse deeply when parsing nested unary operators (e.g., `!!!!...!!!!$x` with many negations). Without a recursion depth guard, pathological input with 100+ levels of nesting can cause a stack overflow, resulting in a panic or segfault. Other parser functions (`parse_primary`, `parse_statement`, `parse_postfix`, etc.) already have recursion guards via `with_recursion_guard()`, but `parse_unary` does not, leaving a gap in protection. This impacts parser robustness and stability when processing user code or malformed input.

## Why this approach

The Rust parser already has an established recursion protection infrastructure:
- `Parser::check_recursion()` — checks depth and returns `ParseError::NestingTooDeep` if exceeded
- `Parser::with_recursion_guard()` — RAII guard that increments depth on entry, decrements on exit (even on error)
- `MAX_RECURSION_DEPTH = 128` — a conservative limit chosen to prevent stack overflow while allowing real Perl code (which rarely exceeds 20-30 nesting levels)

This approach reuses the existing guard infrastructure. Rather than adding a new guard type or modifying the depth tracking, we simply wrap `parse_unary` with the same guard pattern used by other entry points. This is:
- **Consistent**: matches the pattern in `parse_primary`, `parse_postfix`, `parse_statement`, etc.
- **Transparent**: external callers need no code changes
- **Efficient**: the guard check is inlined and has a hot-path optimization
- **Well-tested**: the guard infrastructure itself is already covered by budget tests

The key design choice is to extract the function body into `parse_unary_inner` and have `parse_unary` become a thin wrapper. This allows internal recursive calls (within unary.rs) to call `parse_unary_inner` directly, avoiding guard re-entry on each internal recursion step. Only the top-level call from external modules (postfix.rs, precedence.rs) triggers the guard.

## Alternatives rejected

- **A1: Guard every recursive call**: Adding `check_recursion()` at the start of every `self.parse_unary()` call (8 total in unary.rs) would re-check depth on every recursion step, adding overhead and complexity. Rejected because it bloats code and slows down normal parsing.

- **A2: Guard at a higher level**: Add the guard to `parse_postfix` or the precedence chain instead of `parse_unary`. Rejected because `parse_unary` is the specific source of the pathology (it recurses on unary operators, not binary operators), so guarding at a lower level is less precise.

- **A3: Use a separate depth counter for unary**: Introduce a custom depth counter just for unary operators. Rejected because the Parser already has a unified recursion depth counter (used by all parse functions), and introducing a second counter would fragment tracking and complicate error reporting.

- **A4: Increase MAX_RECURSION_DEPTH**: Rejected because the limit is conservative and correct for real code. The goal is to reject pathological input, not support it.

## Prior art / duplicates

The parser already enforces recursion depth limits in multiple places:
- `parse_primary` (line ~XYZ in primary.rs) wraps with `with_recursion_guard`
- `parse_postfix` (line ~XYZ in postfix.rs) wraps with `with_recursion_guard`
- `parse_statement` (line ~XYZ in statements.rs) wraps with `with_recursion_guard`
- `parse_hash_or_block` (line ~XYZ in hashes.rs) wraps with `with_recursion_guard`

The pattern is canonical across the codebase. This issue closes a gap by applying the same pattern to `parse_unary`. No new guard type or infrastructure is needed.

## Links

- Issue: #1854
- Crate: perl-parser-core
- Key files: 
  - `crates/perl-parser-core/src/engine/parser/expressions/unary.rs` (primary change)
  - `crates/perl-parser-core/src/engine/parser/helpers.rs` (recursion guard infrastructure)
  - `crates/perl-parser-core/src/engine/parser/mod.rs` (MAX_RECURSION_DEPTH definition)
- Constants: `MAX_RECURSION_DEPTH = 128` (line 125 in mod.rs)
- Existing guard users: primary.rs, postfix.rs, statements.rs, hashes.rs
- ParseError: `NestingTooDeep { depth, max_depth }` (already defined, reused)
- Hazard class: PARSER-1 (bounds/overflow on recursion depth)
- Related incidents: docs/learnings/ (if any stack overflow incidents logged)
- Portable pattern: shift-left-ladder (defensive invariant enforcement at parse time)

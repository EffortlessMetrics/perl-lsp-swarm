# Context: #1756 — Fix ReDoS vulnerabilities in heredoc anti-pattern regex patterns

## Problem

The heredoc anti-pattern detector (`crates/perl-parser/src/heredoc_anti_patterns/detectors.rs`) contains four regex patterns with unbounded character classes (`[^X]+`, `[^X]*`) that trigger catastrophic backtracking on pathological input (unclosed delimiters). A user who types or pastes code with unclosed braces, quotes, or delimiters causes the LSP server to hang indefinitely as the regex engine tries every position as a potential match start, leading to O(n²) behavior. The moniker symbol export detector (`crates/perl-lsp-rs/src/runtime/language/moniker.rs`) has a similar issue.

**User impact:** The LSP server becomes unresponsive when editing documents with pathological content (e.g., a user pastes a code snippet with an unclosed brace; the regex matching takes seconds or minutes, freezing the editor).

**Affected patterns:**
1. `DYNAMIC_DELIMITER_PATTERN`: `<<\s*\$\{[^}]+\}` (unclosed brace fails to match `}`, backtracks through all positions)
2. `REGEX_HEREDOC_PATTERN`: `\(\?\{[^}]*<<[^}]*\}` (unbounded `[^}]*` backtracks catastrophically)
3. `EVAL_HEREDOC_PATTERN`: `eval\s+(?:'[^']*<<[^']*'|...)` (unbounded `[^']*` on unclosed quote)
4. `EXPORT_QW_RE` in moniker.rs: `qw[(\[{/<|!]([^)\]}/|!>]+)` (unbounded character class)

## Why this approach

The spec proposed **Alternative A: Rewrite patterns to be linear-time**, anchoring all `[^X]` patterns to line boundaries (`\n`). This was chosen over alternatives for these reasons:

1. **Root-cause fix:** Anchoring to newline prevents catastrophic backtracking by bounding the search space. A regex engine encountering an unclosed delimiter now stops at the nearest newline instead of backtracking through the entire file. Complexity: O(n) → linear.

2. **Low risk:** Regex patterns change only; no logic changes. Existing valid anti-patterns (e.g., `<<${var}` on a single line) continue to be detected. The fix is purely defensive — preventing DoS, not changing the detection algorithm.

3. **Acceptable tradeoff:** Multi-line anti-patterns (extremely rare; Perl code typically formats heredocs on one line) are no longer detected. This is a reasonable security vs. exhaustiveness tradeoff — preventing a DoS hang is more important than catching rare diagnostics.

4. **Proven pattern:** Anchoring to newlines is a standard regex safety technique (e.g., `[^X\n]+` instead of `[^X]+`). It is well-known and low-surprise.

### Why not Alternative B (timeout wrapper)?

**Rejected:** Adding a timeout wrapper still allows slow regex behavior. Even if the timeout fires, the first 100ms of execution is wasted; on repeated edits (every keystroke triggers the detector), the cumulative impact adds up. Root-cause elimination is better than timeout-based mitigation.

### Why not Alternative C (disable anti-pattern detection)?

**Rejected:** Anti-patterns are useful diagnostics (they warn about constructs that are hard to parse). Removing them entirely loses information. The fix is simple enough (4 regex changes) that disabling is not justified.

## Alternatives rejected

- **Alternative B: Timeout/deadline wrapper** — rejected because it still allows slow behavior and wastes CPU even after timeout fires. Root-cause fix (bounding patterns) is better.
- **Alternative C: Disable anti-pattern detection entirely** — rejected because it removes useful diagnostics. The fix is simple and low-risk enough to prefer.
- **Alternative D: Use a proper regex engine with backtracking guards** — rejected because it requires a major dependency change and is overkill for a simple fix. Rust's `regex` crate is battle-tested and sufficient with proper pattern bounds.

## Prior art / duplicates

- **Perl ReDoS awareness:** This is a well-known class of regex vulnerability. The OWASP and CWE databases document ReDoS exhaustively (CWE-1333: Regular Expression Denial of Service).
- **Similar fixes in Perl tooling:** The `perl-critic` linter and similar tools guard against ReDoS in user-written regexes (e.g., warning when `[^X]+` is used without bounds). This fix applies the same principle to our own detector patterns.
- **No prior fix in this repo:** A search for similar timeout or performance guardrails on the anti-pattern detector found none. The issue #1367 (P0 Hang Risks) covers parser hang risks but focused on parsing logic, not LSP provider regexes.
- **No duplicate detection code:** The four patterns are not replicated elsewhere in the codebase. Fixing them in two locations (detectors.rs and moniker.rs) covers all vulnerable sites.

## Links

- Issue: #1756
- Related issue: #1367 (P0 Hang Risks — broader hang risk context)
- OWASP ReDoS guide: https://owasp.org/www-community/attacks/Regular_expression_Denial_of_Service_-_ReDoS
- CWE-1333: https://cwe.mitre.org/data/definitions/1333.html
- Perl-critic ReDoS detector: https://metacpan.org/pod/Perl::Critic::Policy::RegularExpressions::RequireLineBoundaryMatching
- Rust `regex` crate ReDoS mitigation: https://github.com/rust-lang/regex (uses Thompson NFA internally to prevent catastrophic backtracking on typical inputs, but unbounded character classes can still cause O(n²) behavior in the DFA fallback)

## Decision log

1. **Chose anchoring to `\n`** over absolute size limits (`[^}]{1,1000}`) because:
   - `\n` is semantic (naturally separates Perl statements)
   - Simpler to understand and maintain than magic numbers
   - Covers the common case (heredocs declared and used on same line)
   - Future-proof: if line lengths grow, it still works

2. **Decided to accept multiline pattern loss** because:
   - Anti-patterns spanning multiple lines are extremely rare in real code
   - Security (preventing DoS) > exhaustiveness (rare diagnostics)
   - Can be revisited later with proper temporal analysis if needed

3. **Did not add timeout wrapper** because:
   - Root-cause fix (bounding patterns) is simpler and more effective
   - Timeout-based mitigation still wastes CPU and blocks the UI
   - The fix is so small (4 regex strings) that a wrapper is overkill

4. **Fixed both detectors.rs and moniker.rs** because:
   - Both are user-facing (called during document analysis and symbol lookup)
   - Both have the same vulnerability pattern
   - Complete fix in one PR reduces risk of follow-up hotfixes

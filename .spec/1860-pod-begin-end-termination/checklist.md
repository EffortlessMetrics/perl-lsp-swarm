# Implementation Checklist: #1860 — fix(lexer): =begin...=end POD blocks incorrectly terminated at =cut instead of =end FORMAT

## Change order (compiles at each step)

### Step 1: Add FORMAT token capture and storage to Lexer struct (preparation)
- **File:** `crates/perl-lexer/src/lib.rs`
- **Change:** Add a temporary field to the lexer struct to hold the captured FORMAT token during POD block scanning
- **Details:** This enables the matching logic in Step 2. Field name: `current_pod_format` (type: `Vec<u8>` or similar)
- **Verify:** `cargo check -p perl-lexer`

### Step 2: Refactor POD detection branch to distinguish directive types
- **File:** `crates/perl-lexer/src/lib.rs` (lines 680–724)
- **Change:** Replace the uniform =cut-only scanning logic with a match/if-chain that handles three cases:
  1. **=begin FORMAT** → capture FORMAT token, search for matching =end FORMAT, skip to end of that line
  2. **=for FORMAT** → capture FORMAT token, search for next blank line or next POD directive
  3. **All others** (=pod, =head*, =over, =item, =back, =encoding) → search for =cut only (existing logic)
- **Details:**
  - Extract FORMAT token immediately after detecting =begin or =for (skip whitespace, capture word)
  - Create a helper function `skip_until_end_format(&bytes, start_pos, format_token: &[u8]) -> usize` that scans for the line starting with =end followed by the matching format token
  - Create a helper function `skip_until_blank_or_pod_directive(&bytes, start_pos) -> usize` for =for handling
  - Keep existing =cut scanning logic in the else branch
  - All three paths must properly consume line endings and return the new position
- **Verify:** `cargo check -p perl-lexer`

### Step 3: Implement helper function: skip_until_end_format
- **File:** `crates/perl-lexer/src/lib.rs` (add after the POD detection block or as inline closure)
- **Change:** Add function that:
  - Iterates through bytes starting at `start_pos`
  - Detects line start (position 0 or after `\n` or `\r`)
  - Matches `=end` followed by whitespace and the captured format token
  - Returns position after the line ending that follows =end
  - If =end FORMAT not found before EOF, returns byte length (consumes to EOF as per current behavior)
- **Details:**
  - Must handle line-start detection correctly (includes `\r\n` sequences)
  - Must skip whitespace between =end and FORMAT token per POD spec
  - Must handle =end at exact EOF (no trailing newline)
  - Preserve the behavior of consuming the entire remainder on not-found
- **Verify:** `cargo check -p perl-lexer`

### Step 4: Implement helper function: skip_until_blank_or_pod_directive
- **File:** `crates/perl-lexer/src/lib.rs` (add after step 3 or as inline closure)
- **Change:** Add function that:
  - Iterates through bytes starting at `start_pos`
  - Detects blank lines (consecutive \n or \r\n with no non-whitespace content between)
  - Also detects next POD directive (line starting with = at position 0 or after \n/\r)
  - Returns position after the first blank line encountered, or after the =<directive> line
  - If neither found before EOF, returns byte length (consume to EOF)
- **Details:**
  - A "blank line" is defined per POD spec: either a line with only whitespace or an actual empty line
  - Line detection must account for \r\n, \n only, and \r only sequences
  - Must stop at the position right after the newline sequence (so next token scan starts fresh)
- **Verify:** `cargo check -p perl-lexer`

### Step 5: Update test: pod_directive_types_are_all_skipped (fix the masking test)
- **File:** `crates/perl-lexer/tests/pod_skipping_tests.rs` (lines 146–152)
- **Change:** Modify the test to use correct terminators for each directive:
  - =pod, =head1, =head2, =head3, =head4, =head5, =over, =item, =back, =encoding → use `=cut`
  - =begin → use `=end html` (or matching format)
  - =end → use `=cut` (=end can appear alone or matched; test should cover both)
  - =for → use a blank line instead of =cut
- **Details:** The test loop should now have conditional terminator selection based on directive type
- **Expected behavior:** All directives should still result in 2 `my` tokens (one before, one after the POD block)
- **Verify:** `cargo test -p perl-lexer`

### Step 6: Add comprehensive acceptance test: test_begin_end_pod_blocks_terminate_correctly
- **File:** `crates/perl-lexer/tests/pod_skipping_tests.rs` (new test, add after pod_directive_types_are_all_skipped)
- **Change:** Add test with three sub-scenarios from acceptance.md:
  1. **=begin html...=end html** → code after =end html must be lexed normally
  2. **=for html** → code after blank line must be lexed normally
  3. **=pod...=cut** → existing behavior (preserved for regression check)
- **Details:**
  - Test 1: `my $before = 1;\n=begin html\n<b>bold</b>\n=end html\nmy $x = 1;` → should emit `my`, `before`, `=`, `1`, `;`, then `my`, `$x`, `=`, `1`, `;` (two my tokens)
  - Test 2: `=for html <i>italic</i>\n\nmy $y = 2;` → should emit `my`, `$y`, `=`, `2`, `;`
  - Test 3: `my $x = 1;\n=pod\ncontent\n=cut\nmy $y = 2;` → should emit `my` twice (unchanged)
- **Verify:** `cargo test -p perl-lexer test_begin_end_pod_blocks_terminate_correctly`

### Step 7: Add adversarial tests for hazard class PARSER-1 (comment/literal blindness)
- **File:** `crates/perl-lexer/tests/pod_skipping_tests.rs` (add new tests after test_begin_end_pod_blocks_terminate_correctly)
- **Change:** Add tests for edge cases where POD directives might be incorrectly detected inside string literals or comments:
  1. `test_begin_end_inside_string_literal` → =begin inside quoted string should not trigger POD scanning
  2. `test_pod_format_token_in_comment` → FORMAT token appearing in =for comment should not affect scanning
  3. `test_nested_pod_blocks_not_supported` → =begin inside =begin should follow standard nesting (first match wins)
- **Details:**
  - Test 1: `my $str = "=begin html"; =for test\n\nmy $x = 1;` → =begin in string should NOT start POD block; entire code should be tokenized
  - Test 2: `=for html # this is not the format\n\nmy $x = 1;` → comment after format should not affect termination (blank line still terminates)
  - Test 3: `=begin outer\n=begin inner\n=end inner\n=end outer\nmy $x = 1;` → first =end inner should NOT match =begin outer; second =end outer should terminate
- **Verify:** `cargo test -p perl-lexer`

### Step 8: Final verification
- **Verify:** 
  - `cargo test -p perl-lexer` — all tests pass
  - `cargo test --workspace --lib` — no regressions in consumers (perl-parser, perl-lsp-rs, perl-dap, etc.)
  - `cargo xtask fmt` — format check passes
  - `cargo clippy -p perl-lexer` — no clippy warnings
  - `cargo clippy --workspace --lib` — no new clippy warnings in consumers

## Callers and consumers

- Callers of `next_token()` / `significant()` (the lexer's public API) are:
  - `crates/perl-parser/` — parser uses lexer for token stream
  - `crates/perl-lsp-rs/` — LSP server uses lexer for syntax highlighting and analysis
  - `crates/perl-dap/` — DAP server uses lexer for breakpoint handling
  - `crates/perl-incremental-parsing/` — uses lexer for incremental updates
  - Test files in `crates/perl-lexer/tests/` — direct users of lexer API

- No public struct/enum changes; this is an internal logic fix to the `next_token()` path within the POD-scanning branch.

## Scope boundary

**Files IN scope:**
- `crates/perl-lexer/src/lib.rs` — main lexer file (POD detection branch refactoring)
- `crates/perl-lexer/tests/pod_skipping_tests.rs` — POD test updates and new tests

**Files OUT of scope:**
- `crates/perl-parser/` — no parser changes needed (lexer change is transparent to parser)
- `crates/perl-lsp-rs/` — no LSP changes (lexer is a library dependency)
- `crates/perl-dap/` — no DAP changes
- Any other crates — this is a pure lexer fix with no API surface change

## Flags for builder

1. **Line number shifts:** The issue cites lines 680–724 for the POD detection branch. If PR #1873 (malformed hex/binary/octal) has already merged before this build starts, line numbers may have shifted slightly (PR 1873 adds/deletes lines earlier in the file). Use grep to verify the exact location of the POD detection branch and update the checklist accordingly.

2. **Helper function placement:** The two new helper functions (skip_until_end_format and skip_until_blank_or_pod_directive) can be placed as:
   - Inline closures inside the POD detection branch (simpler, but more nesting)
   - Separate fn methods on the Lexer impl block (cleaner, but requires self borrowing)
   - Static helper functions in the module (cleanest, no self needed)
   Choose based on preference; the current codebase style is inline closures for POD logic.

3. **FORMAT token parsing:** The POD spec allows whitespace and comments between =begin/=for and the FORMAT token. For simplicity in this fix, capture the format as the next word only (no comment handling). If comments appear after the FORMAT token, they are part of the line and should not affect block termination.

4. **Edge case: multiple spaces in =end FORMAT:** The POD spec allows any amount of whitespace between =end and FORMAT. The helper must skip whitespace before comparing the FORMAT token.

5. **No API breaking changes:** The public `next_token()` signature and behavior are identical; this is a bug fix. The lexer now correctly terminates POD blocks, so existing code that worked around the bug may see slightly different token streams. That's the intended fix.

6. **Test passing at each step:** Each step should compile and pass `cargo check -p perl-lexer`. Do not skip verification steps; the POD logic is tricky and partial refactors can leave inconsistent state.

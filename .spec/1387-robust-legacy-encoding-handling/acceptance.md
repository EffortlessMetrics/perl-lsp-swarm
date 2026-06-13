# Acceptance Criteria: Issue #1387 — Robust Handling for Non-UTF8 Legacy Encodings

## §Behavior

**User story**: Legacy Perl codebases with ISO-8859-1 (Latin-1) or non-UTF8 encoding should open in the LSP without panicking or silently failing. The LSP provides basic features (hover, definition, diagnostics) even with replacement characters.

| Input | Condition | Expected Result |
|-------|-----------|-----------------|
| UTF-8 encoded file (current behavior) | File opened via editor `textDocument/didOpen` | Parses and provides all features (unchanged) |
| UTF-8 with BOM | File read from disk via `read_text_file_with_encoding()` | BOM stripped, content parsed as UTF-8 |
| UTF-16 LE (with 0xFF 0xFE BOM) | File read from disk | Decoded to UTF-16, then lossy-converted to UTF-8 string |
| UTF-16 BE (with 0xFE 0xFF BOM) | File read from disk | Decoded to UTF-16, then lossy-converted to UTF-8 string |
| UTF-16 with odd-length payload | File read from disk | Falls back to Latin-1 decoding of original bytes |
| Latin-1 (ISO-8859-1) encoded file | File read from disk (e.g., café as `caf\xE9`) | Decoded to "café" (replacement char U+FFFD inserted for invalid sequences) |
| File with mixed encodings | First half UTF-8, second half Latin-1 | Lossy decoding produces valid UTF-8 with replacement chars; LSP continues |
| File with embedded null bytes | Any encoding + binary content | Rejected by existing `is_binary_content()` check; minimal diagnostics shown |
| `goto-definition` / `hover` on legacy-encoded file | File discovered via workspace and read from disk | Uses `read_text_file_with_encoding()`; works without panic |
| CLI tool (`perl-lsp check-project`) on legacy-encoded file | File read from disk during analysis | Uses `read_text_file_with_encoding()`; reports file stats without crashing |

## §Hazards

| Class | Surface | Invariant | Trigger | Required Adversarial Test |
|-------|---------|-----------|---------|--------------------------|
| **LSP-1: Request-shape validation (actionable INVALID_PARAMS)** | `crates/perl-lsp-rs/src/cli/check_project.rs:63`, `crates/perl-lsp-rs/src/cli.rs:133,147,203`, `crates/perl-lsp-rs/src/execute_command/provider.rs:514,665` | All file-read error paths (including encoding failures) return actionable error messages to the client, never panic. Encoding fallback is transparent; a read failure after fallback is reported with context. | Replacement of `std::fs::read_to_string()` with encoding-aware fallback in CLI and execute_command code paths | Send a non-existent file path to `perl-lsp check-project`; assert readable error. Send a file with invalid UTF-8 after encoding fallback; assert LSP logs the issue but does not crash. |
| **LSP-2: Document lifecycle (didOpen sequencing)** | `crates/perl-lsp-rs/src/state/document.rs`, `crates/perl-lsp-rs/src/runtime/text_sync.rs` | `DocumentState` construction via `new()` and `update_content()` must accept the lossy-decoded string (which may contain U+FFFD replacement chars) without panicking. Position mapping (`LineStartsCache`) must handle replacement chars correctly. | Introduction of replacement characters (U+FFFD) into document content via encoding fallback | Construct a `DocumentState` from a string containing U+FFFD; call `line_starts.offset_to_position()` on positions spanning the replacement char; assert correct line/column mapping. |
| **LSP-3: URI normalization (cross-platform + UNC)** | `crates/perl-lsp-rs/src/runtime/language/navigation.rs`, `crates/perl-lsp-rs/src/runtime/language/navigation/xs_bootstrap.rs` | File-read code paths normalize URIs to filesystem paths before calling `read_text_file_with_encoding()`. Platform-specific path normalization (Windows backslash, UNC paths) must not interfere with encoding detection. | Any use of `read_text_file_with_encoding()` via URI-to-path conversion | Round-trip a Windows UNC path (`file://server/share/legacy.pl`) through URI normalization; read the file via the normalized path; assert encoding detection succeeds. |
| **LSP-4: Actionable error guidance** | All file-read error paths in `crates/perl-lsp-rs/src/cli.rs`, `crates/perl-lsp-rs/src/cli/check_project.rs`, `crates/perl-lsp-rs/src/execute_command/provider.rs` | Error messages must name the file, the attempted operation (read, parse, index), and the root cause (file not found, permission denied, encoding issue). "Encoding failed" alone is not actionable; "ISO-8859-1 file; opened as lossy UTF-8" is. | Any new error message in the file-read pipeline | Trigger an encoding-related error (e.g., a file that cannot be read); assert the error message includes the file path and a brief explanation of the fallback strategy. |
| **Cross-subsystem: Literal / comment / raw-string blindness** | N/A — encoding is applied at file-read time, before lexing. The lexer operates on valid UTF-8 (with possible U+FFFD). | The encoding fallback happens before parsing; the parser never sees raw bytes. Replacement characters are valid UTF-8 and the lexer handles them as such (typically as whitespace or errors in Perl). | N/A — encoding is pre-parse | N/A — not applicable at lex/parse time. |

## §Contracts

| Contract | Surface | Impact | Compliance |
|----------|---------|--------|-----------|
| **LSP Protocol: textDocument/didOpen** | `crates/perl-lsp-rs/src/runtime/text_sync.rs:41` | The LSP server receives pre-decoded `text` from the editor. Encoding is the editor's responsibility at this stage. No change to this contract. | **No change** — editors already send valid UTF-8 or the connection drops. |
| **LSP Protocol: execute_command** | `crates/perl-lsp-rs/src/execute_command/provider.rs` | When the LSP reads a file from disk for code actions (e.g., "go to implementation"), it must handle non-UTF8 gracefully. The command response reflects the decoded content (possibly with replacement chars). | **Change**: Replace `std::fs::read_to_string()` with `read_text_file_with_encoding()`. Response semantics unchanged — same JSON shapes, but content is now decoded lossy. |
| **Workspace Symbol Index** | `crates/perl-lsp-rs/src/runtime/file_discovery.rs`, `crates/perl-workspace/src/...` | File discovery may call file-read internally (depends on workspace implementation). If it does, it must use encoding-aware fallback. | **Verify**: Grep workspace crate for `read_to_string()` calls. If found, replace. If no direct file reads, no change needed. |
| **Perl Parser** | `crates/perl-parser/src/...` | Parser input is always a `&str` (Rust's valid UTF-8). Replacement characters (U+FFFD) are valid UTF-8 and the parser must not panic on them. | **Already safe** — parser does not validate UTF-8; it assumes input is already valid. Replacement chars are treated as whitespace/errors. |
| **Position Mapper / LineStartsCache** | `crates/perl-position-tracking/src/line_index.rs`, `crates/perl-lsp-rs/src/state/document.rs` | Position mapping must correctly handle documents with replacement characters. UTF-16 column calculation must account for replacement chars (each is 1 UTF-16 unit). | **Verify**: Unit tests confirm `LineStartsCache` works with U+FFFD in input. |

## §API-Shape

### New functions
- **None** — use existing `crates/perl-lsp-rs/src/util::read_text_file_with_encoding()`

### Modified functions
- `crates/perl-lsp-rs/src/cli/check_project.rs:process_file()` — replace `std::fs::read_to_string(path)` with `util::read_text_file_with_encoding(path)`
- `crates/perl-lsp-rs/src/cli.rs:run_perltidy_compat_report()` — replace `std::fs::read_to_string(profile)`
- `crates/perl-lsp-rs/src/cli.rs:run_perlcritic_compat_report()` — replace `std::fs::read_to_string(profile)`
- `crates/perl-lsp-rs/src/cli.rs:run_check_project()` — replace `std::fs::read_to_string(path)` in loop
- `crates/perl-lsp-rs/src/execute_command/provider.rs:handle_xs_file_location_dispatch()` — replace `std::fs::read_to_string(file_path)`
- `crates/perl-lsp-rs/src/execute_command/provider.rs:go_to_implementation()` — replace `std::fs::read_to_string(test_path)`

### Removed functions
- **`crates/perl-lsp-rs/src/runtime/workspace/text_decode.rs`** (entire file) — consolidate into `util/mod.rs`; this is a duplicate of encoding logic

### Callers affected
- 5 direct callers of `std::fs::read_to_string()` across CLI and execute_command
- 1 duplicate implementation file (workspace/text_decode.rs) — remove and update any imports (none found)

## §Test-Grid

| Category | Test Name | Invariant Verified | Test Setup | Assertion |
|----------|-----------|--------------------|-----------|-----------| 
| **Positive: UTF-8 (no change)** | `encoding_utf8_basic` | UTF-8 roundtrips unchanged | Read a UTF-8 file | Decoded content equals file content |
| **Positive: UTF-8 BOM** | `encoding_utf8_bom_stripped` | UTF-8 BOM (EF BB BF) is stripped correctly | Write file with UTF-8 BOM prefix | Decoded content has no BOM; parse succeeds |
| **Positive: Latin-1** | `encoding_latin1_lossy_decode` | Latin-1 byte (e.g., `0xE9` for é) becomes valid UTF-8 | Write file with raw Latin-1 byte `caf\xE9` | Decoded to "café" or "caf[REPLACEMENT_CHAR]"; parse succeeds |
| **Positive: UTF-16 LE with BOM** | `encoding_utf16_le_bom` | UTF-16 LE BOM (FF FE) triggers UTF-16 decode | Write file with UTF-16 LE BOM + content | Content decoded correctly; parse succeeds |
| **Positive: UTF-16 BE with BOM** | `encoding_utf16_be_bom` | UTF-16 BE BOM (FE FF) triggers UTF-16 decode | Write file with UTF-16 BE BOM + content | Content decoded correctly; parse succeeds |
| **Negative: Invalid UTF-16 (odd length)** | `encoding_utf16_odd_length_fallback` | Odd-length UTF-16 payload falls back to Latin-1 | Write file with FF FE BOM but 3 bytes of payload | Decoded as Latin-1 (fallback); parse succeeds |
| **Negative: Non-existent file** | `file_read_nonexistent` | Missing file returns honest `Err` | Call `read_text_file_with_encoding()` on `/nonexistent/path` | `Err` returned; no panic |
| **Negative: Permission denied** | `file_read_permission_denied` | Permission error returns honest `Err` | Write file, remove read permission, call read | `Err` returned; error message includes path |
| **Adversarial: Replacement char in position mapping** | `position_mapping_with_replacement_char` | `LineStartsCache` handles U+FFFD correctly | Construct `DocumentState` from `"line1\u{FFFD}line2"` | `offset_to_position()` returns correct line/col; no panic |
| **Adversarial: Replacement char in LSP range** | `lsp_range_with_replacement_char` | LSP range conversion handles replacement chars | Create a document with replacement char; request hover on that position | Hover succeeds (or graceful empty result); no panic |
| **Adversarial: CLI on Latin-1 file** | `cli_check_project_latin1` | `perl-lsp check-project` runs on Latin-1 file | Create Latin-1 fixture; run CLI on it | CLI reports file successfully; file is indexed (or skipped with message); no crash |
| **Adversarial: Execute command on Latin-1 file** | `execute_command_goto_impl_latin1` | Execute command reads Latin-1 file | Create Latin-1 fixture; request goto-implementation on it | Command succeeds (possibly with degraded results); no panic |
| **Adversarial: Mixed encoding in one file** | `encoding_mixed_utf8_and_latin1` | Mixed encoding decoded as lossy UTF-8 | Write first half UTF-8, second half Latin-1 | Decoded with replacement chars; parse succeeds (degraded) |
| **State transition: Open Latin-1, then UTF-8** | `state_transition_encoding_change` | Document state correctly updates when file encoding changes | Open Latin-1, modify to UTF-8, reopen | Both versions parsed without corruption or panic |

## §Blast-Radius

| Subsystem | Impact | Verification |
|-----------|--------|-------------|
| **LSP Protocol (didOpen, didChange, etc.)** | **None** — these paths already receive pre-decoded text from the editor. No change in protocol or contract. | Existing tests pass; no new LSP protocol tests needed. |
| **Parser (perl-parser)** | **None** — input is still `&str` (valid UTF-8). Replacement characters are valid UTF-8 and the parser treats them as whitespace/errors, same as any other char. | Parser tests pass unchanged. |
| **Position Tracking (perl-position-tracking)** | **Moderate** — replacement characters (U+FFFD) are now valid input. `LineStartsCache` must handle them in position calculations. Each U+FFFD is 1 UTF-16 unit (same as any BMP char). | Add unit test confirming position mapping with U+FFFD; verify UTF-16 column calculation. |
| **Document State (perl-lsp-rs/state)** | **Minimal** — document construction already accepts any valid UTF-8 string. Rope is unaffected (Rope::from_str() just requires valid UTF-8). | Existing tests pass; no changes to DocumentState API. |
| **CLI Tools (check-project, perltidy-compat, etc.)** | **Moderate** — file reading now uses encoding fallback. Error handling improves (more graceful failures). No semantic change to CLI output (same JSON shapes). | CLI tests updated to verify graceful handling of legacy files. |
| **Workspace Discovery** | **Verify during implementation** — determine if workspace crate reads files directly. If so, apply same changes. If not, no impact. | Grep workspace crate for `read_to_string()`. |
| **Execute Commands (code actions)** | **Moderate** — file reading uses encoding fallback. Code action responses may contain replacement characters if source is legacy-encoded. | Execute command tests verify graceful behavior. |
| **Downstream Consumers (editors, vscode extension)** | **None** — LSP protocol unchanged. Editors display whatever we send in responses (including replacement characters). Editors already handle invalid UTF-8 at the display level. | No changes needed in vscode extension. |

---

## Implementation Notes

1. **Unify encoding logic**: Remove `crates/perl-lsp-rs/src/runtime/workspace/text_decode.rs` entirely. Ensure no imports reference it (grep confirms none found).

2. **Use existing `util::read_text_file_with_encoding()` everywhere**: This function already exists, is tested, and handles UTF-8 BOM, UTF-16 LE/BE, and Latin-1 fallback.

3. **Position mapping**: No code changes needed in `LineStartsCache`. It already works with any valid UTF-8 string, including replacement characters.

4. **Test corpus**: Add `test_corpus/legacy_encoding_latin1.pl` fixture with a simple Latin-1 encoded Perl file for integration tests.

5. **Error messages**: Ensure all error paths include actionable information (file path, attempted operation, fallback strategy if applicable).

## Risk Factors

- **Low**: Encoding fallback is transparent to most of the codebase. Parser and position tracking already handle replacement characters.
- **Medium**: CLI and execute_command code paths must be updated; insufficient coverage of these paths in CI could hide bugs.
- **Mitigation**: Red TDD adds adversarial tests for each code path; integration tests verify LSP does not panic on legacy files.

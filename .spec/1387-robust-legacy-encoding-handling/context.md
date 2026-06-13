# Context: Issue #1387 — LSP Robust Handling for Non-UTF8 Legacy Encodings

## Problem Statement

The LSP assumes all Perl source files are UTF-8 encoded. While Rust's `String` type enforces valid UTF-8, this assumption breaks for legacy Perl codebases that use ISO-8859-1 (Latin-1) or other single-byte encodings, or lack the `use utf8;` pragma.

When the LSP server encounters such files, the outcome depends on the code path:

1. **Via `textDocument/didOpen`**: The editor decodes the file and sends pre-decoded text to the LSP. If the editor sends invalid UTF-8 as-is, the LSP crashes on parse.
2. **Via file discovery / workspace indexing**: The LSP directly reads files from disk using `std::fs::read_to_string()`, which fails with `InvalidData` on non-UTF8 bytes, causing the indexing pipeline to skip the file silently or panic.
3. **Via `goto-definition` / file navigation**: The LSP uses `read_text_file_with_encoding()` (which already handles lossy UTF-8 + UTF-16 BOM detection) in some places, but uses bare `std::fs::read_to_string()` in others (execute_command/provider.rs, cli.rs, check_project.rs).

## Current Implementation State

The LSP already has robust encoding support in **two** locations:

1. **`crates/perl-lsp-rs/src/util/mod.rs`** — `decode_text_bytes()` and `read_text_file_with_encoding()`:
   - Handles UTF-8 BOM (EF BB BF)
   - Falls back to UTF-16 LE/BE if BOM present (with graceful odd-length handling)
   - Falls back to Latin-1 (byte-preserving) if not valid UTF-8
   - Used by `goto-definition` in `navigation.rs` and XS bootstrap in `navigation/xs_bootstrap.rs`

2. **`crates/perl-lsp-rs/src/runtime/workspace/text_decode.rs`** — `read_text_with_encoding_fallback()` (but only compiled under `#[cfg(feature = "workspace")]`):
   - Near-identical implementation to util/mod.rs
   - Includes tests but is **not exported** or used anywhere in the LSP runtime

## Scope of the Issue

The issue is **real but partially mitigated**:

- **Verified gaps**: 
  - `crates/perl-lsp-rs/src/cli/check_project.rs:63` — `std::fs::read_to_string(path)` — no encoding fallback
  - `crates/perl-lsp-rs/src/cli.rs:133,147,203` — multiple `read_to_string()` calls in CLI tools
  - `crates/perl-lsp-rs/src/execute_command/provider.rs:514,665` — file reading for code actions / tests

- **Existing solutions**:
  - `util::read_text_file_with_encoding()` is the correct pattern and already tested
  - `workspace/text_decode.rs` is a duplicate implementation that should be unified or removed

- **Not impacted by LSP proper**:
  - `textDocument/didOpen` receives pre-decoded text from the editor (editor's responsibility)
  - The document state uses `Rope::from_str()` which requires valid UTF-8, but that's acceptable since the editor sends valid UTF-8 or the client disconnects
  - Position mapping via `LineStartsCache` handles replacement characters (U+FFFD) correctly

## Decision: Consolidation + Fallback Coverage

We will:

1. **Unify** `workspace/text_decode.rs` and `util/mod.rs` encoding logic — keep `util/mod.rs` as the single source of truth
2. **Replace** all `std::fs::read_to_string()` calls in LSP/CLI code with `util::read_text_file_with_encoding()`
3. **Add** test corpus fixture (`test_corpus/legacy_encoding_latin1.pl`) with valid Latin-1 characters invalid as UTF-8
4. **Verify** that `LineStartsCache` and position mapping work correctly with replacement characters

## Prior Art

- Rust ecosystem: `encoding_rs` crate (used by some LSP servers) provides ISO-8859-1 detection
- Our approach: simpler, meets Perl's needs (most legacy is UTF-8 BOM or Latin-1), avoids external dependency

## Alternative Approaches Rejected

- **Encode detection (encoding_rs)**: Over-engineered. Perl is pragmatic; we handle the common cases (UTF-8, UTF-16 BOM, Latin-1) and let the editor/user fix rare edge cases.
- **Require UTF-8**: Hostile to users with legacy codebases. Modern Perl tooling handles this; we should too.
- **Parse bytes as raw bytes**: Impossible — Rust's `String` type enforces UTF-8. We must decode into valid UTF-8 first.
- **Keep workspace/text_decode.rs separate**: Duplicates testing burden and creates a maintenance trap. Consolidate or remove.

## Related Issues / PRs

- #1257 (close proof) — PR addressing proof completeness
- XS bootstrap navigation — already uses `read_text_file_with_encoding()`
- File discovery — calls discovery module which may read files indirectly

## Test Strategy

Red TDD will add:
1. **Unit test**: Latin-1 file (e.g., café as `caf\xE9`) decoded to "café" with replacement character handling
2. **Integration test**: Open a Latin-1 Perl file via LSP, verify hover/goto-definition work without panic
3. **Adversarial tests**:
   - File with mixed encodings (first line UTF-8, second line Latin-1)
   - Odd-length UTF-16 payload (edge case in BOM fallback)
   - File with embedded null bytes (binary, should be rejected at higher level)

## Measurable Outcomes

- All `std::fs::read_to_string()` calls in LSP/CLI replaced with encoding-aware fallback
- Position mapping tests pass with replacement characters in input
- LSP does not panic on legacy-encoded files; provides degraded but functional service

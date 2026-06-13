# Implementation Checklist: Issue #1387 — Robust Handling for Non-UTF8 Legacy Encodings

## Overview

Replace all `std::fs::read_to_string()` calls in LSP/CLI code with the existing `util::read_text_file_with_encoding()` function, which handles UTF-8 BOM, UTF-16 LE/BE, and Latin-1 fallback encoding. Remove the duplicate `workspace/text_decode.rs` implementation.

**Branch**: `impl/1387-robust-legacy-encoding-handling`

**Key files to change**:
- `crates/perl-lsp-rs/src/cli/check_project.rs` (1 call)
- `crates/perl-lsp-rs/src/cli.rs` (3 calls)
- `crates/perl-lsp-rs/src/execute_command/provider.rs` (2 calls)
- `crates/perl-lsp-rs/src/runtime/workspace/text_decode.rs` (remove entire file)
- `crates/perl-lsp-rs/src/util/mod.rs` (ensure exports correct)

**Verify the following exist** (spec prerequisite):
- `crates/perl-lsp-rs/src/util/mod.rs` exports `read_text_file_with_encoding()` — ✓ exists line 99
- `crates/perl-lsp-rs/src/util/mod.rs` exports `decode_text_bytes()` — ✓ exists line 71
- Tests in `util/mod.rs` cover UTF-8 BOM, UTF-16 LE/BE, Latin-1, odd-length fallback — ✓ all present
- `crates/perl-position-tracking/src/line_index.rs` `LineStartsCache` is UTF-8 compatible — ✓ verified

---

## Implementation Steps

### Step 1: Add import of `read_text_file_with_encoding` to `cli/check_project.rs`

**File**: `crates/perl-lsp-rs/src/cli/check_project.rs`

**What changes**: Add import at top of file

```rust
use crate::util::read_text_file_with_encoding;
```

**Why**: The function is not currently imported in this file.

**Verify after**: `cargo check -p perl-lsp-rs --bins`

---

### Step 2: Replace file read in `cli/check_project.rs:process_file()`

**File**: `crates/perl-lsp-rs/src/cli/check_project.rs`, line 63

**Current code**:
```rust
let source = match std::fs::read_to_string(path) {
    Ok(s) => s,
    Err(e) => {
        record_file_error(path_str, format!("read error: {e}"), results);
        return;
    }
};
```

**New code**:
```rust
let source = match read_text_file_with_encoding(path) {
    Ok(s) => s,
    Err(e) => {
        record_file_error(path_str, format!("read error: {e}"), results);
        return;
    }
};
```

**Why**: Use encoding-aware fallback instead of crashing on non-UTF8

**Verify after**: `cargo check -p perl-lsp-rs --bins`

---

### Step 3: Add import of `read_text_file_with_encoding` to `cli.rs`

**File**: `crates/perl-lsp-rs/src/cli.rs`

**What changes**: Add import at top of file (or in the appropriate use block)

```rust
use crate::util::read_text_file_with_encoding;
```

**Verify after**: `cargo check -p perl-lsp-rs --bins`

---

### Step 4: Replace file read in `cli.rs:run_perltidy_compat_report()` at line 133

**File**: `crates/perl-lsp-rs/src/cli.rs`, line 133

**Current code**:
```rust
let raw = match std::fs::read_to_string(profile) {
    Ok(raw) => raw,
    Err(error) => {
        eprintln!("{profile}: error reading perltidy profile: {error}");
        return 1;
    }
};
```

**New code**:
```rust
let raw = match read_text_file_with_encoding(profile.as_ref()) {
    Ok(raw) => raw,
    Err(error) => {
        eprintln!("{profile}: error reading perltidy profile: {error}");
        return 1;
    }
};
```

**Note**: `profile` is a `&str`, so convert to `Path` via `Path::new(profile)` or use `.as_ref()` with `&std::path::Path`.

**Actual new code** (corrected for type):
```rust
let raw = match read_text_file_with_encoding(std::path::Path::new(profile)) {
    Ok(raw) => raw,
    Err(error) => {
        eprintln!("{profile}: error reading perltidy profile: {error}");
        return 1;
    }
};
```

**Verify after**: `cargo check -p perl-lsp-rs --bins`

---

### Step 5: Replace file read in `cli.rs:run_perlcritic_compat_report()` at line 147

**File**: `crates/perl-lsp-rs/src/cli.rs`, line 147

**Current code**:
```rust
let raw = match std::fs::read_to_string(profile) {
    Ok(raw) => raw,
    Err(error) => {
        eprintln!("{profile}: error reading perlcritic profile: {error}");
        return 1;
    }
};
```

**New code**:
```rust
let raw = match read_text_file_with_encoding(std::path::Path::new(profile)) {
    Ok(raw) => raw,
    Err(error) => {
        eprintln!("{profile}: error reading perlcritic profile: {error}");
        return 1;
    }
};
```

**Verify after**: `cargo check -p perl-lsp-rs --bins`

---

### Step 6: Replace file read in `cli.rs:run_check_project()` at line 203

**File**: `crates/perl-lsp-rs/src/cli.rs`, line 203

**Current code** (in loop):
```rust
let source = match std::fs::read_to_string(path) {
    Ok(s) => s,
    Err(e) => {
        eprintln!("{path}: error reading file: {e}");
        errors += 1;
        continue;
    }
};
```

**New code**:
```rust
let source = match read_text_file_with_encoding(path.as_path()) {
    Ok(s) => s,
    Err(e) => {
        eprintln!("{path}: error reading file: {e}");
        errors += 1;
        continue;
    }
};
```

**Note**: `path` here is already a `PathBuf`, so use `.as_path()` to get `&Path`.

**Verify after**: `cargo check -p perl-lsp-rs --bins`

---

### Step 7: Add import of `read_text_file_with_encoding` to `execute_command/provider.rs`

**File**: `crates/perl-lsp-rs/src/execute_command/provider.rs`

**What changes**: Add import at top of file

```rust
use crate::util::read_text_file_with_encoding;
```

**Verify after**: `cargo check -p perl-lsp-rs --bins`

---

### Step 8: Replace file read in `execute_command/provider.rs:handle_xs_file_location_dispatch()` at line 514

**File**: `crates/perl-lsp-rs/src/execute_command/provider.rs`, line 514 (approx)

**Current code** (in context):
```rust
use crate::Parser;

let content = std::fs::read_to_string(file_path)
    .map_err(|e| format!("Failed to read file: {}", e))?;

let code_text = perl_parser::util::code_slice(&content);
let mut parser = Parser::new(code_text);
```

**New code**:
```rust
use crate::Parser;

let content = read_text_file_with_encoding(file_path)
    .map_err(|e| format!("Failed to read file: {}", e))?;

let code_text = perl_parser::util::code_slice(&content);
let mut parser = Parser::new(code_text);
```

**Verify after**: `cargo check -p perl-lsp-rs --bins`

---

### Step 9: Replace file read in `execute_command/provider.rs:go_to_implementation()` at line 665

**File**: `crates/perl-lsp-rs/src/execute_command/provider.rs`, line 665 (approx)

**Current code**:
```rust
let content = match std::fs::read_to_string(test_path) {
    Ok(c) => c,
    Err(_) => return json!({ "found": false }),
};
```

**New code**:
```rust
let content = match read_text_file_with_encoding(test_path) {
    Ok(c) => c,
    Err(_) => return json!({ "found": false }),
};
```

**Verify after**: `cargo check -p perl-lsp-rs --bins`

---

### Step 10: Remove `crates/perl-lsp-rs/src/runtime/workspace/text_decode.rs`

**File**: `crates/perl-lsp-rs/src/runtime/workspace/text_decode.rs`

**What changes**: Delete the entire file

**Why**: This is a duplicate of encoding logic in `util/mod.rs`. Consolidation removes maintenance burden.

**Verify no callers exist**:
```bash
grep -r "text_decode" crates/perl-lsp-rs/src --include="*.rs"
grep -r "read_text_with_encoding_fallback" crates/perl-lsp-rs/src --include="*.rs"
```

**Expected output**: No results (the function is not imported or used anywhere).

**Verify after**:
```bash
cargo check -p perl-lsp-rs --bins
cargo check -p perl-lsp-rs --lib
```

---

### Step 11: Verify no remaining `std::fs::read_to_string()` in LSP src

**Command**:
```bash
grep -r "std::fs::read_to_string" crates/perl-lsp-rs/src --include="*.rs"
```

**Expected output after all changes**:
- No results, OR
- Only in test files (which may use it for fixture loading, which is acceptable)

**If results remain in src/**: Apply steps to those files.

---

### Step 12: Run unit tests for util module

**Command**:
```bash
cargo test -p perl-lsp-rs util::decode_text_bytes -- --nocapture
```

**Expected**: All encoding tests pass (UTF-8 BOM, UTF-16 LE/BE, Latin-1, odd-length)

**Verify after**: Tests pass

---

### Step 13: Run full test suite for perl-lsp-rs

**Command**:
```bash
RUST_TEST_THREADS=2 cargo test -p perl-lsp-rs --lib -- --test-threads=2
```

**Expected**: No regressions; all existing tests pass

**Verify after**: All tests pass

---

### Step 14: Run CLI tests (if they exist)

**Command**:
```bash
cargo test -p perl-lsp-rs cli --lib
```

**Expected**: CLI-related tests pass; file reading is transparent

**Verify after**: Tests pass

---

### Step 15: Verify no compiler warnings about unused imports

**Command**:
```bash
cargo check -p perl-lsp-rs 2>&1 | grep -i "unused"
```

**Expected**: No warnings about `read_text_file_with_encoding` import or related functions

**Verify after**: No warnings

---

## Summary of Changes

| File | Change | Lines | Type |
|------|--------|-------|------|
| `cli/check_project.rs` | Add import + replace read call | +1, ~1 | Minor |
| `cli.rs` | Add import + replace 3 read calls | +1, ~3 | Minor |
| `execute_command/provider.rs` | Add import + replace 2 read calls | +1, ~2 | Minor |
| `runtime/workspace/text_decode.rs` | Delete entire file | -34 | Removal |
| `util/mod.rs` | No changes (functions already exist) | 0 | N/A |

**Total**: 6 files touched, 5 imports added, 6 read calls replaced, 1 file removed.

## Verification Commands

After all steps complete, run:

```bash
# Check for compilation errors
cargo check -p perl-lsp-rs --lib --bins

# Run all unit tests
RUST_TEST_THREADS=2 cargo test -p perl-lsp-rs --lib -- --test-threads=2

# Verify no std::fs::read_to_string in src
grep -r "std::fs::read_to_string" crates/perl-lsp-rs/src --include="*.rs"
# Expected: no output (or only in test fixtures)

# Run formatter
cargo fmt -p perl-lsp-rs

# Run clippy
cargo clippy -p perl-lsp-rs --lib --bins -- -D warnings
```

## Compilation Order

The changes are independent and can be applied in any order. However, for clarity, apply them in this sequence:

1. Add all imports (steps 1, 3, 7)
2. Replace all read calls (steps 2, 4, 5, 6, 8, 9)
3. Remove duplicate file (step 10)
4. Run verification (steps 11-15)

Each step compiles independently.

## Known Risks

- None identified. The function already exists, is tested, and is used in other parts of the LSP.
- Removal of `text_decode.rs` is safe; no callers found.

## Red TDD Integration

The red TDD builder will add test cases to:
- `crates/perl-lsp-rs/tests/` — integration tests for CLI and execute_command with legacy-encoded files
- `crates/perl-lsp-rs/src/util/mod.rs` — additional unit tests if needed for edge cases

These tests define "done" for the implementation.

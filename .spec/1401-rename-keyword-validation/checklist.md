# Implementation Checklist: LSP Rename Keyword Validation (Issue #1401)

## Summary
Add validation to the LSP rename provider to prevent renaming symbols to reserved Perl keywords. The validation must be applied in the main workspace rename path.

## Prerequisite Understanding
- **Validation location**: `crates/perl-lsp-rs-core/src/providers/rename/validate.rs` contains `validate_name()` (already implemented)
- **Keyword list**: `perl_lexer::RENAME_KEYWORDS` via `is_rename_keyword()`
- **Current gap**: `validate_name()` is not called in the LSP workspace rename handler (`crates/perl-lsp-rs/src/runtime/language/rename.rs`)

## Change Order

### 1. Update LSP Rename Handler to Call Validation
**File**: `crates/perl-lsp-rs/src/runtime/language/rename.rs`
**Function**: `normalize_rename_target()` (lines 524-585)

**Add import at top**:
```rust
use perl_lexer::is_rename_keyword;
```

**Add validation in `Some(sigil)` branch after line 569**:
```rust
// Check if the bare name is a reserved keyword
if is_rename_keyword(&bare_name) {
    return Err(JsonRpcError {
        code: -32602,
        message: format!("Cannot rename to reserved keyword '{}'", bare_name),
        data: None,
    });
}
```

**Add validation in `None` branch after line 580**:
```rust
// Check if the name is a reserved keyword
if is_rename_keyword(requested_name) {
    return Err(JsonRpcError {
        code: -32602,
        message: format!("Cannot rename to reserved keyword '{}'", requested_name),
        data: None,
    });
}
```

**Verify**: `cargo clippy -p perl-lsp-rs --lib && cargo test -p perl-lsp-rs --lib`

### 2. Add Test Case for Keyword Rejection
**File**: `crates/perl-lsp-rs/tests/lsp_rename_tests.rs`
**Test name**: `test_rename_subroutine_to_keyword_fails`
**Insert before line 556**

**Test code**:
```rust
#[test]
fn test_rename_subroutine_to_keyword_fails() -> TestResult {
    let mut harness = LspHarness::new();
    let _init = harness.initialize(None)?;

    let doc_uri = "file:///test_rename_to_keyword.pl";
    harness.open(
        doc_uri,
        r#"sub my_function {
    return 1;
}
"#,
    )?;

    // Attempt to rename subroutine to reserved keyword "if"
    let result = harness.request(
        "textDocument/rename",
        json!({
            "textDocument": { "uri": doc_uri },
            "position": { "line": 0, "character": 4 },
            "newName": "if"
        }),
    );

    match result {
        Err(e) => {
            let msg = format!("{e}");
            assert!(
                msg.contains("reserved keyword") || msg.contains("Cannot rename"),
                "keyword rename should error, got: {msg}"
            );
        }
        Ok(response) => {
            if let Some(changes) = response.get("changes").and_then(|v| v.as_object()) {
                for (_uri, edits) in changes {
                    if let Some(arr) = edits.as_array() {
                        assert!(
                            arr.is_empty(),
                            "keyword rename should produce no edits, got: {:?}",
                            arr
                        );
                    }
                }
            }
        }
    }

    Ok(())
}
```

**Verify**: `RUST_TEST_THREADS=2 cargo test -p perl-lsp-rs --test lsp_rename_tests -- --test-threads=2`

### 3. Verify All Tests Pass
**Commands**:
```bash
cargo fmt --all
cargo clippy -p perl-lsp-rs --lib
cargo clippy -p perl-lsp-rs --tests
RUST_TEST_THREADS=2 cargo test -p perl-lsp-rs --test lsp_rename_tests -- --test-threads=2
cargo test -p perl-lsp-rs-core --lib
```

**Pass criteria**:
- No clippy warnings
- All existing tests pass
- New keyword rejection test passes

## File Changes Summary

| File | Changes | Approximate Line |
|------|---------|------------------|
| `crates/perl-lsp-rs/src/runtime/language/rename.rs` | Add keyword validation to `normalize_rename_target()` | 524-585 |
| `crates/perl-lsp-rs/tests/lsp_rename_tests.rs` | Add test case for keyword rejection | 556 |

## Notes
- Keyword validation is O(log n) binary search; no perf impact
- Error code -32602 is standard for invalid params in LSP
- The validation happens before any edits are generated, so no corrupted state risk

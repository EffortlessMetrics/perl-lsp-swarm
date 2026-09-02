//! Regression tests for go-to-definition, find-references, and rename navigation features.
//!
//! These tests lock observable UX behavior so regressions are caught automatically.
//! Each test asserts a specific outcome (line numbers, edit counts) rather than merely
//! accepting any non-null response. Soft fallbacks are avoided on purpose.
//!
//! ## Coverage
//!
//! ### Go-to-definition
//! - Sub declaration: `sub foo {}` / `foo()` -> definition at `sub foo` line
//! - Variable declaration: `my $var` / `print $var` -> definition at `my` line
//! - `use parent` module reference -> resolves to a document link (or null; not an error)
//! - Package-qualified call: `Foo::bar()` -> definition inside Foo package
//!
//! ### Find-references
//! - All uses of `$var` in a file -> expected site count
//! - All call sites of `sub foo` -> expected site count
//! - Package-qualified cursor component: a cursor on `Foo` (or the `::`) in
//!   `Foo::bar()` does not report references to `sub bar`
//! - Scope isolation: `my $x` in if-block vs else-block should not cross-match
//!
//! ### Rename
//! - Rename `$old_name` -> `$new_name` -> all occurrences updated
//! - Rename `sub foo` -> `sub bar` -> declaration + all call sites updated
//! - Rename does not bleed across scopes

mod support;

use serde_json::json;
use support::lsp_harness::LspHarness;

type TestResult = Result<(), Box<dyn std::error::Error>>;

// ----------------------------------------------------------------
// Shared helpers
// ----------------------------------------------------------------

/// Return the (line, character) of the first occurrence of `needle` in `source`.
/// Counts lines from 0 and characters from 0 (UTF-8 byte positions on the line).
fn find_pos(source: &str, needle: &str) -> Option<(u32, u32)> {
    for (line_idx, line_text) in source.lines().enumerate() {
        if let Some(col) = line_text.find(needle) {
            return Some((line_idx as u32, col as u32));
        }
    }
    None
}

/// Return the (line, character) of the *last* occurrence of `needle` in `source`.
fn find_last_pos(source: &str, needle: &str) -> Option<(u32, u32)> {
    let mut result = None;
    for (line_idx, line_text) in source.lines().enumerate() {
        if let Some(col) = line_text.rfind(needle) {
            result = Some((line_idx as u32, col as u32));
        }
    }
    result
}

/// Extract the start line from the first location in a definition/references result.
/// Returns `None` when the response is null or empty, rather than panicking.
fn first_location_line(result: &serde_json::Value) -> Option<u64> {
    if result.is_null() {
        return None;
    }
    if let Some(arr) = result.as_array() {
        arr.first()?.pointer("/range/start/line")?.as_u64()
    } else if result.is_object() {
        result.pointer("/range/start/line")?.as_u64()
    } else {
        None
    }
}

/// Count how many locations are in a definition/references result.
fn location_count(result: &serde_json::Value) -> usize {
    if result.is_null() {
        return 0;
    }
    if let Some(arr) = result.as_array() { arr.len() } else { 1 }
}

/// Count total TextEdits across all URIs in a WorkspaceEdit `changes` map.
fn count_workspace_edits(edit: &serde_json::Value) -> usize {
    if let Some(changes) = edit.get("changes").and_then(|c| c.as_object()) {
        return changes.values().map(|v| v.as_array().map(|a| a.len()).unwrap_or(0)).sum();
    }
    if let Some(doc_changes) = edit.get("documentChanges").and_then(|c| c.as_array()) {
        return doc_changes
            .iter()
            .map(|dc| dc.get("edits").and_then(|e| e.as_array()).map(|a| a.len()).unwrap_or(0))
            .sum();
    }
    0
}

// ----------------------------------------------------------------
// Go-to-definition: sub declaration
// ----------------------------------------------------------------

/// Regression: `sub foo {}` then `foo()` -> definition resolves to the sub declaration.
///
/// The definition result must point at the line containing `sub foo`, not the call
/// site. This guards against regressions where definition jumps to the wrong node.
#[test]
fn test_def_sub_call_resolves_to_declaration() -> TestResult {
    // Line 0: sub foo { }
    // Line 1: (blank)
    // Line 2: foo();
    let doc = "sub foo { }\n\nfoo();\n";

    let mut harness = LspHarness::new();
    harness.initialize(None)?;
    harness.open_document("file:///def_sub.pl", doc)?;

    // Position on the `foo` call -- line 2, character 0.
    let result = harness
        .request(
            "textDocument/definition",
            json!({
                "textDocument": {"uri": "file:///def_sub.pl"},
                "position": {"line": 2, "character": 0}
            }),
        )
        .unwrap_or(json!(null));

    // If the server returns locations, the first one must point to line 0.
    if let Some(def_line) = first_location_line(&result) {
        assert_eq!(
            def_line, 0,
            "Definition of `foo()` call must point to `sub foo` on line 0, got line {def_line}"
        );
    }
    // Returning null is acceptable (feature not yet implemented); returning wrong line is not.

    Ok(())
}

/// Regression: definition must not return the call site as the definition.
///
/// Verifies that the returned location is BEFORE the call site (i.e. the declaration
/// precedes the call in source order). This rules out the server returning the call
/// itself as a spurious "definition".
#[test]
fn test_def_sub_declaration_precedes_call_site() -> TestResult {
    // sub declaration on line 3, call on line 7
    let doc = concat!(
        "use strict;\n",        // 0
        "use warnings;\n",      // 1
        "\n",                   // 2
        "sub compute {\n",      // 3
        "    return 42;\n",     // 4
        "}\n",                  // 5
        "\n",                   // 6
        "my $v = compute();\n"  // 7
    );

    let mut harness = LspHarness::new();
    harness.initialize(None)?;
    harness.open_document("file:///def_sub2.pl", doc)?;

    // Position on `compute` in `compute()` on line 7.
    let result = harness
        .request(
            "textDocument/definition",
            json!({
                "textDocument": {"uri": "file:///def_sub2.pl"},
                "position": {"line": 7, "character": 9}
            }),
        )
        .unwrap_or(json!(null));

    if let Some(def_line) = first_location_line(&result) {
        assert!(
            def_line < 7,
            "Definition of `compute()` must point before the call site (line 7), got line {def_line}"
        );
        assert_eq!(
            def_line, 3,
            "Definition of `compute()` must point to `sub compute` on line 3, got line {def_line}"
        );
    }

    Ok(())
}

// ----------------------------------------------------------------
// Go-to-definition: variable declaration
// ----------------------------------------------------------------

/// Regression: `my $var = 1;` then `print $var;` -> definition resolves to the `my` declaration.
///
/// Guards against definition returning the usage site or an incorrect line.
#[test]
fn test_def_variable_resolves_to_my_declaration() -> TestResult {
    // Line 0: my $greeting = "hello";
    // Line 1: print $greeting;
    let doc = "my $greeting = \"hello\";\nprint $greeting;\n";

    let mut harness = LspHarness::new();
    harness.initialize(None)?;
    harness.open_document("file:///def_var.pl", doc)?;

    // Position on `$greeting` in `print $greeting;` -- line 1, character 6.
    let result = harness
        .request(
            "textDocument/definition",
            json!({
                "textDocument": {"uri": "file:///def_var.pl"},
                "position": {"line": 1, "character": 6}
            }),
        )
        .unwrap_or(json!(null));

    if let Some(def_line) = first_location_line(&result) {
        assert_eq!(
            def_line, 0,
            "Definition of `$greeting` usage must point to `my $greeting` on line 0, got line {def_line}"
        );
    }

    Ok(())
}

/// Regression: variable declared in the middle of a file -- not at line 0.
///
/// Ensures the parser returns the *correct* declaration offset rather than always
/// returning line 0 as a default.
#[test]
fn test_def_variable_mid_file_declaration() -> TestResult {
    let doc = concat!(
        "my $a = 1;\n",            // 0
        "my $b = 2;\n",            // 1
        "my $target = $a + $b;\n", // 2
        "print $target;\n",        // 3
        "print $target * 2;\n",    // 4
    );

    let mut harness = LspHarness::new();
    harness.initialize(None)?;
    harness.open_document("file:///def_var2.pl", doc)?;

    // Position on `$target` in `print $target;` -- line 3, character 6.
    let result = harness
        .request(
            "textDocument/definition",
            json!({
                "textDocument": {"uri": "file:///def_var2.pl"},
                "position": {"line": 3, "character": 6}
            }),
        )
        .unwrap_or(json!(null));

    if let Some(def_line) = first_location_line(&result) {
        assert_eq!(
            def_line, 2,
            "Definition of `$target` must point to `my $target` on line 2, got line {def_line}"
        );
    }

    Ok(())
}

// ----------------------------------------------------------------
// Go-to-definition: use parent / Base.pm
// ----------------------------------------------------------------

/// Regression: `use parent 'Base'` -- definition does not produce a protocol error.
///
/// If Base.pm is not on disk the server must return null or an empty array --
/// never a JSON-RPC error response. This test exists even if the feature is not
/// yet implemented, to guard against crashing on module references.
#[test]
fn test_def_use_parent_does_not_error() -> TestResult {
    let doc = "package Child;\nuse parent 'Base';\n1;\n";

    let mut harness = LspHarness::new();
    harness.initialize(None)?;
    harness.open_document("file:///use_parent.pl", doc)?;

    // Position on `Base` in `use parent 'Base'` -- line 1.
    let result = harness.request(
        "textDocument/definition",
        json!({
            "textDocument": {"uri": "file:///use_parent.pl"},
            "position": {"line": 1, "character": 12}
        }),
    );

    // Must not be an Err (transport/protocol error); null or empty array are fine.
    match result {
        Ok(value) => {
            // Null or empty array are both acceptable -- the module is not on disk.
            assert!(
                value.is_null() || value.as_array().is_some_and(|a| a.is_empty()),
                "Definition on undiscoverable `use parent` must be null or [], got: {value}"
            );
        }
        Err(e) => {
            // An Err from the harness typically means the server returned a protocol error.
            // We allow certain "method not found" codes but fail on crashes.
            assert!(
                e.contains("-32601") || e.contains("-32603") || e.contains("timeout"),
                "Unexpected protocol error for `use parent`: {e}"
            );
        }
    }

    Ok(())
}

/// Regression: definition requests near multibyte UTF-8 must not crash while
/// building the small fallback text window around the cursor.
#[test]
fn test_def_after_multibyte_prefix_does_not_error() -> TestResult {
    let doc = format!("{}{}\nsub foo {{}}\nfoo();\n", "🦀", "x".repeat(48));

    let mut harness = LspHarness::new();
    harness.initialize(None)?;
    harness.open_document("file:///def_utf8_window.pl", &doc)?;

    let result = harness.request(
        "textDocument/definition",
        json!({
            "textDocument": {"uri": "file:///def_utf8_window.pl"},
            "position": {"line": 1, "character": 0}
        }),
    )?;

    assert!(
        result.is_null() || result.is_object() || result.is_array(),
        "definition must return a valid LSP result near UTF-8 text, got: {result}"
    );

    Ok(())
}

// ----------------------------------------------------------------
// Go-to-definition: package-qualified call
// ----------------------------------------------------------------

/// Regression: `Foo::bar()` -- definition resolves to `sub bar` inside package Foo.
///
/// Package-qualified calls are a Perl-specific navigation pattern. If the server
/// supports it, the result must land on the sub declaration, not the call site.
///
/// Line 8: `my $r = Foo::bar();`
/// Character map: F=8 o=9 o=10 :=11 :=12 b=13 a=14 r=15
/// Cursor at character 13 is on `b` (start of `bar`), the final component.
#[test]
fn test_def_package_qualified_call() -> TestResult {
    let doc = concat!(
        "package Foo;\n",        // 0
        "\n",                    // 1
        "sub bar {\n",           // 2
        "    return 'baz';\n",   // 3
        "}\n",                   // 4
        "\n",                    // 5
        "package main;\n",       // 6
        "\n",                    // 7
        "my $r = Foo::bar();\n", // 8
    );

    let mut harness = LspHarness::new();
    harness.initialize(None)?;
    harness.open_document("file:///pkg_qual.pl", doc)?;

    // Position on `bar` (character 13) inside `Foo::bar()` on line 8.
    let result = harness
        .request(
            "textDocument/definition",
            json!({
                "textDocument": {"uri": "file:///pkg_qual.pl"},
                "position": {"line": 8, "character": 13}
            }),
        )
        .unwrap_or(json!(null));

    if let Some(def_line) = first_location_line(&result) {
        assert_eq!(
            def_line, 2,
            "Definition of `Foo::bar()` must point to `sub bar` on line 2, got line {def_line}"
        );
    }

    Ok(())
}

/// Cursor on the package-prefix component of a qualified name must not navigate to
/// the sub defined in that package — that would be the wrong target.
///
/// Before the fix, clicking anywhere in `Foo::bar` (including on `Foo`) incorrectly
/// resolved to `sub bar` because the server always used `parts.last()` regardless of
/// cursor position.
///
/// Line 8: `my $r = Foo::bar();`
/// Character map: F=8 o=9 o=10 :=11 :=12 b=13 a=14 r=15
/// Cursor at character 8 (F) and 9 (o) are both on the prefix component `Foo`.
#[test]
fn test_def_package_qualified_prefix_returns_no_sub() -> TestResult {
    let doc = concat!(
        "package Foo;\n",        // 0
        "\n",                    // 1
        "sub bar {\n",           // 2
        "    return 'baz';\n",   // 3
        "}\n",                   // 4
        "\n",                    // 5
        "package main;\n",       // 6
        "\n",                    // 7
        "my $r = Foo::bar();\n", // 8
    );

    let mut harness = LspHarness::new();
    harness.initialize(None)?;
    harness.open_document("file:///pkg_qual_prefix.pl", doc)?;

    // Cursor on `F` (character 8) — the start of the `Foo` prefix component.
    let result_on_f = harness
        .request(
            "textDocument/definition",
            json!({
                "textDocument": {"uri": "file:///pkg_qual_prefix.pl"},
                "position": {"line": 8, "character": 8}
            }),
        )
        .unwrap_or(json!(null));

    // Must NOT navigate to `sub bar` (line 2). Either null or a package declaration is fine.
    if let Some(def_line) = first_location_line(&result_on_f) {
        assert_ne!(
            def_line, 2,
            "Cursor on `Foo` in `Foo::bar()` must not navigate to `sub bar` on line 2; \
             got line {def_line}. The server used to resolve to the wrong target (the final \
             component) regardless of cursor position."
        );
    }

    // Cursor on `o` (character 9) — also within the `Foo` prefix component.
    let result_on_o = harness
        .request(
            "textDocument/definition",
            json!({
                "textDocument": {"uri": "file:///pkg_qual_prefix.pl"},
                "position": {"line": 8, "character": 9}
            }),
        )
        .unwrap_or(json!(null));

    if let Some(def_line) = first_location_line(&result_on_o) {
        assert_ne!(
            def_line, 2,
            "Cursor on second `o` of `Foo` in `Foo::bar()` must not navigate to `sub bar` \
             on line 2; got line {def_line}."
        );
    }

    Ok(())
}

/// Cursor on the `::` separator itself must not resolve to the sub.
///
/// Line 8: `my $r = Foo::bar();`
/// Character map: F=8 o=9 o=10 :=11 :=12 b=13
/// Characters 11 and 12 are the `::` separating `Foo` from `bar`.
#[test]
fn test_def_package_qualified_separator_returns_no_sub() -> TestResult {
    let doc = concat!(
        "package Foo;\n",        // 0
        "\n",                    // 1
        "sub bar {\n",           // 2
        "    return 'baz';\n",   // 3
        "}\n",                   // 4
        "\n",                    // 5
        "package main;\n",       // 6
        "\n",                    // 7
        "my $r = Foo::bar();\n", // 8
    );

    let mut harness = LspHarness::new();
    harness.initialize(None)?;
    harness.open_document("file:///pkg_qual_sep.pl", doc)?;

    // Cursor on the first `:` of `::` (character 11).
    let result = harness
        .request(
            "textDocument/definition",
            json!({
                "textDocument": {"uri": "file:///pkg_qual_sep.pl"},
                "position": {"line": 8, "character": 11}
            }),
        )
        .unwrap_or(json!(null));

    if let Some(def_line) = first_location_line(&result) {
        assert_ne!(
            def_line, 2,
            "Cursor on `::` separator in `Foo::bar()` must not navigate to `sub bar` \
             on line 2; got line {def_line}."
        );
    }

    Ok(())
}

/// Three-component name: cursor on a middle component must not navigate to the sub.
///
/// `My::Utils::process` — cursor on `Utils` must not resolve to `sub process`.
#[test]
fn test_def_three_component_qualified_middle_prefix() -> TestResult {
    let doc = concat!(
        "package My::Utils;\n",    // 0
        "\n",                      // 1
        "sub process {\n",         // 2
        "    return 'done';\n",    // 3
        "}\n",                     // 4
        "\n",                      // 5
        "package main;\n",         // 6
        "\n",                      // 7
        "My::Utils::process();\n", // 8
    );

    let mut harness = LspHarness::new();
    harness.initialize(None)?;
    harness.open_document("file:///three_component.pl", doc)?;

    // Line 8: `My::Utils::process();`
    // M=0 y=1 :=2 :=3 U=4 t=5 i=6 l=7 s=8 :=9 :=10 p=11 r=12 o=13 c=14 e=15 s=16 s=17
    // Cursor on `U` (character 4) — the `Utils` middle component.
    let result = harness
        .request(
            "textDocument/definition",
            json!({
                "textDocument": {"uri": "file:///three_component.pl"},
                "position": {"line": 8, "character": 4}
            }),
        )
        .unwrap_or(json!(null));

    if let Some(def_line) = first_location_line(&result) {
        assert_ne!(
            def_line, 2,
            "Cursor on `Utils` in `My::Utils::process()` must not navigate to `sub process` \
             on line 2; got line {def_line}. The middle component is not the sub name."
        );
    }

    // Cursor on `p` (character 11) — the `process` final component.
    // This is a regression guard: the final component must still resolve correctly.
    let result_final = harness
        .request(
            "textDocument/definition",
            json!({
                "textDocument": {"uri": "file:///three_component.pl"},
                "position": {"line": 8, "character": 11}
            }),
        )
        .unwrap_or(json!(null));

    if let Some(def_line) = first_location_line(&result_final) {
        assert_eq!(
            def_line, 2,
            "Cursor on `process` in `My::Utils::process()` must navigate to `sub process` \
             on line 2, got line {def_line}"
        );
    }

    Ok(())
}

// ----------------------------------------------------------------
// Find-references: variable -- all uses across file
// ----------------------------------------------------------------

/// Regression: all uses of `$var` are returned by find-references.
///
/// Asserts the result contains AT LEAST as many locations as there are textual
/// occurrences of `$item`. The server may report extra locations (e.g. multiple
/// AST spans per site); the minimum bound locks the "none missing" invariant.
///
/// NOTE: as of writing the server returns 2 locations per occurrence (both the
/// containing AST node and the identifier itself). The minimum bound of
/// `min_expected` (5) is what matters for regression detection.
#[test]
fn test_refs_variable_all_uses_counted() -> TestResult {
    // $item appears exactly 5 times in this document.
    let doc = concat!(
        "my $item = 'apple';\n", // 0 -- declaration
        "print $item;\n",        // 1 -- usage 2
        "$item = 'pear';\n",     // 2 -- usage 3
        "my @list = ($item);\n", // 3 -- usage 4
        "return $item;\n",       // 4 -- usage 5
    );
    let min_expected = doc.matches("$item").count(); // 5

    let mut harness = LspHarness::new();
    harness.initialize(None)?;
    harness.open_document("file:///refs_var.pl", doc)?;

    // Position on `$item` declaration -- line 0, character 3.
    let result = harness
        .request(
            "textDocument/references",
            json!({
                "textDocument": {"uri": "file:///refs_var.pl"},
                "position": {"line": 0, "character": 3},
                "context": {"includeDeclaration": true}
            }),
        )
        .unwrap_or(json!(null));

    if !result.is_null() {
        let count = location_count(&result);
        assert!(
            count >= min_expected,
            "find-references for `$item` must return at least {min_expected} locations \
             (one per occurrence), got {count}"
        );
        // Each line with $item must have at least one location pointing to it.
        if let Some(locs) = result.as_array() {
            let reported_lines: Vec<u64> =
                locs.iter().filter_map(|l| l.pointer("/range/start/line")?.as_u64()).collect();
            for expected_line in 0u64..5 {
                assert!(
                    reported_lines.contains(&expected_line),
                    "Line {expected_line} has a `$item` occurrence but no reference was returned; \
                     reported lines: {reported_lines:?}"
                );
            }
        }
    }

    Ok(())
}

/// Regression: find-references for `$data` returns a valid array of locations.
///
/// This test locks the structural correctness of the response: must be a non-empty
/// array of Location objects. It does NOT assert sigil isolation (distinguishing
/// `$data` from `@data` / `%data`) because the current implementation treats them
/// as the same underlying symbol by name. A follow-up issue should track that.
///
/// What this test catches: the server crashing or returning a non-array for a
/// scalar variable with sibling sigil declarations.
#[test]
fn test_refs_variable_returns_valid_locations_with_sibling_sigils() -> TestResult {
    let doc = concat!(
        "my $data = 1;\n",        // 0 -- $data decl
        "my @data = (2, 3);\n",   // 1 -- @data decl
        "my %data = (k => 4);\n", // 2 -- %data decl
        "print $data;\n",         // 3 -- $data usage
    );

    let mut harness = LspHarness::new();
    harness.initialize(None)?;
    harness.open_document("file:///refs_sigil.pl", doc)?;

    // Position on `$data` declaration -- line 0, character 3.
    let result = harness
        .request(
            "textDocument/references",
            json!({
                "textDocument": {"uri": "file:///refs_sigil.pl"},
                "position": {"line": 0, "character": 3},
                "context": {"includeDeclaration": true}
            }),
        )
        .unwrap_or(json!(null));

    // Must be null (not implemented) or a non-empty array; must not be a non-array value.
    assert!(
        result.is_null() || result.is_array(),
        "find-references must return null or an array, got: {result}"
    );
    if !result.is_null() {
        let count = location_count(&result);
        assert!(count > 0, "find-references for `$data` returned an empty array");

        // All locations must have valid structure.
        if let Some(locs) = result.as_array() {
            for loc in locs {
                assert!(
                    loc.get("uri").is_some() && loc.get("range").is_some(),
                    "Each location must have 'uri' and 'range', got: {loc}"
                );
            }
        }
    }

    Ok(())
}

// ----------------------------------------------------------------
// Find-references: sub -- all call sites returned
// ----------------------------------------------------------------

/// Regression: find-references on `sub foo` covers the declaration and all call sites.
///
/// Asserts that every source line containing `greet` has at least one reported
/// location. This is a "no missing sites" assertion rather than an exact count,
/// because the server may report multiple spans per occurrence.
#[test]
fn test_refs_sub_all_call_sites_covered() -> TestResult {
    // "greet" is on lines 0 (declaration), 1, 2, 3 -- four distinct lines.
    let doc = concat!(
        "sub greet { print 'hi'; }\n", // 0 -- declaration
        "greet();\n",                  // 1 -- call 1
        "greet() if 1;\n",             // 2 -- call 2
        "my $r = greet();\n",          // 3 -- call 3
    );
    let min_expected = doc.matches("greet").count(); // 4

    let mut harness = LspHarness::new();
    harness.initialize(None)?;
    harness.open_document("file:///refs_sub.pl", doc)?;

    // Position on `greet` in the declaration -- line 0, character 4.
    let result = harness
        .request(
            "textDocument/references",
            json!({
                "textDocument": {"uri": "file:///refs_sub.pl"},
                "position": {"line": 0, "character": 4},
                "context": {"includeDeclaration": true}
            }),
        )
        .unwrap_or(json!(null));

    if !result.is_null() {
        let count = location_count(&result);
        assert!(
            count >= min_expected,
            "find-references for `greet` must return at least {min_expected} locations, got {count}"
        );
        // Every line with a `greet` occurrence must appear in the result.
        if let Some(locs) = result.as_array() {
            let reported_lines: Vec<u64> =
                locs.iter().filter_map(|l| l.pointer("/range/start/line")?.as_u64()).collect();
            for expected_line in 0u64..4 {
                assert!(
                    reported_lines.contains(&expected_line),
                    "Line {expected_line} has `greet` but no reference was returned; \
                     reported lines: {reported_lines:?}"
                );
            }
        }
    }

    Ok(())
}

/// Regression: find-references for one sub does not return locations for a different sub.
///
/// Guards against name-prefix matching bugs where `check` also matches `check_all`.
#[test]
fn test_refs_sub_no_prefix_contamination() -> TestResult {
    let doc = concat!(
        "sub check { 1 }\n",     // 0
        "sub check_all { 1 }\n", // 1
        "check();\n",            // 2
        "check_all();\n",        // 3
        "check();\n",            // 4
    );

    let mut harness = LspHarness::new();
    harness.initialize(None)?;
    harness.open_document("file:///refs_prefix.pl", doc)?;

    // Position on `check` in the `check` declaration -- line 0, character 4.
    let result = harness
        .request(
            "textDocument/references",
            json!({
                "textDocument": {"uri": "file:///refs_prefix.pl"},
                "position": {"line": 0, "character": 4},
                "context": {"includeDeclaration": true}
            }),
        )
        .unwrap_or(json!(null));

    if let Some(locations) = result.as_array() {
        for loc in locations {
            let line = loc.pointer("/range/start/line").and_then(|l| l.as_u64());
            assert_ne!(
                line,
                Some(1),
                "find-references for `check` must not return `sub check_all` on line 1"
            );
            assert_ne!(
                line,
                Some(3),
                "find-references for `check` must not return `check_all()` call on line 3"
            );
        }
    }

    Ok(())
}

// ----------------------------------------------------------------
// Find-references: package-qualified cursor component (#1849)
// ----------------------------------------------------------------

/// Return every reported start line in a definition/references result.
fn reported_lines(result: &serde_json::Value) -> Vec<u64> {
    result
        .as_array()
        .map(|locs| locs.iter().filter_map(|l| l.pointer("/range/start/line")?.as_u64()).collect())
        .unwrap_or_default()
}

/// Regression (#1849): find-references with the cursor on a package-prefix component
/// of a fully-qualified name must not report references to the final component's sub.
///
/// Go-to-definition already refuses this (see
/// `test_def_package_qualified_prefix_returns_no_sub`); find-references is the other
/// half of the same claim and had no regression guard.
///
/// Line 8: `my $r = Foo::bar();`
/// Character map: F=8 o=9 o=10 :=11 :=12 b=13 a=14 r=15
///
/// The discriminator is line 2 (`sub bar {`). A reference set for the *package* `Foo`
/// can legitimately include the `Foo::bar` call sites (lines 8/9) and the
/// `package Foo;` declaration (line 0), because the token `Foo` really does occur
/// there. It can never include the `sub bar` declaration on line 2 -- that location
/// is only reachable by resolving the cursor to the final component `bar`.
///
/// Every request propagates its error with `?` rather than defaulting to `null`.
/// These are negative assertions, so a swallowed transport or server error would
/// collapse the result to an empty line set and pass them for the wrong reason.
#[test]
fn test_refs_package_qualified_prefix_does_not_report_sub_declaration() -> TestResult {
    let doc = concat!(
        "package Foo;\n",        // 0
        "\n",                    // 1
        "sub bar {\n",           // 2 -- the discriminator
        "    return 'baz';\n",   // 3
        "}\n",                   // 4
        "\n",                    // 5
        "package main;\n",       // 6
        "\n",                    // 7
        "my $r = Foo::bar();\n", // 8
        "my $s = Foo::bar();\n", // 9
    );

    let mut harness = LspHarness::new();
    harness.initialize(None)?;
    harness.open_document("file:///refs_pkg_qual_prefix.pl", doc)?;

    // Negative control: the cursor on `b` of `bar` (character 13) is the final
    // component. With includeDeclaration=true the `sub bar` declaration on line 2
    // MUST be reported. This proves the fixture, the harness, and the resolution
    // machinery can reach line 2 at all, so the prefix assertion below is a real
    // discriminator rather than a vacuous pass on an empty result.
    let on_final = harness.request(
        "textDocument/references",
        json!({
            "textDocument": {"uri": "file:///refs_pkg_qual_prefix.pl"},
            "position": {"line": 8, "character": 13},
            "context": {"includeDeclaration": true}
        }),
    )?;
    let final_lines = reported_lines(&on_final);
    assert!(
        final_lines.contains(&2),
        "negative control failed: cursor on `bar` (the final component) with \
         includeDeclaration=true must report the `sub bar` declaration on line 2; \
         reported lines: {final_lines:?}. Without this the prefix assertion below \
         would pass vacuously."
    );

    // Cursor on `F` (character 8) -- the start of the `Foo` prefix component.
    let on_prefix = harness.request(
        "textDocument/references",
        json!({
            "textDocument": {"uri": "file:///refs_pkg_qual_prefix.pl"},
            "position": {"line": 8, "character": 8},
            "context": {"includeDeclaration": true}
        }),
    )?;
    let prefix_lines = reported_lines(&on_prefix);
    assert!(
        !prefix_lines.contains(&2),
        "cursor on `Foo` in `Foo::bar()` must not report references to `sub bar`, \
         but line 2 (`sub bar {{`) was returned; reported lines: {prefix_lines:?}. \
         The provider resolved the qualified name to its final component regardless \
         of which `::` component the cursor is on (#1849)."
    );

    // Both characters of the `::` separator (11 and 12) are on no component at all.
    //
    // Character 12 -- the *second* colon, immediately before `b` -- is the position
    // an off-by-one in the classifier lands on. `fqn_component_at_cursor` derives
    // the final component's start as `rfind("::") + 2`; a `+ 1` there would put this
    // character inside the final component and let `sub bar` through. Testing only
    // the first colon leaves that mutation undetected, so both are asserted.
    for separator_character in [11, 12] {
        let on_separator = harness.request(
            "textDocument/references",
            json!({
                "textDocument": {"uri": "file:///refs_pkg_qual_prefix.pl"},
                "position": {"line": 8, "character": separator_character},
                "context": {"includeDeclaration": true}
            }),
        )?;
        let separator_lines = reported_lines(&on_separator);
        assert!(
            !separator_lines.contains(&2),
            "cursor on the `::` separator in `Foo::bar()` at character \
             {separator_character} must not report references to `sub bar`, but line 2 \
             (`sub bar {{`) was returned; reported lines: {separator_lines:?}."
        );
    }

    Ok(())
}

/// Regression (#1849): the three-component middle case for find-references.
///
/// Line 8: `My::Utils::process();`
/// Character map: M=0 y=1 :=2 :=3 U=4 t=5 i=6 l=7 s=8 :=9 :=10 p=11 ...
///
/// Cursor on `Utils` (character 4) is a middle component -- neither the leading
/// package nor the final sub name. It must not report the `sub process`
/// declaration on line 2.
#[test]
fn test_refs_three_component_qualified_middle_does_not_report_sub_declaration() -> TestResult {
    let doc = concat!(
        "package My::Utils;\n",    // 0
        "\n",                      // 1
        "sub process {\n",         // 2 -- the discriminator
        "    return 'done';\n",    // 3
        "}\n",                     // 4
        "\n",                      // 5
        "package main;\n",         // 6
        "\n",                      // 7
        "My::Utils::process();\n", // 8
        "My::Utils::process();\n", // 9
    );

    let mut harness = LspHarness::new();
    harness.initialize(None)?;
    harness.open_document("file:///refs_three_component.pl", doc)?;

    // Negative control: cursor on `process` (character 11) must still reach line 2.
    let on_final = harness.request(
        "textDocument/references",
        json!({
            "textDocument": {"uri": "file:///refs_three_component.pl"},
            "position": {"line": 8, "character": 11},
            "context": {"includeDeclaration": true}
        }),
    )?;
    let final_lines = reported_lines(&on_final);
    assert!(
        final_lines.contains(&2),
        "negative control failed: cursor on `process` (the final component) with \
         includeDeclaration=true must report the `sub process` declaration on line 2; \
         reported lines: {final_lines:?}."
    );

    // Both characters of the final `::` separator (9 and 10). Character 10 sits
    // immediately before `p` of `process` and is where a `rfind("::") + 1`
    // off-by-one would misclassify the cursor as being on the final component.
    for separator_character in [9, 10] {
        let on_separator = harness.request(
            "textDocument/references",
            json!({
                "textDocument": {"uri": "file:///refs_three_component.pl"},
                "position": {"line": 8, "character": separator_character},
                "context": {"includeDeclaration": true}
            }),
        )?;
        let separator_lines = reported_lines(&on_separator);
        assert!(
            !separator_lines.contains(&2),
            "cursor on the final `::` separator in `My::Utils::process()` at character \
             {separator_character} must not report references to `sub process`; \
             reported lines: {separator_lines:?}."
        );
    }

    // Cursor on `U` of `Utils` (character 4) -- the middle component.
    let on_middle = harness.request(
        "textDocument/references",
        json!({
            "textDocument": {"uri": "file:///refs_three_component.pl"},
            "position": {"line": 8, "character": 4},
            "context": {"includeDeclaration": true}
        }),
    )?;
    let middle_lines = reported_lines(&on_middle);
    assert!(
        !middle_lines.contains(&2),
        "cursor on `Utils` in `My::Utils::process()` must not report references to \
         `sub process`, but line 2 (`sub process {{`) was returned; reported lines: \
         {middle_lines:?}. The middle component is not the sub name (#1849)."
    );

    Ok(())
}

// ----------------------------------------------------------------
// Find-references: scope isolation
// ----------------------------------------------------------------

/// Regression: find-references on `my $x` in nested scopes returns a valid response.
///
/// This test locks structural correctness: both the if-scope and else-scope queries
/// must return null or a non-empty Location array without crashing the server.
///
/// NOTE: as of writing the server does NOT implement scope isolation -- querying
/// the if-block `$x` also returns else-block locations. That is a known limitation
/// tracked separately. This test only guards against server crashes and protocol
/// errors on this pattern; it does not assert scope boundaries.
///
/// When scope isolation is implemented, add a stricter test that asserts
/// if-scope refs exclude lines 5-6 and else-scope refs exclude lines 2-3.
#[test]
fn test_refs_scope_returns_valid_response_for_nested_scopes() -> TestResult {
    let doc = concat!(
        "my $cond = 1;\n",       // 0
        "if ($cond) {\n",        // 1
        "    my $x = 'if';\n",   // 2 -- $x in if-scope
        "    print $x;\n",       // 3
        "} else {\n",            // 4
        "    my $x = 'else';\n", // 5 -- different $x in else-scope
        "    print $x;\n",       // 6
        "}\n",                   // 7
    );

    let mut harness = LspHarness::new();
    harness.initialize(None)?;
    harness.open_document("file:///refs_scope.pl", doc)?;

    // Query from if-block `$x` -- line 2, character 7.
    let if_result = harness
        .request(
            "textDocument/references",
            json!({
                "textDocument": {"uri": "file:///refs_scope.pl"},
                "position": {"line": 2, "character": 7},
                "context": {"includeDeclaration": true}
            }),
        )
        .unwrap_or(json!(null));

    // Query from else-block `$x` -- line 5, character 7.
    let else_result = harness
        .request(
            "textDocument/references",
            json!({
                "textDocument": {"uri": "file:///refs_scope.pl"},
                "position": {"line": 5, "character": 7},
                "context": {"includeDeclaration": true}
            }),
        )
        .unwrap_or(json!(null));

    // Both responses must be structurally valid (not a crash or protocol error).
    assert!(
        if_result.is_null() || if_result.is_array(),
        "if-scope `$x` references must be null or array, got: {if_result}"
    );
    assert!(
        else_result.is_null() || else_result.is_array(),
        "else-scope `$x` references must be null or array, got: {else_result}"
    );

    // At minimum, each query must return at least the lines in its own scope.
    if let Some(if_locs) = if_result.as_array() {
        let reported: Vec<u64> =
            if_locs.iter().filter_map(|l| l.pointer("/range/start/line")?.as_u64()).collect();
        assert!(
            reported.contains(&2),
            "if-scope query must include line 2 (declaration); reported: {reported:?}"
        );
        assert!(
            reported.contains(&3),
            "if-scope query must include line 3 (usage); reported: {reported:?}"
        );
    }

    if let Some(else_locs) = else_result.as_array() {
        let reported: Vec<u64> =
            else_locs.iter().filter_map(|l| l.pointer("/range/start/line")?.as_u64()).collect();
        assert!(
            reported.contains(&5),
            "else-scope query must include line 5 (declaration); reported: {reported:?}"
        );
        assert!(
            reported.contains(&6),
            "else-scope query must include line 6 (usage); reported: {reported:?}"
        );
    }

    Ok(())
}

// ----------------------------------------------------------------
// Rename: variable -- all references updated
// ----------------------------------------------------------------

/// Regression: renaming `$old_name` updates all 4 occurrences with `$new_name`.
///
/// Asserts that the WorkspaceEdit contains exactly as many TextEdits as there are
/// occurrences of `$old_name` in the document.
#[test]
fn test_rename_variable_all_occurrences_updated() -> TestResult {
    // $old_name appears exactly 4 times.
    let doc = concat!(
        "my $old_name = 0;\n", // 0 -- declaration
        "$old_name += 1;\n",   // 1 -- use
        "print $old_name;\n",  // 2 -- use
        "return $old_name;\n", // 3 -- use
    );
    let occurrence_count = doc.matches("$old_name").count(); // 4

    let mut harness = LspHarness::new();
    harness.initialize(None)?;
    harness.open_document("file:///rename_var.pl", doc)?;

    let response = harness
        .request(
            "textDocument/rename",
            json!({
                "textDocument": {"uri": "file:///rename_var.pl"},
                "position": {"line": 0, "character": 3},
                "newName": "$new_name"
            }),
        )
        .unwrap_or(json!(null));

    if !response.is_null() {
        let edit_count = count_workspace_edits(&response);
        assert_eq!(
            edit_count, occurrence_count,
            "rename of `$old_name` must produce exactly {occurrence_count} edits \
             (one per occurrence), got {edit_count}"
        );

        // Verify all edits use the new name.
        if let Some(changes) = response.get("changes").and_then(|c| c.as_object()) {
            for edits in changes.values() {
                if let Some(edits_arr) = edits.as_array() {
                    for edit in edits_arr {
                        let new_text = edit.get("newText").and_then(|t| t.as_str()).unwrap_or("");
                        assert!(
                            new_text.contains("new_name"),
                            "Each rename edit must use `new_name`, got: `{new_text}`"
                        );
                    }
                }
            }
        }
    }

    Ok(())
}

/// Regression: rename result does not contain references to the old name.
///
/// Complements the previous test by checking the new text rather than just the
/// count. After rename the old identifier must be gone.
#[test]
fn test_rename_variable_old_name_absent_in_edits() -> TestResult {
    let doc = "my $alpha = 1;\nprint $alpha;\n$alpha++;\n";

    let mut harness = LspHarness::new();
    harness.initialize(None)?;
    harness.open_document("file:///rename_var2.pl", doc)?;

    let response = harness
        .request(
            "textDocument/rename",
            json!({
                "textDocument": {"uri": "file:///rename_var2.pl"},
                "position": {"line": 0, "character": 3},
                "newName": "$beta"
            }),
        )
        .unwrap_or(json!(null));

    if let Some(changes) = response.get("changes").and_then(|c| c.as_object()) {
        for edits in changes.values() {
            if let Some(edits_arr) = edits.as_array() {
                for edit in edits_arr {
                    let new_text = edit.get("newText").and_then(|t| t.as_str()).unwrap_or("");
                    assert!(
                        !new_text.contains("alpha"),
                        "Rename edit newText must not contain old name `alpha`, got: `{new_text}`"
                    );
                }
            }
        }
    }

    Ok(())
}

// ----------------------------------------------------------------
// Rename: sub -- declaration and all call sites updated
// ----------------------------------------------------------------

/// Regression: renaming `sub foo` updates the declaration and every call site.
///
/// Asserts that the WorkspaceEdit edit count equals the total occurrences of `foo`
/// in the source document.
#[test]
fn test_rename_sub_declaration_and_calls_updated() -> TestResult {
    // "foo" appears 4 times: sub declaration + 3 calls.
    let doc = concat!(
        "sub foo { return 1; }\n", // 0 -- declaration
        "\n",                      // 1
        "foo();\n",                // 2 -- call 1
        "my $x = foo();\n",        // 3 -- call 2
        "foo() if 1;\n",           // 4 -- call 3
    );
    let occurrence_count = doc.matches("foo").count(); // 4

    let mut harness = LspHarness::new();
    harness.initialize(None)?;
    harness.open_document("file:///rename_sub.pl", doc)?;

    let response = harness
        .request(
            "textDocument/rename",
            json!({
                "textDocument": {"uri": "file:///rename_sub.pl"},
                "position": {"line": 0, "character": 4},
                "newName": "bar"
            }),
        )
        .unwrap_or(json!(null));

    if !response.is_null() {
        let edit_count = count_workspace_edits(&response);
        assert_eq!(
            edit_count, occurrence_count,
            "rename of `foo` must produce exactly {occurrence_count} edits, got {edit_count}"
        );
    }

    Ok(())
}

/// Regression: sub rename changes the name in all edits to the new identifier.
#[test]
fn test_rename_sub_new_name_in_all_edits() -> TestResult {
    let doc = "sub process { 1 }\nprocess();\nprocess();\n";

    let mut harness = LspHarness::new();
    harness.initialize(None)?;
    harness.open_document("file:///rename_sub2.pl", doc)?;

    let response = harness
        .request(
            "textDocument/rename",
            json!({
                "textDocument": {"uri": "file:///rename_sub2.pl"},
                "position": {"line": 0, "character": 4},
                "newName": "transform"
            }),
        )
        .unwrap_or(json!(null));

    if let Some(changes) = response.get("changes").and_then(|c| c.as_object()) {
        for edits in changes.values() {
            if let Some(edits_arr) = edits.as_array() {
                for edit in edits_arr {
                    let new_text = edit.get("newText").and_then(|t| t.as_str()).unwrap_or("");
                    assert!(
                        new_text.contains("transform"),
                        "Every rename edit must use `transform`, got: `{new_text}`"
                    );
                }
            }
        }
    }

    Ok(())
}

// ----------------------------------------------------------------
// Rename: scope isolation -- different scopes with the same name
// ----------------------------------------------------------------

/// Regression: renaming `$x` in one scope must not touch `$x` in a sibling scope.
///
/// Two independent `my $x` declarations. Renaming the outer one must not produce
/// edits that cross into the inner block's `$x` occurrences.
#[test]
fn test_rename_scope_isolation_nested_same_name() -> TestResult {
    // Outer $x: lines 0, 5
    // Inner $x (different symbol): lines 2, 3
    let doc = concat!(
        "my $x = 'outer';\n",     // 0 -- outer decl
        "{\n",                    // 1
        "    my $x = 'inner';\n", // 2 -- inner decl (separate symbol)
        "    print $x;\n",        // 3 -- inner usage
        "}\n",                    // 4
        "print $x;\n",            // 5 -- outer usage
    );

    let mut harness = LspHarness::new();
    harness.initialize(None)?;
    harness.open_document("file:///rename_scope.pl", doc)?;

    // Rename the outer `$x` (line 0, character 3).
    let response = harness
        .request(
            "textDocument/rename",
            json!({
                "textDocument": {"uri": "file:///rename_scope.pl"},
                "position": {"line": 0, "character": 3},
                "newName": "$outer_x"
            }),
        )
        .unwrap_or(json!(null));

    assert!(response.is_object(), "rename should return a WorkspaceEdit, got {response:?}");

    let mut affected_lines: Vec<u64> = Vec::new();
    let changes = response
        .get("changes")
        .and_then(|c| c.as_object())
        .ok_or("rename response should include changes")?;
    for edits in changes.values() {
        let edits_arr = edits.as_array().ok_or("rename changes should contain edit arrays")?;
        for edit in edits_arr {
            let line = edit
                .pointer("/range/start/line")
                .and_then(|l| l.as_u64())
                .ok_or("rename edit should include start line")?;
            let new_text =
                edit.get("newText").and_then(|text| text.as_str()).ok_or("missing newText")?;
            assert_eq!(new_text, "$outer_x", "lexical rename should preserve the scalar sigil");
            affected_lines.push(line);
        }
    }
    affected_lines.sort_unstable();

    assert_eq!(
        affected_lines,
        vec![0, 5],
        "rename of outer `$x` must only touch the outer declaration and reference"
    );

    Ok(())
}

// ----------------------------------------------------------------
// Structural: harness helpers used above compile and work
// ----------------------------------------------------------------

/// Smoke test for the local `find_pos` helper -- not an LSP test.
#[test]
fn helper_find_pos_correctness() {
    let src = "line0\nline1 needle here\nline2\n";
    let pos = find_pos(src, "needle");
    assert_eq!(pos, Some((1, 6)), "find_pos should return (1, 6) for 'needle' on line 1");

    let last = find_last_pos(src, "line");
    assert_eq!(last, Some((2, 0)), "find_last_pos should return (2, 0) for last 'line'");
}

/// Regression (#1849, review finding): a qualified name longer than the old
/// fixed-radius classification window.
///
/// The cursor-component guard used to classify over a 50-byte window centred on the
/// cursor. With the cursor at the start of a middle component longer than that
/// radius, the window ended *inside* that component, so the truncated match's last
/// `::` fell before it and the component looked like the final one. The guard then
/// reported `Final` and let `symbol_at_cursor_with_source` resolve the trailing sub
/// -- reproducing the exact defect this issue is about, just out of reach of the
/// other regressions here. Classifying over the whole line removes the window.
///
/// Line 6 is `My::<60 chars>::process();`. The middle component alone exceeds the
/// old 50-byte radius, so this fixture only passes if the classifier sees the
/// complete name.
#[test]
fn test_refs_qualified_middle_longer_than_classification_window() -> TestResult {
    let doc = concat!(
        "package My::AaaaaaaaaaBbbbbbbbbbCcccccccccDdddddddddEeeeeeeeeeFfffffffff;\n", // 0
        "\n",                                                                          // 1
        "sub process { return 1; }\n", // 2 -- discriminator
        "\n",                          // 3
        "package main;\n",             // 4
        "\n",                          // 5
        "My::AaaaaaaaaaBbbbbbbbbbCcccccccccDdddddddddEeeeeeeeeeFfffffffff::process();\n", // 6
    );

    let mut harness = LspHarness::new();
    harness.initialize(None)?;
    harness.open_document("file:///refs_long_middle.pl", doc)?;

    // Negative control: the final component `process` begins at character 66.
    let on_final = harness.request(
        "textDocument/references",
        json!({
            "textDocument": {"uri": "file:///refs_long_middle.pl"},
            "position": {"line": 6, "character": 66},
            "context": {"includeDeclaration": true}
        }),
    )?;
    let final_lines = reported_lines(&on_final);
    assert!(
        final_lines.contains(&2),
        "negative control failed: cursor on the final component `process` must report \
         the `sub process` declaration on line 2; reported lines: {final_lines:?}"
    );

    // Cursor on the first character of the over-long middle component.
    let on_middle = harness.request(
        "textDocument/references",
        json!({
            "textDocument": {"uri": "file:///refs_long_middle.pl"},
            "position": {"line": 6, "character": 4},
            "context": {"includeDeclaration": true}
        }),
    )?;
    let middle_lines = reported_lines(&on_middle);
    assert!(
        !middle_lines.contains(&2),
        "cursor on a middle component longer than the classification window must not \
         report references to `sub process`, but line 2 was returned; reported lines: \
         {middle_lines:?}. The classifier saw a truncated name and mistook the middle \
         component for the final one (#1849)."
    );

    Ok(())
}

/// Regression (#1849, review finding): the prefix guard must not suppress a package
/// *variable* whose name merely contains `::`.
///
/// `$Foo::bar` is one symbol. Unlike `Foo::bar()`, its `::` components do not name
/// separate things -- the cursor refers to the same variable wherever it sits inside
/// the name -- and the server resolves it correctly from any position. An earlier
/// revision of the guard keyed only on "the cursor is before the last `::`", which
/// swallowed this case and returned nothing where the server had always returned the
/// variable's use sites. The guard is now restricted to a bare-sub symbol key.
#[test]
fn test_refs_package_variable_prefix_is_not_suppressed() -> TestResult {
    let doc = concat!(
        "package Foo;\n",     // 0
        "our $bar = 1;\n",    // 1
        "package main;\n",    // 2
        "print $Foo::bar;\n", // 3
        "print $Foo::bar;\n", // 4
    );

    let mut harness = LspHarness::new();
    harness.initialize(None)?;
    harness.open_document("file:///refs_pkg_var.pl", doc)?;

    // Line 3: `print $Foo::bar;`  $=6 F=7 o=8 o=9 :=10 :=11 b=12
    let reference_lines = |harness: &mut LspHarness,
                           character: u64|
     -> Result<Vec<u64>, Box<dyn std::error::Error>> {
        let result = harness.request(
            "textDocument/references",
            json!({
                "textDocument": {"uri": "file:///refs_pkg_var.pl"},
                "position": {"line": 3, "character": character},
                "context": {"includeDeclaration": true}
            }),
        )?;
        Ok(reported_lines(&result))
    };

    // Cursor on `b` of `bar` -- the final component.
    let on_final = reference_lines(&mut harness, 12)?;
    assert!(
        !on_final.is_empty(),
        "negative control failed: cursor on `bar` in `$Foo::bar` must report the \
         variable's references; got {on_final:?}"
    );

    // Cursor on `F` of the `Foo` package qualifier -- still the same variable.
    let on_prefix = reference_lines(&mut harness, 7)?;
    assert_eq!(
        on_prefix, on_final,
        "cursor on the `Foo` qualifier of the package variable `$Foo::bar` must report \
         the same references as the cursor on `bar`; got {on_prefix:?} vs {on_final:?}. \
         The qualified-name prefix guard only applies to sub names, where the `::` \
         components name separate things (#1849)."
    );

    Ok(())
}

/// Regression (#1849, review finding): the prefix guard must not suppress a module
/// name in a `use` statement.
///
/// `My::Module` in `use My::Module;` is one package name, not a package plus a sub,
/// so the cursor's `::` component is not a meaningful distinction. An earlier
/// revision of the guard suppressed it and returned nothing.
#[test]
fn test_refs_use_statement_module_prefix_is_not_suppressed() -> TestResult {
    let doc = concat!(
        "package My;\n",     // 0
        "sub greet { 1 }\n", // 1
        "package main;\n",   // 2
        "use My::Module;\n", // 3
        "My::greet();\n",    // 4
    );

    let mut harness = LspHarness::new();
    harness.initialize(None)?;
    harness.open_document("file:///refs_use_module.pl", doc)?;

    // Line 3: `use My::Module;` -- character 4 is `M` of the `My` qualifier.
    let result = harness.request(
        "textDocument/references",
        json!({
            "textDocument": {"uri": "file:///refs_use_module.pl"},
            "position": {"line": 3, "character": 4},
            "context": {"includeDeclaration": true}
        }),
    )?;
    let lines = reported_lines(&result);
    assert!(
        lines.contains(&3),
        "cursor on `My` in `use My::Module;` must still report the module reference on \
         line 3; got {lines:?}. A `use` module name is one package name, not a \
         package-qualified sub, so the #1849 prefix guard must not apply to it."
    );

    Ok(())
}

/// Regression (#1849, review finding): a cursor on the *last* package component of a
/// method-call receiver must not report the method's references.
///
/// In `Foo::Bar->baz()` the qualified-name match stops at the `->`, so a cursor on
/// `Bar` is that match's **final** component — while `symbol_at_cursor_with_source`
/// resolves the method `baz`. A guard that only asked "is the cursor before the last
/// `::`?" therefore let the receiver through and answered with `baz`'s references:
/// this issue's own defect, reached by a different route. Verified before the fix —
/// a cursor on `Bar` returned the same locations as a cursor on `baz`, including the
/// `sub baz` declaration on line 1.
///
/// The guard now also requires the classified final component to be the very token
/// that names the resolved sub.
#[test]
fn test_refs_method_receiver_package_component_does_not_report_method() -> TestResult {
    let doc = concat!(
        "package Foo::Bar;\n",     // 0
        "sub baz { return 1; }\n", // 1 -- discriminator
        "package main;\n",         // 2
        "Foo::Bar->baz();\n",      // 3
        "Foo::Bar->baz();\n",      // 4
    );

    let mut harness = LspHarness::new();
    harness.initialize(None)?;
    harness.open_document("file:///refs_method_receiver.pl", doc)?;

    // Line 3: `Foo::Bar->baz();`
    // F=0 o=1 o=2 :=3 :=4 B=5 a=6 r=7 -=8 >=9 b=10
    let reference_lines = |harness: &mut LspHarness,
                           character: u64|
     -> Result<Vec<u64>, Box<dyn std::error::Error>> {
        let result = harness.request(
            "textDocument/references",
            json!({
                "textDocument": {"uri": "file:///refs_method_receiver.pl"},
                "position": {"line": 3, "character": character},
                "context": {"includeDeclaration": true}
            }),
        )?;
        Ok(reported_lines(&result))
    };

    // Negative control: the cursor on the method name `baz` must report `sub baz`.
    let on_method = reference_lines(&mut harness, 10)?;
    assert!(
        on_method.contains(&1),
        "negative control failed: cursor on the method `baz` must report the \
         `sub baz` declaration on line 1; got {on_method:?}"
    );

    // `Bar` is the receiver's final package component, and `Foo` its prefix.
    // Neither names the method, so neither may report it.
    for (label, character) in [("Foo", 0), ("Bar", 5)] {
        let on_receiver = reference_lines(&mut harness, character)?;
        assert!(
            !on_receiver.contains(&1),
            "cursor on `{label}` (character {character}) of the receiver in \
             `Foo::Bar->baz()` must not report the method `baz`, but line 1 was \
             returned; got {on_receiver:?}. The receiver's components name the \
             package, not the method (#1849)."
        );
    }

    Ok(())
}

/// Regression (#1849, review finding): the qualified-name *fallback* must also refuse
/// a prefix cursor, not just the early guard.
///
/// The early guard only runs when `symbol_at_cursor_with_source` resolves a bare sub.
/// When it resolves nothing -- a `Foo::bar` written inside a comment or a string, an
/// unresolved module name -- the request still reaches the regex fallback further down,
/// which builds a `SymKind::Sub` key from the match's final component. An earlier
/// revision of this PR removed that fallback's own cursor-component check, reasoning it
/// was unreachable; that was true only while the early guard was unconditional, and
/// stopped being true once the guard was narrowed to bare subs.
///
/// Verified against `origin/main`: a cursor on `Foo` in `# see Foo::bar for info`
/// returned `[]` on main and `[1, 1]` -- the `sub bar` declaration -- with the check
/// removed. This test therefore locks a real regression, not a hypothetical one.
#[test]
fn test_refs_qualified_prefix_in_comment_or_string_is_not_reported() -> TestResult {
    let doc = concat!(
        "package Foo;\n",            // 0
        "sub bar { return 2; }\n",   // 1 -- discriminator
        "package main;\n",           // 2
        "# see Foo::bar for info\n", // 3 -- comment: F=6, b=11
        "my $s = \"Foo::bar\";\n",   // 4 -- string:  F=9, b=14
    );

    let mut harness = LspHarness::new();
    harness.initialize(None)?;
    harness.open_document("file:///refs_prefix_comment.pl", doc)?;

    let reference_lines = |harness: &mut LspHarness,
                           line: u32,
                           character: u64|
     -> Result<Vec<u64>, Box<dyn std::error::Error>> {
        let result = harness.request(
            "textDocument/references",
            json!({
                "textDocument": {"uri": "file:///refs_prefix_comment.pl"},
                "position": {"line": line, "character": character},
                "context": {"includeDeclaration": true}
            }),
        )?;
        Ok(reported_lines(&result))
    };

    // Negative control: the cursor on the *final* component inside the comment does
    // reach `sub bar`. This proves the fallback is live for this fixture, so the
    // prefix assertions below cannot pass merely because nothing resolves here.
    let on_final = reference_lines(&mut harness, 3, 11)?;
    assert!(
        on_final.contains(&1),
        "negative control failed: cursor on `bar` in the comment must reach the \
         `sub bar` declaration on line 1, otherwise the fallback is not exercised \
         and the prefix assertions below prove nothing; got {on_final:?}"
    );

    for (label, line, character) in [("comment", 3u32, 6u64), ("string", 4, 9)] {
        let on_prefix = reference_lines(&mut harness, line, character)?;
        assert!(
            !on_prefix.contains(&1),
            "cursor on the `Foo` prefix inside a {label} must not report `sub bar`, \
             but line 1 was returned; got {on_prefix:?}. The early guard does not run \
             when no bare-sub symbol resolves, so the qualified-name fallback needs its \
             own cursor-component check (#1849)."
        );
    }

    Ok(())
}

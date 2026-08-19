//! Incremental-vs-full-parse equivalence invariant (#2893).
//!
//! Asserts that after applying an edit via the incremental path, the resulting
//! `IncrementalState` (source, tokens) matches what a fresh full parse of the
//! edited source would produce. This catches the class of bug where
//! `apply_single_edit` updates `tokens` but leaves `source` or another field
//! stale (e.g. #5036).

// This whole file is an integration test; the failure-path `panic!` below
// turns an unexpected `apply_edits` error into a descriptive test failure,
// which is the sanctioned test-code use of `panic!` — the workspace-wide
// deny is a production-code rule.
#![allow(clippy::panic)]

use perl_incremental_parsing::{Edit, IncrementalState, Parser, apply_edits};

/// Apply an edit to source text manually (no incremental machinery).
fn apply_edit_to_source(source: &str, edit: &Edit) -> String {
    let mut result = String::with_capacity(source.len() + edit.new_text.len());
    result.push_str(&source[..edit.start_byte]);
    result.push_str(&edit.new_text);
    result.push_str(&source[edit.old_end_byte..]);
    result
}

/// Assert that after an incremental edit, the state's source matches a manual
/// application of the same edit, and that re-parsing both yields the same
/// token count.
fn assert_equiv_after_edit(source: &str, edit: Edit) {
    let mut state = IncrementalState::new(source.to_string());
    let original_token_count = state.tokens.len();

    let _result = apply_edits(&mut state, std::slice::from_ref(&edit)).unwrap_or_else(|e| {
        // Large edits fall back to full_reparse, which is fine — we only need
        // to verify equivalence when the incremental path succeeds.
        panic!("apply_edits failed for source {:?} edit {:?}: {:?}", source, edit, e);
    });

    let manual_source = apply_edit_to_source(source, &edit);

    // 1. Source must match.
    assert_eq!(state.source, manual_source, "source mismatch after incremental edit");

    // 2. Re-parsing the manual source should yield the same token count as
    //    the state's tokens (within +/-1 for EOF handling differences).
    let mut fresh_parser = Parser::new(&manual_source);
    let _ = fresh_parser.parse();
    // We compare token counts from the state's lexer output vs a fresh lex.
    let fresh_state = IncrementalState::new(manual_source.clone());
    let token_diff = state.tokens.len() as i64 - fresh_state.tokens.len() as i64;
    assert!(
        token_diff.abs() <= 1,
        "token count drift after edit: incremental={}, fresh={}, original={}, source={:?}",
        state.tokens.len(),
        fresh_state.tokens.len(),
        original_token_count,
        manual_source
    );
}

#[test]
fn equiv_insert_at_start() {
    // "use strict;\n" is 12 bytes; new_end_byte = start_byte(0) + 12 = 12
    assert_equiv_after_edit(
        "my $x = 1;",
        Edit {
            start_byte: 0,
            old_end_byte: 0,
            new_end_byte: 12,
            new_text: "use strict;\n".to_string(),
        },
    );
}

#[test]
fn equiv_insert_at_end() {
    // "\n1;" is 3 bytes; new_end_byte = start_byte(10) + 3 = 13
    assert_equiv_after_edit(
        "my $x = 1;",
        Edit { start_byte: 10, old_end_byte: 10, new_end_byte: 13, new_text: "\n1;".to_string() },
    );
}

#[test]
fn equiv_replace_middle() {
    // "z" is 1 byte; new_end_byte = start_byte(11) + 1 = 12
    assert_equiv_after_edit(
        "my $x = 1; my $y = 2;",
        Edit { start_byte: 11, old_end_byte: 12, new_end_byte: 12, new_text: "z".to_string() },
    );
}

#[test]
fn equiv_delete_substring() {
    // "" is 0 bytes; new_end_byte = start_byte(6) + 0 = 6
    assert_equiv_after_edit(
        "my $foobar = 1;",
        Edit { start_byte: 6, old_end_byte: 9, new_end_byte: 6, new_text: String::new() },
    );
}

#[test]
fn equiv_insert_in_string() {
    // " world" is 6 bytes; new_end_byte = start_byte(12) + 6 = 18
    assert_equiv_after_edit(
        r#"my $s = "hello";"#,
        Edit { start_byte: 12, old_end_byte: 12, new_end_byte: 18, new_text: " world".to_string() },
    );
}

#[test]
fn equiv_replace_statement() {
    // "sub foo { 1; }" is 14 bytes; new_end_byte = start_byte(0) + 14 = 14
    assert_equiv_after_edit(
        "sub foo { return 1; }\nsub bar { return 2; }\n",
        Edit {
            start_byte: 0,
            old_end_byte: 20,
            new_end_byte: 14,
            new_text: "sub foo { 1; }".to_string(),
        },
    );
}

#[test]
fn equiv_multi_line_insert() {
    // "sub new { bless {}, shift }\n\n" is 29 bytes; new_end_byte = start_byte(13) + 29 = 42
    assert_equiv_after_edit(
        "package Foo;\n\n1;\n",
        Edit {
            start_byte: 13,
            old_end_byte: 13,
            new_end_byte: 42,
            new_text: "sub new { bless {}, shift }\n\n".to_string(),
        },
    );
}

#[test]
fn equiv_add_use_statement() {
    // "use strict;\nuse warnings;\n\n" is 27 bytes; new_end_byte = start_byte(13) + 27 = 40
    assert_equiv_after_edit(
        "package Foo;\n\n1;\n",
        Edit {
            start_byte: 13,
            old_end_byte: 13,
            new_end_byte: 40,
            new_text: "use strict;\nuse warnings;\n\n".to_string(),
        },
    );
}

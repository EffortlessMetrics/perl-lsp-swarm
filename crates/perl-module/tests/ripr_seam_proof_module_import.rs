//! Mutation-proof boundary tests for the qw-whitespace seam in `perl-module`.
//!
//! Each test pins ONE decision boundary so that a single-line mutation of the
//! guarding sub-condition causes exactly that test to fail.  Expectations were
//! derived from the actual binary output of `extract_require_import_symbols`
//! on `origin/main` after the fix shipped — never from predicted behaviour.
//!
//! Seam — qw-delimiter whitespace trim (`import/mod.rs`, `parse_qw_arg_list`, #1204)
//!   Issue: `Foo::Bar->import(qw [alpha beta])` (space between `qw` and `[`)
//!   was silently dropped.  The space was treated as the delimiter, which is
//!   not in the bracket-closing map, so the whole list was rejected.
//!   Fix: `trimmed.strip_prefix("qw")?.trim_start()` — two changes in tandem:
//!     (a) `.trim_start()` removes leading whitespace before the delimiter.
//!     (b) The explicit `|| delimiter.is_whitespace()` guard was removed from
//!         the alphanumeric/underscore rejection check, because after trimming
//!         the delimiter can never be whitespace when the input is valid.
//!
//!   The discriminating observable signal is `extract_require_import_symbols`
//!   (public API).  A mutation that:
//!     - removes `.trim_start()`, OR
//!     - restores `|| delimiter.is_whitespace()` in the guard,
//!   causes the spaced-delimiter tests below to return an empty Vec instead
//!   of the expected symbol list.
//!
//! Key output shapes:
//!   `qw [a b]`   → `["a", "b"]`  (space before delimiter — the fixed seam)
//!   `qw(a b)`    → `["a", "b"]`  (compact form — must stay working; control)
//!   `qw\t[a b]`  → `["a", "b"]`  (tab before delimiter)
//!   `qwfoo`      → `[]`           (bareword after qw — must stay rejected)

use perl_module::extract_require_import_symbols;

// ── helper ────────────────────────────────────────────────────────────────────

/// Wrap `require Mod;\nMod->import(ARG_SUFFIX)\n` and return the extracted
/// symbol names via the public `extract_require_import_symbols` API.
///
/// `arg_suffix` is everything from the opening `(` of `import(...)` onward,
/// e.g. `"(qw [alpha beta])"`.
fn symbols(arg_suffix: &str) -> Vec<String> {
    let source = format!("require Foo::Bar;\nFoo::Bar->import{arg_suffix};\n");
    extract_require_import_symbols(&source)
        .into_iter()
        .map(|e| e.symbol)
        .collect()
}

// ═══════════════════════════════════════════════════════════════════════════════
// SEAM — qw-delimiter whitespace trim (`parse_qw_arg_list`, #1204)
// ═══════════════════════════════════════════════════════════════════════════════
//
// The fix changes two related things in `parse_qw_arg_list`:
//   (a) `let after_operator = trimmed.strip_prefix("qw")?.trim_start();`
//       Without `.trim_start()`, leading whitespace becomes the delimiter.
//       Space is not in the bracket-closing map, so `closing = ' '` and the
//       inner-extraction indexing goes wrong → empty symbol list.
//   (b) The guard `if delimiter.is_ascii_alphanumeric() || delimiter == '_' {`
//       Previously also had `|| delimiter.is_whitespace()`.  After (a) the
//       delimiter is never whitespace for valid input, but without (b) the
//       old guard would reject all spaced-delimiter forms even after (a).
//
// Both (a) and (b) are tested here through the public API so RIPR can trace
// the mutation discriminating signal through the module boundary.

// ── BOUNDARY A: space before `[` delimiter → symbols extracted (tests trim_start) ──

/// `qw [a b]` — space before `[` delimiter must yield `["a", "b"]`.
/// Pinned boundary: `.trim_start()` present → delimiter is `[` → correct extraction.
/// Removing `.trim_start()` makes the delimiter `' '`, which matches `other => ' '`
/// in the closing map, so `inner_end` goes to `after_operator.len() - 1` on the
/// space-prefixed string and produces garbage (or empty) instead of `["a", "b"]`.
#[test]
fn seam_space_before_bracket_delimiter_extracts_symbols() {
    let syms = symbols("(qw [alpha beta])");
    assert!(
        syms.contains(&"alpha".to_string()),
        "qw [..] must extract 'alpha'; got: {syms:?}"
    );
    assert!(
        syms.contains(&"beta".to_string()),
        "qw [..] must extract 'beta'; got: {syms:?}"
    );
    assert_eq!(
        syms.len(),
        2,
        "qw [alpha beta] must yield exactly 2 symbols; got: {syms:?}"
    );
}

/// `qw [a b]` — exact symbol names match, not just non-empty.
/// A mutation that strips the wrong range (off-by-one) would produce mangled names.
#[test]
fn seam_space_before_bracket_symbol_names_are_exact() {
    let syms = symbols("(qw [exact1 exact2])");
    assert_eq!(
        syms,
        vec!["exact1".to_string(), "exact2".to_string()],
        "symbol names must be exact; got: {syms:?}"
    );
}

// ── BOUNDARY B: compact form (no space) still works → control case ────────────

/// `qw(a b)` — compact form must continue to yield `["a", "b"]`.
/// Verifies the guard does not over-trim or break the existing path.
#[test]
fn seam_compact_qw_paren_still_works() {
    let syms = symbols("(qw(compact1 compact2))");
    assert_eq!(
        syms,
        vec!["compact1".to_string(), "compact2".to_string()],
        "compact qw(..) must yield exact symbols; got: {syms:?}"
    );
}

// ── BOUNDARY C: tab before delimiter ─────────────────────────────────────────

/// `qw\t[a b]` — tab as whitespace before `[` must yield `["a", "b"]`.
/// Boundary: `.trim_start()` strips all whitespace, not just ASCII space.
/// Removing `.trim_start()` makes `\t` the delimiter → extraction fails.
#[test]
fn seam_tab_before_bracket_delimiter_extracts_symbols() {
    let syms = symbols("(qw\t[tab1 tab2])");
    assert!(
        syms.contains(&"tab1".to_string()),
        "qw\\t[..] must extract 'tab1'; got: {syms:?}"
    );
    assert!(
        syms.contains(&"tab2".to_string()),
        "qw\\t[..] must extract 'tab2'; got: {syms:?}"
    );
}

// ── BOUNDARY D: space before `(` delimiter ───────────────────────────────────

/// `qw (a b)` — space before `(` delimiter must yield `["a", "b"]`.
/// Tests a different bracket form to ensure the bracket-map path is also covered.
#[test]
fn seam_space_before_paren_delimiter_extracts_symbols() {
    let syms = symbols("(qw (paren1 paren2))");
    assert_eq!(
        syms,
        vec!["paren1".to_string(), "paren2".to_string()],
        "qw (..) must yield exact symbols; got: {syms:?}"
    );
}

// ── BOUNDARY E: space before `/` non-bracket delimiter ───────────────────────

/// `qw /a b/` — space before slash delimiter must yield `["a", "b"]`.
/// Tests the `other => other` arm of the closing-character match.
/// Without `.trim_start()`, space becomes delimiter and `closing = ' '`
/// causing the trailing-space check to fail (the string ends with `/`, not ` `).
#[test]
fn seam_space_before_slash_delimiter_extracts_symbols() {
    let syms = symbols("(qw /slash1 slash2/)");
    assert_eq!(
        syms,
        vec!["slash1".to_string(), "slash2".to_string()],
        "qw /../ must yield exact symbols; got: {syms:?}"
    );
}

// ── BOUNDARY F: bareword after qw stays rejected (no-whitespace guard removed) ──

/// `qwfoo` — bareword directly after qw must produce no symbols.
/// Pinned boundary: the alphanumeric/underscore guard is still in place.
/// A mutation removing `delimiter.is_ascii_alphanumeric()` would let
/// `qwfoo` be treated as a delimiter `'f'` — producing garbled output.
#[test]
fn seam_bareword_after_qw_is_rejected() {
    // `qwfoo` is not a valid qw invocation — it parses as the bareword `qwfoo`.
    let syms = symbols("(qwfoo)");
    assert!(
        syms.is_empty(),
        "qwfoo must not extract any symbols; got: {syms:?}"
    );
}

// ── BOUNDARY G: the whitespace-guard is gone — space is now valid before delimiters ──

/// Regression: the old `|| delimiter.is_whitespace()` guard must NOT be present.
/// If a mutation restores it, `qw [..]` returns empty even after `.trim_start()`.
/// This test is the direct observable proof that the guard change holds.
/// It is structurally identical to BOUNDARY A but documents the specific guard seam.
#[test]
fn seam_whitespace_guard_removed_space_before_bracket_is_accepted() {
    let syms = symbols("(qw [guard_a guard_b])");
    assert_eq!(
        syms.len(),
        2,
        "guard must be absent: space-before-bracket must yield 2 symbols; got: {syms:?}"
    );
    assert!(
        syms.contains(&"guard_a".to_string()),
        "guard_a must be extracted; got: {syms:?}"
    );
    assert!(
        syms.contains(&"guard_b".to_string()),
        "guard_b must be extracted; got: {syms:?}"
    );
}

// ── BOUNDARY H: module field is correctly populated for spaced-delimiter form ──

/// Full `RequireImportEntry` field check: module name is preserved for spaced form.
/// Verifies that the trim doesn't corrupt the module name extracted from the
/// `require Foo::Bar` line.
#[test]
fn seam_space_before_bracket_module_name_is_correct() {
    let source = "require Foo::Bar;\nFoo::Bar->import(qw [modcheck1 modcheck2]);\n";
    let entries = extract_require_import_symbols(source);
    assert_eq!(
        entries.len(),
        2,
        "expected 2 entries for spaced qw; got: {entries:?}"
    );
    for e in &entries {
        assert_eq!(
            e.module, "Foo::Bar",
            "module name must be Foo::Bar; got: {:?}",
            e.module
        );
    }
    let names: Vec<&str> = entries.iter().map(|e| e.symbol.as_str()).collect();
    assert!(names.contains(&"modcheck1"), "modcheck1 must appear; got: {names:?}");
    assert!(names.contains(&"modcheck2"), "modcheck2 must appear; got: {names:?}");
}

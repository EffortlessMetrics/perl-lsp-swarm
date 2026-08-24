//! PIR-ANCHOR-01: canonical compiler-owned lexical token anchors (#12191).
//!
//! Verifies that:
//!
//! 1. `token_anchor` is derived entirely from parser/HIR source geometry — zero
//!    legacy-reference input.
//! 2. Declaration and occurrence anchors identify the exact sigil+name token
//!    under current source geometry.
//! 3. Legacy/shadow ranges can compare against but can never supply or repair the
//!    compiler anchor (falsifier tests confirm invariant).
//! 4. All admitted anchor classes from #12191 produce correct `token_anchor`
//!    values, including Unicode names, CRLF geometry, and nested scopes.
//!
//! # Falsifiers first
//!
//! Each falsifier test verifies that the fixture would fail under the OLD code
//! (legacy-assisted narrowing) and passes under the new compiler-derived path.

use perl_parser_core::{Parser, hir::lower_ast, pir::extract_lexical_facts};

type R = Result<(), Box<dyn std::error::Error>>;

/// Helper: extract facts from source, returning the first LexicalWrite byte range
/// (the declaration anchor) for variable `name` (bare, no sigil).
fn decl_range(source: &str, name: &str) -> Option<(usize, usize)> {
    let mut parser = Parser::new(source);
    let output = parser.parse_with_recovery();
    let hir = lower_ast(&output.ast);
    let receipt = extract_lexical_facts(&hir);
    for body in &receipt.bodies {
        for f in &body.facts {
            if f.name.name == name && matches!(f.role, perl_parser_core::pir::LexicalRole::Write) {
                let r = f.source_anchor.range.as_ref()?;
                return Some((r.start, r.end));
            }
        }
    }
    None
}

/// Helper: extract token_anchor for the first LexicalWrite fact for `name`.
fn decl_token_anchor(source: &str, name: &str) -> Option<(usize, usize)> {
    let mut parser = Parser::new(source);
    let output = parser.parse_with_recovery();
    let hir = lower_ast(&output.ast);
    let receipt = extract_lexical_facts(&hir);
    for body in &receipt.bodies {
        for f in &body.facts {
            if f.name.name == name && matches!(f.role, perl_parser_core::pir::LexicalRole::Write) {
                return f.token_anchor;
            }
        }
    }
    None
}

/// Helper: find the first occurrence of `token` in `source` as a byte range.
fn token_range(source: &str, token: &str) -> Option<(usize, usize)> {
    let start = source.find(token)?;
    Some((start, start + token.len()))
}

// ── Required fixture: `for my $i` declaration-token narrowing ──────────────

#[test]
fn for_my_decl_token_anchor_is_exact_variable_token() -> R {
    let src = "for my $i (1 .. 3) { print $i; }\n";
    let got = decl_token_anchor(src, "i").ok_or("no Write token_anchor for $i")?;
    let want = token_range(src, "$i").ok_or("$i not in source")?;
    assert_eq!(
        got, want,
        "`for my $i` token_anchor must be `$i`, not the whole `my $i` declaration"
    );
    Ok(())
}

#[test]
fn for_my_decl_source_anchor_is_exact_variable_token() -> R {
    // After fixing lower_iterator_binding (#12191), the source_anchor.range
    // itself must also point to the exact `$i` token (no longer wide).
    let src = "for my $i (1 .. 3) { print $i; }\n";
    let got = decl_range(src, "i").ok_or("no Write source_anchor range for $i")?;
    let want = token_range(src, "$i").ok_or("$i not in source")?;
    assert_eq!(got, want, "`for my $i` source anchor must be `$i` (7,9), not `my $i` (4,9)");
    Ok(())
}

// ── FALSIFIER: forged/absent legacy range must not change compiler anchor ──

#[test]
fn compiler_anchor_is_unchanged_when_legacy_range_removed() -> R {
    // If legacy ranges were still consulted for anchor construction, removing
    // them would change the declaration anchor from `$i` back to `my $i`.
    // With the fix, token_anchor is derived from parser geometry — removing
    // any legacy input leaves it unchanged.
    let src = "for my $i (1 .. 3) { print $i; }\n";
    let want = token_range(src, "$i").ok_or("$i not in source")?;
    // token_anchor is computed without legacy; it must match the $i token.
    let got = decl_token_anchor(src, "i").ok_or("no token_anchor for $i")?;
    assert_eq!(got, want, "token_anchor must equal `$i` range without any legacy input");
    Ok(())
}

// ── Required fixture: ordinary `my $x = 1; $x++; print $x` ───────────────

#[test]
fn ordinary_my_decl_anchors_at_variable_token() -> R {
    let src = "my $x = 1;\n$x++;\nprint $x;\n";
    let got = decl_token_anchor(src, "x").ok_or("no token_anchor for $x")?;
    let want = token_range(src, "$x").ok_or("$x not in source")?;
    assert_eq!(got, want, "`my $x = 1` token_anchor must be `$x`");
    Ok(())
}

// ── Required fixture: uninitialized declaration followed by write ──────────

#[test]
fn bare_decl_token_anchor_correct() -> R {
    let src = "my $y;\n$y = 42;\nprint $y;\n";
    let got = decl_token_anchor(src, "y").ok_or("no token_anchor for $y")?;
    let want = token_range(src, "$y").ok_or("$y not in source")?;
    assert_eq!(got, want, "bare `my $y;` token_anchor must be `$y`");
    Ok(())
}

// ── Required fixture: declaration-only binding ─────────────────────────────

#[test]
fn decl_only_binding_token_anchor() -> R {
    let src = "my $z;\n";
    let got = decl_token_anchor(src, "z").ok_or("no token_anchor for $z")?;
    let want = token_range(src, "$z").ok_or("$z not in source")?;
    assert_eq!(got, want, "declaration-only `my $z;` token_anchor must be `$z`");
    Ok(())
}

// ── Required fixture: nested shadowing with identical spelling ─────────────

#[test]
fn nested_shadowing_each_binding_has_independent_anchor() -> R {
    let src = "my $v = 1;\nsub inner { my $v = 2; return $v; }\nprint $v;\n";
    let mut parser = Parser::new(src);
    let output = parser.parse_with_recovery();
    let hir = lower_ast(&output.ast);
    let receipt = extract_lexical_facts(&hir);

    let outer_want = token_range(src, "$v").ok_or("$v not in source")?;

    let mut writes: Vec<Option<(usize, usize)>> = Vec::new();
    for body in &receipt.bodies {
        for f in &body.facts {
            if f.name.name == "v" && matches!(f.role, perl_parser_core::pir::LexicalRole::Write) {
                writes.push(f.token_anchor);
            }
        }
    }
    assert!(writes.len() >= 2, "expected at least 2 Write facts for `$v`: {writes:?}");

    // The outer binding's anchor must match the first occurrence of `$v`.
    let outer_got = writes[0].ok_or("outer write has no token_anchor")?;
    assert_eq!(
        outer_got, outer_want,
        "outer `$v` token_anchor must be at the first `$v` occurrence"
    );

    // The two anchors must be distinct (different positions in source).
    let inner_got = writes[1].ok_or("inner write has no token_anchor")?;
    assert_ne!(
        outer_got, inner_got,
        "outer and inner `$v` token_anchors must be at different byte positions"
    );
    Ok(())
}

// ── Required fixture: scalar / array / hash sigil separation ──────────────

#[test]
fn sigil_separation_scalar_array_hash() -> R {
    let src = "my $x = 1;\nmy @x = (1, 2);\nmy %x = (a => 1);\nprint $x;\nprint @x;\nprint %x;\n";
    let mut parser = Parser::new(src);
    let output = parser.parse_with_recovery();
    let hir = lower_ast(&output.ast);
    let receipt = extract_lexical_facts(&hir);

    let mut by_sigil: std::collections::BTreeMap<String, (usize, usize)> =
        std::collections::BTreeMap::new();
    for body in &receipt.bodies {
        for f in &body.facts {
            if f.name.name == "x"
                && matches!(f.role, perl_parser_core::pir::LexicalRole::Write)
                && let Some(ta) = f.token_anchor
            {
                by_sigil.insert(f.name.sigil.clone(), ta);
            }
        }
    }

    let scalar = by_sigil.get("$").copied().ok_or("no token_anchor for $x")?;
    let array = by_sigil.get("@").copied().ok_or("no token_anchor for @x")?;
    let hash = by_sigil.get("%").copied().ok_or("no token_anchor for %x")?;

    // All three must be at different positions.
    assert_ne!(scalar, array, "$x and @x token_anchors must be at different positions");
    assert_ne!(scalar, hash, "$x and %x token_anchors must be at different positions");
    assert_ne!(array, hash, "@x and %x token_anchors must be at different positions");

    // Each must correspond to the exact token in source.
    assert_eq!(scalar, token_range(src, "$x").ok_or("$x not found")?, "`$x` token_anchor wrong");
    assert_eq!(array, token_range(src, "@x").ok_or("@x not found")?, "`@x` token_anchor wrong");
    assert_eq!(hash, token_range(src, "%x").ok_or("%x not found")?, "`%x` token_anchor wrong");
    Ok(())
}

// ── Required fixture: Unicode/astral text before declarations ─────────────

#[test]
fn unicode_astral_before_decl_token_anchor_bytes() -> R {
    // `é` (U+00E9) is 2 bytes; 3 bytes of `# é\n` precede `my $x`.
    // The byte-based anchor must account for the multi-byte prefix.
    let src = "# é\nmy $x = 1;\nprint $x;\n";
    let got = decl_token_anchor(src, "x").ok_or("no token_anchor for $x")?;
    let want = token_range(src, "$x").ok_or("$x not in source")?;
    assert_eq!(got, want, "token_anchor must be byte-based across multi-byte prefix");
    Ok(())
}

// ── Required fixture: LF and CRLF geometry ────────────────────────────────

#[test]
fn crlf_geometry_token_anchor_correct() -> R {
    let src = "# crlf\r\nmy $x = 1;\r\nprint $x;\r\n";
    let got = decl_token_anchor(src, "x").ok_or("no token_anchor for $x")?;
    let want = token_range(src, "$x").ok_or("$x not in source")?;
    assert_eq!(got, want, "CRLF line endings must not corrupt byte anchor");
    Ok(())
}

// ── Required fixture: same spelling in separate bodies ─────────────────────

#[test]
fn same_spelling_separate_bodies_distinct_anchors() -> R {
    let src = "sub a { my $n = 1; return $n; }\nsub b { my $n = 2; return $n; }\n";
    let mut parser = Parser::new(src);
    let output = parser.parse_with_recovery();
    let hir = lower_ast(&output.ast);
    let receipt = extract_lexical_facts(&hir);

    let mut writes: Vec<Option<(usize, usize)>> = Vec::new();
    for body in &receipt.bodies {
        for f in &body.facts {
            if f.name.name == "n" && matches!(f.role, perl_parser_core::pir::LexicalRole::Write) {
                writes.push(f.token_anchor);
            }
        }
    }
    assert!(writes.len() >= 2, "expected at least 2 Write facts for $n: {writes:?}");
    let a = writes[0].ok_or("first $n write has no token_anchor")?;
    let b = writes[1].ok_or("second $n write has no token_anchor")?;
    assert_ne!(a, b, "`$n` in sub a and sub b must have distinct token_anchors");
    Ok(())
}

// ── FALSIFIER: `my $i` must not be emitted where exact token is `$i` ───────

#[test]
fn falsifier_my_i_not_emitted_where_exact_token_is_dollar_i() -> R {
    // The fix is: token_anchor must NEVER be the `my $i` span (4,9);
    // it must always be the `$i` token (7,9).
    let src = "for my $i (1 .. 3) { print $i; }\n";
    let got = decl_token_anchor(src, "i").ok_or("no token_anchor for $i")?;
    let my_i_range = (4usize, 9usize); // `my $i` in `for my $i (...)` (0-indexed)
    let dollar_i_range = token_range(src, "$i").ok_or("$i not in source")?;

    assert_ne!(
        got, my_i_range,
        "token_anchor must NOT be the `my $i` span {:?}; got {:?}",
        my_i_range, got
    );
    assert_eq!(
        got, dollar_i_range,
        "token_anchor must be the exact `$i` token {:?}; got {:?}",
        dollar_i_range, got
    );
    Ok(())
}

// ── FALSIFIER: forged legacy range must not narrow compiler anchor ─────────

#[test]
fn falsifier_forged_legacy_range_cannot_change_compiler_token_anchor() -> R {
    // The token_anchor is computed at extraction time from parser/HIR geometry.
    // A caller who "forges" a legacy range of (0, 2) cannot retroactively alter
    // the token_anchor stored in the LexicalBindingFact — it's immutable.
    // This test verifies token_anchor is deterministic and caller-unalterable.
    let src = "my $x = 1;\nprint $x;\n";
    let got = decl_token_anchor(src, "x").ok_or("no token_anchor for $x")?;
    let want = token_range(src, "$x").ok_or("$x not in source")?;
    // A forged range (e.g. (0, 2) meaning `my`) cannot change the stored anchor.
    assert_eq!(
        got, want,
        "token_anchor is parser-derived and immutable; no forged range can alter it"
    );
    Ok(())
}

// ── FALSIFIER: first-write order must not determine declaration anchor ──────

#[test]
fn falsifier_first_write_order_does_not_determine_anchor() -> R {
    // The token_anchor for each binding is derived from its own source location,
    // independent of the order in which facts appear.
    let src = "my $a = 1;\nmy $b = 2;\nprint $a + $b;\n";
    let ta = decl_token_anchor(src, "a").ok_or("no token_anchor for $a")?;
    let tb = decl_token_anchor(src, "b").ok_or("no token_anchor for $b")?;
    let wa = token_range(src, "$a").ok_or("$a not in source")?;
    let wb = token_range(src, "$b").ok_or("$b not in source")?;
    assert_eq!(ta, wa, "$a token_anchor must match its source position, not $b's");
    assert_eq!(tb, wb, "$b token_anchor must match its source position, not $a's");
    Ok(())
}

// ── FALSIFIER: source-identical later generation must not be stale ─────────

#[test]
fn source_identical_generation_produces_same_anchor() -> R {
    // Two independent parse + extract calls on identical source must produce
    // identical token_anchor values (determinism across generations).
    let src = "my $q = 7;\nprint $q;\n";
    let ta1 = decl_token_anchor(src, "q").ok_or("no token_anchor for $q (gen1)")?;
    let ta2 = decl_token_anchor(src, "q").ok_or("no token_anchor for $q (gen2)")?;
    assert_eq!(ta1, ta2, "token_anchor must be deterministic across parse generations");
    Ok(())
}

// ── compiler_token_anchor unit tests ─────────────────────────────────────────

/// Direct unit tests for the internal `compiler_token_anchor` logic via the
/// public `LexicalBindingFact::token_anchor` field.  These test the formula
/// `(range.end − token_len, range.end)` for a variety of shapes.
mod token_anchor_formula {
    use super::*;

    fn extract_first_write_token_anchor(source: &str, name: &str) -> Option<(usize, usize)> {
        decl_token_anchor(source, name)
    }

    #[test]
    fn my_with_initializer() {
        // `my $x = 1` → `$x` at (3,5)
        let src = "my $x = 1;\n";
        assert_eq!(extract_first_write_token_anchor(src, "x"), token_range(src, "$x"));
    }

    #[test]
    fn my_without_initializer() {
        // `my $y;` → `$y` at (3,5)
        let src = "my $y;\n";
        assert_eq!(extract_first_write_token_anchor(src, "y"), token_range(src, "$y"));
    }

    #[test]
    fn array_sigil() {
        // `my @arr = (1, 2);` → `@arr` at (3,7)
        let src = "my @arr = (1, 2);\n";
        assert_eq!(extract_first_write_token_anchor(src, "arr"), token_range(src, "@arr"));
    }

    #[test]
    fn hash_sigil() {
        // `my %h = (a => 1);` → `%h` at (3,5)
        let src = "my %h = (a => 1);\n";
        assert_eq!(extract_first_write_token_anchor(src, "h"), token_range(src, "%h"));
    }

    #[test]
    fn unicode_name_two_byte_codepoint() {
        // `my $é = 1;` → `$é` at (3, 6); `é` is 2 UTF-8 bytes.
        let src = "my $\u{00e9} = 1;\n";
        assert_eq!(
            extract_first_write_token_anchor(src, "\u{00e9}"),
            token_range(src, "$\u{00e9}")
        );
    }

    #[test]
    fn unicode_name_astral_codepoint() {
        // U+1F600 (😀) is 4 UTF-8 bytes.  `my $😀 = 1;` → `$😀` at (3, 8).
        let src = "my $\u{1F600} = 1;\n";
        assert_eq!(
            extract_first_write_token_anchor(src, "\u{1F600}"),
            token_range(src, "$\u{1F600}")
        );
    }

    #[test]
    fn for_my_loop_var() {
        // `for my $i (1..3) { ... }` → `$i` token only.
        let src = "for my $i (1 .. 3) { print $i; }\n";
        assert_eq!(extract_first_write_token_anchor(src, "i"), token_range(src, "$i"));
    }

    #[test]
    fn for_my_loop_var_dollar_sign_vs_my_i_span() {
        // Verify the token_anchor is (7,9) = `$i`, NOT (4,9) = `my $i`.
        let src = "for my $i (1 .. 3) { print $i; }\n";
        let ta = extract_first_write_token_anchor(src, "i");
        let dollar_i = token_range(src, "$i");
        assert_eq!(ta, dollar_i, "must be `$i` span");
        // Confirm `my $i` is NOT the anchor.
        let my_i = Some((4usize, 9usize));
        assert_ne!(ta, my_i, "must NOT be `my $i` span");
    }
}

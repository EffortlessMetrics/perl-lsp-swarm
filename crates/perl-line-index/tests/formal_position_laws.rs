//! Exhaustive finite-domain regression tests for the native proof predicates.
//! These are ordinary tests, not a Kani result or an unbounded Unicode claim.

use perl_line_index::LineIndex;
use std::collections::BTreeSet;

#[path = "formal/contract.rs"]
mod contract;

use contract::{CASE_COUNT, MAX_BYTES, MAX_SCALARS, case_text};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn coordinates(limit: usize) -> Vec<usize> {
    (0..=limit).chain([usize::MAX / 2, usize::MAX - 1, usize::MAX]).collect()
}

#[test]
fn domain_is_exactly_585_unique_texts_with_declared_extremes() -> TestResult {
    let mut texts = BTreeSet::new();
    let mut lengths = [0; MAX_SCALARS + 1];
    let mut max_bytes = 0;
    let mut max_lines = 0;
    let mut max_utf16 = 0;
    for case in 0..CASE_COUNT {
        let storage = case_text(case);
        let text = storage.as_str()?;
        assert!(texts.insert(text.to_owned()), "duplicate text at case {case}");
        lengths[text.chars().count()] += 1;
        max_bytes = max_bytes.max(text.len());
        max_lines = max_lines.max(text.bytes().filter(|&byte| byte == b'\n').count() + 1);
        max_utf16 = max_utf16.max(text.encode_utf16().count());
    }
    assert_eq!(texts.len(), usize::from(CASE_COUNT));
    assert_eq!(lengths, [1, 8, 64, 512]);
    assert_eq!((max_bytes, max_lines, max_utf16), (12, 4, 6));
    for witness in ["", "\r\n", "é", "中", "😀", "a\u{0301}", "\n\n\n", "😀😀😀"] {
        assert!(texts.contains(witness), "domain omitted {witness:?}");
    }
    Ok(())
}

#[test]
fn all_domain_byte_queries_match_independent_acceptance_and_clamping() -> TestResult {
    let positions = coordinates(MAX_BYTES + 1);
    let lines = coordinates(MAX_SCALARS + 2);
    for case in 0..CASE_COUNT {
        let storage = case_text(case);
        let text = storage.as_str()?;
        let index = LineIndex::new(text);
        for &offset in &positions {
            // Independent acceptance queries need not share the forward offset.
            contract::check_bytes(text, &index, offset, 0, 0);
        }
        for &line in &lines {
            for &column in &positions {
                contract::check_bytes(text, &index, text.len(), line, column);
            }
        }
    }
    Ok(())
}

#[test]
fn all_domain_utf16_queries_match_independent_acceptance_and_rejection() -> TestResult {
    let columns = coordinates(MAX_BYTES + 1);
    let lines = coordinates(MAX_SCALARS + 2);
    for case in 0..CASE_COUNT {
        let storage = case_text(case);
        let text = storage.as_str()?;
        let index = LineIndex::new(text);
        for &line in &lines {
            for &column in &columns {
                contract::check_utf16_sound(text, &index, line, column);
                assert_eq!(
                    index.position_to_byte_utf16(text, line, column),
                    contract::utf16_at(text, line, column),
                    "case={case}; text={text:?}; line={line}; column={column}"
                );
            }
        }
        for byte in coordinates(MAX_BYTES + 1) {
            contract::check_utf16_complete(text, &index, byte);
        }
    }
    Ok(())
}

#[test]
fn reference_relations_match_literal_byte_and_utf16_witnesses() {
    for (text, offset, expected) in [
        ("", usize::MAX, (0, 0)),
        ("é", 1, (0, 1)),
        ("a\r\nb", 2, (0, 2)),
        ("a\r\nb", 3, (1, 0)),
        ("\n\n", usize::MAX, (2, 0)),
    ] {
        assert_eq!(contract::byte_position(text, offset), expected);
        assert_eq!(LineIndex::new(text).byte_to_position(offset), expected);
    }
    for (text, line, column, expected) in [
        ("", 0, 0, Some(0)),
        ("", 1, 0, None),
        ("é", 0, 1, Some(2)),
        ("😀", 0, 1, None),
        ("😀", 0, 2, Some(4)),
        ("a\r\nb", 0, 1, Some(1)),
        ("a\r\nb", 0, 2, None),
        ("a\r\nb", 1, 0, Some(3)),
        ("a\r", 0, 2, Some(2)),
        ("\r\n", 1, 0, Some(2)),
        ("a\u{0301}", 0, 2, Some(3)),
        ("中", usize::MAX, usize::MAX, None),
    ] {
        assert_eq!(contract::utf16_at(text, line, column), expected);
        assert_eq!(LineIndex::new(text).position_to_byte_utf16(text, line, column), expected);
    }
}

#[test]
fn raw_byte_scalar_boundary_theorem_has_a_real_production_counterexample() {
    let text = "é";
    let index = LineIndex::new(text);
    let position = index.byte_to_position(1);
    assert_eq!(position, (0, 1));
    assert_eq!(index.position_to_byte(position.0, position.1), Some(1));
    assert!(!text.is_char_boundary(1));
    // The same numeral denotes the end of the scalar in UTF-16 units.
    assert_eq!(index.position_to_byte_utf16(text, 0, 1), Some(2));
}

#[test]
fn bom_nul_and_longer_examples_are_separate_from_the_symbolic_domain() {
    // Supplemental regressions: these do not enlarge the 585-text Kani claim.
    for text in ["\0", "\u{feff}a", "\u{feff}😀\r\n", "a\r\n中\n😀\r"] {
        let index = LineIndex::new(text);
        let positions = coordinates(text.len() + 1);
        for &line in &positions {
            for &column in &positions {
                contract::check_bytes(text, &index, column, line, column);
                contract::check_utf16_sound(text, &index, line, column);
                assert_eq!(
                    index.position_to_byte_utf16(text, line, column),
                    contract::utf16_at(text, line, column)
                );
            }
        }
        for &byte in &positions {
            contract::check_utf16_complete(text, &index, byte);
        }
    }
}

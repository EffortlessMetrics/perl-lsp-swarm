#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
#![deny(clippy::map_err_ignore)] // Cohort C0 activation (#12598): census-clean on all targets; new findings move the crate to C1.
use perl_token::TokenSpan;
use std::error::Error;

#[test]
fn contains_uses_half_open_range_semantics() -> Result<(), Box<dyn Error>> {
    let span = TokenSpan::try_new(3, 7).expect("ordered span");

    assert!(!span.contains(2));
    assert!(span.contains(3));
    assert!(span.contains(6));
    assert!(!span.contains(7));

    Ok(())
}

#[test]
fn touches_includes_boundaries_for_cursor_resolution() -> Result<(), Box<dyn Error>> {
    let span = TokenSpan::try_new(3, 7).expect("ordered span");

    assert!(!span.touches(2));
    assert!(span.touches(3));
    assert!(span.touches(7));
    assert!(!span.touches(8));

    let empty = TokenSpan::try_new(5, 5).expect("ordered span");
    assert!(!empty.contains(5));
    assert!(empty.touches(5));

    Ok(())
}

#[test]
fn overlaps_distinguishes_overlap_from_adjacency() -> Result<(), Box<dyn Error>> {
    let span = TokenSpan::try_new(10, 20).expect("ordered span");

    assert!(span.overlaps(TokenSpan::try_new(15, 25).expect("ordered span")));
    assert!(span.overlaps(TokenSpan::try_new(5, 11).expect("ordered span")));
    assert!(!span.overlaps(TokenSpan::try_new(20, 30).expect("ordered span")));
    assert!(!span.overlaps(TokenSpan::try_new(0, 10).expect("ordered span")));
    assert!(!span.overlaps(TokenSpan::try_new(12, 12).expect("ordered span")));

    Ok(())
}

#[test]
fn cover_returns_smallest_span_covering_both_inputs() -> Result<(), Box<dyn Error>> {
    let left = TokenSpan::try_new(8, 12).expect("ordered span");
    let right = TokenSpan::try_new(2, 20).expect("ordered span");

    assert_eq!(left.cover(right), TokenSpan::try_new(2, 20).expect("ordered span"));
    assert_eq!(right.cover(left), TokenSpan::try_new(2, 20).expect("ordered span"));

    Ok(())
}

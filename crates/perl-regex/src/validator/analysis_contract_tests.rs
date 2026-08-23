use super::analysis::RegexRange;

#[test]
fn regex_range_new_accepts_equal_and_ordered_bounds() {
    assert_eq!(RegexRange::new(3, 3), Some(RegexRange { start: 3, end: 3 }));
    assert_eq!(RegexRange::new(2, 5), Some(RegexRange { start: 2, end: 5 }));
    assert_eq!(RegexRange::new(5, 2), None);
}

#[test]
fn anchored_range_preserves_present_and_rejects_absent_ranges() {
    assert_eq!(RegexRange::anchored(2, 3, 5), Some(RegexRange { start: 2, end: 5 }));
    assert_eq!(RegexRange::anchored(6, 0, 5), None);
    assert_eq!(RegexRange::anchored(4, 2, 5), None);
    assert_eq!(RegexRange::anchored(usize::MAX, 1, usize::MAX), None);
}

#[test]
fn half_open_range_overlap_is_exact() {
    let left = RegexRange { start: 2, end: 5 };
    assert!(left.contains(2));
    assert!(left.contains(4));
    assert!(!left.contains(5));
    assert!(left.overlaps(RegexRange { start: 4, end: 8 }));
    assert!(!left.overlaps(RegexRange { start: 5, end: 8 }));
}

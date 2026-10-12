#[test]
fn activating_package_is_not_dropped() {
    assert!(cohort_a::activated());
}

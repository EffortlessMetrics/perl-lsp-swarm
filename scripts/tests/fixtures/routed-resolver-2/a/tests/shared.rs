#[test]
fn dev_feature_and_package_cwd_are_preserved() {
    assert!(cohort_a::activated());
    assert_eq!(
        std::env::current_dir().ok().as_deref(),
        Some(std::path::Path::new(env!("CARGO_MANIFEST_DIR")))
    );
}
#[test]
#[ignore]
fn ignored_inventory_is_preserved() {}

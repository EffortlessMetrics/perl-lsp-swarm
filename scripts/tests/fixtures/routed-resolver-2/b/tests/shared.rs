#[test]
fn activating_package_is_not_dropped() {
    assert!(cohort_a::activated());
}

#[test]
fn disabled_bin_runtime_environment_is_still_present() {
    let executable = std::env::var("CARGO_BIN_EXE_unbuilt_probe");
    assert!(executable.as_deref().is_ok_and(
        |path| path.ends_with("/debug/unbuilt_probe") && !path.starts_with("placeholder:")
    ));
}

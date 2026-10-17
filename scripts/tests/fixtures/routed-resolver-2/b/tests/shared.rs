#[test]
fn activating_package_is_not_dropped() {
    assert!(cohort_a::activated());
}

#[test]
fn feature_disabled_bins_still_have_cargo_runtime_paths() {
    for name in ["enabled_probe", "unbuilt_probe"] {
        let key = format!("CARGO_BIN_EXE_{name}");
        let path = std::env::var(&key).unwrap_or_else(|error| panic!("{key}: {error}"));
        assert!(std::path::Path::new(&path).is_absolute(), "{key} must be absolute");
        if name == "unbuilt_probe" || !cfg!(feature = "enabled-bin") {
            assert!(!std::path::Path::new(&path).exists(), "disabled {name} is not built");
        } else {
            assert!(std::path::Path::new(&path).exists(), "enabled {name} is built");
        }
    }
}

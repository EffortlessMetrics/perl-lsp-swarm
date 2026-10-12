#[test]
fn enabled_and_disabled_binary_environment_matches_cargo_paths() {
    let current = std::env::current_exe().expect("integration test executable");
    let parent = current.parent().expect("integration test parent");
    let profile_dir = if parent.file_name().is_some_and(|name| name == "deps") {
        parent.parent().expect("Cargo profile directory")
    } else {
        parent
    };
    for name in ["enabled_probe", "unbuilt_probe"] {
        let key = format!("CARGO_BIN_EXE_{name}");
        let compiled = match name {
            "enabled_probe" => env!("CARGO_BIN_EXE_enabled_probe"),
            _ => env!("CARGO_BIN_EXE_unbuilt_probe"),
        };
        let runtime = std::env::var(&key).unwrap_or_else(|error| panic!("{key}: {error}"));
        assert_eq!(runtime, compiled, "{key} compile/run path differs");
        assert_eq!(std::path::Path::new(&runtime), profile_dir.join(name), "{key} Cargo profile path differs");
        assert!(std::path::Path::new(&runtime).is_absolute(), "{key} must be absolute");
        assert_eq!(std::path::Path::new(&runtime).exists(), name == "enabled_probe");
    }
}

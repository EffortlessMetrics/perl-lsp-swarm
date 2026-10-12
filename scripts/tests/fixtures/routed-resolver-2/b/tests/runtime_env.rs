#[test]
fn enabled_and_disabled_binary_environment_matches_cargo_paths() {
    for name in ["enabled_probe", "unbuilt_probe"] {
        let key = format!("CARGO_BIN_EXE_{name}");
        let compiled = match name {
            "enabled_probe" => env!("CARGO_BIN_EXE_enabled_probe"),
            _ => env!("CARGO_BIN_EXE_unbuilt_probe"),
        };
        let runtime = std::env::var(&key).unwrap_or_else(|error| panic!("{key}: {error}"));
        assert_eq!(runtime, compiled, "{key} compile/run path differs");
        assert!(std::path::Path::new(&runtime).is_absolute(), "{key} must be absolute");
        assert_eq!(std::path::Path::new(&runtime).exists(), name == "enabled_probe");
    }
}

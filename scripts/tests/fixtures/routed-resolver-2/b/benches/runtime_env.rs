const ENABLED_COMPILE_PATH: &str = env!("CARGO_BIN_EXE_enabled_probe");
const DISABLED_COMPILE_PATH: &str = env!("CARGO_BIN_EXE_unbuilt_probe");

fn main() {
    for name in ["enabled_probe", "unbuilt_probe"] {
        let key = format!("CARGO_BIN_EXE_{name}");
        let path = std::env::var(&key).unwrap_or_else(|error| panic!("{key}: {error}"));
        let compiled = if name == "enabled_probe" { ENABLED_COMPILE_PATH } else { DISABLED_COMPILE_PATH };
        assert_eq!(path, compiled, "{key} compile/run path differs");
        assert!(std::path::Path::new(&path).is_absolute(), "{key} must be absolute");
        assert_eq!(std::path::Path::new(&path).exists(), name == "enabled_probe");
    }
}

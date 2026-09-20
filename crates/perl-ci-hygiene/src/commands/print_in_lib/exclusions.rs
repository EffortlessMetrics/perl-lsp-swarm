use std::ffi::OsStr;
use std::path::Path;

/// Returns `true` for paths that should be skipped by the print-in-lib check.
///
/// This is a superset of `is_excluded_test_path` with extra exclusions specific
/// to the print-macro policy:
///   - `build.rs` files, and the files a `build.rs` splices in with `include!`:
///     Cargo build scripts use `println!("cargo:...")` to communicate with Cargo
///     itself. This is the standard mechanism; it is not "library output".
///   - Files whose name starts with `test_` (e.g. `test_parser.rs`): these are test
///     driver / helper files that live alongside library source but are only invoked
///     during test runs.
///   - Test-support crates whose primary purpose is emitting diagnostic output during
///     test execution (e.g. `perl-lsp-ux-tests`).
pub(super) fn is_excluded_for_print_check(path: &Path) -> bool {
    let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

    // Build scripts may use println!("cargo:...") — standard Cargo convention.
    if file_name == "build.rs" {
        return true;
    }

    // ...and so may a file the build script `include!`s. `crates/perl-dap/build.rs`
    // is `include!("build_catalog.rs")`, which makes `build_catalog.rs` build-script
    // source under a different file name. Matching the name alone scanned it as
    // library source and reported its one `eprintln!` as an offender.
    if is_build_script_include(path) {
        return true;
    }

    // Test-driver files next to src/ but not inside tests/ directory.
    if file_name.starts_with("test_") && file_name.ends_with(".rs") {
        return true;
    }

    // Test-support and UX-test crates use print output intentionally.
    if path.components().any(|c| c.as_os_str() == OsStr::new("perl-lsp-ux-tests")) {
        return true;
    }

    false
}

/// Returns `true` when `path` is spliced into its crate's build script by `include!`.
///
/// Resolution matches rustc's: an `include!` path is relative to the directory
/// holding the file that includes it, which for a crate's build script is the
/// crate root. A computed path (`concat!(env!("OUT_DIR"), …)`) names generated
/// source outside the scanned tree and is not matched, because nothing here
/// would have walked to it in the first place.
fn is_build_script_include(path: &Path) -> bool {
    let Some(crate_root) = path.ancestors().skip(1).find(|dir| dir.join("Cargo.toml").is_file())
    else {
        return false;
    };
    let Ok(source) = std::fs::read_to_string(crate_root.join("build.rs")) else {
        return false;
    };
    source.lines().filter_map(included_path).any(|target| crate_root.join(target) == path)
}

/// Extracts the literal path from an `include!("…")` on one line.
fn included_path(line: &str) -> Option<&str> {
    let rest = line.trim().strip_prefix("include!(\"")?;
    let (target, _) = rest.split_once("\")")?;
    Some(target)
}

#[cfg(test)]
mod tests {
    use super::*;
    use color_eyre::eyre::Result;
    use std::path::PathBuf;

    #[test]
    fn build_rs_is_excluded() {
        assert!(is_excluded_for_print_check(Path::new("crates/my-crate/build.rs")));
    }

    #[test]
    fn test_prefix_rs_file_is_excluded() {
        assert!(is_excluded_for_print_check(Path::new("crates/my-crate/src/test_helpers.rs")));
    }

    #[test]
    fn test_prefix_without_rs_extension_not_excluded() {
        // Only .rs files with test_ prefix are excluded.
        assert!(!is_excluded_for_print_check(Path::new("crates/my-crate/src/test_helpers.txt")));
    }

    #[test]
    fn ux_tests_crate_path_is_excluded() {
        let path: PathBuf = ["crates", "perl-lsp-ux-tests", "src", "lib.rs"].iter().collect();
        assert!(is_excluded_for_print_check(&path));
    }

    #[test]
    fn normal_lib_rs_is_not_excluded() {
        assert!(!is_excluded_for_print_check(Path::new("crates/perl-parser/src/lib.rs")));
    }

    /// A crate root laid out on disk, removed when the test ends.
    ///
    /// Setup errors propagate rather than being discarded. A fixture whose
    /// `write` silently failed still runs its assertion, and passes or fails
    /// on a tree that was never built -- which is the same "the control
    /// observed the wrong condition" defect this module exists to catch.
    struct CrateRoot {
        path: PathBuf,
    }

    impl CrateRoot {
        /// The directory is unique per label, so parallel tests do not collide.
        fn new(label: &str, build_rs: Option<&str>) -> Result<Self> {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|since| since.as_nanos())
                .unwrap_or_default();
            let path = std::env::temp_dir().join(format!("print-in-lib-{label}-{nanos}"));
            std::fs::create_dir_all(&path)?;
            std::fs::write(path.join("Cargo.toml"), "[package]\nname = \"probe\"\n")?;
            if let Some(source) = build_rs {
                std::fs::write(path.join("build.rs"), source)?;
            }
            Ok(Self { path })
        }

        /// Writes a file under the crate root and returns its path.
        fn write(&self, name: &str, contents: &str) -> Result<PathBuf> {
            let file = self.path.join(name);
            if let Some(parent) = file.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&file, contents)?;
            Ok(file)
        }
    }

    impl Drop for CrateRoot {
        fn drop(&mut self) {
            // Cleanup is best-effort by necessity: a destructor has nowhere to
            // report to. Every path is unique per run, so a leftover directory
            // cannot make a later run observe the wrong condition.
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn a_file_the_build_script_includes_is_excluded() -> Result<()> {
        // `crates/perl-dap/build.rs` is `include!("build_catalog.rs")`, which makes
        // `build_catalog.rs` build-script source under a different file name. The
        // `build.rs` name check alone scanned it as library source.
        let root =
            CrateRoot::new("included", Some("fn main() {\n    include!(\"catalog.rs\");\n}\n"))?;
        let catalog = root.write("catalog.rs", "// spliced into the build script\n")?;
        assert!(is_excluded_for_print_check(&catalog));
        Ok(())
    }

    #[test]
    fn a_sibling_the_build_script_does_not_include_is_not_excluded() -> Result<()> {
        let root =
            CrateRoot::new("sibling", Some("fn main() {\n    include!(\"catalog.rs\");\n}\n"))?;
        let other = root.write("other.rs", "// ordinary source\n")?;
        assert!(!is_excluded_for_print_check(&other));
        Ok(())
    }

    #[test]
    fn a_crate_without_a_build_script_excludes_nothing() -> Result<()> {
        let root = CrateRoot::new("nobuild", None)?;
        let catalog = root.write("catalog.rs", "// ordinary source\n")?;
        assert!(!is_excluded_for_print_check(&catalog));
        Ok(())
    }

    #[test]
    fn a_computed_include_path_is_not_invented() -> Result<()> {
        // `concat!(env!("OUT_DIR"), …)` names generated source outside the scanned
        // tree, so nothing would have walked to it to be excluded in the first place.
        let root = CrateRoot::new(
            "computed",
            Some("fn main() {\n    include!(concat!(env!(\"OUT_DIR\"), \"/gen.rs\"));\n}\n"),
        )?;
        let generated = root.write("gen.rs", "// generated\n")?;
        assert!(!is_excluded_for_print_check(&generated));
        Ok(())
    }

    #[test]
    fn file_starting_with_test_but_not_rs_extension_is_not_excluded() {
        // test_helpers.toml should NOT be excluded.
        assert!(!is_excluded_for_print_check(Path::new("crates/my-crate/src/test_helpers.toml")));
    }
}

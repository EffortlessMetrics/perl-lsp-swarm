#![allow(clippy::expect_used, dead_code)]

use std::fs;
use std::path::Path;
use tempfile::TempDir;

pub fn fixture_root() -> TempDir {
    let temp = tempfile::tempdir().expect("tempdir");
    write_policy(temp.path());
    write_package(
        temp.path(),
        "demo",
        r#"
pub fn ready() -> Option<u8> { Some(1) }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registered() {
        let _ = ready();
    }
}
"#,
        &[("known.rs", known_test_file())],
    );
    temp
}

pub fn write_policy(root: &Path) {
    fs::create_dir_all(root.join("policy/clippy-lints.d")).expect("policy dir");
    fs::write(
        root.join("policy/clippy-lints.toml"),
        r#"schema = 2
msrv = "1.95"

[policy]
panic_free_tests = true
allow_test_carveouts = false
suppression_style = "expect-with-reason"
blanket_categories = false
"#,
    )
    .expect("ledger");
    fs::write(
        root.join("policy/clippy-lints.d/00-active.toml"),
        r#"schema = 1

[[lint]]
name = "clippy::panic"
level = "deny"
status = "active"
class = "panic"
reason = "direct panic"

[[lint]]
name = "clippy::unwrap_used"
level = "deny"
status = "active"
class = "panic"
reason = "unwrap"

[[lint]]
name = "clippy::expect_used"
level = "deny"
status = "active"
class = "panic"
reason = "expect"
"#,
    )
    .expect("catalog");
}

pub fn write_package(root: &Path, name: &str, lib: &str, tests: &[(&str, &str)]) {
    fs::write(
        root.join("Cargo.toml"),
        format!("[workspace]\nmembers = [\"crates/{name}\"]\nresolver = \"2\"\n"),
    )
    .expect("workspace manifest");
    let crate_root = root.join("crates").join(name);
    fs::create_dir_all(crate_root.join("src")).expect("src");
    fs::create_dir_all(crate_root.join("tests")).expect("tests");
    fs::write(
        crate_root.join("Cargo.toml"),
        format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n"),
    )
    .expect("package manifest");
    fs::write(crate_root.join("src/lib.rs"), lib).expect("lib");
    for (file, contents) in tests {
        fs::write(crate_root.join("tests").join(file), contents).expect("test file");
    }
}

pub fn write_registry(root: &Path, body: &str) {
    fs::create_dir_all(root.join("ci")).expect("ci");
    fs::write(root.join("ci/panic_test_identities.json"), body).expect("registry");
}

pub fn known_test_file() -> &'static str {
    r#"
#[test]
fn known_panic() {
    panic!("known");
}
"#
}

pub fn active_panic_registry() -> String {
    serde_json::json!({
        "schema_version": 1,
        "sites": [{
            "path": "crates/demo/tests/known.rs",
            "enclosing_test_or_function": "known_panic",
            "macro_family": "panic!",
            "normalized_snippet": "panic!",
            "selector_identity": "placeholder",
            "accepted_reason": "Intentional test failure diagnostic.",
            "state": "active"
        }]
    })
    .to_string()
}

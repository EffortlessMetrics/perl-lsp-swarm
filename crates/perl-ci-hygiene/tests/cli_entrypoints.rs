//! Both CLI entry points remain independently selectable after resolver extraction.
use std::path::Path;
use std::process::Command;

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn cargo_keeps_hygiene_as_the_default_binary() -> TestResult {
    let output = Command::new(env!("CARGO"))
        .args(["metadata", "--no-deps", "--format-version", "1", "--offline"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()?;
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let metadata: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    let package = metadata["packages"]
        .as_array()
        .and_then(|packages| packages.iter().find(|package| package["name"] == "perl-ci-hygiene"))
        .ok_or("missing hygiene package")?;
    assert_eq!(package["default_run"], "perl-ci-hygiene");
    for name in ["perl-ci-hygiene", "resolve-package-name"] {
        assert!(
            package["targets"].as_array().ok_or("missing targets")?.iter().any(|target| {
                target["name"] == name
                    && target["kind"]
                        .as_array()
                        .is_some_and(|kinds| kinds.iter().any(|kind| kind == "bin"))
            }),
            "missing binary {name}"
        );
    }
    Ok(())
}

#[test]
fn hygiene_help_still_exposes_the_existing_commands() -> TestResult {
    let output = Command::new(env!("CARGO_BIN_EXE_perl-ci-hygiene")).arg("--help").output()?;
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert!(String::from_utf8(output.stdout)?.contains("check-version-sync"));
    Ok(())
}

#[test]
fn resolver_binary_maps_a_real_workspace_package_and_rejects_an_unknown_one() -> TestResult {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for (directory, expected) in
        [("crates/perl-ci-hygiene", true), ("crates/does-not-exist-19337", false)]
    {
        let output = Command::new(env!("CARGO_BIN_EXE_resolve-package-name"))
            .current_dir(&root)
            .arg(directory)
            .output()?;
        assert_eq!(
            output.status.success(),
            expected,
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        if expected {
            assert_eq!(String::from_utf8(output.stdout)?.trim(), "perl-ci-hygiene");
        } else {
            assert!(output.stdout.is_empty(), "failed resolution must not print a package");
        }
    }
    Ok(())
}

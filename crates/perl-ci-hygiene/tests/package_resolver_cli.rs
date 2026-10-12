use color_eyre::eyre::{Context, Result};
use std::fs;
use std::process::Command;

#[test]
fn package_identity_uses_metadata_and_refuses_unknown_or_missing_subjects() -> Result<()> {
    let fixture = tempfile::Builder::new().prefix("resolver fixture ").tempdir()?;
    let root = fixture.path();
    fs::create_dir_all(root.join("crates/directory-name/src"))?;
    fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nmembers = [\"crates/directory-name\"]\nresolver = \"2\"\n",
    )?;
    fs::write(
        root.join("crates/directory-name/Cargo.toml"),
        "[package]\nname = \"actual-package-name\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )?;
    fs::write(root.join("crates/directory-name/src/lib.rs"), "")?;
    let binary = env!("CARGO_BIN_EXE_perl-ci-hygiene");
    for spelling in ["crates/directory-name", "crates\\directory-name\\"] {
        let output = Command::new(binary)
            .current_dir(root)
            .args(["resolve-package-name", spelling])
            .output()
            .context("failed to run package resolver")?;
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        assert_eq!(String::from_utf8_lossy(&output.stdout), "actual-package-name\n");
    }
    let unknown = Command::new(binary)
        .current_dir(root)
        .args(["resolve-package-name", "crates/unknown"])
        .output()?;
    assert!(!unknown.status.success());
    assert!(unknown.stdout.is_empty(), "unknown identity must not publish a package name");
    fs::remove_file(root.join("Cargo.toml"))?;
    let missing = Command::new(binary)
        .current_dir(root)
        .args(["resolve-package-name", "crates/directory-name"])
        .output()?;
    assert!(!missing.status.success());
    assert!(missing.stdout.is_empty());
    Ok(())
}

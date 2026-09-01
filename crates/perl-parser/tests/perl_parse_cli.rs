#![cfg(feature = "cli")]

use std::fs;
use std::process::Command;

#[test]
fn continued_read_failure_is_counted_and_reported() -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let valid = temp.path().join("valid.pl");
    let missing = temp.path().join("missing.pl");
    fs::write(&valid, "use strict;\nmy $value = 1;\n")?;

    let output = Command::new(env!("CARGO_BIN_EXE_perl-parse"))
        .arg("--continue")
        .arg("--stats")
        .arg("--quiet")
        .arg(&missing)
        .arg(&valid)
        .output()?;

    assert!(!output.status.success(), "continued read failure must keep a nonzero exit status");

    let stderr = String::from_utf8(output.stderr)?;
    assert!(stderr.contains("Files parsed: 1"), "valid input was not counted: {stderr}");
    assert!(stderr.contains("Files failed: 1"), "read failure was not counted: {stderr}");
    assert!(
        stderr.contains(&format!("{}: FAILED", missing.display())),
        "failed input was omitted from file details: {stderr}"
    );
    assert!(
        stderr.contains(&format!("Error reading {}:", missing.display())),
        "read diagnostic lost the failed input identity: {stderr}"
    );

    Ok(())
}

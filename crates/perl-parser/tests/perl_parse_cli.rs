#![cfg(feature = "cli")]

use std::fs;
use std::process::Command;

#[test]
fn continued_read_failure_is_counted_and_reported() -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let valid_before = temp.path().join("valid-before.pl");
    let valid_after = temp.path().join("valid-after.pl");
    let missing = temp.path().join("missing.pl");
    fs::write(&valid_before, "use strict;\nmy $value = 1;\n")?;
    fs::write(&valid_after, "use strict;\nmy $value = 2;\n")?;

    let output = Command::new(env!("CARGO_BIN_EXE_perl-parse"))
        .arg("--continue")
        .arg("--stats")
        .arg("--quiet")
        .arg(&valid_before)
        .arg(&missing)
        .arg(&valid_after)
        .output()?;

    assert!(
        !output.status.success(),
        "continued read failure must keep a nonzero exit status; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8(output.stderr)?;
    assert!(
        stderr.contains("Files parsed: 2"),
        "successful inputs were not both counted: {stderr}"
    );
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

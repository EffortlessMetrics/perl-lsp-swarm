use std::fs;
use std::process::Command;

fn perl_parse_command() -> Command {
    if let Some(path) = option_env!("CARGO_BIN_EXE_perl-parse") {
        return Command::new(path);
    }

    let mut command = Command::new("cargo");
    command
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .args([
            "run",
            "--quiet",
            "-p",
            "perl-parser",
            "--features",
            "cli",
            "--bin",
            "perl-parse",
            "--",
        ]);
    command
}

#[test]
fn continued_read_failure_is_counted_and_reported() -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let valid = temp.path().join("valid.pl");
    let missing = temp.path().join("missing.pl");
    fs::write(&valid, "use strict;\nmy $value = 1;\n")?;

    let output = perl_parse_command()
        .arg("--continue")
        .arg("--stats")
        .arg("--quiet")
        .arg(&valid)
        .arg(&missing)
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

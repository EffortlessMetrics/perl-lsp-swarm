//! Pin the receipt CLI's exact stdout contract: `run` itself must stay silent
//! (#16906) and the binary must emit exactly the `Display` presentation the
//! caller owes — nothing more on stdout, nothing on stderr.
//!
//! The in-lib test only observes `run`'s return value, so a future print
//! inside `run` would stay green there. This test executes the real binary
//! and reads its pipes, which fails if `run` ever prints.

use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::Command;

type TestResult<T = ()> = Result<T, Box<dyn Error>>;

fn case_dir(case: &str) -> PathBuf {
    std::env::temp_dir().join(format!("ux-receipt-stdout-contract-{case}-{}", std::process::id()))
}

fn write_input(dir: &Path) -> TestResult<(PathBuf, PathBuf)> {
    let _ = std::fs::remove_dir_all(dir);
    std::fs::create_dir_all(dir)?;
    let input = dir.join("ux.log");
    std::fs::write(&input, "test result: ok. 1 passed; 0 failed\n")?;
    let status = dir.join("exit-status");
    std::fs::write(&status, "0\n")?;
    Ok((input, status))
}

fn bin_stdout(args: &[String]) -> TestResult<std::process::Output> {
    // NOTE: dashes preserved — cargo names the variable after the `[[bin]]`
    // target `ux-regression-receipt` verbatim (see the `perl-ci-hygiene`
    // precedent: `CARGO_BIN_EXE_perl-ci-hygiene`).
    let output = Command::new(env!("CARGO_BIN_EXE_ux-regression-receipt")).args(args).output()?;
    if !output.status.success() {
        return Err(format!(
            "receipt binary failed: {args:?}\n{}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    Ok(output)
}

#[test]
fn written_receipt_emits_exact_stdout_line() -> TestResult {
    let dir = case_dir("written");
    let (input, status) = write_input(&dir)?;
    let receipt = dir.join("out").join("receipt.json");
    let args = [
        "--input".to_string(),
        input.display().to_string(),
        "--receipt".to_string(),
        receipt.display().to_string(),
        "--sha".to_string(),
        "abc123".to_string(),
        "--exit-status-file".to_string(),
        status.display().to_string(),
    ];
    let output = bin_stdout(&args)?;
    assert_eq!(
        String::from_utf8(output.stdout)?,
        format!("Wrote UX regression receipt: {}\n", receipt.display()),
        "binary stdout must be exactly the Written presentation line"
    );
    assert!(
        output.stderr.is_empty(),
        "binary must stay silent on stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let written = std::fs::read_to_string(&receipt)?;
    assert!(written.contains("ux_regression_receipt"), "receipt file should hold the receipt JSON");
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn payload_receipt_emits_receipt_json_on_stdout() -> TestResult {
    let dir = case_dir("payload");
    let (input, status) = write_input(&dir)?;
    let args = [
        "--input".to_string(),
        input.display().to_string(),
        "--sha".to_string(),
        "abc123".to_string(),
        "--exit-status-file".to_string(),
        status.display().to_string(),
    ];
    let output = bin_stdout(&args)?;
    let stdout = String::from_utf8(output.stdout)?;
    assert!(stdout.contains("ux_regression_receipt"), "payload stdout must carry the receipt JSON");
    assert!(
        output.stderr.is_empty(),
        "binary must stay silent on stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

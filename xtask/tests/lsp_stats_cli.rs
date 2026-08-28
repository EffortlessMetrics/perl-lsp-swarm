use assert_cmd::cargo::cargo_bin_cmd;
use std::error::Error;
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

type TestResult<T = ()> = Result<T, Box<dyn Error>>;

struct OutputRestore {
    path: PathBuf,
    original: Option<Vec<u8>>,
}

impl Drop for OutputRestore {
    fn drop(&mut self) {
        match &self.original {
            Some(original) => {
                let _ = fs::write(&self.path, original);
            }
            None => {
                let _ = fs::remove_file(&self.path);
            }
        }
    }
}

fn workspace_root() -> TestResult<PathBuf> {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(PathBuf::from)
        .ok_or_else(|| "xtask manifest must have a workspace parent".into())
}

#[test]
fn invalid_timing_receipt_fails_publicly_without_overwriting_output() -> TestResult {
    let root = workspace_root()?;
    let output_path = root.join(".ci/metrics/editor_ux.json");
    let restore =
        OutputRestore { path: output_path.clone(), original: fs::read(&output_path).ok() };
    let sentinel = br#"{"sentinel":"preserve-me"}
"#;
    fs::write(&output_path, sentinel)?;

    let receipts = TempDir::new()?;
    fs::write(
        receipts.path().join("malformed-timing.json"),
        br#"{
            "result": "pass",
            "duration_ms": 10.0,
            "time_to_first_useful_result_ms": "not-a-number"
        }"#,
    )?;

    let output = cargo_bin_cmd!("xtask")
        .args([
            "metrics",
            "lsp-stats",
            "--json",
            "--receipt-dir",
            receipts.path().to_str().ok_or("receipt directory path is not valid UTF-8")?,
        ])
        .output()?;
    let preserved_output = fs::read(&output_path)?;

    assert!(
        !output.status.success(),
        "malformed timing receipt unexpectedly succeeded\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(preserved_output, sentinel, "invalid input overwrote the output artifact");
    drop(restore);
    Ok(())
}

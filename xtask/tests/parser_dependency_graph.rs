//! Run the parser dependency proof through the existing routed xtask test suite.

use std::{path::Path, process::Command};

#[test]
fn parser_tools_follow_the_existing_feature_boundary() -> Result<(), Box<dyn std::error::Error>> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let python = if cfg!(windows) { "python" } else { "python3" };
    let output = Command::new(python)
        .arg(root.join("scripts/tests/test_xtask_parser_dependencies.py"))
        .current_dir(&root)
        .output()?;
    assert!(
        output.status.success(),
        "Cargo graph proof failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}

//! Run parser dependency and helper-target proofs through the routed xtask suite.

use std::{path::Path, process::Command};

#[test]
fn parser_tools_follow_the_existing_feature_boundary() -> Result<(), Box<dyn std::error::Error>> {
    run_proof("test_xtask_parser_dependencies.py")
}

#[test]
fn parser_property_helper_is_not_a_test_target() -> Result<(), Box<dyn std::error::Error>> {
    run_proof("test_parser_property_helper_layout.py")
}

fn run_proof(script: &str) -> Result<(), Box<dyn std::error::Error>> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let python = if cfg!(windows) { "python" } else { "python3" };
    let output = Command::new(python)
        .arg(root.join("scripts/tests").join(script))
        .current_dir(&root)
        .output()?;
    assert!(
        output.status.success(),
        "{script} failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}

//! Drive the real candidate xtask over disposable Git integration subjects.
#[cfg(unix)]
#[test]
fn pr_comparison_subject_controls() -> Result<(), Box<dyn std::error::Error>> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("xtask manifest must have a repository parent")?;
    let result = std::process::Command::new("python3")
        .arg(root.join("scripts/ci/ripr_pr_comparison_controls.py"))
        .arg(env!("CARGO_BIN_EXE_xtask"))
        .current_dir(root)
        .output()?;
    if !result.status.success() {
        return Err(format!(
            "native comparison controls failed:\n{}\n{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        )
        .into());
    }
    Ok(())
}

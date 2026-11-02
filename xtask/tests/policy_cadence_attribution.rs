//! CLI smoke for `cargo xtask policy attribution` (#7061 A1).

use color_eyre::eyre::Result;
use std::process::Command;

#[test]
fn policy_attribution_emits_deterministic_json_for_current_tree() -> Result<()> {
    let root = xtask::utils::project_root()?;
    let json = root.join("target/test-policy-attribution.json");
    let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .current_dir(&root)
        .args([
            "policy",
            "attribution",
            "--as-of",
            "2026-08-12",
            "--json",
            json.to_str().expect("utf-8 path"),
        ])
        .output()?;
    assert!(
        output.status.success(),
        "policy attribution failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = std::fs::read_to_string(&json)?;
    let receipt: serde_json::Value = serde_json::from_str(&text)?;
    assert_eq!(receipt["schema_version"], "policy_attribution.v1");
    assert_eq!(receipt["as_of"], "2026-08-12");
    Ok(())
}

//! CLI integration tests for `cargo xtask source-context` (#7300 / #13993).

#![expect(clippy::expect_used, reason = "test harness asserts on fixture setup")]

use assert_cmd::Command;
#[test]
fn committed_inventory_reconciles_and_projection_is_current() {
    let output = Command::cargo_bin("xtask")
        .expect("xtask binary")
        .args(["source-context", "inventory", "--check"])
        .output()
        .expect("run inventory --check");
    assert!(
        output.status.success(),
        "inventory --check failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn explain_resolves_site_id() {
    let output = Command::cargo_bin("xtask")
        .expect("xtask binary")
        .args([
            "source-context",
            "explain",
            "completion-lexical-is-in-string",
        ])
        .output()
        .expect("run explain");
    assert!(output.status.success(), "explain failed");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("completion-lexical-is-in-string"));
    assert!(stdout.contains("#13244"));
}

//! Live-tree and fixture tests for the Tree-sitter scanner authority gate (#7261).

use std::fs;

use assert_cmd::Command as AssertCommand;
use color_eyre::eyre::{Result, eyre};
use tempfile::TempDir;
use xtask::tree_sitter_scanner::{self, ViolationCode};
use xtask::utils::project_root;

type TestResult = Result<()>;

#[test]
fn live_tree_scanner_authority_gate_is_current() -> TestResult {
    let root = project_root()?;
    let violations = tree_sitter_scanner::check(&root);
    if !violations.is_empty() {
        let lines: Vec<_> = violations.iter().map(|v| v.display_line()).collect();
        return Err(eyre!("scanner authority drift on main tree:\n{}", lines.join("\n")));
    }
    assert!(
        tree_sitter_scanner::archived_copies_do_not_satisfy_gate(&root)
            .map_err(|v| eyre!(v.display_line()))?
    );
    Ok(())
}

#[test]
fn xtask_tree_sitter_scanner_check_cli_succeeds_on_repo() -> TestResult {
    AssertCommand::cargo_bin("xtask")?.args(["tree-sitter-scanner", "check"]).assert().success();
    Ok(())
}

#[test]
fn direct_projection_edit_fails_closed() -> TestResult {
    let dir = TempDir::new()?;
    let fixture = dir.path();

    fs::create_dir_all(fixture.join("grammar"))?;
    fs::create_dir_all(fixture.join("crate"))?;
    fs::write(fixture.join("grammar/scanner.c"), b"authority\n")?;
    fs::write(fixture.join("crate/scanner.c"), b"authority\n")?;
    fs::create_dir_all(fixture.join("policy"))?;

    let hash = tree_sitter_scanner::content_sha256(b"authority\n");
    let ledger = format!(
        r#"
schema_version = "tree_sitter_scanner_authority.v1"
controlling_issue = 7261
sync_command = "cargo xtask tree-sitter-scanner sync"
check_command = "cargo xtask tree-sitter-scanner check"
generator = "fixture"

[authority]
path = "grammar/scanner.c"
role = "authoritative"
content_sha256 = "{hash}"
note = "fixture"

[[projection]]
path = "crate/scanner.c"
role = "active_projection"
authority = "grammar/scanner.c"
consumer = "fixture"
note = "fixture"
"#
    );
    fs::write(fixture.join("policy/tree-sitter-scanner-authority.toml"), ledger)?;

    assert!(tree_sitter_scanner::check(fixture).is_empty());

    fs::write(fixture.join("crate/scanner.c"), b"manual-edit\n")?;
    let violations = tree_sitter_scanner::check(fixture);
    assert!(violations.iter().any(|v| v.code == ViolationCode::ProjectionDrift));
    Ok(())
}

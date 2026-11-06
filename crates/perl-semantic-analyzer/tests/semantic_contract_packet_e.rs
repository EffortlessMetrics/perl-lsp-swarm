//! Semantic packet E contract discovery (#6599).

use std::path::PathBuf;

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn packet_e_spec_and_routing_policy_exist_at_declared_paths() -> TestResult {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let spec = root.join("docs/specs/PLSP-SPEC-0039-semantic-contract-packet-e.md");
    let routing = root.join("policy/semantic/packet-e-issue-routing.v1.toml");
    assert!(spec.is_file(), "missing {}", spec.display());
    assert!(routing.is_file(), "missing {}", routing.display());
    let spec_text = std::fs::read_to_string(&spec)?;
    assert!(spec_text.contains("Parser-to-fact transitions"));
    let routing_text = std::fs::read_to_string(&routing)?;
    assert!(routing_text.contains("issue = 4736"));
    assert!(routing_text.contains("follow_up = \"compatibility_ledger\""));
    Ok(())
}

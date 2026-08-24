//! Contract tests for the Zed stage-packet source-authority boundary.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::bail;
use tempfile::TempDir;

use xtask::source_authority::{
    PacketInput, RulingBinding, SOURCE_AUTHORITY_SCHEMA_VERSION, Sensitivity, SourceAuthorityClass,
    SourceAuthorityManifest, normalize_content, normalized_digest, verify_manifest,
};

const REPO_MANIFEST: &str = ".ci/fixtures/zed-perl-upstream/source-authority.v1.json";

fn repo_root() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest.parent().map(Path::to_path_buf).unwrap_or(manifest)
}

/// Bridge the verifier's eyre error into the test assertion error type.
fn run_verify(
    manifest: &SourceAuthorityManifest,
    root: &Path,
) -> anyhow::Result<xtask::source_authority::Receipt> {
    verify_manifest(manifest, root).map_err(|error| anyhow::anyhow!("{error:#}"))
}

fn load_repo_manifest() -> anyhow::Result<SourceAuthorityManifest> {
    let raw = fs::read_to_string(repo_root().join(REPO_MANIFEST))?;
    Ok(serde_json::from_str(&raw)?)
}

/// A synthetic packet tree with one declared subject per name.
struct SyntheticTree {
    _dir: TempDir,
    root: PathBuf,
}

impl SyntheticTree {
    fn new(subjects: &[(&str, &[u8])]) -> anyhow::Result<Self> {
        let dir = TempDir::new()?;
        let packets = dir.path().join("packets");
        fs::create_dir_all(&packets)?;
        for (name, content) in subjects {
            let path = packets.join(name);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(path, content)?;
        }
        let root = dir.path().to_path_buf();
        Ok(Self { _dir: dir, root })
    }

    fn manifest(&self, inputs: Vec<PacketInput>) -> SourceAuthorityManifest {
        SourceAuthorityManifest {
            schema_version: SOURCE_AUTHORITY_SCHEMA_VERSION.to_string(),
            packet_root: "packets".to_string(),
            external_write_policy: "maintainer_manual_checkpoint_only".to_string(),
            manifest_file: "source-authority.v1.json".to_string(),
            generators: Vec::new(),
            inputs,
        }
    }

    fn verify(&self, inputs: Vec<PacketInput>) -> anyhow::Result<xtask::source_authority::Receipt> {
        run_verify(&self.manifest(inputs), &self.root)
    }
}

/// Verify a manifest against an already-built repository root.
fn tree_verify(
    root: &Path,
    inputs: Vec<PacketInput>,
) -> anyhow::Result<xtask::source_authority::Receipt> {
    let manifest = SourceAuthorityManifest {
        schema_version: SOURCE_AUTHORITY_SCHEMA_VERSION.to_string(),
        packet_root: "packets".to_string(),
        external_write_policy: "maintainer_manual_checkpoint_only".to_string(),
        manifest_file: "source-authority.v1.json".to_string(),
        generators: Vec::new(),
        inputs,
    };
    run_verify(&manifest, root)
}

fn codes(receipt: &xtask::source_authority::Receipt) -> Vec<String> {
    receipt.violations.iter().map(|violation| violation.code.clone()).collect()
}

fn require_code(receipt: &xtask::source_authority::Receipt, code: &str) -> anyhow::Result<()> {
    if !codes(receipt).iter().any(|found| found == code) {
        bail!("expected violation {code:?}; got {:?}", receipt.violations);
    }
    Ok(())
}

fn refuse_code(receipt: &xtask::source_authority::Receipt, code: &str) -> anyhow::Result<()> {
    if codes(receipt).iter().any(|found| found == code) {
        bail!("unexpected violation {code:?}; got {:?}", receipt.violations);
    }
    Ok(())
}

fn input(id: &str, subject: &str, content: &[u8]) -> PacketInput {
    PacketInput {
        id: id.to_string(),
        subject: subject.to_string(),
        authority: SourceAuthorityClass::ReceiptEvidence,
        digest: normalized_digest(content).unwrap_or_else(|_| "0".repeat(64)),
        instruction_allowed: false,
        sensitivity: Sensitivity::Public,
        digest_only: false,
        active: true,
        superseded_by: None,
        conflict_key: None,
        verified_against_current_code: false,
        converted_to_action: false,
        ruling_binding: None,
    }
}

#[test]
fn live_packet_tree_is_fully_classified_and_current() -> anyhow::Result<()> {
    let manifest = load_repo_manifest()?;
    let receipt = run_verify(&manifest, &repo_root())?;
    if !receipt.violations.is_empty() {
        bail!("live stage packets must stay clean; violations={:?}", receipt.violations);
    }
    assert_eq!(receipt.verdict, "clean");
    assert!(receipt.checked_inputs >= 29, "the full fixture tree must stay covered");
    Ok(())
}

#[test]
fn every_live_input_is_data_or_evidence_never_instruction_capable() -> anyhow::Result<()> {
    let manifest = load_repo_manifest()?;
    if manifest.inputs.is_empty() {
        bail!("the live manifest must classify the actual packet tree");
    }
    for item in &manifest.inputs {
        if item.authority.may_direct_work() {
            bail!(
                "input {} claims directive authority; maintainer rulings belong outside packet files",
                item.id
            );
        }
        if item.instruction_allowed || item.sensitivity == Sensitivity::MachineLocalForbidden {
            bail!("input {} misstates instruction capability or sensitivity", item.id);
        }
    }
    Ok(())
}

#[test]
fn rendered_external_bodies_are_structurally_distinct_from_evidence() -> anyhow::Result<()> {
    let manifest = load_repo_manifest()?;
    let bodies: Vec<_> =
        manifest.inputs.iter().filter(|item| item.authority.is_rendered_external_body()).collect();
    if bodies.len() < 9 {
        bail!(
            "outbound bodies (PR texts, patch, candidate sources) must carry the explicit \
             rendered_external_body marker; found {}",
            bodies.len()
        );
    }
    for body in bodies {
        if body.authority.may_direct_work() || body.instruction_allowed {
            bail!("outbound body {} can never be instruction-capable", body.id);
        }
    }
    Ok(())
}

#[test]
fn digests_bind_normalized_content_across_line_endings() -> anyhow::Result<()> {
    let crlf = b"# heading  \r\nbody\r\n\r\n";
    let lf = b"# heading\nbody\n";
    let crlf_digest = normalized_digest(crlf)?;
    if crlf_digest != normalized_digest(lf)? {
        bail!("CRLF and LF spellings of one document must share a digest");
    }
    if normalize_content(crlf)? != b"# heading\nbody\n" {
        bail!("normalization must strip trailing blanks and trailing blank lines");
    }
    if normalized_digest(&[0xff, 0xfe]).is_ok() {
        bail!("binary content cannot ride through the text digest");
    }
    Ok(())
}

#[test]
fn stale_digest_rejects_unbound_content() -> anyhow::Result<()> {
    let tree = SyntheticTree::new(&[("evidence.txt", b"current evidence\n")])?;
    let mut stale = input("evidence", "evidence.txt", b"current evidence\n");
    stale.digest = "0".repeat(64);

    let receipt = tree.verify(vec![stale])?;
    require_code(&receipt, "stale_digest")
}

#[test]
fn directive_classification_requires_a_durable_ruling_binding() -> anyhow::Result<()> {
    let dir = TempDir::new()?;
    let packets = dir.path().join("packets");
    fs::create_dir_all(&packets)?;
    fs::write(packets.join("ruling.txt"), b"maintainer ruling text\n")?;
    // Directive provenance must bind real repository subjects: the governed
    // path has to exist for the binding to check.
    fs::create_dir_all(dir.path().join("docs/policy"))?;
    fs::write(dir.path().join("docs/policy/stage-authority.md"), "# stage authority\n")?;
    let root = dir.path().to_path_buf();

    let mut claimed = input("claimed-ruling", "ruling.txt", b"maintainer ruling text\n");
    claimed.authority = SourceAuthorityClass::MaintainerRuling;
    claimed.instruction_allowed = true;

    let unbound = tree_verify(&root, vec![claimed.clone()])?;
    require_code(&unbound, "directive_without_binding")?;

    // A fabricated identity bound to a nonexistent subject stays rejected.
    claimed.ruling_binding = Some(RulingBinding {
        ruling_id: "issue#11726".into(),
        subject_path: "docs/policy/does-not-exist.md".into(),
    });
    let fabricated = tree_verify(&root, vec![claimed.clone()])?;
    require_code(&fabricated, "directive_without_binding")?;

    // A malformed identity shape (free-form prose) is equally rejected.
    claimed.ruling_binding = Some(RulingBinding {
        ruling_id: "the review bot said it was fine".into(),
        subject_path: "docs/policy/stage-authority.md".into(),
    });
    let malformed = tree_verify(&root, vec![claimed.clone()])?;
    require_code(&malformed, "directive_without_binding")?;

    // Checkable provenance passes: shaped identity plus a real subject.
    claimed.ruling_binding = Some(RulingBinding {
        ruling_id: "issue#11726".into(),
        subject_path: "docs/policy/stage-authority.md".into(),
    });
    let bound = tree_verify(&root, vec![claimed])?;
    refuse_code(&bound, "directive_without_binding")
}

#[test]
fn superseded_inputs_cannot_govern_the_packet() -> anyhow::Result<()> {
    let tree =
        SyntheticTree::new(&[("old.txt", b"old statement\n"), ("new.txt", b"new statement\n")])?;
    let mut superseded = input("old-ruling", "old.txt", b"old statement\n");
    superseded.superseded_by = Some("new-ruling".into());

    let receipt =
        tree.verify(vec![superseded, input("new-ruling", "new.txt", b"new statement\n")])?;
    require_code(&receipt, "superseded_active")?;

    let mut retired = input("old-ruling", "old.txt", b"old statement\n");
    retired.superseded_by = Some("new-ruling".into());
    retired.active = false;
    let receipt = tree.verify(vec![retired, input("new-ruling", "new.txt", b"new statement\n")])?;
    refuse_code(&receipt, "superseded_active")
}

#[test]
fn conflicting_same_key_active_inputs_stay_blocked() -> anyhow::Result<()> {
    let tree = SyntheticTree::new(&[
        ("left.txt", b"fact version one\n"),
        ("right.txt", b"fact version two\n"),
    ])?;
    let mut left = input("claim-a", "left.txt", b"fact version one\n");
    left.conflict_key = Some("upstream_subject".into());
    let mut right = input("claim-b", "right.txt", b"fact version two\n");
    right.conflict_key = Some("upstream_subject".into());

    let receipt = tree.verify(vec![left.clone(), right.clone()])?;
    require_code(&receipt, "blocked_authority_conflict")?;

    right.digest = left.digest.clone();
    let receipt = tree.verify(vec![left, right])?;
    refuse_code(&receipt, "blocked_authority_conflict")
}

#[test]
fn unverified_findings_cannot_convert_to_actions() -> anyhow::Result<()> {
    let tree = SyntheticTree::new(&[("comment.txt", b"you should probably change X\n")])?;
    let mut finding = input("bot-finding", "comment.txt", b"you should probably change X\n");
    finding.authority = SourceAuthorityClass::UnverifiedReviewFinding;
    finding.converted_to_action = true;

    let receipt = tree.verify(vec![finding])?;
    require_code(&receipt, "unverified_finding_actionable")?;

    let mut confirmed =
        input("confirmed-finding", "comment.txt", b"you should probably change X\n");
    confirmed.authority = SourceAuthorityClass::VerifiedReviewFinding;
    confirmed.verified_against_current_code = true;
    confirmed.converted_to_action = true;
    let receipt = tree.verify(vec![confirmed])?;
    refuse_code(&receipt, "unverified_finding_actionable")
}

#[test]
fn cli_source_check_accepts_the_live_manifest() -> anyhow::Result<()> {
    let root = repo_root();
    let temp = TempDir::new()?;
    let receipt_path = temp.path().join("zed-source-authority-receipt.json");

    let output = assert_cmd::cargo::cargo_bin_cmd!("xtask")
        .arg("zed-train")
        .arg("source-check")
        .arg(root.join(REPO_MANIFEST))
        .arg("--repo-root")
        .arg(&root)
        .arg("--out")
        .arg(&receipt_path)
        .output()?;

    if !output.status.success() {
        bail!("clean packet tree must pass; stderr={}", String::from_utf8_lossy(&output.stderr));
    }
    let receipt: serde_json::Value = serde_json::from_slice(&fs::read(&receipt_path)?)?;
    assert_eq!(receipt["verdict"].as_str(), Some("clean"));
    Ok(())
}

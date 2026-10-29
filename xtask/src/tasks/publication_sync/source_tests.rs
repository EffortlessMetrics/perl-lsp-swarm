//! Staged source shape controls; no producer-backed source pass is implemented.
use super::*;
use serde_json::json;

const SOURCE_FIXTURE: &str =
    include_str!("../../../../fixtures/publication_sync/source_manifest.json");

fn source_value() -> Result<Value> {
    serde_json::from_str(SOURCE_FIXTURE).context("reading the staged source fixture")
}

fn staged_receipt(document: &Value, root: &Path) -> Result<Receipt> {
    let raw = serde_json::to_vec(document)?;
    Ok(build_receipt(&raw, root, resolve_checkout, resolve_tree_entry)
        .unwrap_or_else(|failure| Receipt::unevaluated(failure.manifest_digest, failure.finding)))
}

fn has_only_finding(receipt: &Receipt, code: &str) {
    assert_eq!(receipt.verdict, Verdict::NotProven);
    assert_eq!(receipt.findings.len(), 1);
    assert_eq!(receipt.findings[0].code, code);
}

/// Real Git identities cannot turn schema-valid source bytes into proof.
#[test]
fn real_source_subject_needs_no_release_fields_and_cannot_claim_admission() -> Result<()> {
    let root = tempfile::tempdir()?;
    let git = |args: &[&str]| -> Result<String> {
        let output =
            std::process::Command::new("git").arg("-C").arg(root.path()).args(args).output()?;
        if !output.status.success() {
            bail!("git {args:?}: {}", String::from_utf8_lossy(&output.stderr));
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    };
    git(&["init", "-q"])?;
    git(&["config", "user.name", "source fixture"])?;
    git(&["config", "user.email", "source@example.invalid"])?;
    git(&["read-tree", "--empty"])?;
    let tree = git(&["write-tree"])?;
    let b = git(&["commit-tree", &tree, "-m", "B"])?;
    let r = git(&["commit-tree", &tree, "-p", &b, "-m", "R"])?;
    let s = git(&["commit-tree", &tree, "-p", &b, "-m", "S"])?;
    git(&["update-ref", "refs/heads/main", &s])?;
    git(&["symbolic-ref", "HEAD", "refs/heads/main"])?;
    assert_ne!(r, s);
    let mut document = source_value()?;
    document["reconciliation_base_sha"] = json!(b);
    document["destination_base_sha"] = json!(r);
    document["swarm_source_sha"] = json!(s);
    document["expected_projected_tree"] = json!(tree);
    assert!(document.get("release").is_none());
    assert!(document.get("prepared_swarm_sha").is_none());
    assert!(document.get("invariants").is_none());
    let receipt = staged_receipt(&document, root.path())?;
    has_only_finding(&receipt, "source_profile_not_proven");
    assert!(receipt.release.is_none());
    assert!(receipt.track.is_none());
    assert!(receipt.prepared_swarm_sha.is_none());
    Ok(())
}

#[test]
fn each_source_binding_is_required_exactly_once() -> Result<()> {
    for index in 0..4 {
        let mut missing = source_value()?;
        missing["inputs"].as_array_mut().ok_or_else(|| eyre!("inputs"))?.remove(index);
        has_only_finding(&staged_receipt(&missing, Path::new("."))?, "manifest_schema_violation");
        let mut duplicate = source_value()?;
        duplicate["inputs"][index]["id"] = duplicate["inputs"][(index + 1) % 4]["id"].clone();
        has_only_finding(&staged_receipt(&duplicate, Path::new("."))?, "manifest_schema_violation");
    }
    Ok(())
}

#[test]
fn changing_only_the_release_profile_cannot_escape_its_requirements() -> Result<()> {
    let mut document: Value = serde_json::from_str(include_str!(
        "../../../../fixtures/publication_sync/clean_manifest.json"
    ))?;
    document["schema_version"] = json!(SOURCE_MANIFEST_SCHEMA_VERSION);
    document["profile"] = json!("source");
    has_only_finding(&staged_receipt(&document, Path::new("."))?, "manifest_schema_violation");
    Ok(())
}

#[test]
fn candidate_claims_and_release_authority_cannot_enter_source_shape() -> Result<()> {
    for (key, value) in [
        ("release", json!("0.18.0")),
        ("required_invariants", json!([])),
        ("live_controls", json!({"result":"proven"})),
        ("product_proof", json!({"result":"pass"})),
        ("published_channels", json!(["crates.io"])),
        ("release_cut", json!(true)),
        ("profile", json!("release")),
        ("schema_version", json!("source_sync_manifest.v2")),
    ] {
        let mut document = source_value()?;
        document[key] = value;
        has_only_finding(&staged_receipt(&document, Path::new("."))?, "manifest_schema_violation");
    }
    Ok(())
}

#[test]
fn source_rows_use_the_existing_projection_row_model() -> Result<()> {
    let release: Value = serde_json::from_str(include_str!(
        "../../../../fixtures/publication_sync/clean_manifest.json"
    ))?;
    let mut document = source_value()?;
    document["paths"] = release["paths"].clone();
    let model: SourceManifest = serde_json::from_value(document.clone())?;
    assert_eq!(model.paths.len(), release["paths"].as_array().ok_or_else(|| eyre!("rows"))?.len());
    has_only_finding(&staged_receipt(&document, Path::new("."))?, "source_profile_not_proven");
    Ok(())
}

#[test]
fn source_receipt_cannot_overwrite_declared_inputs_or_rows() -> Result<()> {
    let root = tempfile::tempdir()?;
    let mut document = source_value()?;
    let release: Value = serde_json::from_str(include_str!(
        "../../../../fixtures/publication_sync/clean_manifest.json"
    ))?;
    document["paths"] = release["paths"].clone();
    let manifest = root.path().join("source.json");
    for extra in [
        None,
        Some(("release", json!("0.18.0"))),
        Some(("product_proof", json!({"result":"pass"}))),
    ] {
        let mut candidate = document.clone();
        if let Some((key, value)) = extra {
            candidate[key] = value;
            assert!(serde_json::from_value::<SourceManifest>(candidate.clone()).is_err());
        }
        let raw = serde_json::to_vec(&candidate)?;
        fs::write(&manifest, &raw)?;
        for path in [document["inputs"][0]["path"].as_str(), document["paths"][0]["path"].as_str()]
        {
            let destination = root.path().join(path.ok_or_else(|| eyre!("fixture path"))?);
            fs::create_dir_all(destination.parent().ok_or_else(|| eyre!("parent"))?)?;
            fs::write(&destination, b"must survive\n")?;
            let config = PlanConfig {
                manifest: manifest.clone(),
                repo_root: root.path().to_path_buf(),
                receipt: destination.clone(),
            };
            let error =
                plan(config).expect_err("a receipt must never replace declared source bytes");
            assert!(error.to_string().contains("which this plan reads"));
            assert_eq!(fs::read(&destination)?, b"must survive\n");
        }
    }
    Ok(())
}

#[test]
fn malformed_alias_slots_refuse_every_receipt_write() -> Result<()> {
    let root = tempfile::tempdir()?;
    let manifest = root.path().join("source.json");
    let receipt = root.path().join("receipt.json");
    let mut document = source_value()?;
    document["inputs"][0].as_object_mut().ok_or_else(|| eyre!("input"))?.remove("path");
    fs::write(&manifest, serde_json::to_vec(&document)?)?;
    fs::write(&receipt, b"must survive\n")?;
    let config =
        PlanConfig { manifest, repo_root: root.path().to_path_buf(), receipt: receipt.clone() };
    let error = plan(config).expect_err("ambiguous aliases must block receipt writes");
    assert!(error.to_string().contains("declared alias paths cannot be established"));
    assert_eq!(fs::read(&receipt)?, b"must survive\n");
    Ok(())
}

#[test]
fn truncated_source_document_cannot_overwrite_input_or_row_bytes() -> Result<()> {
    let root = tempfile::tempdir()?;
    let manifest = root.path().join("source.json");
    let mut document = source_value()?;
    let release: Value = serde_json::from_str(include_str!(
        "../../../../fixtures/publication_sync/clean_manifest.json"
    ))?;
    document["paths"] = release["paths"].clone();
    let mut raw = serde_json::to_vec(&document)?;
    raw.pop();
    assert!(serde_json::from_slice::<Value>(&raw).is_err());
    fs::write(&manifest, raw)?;
    for path in [document["inputs"][0]["path"].as_str(), document["paths"][0]["path"].as_str()] {
        let destination = root.path().join(path.ok_or_else(|| eyre!("fixture path"))?);
        fs::create_dir_all(destination.parent().ok_or_else(|| eyre!("parent"))?)?;
        fs::write(&destination, b"must survive\n")?;
        let config = PlanConfig {
            manifest: manifest.clone(),
            repo_root: root.path().to_path_buf(),
            receipt: destination.clone(),
        };
        let error = plan(config).expect_err("unparsable alias declarations must block every write");
        assert!(error.to_string().contains("cannot be parsed for alias protection"));
        assert_eq!(fs::read(&destination)?, b"must survive\n");
    }
    Ok(())
}

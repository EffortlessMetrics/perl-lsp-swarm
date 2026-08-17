use std::error::Error;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::Value;

const AUTHORITY: &str = ".ci/fixtures/zed-perl-upstream/train-authority.v1.json";
const DOC: &str = "docs/integrations/ZED_TRAIN_AUTHORITY.md";

type TestResult<T> = Result<T, Box<dyn Error>>;

fn repo_root() -> TestResult<PathBuf> {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| io::Error::other("xtask manifest has no repository parent").into())
}

fn read_json(root: &Path, relative: &str) -> TestResult<Value> {
    Ok(serde_json::from_slice(&fs::read(root.join(relative))?)?)
}

#[test]
fn exactly_one_train_is_active_and_v1_is_historical() -> TestResult<()> {
    let root = repo_root()?;
    let authority = read_json(&root, AUTHORITY)?;

    assert_eq!(
        authority.get("schema_version").and_then(Value::as_str),
        Some("zed_train_authority.v1")
    );
    assert_eq!(
        authority
            .pointer("/active/manifest")
            .and_then(Value::as_str),
        Some(".ci/fixtures/zed-perl-upstream/train-v2/manifest.json")
    );
    assert_eq!(
        authority
            .pointer("/active/frontier_source")
            .and_then(Value::as_str),
        Some("stable_manifest_plus_typed_observation")
    );

    for rule in [
        "one_active_train",
        "frontier_is_generated",
        "live_github_state_is_observation_only",
        "issue_or_pr_state_is_not_product_evidence",
        "historical_train_cannot_route_codex",
        "external_write_is_maintainer_only",
    ] {
        assert_eq!(
            authority
                .pointer(&format!("/rules/{rule}"))
                .and_then(Value::as_bool),
            Some(true),
            "train authority rule `{rule}` drifted"
        );
    }

    let historical = authority
        .get("historical")
        .and_then(Value::as_array)
        .ok_or_else(|| io::Error::other("train authority lacks historical subjects"))?;
    assert_eq!(historical.len(), 2);
    for subject in historical {
        assert_eq!(
            subject.get("disposition").and_then(Value::as_str),
            Some("superseded_migration_subject")
                .filter(|_| {
                    subject.get("path").and_then(Value::as_str)
                        == Some(".ci/fixtures/zed-perl-upstream/codex-train.v1.json")
                })
                .or(Some("superseded_human_projection")),
            "historical train subject lacks an explicit superseded disposition"
        );
    }

    Ok(())
}

#[test]
fn active_manifest_and_observation_template_exist_and_fail_closed() -> TestResult<()> {
    let root = repo_root()?;
    let authority = read_json(&root, AUTHORITY)?;
    let manifest_path = authority
        .pointer("/active/manifest")
        .and_then(Value::as_str)
        .ok_or_else(|| io::Error::other("authority lacks active manifest"))?;
    let observation_path = authority
        .pointer("/active/observation_template")
        .and_then(Value::as_str)
        .ok_or_else(|| io::Error::other("authority lacks observation template"))?;

    let manifest = read_json(&root, manifest_path)?;
    assert_eq!(
        manifest.get("schema_version").and_then(Value::as_str),
        Some("zed_codex_implementation_train.v2")
    );

    let observation = read_json(&root, observation_path)?;
    assert_eq!(
        observation.get("result").and_then(Value::as_str),
        Some("not_run")
    );
    assert!(observation.get("observed_at").is_some_and(Value::is_null));
    assert!(
        observation
            .get("stages")
            .and_then(Value::as_object)
            .is_some_and(serde_json::Map::is_empty)
    );
    Ok(())
}

#[test]
fn human_authority_document_forbids_v1_routing_and_external_writes() -> TestResult<()> {
    let root = repo_root()?;
    let doc = fs::read_to_string(root.join(DOC))?;

    for needle in [
        "one active Zed implementation train",
        "stable version-2 train",
        "separate typed read-only observation",
        "They are not current routing authority",
        "Codex must not execute from its hand-maintained frontier",
        "maintainer-only checkpoints",
        "planned/not-proven",
    ] {
        assert!(doc.contains(needle), "authority document lacks `{needle}`");
    }
    assert!(!doc.contains("Codex may submit upstream"));
    Ok(())
}

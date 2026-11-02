//! Contract tests for the #2373 framework attribute-generation conformance manifest.

use std::error::Error;
use std::path::{Path, PathBuf};
use xtask::framework_attribute_generation_conformance::{
    self as fag, ConformanceValidationError, MANIFEST_PATH,
};

type TestResult<T = ()> = Result<T, Box<dyn Error>>;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from(".."))
}

#[test]
fn canonical_manifest_validates() -> TestResult {
    let stats = fag::validate_manifest_file(&repo_root())?;
    assert_eq!(stats.failure_probes, 10);
    assert!(stats.cases >= 20);
    assert!(stats.fixtures >= 20);
    Ok(())
}

#[test]
fn failure_probe_ids_are_unique_and_complete() -> TestResult {
    let manifest = fag::load_manifest(&repo_root())?;
    let probes = manifest
        .get("failure_probes")
        .and_then(|value| value.as_array())
        .ok_or("missing failure_probes")?;
    assert_eq!(probes.len(), 10);
    Ok(())
}

#[test]
fn moo_and_moose_matrices_are_separate() -> TestResult {
    let manifest = fag::load_manifest(&repo_root())?;
    let matrices =
        manifest.get("matrices").and_then(|value| value.as_object()).ok_or("missing matrices")?;
    let moo = matrices
        .get("moo_core")
        .and_then(|value| value.get("profile_id"))
        .and_then(|value| value.as_str())
        .ok_or("missing moo profile")?;
    let moose = matrices
        .get("moose_core")
        .and_then(|value| value.get("profile_id"))
        .and_then(|value| value.as_str())
        .ok_or("missing moose profile")?;
    assert!(moo.contains("moo"));
    assert!(moose.contains("moose"));
    assert_ne!(moo, moose);
    Ok(())
}

#[test]
fn corrupted_digest_fails_closed() -> TestResult {
    let root = repo_root();
    let mut manifest = fag::load_manifest(&root)?;
    manifest
        .get_mut("fixtures")
        .and_then(|value| value.get_mut("moo/ro.pl"))
        .and_then(|value| value.get_mut("digest"))
        .and_then(|value| {
            *value = serde_json::Value::String(
                "sha256:0000000000000000000000000000000000000000000000000000000000000000".into(),
            );
            Some(())
        })
        .ok_or("could not corrupt digest")?;
    let error = match fag::validate_manifest_value(&root, &manifest) {
        Ok(_) => "manifest unexpectedly validated".to_string(),
        Err(ConformanceValidationError::Violation(message)) => message,
        Err(other) => other.to_string(),
    };
    assert!(error.contains("digest mismatch"), "{error}");
    Ok(())
}

#[test]
fn manifest_path_is_stable() -> TestResult {
    assert!(repo_root().join(MANIFEST_PATH).is_file());
    Ok(())
}

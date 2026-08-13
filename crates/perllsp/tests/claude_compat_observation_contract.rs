use anyhow::{Result, ensure};
use perllsp::claude_compat::{
    CompatibilityReason, CompatibilityResult, PLUGIN_SLUG, PluginSubject, SERVER_EXECUTABLE,
    ServerSubject, embedded_catalog,
};

fn sha256(fill: char) -> String {
    format!("sha256:{}", fill.to_string().repeat(64))
}

fn plugin() -> PluginSubject {
    PluginSubject {
        slug: PLUGIN_SLUG.to_string(),
        version: "0.3.0".to_string(),
        tree_digest: sha256('1'),
        package_digest: sha256('2'),
        contract_digest: sha256('3'),
    }
}

fn server() -> ServerSubject {
    ServerSubject {
        executable: SERVER_EXECUTABLE.to_string(),
        version: "0.18.0".to_string(),
        build_revision: "a".repeat(40),
        artifact_sha256: sha256('4'),
        platform: "linux".to_string(),
        arch: "x86_64".to_string(),
    }
}

#[test]
fn incomplete_runtime_subjects_are_not_proven() -> Result<()> {
    let catalog = embedded_catalog();
    catalog.validate().map_err(anyhow::Error::msg)?;

    for decision in [
        catalog.decision_for_observation(None, None, None),
        catalog.decision_for_observation(Some(&plugin()), None, None),
        catalog.decision_for_observation(None, Some(&server()), None),
    ] {
        ensure!(decision.result == CompatibilityResult::NotProven);
        ensure!(decision.reason == CompatibilityReason::SubjectIdentityIncomplete);
        ensure!(decision.evidence_refs.is_empty());
        ensure!(!decision.limitations.is_empty());
    }
    Ok(())
}

#[test]
fn complete_runtime_subjects_still_use_exact_catalog_lookup() -> Result<()> {
    let catalog = embedded_catalog();
    let plugin = plugin();
    let server = server();
    let decision = catalog.decision_for_observation(Some(&plugin), Some(&server), None);

    ensure!(decision.result == CompatibilityResult::NotProven);
    ensure!(decision.reason == CompatibilityReason::ExactPairNotEstablished);
    Ok(())
}

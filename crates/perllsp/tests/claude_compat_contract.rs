use anyhow::{Result, ensure};
use perllsp::claude_compat::{
    CompatibilityCatalog, CompatibilityReason, CompatibilityResult, CompatibilityRow, HostSubject,
    PLUGIN_SLUG, PluginSubject, SCHEMA_VERSION, SERVER_EXECUTABLE, ServerSubject, embedded_catalog,
};

fn sha256(fill: char) -> String {
    format!("sha256:{}", fill.to_string().repeat(64))
}

fn plugin(version: &str, fill: char) -> PluginSubject {
    PluginSubject {
        slug: PLUGIN_SLUG.to_string(),
        version: version.to_string(),
        tree_digest: sha256(fill),
        package_digest: sha256(fill),
        contract_digest: sha256(fill),
    }
}

fn server(version: &str, revision: char, fill: char) -> ServerSubject {
    ServerSubject {
        executable: SERVER_EXECUTABLE.to_string(),
        version: version.to_string(),
        build_revision: revision.to_string().repeat(40),
        artifact_sha256: sha256(fill),
        platform: "linux".to_string(),
        arch: "x86_64".to_string(),
    }
}

fn host(version: &str) -> HostSubject {
    HostSubject { claude_code_version: version.to_string(), control_plane_schema: None }
}

fn row(
    plugin: PluginSubject,
    server: ServerSubject,
    host: Option<HostSubject>,
    result: CompatibilityResult,
) -> CompatibilityRow {
    let (evidence_refs, limitations) = match result {
        CompatibilityResult::Compatible | CompatibilityResult::Incompatible => {
            (vec!["agent_client_compat.fixture".to_string()], Vec::new())
        }
        CompatibilityResult::NotProven => {
            (Vec::new(), vec!["exact pair deliberately remains unproven".to_string()])
        }
    };
    CompatibilityRow { plugin, server, host, result, evidence_refs, limitations }
}

#[test]
fn embedded_catalog_is_conservative_and_valid() -> Result<()> {
    let catalog = embedded_catalog();
    catalog.validate().map_err(anyhow::Error::msg)?;
    ensure!(catalog.schema_version == SCHEMA_VERSION);
    ensure!(catalog.rows.is_empty(), "initial authority must not invent a compatible pair");

    let plugin = plugin("0.18.0", '1');
    let server = server("0.18.0", 'a', '2');
    let decision = catalog
        .decision_for(&plugin, &server, Some(&host("2.1.205")))
        .map_err(anyhow::Error::msg)?;
    ensure!(decision.result == CompatibilityResult::NotProven);
    ensure!(decision.reason == CompatibilityReason::ExactPairNotEstablished);
    Ok(())
}

#[test]
fn matching_version_numbers_without_evidence_are_not_proven() -> Result<()> {
    let catalog =
        CompatibilityCatalog { schema_version: SCHEMA_VERSION.to_string(), rows: Vec::new() };
    catalog.validate().map_err(anyhow::Error::msg)?;

    let plugin = plugin("0.18.0", '1');
    let server = server("0.18.0", 'a', '2');
    let decision = catalog.decision_for(&plugin, &server, None).map_err(anyhow::Error::msg)?;
    ensure!(decision.result == CompatibilityResult::NotProven);
    ensure!(decision.reason == CompatibilityReason::ExactPairNotEstablished);
    Ok(())
}

#[test]
fn direct_evidence_can_prove_different_numeric_versions() -> Result<()> {
    let plugin = plugin("0.3.0", '1');
    let server = server("0.18.0", 'a', '2');
    let catalog = CompatibilityCatalog {
        schema_version: SCHEMA_VERSION.to_string(),
        rows: vec![row(plugin.clone(), server.clone(), None, CompatibilityResult::Compatible)],
    };
    catalog.validate().map_err(anyhow::Error::msg)?;

    let decision = catalog.decision_for(&plugin, &server, None).map_err(anyhow::Error::msg)?;
    ensure!(decision.result == CompatibilityResult::Compatible);
    ensure!(decision.reason == CompatibilityReason::ExactEvidence);
    Ok(())
}

#[test]
fn known_bad_exact_pair_is_incompatible() -> Result<()> {
    let plugin = plugin("0.3.0", '1');
    let server = server("0.17.0", 'a', '2');
    let catalog = CompatibilityCatalog {
        schema_version: SCHEMA_VERSION.to_string(),
        rows: vec![row(plugin.clone(), server.clone(), None, CompatibilityResult::Incompatible)],
    };
    catalog.validate().map_err(anyhow::Error::msg)?;

    let decision = catalog.decision_for(&plugin, &server, None).map_err(anyhow::Error::msg)?;
    ensure!(decision.result == CompatibilityResult::Incompatible);
    ensure!(decision.reason == CompatibilityReason::ExactKnownBad);
    Ok(())
}

#[test]
fn stale_plugin_tree_or_wrong_server_build_cannot_reuse_row() -> Result<()> {
    let plugin = plugin("0.3.0", '1');
    let server = server("0.18.0", 'a', '2');
    let catalog = CompatibilityCatalog {
        schema_version: SCHEMA_VERSION.to_string(),
        rows: vec![row(plugin.clone(), server.clone(), None, CompatibilityResult::Compatible)],
    };
    catalog.validate().map_err(anyhow::Error::msg)?;

    let mut stale_plugin = plugin.clone();
    stale_plugin.tree_digest = sha256('3');
    ensure!(
        catalog.decision_for(&stale_plugin, &server, None).map_err(anyhow::Error::msg)?.result
            == CompatibilityResult::NotProven
    );

    let mut wrong_build = server.clone();
    wrong_build.build_revision = "b".repeat(40);
    ensure!(
        catalog.decision_for(&plugin, &wrong_build, None).map_err(anyhow::Error::msg)?.result
            == CompatibilityResult::NotProven
    );
    Ok(())
}

#[test]
fn host_coupled_rows_do_not_transfer_to_another_host() -> Result<()> {
    let plugin = plugin("0.3.0", '1');
    let server = server("0.18.0", 'a', '2');
    let tested_host = host("2.1.205");
    let catalog = CompatibilityCatalog {
        schema_version: SCHEMA_VERSION.to_string(),
        rows: vec![row(
            plugin.clone(),
            server.clone(),
            Some(tested_host.clone()),
            CompatibilityResult::Compatible,
        )],
    };
    catalog.validate().map_err(anyhow::Error::msg)?;

    ensure!(
        catalog
            .decision_for(&plugin, &server, Some(&tested_host))
            .map_err(anyhow::Error::msg)?
            .result
            == CompatibilityResult::Compatible
    );
    ensure!(
        catalog
            .decision_for(&plugin, &server, Some(&host("2.2.0")))
            .map_err(anyhow::Error::msg)?
            .result
            == CompatibilityResult::NotProven
    );
    ensure!(
        catalog.decision_for(&plugin, &server, None).map_err(anyhow::Error::msg)?.result
            == CompatibilityResult::NotProven
    );
    Ok(())
}

#[test]
fn host_specific_row_outranks_host_independent_overlap() -> Result<()> {
    let plugin = plugin("0.3.0", '1');
    let server = server("0.18.0", 'a', '2');
    let observed = host("2.1.205");
    let catalog = CompatibilityCatalog {
        schema_version: SCHEMA_VERSION.to_string(),
        rows: vec![
            row(plugin.clone(), server.clone(), None, CompatibilityResult::Compatible),
            row(
                plugin.clone(),
                server.clone(),
                Some(observed.clone()),
                CompatibilityResult::Incompatible,
            ),
        ],
    };
    catalog.validate().map_err(anyhow::Error::msg)?;

    let decision =
        catalog.decision_for(&plugin, &server, Some(&observed)).map_err(anyhow::Error::msg)?;
    ensure!(decision.result == CompatibilityResult::Incompatible);
    ensure!(decision.reason == CompatibilityReason::ExactKnownBad);
    Ok(())
}

#[test]
fn decision_for_rejects_invalid_catalog() -> Result<()> {
    let plugin = plugin("0.3.0", '1');
    let server = server("0.18.0", 'a', '2');
    let mut bad_plugin = plugin.clone();
    bad_plugin.slug = "perl-lsp".to_string();
    let catalog = CompatibilityCatalog {
        schema_version: SCHEMA_VERSION.to_string(),
        rows: vec![row(bad_plugin, server.clone(), None, CompatibilityResult::Compatible)],
    };

    let err =
        catalog.decision_for(&plugin, &server, None).expect_err("invalid catalog must fail closed");
    ensure!(err.contains("plugin.slug"));
    Ok(())
}

#[test]
fn validation_rejects_duplicate_rows_and_unsupported_subjects() -> Result<()> {
    let plugin = plugin("0.3.0", '1');
    let server = server("0.18.0", 'a', '2');
    let duplicate = row(plugin.clone(), server.clone(), None, CompatibilityResult::Compatible);
    let catalog = CompatibilityCatalog {
        schema_version: SCHEMA_VERSION.to_string(),
        rows: vec![duplicate.clone(), duplicate],
    };
    ensure!(catalog.validate().is_err(), "duplicate exact rows were accepted");

    let mut wrong_plugin = plugin.clone();
    wrong_plugin.slug = "perl-lsp".to_string();
    let catalog = CompatibilityCatalog {
        schema_version: SCHEMA_VERSION.to_string(),
        rows: vec![row(wrong_plugin, server.clone(), None, CompatibilityResult::Compatible)],
    };
    ensure!(catalog.validate().is_err(), "old Claude plugin slug was accepted");

    let mut wrong_server = server;
    wrong_server.executable = "perl-lsp".to_string();
    let catalog = CompatibilityCatalog {
        schema_version: SCHEMA_VERSION.to_string(),
        rows: vec![row(plugin, wrong_server, None, CompatibilityResult::Compatible)],
    };
    ensure!(catalog.validate().is_err(), "another server executable was accepted");
    Ok(())
}

#[test]
fn catalog_machine_projection_is_deterministic() -> Result<()> {
    let catalog = CompatibilityCatalog {
        schema_version: SCHEMA_VERSION.to_string(),
        rows: vec![row(
            plugin("0.3.0", '1'),
            server("0.18.0", 'a', '2'),
            None,
            CompatibilityResult::Compatible,
        )],
    };
    catalog.validate().map_err(anyhow::Error::msg)?;

    let first = serde_json::to_string_pretty(&catalog.to_json())?;
    let second = serde_json::to_string_pretty(&catalog.to_json())?;
    ensure!(first == second);
    ensure!(first.contains("\"result\": \"compatible\""));
    ensure!(first.contains("\"schema_version\": \"claude_plugin_server_compat.v1\""));
    Ok(())
}

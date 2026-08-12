//! Regression coverage for the supported/preview profile boundary.

use perl_lsp_rs_core::features::contracts::all_features;
use perl_lsp_rs_core::features::flags::BuildFlags;
use perl_lsp_rs_core::features::grid::to_json_for_profile;
use perl_lsp_rs_core::features::ids::{LSP_NOTEBOOK_CELL_EXECUTION, LSP_NOTEBOOK_DOCUMENT_SYNC};
use perl_lsp_rs_core::features::policy::{FeatureProfile, catalog_advertised_feature_ids};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn ensure(condition: bool, message: impl Into<String>) -> TestResult {
    if condition { Ok(()) } else { Err(message.into().into()) }
}

#[test]
fn production_is_an_explicit_supported_baseline() -> TestResult {
    let expected = BuildFlags {
        completion: true,
        hover: true,
        definition: true,
        type_definition: true,
        implementation: true,
        references: true,
        document_symbol: true,
        workspace_symbol: true,
        inlay_hints: true,
        pull_diagnostics: true,
        workspace_symbol_resolve: true,
        semantic_tokens: true,
        code_actions: true,
        execute_command: true,
        rename: true,
        document_links: true,
        selection_ranges: true,
        on_type_formatting: true,
        code_lens: true,
        call_hierarchy: true,
        type_hierarchy: true,
        linked_editing: true,
        inline_completion: true,
        inline_values: true,
        notebook_document_sync: false,
        notebook_cell_execution: false,
        moniker: true,
        document_color: true,
        source_organize_imports: true,
        formatting: true,
        range_formatting: true,
        folding_range: true,
        signature_help: true,
        document_highlight: true,
        declaration: true,
    };

    ensure(
        BuildFlags::production() == expected,
        "production must equal the explicit supported baseline",
    )
}

#[test]
fn all_opts_into_only_the_declared_notebook_previews() -> TestResult {
    let mut expected = BuildFlags::production();
    expected.notebook_document_sync = true;
    expected.notebook_cell_execution = true;
    ensure(
        BuildFlags::all() == expected,
        "all must opt into exactly the two declared notebook preview flags",
    )
}

#[test]
fn production_and_ga_lock_exclude_notebook_from_every_projection() -> TestResult {
    for (profile_kind, profile) in [
        (FeatureProfile::Production, BuildFlags::production()),
        (FeatureProfile::GaLock, BuildFlags::ga_lock()),
    ] {
        ensure(!profile.notebook_document_sync, "notebook sync flag leaked")?;
        ensure(!profile.notebook_cell_execution, "notebook execution flag leaked")?;
        ensure(
            !profile.to_feature_ids().contains(&LSP_NOTEBOOK_DOCUMENT_SYNC),
            "notebook sync feature ID leaked",
        )?;
        ensure(
            !profile.to_feature_ids().contains(&LSP_NOTEBOOK_CELL_EXECUTION),
            "notebook execution feature ID leaked",
        )?;
        let advertised = profile.to_advertised_features();
        ensure(!advertised.notebook_document_sync, "notebook sync advertisement leaked")?;
        ensure(!advertised.notebook_cell_execution, "notebook execution advertisement leaked")?;
        let catalog_projection = catalog_advertised_feature_ids(profile_kind);
        ensure(
            !catalog_projection.contains(&LSP_NOTEBOOK_DOCUMENT_SYNC),
            "notebook sync leaked into supported catalog projection",
        )?;
        ensure(
            !catalog_projection.contains(&LSP_NOTEBOOK_CELL_EXECUTION),
            "notebook execution leaked into supported catalog projection",
        )?;
    }
    Ok(())
}

#[test]
fn all_includes_notebook_in_every_projection() -> TestResult {
    let all = BuildFlags::all();
    ensure(all.notebook_document_sync, "all omitted notebook sync")?;
    ensure(all.notebook_cell_execution, "all omitted notebook execution")?;
    ensure(
        all.to_feature_ids().contains(&LSP_NOTEBOOK_DOCUMENT_SYNC),
        "all omitted notebook sync feature ID",
    )?;
    ensure(
        all.to_feature_ids().contains(&LSP_NOTEBOOK_CELL_EXECUTION),
        "all omitted notebook execution feature ID",
    )?;
    let advertised = all.to_advertised_features();
    ensure(advertised.notebook_document_sync, "all omitted notebook sync advertisement")?;
    ensure(advertised.notebook_cell_execution, "all omitted notebook execution advertisement")?;

    let catalog_projection = catalog_advertised_feature_ids(FeatureProfile::All);
    ensure(
        catalog_projection.contains(&LSP_NOTEBOOK_DOCUMENT_SYNC),
        "all catalog projection omitted notebook sync preview",
    )?;
    ensure(
        catalog_projection.contains(&LSP_NOTEBOOK_CELL_EXECUTION),
        "all catalog projection omitted notebook execution preview",
    )?;
    let expected_count = all
        .to_feature_ids()
        .into_iter()
        .filter(|id| all_features().iter().any(|feature| feature.id == *id))
        .count();
    ensure(
        catalog_projection.len() == expected_count,
        format!(
            "all catalog count drifted: expected {expected_count}, got {}",
            catalog_projection.len()
        ),
    )?;

    let payload: serde_json::Value =
        serde_json::from_str(&to_json_for_profile(FeatureProfile::All))?;
    let payload_ids = payload["advertised"]
        .as_array()
        .ok_or("all JSON advertised projection must be an array")?;
    ensure(
        payload_ids.iter().any(|id| id.as_str() == Some(LSP_NOTEBOOK_DOCUMENT_SYNC)),
        "all JSON omitted notebook sync preview",
    )?;
    ensure(
        payload_ids.iter().any(|id| id.as_str() == Some(LSP_NOTEBOOK_CELL_EXECUTION)),
        "all JSON omitted notebook execution preview",
    )?;
    let all_summary = payload["profiles"]
        .as_array()
        .and_then(|profiles| profiles.iter().find(|profile| profile["profile"] == "all"))
        .ok_or("all JSON profile summary is missing")?;
    ensure(
        all_summary["advertised_feature_count"].as_u64() == Some(catalog_projection.len() as u64),
        "all JSON profile count does not match its advertised ID projection",
    )
}

#[test]
fn notebook_catalog_rows_cannot_serve_as_ga_evidence_for_7032() -> TestResult {
    for id in [LSP_NOTEBOOK_DOCUMENT_SYNC, LSP_NOTEBOOK_CELL_EXECUTION] {
        let feature = all_features()
            .iter()
            .find(|feature| feature.id == id)
            .ok_or_else(|| format!("missing catalog row {id}"))?;
        ensure(feature.maturity == "preview", format!("{id} is not preview"))?;
        ensure(!feature.advertised, format!("{id} is default-advertised"))?;
        ensure(
            feature.description.contains("not GA evidence for #7032"),
            format!("{id} lacks the #7032 negative evidence boundary"),
        )?;
    }
    Ok(())
}

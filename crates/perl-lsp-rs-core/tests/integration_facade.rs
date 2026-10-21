//! Direct-core integration tests for Wave F absorption.
//!
//! Facade re-export and type-identity checks live in the facade crate's
//! `core_facade_contract` integration target (#8401). These tests verify that:
//! 1. All modules are accessible from perl-lsp-rs-core directly
//! 2. Downstream consumers can use the new module paths
//! 3. The capability map is functional end-to-end

/// Test that a mock governance use case works with the new module paths.
///
/// Governance (Wave G3, published separately) will consume perl-lsp-rs-core
/// post-Wave F. This tests that pattern.
#[test]
fn test_governance_consumer_complete_usage() -> Result<(), Box<dyn std::error::Error>> {
    use perl_lsp_rs_core::features::contracts::feature_ids_from_caps;
    use perl_lsp_rs_core::features::flags::BuildFlags;
    use perl_lsp_rs_core::features::policy::from_str_name;

    // Governance queries available profiles
    let profile = from_str_name("production");
    assert!(profile.is_some(), "governance should be able to parse 'production' profile");

    // Governance instantiates build flags
    let _flags = BuildFlags::default();

    // Governance queries feature IDs from server capabilities
    let server_caps = lsp_types::ServerCapabilities::default();
    let feature_ids = feature_ids_from_caps(&server_caps);
    assert!(feature_ids.is_empty(), "default server capabilities should produce no feature IDs");

    Ok(())
}
/// Test that a mock protocol use case works with the new module paths.
///
/// Protocol imports contracts and flags to build the initialize response.
#[test]
fn test_protocol_consumer_complete_usage() -> Result<(), Box<dyn std::error::Error>> {
    use perl_lsp_rs_core::features::contracts::{all_features, feature_ids_from_caps};
    use perl_lsp_rs_core::features::flags::AdvertisedFeatures;

    // Protocol queries all features for capability reporting
    let all = all_features();
    assert!(!all.is_empty(), "protocol should see all features");

    // Protocol extracts feature IDs from server capabilities
    let server_caps = lsp_types::ServerCapabilities::default();
    let ids = feature_ids_from_caps(&server_caps);
    assert!(ids.is_empty(), "default capabilities should produce empty feature list");

    // Protocol builds advertised features structure
    let _advertised = AdvertisedFeatures::default();

    Ok(())
}

/// Test that all 8 feature modules export public API surfaces.
///
/// Regression guard: ensure the absorbed modules didn't lose their public API
/// during the move to perl-lsp-rs-core/src/features/*.
#[test]
fn test_all_feature_modules_export_api() -> Result<(), Box<dyn std::error::Error>> {
    use perl_lsp_rs_core::features::contracts::{Feature, all_features};
    use perl_lsp_rs_core::features::flags::BuildFlags;
    use perl_lsp_rs_core::features::grid::to_json as grid_to_json;
    use perl_lsp_rs_core::features::ids::LSP_COMPLETION;
    use perl_lsp_rs_core::features::policy::from_str_name;
    use perl_lsp_rs_core::features::profile::from_str_name as profile_from_str_name;
    use perl_lsp_rs_core::features::profile_cli::feature_profile_label;

    // contracts
    assert!(!all_features().is_empty());
    let _ = std::any::type_name::<Feature>();

    // flags
    let _ = std::any::type_name::<BuildFlags>();

    // grid
    let json = grid_to_json();
    assert!(!json.is_empty(), "grid to_json should produce non-empty output");

    // ids
    assert_eq!(LSP_COMPLETION, "lsp.completion");

    // policy
    let prof = from_str_name("production");
    assert!(prof.is_some());

    // profile
    let prof2 = profile_from_str_name("ga-lock");
    assert!(prof2.is_some());

    // profile_cli
    use perl_lsp_rs_core::features::policy::FeatureProfile;
    let label = feature_profile_label(FeatureProfile::GaLock);
    assert!(!label.is_empty(), "feature_profile_label should produce non-empty output");

    Ok(())
}

/// Test that the feature catalog is stable and well-formed.
///
/// Regression guard: build.rs generated the catalog; it should be stable and
/// usable for feature queries.
#[test]
fn test_feature_catalog_stability() -> Result<(), Box<dyn std::error::Error>> {
    use perl_lsp_rs_core::features::contracts::all_features;

    let catalog1 = all_features();
    let catalog2 = all_features();

    // Should be stable (called multiple times)
    assert_eq!(catalog1.len(), catalog2.len(), "catalog size should be stable");

    // Catalog should not be empty
    assert!(!catalog1.is_empty(), "catalog should have features");

    Ok(())
}

/// Test that capability_map correctly filters features by profile.
///
/// Integration test: capability_map takes a feature list and produces server
/// capabilities. This tests the filtering/mapping logic works.
#[test]
fn test_capability_map_profile_filtering() -> Result<(), Box<dyn std::error::Error>> {
    use perl_lsp_rs_core::capability_map::caps_from_feature_ids;

    // Empty list
    let empty = caps_from_feature_ids(&[]);
    assert!(empty.completion_provider.is_none());

    // Single feature
    let single = caps_from_feature_ids(&["lsp.completion"]);
    assert!(single.completion_provider.is_some());

    // Multiple features (if they affect different capabilities)
    let multi = caps_from_feature_ids(&["lsp.completion", "lsp.hover"]);
    assert!(multi.completion_provider.is_some());

    Ok(())
}

//! Facade-owned cross-layer proof moved intact from core for #8401.
//!
//! Keep real `perl_lsp` imports: these tests protect facade re-exports and
//! type identity in addition to the direct core tests retained in core.

/// Test that the complete facade works for a simulated LSP initialization.
///
/// This simulates how the LSP server would use the absorbed crates post-Wave F.
#[test]
fn test_lsp_initialization_facade_shape() -> Result<(), Box<dyn std::error::Error>> {
    use perl_lsp::capability_map::caps_from_feature_ids;
    use perl_lsp::features::flags::BuildFlags;
    use perl_lsp::features::ids::LSP_COMPLETION;

    // Simulate server initialization:
    // 1. Parse feature flags
    let _flags = BuildFlags::default();

    // 2. Get feature ID constant
    assert_eq!(LSP_COMPLETION, "lsp.completion");

    // 3. Build capabilities from feature IDs
    let feature_ids = &["lsp.completion"];
    let capabilities = caps_from_feature_ids(feature_ids);

    // Verify the capabilities object is properly formed
    assert!(
        capabilities.completion_provider.is_some(),
        "completion should be enabled for lsp.completion feature"
    );

    Ok(())
}

/// Test mixed consumption via facade and core (both paths should work).
///
/// Edge case: what if code imports from both perl_lsp and perl_lsp_rs_core?
/// Types should be identical.
#[test]
fn test_mixed_facade_and_core_consumption() -> Result<(), Box<dyn std::error::Error>> {
    use perl_lsp::features::contracts::all_features as facade_all_features;
    use perl_lsp::features::flags::BuildFlags as FacadeFlags;
    use perl_lsp_rs_core::features::contracts::all_features as core_all_features;
    use perl_lsp_rs_core::features::flags::BuildFlags as CoreFlags;

    // Types should be identical
    let facade_type = std::any::type_name::<FacadeFlags>();
    let core_type = std::any::type_name::<CoreFlags>();
    assert_eq!(facade_type, core_type, "BuildFlags via facade and core should be the same type");

    // Functions should be identical
    let facade_count = facade_all_features().len();
    let core_count = core_all_features().len();
    assert_eq!(
        facade_count, core_count,
        "all_features() should return the same count via both paths"
    );

    Ok(())
}

/// Test that capability_map can be accessed from both core and facade.
///
/// Regression guard: capability_map is a special case (top-level, not in features/).
#[test]
fn test_capability_map_dual_access_complete() -> Result<(), Box<dyn std::error::Error>> {
    use perl_lsp::capability_map as facade_cm;
    use perl_lsp_rs_core::capability_map as core_cm;

    // Both should work
    let facade_caps = facade_cm::caps_from_feature_ids(&["lsp.completion"]);
    let core_caps = core_cm::caps_from_feature_ids(&["lsp.completion"]);

    // Results should be identical
    assert!(facade_caps.completion_provider.is_some(), "facade should enable completion");
    assert!(core_caps.completion_provider.is_some(), "core should enable completion");

    Ok(())
}

/// Test that all re-exports from perl-lsp are accessible.
///
/// Regression guard: ensures the facade lib.rs has all necessary re-exports.
#[test]
fn test_perl_lsp_facade_has_all_reexports() -> Result<(), Box<dyn std::error::Error>> {
    use perl_lsp::capability_map;
    use perl_lsp::features::contracts;
    use perl_lsp::features::flags;
    use perl_lsp::features::grid;
    use perl_lsp::features::ids;
    use perl_lsp::features::policy;
    use perl_lsp::features::profile;
    use perl_lsp::features::profile_cli;
    use perl_tdd_support::must_some;

    // All modules should be accessible
    let _ = capability_map::caps_from_feature_ids(&[]);
    let _ = contracts::all_features();
    let _ = flags::BuildFlags::default();
    let _ = grid::to_json();
    assert_eq!(ids::LSP_COMPLETION, "lsp.completion");
    assert!(policy::from_str_name("production").is_some());
    assert!(profile::from_str_name("ga-lock").is_some());
    let ga_lock_profile = must_some(policy::from_str_name("ga-lock"));
    let label = profile_cli::feature_profile_label(ga_lock_profile);
    assert_eq!(label, "ga-lock", "feature_profile_label must return canonical 'ga-lock' name");

    Ok(())
}

/// Test that facade re-exports are accessible from perl-lsp (the main LSP crate)
#[test]
fn test_facade_reexports_from_perl_lsp() -> Result<(), Box<dyn std::error::Error>> {
    // After Wave F, consumers should be able to import from perl_lsp (the facade)
    // and get the same types as importing from perl_lsp_rs_core
    use perl_lsp::features;

    // Verify by accessing a type from each re-exported submodule
    let _ = std::any::type_name::<features::flags::BuildFlags>();
    let _ = std::any::type_name::<features::policy::FeatureProfile>();

    // Verify capability_map re-export
    let _caps = perl_lsp::capability_map::caps_from_feature_ids(&[]);

    Ok(())
}

/// Test type identity: perl-lsp re-exports resolve to same types as core
#[test]
fn test_type_identity_facade_vs_core() -> Result<(), Box<dyn std::error::Error>> {
    // Verify that types imported via the facade (perl_lsp) are identical to
    // types imported directly from perl-lsp-rs-core
    use perl_lsp::features::flags::BuildFlags as FacadeBuildFlags;
    use perl_lsp_rs_core::features::flags::BuildFlags as CoreBuildFlags;

    // The types should be identical — both are the same type from the same crate.
    // Verify by checking type names match.
    let facade_name = std::any::type_name::<FacadeBuildFlags>();
    let core_name = std::any::type_name::<CoreBuildFlags>();
    assert_eq!(facade_name, core_name, "facade and core BuildFlags should be the same type");

    Ok(())
}

/// Test capability_map module re-export from perl-lsp
#[test]
fn test_capability_map_reexport_from_perl_lsp() -> Result<(), Box<dyn std::error::Error>> {
    // perl-lsp should re-export capability_map for downstream consumers
    // Verify by calling a function from the module
    let caps = perl_lsp::capability_map::caps_from_feature_ids(&[]);
    assert!(caps.completion_provider.is_none(), "empty feature list produces empty caps");
    Ok(())
}

/// Test that the public API surface doesn't have conflicting re-exports
#[test]
fn test_facade_no_conflicting_reexports() -> Result<(), Box<dyn std::error::Error>> {
    // Verify that perl-lsp (the facade) and perl-lsp-rs-core export compatible types
    // without duplication or name collision
    use perl_lsp::features::flags::BuildFlags as FacadeFlags;
    use perl_lsp::features::policy::FeatureProfile as FacadeProfile;

    // If these compile without error, name resolution is working
    let _ = std::any::type_name::<FacadeFlags>();
    let _ = std::any::type_name::<FacadeProfile>();

    // Verify capability_map is accessible from the facade
    let caps = perl_lsp::capability_map::caps_from_feature_ids(&[]);
    assert!(caps.completion_provider.is_none());

    Ok(())
}

/// Test that capability_map module is accessible from both core and facade.
///
/// Wave F acceptance criterion: capability_map should be accessible as both:
/// - perl_lsp_rs_core::capability_map (direct)
/// - perl_lsp::capability_map (facade re-export)
#[test]
fn test_capability_map_dual_access_paths() -> Result<(), Box<dyn std::error::Error>> {
    use perl_lsp::capability_map as facade_map;
    use perl_lsp_rs_core::capability_map as core_map;

    // Both paths should resolve to the same function
    let facade_result = facade_map::caps_from_feature_ids(&[]);
    let core_result = core_map::caps_from_feature_ids(&[]);

    // Results should be structurally identical
    assert_eq!(
        std::any::type_name_of_val(&facade_result),
        std::any::type_name_of_val(&core_result),
        "facade and core capability_map should return identical types"
    );

    Ok(())
}

/// Test that facade re-exports don't create type duplication.
///
/// Edge case: when perl-lsp re-exports from perl-lsp-rs-core, do the types
/// remain identical, or are they duplicated?
#[test]
fn test_facade_reexports_preserve_type_identity() -> Result<(), Box<dyn std::error::Error>> {
    use perl_lsp::features::contracts::FeatureProfileKind as FacadeContractKind;
    use perl_lsp_rs_core::features::contracts::FeatureProfileKind as CoreContractKind;

    let facade_name = std::any::type_name::<FacadeContractKind>();
    let core_name = std::any::type_name::<CoreContractKind>();

    assert_eq!(
        facade_name, core_name,
        "facade re-export should preserve type identity (not duplicate)"
    );

    Ok(())
}

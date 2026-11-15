//! The legacy provider path remains the same lower Test2 compatibility API.

use perl_lsp_rs_core::providers::testing::test2 as legacy;
use perl_semantic_analyzer::{Test2Facts, Test2Pragmas, Test2ResolvedImport};
use std::collections::BTreeSet;

#[test]
fn legacy_test2_types_are_the_lower_types() -> Result<(), Box<dyn std::error::Error>> {
    // Struct literals and assignments in both directions exclude a copied
    // provider model or a conversion wrapper at the compatibility path.
    let legacy_facts = legacy::Test2Facts {
        modules: vec!["Test2::V1".to_owned()],
        imported_symbols: BTreeSet::from(["T2".to_owned()]),
        strict: false,
        warnings: false,
    };
    let lower_facts: Test2Facts = legacy_facts;
    let legacy_facts: legacy::Test2Facts = lower_facts;
    assert!(legacy_facts.is_imported("T2"));
    assert!(!legacy_facts.provides_pragma("strict"));

    let lower_pragmas = Test2Pragmas { strict: true, warnings: false };
    let legacy_pragmas: legacy::Test2Pragmas = lower_pragmas;
    let lower_pragmas: Test2Pragmas = legacy_pragmas;
    assert!(lower_pragmas.strict);
    assert!(!lower_pragmas.warnings);

    let lower_import = Test2ResolvedImport {
        symbols: BTreeSet::from(["my_ok".to_owned()]),
        pragmas: Some(lower_pragmas),
    };
    let legacy_import: legacy::ResolvedImport = lower_import;
    let lower_import: Test2ResolvedImport = legacy_import;
    assert_eq!(lower_import.symbols, BTreeSet::from(["my_ok".to_owned()]));

    // The existing callable API still returns the lower resolved type without
    // exposing the private implementation module or adding a new legacy method.
    let resolve: fn(&str, &str) -> Option<Test2ResolvedImport> = legacy::resolve_import;
    let resolved =
        resolve("Test2::V1", "").ok_or("the existing V1 module must remain recognized")?;
    assert_eq!(resolved.symbols, BTreeSet::from(["T2".to_owned()]));
    assert_eq!(resolved.pragmas, Some(Test2Pragmas { strict: false, warnings: false }));
    assert!(legacy::is_test2_module("Test2::V0"));
    assert!(legacy::is_test2_bundle("Test2::V0"));
    assert_eq!(legacy::module_default_exports("Test2::Tools::Compare"), Some(&["is", "like"][..]));
    Ok(())
}

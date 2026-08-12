use perl_lsp_rs_core::providers::testing::test2::{Test2Facts, resolve_import};
use std::io;

#[test]
fn v0_all_tag_keeps_the_reviewed_import_set() {
    let facts = Test2Facts::from_source("use Test2::V0 ':ALL';\n");

    assert!(facts.is_imported("ok"));
    assert!(facts.is_imported("is"));
    assert!(facts.is_imported("subtest"));
    assert!(facts.is_imported("done_testing"));
    assert_eq!((facts.strict, facts.warnings), (true, true));
}

#[test]
fn all_tag_still_honors_an_explicit_exclusion() -> Result<(), Box<dyn std::error::Error>> {
    let resolved = resolve_import("Test2::V0", "':ALL', '!ok'")
        .ok_or_else(|| io::Error::other("Test2::V0 must be recognized"))?;

    assert!(!resolved.symbols.contains("ok"));
    assert!(resolved.symbols.contains("is"));
    assert!(resolved.symbols.contains("subtest"));
    Ok(())
}

#[test]
fn standalone_compare_all_uses_the_reviewed_export_ok_menu()
-> Result<(), Box<dyn std::error::Error>> {
    let resolved = resolve_import("Test2::Tools::Compare", "':ALL'")
        .ok_or_else(|| io::Error::other("Test2::Tools::Compare must be recognized"))?;

    for name in ["is", "like", "match", "array", "hash"] {
        assert!(
            resolved.symbols.contains(name),
            "reviewed Compare :ALL set must contain {name}"
        );
    }
    Ok(())
}

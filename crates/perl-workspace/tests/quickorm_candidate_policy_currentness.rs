//! Keep the advisory QuickORM adapter policy truthful about import containment.
//!
//! `generated_member_extractor_quickorm` no longer models a QuickORM import as
//! replacing the caller's builder (#13367). Upstream `0.000029` installs a set
//! of local names per import and records them for a later `unimport`, so a
//! second import can overwrite some installed names while leaving a distinct
//! earlier renamed alias live. The registry prose is the durable statement of
//! that boundary; this test fails if the retired replacement claim returns.

use std::{fs, path::PathBuf};

fn policy_text() -> Result<String, Box<dyn std::error::Error>> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../policy/framework-adapters.toml")
        .canonicalize()?;
    Ok(fs::read_to_string(path)?)
}

/// The QuickORM adapter block, from its `framework` key to the next `[[adapter]]`.
fn quickorm_adapter_block(policy: &str) -> Result<String, Box<dyn std::error::Error>> {
    let start = policy
        .find("framework = \"DBIx::QuickORM\"")
        .ok_or("no DBIx::QuickORM adapter entry in policy/framework-adapters.toml")?;
    let rest = &policy[start..];
    let end = rest.find("\n[[adapter]]").unwrap_or(rest.len());
    Ok(rest[..end].to_string())
}

#[test]
fn quickorm_policy_does_not_claim_an_import_replaces_the_builder()
-> Result<(), Box<dyn std::error::Error>> {
    let block = quickorm_adapter_block(&policy_text()?)?;

    // The exact retired sentence, plus the rule it encoded in any spelling.
    assert!(
        !block.contains("Each import replaces the caller's builder"),
        "the retired latest-import-wins sentence returned to the QuickORM policy"
    );
    assert!(
        !block.contains("import replaces the caller"),
        "the QuickORM policy again models an import as replacing the caller's builder"
    );

    Ok(())
}

#[test]
fn quickorm_policy_records_the_temporary_containment_and_its_removal_condition()
-> Result<(), Box<dyn std::error::Error>> {
    let block = quickorm_adapter_block(&policy_text()?)?;

    for required in [
        // Containment is named as such rather than presented as the real model.
        "contained",
        "exactly one parser-proven exact table import",
        "no second import or unimport event",
        // Whole-package, retroactive suppression.
        "no candidate for the whole package",
        // The removal condition must stay linked to the exact-state successor.
        "#13238",
        "containment_removal_condition",
    ] {
        assert!(
            block.contains(required),
            "QuickORM policy no longer records {required:?}; containment prose and the \
             extractor in crates/perl-workspace/src/semantic/\
             generated_member_extractor_quickorm.rs have drifted apart"
        );
    }

    for blocked in [
        "repeated_quickorm_import_events_in_one_package",
        "quickorm_unimport",
        "bareword_or_call_type_value_import",
    ] {
        assert!(
            block.contains(blocked),
            "QuickORM policy no longer lists {blocked:?} as a blocked form"
        );
    }

    Ok(())
}

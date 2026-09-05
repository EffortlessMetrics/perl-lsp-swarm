//! Focused proof for the #8843 `perl-ast-v2` lifecycle audit.
//!
//! Shift-left order: the fifteen falsifier shapes #8843 names come first, each
//! as an explicit mutation of the real committed bytes that must fail closed
//! with its own reason, then the positive coverage obligations, then determinism
//! and pin binding.
//!
//! A wrong implementation these tests are built to catch: a validator that
//! accepts anything shaped like JSON and only checks the digest. It would pass
//! every happy-path assertion in this file while rejecting none of the fifteen —
//! and, worse, it would let the inventory rot silently the moment the crate it
//! describes changed, which is the exact failure #8843 exists to prevent.

use super::*;

fn repo_root_for_tests() -> Result<PathBuf> {
    workspace_root()
}

fn manifest_path() -> Result<PathBuf> {
    Ok(repo_root_for_tests()?.join(MANIFEST_RELATIVE_PATH))
}

fn real_value() -> Result<Value> {
    let bytes = std::fs::read(manifest_path()?)
        .with_context(|| "failed to read the workspace audit contract for tests")?;
    serde_json::from_slice(&bytes).with_context(|| "workspace audit contract is not valid JSON")
}

/// Run the same law set `load_audit_from` runs, minus the digest pin — every
/// mutation below deliberately changes the bytes.
fn validate(value: &Value) -> Result<()> {
    let root = repo_root_for_tests()?;
    let manifest: Manifest =
        serde_json::from_value(value.clone()).with_context(|| "strict deserialization failed")?;
    validate_manifest(&manifest)?;
    reconcile_with_source(&manifest, &root)
}

fn assert_rejected(value: &Value, needle: &str) -> Result<()> {
    match validate(value) {
        Ok(()) => bail!("expected rejection containing '{needle}', but validation passed"),
        Err(err) => {
            let rendered = format!("{err:#}");
            assert!(
                rendered.contains(needle),
                "rejection message mismatch\n  wanted substring: {needle}\n  actual: {rendered}"
            );
            Ok(())
        }
    }
}

fn array_mut<'a>(value: &'a mut Value, key: &str) -> Result<&'a mut Vec<Value>> {
    value
        .get_mut(key)
        .and_then(Value::as_array_mut)
        .ok_or_else(|| color_eyre::eyre::eyre!("audit contract has no `{key}` array"))
}

fn row_mut<'a>(value: &'a mut Value, key: &str, id_field: &str, id: &str) -> Result<&'a mut Value> {
    array_mut(value, key)?
        .iter_mut()
        .find(|row| row.get(id_field).and_then(Value::as_str) == Some(id))
        .ok_or_else(|| color_eyre::eyre::eyre!("no `{key}` row with {id_field} = {id}"))
}

// ---------------------------------------------------------------------------
// The committed artifact itself.
// ---------------------------------------------------------------------------

#[test]
fn the_committed_audit_contract_loads_and_reconciles() -> Result<()> {
    let audit = load_audit()?;
    assert_eq!(audit.ruling(), "absorb");
    assert_eq!(audit.public_item_count(), 39);
    assert_eq!(audit.reexport_count(), 6);
    assert!(audit.consumer_count() >= 30);
    Ok(())
}

// ---------------------------------------------------------------------------
// Falsifier 1: package/re-export existence alone selects RETAIN.
// ---------------------------------------------------------------------------

#[test]
fn retain_without_independent_lifecycle_evidence_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    value["ruling"]["ruling"] = Value::String("retain".to_string());
    assert_rejected(&value, "must name independent-lifecycle evidence")
}

#[test]
fn the_committed_ruling_names_no_independent_lifecycle_evidence() -> Result<()> {
    // The absorb ruling is only honest if the independent-lifecycle column is
    // genuinely empty. A ruling that said absorb while listing such evidence
    // would be contradicting itself.
    let value = real_value()?;
    let independent = value["ruling"]["independent_lifecycle_evidence_ids"]
        .as_array()
        .ok_or_else(|| color_eyre::eyre::eyre!("missing independent evidence array"))?;
    assert!(independent.is_empty(), "absorb ruling must not claim independent-lifecycle evidence");
    Ok(())
}

// ---------------------------------------------------------------------------
// Falsifier 2 + 4: a dependency whose exact symbol use is unknown is not an
// inventoried consumer.
// ---------------------------------------------------------------------------

#[test]
fn a_gating_consumer_without_symbols_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    row_mut(&mut value, "consumers", "consumer_id", "c:parser-core-trivia")?["symbols"] =
        Value::Array(vec![]);
    assert_rejected(&value, "names no symbols")
}

// ---------------------------------------------------------------------------
// Falsifier 3: parser-core's public re-export is missed.
// ---------------------------------------------------------------------------

#[test]
fn dropping_the_parser_core_reexport_consumer_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    // Drop the re-export row too, so referential integrity does not fire first.
    // What must catch this is the denominator direction: the scan still finds
    // the file, and nothing in the manifest accounts for it.
    let reexports = array_mut(&mut value, "reexport_paths")?;
    reexports.retain(|row| {
        row.get("reexport_id").and_then(Value::as_str) != Some("rx:parser-core-engine")
    });
    let consumers = array_mut(&mut value, "consumers")?;
    consumers.retain(|row| {
        row.get("consumer_id").and_then(Value::as_str) != Some("c:parser-core-engine-reexport")
    });
    assert_rejected(&value, "crates/perl-parser-core/src/engine/mod.rs")
}

#[test]
fn every_public_reexport_path_is_inventoried() -> Result<()> {
    let value = real_value()?;
    let paths: BTreeSet<&str> = value["reexport_paths"]
        .as_array()
        .ok_or_else(|| color_eyre::eyre::eyre!("missing reexport_paths"))?
        .iter()
        .filter_map(|row| row.get("path").and_then(Value::as_str))
        .collect();
    for expected in [
        "perl_ast::v2",
        "perl_parser_core::engine::ast_v2",
        "perl_parser_core::ast_v2",
        "perl_parser_core::{DiagnosticId, MissingKind}",
        "perl_parser::ast_v2",
        "perl_parser::compat::ast_v2",
    ] {
        assert!(paths.contains(expected), "re-export path {expected} is not inventoried");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Falsifier 5: the unique ErrorRef / MissingKind / NodeId propositions are lost
// under "abbreviated".
// ---------------------------------------------------------------------------

#[test]
fn the_unique_experimental_propositions_stay_explicit() -> Result<()> {
    let value = real_value()?;
    let items = value["public_items"]
        .as_array()
        .ok_or_else(|| color_eyre::eyre::eyre!("missing public_items"))?;
    for path in [
        "perl_ast_v2::NodeKind::ErrorRef",
        "perl_ast_v2::NodeKind::Missing",
        "perl_ast_v2::MissingKind",
        "perl_ast_v2::NodeId",
        "perl_ast_v2::DiagnosticId",
        "perl_ast_v2::NodeIdGenerator",
    ] {
        let row = items
            .iter()
            .find(|row| row.get("path").and_then(Value::as_str) == Some(path))
            .ok_or_else(|| color_eyre::eyre::eyre!("no row for {path}"))?;
        assert_eq!(
            row["v1_relation"].as_str(),
            Some("unique"),
            "{path} must stay an explicit unique proposition, not a gap"
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Falsifier 6: the same variant name is called parity without a field
// comparison.
// ---------------------------------------------------------------------------

#[test]
fn a_node_carrying_variant_may_not_claim_equivalence_by_name() -> Result<()> {
    // Binary is field-identical to the production variant by name and type, and
    // is still `divergent` because its children are v2 Nodes. If someone
    // "corrects" it to equivalent, the parity note no longer matches the row —
    // this test pins the reviewed judgement itself.
    let value = real_value()?;
    let items = value["public_items"]
        .as_array()
        .ok_or_else(|| color_eyre::eyre::eyre!("missing public_items"))?;
    for path in [
        "perl_ast_v2::NodeKind::Binary",
        "perl_ast_v2::NodeKind::Unary",
        "perl_ast_v2::NodeKind::Program",
        "perl_ast_v2::NodeKind::Block",
        "perl_ast_v2::NodeKind::If",
        "perl_ast_v2::NodeKind::Error",
    ] {
        let row = items
            .iter()
            .find(|row| row.get("path").and_then(Value::as_str) == Some(path))
            .ok_or_else(|| color_eyre::eyre::eyre!("no row for {path}"))?;
        assert_eq!(
            row["v1_relation"].as_str(),
            Some("divergent"),
            "{path} shares a name with a production variant but carries a different node contract"
        );
    }
    Ok(())
}

#[test]
fn a_parity_row_naming_a_nonexistent_production_variant_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    row_mut(&mut value, "public_items", "item_id", "item:node-kind.binary")?["v1_counterpart"] =
        Value::String("ThisVariantDoesNotExist".to_string());
    assert_rejected(&value, "not a current `perl_ast::NodeKind` variant")
}

#[test]
fn a_relation_claim_without_a_counterpart_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    row_mut(&mut value, "public_items", "item_id", "item:node-kind.binary")?["v1_counterpart"] =
        Value::Null;
    assert_rejected(&value, "without naming a production counterpart")
}

#[test]
fn a_unique_row_smuggling_in_a_counterpart_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    row_mut(&mut value, "public_items", "item_id", "item:node-kind.errorref")?["v1_counterpart"] =
        Value::String("Error".to_string());
    assert_rejected(&value, "yet names production counterpart")
}

// ---------------------------------------------------------------------------
// Falsifier 7 + 8: publish intent represented as adoption; unknown external
// evidence becoming zero consumers.
// ---------------------------------------------------------------------------

#[test]
fn a_ruling_resting_on_download_volume_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    array_mut(&mut value, "external_evidence")?;
    value["ruling"]["evidence_ids"]
        .as_array_mut()
        .ok_or_else(|| color_eyre::eyre::eyre!("missing evidence_ids"))?
        .push(Value::String("ev:download-volume".to_string()));
    assert_rejected(&value, "download volume is not adoption")
}

#[test]
fn unknown_external_evidence_is_recorded_as_unavailable_not_as_zero() -> Result<()> {
    let value = real_value()?;
    let rows = value["external_evidence"]
        .as_array()
        .ok_or_else(|| color_eyre::eyre::eyre!("missing external_evidence"))?;
    let unavailable = rows
        .iter()
        .find(|row| row.get("class").and_then(Value::as_str) == Some("unavailable"))
        .ok_or_else(|| {
            color_eyre::eyre::eyre!(
                "the audit must record the limits of its external instruments as an explicit \
                 `unavailable` row rather than letting silence read as zero consumers"
            )
        })?;
    assert!(
        !unavailable["observed"].as_str().unwrap_or_default().is_empty(),
        "an unavailable row must still say what could not be observed"
    );
    // And the ruling must actually carry that unknown into its reasoning.
    assert!(
        value["ruling"]["evidence_ids"].as_array().is_some_and(|ids| ids
            .iter()
            .any(|id| id.as_str() == Some("ev:external-source-search"))),
        "the ruling must consume the unavailable-instrument row, not ignore it"
    );
    Ok(())
}

#[test]
fn undated_external_evidence_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    row_mut(&mut value, "external_evidence", "evidence_id", "ev:reverse-dependencies")?["observed_at"] =
        Value::String("   ".to_string());
    assert_rejected(&value, "records no observation date")
}

// ---------------------------------------------------------------------------
// Falsifier 9: docs/tests references counted as production implementation use.
// ---------------------------------------------------------------------------

#[test]
fn a_docs_only_reference_is_not_classified_as_production_use() -> Result<()> {
    // crates/perl-lexer/src/tokenizer/mod.rs mentions the package only in a doc
    // comment, and says the module does NOT depend on it. Promoting that row to
    // production_implementation must fail, because the gating check then demands
    // symbols it cannot have.
    let mut value = real_value()?;
    let row = row_mut(&mut value, "consumers", "consumer_id", "c:lexer-tokenizer-doc")?;
    row["role"] = Value::String("production_implementation".to_string());
    assert_rejected(&value, "names no symbols")
}

#[test]
fn the_unused_lexer_dev_dependency_is_recorded_rather_than_read_as_use() -> Result<()> {
    let value = real_value()?;
    let row = value["consumers"]
        .as_array()
        .ok_or_else(|| color_eyre::eyre::eyre!("missing consumers"))?
        .iter()
        .find(|row| row.get("consumer_id").and_then(Value::as_str) == Some("c:lexer-manifest"))
        .ok_or_else(|| color_eyre::eyre::eyre!("no c:lexer-manifest row"))?;
    let proposition = row["proposition"].as_str().unwrap_or_default();
    assert!(
        proposition.contains("no matching use"),
        "the lexer row must record that the declared dev-dependency has no matching use"
    );
    assert_eq!(row["role"].as_str(), Some("package_dependency"));
    Ok(())
}

// ---------------------------------------------------------------------------
// Falsifier 10: a new public item or re-export lands without an inventory row.
// ---------------------------------------------------------------------------

#[test]
fn a_public_item_with_no_inventory_row_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    let items = array_mut(&mut value, "public_items")?;
    items.retain(|row| {
        row.get("item_id").and_then(Value::as_str) != Some("item:node-kind.errorref")
    });
    assert_rejected(&value, "with no inventory row")
}

#[test]
fn an_inventory_row_describing_a_removed_public_item_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    let mut ghost =
        row_mut(&mut value, "public_items", "item_id", "item:node-kind.binary")?.clone();
    ghost["item_id"] = Value::String("item:ghost".to_string());
    ghost["path"] = Value::String("perl_ast_v2::NodeKind::RemovedLongAgo".to_string());
    array_mut(&mut value, "public_items")?.push(ghost);
    assert_rejected(&value, "no longer exists in")
}

#[test]
fn a_changed_public_shape_under_an_unmoved_row_is_rejected() -> Result<()> {
    // The realistic wrong implementation: someone adds a field to a variant and
    // leaves the row alone because the name still matches.
    let mut value = real_value()?;
    row_mut(&mut value, "public_items", "item_id", "item:node-kind.variable")?["derived_shape"] =
        Value::String("variant Variable { sigil: String }".to_string());
    assert_rejected(&value, "changed shape without moving its row")
}

#[test]
fn a_row_misdeclaring_an_item_kind_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    row_mut(&mut value, "public_items", "item_id", "item:node")?["kind"] =
        Value::String("enum".to_string());
    assert_rejected(&value, "in source but the inventory calls it a")
}

// ---------------------------------------------------------------------------
// Falsifier 11: a new direct consumer does not change the denominator.
// ---------------------------------------------------------------------------

#[test]
fn a_referencing_file_with_no_consumer_row_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    // c:policy-topology is referenced by no public-item row, so referential
    // integrity stays silent and the denominator direction is what must fire.
    let consumers = array_mut(&mut value, "consumers")?;
    consumers
        .retain(|row| row.get("consumer_id").and_then(Value::as_str) != Some("c:policy-topology"));
    assert_rejected(&value, "references the audited package but has no consumer row")
}

#[test]
fn a_gating_row_naming_a_file_that_does_not_reference_the_package_is_rejected() -> Result<()> {
    // Promote a documentation row, which sits outside the scan roots, to a
    // production role. Claiming a real API consumer must require the scan to
    // actually find one — a prose file cannot be talked into being production
    // use by relabelling it.
    let mut value = real_value()?;
    let row = row_mut(&mut value, "consumers", "consumer_id", "c:ast-contract-doc")?;
    row["role"] = Value::String("production_implementation".to_string());
    row["symbols"] = Value::Array(vec![Value::String("Node".to_string())]);
    assert_rejected(&value, "finds no whole-word reference there")
}

#[test]
fn the_consumer_scan_actually_finds_the_known_production_sites() -> Result<()> {
    // Positive control for the scan itself: if the walker silently found nothing,
    // every "no row" test above would pass vacuously.
    let scanned = derive_reference_files(&repo_root_for_tests()?)?;
    for expected in [
        "crates/perl-parser-core/src/engine/parser_context.rs",
        "crates/perl-parser-core/src/engine/error/context_impls.rs",
        "crates/perl-parser-core/src/syntax/error/recovery.rs",
        "crates/perl-parser-core/src/tokens/trivia.rs",
        "crates/perl-ast/src/lib.rs",
        "crates/perl-ast/tests/comprehensive_unit_tests.rs",
        "Cargo.toml",
        "policy/repository-topology.toml",
    ] {
        assert!(scanned.contains(expected), "scan missed {expected}");
    }
    assert!(scanned.len() >= 25, "scan found only {} files", scanned.len());
    Ok(())
}

#[test]
fn the_scan_finds_consumers_that_reach_the_package_through_the_canonical_path() -> Result<()> {
    // perl_ast::v2 contains none of the other three tokens. Before that token
    // was added the scan silently dropped both perl-ast test suites, so this is
    // a regression control for a hole this audit actually had.
    let scanned = derive_reference_files(&repo_root_for_tests()?)?;
    assert!(scanned.contains("crates/perl-ast/tests/additional_unit_tests.rs"));
    assert!(scanned.contains("crates/perl-ast/tests/comprehensive_unit_tests.rs"));
    Ok(())
}

// ---------------------------------------------------------------------------
// Falsifier 12: host paths and map order alter output.
// ---------------------------------------------------------------------------

#[test]
fn the_canonical_digest_is_key_order_invariant() -> Result<()> {
    let value = real_value()?;
    let reserialized: Value = serde_json::from_str(&serde_json::to_string(&value)?)?;
    assert_eq!(canonical_digest(&value)?, canonical_digest(&reserialized)?);
    Ok(())
}

#[test]
fn the_derivation_is_deterministic_across_runs() -> Result<()> {
    let root = repo_root_for_tests()?;
    let source = std::fs::read_to_string(root.join(V2_SOURCE_RELATIVE_PATH))?;
    assert_eq!(derive_public_items(&source)?, derive_public_items(&source)?);
    assert_eq!(derive_reference_files(&root)?, derive_reference_files(&root)?);
    Ok(())
}

#[test]
fn scanned_paths_are_repository_relative_with_forward_slashes() -> Result<()> {
    for file in derive_reference_files(&repo_root_for_tests()?)? {
        assert!(!file.contains('\\'), "{file} carries a host path separator");
        assert!(!file.starts_with('/'), "{file} is not repository-relative");
    }
    Ok(())
}

#[test]
fn a_consumer_row_using_a_host_path_separator_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    row_mut(&mut value, "consumers", "consumer_id", "c:parser-core-trivia")?["file"] =
        Value::String("crates\\perl-parser-core\\src\\tokens\\trivia.rs".to_string());
    assert_rejected(&value, "host path separator")
}

// ---------------------------------------------------------------------------
// Falsifier 13: the lifecycle ruling changes without evidence movement.
// ---------------------------------------------------------------------------

#[test]
fn a_ruling_referencing_unknown_evidence_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    value["ruling"]["evidence_ids"]
        .as_array_mut()
        .ok_or_else(|| color_eyre::eyre::eyre!("missing evidence_ids"))?
        .push(Value::String("ev:invented".to_string()));
    assert_rejected(&value, "ruling references unknown evidence")
}

#[test]
fn a_ruling_without_a_reversal_condition_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    value["ruling"]["reversal_condition"] = Value::String("  ".to_string());
    assert_rejected(&value, "states no reversal condition")
}

#[test]
fn a_ruling_without_a_compatibility_window_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    value["ruling"]["compatibility_window"] = Value::String(String::new());
    assert_rejected(&value, "states no compatibility window")
}

// ---------------------------------------------------------------------------
// Falsifier 14 + 15: ABSORB read as a semantic merge; RETAIN without rationale.
// ---------------------------------------------------------------------------

#[test]
fn absorb_is_stated_as_a_package_move_not_a_semantic_merge() -> Result<()> {
    let value = real_value()?;
    let rationale = value["ruling"]["rationale"].as_str().unwrap_or_default();
    assert!(
        rationale.contains("does not mean merging v1 and v2 semantics"),
        "the absorb ruling must say in the artifact that it is not a v1/v2 semantic merge"
    );
    assert!(
        rationale.contains("does not authorize deleting"),
        "the absorb ruling must say that it does not authorize package deletion"
    );
    Ok(())
}

#[test]
fn the_claim_ceiling_cannot_be_promoted() -> Result<()> {
    let mut value = real_value()?;
    value["claim_ceiling"] = Value::String("migration_authority".to_string());
    assert_rejected(&value, "claim ceiling must remain")
}

#[test]
fn successor_wake_conditions_bind_removal_behind_the_window() -> Result<()> {
    let audit = load_audit()?;
    for successor in [8844u64, 8845, 8847] {
        assert!(audit.wake_event(successor).is_some(), "no wake event for #{successor}");
    }
    let removal =
        audit.wake_event(8847).ok_or_else(|| color_eyre::eyre::eyre!("no #8847 wake event"))?;
    assert!(
        removal.contains("re-run") || removal.contains("re-observe"),
        "#8847 must re-observe registry evidence rather than inherit this audit's snapshot"
    );
    assert!(!audit.compatibility_window().is_empty());
    assert!(!audit.reversal_condition().is_empty());
    Ok(())
}

#[test]
fn a_ruling_with_no_successor_wake_condition_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    value["successor_wake_conditions"] = Value::Array(vec![]);
    assert_rejected(&value, "defines no successor wake condition")
}

// ---------------------------------------------------------------------------
// Schema strictness, referential integrity, and structural laws.
// ---------------------------------------------------------------------------

#[test]
fn an_unknown_field_fails_closed() -> Result<()> {
    let mut value = real_value()?;
    value["speculative_new_column"] = Value::Bool(true);
    match validate(&value) {
        Ok(()) => bail!("an unknown top-level field must fail closed, not be ignored"),
        Err(_) => Ok(()),
    }
}

#[test]
fn a_duplicate_public_item_id_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    let duplicate = row_mut(&mut value, "public_items", "item_id", "item:node")?.clone();
    array_mut(&mut value, "public_items")?.push(duplicate);
    assert_rejected(&value, "duplicate public item id")
}

#[test]
fn a_public_item_referencing_an_unknown_consumer_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    row_mut(&mut value, "public_items", "item_id", "item:node")?["consumer_ids"] =
        Value::Array(vec![Value::String("c:does-not-exist".to_string())]);
    assert_rejected(&value, "references unknown consumer")
}

#[test]
fn a_public_item_with_no_consumer_at_all_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    row_mut(&mut value, "public_items", "item_id", "item:node")?["consumer_ids"] =
        Value::Array(vec![]);
    assert_rejected(&value, "names no consumer")
}

#[test]
fn a_value_outside_a_closed_vocabulary_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    row_mut(&mut value, "public_items", "item_id", "item:node")?["range_disposition"] =
        Value::String("probably_fine".to_string());
    assert_rejected(&value, "outside the closed v1 vocabulary")
}

#[test]
fn a_stale_reexport_site_line_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    row_mut(&mut value, "reexport_paths", "reexport_id", "rx:perl-ast-v2")?["site"] =
        Value::String("crates/perl-ast/src/lib.rs:1".to_string());
    assert_rejected(&value, "no longer mentions the audited")
}

#[test]
fn a_stale_package_surface_site_line_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    row_mut(&mut value, "package_surfaces", "surface_id", "ps:workspace-member")?["site"] =
        Value::String("Cargo.toml:1".to_string());
    assert_rejected(&value, "no longer mentions the audited")
}

#[test]
fn an_audit_claiming_no_limitations_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    value["limitations"] = Value::Array(vec![]);
    assert_rejected(&value, "records no limitation")
}

#[test]
fn a_derivation_block_that_disagrees_with_the_instrument_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    value["derivation"]["gating_scan_roots"] =
        Value::Array(vec![Value::String("crates".to_string())]);
    assert_rejected(&value, "but the derivation scans")
}

// ---------------------------------------------------------------------------
// Instrument properties.
// ---------------------------------------------------------------------------

#[test]
fn whole_word_matching_does_not_confuse_the_audit_module_with_a_consumer() -> Result<()> {
    // `ast_v2` is a prefix of this module's own name. Without a trailing word
    // boundary the scan reports the instrument as a consumer of its subject.
    assert!(!mentions_audited_package("pub mod ast_v2_lifecycle_audit;"));
    assert!(!mentions_audited_package("use crate::ast_v2_lifecycle_audit::load_audit;"));
    assert!(mentions_audited_package("pub use engine::ast_v2;"));
    assert!(mentions_audited_package("use perl_ast_v2::Node;"));
    assert!(mentions_audited_package("use perl_ast::v2::NodeKind;"));
    assert!(mentions_audited_package("perl-ast-v2 = { workspace = true }"));
    assert!(!mentions_audited_package("use perl_ast::NodeKind;"));
    assert!(!mentions_audited_package("perl-ast-v2-experimental = \"1\""));
    Ok(())
}

#[test]
fn instrument_self_exclusion_is_exactly_two_named_files() -> Result<()> {
    // The self-exclusion is the one place the scan is allowed to look away, so
    // it is pinned. If it ever grows, that is a way to hide a real consumer.
    assert_eq!(INSTRUMENT_SELF_FILES.len(), 2);
    assert_eq!(
        INSTRUMENT_SELF_FILES,
        ["xtask/src/ast_v2_lifecycle_audit.rs", "xtask/src/ast_v2_lifecycle_audit_tests.rs"]
    );
    let scanned = derive_reference_files(&repo_root_for_tests()?)?;
    for excluded in INSTRUMENT_SELF_FILES {
        assert!(
            !scanned.contains(excluded),
            "{excluded} must be excluded as instrument self-reference"
        );
    }
    Ok(())
}

#[test]
fn excluded_directories_hide_no_reference_to_the_audited_package() -> Result<()> {
    // The exclusion list matches a bare directory name at any depth, which is
    // right for build output but could in principle skip a real subtree. That is
    // not hypothetical here: `crates/perl-lexer/tests/fixtures/simd_feature_selection/
    // nested/target/hidden.rs` is a checked-in fixture living under an excluded
    // name. So the assumption is checked rather than asserted — walk the scan
    // roots with NO exclusions and prove nothing excluded references the package.
    let root = repo_root_for_tests()?;
    let scanned = derive_reference_files(&root)?;
    let mut hidden = Vec::new();

    for scan_root in ["crates", "xtask", "policy"] {
        for entry in walkdir::WalkDir::new(root.join(scan_root)).into_iter().filter_map(|e| e.ok())
        {
            if !entry.file_type().is_file() {
                continue;
            }
            let path = entry.path();
            let is_rust_or_toml =
                matches!(path.extension().and_then(|ext| ext.to_str()), Some("rs") | Some("toml"));
            if !is_rust_or_toml {
                continue;
            }
            let Ok(relative) = path.strip_prefix(&root) else {
                continue;
            };
            let relative = relative.to_string_lossy().replace('\\', "/");
            // Build output is genuinely not source; only look at tracked-looking
            // paths the repository would actually carry.
            if relative.contains("/target/debug/") || relative.contains("/target/release/") {
                continue;
            }
            if scanned.contains(&relative) || INSTRUMENT_SELF_FILES.contains(&relative.as_str()) {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(path) else {
                continue;
            };
            if mentions_audited_package(&text) {
                hidden.push(relative);
            }
        }
    }

    assert!(
        hidden.is_empty(),
        "these files reference the audited package but the scan never sees them, so they could \
         never be reconciled: {hidden:?}"
    );
    Ok(())
}

/// Build a minimal tree with the shape `derive_reference_files` requires, so
/// symlink behaviour can be proven without mutating the real repository.
#[cfg(unix)]
fn synthetic_root() -> Result<tempfile::TempDir> {
    let dir = tempfile::tempdir()?;
    let root = dir.path();
    std::fs::create_dir_all(root.join("crates/probe/src"))?;
    std::fs::create_dir_all(root.join("xtask/src"))?;
    std::fs::create_dir_all(root.join("policy"))?;
    std::fs::write(root.join("Cargo.toml"), "[workspace]\n")?;
    std::fs::write(root.join("policy/empty.toml"), "\n")?;
    std::fs::write(root.join("xtask/src/lib.rs"), "// nothing\n")?;
    Ok(dir)
}

#[cfg(unix)]
#[test]
fn a_symlink_hiding_a_reference_fails_closed_but_a_harmless_one_does_not() -> Result<()> {
    // Three cases, because the guard has to be narrow to be usable: it must
    // catch a link that hides a consumer, must NOT block an unrelated link (or
    // it becomes a gate on work this audit has no business gating), and must
    // refuse a link it cannot read at all rather than assume it is harmless.
    use std::os::unix::fs::symlink;

    // A link whose target references the package: the scan would skip it
    // silently, so it must fail closed.
    let dir = synthetic_root()?;
    let root = dir.path();
    std::fs::write(root.join("crates/probe/real.rs"), "use perl_ast_v2::Node;\n")?;
    symlink(root.join("crates/probe/real.rs"), root.join("crates/probe/src/linked.rs"))?;
    let err = derive_reference_files(root)
        .err()
        .ok_or_else(|| color_eyre::eyre::eyre!("a reference-hiding symlink must fail closed"))?;
    assert!(
        format!("{err:#}").contains("whose target references the audited package"),
        "unexpected rejection: {err:#}"
    );

    // An unrelated link must not block the audit.
    let dir = synthetic_root()?;
    let root = dir.path();
    std::fs::write(root.join("crates/probe/plain.rs"), "pub fn unrelated() -> u8 { 7 }\n")?;
    symlink(root.join("crates/probe/plain.rs"), root.join("crates/probe/src/linked.rs"))?;
    let scanned = derive_reference_files(root)?;
    assert!(scanned.is_empty(), "an unrelated symlink must not enter the denominator: {scanned:?}");

    // A link that cannot be read cannot be shown harmless.
    let dir = synthetic_root()?;
    let root = dir.path();
    symlink(root.join("crates/probe/does-not-exist.rs"), root.join("crates/probe/src/broken.rs"))?;
    let err = derive_reference_files(root)
        .err()
        .ok_or_else(|| color_eyre::eyre::eyre!("an unreadable symlink must fail closed"))?;
    assert!(format!("{err:#}").contains("cannot be read"), "unexpected rejection: {err:#}");

    Ok(())
}

#[test]
fn the_prose_report_summary_cannot_drift_from_the_inventory() -> Result<()> {
    // This exists because the drift already happened. Adding the missed
    // diagnostic-id consumer moved the denominator 36 -> 37, and the report's
    // summary table and the #8845 wake text kept the old figures — the very
    // numbers a successor reads to size its migration set. The loader checks
    // rows against source but has no opinion about prose, so nothing caught it.
    //
    // The wake text no longer restates counts at all; a duplicate that cannot
    // drift is better than one that is checked. The report's table is a genuine
    // human summary and stays, so it is pinned here instead.
    let value = real_value()?;
    let report = std::fs::read_to_string(
        repo_root_for_tests()?.join(".spec/8843-ast-v2-lifecycle-audit/decision.md"),
    )?;

    let count_of = |key: &str| -> Result<usize> {
        value[key]
            .as_array()
            .map(Vec::len)
            .ok_or_else(|| color_eyre::eyre::eyre!("`{key}` is not an array"))
    };

    for (label, key) in [
        ("public items (incl. every enum variant)", "public_items"),
        ("public re-export paths", "reexport_paths"),
        ("consumer rows", "consumers"),
        ("package/release surfaces", "package_surfaces"),
        ("external evidence rows", "external_evidence"),
    ] {
        let expected = count_of(key)?;
        let row = format!("| {label} | {expected} |");
        assert!(
            report.contains(&row),
            "the report's summary table has drifted from the inventory.\n  expected row: {row}\n  \
             the manifest holds {expected} `{key}` entries"
        );
    }

    // And the wake text must not reintroduce a hardcoded count.
    for wake in value["successor_wake_conditions"]
        .as_array()
        .ok_or_else(|| color_eyre::eyre::eyre!("missing successor_wake_conditions"))?
    {
        let text = wake["wake_event"].as_str().unwrap_or_default();
        for spelled in ["four production", "five public_reexport", "seven test_fixture"] {
            assert!(
                !text.contains(spelled),
                "wake text restates a row count (`{spelled}`); point at the rows instead, because \
                 a duplicated count drifts the moment the inventory moves"
            );
        }
    }
    Ok(())
}

#[test]
fn an_unmodelled_public_item_stops_the_audit_instead_of_vanishing() -> Result<()> {
    // The single worst defect this instrument could have. The first revision's
    // derivation matched only type/struct/enum/impl and skipped everything else,
    // so a crate containing all six of these produced ZERO rows and ZERO errors
    // — the "a public item cannot land without a row" claim was silently false
    // for every item shape the match did not name.
    for (label, source) in [
        ("free fn", "pub fn helper(x: u8) -> u8 { x }"),
        ("const", "pub const LIMIT: usize = 4;"),
        ("static", "pub static NAME: &str = \"n\";"),
        ("trait", "pub trait Visitor { fn visit(&self); }"),
        ("union", "pub union Bits { pub a: u8 }"),
        ("pub use", "pub use std::collections::HashMap;"),
        ("extern crate", "pub extern crate serde;"),
    ] {
        match derive_public_items(source) {
            Ok(items) => bail!(
                "a public {label} must stop the audit, but it produced {} rows and no error",
                items.len()
            ),
            Err(err) => assert!(
                format!("{err:#}").contains("cannot model this public item"),
                "{label} was rejected for the wrong reason: {err:#}"
            ),
        }
    }
    Ok(())
}

#[test]
fn a_public_inline_module_is_walked_rather_than_ignored() -> Result<()> {
    // Nesting must not hide a public item, and a non-inline `pub mod` must fail
    // closed because its body lives in a file this derivation never reads.
    let nested = derive_public_items("pub mod inner { pub type Hidden = u8; }")?;
    assert_eq!(nested.len(), 1, "expected the nested item, got {nested:?}");
    assert_eq!(nested[0].path, "perl_ast_v2::inner::Hidden");

    match derive_public_items("pub mod elsewhere;") {
        Ok(items) => bail!("a non-inline `pub mod` must fail closed, got {items:?}"),
        Err(err) => {
            assert!(format!("{err:#}").contains("no inline body"), "{err:#}");
            Ok(())
        }
    }
}

#[test]
fn a_private_item_the_derivation_cannot_model_is_still_skipped() -> Result<()> {
    // The fail-closed arm must not fire on private helpers, or every ordinary
    // edit to the crate would break the audit for no reason.
    let derived = derive_public_items("fn helper() {}\nconst LIMIT: u8 = 1;\nmod inner { }")?;
    assert!(derived.is_empty(), "private items must be skipped, got {derived:?}");
    Ok(())
}

#[test]
fn breaking_changes_that_touch_no_field_still_move_the_shape() -> Result<()> {
    // Four collisions the first shape renderer had. Each pair is a real,
    // breaking public-API change that left the shape string byte-identical, so
    // it could land under an unmoved inventory row.
    let shape_of = |src: &str| -> Result<String> {
        let derived = derive_public_items(src)?;
        derived
            .iter()
            .find(|item| item.kind == "struct" || item.kind == "enum")
            .map(|item| item.shape.clone())
            .ok_or_else(|| color_eyre::eyre::eyre!("no struct or enum derived from {src}"))
    };

    for (label, left, right) in [
        (
            "non_exhaustive",
            "pub struct S { pub a: u8 }",
            "#[non_exhaustive]\npub struct S { pub a: u8 }",
        ),
        ("array length", "pub struct S { pub a: [u8; 4] }", "pub struct S { pub a: [u8; 8] }"),
        ("lifetime param", "pub struct S { pub a: u8 }", "pub struct S<'a> { pub a: &'a u8 }"),
        (
            "generic bound",
            "pub struct S<T> { pub a: T }",
            "pub struct S<T: Clone + Send> { pub a: T }",
        ),
    ] {
        assert_ne!(
            shape_of(left)?,
            shape_of(right)?,
            "{label}: a breaking change left the shape identical, so it could land under an \
             unmoved row"
        );
    }
    Ok(())
}

#[test]
fn a_real_code_consumer_cannot_be_downgraded_to_a_prose_mention() -> Result<()> {
    // The inverse of falsifier 9, and the hole the first revision had: the
    // non-gating roles skip both the symbol requirement and the scan
    // requirement, so relabelling a real production consumer `docs_reference`
    // and emptying its symbols escaped every check.
    let mut value = real_value()?;
    let row = row_mut(&mut value, "consumers", "consumer_id", "c:parser-core-trivia")?;
    row["role"] = Value::String("docs_reference".to_string());
    row["symbols"] = Value::Array(vec![]);
    assert_rejected(&value, "cannot be downgraded to a prose mention")
}

#[test]
fn naming_the_crate_in_policy_data_is_still_allowed_to_be_inventory() -> Result<()> {
    // The downgrade check must not overreach the other way. A TOML policy row
    // and a crate-name string in a coverage fixture genuinely are inventory
    // references, not API use; forcing them to a gating role would be just as
    // wrong as letting a real consumer hide.
    assert!(!references_package_api_in_code("name = \"perl-ast-v2\"\n", "policy/x.toml"));
    assert!(!references_package_api_in_code(
        "let p = \"crates/perl-ast-v2/src/lib.rs\";\n",
        "xtask/tests/fixture.rs"
    ));
    assert!(!references_package_api_in_code("// see perl_ast_v2::Node\n", "crates/a/src/lib.rs"));
    // But real API use in code is caught, including through the unqualified
    // re-export that names no package token at all.
    assert!(references_package_api_in_code("use perl_ast_v2::Node;\n", "crates/a/src/lib.rs"));
    assert!(references_package_api_in_code("use perl_ast::v2::NodeKind;\n", "crates/a/src/lib.rs"));
    assert!(references_package_api_in_code(
        "use perl_parser_core::DiagnosticId;\n",
        "crates/a/tests/t.rs"
    ));
    Ok(())
}

#[test]
fn the_unqualified_reexport_consumer_is_found_and_inventoried() -> Result<()> {
    // This file reaches the v2 `DiagnosticId` through perl_parser_core's
    // unqualified re-export and contains none of the four package tokens. The
    // first revision of this inventory recorded that path as a blind spot and
    // then asserted it was empty; it was not. Both halves are pinned here: the
    // scan must find the file, and the manifest must carry a row for it.
    let scanned = derive_reference_files(&repo_root_for_tests()?)?;
    assert!(
        scanned.contains("crates/perl-parser-core/tests/diagnostic_id_tests.rs"),
        "the unqualified re-export path must be detected, not merely documented"
    );

    let value = real_value()?;
    let row = value["consumers"]
        .as_array()
        .ok_or_else(|| color_eyre::eyre::eyre!("missing consumers"))?
        .iter()
        .find(|row| {
            row.get("file").and_then(Value::as_str)
                == Some("crates/perl-parser-core/tests/diagnostic_id_tests.rs")
        })
        .ok_or_else(|| color_eyre::eyre::eyre!("no consumer row for the diagnostic-id test"))?;
    assert_eq!(row["role"].as_str(), Some("test_fixture"));
    Ok(())
}

#[test]
fn a_path_qualified_derive_is_recorded_rather_than_silently_dropped() -> Result<()> {
    // Adding `serde::Serialize` is a real public-contract change, and the audit
    // rows assert `serialization_disposition: not_represented`. Rendering only
    // the last path segment would drop it and leave the shape byte-identical,
    // so the inventory would keep claiming serialization is not represented
    // while the crate had gained it.
    let source = "#[derive(Debug, serde::Serialize, Clone)]\npub struct Probe { pub a: u8 }";
    let derived = derive_public_items(source)?;
    let probe = derived
        .iter()
        .find(|item| item.path == "perl_ast_v2::Probe")
        .ok_or_else(|| color_eyre::eyre::eyre!("probe struct missing from the derivation"))?;
    assert!(
        probe.shape.contains("serde::Serialize"),
        "a path-qualified derive must appear in the shape: {}",
        probe.shape
    );
    Ok(())
}

#[test]
fn the_derived_denominator_matches_the_crate_the_audit_describes() -> Result<()> {
    let source = std::fs::read_to_string(repo_root_for_tests()?.join(V2_SOURCE_RELATIVE_PATH))?;
    let derived = derive_public_items(&source)?;
    let kinds: BTreeMap<&str, usize> = derived.iter().fold(BTreeMap::new(), |mut acc, item| {
        *acc.entry(item.kind.as_str()).or_default() += 1;
        acc
    });
    // 18 NodeKind variants + 9 MissingKind variants.
    assert_eq!(kinds.get("enum_variant"), Some(&27));
    assert_eq!(kinds.get("enum"), Some(&2));
    assert_eq!(kinds.get("struct"), Some(&2));
    assert_eq!(kinds.get("type_alias"), Some(&2));
    assert_eq!(kinds.get("trait_impl"), Some(&1));
    assert_eq!(kinds.get("associated_fn"), Some(&5));
    Ok(())
}

#[test]
fn the_production_parity_denominator_is_much_larger_than_the_audited_one() -> Result<()> {
    // The audit's central factual claim about parity: v2 is a deliberately small
    // recovery-oriented subset, not a copy in progress. If the production AST
    // ever shrank to v2's size this assertion would need re-reasoning, which is
    // the point of pinning it.
    let root = repo_root_for_tests()?;
    let v1 = std::fs::read_to_string(root.join(V1_AST_SOURCE_RELATIVE_PATH))?;
    let v1_variants = derive_v1_node_kind_variants(&v1)?;
    assert!(v1_variants.len() > 60, "production NodeKind has only {}", v1_variants.len());
    assert!(v1_variants.contains("Binary"));
    assert!(!v1_variants.contains("ErrorRef"), "ErrorRef must stay a v2-only proposition");
    Ok(())
}

#[test]
fn a_private_field_is_counted_but_never_named() -> Result<()> {
    // NodeIdGenerator's counter is private. The shape must record that a field
    // exists — so adding one moves the row — without publishing its name as API.
    let source = std::fs::read_to_string(repo_root_for_tests()?.join(V2_SOURCE_RELATIVE_PATH))?;
    let derived = derive_public_items(&source)?;
    let generator = derived
        .iter()
        .find(|item| item.path == "perl_ast_v2::NodeIdGenerator")
        .ok_or_else(|| color_eyre::eyre::eyre!("NodeIdGenerator missing from the derivation"))?;
    assert!(generator.shape.contains("+1 non-public"));
    assert!(!generator.shape.contains("next_id"));
    Ok(())
}

#[test]
fn derives_are_part_of_the_recorded_public_shape() -> Result<()> {
    // Losing `Copy` on MissingKind is a breaking change that no field or
    // signature would record.
    let source = std::fs::read_to_string(repo_root_for_tests()?.join(V2_SOURCE_RELATIVE_PATH))?;
    let derived = derive_public_items(&source)?;
    let missing_kind = derived
        .iter()
        .find(|item| item.path == "perl_ast_v2::MissingKind")
        .ok_or_else(|| color_eyre::eyre::eyre!("MissingKind missing from the derivation"))?;
    assert!(
        missing_kind.shape.contains("Copy"),
        "shape must record derives: {}",
        missing_kind.shape
    );
    Ok(())
}

#[test]
fn an_unrenderable_type_stops_the_audit_rather_than_being_approximated() -> Result<()> {
    // Fail-closed control for the renderer: two different function-pointer types
    // must never collapse into one shared placeholder shape.
    let source = "pub type Callback = fn(u8) -> u8;";
    match derive_public_items(source) {
        Ok(items) => bail!("expected a fail-closed rejection, got {items:?}"),
        Err(err) => {
            let rendered = format!("{err:#}");
            assert!(rendered.contains("cannot render this type shape"), "{rendered}");
            Ok(())
        }
    }
}

#[test]
fn the_parity_denominator_fails_closed_when_the_production_enum_disappears() -> Result<()> {
    match derive_v1_node_kind_variants("pub struct NotAnEnum;") {
        Ok(_) => bail!("a source without NodeKind must fail closed"),
        Err(err) => {
            assert!(format!("{err:#}").contains("parity denominator is gone"));
            Ok(())
        }
    }
}

// ---------------------------------------------------------------------------
// Claim-ceiling guard.
// ---------------------------------------------------------------------------

#[test]
fn no_migration_or_mutation_surface_is_added() -> Result<()> {
    // #8843 inventories and rules. If this module ever grows a way to move
    // source, rewrite a manifest, publish, or delete a package, the claim
    // ceiling has been broken regardless of what the JSON says.
    //
    // This is an ALLOWLIST, not a denylist, and the difference is the whole
    // point. The first version listed six forbidden strings, which ordinary
    // code walks straight past: `File::create(..).write_all(..)`,
    // `OpenOptions::new().write(true)`, `fs::copy`, or `use std::process::Command
    // as Cmd` all evade every one of them. Naming what is permitted instead
    // means any new filesystem or process call fails until someone justifies it.
    //
    // Honest boundary: this is a tripwire over one file's own text, not a proof
    // of absence. It cannot see a mutating helper called in another module. It
    // catches the realistic case — someone extending this module — and the
    // claim it supports is scoped to that.
    let source = std::fs::read_to_string(
        repo_root_for_tests()?.join("xtask/src/ast_v2_lifecycle_audit.rs"),
    )?;

    const PERMITTED_FS_CALLS: [&str; 2] = ["fs::read_to_string", "fs::read("];

    let mut offenders: Vec<String> = Vec::new();
    for (index, line) in source.lines().enumerate() {
        // The allowlist entries themselves live in this file's prose; only
        // inspect lines that look like code, not the doc comments describing it.
        let code = line.split("//").next().unwrap_or("");
        // Markers are path forms, not bare words. A bare `duct` matches
        // "pro-duct-ion", and a bare `Command` would match ordinary prose —
        // the same substring hazard the token scan guards against elsewhere.
        for marker in ["fs::", "process::", "Command::", "duct::"] {
            if !code.contains(marker) {
                continue;
            }
            if marker == "fs::" && PERMITTED_FS_CALLS.iter().any(|ok| code.contains(ok)) {
                continue;
            }
            offenders.push(format!("line {}: {}", index + 1, code.trim()));
        }
    }

    assert!(
        offenders.is_empty(),
        "the audit module must stay read-only; only {PERMITTED_FS_CALLS:?} are permitted, and no \
         process execution at all. Found:\n  {}",
        offenders.join("\n  ")
    );
    Ok(())
}

#[test]
fn the_read_only_allowlist_actually_rejects_the_patterns_a_denylist_missed() -> Result<()> {
    // A guard that only ever passes proves nothing. These are the exact shapes
    // the previous six-string denylist let through; the allowlist must reject
    // every one of them.
    let evasions = [
        "std::fs::File::create(path)?.write_all(data.as_bytes())?;",
        "use std::fs::File as F;",
        "std::fs::OpenOptions::new().write(true).open(path)?;",
        "std::fs::copy(src, dst)?;",
        "use std::process::Command as Cmd;",
        "let out = duct::cmd!(\"rm\", path).run()?;",
    ];
    const PERMITTED_FS_CALLS: [&str; 2] = ["fs::read_to_string", "fs::read("];
    for line in evasions {
        let code = line.split("//").next().unwrap_or("");
        let flagged = ["fs::", "process::", "Command::", "duct::"].iter().any(|marker| {
            code.contains(marker)
                && !(*marker == "fs::" && PERMITTED_FS_CALLS.iter().any(|ok| code.contains(ok)))
        });
        assert!(flagged, "the read-only guard must reject `{line}`");
    }
    // And it must not flag the reads this module legitimately performs, nor
    // ordinary prose. A bare `duct` marker matches "pro-duct-ion", a word this
    // module's own text uses constantly — the same substring hazard the token
    // scan guards against, caught here by a control rather than in review.
    for permitted in [
        "let text = std::fs::read_to_string(path)?;",
        "let bytes = std::fs::read(p)?;",
        "\"production_implementation\",",
        "the production AST source declares no NodeKind enum",
    ] {
        let flagged = ["fs::", "process::", "Command::", "duct::"].iter().any(|marker| {
            permitted.contains(marker)
                && !(*marker == "fs::"
                    && PERMITTED_FS_CALLS.iter().any(|ok| permitted.contains(ok)))
        });
        assert!(!flagged, "the read-only guard must permit `{permitted}`");
    }
    Ok(())
}

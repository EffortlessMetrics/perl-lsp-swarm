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
// The read-only classifier, owned in ONE place.
//
// The guard and its control previously each declared their own copy of the
// marker list, so narrowing the guard would still have left the control passing
// against its private copy — the guard could degrade with no failing test. That
// is the same vacuity this suite exists to prevent, so the classification lives
// here and both callers use it.
// ---------------------------------------------------------------------------

/// The only filesystem calls this module may make.
const PERMITTED_FS_CALLS: [&str; 2] = ["fs::read_to_string", "fs::read("];

/// Markers are path forms, not bare words: a bare `duct` matches "pro-duct-ion"
/// and a bare `Command` matches ordinary prose.
/// `std::fs as` and `std::process as` catch module aliasing: `use std::fs as F;`
/// then `F::write(..)` produces no `fs::` substring anywhere and slipped past
/// the first version of this allowlist.
const MUTATION_MARKERS: [&str; 6] =
    ["fs::", "process::", "Command::", "duct::", "std::fs as", "std::process as"];

/// Whether one source line reaches a filesystem-mutating or process API.
fn violates_read_only(line: &str) -> bool {
    // The allowlist entries themselves appear in this file's prose, so only the
    // code part of the line is classified.
    let code = line.split("//").next().unwrap_or("");
    // The permitted reads are removed from the text rather than used to excuse
    // the whole line. Excusing the line let one call vouch for another:
    // `if let Ok(t) = fs::read_to_string(p) { fs::write(q, t)?; }` contains a
    // permitted read, so the write beside it was waved through. The allowlist
    // has to apply per call to mean anything.
    //
    // Known boundary, stated rather than papered over: splitting on `//` also
    // truncates code that follows a `//` inside a string literal, so a mutation
    // written after one on the same line is not seen. That is the same defect
    // the role classifier was moved off text for, and it is why this guard is
    // documented as a tripwire over the realistic case rather than a proof.
    let mut residual = code.to_string();
    for permitted in PERMITTED_FS_CALLS {
        residual = residual.replace(permitted, "");
    }
    MUTATION_MARKERS.iter().any(|marker| residual.contains(marker))
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
    assert_eq!(audit.consumer_count(), 37);
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

#[test]
fn the_reexport_derivation_finds_those_paths_in_the_real_tree() -> Result<()> {
    // Non-vacuity for the widened check. Reconciliation passing proves nothing
    // on its own if the derivation returns empty for every file, so the exact
    // `(file, alias)` pairs the whole scan set yields are pinned here. Any new
    // public path to the package anywhere under the scan roots moves this set,
    // and so does any inventoried one that stops being public.
    let root = repo_root_for_tests()?;
    let mut derived: BTreeSet<(String, String)> = BTreeSet::new();
    for file in derive_reference_files(&root)? {
        if !file.ends_with(".rs") {
            continue;
        }
        let text = std::fs::read_to_string(root.join(&file))
            .with_context(|| format!("failed to re-read scanned file {file}"))?;
        for (alias, _) in derive_public_reexports(&text) {
            derived.insert((file.clone(), alias));
        }
    }

    let expected: BTreeSet<(String, String)> = [
        ("crates/perl-ast/src/lib.rs", "v2"),
        ("crates/perl-parser-core/src/engine/mod.rs", "ast_v2"),
        ("crates/perl-parser-core/src/lib.rs", "ast_v2"),
        ("crates/perl-parser-core/src/lib.rs", "DiagnosticId"),
        ("crates/perl-parser-core/src/lib.rs", "MissingKind"),
        ("crates/perl-parser/src/lib.rs", "ast_v2"),
        ("crates/perl-parser/src/compat.rs", "ast_v2"),
    ]
    .into_iter()
    .map(|(file, alias)| (file.to_string(), alias.to_string()))
    .collect();

    assert_eq!(
        derived, expected,
        "the public re-exports derived from the current tree are not the inventoried ones"
    );
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
fn a_gating_role_the_source_contradicts_cannot_stay_in_the_inventory() -> Result<()> {
    // The four gating roles were a closed vocabulary and nothing else, so they
    // were interchangeable: relabelling a `Cargo.toml` row
    // `production_implementation` reconciled green, and #8845 — which sizes each
    // part of the migration from these roles — would have been handed the wrong
    // breakdown. Only what the source settles is asserted.
    for (role, file, needle) in [
        // A manifest is a manifest, in both directions.
        ("package_dependency", "crates/perl-ast/src/lib.rs", "is not a `Cargo.toml`"),
        ("production_implementation", "crates/perl-ast/Cargo.toml", "can only be"),
        ("public_reexport", "crates/perl-ast/Cargo.toml", "can only be"),
        ("test_fixture", "crates/perl-ast/Cargo.toml", "can only be"),
        // A Rust role needs Rust.
        ("production_implementation", "docs/agents/NOTES.md", "is not a `.rs` file"),
        ("test_fixture", "policy/repository-topology.toml", "is not a `.rs` file"),
        // An integration-test target is not production code.
        (
            "production_implementation",
            "crates/perl-ast/tests/comprehensive_unit_tests.rs",
            "integration-test target",
        ),
    ] {
        let contradiction = role_contradiction(role, file, None);
        let Some(rendered) = contradiction else {
            bail!("role `{role}` on `{file}` must be rejected as contradicted by the source");
        };
        assert!(
            rendered.contains(needle),
            "role `{role}` on `{file}` must be rejected for that reason: {rendered}"
        );
    }

    // A `public_reexport` must really publish one; the derivation answers that,
    // and `None` (unreadable) is never treated as absence.
    assert!(
        role_contradiction("public_reexport", "crates/perl-ast/src/lib.rs", Some(false))
            .is_some_and(|rendered| rendered.contains("finds none there")),
        "a public_reexport row over a file that publishes nothing must be rejected"
    );
    assert!(
        role_contradiction("public_reexport", "crates/perl-ast/src/lib.rs", None).is_none(),
        "an unreadable file is not evidence that it publishes nothing"
    );

    // The honest rows must all survive, or the check is a tripwire on the
    // inventory it is meant to protect.
    for (role, file) in [
        ("package_dependency", "crates/perl-lexer/Cargo.toml"),
        ("package_dependency", "Cargo.toml"),
        ("production_implementation", "crates/perl-parser-core/src/tokens/trivia.rs"),
        ("test_fixture", "crates/perl-ast-v2/tests/node_kinds.rs"),
        ("docs_reference", "docs/agents/NOTES.md"),
        ("policy_inventory", "policy/repository-topology.toml"),
    ] {
        assert!(
            role_contradiction(role, file, Some(true)).is_none(),
            "role `{role}` on `{file}` is consistent and must not be rejected"
        );
    }

    // The direction deliberately left unasserted: a `#[cfg(test)]` module inside
    // `src/` is a legitimate `test_fixture`, and the path cannot tell it from a
    // production file. Rejecting it would fire on other people's PRs.
    assert!(
        role_contradiction("test_fixture", "crates/perl-parser-core/src/tokens/trivia.rs", None)
            .is_none(),
        "a test fixture inside src/ is a real shape and must not be guessed at"
    );
    Ok(())
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
fn every_successor_the_ruling_binds_must_carry_its_wake_condition() -> Result<()> {
    // Non-emptiness was the wrong property. The ruling sets a gate on each of
    // #8844, #8845 and #8847, and `wake_event` answers `None` for a successor
    // with no row — so a revision that dropped one, or swapped all three for an
    // unrelated issue, silently un-gated migration or the compatibility window
    // while the array stayed non-empty and the audit stayed green.
    let mut emptied = real_value()?;
    emptied["successor_wake_conditions"] = Value::Array(vec![]);
    assert_rejected(&emptied, "has no wake condition")?;

    // Dropping any single required successor is rejected, naming that one.
    for required in [8844u64, 8845, 8847] {
        let mut value = real_value()?;
        let rows = value["successor_wake_conditions"]
            .as_array()
            .ok_or_else(|| color_eyre::eyre::eyre!("successor_wake_conditions is not an array"))?
            .iter()
            .filter(|row| row["successor_issue"].as_u64() != Some(required))
            .cloned()
            .collect::<Vec<_>>();
        value["successor_wake_conditions"] = Value::Array(rows);
        assert_rejected(&value, &format!("#{required} has no wake condition"))?;
    }

    // The substitution the old law allowed: a non-empty array carrying none of
    // the bound successors.
    let mut substituted = real_value()?;
    let mut unrelated = substituted["successor_wake_conditions"]
        .as_array()
        .and_then(|rows| rows.first().cloned())
        .ok_or_else(|| color_eyre::eyre::eyre!("no successor row to base a substitution on"))?;
    unrelated["successor_issue"] = Value::from(99999u64);
    substituted["successor_wake_conditions"] = Value::Array(vec![unrelated]);
    assert_rejected(&substituted, "has no wake condition")
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
fn the_instruments_own_files_are_classified_by_parser_not_exempted() -> Result<()> {
    // These two files name the package in every token form because describing it
    // is their job, so the token scan cannot classify them. They were skipped
    // outright, which made them the one place in the repository a real consumer
    // could hide: an actual `use perl_ast_v2::Node;` here would never enter the
    // denominator and #8845 would migrate without it.
    //
    // They are now classified by the parser branch instead, which reads declared
    // paths rather than text. The list stays pinned at two, because a growing
    // list of parser-only files would be a way to weaken the token scan.
    assert_eq!(INSTRUMENT_SELF_FILES.len(), 2);
    assert_eq!(
        INSTRUMENT_SELF_FILES,
        ["xtask/src/ast_v2_lifecycle_audit.rs", "xtask/src/ast_v2_lifecycle_audit_tests.rs"]
    );

    // As they actually stand, neither declares a path into the package, so
    // neither enters the denominator — the exclusion was not load-bearing.
    let scanned = derive_reference_files(&repo_root_for_tests()?)?;
    for instrument_file in INSTRUMENT_SELF_FILES {
        assert!(
            !scanned.contains(instrument_file),
            "{instrument_file} declares no path into the package and must not be a consumer"
        );
        let text = std::fs::read_to_string(repo_root_for_tests()?.join(instrument_file))?;
        assert_eq!(
            parsed_api_use(&text),
            Some(false),
            "{instrument_file} must parse, and declare no real API use of the package"
        );
        // And the reason the token scan cannot be used on them is real: it does
        // fire on their prose. Asserted against the token scan itself, since
        // `reaches_audited_package` now routes these two paths to the parser —
        // asking it here would only restate the branch above.
        assert!(
            mentions_audited_package(&text),
            "{instrument_file} names the package textually, which is why it needs the parser"
        );
    }

    // The property the whole-file skip could not give, asserted through the
    // classifier the walk actually calls rather than through a helper beside it.
    // An earlier version of this control tested `parsed_api_use` directly and so
    // passed with the skip still in place — a control that reported success
    // without proving its constraint.
    for instrument_file in INSTRUMENT_SELF_FILES {
        assert!(
            reaches_audited_package("use perl_ast_v2::Node;\nfn f(_: Node) {}", instrument_file),
            "a genuine import in {instrument_file} must reach the denominator"
        );
        assert!(
            !reaches_audited_package(
                "const P: &str = \"crates/perl-ast-v2/src/lib.rs\";",
                instrument_file
            ),
            "a string naming the package in {instrument_file} must not"
        );
    }
    // Two forms the parser branch has to catch, because on these two files it is
    // the *only* branch and anything it misses is invisible.
    //
    // `extern crate` binds the crate without writing a path, so no path visit
    // sees it.
    for instrument_file in INSTRUMENT_SELF_FILES {
        assert!(
            reaches_audited_package("extern crate perl_ast_v2;", instrument_file),
            "an `extern crate` in {instrument_file} must reach the denominator"
        );
        assert!(
            reaches_audited_package("extern crate perl_ast_v2 as v2;", instrument_file),
            "an aliased `extern crate` in {instrument_file} must reach the denominator"
        );
        // `syn` does not expand macros, so a path inside one is invisible to
        // every other arm.
        assert!(
            reaches_audited_package("fn f() { some_macro!(perl_ast_v2::Node); }", instrument_file),
            "a package path inside a macro in {instrument_file} must reach the denominator"
        );
        // A grouped import inside a macro. The crate name sits *before* the
        // brace, so a scan that restarts at the group loses it and every
        // grouped form goes invisible — the same prefix-lost-at-a-boundary
        // mistake made earlier in re-export matching.
        assert!(
            reaches_audited_package(
                "fn f() { some_macro!(use perl_ast::{v2, Node};); }",
                instrument_file
            ),
            "a grouped canonical import inside a macro in {instrument_file} must be seen"
        );
        assert!(
            reaches_audited_package(
                "fn f() { some_macro!(use perl_parser_core::{DiagnosticId, MissingKind};); }",
                instrument_file
            ),
            "a grouped unqualified re-export inside a macro in {instrument_file} must be seen"
        );
        // Nesting one deeper still carries the prefix.
        assert!(
            reaches_audited_package(
                "fn f() { outer!(inner!(use perl_ast::{v2};)); }",
                instrument_file
            ),
            "a grouped import nested two macros deep in {instrument_file} must be seen"
        );
        // A group that does *not* follow `::` must not inherit a path, or every
        // brace after an identifier would extend one.
        assert!(
            !reaches_audited_package("fn f() { some_macro!(perl_ast { v2 }); }", instrument_file),
            "a block after an identifier is not a path continuation in {instrument_file}"
        );

        // A bare identifier inside a macro is data, not syntax. Accepting it
        // invented a consumer — the audit reporting API use where there is
        // none, which fires on other people's PRs rather than merely missing
        // something.
        for mention in [
            "fn f() { let s = stringify!(perl_ast_v2); }",
            "fn f() { some_macro!(name = perl_ast_v2); }",
            "fn f() { some_macro!(perl_ast_v2, other); }",
        ] {
            assert!(
                !reaches_audited_package(mention, instrument_file),
                "a bare identifier passed as macro data in {instrument_file} is not a use: \
                 {mention}"
            );
        }
        // The binding and traversing forms a lone identifier *can* take are
        // still seen, so the guard did not simply switch the single-segment
        // case off.
        for real in [
            "fn f() { some_macro!(use perl_ast_v2;); }",
            "fn f() { some_macro!(use perl_ast_v2 as v2;); }",
            "fn f() { some_macro!(extern crate perl_ast_v2;); }",
            "fn f() { some_macro!(perl_ast_v2::Node); }",
        ] {
            assert!(
                reaches_audited_package(real, instrument_file),
                "a real binding or traversal in {instrument_file} must be seen: {real}"
            );
        }

        // `as` renames the imported item; the name after it is local and is not
        // part of any path into the package. Rebuilding the path from the
        // enclosing prefix at the alias synthesised `perl_ast::v2` out of an
        // import of `perl_ast::Node` — inventing a consumer.
        for aliased in [
            "fn f() { some_macro!(use perl_ast::{Node as v2};); }",
            "fn f() { some_macro!(use perl_parser_core::{Parser as DiagnosticId};); }",
            "fn f() { some_macro!(use perl_ast::{Node as v2, Span as MissingKind};); }",
        ] {
            assert!(
                !reaches_audited_package(aliased, instrument_file),
                "an alias in {instrument_file} is a local name, not a package path: {aliased}"
            );
        }
        // A genuine import of the package's own path still registers when it is
        // the thing being renamed, so the guard did not switch aliasing off.
        for real_alias in [
            "fn f() { some_macro!(use perl_ast::{v2 as renamed};); }",
            "fn f() { some_macro!(use perl_ast_v2 as renamed;); }",
            "fn f() { some_macro!(use perl_parser_core::{DiagnosticId as D};); }",
        ] {
            assert!(
                reaches_audited_package(real_alias, instrument_file),
                "renaming a real package path in {instrument_file} is still a use: {real_alias}"
            );
        }

        // `as` is a rename only in a use tree. In a cast it is followed by a
        // type path, and suppressing that hid a real reference.
        for cast in [
            "fn f() { some_macro!(value as perl_ast_v2::Node); }",
            "fn f() { some_macro!(x as perl_ast::v2::Node); }",
            "fn f() { some_macro!(a as u8, b as perl_ast_v2::Node); }",
        ] {
            assert!(
                reaches_audited_package(cast, instrument_file),
                "a package path after a cast in {instrument_file} is a real reference: {cast}"
            );
        }

        // And the distinction that makes the token scan usable here at all: a
        // string literal inside a macro is what this module's own fixtures and
        // assertion messages are made of, and must not register.
        assert!(
            !reaches_audited_package(
                "fn f() { assert!(x, \"use perl_ast_v2 as v2;\"); }",
                instrument_file
            ),
            "a package name inside a string literal in {instrument_file} is not a use"
        );
        assert!(
            !reaches_audited_package(
                "fn f() { let s = format!(\"{}\", \"perl_ast_v2::Node\"); }",
                instrument_file
            ),
            "a package path inside a string literal in {instrument_file} is not a use"
        );
    }

    // And the branch is keyed on the path: the same prose in any other file is
    // still a textual reference, so this narrows nothing outside these two.
    assert!(
        reaches_audited_package(
            "const P: &str = \"crates/perl-ast-v2/src/lib.rs\";",
            "crates/perl-parser/src/lib.rs"
        ),
        "outside the instrument's own files the token scan still applies"
    );

    assert_eq!(
        parsed_api_use("use perl_ast_v2::Node;\nfn f(_: Node) {}"),
        Some(true),
        "a real import in an instrument file must be discovered"
    );
    assert_eq!(
        parsed_api_use("fn f() -> perl_ast::v2::Node { unimplemented!() }"),
        Some(true),
        "a real canonical-path use in an instrument file must be discovered"
    );
    // While the shapes these files are actually full of stay invisible to it.
    assert_eq!(
        parsed_api_use("const P: &str = \"crates/perl-ast-v2/src/lib.rs\";"),
        Some(false),
        "a string naming the package is not an import"
    );
    assert_eq!(
        parsed_api_use("/// Describes `perl_ast_v2::Node` for the reader.\npub fn f() {}"),
        Some(false),
        "a doc comment naming the package is not an import"
    );
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
        // Requiring the field rather than defaulting it: an absent or renamed
        // `wake_event` made every `!contains` assertion below vacuously true,
        // so the control reported success for a manifest missing the very field
        // it names.
        let text = wake["wake_event"].as_str().ok_or_else(|| {
            color_eyre::eyre::eyre!("a successor wake row states no `wake_event`")
        })?;
        // Any spelled count, not the three that happen to be current. Pinning
        // the list to today's inventory meant the moment a count moved, the
        // newly-reintroduced hardcoded count was not in the denied set.
        for number in
            ["one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten"]
        {
            for role in ["production", "public_reexport", "package_dependency", "test_fixture"] {
                let spelled = format!("{number} {role}");
                assert!(
                    !text.contains(&spelled),
                    "wake text restates a row count (`{spelled}`); point at the rows instead, \
                     because a duplicated count drifts the moment the inventory moves"
                );
            }
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
fn generic_shapes_that_differ_only_in_bounds_or_defaults_do_not_collide() -> Result<()> {
    // A second round of collisions, in the same class as the first four: each
    // pair is a real breaking API difference that rendered identically once the
    // obvious cases were fixed. Where-clause content was reduced to a predicate
    // count, parameter defaults were dropped entirely, and lifetime outlives
    // bounds were never read.
    let shape_of = |src: &str| -> Result<String> {
        derive_public_items(src)?
            .iter()
            .find(|item| item.kind == "struct")
            .map(|item| item.shape.clone())
            .ok_or_else(|| color_eyre::eyre::eyre!("no struct derived from {src}"))
    };

    for (label, left, right) in [
        (
            "where-clause content",
            "pub struct S<T> where T: Clone { pub a: T }",
            "pub struct S<T> where T: Send { pub a: T }",
        ),
        ("type param default", "pub struct S<T> { pub a: T }", "pub struct S<T = u8> { pub a: T }"),
        (
            "type param default value",
            "pub struct S<T = u8> { pub a: T }",
            "pub struct S<T = i32> { pub a: T }",
        ),
        (
            "const generic default",
            "pub struct S<const N: usize = 4> { pub a: [u8; N] }",
            "pub struct S<const N: usize = 8> { pub a: [u8; N] }",
        ),
        (
            "lifetime outlives bound",
            "pub struct S<'a, 'b> { pub a: &'a u8, pub b: &'b u8 }",
            "pub struct S<'a: 'b, 'b> { pub a: &'a u8, pub b: &'b u8 }",
        ),
    ] {
        assert_ne!(
            shape_of(left)?,
            shape_of(right)?,
            "{label}: a breaking change left the shape identical"
        );
    }
    Ok(())
}

#[test]
fn associated_consts_and_types_inside_an_impl_produce_rows() -> Result<()> {
    // The item-level walk was fixed to bail on unmodelled public items, but the
    // impl arm still matched only `Fn` and skipped the rest — so `pub const
    // LIMIT` inside an impl produced no row and no error. Same silent vanish,
    // one level down.
    let derived = derive_public_items(
        "pub struct S;\nimpl S { pub const LIMIT: usize = 4; pub type Alias = u8; pub fn f() {} }",
    )?;
    let paths: BTreeSet<&str> = derived.iter().map(|item| item.path.as_str()).collect();
    for expected in
        ["perl_ast_v2::S::LIMIT", "perl_ast_v2::S::Alias", "perl_ast_v2::S::f", "perl_ast_v2::S"]
    {
        assert!(paths.contains(expected), "{expected} vanished; got {paths:?}");
    }
    // A private associated item is still correctly skipped.
    let private = derive_public_items("pub struct S;\nimpl S { const HIDDEN: u8 = 1; }")?;
    assert!(private.iter().all(|item| item.path != "perl_ast_v2::S::HIDDEN"));
    Ok(())
}

#[test]
fn signature_contract_changes_that_keep_the_name_still_move_the_shape() -> Result<()> {
    // `fn f()`, `const fn f()`, `unsafe fn f()` and `fn f<T: Clone>()` all
    // rendered as `fn f() -> ()`, so four different public contracts shared one
    // shape and any of those changes could land under an unmoved row.
    let shape_of = |src: &str| -> Result<String> {
        derive_public_items(src)?
            .iter()
            .find(|item| item.kind == "associated_fn")
            .map(|item| item.shape.clone())
            .ok_or_else(|| color_eyre::eyre::eyre!("no associated fn derived from {src}"))
    };
    let plain = shape_of("pub struct S;\nimpl S { pub fn f() {} }")?;
    for (label, src) in [
        ("const", "pub struct S;\nimpl S { pub const fn f() {} }"),
        ("unsafe", "pub struct S;\nimpl S { pub unsafe fn f() {} }"),
        ("generic", "pub struct S;\nimpl S { pub fn f<T: Clone>() {} }"),
        ("where clause", "pub struct S;\nimpl S { pub fn f<T>() where T: Clone {} }"),
    ] {
        assert_ne!(plain, shape_of(src)?, "{label} collided with a plain fn");
    }
    Ok(())
}

#[test]
fn trait_impl_contract_changes_that_keep_the_names_still_move_the_shape() -> Result<()> {
    // `impl Trait for S` was the whole recorded shape, so every property that
    // decides *when* the impl is available collided into it. Adding a bound
    // narrows availability for every downstream consumer, and `impl !Send for S`
    // is the opposite claim from `impl Send for S`; both landed under an
    // unmoved row.
    let shape_of = |src: &str| -> Result<String> {
        derive_public_items(src)?
            .iter()
            .find(|item| item.kind == "trait_impl")
            .map(|item| item.shape.clone())
            .ok_or_else(|| color_eyre::eyre::eyre!("no trait impl derived from {src}"))
    };
    let plain = shape_of("pub struct S;\nimpl Clone for S { }")?;
    for (label, src) in [
        ("generic parameter", "pub struct S<T>(T);\nimpl<T> Clone for S<T> { }"),
        ("bound on the parameter", "pub struct S<T>(T);\nimpl<T: Clone> Clone for S<T> { }"),
        ("where predicate", "pub struct S<T>(T);\nimpl<T> Clone for S<T> where T: Clone { }"),
        ("unsafe", "pub struct S;\nunsafe impl Send for S { }"),
        ("a different self type", "pub struct S;\nimpl Clone for T { }"),
    ] {
        assert_ne!(plain, shape_of(src)?, "{label} collided with a plain trait impl");
    }

    // The generic and the bounded generic must also differ from each other, not
    // merely from the unparameterised form.
    assert_ne!(
        shape_of("pub struct S<T>(T);\nimpl<T> Clone for S<T> { }")?,
        shape_of("pub struct S<T>(T);\nimpl<T: Clone> Clone for S<T> { }")?,
        "adding a bound must move the shape"
    );
    Ok(())
}

#[test]
fn a_symbol_a_consumer_does_not_name_cannot_stay_in_the_migration_set() -> Result<()> {
    // The loader required only that a gating row's `symbols` list be non-empty,
    // so a name left behind by a refactor — or one that was never there — read
    // exactly like a real one. These rows are the migration set #8845 inherits.
    let mut value = real_value()?;
    row_mut(&mut value, "consumers", "consumer_id", "c:parser-core-parser-context")?["symbols"] =
        Value::Array(vec![Value::String("NodeIdGeneratorThatWasRemoved".to_string())]);
    assert_rejected(&value, "the code there does not name")?;

    // A stale name inside an otherwise-live qualified path is caught too, since
    // each identifier in the path is checked rather than the string as a whole.
    let mut value = real_value()?;
    row_mut(&mut value, "consumers", "consumer_id", "c:parser-core-context-impls")?["symbols"] =
        Value::Array(vec![Value::String("NodeKind::RemovedVariant".to_string())]);
    assert_rejected(&value, "the code there does not name")
}

#[test]
fn a_name_surviving_only_in_a_comment_does_not_keep_a_symbol_row_alive() -> Result<()> {
    // The symbol check read the file as text, so a name left behind in a
    // comment after the code stopped using it satisfied it — the exact stale
    // row the check exists to reject, kept alive by its own epitaph.
    let with_code = "use perl_ast_v2::NodeIdGenerator;\npub fn f(_: NodeIdGenerator) {}";
    let idents = source_identifiers(with_code)
        .ok_or_else(|| color_eyre::eyre::eyre!("fixture must parse"))?;
    assert!(idents.contains("NodeIdGenerator"), "code that names it must yield the identifier");

    let comment_only = "// NodeIdGenerator was removed from this file\npub fn f() {}";
    let idents = source_identifiers(comment_only)
        .ok_or_else(|| color_eyre::eyre::eyre!("fixture must parse"))?;
    assert!(
        !idents.contains("NodeIdGenerator"),
        "a name that survives only in a comment is not named by the code: {idents:?}"
    );
    // The text scan it replaced could not tell those two apart, which is why
    // the parse is the instrument here.
    assert!(contains_token(comment_only, "NodeIdGenerator"));

    // A string literal is not a name either.
    let string_only = "pub fn f() -> &'static str { \"NodeIdGenerator\" }";
    let idents = source_identifiers(string_only)
        .ok_or_else(|| color_eyre::eyre::eyre!("fixture must parse"))?;
    assert!(!idents.contains("NodeIdGenerator"), "a string literal names nothing: {idents:?}");
    Ok(())
}

#[test]
fn a_grouped_import_in_a_doctest_is_still_code() -> Result<()> {
    // Doc-attribute classification matched text against `API_USE_FORM`, which
    // does not recognise the grouped canonical form — the same gap that moved
    // ordinary code onto the parser, left in place one branch over. A doctest
    // is compiled and run, so compiled API use could be classified as prose and
    // take a non-gating role.
    let grouped_doctest = "/// ```\n/// use perl_ast::{v2, Node};\n/// ```\npub fn f() {}";
    assert!(!mentions_audited_package(grouped_doctest), "the token scan cannot see this form");
    assert!(
        references_package_api_in_code(grouped_doctest, "crates/a/src/lib.rs"),
        "a grouped import inside a doctest is compiled API use"
    );

    // The inner-doc spelling is the same claim and must classify the same way.
    let inner = "//! ```\n//! use perl_ast::{v2, Node};\n//! ```\npub fn f() {}";
    assert!(references_package_api_in_code(inner, "crates/a/src/lib.rs"));

    // Prose that merely mentions the crate in a doc comment is still prose —
    // otherwise every documentation reference would become a gating consumer.
    let prose = "/// This module predates the perl-ast-v2 experiment.\npub fn f() {}";
    assert!(
        !references_package_api_in_code(prose, "crates/a/src/lib.rs"),
        "a hyphenated crate name in prose is not an API reference"
    );
    Ok(())
}

#[test]
fn an_impl_on_a_private_type_is_not_public_inventory() -> Result<()> {
    // An `impl` carries no visibility of its own, so a `pub fn` on a private
    // helper — or any trait impl for one — produced a public-inventory row for
    // something no consumer can name. That is a false positive in the direction
    // that hurts: it demands a manifest row for internal code and fails
    // reconciliation on an honest change.
    let kinds = |src: &str| -> Result<Vec<String>> {
        Ok(derive_public_items(src)?.iter().map(|item| item.kind.clone()).collect())
    };
    assert!(
        kinds("struct Hidden;\nimpl Hidden { pub fn f() {} }")?.is_empty(),
        "an inherent impl on a private type publishes nothing"
    );
    assert!(
        kinds("struct Hidden;\nimpl Clone for Hidden {}")?.is_empty(),
        "a trait impl on a private type publishes nothing"
    );

    // The public case is unaffected — this narrows the denominator, and a
    // narrowing that also drops real public API would be worse than the
    // false positive it fixes.
    assert!(
        kinds("pub struct Shown;\nimpl Shown { pub fn f() {} }")?
            .contains(&"associated_fn".to_string()),
        "a public type's inherent impl still publishes"
    );
    assert!(
        kinds("pub struct Shown;\nimpl Clone for Shown {}")?.contains(&"trait_impl".to_string()),
        "a public type's trait impl still publishes"
    );

    // A type this file does not declare is treated as public, so an impl on an
    // imported or foreign type keeps the fail-closed behaviour rather than
    // being silently skipped.
    assert!(
        kinds("impl Clone for Imported {}")?.contains(&"trait_impl".to_string()),
        "an impl on a type declared elsewhere is not assumed private"
    );
    Ok(())
}

#[test]
fn field_order_is_contract_only_where_the_representation_makes_it_so() -> Result<()> {
    // Named-field order is not observable in a plain Rust type: construction
    // and matching are by name. Recording it made the audit fire on a cosmetic
    // reordering — the kind of churn that gets a check switched off. Under a
    // layout-fixing `repr` the opposite holds: order *is* the ABI, and a
    // reordering is a breaking change no name or type would record.
    let shape_of = |src: &str| -> Result<String> {
        derive_public_items(src)?
            .iter()
            .find(|item| item.kind == "struct")
            .map(|item| item.shape.clone())
            .ok_or_else(|| color_eyre::eyre::eyre!("no struct derived from {src}"))
    };
    assert_eq!(
        shape_of("pub struct S { pub a: u8, pub b: u16 }")?,
        shape_of("pub struct S { pub b: u16, pub a: u8 }")?,
        "reordering named fields of a plain struct is not a contract change"
    );
    assert_ne!(
        shape_of("#[repr(C)]\npub struct S { pub a: u8, pub b: u16 }")?,
        shape_of("#[repr(C)]\npub struct S { pub b: u16, pub a: u8 }")?,
        "under `repr(C)` the field order is the ABI"
    );
    assert_ne!(
        shape_of("#[repr(packed)]\npub struct S { pub a: u8, pub b: u16 }")?,
        shape_of("#[repr(packed)]\npub struct S { pub b: u16, pub a: u8 }")?,
        "under `repr(packed)` the field order is the ABI"
    );

    // Renaming or retyping a field is still a change in every case — the
    // relaxation is about order alone.
    assert_ne!(
        shape_of("pub struct S { pub a: u8 }")?,
        shape_of("pub struct S { pub a: u16 }")?,
        "a field type is contract regardless of order"
    );
    assert_ne!(
        shape_of("pub struct S { pub a: u8 }")?,
        shape_of("pub struct S { pub renamed: u8 }")?,
        "a field name is contract regardless of order"
    );

    // Tuple fields keep source order unconditionally: the position is the name.
    let tuple_shape = |src: &str| -> Result<String> {
        derive_public_items(src)?
            .iter()
            .find(|item| item.kind == "struct")
            .map(|item| item.shape.clone())
            .ok_or_else(|| color_eyre::eyre::eyre!("no struct derived from {src}"))
    };
    assert_ne!(
        tuple_shape("pub struct T(pub u8, pub u16);")?,
        tuple_shape("pub struct T(pub u16, pub u8);")?,
        "a tuple field's position is its name"
    );
    Ok(())
}

#[test]
fn a_private_fields_type_is_contract_even_though_its_name_is_not() -> Result<()> {
    // Private fields were counted, not typed: the shape said `+1 non-public`
    // whatever that field held. But the auto traits a struct offers — `Send`,
    // `Sync`, `Unpin`, `RefUnwindSafe` — are decided by every field regardless
    // of visibility, and a consumer observes them. So `next_id: usize` becoming
    // `Rc<Cell<usize>>` removes `Send` from the public type while the row
    // reconciled green. The name stays elided: renaming a private field breaks
    // nobody, and firing on it is the churn that gets a check switched off.
    let shape_of = |src: &str| -> Result<String> {
        derive_public_items(src)?
            .iter()
            .find(|item| item.kind == "struct")
            .map(|item| item.shape.clone())
            .ok_or_else(|| color_eyre::eyre::eyre!("no struct derived from {src}"))
    };

    // The distinction this control exists to hold: type moves, name does not.
    assert_ne!(
        shape_of("pub struct S { next_id: usize }")?,
        shape_of("pub struct S { next_id: std::rc::Rc<std::cell::Cell<usize>> }")?,
        "retyping a private field changes the struct's auto traits and must move the shape"
    );
    assert_eq!(
        shape_of("pub struct S { next_id: usize }")?,
        shape_of("pub struct S { counter: usize }")?,
        "a private field's name is not public contract and must not move the shape"
    );

    // The count property the previous rendering did hold is not lost.
    assert_ne!(
        shape_of("pub struct S { a: usize }")?,
        shape_of("pub struct S { a: usize, b: usize }")?,
        "adding a private field still moves the shape"
    );

    // Public fields are unaffected: they keep their names.
    assert_ne!(
        shape_of("pub struct S { pub a: usize }")?,
        shape_of("pub struct S { pub renamed: usize }")?,
        "a public field's name remains contract"
    );

    // Under a layout-fixing `repr` a private field's position is ABI, so it
    // stays in sequence rather than being lifted out of it.
    assert_ne!(
        shape_of("#[repr(C)]\npub struct S { private: u8, pub b: u16 }")?,
        shape_of("#[repr(C)]\npub struct S { pub b: u16, private: u8 }")?,
        "under `repr(C)` a private field's position is part of the layout"
    );
    assert_eq!(
        shape_of("pub struct S { private: u8, pub b: u16 }")?,
        shape_of("pub struct S { pub b: u16, private: u8 }")?,
        "without a layout-fixing repr, moving a private field is still cosmetic"
    );
    Ok(())
}

#[test]
fn a_trait_impls_associated_assignments_are_part_of_its_contract() -> Result<()> {
    // The trait impl row recorded only the header, so `type Item = u8` becoming
    // `= u16` — which changes what every consumer of the trait gets back —
    // moved nothing. The impl is one row, so its assignments belong in its
    // shape.
    let shape_of = |src: &str| -> Result<String> {
        derive_public_items(src)?
            .iter()
            .find(|item| item.kind == "trait_impl")
            .map(|item| item.shape.clone())
            .ok_or_else(|| color_eyre::eyre::eyre!("no trait impl derived from {src}"))
    };
    assert_ne!(
        shape_of("pub struct S;\nimpl Iterator for S { type Item = u8; }")?,
        shape_of("pub struct S;\nimpl Iterator for S { type Item = u16; }")?,
        "an associated type assignment is contract"
    );
    assert_ne!(
        shape_of("pub struct S;\nimpl T for S { const N: u8 = 1; }")?,
        shape_of("pub struct S;\nimpl T for S { const N: u8 = 2; }")?,
        "an associated const value is contract"
    );
    assert_ne!(
        shape_of("pub struct S;\nimpl T for S { fn f() {} }")?,
        shape_of("pub struct S;\nimpl T for S { const fn f() {} }")?,
        "a method qualifier is contract even when the trait fixes the signature"
    );
    // Source order is not contract, or the check would fire on a reordering.
    assert_eq!(
        shape_of("pub struct S;\nimpl T for S { type A = u8; type B = u16; }")?,
        shape_of("pub struct S;\nimpl T for S { type B = u16; type A = u8; }")?,
        "reordering assignments must not move the shape"
    );
    Ok(())
}

#[test]
fn an_associated_constants_value_is_part_of_its_shape() -> Result<()> {
    // `const LIMIT: usize` going from 128 to 64 changes what every consumer
    // reads, and neither the name nor the type moves.
    let shape_of = |src: &str| -> Result<String> {
        derive_public_items(src)?
            .iter()
            .find(|item| item.kind == "associated_const")
            .map(|item| item.shape.clone())
            .ok_or_else(|| color_eyre::eyre::eyre!("no associated const derived from {src}"))
    };
    assert_ne!(
        shape_of("pub struct S;\nimpl S { pub const LIMIT: usize = 128; }")?,
        shape_of("pub struct S;\nimpl S { pub const LIMIT: usize = 64; }")?,
        "the value of a public associated constant is contract"
    );
    Ok(())
}

#[test]
fn a_doctest_import_split_across_lines_is_still_code() -> Result<()> {
    // A doctest is one `#[doc = "..."]` attribute per line, so judging each
    // line alone splits every construct that spans lines: a grouped import
    // written across two lines parses as neither half, and the compiled
    // doctest vanished from classification. `rustdoc` compiles the block, so
    // the block is the unit judged.
    let split =
        "/// ```\n/// use perl_ast::{\n///     v2,\n///     Node,\n/// };\n/// ```\npub fn f() {}";
    assert!(!mentions_audited_package(split), "the token scan cannot see this form");
    assert!(
        references_package_api_in_code(split, "crates/a/src/lib.rs"),
        "a grouped import split across doc lines is still compiled API use"
    );

    // Two items' doc comments must not be concatenated into a construct
    // neither of them wrote — the block boundary is the item.
    let spliced = "/// use perl_ast::{\npub fn a() {}\n/// v2, Node };\npub fn b() {}";
    assert!(
        !references_package_api_in_code(spliced, "crates/a/src/lib.rs"),
        "doc lines from two different items are not one doctest"
    );
    Ok(())
}

#[test]
fn contract_attributes_reach_every_modelled_public_surface() -> Result<()> {
    // The first version rendered contract attributes on structs, enums and
    // variants only, so the same `#[cfg]` or `#[deprecated]` that moved an enum
    // shape moved nothing on a type alias, an associated item, a trait impl or
    // a field.
    let shape_of = |src: &str, kind: &str| -> Result<String> {
        derive_public_items(src)?
            .iter()
            .find(|item| item.kind == kind)
            .map(|item| item.shape.clone())
            .ok_or_else(|| color_eyre::eyre::eyre!("no {kind} derived from {src}"))
    };

    for (kind, plain, attributed) in [
        ("type_alias", "pub type A = u8;", "#[deprecated]\npub type A = u8;"),
        (
            "associated_fn",
            "pub struct S;\nimpl S { pub fn f() {} }",
            "pub struct S;\nimpl S { #[cfg(unix)] pub fn f() {} }",
        ),
        (
            "associated_const",
            "pub struct S;\nimpl S { pub const N: u8 = 1; }",
            "pub struct S;\nimpl S { #[deprecated] pub const N: u8 = 1; }",
        ),
        (
            "trait_impl",
            "pub struct S;\nimpl Clone for S {}",
            "pub struct S;\n#[cfg(unix)]\nimpl Clone for S {}",
        ),
        ("struct", "pub struct S { pub a: u8 }", "pub struct S { #[cfg(unix)] pub a: u8 }"),
        ("struct", "pub struct T(pub u8);", "pub struct T(#[cfg(unix)] pub u8);"),
    ] {
        assert_ne!(
            shape_of(plain, kind)?,
            shape_of(attributed, kind)?,
            "a contract attribute on a {kind} must move the shape"
        );
    }
    Ok(())
}

#[test]
fn a_relaxed_or_higher_ranked_bound_is_not_the_same_bound() -> Result<()> {
    // `render_generics` rendered only the bound's path, so `T: Sized` and
    // `T: ?Sized` shared a shape. The `?` decides whether a caller may pass an
    // unsized type — a widening one way and a breaking narrowing the other, and
    // neither moved a row.
    let shape_of = |src: &str| -> Result<String> {
        derive_public_items(src)?
            .iter()
            .find(|item| item.kind == "struct")
            .map(|item| item.shape.clone())
            .ok_or_else(|| color_eyre::eyre::eyre!("no struct derived from {src}"))
    };
    assert_ne!(
        shape_of("pub struct S<T: Sized>(pub std::marker::PhantomData<T>);")?,
        shape_of("pub struct S<T: ?Sized>(pub std::marker::PhantomData<T>);")?,
        "`?Sized` is a different bound from `Sized`"
    );
    // The same rule has to hold in a `where` clause, which is a separate
    // renderer and was equally path-only.
    assert_ne!(
        shape_of("pub struct S<T>(pub std::marker::PhantomData<T>) where T: Sized;")?,
        shape_of("pub struct S<T>(pub std::marker::PhantomData<T>) where T: ?Sized;")?,
        "`?Sized` is a different bound in a where clause too"
    );
    Ok(())
}

#[test]
fn conditional_layout_and_discriminant_changes_move_the_shape() -> Result<()> {
    // None of these touch a name, a field or a type, so a shape built from
    // those alone read identically before and after. `#[cfg]` decides whether
    // the item exists on a target at all, `#[repr]` fixes the layout an FFI
    // consumer depends on, and a discriminant is the value a `repr` enum
    // crosses a wire as.
    let enum_shape = |src: &str| -> Result<String> {
        derive_public_items(src)?
            .iter()
            .find(|item| item.kind == "enum")
            .map(|item| item.shape.clone())
            .ok_or_else(|| color_eyre::eyre::eyre!("no enum derived from {src}"))
    };
    let variant_shape = |src: &str| -> Result<String> {
        derive_public_items(src)?
            .iter()
            .find(|item| item.kind == "enum_variant")
            .map(|item| item.shape.clone())
            .ok_or_else(|| color_eyre::eyre::eyre!("no variant derived from {src}"))
    };

    let plain = enum_shape("pub enum E { A }")?;
    for (label, src) in [
        ("repr", "#[repr(u8)]\npub enum E { A }"),
        ("a different repr", "#[repr(u16)]\npub enum E { A }"),
        ("cfg", "#[cfg(unix)]\npub enum E { A }"),
        ("deprecated", "#[deprecated]\npub enum E { A }"),
    ] {
        assert_ne!(plain, enum_shape(src)?, "{label} collided with a plain enum");
    }
    assert_ne!(
        enum_shape("#[repr(u8)]\npub enum E { A }")?,
        enum_shape("#[repr(u16)]\npub enum E { A }")?,
        "two different representations must not share a shape"
    );

    let bare_variant = variant_shape("pub enum E { A }")?;
    assert_ne!(
        bare_variant,
        variant_shape("pub enum E { A = 3 }")?,
        "an explicit discriminant is contract"
    );
    assert_ne!(
        variant_shape("pub enum E { A = 3 }")?,
        variant_shape("pub enum E { A = 4 }")?,
        "changing a discriminant value is a wire-format change"
    );
    assert_ne!(
        bare_variant,
        variant_shape("pub enum E { #[cfg(unix)] A }")?,
        "a target-conditional variant is not the unconditional one"
    );

    // Source formatting is not contract: the same attribute written with
    // different spacing must not move the shape, or the check would fire on
    // reformatting and be turned off.
    assert_eq!(
        enum_shape("#[repr(u8)]\npub enum E { A }")?,
        enum_shape("#[repr( u8 )]\npub enum E { A }")?,
        "whitespace inside an attribute is not a contract change"
    );
    Ok(())
}

#[test]
fn a_symlink_hiding_a_grouped_import_is_not_called_harmless() -> Result<()> {
    // The symlink guard checked the token scan alone while the file branch
    // beside it used the union, so a link whose target reached the package
    // through `use perl_ast::{v2, Node};` named none of the four tokens, was
    // called harmless, and was then skipped as a non-file — the exact silent
    // loss the guard exists to prevent. Both branches now share one classifier.
    let grouped = "use perl_ast::{v2, Node};\npub fn f() {}\n";
    assert!(!mentions_audited_package(grouped), "the token scan alone cannot see this form");
    assert!(
        reaches_audited_package(grouped, "crates/a/src/lib.rs"),
        "the shared classifier must see a grouped canonical import"
    );

    // A non-Rust path is not parsed, so the prefilter cannot be turned into a
    // second guess at the answer by a TOML file that happens to contain braces.
    assert!(!reaches_audited_package(grouped, "crates/a/Cargo.toml"));

    // And an unparseable file naming the package nowhere is still not a
    // consumer: discovery keeps its open default.
    assert!(!reaches_audited_package("this is not rust at all {{{", "crates/a/src/lib.rs"));
    Ok(())
}

#[test]
fn a_below_threshold_row_cannot_authorize_a_retain_ruling() -> Result<()> {
    // The law was only a non-emptiness check, so any row placed in
    // `independent_lifecycle_evidence_ids` authorized `retain` — including
    // `ev:registry-publication`, which the ruling's own text calls below the
    // threshold. Qualification is now a property of the evidence.
    let mut value = real_value()?;
    value["ruling"]["ruling"] = Value::String("retain".to_string());
    value["ruling"]["independent_lifecycle_evidence_ids"] =
        Value::Array(vec![Value::String("ev:registry-publication".to_string())]);
    assert_rejected(&value, "does not meet the threshold")?;

    // And a row cannot simply declare itself qualifying: the flag is available
    // only to the classes the reversal condition names.
    let mut value = real_value()?;
    row_mut(&mut value, "external_evidence", "evidence_id", "ev:registry-publication")?["meets_independent_lifecycle_threshold"] =
        Value::Bool(true);
    assert_rejected(&value, "can carry it")
}

#[test]
fn the_qualifying_evidence_classes_are_the_recorded_reversal_clauses() -> Result<()> {
    // The executable threshold and the ruling's own reversal condition have to
    // describe the same three grounds. Restricting the flag to
    // `reverse_dependency` made the law narrower than the ruling it enforces:
    // an observed divergence in release cadence, or a public proposition
    // reachable only under the package's own path, is a stated ground for
    // `retain` that no evidence row could then express — so two of the three
    // promised reversals were unreachable.
    let qualifying: BTreeSet<&str> = QUALIFYING_EVIDENCE_CLASSES.into_iter().collect();
    assert_eq!(
        qualifying,
        BTreeSet::from(["reverse_dependency", "release_cadence", "package_only_proposition"]),
        "one qualifying class per reversal clause"
    );

    // Each qualifying class is representable and can carry the flag.
    for class in QUALIFYING_EVIDENCE_CLASSES {
        let mut value = real_value()?;
        let row =
            row_mut(&mut value, "external_evidence", "evidence_id", "ev:registry-publication")?;
        row["class"] = Value::String(class.to_string());
        row["meets_independent_lifecycle_threshold"] = Value::Bool(true);
        value["ruling"]["ruling"] = Value::String("retain".to_string());
        value["ruling"]["independent_lifecycle_evidence_ids"] =
            Value::Array(vec![Value::String("ev:registry-publication".to_string())]);
        validate(&value).with_context(|| {
            format!("`{class}` is a recorded reversal ground and must be able to authorize retain")
        })?;
    }

    // Everything else stays below the bar, each for its own recorded reason.
    for class in ["registry_publication", "not_consumer_evidence", "unavailable"] {
        let mut value = real_value()?;
        let row =
            row_mut(&mut value, "external_evidence", "evidence_id", "ev:registry-publication")?;
        row["class"] = Value::String(class.to_string());
        row["meets_independent_lifecycle_threshold"] = Value::Bool(true);
        assert_rejected(&value, "can carry it")
            .with_context(|| format!("`{class}` must not be able to carry the threshold"))?;
    }
    Ok(())
}

#[test]
fn an_absorb_ruling_cannot_stand_beside_qualifying_evidence() -> Result<()> {
    // The consistency rule in the other direction: if the audit's own evidence
    // said the package earns an independent lifecycle, `absorb` would be
    // contradicting it.
    let mut value = real_value()?;
    row_mut(&mut value, "external_evidence", "evidence_id", "ev:reverse-dependencies")?["meets_independent_lifecycle_threshold"] =
        Value::Bool(true);
    assert_rejected(&value, "cannot stand while its own evidence")
}

#[test]
fn a_new_public_reexport_alias_must_move_the_inventory() -> Result<()> {
    // `reconcile_reexport_sites` only proved authored rows point at live lines.
    // It never asked the opposite question, so a second `pub use perl_ast_v2 as
    // other;` in an already-inventoried file changed no checked set.
    let derived =
        derive_public_reexports("pub use perl_ast_v2 as v2;\npub use perl_ast_v2 as alt;");
    let aliases: BTreeSet<&str> = derived.iter().map(|(alias, _)| alias.as_str()).collect();
    assert!(aliases.contains("v2"), "the inventoried alias must be derived: {aliases:?}");
    assert!(aliases.contains("alt"), "a second alias must be derived too: {aliases:?}");

    // A private `use` is not a re-export and must not be derived.
    assert!(derive_public_reexports("use perl_ast_v2 as internal;").is_empty());
    Ok(())
}

/// Build re-export rows from `(id, path, site)` triples, so the two directions
/// of the inventory law can be falsified without a fixture repository.
fn reexport_rows(rows: &[(&str, &str, &str)]) -> Result<Vec<ReexportRow>> {
    rows.iter()
        .map(|(id, path, site)| {
            serde_json::from_value(serde_json::json!({
                "reexport_id": id,
                "path": path,
                "site": site,
                "exposes": "whole package",
                "consumer_id": "c:test",
                "compatibility_obligation": "a test row",
            }))
            .with_context(|| "a test re-export row failed to deserialize")
        })
        .collect()
}

fn sources(entries: &[(&str, &str)]) -> BTreeMap<String, String> {
    entries.iter().map(|(path, text)| ((*path).to_string(), (*text).to_string())).collect()
}

#[test]
fn a_public_reexport_in_an_uninventoried_file_moves_the_inventory() -> Result<()> {
    // The first version of this check only looked inside files a row already
    // named, which made it circular: it could find a second alias beside a
    // recorded one, but a first public path in any other file — a new crate
    // forwarding the package, or a compatibility shim added during absorption —
    // changed no checked set. The scan set is now the candidate set.
    let rows = reexport_rows(&[("rx:known", "perl_ast::v2", "crates/perl-ast/src/lib.rs:93")])?;
    let live = sources(&[
        ("crates/perl-ast/src/lib.rs", "pub use perl_ast_v2 as v2;"),
        ("crates/other/src/lib.rs", "// nothing public here\nuse perl_ast_v2 as internal;"),
    ]);
    reconcile_reexport_inventory(&rows, &live)?;

    let drifted = sources(&[
        ("crates/perl-ast/src/lib.rs", "pub use perl_ast_v2 as v2;"),
        ("crates/other/src/lib.rs", "pub use perl_ast_v2 as forwarded;"),
    ]);
    let Err(err) = reconcile_reexport_inventory(&rows, &drifted) else {
        bail!("a public re-export in an uninventoried file must be rejected");
    };
    let rendered = format!("{err:?}");
    assert!(
        rendered.contains("crates/other/src/lib.rs") && rendered.contains("forwarded"),
        "the rejection must name the new public path: {rendered}"
    );
    Ok(())
}

#[test]
fn a_reexport_that_stops_being_public_cannot_stay_inventoried() -> Result<()> {
    // The reverse direction, and the one `reconcile_reexport_sites` cannot see:
    // it only asks whether the named line still mentions the package, so
    // `pub use perl_ast_v2 as v2;` becoming `pub(crate)` — or being renamed —
    // leaves the row green while the compatibility obligation attached to it
    // describes a path consumers can no longer write.
    let rows = reexport_rows(&[("rx:known", "perl_ast::v2", "crates/perl-ast/src/lib.rs:93")])?;

    for (label, text, needle) in [
        ("demoted to crate-private", "pub(crate) use perl_ast_v2 as v2;", "binds that path"),
        // A rename fails on the forward direction first: the new name is a
        // public path with no row of its own.
        ("renamed", "pub use perl_ast_v2 as v_two;", "v_two"),
        ("deleted", "// pub use perl_ast_v2 as v2;", "binds that path"),
    ] {
        let drifted = sources(&[("crates/perl-ast/src/lib.rs", text)]);
        let Err(err) = reconcile_reexport_inventory(&rows, &drifted) else {
            bail!("a re-export {label} must not leave the inventory green");
        };
        let rendered = format!("{err:?}");
        assert!(
            rendered.contains(needle),
            "a re-export {label} must be rejected for that reason: {rendered}"
        );
    }
    Ok(())
}

#[test]
fn a_reexport_row_must_name_the_crate_that_owns_its_site() -> Result<()> {
    // Row paths were matched against derived bindings by `::`-segment suffix
    // alone. The suffix is needed — the derivation sees only inside one file, so
    // a file that *is* module `engine` renders `ast_v2` for the row
    // `perl_parser_core::engine::ast_v2` — but on its own it accepted any crate
    // prefix whatsoever. A row could therefore name a crate that publishes
    // nothing and be validated by a real binding in a different crate's file:
    // the compatibility inventory recording a public path that does not exist,
    // which is the one thing these rows are for.
    let live = sources(&[("crates/perl-ast/src/lib.rs", "pub use perl_ast_v2 as v2;")]);

    // The honest row still reconciles.
    let correct = reexport_rows(&[("rx:known", "perl_ast::v2", "crates/perl-ast/src/lib.rs:93")])?;
    reconcile_reexport_inventory(&correct, &live)?;

    // The same binding suffix under a crate that does not own the site must not.
    for wrong in ["perl_parser::v2", "perl_ast_v2::v2", "not_a_crate::v2"] {
        let rows = reexport_rows(&[("rx:wrong", wrong, "crates/perl-ast/src/lib.rs:93")])?;
        let Err(err) = reconcile_reexport_inventory(&rows, &live) else {
            bail!("a row claiming `{wrong}` at a `perl-ast` site must not reconcile");
        };
        let rendered = format!("{err:?}");
        assert!(
            rendered.contains("perl_ast"),
            "the rejection must name the crate that owns the site: {rendered}"
        );
    }

    // The suffix relaxation the prefix check must not have broken: a row whose
    // path carries an intermediate module the file cannot see still binds, so
    // long as it starts at that file's own crate.
    let nested = sources(&[("crates/perl-parser-core/src/engine/mod.rs", "pub use perl_ast_v2;")]);
    let engine_row = reexport_rows(&[(
        "rx:engine",
        "perl_parser_core::engine::perl_ast_v2",
        "crates/perl-parser-core/src/engine/mod.rs:12",
    )])?;
    reconcile_reexport_inventory(&engine_row, &nested)?;
    Ok(())
}

#[test]
fn a_reexport_row_must_name_the_module_that_actually_publishes_it() -> Result<()> {
    // Anchoring the crate root was not enough. A row could still name any module
    // inside that crate, because the tail was matched as a suffix: `c::b::ast_v2`
    // was satisfied by the binding derived in `crates/c/src/a.rs`. The
    // compatibility inventory could therefore attach an obligation to a path
    // nobody can write. The whole path is now compared against the file's own
    // module path.
    let live = sources(&[("crates/c/src/a.rs", "pub use perl_ast_v2 as ast_v2;")]);

    let correct = reexport_rows(&[("rx:a", "c::a::ast_v2", "crates/c/src/a.rs:1")])?;
    reconcile_reexport_inventory(&correct, &live)?;

    // Right crate, right alias, wrong module.
    let wrong = reexport_rows(&[("rx:b", "c::b::ast_v2", "crates/c/src/a.rs:1")])?;
    let Err(err) = reconcile_reexport_inventory(&wrong, &live) else {
        bail!("a row naming a module that does not publish the alias must not reconcile");
    };
    // The row does not cover the binding, so the real public path is reported as
    // uninventoried. That is the distinction: under suffix matching this row
    // *did* cover it and the reconciliation passed.
    let rendered = format!("{err:?}");
    assert!(
        rendered.contains("crates/c/src/a.rs") && rendered.contains("ast_v2"),
        "the rejection must name the file and alias the row failed to cover: {rendered}"
    );

    // The layout cases the derivation has to get right for the real rows.
    for (file, expected) in [
        ("crates/perl-ast/src/lib.rs", "perl_ast"),
        ("crates/perl-parser/src/compat.rs", "perl_parser::compat"),
        ("crates/perl-parser-core/src/engine/mod.rs", "perl_parser_core::engine"),
        ("crates/perl-parser-core/src/tokens/trivia.rs", "perl_parser_core::tokens::trivia"),
        ("crates/crate-a/src/shim.rs", "crate_a::shim"),
    ] {
        assert_eq!(
            module_path_of(file).as_deref(),
            Some(expected),
            "module path derived from {file}"
        );
    }

    // Outside the crate source layout the answer is unknown, and unknown must
    // not read as "matches" — a `#[path]` attribute can put a file anywhere.
    assert!(module_path_of("xtask/src/lib.rs").is_none());
    assert!(module_path_of("Cargo.toml").is_none());

    // An inline module still composes: the file supplies the outer namespace
    // and the derivation supplies the inner one.
    let inline = sources(&[(
        "crates/perl-parser/src/lib.rs",
        "pub mod compat { pub use perl_ast_v2 as ast_v2; }",
    )]);
    let nested = reexport_rows(&[(
        "rx:compat",
        "perl_parser::compat::ast_v2",
        "crates/perl-parser/src/lib.rs:1",
    )])?;
    reconcile_reexport_inventory(&nested, &inline)?;
    Ok(())
}

#[test]
fn a_conditional_layout_attribute_still_makes_field_order_contract() -> Result<()> {
    // `render_contract_attrs` already carried `cfg_attr`; `field_order_is_contract`
    // looked only at a bare `repr`. So `#[cfg_attr(unix, repr(C))]` had its fields
    // sorted, and reordering them — an ABI break for every unix consumer — moved
    // no row. The same rule, syntax-aware in one place and blind in its sibling.
    let shape_of = |src: &str| -> Result<String> {
        derive_public_items(src)?
            .iter()
            .find(|item| item.kind == "struct")
            .map(|item| item.shape.clone())
            .ok_or_else(|| color_eyre::eyre::eyre!("no struct derived from {src}"))
    };
    for attr in ["#[cfg_attr(unix, repr(C))]", "#[cfg_attr(feature = \"x\", repr(packed))]"] {
        assert_ne!(
            shape_of(&format!("{attr}\npub struct S {{ pub a: u8, pub b: u16 }}"))?,
            shape_of(&format!("{attr}\npub struct S {{ pub b: u16, pub a: u8 }}"))?,
            "under `{attr}` the field order can be the ABI and must move the shape"
        );
    }

    // A `cfg_attr` that applies no layout-fixing representation must not switch
    // ordering on, or every conditional attribute would start firing on cosmetic
    // reorderings. The `feature = "c"` predicate is the trap: it contains a `C`.
    for attr in
        ["#[cfg_attr(feature = \"c\", derive(Debug))]", "#[cfg_attr(unix, repr(transparent))]"]
    {
        assert_eq!(
            shape_of(&format!("{attr}\npub struct S {{ pub a: u8, pub b: u16 }}"))?,
            shape_of(&format!("{attr}\npub struct S {{ pub b: u16, pub a: u8 }}"))?,
            "`{attr}` fixes no field order and must not make a reordering fire"
        );
    }
    Ok(())
}

#[test]
fn a_qualified_public_impl_is_not_mistaken_for_a_private_local_type() -> Result<()> {
    // The private-type filter matched on the self type's *last* segment, so a
    // module holding a private `Helper` swallowed `impl exported::Helper` too —
    // a public type's methods vanishing from the inventory with no error. The
    // audit losing public API is the failure it is least permitted, and this one
    // was silent.
    let source = "struct Helper;\n\
                  impl Helper { pub fn hidden(&self) {} }\n\
                  pub mod exported { pub struct Helper; }\n\
                  impl exported::Helper { pub fn surfaced(&self) {} }";
    let derived = derive_public_items(source)?;
    let shapes: Vec<&str> = derived.iter().map(|item| item.shape.as_str()).collect();

    assert!(
        shapes.iter().any(|shape| shape.contains("surfaced")),
        "the impl on the qualified public type must be derived: {shapes:?}"
    );
    assert!(
        !shapes.iter().any(|shape| shape.contains("hidden")),
        "the impl on the private local type must still be skipped: {shapes:?}"
    );
    Ok(())
}

#[test]
fn a_public_reexport_inside_a_public_module_is_a_public_path() -> Result<()> {
    // `pub mod compat { pub use perl_ast_v2 as ast_v2; }` publishes the package
    // exactly as a top-level `pub use` does. A top-level-only walk called such a
    // file re-export-free, which is the same blind spot in a different shape.
    let derived = derive_public_reexports("pub mod compat { pub use perl_ast_v2 as ast_v2; }");
    let bindings: BTreeSet<&str> = derived.iter().map(|(binding, _)| binding.as_str()).collect();
    assert!(
        bindings.contains("compat::ast_v2"),
        "a public module's re-export is public, under its module path: {bindings:?}"
    );

    // A private module publishes nothing outside the crate, so it is not one.
    assert!(
        derive_public_reexports("mod internal { pub use perl_ast_v2 as ast_v2; }").is_empty(),
        "a private module's re-export is not a public path"
    );
    Ok(())
}

#[test]
fn a_forwarding_reexport_must_terminate_in_the_inventory() -> Result<()> {
    // Half the real rows forward through a local path rather than naming the
    // package — `pub use engine::ast_v2;` — and the pattern that recognizes
    // those cannot tell them from `pub use unrelated::ast_v2;`, a same-named
    // module of some other package. Telling those apart needs name resolution
    // this instrument does not do. What it can require is that the target
    // terminate in the inventory, so swapping one for an unrelated module names
    // a path no row describes.
    let rows = reexport_rows(&[
        ("rx:direct", "c::engine::ast_v2", "crates/c/src/engine/mod.rs:1"),
        ("rx:forward", "c::ast_v2", "crates/c/src/lib.rs:1"),
    ])?;

    let chained = sources(&[
        ("crates/c/src/engine/mod.rs", "pub use perl_ast_v2 as ast_v2;"),
        ("crates/c/src/lib.rs", "pub use engine::ast_v2;"),
    ]);
    reconcile_reexport_inventory(&rows, &chained)?;

    // `crate::`-rooted forwarding is the same chain written differently and
    // must reconcile identically, or the rule would fire on ordinary style.
    let rooted = sources(&[
        ("crates/c/src/engine/mod.rs", "pub use perl_ast_v2 as ast_v2;"),
        ("crates/c/src/lib.rs", "pub use crate::engine::ast_v2;"),
    ]);
    reconcile_reexport_inventory(&rows, &rooted)?;

    let unrelated = sources(&[
        ("crates/c/src/engine/mod.rs", "pub use perl_ast_v2 as ast_v2;"),
        ("crates/c/src/lib.rs", "pub use some_other_package::ast_v2;"),
    ]);
    let Err(err) = reconcile_reexport_inventory(&rows, &unrelated) else {
        bail!("a row must not stay live on a forwarding path to an unrelated module");
    };
    let rendered = format!("{err:?}");
    assert!(
        rendered.contains("some_other_package::ast_v2"),
        "the rejection must name the target that leaves the inventory: {rendered}"
    );
    Ok(())
}

#[test]
fn each_forwarding_hop_resolves_in_its_own_crate() -> Result<()> {
    // The crate a relative target belongs to is the crate of the path the chain
    // is standing on. Deriving it once from the origin file meant that after a
    // hop into another crate, that crate's own relative target was resolved
    // against the origin — a namespace neither path belongs to.
    //
    // Three rows: crate A forwards into crate B, B forwards through a local
    // relative path, and that lands on the audited package. Crate A also
    // carries a same-named `shim` path, so a second hop that retained the
    // origin context would resolve into A and terminate on the wrong chain.
    let rows = reexport_rows(&[
        ("rx:a", "crate_a::ast_v2", "crates/crate-a/src/lib.rs:1"),
        ("rx:a-shim", "crate_a::shim::ast_v2", "crates/crate-a/src/shim.rs:1"),
        ("rx:b", "crate_b::ast_v2", "crates/crate-b/src/lib.rs:1"),
        ("rx:b-shim", "crate_b::shim::ast_v2", "crates/crate-b/src/shim.rs:1"),
    ])?;
    let chain = sources(&[
        // A forwards into B.
        ("crates/crate-a/src/lib.rs", "pub use crate_b::ast_v2;"),
        // A's own `shim` is a dead end that names the package nowhere. If the
        // second hop resolved `shim` against A, it would land here.
        ("crates/crate-a/src/shim.rs", "pub use crate_b::ast_v2;"),
        // B forwards through its own relative `shim`.
        ("crates/crate-b/src/lib.rs", "pub use shim::ast_v2;"),
        // B's `shim` terminates on the package.
        ("crates/crate-b/src/shim.rs", "pub use perl_ast_v2 as ast_v2;"),
    ]);
    reconcile_reexport_inventory(&rows, &chain)?;

    // And the chain still has to terminate: break only B's shim and the whole
    // chain fails, which proves the walk actually traverses all three hops
    // rather than stopping early on the first inventoried match.
    let broken = sources(&[
        ("crates/crate-a/src/lib.rs", "pub use crate_b::ast_v2;"),
        ("crates/crate-a/src/shim.rs", "pub use crate_b::ast_v2;"),
        ("crates/crate-b/src/lib.rs", "pub use shim::ast_v2;"),
        ("crates/crate-b/src/shim.rs", "pub use somewhere_else::ast_v2;"),
    ]);
    assert!(
        reconcile_reexport_inventory(&rows, &broken).is_err(),
        "a chain whose final hop leaves the inventory must be rejected"
    );
    Ok(())
}

#[test]
fn a_forwarding_target_does_not_resolve_through_another_crate() -> Result<()> {
    // A bare `ast_v2` is a suffix of several inventoried paths across crates.
    // Taking the first suffix match made the answer depend on map order and let
    // a local module validate through another crate's genuine chain.
    let rows = reexport_rows(&[
        // A real chain in one crate.
        (
            "rx:core-engine",
            "perl_parser_core::engine::ast_v2",
            "crates/perl-parser-core/src/engine/mod.rs:1",
        ),
        // An unrelated local `ast_v2` in a different crate, forwarding to
        // nothing of its own.
        ("rx:other", "other_crate::ast_v2", "crates/other-crate/src/lib.rs:1"),
    ])?;
    let across = sources(&[
        ("crates/perl-parser-core/src/engine/mod.rs", "pub use perl_ast_v2 as ast_v2;"),
        ("crates/other-crate/src/lib.rs", "pub use local::ast_v2;"),
    ]);
    let Err(err) = reconcile_reexport_inventory(&rows, &across) else {
        bail!("an unrelated local module must not resolve through another crate's chain");
    };
    let rendered = format!("{err:?}");
    assert!(
        rendered.contains("local::ast_v2") || rendered.contains("other-crate"),
        "the rejection must be about the unrelated path: {rendered}"
    );

    // The crate root is read from the site path, so the same-crate case still
    // resolves and this is not simply rejecting every forward.
    let same_crate = reexport_rows(&[
        (
            "rx:core-engine",
            "perl_parser_core::engine::ast_v2",
            "crates/perl-parser-core/src/engine/mod.rs:1",
        ),
        ("rx:core-root", "perl_parser_core::ast_v2", "crates/perl-parser-core/src/lib.rs:1"),
    ])?;
    let within = sources(&[
        ("crates/perl-parser-core/src/engine/mod.rs", "pub use perl_ast_v2 as ast_v2;"),
        ("crates/perl-parser-core/src/lib.rs", "pub use engine::ast_v2;"),
    ]);
    reconcile_reexport_inventory(&same_crate, &within)?;
    Ok(())
}

#[test]
fn an_item_position_macro_invocation_stops_the_audit() -> Result<()> {
    // A macro invocation at item position can expand to public structs, enums,
    // functions, impls or re-exports, and `syn` does not expand macros. Skipping
    // it because the macro itself is private let a whole public surface exist
    // with no row and no error — the one outcome this derivation refuses. The
    // privacy of the macro says nothing about the privacy of what it emits.
    let Err(err) = derive_public_items("define_nodes! { Alpha, Beta }") else {
        bail!("an item-position macro invocation must stop the audit");
    };
    let rendered = format!("{err:?}");
    assert!(
        rendered.contains("define_nodes!"),
        "the bail must name the invocation it cannot see through: {rendered}"
    );

    // A private `macro_rules!` *definition* is still skipped: it declares a
    // macro, not items, so failing on it would reject an ordinary internal
    // helper for a reason unrelated to the public API.
    derive_public_items("macro_rules! helper { () => {} }")?;
    // And an exported one still bails, on its own separate reason.
    let Err(err) = derive_public_items("#[macro_export]\nmacro_rules! shown { () => {} }") else {
        bail!("an exported macro must still stop the audit");
    };
    assert!(format!("{err:?}").contains("macro_export"));
    Ok(())
}

#[test]
fn a_public_extern_crate_alias_is_a_public_path() -> Result<()> {
    // `pub extern crate perl_ast_v2 as ast_v2;` publishes the package under
    // that alias to every downstream consumer, exactly as a `pub use` does. It
    // is a different item kind, so it fell through the catch-all and published
    // a path with no row behind it.
    let derived: BTreeSet<String> =
        derive_public_reexports("pub extern crate perl_ast_v2 as ast_v2;")
            .into_iter()
            .map(|(binding, _)| binding)
            .collect();
    assert_eq!(derived, BTreeSet::from(["ast_v2".to_string()]));

    // Unaliased, the crate name is the binding.
    let plain: BTreeSet<String> = derive_public_reexports("pub extern crate perl_ast_v2;")
        .into_iter()
        .map(|(binding, _)| binding)
        .collect();
    assert_eq!(plain, BTreeSet::from(["perl_ast_v2".to_string()]));

    // A private one publishes nothing, and an unrelated crate is not the
    // package.
    assert!(derive_public_reexports("extern crate perl_ast_v2 as ast_v2;").is_empty());
    assert!(derive_public_reexports("pub extern crate serde as ast_v2;").is_empty());

    // It reconciles like any other public path: no row, no pass.
    let rows = reexport_rows(&[("rx:one", "c::other", "crates/c/src/lib.rs:1")])?;
    let sources = sources(&[(
        "crates/c/src/lib.rs",
        "pub use perl_ast_v2 as other;\npub extern crate perl_ast_v2 as ast_v2;",
    )]);
    let Err(err) = reconcile_reexport_inventory(&rows, &sources) else {
        bail!("a public extern crate alias must demand its own row");
    };
    assert!(
        format!("{err:?}").contains("ast_v2"),
        "the rejection must name the uninventoried alias: {err:?}"
    );
    Ok(())
}

#[test]
fn a_forwarding_chain_that_never_reaches_the_package_is_rejected() -> Result<()> {
    // Stepping once was not enough: a target that merely matches *some*
    // inventoried path is satisfied by another forwarding row, so two rows
    // pointing at each other passed with no direct export anywhere — the
    // inventory vouching for itself.
    let rows = reexport_rows(&[
        ("rx:a", "c::a::ast_v2", "crates/c/src/a.rs:1"),
        ("rx:b", "c::b::ast_v2", "crates/c/src/b.rs:1"),
    ])?;

    // The cycle: a forwards to b, b forwards to a, neither names the package.
    let cyclic = sources(&[
        ("crates/c/src/a.rs", "pub use crate::b::ast_v2;"),
        ("crates/c/src/b.rs", "pub use crate::a::ast_v2;"),
    ]);
    let Err(err) = reconcile_reexport_inventory(&rows, &cyclic) else {
        bail!("a forwarding cycle with no direct export must be rejected");
    };
    assert!(
        format!("{err:?}").contains("cycles through"),
        "the rejection must name the cycle rather than a missing path: {err:?}"
    );

    // The same two rows resolve once one end is a real export, so the rule is
    // about reaching the package and not about chain length.
    let grounded = sources(&[
        ("crates/c/src/a.rs", "pub use crate::b::ast_v2;"),
        ("crates/c/src/b.rs", "pub use perl_ast_v2 as ast_v2;"),
    ]);
    reconcile_reexport_inventory(&rows, &grounded)?;
    Ok(())
}

#[test]
fn a_nested_module_sharing_the_package_name_is_not_the_package() -> Result<()> {
    // `names_package_directly` used a substring test, so `other::perl_ast_v2` —
    // a nested module of some other package that happens to share the name —
    // counted as the package itself. That let the chaining rule wave through
    // exactly the lookalike it exists to catch: the path was called direct, so
    // it never had to terminate in the inventory.
    assert!(names_package_directly("perl_ast_v2"), "the crate itself is direct");
    assert!(names_package_directly("perl_ast_v2::Node"));
    assert!(names_package_directly("perl_ast::v2"));
    assert!(names_package_directly("perl_parser_core::DiagnosticId"));

    assert!(!names_package_directly("other::perl_ast_v2"), "a nested lookalike is not the crate");
    assert!(!names_package_directly("other::perl_ast::v2"));
    assert!(!names_package_directly("wrapper::perl_parser_core::DiagnosticId"));
    // A `perl_parser_core` path to something that is not one of the two
    // re-exported v2 types is not a direct reference either.
    assert!(!names_package_directly("perl_parser_core::Parser"));

    // End to end: the lookalike must now be forced to chain, and fail.
    let rows = reexport_rows(&[("rx:one", "c::ast_v2", "crates/c/src/lib.rs:1")])?;
    let lookalike = sources(&[("crates/c/src/lib.rs", "pub use other::perl_ast_v2 as ast_v2;")]);
    let Err(err) = reconcile_reexport_inventory(&rows, &lookalike) else {
        bail!("a nested lookalike must not satisfy a compatibility row");
    };
    assert!(
        format!("{err:?}").contains("other::perl_ast_v2"),
        "the rejection must name the lookalike path: {err:?}"
    );
    Ok(())
}

#[test]
fn two_modules_exporting_one_alias_are_two_compatibility_paths() -> Result<()> {
    // Carrying only the leaf alias collapsed `a::ast_v2` and `b::ast_v2` into
    // one indistinguishable binding, so a single row covered both and a real
    // public path carried no compatibility obligation — which is the entire
    // purpose of these rows.
    let source = "pub mod a { pub use perl_ast_v2 as ast_v2; }\n\
                  pub mod b { pub use perl_ast_v2 as ast_v2; }";
    let derived: BTreeSet<String> =
        derive_public_reexports(source).into_iter().map(|(binding, _)| binding).collect();
    assert_eq!(
        derived,
        BTreeSet::from(["a::ast_v2".to_string(), "b::ast_v2".to_string()]),
        "each module's re-export is its own public path"
    );

    // One row cannot cover both, and the row that exists covers only its own.
    let rows = reexport_rows(&[("rx:a", "c::a::ast_v2", "crates/c/src/lib.rs:1")])?;
    let both = sources(&[("crates/c/src/lib.rs", source)]);
    let Err(err) = reconcile_reexport_inventory(&rows, &both) else {
        bail!("the second module's public path must demand its own row");
    };
    let rendered = format!("{err:?}");
    assert!(
        rendered.contains("b::ast_v2"),
        "the rejection must name the uncovered path, not the covered one: {rendered}"
    );

    // With only the first module present, the same row reconciles cleanly, so
    // the rejection above is about the second path and not about the form of
    // the row.
    let only_a =
        sources(&[("crates/c/src/lib.rs", "pub mod a { pub use perl_ast_v2 as ast_v2; }")]);
    reconcile_reexport_inventory(&rows, &only_a)?;
    Ok(())
}

#[test]
fn a_grouped_reexport_row_claims_each_name_it_lists() -> Result<()> {
    // `perl_parser_core::{DiagnosticId, MissingKind}` is one row carrying two
    // public names. The earlier coverage test was a substring search over the
    // whole row path, which would call an alias covered because its letters
    // happened to appear somewhere in the text; the row now resolves to the
    // exact set of names it publishes, and both must stay live.
    // Both rows are present because the grouped one forwards through the local
    // `ast_v2` alias, and a forwarding target has to terminate in the
    // inventory — the same chain the real crate has.
    let rows = reexport_rows(&[
        (
            "rx:types",
            "perl_parser_core::{DiagnosticId, MissingKind}",
            "crates/perl-parser-core/src/lib.rs:97",
        ),
        ("rx:module", "perl_parser_core::ast_v2", "crates/perl-parser-core/src/lib.rs:101"),
    ])?;
    let module = "pub use perl_ast_v2 as ast_v2;\n";
    let both = sources(&[(
        "crates/perl-parser-core/src/lib.rs",
        &format!("{module}pub use ast_v2::{{DiagnosticId, MissingKind}};"),
    )]);
    reconcile_reexport_inventory(&rows, &both)?;

    let halved = sources(&[(
        "crates/perl-parser-core/src/lib.rs",
        &format!("{module}pub use ast_v2::{{DiagnosticId}};"),
    )]);
    let Err(err) = reconcile_reexport_inventory(&rows, &halved) else {
        bail!("dropping one name of a grouped row must be rejected");
    };
    assert!(
        format!("{err:?}").contains("MissingKind"),
        "the rejection must name the dropped type: {err:?}"
    );
    Ok(())
}

#[test]
fn a_grouped_canonical_import_enters_the_denominator() -> Result<()> {
    // `use perl_ast::{v2, Node};` names none of the four scan tokens, because
    // the canonical path is split by the braces. The audit recorded this as a
    // known limitation; it is now detected instead.
    assert!(!mentions_audited_package("use perl_ast::{v2, Node};"));
    assert_eq!(parsed_api_use("use perl_ast::{v2, Node};"), Some(true));
    assert!(references_package_api_in_code("use perl_ast::{v2, Node};", "crates/a/src/lib.rs"));
    Ok(())
}

#[test]
fn an_unparseable_file_that_never_names_the_package_is_not_a_consumer() -> Result<()> {
    // Discovery and classification take opposite defaults on a parse failure,
    // deliberately. Failing closed in discovery flagged a 590-line Perl fixture
    // with zero mentions of the package, which would have forced a meaningless
    // inventory row on an unrelated file.
    assert_eq!(parsed_api_use("this is not valid rust {{{"), None);
    let root = repo_root_for_tests()?;
    let scanned = derive_reference_files(&root)?;
    // The negative assertion below is satisfied for free by a renamed or
    // deleted file, and this is the only control for the open default discovery
    // takes on a parse failure. Pin the fixture's existence so the control
    // cannot pass while proving nothing.
    let fixture = "crates/perl-lsp-rs/tests/fixtures/parser/comprehensive_syntax_fixtures.rs";
    assert!(
        root.join(fixture).exists(),
        "the unparseable-fixture control names a file that no longer exists: {fixture}"
    );
    assert_eq!(
        parsed_api_use(&std::fs::read_to_string(root.join(fixture))?),
        None,
        "the control is only meaningful while that fixture is still unparseable as Rust"
    );
    assert!(
        !scanned.contains(fixture),
        "a fixture that never names the package must not be pulled into the denominator"
    );
    Ok(())
}

#[test]
fn a_private_macro_is_skipped_but_an_exported_one_stops_the_audit() -> Result<()> {
    // Macros and extern blocks carry no visibility, so the private-item skip
    // cannot reach them. Bailing on all of them would block an ordinary
    // refactor that adds an internal helper macro; bailing on none would let
    // exported surface vanish. Privacy is decided per kind instead.
    assert!(
        derive_public_items("pub mod inner { macro_rules! helper { () => {} } }")?.is_empty(),
        "a private helper macro must not fail the audit"
    );
    assert!(
        derive_public_items("extern \"C\" { fn private_thing(); }")?.is_empty(),
        "an extern block with no public items must not fail the audit"
    );

    match derive_public_items("#[macro_export]\nmacro_rules! shipped { () => {} }") {
        Ok(items) => bail!("an exported macro is public API and must bail, got {items:?}"),
        Err(err) => assert!(format!("{err:#}").contains("macro_export"), "{err:#}"),
    }
    match derive_public_items("extern \"C\" { pub fn shipped(); }") {
        Ok(items) => bail!("an extern block with public items must bail, got {items:?}"),
        Err(err) => assert!(format!("{err:#}").contains("extern block"), "{err:#}"),
    }
    Ok(())
}

#[test]
fn the_read_only_guard_catches_module_aliasing() -> Result<()> {
    // `use std::fs as F;` then `F::write(..)` produces no `fs::` substring
    // anywhere, so the first version of this allowlist passed it with zero
    // offenders — the same substring fragility the allowlist was meant to end.
    assert!(violates_read_only("use std::fs as F;"));
    assert!(violates_read_only("use std::process as P;"));
    assert!(!violates_read_only("let text = std::fs::read_to_string(path)?;"));
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
    // A crate path used as sample data, and the same underscored path inside a
    // string literal: both are strings, not paths, so neither is API use.
    assert!(!references_package_api_in_code(
        "fn fixture() { let p = \"crates/perl-ast-v2/src/lib.rs\"; let _ = p; }\n",
        "xtask/tests/fixture.rs"
    ));
    assert!(!references_package_api_in_code(
        "fn fixture() { let s = r#\"use perl_ast_v2::Node;\"#; let _ = s; }\n",
        "xtask/tests/fixture.rs"
    ));
    // A glob string contains `/*`. The stripper this replaced treated that as an
    // unterminated block comment and discarded every line after it, including
    // real code — the defect that defeated this guard outright.
    assert!(references_package_api_in_code(
        "fn f() { let g = \"crates/perl-ast-v2/*.rs\"; let _ = g; }\nuse perl_ast_v2::Node;\n",
        "crates/a/src/lib.rs"
    ));
    // And a `//` inside a string literal must not eat the code beside it.
    assert!(references_package_api_in_code(
        "use perl_ast_v2::Node;\nfn f() { let s = \"// see docs\"; let _ = s; }\n",
        "crates/a/src/lib.rs"
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
fn a_doctest_counts_as_code_but_a_plain_comment_does_not() -> Result<()> {
    // A doctest inside `///` or `//!` is compiled and run, so an API reference
    // there is real use and must not be downgradable to a prose mention. An
    // ordinary `//` comment genuinely is prose. `////` is an ordinary comment,
    // not a doc comment, which is why the third slash is checked rather than
    // assumed.
    // Revised deliberately: an earlier version expected an *unfenced* `//!`
    // line naming an API path to count as code. It does not — `rustdoc`
    // compiles fenced and indented blocks, not prose — and treating a sentence
    // as compiled use forced a gating consumer role onto documentation-only
    // files. The single loose case is replaced by the precise ones: what
    // `rustdoc` compiles is code, in either doc spelling, and prose is not.
    for (label, source, expected) in [
        (
            "fenced doctest in ///",
            "/// ```\n/// use perl_ast_v2::Node;\n/// ```\npub fn f() {}",
            true,
        ),
        ("fenced doctest in //!", "//! ```\n//! use perl_ast_v2::Node;\n//! ```", true),
        ("indented doctest", "///     use perl_ast_v2::Node;\npub fn f() {}", true),
        (
            "hidden doctest line",
            "/// ```\n/// # use perl_ast_v2::Node;\n/// ```\npub fn f() {}",
            true,
        ),
        ("prose naming a path", "//! See perl_ast::v2::NodeKind for the variants.", false),
        ("prose in ///", "/// Superseded by perl_ast_v2::Node.\npub fn f() {}", false),
        (
            "non-rust fence",
            "/// ```text\n/// use perl_ast_v2::Node;\n/// ```\npub fn f() {}",
            false,
        ),
        ("plain // comment", "// use perl_ast_v2::Node;\npub fn f() {}", false),
        ("//// ordinary comment", "//// use perl_ast_v2::Node;\npub fn f() {}", false),
        ("block comment", "/* use perl_ast_v2::Node; */\npub fn f() {}", false),
        ("real code", "use perl_ast_v2::Node;", true),
    ] {
        assert_eq!(
            references_package_api_in_code(source, "crates/a/src/lib.rs"),
            expected,
            "{label} was classified wrongly"
        );
    }
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
fn a_private_field_is_typed_but_never_named() -> Result<()> {
    // NodeIdGenerator's counter is private. The shape must record its type — so
    // a retype that drops `Send` moves the row — without publishing its name as
    // API. This is the one struct in the crate with a private field, so it is
    // the real-source anchor for the rendering that
    // `a_private_fields_type_is_contract_even_though_its_name_is_not` proves in
    // the abstract.
    let source = std::fs::read_to_string(repo_root_for_tests()?.join(V2_SOURCE_RELATIVE_PATH))?;
    let derived = derive_public_items(&source)?;
    let generator = derived
        .iter()
        .find(|item| item.path == "perl_ast_v2::NodeIdGenerator")
        .ok_or_else(|| color_eyre::eyre::eyre!("NodeIdGenerator missing from the derivation"))?;
    assert!(
        generator.shape.contains("+non-public(NodeId)"),
        "the private field's type must be recorded: {}",
        generator.shape
    );
    assert!(
        !generator.shape.contains("next_id"),
        "the private field's name must not be published as API: {}",
        generator.shape
    );
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

    let mut offenders: Vec<String> = Vec::new();
    for (index, line) in source.lines().enumerate() {
        if violates_read_only(line) {
            offenders.push(format!("line {}: {}", index + 1, line.trim()));
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
        "use std::fs as F;",
        "use std::process as P;",
        "let out = duct::cmd!(\"rm\", path).run()?;",
    ];
    for line in evasions {
        assert!(violates_read_only(line), "the read-only guard must reject `{line}`");
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
        assert!(!violates_read_only(permitted), "the read-only guard must permit `{permitted}`");
    }
    Ok(())
}

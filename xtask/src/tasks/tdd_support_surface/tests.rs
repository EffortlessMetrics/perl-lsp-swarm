//! Falsifiers for the `perl-tdd-support` surface ledger.
//!
//! Each negative test names the specific way the ledger could stop being true
//! and proves the checker rejects it. A checker that only ever runs against a
//! correct ledger proves nothing: the point of these fixtures is that they fail
//! when the corresponding guard is removed.

use std::fs;
use std::path::{Path, PathBuf};

use color_eyre::eyre::{Result, bail};

use super::*;

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

/// Build a throwaway workspace root containing only the governed crate.
fn fixture_root(lib_rs: &str, manifest: &str) -> Result<tempfile::TempDir> {
    let dir = tempfile::tempdir()?;
    let src = dir.path().join(SUBJECT_CRATE_DIR).join("src");
    fs::create_dir_all(&src)?;
    fs::write(dir.path().join(SUBJECT_CRATE_DIR).join("Cargo.toml"), manifest)?;
    fs::write(src.join("lib.rs"), lib_rs)?;
    Ok(dir)
}

const MINIMAL_MANIFEST: &str = r#"
[package]
name = "perl-tdd-support"
version = "0.0.0"

[features]
default = []
"#;

/// A valid row for `id`, so a test can mutate exactly one field.
fn entry(id: &str, api_kind: &str, path: &str) -> LedgerEntry {
    LedgerEntry {
        id: id.to_string(),
        api_kind: api_kind.to_string(),
        path: path.to_string(),
        cfg: String::new(),
        behavior: "behaviour under test".to_string(),
        consumers: vec![],
        consumer_class: "self_only".to_string(),
        compatibility: "published".to_string(),
        disposition: "retain_pure".to_string(),
        replacement_owner: "none".to_string(),
        owner_issue: 8418,
        exit_condition: "reviewed at train close".to_string(),
        proof_command: "cargo test -p perl-tdd-support --locked".to_string(),
    }
}

fn ledger(entries: Vec<LedgerEntry>) -> Ledger {
    Ledger {
        schema_version: SCHEMA_VERSION,
        policy: POLICY_NAME.to_string(),
        subject_crate: SUBJECT_CRATE.to_string(),
        entry: entries,
    }
}

fn expect_err(result: Result<()>, needle: &str, what: &str) -> Result<()> {
    let Err(error) = result else {
        bail!("{what}: expected the checker to reject this, but it passed");
    };
    let rendered = format!("{error:#}");
    if !rendered.contains(needle) {
        bail!("{what}: expected an error mentioning {needle:?}, got: {rendered}");
    }
    Ok(())
}

fn repo_root() -> Result<PathBuf> {
    crate::utils::project_root()
}

// ---------------------------------------------------------------------------
// The live ledger must describe the live crate
// ---------------------------------------------------------------------------

#[test]
fn real_ledger_parses_and_satisfies_its_own_schema() -> Result<()> {
    let root = repo_root()?;
    let path = root.join(LEDGER_PATH);
    let ledger = load_ledger(&path)?;
    validate_ledger(&ledger, &path)?;
    if ledger.entry.is_empty() {
        bail!("the committed ledger governs nothing");
    }
    Ok(())
}

/// The load-bearing currency assertion: what the crate exports and what the
/// ledger governs are the same set, on this exact checkout.
#[test]
fn real_surface_and_real_ledger_agree() -> Result<()> {
    let root = repo_root()?;
    let ledger = load_ledger(&root.join(LEDGER_PATH))?;
    let discovered = discover_surface(&root)?;
    reconcile(&discovered, &ledger)
}

#[test]
fn real_projection_is_current() -> Result<()> {
    let root = repo_root()?;
    let ledger = load_ledger(&root.join(LEDGER_PATH))?;
    let edges = discover_consumers(&root)?;
    let rendered = render_projection(&ledger, &edges);
    let committed = fs::read_to_string(root.join(PROJECTION_PATH))?;
    if committed != rendered {
        bail!(
            "{PROJECTION_PATH} is stale; regenerate with `cargo xtask tdd-support-surface --write`"
        );
    }
    Ok(())
}

#[test]
fn every_named_consumer_really_depends_on_the_crate() -> Result<()> {
    let root = repo_root()?;
    let ledger = load_ledger(&root.join(LEDGER_PATH))?;
    let edges = discover_consumers(&root)?;
    validate_consumer_references(&ledger, &edges)
}

// ---------------------------------------------------------------------------
// Surface drift
// ---------------------------------------------------------------------------

#[test]
fn new_public_item_without_a_row_is_rejected() -> Result<()> {
    let dir = fixture_root("pub struct Governed;\npub struct Ungoverned;\n", MINIMAL_MANIFEST)?;
    let discovered = discover_surface(dir.path())?;
    let ledger = ledger(vec![
        entry("struct:perl_tdd_support::Governed", "struct", "perl_tdd_support::Governed"),
        entry("feature:default", "feature", "default"),
    ]);
    expect_err(
        reconcile(&discovered, &ledger),
        "unclassified public surface",
        "an added public struct with no ledger row",
    )
}

#[test]
fn new_public_module_without_a_row_is_rejected() -> Result<()> {
    let dir = fixture_root("pub mod extra { pub struct Inner; }\n", MINIMAL_MANIFEST)?;
    let discovered = discover_surface(dir.path())?;
    let ledger = ledger(vec![entry("feature:default", "feature", "default")]);
    expect_err(
        reconcile(&discovered, &ledger),
        "unclassified public surface",
        "an added public module with no ledger row",
    )
}

#[test]
fn new_cargo_feature_without_a_row_is_rejected() -> Result<()> {
    let manifest = format!("{MINIMAL_MANIFEST}extra = []\n");
    let dir = fixture_root("", &manifest)?;
    let discovered = discover_surface(dir.path())?;
    let ledger = ledger(vec![entry("feature:default", "feature", "default")]);
    expect_err(
        reconcile(&discovered, &ledger),
        "unclassified public surface",
        "an added Cargo feature with no ledger row",
    )
}

#[test]
fn deleted_symbol_leaves_a_stale_row_that_is_rejected() -> Result<()> {
    let dir = fixture_root("pub struct Kept;\n", MINIMAL_MANIFEST)?;
    let discovered = discover_surface(dir.path())?;
    let ledger = ledger(vec![
        entry("struct:perl_tdd_support::Kept", "struct", "perl_tdd_support::Kept"),
        entry("struct:perl_tdd_support::Removed", "struct", "perl_tdd_support::Removed"),
        entry("feature:default", "feature", "default"),
    ]);
    expect_err(
        reconcile(&discovered, &ledger),
        "stale ledger row",
        "a governed symbol deleted from source while its row remains",
    )
}

/// A rename must not be able to ride the old row: the identity key encodes the
/// name, so the new name is unclassified and the old row is stale.
#[test]
fn renamed_symbol_cannot_continue_under_the_old_row() -> Result<()> {
    let dir = fixture_root("pub struct NewName;\n", MINIMAL_MANIFEST)?;
    let discovered = discover_surface(dir.path())?;
    let ledger = ledger(vec![
        entry("struct:perl_tdd_support::OldName", "struct", "perl_tdd_support::OldName"),
        entry("feature:default", "feature", "default"),
    ]);
    let Err(error) = reconcile(&discovered, &ledger) else {
        bail!("a renamed symbol was allowed to reuse the previous row");
    };
    let rendered = format!("{error:#}");
    for needle in ["unclassified public surface", "stale ledger row"] {
        if !rendered.contains(needle) {
            bail!("expected the rename to report {needle:?}; got: {rendered}");
        }
    }
    Ok(())
}

/// Changing an item's kind is an API change even when the name is identical.
#[test]
fn kind_change_under_the_same_name_is_rejected() -> Result<()> {
    let dir = fixture_root("pub enum Shape { A }\n", MINIMAL_MANIFEST)?;
    let discovered = discover_surface(dir.path())?;
    let ledger = ledger(vec![
        entry("struct:perl_tdd_support::Shape", "struct", "perl_tdd_support::Shape"),
        entry("feature:default", "feature", "default"),
    ]);
    expect_err(
        reconcile(&discovered, &ledger),
        "unclassified public surface",
        "a struct replaced by an enum of the same name",
    )
}

/// A `cfg` gate is part of what a consumer must satisfy to name the item.
#[test]
fn cfg_gate_drift_is_rejected() -> Result<()> {
    let dir = fixture_root("#[cfg(windows)]\npub struct Gated;\n", MINIMAL_MANIFEST)?;
    let discovered = discover_surface(dir.path())?;
    let ledger = ledger(vec![
        entry("struct:perl_tdd_support::Gated", "struct", "perl_tdd_support::Gated"),
        entry("feature:default", "feature", "default"),
    ]);
    expect_err(
        reconcile(&discovered, &ledger),
        "cfg drift",
        "an item whose cfg gate changed without the row following",
    )
}

#[test]
fn cfg_gate_is_recorded_verbatim_when_it_matches() -> Result<()> {
    let dir = fixture_root("#[cfg(windows)]\npub struct Gated;\n", MINIMAL_MANIFEST)?;
    let discovered = discover_surface(dir.path())?;
    let gated = discovered
        .iter()
        .find(|item| item.path == "perl_tdd_support::Gated")
        .ok_or_else(|| color_eyre::eyre::eyre!("fixture item was not discovered"))?;
    if gated.cfg != "windows" {
        bail!("expected cfg `windows`, discovered {:?}", gated.cfg);
    }
    let mut row = entry("struct:perl_tdd_support::Gated", "struct", "perl_tdd_support::Gated");
    row.cfg = "windows".to_string();
    reconcile(&discovered, &ledger(vec![row, entry("feature:default", "feature", "default")]))
}

/// Discovery parses rather than compiles, so a Windows-gated item is found on
/// every host. Without this the ledger would be platform-dependent and the
/// check would pass or fail depending on the runner.
#[test]
fn platform_gated_items_are_discovered_on_every_host() -> Result<()> {
    let root = repo_root()?;
    let discovered = discover_surface(&root)?;
    let windows_only = discovered.iter().filter(|item| item.cfg.contains("windows")).count();
    if windows_only == 0 {
        bail!("expected the Windows-gated symlink surface to be discovered on this host");
    }
    Ok(())
}

/// A glob re-export would republish symbols no row could name.
#[test]
fn glob_reexport_is_refused_rather_than_skipped() -> Result<()> {
    let dir = fixture_root(
        "pub mod inner { pub struct Hidden; }\npub use inner::*;\n",
        MINIMAL_MANIFEST,
    )?;
    expect_err(
        discover_surface(dir.path()).map(|_| ()),
        "glob re-export",
        "a glob re-export that would hide public symbols from the ledger",
    )
}

#[test]
fn private_and_test_only_items_are_not_governed() -> Result<()> {
    let dir = fixture_root(
        "struct Private;\n\
         #[cfg(test)]\npub struct TestOnly;\n\
         #[cfg(test)]\nmod tests { pub struct Inner; }\n\
         pub struct Real;\n",
        MINIMAL_MANIFEST,
    )?;
    let discovered = discover_surface(dir.path())?;
    let governed: Vec<&str> = discovered.iter().map(|item| item.id.as_str()).collect();
    for unexpected in [
        "struct:perl_tdd_support::Private",
        "struct:perl_tdd_support::TestOnly",
        "struct:perl_tdd_support::tests::Inner",
    ] {
        if governed.contains(&unexpected) {
            bail!("{unexpected} should not be public surface, but discovery reported it");
        }
    }
    if !governed.contains(&"struct:perl_tdd_support::Real") {
        bail!("a genuinely public struct was not discovered: {governed:?}");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Ledger schema
// ---------------------------------------------------------------------------

#[test]
fn duplicate_row_ids_are_rejected() -> Result<()> {
    let bad = ledger(vec![
        entry("struct:perl_tdd_support::A", "struct", "perl_tdd_support::A"),
        entry("struct:perl_tdd_support::A", "struct", "perl_tdd_support::A"),
    ]);
    expect_err(
        validate_ledger(&bad, Path::new("ledger.toml")),
        "duplicate row id",
        "two rows claiming the same identity",
    )
}

#[test]
fn a_row_whose_id_disagrees_with_its_fields_is_rejected() -> Result<()> {
    let mut bad = entry("struct:perl_tdd_support::A", "struct", "perl_tdd_support::A");
    bad.path = "perl_tdd_support::B".to_string();
    expect_err(
        validate_ledger(&ledger(vec![bad]), Path::new("ledger.toml")),
        "inconsistent identity",
        "a row whose id no longer spells its own kind and path",
    )
}

#[test]
fn a_row_without_an_owner_issue_is_rejected() -> Result<()> {
    let mut bad = entry("struct:perl_tdd_support::A", "struct", "perl_tdd_support::A");
    bad.owner_issue = 0;
    expect_err(
        validate_ledger(&ledger(vec![bad]), Path::new("ledger.toml")),
        "no owner_issue",
        "a disposition with nobody accountable for carrying it out",
    )
}

#[test]
fn a_row_without_an_exit_condition_is_rejected() -> Result<()> {
    let mut bad = entry("struct:perl_tdd_support::A", "struct", "perl_tdd_support::A");
    bad.exit_condition = "   ".to_string();
    expect_err(
        validate_ledger(&ledger(vec![bad]), Path::new("ledger.toml")),
        "empty required field `exit_condition`",
        "a row that never expires and never gets reviewed",
    )
}

#[test]
fn a_wildcard_row_is_rejected() -> Result<()> {
    let mut bad = entry("struct:perl_tdd_support::*", "struct", "perl_tdd_support::*");
    bad.id = "struct:perl_tdd_support::*".to_string();
    expect_err(
        validate_ledger(&ledger(vec![bad]), Path::new("ledger.toml")),
        "wildcard path",
        "a catch-all row that would pre-classify future public additions",
    )
}

#[test]
fn an_unknown_disposition_is_rejected() -> Result<()> {
    let mut bad = entry("struct:perl_tdd_support::A", "struct", "perl_tdd_support::A");
    bad.disposition = "probably_fine".to_string();
    expect_err(
        validate_ledger(&ledger(vec![bad]), Path::new("ledger.toml")),
        "unknown disposition",
        "a disposition outside the closed vocabulary",
    )
}

#[test]
fn an_unknown_consumer_class_is_rejected() -> Result<()> {
    let mut bad = entry("struct:perl_tdd_support::A", "struct", "perl_tdd_support::A");
    bad.consumer_class = "some_users".to_string();
    expect_err(
        validate_ledger(&ledger(vec![bad]), Path::new("ledger.toml")),
        "unknown consumer_class",
        "a consumer class outside the closed vocabulary",
    )
}

/// "No consumers" has to be stated, not left blank: an unproven consumer set
/// and an empty one are different claims.
#[test]
fn an_external_consumer_class_with_no_named_consumers_is_rejected() -> Result<()> {
    let mut bad = entry("struct:perl_tdd_support::A", "struct", "perl_tdd_support::A");
    bad.consumer_class = "test_dev_workspace_consumer".to_string();
    bad.consumers = vec![];
    expect_err(
        validate_ledger(&ledger(vec![bad]), Path::new("ledger.toml")),
        "lists no consumers",
        "a row claiming external consumers without naming one",
    )
}

#[test]
fn not_proven_is_an_accepted_way_to_record_unknown_consumers() -> Result<()> {
    let mut row = entry("struct:perl_tdd_support::A", "struct", "perl_tdd_support::A");
    row.consumer_class = "not_proven".to_string();
    row.consumers = vec![];
    validate_ledger(&ledger(vec![row]), Path::new("ledger.toml"))
}

#[test]
fn a_wrong_schema_version_is_rejected() -> Result<()> {
    let mut bad =
        ledger(vec![entry("struct:perl_tdd_support::A", "struct", "perl_tdd_support::A")]);
    bad.schema_version = SCHEMA_VERSION + 1;
    expect_err(
        validate_ledger(&bad, Path::new("ledger.toml")),
        "unsupported schema_version",
        "a ledger written against a schema this checker does not understand",
    )
}

#[test]
fn an_empty_ledger_is_rejected() -> Result<()> {
    expect_err(
        validate_ledger(&ledger(vec![]), Path::new("ledger.toml")),
        "no rows",
        "an emptied ledger that would trivially satisfy nothing",
    )
}

#[test]
fn a_consumer_that_does_not_depend_on_the_crate_is_rejected() -> Result<()> {
    let mut row = entry("struct:perl_tdd_support::A", "struct", "perl_tdd_support::A");
    row.consumer_class = "test_dev_workspace_consumer".to_string();
    row.consumers = vec!["perl-not-a-consumer".to_string()];
    let edges = vec![ConsumerEdge {
        crate_name: "perl-lexer".to_string(),
        manifest: "crates/perl-lexer/Cargo.toml".to_string(),
        dep_kind: "dev-dependencies".to_string(),
        referenced: ["must".to_string()].into_iter().collect(),
        class: "must_only".to_string(),
    }];
    expect_err(
        validate_consumer_references(&ledger(vec![row]), &edges),
        "declares no dependency",
        "a consumer citation that has gone stale",
    )
}

// ---------------------------------------------------------------------------
// Consumer edge classification
// ---------------------------------------------------------------------------

fn names(list: &[&str]) -> BTreeSet<String> {
    list.iter().map(|name| (*name).to_string()).collect()
}

#[test]
fn edge_classes_separate_must_only_from_mixed_and_unused() -> Result<()> {
    let cases = [
        (vec!["must", "must_some"], "must_only"),
        (vec!["must_with", "must_err_with"], "must_only"),
        (vec!["must", "BddScenario"], "mixed"),
        (vec!["BddScenario"], "other_only"),
        (vec!["tdd_basic", "test_runner"], "other_only"),
        (vec![], "declared_unused"),
    ];
    for (referenced, expected) in cases {
        let actual = classify_edge(&names(&referenced));
        if actual != expected {
            bail!("{referenced:?} classified as {actual}, expected {expected}");
        }
    }
    Ok(())
}

/// #8605 acts on `must_only` edges wholesale, so a mixed edge misread as
/// `must_only` would have its dependency removed while it still needs it.
#[test]
fn a_mixed_edge_is_never_reported_as_must_only() -> Result<()> {
    let root = repo_root()?;
    let edges = discover_consumers(&root)?;
    for edge in &edges {
        if edge.class != "must_only" {
            continue;
        }
        let non_must: Vec<&String> =
            edge.referenced.iter().filter(|name| !MUST_FAMILY.contains(&name.as_str())).collect();
        if !non_must.is_empty() {
            bail!("{} is classified must_only but also references {non_must:?}", edge.crate_name);
        }
    }
    Ok(())
}

/// Prose mentioning these symbols must not manufacture a dependency edge.
/// The repository contains files whose entire purpose is naming them in
/// comments and string literals.
#[test]
fn comments_and_string_literals_do_not_create_references() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let crate_dir = dir.path().join("consumer");
    fs::create_dir_all(&crate_dir)?;
    fs::write(
        crate_dir.join("lib.rs"),
        "//! Mentions perl_tdd_support::tdd_basic::TestGenerator in prose.\n\
         const DOC: &str = \"perl_tdd_support::test_runner::TestRunner\";\n\
         use perl_tdd_support::must;\n\
         pub fn go() { let _ = DOC; let _ = must(Ok::<(), ()>(())); }\n",
    )?;
    let referenced = referenced_symbols(&crate_dir)?;
    if referenced != names(&["must"]) {
        bail!("expected only the real import to count, found {referenced:?}");
    }
    Ok(())
}

#[test]
fn production_and_dev_dependency_edges_are_distinguished() -> Result<()> {
    let root = repo_root()?;
    let edges = discover_consumers(&root)?;
    let kinds: BTreeSet<&str> = edges.iter().map(|edge| edge.dep_kind.as_str()).collect();
    if !kinds.contains("dependencies") || !kinds.contains("dev-dependencies") {
        bail!(
            "expected both production and dev dependency edges in the workspace, found {kinds:?}"
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Projection
// ---------------------------------------------------------------------------

#[test]
fn the_projection_is_deterministic() -> Result<()> {
    let root = repo_root()?;
    let ledger = load_ledger(&root.join(LEDGER_PATH))?;
    let edges = discover_consumers(&root)?;
    if render_projection(&ledger, &edges) != render_projection(&ledger, &edges) {
        bail!("the generated projection is not reproducible from the same inputs");
    }
    Ok(())
}

#[test]
fn the_projection_carries_a_do_not_edit_marker() -> Result<()> {
    let root = repo_root()?;
    let ledger = load_ledger(&root.join(LEDGER_PATH))?;
    let rendered = render_projection(&ledger, &discover_consumers(&root)?);
    if !rendered.to_lowercase().contains("do not edit") {
        bail!("the generated projection must announce that it is generated");
    }
    Ok(())
}

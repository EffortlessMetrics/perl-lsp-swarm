//! Falsifiers for the `perl-tdd-support` surface ledger.
//!
//! Each negative test names the specific way the ledger could stop being true
//! and proves the checker rejects it. A checker that only ever runs against a
//! correct ledger proves nothing: the point of these fixtures is that they fail
//! when the corresponding guard is removed.

use std::collections::BTreeSet;
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
fn a_consumer_the_symbol_never_reaches_is_rejected() -> Result<()> {
    let mut row = entry("struct:perl_tdd_support::A", "struct", "perl_tdd_support::A");
    row.consumer_class = "test_dev_workspace_consumer".to_string();
    row.consumers = vec!["perl-not-a-consumer".to_string()];
    let edges = vec![ConsumerEdge {
        crate_name: "perl-lexer".to_string(),
        manifest: "crates/perl-lexer/Cargo.toml".to_string(),
        dep_kind: "dev-dependencies".to_string(),
        referenced: ["must".to_string()].into_iter().collect(),
        class: "must_only".to_string(),
        enabled_features: BTreeSet::new(),
    }];
    expect_err(
        validate_derived_consumers(&ledger(vec![row]), &edges),
        "consumers are stale",
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
// Implicit Cargo features
// ---------------------------------------------------------------------------

/// An `optional = true` dependency creates an activatable feature even with no
/// `[features]` entry; a downstream crate can name it, so it is public surface.
#[test]
fn optional_dependency_creates_an_implicit_feature() -> Result<()> {
    let manifest = format!(
        "{MINIMAL_MANIFEST}\n[dependencies]\nserde = {{ version = \"1\", optional = true }}\n"
    );
    let dir = fixture_root("", &manifest)?;
    let discovered = discover_surface(dir.path())?;
    let ids: Vec<&str> = discovered.iter().map(|item| item.id.as_str()).collect();
    if !ids.contains(&"feature:serde") {
        bail!("implicit feature `serde` from the optional dependency was not discovered: {ids:?}");
    }
    Ok(())
}

/// A `dep:foo` reference in a feature value suppresses the implicit `foo`
/// feature — that is Cargo's rule, and the checker must follow it.
#[test]
fn dep_prefixed_reference_suppresses_the_implicit_feature() -> Result<()> {
    let manifest = format!(
        "{MINIMAL_MANIFEST}extra = [\"dep:serde\"]\n\n\
         [dependencies]\nserde = {{ version = \"1\", optional = true }}\n"
    );
    let dir = fixture_root("", &manifest)?;
    let discovered = discover_surface(dir.path())?;
    let ids: Vec<&str> = discovered.iter().map(|item| item.id.as_str()).collect();
    if ids.contains(&"feature:serde") {
        bail!("`dep:serde` should suppress the implicit `serde` feature, but it was discovered");
    }
    if !ids.contains(&"feature:extra") {
        bail!("the explicit `extra` feature was not discovered");
    }
    Ok(())
}

/// The real crate's two optional dependencies are discovered as features.
#[test]
fn real_crate_optional_deps_are_governed_features() -> Result<()> {
    let root = repo_root()?;
    let discovered = discover_surface(&root)?;
    let ids: BTreeSet<&str> = discovered.iter().map(|item| item.id.as_str()).collect();
    for expected in ["feature:lsp-types", "feature:url"] {
        if !ids.contains(expected) {
            bail!("implicit feature `{expected}` from an optional dependency was not discovered");
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Compound cfg(test)
// ---------------------------------------------------------------------------

/// `all(test, ..)` requires test, so the item is test-only and excluded.
#[test]
fn all_test_predicate_is_treated_as_test_only() -> Result<()> {
    let dir =
        fixture_root("#[cfg(all(test, windows))]\npub struct OnlyInTest;\n", MINIMAL_MANIFEST)?;
    let discovered = discover_surface(dir.path())?;
    if discovered.iter().any(|item| item.path == "perl_tdd_support::OnlyInTest") {
        bail!("`#[cfg(all(test, windows))]` item must be excluded as test-only");
    }
    Ok(())
}

/// `any(test, feature = "x")` is reachable via the feature alone, so the item
/// is NOT test-only and must be governed. A naive contains("test") would drop it.
#[test]
fn any_test_predicate_is_not_treated_as_test_only() -> Result<()> {
    let dir = fixture_root(
        "#[cfg(any(test, feature = \"x\"))]\npub struct AlsoInProd;\n",
        MINIMAL_MANIFEST,
    )?;
    let discovered = discover_surface(dir.path())?;
    if !discovered.iter().any(|item| item.path == "perl_tdd_support::AlsoInProd") {
        bail!(
            "`#[cfg(any(test, feature = ..))]` item is reachable without test and must be governed"
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Inherited cfg
// ---------------------------------------------------------------------------

/// An item inside a `#[cfg(..)]` module inherits that gate.
#[test]
fn item_inherits_enclosing_module_cfg() -> Result<()> {
    let dir = fixture_root(
        "#[cfg(feature = \"x\")]\npub mod gated { pub struct Inner; pub fn f() {} }\n",
        MINIMAL_MANIFEST,
    )?;
    let discovered = discover_surface(dir.path())?;
    for path in ["perl_tdd_support::gated::Inner", "perl_tdd_support::gated::f"] {
        let item = discovered
            .iter()
            .find(|d| d.path == path)
            .ok_or_else(|| color_eyre::eyre::eyre!("{path} not discovered"))?;
        if item.cfg != "feature=\"x\"" {
            bail!("{path} should inherit `feature=\"x\"`, recorded {:?}", item.cfg);
        }
    }
    Ok(())
}

/// A module gate and an identical item gate collapse to one term.
#[test]
fn duplicate_cfg_terms_are_deduplicated() -> Result<()> {
    let dir = fixture_root(
        "#[cfg(windows)]\npub mod w { #[cfg(windows)] pub fn f() {} }\n",
        MINIMAL_MANIFEST,
    )?;
    let discovered = discover_surface(dir.path())?;
    let f = discovered
        .iter()
        .find(|d| d.path == "perl_tdd_support::w::f")
        .ok_or_else(|| color_eyre::eyre::eyre!("w::f not discovered"))?;
    if f.cfg != "windows" {
        bail!("expected deduplicated `windows`, recorded {:?}", f.cfg);
    }
    Ok(())
}

/// The real crate's `lsp_integration` functions carry their module's feature
/// gate, not an empty cfg.
#[test]
fn real_lsp_integration_functions_carry_the_feature_gate() -> Result<()> {
    let root = repo_root()?;
    let discovered = discover_surface(&root)?;
    let f = discovered
        .iter()
        .find(|d| d.path.ends_with("lsp_integration::coverage_to_diagnostics"))
        .ok_or_else(|| color_eyre::eyre::eyre!("lsp_integration function not discovered"))?;
    if !f.cfg.contains("lsp-compat") {
        bail!("expected the lsp-compat gate to be inherited, recorded {:?}", f.cfg);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Exported macros
// ---------------------------------------------------------------------------

/// A `#[macro_export] macro_rules!` is public surface and needs a row.
#[test]
fn exported_macro_is_governed_surface() -> Result<()> {
    let dir =
        fixture_root("#[macro_export]\nmacro_rules! shout { () => {}; }\n", MINIMAL_MANIFEST)?;
    let discovered = discover_surface(dir.path())?;
    if !discovered.iter().any(|d| d.id == "macro:perl_tdd_support::shout") {
        bail!("`#[macro_export] macro_rules! shout` was not discovered as public surface");
    }
    Ok(())
}

/// A macro_rules! WITHOUT `#[macro_export]` is not crate-public and is skipped.
#[test]
fn non_exported_macro_is_not_governed() -> Result<()> {
    let dir = fixture_root("macro_rules! local { () => {}; }\n", MINIMAL_MANIFEST)?;
    let discovered = discover_surface(dir.path())?;
    if discovered.iter().any(|d| d.path == "perl_tdd_support::local") {
        bail!("a non-exported macro must not be governed public surface");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Target-specific dependency edges
// ---------------------------------------------------------------------------

/// A consumer that depends on the crate only under a `[target.'cfg(..)']`
/// table is still discovered as an edge.
#[test]
fn target_specific_dependency_is_a_consumer_edge() -> Result<()> {
    let manifest: toml::Value = toml::from_str(
        "[package]\nname = \"c\"\n\n\
         [target.'cfg(windows)'.dependencies]\nperl-tdd-support = { path = \"..\" }\n",
    )?;
    let kind = declared_dep_kind(&manifest);
    if kind.as_deref() != Some("dependencies") {
        bail!("target-specific production dependency was not recognized, got {kind:?}");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Derived consumers
// ---------------------------------------------------------------------------

/// The committed ledger's `consumers` match what the crates actually reference.
#[test]
fn real_ledger_consumers_are_derivation_current() -> Result<()> {
    let root = repo_root()?;
    let ledger = load_ledger(&root.join(LEDGER_PATH))?;
    let edges = discover_consumers(&root)?;
    validate_derived_consumers(&ledger, &edges)
}

/// A row that lists a crate not referencing its symbol is rejected — the exact
/// `must_err`/`perl-lexer` shape the review caught.
#[test]
fn a_consumer_the_symbol_does_not_reach_is_rejected() -> Result<()> {
    let edges = vec![
        ConsumerEdge {
            crate_name: "perl-dap".to_string(),
            manifest: "crates/perl-dap/Cargo.toml".to_string(),
            dep_kind: "dev-dependencies".to_string(),
            referenced: names(&["must_err"]),
            class: "must_only".to_string(),
            enabled_features: BTreeSet::new(),
        },
        ConsumerEdge {
            crate_name: "perl-lexer".to_string(),
            manifest: "crates/perl-lexer/Cargo.toml".to_string(),
            dep_kind: "dev-dependencies".to_string(),
            referenced: names(&["must"]),
            class: "must_only".to_string(),
            enabled_features: BTreeSet::new(),
        },
    ];
    let mut row =
        entry("reexport:perl_tdd_support::must_err", "reexport", "perl_tdd_support::must_err");
    row.consumer_class = "published_compatibility_surface".to_string();
    row.consumers = vec!["perl-dap".to_string(), "perl-lexer".to_string()]; // perl-lexer is wrong
    expect_err(
        validate_derived_consumers(&ledger(vec![row]), &edges),
        "consumers are stale",
        "a consumer list naming a crate that does not reference the symbol",
    )
}

/// A row that omits a real consumer is equally rejected.
#[test]
fn a_missing_real_consumer_is_rejected() -> Result<()> {
    let edges = vec![ConsumerEdge {
        crate_name: "perl-dap".to_string(),
        manifest: "crates/perl-dap/Cargo.toml".to_string(),
        dep_kind: "dev-dependencies".to_string(),
        referenced: names(&["must_some_with"]),
        class: "must_only".to_string(),
        enabled_features: BTreeSet::new(),
    }];
    let mut row = entry(
        "reexport:perl_tdd_support::must_some_with",
        "reexport",
        "perl_tdd_support::must_some_with",
    );
    row.consumer_class = "published_compatibility_surface".to_string();
    row.consumers = vec![]; // perl-dap is missing
    expect_err(
        validate_derived_consumers(&ledger(vec![row]), &edges),
        "consumers are stale",
        "a consumer list omitting a crate that really references the symbol",
    )
}

/// The tdd re-export alias means an item under `tdd::tdd_basic` is attributed
/// to crates reaching it by either path.
#[test]
fn tdd_reexport_alias_attributes_both_paths() -> Result<()> {
    let edges = vec![
        ConsumerEdge {
            crate_name: "perl-parser".to_string(),
            manifest: "crates/perl-parser/Cargo.toml".to_string(),
            dep_kind: "dependencies".to_string(),
            referenced: names(&["tdd"]),
            class: "other_only".to_string(),
            enabled_features: BTreeSet::new(),
        },
        ConsumerEdge {
            crate_name: "perl-lsp-rs".to_string(),
            manifest: "crates/perl-lsp-rs/Cargo.toml".to_string(),
            dep_kind: "dependencies".to_string(),
            referenced: names(&["tdd_basic"]),
            class: "other_only".to_string(),
            enabled_features: BTreeSet::new(),
        },
    ];
    let index = reference_index(&edges);
    let derived = derived_consumers("perl_tdd_support::tdd::tdd_basic::TddWorkflow", &index);
    if derived != names(&["perl-parser", "perl-lsp-rs"]) {
        bail!("expected both the full-path and re-export-path consumers, found {derived:?}");
    }
    Ok(())
}

/// A proven consumer set is incompatible with a `self_only` class.
#[test]
fn proven_consumers_reject_a_self_only_class() -> Result<()> {
    let edges = vec![ConsumerEdge {
        crate_name: "perl-dap".to_string(),
        manifest: "crates/perl-dap/Cargo.toml".to_string(),
        dep_kind: "dev-dependencies".to_string(),
        referenced: names(&["must"]),
        class: "must_only".to_string(),
        enabled_features: BTreeSet::new(),
    }];
    let mut row = entry("reexport:perl_tdd_support::must", "reexport", "perl_tdd_support::must");
    row.consumer_class = "self_only".to_string();
    row.consumers = vec!["perl-dap".to_string()];
    expect_err(
        validate_derived_consumers(&ledger(vec![row]), &edges),
        "claims consumer_class",
        "a row with real consumers claiming self_only",
    )
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

// ---------------------------------------------------------------------------
// Members: methods, fields, variants (#8418 scopes them explicitly)
// ---------------------------------------------------------------------------

const MEMBER_FIXTURE: &str = "\
pub struct Scenario { pub title: String, hidden: u8 }\n\
impl Scenario {\n\
    pub fn new() -> Self { Scenario { title: String::new(), hidden: 0 } }\n\
    pub fn given(self) -> Self { self }\n\
    fn private_step(&self) {}\n\
    #[cfg(test)]\n    pub fn only_in_tests(&self) {}\n\
}\n\
impl Default for Scenario { fn default() -> Self { Self::new() } }\n\
pub struct Pair(pub u8, u8);\n\
pub enum Outcome { Passed, Failed { reason: String } }\n\
struct Hidden;\n\
impl Hidden { pub fn reachable_only_in_crate() {} }\n";

fn governed_ids(root: &Path) -> Result<Vec<String>> {
    Ok(discover_surface(root)?.into_iter().map(|item| item.id).collect())
}

#[test]
fn public_inherent_methods_are_governed_members() -> Result<()> {
    let dir = fixture_root(MEMBER_FIXTURE, MINIMAL_MANIFEST)?;
    let ids = governed_ids(dir.path())?;
    for expected in
        ["method:perl_tdd_support::Scenario::new", "method:perl_tdd_support::Scenario::given"]
    {
        if !ids.iter().any(|id| id == expected) {
            bail!("public inherent method {expected} was not discovered: {ids:?}");
        }
    }
    for unexpected in [
        "method:perl_tdd_support::Scenario::private_step",
        "method:perl_tdd_support::Scenario::only_in_tests",
        "method:perl_tdd_support::Scenario::default",
        "method:perl_tdd_support::Hidden::reachable_only_in_crate",
    ] {
        if ids.iter().any(|id| id == unexpected) {
            bail!("{unexpected} is not public inherent surface, but discovery reported it");
        }
    }
    Ok(())
}

#[test]
fn public_fields_and_variants_are_governed_members() -> Result<()> {
    let dir = fixture_root(MEMBER_FIXTURE, MINIMAL_MANIFEST)?;
    let ids = governed_ids(dir.path())?;
    for expected in [
        "field:perl_tdd_support::Scenario::title",
        "field:perl_tdd_support::Pair::0",
        "variant:perl_tdd_support::Outcome::Passed",
        "variant:perl_tdd_support::Outcome::Failed",
    ] {
        if !ids.iter().any(|id| id == expected) {
            bail!("{expected} was not discovered: {ids:?}");
        }
    }
    for unexpected in
        ["field:perl_tdd_support::Scenario::hidden", "field:perl_tdd_support::Pair::1"]
    {
        if ids.iter().any(|id| id == unexpected) {
            bail!("private field {unexpected} must not be governed surface");
        }
    }
    Ok(())
}

#[test]
fn a_member_without_a_row_is_rejected() -> Result<()> {
    let dir = fixture_root("pub struct S;\nimpl S { pub fn m() {} }\n", MINIMAL_MANIFEST)?;
    let discovered = discover_surface(dir.path())?;
    let ledger = ledger(vec![
        entry("struct:perl_tdd_support::S", "struct", "perl_tdd_support::S"),
        entry("feature:default", "feature", "default"),
    ]);
    expect_err(
        reconcile(&discovered, &ledger),
        "`method:perl_tdd_support::S::m`",
        "a public method whose owning type is governed but which has no row of its own",
    )
}

#[test]
fn every_real_member_row_names_a_governed_owning_type() -> Result<()> {
    let root = repo_root()?;
    let ledger = load_ledger(&root.join(LEDGER_PATH))?;
    let paths: BTreeSet<&str> = ledger.entry.iter().map(|e| e.path.as_str()).collect();
    for entry in ledger.entry.iter().filter(|e| MEMBER_KINDS.contains(&e.api_kind.as_str())) {
        let Some((owner, _)) = entry.path.rsplit_once("::") else {
            bail!("member row `{}` has no owning type segment", entry.id);
        };
        if !paths.contains(owner) {
            bail!("member row `{}` names owner `{owner}` which has no ledger row", entry.id);
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Feature activation is a consumer edge
// ---------------------------------------------------------------------------

fn feature_edge(crate_name: &str, features: &[&str]) -> ConsumerEdge {
    ConsumerEdge {
        crate_name: crate_name.to_string(),
        manifest: format!("crates/{crate_name}/Cargo.toml"),
        dep_kind: "dependencies".to_string(),
        referenced: BTreeSet::new(),
        class: "declared_unused".to_string(),
        enabled_features: names(features),
    }
}

#[test]
fn a_feature_row_omitting_an_activating_consumer_is_rejected() -> Result<()> {
    let row = entry("feature:lsp-compat", "feature", "lsp-compat");
    let edges = vec![feature_edge("perl-parser", &["default", "lsp-compat"])];
    expect_err(
        validate_derived_consumers(&ledger(vec![row]), &edges),
        "row `feature:lsp-compat` consumers are stale",
        "a feature activated by a workspace crate but recorded as self_only",
    )
}

#[test]
fn a_feature_row_naming_its_activating_consumer_is_accepted() -> Result<()> {
    let mut row = entry("feature:lsp-compat", "feature", "lsp-compat");
    row.consumers = vec!["perl-parser".to_string()];
    row.consumer_class = "production_workspace_consumer".to_string();
    let edges = vec![feature_edge("perl-parser", &["default", "lsp-compat"])];
    validate_derived_consumers(&ledger(vec![row]), &edges)
}

#[test]
fn feature_activation_follows_cargo_spellings() -> Result<()> {
    let manifest: toml::Value = toml::from_str(
        r#"
[package]
name = "consumer"

[dependencies]
perl-tdd-support = { workspace = true, features = ["url"] }

[features]
lsp = ["perl-tdd-support/lsp-compat"]
maybe = ["perl-tdd-support?/lsp-types"]
"#,
    )?;
    let root_spec: toml::Value = toml::from_str(
        r#"path = "crates/perl-tdd-support"
default-features = false
features = ["extra"]"#,
    )?;
    let enabled = enabled_features(&manifest, Some(&root_spec));
    let expected = names(&["extra", "lsp-compat", "lsp-types", "url"]);
    if enabled != expected {
        bail!("expected {expected:?}, derived {enabled:?}");
    }
    let plain: toml::Value = toml::from_str(
        "[package]\nname = \"consumer\"\n[dependencies]\nperl-tdd-support = \"1\"\n",
    )?;
    if enabled_features(&plain, None) != names(&["default"]) {
        bail!("a plain dependency must activate exactly `default`");
    }
    Ok(())
}

#[test]
fn real_feature_rows_reflect_manifest_activation() -> Result<()> {
    let root = repo_root()?;
    let edges = discover_consumers(&root)?;
    let activated = feature_consumers("lsp-compat", &edges);
    if !activated.contains("perl-parser") {
        bail!("perl-parser activates `perl-tdd-support/lsp-compat` in its manifest: {activated:?}");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// The receipt is written in every mode
// ---------------------------------------------------------------------------

#[test]
fn propose_still_writes_the_requested_receipt() -> Result<()> {
    let dir = fixture_root("pub struct Ungoverned;\n", MINIMAL_MANIFEST)?;
    let ledger_path = dir.path().join(LEDGER_PATH);
    fs::create_dir_all(ledger_path.parent().ok_or_else(|| color_eyre::eyre::eyre!("no parent"))?)?;
    fs::write(
        &ledger_path,
        format!(
            "schema_version = {SCHEMA_VERSION}\npolicy = \"{POLICY_NAME}\"\n\
             subject_crate = \"{SUBJECT_CRATE}\"\n\n[[entry]]\nid = \"feature:default\"\n\
             api_kind = \"feature\"\npath = \"default\"\nbehavior = \"b\"\n\
             consumer_class = \"self_only\"\ncompatibility = \"published\"\n\
             disposition = \"retain_pure\"\nreplacement_owner = \"none\"\nowner_issue = 8418\n\
             exit_condition = \"e\"\nproof_command = \"p\"\n"
        ),
    )?;
    let receipt = Path::new("target/tdd-surface-receipt.json");
    let result = run(dir.path(), false, true, false, Some(receipt));
    if result.is_ok() {
        bail!("--propose must fail when an item is unclassified");
    }
    let written = dir.path().join(receipt);
    if !written.is_file() {
        bail!("--propose --json returned without writing {}", written.display());
    }
    let parsed: serde_json::Value = serde_json::from_str(&fs::read_to_string(&written)?)?;
    if parsed.get("policy").and_then(serde_json::Value::as_str) != Some(POLICY_NAME) {
        bail!("receipt does not carry the policy name: {parsed}");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Re-exports from private modules lift the type's members into surface
// ---------------------------------------------------------------------------

#[test]
fn members_of_a_type_reexported_from_a_private_module_are_governed() -> Result<()> {
    let dir = fixture_root(
        "mod inner {\n\
             pub struct Scenario { pub title: String }\n\
             impl Scenario { pub fn new() -> Self { Scenario { title: String::new() } } }\n\
             pub enum Verdict { Pass, Fail }\n\
             pub struct NotReexported { pub leaked: u8 }\n\
         }\n\
         pub use inner::Scenario;\n\
         pub use self::inner::Verdict as Outcome;\n",
        MINIMAL_MANIFEST,
    )?;
    let ids = governed_ids(dir.path())?;
    for expected in [
        "reexport:perl_tdd_support::Scenario",
        "method:perl_tdd_support::Scenario::new",
        "field:perl_tdd_support::Scenario::title",
        "reexport:perl_tdd_support::Outcome",
        "variant:perl_tdd_support::Outcome::Pass",
        "variant:perl_tdd_support::Outcome::Fail",
    ] {
        if !ids.iter().any(|id| id == expected) {
            bail!("{expected} was not lifted through the re-export: {ids:?}");
        }
    }
    for unexpected in [
        "struct:perl_tdd_support::inner::Scenario",
        "method:perl_tdd_support::inner::Scenario::new",
        "module:perl_tdd_support::inner",
        "struct:perl_tdd_support::inner::NotReexported",
        "field:perl_tdd_support::inner::NotReexported::leaked",
        "field:perl_tdd_support::NotReexported::leaked",
    ] {
        if ids.iter().any(|id| id == unexpected) {
            bail!("{unexpected} is behind a private module and must not be surface");
        }
    }
    Ok(())
}

#[test]
fn a_lifted_member_without_a_row_is_rejected() -> Result<()> {
    let dir = fixture_root(
        "mod inner { pub struct S; impl S { pub fn m() {} } }\npub use inner::S;\n",
        MINIMAL_MANIFEST,
    )?;
    let discovered = discover_surface(dir.path())?;
    let ledger = ledger(vec![
        entry("reexport:perl_tdd_support::S", "reexport", "perl_tdd_support::S"),
        entry("feature:default", "feature", "default"),
    ]);
    expect_err(
        reconcile(&discovered, &ledger),
        "`method:perl_tdd_support::S::m`",
        "a method reached only through a re-export from a private module",
    )
}

// ---------------------------------------------------------------------------
// Crate aliases are consumers too
// ---------------------------------------------------------------------------

fn consumer_dir(source: &str) -> Result<tempfile::TempDir> {
    let dir = tempfile::tempdir()?;
    fs::create_dir_all(dir.path().join("consumer"))?;
    fs::write(dir.path().join("consumer").join("lib.rs"), source)?;
    Ok(dir)
}

#[test]
fn a_renamed_crate_import_still_counts_as_a_reference() -> Result<()> {
    let dir = consumer_dir(
        "use perl_tdd_support as support;\n\
         use support::must;\n\
         pub fn go() { let _ = must(Ok::<(), ()>(())); support::tdd_basic::TestGenerator::new(); }\n",
    )?;
    let referenced = referenced_symbols(&dir.path().join("consumer"))?;
    if referenced != names(&["must", "tdd_basic"]) {
        bail!("alias paths must resolve to crate references, found {referenced:?}");
    }
    Ok(())
}

#[test]
fn self_rename_and_extern_crate_aliases_are_tracked() -> Result<()> {
    let dir = consumer_dir(
        "extern crate perl_tdd_support as legacy;\n\
         use perl_tdd_support::{self as fresh};\n\
         pub fn go() { legacy::governance::noop(); fresh::bdd::noop(); }\n",
    )?;
    let referenced = referenced_symbols(&dir.path().join("consumer"))?;
    if referenced != names(&["bdd", "governance"]) {
        bail!("expected both alias forms to resolve, found {referenced:?}");
    }
    Ok(())
}

#[test]
fn an_alias_of_another_crate_does_not_create_references() -> Result<()> {
    let dir = consumer_dir(
        "use some_other_crate as support;\n\
         use support::must;\n\
         pub fn go() { let _ = must(1); }\n",
    )?;
    let referenced = referenced_symbols(&dir.path().join("consumer"))?;
    if !referenced.is_empty() {
        bail!("an alias of an unrelated crate must not count, found {referenced:?}");
    }
    Ok(())
}

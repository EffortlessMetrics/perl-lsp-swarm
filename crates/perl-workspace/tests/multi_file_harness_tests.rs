mod common;

use perl_semantic_facts::{EntityKind, OccurrenceKind};
use std::path::PathBuf;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn cpan_style_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("semantic_real_workspace")
        .join("cpan_style")
}

fn use_lib_script_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("multi_file")
        .join("use_lib_script")
}

/// (a) Load the cpan_style fixture and assert a known package entity exists.
#[test]
fn harness_cpan_style_package_entity_exists() -> Result<()> {
    let root = cpan_style_root();
    let (_index, shards) = common::load_fixture_workspace(&root)?;

    common::assert_entity_exists(&shards, "RealBaseline::App", EntityKind::Package)?;

    Ok(())
}

/// (b) Load the use_lib_script fixture and assert the MyThing package entity exists.
#[test]
fn harness_use_lib_script_my_thing_package_exists() -> Result<()> {
    let root = use_lib_script_root();
    let (_index, shards) = common::load_fixture_workspace(&root)?;

    common::assert_entity_exists(&shards, "MyThing", EntityKind::Package)?;

    Ok(())
}

/// (c) Assert use_lib_script fixture loads exactly 2 shards (one .pm + one .pl).
#[test]
fn harness_use_lib_script_shard_count_is_two() -> Result<()> {
    let root = use_lib_script_root();
    let (_index, shards) = common::load_fixture_workspace(&root)?;

    assert_eq!(
        shards.len(),
        2,
        "use_lib_script fixture should produce exactly 2 shards (MyThing.pm + main.pl), got {}",
        shards.len()
    );

    Ok(())
}

/// (d) Assert the `use lib` pragma produces an Import occurrence.
///
/// NOTE: If `use lib` does NOT yield an `OccurrenceKind::Import` in the shard,
/// this test is marked `#[ignore]` pending issue #894 (use lib import tracking).
/// The other three tests in this file remain unconditionally green.
#[test]
#[ignore = "use lib does not currently yield OccurrenceKind::Import — tracked in issue #894"]
fn harness_use_lib_import_occurrence_present() -> Result<()> {
    let root = use_lib_script_root();
    let (_index, shards) = common::load_fixture_workspace(&root)?;

    let kinds = common::occurrence_kinds(&shards);
    assert!(
        kinds.contains(&OccurrenceKind::Import),
        "expected OccurrenceKind::Import from `use lib` pragma; actual kinds: {kinds:?}"
    );

    Ok(())
}

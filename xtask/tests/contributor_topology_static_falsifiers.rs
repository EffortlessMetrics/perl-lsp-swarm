//! Fixture-driven projection tests; localized expect calls keep setup readable.
#![allow(clippy::expect_used)]

#[path = "contributor_topology/support.rs"]
mod support;

use assert_cmd::Command;
use std::fs;
use std::path::{Path, PathBuf};
use support::contributor_topology::{build_projection, validate_projection};
use support::fixture_root;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .expect("xtask manifest has no repository parent")
}

/// Rewrite one authority file in the fixture and assert the projection refuses it.
///
/// Each guard in `static_sources` exists to stop a specific way the two source
/// files can stop meaning what the projection claims they mean. A guard with no
/// falsifier is an unproven guard, so every `bail!` there is driven from here.
fn rejects_when(file: &str, edit: impl Fn(&str) -> String) {
    let temp = fixture_root();
    let path = temp.path().join(file);
    let original = fs::read_to_string(&path).expect("read fixture source");
    fs::write(&path, edit(&original)).expect("write mutated fixture source");
    assert!(
        build_projection(temp.path(), None).is_err(),
        "projection accepted a {file} mutation it must reject"
    );
}

const IDENTITY: &str = "policy/product-identity.toml";
const PROTOCOL: &str = "docs/swarm/sync-protocol.md";
const RELEASE_SCHEMA: &str = "schemas/release_topology.v1.schema.json";

#[test]
fn unsupported_identity_schema_is_rejected() {
    rejects_when(IDENTITY, |text| text.replace("schema_version = 1", "schema_version = 2"));
}

#[test]
fn missing_product_table_is_rejected() {
    rejects_when(IDENTITY, |text| text.replace("[product]", "[produkt]"));
}

#[test]
fn missing_development_repository_is_rejected() {
    rejects_when(IDENTITY, |text| {
        text.lines()
            .filter(|line| !line.starts_with("development_repository"))
            .collect::<Vec<_>>()
            .join("\n")
    });
}

#[test]
fn missing_publication_repository_is_rejected() {
    rejects_when(IDENTITY, |text| {
        text.lines()
            .filter(|line| !line.starts_with("public_repository"))
            .collect::<Vec<_>>()
            .join("\n")
    });
}

#[test]
fn identical_development_and_publication_repositories_are_rejected() {
    rejects_when(IDENTITY, |text| {
        text.replace(
            "public_repository = \"EffortlessMetrics/perl-lsp\"",
            "public_repository = \"EffortlessMetrics/perl-lsp-swarm\"",
        )
    });
}

#[test]
fn repository_without_owner_slug_is_rejected() {
    rejects_when(IDENTITY, |text| text.replace("\"EffortlessMetrics/perl-lsp\"", "\"perl-lsp\""));
}

#[test]
fn repository_with_unsupported_characters_is_rejected() {
    rejects_when(IDENTITY, |text| {
        text.replace("\"EffortlessMetrics/perl-lsp\"", "\"Effortless Metrics/perl lsp\"")
    });
}

#[test]
fn duplicate_authority_rows_are_rejected() {
    rejects_when(PROTOCOL, |text| {
        text.replace(
            "| `perl-lsp-swarm/main` | Active development |",
            "| `perl-lsp-swarm/main` | Active development |\n| `perl-lsp-swarm/trunk` | Duplicate |",
        )
    });
}

#[test]
fn missing_authority_row_is_rejected() {
    rejects_when(PROTOCOL, |text| text.replace("| `perl-lsp/master` | Release lineage |", ""));
}

#[test]
fn missing_development_role_sentence_is_rejected() {
    rejects_when(PROTOCOL, |text| {
        text.replace("is the active development source of truth.", "is one of several repos.")
    });
}

#[test]
fn missing_publication_role_sentence_is_rejected() {
    rejects_when(PROTOCOL, |text| {
        text.replace("release, history, and canonical package-lineage repo.", "a mirror.")
    });
}

#[test]
fn contradictory_authority_labels_are_rejected() {
    rejects_when(PROTOCOL, |text| {
        text.replace("| Active development |", "| Temporary role |")
            .replace("| Release lineage |", "| Active development |")
            .replace("| Temporary role |", "| Release lineage |")
    });
}

#[test]
fn invalid_branch_syntax_is_rejected() {
    rejects_when(PROTOCOL, |text| text.replace("perl-lsp-swarm/main", "perl-lsp-swarm/-main"));
}

#[test]
fn missing_merge_marker_is_rejected() {
    rejects_when(PROTOCOL, |text| {
        text.replace("git merge -s ours --no-commit swarm/main", "git merge swarm/main")
    });
}

#[test]
fn missing_primary_channel_authority_is_rejected() {
    rejects_when(RELEASE_SCHEMA, |text| text.replace("\"primary_channels\"", "\"other_channels\""));
}

#[test]
fn duplicate_primary_channel_authority_is_rejected() {
    rejects_when(RELEASE_SCHEMA, |text| text.replace("open_vsx", "crates_io"));
}

#[test]
fn missing_promotion_mechanics_heading_is_rejected() {
    rejects_when(PROTOCOL, |text| {
        text.replace("#### Mechanics: history-preserving complete-tree merge", "#### Mechanics")
    });
}

#[test]
fn missing_promotion_command_marker_is_rejected() {
    rejects_when(PROTOCOL, |text| {
        text.replace("git read-tree -u --reset swarm/main", "git checkout swarm/main")
    });
}

#[test]
fn swapped_branches_fail_static_validation() {
    let temp = fixture_root();
    let path = temp.path().join("docs/swarm/sync-protocol.md");
    let text = fs::read_to_string(&path).expect("read protocol");
    fs::write(
        &path,
        text.replace("perl-lsp-swarm/main", "perl-lsp-swarm/master")
            .replace("perl-lsp/master", "perl-lsp/main"),
    )
    .expect("write swapped protocol");
    assert!(build_projection(temp.path(), None).is_err());
}

/// The projection's whole value is deriving facts from the repository's real
/// authority files. Fixtures cannot catch drift in those files, so bind the
/// contract to the actual sources: rewording the sync-protocol role sentences,
/// the branch authority table, or the promotion markers must fail here rather
/// than only at the contributor's terminal.
#[test]
fn real_repository_authority_still_projects() {
    let root = repo_root();
    let projection = build_projection(&root, None).expect("project real repository topology");
    let static_topology = &projection.static_topology;
    assert_eq!(static_topology.development_repository, "EffortlessMetrics/perl-lsp-swarm");
    assert_eq!(static_topology.development_default_branch, "main");
    assert_eq!(static_topology.publication_repository, "EffortlessMetrics/perl-lsp");
    assert_eq!(static_topology.publication_branch, "master");
    assert_eq!(static_topology.issue_repository, static_topology.development_repository);
    assert_eq!(
        static_topology.primary_channels,
        ["github_release", "crates_io", "vscode_marketplace", "open_vsx"]
    );
    validate_projection(&root, &projection).expect("validate real repository projection");
}

#[test]
fn source_change_makes_checked_projection_stale() {
    let temp = fixture_root();
    let projection = build_projection(temp.path(), None).expect("build projection");
    let path = temp.path().join("docs/swarm/sync-protocol.md");
    let mut text = fs::read_to_string(&path).expect("read protocol");
    text.push_str("\nAdditional historical note.\n");
    fs::write(path, text).expect("write protocol");
    assert!(validate_projection(temp.path(), &projection).is_err());
}

#[test]
fn source_digest_changes_when_authority_changes() {
    let temp = fixture_root();
    let original = build_projection(temp.path(), None).expect("build projection");
    let path = temp.path().join(PROTOCOL);
    let mut text = fs::read_to_string(&path).expect("read protocol");
    text.push_str("\nAuthority digest mutation.\n");
    fs::write(path, text).expect("write protocol mutation");
    let changed = build_projection(temp.path(), None).expect("build changed projection");
    assert_ne!(
        original.sources.get(PROTOCOL).expect("protocol source").sha256,
        changed.sources.get(PROTOCOL).expect("changed protocol source").sha256
    );
}

#[test]
fn schema_change_makes_checked_projection_stale() {
    let temp = fixture_root();
    let projection = build_projection(temp.path(), None).expect("build projection");
    let path = temp.path().join(RELEASE_SCHEMA);
    let mut text = fs::read_to_string(&path).expect("read release schema");
    text.push('\n');
    fs::write(path, text).expect("write release schema mutation");
    assert!(validate_projection(temp.path(), &projection).is_err());
}

#[test]
fn cli_check_rejects_stale_output() {
    let temp = fixture_root();
    let output = temp.path().join("projection.json");
    Command::cargo_bin("contributor-topology")
        .expect("find contributor-topology binary")
        .args([
            "--root",
            temp.path().to_str().expect("fixture root is UTF-8"),
            "--output",
            output.to_str().expect("output path is UTF-8"),
        ])
        .assert()
        .success();

    Command::cargo_bin("contributor-topology")
        .expect("find contributor-topology binary")
        .args([
            "--root",
            temp.path().to_str().expect("fixture root is UTF-8"),
            "--check",
            "--output",
            output.to_str().expect("output path is UTF-8"),
        ])
        .assert()
        .success()
        .stdout("contributor-topology: OK\n");

    let protocol = temp.path().join(PROTOCOL);
    let mut text = fs::read_to_string(&protocol).expect("read protocol");
    text.push_str("\nStale output must be rejected.\n");
    fs::write(protocol, text).expect("write protocol mutation");

    Command::cargo_bin("contributor-topology")
        .expect("find contributor-topology binary")
        .args([
            "--root",
            temp.path().to_str().expect("fixture root is UTF-8"),
            "--check",
            "--output",
            output.to_str().expect("output path is UTF-8"),
        ])
        .assert()
        .failure();
}

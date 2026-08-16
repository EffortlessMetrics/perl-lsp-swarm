//! Fixture-driven projection tests; localized expect calls keep setup readable.
#![allow(clippy::expect_used)]

#[path = "contributor_topology/support.rs"]
mod support;

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

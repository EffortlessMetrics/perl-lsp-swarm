#[path = "contributor_topology/support.rs"]
mod support;

use std::fs;
use support::contributor_topology::{build_projection, validate_projection};
use support::fixture_root;

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

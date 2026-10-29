//! No-diff provenance caveat (#17258).
//!
//! On the no-diff path (the only path `perllsp --ripr-facts` offers),
//! `input.base`/`input.head` echo caller strings verbatim — including garbage
//! — with no provenance caveat in the packet. The
//! `diff-provenance-unverified` disclosure existed only inside
//! `emit_changes_from_diff`, unreachable from the no-diff CLI. Packets with
//! caller-asserted refs and no analyzed diff must carry the caveat alongside
//! `no-diff-supplied` (when `changes` was requested) or on its own (when it
//! was not); packets without base/head gain no new noise; the diff-supplied
//! path keeps a single caveat (no duplicate).

#![deny(clippy::map_err_ignore)] // Cohort C0 activation (#12598); see src/lib.rs.

use perl_ripr_facts::{RiprFactsRequest, build_ripr_facts_packet};
use perl_tdd_support::{must, must_some, must_with};

const APP_PM: &str = "package App;\nuse strict;\nsub price {\n    my ($amount) = @_;\n    return $amount * 0.9;\n}\n1;\n";

const CAVEAT_DIFF: &str = "+++ b/lib/App.pm\n@@ -1,1 +1,2 @@\n use strict;\n+use POSIX;\n";

fn build_packet(
    root: &str,
    fact_classes: &str,
    base: Option<&str>,
    head: Option<&str>,
    diff: Option<&str>,
) -> serde_json::Value {
    must(build_ripr_facts_packet(&RiprFactsRequest {
        schema: "ripr-perl-facts-v1",
        root,
        base,
        head,
        fact_classes,
        diff,
    }))
}

fn stage_once(root: &str) {
    let _ = std::fs::remove_dir_all(root);
    let path = format!("{root}/lib/App.pm");
    must_with(std::fs::create_dir_all(format!("{root}/lib")), "mkdir caveat fixture");
    must_with(std::fs::write(&path, APP_PM), "stage App.pm");
}

fn limitation_ids(packet: &serde_json::Value) -> Vec<String> {
    must_some(packet["limitations"].as_array())
        .iter()
        .filter_map(|l| l["limitation_id"].as_str().map(String::from))
        .collect()
}

#[test]
fn nodiff_path_caveats_caller_asserted_refs() {
    let root = "target/ripr-g4/caveat-both-refs";
    stage_once(root);
    // Garbage refs echo verbatim (no-git-spawn design keeps them unverified),
    // but the packet must caveat them.
    let packet = build_packet(
        root,
        "files,owners,changes",
        Some("not a ref !!!"),
        Some("http://evil/x"),
        None,
    );
    let ids = limitation_ids(&packet);
    assert!(ids.contains(&"no-diff-supplied".to_string()), "got {ids:?}");
    assert!(
        ids.contains(&"diff-provenance-unverified".to_string()),
        "caller-asserted refs on the no-diff path must be caveated; got {ids:?}"
    );
    assert_eq!(packet["input"]["base"], "not a ref !!!");
}

#[test]
fn nodiff_path_caveats_base_only_ref() {
    let root = "target/ripr-g4/caveat-base-only";
    stage_once(root);
    let packet = build_packet(root, "files,owners,changes", Some("origin/main"), None, None);
    let ids = limitation_ids(&packet);
    assert!(
        ids.contains(&"diff-provenance-unverified".to_string()),
        "a base-only caller-asserted ref must be caveated; got {ids:?}"
    );
}

#[test]
fn nodiff_path_caveats_head_only_ref() {
    let root = "target/ripr-g4/caveat-head-only";
    stage_once(root);
    let packet = build_packet(root, "files,owners,changes", None, Some("HEAD"), None);
    let ids = limitation_ids(&packet);
    assert!(
        ids.contains(&"diff-provenance-unverified".to_string()),
        "a head-only caller-asserted ref must be caveated; got {ids:?}"
    );
}

#[test]
fn nodiff_path_caveats_refs_without_changes_requested() {
    // The caveat is derived independently of `wants_changes`: a caller
    // supplying base/head with `files` or `tests,oracles,relations` (and no
    // diff) gets unverified refs echoed, so the packet must caveat them even
    // though `no-diff-supplied` (a `changes`-request disclosure) is absent.
    let root = "target/ripr-g4/caveat-no-changes";
    stage_once(root);
    for fact_classes in ["files", "tests,oracles,relations"] {
        let packet = build_packet(root, fact_classes, Some("not a ref"), Some("bogus"), None);
        let ids = limitation_ids(&packet);
        assert!(
            !ids.contains(&"no-diff-supplied".to_string()),
            "{fact_classes}: changes not requested, so no no-diff-supplied; got {ids:?}"
        );
        assert!(
            ids.contains(&"diff-provenance-unverified".to_string()),
            "{fact_classes}: caller-asserted refs with no analyzed diff must be caveated; got {ids:?}"
        );
    }
}

#[test]
fn nodiff_path_without_refs_gains_no_caveat() {
    let root = "target/ripr-g4/caveat-no-refs";
    stage_once(root);
    let packet = build_packet(root, "files,owners,changes", None, None, None);
    let ids = limitation_ids(&packet);
    assert!(ids.contains(&"no-diff-supplied".to_string()), "got {ids:?}");
    assert!(
        !ids.contains(&"diff-provenance-unverified".to_string()),
        "no base/head means nothing to caveat; got {ids:?}"
    );
}

#[test]
fn diff_supplied_path_keeps_a_single_caveat() {
    let root = "target/ripr-g4/caveat-diff";
    stage_once(root);
    let packet = build_packet(
        root,
        "files,owners,changes",
        Some("origin/main"),
        Some("HEAD"),
        Some(CAVEAT_DIFF),
    );
    let ids = limitation_ids(&packet);
    assert!(
        !ids.contains(&"no-diff-supplied".to_string()),
        "a supplied diff is not the no-diff path; got {ids:?}"
    );
    assert_eq!(
        ids.iter().filter(|id| id.as_str() == "diff-provenance-unverified").count(),
        1,
        "diff-supplied path must keep exactly one caveat (no duplicate); got {ids:?}"
    );
}

//! No-diff provenance caveat (#17258).
//!
//! On the no-diff path (the only path `perllsp --ripr-facts` offers),
//! `input.base`/`input.head` echo caller strings verbatim — including garbage
//! — with no provenance caveat in the packet. The
//! `diff-provenance-unverified` disclosure existed only inside
//! `emit_changes_from_diff`, unreachable from the no-diff CLI. Packets with
//! caller-asserted refs and no diff must carry the caveat alongside
//! `no-diff-supplied`; packets without base/head gain no new noise; the
//! diff-supplied path keeps a single caveat (no duplicate).

#![deny(clippy::map_err_ignore)] // Cohort C0 activation (#12598); see src/lib.rs.

use perl_ripr_facts::{RiprFactsRequest, build_ripr_facts_packet};
use perl_tdd_support::{must, must_some, must_with};

const APP_PM: &str = "package App;\nuse strict;\nsub price {\n    my ($amount) = @_;\n    return $amount * 0.9;\n}\n1;\n";

const CAVEAT_DIFF: &str = "+++ b/lib/App.pm\n@@ -1,1 +1,2 @@\n use strict;\n+use POSIX;\n";

fn build_packet(base: Option<&str>, head: Option<&str>, diff: Option<&str>) -> serde_json::Value {
    must(build_ripr_facts_packet(&RiprFactsRequest {
        schema: "ripr-perl-facts-v1",
        root: "target/ripr-g4/caveat",
        base,
        head,
        fact_classes: "files,owners,changes",
        diff,
    }))
}

fn stage_once() {
    let root = "target/ripr-g4/caveat";
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
    stage_once();
    // Garbage refs echo verbatim (no-git-spawn design keeps them unverified),
    // but the packet must caveat them.
    let packet = build_packet(Some("not a ref !!!"), Some("http://evil/x"), None);
    let ids = limitation_ids(&packet);
    assert!(ids.contains(&"no-diff-supplied".to_string()), "got {ids:?}");
    assert!(
        ids.contains(&"diff-provenance-unverified".to_string()),
        "caller-asserted refs on the no-diff path must be caveated; got {ids:?}"
    );
    assert_eq!(packet["input"]["base"], "not a ref !!!");
}

#[test]
fn nodiff_path_without_refs_gains_no_caveat() {
    stage_once();
    let packet = build_packet(None, None, None);
    let ids = limitation_ids(&packet);
    assert!(ids.contains(&"no-diff-supplied".to_string()), "got {ids:?}");
    assert!(
        !ids.contains(&"diff-provenance-unverified".to_string()),
        "no base/head means nothing to caveat; got {ids:?}"
    );
}

#[test]
fn diff_supplied_path_keeps_a_single_caveat() {
    stage_once();
    let packet = build_packet(Some("origin/main"), Some("HEAD"), Some(CAVEAT_DIFF));
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

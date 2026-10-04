//! Recovered-parse honesty (#17255).
//!
//! `Parser::parse()` returns `Ok` whenever the parser *recovered*, so owners /
//! test facts projected from a recovered tree are real — but silently shipping
//! them contradicts `--check`, which FAILs the same files. Files whose parse
//! recovers with errors must carry a `parse-recovered:` /
//! `test-parse-recovered:` limitation naming the file and counts; hard
//! failures keep the `parse-failed:` path; clean files gain nothing.

#![deny(clippy::map_err_ignore)] // Cohort C0 activation (#12598); see src/lib.rs.

use perl_ripr_facts::{RiprFactsRequest, build_ripr_facts_packet};
use perl_tdd_support::{must, must_some, must_with};

const ALL_FACT_CLASSES: &str = "files,owners,changes,tests,oracles,relations,dynamic_boundaries,verify_commands,limitations,provenance";

/// Broken.pm from the #17255 probe: bad `my` list, missing `;`, unclosed
/// block — parses `Ok` via recovery with non-empty `errors()`.
const BROKEN_PM: &str = "package Broken;\nmy ($class = @_;\nsub new {\n    my $self = {};\n    return bless $self, $class;\n1;\n";

/// broken.t from the probe: unclosed `new(` — recovers with errors.
const BROKEN_T: &str =
    "use Test::More;\nmy $obj = Broken->new(;\nok($obj, 'constructed');\ndone_testing();\n";

const CLEAN_PM: &str = "package Clean;\nsub greet { return 'hi'; }\n1;\n";

/// Advisory-only fixtures: the nested-quantifier `qr/^(a+)+b$/` parses `Ok`
/// with one `Advisory` diagnostic and zero blocking errors (verified by probe
/// against `Parser::errors()` + `blocks_clean_parse`), so `--check` PASSES
/// these files (`cli.rs`: `failed = fatal || !blocking.is_empty()`).
const ADVISORY_PM: &str =
    "package Advisory;\nmy $re = qr/^(a+)+b$/;\nsub check { return $_[0] =~ $re; }\n1;\n";
const ADVISORY_T: &str =
    "use Test::More;\nmy $re = qr/^(a+)+b$/;\nok('aab' =~ $re, 'matches');\ndone_testing();\n";

fn build_packet(root: &str) -> serde_json::Value {
    must(build_ripr_facts_packet(&RiprFactsRequest {
        schema: "ripr-perl-facts-v1",
        root,
        base: Some("origin/main"),
        head: Some("HEAD"),
        fact_classes: ALL_FACT_CLASSES,
        diff: None,
    }))
}

fn stage_file(root: &str, rel: &str, content: &str) {
    let path = format!("{root}/{rel}");
    if let Some(parent) = std::path::Path::new(&path).parent() {
        must_with(std::fs::create_dir_all(parent), format!("mkdir for {rel}"));
    }
    must_with(std::fs::write(&path, content), format!("stage {rel}"));
}

fn fresh_root(name: &str) -> String {
    let root = format!("target/ripr-g1/{name}");
    let _ = std::fs::remove_dir_all(&root);
    root
}

fn limitation_ids(packet: &serde_json::Value) -> Vec<String> {
    must_some(packet["limitations"].as_array())
        .iter()
        .filter_map(|l| l["limitation_id"].as_str().map(String::from))
        .collect()
}

#[test]
fn recovered_parse_errors_are_limited_not_silent() {
    let root = fresh_root("broken");
    stage_file(&root, "lib/Broken.pm", BROKEN_PM);
    stage_file(&root, "t/broken.t", BROKEN_T);
    let packet = build_packet(&root);
    let _ = std::fs::remove_dir_all(&root);

    let ids = limitation_ids(&packet);
    assert!(
        ids.iter().any(|id| id == "parse-recovered:file:lib/Broken.pm"),
        "recovered .pm must surface parse-recovered; got {ids:?}"
    );
    assert!(
        ids.iter().any(|id| id == "test-parse-recovered:file:t/broken.t"),
        "recovered .t must surface test-parse-recovered; got {ids:?}"
    );
    // Recovery succeeded: facts are still emitted (fail soft, not silent).
    assert!(
        !must_some(packet["owners"].as_array()).is_empty(),
        "recovered tree must still yield owner facts"
    );
    assert!(
        !must_some(packet["tests"].as_array()).is_empty(),
        "recovered tree must still yield test facts"
    );
}

#[test]
fn advisory_only_files_carry_advisories_limitation_not_recovered() {
    let root = fresh_root("advisory");
    stage_file(&root, "lib/Advisory.pm", ADVISORY_PM);
    stage_file(&root, "t/advisory.t", ADVISORY_T);
    let packet = build_packet(&root);
    let _ = std::fs::remove_dir_all(&root);

    let ids = limitation_ids(&packet);
    assert!(
        ids.iter().any(|id| id == "parse-advisories:file:lib/Advisory.pm"),
        "advisory-only .pm must surface parse-advisories; got {ids:?}"
    );
    assert!(
        ids.iter().any(|id| id == "test-parse-advisories:file:t/advisory.t"),
        "advisory-only .t must surface test-parse-advisories; got {ids:?}"
    );
    assert!(
        ids.iter()
            .all(|id| !id.starts_with("parse-recovered:")
                && !id.starts_with("test-parse-recovered:")),
        "advisory-only files PASS --check, so no recovered limitation may fire; got {ids:?}"
    );
    // No recovery happened: facts still emitted from the clean tree.
    assert!(
        !must_some(packet["owners"].as_array()).is_empty(),
        "clean tree must still yield owner facts"
    );
    assert!(
        !must_some(packet["tests"].as_array()).is_empty(),
        "clean tree must still yield test facts"
    );
}

#[test]
fn clean_files_carry_no_recovered_limitation() {
    let root = fresh_root("clean");
    stage_file(&root, "lib/Clean.pm", CLEAN_PM);
    let packet = build_packet(&root);
    let _ = std::fs::remove_dir_all(&root);

    assert!(
        limitation_ids(&packet)
            .iter()
            .all(|id| !id.starts_with("parse-recovered:")
                && !id.starts_with("test-parse-recovered:")),
        "clean files must gain no recovered limitation; got {:?}",
        limitation_ids(&packet)
    );
}

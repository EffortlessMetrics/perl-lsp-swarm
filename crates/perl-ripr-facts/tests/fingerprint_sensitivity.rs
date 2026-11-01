//! D2 fingerprint-sensitivity bench (#17154).
//!
//! `packet_fingerprint` covers the packet's stable semantic identity tuples
//! (file ids/paths, owner ids, change ids, oracle target/sink tuples,
//! relation id/owner/test/oracle tuples — see `ripr_packet_fingerprint`). A
//! one-fact perturbation that moves any covered tuple must therefore change
//! the fingerprint; a perturbation that does not is a silent integrity hole.
//!
//! Each perturbation below rebuilds the same fixture with exactly one
//! input-level change and asserts the fingerprint differs from baseline:
//!
//! | # | perturbation | covered tuple(s) moved |
//! | P1 | append one `ok(...)` oracle at the end of `t/app.t` | +1 oracle tuple (earlier spans untouched) |
//! | P2 | remove `lib/Other.pm` | −1 file tuple, −owner tuples |
//! | P3 | rename `sub discount` → `sub discound` (same length) | 1 owner id (+ its relations' owner ids) |
//! | P4 | add `lib/Extra.pm` | +1 file tuple, +owner tuples |
//! | P5 | delete `sub risky` from `lib/App.pm` | −1 owner tuple |
//!
//! Threshold (D2): 100% — all 5 perturbations change the fingerprint (5/5).
//!
//! Out of scope by recipe design (documented in README + `ripr_packet_fingerprint`
//! rustdoc, table-pinned in `packet_fingerprint_doc_contract_*` unit tests
//! #17249): perturbations confined to uncovered families or fields (e.g. a
//! diff-content change that keeps the same change id) legitimately keep the
//! fingerprint stable.

#![deny(clippy::map_err_ignore)] // Cohort C0 activation (#12598); see src/lib.rs.

use perl_ripr_facts::{RiprFactsRequest, build_ripr_facts_packet};
use perl_tdd_support::{must, must_some_with, must_with};

const ALL_FACT_CLASSES: &str = "files,owners,changes,tests,oracles,relations,dynamic_boundaries,verify_commands,limitations,provenance";

/// Owner-isolation slice: P3/P5 move relation owner tuples as well as owner
/// ids, so the full-packet comparison would still pass if the fingerprint
/// stopped hashing `owners[]`. Rebuilding with only `files,owners` keeps
/// `relations[]` empty (asserted below), so a changed fingerprint proves the
/// owner ids themselves are covered.
const OWNER_ONLY_FACT_CLASSES: &str = "files,owners";

/// D2 bar from #17154: 1-fact perturbations are detected 100% (5/5 here).
const EXPECTED_DETECTED: usize = 5;

const APP_PM: &str = "package App;\nuse strict;\nuse warnings;\n\nsub discount {\n    my ($amount) = @_;\n    if ($amount > 100) {\n        return $amount * 0.9;\n    }\n    return $amount;\n}\n\nsub risky {\n    return 1;\n}\n\n1;\n";

const OTHER_PM: &str = "package Other;\n\nsub other_sub {\n    return 1;\n}\n\n1;\n";

const APP_T: &str = "use strict;\nuse warnings;\nuse Test::More;\n\nuse App;\n\nis(App::discount(50), 50, 'no discount under threshold');\nok(App::risky(), 'risky returns true');\n\ndone_testing;\n";

const EXTRA_PM: &str = "package Extra;\n\nsub bonus {\n    return 7;\n}\n\n1;\n";

/// Same-length rename of `discount` (8 chars → 8 chars) so byte spans — and
/// therefore every other span-derived id — stay put; only the renamed owner
/// id (and the relations referencing it) move.
const APP_PM_RENAMED: &str = "package App;\nuse strict;\nuse warnings;\n\nsub discound {\n    my ($amount) = @_;\n    if ($amount > 100) {\n        return $amount * 0.9;\n    }\n    return $amount;\n}\n\nsub risky {\n    return 1;\n}\n\n1;\n";

const APP_PM_NO_RISKY: &str = "package App;\nuse strict;\nuse warnings;\n\nsub discount {\n    my ($amount) = @_;\n    if ($amount > 100) {\n        return $amount * 0.9;\n    }\n    return $amount;\n}\n\n1;\n";

fn build_packet(root: &str, files: &[(&str, &str)]) -> serde_json::Value {
    build_packet_with_classes(root, files, ALL_FACT_CLASSES)
}

fn build_packet_with_classes(
    root: &str,
    files: &[(&str, &str)],
    fact_classes: &str,
) -> serde_json::Value {
    let _ = std::fs::remove_dir_all(root);
    for (rel, content) in files {
        let path = format!("{root}/{rel}");
        if let Some(parent) = std::path::Path::new(&path).parent() {
            must_with(std::fs::create_dir_all(parent), format!("mkdir for {rel}"));
        }
        must_with(std::fs::write(&path, content), format!("stage D2 file {rel}"));
    }
    let packet = must(build_ripr_facts_packet(&RiprFactsRequest {
        schema: "ripr-perl-facts-v1",
        root,
        base: Some("origin/main"),
        head: Some("HEAD"),
        fact_classes,
        diff: None,
    }));
    let _ = std::fs::remove_dir_all(root);
    packet
}

/// A `files,owners` packet must carry no relations: any entry would hash into
/// the fingerprint and void the owner-isolation argument below.
fn assert_no_relations(packet: &serde_json::Value, what: &str) {
    let relations = must_some_with(
        packet["relations"].as_array(),
        format!("{what} packet carries relations[]"),
    );
    assert!(
        relations.is_empty(),
        "{what} owners-only packet must have empty relations[], found {relations:?}"
    );
}

fn fingerprint(packet: &serde_json::Value, what: &str) -> String {
    must_some_with(
        packet["packet_fingerprint"].as_str(),
        format!("{what} packet carries a fingerprint"),
    )
    .to_string()
}

#[test]
fn one_fact_perturbations_always_change_the_fingerprint() {
    let baseline_files: &[(&str, &str)] =
        &[("lib/App.pm", APP_PM), ("lib/Other.pm", OTHER_PM), ("t/app.t", APP_T)];
    let baseline_fp =
        fingerprint(&build_packet("target/ripr-d2/baseline", baseline_files), "baseline");
    let owners_baseline_packet = build_packet_with_classes(
        "target/ripr-d2/owners-baseline",
        baseline_files,
        OWNER_ONLY_FACT_CLASSES,
    );
    assert_no_relations(&owners_baseline_packet, "owners-only baseline");
    let baseline_owner_fp = fingerprint(&owners_baseline_packet, "owners-only baseline");

    // P1 appends at the end so no existing span-derived id shifts: exactly one
    // oracle (and any relation it induces) is added.
    let p1_t = format!("{APP_T}ok(App::risky(), 'appended oracle');\n");
    let perturbations: &[(&str, Vec<(&str, &str)>)] = &[
        (
            "P1 add-one-oracle",
            vec![("lib/App.pm", APP_PM), ("lib/Other.pm", OTHER_PM), ("t/app.t", &p1_t)],
        ),
        ("P2 remove-one-file", vec![("lib/App.pm", APP_PM), ("t/app.t", APP_T)]),
        (
            "P3 alter-one-owner",
            vec![("lib/App.pm", APP_PM_RENAMED), ("lib/Other.pm", OTHER_PM), ("t/app.t", APP_T)],
        ),
        (
            "P4 add-one-file",
            vec![
                ("lib/App.pm", APP_PM),
                ("lib/Other.pm", OTHER_PM),
                ("lib/Extra.pm", EXTRA_PM),
                ("t/app.t", APP_T),
            ],
        ),
        (
            "P5 remove-one-owner",
            vec![("lib/App.pm", APP_PM_NO_RISKY), ("lib/Other.pm", OTHER_PM), ("t/app.t", APP_T)],
        ),
    ];

    let mut detected = 0usize;
    let mut report = format!("baseline fingerprint: {baseline_fp}\n");
    for (i, (label, files)) in perturbations.iter().enumerate() {
        let fp = fingerprint(&build_packet(&format!("target/ripr-d2/p{i}"), files), label);
        let changed = fp != baseline_fp;
        if changed {
            detected += 1;
        }
        report.push_str(&format!("{label}: {fp} (changed: {changed})\n"));
        if matches!(*label, "P3 alter-one-owner" | "P5 remove-one-owner") {
            let owner_packet = build_packet_with_classes(
                &format!("target/ripr-d2/owners-p{i}"),
                files,
                OWNER_ONLY_FACT_CLASSES,
            );
            assert_no_relations(&owner_packet, label);
            let owner_fp = fingerprint(&owner_packet, label);
            assert_ne!(
                owner_fp, baseline_owner_fp,
                "{label} must change the owners-only fingerprint"
            );
        }
    }
    assert_eq!(
        detected, EXPECTED_DETECTED,
        "D2 bar: 100% of one-fact perturbations change packet_fingerprint \
         ({EXPECTED_DETECTED}/{EXPECTED_DETECTED}); got {detected}\n{report}"
    );
}

const APP_PM_FOR_DIFF: &str = "package App;\nuse strict;\nuse warnings;\n\nsub compute {\n    my ($a, $b) = @_;\n    return $a + $b;\n}\n\n1;\n";

/// Same hunk span, different added line text → identical `change_id` → stable fingerprint (#17249).
const DIFF_RETURN_MINUS: &str =
    "+++ b/lib/App.pm\n@@ -7,1 +7,1 @@\n-return $a + $b;\n+return $a - $b;\n";
const DIFF_RETURN_TIMES: &str =
    "+++ b/lib/App.pm\n@@ -7,1 +7,1 @@\n-return $a + $b;\n+return $a * $b;\n";

fn build_changes_packet(root: &str, pm: &str, diff: &str) -> serde_json::Value {
    let _ = std::fs::remove_dir_all(root);
    must_with(std::fs::create_dir_all(format!("{root}/lib")), format!("mkdir {root}/lib"));
    must_with(
        std::fs::write(format!("{root}/lib/App.pm"), pm),
        "stage App.pm for diff fingerprint test",
    );
    let packet = must(build_ripr_facts_packet(&RiprFactsRequest {
        schema: "ripr-perl-facts-v1",
        root,
        base: Some("origin/main"),
        head: Some("HEAD"),
        fact_classes: "files,owners,changes",
        diff: Some(diff),
    }));
    let _ = std::fs::remove_dir_all(root);
    packet
}

#[test]
fn same_span_diff_content_keeps_fingerprint_stable() {
    let minus =
        build_changes_packet("target/ripr-d2/same-span-minus", APP_PM_FOR_DIFF, DIFF_RETURN_MINUS);
    let times =
        build_changes_packet("target/ripr-d2/same-span-times", APP_PM_FOR_DIFF, DIFF_RETURN_TIMES);
    let minus_fp = fingerprint(&minus, "same-span minus diff");
    let times_fp = fingerprint(&times, "same-span times diff");
    assert_eq!(
        minus["changes"][0]["change_id"], times["changes"][0]["change_id"],
        "fixture must keep the same change_id across content-only edits"
    );
    assert_ne!(
        minus["changes"][0]["changed_text_digest"], times["changes"][0]["changed_text_digest"],
        "content edit must still change changed_text_digest"
    );
    assert_eq!(
        minus_fp, times_fp,
        "identity-only fingerprint must ignore changed_text_digest when change_id is unchanged"
    );
}

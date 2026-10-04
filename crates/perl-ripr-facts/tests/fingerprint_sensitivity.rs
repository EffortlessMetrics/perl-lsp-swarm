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
//! Out of scope by recipe design (documented, not asserted): perturbations
//! confined to uncovered families (test facts, boundaries, verify commands,
//! limitations) or to uncovered fields (e.g. a diff-content change that keeps
//! the same change id) legitimately keep the fingerprint stable.

#![deny(clippy::map_err_ignore)] // Cohort C0 activation (#12598); see src/lib.rs.

use perl_ripr_facts::{RiprFactsRequest, build_ripr_facts_packet};
use perl_tdd_support::{must, must_some_with, must_with};

const ALL_FACT_CLASSES: &str = "files,owners,changes,tests,oracles,relations,dynamic_boundaries,verify_commands,limitations,provenance";

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
        fact_classes: ALL_FACT_CLASSES,
        diff: None,
    }));
    let _ = std::fs::remove_dir_all(root);
    packet
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
    }
    assert_eq!(
        detected, EXPECTED_DETECTED,
        "D2 bar: 100% of one-fact perturbations change packet_fingerprint \
         ({EXPECTED_DETECTED}/{EXPECTED_DETECTED}); got {detected}\n{report}"
    );
}

//! D3 input-order-invariance bench (#17154).
//!
//! The emitter neutralizes input order by construction (sorted file walks,
//! canonical fact-class order, span-derived ids), so the only order seams a
//! caller can perturb are the filesystem creation order and the
//! `fact_classes` request order. All three builds below must produce
//! byte-identical packets:
//!
//! 1. baseline build over the fixture root;
//! 2. rebuild over the untouched root (stability control);
//! 3. delete the root and recreate the same files in reverse-alphabetical
//!    creation order (filesystem-order perturbation);
//! 4. baseline files with the `fact_classes` list in reverse order
//!    (request-order perturbation — normalization must canonicalize it).
//!
//! Threshold (D3): byte-identical (`to_string_pretty` bytes equal, not just
//! `Value` equality, so key order and formatting are pinned too).
//!
//! Out of scope by design: diff-hunk order is caller-supplied semantic input
//! (`changes[]` follows diff order), so these builds request no diff.

#![deny(clippy::map_err_ignore)] // Cohort C0 activation (#12598); see src/lib.rs.

use perl_ripr_facts::{RiprFactsRequest, build_ripr_facts_packet};
use perl_tdd_support::{must, must_with};

const FACT_CLASSES: &str = "files,owners,changes,tests,oracles,relations,dynamic_boundaries,verify_commands,limitations,provenance";
const FACT_CLASSES_REVERSED: &str = "provenance,limitations,verify_commands,dynamic_boundaries,relations,oracles,tests,changes,owners,files";

const APPLE_PM: &str = "package Apple;\n\nsub core {\n    return 1;\n}\n\n1;\n";
const MANGO_PM: &str = "package Mango;\n\nsub pit {\n    return 2;\n}\n\n1;\n";
const ZEBRA_PM: &str = "package Zebra;\n\nsub stripe {\n    return 3;\n}\n\n1;\n";
const ALPHA_T: &str = "use strict;\nuse warnings;\nuse Test::More;\n\nuse Apple;\n\nok(Apple::core(), 'apple core');\n\ndone_testing;\n";
const BETA_T: &str = "use strict;\nuse warnings;\nuse Test::More;\n\nuse Zebra;\n\nok(Zebra::stripe(), 'zebra stripe');\n\ndone_testing;\n";

/// (relative path, content) in forward-alphabetical creation order.
const FIXTURE_FILES: &[(&str, &str)] = &[
    ("lib/Apple.pm", APPLE_PM),
    ("lib/Mango.pm", MANGO_PM),
    ("lib/Zebra.pm", ZEBRA_PM),
    ("t/alpha.t", ALPHA_T),
    ("t/beta.t", BETA_T),
];

fn stage_files(root: &str, files: &[(&str, &str)]) {
    let _ = std::fs::remove_dir_all(root);
    for (rel, content) in files {
        let path = format!("{root}/{rel}");
        if let Some(parent) = std::path::Path::new(&path).parent() {
            must_with(std::fs::create_dir_all(parent), format!("mkdir for {rel}"));
        }
        must_with(std::fs::write(&path, content), format!("stage D3 file {rel}"));
    }
}

fn build_packet_bytes(root: &str, fact_classes: &str) -> String {
    let packet = must(build_ripr_facts_packet(&RiprFactsRequest {
        schema: "ripr-perl-facts-v1",
        root,
        base: Some("origin/main"),
        head: Some("HEAD"),
        fact_classes,
        diff: None,
    }));
    must(serde_json::to_string_pretty(&packet))
}

#[test]
fn shuffled_input_order_yields_byte_identical_packets() {
    let root = "target/ripr-d3/order";
    stage_files(root, FIXTURE_FILES);
    let baseline = build_packet_bytes(root, FACT_CLASSES);

    // Stability control: rebuild over the untouched root.
    let rebuild = build_packet_bytes(root, FACT_CLASSES);
    assert_eq!(rebuild, baseline, "D3: rebuilding the untouched root must be byte-identical");

    // Filesystem-order perturbation: same files, reverse creation order.
    let mut reversed = FIXTURE_FILES.to_vec();
    reversed.reverse();
    stage_files(root, &reversed);
    let reordered = build_packet_bytes(root, FACT_CLASSES);
    let _ = std::fs::remove_dir_all(root);
    assert_eq!(
        reordered, baseline,
        "D3: reverse-alphabetical creation order must yield a byte-identical packet"
    );

    // Request-order perturbation: reversed fact_classes must canonicalize.
    stage_files(root, FIXTURE_FILES);
    let reordered_classes = build_packet_bytes(root, FACT_CLASSES_REVERSED);
    let _ = std::fs::remove_dir_all(root);
    assert_eq!(
        reordered_classes, baseline,
        "D3: reversed fact_classes order must yield a byte-identical packet"
    );
}

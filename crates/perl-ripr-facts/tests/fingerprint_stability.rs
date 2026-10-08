//! D1 fingerprint-stability proof (#17154).
//!
//! Twenty consecutive [`build_ripr_facts_packet`] calls over the same fixture
//! must produce byte-identical serialized packets, so the embedded
//! `packet_fingerprint` — and every other packet byte — is stable across runs.
//! Fixture layout mirrors `tests/packet_characterization.rs` (fixed
//! `target/`-relative root; [`RiprFactsRequest::root`] must be repo-relative).

#![deny(clippy::map_err_ignore)]

use perl_ripr_facts::{RiprFactsRequest, build_ripr_facts_packet};
use perl_tdd_support::{must, must_with};

const FACT_CLASSES: &str = "files,owners,changes,tests,oracles,relations,dynamic_boundaries,verify_commands,limitations,provenance";
const STABILITY_BUILDS: usize = 20;

const APP_PM: &str = "use strict;\nuse warnings;\n\npackage App;\n\nsub discount {\n    my ($amount) = @_;\n    if ($amount > 100) {\n        return $amount * 0.9;\n    }\n    return $amount;\n}\n\nsub risky {\n    eval { die \"boom\" };\n    return 1;\n}\n\n1;\n";

const APP_T: &str = "use strict;\nuse warnings;\nuse Test::More;\n\nuse App;\n\nis(App::discount(50), 50, 'no discount under threshold');\nok(App::risky(), 'risky returns true');\n\ndone_testing;\n";

#[test]
fn twenty_consecutive_builds_are_byte_identical() -> std::io::Result<()> {
    let root = "target/ripr-stability/twenty-builds";
    let _ = std::fs::remove_dir_all(root);
    must_with(std::fs::create_dir_all(format!("{root}/lib")), "create stability fixture lib dir");
    must_with(std::fs::create_dir_all(format!("{root}/t")), "create stability fixture t dir");
    must_with(
        std::fs::write(format!("{root}/lib/App.pm"), APP_PM),
        "write stability fixture App.pm",
    );
    must_with(std::fs::write(format!("{root}/t/app.t"), APP_T), "write stability fixture app.t");

    let request = RiprFactsRequest {
        schema: "ripr-perl-facts-v1",
        root,
        base: Some("origin/main"),
        head: Some("HEAD"),
        fact_classes: FACT_CLASSES,
        diff: None,
    };
    let first = must_with(
        serde_json::to_string(&must(build_ripr_facts_packet(&request))),
        "serialize stability build 0",
    );
    for build in 1..STABILITY_BUILDS {
        let packet = must(build_ripr_facts_packet(&request));
        let serialized =
            must_with(serde_json::to_string(&packet), format!("serialize stability build {build}"));
        assert_eq!(serialized, first, "stability build {build} differed from build 0");
    }

    let _ = std::fs::remove_dir_all(root);
    Ok(())
}

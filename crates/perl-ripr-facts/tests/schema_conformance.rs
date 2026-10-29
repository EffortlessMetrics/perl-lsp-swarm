//! C3 schema-conformance proof (#17154).
//!
//! Emitted packets (both without and with a caller-supplied diff) must
//! validate against the published `schemas/ripr-perl-facts-v1.schema.json`,
//! using the same `jsonschema::validator_for` helper shape as
//! `perl-core-harness-types`. The negative control below (a packet with a
//! required field removed) must NOT validate, proving the check is not vacuous.

use perl_ripr_facts::{RiprFactsRequest, build_ripr_facts_packet};
use perl_tdd_support::{must, must_some_with, must_with};

const FACT_CLASSES: &str = "files,owners,changes,tests,oracles,relations,dynamic_boundaries,verify_commands,limitations,provenance";

const APP_PM: &str = "use strict;\nuse warnings;\n\npackage App;\n\nsub discount {\n    my ($amount) = @_;\n    if ($amount > 100) {\n        return $amount * 0.9;\n    }\n    return $amount;\n}\n\nsub risky {\n    eval { die \"boom\" };\n    return 1;\n}\n\n1;\n";

const APP_T: &str = "use strict;\nuse warnings;\nuse Test::More;\n\nuse App;\n\nis(App::discount(50), 50, 'no discount under threshold');\nok(App::risky(), 'risky returns true');\n\ndone_testing;\n";

/// Opaque caller-supplied diff text, copied from the characterization fixture
/// (proven to assemble a well-shaped packet there); only the head-file line
/// numbers steer change attribution.
const CONFORMANCE_DIFF: &str = "+++ b/lib/App.pm\n@@ -1,1 +1,2 @@\n use strict;\n+use POSIX;\n@@ -8,1 +9,2 @@\n         return $amount * 0.9;\n+    if ($amount >= 500) {\n@@ -14,1 +15,3 @@\n     eval { die \"boom\" };\n+    eval { warn 'test' };\n+    die \"boom\" if $x < 0;\n+++ b/lib/Missing.pm\n@@ -1,1 +1,2 @@\n package Missing;\n+sub gone { 1 }\n";

fn schema_document() -> serde_json::Value {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../schemas/ripr-perl-facts-v1.schema.json");
    let text = must_with(
        std::fs::read_to_string(&path),
        format!("read published schema {}", path.display()),
    );
    must_with(serde_json::from_str(&text), "parse ripr-perl-facts-v1 schema")
}

fn build_packet(root: &str, diff: Option<&str>) -> serde_json::Value {
    must(build_ripr_facts_packet(&RiprFactsRequest {
        schema: "ripr-perl-facts-v1",
        root,
        base: Some("origin/main"),
        head: Some("HEAD"),
        fact_classes: FACT_CLASSES,
        diff,
    }))
}

#[test]
fn emitted_packets_conform_to_published_schema() -> std::io::Result<()> {
    let root = "target/ripr-conformance/packets";
    let _ = std::fs::remove_dir_all(root);
    must_with(std::fs::create_dir_all(format!("{root}/lib")), "create conformance fixture lib dir");
    must_with(std::fs::create_dir_all(format!("{root}/t")), "create conformance fixture t dir");
    must_with(
        std::fs::write(format!("{root}/lib/App.pm"), APP_PM),
        "write conformance fixture App.pm",
    );
    must_with(std::fs::write(format!("{root}/t/app.t"), APP_T), "write conformance fixture app.t");

    let validator = must_with(
        jsonschema::validator_for(&schema_document()),
        "compile ripr-perl-facts-v1 validator",
    );
    for (name, diff) in [("no-diff", None), ("with-diff", Some(CONFORMANCE_DIFF))] {
        let packet = build_packet(root, diff);
        must_with(
            validator.validate(&packet),
            format!("{name} packet must validate against ripr-perl-facts-v1"),
        );
    }

    // Negative control: a packet missing a required field must NOT validate.
    let mut invalid = build_packet(root, None);
    let object =
        must_some_with(invalid.as_object_mut(), "conformance packet must be a JSON object");
    let removed = object.remove("packet_id");
    assert!(removed.is_some(), "the negative control must actually remove `packet_id`");
    assert!(
        validator.validate(&invalid).is_err(),
        "a packet without required `packet_id` must not validate — \
         if it does, the conformance check is vacuous"
    );

    let _ = std::fs::remove_dir_all(root);
    Ok(())
}

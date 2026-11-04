//! Package-qualified calls resolve against every `package` in a file (ripr-swarm#10525).
//!
//! A module may declare an exception or helper package ahead of its own
//! (`Forms::Error` before `Forms`). The relation emitter used to key a `.pm`
//! on its first `package` statement only, so a test calling `Forms::prefix_only`
//! produced nothing but a `file_proximity` relation owned by `Forms::Error`, and
//! the consumer abstained on a test that discriminates the change.

#![deny(clippy::map_err_ignore)] // Cohort C0 activation (#12598); see src/lib.rs.

use perl_ripr_facts::{RiprFactsRequest, build_ripr_facts_packet};
use perl_tdd_support::must;

const RELATION_CLASSES: &str = "files,owners,tests,relations,limitations,provenance";

const SIBLING_FIRST_PM: &str = "package Forms::Error;\nuse strict;\nuse warnings;\nsub new { my ($class, %args) = @_; return bless {%args}, $class; }\nsub message { $_[0]{message} }\npackage Forms;\nuse strict;\nuse warnings;\nsub prefix_only {\n    my ($n) = @_;\n    die \"insufficient: need $n\\n\" if $n > 5;\n    return $n;\n}\n1;\n";

const QUALIFIED_CALL_T: &str = "use strict;\nuse warnings;\nuse Test::More;\nuse Test::Exception;\nuse Forms;\nthrows_ok { Forms::prefix_only(9) } qr/^insufficient/, 'x';\ndone_testing();\n";

const ERROR_ONLY_T: &str = "use strict;\nuse warnings;\nuse Test::More;\nuse Forms::Error;\nmy $e = Forms::Error->new(message => 'm');\nis($e->message, 'm', 'message');\ndone_testing();\n";

fn relations_for(root: &str, files: &[(&str, &str)]) -> Vec<serde_json::Value> {
    let _ = std::fs::remove_dir_all(root);
    must(std::fs::create_dir_all(format!("{root}/lib")));
    must(std::fs::create_dir_all(format!("{root}/t")));
    for (rel, body) in files {
        must(std::fs::write(format!("{root}/{rel}"), body));
    }
    let packet = must(build_ripr_facts_packet(&RiprFactsRequest {
        schema: "ripr-perl-facts-v1",
        root,
        base: None,
        head: None,
        fact_classes: RELATION_CLASSES,
        diff: None,
    }));
    let _ = std::fs::remove_dir_all(root);
    packet["relations"].as_array().cloned().unwrap_or_default()
}

fn kinds_for_owner(relations: &[serde_json::Value], owner_fragment: &str) -> Vec<String> {
    let mut kinds: Vec<String> = relations
        .iter()
        .filter(|r| r["owner_id"].as_str().is_some_and(|id| id.contains(owner_fragment)))
        .filter_map(|r| r["relation_kind"].as_str().map(str::to_string))
        .collect();
    kinds.sort();
    kinds
}

#[test]
fn qualified_call_to_a_later_package_is_a_direct_owner_call() {
    let relations = relations_for(
        "target/ripr-multi-package/qualified-call",
        &[("lib/Forms.pm", SIBLING_FIRST_PM), ("t/forms.t", QUALIFIED_CALL_T)],
    );
    assert_eq!(
        kinds_for_owner(&relations, "prefix_only"),
        vec!["direct_owner_call".to_string()],
        "relations: {relations:#?}"
    );
}

#[test]
fn a_sibling_package_gets_no_call_relation_from_a_call_to_another_package() {
    let relations = relations_for(
        "target/ripr-multi-package/sibling-not-linked",
        &[("lib/Forms.pm", SIBLING_FIRST_PM), ("t/forms.t", QUALIFIED_CALL_T)],
    );
    // The first package still takes the path-proximity heuristic (forms.t ~ Forms.pm);
    // that is advisory and never a call proof.
    assert!(
        !kinds_for_owner(&relations, "Forms::Error").contains(&"direct_owner_call".to_string()),
        "a call into Forms must not be a direct call on Forms::Error: {relations:#?}"
    );
}

#[test]
fn a_test_that_only_uses_the_first_package_does_not_link_the_later_one() {
    let relations = relations_for(
        "target/ripr-multi-package/first-package-only",
        &[("lib/Forms.pm", SIBLING_FIRST_PM), ("t/errors.t", ERROR_ONLY_T)],
    );
    assert!(
        kinds_for_owner(&relations, "prefix_only").is_empty(),
        "a test that never names Forms must not reach prefix_only: {relations:#?}"
    );
}

const BLOCK_AND_VERSION_PM: &str = "package First;\nuse strict;\nsub first_one { 1 }\npackage Second 1.0 {\n    sub target { 42 }\n}\npackage Third {\n    sub other { 7 }\n}\n1;\n";

const BLOCK_CALLS_T: &str = "use strict;\nuse warnings;\nuse Test::More;\nuse First;\nis(Second::target(), 42, 'second');\nis(Third::other(), 7, 'third');\ndone_testing();\n";

#[test]
fn block_and_versioned_packages_get_direct_owner_calls() {
    let relations = relations_for(
        "target/ripr-multi-package/block-version",
        &[("lib/First.pm", BLOCK_AND_VERSION_PM), ("t/calls.t", BLOCK_CALLS_T)],
    );
    for sub in ["target", "other"] {
        assert_eq!(
            kinds_for_owner(&relations, sub),
            vec!["direct_owner_call".to_string()],
            "{sub}: {relations:#?}"
        );
    }
}

#[test]
fn pod_package_lines_create_no_package_relations() {
    let pm = "package Real;\nsub run { 1 }\n\n=head1 NOTES\n\npackage Fake;\nsub ghost { 2 }\n\n=cut\n\n1;\n";
    let t =
        "use strict;\nuse Test::More;\nuse Real;\nis(Fake::ghost(), 2, 'x');\ndone_testing();\n";
    let relations = relations_for(
        "target/ripr-multi-package/pod-control",
        &[("lib/Real.pm", pm), ("t/real.t", t)],
    );
    assert!(
        !relations.iter().any(|r| r["owner_id"].as_str().is_some_and(|id| id.contains("Fake"))),
        "POD text must not declare a package: {relations:#?}"
    );
}

#[test]
fn version_declaration_followed_by_a_comment_still_links_the_package() {
    let pm = "package First;\nsub one { 1 }\npackage Second 1.0; # helper\nsub target { 42 }\n1;\n";
    let t = "use strict;\nuse Test::More;\nuse First;\nis(Second::target(), 42, 'x');\ndone_testing();\n";
    let relations = relations_for(
        "target/ripr-multi-package/version-comment",
        &[("lib/First.pm", pm), ("t/calls.t", t)],
    );
    assert_eq!(
        kinds_for_owner(&relations, "target"),
        vec!["direct_owner_call".to_string()],
        "{relations:#?}"
    );
}

#[test]
fn pod_cutover_directive_does_not_end_the_pod_block() {
    let pm = "package Real;\nsub run { 1 }\n\n=pod\n\n=cutover\n\npackage Decoy;\nsub ghost { 2 }\n\n=cut\n\n1;\n";
    let t =
        "use strict;\nuse Test::More;\nuse Real;\nis(Decoy::ghost(), 2, 'x');\ndone_testing();\n";
    let relations = relations_for(
        "target/ripr-multi-package/pod-cutover",
        &[("lib/Real.pm", pm), ("t/real.t", t)],
    );
    assert!(
        !relations.iter().any(|r| r["owner_id"].as_str().is_some_and(|id| id.contains("Decoy"))),
        "=cutover must not end POD: {relations:#?}"
    );
}

#[test]
fn heredoc_package_text_creates_no_package_relations() {
    let pm = "package Real;\nsub run { 1 }\nmy $doc = <<'EOT';\npackage Decoy;\nsub ghost { 2 }\nEOT\n1;\n";
    let t =
        "use strict;\nuse Test::More;\nuse Real;\nis(Decoy::ghost(), 2, 'x');\ndone_testing();\n";
    let relations = relations_for(
        "target/ripr-multi-package/heredoc-decoy",
        &[("lib/Real.pm", pm), ("t/real.t", t)],
    );
    assert!(
        !relations.iter().any(|r| r["owner_id"].as_str().is_some_and(|id| id.contains("Decoy"))),
        "heredoc text must not declare a package: {relations:#?}"
    );
}

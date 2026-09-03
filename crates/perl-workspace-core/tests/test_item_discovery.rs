#![deny(clippy::map_err_ignore)] // Cohort C0 activation (#12598): census-clean on all targets; new findings move the crate to C1.
//! Discriminating fixtures for canonical TestItem discovery.
//!
//! These tests exercise the producer against parser-backed subtest walking and
//! the generation/publication contract. They do not cut over code lenses, Test
//! Explorer, runner execution, or TAP.

#![deny(clippy::map_err_ignore)] // Cohort C0 activation (#12598): census-clean on all targets; new findings move the crate to C1.
#![expect(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "discovery fixtures panic on missing discriminator items"
)]

use perl_parser_core::{Node, Parser};
use perl_workspace_core::{
    CompatibilityMismatchKind, Confidence, Digest, FileRole, NamedSubroutinePolicy, ParseStatus,
    ParserBackedTree, SourceIdentityRef, SourceRange, TestFrameworkIdentity,
    TestItemDiscoveryRequest, TestItemId, TestItemKind, TestItemName, TestItemPublicationError,
    TestItemSnapshot, compare_with_parser_backed, discover_test_item_snapshot,
    parser_backed_subtests,
};

fn source_ref(seed: u8) -> SourceIdentityRef {
    SourceIdentityRef::from_sha256([seed; 32])
}

fn parse(source: &str) -> Node {
    Parser::new(source).parse_with_recovery().ast
}

fn discover(
    source: &str,
    generation: u64,
    source_ref: &SourceIdentityRef,
    policy: NamedSubroutinePolicy,
    framework: Option<&TestFrameworkIdentity>,
) -> TestItemSnapshot {
    let ast = parse(source);
    discover_test_item_snapshot(&TestItemDiscoveryRequest {
        source_ref,
        source,
        generation,
        ast: &ast,
        parse_status: ParseStatus::Clean,
        display_name: "t/example.t",
        file_role: FileRole::Test,
        framework,
        named_subroutine_policy: policy,
    })
    .expect("discovery should produce a validated snapshot")
}

fn names(snapshot: &TestItemSnapshot) -> Vec<TestItemName> {
    snapshot
        .items
        .iter()
        .filter(|item| item.kind != TestItemKind::File)
        .map(|item| item.name.clone())
        .collect()
}

#[test]
fn file_only_t_file_emits_one_file_item() {
    let source = "use Test::More;\nok(1);\ndone_testing;\n";
    let snapshot = discover(source, 1, &source_ref(1), NamedSubroutinePolicy::Off, None);
    snapshot.validate().expect("file-only snapshot must validate");
    assert_eq!(snapshot.items.len(), 1);
    assert_eq!(snapshot.items[0].kind, TestItemKind::File);
    assert_eq!(snapshot.items[0].name, TestItemName::Named("t/example.t".to_string()));
    assert!(snapshot.items[0].capabilities.runnable);
    assert!(snapshot.items[0].capabilities.debuggable);
    assert!(!snapshot.items[0].capabilities.selectively_runnable);
}

#[test]
fn nested_static_subtests_form_a_tree() {
    let source = "subtest 'outer' => sub {\n    subtest 'inner' => sub {\n        subtest 'deepest' => sub { ok(1); };\n    };\n};\n";
    let snapshot = discover(source, 3, &source_ref(1), NamedSubroutinePolicy::Off, None);
    let file = &snapshot.items.iter().find(|item| item.kind == TestItemKind::File).unwrap();
    let outer = snapshot.children_of(&file.id);
    assert_eq!(outer.len(), 1);
    assert_eq!(outer[0].name, TestItemName::Named("outer".to_string()));
    let inner = snapshot.children_of(&outer[0].id);
    assert_eq!(inner.len(), 1);
    assert_eq!(inner[0].name, TestItemName::Named("inner".to_string()));
    let deepest = snapshot.children_of(&inner[0].id);
    assert_eq!(deepest.len(), 1);
    assert_eq!(deepest[0].name, TestItemName::Named("deepest".to_string()));
}

#[test]
fn duplicate_sibling_names_remain_distinct() {
    let source = "subtest 'same' => sub { ok(1); };\nsubtest 'same' => sub { ok(2); };\n";
    let snapshot = discover(source, 1, &source_ref(1), NamedSubroutinePolicy::Off, None);
    let file = snapshot.items.iter().find(|item| item.kind == TestItemKind::File).unwrap();
    let children = snapshot.children_of(&file.id);
    assert_eq!(children.len(), 2);
    assert_eq!(children[0].name, children[1].name);
    assert_ne!(children[0].id, children[1].id);
}

#[test]
fn duplicate_names_under_different_parents_remain_distinct() {
    let source = "subtest 'left' => sub { subtest 'same' => sub { ok(1); }; };\nsubtest 'right' => sub { subtest 'same' => sub { ok(1); }; };\n";
    let snapshot = discover(source, 1, &source_ref(1), NamedSubroutinePolicy::Off, None);
    let file = snapshot.items.iter().find(|item| item.kind == TestItemKind::File).unwrap();
    let parents = snapshot.children_of(&file.id);
    let left = snapshot.children_of(&parents[0].id);
    let right = snapshot.children_of(&parents[1].id);
    assert_eq!(left[0].name, TestItemName::Named("same".to_string()));
    assert_eq!(right[0].name, TestItemName::Named("same".to_string()));
    assert_ne!(left[0].id, right[0].id);
}

#[test]
fn buffered_and_streamed_subtests_are_discovered() {
    let source =
        "subtest_buffered 'buf' => sub { ok(1); };\nsubtest_streamed 'str' => sub { ok(1); };\n";
    let snapshot = discover(source, 1, &source_ref(1), NamedSubroutinePolicy::Off, None);
    assert!(names(&snapshot).contains(&TestItemName::Named("buf".to_string())));
    assert!(names(&snapshot).contains(&TestItemName::Named("str".to_string())));
}

#[test]
fn test_more_compatible_subtest_is_discovered() {
    let source = "use Test::More;\nsubtest 'more' => sub { ok(1); };\ndone_testing;\n";
    let framework = TestFrameworkIdentity {
        family: "test_more".to_string(),
        module: Some("Test::More".to_string()),
        version: None,
    };
    let snapshot =
        discover(source, 1, &source_ref(1), NamedSubroutinePolicy::Off, Some(&framework));
    let subtest =
        snapshot.items.iter().find(|item| item.kind == TestItemKind::Subtest).expect("subtest");
    assert_eq!(subtest.framework.as_ref(), Some(&framework));
    assert!(!subtest.limitations.iter().any(|limit| limit == "parser_backed_compatibility"));
}

#[test]
fn call_name_alone_does_not_claim_framework_proof() {
    let source = "subtest 'bare' => sub { ok(1); };\n";
    let snapshot = discover(source, 1, &source_ref(1), NamedSubroutinePolicy::Off, None);
    let subtest =
        snapshot.items.iter().find(|item| item.kind == TestItemKind::Subtest).expect("subtest");
    assert!(subtest.framework.is_none());
    assert!(subtest.limitations.iter().any(|limit| limit == "parser_backed_compatibility"));
}

#[test]
fn interpolated_and_variable_names_are_dynamic() {
    let source = "subtest $name => sub { ok(1); };\nsubtest \"case $i\" => sub { ok(1); };\n";
    let snapshot = discover(source, 1, &source_ref(1), NamedSubroutinePolicy::Off, None);
    let dynamics: Vec<_> =
        snapshot.items.iter().filter(|item| item.kind == TestItemKind::Subtest).collect();
    assert_eq!(dynamics.len(), 2);
    assert!(dynamics.iter().all(|item| item.name == TestItemName::Dynamic));
    assert!(
        dynamics.iter().all(|item| item.limitations.iter().any(|limit| limit == "dynamic_name"))
    );
    assert!(dynamics.iter().all(|item| !item.capabilities.selectively_runnable));
}

#[test]
fn local_sub_named_subtest_is_not_a_subtest_item() {
    let source = "sub subtest { ok(1); }\nok(1);\n";
    let snapshot = discover(source, 1, &source_ref(1), NamedSubroutinePolicy::Off, None);
    assert!(snapshot.items.iter().all(|item| item.kind != TestItemKind::Subtest));
    let conservative =
        discover(source, 1, &source_ref(1), NamedSubroutinePolicy::ConservativeFileScope, None);
    assert!(conservative.items.iter().all(|item| {
        item.kind != TestItemKind::Subtest && item.kind != TestItemKind::NamedSubroutine
    }));
}

#[test]
fn conservative_named_subroutine_policy_keeps_ordinary_subs_out() {
    let source = "sub test_lookup { ok(1); }\nsub helper { 1 }\nsub widget_test { ok(1); }\n";
    let off = discover(source, 1, &source_ref(1), NamedSubroutinePolicy::Off, None);
    assert!(off.items.iter().all(|item| item.kind != TestItemKind::NamedSubroutine));

    let on =
        discover(source, 1, &source_ref(1), NamedSubroutinePolicy::ConservativeFileScope, None);
    let file = on.items.iter().find(|item| item.kind == TestItemKind::File).expect("file item");
    let named: Vec<_> = on
        .children_of(&file.id)
        .into_iter()
        .filter(|item| item.kind == TestItemKind::NamedSubroutine)
        .map(|item| item.name.clone())
        .collect();
    assert_eq!(
        named,
        vec![
            TestItemName::Named("test_lookup".to_string()),
            TestItemName::Named("widget_test".to_string())
        ]
    );
    assert!(!named.contains(&TestItemName::Named("helper".to_string())));
}

#[test]
fn conservative_named_subroutine_policy_rejects_vacuous_prefix_and_suffix() {
    let source = "sub test_ { ok(1); }\nsub _test { ok(1); }\nsub test_foo { ok(1); }\nsub foo_test { ok(1); }\n";
    let snapshot =
        discover(source, 1, &source_ref(1), NamedSubroutinePolicy::ConservativeFileScope, None);
    let file = snapshot.items.iter().find(|item| item.kind == TestItemKind::File).unwrap();
    let named: Vec<_> = snapshot
        .children_of(&file.id)
        .into_iter()
        .filter(|item| item.kind == TestItemKind::NamedSubroutine)
        .map(|item| item.name.clone())
        .collect();
    assert_eq!(
        named,
        vec![
            TestItemName::Named("test_foo".to_string()),
            TestItemName::Named("foo_test".to_string())
        ]
    );
}

#[test]
fn malformed_source_keeps_only_safe_items() {
    let source = "subtest 'kept' => sub { ok(1); };\nsubtest 'broken' => sub {\n";
    let ast = Parser::new(source).parse_with_recovery().ast;
    let snapshot = discover_test_item_snapshot(&TestItemDiscoveryRequest {
        source_ref: &source_ref(1),
        source,
        generation: 4,
        ast: &ast,
        parse_status: ParseStatus::Recovered,
        display_name: "t/broken.t",
        file_role: FileRole::Test,
        framework: None,
        named_subroutine_policy: NamedSubroutinePolicy::Off,
    })
    .expect("recovered discovery must still validate");
    snapshot.validate().expect("recovered snapshot must validate");
    assert_eq!(snapshot.items[0].confidence, Confidence::Medium);
    assert!(snapshot.items.iter().any(|item| {
        item.kind == TestItemKind::Subtest && item.name == TestItemName::Named("kept".to_string())
    }));
}

#[test]
fn comment_only_edit_preserves_item_ids() {
    let source_ref = source_ref(3);
    let original = "subtest 'outer' => sub {\n    subtest 'inner' => sub { ok(1); };\n};\n";
    let commented =
        "# note\nsubtest 'outer' => sub {\n    subtest 'inner' => sub { ok(1); };\n};\n";
    let old = discover(original, 1, &source_ref, NamedSubroutinePolicy::Off, None);
    let newer = discover(commented, 2, &source_ref, NamedSubroutinePolicy::Off, None);
    let old_ids: Vec<_> = old.items.iter().map(|item| item.id.clone()).collect();
    let new_ids: Vec<_> = newer.items.iter().map(|item| item.id.clone()).collect();
    assert_eq!(old_ids, new_ids);
    assert_ne!(old.source_digest, newer.source_digest);
    assert_ne!(old.fingerprint(), newer.fingerprint());
}

#[test]
fn rename_add_and_remove_change_snapshot_identity() {
    let source_ref = source_ref(4);
    let old = discover(
        "subtest 'alpha' => sub { ok(1); };\n",
        1,
        &source_ref,
        NamedSubroutinePolicy::Off,
        None,
    );
    let renamed = discover(
        "subtest 'gamma' => sub { ok(1); };\n",
        2,
        &source_ref,
        NamedSubroutinePolicy::Off,
        None,
    );
    let added = discover(
        "subtest 'alpha' => sub { ok(1); };\nsubtest 'gamma' => sub { ok(1); };\n",
        3,
        &source_ref,
        NamedSubroutinePolicy::Off,
        None,
    );
    let delta_rename = old.diff(&renamed).expect("rename diff");
    assert_eq!(delta_rename.changed.len(), 1);
    assert!(delta_rename.added.is_empty());
    assert!(delta_rename.removed.is_empty());
    let delta_add = old.diff(&added).expect("add diff");
    assert_eq!(delta_add.added.len(), 1);
}

#[test]
fn close_reopen_and_wrong_root_stay_distinct() {
    let first = discover(
        "subtest 'x' => sub { ok(1); };\n",
        1,
        &source_ref(1),
        NamedSubroutinePolicy::Off,
        None,
    );
    let reopened = discover(
        "subtest 'x' => sub { ok(1); };\n",
        2,
        &source_ref(1),
        NamedSubroutinePolicy::Off,
        None,
    );
    reopened.may_replace(&first).expect("newer generation of same source may publish");
    assert_eq!(first.items[0].id, reopened.items[0].id);

    let other_root = discover(
        "subtest 'x' => sub { ok(1); };\n",
        2,
        &source_ref(2),
        NamedSubroutinePolicy::Off,
        None,
    );
    assert_ne!(first.items[0].id, other_root.items[0].id);
    assert!(matches!(
        other_root.may_replace(&first),
        Err(TestItemPublicationError::DifferentSource)
    ));
}

#[test]
fn stale_and_content_equal_generations_cannot_replace() {
    let source = "subtest 'x' => sub { ok(1); };\n";
    let current = discover(source, 5, &source_ref(1), NamedSubroutinePolicy::Off, None);
    let stale = discover(source, 4, &source_ref(1), NamedSubroutinePolicy::Off, None);
    let same = discover(source, 5, &source_ref(1), NamedSubroutinePolicy::Off, None);
    assert!(matches!(
        stale.may_replace(&current),
        Err(TestItemPublicationError::StaleOrNonMonotonic { .. })
    ));
    assert!(matches!(
        same.may_replace(&current),
        Err(TestItemPublicationError::StaleOrNonMonotonic { .. })
    ));
    assert_eq!(current.source_digest, same.source_digest);
}

#[test]
fn lookup_and_nearest_use_the_snapshot_api() {
    let source = "ok(1);\nsubtest 'outer' => sub {\n    subtest 'inner' => sub { ok(2); };\n};\n";
    let snapshot = discover(source, 1, &source_ref(1), NamedSubroutinePolicy::Off, None);
    let inner = snapshot
        .items
        .iter()
        .find(|item| item.name == TestItemName::Named("inner".to_string()))
        .expect("inner");
    assert_eq!(snapshot.item(&inner.id).map(|item| &item.id), Some(&inner.id));
    let nearest = snapshot.nearest_at(inner.range.start_byte.saturating_add(1)).expect("nearest");
    assert_eq!(nearest.id, inner.id);
    let file = snapshot.nearest_at(0).expect("file fallback");
    assert_eq!(file.kind, TestItemKind::File);
}

#[test]
fn parser_backed_comparison_classifies_file_item_as_extra() {
    let source = "subtest 'outer' => sub {\n    subtest 'inner' => sub { ok(1); };\n};\n";
    let snapshot = discover(source, 1, &source_ref(1), NamedSubroutinePolicy::Off, None);
    let ast = parse(source);
    let mismatches = compare_with_parser_backed(
        &snapshot,
        &parser_backed_subtests(&ast, source, snapshot.generation),
    );
    assert!(mismatches.iter().all(|mismatch| {
        mismatch.kind != CompatibilityMismatchKind::MissingItem
            && mismatch.kind != CompatibilityMismatchKind::RangeMismatch
            && mismatch.kind != CompatibilityMismatchKind::NameStateMismatch
    }));
    assert_eq!(
        mismatches
            .iter()
            .filter(|mismatch| mismatch.kind == CompatibilityMismatchKind::ExtraItem)
            .map(|mismatch| mismatch.detail.as_str())
            .collect::<Vec<_>>(),
        vec!["file_item"]
    );
}

#[test]
fn qualified_test_more_subtest_is_discovered_local_package_is_not() {
    let source = "Test::More::subtest('qualified' => sub { ok(1); });\nLocal::subtest('nope' => sub { ok(1); });\n";
    let snapshot = discover(source, 1, &source_ref(1), NamedSubroutinePolicy::Off, None);
    let names = names(&snapshot);
    assert!(names.contains(&TestItemName::Named("qualified".to_string())));
    assert!(!names.contains(&TestItemName::Named("nope".to_string())));
    let ast = parse(source);
    let extras: Vec<_> = compare_with_parser_backed(
        &snapshot,
        &parser_backed_subtests(&ast, source, snapshot.generation),
    )
    .into_iter()
    .filter(|mismatch| mismatch.kind == CompatibilityMismatchKind::ExtraItem)
    .map(|mismatch| mismatch.detail)
    .collect();
    assert_eq!(extras, vec!["file_item".to_string(), "subtest".to_string()]);
}

#[test]
fn mixed_qualified_then_bare_pairs_by_range_not_sibling_index() {
    let source = "Test::More::subtest('a' => sub { ok(1); });\nsubtest 'b' => sub { ok(1); };\n";
    let snapshot = discover(source, 1, &source_ref(1), NamedSubroutinePolicy::Off, None);
    let ast = parse(source);
    let mismatches = compare_with_parser_backed(
        &snapshot,
        &parser_backed_subtests(&ast, source, snapshot.generation),
    );
    assert!(mismatches.iter().all(|mismatch| {
        mismatch.kind != CompatibilityMismatchKind::NameStateMismatch
            && mismatch.kind != CompatibilityMismatchKind::RangeMismatch
            && mismatch.kind != CompatibilityMismatchKind::MissingItem
    }));
    let extras: Vec<_> = mismatches
        .iter()
        .filter(|mismatch| mismatch.kind == CompatibilityMismatchKind::ExtraItem)
        .map(|mismatch| mismatch.detail.as_str())
        .collect();
    assert_eq!(extras, vec!["file_item", "subtest"]);
    let extra_id = mismatches
        .iter()
        .find(|mismatch| {
            mismatch.kind == CompatibilityMismatchKind::ExtraItem && mismatch.detail == "subtest"
        })
        .and_then(|mismatch| mismatch.snapshot_id.as_ref())
        .expect("qualified extra");
    let extra = snapshot.item(extra_id).expect("extra item");
    assert_eq!(extra.name, TestItemName::Named("a".to_string()));
}

#[test]
fn parser_backed_instrument_failure_short_circuits_comparison() {
    let source = "subtest 'bare' => sub { ok(1); };\n";
    let snapshot = discover(source, 7, &source_ref(1), NamedSubroutinePolicy::Off, None);
    let tree = ParserBackedTree {
        generation: snapshot.generation,
        source_digest: Digest::of(source),
        roots: Vec::new(),
        instrument_failure: Some("parser instrumentation failed".to_string()),
    };

    let mismatches = compare_with_parser_backed(&snapshot, &tree);
    assert_eq!(mismatches.len(), 1);
    assert_eq!(mismatches[0].kind, CompatibilityMismatchKind::InstrumentFailure);
    assert_eq!(mismatches[0].snapshot_id, None);
    assert_eq!(mismatches[0].detail, "parser instrumentation failed");
}

#[test]
fn parser_backed_generation_mismatch_short_circuits_comparison() {
    let source = "subtest 'bare' => sub { ok(1); };\n";
    let snapshot = discover(source, 7, &source_ref(1), NamedSubroutinePolicy::Off, None);
    let ast = parse(source);
    let tree = parser_backed_subtests(&ast, source, snapshot.generation + 1);

    let mismatches = compare_with_parser_backed(&snapshot, &tree);
    assert_eq!(mismatches.len(), 1);
    assert_eq!(mismatches[0].kind, CompatibilityMismatchKind::FreshnessMismatch);
    assert_eq!(mismatches[0].snapshot_id, None);
    assert!(mismatches[0].detail.contains("snapshot generation 7"));
    assert!(mismatches[0].detail.contains("parser-backed generation 8"));
}

#[test]
fn invalid_supplied_snapshot_short_circuits_comparison() {
    let source = "subtest 'bare' => sub { ok(1); };\n";
    let source_ref = source_ref(1);
    let snapshot = discover(source, 7, &source_ref, NamedSubroutinePolicy::Off, None);
    let ast = parse(source);
    let tree = parser_backed_subtests(&ast, source, snapshot.generation);
    let mut items = snapshot.items.clone();
    let file_index = items.iter().position(|item| item.kind == TestItemKind::File).expect("file");
    items[file_index].id = TestItemId::new(&source_ref, None, TestItemKind::File, "mutated");
    let invalid = TestItemSnapshot::new(
        source_ref,
        snapshot.source_digest.clone(),
        snapshot.generation,
        snapshot.source_len,
        items,
    );

    let mismatches = compare_with_parser_backed(&invalid, &tree);
    assert_eq!(mismatches.len(), 1);
    assert_eq!(mismatches[0].kind, CompatibilityMismatchKind::InstrumentFailure);
    assert_eq!(mismatches[0].snapshot_id, None);
    assert!(mismatches[0].detail.starts_with("supplied snapshot is invalid: "));
}

#[test]
fn exact_range_pairing_beats_start_byte_fallback() {
    let source = "subtest 'bare' => sub { ok(1); };\n";
    let snapshot = discover(source, 7, &source_ref(1), NamedSubroutinePolicy::Off, None);
    let ast = parse(source);
    let mut tree = parser_backed_subtests(&ast, source, snapshot.generation);
    let exact = tree.roots.pop().expect("bare subtest oracle root");
    let mut decoy = exact.clone();
    decoy.range = SourceRange { end_byte: exact.range.end_byte - 1, ..exact.range };
    tree.roots = vec![decoy, exact];

    let mismatches = compare_with_parser_backed(&snapshot, &tree);
    assert_eq!(
        mismatches
            .iter()
            .filter(|mismatch| mismatch.kind == CompatibilityMismatchKind::MissingItem)
            .count(),
        1
    );
    assert!(
        !mismatches
            .iter()
            .any(|mismatch| { mismatch.kind == CompatibilityMismatchKind::RangeMismatch })
    );
    assert_eq!(
        mismatches
            .iter()
            .filter(|mismatch| mismatch.kind == CompatibilityMismatchKind::ExtraItem)
            .map(|mismatch| mismatch.detail.as_str())
            .collect::<Vec<_>>(),
        vec!["file_item"]
    );
}

#[test]
fn unsupported_custom_wrapper_is_not_a_subtest() {
    let source = "my_subtest 'custom' => sub { ok(1); };\n";
    let snapshot = discover(source, 1, &source_ref(1), NamedSubroutinePolicy::Off, None);
    assert!(snapshot.items.iter().all(|item| item.kind != TestItemKind::Subtest));
}

#[test]
fn failed_parse_emits_file_item_only() {
    let source = "subtest 'x' => sub { ok(1); };\n";
    let ast = parse(source);
    let snapshot = discover_test_item_snapshot(&TestItemDiscoveryRequest {
        source_ref: &source_ref(1),
        source,
        generation: 1,
        ast: &ast,
        parse_status: ParseStatus::Failed,
        display_name: "t/example.t",
        file_role: FileRole::Test,
        framework: None,
        named_subroutine_policy: NamedSubroutinePolicy::Off,
    })
    .expect("failed parse still emits a file snapshot");
    assert_eq!(snapshot.items.len(), 1);
    assert_eq!(snapshot.items[0].kind, TestItemKind::File);
    assert!(snapshot.items[0].limitations.iter().any(|limit| limit == "parse_failed"));
}

#[test]
fn unrecognized_file_role_is_not_runnable_or_debuggable() {
    let source = "subtest 'x' => sub { ok(1); };\n";
    for role in [FileRole::Lib, FileRole::Script, FileRole::Unknown] {
        let ast = parse(source);
        let snapshot = discover_test_item_snapshot(&TestItemDiscoveryRequest {
            source_ref: &source_ref(1),
            source,
            generation: 1,
            ast: &ast,
            parse_status: ParseStatus::Clean,
            display_name: "lib/Example.pm",
            file_role: role,
            framework: None,
            named_subroutine_policy: NamedSubroutinePolicy::Off,
        })
        .expect("unrecognized-role discovery must still validate");
        let file =
            snapshot.items.iter().find(|item| item.kind == TestItemKind::File).expect("file");
        assert!(
            !file.capabilities.runnable && !file.capabilities.debuggable,
            "{role:?} must not advertise Run/Debug"
        );
        assert!(file.limitations.iter().any(|limit| limit == "unrecognized_test_file"));
        assert!(
            snapshot.items.iter().any(|item| {
                item.kind == TestItemKind::Subtest
                    && item.name == TestItemName::Named("x".to_string())
            }),
            "{role:?} still discovers structure"
        );
    }
}

#[test]
fn fingerprint_changes_when_framework_identity_changes() {
    let source = "subtest 'x' => sub { ok(1); };\n";
    let more = TestFrameworkIdentity {
        family: "test_more".to_string(),
        module: Some("Test::More".to_string()),
        version: None,
    };
    let test2 = TestFrameworkIdentity {
        family: "test2".to_string(),
        module: Some("Test2::V0".to_string()),
        version: None,
    };
    let first = discover(source, 1, &source_ref(1), NamedSubroutinePolicy::Off, Some(&more));
    let second = discover(source, 1, &source_ref(1), NamedSubroutinePolicy::Off, Some(&test2));
    let again = discover(source, 1, &source_ref(1), NamedSubroutinePolicy::Off, Some(&more));
    assert_ne!(first.fingerprint(), second.fingerprint());
    assert_eq!(first.fingerprint(), again.fingerprint());
}

#[test]
fn subtest_in_assignment_and_return_is_discovered() {
    let source = "my $passed = subtest 'assigned' => sub { ok(1); };\nreturn subtest 'returned' => sub { ok(1); };\n";
    let snapshot = discover(source, 1, &source_ref(1), NamedSubroutinePolicy::Off, None);
    let names = names(&snapshot);
    assert!(names.contains(&TestItemName::Named("assigned".to_string())));
    assert!(names.contains(&TestItemName::Named("returned".to_string())));
}

#[test]
fn unrelated_test2_prefix_packages_are_not_subtests() {
    let source = concat!(
        "Test2Fake::subtest('fake' => sub { ok(1); });\n",
        "Test20::subtest('twenty' => sub { ok(1); });\n",
        "Test2::Tools::Subtest::subtest('real' => sub { ok(1); });\n",
    );
    let snapshot = discover(source, 1, &source_ref(1), NamedSubroutinePolicy::Off, None);
    let names = names(&snapshot);
    assert!(!names.contains(&TestItemName::Named("fake".to_string())));
    assert!(!names.contains(&TestItemName::Named("twenty".to_string())));
    assert!(names.contains(&TestItemName::Named("real".to_string())));
}

#[test]
fn array_interpolation_in_double_quoted_name_is_dynamic() {
    let source =
        "subtest \"cases @names\" => sub { ok(1); };\nsubtest 'cases @names' => sub { ok(1); };\n";
    let snapshot = discover(source, 1, &source_ref(1), NamedSubroutinePolicy::Off, None);
    let subtests: Vec<_> =
        snapshot.items.iter().filter(|item| item.kind == TestItemKind::Subtest).collect();
    assert_eq!(subtests.len(), 2);
    assert_eq!(subtests[0].name, TestItemName::Dynamic);
    assert!(subtests[0].limitations.iter().any(|limit| limit == "dynamic_name"));
    assert_eq!(subtests[1].name, TestItemName::Named("cases @names".to_string()));
    assert!(!subtests[1].limitations.iter().any(|limit| limit == "dynamic_name"));
}

#[test]
fn empty_display_name_is_rejected() {
    let source = "ok(1);\n";
    let ast = parse(source);
    let error = discover_test_item_snapshot(&TestItemDiscoveryRequest {
        source_ref: &source_ref(1),
        source,
        generation: 1,
        ast: &ast,
        parse_status: ParseStatus::Clean,
        display_name: "",
        file_role: FileRole::Test,
        framework: None,
        named_subroutine_policy: NamedSubroutinePolicy::Off,
    });
    assert!(matches!(error, Err(perl_workspace_core::TestItemDiscoveryError::EmptyDisplayName)));
}

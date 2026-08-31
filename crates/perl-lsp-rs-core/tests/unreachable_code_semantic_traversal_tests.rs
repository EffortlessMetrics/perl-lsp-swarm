//! PL406 semantic traversal governance tests (issue #10844).
//!
//! These tests reconcile the PL406 disposition registry with the canonical
//! AST authorities and prove the summarizer's traversal and evaluation-order
//! semantics against realistic wrong candidates:
//!
//! - registry completeness against [`perl_ast::NodeKind::ALL_KIND_NAMES`]
//!   (a new variant fails until its disposition and fixture exist);
//! - declared child fields against #7298's canonical child traversal over
//!   fully populated fixture samples;
//! - leaf dispositions against #7015's structural classification;
//! - evaluation order, local analysis versus parent propagation, boundary
//!   isolation, short-circuit discipline, recovery fail-closed behavior,
//!   and the single-traversal-core guard.

use perl_ast::{AstNodeClassification, NodeKind, ast_node_policy, node_kind_fixtures};
use perl_lsp_rs_core::providers::diagnostics::Diagnostic;
use perl_lsp_rs_core::providers::diagnostics::unreachable_code::check_unreachable_code;
use perl_lsp_rs_core::providers::diagnostics::unreachable_code_disposition::{
    PL406_DISPOSITION_SCHEMA_VERSION, Pl406ProofCeiling, Pl406SemanticClass,
    all_pl406_dispositions, pl406_disposition_of,
};
use perl_parser::Parser;
use perl_test_must::must_some_with;

// ── helpers ──────────────────────────────────────────────────────────────────

fn diagnostics(source: &str) -> Result<Vec<Diagnostic>, Box<dyn std::error::Error>> {
    let mut parser = Parser::new(source);
    let ast = parser.parse()?;
    assert!(
        parser.errors().is_empty(),
        "test source must parse cleanly; a recovered Error node falls through and \
         would make presence assertions unreliable: {:?}",
        parser.errors()
    );
    let mut diagnostics = Vec::new();
    check_unreachable_code(&ast, &mut diagnostics);
    Ok(diagnostics)
}

fn count_pl406(diagnostics: &[Diagnostic]) -> usize {
    diagnostics.iter().filter(|diagnostic| diagnostic.code.as_deref() == Some("PL406")).count()
}

fn assert_pl406_count(
    source: &str,
    expected: usize,
    context: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let diagnostics = diagnostics(source)?;
    assert_eq!(count_pl406(&diagnostics), expected, "{context}: {diagnostics:?}");
    Ok(())
}

/// Collect the canonical child field names observed by #7298's traversal over
/// a fully populated sample of each variant.
fn canonically_observed_fields() -> std::collections::BTreeMap<String, Vec<String>> {
    let mut observed = std::collections::BTreeMap::new();
    for fixture in node_kind_fixtures() {
        let kind_name = fixture.sample.kind.kind_name().to_string();
        let mut fields: Vec<String> = Vec::new();
        let _ = fixture.sample.try_for_each_child_with_field_observed(
            |field, _| {
                if let Some(field) = field {
                    let name = field.name().to_string();
                    if !fields.contains(&name) {
                        fields.push(name);
                    }
                }
            },
            |_, _| std::ops::ControlFlow::<()>::Continue(()),
        );
        observed.insert(kind_name, fields);
    }
    observed
}

// ── registry reconciliation gates ─────────────────────────────────────────────

#[test]
fn schema_version_is_pinned() {
    assert_eq!(PL406_DISPOSITION_SCHEMA_VERSION, 1, "bump requires reviewing every consumer");
}

/// Negative control 1 and 10: every primary `NodeKind` has exactly one
/// disposition, in declaration order, with no extra rows.
#[test]
fn every_node_kind_has_exactly_one_in_order_disposition() {
    let rows = all_pl406_dispositions();
    let expected: Vec<&str> = NodeKind::ALL_KIND_NAMES.to_vec();
    let actual: Vec<&str> = rows.iter().map(|row| row.kind_name).collect();
    assert_eq!(
        actual, expected,
        "PL406 dispositions must enumerate ALL_KIND_NAMES exactly, in declaration order; \
         a new or renamed variant fails here until its semantic disposition is added"
    );
}

/// Every disposition resolves from its enum-derived token.
///
/// Exercises the public `pl406_disposition_of` accessor over fully populated
/// fixture samples, so a lookup that always returned `None` or mismatched
/// rows fails here rather than passing on a raw-vec scan alone.
#[test]
fn every_kind_resolves_its_disposition_from_the_enum_token() {
    for fixture in node_kind_fixtures() {
        let resolved = pl406_disposition_of(&fixture.sample.kind);
        let expected_name = fixture.sample.kind.kind_name();
        let row = must_some_with(
            resolved,
            format_args!("{expected_name} must resolve its PL406 disposition from the enum token"),
        );
        assert_eq!(
            row.kind_name, expected_name,
            "pl406_disposition_of resolved the wrong row for {expected_name}"
        );
    }
}

/// Negative control 9: declared executable child fields must be consistent
/// with the canonical child inventory (#7298): each declared field must be
/// observed by the canonical traversal over the fully populated fixture.
#[test]
fn declared_child_fields_match_canonical_traversal() -> Result<(), Box<dyn std::error::Error>> {
    let observed = canonically_observed_fields();
    for row in all_pl406_dispositions() {
        let observed_fields = observed
            .get(row.kind_name)
            .ok_or_else(|| format!("canonical fixture inventory must cover {}", row.kind_name))?;
        for field in row.executable_children.iter().chain(row.analyzed_not_propagated) {
            assert!(
                observed_fields.iter().any(|observed| observed == field),
                "{} declares PL406 child field {field:?} but the canonical #7298 traversal \
                 observes {observed_fields:?}; the registry claims coverage inconsistent \
                 with the structural authority",
                row.kind_name
            );
        }
    }
    Ok(())
}

/// Non-executable child fields that a traversing disposition intentionally
/// leaves undeclared, keyed by kind name. Every entry must stay observed by
/// the canonical #7298 traversal and remain undeclared in the registry row
/// (`pl406_field_exclusions_stay_observed_and_undeclared`), so the map cannot
/// silently grow to mask a missing executable-child declaration.
fn pl406_non_executable_field_exclusions()
-> std::collections::BTreeMap<&'static str, &'static [&'static str]> {
    let mut exclusions = std::collections::BTreeMap::new();
    // Signature and prototype variables are parameter bindings evaluated at
    // dispatch, not flow-relevant executable children of the body list.
    exclusions.insert("Method", &["signature"][..]);
    exclusions.insert("Subroutine", &["prototype", "signature"][..]);
    exclusions
}

/// Negative control 9 (completeness direction): every child field the
/// canonical #7298 traversal observes on a non-leaf disposition must be
/// declared as executable, analyzed-not-propagated, or explicitly excluded.
/// The forward gate alone cannot see a declaration being removed; this
/// direction fails when an observed executable child loses its declaration,
/// so registry rows can no longer silently shrink coverage.
#[test]
fn canonically_observed_children_are_declared_or_excluded() -> Result<(), Box<dyn std::error::Error>>
{
    let observed = canonically_observed_fields();
    let exclusions = pl406_non_executable_field_exclusions();
    let mut violations: Vec<String> = Vec::new();
    for row in all_pl406_dispositions() {
        if matches!(row.class, Pl406SemanticClass::Leaf) {
            continue;
        }
        let observed_fields = observed
            .get(row.kind_name)
            .ok_or_else(|| format!("canonical fixture inventory must cover {}", row.kind_name))?;
        let excluded = exclusions.get(row.kind_name).copied().unwrap_or(&[] as &[&str]);
        for field in observed_fields {
            let declared = row
                .executable_children
                .iter()
                .chain(row.analyzed_not_propagated)
                .any(|declared| declared == field);
            if !declared && !excluded.contains(&field.as_str()) {
                violations.push(format!(
                    "{} observes child field {field:?} that is neither declared nor excluded",
                    row.kind_name
                ));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "PL406 registry completeness violated:\n  {}",
        violations.join("\n  ")
    );
    Ok(())
}

/// Exclusion entries are honest: each one is still canonically observed for
/// its kind and still undeclared, so stale or masking entries fail here.
#[test]
fn pl406_field_exclusions_stay_observed_and_undeclared() -> Result<(), Box<dyn std::error::Error>> {
    let observed = canonically_observed_fields();
    for (kind_name, excluded) in pl406_non_executable_field_exclusions() {
        let observed_fields = observed
            .get(kind_name)
            .ok_or_else(|| format!("canonical fixture inventory must cover {kind_name}"))?;
        let row = all_pl406_dispositions()
            .iter()
            .find(|row| row.kind_name == kind_name)
            .ok_or_else(|| format!("PL406 registry must cover {kind_name}"))?;
        for field in excluded {
            assert!(
                observed_fields.iter().any(|observed| observed == field),
                "{kind_name} excludes {field:?} but the canonical #7298 traversal no \
                 longer observes it; drop the stale exclusion"
            );
            assert!(
                !row.executable_children.contains(field)
                    && !row.analyzed_not_propagated.contains(field),
                "{kind_name} excludes {field:?} but the registry declares it; the \
                 exclusion is a mask, not a disposition"
            );
        }
    }
    Ok(())
}

/// Negative control 2: a structurally child-bearing variant may only be
/// disposed as a PL406 leaf with an explicit reason.
#[test]
fn leaf_dispositions_of_child_bearing_variants_carry_explicit_reasons()
-> Result<(), Box<dyn std::error::Error>> {
    for row in all_pl406_dispositions() {
        let policy = ast_node_policy(row.kind_name)
            .ok_or_else(|| format!("#7015 policy must cover {}", row.kind_name))?;
        if row.class == Pl406SemanticClass::Leaf && policy.classification.permits_children() {
            assert!(
                row.leaf_reason.is_some(),
                "{} is structurally child-bearing ({:?}) but disposed as a PL406 leaf \
                 without an explicit leaf_reason",
                row.kind_name,
                policy.classification
            );
        }
    }
    Ok(())
}

/// Structural leaves stay leaves: a #7015 Leaf/SourceBoundary variant must
/// not claim executable children in the PL406 registry.
#[test]
fn structurally_leaf_variants_claim_no_executable_children()
-> Result<(), Box<dyn std::error::Error>> {
    for row in all_pl406_dispositions() {
        let policy = ast_node_policy(row.kind_name)
            .ok_or_else(|| format!("#7015 policy must cover {}", row.kind_name))?;
        if matches!(
            policy.classification,
            AstNodeClassification::Leaf | AstNodeClassification::SourceBoundary
        ) {
            assert!(
                row.executable_children.is_empty(),
                "{} is structurally a leaf/source-boundary but declares executable PL406 \
                 children {:?}",
                row.kind_name,
                row.executable_children
            );
        }
    }
    Ok(())
}

/// Registry internal consistency: determining children are executable, and a
/// child is never both propagation-relevant and explicitly not propagated.
#[test]
fn disposition_child_roles_are_consistent() {
    for row in all_pl406_dispositions() {
        for field in row.fallthrough_determining {
            assert!(
                row.executable_children.contains(field),
                "{} lists {field:?} as fallthrough-determining but not executable",
                row.kind_name
            );
        }
        for field in row.analyzed_not_propagated {
            assert!(
                !row.fallthrough_determining.contains(field),
                "{} lists {field:?} as both analyzed-not-propagated and fallthrough-determining",
                row.kind_name
            );
        }
        match row.class {
            Pl406SemanticClass::Leaf | Pl406SemanticClass::Recovery => {
                assert!(
                    row.fallthrough_determining.is_empty(),
                    "{} leaf/recovery rows cannot propagate child transfers",
                    row.kind_name
                );
            }
            _ => {}
        }
    }
}

/// Evaluation-boundary and callable-boundary rows preserve conservative parent
/// fallthrough; only exact-local-transfer rows may close a parent.
#[test]
fn boundary_rows_preserve_conservative_fallthrough() {
    for row in all_pl406_dispositions() {
        if matches!(
            row.class,
            Pl406SemanticClass::EvaluationBoundary | Pl406SemanticClass::CallableBoundary
        ) {
            assert_eq!(
                row.proof_ceiling,
                Pl406ProofCeiling::ConservativeFallthrough,
                "{} boundary rows must preserve parent fallthrough",
                row.kind_name
            );
            assert!(
                row.fallthrough_determining.is_empty(),
                "{} boundary rows cannot declare fallthrough-determining children",
                row.kind_name
            );
        }
    }
}

/// Unsupported rows would need an owner; today none may exist silently.
///
/// The registry deliberately disposes no variant as unsupported: every
/// current row is admitted with fixtures or carries an explicit leaf_reason.
/// Introducing an unsupported row later requires an owning successor issue
/// and preserved parent fallthrough (see the registry module docs), so its
/// arrival is a reviewed decision rather than silent drift.
#[test]
fn no_variant_is_disposed_without_admission_evidence() {
    for row in all_pl406_dispositions() {
        let admitted_with_children = !row.executable_children.is_empty();
        let admitted_as_leaf = row.class == Pl406SemanticClass::Leaf;
        if admitted_as_leaf {
            // Leaf rows either are structurally leaves (#7015) or carry an
            // explicit reason — checked in detail by the dedicated gate.
            assert!(
                row.leaf_reason.is_some()
                    || ast_node_policy(row.kind_name)
                        .is_some_and(|policy| !policy.classification.permits_children()),
                "leaf disposition {} needs structural backing or an explicit reason",
                row.kind_name
            );
        } else {
            assert!(
                admitted_with_children || row.class == Pl406SemanticClass::Recovery,
                "non-leaf disposition {} must declare executable children or be recovery",
                row.kind_name
            );
        }
    }
}

// ── single traversal core guard ───────────────────────────────────────────────

/// Negative control 8: PL406 keeps exactly one recursive traversal core and
/// never opens a second structural walker.
#[test]
fn lint_module_keeps_a_single_traversal_core() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src/providers/diagnostics/lints/unreachable_code.rs");
    let source = std::fs::read_to_string(path)?;

    for core in ["fn summarize_node(", "fn summarize_expression(", "fn summarize_statement_list("] {
        let definitions = source.matches(core).count();
        assert_eq!(
            definitions, 1,
            "PL406 must keep exactly one definition of {core}; found {definitions}"
        );
    }

    assert!(
        !source.contains(".children()"),
        "PL406 must not open a second structural walker via Node::children"
    );
    assert!(
        !source.contains("for_each_child"),
        "PL406 must not delegate to a second structural walker via for_each_child"
    );
    Ok(())
}

// ── assignment evaluation order ────────────────────────────────────────────────

/// The first transferring side in execution order (rhs before lhs) selects
/// the assignment summary and closes parent fallthrough.
#[test]
fn transferring_rhs_closes_assignment_parent_fallthrough() -> Result<(), Box<dyn std::error::Error>>
{
    assert_pl406_count(
        r#"sub f { my $x = die("stop"); print "dead"; }"#,
        1,
        "rhs die is evaluated first and must close the following sibling",
    )
}

/// Opposite-direction control: a clean rhs and clean lhs leave the parent
/// fallthrough open.
#[test]
fn clean_assignment_keeps_parent_fallthrough() -> Result<(), Box<dyn std::error::Error>> {
    assert_pl406_count(r#"sub f { my $x = compute(); print "alive"; }"#, 0, "clean assignment")
}

/// Both sides are visited even when the lhs provably transfers: nested
/// diagnostics in the rhs must survive (skipped-second-child falsifier).
#[test]
fn assignment_lhs_transfer_does_not_skip_rhs_diagnostics() -> Result<(), Box<dyn std::error::Error>>
{
    assert_pl406_count(
        r#"sub f { ($c ? exit : die) = do { die "x"; print "dead"; }; }"#,
        1,
        "rhs do-block must report its unreachable statement although the lhs transfers",
    )
}

// ── sequential expressions and call arguments ─────────────────────────────────

#[test]
fn comma_list_arguments_all_receive_local_analysis() -> Result<(), Box<dyn std::error::Error>> {
    assert_pl406_count(
        r#"sub f { work(do { die "x"; print "one"; }, do { die "y"; print "two"; }); }"#,
        2,
        "every argument expression keeps its own nested diagnostics",
    )
}

#[test]
fn method_call_object_and_args_are_both_visited() -> Result<(), Box<dyn std::error::Error>> {
    assert_pl406_count(
        r#"sub f { $obj->method(do { die "x"; print "dead"; }); print "alive"; }"#,
        1,
        "argument diagnostics survive without promoting method-call transfers",
    )?;
    assert_pl406_count(
        r#"sub f { $obj->{do { die "x"; print "dead"; }} = 1; }"#,
        1,
        "hash index diagnostics survive",
    )
}

// ── block-taking builtins (map/grep/sort) ────────────────────────────────────

/// Block arguments to map/grep/sort are semantically visited: unreachable
/// statements nested inside them are reported (the #10004 omission class).
#[test]
fn block_builtin_arguments_report_nested_dead_code() -> Result<(), Box<dyn std::error::Error>> {
    for (source, context) in [
        (r#"sub f { map { die "x"; print "dead"; } @list; }"#, "map"),
        (r#"sub f { grep { die "x"; print "dead"; } @list; }"#, "grep"),
        (r#"sub f { sort { die "x"; print "dead"; } @list; }"#, "sort"),
        (r#"sub f { my @out = map { die "x"; print "dead"; } @in; }"#, "map assigned"),
    ] {
        assert_pl406_count(source, 1, context)?;
    }
    Ok(())
}

/// Opposite-direction boundary control: a die inside a map block must NOT
/// close the fallthrough of the enclosing statement list.
#[test]
fn block_builtin_transfer_does_not_promote_to_parent() -> Result<(), Box<dyn std::error::Error>> {
    for (source, context) in [
        (r#"sub f { map { die "x"; } @list; print "reachable"; }"#, "map die"),
        (r#"sub f { grep { last; } @list; print "reachable"; }"#, "grep last"),
        (r#"sub f { sort { redo; } @list; print "reachable"; }"#, "sort redo"),
    ] {
        assert_pl406_count(source, 0, context)?;
    }
    Ok(())
}

/// Clean blocks keep sources quiet (positive-example opposite control).
#[test]
fn clean_block_builtin_arguments_stay_silent() -> Result<(), Box<dyn std::error::Error>> {
    assert_pl406_count(r#"sub f { my @o = map { $_ + 1 } @in; print "alive"; }"#, 0, "clean map")
}

// ── slices, dereference/index forms, literals, nested anonymous subs ─────────

#[test]
fn slice_and_tie_children_receive_local_analysis() -> Result<(), Box<dyn std::error::Error>> {
    assert_pl406_count(
        r#"sub f { @a[do { die "x"; print "dead"; }] = (1); }"#,
        1,
        "array slice index",
    )?;
    assert_pl406_count(
        r#"sub f { tie $h, "P", do { die "x"; print "dead"; }; }"#,
        1,
        "tie argument",
    )?;
    assert_pl406_count(
        r#"sub f { my %h = (key => do { die "x"; print "dead"; }); }"#,
        1,
        "hash literal value",
    )
}

#[test]
fn regex_expr_children_receive_local_analysis() -> Result<(), Box<dyn std::error::Error>> {
    assert_pl406_count(
        r#"sub f { if ($v =~ do { die "x"; print "dead"; }) { work(); } }"#,
        1,
        "match operand diagnostics survive",
    )
}

#[test]
fn nested_anonymous_sub_transfers_remain_local() -> Result<(), Box<dyn std::error::Error>> {
    assert_pl406_count(
        r#"sub f { my $mk = sub { return 1; my $dead = 2; }; print "outer alive"; }"#,
        1,
        "only the anonymous sub's inner sibling is unreachable",
    )
}

// ── alternatives: ternary ──────────────────────────────────────────────────────

#[test]
fn ternary_closes_parent_only_when_every_alternative_transfers()
-> Result<(), Box<dyn std::error::Error>> {
    assert_pl406_count(
        r#"sub f { $c ? exit : die("x"); print "dead"; }"#,
        1,
        "both ternary alternatives transfer",
    )?;
    assert_pl406_count(
        r#"sub f { $c ? exit : 0; print "alive"; }"#,
        0,
        "one fallthrough alternative keeps the parent open",
    )
}

// ── short-circuit discipline ──────────────────────────────────────────────────

#[test]
fn short_circuit_operands_never_promote_transfers() -> Result<(), Box<dyn std::error::Error>> {
    for (source, context) in [
        (r#"exec("p","-e","1") or die; my $x = 1;"#, "or"),
        (r#"exec("p","-e","1") || die; my $x = 1;"#, "||"),
        (r#"work() && exit; my $x = 1;"#, "&&"),
        (r#"work() and exit; my $x = 1;"#, "and"),
        (r#"work() // exit; my $x = 1;"#, "//"),
    ] {
        assert_pl406_count(source, 0, context)?;
    }
    Ok(())
}

// ── statement modifiers ───────────────────────────────────────────────────────

#[test]
fn modifier_gated_statement_keeps_the_skip_path() -> Result<(), Box<dyn std::error::Error>> {
    assert_pl406_count(r#"sub f { exit if $cond; my $x = 1; }"#, 0, "conditional exit")?;
    assert_pl406_count(
        r#"sub f { print "side" if do { die "x"; print "dead"; }; my $y = 2; }"#,
        1,
        "condition-side diagnostics survive and the skip path remains",
    )
}

// ── execution boundaries: eval/do/defer/try ───────────────────────────────────

#[test]
fn boundary_blocks_report_locally_without_poisoning_the_outer_unit()
-> Result<(), Box<dyn std::error::Error>> {
    for (source, context) in [
        (r#"sub f { eval { die "i"; print "d"; }; print "alive"; }"#, "eval"),
        (r#"sub f { my $x = do { die "i"; print "d"; }; print "alive"; }"#, "do"),
        (r#"use feature 'defer'; sub f { defer { die "i"; print "d"; } print "alive"; }"#, "defer"),
        (r#"sub f { try { die "i"; print "d"; } catch ($e) { } print "alive"; }"#, "try"),
    ] {
        assert_pl406_count(source, 1, context)?;
    }
    Ok(())
}

// ── loops, continue blocks, labels ─────────────────────────────────────────────

#[test]
fn loop_body_and_continue_analysis_stays_local() -> Result<(), Box<dyn std::error::Error>> {
    assert_pl406_count(
        r#"foreach my $x (do { die "i"; print "d"; }, 2) { work(); } print "alive";"#,
        1,
        "foreach list diagnostics survive without closing after-loop flow",
    )?;
    assert_pl406_count(
        r#"while (work()) { next; print "dead"; } print "after";"#,
        1,
        "unconditional next closes only inside the loop body",
    )
}

#[test]
fn goto_target_carried_by_a_fallthrough_branch_restores_reachability()
-> Result<(), Box<dyn std::error::Error>> {
    // The conditional goto keeps NEXT live even though `return` later closes
    // the sequential path (issue #10844: targets carried by fallthrough
    // branches must not be discarded).
    assert_pl406_count(
        r#"sub f { goto NEXT if $c; return; NEXT: print "alive"; }"#,
        0,
        "NEXT stays reachable via the earlier conditional goto",
    )?;
    // Intermediate statements between the transfer point and the target stay
    // unreachable: the goto restores only the labeled target itself.
    assert_pl406_count(
        r#"sub f { goto NEXT if $c; return; print "stranded"; NEXT: print "alive"; }"#,
        1,
        "only the stranded sibling between return and NEXT is unreachable",
    )?;
    // Opposite-direction control: without a goto, the labeled statement is
    // still flagged.
    assert_pl406_count(
        r#"sub f { return; NEXT: print "dead"; }"#,
        1,
        "labels alone never restore reachability",
    )
}

#[test]
fn computed_goto_targets_receive_local_analysis() -> Result<(), Box<dyn std::error::Error>> {
    // A computed goto target executes at runtime before the transfer: the
    // `do { ... }` block runs, so a dead statement inside it must receive
    // nested PL406 analysis instead of being skipped by the transfer summary.
    assert_pl406_count(
        r#"sub f { goto(do { return 1; print "dead"; }); }"#,
        1,
        "the dead statement inside the computed do-block target is reported",
    )?;
    // Plain-label targets are bare names with nothing to execute; they must
    // stay free of expression traversal (no diagnostics invented for labels).
    assert_pl406_count(
        r#"sub f { goto NEXT; NEXT: print "alive"; }"#,
        0,
        "label targets carry no executable children to analyze",
    )
}

// ── package-level units under their declared ceilings ─────────────────────────

#[test]
fn package_level_units_keep_conservative_fallthrough() -> Result<(), Box<dyn std::error::Error>> {
    for (source, context) in [
        (r#"package P { sub x { die "i"; print "d"; } } print "alive";"#, "package block"),
        (r#"BEGIN { die "i"; print "d"; } print "alive";"#, "phase block"),
    ] {
        assert_pl406_count(source, 1, context)?;
    }
    Ok(())
}

// ── terminal loop-entry and modifier conditions close the enclosing list ──────

#[test]
fn terminal_for_entry_gates_close_the_enclosing_list() -> Result<(), Box<dyn std::error::Error>> {
    // The C-style-for initializer runs exactly once before the first
    // iteration: a terminal transfer there means neither the loop body nor
    // anything after the loop executes.
    assert_pl406_count(
        r#"sub f { for (die "stop"; ; ) { work(); } print "dead"; }"#,
        1,
        "a terminal for initializer makes following siblings unreachable",
    )?;
    // The initial condition evaluation gates the first iteration; dying there
    // leaves post-loop statements unreachable.
    assert_pl406_count(
        r#"sub f { my $i = 0; for ($i = 1; die "stop"; ) { work(); } print "dead"; }"#,
        1,
        "a terminal initial condition makes following siblings unreachable",
    )?;
    // Ordinary entry conditions keep the loop's conservative fallthrough.
    assert_pl406_count(
        r#"sub f { for (my $i = 0; $i < 2; $i = $i + 1) { work($i); } print "alive"; }"#,
        0,
        "an ordinary for loop never closes its parent",
    )
}

#[test]
fn terminal_modifier_condition_skips_statement_and_sibling()
-> Result<(), Box<dyn std::error::Error>> {
    // The modifier condition evaluates first: dying there means the
    // controlled statement never runs and control exits before the sibling.
    assert_pl406_count(
        r#"sub f { print "side" if die "stop"; print "dead"; }"#,
        1,
        "a terminal modifier condition makes the next sibling unreachable",
    )?;
    // A fall-through condition keeps both the skip path and the sibling live.
    assert_pl406_count(
        r#"sub f { print "side" if $c; print "alive"; }"#,
        0,
        "an ordinary modifier condition preserves the skip path",
    )
}

#[test]
fn restoring_one_goto_label_keeps_other_targets_restorable()
-> Result<(), Box<dyn std::error::Error>> {
    // Consuming label A must not discard pending target B: B stays reachable
    // through its own conditional goto recorded on an earlier live statement.
    assert_pl406_count(
        r#"sub f { goto A if $a; goto B if $b; return; A: return; B: print "live"; }"#,
        0,
        "pending goto targets survive consumption of a different label",
    )?;
    // Without a second live target the stranded sibling stays flagged.
    assert_pl406_count(
        r#"sub f { goto A if $a; return; print "stranded"; A: print "live"; }"#,
        1,
        "only labels with a live goto restore reachability",
    )
}

// ── recovery fails closed ─────────────────────────────────────────────────────

/// Recovered/missing syntax must never fabricate exact non-fallthrough
/// (negative control 7): a recovered operand leaves following siblings live.
#[test]
fn recovered_syntax_preserves_fallthrough() -> Result<(), Box<dyn std::error::Error>> {
    let mut parser = Parser::new("my $x = ; print \"live\";");
    let ast = parser.parse()?;
    assert!(!parser.errors().is_empty(), "fixture must exercise the recovery path");
    let mut diags = Vec::new();
    check_unreachable_code(&ast, &mut diags);
    assert_eq!(
        count_pl406(&diags),
        0,
        "recovered syntax must not close parent fallthrough: {diags:?}"
    );
    Ok(())
}

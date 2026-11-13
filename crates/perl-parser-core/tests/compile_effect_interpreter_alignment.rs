//! Pins #2444 acceptance seams that are already on main:
//!
//! - HIR `CompileEnvironment` pragma state facts are projections of the shared
//!   `perl-pragma` source-ordered interpreter (`CompileTimePragmaEnvironment`).
//! - `require VERSION` records a module request without mutating lexical pragma state.

use perl_parser_core::Parser;
use perl_parser_core::hir::{CompileEnvironment, HirKind, lower_ast};
use perl_pragma::CompileTimePragmaEnvironment;
use perl_tdd_support::must_some_with;

fn lower(source: &str) -> (perl_parser_core::hir::HirFile, CompileTimePragmaEnvironment) {
    let mut parser = Parser::new(source);
    let output = parser.parse_with_recovery();
    let environment = CompileTimePragmaEnvironment::build(&output.ast);
    let file = lower_ast(&output.ast);
    (file, environment)
}

fn assert_snapshot_matches_fact(
    fact: &perl_parser_core::hir::PragmaStateFact,
    snapshot: &perl_pragma::PragmaSnapshot,
) {
    let state = snapshot.state();
    assert_eq!(fact.strict_vars, state.strict_vars, "strict_vars at {}", fact.range.start);
    assert_eq!(fact.strict_subs, state.strict_subs, "strict_subs at {}", fact.range.start);
    assert_eq!(fact.strict_refs, state.strict_refs, "strict_refs at {}", fact.range.start);
    assert_eq!(fact.warnings, state.warnings, "warnings at {}", fact.range.start);
    assert_eq!(
        fact.disabled_warning_categories, state.disabled_warning_categories,
        "disabled_warning_categories at {}",
        fact.range.start
    );
    assert_eq!(
        fact.features,
        state.features.iter().map(|name| (*name).to_string()).collect::<Vec<_>>(),
        "features at {}",
        fact.range.start
    );
}

fn assert_interpreter_entries_projected_to_hir(
    environment: &CompileEnvironment,
    interpreter: &CompileTimePragmaEnvironment,
) {
    let facts = environment.pragma_state_facts();
    let entries = interpreter.map().entries();
    assert_eq!(facts.len(), entries.len(), "HIR must record one fact per interpreter transition");

    for entry in entries {
        let fact = must_some_with(
            facts.iter().find(|fact| fact.range.start == entry.range.start),
            format!("missing HIR fact for interpreter transition at offset {}", entry.range.start),
        );
        assert_snapshot_matches_fact(fact, &entry.snapshot);
    }
}

fn assert_fact_matches_interpreter(
    environment: &CompileEnvironment,
    interpreter: &CompileTimePragmaEnvironment,
    offset: usize,
) {
    let fact = must_some_with(
        environment.pragma_state_at(offset),
        format!("expected HIR pragma state at offset {offset}"),
    );
    let snapshot = interpreter.snapshot_at(fact.range.start);
    assert_snapshot_matches_fact(fact, &snapshot);
}

#[test]
fn hir_pragma_state_facts_match_shared_interpreter_snapshots() {
    let source = "package Demo;\n\
         use strict;\n\
         use warnings;\n\
         use feature 'say';\n\
         use v5.36;\n\
         {\n\
             no strict 'refs';\n\
             no warnings 'once';\n\
         }\n\
         my $x = 1;\n";
    let (file, interpreter) = lower(source);
    let environment = &file.compile_environment;

    assert_interpreter_entries_projected_to_hir(environment, &interpreter);

    let marker = source.find("my $x").expect("marker");
    assert_fact_matches_interpreter(environment, &interpreter, marker);
}

#[test]
fn require_version_does_not_mutate_lexical_pragma_state_in_hir() {
    let source = "package Demo;\n\
         use strict;\n\
         require 5.036;\n\
         my $after = 1;\n";
    let (file, interpreter) = lower(source);
    let environment = &file.compile_environment;

    let after_offset = source.find("my $after").expect("after marker");
    let before_strict = environment.pragma_state_at(after_offset).expect("state after require");
    assert!(
        before_strict.strict_enabled(),
        "use strict must remain in effect after require VERSION"
    );

    assert!(
        !before_strict.has_feature("signatures"),
        "HIR must not gain version-bundle features from require VERSION"
    );

    let snapshot = interpreter.snapshot_at(after_offset);
    let interpreter_state = snapshot.state();
    assert!(
        interpreter_state.strict_vars
            && interpreter_state.strict_subs
            && interpreter_state.strict_refs,
        "interpreter must also keep strict after require VERSION"
    );
    assert!(
        !interpreter_state.has_feature("signatures"),
        "require VERSION must not apply version-bundle features lexically"
    );

    assert!(
        file.items.iter().any(|item| matches!(&item.kind, HirKind::RequireDecl(_))),
        "require VERSION must lower as a RequireDecl without applying lexical pragma deltas"
    );
}

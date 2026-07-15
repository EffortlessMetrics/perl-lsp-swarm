//! General compiler dispositions for runtime dereferences and lifecycle phases.

use perl_parser_core::Parser;
use perl_parser_core::hir::{
    CompileEffectSourceKind, CompileEnvironmentBoundaryKind, CompilePhase, lower_ast,
};

fn lower_source(source: &str) -> perl_parser_core::hir::HirFile {
    let mut parser = Parser::new(source);
    let output = parser.parse_with_recovery();
    lower_ast(&output.ast)
}

#[test]
fn variable_dereferences_are_ordinary_runtime_expressions() {
    // A variable operand may hold a hard reference at runtime. Its AST shape
    // therefore does not prove a symbolic reference, so it remains a typed
    // runtime expression and does not create a compile-time boundary.
    let file = lower_source(
        "sub inspect { my ($hash, $array, $row) = @_; keys %$hash; scalar @$array; my ($name) = @$_; }",
    );

    assert_eq!(
        file.items
            .iter()
            .filter(|item| matches!(&item.kind, perl_parser_core::hir::HirKind::DerefExpr(_)))
            .count(),
        3,
        "variable-operand dereferences should remain typed runtime expressions"
    );
    assert!(
        !file.compile_effects().iter().any(|effect| {
            effect.source_kind == CompileEffectSourceKind::SymbolicReferenceDeref
        })
    );
}

#[test]
fn literal_symbolic_reference_remains_a_dynamic_boundary() {
    let file = lower_source("no strict 'refs'; @{'Runtime::names'} = (); ");

    assert!(file.compile_environment.dynamic_boundaries.iter().any(|boundary| {
        boundary.kind == CompileEnvironmentBoundaryKind::SymbolicReferenceDeref
    }));
}

#[test]
fn constructed_symbolic_reference_remains_a_dynamic_boundary() {
    let file = lower_source("no strict 'refs'; my $name = 'names'; @{'Runtime::' . $name} = (); ");

    assert!(file.compile_environment.dynamic_boundaries.iter().any(|boundary| {
        boundary.kind == CompileEnvironmentBoundaryKind::SymbolicReferenceDeref
    }));
}

#[test]
fn init_and_end_are_deferred_lifecycle_phases() {
    let file = lower_source("INIT { initialize() } END { cleanup() }");

    assert!(
        file.compile_environment.phase_blocks.iter().any(|block| block.phase == CompilePhase::Init)
    );
    assert!(
        file.compile_environment.phase_blocks.iter().any(|block| block.phase == CompilePhase::End)
    );
    assert!(
        !file.compile_environment.dynamic_boundaries.iter().any(|boundary| {
            boundary.kind == CompileEnvironmentBoundaryKind::PhaseBlockExecution
        }),
        "deferred lifecycle phases must not be compile-execution boundaries"
    );
}

#[test]
fn pure_begin_data_flow_preserves_the_phase_fact_without_a_boundary() {
    let file = lower_source("BEGIN { $numtests = $^O eq 'VMS' ? 16 : 17; }");

    assert!(
        file.compile_environment
            .phase_blocks
            .iter()
            .any(|block| block.phase == CompilePhase::Begin)
    );
    assert!(
        !file.compile_environment.dynamic_boundaries.iter().any(|boundary| {
            boundary.kind == CompileEnvironmentBoundaryKind::PhaseBlockExecution
        }),
        "pure data-only BEGIN blocks must not require compile-time execution"
    );
}

#[test]
fn begin_calls_and_compile_environment_writes_remain_boundaries() {
    for source in [
        "BEGIN { chdir 't'; }",
        "BEGIN { @INC = ('../lib'); }",
        "BEGIN { $INC{'Module.pm'} = __FILE__; }",
        "BEGIN { $^H{'feature'} = 1; }",
        "BEGIN { @INC[0] = 'extra'; }",
        "BEGIN { local $^H = 1; }",
        "BEGIN { our @INC = ('../lib'); }",
    ] {
        let file = lower_source(source);

        assert!(
            file.compile_environment.dynamic_boundaries.iter().any(|boundary| {
                boundary.kind == CompileEnvironmentBoundaryKind::PhaseBlockExecution
            }),
            "BEGIN block {source:?} must remain a compile-execution boundary"
        );
    }
}

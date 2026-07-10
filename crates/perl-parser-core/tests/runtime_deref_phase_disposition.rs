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
fn variable_dereferences_do_not_emit_compile_boundaries() {
    let file = lower_source(
        "sub inspect { my ($hash, $array, $row) = @_; keys %$hash; scalar @$array; my ($name) = @$_; }",
    );

    assert!(
        !file.compile_effects().iter().any(|effect| {
            effect.source_kind == CompileEffectSourceKind::SymbolicReferenceDeref
        }),
        "ordinary runtime dereferences must not enter the compile-effect stream"
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
fn variable_symbolic_reference_is_a_dynamic_boundary() {
    // Block-dereference of a variable operand under no strict 'refs' is a symbolic reference
    // because the variable might contain a package name (e.g., @{$name} where $name='Foo::Bar').
    let file = lower_source("no strict 'refs'; my $name = 'Runtime::names'; @{$name} = (); ");

    assert!(
        file.compile_environment.dynamic_boundaries.iter().any(|boundary| {
            boundary.kind == CompileEnvironmentBoundaryKind::SymbolicReferenceDeref
        }),
        "block-dereference of a variable under no strict 'refs' must emit SymbolicReferenceDeref boundary"
    );
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

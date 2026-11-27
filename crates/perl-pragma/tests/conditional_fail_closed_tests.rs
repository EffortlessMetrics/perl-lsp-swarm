//! Parser-backed regression tests for fail-closed conditional pragma handling (#8438).

#![deny(clippy::map_err_ignore)]

use perl_pragma::{CompileTimePragmaEnvironment, PragmaSnapshot, PragmaState, PragmaTracker};

type TestResult<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

fn snapshot_for(source: &str) -> TestResult<PragmaSnapshot> {
    let ast = perl_parser_core::Parser::new(source).parse()?;
    Ok(CompileTimePragmaEnvironment::build(&ast).snapshot_at(source.len().saturating_sub(1)))
}

fn state_for(source: &str) -> TestResult<PragmaState> {
    Ok(snapshot_for(source)?.into())
}

fn assert_no_strict(state: &PragmaState, label: &str) -> TestResult {
    if state.strict_vars || state.strict_subs || state.strict_refs {
        return Err(format!("{label}: strict must stay disabled").into());
    }
    Ok(())
}

#[test]
fn use_if_literal_true_enables_strict_once() -> TestResult {
    let state = state_for("use if 1, 'strict';")?;
    assert!(state.strict_vars && state.strict_subs && state.strict_refs);
    Ok(())
}

#[test]
fn use_if_literal_false_leaves_strict_disabled() -> TestResult {
    assert_no_strict(&state_for("use if 0, 'strict';")?, "proven false")?;
    Ok(())
}

#[test]
fn dynamic_use_if_strict_does_not_enable_exact_strict() -> TestResult {
    assert_no_strict(&state_for("use if $dynamic, 'strict';")?, "dynamic strict")?;
    Ok(())
}

#[test]
fn platform_use_if_strict_does_not_enable_exact_strict() -> TestResult {
    assert_no_strict(&state_for("use if $^O eq 'MSWin32', 'strict';")?, "platform conditional")?;
    Ok(())
}

#[test]
fn use_if_literal_true_enables_feature_target() -> TestResult {
    let state = state_for("use if 1, 'feature', 'say';")?;
    assert!(state.has_feature("say"));
    Ok(())
}

#[test]
fn use_if_literal_false_leaves_feature_disabled() -> TestResult {
    let state = state_for("use if 0, 'feature', 'say';")?;
    assert!(!state.has_feature("say"));
    Ok(())
}

#[test]
fn use_unless_is_not_builtin_conditional_inverse() -> TestResult {
    assert_no_strict(&state_for("use unless $dynamic, 'strict';")?, "unless strict")?;
    Ok(())
}

#[test]
fn nested_block_restores_outer_state_after_proven_true_conditional() -> TestResult {
    let source = "use if 1, 'strict'; { use if 0, 'warnings'; } my $x;";
    let ast = perl_parser_core::Parser::new(source).parse()?;
    let map = PragmaTracker::build(&ast);
    let block_start = source.find('{').ok_or("fixture missing block")?;
    let inner = PragmaTracker::state_for_offset(&map, block_start + 5);
    let outer = PragmaTracker::state_for_offset(&map, source.len() - 1);
    assert!(inner.strict_vars, "inner scope keeps proven-true strict");
    assert!(!inner.warnings, "proven-false warnings must not apply in block");
    assert!(outer.strict_vars, "outer strict remains after block restore");
    assert!(!outer.warnings, "outer warnings remain off after restore");
    Ok(())
}

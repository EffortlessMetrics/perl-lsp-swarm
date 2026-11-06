//! Parser-backed regression tests for issue #8422.

#![deny(clippy::map_err_ignore)]

use std::error::Error;

use perl_pragma::{CompileTimePragmaEnvironment, PragmaTracker};

type TestResult<T = ()> = Result<T, Box<dyn Error>>;

fn environment_for(source: &str) -> TestResult<CompileTimePragmaEnvironment> {
    let ast = perl_parser_core::Parser::new(source).parse()?;
    Ok(CompileTimePragmaEnvironment::build(&ast))
}

fn state_at(source: &str, offset: usize) -> TestResult<perl_pragma::PragmaState> {
    let ast = perl_parser_core::Parser::new(source).parse()?;
    let map = PragmaTracker::build(&ast);
    Ok(PragmaTracker::state_for_offset(&map, offset))
}

#[test]
fn feature_signatures_leaves_strict_categories_off() -> TestResult {
    let source = "use feature 'signatures';\nmy $x = 1;\n";
    let env = environment_for(source)?;
    let snap = env.snapshot_at(source.find("my").unwrap_or(0));
    assert!(snap.has_feature("signatures"));
    assert!(!snap.strict_enabled());
    Ok(())
}

#[test]
fn experimental_signatures_leaves_strict_categories_off() -> TestResult {
    let source = "use experimental 'signatures';\nsub f ($x) { $x }\n";
    let env = environment_for(source)?;
    let snap = env.snapshot_at(source.find("sub").unwrap_or(0));
    assert!(snap.has_feature("signatures"));
    assert!(!snap.strict_enabled());
    Ok(())
}

#[test]
fn selective_strict_does_not_count_as_full_strict() -> TestResult {
    let source = "use strict 'refs';\nuse strict 'vars';\nuse strict 'subs';\n";
    let refs_only = "use strict 'refs';\n";
    let state = state_at(refs_only, refs_only.len().saturating_sub(1))?;
    assert!(state.strict_refs);
    assert!(!state.strict_vars);
    assert!(!state.strict_enabled());

    let full = state_at(source, source.len().saturating_sub(1))?;
    assert!(full.strict_enabled());
    Ok(())
}

#[test]
fn plain_strict_and_version_implied_strict_remain_full() -> TestResult {
    let plain = "use strict;\n";
    let plain_state = state_at(plain, plain.len().saturating_sub(1))?;
    assert!(plain_state.strict_enabled());

    let versioned = "use v5.36;\n";
    let version_state = state_at(versioned, versioned.len().saturating_sub(1))?;
    assert!(version_state.strict_enabled());
    Ok(())
}

#[test]
fn lexical_block_restores_outer_strict_without_signatures_leak() -> TestResult {
    let source = "use strict;\n{\n    use strict;\n}\n";
    let env = environment_for(source)?;
    let before = env.snapshot_at(source.find('{').unwrap_or(0).saturating_sub(1));
    let after = env.snapshot_at(source.rfind('}').unwrap_or(0) + 1);
    assert!(before.strict_enabled());
    assert!(after.strict_enabled());
    Ok(())
}

#[test]
fn partial_strict_then_no_strict_refs_stays_partial() -> TestResult {
    let source = "use strict;\nno strict 'refs';\n";
    let state = state_at(source, source.len().saturating_sub(1))?;
    assert!(!state.strict_refs);
    assert!(state.strict_vars);
    assert!(state.strict_subs);
    assert!(!state.strict_enabled());
    Ok(())
}

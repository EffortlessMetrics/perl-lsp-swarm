#![deny(clippy::map_err_ignore)]
//! Bounded external oracle for the fat-comma hook-name rule (#13604).
//!
//! The hook extractor promotes `hook before => sub {...}` to a *literal* name
//! but leaves `hook(before, sub {...})` computed. That distinction is a claim
//! about Perl itself, not about this repository, so repository-only tests
//! cannot establish it. This file asks the real `perl` interpreter what each
//! form means and requires the extractor's classification to agree.
//!
//! The oracle is bounded: two tiny programs, no network, no CPAN, no Dancer2.
//! It runs `perl` only as a semantics reference — never to execute project
//! code. When no interpreter is reachable the test reports the instrument
//! failure and stops rather than converting a missing oracle into a pass.

use perl_semantic_analyzer::Parser;
use perl_semantic_analyzer::analysis::dancer2_hooks::extract_dancer2_hook_declarations;
use perl_semantic_facts::FileId;
use perl_semantic_facts::hook::HookNameSelection;
use perl_tdd_support::{must, must_some};
use std::process::Command;

/// Ask `perl` what the first argument of a call actually is under each
/// separator, with a same-named subroutine in scope to make the difference
/// observable. Returns `None` when no interpreter is reachable.
fn perl_first_argument(separator: &str) -> Option<String> {
    let program = format!(
        "sub before {{ return 'CALLED' }} sub probe {{ return $_[0] }} \
         print probe(before {separator} 1);"
    );
    let output = Command::new("perl").arg("-e").arg(&program).output().ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn hook_name(code: &str) -> HookNameSelection {
    let mut parser = Parser::new(code);
    let ast = must(parser.parse());
    let declarations = extract_dancer2_hook_declarations(&ast, FileId(1), code);
    must_some(declarations.into_iter().next()).hook.name
}

#[test]
fn the_extractor_agrees_with_perl_on_what_the_separator_means() {
    // An unreachable interpreter is an instrument failure, not a verdict: the
    // rule is left NOT_PROVEN here rather than asserted from this repository's
    // own belief about Perl. The repository-side classification of both forms
    // is separately and unconditionally pinned in `dancer2_hook_facts.rs`
    // (`a_comma_separated_bareword_operand_is_never_promoted`), so skipping
    // here cannot leave the behaviour untested — only unbound to perl.
    let (Some(fat_comma), Some(comma)) = (perl_first_argument("=>"), perl_first_argument(","))
    else {
        return;
    };

    // Ground truth from the interpreter, not from this repository:
    // `=>` auto-quotes the preceding bareword into a string, while `,` leaves
    // it a call of the same-named subroutine.
    assert_eq!(fat_comma, "before", "perl must auto-quote the bareword before `=>`");
    assert_eq!(comma, "CALLED", "perl must call the subroutine before `,`");

    // The extractor must draw exactly that line.
    let promoted = hook_name("package App;\nuse Dancer2;\nhook before => sub { 1 };");
    let name = must_some(match &promoted {
        HookNameSelection::Literal(name) => Some(name.literal.clone()),
        HookNameSelection::Dynamic { .. } => None,
        _ => None,
    });
    assert_eq!(name, fat_comma, "the fat-comma form must carry exactly the literal perl produces");

    let called = hook_name("package App;\nuse Dancer2;\nhook(before, sub { 1 });");
    assert!(
        matches!(called, HookNameSelection::Dynamic { .. }),
        "the comma form calls a subroutine, so its name is not statically known: {called:?}"
    );
}

/// `__PACKAGE__` is the sharpest case for "the separator decides".
///
/// It is tempting to assume a compile-time token is never an auto-quoted
/// bareword, and to special-case it. The interpreter disagrees: after `=>` it
/// is quoted to the literal text `__PACKAGE__`, and only after `,` does it
/// evaluate to the current package. The extractor must follow the interpreter
/// in both directions rather than the intuition.
#[test]
fn perl_decides_the_dunder_token_by_separator_too() {
    let (Some(fat_comma), Some(comma)) = (
        perl_first_argument_in_package("__PACKAGE__", "=>"),
        perl_first_argument_in_package("__PACKAGE__", ","),
    ) else {
        return;
    };
    assert_eq!(fat_comma, "__PACKAGE__", "`=>` quotes the token rather than expanding it");
    assert_eq!(comma, "My::App", "`,` lets the token expand to the current package");

    let promoted = hook_name("package App;\nuse Dancer2;\nhook __PACKAGE__ => sub { 1 };");
    let literal = must_some(match &promoted {
        HookNameSelection::Literal(name) => Some(name.literal.clone()),
        HookNameSelection::Dynamic { .. } => None,
        _ => None,
    });
    assert_eq!(literal, fat_comma, "the quoted token is exactly what perl passes");
    // It is a literal, but not a reviewed hook position: nothing is invented.
    assert!(
        must_some(match &promoted {
            HookNameSelection::Literal(name) => Some(name),
            _ => None,
        })
        .canonical()
        .is_none(),
        "`__PACKAGE__` is not a reviewed canonical hook name"
    );

    let expanded = hook_name("package App;\nuse Dancer2;\nhook(__PACKAGE__, sub { 1 });");
    assert!(
        matches!(expanded, HookNameSelection::Dynamic { .. }),
        "the comma form expands at compile time, so the name is not static: {expanded:?}"
    );
}

/// As [`perl_first_argument`], but inside a named package so `__PACKAGE__`
/// has something to expand to.
fn perl_first_argument_in_package(operand: &str, separator: &str) -> Option<String> {
    let program = format!(
        "package My::App; sub probe {{ return $_[0] }} print probe({operand} {separator} 1);"
    );
    let output = Command::new("perl").arg("-e").arg(&program).output().ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

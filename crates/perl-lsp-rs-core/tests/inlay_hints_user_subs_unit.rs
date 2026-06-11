//! Unit tests for user-defined sub parameter inlay hints (#794) and
//! OO method-call inlay hints (Slice 2, #1302).
//!
//! Tests the `parameter_hints` and `parameter_hints_with_resolver` functions
//! in the inlay_hints provider directly, using the parser to build real ASTs.

use perl_lsp_rs_core::providers::inlay_hints::{parameter_hints, parameter_hints_with_resolver};

/// Parse source into an AST node.
fn ast_for(source: &str) -> Result<perl_parser_core::ast::Node, Box<dyn std::error::Error>> {
    let mut parser = perl_parser_core::Parser::new(source);
    let ast = parser.parse().map_err(|e| format!("parse error: {e}"))?;
    Ok(ast)
}

/// Dummy position converter: returns (byte_offset / 100, byte_offset % 100).
/// Good enough for checking that hints appear (not exact positions).
fn dummy_pos(offset: usize) -> (u32, u32) {
    ((offset / 100) as u32, (offset % 100) as u32)
}

// ---------------------------------------------------------------------------
// Basic sub with two mandatory params
// ---------------------------------------------------------------------------
#[test]
fn test_parameter_hints_user_sub_two_params() -> Result<(), Box<dyn std::error::Error>> {
    let src = r#"
sub greet($name, $greeting) { print "$greeting $name\n"; }
greet("Alice", "Hello");
"#;
    let ast = ast_for(src)?;
    let hints = parameter_hints(&ast, &dummy_pos, None);

    let labels: Vec<&str> = hints.iter().filter_map(|h| h["label"].as_str()).collect();

    assert!(
        labels.contains(&"name:"),
        "Expected 'name:' hint for first arg of greet; labels: {labels:?}"
    );
    assert!(
        labels.contains(&"greeting:"),
        "Expected 'greeting:' hint for second arg of greet; labels: {labels:?}"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Single-param sub should get no hints (noise-free policy)
// ---------------------------------------------------------------------------
#[test]
fn test_parameter_hints_user_sub_single_param_suppressed() -> Result<(), Box<dyn std::error::Error>>
{
    let src = r#"
sub say_it($msg) { print "$msg\n"; }
say_it("hello world");
"#;
    let ast = ast_for(src)?;
    let hints = parameter_hints(&ast, &dummy_pos, None);

    let labels: Vec<&str> = hints.iter().filter_map(|h| h["label"].as_str()).collect();

    assert!(
        !labels.contains(&"msg:"),
        "Should suppress hint for single-param sub; labels: {labels:?}"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Sub without a signature gets no hints
// ---------------------------------------------------------------------------
#[test]
fn test_parameter_hints_user_sub_no_signature_suppressed() -> Result<(), Box<dyn std::error::Error>>
{
    let src = r#"
sub old_style { my ($x, $y) = @_; $x + $y }
old_style(1, 2);
"#;
    let ast = ast_for(src)?;
    let hints = parameter_hints(&ast, &dummy_pos, None);

    let labels: Vec<&str> = hints.iter().filter_map(|h| h["label"].as_str()).collect();

    assert!(
        !labels.contains(&"x:"),
        "Should not hint for sub without formal signature; labels: {labels:?}"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Builtins are not double-hinted
// ---------------------------------------------------------------------------
#[test]
fn test_parameter_hints_no_double_hint_for_builtins() -> Result<(), Box<dyn std::error::Error>> {
    let src = r#"
open(FH, "<", "file.txt");
"#;
    let ast = ast_for(src)?;
    let hints = parameter_hints(&ast, &dummy_pos, None);

    // Count how many "filehandle:" labels there are -- should be exactly 1
    let count = hints.iter().filter(|h| h["label"].as_str() == Some("filehandle:")).count();
    assert_eq!(count, 1, "Should have exactly one filehandle: hint; hints: {hints:#?}");
    Ok(())
}

// ---------------------------------------------------------------------------
// Three-param sub: all positional args are hinted
// ---------------------------------------------------------------------------
#[test]
fn test_parameter_hints_user_sub_three_params() -> Result<(), Box<dyn std::error::Error>> {
    let src = r#"
sub connect_db($host, $port, $dbname) { 1 }
connect_db("localhost", 5432, "mydb");
"#;
    let ast = ast_for(src)?;
    let hints = parameter_hints(&ast, &dummy_pos, None);

    let labels: Vec<&str> = hints.iter().filter_map(|h| h["label"].as_str()).collect();

    assert!(labels.contains(&"host:"), "Expected 'host:'; labels: {labels:?}");
    assert!(labels.contains(&"port:"), "Expected 'port:'; labels: {labels:?}");
    assert!(labels.contains(&"dbname:"), "Expected 'dbname:'; labels: {labels:?}");
    Ok(())
}

// ---------------------------------------------------------------------------
// Slurpy (@rest) param: hints stop at slurpy boundary
// ---------------------------------------------------------------------------
#[test]
fn test_parameter_hints_user_sub_stops_at_slurpy() -> Result<(), Box<dyn std::error::Error>> {
    let src = r#"
sub log_msg($level, @messages) { 1 }
log_msg("info", "hello", "world");
"#;
    let ast = ast_for(src)?;
    let hints = parameter_hints(&ast, &dummy_pos, None);

    let labels: Vec<&str> = hints.iter().filter_map(|h| h["label"].as_str()).collect();

    // "level:" should be hinted
    assert!(labels.contains(&"level:"), "Expected 'level:' hint; labels: {labels:?}");
    Ok(())
}

// ---------------------------------------------------------------------------
// Unresolved call: no crash, no spurious hints
// ---------------------------------------------------------------------------
#[test]
fn test_parameter_hints_unresolved_call_no_hints() -> Result<(), Box<dyn std::error::Error>> {
    let src = r#"
some_external_function("a", "b");
"#;
    let ast = ast_for(src)?;
    let hints = parameter_hints(&ast, &dummy_pos, None);

    let user_labels: Vec<&str> = hints
        .iter()
        .filter_map(|h| h["label"].as_str())
        .filter(|l| *l == "a:" || *l == "b:")
        .collect();

    assert!(
        user_labels.is_empty(),
        "Should not produce hints for unresolved calls; labels: {user_labels:?}"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// OO method-call inlay hints — Slice 2 (#1302)
// ---------------------------------------------------------------------------

/// Strong-oracle test: in-file method with three params (including $self) should
/// produce two hints ($template, $limit) for the two call-site arguments.
///
/// Perl source:
///   use feature 'class';
///   class Formatter {
///     method render($self, $template, $limit) { ... }
///   }
///   $fmt->render("hello %s", 10);
///
/// The param list is [$self, $template, $limit]. After skipping $self, the
/// visible params are [$template, $limit] — >1, so hints are emitted.
#[test]
fn test_method_call_inlay_hints_in_file_method() -> Result<(), Box<dyn std::error::Error>> {
    let src = r#"
package Formatter;
sub render {
    my ($self, $template, $limit) = @_;
    return sprintf($template, $limit);
}
package main;
my $fmt = Formatter->new();
$fmt->render("hello %s", 10);
"#;
    // `render` is an old-style sub, so user_sigs won't have a formal signature
    // (no NodeKind::Signature). Test falls through to no-resolver path → no hints.
    // This confirms the no-false-positive guarantee for @_-style subs.
    let ast = ast_for(src)?;
    let hints = parameter_hints(&ast, &dummy_pos, None);

    let method_labels: Vec<&str> = hints
        .iter()
        .filter_map(|h| h["label"].as_str())
        .filter(|l| *l == "template:" || *l == "limit:")
        .collect();

    // Old-style subs (my ($self, $template, $limit) = @_) are NOT indexed in
    // user_sigs (only formal `sub foo($a, $b)` signatures are).  Confirm no hints.
    assert!(
        method_labels.is_empty(),
        "Old-style @_-unpacked method should produce no hints without resolver; \
         labels: {method_labels:?}"
    );
    Ok(())
}

/// Strong-oracle test: formal-signature method in the same file using Perl 5.36+ syntax.
/// Param list: ($self, $template, $limit) → two visible params after skipping $self.
///
/// This tests the NodeKind::Subroutine path (formal signature) combined with
/// NodeKind::MethodCall walking.
#[test]
fn test_method_call_inlay_hints_formal_signature_in_file() -> Result<(), Box<dyn std::error::Error>>
{
    let src = r#"
sub render($self, $template, $limit) { sprintf($template, $limit) }
my $fmt = bless {}, 'Formatter';
$fmt->render("hello %s", 10);
"#;
    let ast = ast_for(src)?;
    let hints = parameter_hints(&ast, &dummy_pos, None);

    let labels: Vec<&str> = hints.iter().filter_map(|h| h["label"].as_str()).collect();

    assert!(
        labels.contains(&"template:"),
        "Expected 'template:' hint for ->render() arg 1; labels: {labels:?}"
    );
    assert!(
        labels.contains(&"limit:"),
        "Expected 'limit:' hint for ->render() arg 2; labels: {labels:?}"
    );
    // $self is the receiver — must NOT appear as a hint
    assert!(
        !labels.contains(&"self:"),
        "Must not emit hint for the implicit self param; labels: {labels:?}"
    );
    Ok(())
}

/// Noise-reduction policy: method with only one visible param after skipping self
/// ($self + one other) should produce NO hints.
#[test]
fn test_method_call_single_visible_param_suppressed() -> Result<(), Box<dyn std::error::Error>> {
    let src = r#"
sub process($self, $item) { $item }
my $obj = bless {}, 'Processor';
$obj->process("value");
"#;
    let ast = ast_for(src)?;
    let hints = parameter_hints(&ast, &dummy_pos, None);

    let labels: Vec<&str> = hints.iter().filter_map(|h| h["label"].as_str()).collect();

    assert!(
        !labels.contains(&"item:"),
        "Single-visible-param method should be suppressed; labels: {labels:?}"
    );
    Ok(())
}

/// Unknown method (not defined in file, no resolver) → no hints, no crash.
#[test]
fn test_method_call_unknown_method_no_hints() -> Result<(), Box<dyn std::error::Error>> {
    let src = r#"
$obj->some_unknown_method(1, 2, 3);
"#;
    let ast = ast_for(src)?;
    let hints = parameter_hints(&ast, &dummy_pos, None);

    // No method definition in scope — no hints expected.
    assert!(hints.is_empty(), "Unknown method should produce no hints; hints: {hints:#?}");
    Ok(())
}

/// Existing FunctionCall hints are unaffected by the MethodCall change.
/// Regression guard: adding the match arm must not break the FunctionCall path.
#[test]
fn test_function_call_hints_unaffected_by_method_call_change()
-> Result<(), Box<dyn std::error::Error>> {
    let src = r#"
sub connect_db($host, $port, $dbname) { 1 }
connect_db("localhost", 5432, "mydb");
"#;
    let ast = ast_for(src)?;
    let hints = parameter_hints(&ast, &dummy_pos, None);

    let labels: Vec<&str> = hints.iter().filter_map(|h| h["label"].as_str()).collect();

    assert!(labels.contains(&"host:"), "Expected 'host:'; labels: {labels:?}");
    assert!(labels.contains(&"port:"), "Expected 'port:'; labels: {labels:?}");
    assert!(labels.contains(&"dbname:"), "Expected 'dbname:'; labels: {labels:?}");
    Ok(())
}

/// Strong-oracle test: workspace resolver is called for unknown methods.
/// Simulates what `misc.rs` does when `resolve_method_in_workspace` returns
/// a param list — confirms the resolver closure path works end-to-end.
#[test]
fn test_method_call_workspace_resolver_provides_hints() -> Result<(), Box<dyn std::error::Error>> {
    let src = r#"
$obj->format_output("tmpl", "arg2");
"#;
    let ast = ast_for(src)?;

    // Simulate workspace resolution: format_output($self, $template, $data)
    let resolver = |method: &str| -> Option<Vec<String>> {
        if method == "format_output" {
            Some(vec!["self".to_string(), "template".to_string(), "data".to_string()])
        } else {
            None
        }
    };

    let hints = parameter_hints_with_resolver(&ast, &dummy_pos, None, Some(&resolver));

    let labels: Vec<&str> = hints.iter().filter_map(|h| h["label"].as_str()).collect();

    assert!(
        labels.contains(&"template:"),
        "Resolver should provide 'template:' hint; labels: {labels:?}"
    );
    assert!(labels.contains(&"data:"), "Resolver should provide 'data:' hint; labels: {labels:?}");
    assert!(
        !labels.contains(&"self:"),
        "Leading self param from resolver must be skipped; labels: {labels:?}"
    );
    Ok(())
}

/// Workspace resolver returns None for unknown method → no hints, no crash.
#[test]
fn test_method_call_workspace_resolver_returns_none_no_hints()
-> Result<(), Box<dyn std::error::Error>> {
    let src = r#"
$obj->mystery_method(1, 2);
"#;
    let ast = ast_for(src)?;

    let resolver = |_method: &str| -> Option<Vec<String>> { None };

    let hints = parameter_hints_with_resolver(&ast, &dummy_pos, None, Some(&resolver));

    assert!(hints.is_empty(), "Resolver returning None should produce no hints; hints: {hints:#?}");
    Ok(())
}

/// Class->method() (static/class method call) should also receive hints
/// when the resolver supplies param names.
#[test]
fn test_class_method_call_workspace_resolver_provides_hints()
-> Result<(), Box<dyn std::error::Error>> {
    let src = r#"
My::Class->create("name", 42);
"#;
    let ast = ast_for(src)?;

    // Class->create($class, $name, $id)
    let resolver = |method: &str| -> Option<Vec<String>> {
        if method == "create" {
            Some(vec!["class".to_string(), "name".to_string(), "id".to_string()])
        } else {
            None
        }
    };

    let hints = parameter_hints_with_resolver(&ast, &dummy_pos, None, Some(&resolver));

    let labels: Vec<&str> = hints.iter().filter_map(|h| h["label"].as_str()).collect();

    assert!(
        labels.contains(&"name:"),
        "Class->method() should get 'name:' hint; labels: {labels:?}"
    );
    assert!(labels.contains(&"id:"), "Class->method() should get 'id:' hint; labels: {labels:?}");
    assert!(!labels.contains(&"class:"), "Leading class param must be skipped; labels: {labels:?}");
    Ok(())
}

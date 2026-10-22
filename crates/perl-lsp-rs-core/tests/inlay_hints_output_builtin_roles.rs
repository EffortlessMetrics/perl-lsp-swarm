//! Regression tests for conservative output-builtin parameter roles (#18132).

use perl_lsp_rs_core::providers::inlay_hints::{inlay_hint_label_str, parameter_hints};
use perl_position_tracking::{WirePosition as Position, WireRange as Range};
use serde_json::Value;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn ast_for(source: &str) -> Result<perl_parser_core::ast::Node, Box<dyn std::error::Error>> {
    let mut parser = perl_parser_core::Parser::new(source);
    parser.parse().map_err(|error| format!("parse error: {error}").into())
}

fn offset_position(offset: usize) -> (u32, u32) {
    (0, offset as u32)
}

fn labels(hints: &[Value]) -> Vec<&str> {
    hints.iter().filter_map(inlay_hint_label_str).collect()
}

fn positions_for_label(hints: &[Value], wanted: &str) -> Vec<(u64, u64)> {
    hints
        .iter()
        .filter(|hint| inlay_hint_label_str(hint) == Some(wanted))
        .filter_map(|hint| {
            Some((
                hint.get("position")?.get("line")?.as_u64()?,
                hint.get("position")?.get("character")?.as_u64()?,
            ))
        })
        .collect()
}

#[test]
fn ordinary_output_calls_do_not_guess_filehandle_roles() -> TestResult {
    let source = r#"print "x"; print $value; print($fh, "x");"#;
    let ast = ast_for(source)?;
    let hints = parameter_hints(&ast, &offset_position, None);
    let actual = labels(&hints);

    assert!(
        !actual.contains(&"filehandle:"),
        "ordinary or flattened output calls must not guess filehandle roles: {hints:#?}"
    );
    Ok(())
}

#[test]
fn parser_preserved_output_handles_receive_exact_roles() -> TestResult {
    let source =
        r#"print STDERR "x"; print $fh "x"; print { $fh } "x"; printf $out "%s", $value;"#;
    let ast = ast_for(source)?;
    let hints = parameter_hints(&ast, &offset_position, None);
    let actual = labels(&hints);

    assert_eq!(
        actual.iter().filter(|label| **label == "filehandle:").count(),
        4,
        "every parser-preserved explicit handle should receive one exact role: {hints:#?}"
    );
    assert_eq!(
        actual.iter().filter(|label| **label == "format:").count(),
        1,
        "the explicit-handle printf format operand should be labeled exactly: {hints:#?}"
    );

    for hint in hints.iter().filter(|hint| {
        matches!(inlay_hint_label_str(hint), Some("filehandle:") | Some("format:"))
    }) {
        assert!(hint.get("label").is_some_and(Value::is_array));
        assert_eq!(
            hint.get("data")
                .and_then(|data| data.get("compatibilityOwner"))
                .and_then(Value::as_str),
            Some("EffortlessMetrics/perl-lsp-swarm#18132")
        );
    }
    Ok(())
}

#[test]
fn ordinary_printf_string_literal_uses_format_not_filehandle() -> TestResult {
    let source = r#"printf "%s", $value;"#;
    let ast = ast_for(source)?;
    let hints = parameter_hints(&ast, &offset_position, None);
    let actual = labels(&hints);

    assert_eq!(
        actual,
        vec!["format:"],
        "ordinary printf should expose only its proven format role"
    );
    assert_eq!(
        positions_for_label(&hints, "format:"),
        vec![(0, source.find("\"%s\"").ok_or("format literal")? as u64)]
    );
    Ok(())
}

#[test]
fn same_named_method_keeps_user_parameters_without_core_roles() -> TestResult {
    let source = r#"
sub print($self, $left, $right) { 1 }
print($a, $b);
$obj->print($a, $b);
"#;
    let ast = ast_for(source)?;
    let hints = parameter_hints(&ast, &offset_position, None);
    let actual = labels(&hints);

    assert!(!actual.contains(&"filehandle:"), "same-named user calls must not inherit CORE roles");
    assert!(actual.contains(&"left:"), "method parameter hints should remain intact: {hints:#?}");
    assert!(actual.contains(&"right:"), "method parameter hints should remain intact: {hints:#?}");
    Ok(())
}

#[test]
fn non_output_builtin_hints_remain_unchanged() -> TestResult {
    let source = r#"open(FH, "<", "file.txt");"#;
    let ast = ast_for(source)?;
    let hints = parameter_hints(&ast, &offset_position, None);
    let actual = labels(&hints);

    assert!(actual.contains(&"filehandle:"), "non-output builtin roles must remain available");
    assert!(actual.contains(&"mode:"), "non-output builtin roles must remain available");
    assert!(actual.contains(&"filename:"), "non-output builtin roles must remain available");
    Ok(())
}

#[test]
fn exact_output_roles_respect_the_requested_range() -> TestResult {
    let source = r#"print STDERR "x";"#;
    let handle_offset = source.find("STDERR").ok_or("handle")? as u32;
    let ast = ast_for(source)?;

    let before_handle = Range::new(Position::new(0, 0), Position::new(0, handle_offset));
    let excluded = parameter_hints(&ast, &offset_position, Some(before_handle));
    assert!(
        !labels(&excluded).contains(&"filehandle:"),
        "range end is exclusive and must exclude the handle-start hint"
    );

    let handle_range = Range::new(
        Position::new(0, handle_offset),
        Position::new(0, source.len() as u32),
    );
    let included = parameter_hints(&ast, &offset_position, Some(handle_range));
    assert!(labels(&included).contains(&"filehandle:"));
    Ok(())
}

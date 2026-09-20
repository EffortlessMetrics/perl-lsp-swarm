//! Ordering proof from the independently observed baseline, with exact typed reasons and byte ranges.
use perl_parser_core::InvalidSignatureOrderingKind as Ordering;
use perl_parser_core::syntax::error::ParseDiagnosticAnchor;
use perl_parser_core::{
    InvalidSignatureParameterKind, Node, NodeKind, ParseError, Parser, RecoverySalvageClass,
    RecoverySalvageProfile,
};
use std::error::Error;
type R = Result<(), Box<dyn Error>>;
#[derive(Clone, Copy)]
enum Kind {
    Mandatory,
    Optional,
    Named,
    Slurpy,
    Error,
}
struct Param {
    text: &'static str,
    kind: Kind,
    variable: &'static str,
    default: Option<(&'static str, &'static str)>,
}
fn p(
    text: &'static str,
    kind: Kind,
    variable: &'static str,
    default: Option<(&'static str, &'static str)>,
) -> Param {
    Param { text, kind, variable, default }
}
fn sub(node: &Node) -> Option<&Node> {
    if matches!(node.kind, NodeKind::Subroutine { .. }) {
        return Some(node);
    }
    node.children().into_iter().find_map(sub)
}
fn after(node: &Node) -> Option<&Node> {
    if matches!(&node.kind, NodeKind::VariableDeclaration { variable, .. } if matches!(&variable.kind, NodeKind::Variable { name, .. } if name == "after"))
    {
        return Some(node);
    }
    node.children().into_iter().find_map(after)
}
fn slice<'a>(source: &'a str, node: &Node) -> Result<&'a str, Box<dyn Error>> {
    source
        .get(node.location.start..node.location.end)
        .ok_or_else(|| "invalid node byte range".into())
}
fn number(node: &Node, wanted: &str) -> R {
    if !matches!(&node.kind, NodeKind::Number { value } if value == wanted) {
        return Err("wrong Number topology/value".into());
    }
    Ok(())
}
fn check(params: &[Param], ordering: &[(usize, Ordering)], forms: &[usize], inline: bool) -> R {
    let signature = params.iter().map(|p| p.text).collect::<Vec<_>>().join("  , ");
    let source = if inline {
        format!(
            "# λ\nuse feature 'signatures';\nmy $glob = *{{sub ({signature}) {{ 7 }} }};\nmy $after = 9;"
        )
    } else {
        format!("# λ\nuse feature 'signatures';\nsub f ({signature}) {{ 7 }}\nmy $after = 9;")
    };
    let output = Parser::new(&source).parse_with_recovery();
    if output.stop_cause().is_some() {
        return Err(format!("terminal parse: {signature}").into());
    }
    let callable = sub(&output.ast).ok_or("lost callable")?;
    let NodeKind::Subroutine { signature: Some(header), body, name, .. } = &callable.kind else {
        return Err("lost signature/body".into());
    };
    if name.as_deref() != if inline { None } else { Some("f") } {
        return Err("wrong callable identity".into());
    }
    let NodeKind::Signature { parameters } = &header.kind else {
        return Err("wrong header kind".into());
    };
    if parameters.len() != params.len() {
        return Err("parameter count/order lost".into());
    }
    let header_start = source.find(&format!("({signature})")).ok_or("fixture header")?;
    if header.location.start != header_start
        || header.location.end != header_start + signature.len() + 2
    {
        return Err("header geometry changed".into());
    }
    let mut cursor = header_start + 1;
    let mut starts = Vec::new();
    for (actual, expected) in parameters.iter().zip(params) {
        starts.push(cursor);
        if actual.location.start != cursor
            || actual.location.end != cursor + expected.text.len()
            || slice(&source, actual)? != expected.text
        {
            return Err("parameter byte geometry/order changed".into());
        }
        let (variable, default, operator) = match (&actual.kind, expected.kind) {
            (NodeKind::MandatoryParameter { variable }, Kind::Mandatory) => {
                (variable.as_ref(), None, None)
            }
            (NodeKind::SlurpyParameter { variable }, Kind::Slurpy) => {
                (variable.as_ref(), None, None)
            }
            (
                NodeKind::OptionalParameter {
                    variable,
                    default_value,
                    default_operator,
                    default_operator_span,
                },
                Kind::Optional,
            ) => (
                variable.as_ref(),
                Some(default_value.as_ref()),
                Some((default_operator.as_str(), *default_operator_span)),
            ),
            (
                NodeKind::NamedParameter {
                    variable,
                    default_value,
                    default_operator,
                    default_operator_span,
                    external_name,
                    required,
                },
                Kind::Named,
            ) => {
                if external_name != expected.variable.get(1..).ok_or("fixture name")?
                    || *required != expected.default.is_none()
                {
                    return Err("named binding/requiredness changed".into());
                }
                let op = match (default_operator.as_deref(), default_operator_span) {
                    (None, None) => None,
                    (Some(op), Some(span)) => Some((op, *span)),
                    _ => return Err("unpaired named operator".into()),
                };
                (variable.as_ref(), default_value.as_deref(), op)
            }
            (NodeKind::Error { partial: Some(partial), .. }, Kind::Error) => {
                (partial.as_ref(), None, None)
            }
            _ => return Err(format!("wrong parameter kind for {}", expected.text).into()),
        };
        let variable_start =
            cursor + expected.text.find(expected.variable).ok_or("fixture variable")?;
        if !matches!(&variable.kind, NodeKind::Variable { sigil, name } if sigil == expected.variable.get(..1).ok_or("fixture sigil")? && name == expected.variable.get(1..).ok_or("fixture name")?)
            || variable.location.start != variable_start
            || variable.location.end != variable_start + expected.variable.len()
        {
            return Err("variable identity/range changed".into());
        }
        match (expected.default, default, operator) {
            (None, None, None) => {}
            (Some((op, value)), Some(default), Some((actual_op, span))) => {
                number(default, value)?;
                let op_start = cursor + expected.text.find(op).ok_or("fixture operator")?;
                let value_start = cursor + expected.text.rfind(value).ok_or("fixture default")?;
                if actual_op != op
                    || span.start != op_start
                    || span.end != op_start + op.len()
                    || default.location.start != value_start
                    || default.location.end != value_start + value.len()
                {
                    return Err("default/operator endpoints changed".into());
                }
            }
            _ => return Err("default topology/presence changed".into()),
        }
        cursor += expected.text.len() + 4;
    }
    if slice(&source, body)? != "{ 7 }" {
        return Err("body extent changed".into());
    }
    let NodeKind::Block { statements } = &body.kind else {
        return Err("body not Block".into());
    };
    if statements.len() != 1 {
        return Err("body statement count changed".into());
    }
    let stmt = statements.first().ok_or("body statement missing")?;
    number(
        match &stmt.kind {
            NodeKind::ExpressionStatement { expression } => expression,
            _ => stmt,
        },
        "7",
    )?;
    let NodeKind::VariableDeclaration {
        declarator, variable, initializer: Some(initializer), ..
    } = &after(&output.ast).ok_or("lost following declaration")?.kind
    else {
        return Err("following declaration topology changed".into());
    };
    if declarator != "my"
        || !matches!(&variable.kind, NodeKind::Variable { sigil, name } if sigil == "$" && name == "after")
        || slice(&source, variable)? != "$after"
    {
        return Err("following declaration identity changed".into());
    }
    number(initializer, "9")?;
    let form_diagnostics: Vec<_> = output
        .diagnostics
        .iter()
        .filter(|e| matches!(e, ParseError::InvalidSignatureParameter { .. }))
        .collect();
    let order_diagnostics: Vec<_> = output
        .diagnostics
        .iter()
        .filter(|e| matches!(e, ParseError::InvalidSignatureOrdering { .. }))
        .collect();
    if output.diagnostics.len() != form_diagnostics.len() + order_diagnostics.len() {
        return Err(format!(
            "{signature}: unexpected non-signature diagnostic: {:?}",
            output.diagnostics
        )
        .into());
    }
    if form_diagnostics.len() != forms.len() || order_diagnostics.len() != ordering.len() {
        return Err(format!(
            "{signature}: expected form/order counts {}/{}, got {}/{}",
            forms.len(),
            ordering.len(),
            form_diagnostics.len(),
            order_diagnostics.len()
        )
        .into());
    }
    for (error, index) in form_diagnostics.iter().zip(forms) {
        let param = parameters.get(*index).ok_or("fixture form index")?;
        if !matches!(error, ParseError::InvalidSignatureParameter { kind: InvalidSignatureParameterKind::NamedAggregate, range } if *range == param.location)
            || !error.blocks_clean_parse()
        {
            return Err("form error kind/full range changed".into());
        }
    }
    for (error, (index, expected_kind)) in order_diagnostics.iter().zip(ordering) {
        let parameter = parameters.get(*index).ok_or("fixture order index")?;
        if !matches!(error, ParseError::InvalidSignatureOrdering { kind, range } if kind == expected_kind && *range == parameter.location)
        {
            return Err(format!(
                "{signature}: wrong ordering reason or full parameter range: {error:?}"
            )
            .into());
        }
        if perl_parser_core::ErrorClass::error_class(*error)
            != perl_parser_core::ErrorCategory::UserError
        {
            return Err("ordering is not UserError".into());
        }
        let start = *starts.get(*index).ok_or("fixture order index")?;
        if !error.blocks_clean_parse()
            || error.location() != Some(start)
            || error.diagnostic_anchor() != ParseDiagnosticAnchor::Exact(start)
        {
            return Err(format!("{signature}: ordering diagnostic must anchor offending parameter {index} at byte {start}, got {error:?}").into());
        }
    }
    if ordering.is_empty()
        && forms.is_empty()
        && RecoverySalvageProfile::from_parse(
            &output.ast,
            &output.diagnostics,
            output.terminated_early(),
        )
        .class
            != RecoverySalvageClass::Clean
    {
        return Err("positive signature not clean".into());
    }
    Ok(())
}
use Kind::{Error as Bad, Mandatory as M, Named as N, Optional as O, Slurpy as S};
#[test]
fn issue_positive_sequences_preserve_parameter_roles() -> R {
    let cases = vec![
        vec![p("$a", M, "$a", None), p("$b", M, "$b", None)],
        vec![p("$a", M, "$a", None), p("$b = 1", O, "$b", Some(("=", "1")))],
        vec![p("$a = 1", O, "$a", Some(("=", "1"))), p("$b = 2", O, "$b", Some(("=", "2")))],
        vec![
            p("$a", M, "$a", None),
            p(":$x", N, "$x", None),
            p(":$y = 1", N, "$y", Some(("=", "1"))),
        ],
        vec![
            p("$a = 1", O, "$a", Some(("=", "1"))),
            p(":$x = 2", N, "$x", Some(("=", "2"))),
            p(":$y ||= 3", N, "$y", Some(("||=", "3"))),
        ],
        vec![
            p(":$x", N, "$x", None),
            p(":$y = 1", N, "$y", Some(("=", "1"))),
            p(":$z", N, "$z", None),
        ],
        vec![p("$a", M, "$a", None), p(":$x", N, "$x", None), p("@rest", S, "@rest", None)],
        vec![
            p("$a = 1", O, "$a", Some(("=", "1"))),
            p(":$x = 2", N, "$x", Some(("=", "2"))),
            p("%rest", S, "%rest", None),
        ],
        vec![p(":$x = 1", N, "$x", Some(("=", "1"))), p(":$required", N, "$required", None)],
    ];
    for case in cases {
        check(&case, &[], &[], false)?;
    }
    Ok(())
}
#[test]
fn mandatory_after_optional_has_one_offending_anchor() -> R {
    check(
        &[p("$a = 1", O, "$a", Some(("=", "1"))), p("$b", M, "$b", None)],
        &[(1, Ordering::MandatoryAfterOptional)],
        &[],
        false,
    )
}
#[test]
fn positional_after_named_is_rejected_without_losing_structure() -> R {
    let mut failures = Vec::new();
    for case in [
        vec![p(":$x", N, "$x", None), p("$a", M, "$a", None)],
        vec![p("$a", M, "$a", None), p(":$x", N, "$x", None), p("$b", M, "$b", None)],
        vec![p(":$x", N, "$x", None), p("$later = 2", O, "$later", Some(("=", "2")))],
    ] {
        if let Err(error) =
            check(&case, &[(case.len() - 1, Ordering::PositionalAfterNamed)], &[], false)
        {
            failures.push(error.to_string());
        }
    }
    if failures.is_empty() { Ok(()) } else { Err(failures.join("\n").into()) }
}
#[test]
fn required_named_after_optional_positional_is_rejected() -> R {
    check(
        &[p("$a = 1", O, "$a", Some(("=", "1"))), p(":$required", N, "$required", None)],
        &[(1, Ordering::RequiredNamedAfterOptional)],
        &[],
        false,
    )
}
#[test]
fn slurpy_boundary_blames_only_the_later_parameter() -> R {
    let mut failures = Vec::new();
    for case in [
        vec![p("@rest", S, "@rest", None), p("$later", M, "$later", None)],
        vec![p("%rest", S, "%rest", None), p(":$later", N, "$later", None)],
        vec![p("@a", S, "@a", None), p("%b", S, "%b", None)],
        vec![p("%a", S, "%a", None), p("@b", S, "@b", None)],
        vec![p("@a", S, "@a", None), p("@b", S, "@b", None)],
        vec![p("%a", S, "%a", None), p("%b", S, "%b", None)],
    ] {
        if let Err(error) = check(&case, &[(1, Ordering::ParameterAfterSlurpy)], &[], false) {
            failures.push(error.to_string());
        }
    }
    if failures.is_empty() { Ok(()) } else { Err(failures.join("\n").into()) }
}
#[test]
fn error_parameter_does_not_invent_named_state() -> R {
    check(&[p(":@bad", Bad, "@bad", None), p("$later", M, "$later", None)], &[], &[0], false)
}
#[test]
fn established_named_state_survives_error_parameter() -> R {
    check(
        &[
            p(":$known", N, "$known", None),
            p(":@bad", Bad, "@bad", None),
            p("$later", M, "$later", None),
        ],
        &[(2, Ordering::PositionalAfterNamed)],
        &[1],
        false,
    )
}
#[test]
fn established_optional_state_survives_error_parameter() -> R {
    check(
        &[
            p("$first = 1", O, "$first", Some(("=", "1"))),
            p(":@bad", Bad, "@bad", None),
            p(":$required", N, "$required", None),
        ],
        &[(2, Ordering::RequiredNamedAfterOptional)],
        &[1],
        false,
    )
}
#[test]
fn established_slurpy_state_survives_error_parameter() -> R {
    check(
        &[
            p("@rest", S, "@rest", None),
            p(":@bad", Bad, "@bad", None),
            p("$later", M, "$later", None),
        ],
        &[(2, Ordering::ParameterAfterSlurpy)],
        &[1],
        false,
    )
}
#[test]
fn actual_inline_ordering_anchors_use_original_bytes() -> R {
    check(
        &[p("$a = 1", O, "$a", Some(("=", "1"))), p("$b", M, "$b", None)],
        &[(1, Ordering::MandatoryAfterOptional)],
        &[],
        true,
    )?;
    check(
        &[p(":$x", N, "$x", None), p("$later = 2", O, "$later", Some(("=", "2")))],
        &[(1, Ordering::PositionalAfterNamed)],
        &[],
        true,
    )
}
#[test]
fn actual_inline_positive_named_mix_is_clean() -> R {
    check(
        &[p(":$x = 1", N, "$x", Some(("=", "1"))), p(":$required", N, "$required", None)],
        &[],
        &[],
        true,
    )
}

//! Baseline-executable parameter-form proof for #8915, governed by #8912.
use perl_parser_core::{Node, NodeKind, Parser, RecoverySalvageClass, RecoverySalvageProfile};
use std::error::Error;
type R = Result<(), Box<dyn Error>>;

#[test]
fn missing_default_before_invocant_colon_preserves_method_8915() -> R {
    fn method(node: &Node) -> Option<&Node> {
        if matches!(&node.kind, NodeKind::Method { name, .. } if name == "f") {
            return Some(node);
        }
        node.children().into_iter().find_map(method)
    }
    // Existing parser dialect recovery, not a claim that core Perl admits invocant colons.
    for operator in ["=", "//=", "||="] {
        let source = format!(
            "# λ\nuse feature 'signatures';\nmethod f ($self {operator} : $arg) {{ $arg }}\nmy $after = 7;"
        );
        let output = Parser::new(&source).parse_with_recovery();
        if output.stop_cause().is_some() {
            return Err("colon recovery terminated".into());
        }
        let NodeKind::Method { signature: Some(signature), body, .. } =
            &method(&output.ast).ok_or("lost method")?.kind
        else {
            return Err("lost method header".into());
        };
        let NodeKind::Signature { parameters } = &signature.kind else {
            return Err("lost signature".into());
        };
        if parameters.len() != 2 {
            return Err("lost later parameter".into());
        }
        let first = parameters.first().ok_or("lost first parameter")?;
        let expected = format!("$self {operator}");
        let NodeKind::Error { partial: Some(partial), found: Some(found), .. } = &first.kind else {
            return Err("missing default did not retain partial/token".into());
        };
        variable_identity(partial, "$", "self")?;
        if text(&source, first)? != expected
            || text(&source, partial)? != "$self"
            || found.text.as_ref() != operator
            || found.end() != first.location.end
            || found.start() != first.location.end - operator.len()
        {
            return Err("wrong missing-default geometry".into());
        }
        if output.diagnostics.len() != 1
            || !matches!(output.diagnostics.first(), Some(perl_parser_core::ParseError::InvalidSignatureParameter { kind: perl_parser_core::InvalidSignatureParameterKind::MissingDefaultExpression, range }) if *range == first.location)
        {
            return Err(format!("wrong colon diagnostic: {:?}", output.diagnostics).into());
        }
        let next = parameters.get(1).ok_or("lost arg")?;
        let NodeKind::MandatoryParameter { variable } = &next.kind else {
            return Err("wrong arg kind".into());
        };
        variable_identity(variable, "$", "arg")?;
        let arg_start = source.find(": $arg").ok_or("fixture arg")? + 2;
        if next.location.start != arg_start
            || next.location.end != arg_start + 4
            || variable.location != next.location
            || text(&source, body)? != "{ $arg }"
        {
            return Err("lost arg/body extent".into());
        }
        let after = after_declaration(&output.ast).ok_or("lost following declaration")?;
        if !matches!(&after.kind, NodeKind::VariableDeclaration { initializer: Some(value), .. } if matches!(&value.kind, NodeKind::Number { value } if value == "7"))
        {
            return Err("changed following declaration".into());
        }
    }
    Ok(())
}
fn callable(node: &Node) -> Option<&Node> {
    if matches!(&node.kind, NodeKind::Subroutine { name: Some(name), .. } if name == "f") {
        return Some(node);
    }
    node.children().into_iter().find_map(callable)
}
fn parameters(node: &Node) -> Result<&[Node], Box<dyn Error>> {
    let NodeKind::Subroutine { signature: Some(signature), .. } =
        &callable(node).ok_or("lost f")?.kind
    else {
        return Err("lost signature".into());
    };
    let NodeKind::Signature { parameters } = &signature.kind else {
        return Err("wrong header".into());
    };
    Ok(parameters)
}
fn text<'a>(source: &'a str, node: &Node) -> Result<&'a str, Box<dyn Error>> {
    source.get(node.location.start..node.location.end).ok_or_else(|| "invalid range".into())
}
fn variable_identity(node: &Node, sigil: &str, name: &str) -> R {
    if !matches!(&node.kind, NodeKind::Variable { sigil: actual_sigil, name: actual_name } if actual_sigil == sigil && actual_name == name)
    {
        return Err("wrong variable identity".into());
    }
    Ok(())
}
fn suffix(source: &str, ast: &Node) -> R {
    let param = parameters(ast)?.get(1).ok_or("lost suffix")?;
    let NodeKind::MandatoryParameter { variable } = &param.kind else {
        return Err("wrong suffix kind".into());
    };
    variable_identity(variable, "$", "next")?;
    let start = source.find(", $next").ok_or("fixture suffix missing")? + 2;
    if param.location.start != start
        || param.location.end != start + 5
        || variable.location != param.location
    {
        return Err("wrong suffix geometry".into());
    }
    let NodeKind::Subroutine { body, .. } = &callable(ast).ok_or("lost callable")?.kind else {
        return Err("lost body".into());
    };
    if text(source, body)? != "{ $next }" {
        return Err("wrong body extent".into());
    }
    let NodeKind::Block { statements } = &body.kind else {
        return Err("wrong body kind".into());
    };
    if statements.len() != 1 {
        return Err("lost body statement".into());
    }
    let statement = statements.first().ok_or("missing body statement")?;
    let expression = match &statement.kind {
        NodeKind::ExpressionStatement { expression } => expression.as_ref(),
        _ => statement,
    };
    variable_identity(expression, "$", "next")?;
    Ok(())
}
fn after_declaration(node: &Node) -> Option<&Node> {
    if matches!(&node.kind, NodeKind::VariableDeclaration { variable, .. } if matches!(&variable.kind, NodeKind::Variable { name, .. } if name == "after"))
    {
        return Some(node);
    }
    node.children().into_iter().find_map(after_declaration)
}
#[test]
fn six_scalar_default_forms_preserve_expression_and_binding() -> R {
    for named in [false, true] {
        for operator in ["=", "//=", "||="] {
            let marker = if named { ":" } else { "" };
            let parameter = format!("{marker}$value  {operator}  42");
            let source =
                format!("# λ\nuse feature 'signatures';\nsub f ({parameter}) {{ $value }}");
            let output = Parser::new(&source).parse_with_recovery();
            if RecoverySalvageProfile::from_parse(
                &output.ast,
                &output.diagnostics,
                output.terminated_early(),
            )
            .class
                != RecoverySalvageClass::Clean
            {
                return Err(format!("rejected scalar {parameter}").into());
            }
            let params = parameters(&output.ast)?;
            if params.len() != 1 {
                return Err("scalar parameter count drift".into());
            }
            let param = params.first().ok_or("missing scalar")?;
            if text(&source, param)? != parameter {
                return Err("wrong parameter extent".into());
            }
            let (variable, default) = match &param.kind {
                NodeKind::OptionalParameter { variable, default_value, .. } if !named => {
                    (variable, default_value)
                }
                NodeKind::NamedParameter {
                    variable,
                    default_value: Some(default),
                    default_operator,
                    external_name,
                    required,
                    ..
                } if named => {
                    if default_operator.as_deref() != Some(operator)
                        || external_name != "value"
                        || *required
                    {
                        return Err("wrong named metadata".into());
                    }
                    (variable, default)
                }
                _ => return Err("wrong scalar kind".into()),
            };
            let (actual_operator, operator_span) = match &param.kind {
                NodeKind::OptionalParameter { default_operator, default_operator_span, .. } => {
                    (default_operator.as_str(), *default_operator_span)
                }
                NodeKind::NamedParameter {
                    default_operator: Some(default_operator),
                    default_operator_span: Some(default_operator_span),
                    ..
                } => (default_operator.as_str(), *default_operator_span),
                _ => return Err("missing operator identity/geometry".into()),
            };
            let expected_start = source.find(operator).ok_or("fixture operator missing")?;
            if actual_operator != operator
                || operator_span.start != expected_start
                || operator_span.end != expected_start + operator.len()
            {
                return Err("operator identity/range drift".into());
            }
            let shifted = param
                .clone_with_mapped_locations(|range| perl_parser_core::SourceLocation {
                    start: range.start + 17,
                    end: range.end + 17,
                })
                .ok_or("operator clone failed")?;
            let shifted_span = match shifted.kind {
                NodeKind::OptionalParameter { default_operator_span, .. } => default_operator_span,
                NodeKind::NamedParameter { default_operator_span: Some(span), .. } => span,
                _ => return Err("clone lost operator span".into()),
            };
            if shifted_span.start != operator_span.start + 17
                || shifted_span.end != operator_span.end + 17
            {
                return Err("operator clone lost endpoint".into());
            }
            variable_identity(variable, "$", "value")?;
            if !matches!(&default.kind, NodeKind::Number { value } if value == "42") {
                return Err("wrong scalar default topology".into());
            }
            if text(&source, variable)? != "$value" || text(&source, default)? != "42" {
                return Err("wrong child geometry".into());
            }
        }
    }
    Ok(())
}
#[test]
fn mandatory_named_and_ordinary_slurpies_remain_clean() -> R {
    for header in ["$value", ":$known", "@rest", "%rest", ":$known, @rest", ":$known, %rest"] {
        let source = format!("use feature 'signatures';\nsub f ({header}) {{ 7 }}");
        let output = Parser::new(&source).parse_with_recovery();
        if RecoverySalvageProfile::from_parse(
            &output.ast,
            &output.diagnostics,
            output.terminated_early(),
        )
        .class
            != RecoverySalvageClass::Clean
        {
            return Err(format!("ordinary form rejected {header}").into());
        }
        let expected: Vec<_> = header.split(", ").collect();
        let params = parameters(&output.ast)?;
        if params.len() != expected.len() {
            return Err("ordinary parameter count drift".into());
        }
        for (param, spelling) in params.iter().zip(expected) {
            let variable = match (&param.kind, spelling.chars().next()) {
                (NodeKind::MandatoryParameter { variable }, Some('$')) => variable,
                (NodeKind::NamedParameter { variable, .. }, Some(':')) => variable,
                (NodeKind::SlurpyParameter { variable }, Some('@' | '%')) => variable,
                _ => return Err("ordinary parameter kind drift".into()),
            };
            let bare = match spelling.strip_prefix(':') {
                Some(bare) => bare,
                None => spelling,
            };
            let sigil = bare.get(..1).ok_or("fixture sigil")?;
            let name = bare.get(1..).ok_or("fixture name")?;
            variable_identity(variable, sigil, name)?;
            if text(&source, param)? != spelling || text(&source, variable)? != bare {
                return Err("ordinary geometry drift".into());
            }
            if let NodeKind::NamedParameter {
                external_name,
                required,
                default_value,
                default_operator,
                default_operator_span,
                ..
            } = &param.kind
            {
                if external_name != "known"
                    || !required
                    || default_value.is_some()
                    || default_operator.is_some()
                    || default_operator_span.is_some()
                {
                    return Err("mandatory metadata drift".into());
                }
                let shifted = param
                    .clone_with_mapped_locations(|range| perl_parser_core::SourceLocation {
                        start: range.start + 17,
                        end: range.end + 17,
                    })
                    .ok_or("mandatory clone failed")?;
                if !matches!(
                    &shifted.kind,
                    NodeKind::NamedParameter {
                        default_operator: None,
                        default_operator_span: None,
                        default_value: None,
                        required: true,
                        ..
                    }
                ) {
                    return Err("mandatory clone invented default metadata".into());
                }
            }
        }
    }
    Ok(())
}
#[test]
fn forbidden_aggregate_forms_preserve_error_extent_partial_and_suffix() -> R {
    for (bad, partial_text, operator) in [
        (":@rest", "@rest", None),
        (":%rest", "%rest", None),
        ("@rest = []", "[]", Some("=")),
        ("%rest = {}", "{}", Some("=")),
        ("@rest //= []", "[]", Some("//=")),
        ("%rest ||= {}", "{}", Some("||=")),
    ] {
        let source = format!(
            "# λ\nuse feature 'signatures';\nsub f ({bad}  , $next) {{ $next }}\nmy $after = 7;"
        );
        let output = Parser::new(&source).parse_with_recovery();
        if output.stop_cause().is_some() {
            return Err("invalid parameter terminated parse".into());
        }
        let params = parameters(&output.ast)?;
        if params.len() != 2 {
            return Err(format!("lost suffix for {bad}").into());
        }
        let param = params.first().ok_or("missing invalid parameter")?;
        let NodeKind::Error { partial: Some(partial), found, .. } = &param.kind else {
            return Err(format!("invalid {bad} became valid or lost partial").into());
        };
        if text(&source, param)? != bad || text(&source, partial)? != partial_text {
            return Err("wrong error or partial range".into());
        }
        if let Some(operator) = operator {
            let op_start = source.find(bad).ok_or("fixture parameter")?
                + bad.find(operator).ok_or("fixture operator")?;
            if found.as_ref().is_none_or(|token| {
                &*token.text != operator
                    || token.start() != op_start
                    || token.end() != op_start + operator.len()
            }) {
                return Err("lost operator token".into());
            }
        }
        match (partial_text, &partial.kind) {
            ("[]", NodeKind::ArrayLiteral { elements }) if elements.is_empty() => {}
            ("{}", NodeKind::HashLiteral { pairs }) if pairs.is_empty() => {}
            ("@rest", _) => variable_identity(partial, "@", "rest")?,
            ("%rest", _) => variable_identity(partial, "%", "rest")?,
            _ => return Err("wrong retained expression topology".into()),
        }
        let expected_kind = if bad.starts_with(':') {
            perl_parser_core::InvalidSignatureParameterKind::NamedAggregate
        } else {
            perl_parser_core::InvalidSignatureParameterKind::SlurpyDefault
        };
        if !output.diagnostics.iter().any(|error| matches!(error, perl_parser_core::ParseError::InvalidSignatureParameter { kind, .. } if *kind == expected_kind)) { return Err("wrong parser-produced parameter error kind".into()); }
        let invalid = output
            .diagnostics
            .iter()
            .find_map(|error| match error {
                perl_parser_core::ParseError::InvalidSignatureParameter { range, .. } => {
                    Some(*range)
                }
                _ => None,
            })
            .ok_or("missing typed parameter range")?;
        let first = params.first().ok_or("missing parameter")?;
        if invalid != first.location {
            return Err("typed diagnostic lost parameter endpoint".into());
        }
        suffix(&source, &output.ast)?;
        let after = after_declaration(&output.ast).ok_or("lost following declaration")?;
        let NodeKind::VariableDeclaration {
            declarator,
            variable,
            initializer: Some(initializer),
            ..
        } = &after.kind
        else {
            return Err("wrong following declaration".into());
        };
        variable_identity(variable, "$", "after")?;
        if declarator != "my"
            || !matches!(&initializer.kind, NodeKind::Number { value } if value == "7")
            || text(&source, variable)? != "$after"
        {
            return Err("following declaration changed".into());
        }
        if !output
            .diagnostics
            .iter()
            .any(|d| d.blocks_clean_parse() && d.location() == Some(param.location.start))
        {
            return Err("missing blocking anchored diagnostic".into());
        }
        if !matches!(
            &params.get(1).ok_or("missing suffix")?.kind,
            NodeKind::MandatoryParameter { .. }
        ) {
            return Err("wrong suffix shape".into());
        }
        let NodeKind::Subroutine { body, .. } = &callable(&output.ast).ok_or("lost callable")?.kind
        else {
            return Err("lost body".into());
        };
        if text(&source, body)? != "{ $next }" {
            return Err("wrong recovered body".into());
        }
        let shifted = output
            .ast
            .clone_with_mapped_locations(|range| perl_parser_core::SourceLocation {
                start: range.start + 17,
                end: range.end + 17,
            })
            .ok_or("failed clone")?;
        let shifted_error = parameters(&shifted)?.first().ok_or("missing shifted error")?;
        if shifted_error.location.start != param.location.start + 17
            || shifted_error.location.end != param.location.end + 17
        {
            return Err("error range did not shift".into());
        }
        let NodeKind::Error { partial: Some(shifted_partial), found: shifted_found, .. } =
            &shifted_error.kind
        else {
            return Err("shift lost recovery payload".into());
        };
        if shifted_partial.location.start != partial.location.start + 17
            || shifted_partial.location.end != partial.location.end + 17
        {
            return Err("partial range did not shift".into());
        }
        if let Some(found) = found {
            let shifted_found = shifted_found.as_ref().ok_or("shift lost token")?;
            if shifted_found.start() != found.start() + 17
                || shifted_found.end() != found.end() + 17
                || shifted_found.text != found.text
            {
                return Err("token range did not shift".into());
            }
        }
    }
    Ok(())
}
#[test]
fn missing_aggregate_default_leaves_separator_and_body() -> R {
    for bad in ["@rest =", "%rest ||=", "@rest //="] {
        let source = format!("use feature 'signatures';\nsub f ({bad}, $next) {{ $next }}");
        let output = Parser::new(&source).parse_with_recovery();
        if output.stop_cause().is_some() {
            return Err("missing default terminated parse".into());
        }
        let params = parameters(&output.ast)?;
        if params.len() != 2 || text(&source, params.first().ok_or("missing error")?)? != bad {
            return Err("lost separator or invented extent".into());
        }
        let expected_kind =
            perl_parser_core::InvalidSignatureParameterKind::MissingDefaultExpression;
        if !output.diagnostics.iter().any(|error| matches!(error, perl_parser_core::ParseError::InvalidSignatureParameter { kind, .. } if *kind == expected_kind)) { return Err("wrong parser-produced parameter error kind".into()); }
        let invalid = output
            .diagnostics
            .iter()
            .find_map(|error| match error {
                perl_parser_core::ParseError::InvalidSignatureParameter { range, .. } => {
                    Some(*range)
                }
                _ => None,
            })
            .ok_or("missing typed parameter range")?;
        let first = params.first().ok_or("missing parameter")?;
        if invalid != first.location {
            return Err("typed diagnostic lost parameter endpoint".into());
        }
        suffix(&source, &output.ast)?;
        let error = params.first().ok_or("missing error")?;
        let NodeKind::Error { partial, found: Some(found), .. } = &error.kind else {
            return Err("missing default lost operator evidence".into());
        };
        if let Some(partial) = partial {
            variable_identity(partial, if bad.starts_with('@') { "@" } else { "%" }, "rest")?;
        }
        let op = bad.split_whitespace().nth(1).ok_or("fixture operator")?;
        if &*found.text != op
            || found.end() != error.location.end
            || found.start() + op.len() != found.end()
        {
            return Err("wrong missing-default token extent".into());
        }
        if !output
            .diagnostics
            .iter()
            .any(|d| d.blocks_clean_parse() && d.location() == Some(error.location.start))
        {
            return Err("missing anchored blocking diagnostic".into());
        }
        if !matches!(params.first().map(|n| &n.kind), Some(NodeKind::Error { .. }))
            || output.diagnostics.is_empty()
        {
            return Err("missing default accepted".into());
        }
    }
    Ok(())
}
#[test]
fn unclosed_default_is_not_clean() -> R {
    let output =
        Parser::new("use feature 'signatures';\nsub f (@rest = [1, 2").parse_with_recovery();
    if RecoverySalvageProfile::from_parse(
        &output.ast,
        &output.diagnostics,
        output.terminated_early(),
    )
    .class
        == RecoverySalvageClass::Clean
    {
        return Err("unclosed default clean".into());
    }
    // No full-parameter-range or suffix claim for missing closing boundaries.
    Ok(())
}

// Reach the lexer's captured typeglob-body route, not only the shifting helper.
#[test]
fn inline_anonymous_signature_offsets_operator_and_invalid_ranges() -> R {
    fn first_parameter(node: &Node) -> Option<&Node> {
        if matches!(
            node.kind,
            NodeKind::OptionalParameter { .. }
                | NodeKind::NamedParameter { .. }
                | NodeKind::Error { .. }
        ) {
            return Some(node);
        }
        node.children().into_iter().find_map(first_parameter)
    }
    for parameter in ["$value //= 42", ":$value ||= 42", "@rest = []"] {
        let source =
            format!("# λ\nuse feature 'signatures'; my $glob = *{{sub ({parameter}) {{ 7 }} }};");
        let output = Parser::new(&source).parse_with_recovery();
        if output.stop_cause().is_some() {
            return Err("inline parse terminated".into());
        }
        let actual = first_parameter(&output.ast).ok_or("lost inline parameter")?;
        if parameter.starts_with('@') {
            let range = output
                .diagnostics
                .iter()
                .find_map(|error| match error {
                    perl_parser_core::ParseError::InvalidSignatureParameter { range, .. } => {
                        Some(*range)
                    }
                    _ => None,
                })
                .ok_or("lost forwarded diagnostic")?;
            let start = source.find(parameter).ok_or("fixture parameter")?;
            if range.start != start || range.end != start + parameter.len() {
                return Err("forwarded diagnostic endpoints wrong".into());
            }
        } else {
            if RecoverySalvageProfile::from_parse(
                &output.ast,
                &output.diagnostics,
                output.terminated_early(),
            )
            .class
                != RecoverySalvageClass::Clean
            {
                return Err("valid inline signature has blocking recovery".into());
            }
            let span = match actual.kind {
                NodeKind::OptionalParameter { default_operator_span, .. } => default_operator_span,
                NodeKind::NamedParameter { default_operator_span: Some(span), .. } => span,
                _ => return Err("lost inline operator".into()),
            };
            let operator = if parameter.starts_with(':') { "||=" } else { "//=" };
            let start = source.find(operator).ok_or("fixture operator")?;
            if span.start != start || span.end != start + 3 {
                return Err("inline operator endpoint not shifted".into());
            }
        }
    }
    Ok(())
}

#[test]
fn closed_malformed_aggregate_default_is_not_silently_accepted() -> R {
    let source = "use feature 'signatures'; sub f (@rest = [1 + ], $next) { $next }";
    let output = Parser::new(source).parse_with_recovery();
    let profile = RecoverySalvageProfile::from_parse(
        &output.ast,
        &output.diagnostics,
        output.terminated_early(),
    );
    if profile.class == RecoverySalvageClass::Clean {
        return Err("malformed aggregate default accepted cleanly".into());
    }
    if let Ok(params) = parameters(&output.ast)
        && matches!(params.first().map(|node| &node.kind), Some(NodeKind::SlurpyParameter { .. }))
    {
        return Err("malformed default silently discarded into slurpy".into());
    }
    // Interior malformed expressions retain the expression parser's recovery
    // boundary; this test does not promise a surviving callable or suffix.
    Ok(())
}

// ------------------------------------------------------------------
// #16242: a grouped default consumes its closing grouping delimiters,
// but the parenthesized primary returns the inner expression node, so
// the consumed closers must not fall outside the parameter (or
// InvalidSignatureParameter) extent.
// ------------------------------------------------------------------

#[test]
fn grouped_default_extent_includes_consumed_closer_16242() -> R {
    let source = "# λ\nuse feature 'signatures';\nsub f ($alpha = (1+2)) { 'body' }";
    let output = Parser::new(&source).parse_with_recovery();
    if RecoverySalvageProfile::from_parse(
        &output.ast,
        &output.diagnostics,
        output.terminated_early(),
    )
    .class
        != RecoverySalvageClass::Clean
    {
        return Err("grouped scalar default rejected".into());
    }
    let param = parameters(&output.ast)?.first().ok_or("lost grouped default param")?;
    let NodeKind::OptionalParameter { default_value, .. } = &param.kind else {
        return Err("wrong grouped-default kind".into());
    };
    if text(&source, param)? != "$alpha = (1+2)" {
        return Err(format!("wrong grouped-default extent: {:?}", text(&source, param)?).into());
    }
    // The child expression's own canonical span contract is unchanged.
    if text(&source, default_value)? != "1+2" {
        return Err(
            format!("child span contract changed: {:?}", text(&source, default_value)?).into()
        );
    }
    Ok(())
}

#[test]
fn nested_grouped_default_extent_includes_both_closers_16242() -> R {
    let source = "# λ\nuse feature 'signatures';\nsub f ($c = (($y))) { $c }";
    let output = Parser::new(&source).parse_with_recovery();
    if RecoverySalvageProfile::from_parse(
        &output.ast,
        &output.diagnostics,
        output.terminated_early(),
    )
    .class
        != RecoverySalvageClass::Clean
    {
        return Err("nested grouped default rejected".into());
    }
    let param = parameters(&output.ast)?.first().ok_or("lost nested grouped param")?;
    if text(&source, param)? != "$c = (($y))" {
        return Err(format!("wrong nested extent: {:?}", text(&source, param)?).into());
    }
    Ok(())
}

#[test]
fn named_grouped_default_extent_includes_consumed_closer_16242() -> R {
    let source = "# λ\nuse feature 'signatures';\nsub f (:$x = (5)) { $x }";
    let output = Parser::new(&source).parse_with_recovery();
    if RecoverySalvageProfile::from_parse(
        &output.ast,
        &output.diagnostics,
        output.terminated_early(),
    )
    .class
        != RecoverySalvageClass::Clean
    {
        return Err("named grouped default rejected".into());
    }
    let param = parameters(&output.ast)?.first().ok_or("lost named grouped param")?;
    let NodeKind::NamedParameter { variable, .. } = &param.kind else {
        return Err("wrong named kind".into());
    };
    variable_identity(variable, "$", "x")?;
    if text(&source, param)? != ":$x = (5)" {
        return Err(format!("wrong named extent: {:?}", text(&source, param)?).into());
    }
    Ok(())
}

#[test]
fn grouped_default_with_following_parameter_and_body_16242() -> R {
    let source = "# λ\nuse feature 'signatures';\nsub f ($a = (2*3), $b = 4) { $a + $b }";
    let output = Parser::new(&source).parse_with_recovery();
    if RecoverySalvageProfile::from_parse(
        &output.ast,
        &output.diagnostics,
        output.terminated_early(),
    )
    .class
        != RecoverySalvageClass::Clean
    {
        return Err("grouped default with following parameter rejected".into());
    }
    let params = parameters(&output.ast)?;
    if params.len() != 2 {
        return Err("lost following parameter".into());
    }
    if text(&source, params.first().ok_or("lost first")?)? != "$a = (2*3)" {
        return Err("first parameter extent dropped the consumed closer".into());
    }
    if text(&source, params.get(1).ok_or("lost second")?)? != "$b = 4" {
        return Err("following parameter extent changed".into());
    }
    let NodeKind::Subroutine { body, .. } = &callable(&output.ast).ok_or("lost f")?.kind else {
        return Err("lost subroutine".into());
    };
    if text(&source, body)? != "{ $a + $b }" {
        return Err("body sentinel extent changed".into());
    }
    Ok(())
}

#[test]
fn rejected_slurpy_default_invalid_range_includes_consumed_closer_16242() -> R {
    let source = "# λ\nuse feature 'signatures';\nsub f (@a = (1,2), $next) { $next }";
    let output = Parser::new(&source).parse_with_recovery();
    let param = parameters(&output.ast)?.first().ok_or("lost slurpy param")?;
    // The invalid-parameter range stays honest to the consumed aggregate default.
    if text(&source, param)? != "@a = (1,2)" {
        return Err(format!("wrong slurpy extent: {:?}", text(&source, param)?).into());
    }
    if !output.diagnostics.iter().any(|diagnostic| {
        matches!(
            diagnostic,
            perl_parser_core::ParseError::InvalidSignatureParameter {
                kind: perl_parser_core::InvalidSignatureParameterKind::SlurpyDefault,
                range,
            } if *range == param.location
        )
    }) {
        return Err(format!("missing SlurpyDefault range: {:?}", output.diagnostics).into());
    }
    // Recovery preserves the suffix and body.
    suffix(&source, &output.ast)?;
    Ok(())
}

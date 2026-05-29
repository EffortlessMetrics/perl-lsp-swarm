use super::statement::{
    format_simple_assignment_clause, format_simple_condition_tokens,
    format_simple_expression_tokens, format_simple_lexical_clause, format_simple_statement_tokens,
    format_variable_tokens, indent_unit,
};
use super::{BracePlacement, ElsePlacement, FormatConfig, FormatDoc, KeywordSpacing};

pub(super) fn format_simple_subroutine_tokens(
    tokens: &[perl_parser_core::Token],
    indent: &str,
    config: &FormatConfig,
) -> Option<String> {
    use perl_parser_core::TokenKind;

    if tokens.len() < 4 {
        return None;
    }
    if tokens[0].kind != TokenKind::Sub
        || tokens[1].kind != TokenKind::Identifier
        || tokens[2].kind != TokenKind::LeftBrace
        || tokens.last()?.kind != TokenKind::RightBrace
    {
        return None;
    }

    let body_tokens = &tokens[3..tokens.len() - 1];
    let statements = format_simple_statement_block(body_tokens, config)?;
    let body_indent = format!("{indent}{}", indent_unit(config));
    Some(render_simple_block_doc(
        format!("{indent}sub {} {{", tokens[1].text),
        &statements,
        indent,
        &body_indent,
        config,
    ))
}

pub(super) fn format_simple_control_block_tokens(
    tokens: &[perl_parser_core::Token],
    indent: &str,
    config: &FormatConfig,
) -> Option<String> {
    use perl_parser_core::TokenKind;

    if let Some(formatted) = format_simple_c_style_for_block_tokens(tokens, indent, config) {
        return Some(formatted);
    }

    if let Some(formatted) = format_simple_foreach_block_tokens(tokens, indent, config) {
        return Some(formatted);
    }

    if tokens.len() < 6 {
        return None;
    }
    let keyword = match tokens[0].kind {
        TokenKind::If => "if",
        TokenKind::Unless => "unless",
        TokenKind::While => "while",
        TokenKind::Until => "until",
        _ => return None,
    };
    if tokens[1].kind != TokenKind::LeftParen {
        return None;
    }

    let (condition, next_index) = format_simple_condition_tokens(tokens, 2, config)?;
    if tokens.get(next_index)?.kind != TokenKind::RightParen
        || tokens.get(next_index + 1)?.kind != TokenKind::LeftBrace
    {
        return None;
    }

    let body_start = next_index + 2;
    let body_end = tokens[body_start..]
        .iter()
        .position(|token| token.kind == TokenKind::RightBrace)
        .map(|offset| body_start + offset)?;
    let body_tokens = &tokens[body_start..body_end];
    let statements = format_simple_statement_block(body_tokens, config)?;

    let body_indent = format!("{indent}{}", indent_unit(config));
    let mut formatted = render_simple_block_doc(
        render_condition_block_header(indent, keyword, &condition, config),
        &statements,
        indent,
        &body_indent,
        config,
    );

    match keyword {
        "if" | "unless" => {
            let tail = format_simple_control_tail(tokens, body_end, keyword, config)?;
            for (condition, statements) in tail.elsif_branches {
                formatted.push_str(&render_simple_elsif_doc(
                    &condition,
                    &statements,
                    indent,
                    &body_indent,
                    config,
                ));
            }
            if let Some(else_statements) = tail.else_statements {
                formatted.push_str(&render_simple_else_doc(
                    &else_statements,
                    indent,
                    &body_indent,
                    config,
                ));
            }
        }
        "while" | "until" => {
            if let Some(continue_statements) =
                format_simple_continue_tail(tokens, body_end, config)?
            {
                formatted.push_str(&render_simple_continue_doc(
                    &continue_statements,
                    indent,
                    &body_indent,
                    config,
                ));
            }
        }
        _ => return None,
    }
    Some(formatted)
}

pub(super) fn format_simple_c_style_for_block_tokens(
    tokens: &[perl_parser_core::Token],
    indent: &str,
    config: &FormatConfig,
) -> Option<String> {
    use perl_parser_core::TokenKind;

    if tokens.first()?.kind != TokenKind::For || tokens.get(1)?.kind != TokenKind::LeftParen {
        return None;
    }

    let (first_semicolon, second_semicolon, header_end) = find_for_header_boundaries(tokens, 1)?;
    if tokens.get(header_end + 1)?.kind != TokenKind::LeftBrace {
        return None;
    }

    let init = format_simple_for_init_clause(tokens, 2, first_semicolon, config)?;
    let condition =
        format_simple_for_condition_clause(tokens, first_semicolon + 1, second_semicolon, config)?;
    let update = format_simple_for_update_clause(tokens, second_semicolon + 1, header_end, config)?;

    let body_start = header_end + 2;
    let body_end = tokens[body_start..]
        .iter()
        .position(|token| token.kind == TokenKind::RightBrace)
        .map(|offset| body_start + offset)?;
    let statements = format_simple_statement_block(&tokens[body_start..body_end], config)?;

    let body_indent = format!("{indent}{}", indent_unit(config));
    let mut formatted = render_simple_block_doc(
        format!("{indent}{}", render_simple_for_header(&init, &condition, &update)),
        &statements,
        indent,
        &body_indent,
        config,
    );
    if let Some(continue_statements) = format_simple_continue_tail(tokens, body_end, config)? {
        formatted.push_str(&render_simple_continue_doc(
            &continue_statements,
            indent,
            &body_indent,
            config,
        ));
    }
    Some(formatted)
}

pub(super) fn find_for_header_boundaries(
    tokens: &[perl_parser_core::Token],
    open_index: usize,
) -> Option<(usize, usize, usize)> {
    use perl_parser_core::TokenKind;

    let mut depth = 0usize;
    let mut first_semicolon = None;
    let mut second_semicolon = None;

    for (index, token) in tokens.iter().enumerate().skip(open_index) {
        match token.kind {
            TokenKind::LeftParen => depth += 1,
            TokenKind::RightParen => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some((first_semicolon?, second_semicolon?, index));
                }
            }
            TokenKind::Semicolon if depth == 1 => {
                if first_semicolon.is_none() {
                    first_semicolon = Some(index);
                } else if second_semicolon.is_none() {
                    second_semicolon = Some(index);
                } else {
                    return None;
                }
            }
            _ => {}
        }
    }
    None
}

pub(super) fn render_simple_for_header(init: &str, condition: &str, update: &str) -> String {
    let mut header = format!("for ({init};");
    if !condition.is_empty() {
        header.push(' ');
        header.push_str(condition);
    }
    header.push(';');
    if !update.is_empty() {
        header.push(' ');
        header.push_str(update);
    }
    header.push_str(") {");
    header
}

pub(super) fn format_simple_for_init_clause(
    tokens: &[perl_parser_core::Token],
    start: usize,
    end: usize,
    config: &FormatConfig,
) -> Option<String> {
    use perl_parser_core::TokenKind;

    if start == end {
        return Some(String::new());
    }

    match tokens.get(start)?.kind {
        TokenKind::My | TokenKind::Our | TokenKind::State => {
            format_simple_lexical_clause(tokens, start, end, config)
        }
        _ => format_simple_assignment_clause(tokens, start, end, config),
    }
}

pub(super) fn format_simple_for_condition_clause(
    tokens: &[perl_parser_core::Token],
    start: usize,
    end: usize,
    config: &FormatConfig,
) -> Option<String> {
    if start == end {
        return Some(String::new());
    }
    format_simple_expression_tokens(tokens, start, end, config, 0)
}

pub(super) fn format_simple_for_update_clause(
    tokens: &[perl_parser_core::Token],
    start: usize,
    end: usize,
    config: &FormatConfig,
) -> Option<String> {
    use perl_parser_core::TokenKind;

    if start == end {
        return Some(String::new());
    }

    if let Some((variable, next_index)) = format_variable_tokens(tokens, start)
        && next_index + 1 == end
    {
        return match tokens.get(next_index)?.kind {
            TokenKind::Increment => Some(format!("{variable}++")),
            TokenKind::Decrement => Some(format!("{variable}--")),
            _ => None,
        };
    }

    if matches!(tokens.get(start)?.kind, TokenKind::Increment | TokenKind::Decrement) {
        let (variable, next_index) = format_variable_tokens(tokens, start + 1)?;
        if next_index == end {
            return Some(format!("{}{variable}", tokens[start].text));
        }
    }

    format_simple_assignment_clause(tokens, start, end, config)
}

pub(super) fn format_simple_foreach_block_tokens(
    tokens: &[perl_parser_core::Token],
    indent: &str,
    config: &FormatConfig,
) -> Option<String> {
    use perl_parser_core::TokenKind;

    let keyword = match tokens.first()?.kind {
        TokenKind::For => "for",
        TokenKind::Foreach => "foreach",
        _ => return None,
    };

    let mut index = 1;
    let iterator =
        if matches!(tokens.get(index)?.kind, TokenKind::My | TokenKind::Our | TokenKind::State) {
            let lexical = tokens[index].text.as_ref();
            let (variable, next_index) = format_variable_tokens(tokens, index + 1)?;
            index = next_index;
            format!("{lexical} {variable}")
        } else {
            let (variable, next_index) = format_variable_tokens(tokens, index)?;
            index = next_index;
            variable
        };

    if tokens.get(index)?.kind != TokenKind::LeftParen {
        return None;
    }
    let list_start = index + 1;
    let list_end = tokens[list_start..]
        .iter()
        .position(|token| token.kind == TokenKind::RightParen)
        .map(|offset| list_start + offset)?;
    let list = format_simple_expression_tokens(tokens, list_start, list_end, config, 0)?;

    if tokens.get(list_end)?.kind != TokenKind::RightParen
        || tokens.get(list_end + 1)?.kind != TokenKind::LeftBrace
    {
        return None;
    }

    let body_start = list_end + 2;
    let body_end = tokens[body_start..]
        .iter()
        .position(|token| token.kind == TokenKind::RightBrace)
        .map(|offset| body_start + offset)?;
    let body_tokens = &tokens[body_start..body_end];
    let statements = format_simple_statement_block(body_tokens, config)?;
    let body_indent = format!("{indent}{}", indent_unit(config));
    let mut formatted = render_simple_block_doc(
        format!("{indent}{keyword} {iterator} ({list}) {{"),
        &statements,
        indent,
        &body_indent,
        config,
    );
    if let Some(continue_statements) = format_simple_continue_tail(tokens, body_end, config)? {
        formatted.push_str(&render_simple_continue_doc(
            &continue_statements,
            indent,
            &body_indent,
            config,
        ));
    }
    Some(formatted)
}

pub(super) fn render_simple_block_doc(
    header: String,
    statements: &[String],
    indent: &str,
    body_indent: &str,
    config: &FormatConfig,
) -> String {
    let mut parts = vec![FormatDoc::text(render_block_header(&header, indent, config))];
    push_simple_block_body_docs(&mut parts, statements, indent, body_indent);
    FormatDoc::group(parts).render(config)
}

pub(super) fn render_simple_else_doc(
    statements: &[String],
    indent: &str,
    body_indent: &str,
    config: &FormatConfig,
) -> String {
    let header = if config.else_placement == ElsePlacement::SeparateLine {
        format!("\n{indent}else {{")
    } else {
        " else {".to_string()
    };
    let mut parts = vec![FormatDoc::text(render_block_header(&header, indent, config))];
    push_simple_block_body_docs(&mut parts, statements, indent, body_indent);
    FormatDoc::group(parts).render(config)
}

pub(super) fn render_simple_elsif_doc(
    condition: &str,
    statements: &[String],
    indent: &str,
    body_indent: &str,
    config: &FormatConfig,
) -> String {
    let header = if config.else_placement == ElsePlacement::SeparateLine {
        format!("\n{}", render_condition_block_header(indent, "elsif", condition, config))
    } else {
        let gap = keyword_condition_gap(config);
        format!(" elsif{gap}({condition}) {{")
    };
    let mut parts = vec![FormatDoc::text(render_block_header(&header, indent, config))];
    push_simple_block_body_docs(&mut parts, statements, indent, body_indent);
    FormatDoc::group(parts).render(config)
}

pub(super) fn render_simple_continue_doc(
    statements: &[String],
    indent: &str,
    body_indent: &str,
    config: &FormatConfig,
) -> String {
    let mut parts = vec![FormatDoc::text(render_block_header(" continue {", indent, config))];
    push_simple_block_body_docs(&mut parts, statements, indent, body_indent);
    FormatDoc::group(parts).render(config)
}

pub(super) fn render_block_header(header: &str, indent: &str, config: &FormatConfig) -> String {
    if config.brace_placement != BracePlacement::NextLine {
        return header.to_string();
    }

    header
        .strip_suffix(" {")
        .map_or_else(|| header.to_string(), |prefix| format!("{prefix}\n{indent}{{"))
}

pub(super) fn render_condition_block_header(
    indent: &str,
    keyword: &str,
    condition: &str,
    config: &FormatConfig,
) -> String {
    let gap = keyword_condition_gap(config);
    format!("{indent}{keyword}{gap}({condition}) {{")
}

pub(super) fn keyword_condition_gap(config: &FormatConfig) -> &'static str {
    match config.keyword_spacing {
        KeywordSpacing::Space => " ",
        KeywordSpacing::Compact => "",
    }
}

pub(super) fn push_simple_block_body_docs(
    parts: &mut Vec<FormatDoc>,
    statements: &[String],
    indent: &str,
    body_indent: &str,
) {
    for statement in statements {
        parts.push(FormatDoc::HardLine);
        parts.push(FormatDoc::text(format!("{body_indent}{statement}")));
    }
    parts.push(FormatDoc::HardLine);
    parts.push(FormatDoc::text(format!("{indent}}}")));
}

pub(super) struct SimpleControlTail {
    elsif_branches: Vec<(String, Vec<String>)>,
    else_statements: Option<Vec<String>>,
}

pub(super) fn format_simple_control_tail(
    tokens: &[perl_parser_core::Token],
    body_end: usize,
    keyword: &str,
    config: &FormatConfig,
) -> Option<SimpleControlTail> {
    use perl_parser_core::TokenKind;

    let mut index = body_end + 1;
    let mut tail = SimpleControlTail { elsif_branches: Vec::new(), else_statements: None };
    if index == tokens.len() {
        return Some(tail);
    }

    while tokens.get(index)?.kind == TokenKind::Elsif {
        if keyword != "if" {
            return None;
        }
        if tokens.get(index + 1)?.kind != TokenKind::LeftParen {
            return None;
        }

        let (condition, next_index) = format_simple_condition_tokens(tokens, index + 2, config)?;
        if tokens.get(next_index)?.kind != TokenKind::RightParen
            || tokens.get(next_index + 1)?.kind != TokenKind::LeftBrace
        {
            return None;
        }

        let elsif_body_start = next_index + 2;
        let elsif_body_end = tokens[elsif_body_start..]
            .iter()
            .position(|token| token.kind == TokenKind::RightBrace)
            .map(|offset| elsif_body_start + offset)?;
        let statements =
            format_simple_statement_block(&tokens[elsif_body_start..elsif_body_end], config)?;
        tail.elsif_branches.push((condition, statements));
        index = elsif_body_end + 1;

        if index == tokens.len() {
            return Some(tail);
        }
    }

    if tokens.get(index)?.kind != TokenKind::Else {
        return None;
    }
    if !matches!(keyword, "if" | "unless")
        || tokens.get(index + 1)?.kind != TokenKind::LeftBrace
        || tokens.last()?.kind != TokenKind::RightBrace
    {
        return None;
    }

    let else_body_start = index + 2;
    let else_body_tokens = &tokens[else_body_start..tokens.len() - 1];
    let statements = format_simple_statement_block(else_body_tokens, config)?;
    tail.else_statements = Some(statements);
    Some(tail)
}

pub(super) fn format_simple_continue_tail(
    tokens: &[perl_parser_core::Token],
    body_end: usize,
    config: &FormatConfig,
) -> Option<Option<Vec<String>>> {
    use perl_parser_core::TokenKind;

    let next = body_end + 1;
    if next == tokens.len() {
        return Some(None);
    }
    if tokens.get(next)?.kind != TokenKind::Continue
        || tokens.get(next + 1)?.kind != TokenKind::LeftBrace
        || tokens.last()?.kind != TokenKind::RightBrace
    {
        return None;
    }

    let continue_body_start = next + 2;
    let continue_body_tokens = &tokens[continue_body_start..tokens.len() - 1];
    let statements = format_simple_statement_block(continue_body_tokens, config)?;
    Some(Some(statements))
}

pub(super) fn format_simple_statement_block(
    tokens: &[perl_parser_core::Token],
    config: &FormatConfig,
) -> Option<Vec<String>> {
    use perl_parser_core::TokenKind;

    if tokens.is_empty() {
        return Some(Vec::new());
    }

    let mut statements = Vec::new();
    let mut start = 0;
    for (idx, token) in tokens.iter().enumerate() {
        if token.kind != TokenKind::Semicolon {
            continue;
        }

        let statement_tokens = &tokens[start..=idx];
        statements.push(format_simple_statement_tokens(statement_tokens, config)?);
        start = idx + 1;
    }

    (start == tokens.len()).then_some(statements)
}

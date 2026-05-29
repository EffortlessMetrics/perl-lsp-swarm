use super::{FormatConfig, FormatDoc, TrailingComma};

pub(super) fn format_simple_statement_tokens(
    tokens: &[perl_parser_core::Token],
    config: &FormatConfig,
) -> Option<String> {
    format_simple_lexical_tokens(tokens, config)
        .or_else(|| format_simple_return_tokens(tokens, config))
        .or_else(|| format_simple_loop_control_tokens(tokens))
        .or_else(|| format_simple_assignment_tokens(tokens, config))
        .or_else(|| format_simple_expression_statement_tokens(tokens, config))
}

pub(super) fn format_simple_module_tokens(
    tokens: &[perl_parser_core::Token],
    config: &FormatConfig,
) -> Option<String> {
    use perl_parser_core::TokenKind;

    if tokens.last()?.kind != TokenKind::Semicolon {
        return None;
    }

    match tokens.first()?.kind {
        TokenKind::Package => format_simple_package_tokens(tokens, config),
        TokenKind::Use => format_simple_import_tokens("use", tokens, 1, config),
        TokenKind::No => format_simple_import_tokens("no", tokens, 1, config),
        TokenKind::Identifier if tokens.first()?.text.as_ref() == "require" => {
            format_simple_import_tokens("require", tokens, 1, config)
        }
        _ => None,
    }
}

pub(super) fn format_simple_package_tokens(
    tokens: &[perl_parser_core::Token],
    config: &FormatConfig,
) -> Option<String> {
    use perl_parser_core::TokenKind;

    if tokens.len() < 3 || tokens.first()?.kind != TokenKind::Package {
        return None;
    }

    let semicolon_index = tokens.len() - 1;
    let name = tokens.get(1)?;
    if name.kind != TokenKind::Identifier {
        return None;
    }

    if semicolon_index == 2 {
        return Some(format!("package {};", name.text));
    }

    let version = format_simple_module_args(tokens, 2, semicolon_index, config, "package ".len())?;
    Some(format!("package {} {version};", name.text))
}

pub(super) fn format_simple_import_tokens(
    keyword: &str,
    tokens: &[perl_parser_core::Token],
    args_start: usize,
    config: &FormatConfig,
) -> Option<String> {
    let semicolon_index = tokens.len() - 1;
    let args = format_simple_module_args(
        tokens,
        args_start,
        semicolon_index,
        config,
        keyword.chars().count() + 1,
    )?;
    Some(format!("{keyword} {args};"))
}

pub(super) fn format_simple_module_args(
    tokens: &[perl_parser_core::Token],
    start: usize,
    end: usize,
    config: &FormatConfig,
    start_column: usize,
) -> Option<String> {
    let mut parts = Vec::new();
    let mut index = start;
    let mut column = start_column;

    while index < end {
        let (part, next_index) = format_simple_atom_tokens(tokens, index, config, column)?;
        column = advance_column(column, &part) + 1;
        parts.push(part);
        index = next_index;
    }

    (!parts.is_empty()).then(|| parts.join(" "))
}

pub(super) fn format_simple_lexical_tokens(
    tokens: &[perl_parser_core::Token],
    config: &FormatConfig,
) -> Option<String> {
    if tokens.last()?.kind != perl_parser_core::TokenKind::Semicolon {
        return None;
    }

    let semicolon_index = tokens.len() - 1;
    Some(format!("{};", format_simple_lexical_clause(tokens, 0, semicolon_index, config)?))
}

pub(super) fn format_simple_lexical_clause(
    tokens: &[perl_parser_core::Token],
    start: usize,
    end: usize,
    config: &FormatConfig,
) -> Option<String> {
    use perl_parser_core::TokenKind;

    let keyword = match tokens.get(start)?.kind {
        TokenKind::My => "my",
        TokenKind::Our => "our",
        TokenKind::State => "state",
        _ => return None,
    };

    let (variable, next_index) = format_lexical_target_tokens(tokens, start + 1)?;
    if next_index == end {
        Some(format!("{keyword} {variable}"))
    } else if tokens[next_index].kind == TokenKind::Assign {
        let prefix = format!("{keyword} {variable} = ");
        let value = format_simple_expression_tokens(
            tokens,
            next_index + 1,
            end,
            config,
            prefix.chars().count(),
        )?;
        Some(format!("{prefix}{value}"))
    } else {
        None
    }
}

pub(super) fn format_lexical_target_tokens(
    tokens: &[perl_parser_core::Token],
    start: usize,
) -> Option<(String, usize)> {
    format_variable_list_tokens(tokens, start).or_else(|| format_variable_tokens(tokens, start))
}

pub(super) fn format_variable_list_tokens(
    tokens: &[perl_parser_core::Token],
    start: usize,
) -> Option<(String, usize)> {
    use perl_parser_core::TokenKind;

    if tokens.get(start)?.kind != TokenKind::LeftParen {
        return None;
    }

    let mut variables = Vec::new();
    let mut index = start + 1;
    if tokens.get(index)?.kind == TokenKind::RightParen {
        return Some(("()".to_string(), index + 1));
    }

    loop {
        let (variable, next_index) = format_variable_tokens(tokens, index)?;
        variables.push(variable);
        index = next_index;

        match tokens.get(index)?.kind {
            TokenKind::Comma => index += 1,
            TokenKind::RightParen => {
                return Some((format!("({})", variables.join(", ")), index + 1));
            }
            _ => return None,
        }
    }
}

pub(super) fn format_variable_tokens(
    tokens: &[perl_parser_core::Token],
    start: usize,
) -> Option<(String, usize)> {
    use perl_parser_core::TokenKind;

    let first = tokens.get(start)?;
    if first.kind == TokenKind::Identifier
        && first.text.chars().next().is_some_and(|ch| matches!(ch, '$' | '@' | '%'))
    {
        return Some((first.text.to_string(), start + 1));
    }

    let sigil = first;
    let name = tokens.get(start + 1)?;
    if !matches!(sigil.kind, TokenKind::ScalarSigil | TokenKind::ArraySigil | TokenKind::HashSigil)
    {
        return None;
    }
    if name.kind != TokenKind::Identifier {
        return None;
    }

    Some((format!("{}{}", sigil.text, name.text), start + 2))
}

pub(super) fn format_simple_return_tokens(
    tokens: &[perl_parser_core::Token],
    config: &FormatConfig,
) -> Option<String> {
    use perl_parser_core::TokenKind;

    if tokens.first()?.kind != TokenKind::Return || tokens.last()?.kind != TokenKind::Semicolon {
        return None;
    }

    let semicolon_index = tokens.len() - 1;
    if semicolon_index == 1 {
        return Some("return;".to_string());
    }

    let prefix = "return ";
    let value = format_simple_expression_tokens(
        tokens,
        1,
        semicolon_index,
        config,
        prefix.chars().count(),
    )?;
    Some(format!("return {value};"))
}

pub(super) fn format_simple_loop_control_tokens(
    tokens: &[perl_parser_core::Token],
) -> Option<String> {
    use perl_parser_core::TokenKind;

    let keyword = match tokens.first()?.kind {
        TokenKind::Next => "next",
        TokenKind::Last => "last",
        TokenKind::Redo => "redo",
        _ => return None,
    };
    if tokens.last()?.kind != TokenKind::Semicolon {
        return None;
    }

    match tokens {
        [_, _] => Some(format!("{keyword};")),
        [_, label, _] if label.kind == TokenKind::Identifier => {
            Some(format!("{keyword} {};", label.text))
        }
        _ => None,
    }
}

pub(super) fn format_simple_assignment_tokens(
    tokens: &[perl_parser_core::Token],
    config: &FormatConfig,
) -> Option<String> {
    use perl_parser_core::TokenKind;

    if tokens.last()?.kind != TokenKind::Semicolon {
        return None;
    }

    let semicolon_index = tokens.len() - 1;
    Some(format!("{};", format_simple_assignment_clause(tokens, 0, semicolon_index, config)?))
}

pub(super) fn format_simple_assignment_clause(
    tokens: &[perl_parser_core::Token],
    start: usize,
    end: usize,
    config: &FormatConfig,
) -> Option<String> {
    use perl_parser_core::TokenKind;

    let (variable, next_index) = format_variable_tokens(tokens, start)?;
    if tokens.get(next_index)?.kind != TokenKind::Assign {
        return None;
    }

    let prefix = format!("{variable} = ");
    let value = format_simple_expression_tokens(
        tokens,
        next_index + 1,
        end,
        config,
        prefix.chars().count(),
    )?;
    Some(format!("{variable} = {value}"))
}

pub(super) fn format_simple_expression_statement_tokens(
    tokens: &[perl_parser_core::Token],
    config: &FormatConfig,
) -> Option<String> {
    use perl_parser_core::TokenKind;

    if tokens.last()?.kind != TokenKind::Semicolon {
        return None;
    }

    let semicolon_index = tokens.len() - 1;
    let (call, next_index) = format_simple_call_tokens(tokens, 0, config, 0)?;
    (next_index == semicolon_index).then(|| format!("{call};"))
}

pub(super) fn format_simple_condition_tokens(
    tokens: &[perl_parser_core::Token],
    start: usize,
    config: &FormatConfig,
) -> Option<(String, usize)> {
    use perl_parser_core::TokenKind;

    let end = tokens[start..]
        .iter()
        .position(|token| token.kind == TokenKind::RightParen)
        .map(|offset| start + offset)?;
    let condition_config = FormatConfig { line_width: u32::MAX, ..config.clone() };
    let condition = format_simple_expression_tokens(tokens, start, end, &condition_config, 0)?;
    Some((condition, end))
}

pub(super) fn format_simple_expression_tokens(
    tokens: &[perl_parser_core::Token],
    start: usize,
    end: usize,
    config: &FormatConfig,
    start_column: usize,
) -> Option<String> {
    let (left, next_index) = format_simple_atom_tokens(tokens, start, config, start_column)?;
    if next_index == end {
        return Some(left);
    }

    let operator = simple_binary_operator_text(tokens.get(next_index)?)?;
    let right_column = advance_column(start_column, &left) + operator.chars().count() + 2;
    let (right, final_index) =
        format_simple_atom_tokens(tokens, next_index + 1, config, right_column)?;
    (final_index == end).then(|| format!("{left} {operator} {right}"))
}

pub(super) fn format_simple_atom_tokens(
    tokens: &[perl_parser_core::Token],
    start: usize,
    config: &FormatConfig,
    start_column: usize,
) -> Option<(String, usize)> {
    if let Some((method_call, next_index)) =
        format_simple_method_call_tokens(tokens, start, config, start_column)
    {
        return Some((method_call, next_index));
    }

    if let Some((variable, next_index)) = format_variable_tokens(tokens, start) {
        return Some((variable, next_index));
    }

    if let Some((call, next_index)) = format_simple_call_tokens(tokens, start, config, start_column)
    {
        return Some((call, next_index));
    }

    if let Some((list, next_index)) = format_simple_list_tokens(tokens, start, config, start_column)
    {
        return Some((list, next_index));
    }

    if let Some((hash, next_index)) = format_simple_hash_tokens(tokens, start, config, start_column)
    {
        return Some((hash, next_index));
    }

    let token = tokens.get(start)?;
    let value = simple_value_text(token)?;
    Some((value.to_string(), start + 1))
}

pub(super) fn format_simple_method_call_tokens(
    tokens: &[perl_parser_core::Token],
    start: usize,
    config: &FormatConfig,
    start_column: usize,
) -> Option<(String, usize)> {
    use perl_parser_core::TokenKind;

    let (mut expression, mut index) = format_variable_tokens(tokens, start)?;
    let mut saw_method = false;

    loop {
        if tokens.get(index)?.kind != TokenKind::Arrow {
            break;
        }
        let (method_call, next_index) =
            format_simple_method_call_segment(tokens, index, &expression, config, start_column)?;
        expression = method_call;
        index = next_index;
        saw_method = true;
    }

    saw_method.then_some((expression, index))
}

pub(super) fn format_simple_method_call_segment(
    tokens: &[perl_parser_core::Token],
    arrow_index: usize,
    receiver: &str,
    config: &FormatConfig,
    start_column: usize,
) -> Option<(String, usize)> {
    use perl_parser_core::TokenKind;

    if tokens.get(arrow_index)?.kind != TokenKind::Arrow {
        return None;
    }

    let method = tokens.get(arrow_index + 1)?;
    if method.kind != TokenKind::Identifier
        || tokens.get(arrow_index + 2)?.kind != TokenKind::LeftParen
    {
        return None;
    }

    let mut args = Vec::new();
    let mut index = arrow_index + 3;
    if tokens.get(index)?.kind == TokenKind::RightParen {
        return Some((format!("{receiver}->{}()", method.text), index + 1));
    }

    let open = format!("{receiver}->{}(", method.text);
    let mut arg_column = start_column + open.chars().count();
    loop {
        let (arg, next_index) = format_simple_atom_tokens(tokens, index, config, arg_column)?;
        arg_column = advance_column(arg_column, &arg) + 2;
        args.push(arg);
        index = next_index;

        match tokens.get(index)?.kind {
            TokenKind::Comma => index += 1,
            TokenKind::RightParen => {
                return Some((
                    render_delimited_doc(&open, ")", &args, config, start_column),
                    index + 1,
                ));
            }
            _ => return None,
        }
    }
}

pub(super) fn format_simple_call_tokens(
    tokens: &[perl_parser_core::Token],
    start: usize,
    config: &FormatConfig,
    start_column: usize,
) -> Option<(String, usize)> {
    use perl_parser_core::TokenKind;

    let name = tokens.get(start)?;
    if name.kind != TokenKind::Identifier || tokens.get(start + 1)?.kind != TokenKind::LeftParen {
        return None;
    }

    let mut args = Vec::new();
    let mut index = start + 2;
    if tokens.get(index)?.kind == TokenKind::RightParen {
        return Some((format!("{}()", name.text), index + 1));
    }

    let open = format!("{}(", name.text);
    let mut arg_column = start_column + open.chars().count();
    loop {
        let (arg, next_index) = format_simple_atom_tokens(tokens, index, config, arg_column)?;
        arg_column = advance_column(arg_column, &arg) + 2;
        args.push(arg);
        index = next_index;

        match tokens.get(index)?.kind {
            TokenKind::Comma => index += 1,
            TokenKind::RightParen => {
                return Some((
                    render_delimited_doc(&open, ")", &args, config, start_column),
                    index + 1,
                ));
            }
            _ => return None,
        }
    }
}

pub(super) fn format_simple_list_tokens(
    tokens: &[perl_parser_core::Token],
    start: usize,
    config: &FormatConfig,
    start_column: usize,
) -> Option<(String, usize)> {
    use perl_parser_core::TokenKind;

    if tokens.get(start)?.kind != TokenKind::LeftParen {
        return None;
    }

    let mut items = Vec::new();
    let mut index = start + 1;
    if tokens.get(index)?.kind == TokenKind::RightParen {
        return Some(("()".to_string(), index + 1));
    }

    let mut item_column = start_column + 1;
    loop {
        let (item, next_index) = format_simple_atom_tokens(tokens, index, config, item_column)?;
        item_column = advance_column(item_column, &item) + 2;
        items.push(item);
        index = next_index;

        match tokens.get(index)?.kind {
            TokenKind::Comma => index += 1,
            TokenKind::RightParen => {
                return Some((
                    render_delimited_doc("(", ")", &items, config, start_column),
                    index + 1,
                ));
            }
            _ => return None,
        }
    }
}

pub(super) fn format_simple_hash_tokens(
    tokens: &[perl_parser_core::Token],
    start: usize,
    config: &FormatConfig,
    start_column: usize,
) -> Option<(String, usize)> {
    use perl_parser_core::TokenKind;

    if tokens.get(start)?.kind != TokenKind::LeftBrace {
        return None;
    }

    let mut pairs = Vec::new();
    let mut index = start + 1;
    if tokens.get(index)?.kind == TokenKind::RightBrace {
        return Some(("{}".to_string(), index + 1));
    }

    loop {
        let key = format_simple_hash_key_token(tokens.get(index)?)?;
        index += 1;
        if tokens.get(index)?.kind != TokenKind::FatArrow {
            return None;
        }
        index += 1;

        let value_column = start_column + 1 + key.chars().count() + " => ".len();
        let (value, next_index) = format_simple_atom_tokens(tokens, index, config, value_column)?;
        pairs.push((key, value));
        index = next_index;

        match tokens.get(index)?.kind {
            TokenKind::Comma => index += 1,
            TokenKind::RightBrace => {
                return Some((render_simple_hash_doc(&pairs, config, start_column), index + 1));
            }
            _ => return None,
        }
    }
}

pub(super) fn format_simple_hash_key_token(token: &perl_parser_core::Token) -> Option<String> {
    simple_value_text(token).map(str::to_string)
}

pub(super) fn render_simple_hash_doc(
    pairs: &[(String, String)],
    config: &FormatConfig,
    start_column: usize,
) -> String {
    let items = pairs.iter().map(|(key, value)| format!("{key} => {value}")).collect::<Vec<_>>();
    render_delimited_doc("{", "}", &items, config, start_column)
}

pub(super) fn render_delimited_doc(
    open: &str,
    close: &str,
    items: &[String],
    config: &FormatConfig,
    start_column: usize,
) -> String {
    let render_config = config_for_start_column(config, start_column);
    let mut parts = vec![FormatDoc::text(open)];
    if !items.is_empty() {
        let mut item_docs = vec![FormatDoc::if_break(FormatDoc::SoftLine, FormatDoc::text(""))];
        for (index, item) in items.iter().enumerate() {
            if index > 0 {
                item_docs.push(FormatDoc::text(","));
                item_docs.push(FormatDoc::SoftLine);
            }
            item_docs.push(FormatDoc::text(item));
        }
        if config.trailing_comma == TrailingComma::AddWhenWrapped {
            item_docs.push(FormatDoc::if_break(FormatDoc::text(","), FormatDoc::text("")));
        }
        parts.push(FormatDoc::indent(item_docs));
        parts.push(FormatDoc::if_break(FormatDoc::SoftLine, FormatDoc::text("")));
    }
    parts.push(FormatDoc::text(close));
    FormatDoc::group(parts).render(&render_config)
}

pub(super) fn config_for_start_column(config: &FormatConfig, start_column: usize) -> FormatConfig {
    if config.line_width == u32::MAX {
        return config.clone();
    }

    let remaining = (config.line_width as usize).saturating_sub(start_column).max(1);
    FormatConfig { line_width: remaining.min(u32::MAX as usize) as u32, ..config.clone() }
}

pub(super) fn advance_column(start_column: usize, text: &str) -> usize {
    if let Some((_, tail)) = text.rsplit_once('\n') {
        tail.chars().count()
    } else {
        start_column + text.chars().count()
    }
}

pub(super) fn simple_binary_operator_text(token: &perl_parser_core::Token) -> Option<&str> {
    use perl_parser_core::TokenKind;

    matches!(
        token.kind,
        TokenKind::Plus
            | TokenKind::Minus
            | TokenKind::Star
            | TokenKind::Percent
            | TokenKind::Dot
            | TokenKind::Equal
            | TokenKind::NotEqual
            | TokenKind::Less
            | TokenKind::LessEqual
            | TokenKind::Greater
            | TokenKind::GreaterEqual
            | TokenKind::StringCompare
            | TokenKind::Spaceship
            | TokenKind::And
            | TokenKind::Or
            | TokenKind::DefinedOr
            | TokenKind::WordAnd
            | TokenKind::WordOr
    )
    .then_some(token.text.as_ref())
}

pub(super) fn indent_unit(config: &FormatConfig) -> String {
    if config.use_tabs { "\t".to_string() } else { " ".repeat(config.indent_width as usize) }
}

pub(super) fn simple_value_text(token: &perl_parser_core::Token) -> Option<&str> {
    use perl_parser_core::TokenKind;

    matches!(token.kind, TokenKind::Number | TokenKind::String | TokenKind::Identifier)
        .then_some(token.text.as_ref())
}

use perl_parser_core::{NodeKind, Parser};

fn check_import(keyword: &str, quote: &str) -> Result<(), String> {
    let directive = format!("{keyword} Example 'key' => {quote}");
    let source = format!("{directive}; my $after = 7;");
    let mut parser = Parser::new(&source);
    let ast = parser.parse().map_err(|error| format!("{source:?}: {error:?}"))?;
    if !parser.errors().is_empty() {
        return Err(format!("{source:?}: {:?}", parser.errors()));
    }
    let NodeKind::Program { statements } = &ast.kind else {
        return Err("missing program".into());
    };
    let [import, suffix] = statements.as_slice() else {
        return Err(format!("lost statement boundary: {statements:?}"));
    };
    let args = match (&import.kind, keyword) {
        (NodeKind::Use { module, args, .. }, "use") | (NodeKind::No { module, args, .. }, "no")
            if module == "Example" =>
        {
            args
        }
        _ => return Err(format!("wrong directive: {import:?}")),
    };
    if args != &["'key'", quote]
        || import.location.start != 0
        || import.location.end != directive.len()
    {
        return Err(format!("wrong raw arguments or directive extent: {import:?}"));
    }
    let NodeKind::VariableDeclaration { variable, initializer, .. } = &suffix.kind else {
        return Err(format!("lost declaration: {suffix:?}"));
    };
    if !matches!(&variable.kind, NodeKind::Variable { sigil, name } if sigil == "$" && name == "after")
        || !matches!(initializer.as_deref().map(|node| &node.kind), Some(NodeKind::Number { value }) if value == "7")
        || source.get(variable.location.start..variable.location.end) != Some("$after")
        || suffix.location.start != directive.len() + 2
    {
        return Err(format!("changed declaration suffix: {suffix:?}"));
    }
    Ok(())
}

#[test]
fn use_backslash_q_and_qq_preserve_import_and_suffix() -> Result<(), String> {
    for quote in [r"q\foo\", r"qq\foo\", r"q\\", r"qq\\"] {
        check_import("use", quote)?;
    }
    Ok(())
}

#[test]
fn no_backslash_q_and_qq_preserve_import_and_suffix() -> Result<(), String> {
    for quote in [r"q\foo\", r"qq\foo\", r"q\\", r"qq\\"] {
        check_import("no", quote)?;
    }
    Ok(())
}

#[test]
fn backslash_missing_closer_is_not_clean() -> Result<(), String> {
    for keyword in ["use", "no"] {
        for quote in [r"q\foo", r"qq\foo"] {
            let source = format!("{keyword} Example 'key' => {quote}");
            let mut parser = Parser::new(&source);
            if parser.parse().is_ok() && parser.errors().is_empty() {
                return Err(format!("missing closer accepted: {source:?}"));
            }
        }
    }
    Ok(())
}

#[test]
fn escaped_slash_and_paired_quote_controls_preserve_boundaries() -> Result<(), String> {
    for keyword in ["use", "no"] {
        for quote in [r"q/a\/b/", r"qq/a\/b/", r"q{a{b}c}", r"qq{a\}b}"] {
            check_import(keyword, quote)?;
        }
    }
    Ok(())
}

#[test]
fn multiline_backslash_qw_preserves_declaration_shaped_content() -> Result<(), String> {
    for keyword in ["use", "no"] {
        check_import(keyword, "qw\\a\nmy b\\")?;
    }
    Ok(())
}

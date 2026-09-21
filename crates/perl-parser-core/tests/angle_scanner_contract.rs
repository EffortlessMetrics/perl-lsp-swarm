//! Direct production-path baseline for #16224; budget API controls follow its implementation.
use perl_parser_core::{Node, NodeKind, Parser, RecoverySalvageClass, RecoverySalvageProfile};

type R = Result<(), Box<dyn std::error::Error>>;

fn collect<'a>(node: &'a Node, predicate: &impl Fn(&NodeKind) -> bool, out: &mut Vec<&'a Node>) {
    if predicate(&node.kind) {
        out.push(node);
    }
    for child in node.children() {
        collect(child, predicate, out);
    }
}

#[test]
fn closed_globs_preserve_source_content_and_following_declaration() -> R {
    for body in [
        "a;b", "a,b", "a)b", "a b", r"a\>b", "λ.pm", "a#b", "a\rb", " STDIN ", " $fh ", "a.pm",
        "a-b", "$$$", "℘", "℮", "a·b", "$foo:bar", "\u{301}a", "🚀", "1\u{301}",
    ] {
        let source = format!("# λ\nmy $value = <{body}>; my $after = 7;");
        let output = Parser::new(&source).parse_with_recovery();
        if RecoverySalvageProfile::from_parse(
            &output.ast,
            &output.diagnostics,
            output.terminated_early(),
        )
        .class
            != RecoverySalvageClass::Clean
        {
            return Err(format!("non-clean closed angle {body:?}: {:?}", output.diagnostics).into());
        }
        let mut globs = Vec::new();
        collect(&output.ast, &|kind| matches!(kind, NodeKind::Glob { .. }), &mut globs);
        if globs.len() != 1 {
            return Err(format!("expected one glob for {body:?}").into());
        }
        let glob = globs.first().ok_or("missing glob")?;
        let NodeKind::Glob { pattern } = &glob.kind else {
            return Err("wrong kind".into());
        };
        if pattern != body
            || source.get(glob.location.start..glob.location.end)
                != Some(format!("<{body}>").as_str())
        {
            return Err(format!("lost raw angle body/range: {body:?} -> {pattern:?}").into());
        }
        require_after(&output.ast, &source)?;
    }
    Ok(())
}

fn require_after(ast: &Node, source: &str) -> R {
    let mut found = Vec::new();
    collect(
        ast,
        &|kind| {
            matches!(kind, NodeKind::VariableDeclaration { variable, initializer: Some(value), .. }
        if matches!(&variable.kind, NodeKind::Variable { sigil, name } if sigil == "$" && name == "after")
        && matches!(&value.kind, NodeKind::Number { value } if value == "7"))
        },
        &mut found,
    );
    if found.len() != 1 {
        return Err("lost/duplicated following initialized declaration".into());
    }
    let node = found.first().ok_or("missing following declaration")?;
    let NodeKind::VariableDeclaration { variable, .. } = &node.kind else {
        return Err("wrong following node".into());
    };
    let expected = source.find("$after").ok_or("fixture suffix missing")?;
    if variable.location.start != expected || variable.location.end != expected + 6 {
        return Err("following variable range shifted".into());
    }
    Ok(())
}

#[test]
fn proven_missing_closer_preserves_first_statement_boundary() -> R {
    for ending in ["", "\n", "\r\n"] {
        let source = format!("my $value = <STDIN; my $after = 7;{ending}");
        let output = Parser::new(&source).parse_with_recovery();
        if output.stop_cause().is_some() || output.diagnostics.is_empty() {
            return Err(
                format!("missing closer has wrong disposition: {:?}", output.diagnostics).into()
            );
        }
        let opener = source.find('<').ok_or("fixture opener missing")?;
        let boundary = source.find(';').ok_or("fixture recovery missing")?;
        if !output.diagnostics.iter().any(|error| {
            matches!(error, perl_parser_core::ParseError::AngleScan {
            error: perl_lexer::LexerError::UnterminatedAngle { position, recovery }
        } if *position == opener && *recovery == boundary)
        }) {
            return Err(format!("missing typed malformed range: {:?}", output.diagnostics).into());
        }
        require_after(&output.ast, &source)?;
    }
    Ok(())
}

#[test]
fn bare_and_scalar_angle_handles_follow_perl_inputsymbol_classification() -> R {
    if unicode_ident::UNICODE_VERSION != (17, 0, 0) {
        return Err("regenerate Perl Word exclusions for new Unicode tables".into());
    }
    for body in [
        "foo",
        "a",
        "Foo",
        "foo_bar",
        "foo1",
        "1foo",
        "_",
        "Foo::bar",
        "::foo",
        "foo::",
        "foo::1",
        "$fh",
        "$1foo",
        "$foo::bar",
        "::",
        "::::",
        "$::",
        "foo::::bar",
        "$_",
        "$1",
        "λ",
        "\u{88f}",
        "1λ",
        "λ::β",
        "a\u{301}",
    ] {
        let source = format!("# λ\nmy $value = <{body}>; my $after = 7;");
        let output = Parser::new(&source).parse_with_recovery();
        if RecoverySalvageProfile::from_parse(
            &output.ast,
            &output.diagnostics,
            output.terminated_early(),
        )
        .class
            != RecoverySalvageClass::Clean
        {
            return Err(format!("non-clean handle {body:?}: {:?}", output.diagnostics).into());
        }
        let mut nodes = Vec::new();
        collect(
            &output.ast,
            &|kind| matches!(kind, NodeKind::Readline { .. } | NodeKind::Glob { .. }),
            &mut nodes,
        );
        if nodes.len() != 1 {
            return Err(format!("expected exactly one angle node for {body:?}").into());
        }
        let node = nodes.first().ok_or("missing angle node")?;
        if !matches!(&node.kind, NodeKind::Readline { filehandle: Some(handle) } if handle == body)
            || source.get(node.location.start..node.location.end)
                != Some(format!("<{body}>").as_str())
        {
            return Err(format!(
                "wrong handle classification/raw range for {body:?}: {:?}",
                node.kind
            )
            .into());
        }
        require_after(&output.ast, &source)?;
    }
    Ok(())
}

#[test]
fn missing_angle_closer_preserves_caller_delimiter_and_following_statement() -> R {
    for (source, delimiter) in [
        ("# λ\nmy $value = f(<oops, 99); my $after = 7;", ','),
        ("# λ\nmy $value = f(<oops); my $after = 7;", ')'),
    ] {
        let output = Parser::new(source).parse_with_recovery();
        let opener = source.find('<').ok_or("fixture opener missing")?;
        let boundary = source.find(delimiter).ok_or("fixture caller delimiter missing")?;
        if output.stop_cause().is_some() || !output.diagnostics.iter().any(|error| matches!(error,
            perl_parser_core::ParseError::AngleScan { error: perl_lexer::LexerError::UnterminatedAngle { position, recovery } }
            if *position == opener && *recovery == boundary)) {
            return Err(format!("wrong caller recovery disposition: {:?}", output.diagnostics).into());
        }
        if output.diagnostics.len() != 1
            || !output.diagnostics.first().is_some_and(|error| error.blocks_clean_parse())
        {
            return Err("caller recovery duplicated or softened diagnostics".into());
        }
        let mut calls = Vec::new();
        collect(
            &output.ast,
            &|kind| matches!(kind, NodeKind::FunctionCall { name, .. } if name == "f"),
            &mut calls,
        );
        if calls.len() != 1 {
            return Err("lost recovered function call".into());
        }
        let NodeKind::FunctionCall { args, .. } = &calls.first().ok_or("missing call")?.kind else {
            return Err("wrong call node".into());
        };
        let expected_count = if delimiter == ',' { 2 } else { 1 };
        if args.len() != expected_count {
            return Err("lost caller arguments".into());
        }
        let bad = args.first().ok_or("missing malformed argument")?;
        if !matches!(&bad.kind, NodeKind::Error { .. })
            || bad.location.start != opener
            || bad.location.end != boundary
        {
            return Err("wrong bounded angle error argument".into());
        }
        if delimiter == ',' {
            let valid = args.get(1).ok_or("missing valid later argument")?;
            let start = source.find("99").ok_or("fixture later argument missing")?;
            if !matches!(&valid.kind, NodeKind::Number { value } if value == "99")
                || valid.location.start != start
                || valid.location.end != start + 2
            {
                return Err("lost valid later argument identity/range".into());
            }
        }
        require_after(&output.ast, source)?;
    }
    Ok(())
}

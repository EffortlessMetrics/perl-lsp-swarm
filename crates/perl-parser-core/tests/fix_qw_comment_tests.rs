//! `#` inside qw is literal data, even when Perl warns about possible comments.
//! Oracle: Perl 5.40.1; see perldiag's "Possible attempt to put comments in qw() list".

use perl_parser_core::error::{ParseError, RecoveryKind, RecoverySite};
use perl_parser_core::{Node, NodeKind, Parser};

fn parse(source: &str) -> Result<Node, String> {
    let mut parser = Parser::new(source);
    let ast = parser.parse().map_err(|error| format!("{error:?}"))?;
    if !parser.errors().is_empty() {
        return Err(format!("unexpected diagnostics: {:?}", parser.errors()));
    }
    Ok(ast)
}

fn array(node: &Node) -> Result<&Node, String> {
    if matches!(node.kind, NodeKind::ArrayLiteral { .. }) {
        return Ok(node);
    }
    node.children().into_iter().find_map(|child| array(child).ok()).ok_or("missing qw array".into())
}

fn words(node: &Node) -> Result<Vec<&str>, String> {
    let NodeKind::ArrayLiteral { elements } = &array(node)?.kind else {
        return Err("expected qw array".into());
    };
    elements
        .iter()
        .map(|element| match &element.kind {
            NodeKind::String { value, interpolated: false } => Ok(value.as_str()),
            _ => Err(format!("expected literal word: {element:?}")),
        })
        .collect()
}

fn assert_spans(node: &Node, expected: &[(&str, usize, usize)]) -> Result<(), String> {
    let NodeKind::ArrayLiteral { elements } = &array(node)?.kind else {
        return Err("expected qw array".into());
    };
    let actual = elements
        .iter()
        .map(|element| match &element.kind {
            NodeKind::String { value, interpolated: false } => {
                Ok((value.as_str(), element.location.start, element.location.end))
            }
            _ => Err(format!("expected literal word: {element:?}")),
        })
        .collect::<Result<Vec<_>, String>>()?;
    assert_eq!(actual, expected);
    Ok(())
}

#[test]
fn qw_hash_words_are_literal_across_delimiters() -> Result<(), String> {
    for quote in [
        "qw(red # green blue)",
        "qw{red # green blue}",
        "qw[red # green blue]",
        "qw<red # green blue>",
        "qw/red # green blue/",
        "qw (red # green blue)",
        "qw(red # green\nblue)",
        "qw(red # green\r\nblue)",
    ] {
        let source = format!("my @x = {quote};");
        assert_eq!(words(&parse(&source)?)?, ["red", "#", "green", "blue"], "{source:?}");
    }
    assert_eq!(
        words(&parse("my @x = qw(red#green #tag trailing#);")?)?,
        ["red#green", "#tag", "trailing#"]
    );
    assert_eq!(
        words(&parse("my @x = qw(https://example.test/docs#install https://example.test/help);")?)?,
        ["https://example.test/docs#install", "https://example.test/help"]
    );
    Ok(())
}

#[test]
fn qw_literal_hash_corpus_has_exact_words_and_spans() -> Result<(), String> {
    let source = include_str!("../../../test_corpus/qw_literal_hash_words.pl");
    let ast = parse(source)?;
    let NodeKind::Program { statements } = &ast.kind else {
        return Err("expected Program".into());
    };
    assert_eq!(statements.len(), 2, "outside comment must not add AST words");
    assert_spans(
        &statements[0],
        &[
            ("#todo", 14, 19),
            ("red#green", 20, 29),
            ("#", 30, 31),
            ("blue", 32, 36),
            ("red", 37, 40),
        ],
    )?;
    assert_eq!(
        (array(&statements[0])?.location.start, array(&statements[0])?.location.end),
        (11, 41)
    );
    assert_eq!((statements[1].location.start, statements[1].location.end), (65, 78));
    assert!(
        matches!(&statements[1].kind, NodeKind::VariableDeclaration { variable, initializer: Some(value), .. }
        if matches!(&variable.kind, NodeKind::Variable { sigil, name } if sigil == "$" && name == "after")
            && matches!(&value.kind, NodeKind::Number { value } if value == "1"))
    );
    Ok(())
}

#[test]
fn qw_hash_words_keep_utf8_and_repeated_word_spans() -> Result<(), String> {
    assert_spans(
        &parse("my @x = qw(é#tag # é#tag); my $after = 1;")?,
        &[("é#tag", 11, 17), ("#", 18, 19), ("é#tag", 20, 26)],
    )
}

#[test]
fn qw_prefix_comments_do_not_steal_hash_word_spans() -> Result<(), String> {
    assert_spans(&parse("my @x = qw # # fake\n(# #tag);")?, &[("#", 21, 22), ("#tag", 23, 27)])
}

#[test]
fn qw_directives_preserve_hash_words() -> Result<(), String> {
    for quote in ["qw(red # green blue)", "qw (red # green blue)", "qw{red # green blue}"] {
        for directive in ["use", "no"] {
            let source = format!("{directive} Example {quote}; my $after = 1;");
            let ast = parse(&source)?;
            let NodeKind::Program { statements } = &ast.kind else {
                return Err("expected Program".into());
            };
            assert_eq!(statements.len(), 2);
            let args = match &statements[0].kind {
                NodeKind::Use { args, .. } | NodeKind::No { args, .. } => args,
                _ => return Err(format!("expected directive: {:?}", statements[0])),
            };
            assert_eq!(args, &["qw(red # green blue)"], "{source:?}");
        }
    }
    Ok(())
}

#[test]
fn qw_outside_comments_and_empty_list_are_controls() -> Result<(), String> {
    assert_eq!(
        words(&parse("my @x = qw(red green); # blue qw(ignored)\nmy $after = 1;")?)?,
        ["red", "green"]
    );
    assert_eq!(words(&parse("my @x = qw();")?)?, Vec::<&str>::new());
    Ok(())
}

#[test]
fn qw_unclosed_hash_words_recover_before_following_statement() -> Result<(), String> {
    let source = "my @w = qw(red # green\nprint 'tail';";
    let mut parser = Parser::new(source);
    let ast = parser.parse().map_err(|error| format!("{error:?}"))?;
    assert!(!parser.errors().is_empty(), "unclosed list must remain diagnosed");
    assert!(parser.errors().iter().any(|error| matches!(
        error,
        ParseError::Recovered {
            site: RecoverySite::ArgList,
            kind: RecoveryKind::InsertedCloser,
            location: 23
        }
    )));
    let NodeKind::Program { statements } = &ast.kind else {
        return Err("expected Program".into());
    };
    assert_eq!(statements.len(), 2);
    assert_spans(&statements[0], &[("red", 11, 14), ("#", 15, 16), ("green", 17, 22)])?;
    assert_eq!(
        (array(&statements[0])?.location.start, array(&statements[0])?.location.end),
        (8, 23)
    );
    assert_eq!((statements[1].location.start, statements[1].location.end), (23, 35));
    assert!(statements[1].to_sexp().contains("tail"));
    assert_eq!(words(&parse("my @w = qw(red # green); print 'tail';")?)?, ["red", "#", "green"]);
    Ok(())
}

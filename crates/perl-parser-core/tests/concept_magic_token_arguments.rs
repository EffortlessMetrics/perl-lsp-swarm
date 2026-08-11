//! Parser contracts for Perl's compile-time magic tokens (#6670).
//!
//! These tests prove parser identity and source geometry only. They do not claim
//! runtime values for the tokens or version availability beyond parser acceptance.

use std::collections::BTreeMap;

use perl_parser_core::{Node, NodeKind, Parser};

const MAGIC_TOKENS: [&str; 5] = ["__FILE__", "__LINE__", "__PACKAGE__", "__SUB__", "__CLASS__"];

fn parse_clean(source: &str) -> Result<Node, String> {
    let mut parser = Parser::new(source);
    let ast = parser.parse().map_err(|error| format!("parse failed: {error:?}"))?;
    if parser.errors().is_empty() {
        Ok(ast)
    } else {
        Err(format!("expected a clean parse, got diagnostics: {:?}", parser.errors()))
    }
}

fn walk(node: &Node, visit: &mut impl FnMut(&Node)) {
    visit(node);
    for child in node.children() {
        walk(child, visit);
    }
}

#[test]
fn magic_tokens_remain_nullary_calls_in_value_and_argument_positions() -> Result<(), String> {
    let source = concat!(
        "my @metadata = (__FILE__, __LINE__, __PACKAGE__, __SUB__, __CLASS__);\n",
        "die __FILE__, __LINE__;\n",
    );
    let ast = parse_clean(source)?;
    let mut observed: BTreeMap<String, Vec<String>> = BTreeMap::new();

    walk(&ast, &mut |node| {
        if let NodeKind::FunctionCall { name, args } = &node.kind
            && MAGIC_TOKENS.contains(&name.as_str())
            && args.is_empty()
            && let Some(text) = source.get(node.location.start..node.location.end)
        {
            observed.entry(name.clone()).or_default().push(text.to_owned());
        }
    });

    for token in MAGIC_TOKENS {
        let spans = observed
            .get(token)
            .ok_or_else(|| format!("{token} lost its nullary FunctionCall identity"))?;
        assert!(spans.iter().all(|span| span == token), "{token} has incorrect source spans: {spans:?}");
    }
    assert_eq!(observed["__FILE__"].len(), 2);
    assert_eq!(observed["__LINE__"].len(), 2);
    assert_eq!(observed["__PACKAGE__"].len(), 1);
    assert_eq!(observed["__SUB__"].len(), 1);
    assert_eq!(observed["__CLASS__"].len(), 1);
    Ok(())
}

#[test]
fn quoted_magic_token_spellings_do_not_become_nullary_calls() -> Result<(), String> {
    let source = "my @labels = (\"__FILE__\", q{__LINE__}, '__PACKAGE__');";
    let ast = parse_clean(source)?;
    let mut fabricated = Vec::new();

    walk(&ast, &mut |node| {
        if let NodeKind::FunctionCall { name, .. } = &node.kind
            && MAGIC_TOKENS.contains(&name.as_str())
        {
            fabricated.push(name.clone());
        }
    });

    assert!(fabricated.is_empty(), "quoted spellings fabricated magic-token calls: {fabricated:?}");
    Ok(())
}

#[test]
fn magic_tokens_stop_before_comma_fat_arrow_and_closing_delimiters() -> Result<(), String> {
    let source = concat!(
        "my %metadata = (file => __FILE__, line => __LINE__);\n",
        "my @nested = [__PACKAGE__, { sub_name => __SUB__ }];\n",
    );
    let ast = parse_clean(source)?;
    let mut observed = Vec::new();

    walk(&ast, &mut |node| {
        if let NodeKind::FunctionCall { name, args } = &node.kind
            && MAGIC_TOKENS.contains(&name.as_str())
            && args.is_empty()
        {
            observed.push(name.clone());
        }
    });
    observed.sort();

    assert_eq!(observed, vec!["__FILE__", "__LINE__", "__PACKAGE__", "__SUB__"]);
    Ok(())
}

//! Parser contracts for Perl's compile-time magic tokens (#6670).
//!
//! These tests prove parser identity and source geometry only. They do not claim
//! runtime values for the tokens or version availability beyond parser acceptance.

use std::collections::BTreeMap;

use perl_parser_core::{Node, NodeKind, Parser};

const MAGIC_TOKENS: [&str; 5] = ["__FILE__", "__LINE__", "__PACKAGE__", "__SUB__", "__CLASS__"];
const QUOTED_MAGIC_LITERALS: [&str; 3] = ["\"__FILE__\"", "'__PACKAGE__'", "q{__LINE__}"];

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

fn node_shape(node: &Node) -> &'static str {
    match &node.kind {
        NodeKind::FunctionCall { .. } => "FunctionCall",
        NodeKind::String { .. } => "String",
        NodeKind::Identifier { .. } => "Identifier",
        _ => "Other",
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
    let mut die_arguments = None;

    walk(&ast, &mut |node| {
        if let NodeKind::FunctionCall { name, args } = &node.kind
            && MAGIC_TOKENS.contains(&name.as_str())
            && args.is_empty()
            && let Some(text) = source.get(node.location.start..node.location.end)
        {
            observed.entry(name.clone()).or_default().push(text.to_owned());
        }
    });

    walk(&ast, &mut |node| {
        if let NodeKind::FunctionCall { name, args } = &node.kind
            && name == "die"
        {
            die_arguments = args
                .iter()
                .map(|argument| {
                    let NodeKind::FunctionCall { name, args } = &argument.kind else {
                        return None;
                    };
                    if !args.is_empty() {
                        return None;
                    }
                    source
                        .get(argument.location.start..argument.location.end)
                        .map(|text| (name.clone(), text.to_owned()))
                })
                .collect::<Option<Vec<_>>>();
        }
    });

    for token in MAGIC_TOKENS {
        let spans = observed
            .get(token)
            .ok_or_else(|| format!("{token} lost its nullary FunctionCall identity"))?;
        assert!(
            spans.iter().all(|span| span == token),
            "{token} has incorrect source spans: {spans:?}"
        );
    }
    assert_eq!(observed["__FILE__"].len(), 2);
    assert_eq!(observed["__LINE__"].len(), 2);
    assert_eq!(observed["__PACKAGE__"].len(), 1);
    assert_eq!(observed["__SUB__"].len(), 1);
    assert_eq!(observed["__CLASS__"].len(), 1);
    assert_eq!(
        die_arguments,
        Some(vec![
            ("__FILE__".to_owned(), "__FILE__".to_owned()),
            ("__LINE__".to_owned(), "__LINE__".to_owned()),
        ])
    );
    Ok(())
}

#[test]
fn quoted_magic_token_spellings_remain_literals_not_nullary_calls() -> Result<(), String> {
    let source = "my @labels = (\"__FILE__\", q{__LINE__}, '__PACKAGE__');";
    let ast = parse_clean(source)?;
    let mut fabricated = Vec::new();
    let mut literals = Vec::new();

    walk(&ast, &mut |node| match &node.kind {
        NodeKind::FunctionCall { name, .. } if MAGIC_TOKENS.contains(&name.as_str()) => {
            fabricated.push(name.clone());
        }
        NodeKind::String { value, .. } if QUOTED_MAGIC_LITERALS.contains(&value.as_str()) => {
            if let Some(text) = source.get(node.location.start..node.location.end) {
                literals.push((text.to_owned(), value.clone()));
            }
        }
        _ => {}
    });
    literals.sort();

    assert_eq!(
        literals,
        vec![
            ("\"__FILE__\"".to_string(), "\"__FILE__\"".to_string()),
            ("'__PACKAGE__'".to_string(), "'__PACKAGE__'".to_string()),
            ("q{__LINE__}".to_string(), "q{__LINE__}".to_string()),
        ]
    );
    assert!(fabricated.is_empty(), "quoted spellings fabricated magic-token calls: {fabricated:?}");
    Ok(())
}

#[test]
fn magic_tokens_stop_before_comma_fat_arrow_and_closing_delimiters() -> Result<(), String> {
    let source = concat!(
        "my %metadata = (__FILE__ => line, file => __FILE__, line => __LINE__);\n",
        "my @nested = [__PACKAGE__, { sub_name => __SUB__ }];\n",
    );
    let ast = parse_clean(source)?;
    let mut observed = Vec::new();
    let mut hash_pairs = None;

    walk(&ast, &mut |node| {
        if let NodeKind::FunctionCall { name, args } = &node.kind
            && MAGIC_TOKENS.contains(&name.as_str())
            && args.is_empty()
        {
            observed.push(name.clone());
        }
        if let NodeKind::HashLiteral { pairs } = &node.kind {
            let contracts = pairs
                .iter()
                .map(|(key, value)| {
                    Some((
                        node_shape(key),
                        source.get(key.location.start..key.location.end)?.to_owned(),
                        node_shape(value),
                        source.get(value.location.start..value.location.end)?.to_owned(),
                    ))
                })
                .collect::<Option<Vec<_>>>();
            if contracts
                .as_ref()
                .is_some_and(|pairs| pairs.iter().any(|(_, key, _, _)| key == "__FILE__"))
            {
                hash_pairs = contracts;
            }
        }
    });
    observed.sort();

    // The fat-arrow operator autoquotes the word on its left (per perlop): `__FILE__ => x`
    // produces a hash key that is the string "__FILE__", not the runtime magic-token value.
    // Only the `__FILE__` appearing on the *value* side of a fat-arrow pair is emitted as
    // a nullary FunctionCall; the one in key position is converted to a String node.
    assert_eq!(observed, vec!["__FILE__", "__LINE__", "__PACKAGE__", "__SUB__"]);
    assert_eq!(
        hash_pairs,
        Some(vec![
            ("String", "__FILE__".to_owned(), "Identifier", "line".to_owned()),
            ("String", "file".to_owned(), "FunctionCall", "__FILE__".to_owned()),
            ("String", "line".to_owned(), "FunctionCall", "__LINE__".to_owned()),
        ])
    );
    Ok(())
}

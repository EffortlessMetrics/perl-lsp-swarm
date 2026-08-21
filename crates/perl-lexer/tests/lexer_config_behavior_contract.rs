//! Public behavior contract for `LexerConfig` and feature-independent tokenization.
//!
//! These tests exercise `PerlLexer::with_config` rather than treating field
//! names as evidence. They distinguish observable effects, compatibility fields,
//! shared-cursor thresholds, and the currently empty `simd` Cargo feature.

use std::sync::Arc;

use perl_lexer::{
    Checkpointable, LexerConfig, LocalSymbolTable, PerlLexer, StringPart, Token, TokenType,
};

type R<T = ()> = Result<T, Box<dyn std::error::Error>>;
type TokenSignature = (TokenType, Arc<str>, usize, usize);

fn missing(message: impl Into<String>) -> Box<dyn std::error::Error> {
    std::io::Error::other(message.into()).into()
}

fn first_token(input: &str, config: LexerConfig) -> R<Token> {
    PerlLexer::with_config(input, config)
        .next_token()
        .ok_or_else(|| missing("expected one lexer token"))
}

fn signatures(input: &str, config: LexerConfig) -> Vec<TokenSignature> {
    let mut lexer = PerlLexer::with_config(input, config);
    collect_remaining(&mut lexer)
}

fn collect_remaining(lexer: &mut PerlLexer<'_>) -> Vec<TokenSignature> {
    lexer
        .collect_tokens()
        .into_iter()
        .map(|token| (token.token_type, token.text, token.start, token.end))
        .collect()
}

fn assert_symbol_table_invariant(label: &str, input: &str) {
    let without_table = signatures(input, LexerConfig::default());
    let with_table = signatures(
        input,
        LexerConfig {
            symbol_table: Some(LocalSymbolTable::scan_subs(input)),
            ..LexerConfig::default()
        },
    );

    assert_eq!(without_table, with_table, "symbol table changed {label}");
}

#[test]
fn interpolation_switch_has_an_exact_legacy_segmentation_contract() -> R {
    let cases = [
        (
            r#""hello $name""#,
            vec![
                StringPart::Literal(Arc::from("hello ")),
                StringPart::Variable(Arc::from("$name")),
            ],
        ),
        (r#""@items""#, vec![StringPart::Variable(Arc::from("@items"))]),
        (r#""${name}""#, vec![StringPart::Expression(Arc::from("${name}"))]),
        (
            r#""$items[0]""#,
            vec![
                StringPart::Variable(Arc::from("$items")),
                StringPart::ArraySlice(Arc::from("[0]")),
            ],
        ),
        (r#""\$name""#, vec![StringPart::Literal(Arc::from(r"\$name"))]),
        (
            r#""$x:$x""#,
            vec![
                StringPart::Variable(Arc::from("$x")),
                StringPart::Literal(Arc::from(":")),
                StringPart::Variable(Arc::from("$x")),
            ],
        ),
    ];

    for (input, expected_enabled_parts) in cases {
        let enabled = first_token(input, LexerConfig::default())?;
        let TokenType::InterpolatedString(enabled_parts) = &enabled.token_type else {
            return Err(missing(format!("enabled interpolation was not structured for {input:?}")));
        };
        assert_eq!(enabled_parts, &expected_enabled_parts, "enabled parts for {input:?}");
        assert_eq!(enabled.text.as_ref(), input);
        assert_eq!((enabled.start, enabled.end), (0, input.len()));

        let disabled = first_token(
            input,
            LexerConfig { parse_interpolation: false, ..LexerConfig::default() },
        )?;
        let TokenType::InterpolatedString(disabled_parts) = &disabled.token_type else {
            return Err(missing(format!(
                "disabled interpolation changed token shape for {input:?}"
            )));
        };
        let inner = input
            .get(1..input.len().saturating_sub(1))
            .ok_or_else(|| missing("ordinary string fixture lost its quote boundaries"))?;
        assert_eq!(
            disabled_parts,
            &vec![StringPart::Literal(Arc::from(inner))],
            "disabled interpolation must retain one literal part for {input:?}"
        );
        assert_eq!(disabled.text.as_ref(), input);
        assert_eq!((disabled.start, disabled.end), (enabled.start, enabled.end));
    }
    Ok(())
}

#[test]
fn interpolation_switch_does_not_claim_opaque_quote_like_bodies() {
    let input = "qq{hello $name}";
    let enabled = signatures(input, LexerConfig::default());
    let disabled =
        signatures(input, LexerConfig { parse_interpolation: false, ..LexerConfig::default() });

    assert_eq!(enabled, disabled);
    assert!(matches!(enabled.first().map(|token| &token.0), Some(TokenType::QuoteDouble)));
}

#[test]
fn malformed_double_quote_recovery_is_configuration_invariant() {
    let input = "\"unterminated $name";
    let enabled = signatures(input, LexerConfig::default());
    let disabled =
        signatures(input, LexerConfig { parse_interpolation: false, ..LexerConfig::default() });

    assert_eq!(enabled, disabled);
    assert!(matches!(enabled.first().map(|token| &token.0), Some(TokenType::Error(_))));
    assert!(matches!(enabled.last().map(|token| &token.0), Some(TokenType::EOF)));
}

#[test]
fn position_compatibility_field_does_not_change_authoritative_tokens() {
    let input = "my $café = 1;\r\nprint $café;";
    let enabled = signatures(input, LexerConfig::default());
    let disabled =
        signatures(input, LexerConfig { track_positions: false, ..LexerConfig::default() });

    // POSITIONS_ARE_ALWAYS_TRACKED makes `track_positions: false` a no-op; the
    // equality assertion below is the behavioral proof of that contract.
    assert_eq!(enabled, disabled);
    for (token_type, text, start, end) in
        disabled.iter().filter(|token| !matches!(&token.0, TokenType::EOF))
    {
        assert!(start <= end, "reversed token span for {token_type:?}");
        assert_eq!(input.get(*start..*end), Some(text.as_ref()));
    }
}

#[test]
fn shared_lookahead_limit_has_distinct_zero_one_and_two_boundaries() -> R {
    let zero = LexerConfig { max_lookahead: 0, ..LexerConfig::default() };
    let one = LexerConfig { max_lookahead: 1, ..LexerConfig::default() };
    let two = LexerConfig { max_lookahead: 2, ..LexerConfig::default() };

    let qualified_zero = first_token("Foo::bar", zero.clone())?;
    assert!(
        matches!(&qualified_zero.token_type, TokenType::Identifier(name) if name.as_ref() == "Foo")
    );
    let qualified_one = first_token("Foo::bar", one.clone())?;
    assert!(
        matches!(&qualified_one.token_type, TokenType::Identifier(name) if name.as_ref() == "Foo::bar")
    );

    let decimal_zero = first_token(".5", zero)?;
    assert!(
        matches!(&decimal_zero.token_type, TokenType::Operator(operator) if operator.as_ref() == ".")
    );
    let decimal_one = first_token(".5", one.clone())?;
    assert!(
        matches!(&decimal_one.token_type, TokenType::Number(number) if number.as_ref() == ".5")
    );

    let bom_source = "\u{feff}my $x = 1;";
    let bom_blocked = first_token(bom_source, one)?;
    assert_eq!(bom_blocked.start, 0);
    assert!(!matches!(&bom_blocked.token_type, TokenType::Keyword(name) if name.as_ref() == "my"));

    let bom_admitted = first_token(bom_source, two)?;
    assert!(matches!(&bom_admitted.token_type, TokenType::Keyword(name) if name.as_ref() == "my"));
    assert_eq!((bom_admitted.start, bom_admitted.end), (3, 5));
    Ok(())
}

#[test]
fn configured_lookahead_survives_checkpoint_replay() -> R {
    let input = "Foo::bar / 2;";
    for max_lookahead in [0, 1, 2, LexerConfig::DEFAULT_MAX_LOOKAHEAD] {
        let config = LexerConfig { max_lookahead, ..LexerConfig::default() };
        let mut lexer = PerlLexer::with_config(input, config);
        let first = lexer.next_token().ok_or_else(|| missing("missing prefix token"))?;
        assert!(!matches!(&first.token_type, TokenType::EOF));

        let checkpoint = lexer.checkpoint();
        assert!(lexer.can_restore(&checkpoint));
        let uninterrupted = collect_remaining(&mut lexer);
        lexer.restore(&checkpoint);
        let replayed = collect_remaining(&mut lexer);
        assert_eq!(uninterrupted, replayed, "lookahead limit {max_lookahead}");
    }
    Ok(())
}

#[test]
fn symbol_table_changes_only_the_declared_bareword_slash_case() {
    let ambiguous = "builder /pattern/; sub builder { 1 }";
    let heuristic = signatures(ambiguous, LexerConfig::default());
    assert!(heuristic.iter().any(|token| matches!(&token.0, TokenType::Division)));
    assert!(!heuristic.iter().any(|token| matches!(&token.0, TokenType::RegexMatch)));

    let table = LocalSymbolTable::scan_subs(ambiguous);
    let configured =
        signatures(ambiguous, LexerConfig { symbol_table: Some(table), ..LexerConfig::default() });
    assert!(configured.iter().any(|token| matches!(&token.0, TokenType::RegexMatch)));
    assert!(!configured.iter().any(|token| matches!(&token.0, TokenType::Division)));

    let undeclared = "consumer /pattern/; sub builder { 1 }";
    let undeclared_tokens = signatures(undeclared, LexerConfig::default());
    assert!(undeclared_tokens.iter().any(|token| matches!(&token.0, TokenType::Division)));
    assert!(!undeclared_tokens.iter().any(|token| matches!(&token.0, TokenType::RegexMatch)));
    assert_symbol_table_invariant("an undeclared bareword/slash case", undeclared);

    let builtin = "print /pattern/; sub builder { 1 }";
    let builtin_tokens = signatures(builtin, LexerConfig::default());
    assert!(builtin_tokens.iter().any(|token| matches!(&token.0, TokenType::RegexMatch)));
    assert!(!builtin_tokens.iter().any(|token| matches!(&token.0, TokenType::Division)));
    assert_symbol_table_invariant("a builtin-controlled regex", builtin);

    assert_symbol_table_invariant("a method name", "$obj->builder(); sub builder { 1 }");
    assert_symbol_table_invariant("a hash key", "$h{builder}; sub builder { 1 }");
    assert_symbol_table_invariant("the declaration itself", "sub builder { 1 }");

    let unrelated_division = "$value / 2; sub builder { 1 }";
    let division_tokens = signatures(unrelated_division, LexerConfig::default());
    assert!(division_tokens.iter().any(|token| matches!(&token.0, TokenType::Division)));
    assert!(!division_tokens.iter().any(|token| matches!(&token.0, TokenType::RegexMatch)));
    assert_symbol_table_invariant("an unrelated division operator", unrelated_division);
}

#[test]
fn canonical_token_contract_is_exact_under_every_compiled_feature_set() {
    // This same golden is executed in default and all-features builds. The
    // compatibility `simd` feature is not allowed to alter any token field.
    let input = "my $x = q{value}; $x =~ /value/;";
    let actual = signatures(input, LexerConfig::default());
    let expected = vec![
        (TokenType::Keyword(Arc::from("my")), Arc::from("my"), 0, 2),
        (TokenType::Identifier(Arc::from("$x")), Arc::from("$x"), 3, 5),
        (TokenType::Operator(Arc::from("=")), Arc::from("="), 6, 7),
        (TokenType::QuoteSingle, Arc::from("q{value}"), 8, 16),
        (TokenType::Semicolon, Arc::from(";"), 16, 17),
        (TokenType::Identifier(Arc::from("$x")), Arc::from("$x"), 18, 20),
        (TokenType::Operator(Arc::from("=~")), Arc::from("=~"), 21, 23),
        (TokenType::RegexMatch, Arc::from("/value/"), 24, 31),
        (TokenType::Semicolon, Arc::from(";"), 31, 32),
        (TokenType::EOF, Arc::from(""), 32, 32),
    ];

    assert_eq!(actual, expected);
}

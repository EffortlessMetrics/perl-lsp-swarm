//! Public behavior contract for `LexerConfig` and feature-independent tokenization.
//!
//! These tests exercise `PerlLexer::with_config` rather than treating field
//! names as evidence. They distinguish observable effects, compatibility fields,
//! shared-cursor thresholds, and the deprecated empty `simd` Cargo feature.
//!
//! The `track_positions` field is deprecated (since 0.17.0, removal owned by
//! #8749) but remains the exact surface under contract until removal, so these
//! tests deliberately keep naming it.

#![allow(deprecated)]

use std::{collections::HashSet, sync::Arc};

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

fn assert_spans_slice_source(input: &str, tokens: &[TokenSignature]) {
    for (token_type, text, start, end) in tokens {
        assert!(start <= end, "reversed token span for {token_type:?} in {input:?}");
        assert_eq!(
            input.get(*start..*end),
            Some(text.as_ref()),
            "token {token_type:?} text {text:?} is not the source slice [{start}, {end}) of {input:?}"
        );
    }
}

fn assert_track_positions_is_ignored(input: &str, base: LexerConfig) {
    let tracked = signatures(input, base.clone());
    let untracked = signatures(input, LexerConfig { track_positions: false, ..base });
    assert_eq!(tracked, untracked, "track_positions changed tokens for {input:?}");
    assert_spans_slice_source(input, &untracked);
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
    // `assert!(LexerConfig::POSITIONS_ARE_ALWAYS_TRACKED)` cannot fail: the
    // const is `true` by construction. These cases fail if `track_positions:
    // false` drops, zeros, or rewrites byte spans — including on empty input
    // and EOF, which the previous non-EOF filter could not see.
    let interpolating = r#""hello $name""#;
    assert_ne!(
        signatures(interpolating, LexerConfig::default()),
        signatures(
            interpolating,
            LexerConfig { parse_interpolation: false, ..LexerConfig::default() }
        ),
        "interpolation must remain a real switch so position-field equality is not vacuous"
    );

    for input in ["", " ", "# c\n", "my $café = 1;\r\nprint $café;", interpolating] {
        assert_track_positions_is_ignored(input, LexerConfig::default());
        assert_track_positions_is_ignored(
            input,
            LexerConfig { parse_interpolation: false, ..LexerConfig::default() },
        );
        assert_track_positions_is_ignored(
            input,
            LexerConfig { max_lookahead: 0, ..LexerConfig::default() },
        );
        assert_track_positions_is_ignored(
            input,
            LexerConfig { track_positions: false, ..LexerConfig::default() }.clone(),
        );
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

#[test]
fn checkpoint_identity_ignores_noop_configuration_variation() -> R {
    // Checkpoints capture replay state only. The deprecated `track_positions`
    // field is a no-op, so flipping it between capture and restore must neither
    // reject the checkpoint nor change the replayed token projection.
    let input = "my $x = q{v}; print $x;";
    for prefix_tokens in [1usize, 3] {
        let mut tracked = PerlLexer::with_config(input, LexerConfig::default());
        for _ in 0..prefix_tokens {
            if tracked.next_token().is_none() {
                break;
            }
        }
        let checkpoint = tracked.checkpoint();
        assert!(tracked.can_restore(&checkpoint), "captured checkpoint must be valid");

        let mut untracked = PerlLexer::with_config(
            input,
            LexerConfig { track_positions: false, ..LexerConfig::default() },
        );
        assert!(
            untracked.can_restore(&checkpoint),
            "no-op field variation must not invalidate checkpoint identity"
        );

        let from_tracked = collect_remaining(&mut tracked);
        untracked.restore(&checkpoint);
        let replayed = collect_remaining(&mut untracked);
        assert_eq!(
            from_tracked, replayed,
            "replay changed under track_positions variation after {prefix_tokens} prefix token(s)"
        );

        // The reverse direction: a checkpoint captured under the no-op value
        // restores into the default configuration unchanged.
        let mut source = PerlLexer::with_config(
            input,
            LexerConfig { track_positions: false, ..LexerConfig::default() },
        );
        for _ in 0..prefix_tokens {
            if source.next_token().is_none() {
                break;
            }
        }
        let flipped_checkpoint = source.checkpoint();
        let expected_reverse = collect_remaining(&mut source);
        let mut back_on_default = PerlLexer::with_config(input, LexerConfig::default());
        assert!(
            back_on_default.can_restore(&flipped_checkpoint),
            "default configuration must accept checkpoints captured under the no-op value"
        );
        back_on_default.restore(&flipped_checkpoint);
        let reverse_replayed = collect_remaining(&mut back_on_default);
        assert_eq!(
            expected_reverse, reverse_replayed,
            "reverse checkpoint replay changed under track_positions variation after {prefix_tokens} prefix token(s)"
        );
    }
    Ok(())
}

fn collect_rust_sources(
    root: &std::path::Path,
    excluded_directories: &[&str],
    sources: &mut Vec<std::path::PathBuf>,
    is_package_root: bool,
) -> R {
    let metadata = std::fs::symlink_metadata(root)?;
    if metadata.file_type().is_symlink() {
        return Err(missing(format!(
            "cannot verify Rust source through symlink {}; keep the simd gate closed",
            root.display()
        )));
    }
    if metadata.is_dir() {
        for entry in std::fs::read_dir(root)? {
            let entry = entry?;
            let path = entry.path();
            // Inspect metadata before applying exclusions so a symlink named
            // `target`, even under `src`, cannot disappear from the scan.
            let entry_metadata = std::fs::symlink_metadata(&path)?;
            if entry_metadata.file_type().is_symlink() {
                return Err(missing(format!(
                    "cannot verify Rust source through symlink {}; keep the simd gate closed",
                    path.display()
                )));
            }
            if is_package_root
                && entry_metadata.is_dir()
                && path
                    .file_name()
                    .and_then(std::ffi::OsStr::to_str)
                    .is_some_and(|name| excluded_directories.contains(&name))
            {
                continue;
            }
            collect_rust_sources(&path, excluded_directories, sources, false)?;
        }
    } else if metadata.is_file() && root.extension().is_some_and(|ext| ext == "rs") {
        sources.push(root.to_path_buf());
    } else if !metadata.is_file() {
        return Err(missing(format!(
            "cannot verify non-regular source entry {}; keep the simd gate closed",
            root.display()
        )));
    }
    Ok(())
}

fn simd_selection_sources(
    root: &std::path::Path,
    excluded_directories: &[&str],
) -> R<Vec<std::path::PathBuf>> {
    let canonical_root = std::fs::canonicalize(root)?;
    reject_configured_build_script(root)?;
    let mut sources = Vec::new();
    collect_rust_sources(root, excluded_directories, &mut sources, true)?;
    let mut offenders = Vec::new();
    let mut inspected = HashSet::new();
    for path in sources {
        inspect_source(&path, &canonical_root, &mut inspected, &mut offenders)?;
    }
    Ok(offenders)
}

fn inspect_source(
    path: &std::path::Path,
    canonical_root: &std::path::Path,
    inspected: &mut HashSet<std::path::PathBuf>,
    offenders: &mut Vec<std::path::PathBuf>,
) -> R {
    let canonical_path = std::fs::canonicalize(path)?;
    if !canonical_path.starts_with(canonical_root) {
        return Err(missing(format!(
            "Rust source {} escapes the checked-in package root {}; keep the simd gate closed",
            path.display(),
            canonical_root.display()
        )));
    }
    let metadata = std::fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() {
        return Err(missing(format!(
            "cannot verify included Rust source through symlink {}; keep the simd gate closed",
            path.display()
        )));
    }
    if !metadata.is_file() {
        return Err(missing(format!(
            "cannot verify non-regular included source {}; keep the simd gate closed",
            path.display()
        )));
    }
    if !inspected.insert(canonical_path) {
        return Ok(());
    }

    if path.file_name().and_then(std::ffi::OsStr::to_str) == Some("build.rs") {
        return Err(missing(format!(
            "cannot verify OUT_DIR-generated Rust from build script {}; keep the simd gate closed until generated output is inspectable",
            path.display()
        )));
    }
    let contents = std::fs::read_to_string(path)?;
    if contents.contains("CARGO_FEATURE_SIMD") {
        return Err(missing(format!(
            "cannot verify custom build-script feature handling in {}; CARGO_FEATURE_SIMD must remain unused",
            path.display()
        )));
    }
    if contains_simd_selection(&contents)? {
        offenders.push(path.to_path_buf());
    }
    for included in literal_include_sources(&contents, path, canonical_root)? {
        inspect_source(&included, canonical_root, inspected, offenders)?;
    }
    Ok(())
}

fn contains_simd_selection(source: &str) -> R<bool> {
    let source = sanitize_rust_source(source)?;
    let bytes = source.as_bytes();
    let cfg_contexts = cfg_selector_contexts(bytes)?;
    for (start, _) in source.match_indices("feature") {
        let before_is_identifier = start
            .checked_sub(1)
            .and_then(|index| bytes.get(index))
            .is_some_and(|character| character.is_ascii_alphanumeric() || *character == b'_');
        let after_feature = start + "feature".len();
        let after_is_identifier = bytes
            .get(after_feature)
            .is_some_and(|character| character.is_ascii_alphanumeric() || *character == b'_');
        if before_is_identifier || after_is_identifier {
            continue;
        }
        if !cfg_contexts
            .iter()
            .any(|&(context_start, context_end)| context_start <= start && start < context_end)
        {
            continue;
        }
        let Some(equal_start) = skip_rust_whitespace_and_comments(bytes, after_feature) else {
            continue;
        };
        if bytes.get(equal_start) != Some(&b'=') {
            continue;
        }
        let Some(value_start) = skip_rust_whitespace_and_comments(bytes, equal_start + 1) else {
            continue;
        };
        if is_simd_string_literal(bytes, value_start)? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn cfg_selector_contexts(bytes: &[u8]) -> R<Vec<(usize, usize)>> {
    let source = std::str::from_utf8(bytes)?;
    let mut contexts = Vec::new();
    for (start, _) in source.match_indices("cfg") {
        let before_is_identifier = start
            .checked_sub(1)
            .and_then(|index| bytes.get(index))
            .is_some_and(|character| character.is_ascii_alphanumeric() || *character == b'_');
        if before_is_identifier {
            continue;
        }

        let cfg_end = start + "cfg".len();
        let is_cfg_attr = bytes.get(cfg_end..cfg_end + "_attr".len()) == Some(b"_attr")
            && !bytes
                .get(cfg_end + "_attr".len())
                .is_some_and(|character| character.is_ascii_alphanumeric() || *character == b'_');
        let name_end = if is_cfg_attr { cfg_end + "_attr".len() } else { cfg_end };
        let Some(mut cursor) = skip_rust_whitespace_and_comments(bytes, name_end) else {
            continue;
        };
        let is_cfg_macro = !is_cfg_attr && bytes.get(cursor) == Some(&b'!');
        if is_cfg_macro {
            cursor = skip_rust_whitespace_and_comments(bytes, cursor + 1)
                .ok_or_else(|| missing("unterminated cfg! selector invocation"))?;
        } else if !is_attribute_name(bytes, start) {
            continue;
        }
        if bytes.get(cursor) != Some(&b'(') {
            continue;
        }
        let close = matching_parenthesis(bytes, cursor)?;
        contexts.push((cursor + 1, close));
    }
    Ok(contexts)
}

fn is_attribute_name(bytes: &[u8], start: usize) -> bool {
    let Some(bracket) = previous_non_whitespace(bytes, start) else {
        return false;
    };
    if bytes.get(bracket) != Some(&b'[') {
        return false;
    }
    let Some(prefix) = previous_non_whitespace(bytes, bracket) else {
        return false;
    };
    if bytes.get(prefix) == Some(&b'#') {
        return true;
    }
    bytes.get(prefix) == Some(&b'!')
        && previous_non_whitespace(bytes, prefix).is_some_and(|hash| bytes.get(hash) == Some(&b'#'))
}

fn reject_configured_build_script(root: &std::path::Path) -> R {
    let manifest = root.join("Cargo.toml");
    let metadata = match std::fs::symlink_metadata(&manifest) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(missing(format!(
            "cannot verify package manifest {}; keep the simd gate closed",
            manifest.display()
        )));
    }
    let contents = std::fs::read_to_string(&manifest)?;
    let mut in_package = false;
    for line in contents.lines() {
        let line = line.split_once('#').map_or(line, |(code, _)| code).trim();
        if line.starts_with('[') {
            in_package = line == "[package]";
            continue;
        }
        if in_package && is_build_assignment(line) {
            return Err(missing(format!(
                "cannot verify configured build script in {}; generated source is not inspectable, so keep the simd gate closed",
                manifest.display()
            )));
        }
    }
    Ok(())
}

fn is_build_assignment(line: &str) -> bool {
    line.split_once('=')
        .map(|(key, _)| matches!(key.trim(), "build" | "\"build\""))
        .unwrap_or(false)
}

fn previous_non_whitespace(bytes: &[u8], mut index: usize) -> Option<usize> {
    while index > 0 {
        index -= 1;
        if !bytes[index].is_ascii_whitespace() {
            return Some(index);
        }
    }
    None
}

fn matching_parenthesis(bytes: &[u8], open: usize) -> R<usize> {
    let mut depth = 0usize;
    for (index, character) in bytes.iter().enumerate().skip(open) {
        match character {
            b'(' => depth += 1,
            b')' => {
                depth = depth
                    .checked_sub(1)
                    .ok_or_else(|| missing("unbalanced cfg selector invocation"))?;
                if depth == 0 {
                    return Ok(index);
                }
            }
            _ => {}
        }
    }
    Err(missing("unterminated cfg selector invocation"))
}

fn literal_include_sources(
    source: &str,
    source_path: &std::path::Path,
    canonical_root: &std::path::Path,
) -> R<Vec<std::path::PathBuf>> {
    let original = source;
    let source = sanitize_rust_source(source)?;
    let sanitized_bytes = source.as_bytes();
    let original_bytes = original.as_bytes();
    let mut included_sources = Vec::new();
    for (start, _) in source.match_indices("include") {
        let before_is_identifier = start
            .checked_sub(1)
            .and_then(|index| sanitized_bytes.get(index))
            .is_some_and(|character| character.is_ascii_alphanumeric() || *character == b'_');
        if before_is_identifier {
            continue;
        }
        let Some(mut cursor) =
            skip_rust_whitespace_and_comments(original_bytes, start + "include".len())
        else {
            continue;
        };
        if original_bytes.get(cursor) != Some(&b'!') {
            continue;
        }
        cursor =
            skip_rust_whitespace_and_comments(original_bytes, cursor + 1).ok_or_else(|| {
                missing(format!("cannot inspect include! in {}", source_path.display()))
            })?;
        if original_bytes.get(cursor) != Some(&b'(') {
            return Err(missing(format!("cannot inspect include! in {}", source_path.display())));
        }
        cursor = skip_rust_whitespace_and_comments(original_bytes, cursor + 1).ok_or_else(|| {
            missing(format!(
                "cannot inspect dynamically generated include! in {}; keep the simd gate closed",
                source_path.display()
            ))
        })?;
        if original_bytes.get(cursor) != Some(&b'"') {
            return Err(missing(format!(
                "cannot inspect dynamically generated include! in {}; keep the simd gate closed",
                source_path.display()
            )));
        }
        let literal_start = cursor + 1;
        let literal_end = original_bytes
            .get(literal_start..)
            .and_then(|tail| tail.iter().position(|character| *character == b'"'))
            .map(|offset| literal_start + offset)
            .ok_or_else(|| missing(format!(
                "cannot inspect dynamically generated include! in {}; keep the simd gate closed",
                source_path.display()
            )))?;
        let literal = original.get(literal_start..literal_end).ok_or_else(|| {
            missing(format!("cannot resolve include! in {}", source_path.display()))
        })?;
        if literal.contains('\\') {
            return Err(missing(format!(
                "cannot inspect escaped include! path in {}; keep the simd gate closed",
                source_path.display()
            )));
        }
        let parent = source_path.parent().ok_or_else(|| {
            missing(format!("cannot resolve include! in {}", source_path.display()))
        })?;
        let candidate = parent.join(literal);
        let metadata = std::fs::symlink_metadata(&candidate)?;
        if metadata.file_type().is_symlink() {
            return Err(missing(format!(
                "cannot verify include! source through symlink {}; keep the simd gate closed",
                candidate.display()
            )));
        }
        if !metadata.is_file() {
            return Err(missing(format!(
                "cannot verify non-regular include! source {}; keep the simd gate closed",
                candidate.display()
            )));
        }
        let included = std::fs::canonicalize(candidate)?;
        if !included.starts_with(canonical_root) {
            return Err(missing(format!(
                "include! source {} escapes the checked-in package root {}; keep the simd gate closed",
                included.display(),
                canonical_root.display()
            )));
        }
        included_sources.push(included);
    }
    Ok(included_sources)
}

fn sanitize_rust_source(source: &str) -> R<String> {
    let bytes = source.as_bytes();
    let mut sanitized = source.as_bytes().to_vec();
    let mut index = 0;
    while index < bytes.len() {
        if bytes.get(index..index + 2) == Some(b"//") {
            let start = index;
            index += 2;
            while bytes.get(index).is_some_and(|character| *character != b'\n') {
                index += 1;
            }
            for character in &mut sanitized[start..index] {
                if *character != b'\n' {
                    *character = b' ';
                }
            }
        } else if bytes.get(index..index + 2) == Some(b"/*") {
            let start = index;
            index += 2;
            let mut depth = 1usize;
            while depth > 0 {
                if bytes.get(index..index + 2) == Some(b"/*") {
                    depth += 1;
                    index += 2;
                } else if bytes.get(index..index + 2) == Some(b"*/") {
                    depth -= 1;
                    index += 2;
                } else if bytes.get(index).is_some() {
                    index += 1;
                } else {
                    return Err(missing("unterminated Rust block comment"));
                }
            }
            for character in &mut sanitized[start..index] {
                if *character != b'\n' {
                    *character = b' ';
                }
            }
        } else if bytes[index] == b'"'
            || (bytes[index] == b'\''
                && (bytes.get(index + 2) == Some(&b'\'') || bytes.get(index + 1) == Some(&b'\\')))
        {
            let quote = bytes[index];
            let start = index;
            index += 1;
            let mut escaped = false;
            let mut closed = false;
            while let Some(&character) = bytes.get(index) {
                index += 1;
                if escaped {
                    escaped = false;
                } else if character == b'\\' {
                    escaped = true;
                } else if character == quote {
                    closed = true;
                    break;
                }
            }
            if !closed {
                return Err(missing("unterminated Rust string or character literal"));
            }
            let is_simd = quote == b'"'
                && ordinary_string_is_simd(
                    source
                        .get(start + 1..index - 1)
                        .ok_or_else(|| missing("invalid Rust string literal boundaries"))?,
                )?;
            if !is_simd {
                for character in &mut sanitized[start..index] {
                    *character = b' ';
                }
            }
        } else if bytes[index] == b'r'
            && bytes
                .get(index + 1)
                .is_some_and(|character| *character == b'#' || *character == b'"')
        {
            let start = index;
            index += 1;
            let mut hashes = 0usize;
            while bytes.get(index) == Some(&b'#') {
                hashes += 1;
                index += 1;
            }
            if bytes.get(index) != Some(&b'"') {
                continue;
            }
            index += 1;
            let content_start = index;
            let mut content_end = None;
            while index < bytes.len() {
                if bytes[index] == b'"'
                    && (0..hashes).all(|offset| bytes.get(index + 1 + offset) == Some(&b'#'))
                {
                    content_end = Some(index);
                    index += 1 + hashes;
                    break;
                }
                index += 1;
            }
            let content_end = content_end.ok_or_else(|| missing("unterminated Rust raw string"))?;
            if &source[content_start..content_end] != "simd" {
                for character in &mut sanitized[start..index] {
                    *character = b' ';
                }
            }
        } else {
            index += 1;
        }
    }
    String::from_utf8(sanitized)
        .map_err(|error| missing(format!("Rust source was not UTF-8: {error}")))
}

fn is_simd_string_literal(bytes: &[u8], start: usize) -> R<bool> {
    if bytes.get(start) == Some(&b'"') {
        let mut cursor = start + 1;
        let mut escaped = false;
        while let Some(&character) = bytes.get(cursor) {
            cursor += 1;
            if escaped {
                escaped = false;
            } else if character == b'\\' {
                escaped = true;
            } else if character == b'"' {
                let contents = std::str::from_utf8(
                    bytes
                        .get(start + 1..cursor - 1)
                        .ok_or_else(|| missing("invalid Rust string literal boundaries"))?,
                )?;
                return ordinary_string_is_simd(contents);
            }
        }
        return Err(missing("unterminated Rust string literal"));
    }
    if bytes.get(start) != Some(&b'r') {
        return Ok(false);
    }

    let mut cursor = start + 1;
    let mut hashes = 0usize;
    while bytes.get(cursor) == Some(&b'#') {
        hashes += 1;
        cursor += 1;
    }
    if bytes.get(cursor) != Some(&b'"') {
        return Ok(false);
    }
    cursor += 1;
    if bytes.get(cursor..cursor + 4) != Some(b"simd") {
        return Ok(false);
    }
    cursor += 4;
    if bytes.get(cursor) != Some(&b'"') {
        return Ok(false);
    }
    cursor += 1;
    Ok((0..hashes).all(|_| {
        let matches = bytes.get(cursor) == Some(&b'#');
        cursor += usize::from(matches);
        matches
    }))
}

fn ordinary_string_is_simd(contents: &str) -> R<bool> {
    let mut decoded = String::new();
    let mut characters = contents.chars().peekable();
    while let Some(character) = characters.next() {
        if character != '\\' {
            decoded.push(character);
            continue;
        }
        let escape = characters.next().ok_or_else(|| missing("unterminated Rust string escape"))?;
        match escape {
            '"' | '\\' | '\'' => decoded.push(escape),
            'n' => decoded.push('\n'),
            'r' => decoded.push('\r'),
            't' => decoded.push('\t'),
            '0' => decoded.push('\0'),
            '\n' => {
                while characters.peek().is_some_and(|character| character.is_whitespace()) {
                    characters.next();
                }
            }
            '\r' => {
                if characters.peek() == Some(&'\n') {
                    characters.next();
                }
                while characters.peek().is_some_and(|character| character.is_whitespace()) {
                    characters.next();
                }
            }
            'x' => {
                let high = characters
                    .next()
                    .and_then(hex_digit)
                    .ok_or_else(|| missing("invalid Rust hex string escape"))?;
                let low = characters
                    .next()
                    .and_then(hex_digit)
                    .ok_or_else(|| missing("invalid Rust hex string escape"))?;
                decoded.push(char::from((high << 4) | low));
            }
            'u' => {
                if characters.next() != Some('{') {
                    return Err(missing("invalid Rust unicode string escape"));
                }
                let mut value = 0u32;
                let mut digits = 0usize;
                loop {
                    let next = characters
                        .next()
                        .ok_or_else(|| missing("unterminated Rust unicode string escape"))?;
                    if next == '}' {
                        break;
                    }
                    let digit = hex_digit(next)
                        .ok_or_else(|| missing("invalid Rust unicode string escape"))?;
                    digits += 1;
                    value = value
                        .checked_mul(16)
                        .and_then(|value| value.checked_add(u32::from(digit)))
                        .ok_or_else(|| missing("overflowing Rust unicode string escape"))?;
                }
                if digits == 0 {
                    return Err(missing("empty Rust unicode string escape"));
                }
                decoded.push(
                    char::from_u32(value)
                        .ok_or_else(|| missing("invalid Rust unicode string escape"))?,
                );
            }
            _ => return Err(missing(format!("unsupported Rust string escape \\{escape}"))),
        }
    }
    Ok(decoded == "simd")
}

fn hex_digit(character: char) -> Option<u8> {
    character.to_digit(16).and_then(|digit| u8::try_from(digit).ok())
}

fn skip_rust_whitespace_and_comments(bytes: &[u8], mut index: usize) -> Option<usize> {
    loop {
        while bytes.get(index).is_some_and(u8::is_ascii_whitespace) {
            index += 1;
        }
        if bytes.get(index..index + 2) == Some(b"//") {
            index += 2;
            while bytes.get(index).is_some_and(|character| *character != b'\n') {
                index += 1;
            }
            continue;
        }
        if bytes.get(index..index + 2) != Some(b"/*") {
            return Some(index);
        }
        index += 2;
        let mut depth = 1usize;
        while depth > 0 {
            if bytes.get(index..index + 2) == Some(b"/*") {
                depth += 1;
                index += 2;
            } else if bytes.get(index..index + 2) == Some(b"*/") {
                depth -= 1;
                index += 2;
            } else if bytes.get(index).is_some() {
                index += 1;
            } else {
                return None;
            }
        }
    }
}

fn validate_simd_readme_contract(readme: &str) -> R {
    let simd_row = readme
        .lines()
        .find(|line| line.starts_with("| Cargo feature `simd` |"))
        .ok_or_else(|| missing("README lost the Cargo feature simd configuration row"))?;
    for required in [
        "**Deprecated**",
        "Compatibility no-op",
        "no `simd` selector was found",
        "checked-in Rust sources or literal `include!` output",
        "Top-level excluded package directories are not traversed as source roots, while literal `include!` output from scanned sources is inspected",
        "Symlinks, build scripts, and dynamic or out-of-tree includes are rejected closed",
        "No SIMD performance claim is made",
        "#8749",
    ] {
        if !simd_row.contains(required) {
            return Err(missing(format!(
                "README simd row lost required deprecation wording: {required}"
            )));
        }
    }
    let lowercase = readme.to_ascii_lowercase();
    for forbidden in [
        "enabled",
        "active",
        "available",
        "accelerat",
        "vectorized",
        "optimized",
        "optimization",
        "simd implementation",
        "simd processing",
        "simd scanner",
        "simd capability",
        "simd support",
        "simd path",
        "simd is used",
        "uses simd",
        "simd acceleration",
        "simd implementation",
    ] {
        if lowercase.contains(forbidden) {
            return Err(missing(format!(
                "README simd contract contains a contradictory capability claim: {forbidden}"
            )));
        }
    }
    for line in lowercase.lines() {
        if line.contains("simd")
            && line.contains("performance")
            && !line.contains("no simd performance claim is made")
        {
            return Err(missing(format!(
                "README simd contract contains a contradictory performance claim: {line}"
            )));
        }
    }
    Ok(())
}

#[test]
fn simd_feature_stays_declared_empty_and_unreferenced() -> R {
    // The `simd` feature is a deprecated compatibility no-op (since 0.17.0,
    // removal owned by #8749). This gate fails if anyone gives the feature a
    // dependency list, selects it from code, or drops the no-op declaration —
    // which would silently turn the advertised feature into a real claim. The
    // checked-in Rust source scan is bounded, and build scripts are rejected
    // closed because their generated output is not statically inspectable here.
    let manifest = include_str!("../Cargo.toml");
    let features_block = manifest
        .split("[features]")
        .nth(1)
        .ok_or_else(|| missing("Cargo.toml lost its [features] table"))?;
    let simd_row =
        features_block.lines().map(str::trim).find(|line| line.starts_with("simd ")).ok_or_else(
            || missing("Cargo.toml no longer declares the compatibility simd feature"),
        )?;
    assert_eq!(
        simd_row, "simd = []",
        "the deprecated simd feature must stay an empty dependency-free no-op until #8749 removes it"
    );

    let readme = include_str!("../README.md");
    validate_simd_readme_contract(readme)?;

    let package_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let offenders =
        simd_selection_sources(package_root, &["tests", "examples", "benches", "target"])?;
    assert!(
        offenders.is_empty(),
        "no production, generated, or build source may select the compatibility simd feature, but these do: {offenders:?}"
    );
    Ok(())
}

#[test]
fn simd_gate_detects_selection_in_build_surface_fixture() -> R {
    let fixture_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("simd_feature_selection")
        .join("generated");
    let offenders = simd_selection_sources(&fixture_root, &[])?;
    assert_eq!(
        offenders,
        vec![fixture_root.join("selector.rs")],
        "the negative control must detect simd selection in a generated surface outside src"
    );
    Ok(())
}

#[test]
fn simd_gate_does_not_hide_nested_excluded_directory_sources() -> R {
    let fixture_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("simd_feature_selection");
    let offenders = simd_selection_sources(&fixture_root, &["target"])?;
    assert!(
        offenders.contains(&fixture_root.join("generated").join("selector.rs")),
        "the generated fixture must remain a detected simd selector"
    );
    assert!(
        offenders.contains(&fixture_root.join("nested").join("target").join("hidden.rs")),
        "a nested directory named target must not hide a checked-in Rust source"
    );
    Ok(())
}

#[test]
fn simd_gate_inspects_literal_non_rust_include_fixture() -> R {
    let fixture_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("simd_feature_includes");
    let offenders = simd_selection_sources(&fixture_root, &[])?;
    assert!(
        offenders.contains(&std::fs::canonicalize(fixture_root.join("payload.inc"))?),
        "literal non-Rust include output must be inspected for simd selectors"
    );
    Ok(())
}

#[test]
fn simd_gate_rejects_literal_include_under_excluded_directory() -> R {
    let fixture_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("simd_feature_includes");
    let offenders = simd_selection_sources(&fixture_root, &["excluded"])?;
    assert!(
        offenders.contains(&std::fs::canonicalize(fixture_root.join("excluded/selector.inc"))?),
        "literal include! output under an excluded directory must still be inspected"
    );
    Ok(())
}

#[test]
fn simd_gate_detects_raw_string_feature_selection() -> R {
    assert!(contains_simd_selection(r##"#[cfg(feature = r#"simd"#)]"##)?);
    assert!(contains_simd_selection(r###"cfg!(feature = r##"simd"##)"###)?);
    assert!(contains_simd_selection(r##"#[cfg_attr(feature = "simd", allow(dead_code))]"##)?);
    assert!(contains_simd_selection(r##"#[cfg(feature = "\x73imd")]"##)?);
    assert!(!contains_simd_selection(r#"let feature = "simd";"#)?);
    assert!(!contains_simd_selection(r#"const FEATURE: &str = "simd";"#)?);
    assert!(!contains_simd_selection("// cfg(feature = \"simd\")")?);
    assert!(!contains_simd_selection("const TEXT: &str = \"cfg(feature = \\\"simd\\\")\";")?);
    Ok(())
}

#[test]
fn simd_gate_detects_inner_attributes_and_ignores_cfg_attr_calls() -> R {
    assert!(contains_simd_selection("#![cfg(feature = \"simd\")]")?);
    assert!(contains_simd_selection("#![cfg_attr(feature = \"simd\", allow(dead_code))]")?);
    assert!(!contains_simd_selection("cfg(feature = \"simd\")")?);
    assert!(!contains_simd_selection("fn cfg_attr(feature: &str) {}\ncfg_attr(\"simd\");")?);
    assert!(!contains_simd_selection(
        "macro_rules! cfg_attr { ($($item:tt)*) => {} }\ncfg_attr!(feature = \"simd\");"
    )?);
    let fixture_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("simd_feature_inner_attribute");
    let offenders = simd_selection_sources(&fixture_root, &[])?;
    assert_eq!(offenders, vec![fixture_root.join("selector.rs")]);
    Ok(())
}

#[test]
fn simd_gate_rejects_uninspectable_out_dir_generation_fixture() -> R {
    let fixture_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("simd_feature_generation");
    let build_script = std::fs::read_to_string(fixture_root.join("build.rs"))?;
    assert!(
        build_script.contains("concat!(\"OUT\", \"_DIR\")"),
        "fixture must exercise a dynamically constructed OUT_DIR key"
    );
    let error = simd_selection_sources(&fixture_root, &[])
        .err()
        .ok_or_else(|| missing("OUT_DIR generation fixture unexpectedly passed the simd gate"))?;
    assert!(
        error.to_string().contains("OUT_DIR"),
        "OUT_DIR generation must remain an explicit uninspectable gate failure: {error}"
    );
    Ok(())
}

#[test]
fn simd_gate_rejects_uninspectable_custom_build_script_fixture() -> R {
    let fixture_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("simd_feature_custom_build");
    let build_script = std::fs::read_to_string(fixture_root.join("custom_build.rs"))?;
    assert!(build_script.contains("CARGO_FEATURE_"));
    assert!(!build_script.contains("CARGO_FEATURE_SIMD"));
    let error = simd_selection_sources(&fixture_root, &[])
        .err()
        .ok_or_else(|| missing("custom build-script feature environment unexpectedly passed"))?;
    assert!(
        error.to_string().contains("configured build script"),
        "custom build-script feature handling must fail closed beyond exact text matches: {error}"
    );
    Ok(())
}

#[test]
fn quoted_build_manifest_key_is_fail_closed_and_unrelated_keys_are_allowed() -> R {
    assert!(is_build_assignment("build = \"custom_build.rs\""));
    assert!(is_build_assignment("\"build\" = \"custom_build.rs\""));
    assert!(!is_build_assignment("builder = \"custom_build.rs\""));
    assert!(!is_build_assignment("\"build-script\" = \"custom_build.rs\""));
    Ok(())
}

#[test]
fn simd_readme_guard_rejects_contradictory_capability_claim() -> R {
    let readme = include_str!("../README.md");
    let contradictory = format!("{readme}\nThe lexer uses SIMD acceleration.\n");
    let error = validate_simd_readme_contract(&contradictory)
        .err()
        .ok_or_else(|| missing("contradictory README SIMD wording unexpectedly passed"))?;
    assert!(
        error.to_string().contains("accelerat"),
        "README contradiction must identify the capability claim: {error}"
    );
    Ok(())
}

#[test]
fn simd_readme_guard_rejects_contradictory_performance_claim() -> R {
    let readme = include_str!("../README.md");
    let contradictory = format!("{readme}\nThe lexer provides better SIMD performance.\n");
    let error = validate_simd_readme_contract(&contradictory).err().ok_or_else(|| {
        missing("contradictory README SIMD performance wording unexpectedly passed")
    })?;
    assert!(
        error.to_string().contains("performance"),
        "README contradiction must identify the performance claim: {error}"
    );
    validate_simd_readme_contract(readme)?;
    Ok(())
}

/// Tests for prototype character validation (issue #3355).
///
/// Perl only allows a specific set of characters in old-style prototypes:
/// `$`, `@`, `%`, `&`, `*`, `\`, `;`, `+`, `_`, brackets, and spaces.
/// Any other character should emit a warning-level diagnostic.
use perl_parser_core::{NodeKind, Parser};
use perl_tdd_support::must;

/// Helper: parse code and collect diagnostic messages from `parse_with_recovery`.
fn parse_and_get_diagnostics(code: &str) -> Vec<String> {
    let mut parser = Parser::new(code);
    let output = parser.parse_with_recovery();
    output.diagnostics.iter().map(|e| e.to_string()).collect()
}

/// Helper: parse code and assert no prototype-invalid diagnostic is emitted.
fn assert_no_prototype_warning(code: &str) {
    let diagnostics = parse_and_get_diagnostics(code);
    let proto_diags: Vec<_> = diagnostics.iter().filter(|m| m.contains("prototype")).collect();
    assert!(
        proto_diags.is_empty(),
        "Expected no prototype diagnostics for `{}`, got: {:?}",
        code,
        proto_diags
    );
}

/// Helper: parse code and assert that a prototype-invalid diagnostic IS emitted.
fn assert_has_prototype_warning(code: &str) {
    let diagnostics = parse_and_get_diagnostics(code);
    let has_proto_diag = diagnostics.iter().any(|m| {
        let lower = m.to_lowercase();
        lower.contains("prototype") && (lower.contains("invalid") || lower.contains("character"))
    });
    assert!(
        has_proto_diag,
        "Expected a prototype-invalid diagnostic for `{}`, got diagnostics: {:?}",
        code, diagnostics
    );
}

// --- Valid prototype tests (no warning expected) ---

#[test]
fn valid_proto_dollar_dollar_at() {
    // $$@ — all valid prototype characters
    assert_no_prototype_warning("sub valid_proto ($$@) { }");
}

#[test]
fn valid_proto_backslash_ref() {
    // \@ \% — backslash-ref prototypes are valid
    assert_no_prototype_warning(r"sub backslash_ref (\@\%) { }");
}

#[test]
fn valid_proto_with_semi() {
    // $;@ — semicolon as optional separator is valid
    assert_no_prototype_warning("sub with_semi ($;@) { }");
}

#[test]
fn valid_proto_empty() {
    // () — empty prototype is valid
    assert_no_prototype_warning("sub no_args () { }");
}

#[test]
fn valid_proto_glob() {
    // * — glob prototype character
    assert_no_prototype_warning("sub glob_proto (*) { }");
}

#[test]
fn valid_proto_ampersand() {
    // & — coderef prototype character
    assert_no_prototype_warning("sub code_proto (&) { }");
}

#[test]
fn valid_proto_plus() {
    // + — scalar or array/hash ref
    assert_no_prototype_warning("sub plus_proto (+) { }");
}

#[test]
fn valid_proto_underscore() {
    // _ — default $_ prototype character
    assert_no_prototype_warning("sub under_proto (_) { }");
}

#[test]
fn valid_proto_bracketed_ref_group() {
    // Bracketed reference groups from core comp/proto.t: \[$@%&*].
    assert_no_prototype_warning(r"sub ref_group (\[$@%&*]) { }");
}

#[test]
fn valid_proto_mixed_bracketed_groups() {
    assert_no_prototype_warning(r"sub multi1 (\[%@]) { }");
    assert_no_prototype_warning(r"sub multi2 (\[$*&]) { }");
    assert_no_prototype_warning(r"sub multi4 ($\[%]) { }");
    assert_no_prototype_warning(r"sub multi5 (\[$@]$) { }");
}

// --- Invalid prototype tests (warning expected) ---

#[test]
fn invalid_proto_xyz() {
    // XYZ are not valid prototype characters
    assert_has_prototype_warning("sub invalid_proto (XYZ) { }");
}

#[test]
fn invalid_proto_letter_a() {
    // a single letter is not a valid prototype character
    assert_has_prototype_warning("sub bad_proto (a) { }");
}

#[test]
fn invalid_proto_bare_mixed() {
    // A bare identifier with no sigil prefix: $X@Y are prototype-ish but X alone is invalid.
    // "XY" as a bare prototype string (no leading sigil) contains only invalid chars.
    assert_has_prototype_warning("sub mixed_proto (XY) { }");
}

#[test]
fn typed_signature_type_constraint_does_not_warn() {
    assert_no_prototype_warning("sub typed_sig (Type $x) { }");
}

// --- Quote-like spellings as prototype text (#13919) ---

#[test]
fn quote_like_spellings_survive_as_prototype_payload() {
    // Quote-like spellings inside a prototype are payload text: the lexer
    // must not let the closing paren start a quote, the declaration and its
    // prototype content survive, and the invalid-character warning contract
    // is unchanged.
    for word in ["q", "qq", "qw", "qx", "qr", "m", "s", "tr", "y"] {
        for source in [format!("sub named({word}) {{ 1 }}"), format!("sub forward({word});")] {
            let mut parser = Parser::new(&source);
            let ast = must(parser.parse());
            let NodeKind::Program { statements } = &ast.kind else {
                panic!("expected a program for `{source}`");
            };
            assert_eq!(statements.len(), 1, "declaration lost for `{source}`");
            let NodeKind::Subroutine { prototype, .. } = &statements[0].kind else {
                panic!("expected a subroutine for `{source}`, got {:?}", statements[0].kind);
            };
            let prototype =
                prototype.as_deref().unwrap_or_else(|| panic!("prototype lost for `{source}`"));
            let NodeKind::Prototype { content } = &prototype.kind else {
                panic!("expected a prototype node for `{source}`, got {:?}", prototype.kind);
            };
            assert_eq!(content, word, "prototype payload changed for `{source}`");
            assert_has_prototype_warning(&source);
        }
    }
}

#[test]
fn quote_like_signature_defaults_keep_their_quote_tokens() {
    // Signature defaults are expressions in the same parenthesised window:
    // their quote-like operators must not be flattened to identifiers.
    for source in ["sub f($x = q{hello}) { return $x }", "sub f($x = m/ab/) { return $x }"] {
        let mut parser = Parser::new(source);
        let ast = must(parser.parse());
        let NodeKind::Program { statements } = &ast.kind else {
            panic!("expected a program for `{source}`");
        };
        assert_eq!(statements.len(), 1, "declaration lost for `{source}`");
        let NodeKind::Subroutine { prototype, signature, .. } = &statements[0].kind else {
            panic!("expected a subroutine for `{source}`, got {:?}", statements[0].kind);
        };
        assert!(prototype.is_none(), "signature misread as prototype for `{source}`");
        assert!(signature.is_some(), "signature lost for `{source}`");
        assert_no_prototype_warning(source);
    }
}

#[test]
fn quote_like_after_prototype_still_lexes_as_operator() {
    // Positive control: leaving prototype mode restores ordinary quote-like
    // recognition immediately.
    assert_no_prototype_warning("sub named($) { q{text} }");
    let mut parser = Parser::new("sub named($) { q{text} }");
    let ast = must(parser.parse());
    let NodeKind::Program { statements } = &ast.kind else {
        panic!("expected a program");
    };
    assert!(
        matches!(statements[0].kind, NodeKind::Subroutine { .. }),
        "expected a subroutine, got {:?}",
        statements[0].kind
    );
}

// --- Structural check: invalid prototype still parses as a subroutine ---

#[test]
fn invalid_proto_still_produces_subroutine_node() {
    // Even with an invalid prototype, the parser should produce a Subroutine node
    // (warning, not fatal error).
    let mut parser = Parser::new("sub bar (XYZ) { }");
    let ast = must(parser.parse());
    match &ast.kind {
        NodeKind::Program { statements } => {
            assert!(!statements.is_empty(), "expected at least one statement");
            assert!(
                matches!(statements[0].kind, NodeKind::Subroutine { .. }),
                "expected Subroutine node, got: {}",
                statements[0].kind.kind_name()
            );
        }
        other => {
            let got = other.kind_name();
            assert_eq!("Program", got, "expected Program node at top level");
        }
    }
}

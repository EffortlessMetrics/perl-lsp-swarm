//! #14998: attribute arguments retain source identity before semantic decoding.
use super::*;

#[test]
fn attribute_source_bodies_preserve_internal_bytes() -> Result<(), String> {
    // Perl 5.42 accepts the internal spaces, sigils, and comment-looking bytes
    // as literal constructor-key text. It trims outer spaces semantically;
    // this source owner deliberately retains those too.
    for body in [
        "externalname",
        "external name",
        "external #c\n name",
        "$dyn + 1",
        "foo - bar",
        " external ",
        "a(b)c",
        "manĝis",
        "a\t b",
        "a\r\n b",
        "",
    ] {
        let source = format!(":param({body});");
        let mut parser = Parser::new(&source);
        let actual = parser.parse_variable_attributes().map_err(|error| error.to_string())?;
        let expected = vec![format!("param({body})")];
        if actual != expected {
            return Err(format!("argument {body:?}: expected {expected:?}, got {actual:?}"));
        }
        if parser.peek_kind() != Some(TokenKind::Semicolon) {
            return Err(format!("argument {body:?} consumed the following statement boundary"));
        }
    }
    Ok(())
}

#[test]
fn field_node_receives_distinct_spaced_and_contiguous_arguments() -> Result<(), String> {
    for body in ["externalname", "external name", "external #c\n name", "$dyn + 1"] {
        let source = format!("field $x :param({body}) = 0;");
        let mut parser = Parser::new(&source);
        let ast = parser.parse().map_err(|error| error.to_string())?;
        let NodeKind::Program { statements } = &ast.kind else {
            return Err("expected a program".to_owned());
        };
        let field = statements.first().ok_or("missing field declaration")?;
        let NodeKind::VariableDeclaration { attributes, .. } = &field.kind else {
            return Err(format!("expected a field declaration, got {}", field.to_sexp()));
        };
        if attributes != &vec![format!("param({body})")] {
            return Err(format!("field lost source argument {body:?}: {attributes:?}"));
        }
    }
    Ok(())
}

#[test]
fn attribute_source_preservation_keeps_adjacent_and_prototype_forms() -> Result<(), String> {
    for (source, expected) in [
        (":param(external name) :reader(read_x);", vec!["param(external name)", "reader(read_x)"]),
        (":Custom(a b) Other(c d);", vec!["Custom(a b)", "Other(c d)"]),
        (":default(1 + 2);", vec!["default(1 + 2)"]),
        (":prototype($);", vec!["prototype($)"]),
        (":prototype( $ $ );", vec!["prototype( $ $ )"]),
        (":param :reader;", vec!["param", "reader"]),
    ] {
        let mut parser = Parser::new(source);
        let actual = parser.parse_declaration_attributes().map_err(|error| error.to_string())?;
        if actual != expected {
            return Err(format!("{source:?}: expected {expected:?}, got {actual:?}"));
        }
    }
    Ok(())
}

#[test]
fn attribute_source_boundary_obeys_escaped_parentheses() -> Result<(), String> {
    for source in ["(a\\)b)", "(a\\(b)", "(a(b)c)", "(external #c\n name)", "()"] {
        let parser = Parser::new(source);
        let actual =
            parser.source_attribute_argument(0, source.len()).map_err(|error| error.to_string())?;
        if actual != source {
            return Err(format!("source validator rewrote {source:?}"));
        }
    }
    for source in ["(a\\)", "(a) b)", "(a(b)", "a)"] {
        let parser = Parser::new(source);
        if parser.source_attribute_argument(0, source.len()).is_ok() {
            return Err(format!("untrusted boundary was accepted: {source:?}"));
        }
    }
    Ok(())
}

#[test]
fn attribute_source_disagreement_and_unclosed_input_refuse_a_name() -> Result<(), String> {
    // Ordinary tokenization closes at the escaped ')' or skips a ')' as a
    // comment. Neither token-derived boundary can establish a source argument.
    // This repair refuses those existing unsupported shapes rather than adding
    // a separate source scanner that mutates the token stream.
    for source in [":param(a\\)b);", ":param(a #)\n b);", ":param(unclosed"] {
        let mut parser = Parser::new(source);
        if parser.parse_variable_attributes().is_ok() {
            return Err(format!("an untrusted argument published a name: {source:?}"));
        }
    }
    Ok(())
}

fn untrusted_boundary_error(
    error: ParseResult<&str>,
    expected_location: usize,
    variant: &str,
) -> Result<(), String> {
    match error {
        Err(ParseError::SyntaxError { message, location })
            if message == "Untrusted attribute argument boundary"
                && location == expected_location =>
        {
            Ok(())
        }
        other => Err(format!(
            "boundary variant {variant:?} produced {other:?}, expected an exact \
             Untrusted attribute argument boundary error at {expected_location}"
        )),
    }
}

#[test]
fn attribute_argument_rejects_an_out_of_range_boundary_exactly() -> Result<(), String> {
    // A token-derived end beyond the source bytes cannot name a slice; the
    // validator must refuse it instead of fabricating an attribute name.
    let parser = Parser::new(":param(x);");
    untrusted_boundary_error(parser.source_attribute_argument(3, 100), 3, "out-of-range end")?;
    let parser = Parser::new(":param(x);");
    untrusted_boundary_error(parser.source_attribute_argument(100, 110), 100, "out-of-range start")
}

#[test]
fn attribute_argument_rejects_a_non_parenthesis_boundary_exactly() -> Result<(), String> {
    // The boundary must begin at the opening parenthesis byte; a start that
    // names any other byte is an untrusted boundary and is refused.
    let parser = Parser::new(":param(x);");
    untrusted_boundary_error(parser.source_attribute_argument(4, 6), 4, "identifier start byte")
}

#[test]
fn attribute_argument_rejects_split_utf8_and_open_escape_exactly() -> Result<(), String> {
    // A boundary ending mid-UTF-8-character is refused rather than decoded.
    let parser = Parser::new(":param(manĝis);");
    untrusted_boundary_error(parser.source_attribute_argument(6, 10), 6, "split UTF-8 character")?;
    // A boundary ending on an open escape has no closing parenthesis byte.
    let parser = Parser::new("(a\\");
    untrusted_boundary_error(parser.source_attribute_argument(0, 3), 0, "open escape")
}

//! Discriminating proof for the native debug S-expression projection (#8829).
//!
//! These tests pin the one-root grammar, #8424 child order, escaping, recovery
//! visibility, and authority boundaries. Snapshot byte changes are projection
//! changes, not AST semantic proof. Tree-sitter CST compatibility is issue 8047.
//! AST equality is issue 7045. Bounded iterative rendering is issue 8832
//! (`crates/perl-ast/tests/bounded_native_debug_render.rs`).

#[path = "helpers.rs"]
mod helpers;

use helpers::all_nodekind_instances;
use perl_ast::ast::{Token, TokenKind};
use perl_ast::{
    FieldId, GotoTargetForm, NATIVE_DEBUG_SEXP_DEPTH_LIMIT_MARKER, NATIVE_DEBUG_SEXP_GRAMMAR, Node,
    NodeKind, SourceLocation,
};

fn loc() -> SourceLocation {
    SourceLocation { start: 0, end: 1 }
}

fn loc_at(start: usize, end: usize) -> SourceLocation {
    SourceLocation { start, end }
}

fn num(value: &str) -> Node {
    Node::new(NodeKind::Number { value: value.to_string() }, loc())
}

fn block() -> Node {
    Node::new(NodeKind::Block { statements: vec![] }, loc())
}

fn expr_stmt(inner: Node) -> Node {
    Node::new(NodeKind::ExpressionStatement { expression: Box::new(inner) }, loc())
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Sexp {
    Atom(String),
    List(Vec<Sexp>),
}

fn parse_all(input: &str) -> Result<Vec<Sexp>, String> {
    let mut parser = Parser { input, index: 0 };
    let mut forms = Vec::new();
    parser.skip_ws();
    while !parser.eof() {
        forms.push(parser.parse_form()?);
        parser.skip_ws();
    }
    Ok(forms)
}

fn parse_one(input: &str) -> Result<Sexp, String> {
    let mut forms = parse_all(input)?;
    match forms.len() {
        1 => Ok(forms.remove(0)),
        n => Err(format!("expected one root form, got {n}: {input}")),
    }
}

struct Parser<'a> {
    input: &'a str,
    index: usize,
}

impl<'a> Parser<'a> {
    fn eof(&self) -> bool {
        self.index >= self.input.len()
    }

    fn rest(&self) -> &'a str {
        self.input.get(self.index..).unwrap_or("")
    }

    fn skip_ws(&mut self) {
        let rest = self.rest();
        let trimmed = rest.trim_start();
        self.index += rest.len() - trimmed.len();
    }

    fn parse_form(&mut self) -> Result<Sexp, String> {
        self.skip_ws();
        match self.rest().chars().next() {
            Some('(') => self.parse_list(),
            Some('"') => Ok(Sexp::Atom(self.parse_quoted()?)),
            Some(_) => Ok(Sexp::Atom(self.parse_symbol()?)),
            None => Err("unexpected end of debug projection".to_string()),
        }
    }

    fn parse_list(&mut self) -> Result<Sexp, String> {
        self.index += 1;
        let mut items = Vec::new();
        loop {
            self.skip_ws();
            if self.rest().starts_with(')') {
                self.index += 1;
                return Ok(Sexp::List(items));
            }
            if self.eof() {
                return Err("unclosed list in debug projection".to_string());
            }
            items.push(self.parse_form()?);
        }
    }

    fn parse_symbol(&mut self) -> Result<String, String> {
        let rest = self.rest();
        let end = rest
            .find(|ch: char| ch.is_whitespace() || ch == '(' || ch == ')')
            .unwrap_or(rest.len());
        if end == 0 {
            return Err(format!("expected symbol, got {:?}", self.rest()));
        }
        let symbol = rest[..end].to_string();
        self.index += end;
        Ok(symbol)
    }

    fn parse_quoted(&mut self) -> Result<String, String> {
        let rest = self.rest();
        if !rest.starts_with('"') {
            return Err("expected quoted atom".to_string());
        }
        let bytes = rest.as_bytes();
        let mut i = 1;
        let mut out = String::new();
        while i < bytes.len() {
            match bytes[i] {
                b'"' => {
                    self.index += i + 1;
                    return Ok(out);
                }
                b'\\' => {
                    let escape = rest.get(i..).ok_or("truncated escape")?;
                    let mut chars = escape.chars();
                    let _slash = chars.next();
                    match chars.next() {
                        Some('n') => {
                            out.push('\n');
                            i += 2;
                        }
                        Some('r') => {
                            out.push('\r');
                            i += 2;
                        }
                        Some('t') => {
                            out.push('\t');
                            i += 2;
                        }
                        Some('\\') => {
                            out.push('\\');
                            i += 2;
                        }
                        Some('"') => {
                            out.push('"');
                            i += 2;
                        }
                        Some('\'') => {
                            out.push('\'');
                            i += 2;
                        }
                        Some(ch) => {
                            out.push(ch);
                            i += ch.len_utf8() + 1;
                        }
                        None => return Err("truncated escape in quoted atom".to_string()),
                    }
                }
                _ => {
                    let ch = rest[i..].chars().next().ok_or("invalid utf-8 in quoted atom")?;
                    out.push(ch);
                    i += ch.len_utf8();
                }
            }
        }
        Err("unclosed quoted atom".to_string())
    }
}

fn field_names(sexp: &Sexp) -> Vec<String> {
    let Sexp::List(items) = sexp else {
        return Vec::new();
    };
    items
        .iter()
        .skip(1)
        .filter_map(|item| {
            let Sexp::List(inner) = item else {
                return None;
            };
            let Sexp::Atom(name) = inner.first()? else {
                return None;
            };
            // Child fields wrap a nested form. Payload summaries are atoms only
            // and may reuse a FieldId spelling such as `value`.
            if !inner.iter().skip(1).any(|part| matches!(part, Sexp::List(_))) {
                return None;
            }
            FieldId::from_name(name).map(|_| name.clone())
        })
        .collect()
}

fn visit_field_names(node: &Node) -> Vec<String> {
    let mut names = Vec::new();
    node.for_each_child_with_field(|field, _child| {
        if let Some(field) = field {
            names.push(field.name().to_string());
        }
    });
    names
}

fn assert_one_root(sexp: &str) -> Sexp {
    let parsed = parse_one(sexp);
    assert!(parsed.is_ok(), "{}", parsed.as_ref().err().cloned().unwrap_or_default());
    parsed.unwrap_or(Sexp::List(Vec::new()))
}

#[test]
fn grammar_identity_is_versioned_and_not_compatibility() {
    assert_eq!(NATIVE_DEBUG_SEXP_GRAMMAR, "perl-ast-native-debug-sexp/v1");
    assert!(!NATIVE_DEBUG_SEXP_GRAMMAR.contains("tree-sitter"));
}

#[test]
fn every_representative_nodekind_emits_exactly_one_parseable_root() {
    for node in all_nodekind_instances() {
        let sexp = node.to_sexp();
        let parsed = assert_one_root(&sexp);
        assert!(
            matches!(parsed, Sexp::List(_)),
            "{} must render as a list, got {parsed:?} from {sexp}",
            node.kind.kind_name()
        );
        assert!(
            !sexp.contains(NATIVE_DEBUG_SEXP_DEPTH_LIMIT_MARKER),
            "{} representative hit the depth guard: {sexp}",
            node.kind.kind_name()
        );
    }
}

#[test]
fn child_fields_follow_canonical_visit_order() {
    for node in all_nodekind_instances() {
        let sexp = node.to_sexp();
        let parsed = assert_one_root(&sexp);
        assert_eq!(
            field_names(&parsed),
            visit_field_names(&node),
            "{} field order drifted from #8424 visit table; sexp = {sexp}",
            node.kind.kind_name()
        );
    }
}

#[test]
fn try_is_one_root_with_nested_catch_finally_and_binder() {
    let node = Node::new(
        NodeKind::Try {
            body: Box::new(block()),
            catch_blocks: vec![
                (Some(("err".to_string(), loc_at(4, 7))), Box::new(block())),
                (None, Box::new(block())),
            ],
            finally_block: Some(Box::new(block())),
        },
        loc(),
    );
    let sexp = node.to_sexp();
    let parsed = assert_one_root(&sexp);
    assert_eq!(field_names(&parsed), visit_field_names(&node));
    assert!(sexp.starts_with("(try "), "sexp = {sexp}");
    assert!(
        sexp.contains("(catch err (block))") || sexp.contains("(catch err (block "),
        "binder nested: {sexp}"
    );
    assert!(sexp.contains("(finally "), "finally nested: {sexp}");
}

#[test]
fn if_is_one_root_with_nested_elsif_else_in_visit_order() {
    let node = Node::new(
        NodeKind::If {
            condition: Box::new(num("1")),
            then_branch: Box::new(block()),
            elsif_branches: vec![(Box::new(num("2")), Box::new(block()))],
            else_branch: Some(Box::new(block())),
            keyword: Some("unless".to_string()),
        },
        loc(),
    );
    let sexp = node.to_sexp();
    let parsed = assert_one_root(&sexp);
    assert_eq!(
        field_names(&parsed),
        vec!["condition", "then_branch", "condition", "body", "else_branch"]
    );
    assert_eq!(field_names(&parsed), visit_field_names(&node));
    assert!(sexp.contains("(keyword unless)"), "keyword payload: {sexp}");
    assert!(
        !sexp.contains("(elsif "),
        "elsif is nested as condition/body, not a sibling root: {sexp}"
    );
}

#[test]
fn while_and_for_continue_blocks_nest_under_the_owning_root() {
    let while_node = Node::new(
        NodeKind::While {
            condition: Box::new(num("1")),
            body: Box::new(block()),
            continue_block: Some(Box::new(block())),
            keyword: None,
        },
        loc(),
    );
    let for_node = Node::new(
        NodeKind::For {
            init: Some(Box::new(num("0"))),
            condition: Some(Box::new(num("1"))),
            update: Some(Box::new(num("2"))),
            body: Box::new(block()),
            continue_block: Some(Box::new(block())),
        },
        loc(),
    );
    for node in [&while_node, &for_node] {
        let sexp = node.to_sexp();
        assert_one_root(&sexp);
        assert!(sexp.contains("(continue_block "), "continue nested: {sexp}");
        assert_eq!(field_names(&assert_one_root(&sexp)), visit_field_names(node));
    }
}

#[test]
fn program_and_direct_expression_statement_share_one_projection() {
    let expr = expr_stmt(num("42"));
    let program = Node::new(NodeKind::Program { statements: vec![expr.clone()] }, loc());
    let inner = expr.to_sexp_inner();
    let outer = expr.to_sexp();
    assert_eq!(inner, outer, "to_sexp_inner must not unwrap");
    let program_sexp = program.to_sexp();
    assert!(program_sexp.contains(&outer), "program nests the same statement form: {program_sexp}");
    assert!(outer.contains("expression_statement"), "wrapper stays visible: {outer}");
}

#[test]
fn optional_and_repeated_fields_preserve_canonical_order() {
    let empty = Node::new(
        NodeKind::VariableDeclaration {
            declarator: "my".to_string(),
            variable: Box::new(Node::new(
                NodeKind::Variable { sigil: "$".to_string(), name: "x".to_string() },
                loc(),
            )),
            attributes: vec![],
            initializer: None,
        },
        loc(),
    );
    let full = Node::new(
        NodeKind::VariableDeclaration {
            declarator: "my".to_string(),
            variable: Box::new(Node::new(
                NodeKind::Variable { sigil: "$".to_string(), name: "x".to_string() },
                loc(),
            )),
            attributes: vec!["shared".to_string()],
            initializer: Some(Box::new(num("1"))),
        },
        loc(),
    );
    let empty_sexp = empty.to_sexp();
    let full_sexp = full.to_sexp();
    assert!(!empty_sexp.contains("(initializer "), "absent optional omitted: {empty_sexp}");
    assert!(!empty_sexp.contains("(attributes "), "empty repeated omitted: {empty_sexp}");
    assert!(full_sexp.contains("(attributes shared)"), "attributes nested: {full_sexp}");
    assert_eq!(field_names(&assert_one_root(&full_sexp)), visit_field_names(&full));
}

#[test]
fn quotes_backslash_newline_control_and_unicode_escape_deterministically() {
    let node = Node::new(
        NodeKind::String { value: "say \"hi\"\\\n\u{1b} café".to_string(), interpolated: true },
        loc(),
    );
    let sexp = node.to_sexp();
    assert_one_root(&sexp);
    assert!(sexp.contains("string_interpolated"), "raw vs cooked kind: {sexp}");
    assert!(sexp.contains("(interpolated true)"), "interpolated payload: {sexp}");
    assert!(sexp.contains(r#"\""#), "quotes escaped: {sexp}");
    assert!(sexp.contains(r#"\\"#), "backslash escaped: {sexp}");
    assert!(sexp.contains(r#"\n"#), "newline escaped: {sexp}");
    assert!(sexp.contains(r#"\u{1b}"#) || sexp.contains(r#"\u{1B}"#), "control escaped: {sexp}");
    assert!(sexp.contains("café"), "unicode preserved: {sexp}");
    let cooked = Node::new(
        NodeKind::String { value: "say \"hi\"\\\n\u{1b} café".to_string(), interpolated: false },
        loc(),
    );
    assert_ne!(node.to_sexp(), cooked.to_sexp(), "raw and cooked must not collapse");
}

#[test]
fn chained_ops_are_payloads_not_bare_tokens() {
    let node = Node::new(
        NodeKind::ChainedComparison {
            operands: vec![num("1"), num("2"), num("3")],
            ops: vec!["<".to_string(), "==".to_string()],
        },
        loc(),
    );
    let sexp = node.to_sexp();
    assert_one_root(&sexp);
    assert!(sexp.contains("(ops < ==)"), "ops payload: {sexp}");
    assert_eq!(field_names(&assert_one_root(&sexp)), vec!["elements", "elements", "elements"]);
}

#[test]
fn recovery_nodes_remain_visible_under_their_owning_field() -> Result<(), perl_token::TokenSpanError>
{
    let error = Node::new(
        NodeKind::Error {
            message: "oops".to_string(),
            expected: vec![TokenKind::Identifier],
            found: Some(Token::new_checked(TokenKind::Number, "1", 0, 1)?),
            partial: Some(Box::new(num("1"))),
        },
        loc(),
    );
    let program = Node::new(
        NodeKind::Program {
            statements: vec![
                error.clone(),
                Node::new(NodeKind::MissingExpression, loc()),
                Node::new(NodeKind::UnknownRest, loc()),
            ],
        },
        loc(),
    );
    let sexp = program.to_sexp();
    assert_one_root(&sexp);
    assert!(sexp.contains("(ERROR "), "error visible: {sexp}");
    assert!(sexp.contains("(expected Identifier)"), "expected visible: {sexp}");
    assert!(sexp.contains("(found Number 1)"), "found visible: {sexp}");
    assert!(sexp.contains("(partial "), "partial nested: {sexp}");
    assert!(sexp.contains("(missing_expression)"), "missing visible: {sexp}");
    assert!(sexp.contains("(UNKNOWN_REST)"), "unknown visible: {sexp}");
    Ok(())
}

#[test]
fn second_top_level_form_fails_the_one_root_oracle() {
    let malformed = "(try (body (block))) (catch (block)) (finally (block))";
    let parsed = parse_one(malformed);
    assert!(parsed.is_err(), "multi-root must fail, got {parsed:?}");
    if let Err(err) = parsed {
        assert!(err.contains("expected one root form"), "{err}");
    }
}

#[test]
fn depth_limit_marker_is_not_an_ast_node_kind() {
    assert!(
        !NodeKind::ALL_KIND_NAMES
            .iter()
            .any(|name| name.eq_ignore_ascii_case("depth_limit_exceeded"))
    );
    assert_eq!(NATIVE_DEBUG_SEXP_DEPTH_LIMIT_MARKER, "(depth_limit_exceeded)");
    let parsed = assert_one_root(NATIVE_DEBUG_SEXP_DEPTH_LIMIT_MARKER);
    assert_eq!(parsed, Sexp::List(vec![Sexp::Atom("depth_limit_exceeded".to_string())]));
}

#[test]
fn native_projection_docs_do_not_claim_tree_sitter_compatibility() {
    let sources = [
        include_str!("../src/lib.rs"),
        include_str!("../src/ast.rs"),
        include_str!("../src/ast/node_sexp.rs"),
    ];
    for src in sources {
        let lowered = src.to_ascii_lowercase();
        assert!(
            !lowered.contains("tree-sitter compatible"),
            "native debug docs still claim Tree-sitter compatibility"
        );
        assert!(
            !lowered.contains("tree-sitter-compatible"),
            "native debug docs still claim Tree-sitter compatibility"
        );
    }
}

#[test]
fn sexp_equality_is_not_ast_equality() {
    let left = Node::new(
        NodeKind::Heredoc {
            delimiter: "EOF".to_string(),
            content: "hi".to_string(),
            interpolated: false,
            indented: false,
            command: false,
            body_span: Some(loc_at(4, 6)),
        },
        loc(),
    );
    let right = Node::new(
        NodeKind::Heredoc {
            delimiter: "EOF".to_string(),
            content: "hi".to_string(),
            interpolated: false,
            indented: false,
            command: false,
            body_span: Some(loc_at(8, 10)),
        },
        loc(),
    );
    assert_eq!(left.to_sexp(), right.to_sexp());
    assert_ne!(left, right);
}

#[test]
fn subroutine_nests_prototype_signature_attributes_and_body() {
    let node = Node::new(
        NodeKind::Subroutine {
            name: Some("foo".to_string()),
            name_span: Some(loc_at(4, 7)),
            declarator: Some("my".to_string()),
            prototype: Some(Box::new(Node::new(
                NodeKind::Prototype { content: "$;@".to_string() },
                loc(),
            ))),
            signature: Some(Box::new(Node::new(NodeKind::Signature { parameters: vec![] }, loc()))),
            attributes: vec!["lvalue".to_string()],
            body: Box::new(block()),
        },
        loc(),
    );
    let sexp = node.to_sexp();
    assert_one_root(&sexp);
    assert!(sexp.contains("(name foo)"), "{sexp}");
    assert!(sexp.contains("(declarator my)"), "{sexp}");
    assert!(sexp.contains("(attributes lvalue)"), "{sexp}");
    assert!(sexp.contains("(prototype "), "{sexp}");
    assert!(sexp.contains("(signature "), "{sexp}");
    assert!(sexp.contains("(body "), "{sexp}");
    assert!(!sexp.contains("name_span"), "location payloads stay omitted: {sexp}");
}

#[test]
fn goto_form_is_a_payload_not_a_second_root() {
    let node = Node::new(
        NodeKind::Goto {
            target: Box::new(Node::new(NodeKind::Identifier { name: "LABEL".to_string() }, loc())),
            form: GotoTargetForm::Label,
        },
        loc(),
    );
    let sexp = node.to_sexp();
    assert_one_root(&sexp);
    assert!(sexp.contains("(form label)"), "{sexp}");
    assert_eq!(field_names(&assert_one_root(&sexp)), vec!["target"]);
}

#[test]
fn display_matches_to_sexp() {
    let node = num("7");
    assert_eq!(node.to_string(), node.to_sexp());
}

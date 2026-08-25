//! Dancer2 route-declaration extraction (#8918).
//!
//! Extracts the statically supported Dancer2 route declaration grammar from an
//! AST into [`Dancer2RouteDeclaration`] carriers for the registry-activated
//! minting in `perl_semantic_facts::framework_adapters::dancer2_routes`. This
//! is pure source observation: extraction knows the reviewed route grammar, it
//! does not decide activation — a route fact exists only after the registry
//! adapter minted it over an exact activation (#8914 seam).
//!
//! Supported forms (reviewed Dancer2 1.x `_normalize_route` profile):
//!
//! ```perl
//! VERB PATTERN, CODE
//! VERB PATTERN, OPTIONS, CODE
//! VERB NAME, PATTERN, CODE
//! VERB NAME, PATTERN, OPTIONS, CODE
//! any [METHODS], PATTERN, CODE
//! ```
//!
//! with the admitted verbs `get post put del options patch any` and the
//! two-statement `VERB / (pattern, handler)` shape the parser produces for
//! regex patterns such as `get qr{^/re/(\d+)$} => sub {...};`.
//!
//! Positional binding follows the reviewed form table by arity: the handler is
//! always the last operand; a non-literal first operand of a three-plus
//! operand `any` is a dynamic method list (Dancer2 method lists are arrayrefs),
//! while a two-operand `any` binds its first operand as the pattern.
//!
//! Package scoping mirrors the #8914 activation walk: an unqualified file
//! defaults to `main`, bare `package X;` switches the current package for
//! following statements, and a lexical block restores the enclosing package
//! state afterwards.

use crate::ast::{Node, NodeKind};
use perl_semantic_facts::framework_adapters::dancer2_routes::{
    DANCER2_ROUTE_KEYWORDS, Dancer2RouteDeclaration, dancer2_keyword_methods,
    normalize_dancer2_method,
};
use perl_semantic_facts::route::{
    RouteDeclaration, RouteHandler, RouteHandlerBoundary, RouteMethodSet, RouteName,
    RouteNameSelection, RouteOption, RouteOptionValue, RouteOptions, RoutePattern,
    RoutePatternKind,
};
use perl_semantic_facts::{AnchorId, FileId, SourceAnchor};

/// Extract every supported Dancer2 route declaration from `ast`, in source
/// order, with per-declaration package/file identity and a source-order
/// declaration index.
#[must_use]
pub fn extract_dancer2_route_declarations(
    ast: &Node,
    file_id: FileId,
) -> Vec<Dancer2RouteDeclaration> {
    let mut declarations = Vec::new();
    let mut current_package: Option<String> = Some("main".to_string());
    let mut next_index: u32 = 0;
    walk_node(ast, file_id, &mut current_package, &mut declarations, &mut next_index);
    declarations
}

fn walk_node(
    node: &Node,
    file_id: FileId,
    current_package: &mut Option<String>,
    declarations: &mut Vec<Dancer2RouteDeclaration>,
    next_index: &mut u32,
) {
    match &node.kind {
        NodeKind::Program { statements } | NodeKind::Block { statements } => {
            // A lexical block scopes statement-form `package X;` declarations:
            // walk it with a block-local copy so the enclosing package state
            // is restored afterwards (mirrors the #8914 activation walk).
            let mut block_package = current_package.clone();
            walk_statements(statements, file_id, &mut block_package, declarations, next_index);
        }
        NodeKind::Package { name, block: Some(block), .. } => {
            let mut package_scope = Some(name.clone());
            if let NodeKind::Block { statements } = &block.kind {
                walk_statements(statements, file_id, &mut package_scope, declarations, next_index);
            }
        }
        NodeKind::Package { name, block: None, .. } => {
            *current_package = Some(name.clone());
        }
        // Route calls inside a subroutine body register only when that sub
        // executes — statically execution-conditional, never a load-time
        // declaration. Do not descend: a route-looking call inside any
        // `sub { ... }` mints nothing.
        NodeKind::Subroutine { .. } => {}
        _ => {
            for child in node.children() {
                walk_node(child, file_id, current_package, declarations, next_index);
            }
        }
    }
}

fn walk_statements(
    statements: &[Node],
    file_id: FileId,
    current_package: &mut Option<String>,
    declarations: &mut Vec<Dancer2RouteDeclaration>,
    next_index: &mut u32,
) {
    let mut index = 0;
    while index < statements.len() {
        let statement = &statements[index];
        if let NodeKind::ExpressionStatement { expression } = &statement.kind {
            // Single-statement forms: `VERB ...` call or `any [...] ...` list.
            if let Some(declaration) =
                route_from_expression(expression, file_id, current_package, *next_index)
            {
                declarations.push(declaration);
                *next_index += 1;
                index += 1;
                continue;
            }
            // Two-statement form: `VERB` then a single-pair hash of
            // (regex pattern, handler) — the recovery shape the parser
            // produces for `qr{...}` route patterns. The pattern operand must
            // be a regex: a bare keyword statement fused with an unrelated
            // single-pair hash (`get; { foo => sub {} };`) is not a route.
            if let NodeKind::Identifier { name } = &expression.kind
                && DANCER2_ROUTE_KEYWORDS.contains(&name.as_str())
                && index + 1 < statements.len()
                && let Some((pattern_node, handler_node)) =
                    single_pair_pattern_handler(&statements[index + 1])
                && matches!(pattern_node.kind, NodeKind::Regex { .. })
            {
                declarations.push(Dancer2RouteDeclaration {
                    package: current_package.clone(),
                    file_id,
                    declaration_start_byte: span_u32(statement.location.start),
                    declaration_end_byte: span_u32(statements[index + 1].location.end),
                    route: RouteDeclaration {
                        declaration_index: *next_index,
                        keyword: name.clone(),
                        keyword_anchor: anchor(
                            expression.location.start,
                            expression.location.start + name.len(),
                            file_id,
                        ),
                        route_name: RouteNameSelection::Absent,
                        methods: keyword_methods(name),
                        pattern: pattern_from_node(pattern_node, file_id),
                        options: RouteOptions::Map(Vec::new()),
                        handler: handler_from_node(handler_node, file_id),
                    },
                });
                *next_index += 1;
                index += 2;
                continue;
            }
        }
        walk_node(statement, file_id, current_package, declarations, next_index);
        index += 1;
    }
}

fn keyword_methods(keyword: &str) -> RouteMethodSet {
    dancer2_keyword_methods(keyword).unwrap_or(RouteMethodSet::Dynamic {
        reason: format!("keyword `{keyword}` has no reviewed method profile"),
    })
}

/// Route keyword operand of the `any [...]` list form: the parser represents
/// `any [qw/get post/] => '/x' => sub {...}` with the method list as an
/// `any[...]` subscript inside one array literal.
fn any_list_head(expression: &Node) -> Option<(&Node, &Node, &[Node])> {
    let NodeKind::ArrayLiteral { elements } = &expression.kind else {
        return None;
    };
    let head = elements.first()?;
    let NodeKind::Binary { op, left, right } = &head.kind else {
        return None;
    };
    if op != "[]" {
        return None;
    }
    match &left.kind {
        NodeKind::Identifier { name } if name == "any" => {
            Some((left.as_ref(), right.as_ref(), &elements[1..]))
        }
        _ => None,
    }
}

fn route_from_expression(
    expression: &Node,
    file_id: FileId,
    current_package: &Option<String>,
    declaration_index: u32,
) -> Option<Dancer2RouteDeclaration> {
    let context = DeclarationContext { file_id, current_package, declaration_index };
    if let NodeKind::FunctionCall { name, args } = &expression.kind {
        if !DANCER2_ROUTE_KEYWORDS.contains(&name.as_str()) {
            return None;
        }
        let keyword_start = expression.location.start;
        let mut operands: Vec<&Node> = args.iter().collect();
        let methods =
            if name == "any" { bind_any_methods(&mut operands) } else { keyword_methods(name) };
        return build_from_operands(
            name,
            keyword_start,
            keyword_start + name.len(),
            expression.location.end,
            methods,
            &operands,
            &context,
        );
    }

    let (keyword_node, method_list, rest) = any_list_head(expression)?;
    let NodeKind::Identifier { name } = &keyword_node.kind else {
        return None;
    };
    let operands: Vec<&Node> = rest.iter().collect();
    build_from_operands(
        name,
        keyword_node.location.start,
        keyword_node.location.end,
        expression.location.end,
        method_set_from_list(method_list),
        &operands,
        &context,
    )
}

/// Bind the method operand of an `any` call, peeling it from `operands`.
///
/// Dancer2 method lists are arrayrefs: a literal array (or the parser's
/// subscript shape, handled by the caller) is an explicit method list; a
/// non-literal first operand of a three-plus operand call can only be a
/// computed method list; otherwise the first operand is the pattern.
fn bind_any_methods(operands: &mut Vec<&Node>) -> RouteMethodSet {
    let Some(first) = operands.first() else {
        return keyword_methods("any");
    };
    match &first.kind {
        NodeKind::ArrayLiteral { elements } => {
            let methods = method_set_from_elements(elements);
            operands.remove(0);
            methods
        }
        NodeKind::String { .. } | NodeKind::Regex { .. } | NodeKind::HashLiteral { .. } => {
            keyword_methods("any")
        }
        _ if operands.len() >= 3 => {
            let reason =
                "computed `any` method list is a dynamic boundary, not `ANY` exactness".to_string();
            operands.remove(0);
            RouteMethodSet::Dynamic { reason }
        }
        _ => keyword_methods("any"),
    }
}

fn method_set_from_list(list: &Node) -> RouteMethodSet {
    match &list.kind {
        NodeKind::ArrayLiteral { elements } => method_set_from_elements(elements),
        _ => RouteMethodSet::Dynamic {
            reason: "computed `any` method list is a dynamic boundary".to_string(),
        },
    }
}

fn method_set_from_elements(elements: &[Node]) -> RouteMethodSet {
    if elements.is_empty() {
        return RouteMethodSet::Dynamic {
            reason: "empty `any` method list has no exact method set".to_string(),
        };
    }
    let mut methods = Vec::with_capacity(elements.len());
    for element in elements {
        let NodeKind::String { value, interpolated } = &element.kind else {
            return RouteMethodSet::Dynamic {
                reason: "computed entry in `any` method list".to_string(),
            };
        };
        if *interpolated && interpolated_value_is_dynamic(value) {
            return RouteMethodSet::Dynamic {
                reason: "interpolated entry in `any` method list".to_string(),
            };
        }
        let normalized = normalize_dancer2_method(value);
        if normalized.is_empty() {
            return RouteMethodSet::Dynamic {
                reason: "empty entry in `any` method list".to_string(),
            };
        }
        methods.push(normalized);
    }
    RouteMethodSet::Exact(methods)
}

/// File/package identity and source-order index shared by the declarations
/// minted from one extraction walk.
struct DeclarationContext<'a> {
    file_id: FileId,
    current_package: &'a Option<String>,
    declaration_index: u32,
}

/// Bind name/pattern/options/handler operands by the reviewed form table.
///
/// The handler is always the last operand. The remaining operands bind as
/// `[PATTERN]`, `[PATTERN, OPTIONS]`, `[NAME, PATTERN]`, or
/// `[NAME, PATTERN, OPTIONS]`; other shapes are malformed and mint nothing.
fn build_from_operands(
    keyword: &str,
    keyword_start: usize,
    keyword_end: usize,
    declaration_end: usize,
    methods: RouteMethodSet,
    operands: &[&Node],
    context: &DeclarationContext<'_>,
) -> Option<Dancer2RouteDeclaration> {
    let DeclarationContext { file_id, current_package, declaration_index } = *context;
    if operands.len() < 2 {
        // A route needs at least a pattern operand and a handler operand.
        return None;
    }
    let handler_node = operands[operands.len() - 1];
    let rest = &operands[..operands.len() - 1];
    // Positional binding follows the reviewed `_normalize_route` profile: a
    // hashref-shaped operand directly before the handler is the options map;
    // otherwise three operands bind as NAME, PATTERN (any non-literal middle
    // operand is a dynamic pattern, not dynamic options).
    let (name, pattern, options) = match rest {
        [pattern] => (
            RouteNameSelection::Absent,
            pattern_from_node(pattern, file_id),
            RouteOptions::Map(Vec::new()),
        ),
        [pattern, options] if matches!(options.kind, NodeKind::HashLiteral { .. }) => (
            RouteNameSelection::Absent,
            pattern_from_node(pattern, file_id),
            options_from_node(options, file_id),
        ),
        [name, pattern] => (
            name_from_node(name, file_id),
            pattern_from_node(pattern, file_id),
            RouteOptions::Map(Vec::new()),
        ),
        [name, pattern, options] if matches!(options.kind, NodeKind::HashLiteral { .. }) => (
            name_from_node(name, file_id),
            pattern_from_node(pattern, file_id),
            options_from_node(options, file_id),
        ),
        _ => return None,
    };
    Some(Dancer2RouteDeclaration {
        package: current_package.clone(),
        file_id,
        declaration_start_byte: span_u32(keyword_start),
        declaration_end_byte: span_u32(declaration_end),
        route: RouteDeclaration {
            declaration_index,
            keyword: keyword.to_string(),
            keyword_anchor: anchor(keyword_start, keyword_end, file_id),
            route_name: name,
            methods,
            pattern,
            options,
            handler: handler_from_node(handler_node, file_id),
        },
    })
}

fn span_u32(value: usize) -> u32 {
    value.min(u32::MAX as usize) as u32
}

fn anchor(start: usize, end: usize, file_id: FileId) -> SourceAnchor {
    SourceAnchor::new(Some(AnchorId(start as u64)), file_id, span_u32(start), span_u32(end))
}

fn unquote(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    let stripped = trimmed
        .strip_prefix('\'')
        .and_then(|value| value.strip_suffix('\''))
        .or_else(|| trimmed.strip_prefix('"').and_then(|value| value.strip_suffix('"')))
        .unwrap_or(trimmed);
    if stripped.is_empty() { None } else { Some(stripped.to_string()) }
}

/// Whether an interpolated string operand is statically a computed value.
///
/// Perl interpolation only occurs through `$`/`@` sigils **followed by an
/// identifier or index** (`$name`, `${name}`, `@list`, `$arr[0]`), so a
/// trailing sigil (e.g. the regex anchor `$` in `^/re/(\d+)$`) stays static.
/// Escaped sigils (`"\\$x"`) stay conservatively dynamic: the boundary is
/// honest even when the escape would make the value static.
fn interpolated_value_is_dynamic(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.iter().enumerate().any(|(index, byte)| {
        matches!(byte, b'$' | b'@')
            && bytes.get(index + 1).is_some_and(|next| {
                next.is_ascii_alphabetic() || matches!(next, b'_' | b'{' | b'[')
            })
    })
}

/// Whether a `qr{...}` pattern interpolates at runtime.
///
/// Same sigil rule as [`interpolated_value_is_dynamic`]; regex anchors
/// (`$` at pattern end, before `)` or `|`) do not interpolate.
fn regex_pattern_interpolates(pattern: &str) -> bool {
    interpolated_value_is_dynamic(pattern)
}

fn pattern_from_node(node: &Node, file_id: FileId) -> RoutePattern {
    match &node.kind {
        NodeKind::String { value, interpolated }
            if !*interpolated || !interpolated_value_is_dynamic(value) =>
        {
            RoutePattern {
                kind: RoutePatternKind::Literal,
                value: unquote(value),
                anchor: anchor(node.location.start, node.location.end, file_id),
            }
        }
        NodeKind::Regex { pattern, has_embedded_code, .. }
            if !*has_embedded_code && !regex_pattern_interpolates(pattern) =>
        {
            RoutePattern {
                kind: RoutePatternKind::Regex,
                value: Some(pattern.clone()),
                anchor: anchor(node.location.start, node.location.end, file_id),
            }
        }
        _ => RoutePattern {
            kind: RoutePatternKind::Dynamic,
            value: None,
            anchor: anchor(node.location.start, node.location.end, file_id),
        },
    }
}

fn name_from_node(node: &Node, file_id: FileId) -> RouteNameSelection {
    match &node.kind {
        NodeKind::String { value, interpolated }
            if !*interpolated || !interpolated_value_is_dynamic(value) =>
        {
            match unquote(value) {
                Some(value) => RouteNameSelection::Literal(RouteName {
                    value,
                    anchor: anchor(node.location.start, node.location.end, file_id),
                }),
                None => RouteNameSelection::Dynamic {
                    reason: "empty route name operand".to_string(),
                    anchor: anchor(node.location.start, node.location.end, file_id),
                },
            }
        }
        _ => RouteNameSelection::Dynamic {
            reason: "computed route name operand".to_string(),
            anchor: anchor(node.location.start, node.location.end, file_id),
        },
    }
}

fn options_from_node(node: &Node, file_id: FileId) -> RouteOptions {
    let NodeKind::HashLiteral { pairs } = &node.kind else {
        return RouteOptions::Dynamic {
            reason: "computed matching options are an explicit boundary".to_string(),
            anchor: Some(anchor(node.location.start, node.location.end, file_id)),
        };
    };
    let mut entries = Vec::with_capacity(pairs.len());
    for (key_node, value_node) in pairs {
        let literal_key = match &key_node.kind {
            NodeKind::String { value: key_value, interpolated }
                if !*interpolated || !interpolated_value_is_dynamic(key_value) =>
            {
                unquote(key_value)
            }
            _ => None,
        };
        let Some(key) = literal_key else {
            return RouteOptions::Dynamic {
                reason: "computed or empty option key is an explicit boundary".to_string(),
                anchor: Some(anchor(node.location.start, node.location.end, file_id)),
            };
        };
        let value = match &value_node.kind {
            NodeKind::String { value, interpolated }
                if !*interpolated || !interpolated_value_is_dynamic(value) =>
            {
                match unquote(value) {
                    Some(literal) => RouteOptionValue::Literal(literal),
                    None => RouteOptionValue::Dynamic { reason: "empty option value".to_string() },
                }
            }
            NodeKind::String { .. } => {
                RouteOptionValue::Dynamic { reason: "interpolated option value".to_string() }
            }
            _ => RouteOptionValue::Dynamic { reason: "computed option value".to_string() },
        };
        entries.push(RouteOption {
            key,
            key_anchor: anchor(key_node.location.start, key_node.location.end, file_id),
            value,
            value_anchor: anchor(value_node.location.start, value_node.location.end, file_id),
        });
    }
    RouteOptions::Map(entries)
}

fn handler_from_node(node: &Node, file_id: FileId) -> RouteHandler {
    match &node.kind {
        NodeKind::Subroutine { name, .. } if name.is_none() => RouteHandler::InlineSub {
            anchor: anchor(node.location.start, node.location.end, file_id),
        },
        NodeKind::String { value, .. } => RouteHandler::Bounded {
            boundary: RouteHandlerBoundary::String,
            anchor: Some(anchor(node.location.start, node.location.end, file_id)),
            reason: format!("string handler `{value}` is not an exact Dancer2 subroutine target"),
        },
        NodeKind::Unary { op, .. } if op == "\\" => RouteHandler::Bounded {
            boundary: RouteHandlerBoundary::StaticCoderef,
            anchor: Some(anchor(node.location.start, node.location.end, file_id)),
            reason: "static coderef handler is anchored but its named subroutine target is \
                     not proven by the canonical callable fact layer"
                .to_string(),
        },
        _ => RouteHandler::Bounded {
            boundary: RouteHandlerBoundary::Computed,
            anchor: Some(anchor(node.location.start, node.location.end, file_id)),
            reason: "computed handler expression is not an exact handler target".to_string(),
        },
    }
}

/// Single `(pattern, handler)` pair statement of the two-statement route form.
fn single_pair_pattern_handler(statement: &Node) -> Option<(&Node, &Node)> {
    let NodeKind::ExpressionStatement { expression } = &statement.kind else {
        return None;
    };
    let NodeKind::HashLiteral { pairs } = &expression.kind else {
        return None;
    };
    let (pattern, handler) = pairs.first()?;
    if pairs.len() == 1 { Some((pattern, handler)) } else { None }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Parser;
    use perl_tdd_support::{must, must_some};

    fn declarations(code: &str) -> Vec<Dancer2RouteDeclaration> {
        let mut parser = Parser::new(code);
        let ast = must(parser.parse());
        extract_dancer2_route_declarations(&ast, FileId(1))
    }

    fn methods_of(declaration: &Dancer2RouteDeclaration) -> Vec<String> {
        must_some(match &declaration.route.methods {
            RouteMethodSet::Exact(methods) => Some(methods.clone()),
            _ => None,
        })
    }

    #[test]
    fn simple_get_route_binds_pattern_and_inline_handler() {
        let found = declarations("get '/x' => sub { 1 };");
        assert_eq!(found.len(), 1);
        let route = &found[0].route;
        assert_eq!(route.keyword, "get");
        assert_eq!(route.pattern.kind, RoutePatternKind::Literal);
        assert_eq!(route.pattern.value.as_deref(), Some("/x"));
        assert!(matches!(route.handler, RouteHandler::InlineSub { .. }));
        assert_eq!(methods_of(&found[0]), vec!["GET".to_string(), "HEAD".to_string()]);
        assert_eq!(found[0].package.as_deref(), Some("main"));
        assert_eq!(route.declaration_index, 0);
    }

    #[test]
    fn pattern_anchor_covers_exact_tokens() {
        let code = "get '/x' => sub { 1 };";
        let found = declarations(code);
        let anchor = found[0].route.pattern.anchor;
        assert_eq!(&code[anchor.start_byte as usize..anchor.end_byte as usize], "'/x'");
        let keyword = found[0].route.keyword_anchor;
        assert_eq!(&code[keyword.start_byte as usize..keyword.end_byte as usize], "get");
    }

    #[test]
    fn every_admitted_verb_binds_its_method_profile() {
        for (verb, expected) in [
            ("post", vec!["POST"]),
            ("put", vec!["PUT"]),
            ("del", vec!["DELETE"]),
            ("options", vec!["OPTIONS"]),
            ("patch", vec!["PATCH"]),
        ] {
            let found = declarations(&format!("{verb} '/x' => sub {{ 1 }};"));
            assert_eq!(found.len(), 1, "{verb}");
            assert_eq!(methods_of(&found[0]), expected, "{verb}");
        }
    }

    #[test]
    fn bare_delete_is_not_a_route_keyword() {
        assert!(declarations("delete '/x' => sub { 1 };").is_empty());
    }

    #[test]
    fn pattern_options_handler_form_binds_options_map() {
        let found = declarations("post '/x' => { content_type => 'application/json' }, sub { 1 };");
        assert_eq!(found.len(), 1);
        let entries = must_some(match &found[0].route.options {
            RouteOptions::Map(entries) => Some(entries),
            _ => None,
        });
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].key, "content_type");
        assert_eq!(entries[0].value, RouteOptionValue::Literal("application/json".to_string()));
        assert_eq!(methods_of(&found[0]), vec!["POST".to_string()]);
    }

    #[test]
    fn named_route_form_keeps_name_and_pattern_distinct() {
        let found = declarations("get 'user_show', '/users/:id', sub { 1 };");
        assert_eq!(found.len(), 1);
        let name = must_some(match &found[0].route.route_name {
            RouteNameSelection::Literal(name) => Some(name),
            _ => None,
        });
        assert_eq!(name.value, "user_show");
        assert_ne!(name.value, "/users/:id");
        assert_eq!(found[0].route.pattern.value.as_deref(), Some("/users/:id"));
    }

    #[test]
    fn named_route_with_options_form_binds_all_operands() {
        let found = declarations("get 'user_show', '/users/:id', { agent => 'curl' }, sub { 1 };");
        assert_eq!(found.len(), 1);
        assert!(matches!(found[0].route.route_name, RouteNameSelection::Literal(_)));
        assert_eq!(found[0].route.pattern.value.as_deref(), Some("/users/:id"));
        assert!(matches!(found[0].route.options, RouteOptions::Map(_)));
        assert!(matches!(found[0].route.handler, RouteHandler::InlineSub { .. }));
    }

    #[test]
    fn bare_any_binds_default_method_vocabulary() {
        let found = declarations("any '/x' => sub { 1 };");
        assert_eq!(found.len(), 1);
        assert_eq!(methods_of(&found[0]).len(), 7);
        assert!(methods_of(&found[0]).contains(&"DELETE".to_string()));
    }

    #[test]
    fn any_with_qw_method_list_normalizes_del() {
        let found = declarations("any [qw/get post del/] => '/x' => sub { 1 };");
        assert_eq!(found.len(), 1);
        assert_eq!(
            methods_of(&found[0]),
            vec!["GET".to_string(), "POST".to_string(), "DELETE".to_string()]
        );
        assert_eq!(found[0].route.pattern.value.as_deref(), Some("/x"));
    }

    #[test]
    fn any_with_quoted_method_list_binds_exact_set() {
        let found = declarations("any ['get', 'post'] => '/form' => sub { 1 };");
        assert_eq!(found.len(), 1);
        assert_eq!(methods_of(&found[0]), vec!["GET".to_string(), "POST".to_string()]);
    }

    #[test]
    fn any_with_dynamic_method_list_is_a_boundary() {
        let found = declarations("any $methods => '/x' => sub { 1 };");
        assert_eq!(found.len(), 1);
        assert!(matches!(found[0].route.methods, RouteMethodSet::Dynamic { .. }));
        assert_eq!(found[0].route.pattern.value.as_deref(), Some("/x"));
        assert!(matches!(found[0].route.handler, RouteHandler::InlineSub { .. }));
    }

    #[test]
    fn two_operand_any_binds_dynamic_pattern() {
        let found = declarations("any $path => sub { 1 };");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].route.pattern.kind, RoutePatternKind::Dynamic);
        assert_eq!(methods_of(&found[0]).len(), 7);
    }

    #[test]
    fn regex_pattern_two_statement_form_binds_regex_kind() {
        let code = "get qr{^/re/(\\d+)$} => sub { 1 };";
        let found = declarations(code);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].route.pattern.kind, RoutePatternKind::Regex);
        let anchor = found[0].route.pattern.anchor;
        assert_eq!(&code[anchor.start_byte as usize..anchor.end_byte as usize], "qr{^/re/(\\d+)$}");
        assert!(matches!(found[0].route.handler, RouteHandler::InlineSub { .. }));
        assert_eq!(methods_of(&found[0]), vec!["GET".to_string(), "HEAD".to_string()]);
    }

    #[test]
    fn interpolated_pattern_with_sigils_is_a_dynamic_boundary() {
        // Interpolated strings only compute through $/@ sigils; carrying a
        // sigil makes the pattern computed, not literal.
        let found = declarations("get \"$prefix/users\" => sub { 1 };");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].route.pattern.kind, RoutePatternKind::Dynamic);
        assert!(found[0].route.pattern.value.is_none());
    }

    #[test]
    fn interpolated_pattern_without_sigils_stays_literal() {
        let found = declarations("get \"/static\" => sub { 1 };");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].route.pattern.kind, RoutePatternKind::Literal);
        assert_eq!(found[0].route.pattern.value.as_deref(), Some("/static"));
    }

    #[test]
    fn interpolated_name_and_option_values_are_boundaries() {
        let found = declarations("get \"$name\", '/x', sub { 1 };");
        assert_eq!(found.len(), 1);
        assert!(matches!(found[0].route.route_name, RouteNameSelection::Dynamic { .. }));

        let found = declarations("get '/x' => { agent => \"$agent\" }, sub { 1 };");
        assert_eq!(found.len(), 1);
        let entries = must_some(match &found[0].route.options {
            RouteOptions::Map(entries) => Some(entries),
            _ => None,
        });
        assert!(matches!(entries[0], RouteOption { value: RouteOptionValue::Dynamic { .. }, .. }));
    }

    #[test]
    fn dynamic_pattern_is_a_boundary() {
        let found = declarations("get $path => sub { 1 };");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].route.pattern.kind, RoutePatternKind::Dynamic);
        assert!(found[0].route.pattern.value.is_none());
    }

    #[test]
    fn dynamic_name_is_a_boundary() {
        let found = declarations("get $name, '/x', sub { 1 };");
        assert_eq!(found.len(), 1);
        assert!(matches!(found[0].route.route_name, RouteNameSelection::Dynamic { .. }));
        assert_eq!(found[0].route.pattern.value.as_deref(), Some("/x"));
    }

    #[test]
    fn string_handler_is_a_bounded_boundary() {
        let found = declarations("get '/x' => 'handler_name';");
        assert_eq!(found.len(), 1);
        assert_eq!(
            must_some(match &found[0].route.handler {
                RouteHandler::Bounded { boundary, .. } => Some(*boundary),
                _ => None,
            }),
            RouteHandlerBoundary::String
        );
    }

    #[test]
    fn static_coderef_handler_is_bounded_not_exact() {
        let found = declarations("get '/x' => \\&handler;");
        assert_eq!(found.len(), 1);
        assert_eq!(
            must_some(match &found[0].route.handler {
                RouteHandler::Bounded { boundary, .. } => Some(*boundary),
                _ => None,
            }),
            RouteHandlerBoundary::StaticCoderef
        );
    }

    #[test]
    fn non_hashref_middle_operand_binds_as_dynamic_pattern_not_options() {
        // Upstream `_normalize_route` only reads an options map from a
        // hashref-shaped operand; a non-literal middle operand of a
        // three-operand call binds as the pattern.
        let found = declarations("get '/x' => $middle, sub { 1 };");
        assert_eq!(found.len(), 1);
        assert!(matches!(found[0].route.route_name, RouteNameSelection::Literal(_)));
        assert_eq!(found[0].route.pattern.kind, RoutePatternKind::Dynamic);
        assert!(
            matches!(&found[0].route.options, RouteOptions::Map(entries) if entries.is_empty())
        );
    }

    #[test]
    fn non_literal_option_key_is_an_options_boundary() {
        let found = declarations("get '/x' => { $key => 'v' }, sub { 1 };");
        assert_eq!(found.len(), 1);
        assert!(matches!(found[0].route.options, RouteOptions::Dynamic { .. }));
    }

    #[test]
    fn computed_option_value_is_a_per_entry_boundary() {
        let found = declarations("get '/x' => { agent => compute_agent() }, sub { 1 };");
        assert_eq!(found.len(), 1);
        let entries = must_some(match &found[0].route.options {
            RouteOptions::Map(entries) => Some(entries),
            _ => None,
        });
        assert!(matches!(entries[0], RouteOption { value: RouteOptionValue::Dynamic { .. }, .. }));
    }

    #[test]
    fn malformed_arities_mint_nothing() {
        for code in [
            "get;",
            "get sub { 1 };",
            "get '/a', '/b', '/c', sub { 1 };",
            "get 'n', '/p', { a => 'b' }, sub { 1 }, sub { 2 };",
        ] {
            assert!(declarations(code).is_empty(), "`{code}` must not mint a route");
        }
    }

    #[test]
    fn coderef_middle_operand_binds_as_dynamic_pattern() {
        // `get '/x', sub { 1 }, sub { 2 };` follows the reviewed positional
        // table: middle operand is not a hashref, so it binds as a dynamic
        // pattern boundary (never a false exact pattern).
        let found = declarations("get '/x', sub { 1 }, sub { 2 };");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].route.pattern.kind, RoutePatternKind::Dynamic);
        assert!(matches!(found[0].route.handler, RouteHandler::InlineSub { .. }));
    }

    #[test]
    fn routes_are_package_scoped_and_main_defaulted() {
        let found = declarations(
            "get '/a' => sub { 1 };\npackage App;\nget '/b' => sub { 1 };\npackage Other;\nget '/c' => sub { 1 };\n",
        );
        assert_eq!(found.len(), 3);
        assert_eq!(found[0].package.as_deref(), Some("main"));
        assert_eq!(found[1].package.as_deref(), Some("App"));
        assert_eq!(found[2].package.as_deref(), Some("Other"));
    }

    #[test]
    fn lexical_block_package_state_is_restored() {
        let found = declarations(
            "package Outer; { package Inner; get '/i' => sub { 1 }; } get '/o' => sub { 1 };",
        );
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].package.as_deref(), Some("Inner"));
        assert_eq!(found[1].package.as_deref(), Some("Outer"));
    }

    #[test]
    fn package_block_routes_stay_in_their_package() {
        let found = declarations("package App { get '/x' => sub { 1 }; }");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].package.as_deref(), Some("App"));
    }

    #[test]
    fn duplicate_looking_routes_keep_source_order_identity() {
        let found =
            declarations("get '/x' => sub { 1 };\nget '/x' => sub { 2 };\nget '/x' => sub { 3 };");
        assert_eq!(found.len(), 3);
        for (index, declaration) in found.iter().enumerate() {
            assert_eq!(declaration.route.declaration_index, index as u32);
            assert_eq!(declaration.route.pattern.value.as_deref(), Some("/x"));
        }
        let first_anchor = must_some(found.first().map(|d| d.declaration_start_byte));
        let last_anchor = must_some(found.last().map(|d| d.declaration_start_byte));
        assert_ne!(first_anchor, last_anchor);
    }

    #[test]
    fn declaration_spans_cover_the_whole_statement() {
        let code = "get '/x' => sub { 1 };";
        let found = declarations(code);
        assert_eq!(
            &code[found[0].declaration_start_byte as usize..found[0].declaration_end_byte as usize],
            "get '/x' => sub { 1 }"
        );
    }

    #[test]
    fn route_calls_inside_sub_bodies_mint_nothing() {
        // A route-looking call inside a sub registers only when that sub
        // executes — execution-conditional, never a load-time declaration.
        let code = "package App;
use Dancer2;
sub later { get '/x' => sub { 1 }; }
get '/y' => sub { 2 };
";
        let found = declarations(code);
        assert_eq!(found.len(), 1, "only the load-time route mints");
        assert_eq!(found[0].route.pattern.value.as_deref(), Some("/y"));
    }

    #[test]
    fn bare_keyword_plus_unrelated_hash_is_not_a_route() {
        // The two-statement recovery shape is reserved for regex patterns;
        // `get; { foo => sub {} };` must not fuse into a route.
        assert!(
            declarations(
                "get;
{ foo => sub { 1 }; }
"
            )
            .is_empty()
        );
    }

    #[test]
    fn interpolated_method_entry_is_a_boundary() {
        let found = declarations("any [\"$method\"] => '/x' => sub { 1 };");
        assert_eq!(found.len(), 1);
        assert!(
            matches!(found[0].route.methods, RouteMethodSet::Dynamic { .. }),
            "an interpolated method entry must not normalize into an exact set"
        );
    }

    #[test]
    fn interpolated_and_embedded_code_regex_patterns_are_boundaries() {
        let interpolated = declarations(r"get qr{^/$prefix/(\d+)$} => sub { 1 };");
        assert_eq!(interpolated.len(), 1);
        assert_eq!(interpolated[0].route.pattern.kind, RoutePatternKind::Dynamic);
        assert!(interpolated[0].route.pattern.value.is_none());

        // The anchored fixture regex stays exact: a trailing `$` anchor does
        // not interpolate.
        let anchored = declarations(r"get qr{^/re/(\d+)$} => sub { 1 };");
        assert_eq!(anchored.len(), 1);
        assert_eq!(anchored[0].route.pattern.kind, RoutePatternKind::Regex);
        assert!(anchored[0].route.pattern.value.is_some());
    }

    #[test]
    fn unrelated_calls_are_not_routes() {
        assert!(declarations("print '/x';\nget_something '/x' => sub { 1 };").is_empty());
    }
}

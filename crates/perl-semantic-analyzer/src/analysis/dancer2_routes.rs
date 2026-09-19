//! Dancer2 route-declaration and route-context extraction (#8918, #8921).
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
//!
//! #8921 adds the route context extraction:
//!
//! - `prefix` declarations (reviewed upstream v1.1.1 `Dancer2::Core::DSL`
//!   `prefix`): one-argument sticky set/clear (`prefix '/api';`,
//!   `prefix undef;`, `prefix '/';`) and the two-argument lexical block form
//!   (`prefix '/api' => sub { ... };`) whose block the reviewed DSL contract
//!   invokes at load time — the one deliberate exception to the
//!   sub-containment rule below. Sticky prefixes replace the application
//!   prefix; lexical prefixes concatenate onto the enclosing value and
//!   restore it after the block; prefix state is tracked per package.
//! - Effective patterns composed exactly as the reviewed
//!   `Dancer2::Core::Route` BUILDARGS does: plain string concatenation under
//!   a prefix, leading-`/` normalization without one, typed boundaries for
//!   dynamic operands and regex-under-prefix.
//! - Handler operands resolve static coderefs against the in-file
//!   package-scoped subroutine declaration index (#8924 promotion):
//!   `\&handler` with an in-file `sub handler` - including forward
//!   declarations and stubs - binds an exact declaration target; anything
//!   else stays a typed boundary.
//! - Route-local parameter/capture segments of literal patterns, mirroring
//!   the upstream `_build_regexp_from_string` scanner: `:name` tokens
//!   (`[^/.?]+`), typed `:name[Type]` tokens (last `[...]` group stripped),
//!   `**` megasplat before `*` splat. Regex patterns and ambiguous
//!   prefix/pattern token boundaries are extracted as unsupported-capture
//!   boundaries, never guessed keys.

use crate::analysis::dancer2_handler_targets::{SubroutineTargetIndex, handler_from_node};
use crate::ast::{Node, NodeKind};
use perl_semantic_facts::framework_adapters::dancer2_routes::{
    DANCER2_ROUTE_KEYWORDS, Dancer2PrefixDeclaration, Dancer2RouteDeclaration, RouteRegistration,
    dancer2_keyword_methods, normalize_dancer2_method,
};
use perl_semantic_facts::route::{
    RouteDeclaration, RouteEffectivePattern, RouteMethodSet, RouteName, RouteNameSelection,
    RouteOption, RouteOptionValue, RouteOptions, RouteParameterKind, RouteParameterSegment,
    RoutePattern, RoutePatternKind, RoutePrefixDeclaration, RoutePrefixLiteral, RoutePrefixScope,
    RoutePrefixSelection,
};
use perl_semantic_facts::{AnchorId, FileId, SourceAnchor};
use std::collections::HashMap;

/// Prefix state of one package while walking (#8921).
#[derive(Clone, Debug, PartialEq, Eq)]
enum PrefixState {
    /// No active prefix.
    None,
    /// Known literal prefix value plus the source-order indices of the prefix
    /// declarations that contributed to it.
    Literal { value: String, contributions: Vec<u32> },
    /// A computed prefix statement executed in this package: the effective
    /// prefix stays unknown until the next literal set or clear.
    Dynamic,
}

/// Source-extracted Dancer2 route context for one file (#8921): route
/// declarations plus prefix declarations, in source order.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Dancer2RouteContexts {
    /// Route declarations in source order.
    pub routes: Vec<Dancer2RouteDeclaration>,
    /// Prefix declarations in source order.
    pub prefixes: Vec<Dancer2PrefixDeclaration>,
}

/// Extract every supported Dancer2 route declaration from `ast`, in source
/// order, with per-declaration package/file identity and a source-order
/// declaration index.
#[must_use]
pub fn extract_dancer2_route_declarations(
    ast: &Node,
    file_id: FileId,
) -> Vec<Dancer2RouteDeclaration> {
    extract_dancer2_route_contexts(ast, file_id).routes
}

/// Extract the Dancer2 route context from `ast`: route declarations with their
/// effective patterns and route-local parameter segments, plus prefix
/// declarations, in source order (#8921).
#[must_use]
pub fn extract_dancer2_route_contexts(ast: &Node, file_id: FileId) -> Dancer2RouteContexts {
    let mut state = WalkState {
        file_id,
        current_package: Some("main".to_string()),
        prefix_states: HashMap::new(),
        next_route_index: 0,
        next_prefix_index: 0,
        routes: Vec::new(),
        prefixes: Vec::new(),
    };
    let targets = SubroutineTargetIndex::build(ast, file_id);
    walk_node(ast, file_id, &mut state, &targets);
    Dancer2RouteContexts { routes: state.routes, prefixes: state.prefixes }
}

struct WalkState {
    file_id: FileId,
    current_package: Option<String>,
    prefix_states: HashMap<String, PrefixState>,
    next_route_index: u32,
    next_prefix_index: u32,
    routes: Vec<Dancer2RouteDeclaration>,
    prefixes: Vec<Dancer2PrefixDeclaration>,
}

impl WalkState {
    fn prefix_state(&self) -> PrefixState {
        match &self.current_package {
            Some(package) => self.prefix_states.get(package).cloned().unwrap_or(PrefixState::None),
            None => PrefixState::None,
        }
    }

    fn set_prefix_state(&mut self, prefix: PrefixState) {
        if let Some(package) = self.current_package.clone() {
            self.prefix_states.insert(package, prefix);
        }
    }
}

fn walk_node(node: &Node, file_id: FileId, state: &mut WalkState, targets: &SubroutineTargetIndex) {
    match &node.kind {
        NodeKind::Program { statements } | NodeKind::Block { statements } => {
            // A lexical block scopes statement-form `package X;` declarations:
            // walk it with a block-local copy so the enclosing package state
            // is restored afterwards (mirrors the #8914 activation walk). The
            // per-package prefix map restores prefix state the same way.
            let saved_package = state.current_package.clone();
            walk_statements(statements, file_id, state, targets);
            state.current_package = saved_package;
        }
        NodeKind::Package { name, block: Some(block), .. } => {
            let saved_package = state.current_package.clone();
            state.current_package = Some(name.clone());
            if let NodeKind::Block { statements } = &block.kind {
                walk_statements(statements, file_id, state, targets);
            }
            state.current_package = saved_package;
        }
        NodeKind::Package { name, block: None, .. } => {
            state.current_package = Some(name.clone());
        }
        // Route calls inside a subroutine body register only when that sub
        // executes — statically execution-conditional, never a load-time
        // declaration. Do not descend: a route-looking call inside any
        // `sub { ... }` mints nothing. The one deliberate exception is the
        // reviewed `prefix VALUE => sub {...}` lexical block, whose callback
        // the DSL itself invokes at load time; it is walked explicitly by
        // `handle_prefix_statement`.
        NodeKind::Subroutine { .. } => {}
        _ => {
            for child in node.children() {
                walk_node(child, file_id, state, targets);
            }
        }
    }
}

fn walk_statements(
    statements: &[Node],
    file_id: FileId,
    state: &mut WalkState,
    targets: &SubroutineTargetIndex,
) {
    let mut index = 0;
    while index < statements.len() {
        let statement = &statements[index];
        if let NodeKind::ExpressionStatement { expression } = &statement.kind {
            // Single-statement forms: `VERB ...` call or `any [...] ...` list.
            if let Some(declaration) = route_from_expression(expression, state, targets) {
                state.routes.push(declaration);
                state.next_route_index += 1;
                index += 1;
                continue;
            }
            // Prefix declaration (sticky or lexical block form).
            if handle_prefix_statement(expression, state, targets) {
                index += 1;
                continue;
            }
            // Two-statement prefix-clear form: the parser represents
            // `prefix undef;` as a bare `prefix` identifier followed by an
            // `undef` call statement.
            if let NodeKind::Identifier { name } = &expression.kind
                && name == "prefix"
                && index + 1 < statements.len()
                && let NodeKind::ExpressionStatement { expression: next } =
                    &statements[index + 1].kind
                && matches!(&next.kind,
                    NodeKind::FunctionCall { name, args } if name == "undef" && args.is_empty())
            {
                state.prefixes.push(Dancer2PrefixDeclaration {
                    package: state.current_package.clone(),
                    file_id,
                    declaration_start_byte: span_u32(statement.location.start()),
                    declaration_end_byte: span_u32(statements[index + 1].location.end()),
                    prefix: RoutePrefixDeclaration {
                        declaration_index: state.next_prefix_index,
                        keyword: "prefix".to_string(),
                        keyword_anchor: anchor(
                            expression.location.start(),
                            expression.location.end(),
                            file_id,
                        ),
                        selection: RoutePrefixSelection::Cleared,
                        scope: RoutePrefixScope::Sticky,
                    },
                });
                state.next_prefix_index += 1;
                state.set_prefix_state(PrefixState::None);
                index += 2;
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
                let pattern = pattern_from_node(pattern_node, file_id);
                state.routes.push(Dancer2RouteDeclaration {
                    package: state.current_package.clone(),
                    file_id,
                    declaration_start_byte: span_u32(statement.location.start()),
                    declaration_end_byte: span_u32(statements[index + 1].location.end()),
                    route: RouteDeclaration {
                        declaration_index: state.next_route_index,
                        keyword: name.clone(),
                        keyword_anchor: anchor(
                            expression.location.start(),
                            expression.location.start() + name.len(),
                            file_id,
                        ),
                        route_name: RouteNameSelection::Absent,
                        methods: keyword_methods(name),
                        pattern: pattern.clone(),
                        effective_pattern: effective_pattern(&pattern, &state.prefix_state()),
                        options: RouteOptions::Map(Vec::new()),
                        handler: handler_from_node(
                            handler_node,
                            file_id,
                            state.current_package.as_deref(),
                            targets,
                        ),
                    },
                    // Regex patterns are not scanned for deprecated
                    // placeholders (their capture shape is already a typed
                    // unsupported-capture boundary), so this form registers.
                    parameters: parameter_segments_for_regex_pattern(&pattern),
                    registration: RouteRegistration::Registers,
                });
                state.next_route_index += 1;
                index += 2;
                continue;
            }
        }
        walk_node(statement, file_id, state, targets);
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
    state: &WalkState,
    targets: &SubroutineTargetIndex,
) -> Option<Dancer2RouteDeclaration> {
    if let NodeKind::FunctionCall { name, args } = &expression.kind {
        if !DANCER2_ROUTE_KEYWORDS.contains(&name.as_str()) {
            return None;
        }
        let keyword_start = expression.location.start();
        let mut operands: Vec<&Node> = args.iter().collect();
        let methods =
            if name == "any" { bind_any_methods(&mut operands) } else { keyword_methods(name) };
        return build_from_operands(
            name,
            keyword_start,
            keyword_start + name.len(),
            expression.location.end(),
            methods,
            &operands,
            state,
            targets,
        );
    }

    let (keyword_node, method_list, rest) = any_list_head(expression)?;
    let NodeKind::Identifier { name } = &keyword_node.kind else {
        return None;
    };
    let operands: Vec<&Node> = rest.iter().collect();
    build_from_operands(
        name,
        keyword_node.location.start(),
        keyword_node.location.end(),
        expression.location.end(),
        method_set_from_list(method_list),
        &operands,
        state,
        targets,
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

/// Bind name/pattern/options/handler operands by the reviewed form table.
///
/// The handler is always the last operand. The remaining operands bind as
/// `[PATTERN]`, `[PATTERN, OPTIONS]`, `[NAME, PATTERN]`, or
/// `[NAME, PATTERN, OPTIONS]`; other shapes are malformed and mint nothing.
///
/// The route's effective pattern composes against the package's prefix state,
/// and the route-local parameter segments are scanned from the literal
/// pattern; a literal prefix that ends with an open token run makes the
/// composed token boundary ambiguous, so the capture shape becomes an
/// unsupported-capture boundary instead of guessed keys.
fn build_from_operands(
    keyword: &str,
    keyword_start: usize,
    keyword_end: usize,
    declaration_end: usize,
    methods: RouteMethodSet,
    operands: &[&Node],
    state: &WalkState,
    targets: &SubroutineTargetIndex,
) -> Option<Dancer2RouteDeclaration> {
    let file_id = state.file_id;
    let declaration_index = state.next_route_index;
    let current_package = state.current_package.clone();
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
    let pattern_node;
    let (name, options) = match rest {
        [pattern] => {
            pattern_node = pattern;
            (RouteNameSelection::Absent, RouteOptions::Map(Vec::new()))
        }
        [pattern, options] if matches!(options.kind, NodeKind::HashLiteral { .. }) => {
            pattern_node = pattern;
            (RouteNameSelection::Absent, options_from_node(options, file_id))
        }
        [name, pattern] => {
            pattern_node = pattern;
            (name_from_node(name, file_id), RouteOptions::Map(Vec::new()))
        }
        [name, pattern, options] if matches!(options.kind, NodeKind::HashLiteral { .. }) => {
            pattern_node = pattern;
            (name_from_node(name, file_id), options_from_node(options, file_id))
        }
        _ => return None,
    };
    let prefix_state = state.prefix_state();
    let pattern = pattern_from_node(pattern_node, file_id);
    let mut parameters = parameter_segments_from_pattern(pattern_node, &pattern, file_id);
    let mut effective = effective_pattern(&pattern, &prefix_state);
    let mut never_registers = false;
    if pattern.kind == RoutePatternKind::Literal {
        // A literal prefix ending with an open token run merges tokens (or
        // splat shapes) across the prefix/pattern boundary: the composed
        // capture shape cannot be attributed to the local pattern honestly.
        if let PrefixState::Literal { value, .. } = &prefix_state
            && prefix_ends_open_token(value)
        {
            parameters = vec![RouteParameterSegment {
                kind: RouteParameterKind::CaptureUnsupported,
                name: None,
                anchor: pattern.anchor,
                limitation: Some(
                    "literal prefix ends with an open token run; the composed capture shape \
                     cannot be attributed to the local pattern"
                        .to_string(),
                ),
            }];
        }
        // The upstream route constructor croaks on the deprecated
        // `:splat`/`:captures` placeholders — in the local pattern *or*
        // contributed by the composed prefix, because upstream scans the
        // composed pattern: such a route never registers, so it has no
        // effective path and no runnable handler context.
        let local_deprecated = parameters.iter().any(|segment| {
            matches!(segment.kind, RouteParameterKind::Named)
                && matches!(&segment.name,
                    Some(name) if name == "splat" || name == "captures")
        });
        let composed_deprecated = match &effective {
            RouteEffectivePattern::Composed { value, .. } => contains_deprecated_placeholder(value),
            _ => false,
        };
        if local_deprecated || composed_deprecated {
            never_registers = true;
            effective = RouteEffectivePattern::Boundary {
                reason: "deprecated named placeholder `splat`/`captures` (local or \
                         prefix-contributed); the route fails to register upstream"
                    .to_string(),
            };
        }
    }
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
            effective_pattern: effective,
            options,
            handler: handler_from_node(
                handler_node,
                file_id,
                state.current_package.as_deref(),
                targets,
            ),
        },
        parameters,
        registration: if never_registers {
            RouteRegistration::NeverRegisters
        } else {
            RouteRegistration::Registers
        },
    })
}

/// Handle one statement-position `prefix` declaration (#8921).
///
/// Returns `true` when the expression is a prefix declaration of the reviewed
/// grammar (exact or bounded), so the caller consumes the statement. The
/// lexical block form walks its load-time callback with the composed prefix
/// state and restores the enclosing state afterwards.
fn handle_prefix_statement(
    expression: &Node,
    state: &mut WalkState,
    targets: &SubroutineTargetIndex,
) -> bool {
    let NodeKind::FunctionCall { name, args } = &expression.kind else {
        return false;
    };
    if name != "prefix" {
        return false;
    }
    let file_id = state.file_id;
    let keyword_start = expression.location.start();
    let declaration_end = expression.location.end();
    let declaration_index = state.next_prefix_index;
    let (selection, scope, composed) = match args.as_slice() {
        [operand] => {
            let selection = prefix_selection_from_operand(operand, file_id);
            let composed = match &selection {
                RoutePrefixSelection::Literal(literal) => PrefixState::Literal {
                    value: literal.value.clone(),
                    contributions: vec![declaration_index],
                },
                // `prefix undef;`, `prefix '/';`, and an empty-string prefix
                // all reduce to no prefix under the reviewed app coercion.
                RoutePrefixSelection::Cleared => PrefixState::None,
                RoutePrefixSelection::Dynamic { .. } => PrefixState::Dynamic,
                // Future selection variants compose nothing exactly.
                _ => PrefixState::Dynamic,
            };
            (selection, RoutePrefixScope::Sticky, composed)
        }
        [operand, block] if matches!(block.kind, NodeKind::Subroutine { name: None, .. }) => {
            let selection = prefix_selection_from_operand(operand, file_id);
            let block_anchor = anchor(block.location.start(), block.location.end(), file_id);
            let enclosing = state.prefix_state();
            let composed = match (&selection, &enclosing) {
                (
                    RoutePrefixSelection::Literal(literal),
                    PrefixState::Literal { value, contributions },
                ) => PrefixState::Literal {
                    value: format!("{value}{}", literal.value),
                    contributions: {
                        let mut contributions = contributions.clone();
                        contributions.push(declaration_index);
                        contributions
                    },
                },
                // The reviewed upstream `lexical_prefix` concatenates onto
                // the enclosing app prefix: with no enclosing prefix the
                // lexical literal stands alone.
                (RoutePrefixSelection::Literal(literal), PrefixState::None) => {
                    PrefixState::Literal {
                        value: literal.value.clone(),
                        contributions: vec![declaration_index],
                    }
                }
                // A literal lexical prefix under a computed enclosing prefix
                // concatenates onto an unknown value: the composition stays a
                // boundary, never the lexical literal alone.
                (RoutePrefixSelection::Literal(_), PrefixState::Dynamic) => PrefixState::Dynamic,
                // A lexical `prefix '/'` (or empty) composes nothing: the
                // enclosing state carries through unchanged.
                (RoutePrefixSelection::Cleared, _) => enclosing.clone(),
                (RoutePrefixSelection::Dynamic { .. }, _) => PrefixState::Dynamic,
                // Future selection variants compose nothing exactly.
                _ => PrefixState::Dynamic,
            };
            (selection, RoutePrefixScope::Lexical { block_anchor }, composed)
        }
        // Zero operands or an unrecognized shape: upstream would croak
        // (`lexical_prefix` without a callback); nothing is minted and the
        // statement is left to the generic walk.
        _ => return false,
    };
    state.prefixes.push(Dancer2PrefixDeclaration {
        package: state.current_package.clone(),
        file_id,
        declaration_start_byte: span_u32(keyword_start),
        declaration_end_byte: span_u32(declaration_end),
        prefix: RoutePrefixDeclaration {
            declaration_index,
            keyword: "prefix".to_string(),
            keyword_anchor: anchor(keyword_start, keyword_start + name.len(), file_id),
            selection,
            scope: scope.clone(),
        },
    });
    state.next_prefix_index += 1;
    match &scope {
        RoutePrefixScope::Sticky => state.set_prefix_state(composed),
        RoutePrefixScope::Lexical { .. } => {
            // The reviewed DSL contract invokes the block at load time: walk
            // its statements with the composed prefix state, then restore the
            // enclosing state (upstream `lexical_prefix` saves and restores
            // the app prefix around the callback).
            let saved_package = state.current_package.clone();
            let saved_state = state.prefix_state();
            state.set_prefix_state(composed);
            if let NodeKind::Subroutine { body, .. } = &args[1].kind {
                walk_node(body, file_id, state, targets);
            }
            state.current_package = saved_package;
            state.set_prefix_state(saved_state);
        }
        // Future scope variants walk nothing: only the reviewed lexical block
        // is a load-time callback.
        _ => {}
    }
    true
}

/// Prefix-selection slot from one prefix operand.
fn prefix_selection_from_operand(node: &Node, file_id: FileId) -> RoutePrefixSelection {
    match &node.kind {
        NodeKind::Undef => RoutePrefixSelection::Cleared,
        NodeKind::String { value, interpolated }
            if !*interpolated || !interpolated_value_is_dynamic(value) =>
        {
            match static_string(value) {
                // The reviewed app-level prefix coercion treats `/` (and the
                // falsy empty string) as no prefix.
                StaticString::Exact(value) if value == "/" => RoutePrefixSelection::Cleared,
                StaticString::Exact(value) => RoutePrefixSelection::Literal(RoutePrefixLiteral {
                    value,
                    anchor: anchor(node.location.start(), node.location.end(), file_id),
                }),
                StaticString::Empty => RoutePrefixSelection::Cleared,
                // An escaped operand decodes to unknown runtime bytes: the
                // prefix is a boundary, never cleared (which would claim the
                // application has no prefix) and never a guessed literal.
                StaticString::Escaped => RoutePrefixSelection::Dynamic {
                    reason: "escaped prefix operand; escapes are not evaluated".to_string(),
                    anchor: Some(anchor(node.location.start(), node.location.end(), file_id)),
                },
            }
        }
        _ => RoutePrefixSelection::Dynamic {
            reason: "computed prefix operand is an explicit boundary".to_string(),
            anchor: Some(anchor(node.location.start(), node.location.end(), file_id)),
        },
    }
}

/// Effective (prefix-composed) pattern for one route under a prefix state,
/// mirroring the reviewed `Dancer2::Core::Route` BUILDARGS composition.
fn effective_pattern(pattern: &RoutePattern, prefix: &PrefixState) -> RouteEffectivePattern {
    if pattern.kind == RoutePatternKind::Dynamic {
        return RouteEffectivePattern::Boundary {
            reason: "dynamic route pattern has no exact effective pattern".to_string(),
        };
    }
    let Some(pattern_value) = &pattern.value else {
        return RouteEffectivePattern::Boundary {
            reason: "pattern without a literal value has no exact effective pattern".to_string(),
        };
    };
    match prefix {
        PrefixState::None => RouteEffectivePattern::Local {
            value: normalize_unprefixed(pattern_value, pattern.kind),
        },
        PrefixState::Literal { value, contributions } => {
            if pattern.kind == RoutePatternKind::Regex {
                // The reviewed composition for regexref patterns is a
                // `\Q`-quoted, fully anchored regex construction — not a
                // literal string that can be claimed exact here.
                RouteEffectivePattern::Boundary {
                    reason: "regex pattern under a literal prefix composes through a \
                             \\Q-quoted anchored construction, not an exact literal string"
                        .to_string(),
                }
            } else {
                RouteEffectivePattern::Composed {
                    value: format!("{value}{pattern_value}"),
                    prefix_declarations: contributions.clone(),
                }
            }
        }
        PrefixState::Dynamic => RouteEffectivePattern::Boundary {
            reason: "computed prefix is a dynamic boundary; the effective pattern is unknown"
                .to_string(),
        },
    }
}

/// Leading-`/` normalization of an unprefixed string pattern (regex patterns
/// are carried as-is).
fn normalize_unprefixed(value: &str, kind: RoutePatternKind) -> String {
    if kind == RoutePatternKind::Literal && !value.starts_with('/') {
        format!("/{value}")
    } else {
        value.to_string()
    }
}

/// Whether a prefix value ends with a construct whose parsing could continue
/// into the route pattern: a `:`-opened token run that has not terminated
/// (`/`, `.`, `?` terminate; the token class admits `[`, `]`, and `*`), or a
/// trailing `*` that could fuse with a leading pattern `*` into a megasplat.
fn prefix_ends_open_token(value: &str) -> bool {
    let after_last_terminator = match value.rfind(['/', '.', '?']) {
        Some(index) => &value[index + 1..],
        None => value,
    };
    after_last_terminator.contains(':') || value.ends_with('*')
}

/// Whether a composed pattern value contains the deprecated `:splat` or
/// `:captures` placeholder anywhere — upstream scans the composed pattern
/// (prefix + route pattern) and croaks on it at registration. Token rules
/// mirror the parameter scanner: `:`-opened runs terminated by `/`, `.`, or
/// `?`.
fn contains_deprecated_placeholder(composed: &str) -> bool {
    let bytes = composed.as_bytes();
    let mut cursor = 0;
    while cursor < bytes.len() {
        if bytes[cursor] != b':' {
            cursor += 1;
            continue;
        }
        let mut end = cursor + 1;
        while end < bytes.len() && !matches!(bytes[end], b'/' | b'.' | b'?') {
            end += 1;
        }
        if end > cursor + 1 {
            let token = &composed[cursor + 1..end];
            if token == "splat" || token == "captures" {
                return true;
            }
        }
        cursor = end.max(cursor + 1);
    }
    false
}

/// Route-local parameter/capture segments of one route pattern operand
/// (#8921), mirroring the upstream `_build_regexp_from_string` scanner.
fn parameter_segments_from_pattern(
    pattern_node: &Node,
    pattern: &RoutePattern,
    file_id: FileId,
) -> Vec<RouteParameterSegment> {
    if pattern.kind == RoutePatternKind::Dynamic {
        return Vec::new();
    }
    if pattern.kind == RoutePatternKind::Regex {
        return parameter_segments_for_regex_pattern(pattern);
    }
    let Some(value) = &pattern.value else {
        return Vec::new();
    };
    // The unquoted value of a quoted token maps 1:1 onto the source bytes
    // after the opening quote.
    let base = pattern.anchor.start_byte as usize + quote_padding(pattern_node);
    let mut segments: Vec<RouteParameterSegment> = Vec::new();
    let bytes = value.as_bytes();
    let mut cursor = 0;
    while cursor < bytes.len() {
        match bytes[cursor] {
            b':' => {
                let mut end = cursor + 1;
                while end < bytes.len() && !matches!(bytes[end], b'/' | b'.' | b'?') {
                    end += 1;
                }
                if end == cursor + 1 {
                    // `:` with no token characters: not a placeholder.
                    cursor += 1;
                    continue;
                }
                segments.push(token_parameter(
                    &value[cursor + 1..end],
                    base + cursor,
                    base + end,
                    file_id,
                ));
                cursor = end;
            }
            b'*' => {
                if bytes.get(cursor + 1) == Some(&b'*') {
                    segments.push(RouteParameterSegment {
                        kind: RouteParameterKind::Megasplat,
                        name: None,
                        anchor: anchor(base + cursor, base + cursor + 2, file_id),
                        limitation: None,
                    });
                    cursor += 2;
                } else {
                    segments.push(RouteParameterSegment {
                        kind: RouteParameterKind::Splat,
                        name: None,
                        anchor: anchor(base + cursor, base + cursor + 1, file_id),
                        limitation: None,
                    });
                    cursor += 1;
                }
            }
            _ => cursor += 1,
        }
    }
    segments
}

/// One unsupported-capture boundary segment covering a regex pattern: no
/// canonical regex fact layer proves its capture shape without runtime
/// execution.
fn parameter_segments_for_regex_pattern(pattern: &RoutePattern) -> Vec<RouteParameterSegment> {
    vec![RouteParameterSegment {
        kind: RouteParameterKind::CaptureUnsupported,
        name: None,
        anchor: pattern.anchor,
        limitation: Some(
            "regex capture shape is not interpreted without a canonical regex fact layer"
                .to_string(),
        ),
    }]
}

/// Classify one `:token` placeholder segment, stripping the last `[...]`
/// group as the declared type when the token ends with `]`.
fn token_parameter(
    token: &str,
    start: usize,
    end: usize,
    file_id: FileId,
) -> RouteParameterSegment {
    let named = RouteParameterSegment {
        kind: RouteParameterKind::Named,
        name: Some(token.to_string()),
        anchor: anchor(start, end, file_id),
        limitation: None,
    };
    let Some(open) = token.find('[') else {
        return named;
    };
    if !token.ends_with(']') || open + 1 >= token.len() - 1 {
        // No bracket group at the end, or an empty `[]`: stays a plain token.
        return named;
    }
    let type_name = &token[open + 1..token.len() - 1];
    RouteParameterSegment {
        kind: RouteParameterKind::Typed { type_name: type_name.to_string() },
        name: Some(token[..open].to_string()),
        anchor: anchor(start, end, file_id),
        limitation: Some(
            "declared type constraint is runtime-validated, never proven statically".to_string(),
        ),
    }
}

/// Byte padding between a pattern operand's anchor start and its unquoted
/// value (exactly one pair of quotes when quoted). Only leading whitespace is
/// measured: trailing whitespace would shift the content start backwards.
fn quote_padding(node: &Node) -> usize {
    if let NodeKind::String { value, .. } = &node.kind {
        let trimmed_start = value.trim_start();
        if trimmed_start.starts_with('\'') || trimmed_start.starts_with('"') {
            return 1 + (value.len() - trimmed_start.len());
        }
    }
    0
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

/// Classification of a quoted string operand's statically claimable value.
///
/// The AST string node carries the raw token spelling; escapes are not
/// evaluated, so any interior backslash means the runtime bytes differ from
/// the source bytes (`"/u/\x3aid"` is `/u/:id` to Dancer2 at runtime). Such
/// operands stay typed boundaries — the extractor never guesses the decoded
/// value — while escape-free interiors map 1:1 onto the source bytes after
/// the opening quote, keeping parameter anchors exact.
enum StaticString {
    /// Exact runtime value, byte-for-byte the unquoted token interior.
    Exact(String),
    /// Empty string operand.
    Empty,
    /// Interior contains escape sequences this extractor does not evaluate.
    Escaped,
}

fn static_string(raw: &str) -> StaticString {
    let Some(unquoted) = unquote(raw) else {
        return StaticString::Empty;
    };
    if unquoted.contains('\\') { StaticString::Escaped } else { StaticString::Exact(unquoted) }
}

/// Whether an interpolated string operand is statically a computed value.
///
/// Perl interpolation only occurs through `$`/`@` sigils **followed by an
/// identifier or index** (`$name`, `${name}`, `@list`, `$arr[0]`), so a
/// trailing sigil (e.g. the regex anchor `$` in `^/re/(\d+)$`) stays static.
/// Escaped sigils (`"\\$x"`) stay conservatively dynamic: the boundary is
/// honest even when the escape would make the value static.
pub(crate) fn interpolated_value_is_dynamic(value: &str) -> bool {
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
            match static_string(value) {
                StaticString::Exact(value) => RoutePattern {
                    kind: RoutePatternKind::Literal,
                    value: Some(value),
                    anchor: anchor(node.location.start(), node.location.end(), file_id),
                },
                // An empty pattern stays a valueless literal: the fact
                // constructor coerces it to the dynamic boundary.
                StaticString::Empty => RoutePattern {
                    kind: RoutePatternKind::Literal,
                    value: None,
                    anchor: anchor(node.location.start(), node.location.end(), file_id),
                },
                // Escapes are not evaluated: the runtime pattern bytes are
                // unknown here, so the operand is a typed boundary rather
                // than an exact pattern with wrong content and wrong
                // parameter anchors.
                StaticString::Escaped => RoutePattern {
                    kind: RoutePatternKind::Dynamic,
                    value: None,
                    anchor: anchor(node.location.start(), node.location.end(), file_id),
                },
            }
        }
        NodeKind::Regex { pattern, has_embedded_code, .. }
            if !*has_embedded_code && !regex_pattern_interpolates(pattern) =>
        {
            RoutePattern {
                kind: RoutePatternKind::Regex,
                value: Some(pattern.clone()),
                anchor: anchor(node.location.start(), node.location.end(), file_id),
            }
        }
        _ => RoutePattern {
            kind: RoutePatternKind::Dynamic,
            value: None,
            anchor: anchor(node.location.start(), node.location.end(), file_id),
        },
    }
}

fn name_from_node(node: &Node, file_id: FileId) -> RouteNameSelection {
    match &node.kind {
        NodeKind::String { value, interpolated }
            if !*interpolated || !interpolated_value_is_dynamic(value) =>
        {
            match static_string(value) {
                StaticString::Exact(value) => RouteNameSelection::Literal(RouteName {
                    value,
                    anchor: anchor(node.location.start(), node.location.end(), file_id),
                }),
                StaticString::Empty => RouteNameSelection::Dynamic {
                    reason: "empty route name operand".to_string(),
                    anchor: anchor(node.location.start(), node.location.end(), file_id),
                },
                StaticString::Escaped => RouteNameSelection::Dynamic {
                    reason: "escaped route name operand; escapes are not evaluated".to_string(),
                    anchor: anchor(node.location.start(), node.location.end(), file_id),
                },
            }
        }
        _ => RouteNameSelection::Dynamic {
            reason: "computed route name operand".to_string(),
            anchor: anchor(node.location.start(), node.location.end(), file_id),
        },
    }
}

fn options_from_node(node: &Node, file_id: FileId) -> RouteOptions {
    let NodeKind::HashLiteral { pairs } = &node.kind else {
        return RouteOptions::Dynamic {
            reason: "computed matching options are an explicit boundary".to_string(),
            anchor: Some(anchor(node.location.start(), node.location.end(), file_id)),
        };
    };
    let mut entries = Vec::with_capacity(pairs.len());
    for (key_node, value_node) in pairs {
        let literal_key = match &key_node.kind {
            NodeKind::String { value: key_value, interpolated }
                if !*interpolated || !interpolated_value_is_dynamic(key_value) =>
            {
                match static_string(key_value) {
                    StaticString::Exact(key) => Some(key),
                    StaticString::Empty | StaticString::Escaped => None,
                }
            }
            _ => None,
        };
        let Some(key) = literal_key else {
            return RouteOptions::Dynamic {
                reason: "computed, empty, or escaped option key is an explicit boundary"
                    .to_string(),
                anchor: Some(anchor(node.location.start(), node.location.end(), file_id)),
            };
        };
        let value = match &value_node.kind {
            NodeKind::String { value, interpolated }
                if !*interpolated || !interpolated_value_is_dynamic(value) =>
            {
                match static_string(value) {
                    StaticString::Exact(literal) => RouteOptionValue::Literal(literal),
                    StaticString::Empty => {
                        RouteOptionValue::Dynamic { reason: "empty option value".to_string() }
                    }
                    StaticString::Escaped => RouteOptionValue::Dynamic {
                        reason: "escaped option value; escapes are not evaluated".to_string(),
                    },
                }
            }
            NodeKind::String { .. } => {
                RouteOptionValue::Dynamic { reason: "interpolated option value".to_string() }
            }
            _ => RouteOptionValue::Dynamic { reason: "computed option value".to_string() },
        };
        entries.push(RouteOption {
            key,
            key_anchor: anchor(key_node.location.start(), key_node.location.end(), file_id),
            value,
            value_anchor: anchor(value_node.location.start(), value_node.location.end(), file_id),
        });
    }
    RouteOptions::Map(entries)
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
    use perl_semantic_facts::route::{RouteHandler, RouteHandlerBoundary};
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

    // ------------------------------------------------------------------
    // #8921: prefix declarations, effective patterns, parameters.
    // ------------------------------------------------------------------

    fn contexts(code: &str) -> Dancer2RouteContexts {
        let mut parser = Parser::new(code);
        let ast = must(parser.parse());
        extract_dancer2_route_contexts(&ast, FileId(1))
    }

    fn effective_of(declaration: &Dancer2RouteDeclaration) -> &RouteEffectivePattern {
        &declaration.route.effective_pattern
    }

    fn composed_value(declaration: &Dancer2RouteDeclaration) -> String {
        match effective_of(declaration) {
            RouteEffectivePattern::Composed { value, .. } => value.clone(),
            other => panic!("expected a composed effective pattern, got {other:?}"),
        }
    }

    #[test]
    fn sticky_prefix_composes_effective_pattern_with_dependency() {
        let code = "prefix '/api';\nget '/users/:id' => sub { 1 };\nget '/health' => sub { 1 };\n";
        let found = contexts(code);
        assert_eq!(found.prefixes.len(), 1);
        let prefix = &found.prefixes[0];
        assert_eq!(prefix.prefix.declaration_index, 0);
        assert!(matches!(prefix.prefix.scope, RoutePrefixScope::Sticky));
        assert!(matches!(&prefix.prefix.selection, RoutePrefixSelection::Literal(literal)
                if literal.value == "/api"));
        assert_eq!(found.routes.len(), 2);
        for route in &found.routes {
            match effective_of(route) {
                RouteEffectivePattern::Composed { value, prefix_declarations } => {
                    assert!(value.starts_with("/api/"), "composed from the literal prefix");
                    assert_eq!(prefix_declarations, &[0], "source-order prefix dependency");
                }
                other => panic!("expected a composed effective pattern, got {other:?}"),
            }
        }
        assert_eq!(composed_value(&found.routes[0]), "/api/users/:id");
        assert_eq!(composed_value(&found.routes[1]), "/api/health");
    }

    #[test]
    fn prefix_clear_spellings_reset_composition() {
        for clearer in ["prefix undef;", "prefix '/';"] {
            let code = format!("prefix '/api';\n{clearer}\nget '/x' => sub {{ 1 }};\n");
            let found = contexts(&code);
            assert_eq!(found.prefixes.len(), 2, "{clearer}");
            assert!(
                matches!(found.prefixes[1].prefix.selection, RoutePrefixSelection::Cleared),
                "{clearer} clears the prefix"
            );
            assert!(
                matches!(effective_of(&found.routes[0]), RouteEffectivePattern::Local { .. }),
                "{clearer} leaves an unprefixed effective pattern"
            );
        }
    }

    #[test]
    fn lexical_prefix_block_composes_and_restores() {
        let code = "prefix '/api';\nprefix '/v1' => sub {\n  get '/users' => sub { 1 };\n};\nget '/health' => sub { 1 };\n";
        let found = contexts(code);
        assert_eq!(found.prefixes.len(), 2);
        assert!(
            matches!(found.prefixes[1].prefix.scope, RoutePrefixScope::Lexical { .. }),
            "the block form is lexical"
        );
        // Routes inside the load-time block register under the concatenated
        // prefix with both contributing declarations.
        assert_eq!(found.routes.len(), 2);
        match effective_of(&found.routes[0]) {
            RouteEffectivePattern::Composed { value, prefix_declarations } => {
                assert_eq!(value, "/api/v1/users");
                assert_eq!(prefix_declarations, &[0, 1]);
            }
            other => panic!("expected a composed effective pattern, got {other:?}"),
        }
        // The enclosing sticky prefix is restored after the block.
        assert_eq!(composed_value(&found.routes[1]), "/api/health");
    }

    #[test]
    fn nested_lexical_prefixes_concatenate_in_order() {
        let code =
            "prefix '/a' => sub {\n  prefix '/b' => sub {\n    get '/c' => sub { 1 };\n  };\n};\n";
        let found = contexts(code);
        assert_eq!(found.routes.len(), 1);
        match effective_of(&found.routes[0]) {
            RouteEffectivePattern::Composed { value, prefix_declarations } => {
                assert_eq!(value, "/a/b/c");
                assert_eq!(prefix_declarations, &[0, 1]);
            }
            other => panic!("expected a composed effective pattern, got {other:?}"),
        }
    }

    #[test]
    fn computed_prefix_is_a_boundary_not_a_guess() {
        let code = "prefix $base;\nget '/x' => sub { 1 };\n";
        let found = contexts(code);
        assert_eq!(found.prefixes.len(), 1);
        assert!(
            matches!(found.prefixes[0].prefix.selection, RoutePrefixSelection::Dynamic { .. }),
            "a computed prefix operand stays a typed boundary"
        );
        assert!(
            matches!(effective_of(&found.routes[0]), RouteEffectivePattern::Boundary { .. }),
            "routes after a computed prefix compose nothing"
        );
    }

    #[test]
    fn lexical_literal_prefix_under_dynamic_parent_stays_a_boundary() {
        // The reviewed upstream `lexical_prefix` concatenates onto the
        // enclosing app prefix: a literal under a computed parent composes
        // onto an unknown value and must never surface as the literal alone.
        let code = "prefix $base;\nprefix '/v1' => sub {\n  get '/x' => sub { 1 };\n};\n";
        let found = contexts(code);
        assert_eq!(found.routes.len(), 1);
        assert!(
            matches!(effective_of(&found.routes[0]), RouteEffectivePattern::Boundary { .. }),
            "a lexical literal under a computed sticky prefix composes onto an unknown value"
        );
        // Control: the same lexical literal with no enclosing prefix stands
        // alone exactly.
        let control = contexts("prefix '/v1' => sub {\n  get '/x' => sub { 1 };\n};\n");
        assert_eq!(composed_value(&control.routes[0]), "/v1/x");
    }

    #[test]
    fn escaped_string_operands_stay_boundaries() {
        // Escapes are not evaluated: the runtime pattern bytes are unknown,
        // so an escaped pattern operand mints no exact pattern, no parameter
        // facts, and no exact effective path (`"/u/\x3aid"` is `/u/:id` to
        // Dancer2 at runtime).
        let found = declarations(r#"get "/u/\x3aid" => sub { 1 };"#);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].route.pattern.kind, RoutePatternKind::Dynamic);
        assert!(
            found[0].parameters.is_empty(),
            "no parameter keys may be claimed for an escaped pattern"
        );
        assert!(matches!(effective_of(&found[0]), RouteEffectivePattern::Boundary { .. }));

        // An escaped prefix operand is a dynamic boundary, never cleared
        // (which would falsely claim the application has no prefix).
        let found = contexts(r#"prefix "/api\n"; get '/x' => sub { 1 };"#);
        assert!(
            matches!(found.prefixes[0].prefix.selection, RoutePrefixSelection::Dynamic { .. }),
            "an escaped prefix operand stays a typed boundary"
        );
        assert!(matches!(effective_of(&found.routes[0]), RouteEffectivePattern::Boundary { .. }));

        // An escaped route name stays a boundary rather than a guessed
        // literal.
        let found = declarations(r#"get "a\x20b", '/x' => sub { 1 };"#);
        assert!(matches!(&found[0].route.route_name, RouteNameSelection::Dynamic { .. }));

        // Control: escape-free quoted operands keep their exact values.
        let found = declarations("get '/u/:id' => sub { 1 };");
        assert_eq!(found[0].route.pattern.value.as_deref(), Some("/u/:id"));
        assert_eq!(found[0].parameters.len(), 1);
        assert_eq!(found[0].parameters[0].name.as_deref(), Some("id"));
    }

    #[test]
    fn prefix_contributed_deprecated_placeholder_never_registers() {
        // Upstream scans the composed pattern: a `:splat` contributed by the
        // prefix croaks registration exactly like a local one, even though
        // the route-local pattern operand has no parameters of its own.
        let found = contexts("prefix '/:splat/';\nget '/x' => sub { 1 };\n");
        assert_eq!(found.routes.len(), 1);
        assert!(
            matches!(effective_of(&found.routes[0]), RouteEffectivePattern::Boundary { .. }),
            "prefix-contributed deprecated placeholders must degrade the composed projection"
        );
        assert_eq!(
            found.routes[0].registration,
            RouteRegistration::NeverRegisters,
            "the route never registers, so no handler context may claim DSL availability"
        );

        // Control: a plain `:id` under the same prefix composes exactly and
        // registers.
        let found = contexts("prefix '/api';\nget '/x/:id' => sub { 1 };\n");
        assert_eq!(composed_value(&found.routes[0]), "/api/x/:id");
        assert_eq!(found.routes[0].registration, RouteRegistration::Registers);
    }

    #[test]
    fn prefix_state_is_package_scoped() {
        let code = "package A;\nprefix '/a';\nget '/x' => sub { 1 };\npackage B;\nget '/y' => sub { 1 };\n";
        let found = contexts(code);
        assert_eq!(found.routes.len(), 2);
        assert_eq!(composed_value(&found.routes[0]), "/a/x");
        assert!(
            matches!(effective_of(&found.routes[1]), RouteEffectivePattern::Local { .. }),
            "package B has its own (empty) prefix state"
        );
    }

    #[test]
    fn prefix_keyword_anchor_and_range_are_exact() {
        let code = "prefix '/api';";
        let found = contexts(code);
        let prefix = &found.prefixes[0];
        let keyword = prefix.prefix.keyword_anchor;
        assert_eq!(&code[keyword.start_byte as usize..keyword.end_byte as usize], "prefix");
        assert_eq!(
            &code[prefix.declaration_start_byte as usize..prefix.declaration_end_byte as usize],
            "prefix '/api'"
        );
        match &prefix.prefix.selection {
            RoutePrefixSelection::Literal(literal) => {
                assert_eq!(
                    &code[literal.anchor.start_byte as usize..literal.anchor.end_byte as usize],
                    "'/api'"
                );
            }
            other => panic!("expected a literal selection, got {other:?}"),
        }
    }

    #[test]
    fn malformed_prefix_statement_mints_nothing() {
        for code in ["prefix;", "prefix '/a', '/b';"] {
            let found = contexts(code);
            assert!(found.prefixes.is_empty(), "`{code}` must not mint a prefix declaration");
        }
    }

    #[test]
    fn unprefixed_literal_pattern_normalizes_leading_slash() {
        let found = declarations("get 'users' => sub { 1 };");
        assert_eq!(found.len(), 1);
        match effective_of(&found[0]) {
            RouteEffectivePattern::Local { value } => assert_eq!(value, "/users"),
            other => panic!("expected a local effective pattern, got {other:?}"),
        }
    }

    #[test]
    fn regex_pattern_under_prefix_is_a_composition_boundary() {
        let code = "prefix '/api';\nget qr{^/re/(\\d+)$} => sub { 1 };\n";
        let found = contexts(code);
        assert_eq!(found.routes.len(), 1);
        assert!(matches!(effective_of(&found.routes[0]), RouteEffectivePattern::Boundary { .. }));
    }

    #[test]
    fn parameter_segments_cover_named_typed_splat_megasplat() {
        let code = "get '/u/:id/:name[Str]/*/**' => sub { 1 };";
        let found = declarations(code);
        assert_eq!(found.len(), 1);
        let parameters = &found[0].parameters;
        assert_eq!(parameters.len(), 4, "one segment per source-order capture");
        assert!(matches!(parameters[0].kind, RouteParameterKind::Named));
        assert_eq!(parameters[0].name.as_deref(), Some("id"));
        assert!(matches!(&parameters[1].kind,
            RouteParameterKind::Typed { type_name } if type_name == "Str"));
        assert_eq!(parameters[1].name.as_deref(), Some("name"));
        assert!(parameters[1].limitation.is_some(), "typed segments retain the limitation");
        assert!(matches!(parameters[2].kind, RouteParameterKind::Splat));
        assert!(matches!(parameters[3].kind, RouteParameterKind::Megasplat));
        // Anchors point at the exact segment bytes inside the pattern token.
        for (segment, text) in parameters.iter().zip([":id", ":name[Str]", "*", "**"]) {
            let range = &code[segment.anchor.start_byte as usize..segment.anchor.end_byte as usize];
            assert_eq!(range, text, "exact segment anchor");
        }
    }

    #[test]
    fn token_syntax_admits_only_the_reviewed_variants() {
        // A `:` without token characters is not a placeholder; a token stops
        // at `/`, `.`, and `?`.
        let code = "get '/a/:/b/:x.json/:y?' => sub { 1 };";
        let found = declarations(code);
        assert_eq!(found.len(), 1);
        let parameters = &found[0].parameters;
        let names: Vec<_> = parameters.iter().filter_map(|segment| segment.name.clone()).collect();
        assert_eq!(names, vec!["x".to_string(), "y".to_string()]);
        // `:y?` binds the token `y` without the optional marker.
        assert_eq!(
            &code[parameters[1].anchor.start_byte as usize..parameters[1].anchor.end_byte as usize],
            ":y"
        );
    }

    #[test]
    fn megasplat_is_parsed_before_splat() {
        let found = declarations("get '/f/***' => sub { 1 };");
        let parameters = &found[0].parameters;
        assert_eq!(parameters.len(), 2, "`***` is one megasplat then one splat");
        assert!(matches!(parameters[0].kind, RouteParameterKind::Megasplat));
        assert!(matches!(parameters[1].kind, RouteParameterKind::Splat));
    }

    #[test]
    fn regex_route_parameters_are_an_unsupported_capture_boundary() {
        let found = declarations(r"get qr{^/re/(\d+)/(?<name>\w+)$} => sub { 1 };");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].parameters.len(), 1);
        assert!(
            matches!(found[0].parameters[0].kind, RouteParameterKind::CaptureUnsupported),
            "regex captures are never guessed keys"
        );
        assert!(found[0].parameters[0].limitation.is_some());
    }

    #[test]
    fn dynamic_patterns_have_no_parameter_segments() {
        let found = declarations("get $path => sub { 1 };");
        assert!(found[0].parameters.is_empty());
    }

    #[test]
    fn open_token_prefix_boundary_is_an_unsupported_capture() {
        let code = "prefix '/api:';\nget 'id' => sub { 1 };\n";
        let found = contexts(code);
        assert_eq!(found.routes.len(), 1);
        let parameters = &found.routes[0].parameters;
        assert_eq!(parameters.len(), 1);
        assert!(
            matches!(parameters[0].kind, RouteParameterKind::CaptureUnsupported),
            "a prefix ending with an open token run merges tokens across the boundary"
        );
        // The composed effective pattern itself stays exact: the composition
        // is plain concatenation.
        assert_eq!(composed_value(&found.routes[0]), "/api:id");
    }

    #[test]
    fn deprecated_named_placeholders_never_register() {
        for pattern in ["/:splat", "/:captures"] {
            let found = declarations(&format!("get '{pattern}' => sub {{ 1 }};"));
            assert_eq!(found.len(), 1);
            assert!(
                matches!(effective_of(&found[0]), RouteEffectivePattern::Boundary { .. }),
                "upstream croaks on `{pattern}`; the route has no effective path"
            );
            assert_eq!(
                found[0].registration,
                RouteRegistration::NeverRegisters,
                "the route never registers upstream"
            );
        }
        // The typed spelling is not subject to the deprecated-name croak.
        let found = declarations("get '/:splat[Int]' => sub { 1 };");
        assert!(matches!(effective_of(&found[0]), RouteEffectivePattern::Local { .. }));
        assert_eq!(found[0].registration, RouteRegistration::Registers);
    }

    #[test]
    fn same_parameter_name_in_two_routes_stays_route_local() {
        let found = declarations("get '/a/:id' => sub { 1 };\nget '/b/:id' => sub { 2 };\n");
        assert_eq!(found.len(), 2);
        for route in &found {
            assert_eq!(route.parameters.len(), 1);
            assert_eq!(route.parameters[0].name.as_deref(), Some("id"));
        }
        assert_ne!(
            found[0].route.declaration_index, found[1].route.declaration_index,
            "independent route identities"
        );
        // Route-scoped segments anchor inside their own pattern operand.
        let first = found[0].parameters[0].anchor.start_byte;
        let second = found[1].parameters[0].anchor.start_byte;
        assert!(first < second);
    }

    #[test]
    fn issue_fixture_composes_prefix_params_and_handler() {
        let code = "use Dancer2;\nprefix '/api';\nget '/users/:id' => sub {\n    my $id = route_parameters->{id};\n};\n";
        let found = contexts(code);
        assert_eq!(found.prefixes.len(), 1);
        assert_eq!(found.routes.len(), 1);
        let route = &found.routes[0];
        assert_eq!(composed_value(route), "/api/users/:id");
        assert_eq!(route.parameters.len(), 1);
        assert_eq!(route.parameters[0].name.as_deref(), Some("id"));
        assert!(matches!(route.route.handler, RouteHandler::InlineSub { .. }));
    }

    #[test]
    fn unrelated_calls_are_not_routes() {
        assert!(declarations("print '/x';\nget_something '/x' => sub { 1 };").is_empty());
    }
}

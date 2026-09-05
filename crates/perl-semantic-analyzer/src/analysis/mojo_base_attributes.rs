//! Mojo::Base `has` attribute-declaration extraction (#9682).
//!
//! Extracts the statically supported `Mojo::Base` attribute grammar from an
//! AST into [`MojoBaseAttributeDeclaration`] carriers for the
//! registry-activated minting in
//! `perl_semantic_facts::framework_adapters::mojo_base_facts`. This is pure
//! source observation: extraction knows the reviewed `has` grammar, it does
//! **not** decide activation — an object fact exists only after the registry
//! adapter minted it over an exact #9681 activation. A `has` call is never
//! activation evidence on its own.
//!
//! Supported forms (reviewed `Mojo::Base::attr` profile):
//!
//! ```perl
//! has 'name';
//! has name => 'default';
//! has name => sub { ... };
//! has [qw(host port)];
//! has [qw(host port)] => 'default';
//! ```
//!
//! **Known unmodeled spelling.** A bare `qw` list with no brackets and no
//! parentheses — `has qw(name default);` — is not extracted, and unlike every
//! other unsupported form here it yields no typed boundary either. The parser
//! does not bind the `qw` list to the `has` bareword: it emits two sibling
//! statements (a bare `Identifier` and a free-standing `ArrayLiteral`), so no
//! `has` declaration shape is present to observe. Recognizing it would mean
//! stitching two statements back together in this extractor to compensate for
//! a parse shape, which belongs upstream in the parser rather than here. The
//! spelling is legal Perl (`qw` flattens, so it means name plus default, not
//! two attributes) but is rare in reviewed `Mojo::Base` source, which spells a
//! multi-attribute declaration `has [qw(...)]`. Tracked as #14808; see
//! `a_bare_qw_list_without_brackets_is_a_known_unmodeled_spelling`.
//!
//! `Mojo::Base` binds the first operand to the attribute name (or an array
//! reference of names) and the optional second operand to the default, so
//! `has 'a', 'b';` declares the attribute `a` with default `'b'` — it is not
//! two attributes. Every generated accessor is read-write and a write returns
//! the invocant; those semantics live on the facts side.
//!
//! Package scoping mirrors the #9681 activation walk: an unqualified file
//! defaults to `main`, bare `package X;` switches the current package for
//! following statements, and a lexical block restores the enclosing package
//! state afterwards. Subroutine bodies are not descended: `Mojo::Base`
//! attributes are declared at package level, and a `has` call inside a sub
//! body runs at call time rather than declaring a class attribute.
//!
//! The parser shapes three different trees for the reviewed forms, all
//! handled here:
//!
//! - `has NAME [, DEFAULT]` is an ordinary `has(...)` function call;
//! - `has [LIST]` parses as an index expression over the `has` bareword
//!   (`Binary { op: "[]" }`), because the bracket follows the identifier;
//! - `has [LIST] => DEFAULT` wraps that index expression as the key of a
//!   one-pair hash literal.

use crate::analysis::dancer2_handler_targets::SubroutineTargetIndex;
use crate::ast::{Node, NodeKind};
use perl_semantic_facts::framework_adapters::mojo_base_facts::{
    MojoBaseAttributeDeclaration, MojoBaseAttributeDefault, MojoBaseAttributeName,
    MojoBaseExplicitMethodState,
};
use perl_semantic_facts::{AnchorId, FileId, SourceAnchor, SourceGeneration};

/// The `Mojo::Base` attribute-declaration keyword.
const HAS_KEYWORD: &str = "has";

/// Extract every supported `Mojo::Base` `has` attribute declaration from
/// `ast`, in source order.
///
/// Each declaration carries its owning package, the `has` statement's
/// source-order index, and — for an array-reference name list — the position
/// of the name inside that list, so one statement's names never collide.
/// `generation` is the source generation `ast` was parsed from and is retained
/// on every declaration, so minting can refuse a carrier from an older parse
/// instead of restamping it as current.
///
/// Declarations are emitted for every package in the file. Restricting them
/// to an activated package is the minting side's contract, not extraction's:
/// this function deliberately reports `has` calls it observed without
/// claiming they generate anything.
///
/// Only declarations that run unconditionally when the package is loaded are
/// extracted. A `has` call inside a subroutine body, a conditional, a loop, or
/// an `eval`/`try` block does not unconditionally declare a class attribute —
/// it runs when (and if) that construct runs — so extracting it would claim an
/// accessor exists on paths where the call never executes. A bare lexical
/// block is not control flow and is still extracted.
#[must_use]
pub fn extract_mojo_base_attribute_declarations(
    ast: &Node,
    file_id: FileId,
    generation: SourceGeneration,
) -> Vec<MojoBaseAttributeDeclaration> {
    let subroutines = SubroutineTargetIndex::build(ast, file_id);
    let mut state = WalkState {
        file_id,
        generation,
        subroutines: &subroutines,
        next_declaration_index: 0,
        declarations: Vec::new(),
    };
    // An unqualified file's caller package is `main` in Perl, matching the
    // #9681 activation walk.
    let mut current_package: Option<String> = Some("main".to_string());
    state.walk(ast, &mut current_package);
    state.declarations
}

struct WalkState<'a> {
    file_id: FileId,
    generation: SourceGeneration,
    subroutines: &'a SubroutineTargetIndex,
    next_declaration_index: u32,
    declarations: Vec<MojoBaseAttributeDeclaration>,
}

/// Whether this node owns runtime control flow, so statements inside it do not
/// run unconditionally at package load.
///
/// A bare lexical block is deliberately absent: `{ has 'x'; }` at package level
/// executes exactly once, like any other package statement.
fn owns_runtime_control_flow(node: &Node) -> bool {
    matches!(
        node.kind,
        NodeKind::If { .. }
            | NodeKind::While { .. }
            | NodeKind::For { .. }
            | NodeKind::Foreach { .. }
            | NodeKind::Given { .. }
            | NodeKind::When { .. }
            | NodeKind::Eval { .. }
            | NodeKind::Try { .. }
    )
}

impl WalkState<'_> {
    fn walk(&mut self, node: &Node, current_package: &mut Option<String>) {
        match &node.kind {
            NodeKind::Program { statements } => {
                // File scope: a bare `package X;` persists for the rest of the
                // file.
                for statement in statements {
                    self.walk(statement, current_package);
                }
                return;
            }
            NodeKind::Block { statements } => {
                // A lexical block scopes statement-form `package X;`
                // declarations: walk with a block-local copy so the enclosing
                // package state is restored afterwards.
                let mut block_package = current_package.clone();
                for statement in statements {
                    self.walk(statement, &mut block_package);
                }
                return;
            }
            NodeKind::Package { name, block: Some(block), .. } => {
                if let NodeKind::Block { statements } = &block.kind {
                    let mut package_scope = Some(name.clone());
                    for statement in statements {
                        self.walk(statement, &mut package_scope);
                    }
                }
                return;
            }
            NodeKind::Package { name, block: None, .. } => {
                *current_package = Some(name.clone());
            }
            // `Mojo::Base` attributes are declared at package level. A `has`
            // call inside a sub body executes at call time and does not
            // declare a class attribute, so sub bodies are not descended.
            NodeKind::Subroutine { .. } => return,
            // Same rule for runtime control flow: a `has` under a conditional,
            // loop, or eval/try runs only when that construct runs, so it
            // never unconditionally declares a class attribute.
            _ if owns_runtime_control_flow(node) => return,
            // A collected `has` statement is fully consumed here: descending
            // into its operands would re-observe them as ordinary source.
            NodeKind::ExpressionStatement { expression }
                if self.collect_declaration(expression, current_package.as_deref()) =>
            {
                return;
            }
            _ => {}
        }
        for child in node.children() {
            self.walk(child, current_package);
        }
    }

    /// Collect one `has` declaration from a statement expression.
    ///
    /// Returns whether the expression was a `has` declaration (and therefore
    /// must not be descended into as ordinary source).
    fn collect_declaration(&mut self, expression: &Node, package: Option<&str>) -> bool {
        let Some(parsed) = parse_has_expression(expression) else {
            return false;
        };
        let declaration_index = self.next_declaration_index;
        self.next_declaration_index += 1;
        let declaration_anchor =
            anchor(expression.location.start, expression.location.end, self.file_id);
        for (name_index, (name, name_node)) in parsed.names.into_iter().enumerate() {
            let name_anchor = anchor(name_node.0, name_node.1, self.file_id);
            let explicit_method = match name.literal() {
                Some(literal) if self.subroutines.resolve(literal, package).is_some() => {
                    MojoBaseExplicitMethodState::Collides
                }
                _ => MojoBaseExplicitMethodState::None,
            };
            self.declarations.push(MojoBaseAttributeDeclaration {
                declaration_index,
                name_index: span_u32(name_index),
                file_id: self.file_id,
                package: package.map(ToString::to_string),
                declaration_anchor,
                name_anchor,
                name,
                default: parsed.default.clone(),
                explicit_method,
                unmodeled_options: parsed.unmodeled_options.clone(),
                source_generation: self.generation.clone(),
            });
        }
        true
    }
}

/// One parsed `has` statement: its name selections with their source ranges,
/// plus the shared default evidence.
struct ParsedHas {
    names: Vec<(MojoBaseAttributeName, (usize, usize))>,
    default: MojoBaseAttributeDefault,
    unmodeled_options: Vec<String>,
}

/// Recognize the three parser shapes of a reviewed `has` statement.
fn parse_has_expression(expression: &Node) -> Option<ParsedHas> {
    match &expression.kind {
        // `has NAME;` / `has NAME => DEFAULT;` / `has(NAME, DEFAULT)`
        NodeKind::FunctionCall { name, args } if name == HAS_KEYWORD => {
            let (name_operand, rest) = args.split_first()?;
            let (default, unmodeled_options) = default_and_options(rest);
            Some(ParsedHas { names: names_from_operand(name_operand), default, unmodeled_options })
        }
        // `has [LIST];` — the bracket follows the bareword, so the parser
        // shapes it as an index expression rather than a call.
        NodeKind::Binary { .. } => {
            let list = has_index_list(expression)?;
            Some(ParsedHas {
                names: names_from_operand(list),
                default: MojoBaseAttributeDefault::Absent,
                unmodeled_options: Vec::new(),
            })
        }
        // `has [LIST] => DEFAULT;` — the index expression becomes the key of a
        // one-pair hash literal.
        NodeKind::HashLiteral { pairs } => {
            let [(key, value)] = pairs.as_slice() else {
                return None;
            };
            let list = has_index_list(key)?;
            Some(ParsedHas {
                names: names_from_operand(list),
                default: classify_default(value),
                unmodeled_options: Vec::new(),
            })
        }
        _ => None,
    }
}

/// The list operand of a `has [LIST]` index expression, when this node is one.
fn has_index_list(node: &Node) -> Option<&Node> {
    let NodeKind::Binary { op, left, right } = &node.kind else {
        return None;
    };
    if op != "[]" {
        return None;
    }
    let NodeKind::Identifier { name } = &left.kind else {
        return None;
    };
    if name != HAS_KEYWORD {
        return None;
    }
    Some(right)
}

/// Name selections contributed by one `has` name operand.
///
/// An array-reference operand contributes one selection per element; every
/// other operand contributes exactly one.
fn names_from_operand(operand: &Node) -> Vec<(MojoBaseAttributeName, (usize, usize))> {
    match &operand.kind {
        NodeKind::ArrayLiteral { elements } if !elements.is_empty() => elements
            .iter()
            .map(|element| (classify_name(element), (element.location.start, element.location.end)))
            .collect(),
        // An empty list declares nothing; keep it an explicit malformed
        // selection rather than silently dropping the statement.
        NodeKind::ArrayLiteral { .. } => vec![(
            MojoBaseAttributeName::Malformed {
                reason: "empty attribute-name list declares no attribute".to_string(),
            },
            (operand.location.start, operand.location.end),
        )],
        _ => vec![(classify_name(operand), (operand.location.start, operand.location.end))],
    }
}

/// Classify one attribute-name operand.
///
/// Only a static string spelling names a method. An interpolated spelling
/// whose value is computed, a variable, and every other expression stay
/// explicit dynamic boundaries: a guessed accessor name would be a fabricated
/// member.
fn classify_name(node: &Node) -> MojoBaseAttributeName {
    match &node.kind {
        NodeKind::String { value, interpolated } => {
            if *interpolated && interpolated_value_is_dynamic(value) {
                return MojoBaseAttributeName::Dynamic {
                    reason: "interpolated attribute name is computed at runtime".to_string(),
                };
            }
            match unquote(value) {
                Some(name) if !name.contains('\\') => MojoBaseAttributeName::Literal(name),
                // Escapes are not evaluated here, so the runtime name may
                // differ from the source bytes.
                Some(_) => MojoBaseAttributeName::Malformed {
                    reason: "attribute name contains unevaluated escape sequences".to_string(),
                },
                None => {
                    MojoBaseAttributeName::Malformed { reason: "empty attribute name".to_string() }
                }
            }
        }
        NodeKind::Identifier { name } => MojoBaseAttributeName::Literal(name.clone()),
        NodeKind::Variable { .. } => MojoBaseAttributeName::Dynamic {
            reason: "attribute name comes from a variable".to_string(),
        },
        _ => MojoBaseAttributeName::Dynamic {
            reason: "attribute name is a computed expression".to_string(),
        },
    }
}

/// Default and option evidence contributed by the operands following the
/// name.
///
/// `Mojo::Base::attr` binds `($self, $attrs, $value, %kv)`: the operand after
/// the name is the default, and anything after that is a key/value option
/// list. The corpus spells `has app => undef, weak => 1;`, so trailing pairs
/// are ordinary supported syntax — not extra operands — even though this
/// profile does not model what each option means. Option keys are returned so
/// the fact side can limit the reader without disturbing the accessor
/// identity. An odd trailing operand cannot be a `%kv` list at all and stays
/// an explicit unsupported boundary.
fn default_and_options(rest: &[Node]) -> (MojoBaseAttributeDefault, Vec<String>) {
    let Some((default, options)) = rest.split_first() else {
        return (MojoBaseAttributeDefault::Absent, Vec::new());
    };
    if options.is_empty() {
        return (classify_default(default), Vec::new());
    }
    if options.len() % 2 != 0 {
        return (
            MojoBaseAttributeDefault::Unsupported {
                reason: "trailing operands cannot form the `%kv` option list `Mojo::Base::attr` \
                         binds"
                    .to_string(),
            },
            Vec::new(),
        );
    }
    let keys = options
        .iter()
        .step_by(2)
        .map(|key| match &key.kind {
            NodeKind::String { value, .. } => {
                unquote(value).unwrap_or_else(|| "<empty>".to_string())
            }
            NodeKind::Identifier { name } => name.clone(),
            _ => "<computed>".to_string(),
        })
        .collect();
    (classify_default(default), keys)
}

/// Classify one default operand.
///
/// `Mojo::Base` admits a constant value or a code reference that builds the
/// value lazily; it croaks on any other reference. An explicit `undef`
/// default is indistinguishable from no default at runtime (the accessor
/// stores undef either way), so it is reported as absent rather than as a
/// value-shaped constant.
fn classify_default(node: &Node) -> MojoBaseAttributeDefault {
    match &node.kind {
        NodeKind::Undef => MojoBaseAttributeDefault::Absent,
        NodeKind::Number { .. } => MojoBaseAttributeDefault::Constant,
        NodeKind::String { value, interpolated } => {
            if *interpolated && interpolated_value_is_dynamic(value) {
                MojoBaseAttributeDefault::Dynamic {
                    reason: "interpolated default is computed at runtime".to_string(),
                }
            } else {
                MojoBaseAttributeDefault::Constant
            }
        }
        NodeKind::Subroutine { name: None, .. } => MojoBaseAttributeDefault::LazyBuilder,
        NodeKind::ArrayLiteral { .. } | NodeKind::HashLiteral { .. } => {
            MojoBaseAttributeDefault::Unsupported {
                reason: "Mojo::Base rejects a non-code reference default at runtime".to_string(),
            }
        }
        NodeKind::Variable { .. } => MojoBaseAttributeDefault::Dynamic {
            reason: "default comes from a variable".to_string(),
        },
        _ => MojoBaseAttributeDefault::Dynamic {
            reason: "default is a computed expression".to_string(),
        },
    }
}

fn span_u32(value: usize) -> u32 {
    value.min(u32::MAX as usize) as u32
}

fn anchor(start: usize, end: usize, file_id: FileId) -> SourceAnchor {
    SourceAnchor::new(Some(AnchorId(start as u64)), file_id, span_u32(start), span_u32(end))
}

/// Strip one matched pair of surrounding quotes, if present.
///
/// The parser retains the raw token spelling for quoted strings and drops the
/// quotes for fat-comma autoquoted barewords and `qw` words, so both shapes
/// reach here.
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
/// Perl interpolation only occurs through `$`/`@` sigils followed by an
/// identifier or index, so a trailing sigil stays static.
fn interpolated_value_is_dynamic(value: &str) -> bool {
    crate::analysis::dancer2_routes::interpolated_value_is_dynamic(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Parser;
    use perl_tdd_support::must;

    fn declarations(code: &str) -> Vec<MojoBaseAttributeDeclaration> {
        let mut parser = Parser::new(code);
        let ast = must(parser.parse());
        extract_mojo_base_attribute_declarations(&ast, FileId(1), SourceGeneration::known("gen-1"))
    }

    fn names(code: &str) -> Vec<String> {
        declarations(code)
            .iter()
            .filter_map(|declaration| declaration.name.literal().map(ToString::to_string))
            .collect()
    }

    #[test]
    fn quoted_and_autoquoted_names_are_literal() {
        assert_eq!(names("package App;\nhas 'name';\nhas other => 1;\n"), ["name", "other"]);
    }

    #[test]
    fn an_array_reference_declares_one_attribute_per_name() {
        let found = declarations("package App;\nhas [qw(host port)];\n");
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].name.literal(), Some("host"));
        assert_eq!(found[1].name.literal(), Some("port"));
        assert_eq!(
            found[0].declaration_index, found[1].declaration_index,
            "one statement is one declaration index"
        );
        assert_eq!((found[0].name_index, found[1].name_index), (0, 1));
    }

    #[test]
    fn an_array_reference_with_a_default_keeps_both() {
        let found = declarations("package App;\nhas [qw(a b)] => 'shared';\n");
        assert_eq!(found.len(), 2);
        assert!(found.iter().all(|d| d.default == MojoBaseAttributeDefault::Constant));
        assert_eq!(names("package App;\nhas [qw(a b)] => 'shared';\n"), ["a", "b"]);
    }

    #[test]
    fn a_second_operand_is_the_default_not_a_second_attribute() {
        // `Mojo::Base::attr` binds one name and one default, so `has 'a', 'b'`
        // is the attribute `a` defaulting to `'b'`.
        let found = declarations("package App;\nhas 'a', 'b';\n");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name.literal(), Some("a"));
        assert_eq!(found[0].default, MojoBaseAttributeDefault::Constant);
    }

    #[test]
    fn a_sub_default_is_a_lazy_builder() {
        let found = declarations("package App;\nhas config => sub { {} };\n");
        assert_eq!(found[0].default, MojoBaseAttributeDefault::LazyBuilder);
    }

    #[test]
    fn an_undef_default_reports_as_absent() {
        let found = declarations("package App;\nhas 'name' => undef;\n");
        assert_eq!(found[0].default, MojoBaseAttributeDefault::Absent);
    }

    #[test]
    fn a_non_code_reference_default_is_unsupported() {
        let found = declarations("package App;\nhas 'list' => [];\n");
        assert!(matches!(found[0].default, MojoBaseAttributeDefault::Unsupported { .. }));
    }

    #[test]
    fn computed_names_and_defaults_stay_typed_boundaries() {
        let dynamic_name = declarations("package App;\nhas $field => 1;\n");
        assert!(matches!(dynamic_name[0].name, MojoBaseAttributeName::Dynamic { .. }));
        let dynamic_default = declarations("package App;\nhas 'name' => $value;\n");
        assert!(matches!(dynamic_default[0].default, MojoBaseAttributeDefault::Dynamic { .. }));
    }

    #[test]
    fn an_interpolated_name_is_dynamic_but_a_plain_one_is_literal() {
        let interpolated = declarations("package App;\nhas \"pre$suffix\";\n");
        assert!(matches!(interpolated[0].name, MojoBaseAttributeName::Dynamic { .. }));
        let plain = declarations("package App;\nhas \"plain\";\n");
        assert_eq!(plain[0].name.literal(), Some("plain"));
    }

    #[test]
    fn packages_scope_each_declaration() {
        let found = declarations("package App;\nhas 'a';\npackage Other;\nhas 'b';\n");
        assert_eq!(found[0].package.as_deref(), Some("App"));
        assert_eq!(found[1].package.as_deref(), Some("Other"));
    }

    #[test]
    fn a_lexical_block_restores_the_enclosing_package() {
        let found = declarations("package Outer;\n{ package Inner; has 'inner'; }\nhas 'outer';\n");
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].package.as_deref(), Some("Inner"));
        assert_eq!(found[1].package.as_deref(), Some("Outer"));
    }

    #[test]
    fn an_unqualified_file_defaults_to_main() {
        assert_eq!(declarations("has 'name';\n")[0].package.as_deref(), Some("main"));
    }

    #[test]
    fn a_has_call_inside_a_sub_body_is_not_a_class_attribute() {
        assert!(declarations("package App;\nsub build { has 'runtime'; }\n").is_empty());
    }

    #[test]
    fn trailing_option_pairs_are_bound_as_options_not_extra_operands() {
        // Verbatim from the bundled corpus
        // (test_corpus/real_projects/mojolicious_skeleton/lib/Mojolicious/
        // Controller.pm): `Mojo::Base::attr` binds ($self, $attrs, $value,
        // %kv), so `weak => 1` is a supported option, not a malformed extra
        // operand.
        let found = declarations("package App;\nhas app => undef, weak => 1;\n");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name.literal(), Some("app"));
        assert_eq!(
            found[0].default,
            MojoBaseAttributeDefault::Absent,
            "the default operand is `undef`, which stores undef either way"
        );
        assert_eq!(found[0].unmodeled_options, ["weak"], "the option key is recorded, not dropped");
    }

    #[test]
    fn an_odd_trailing_operand_cannot_be_an_option_list() {
        let found = declarations("package App;\nhas name => 'v', weak;\n");
        assert!(matches!(found[0].default, MojoBaseAttributeDefault::Unsupported { .. }));
        assert!(found[0].unmodeled_options.is_empty());
    }

    #[test]
    fn a_plain_declaration_records_no_options() {
        let found = declarations("package App;\nhas name => 'v';\n");
        assert_eq!(found[0].default, MojoBaseAttributeDefault::Constant);
        assert!(found[0].unmodeled_options.is_empty());
    }

    #[test]
    fn runtime_control_flow_does_not_declare_a_class_attribute() {
        // A `has` under a conditional or loop runs only when that construct
        // runs, so claiming an unconditional accessor would be an overclaim.
        assert!(declarations("package App;\nif ($c) { has 'cond'; }\n").is_empty());
        assert!(declarations("package App;\nfor (1..2) { has 'loop'; }\n").is_empty());
        assert!(declarations("package App;\nwhile ($c) { has 'spin'; }\n").is_empty());
        assert!(declarations("package App;\neval { has 'risky'; };\n").is_empty());
        // Control: a bare lexical block is not control flow and still counts.
        assert_eq!(names("package App;\n{ has 'bare'; }\n"), ["bare"]);
    }

    #[test]
    fn declarations_carry_the_extraction_generation() {
        let found = declarations("package App;\nhas 'name';\n");
        assert_eq!(found[0].source_generation, SourceGeneration::known("gen-1"));
    }

    #[test]
    fn a_bare_qw_list_without_brackets_is_a_known_unmodeled_spelling() {
        // Pins the documented limitation rather than asserting it is correct:
        // the parser emits `has` and the `qw` list as two sibling statements,
        // so no declaration shape reaches this extractor. The bracketed
        // spelling immediately below is the control proving the extractor
        // itself handles `qw` words fine — the gap is the unbracketed parse,
        // not `qw` support.
        assert!(declarations("package App;\nhas qw(name);\n").is_empty());
        assert!(declarations("package App;\nhas qw(name default);\n").is_empty());
        assert_eq!(names("package App;\nhas [qw(name other)];\n"), ["name", "other"]);
    }

    #[test]
    fn a_bare_has_bareword_declares_nothing() {
        assert!(declarations("package App;\nhas;\n").is_empty());
    }

    #[test]
    fn a_same_named_explicit_sub_is_recorded_as_a_collision() {
        let found = declarations("package App;\nhas 'name';\nsub name { 'explicit' }\n");
        assert_eq!(found[0].explicit_method, MojoBaseExplicitMethodState::Collides);
        let clean = declarations("package App;\nhas 'name';\nsub other { 1 }\n");
        assert_eq!(clean[0].explicit_method, MojoBaseExplicitMethodState::None);
    }

    #[test]
    fn the_declaration_anchor_covers_the_real_has_statement() {
        let code = "package App;\nhas 'name';\n";
        let found = declarations(code);
        let anchor = found[0].declaration_anchor;
        assert!(
            code[(anchor.start_byte as usize)..(anchor.end_byte as usize)].contains("has 'name'"),
            "the generator anchor must cover the declaring statement"
        );
        let name_anchor = found[0].name_anchor;
        assert_eq!(
            &code[(name_anchor.start_byte as usize)..(name_anchor.end_byte as usize)],
            "'name'",
            "the name anchor must cover the name operand"
        );
    }

    #[test]
    fn declaration_indices_follow_source_order() {
        let found = declarations("package App;\nhas 'a';\nhas 'b';\nhas 'c';\n");
        let indices: Vec<u32> = found.iter().map(|d| d.declaration_index).collect();
        assert_eq!(indices, [0, 1, 2]);
    }
}

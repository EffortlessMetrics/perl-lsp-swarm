//! `NodeKind` structural registry.
//!
//! One row per primary variant. This table is production authority for
//! [`crate::FieldId`] set membership and field-aware child traversal, and the
//! input to schema identity / NodeKind inventory. It does not drive rendering
//! or `source_boundary` classification.

use super::{
    ChildFieldSpec, FieldCardinality, GrammarNameSpec, KindBody, KindStructuralRow,
    SchemaCompatibility,
};
use crate::FieldId;

macro_rules! kind_row {
    (
        $name:literal,
        $body:ident,
        recovery = $recovery:literal,
        boundary = $boundary:literal,
        children = [$($field:ident: $card:ident),* $(,)?],
        static $grammar:literal
    ) => {
        KindStructuralRow {
            kind_name: $name,
            children: &[
                $(ChildFieldSpec {
                    field: FieldId::$field,
                    cardinality: FieldCardinality::$card,
                },)*
            ],
            body: KindBody::$body,
            recovery: $recovery,
            source_boundary: $boundary,
            grammar: GrammarNameSpec::Static($grammar),
            compatibility: SchemaCompatibility::Current,
        }
    };
    (
        $name:literal,
        $body:ident,
        recovery = $recovery:literal,
        boundary = $boundary:literal,
        children = [$($field:ident: $card:ident),* $(,)?],
        runtime [$($input:literal),* $(,)?]
    ) => {
        KindStructuralRow {
            kind_name: $name,
            children: &[
                $(ChildFieldSpec {
                    field: FieldId::$field,
                    cardinality: FieldCardinality::$card,
                },)*
            ],
            body: KindBody::$body,
            recovery: $recovery,
            source_boundary: $boundary,
            grammar: GrammarNameSpec::RuntimeDerived { inputs: &[$($input),*] },
            compatibility: SchemaCompatibility::Current,
        }
    };
}

/// Structural rows in `NodeKind` declaration order.
///
/// Field-aware walkers emit the [`FieldId`] values named here. Physical child
/// storage may interleave fields (`If` elsif, `HashLiteral` pairs); emission
/// order is the shared visit table, not a regrouping by this slice.
#[rustfmt::skip]
pub const NODE_KIND_STRUCTURAL_REGISTRY: &[KindStructuralRow<'static>] = &[
    kind_row!("Program", ChildBearing, recovery = false, boundary = false, children = [STATEMENTS: Repeated], static "source_file"),
    kind_row!("ExpressionStatement", ChildBearing, recovery = false, boundary = false, children = [EXPRESSION: Required], static "expression_statement"),
    kind_row!("VariableDeclaration", ChildBearing, recovery = false, boundary = false, children = [VARIABLE: Required, INITIALIZER: Optional], runtime ["declarator"]),
    kind_row!("VariableListDeclaration", ChildBearing, recovery = false, boundary = false, children = [VARIABLE: Repeated, INITIALIZER: Optional], runtime ["declarator"]),
    kind_row!("NestedVariableList", ChildBearing, recovery = false, boundary = false, children = [ITEMS: Repeated], static "nested_variable_list"),
    kind_row!("Variable", Leaf, recovery = false, boundary = false, children = [], static "variable"),
    kind_row!("VariableWithAttributes", ChildBearing, recovery = false, boundary = false, children = [VARIABLE: Required], static "variable_with_attributes"),
    kind_row!("Assignment", ChildBearing, recovery = false, boundary = false, children = [LHS: Required, RHS: Required], runtime ["op"]),
    kind_row!("Binary", ChildBearing, recovery = false, boundary = false, children = [LEFT: Required, RIGHT: Required], runtime ["op"]),
    kind_row!("ArraySlice", ChildBearing, recovery = false, boundary = false, children = [TARGET: Required, ELEMENTS: Required], static "array_slice"),
    kind_row!("HashSlice", ChildBearing, recovery = false, boundary = false, children = [TARGET: Required, KEY: Required], static "hash_slice"),
    kind_row!("KeyValueSlice", ChildBearing, recovery = false, boundary = false, children = [TARGET: Required, KEY: Required], static "key_value_slice"),
    kind_row!("ChainedComparison", ChildBearing, recovery = false, boundary = false, children = [ELEMENTS: Repeated], static "chained_comparison"),
    kind_row!("Ternary", ChildBearing, recovery = false, boundary = false, children = [CONDITION: Required, THEN_EXPR: Required, ELSE_EXPR: Required], static "ternary"),
    kind_row!("Unary", ChildBearing, recovery = false, boundary = false, children = [OPERAND: Required], runtime ["op"]),
    kind_row!("Diamond", Leaf, recovery = false, boundary = false, children = [], static "diamond"),
    kind_row!("Ellipsis", Leaf, recovery = false, boundary = false, children = [], static "ellipsis"),
    kind_row!("Undef", Leaf, recovery = false, boundary = false, children = [], static "undef"),
    kind_row!("Readline", Leaf, recovery = false, boundary = false, children = [], static "readline"),
    kind_row!("Glob", Leaf, recovery = false, boundary = false, children = [], static "glob"),
    kind_row!("Typeglob", Leaf, recovery = false, boundary = false, children = [], static "typeglob"),
    kind_row!("Number", Leaf, recovery = false, boundary = false, children = [], static "number"),
    kind_row!("String", Leaf, recovery = false, boundary = false, children = [], runtime ["interpolated"]),
    kind_row!("VString", Leaf, recovery = false, boundary = false, children = [], static "vstring"),
    kind_row!("Heredoc", Leaf, recovery = false, boundary = true, children = [], runtime ["interpolated", "indented", "command"]),
    kind_row!("ArrayLiteral", ChildBearing, recovery = false, boundary = false, children = [ELEMENTS: Repeated], static "array"),
    kind_row!("HashLiteral", ChildBearing, recovery = false, boundary = false, children = [KEY: Repeated, VALUE: Repeated], static "hash"),
    kind_row!("Block", ChildBearing, recovery = false, boundary = false, children = [STATEMENTS: Repeated], static "block"),
    kind_row!("Eval", ChildBearing, recovery = false, boundary = false, children = [BLOCK: Required], static "eval"),
    kind_row!("Do", ChildBearing, recovery = false, boundary = false, children = [BLOCK: Required], static "do"),
    kind_row!("Defer", ChildBearing, recovery = false, boundary = false, children = [BLOCK: Required], static "defer"),
    kind_row!("Try", ChildBearing, recovery = false, boundary = false, children = [BODY: Required, CATCH: Repeated, FINALLY: Optional], static "try"),
    kind_row!("If", ChildBearing, recovery = false, boundary = false, children = [CONDITION: Required, THEN_BRANCH: Required, BODY: Repeated, ELSE_BRANCH: Optional], runtime ["keyword"]),
    kind_row!("LabeledStatement", ChildBearing, recovery = false, boundary = false, children = [STATEMENT: Required], static "labeled_statement"),
    kind_row!("While", ChildBearing, recovery = false, boundary = false, children = [CONDITION: Required, BODY: Required, CONTINUE_BLOCK: Optional], runtime ["keyword"]),
    kind_row!("Tie", ChildBearing, recovery = false, boundary = false, children = [VARIABLE: Required, PACKAGE: Required, ARGS: Repeated], static "tie"),
    kind_row!("Untie", ChildBearing, recovery = false, boundary = false, children = [VARIABLE: Required], static "untie"),
    kind_row!("For", ChildBearing, recovery = false, boundary = false, children = [INIT: Optional, CONDITION: Optional, UPDATE: Optional, BODY: Required, CONTINUE_BLOCK: Optional], static "for"),
    kind_row!("Foreach", ChildBearing, recovery = false, boundary = false, children = [VARIABLE: Required, LIST: Required, BODY: Required, CONTINUE_BLOCK: Optional], static "foreach"),
    kind_row!("Given", ChildBearing, recovery = false, boundary = false, children = [EXPR: Required, BODY: Required], static "given"),
    kind_row!("When", ChildBearing, recovery = false, boundary = false, children = [CONDITION: Required, BODY: Required], static "when"),
    kind_row!("Default", ChildBearing, recovery = false, boundary = false, children = [BODY: Required], static "default"),
    kind_row!("StatementModifier", ChildBearing, recovery = false, boundary = false, children = [STATEMENT: Required, CONDITION: Required], runtime ["modifier"]),
    kind_row!("Subroutine", ChildBearing, recovery = false, boundary = false, children = [PROTOTYPE: Optional, SIGNATURE: Optional, BODY: Required], runtime ["name"]),
    kind_row!("Prototype", Leaf, recovery = false, boundary = false, children = [], static "prototype"),
    kind_row!("Signature", ChildBearing, recovery = false, boundary = false, children = [PARAMETERS: Repeated], static "signature"),
    kind_row!("MandatoryParameter", ChildBearing, recovery = false, boundary = false, children = [VARIABLE: Required], static "mandatory_parameter"),
    kind_row!("OptionalParameter", ChildBearing, recovery = false, boundary = false, children = [VARIABLE: Required, DEFAULT_VALUE: Required], static "optional_parameter"),
    kind_row!("SlurpyParameter", ChildBearing, recovery = false, boundary = false, children = [VARIABLE: Required], static "slurpy_parameter"),
    kind_row!("NamedParameter", ChildBearing, recovery = false, boundary = false, children = [VARIABLE: Required, DEFAULT_VALUE: Optional], static "named_parameter"),
    kind_row!("Method", ChildBearing, recovery = false, boundary = false, children = [SIGNATURE: Optional, BODY: Required], static "method_declaration_statement"),
    kind_row!("Return", ChildBearing, recovery = false, boundary = false, children = [VALUE: Optional], static "return"),
    kind_row!("LoopControl", Leaf, recovery = false, boundary = false, children = [], runtime ["op"]),
    kind_row!("Goto", ChildBearing, recovery = false, boundary = false, children = [TARGET: Required], static "goto"),
    kind_row!("MethodCall", ChildBearing, recovery = false, boundary = false, children = [OBJECT: Required, ARGS: Repeated], static "method_call"),
    kind_row!("FunctionCall", ChildBearing, recovery = false, boundary = false, children = [ARGS: Repeated], runtime ["name", "args"]),
    kind_row!("AmperCall", ChildBearing, recovery = false, boundary = false, children = [ARGS: Repeated], runtime ["args"]),
    kind_row!("IndirectCall", ChildBearing, recovery = false, boundary = false, children = [OBJECT: Required, ARGS: Repeated], static "indirect_call"),
    kind_row!("Regex", Leaf, recovery = false, boundary = false, children = [], static "regex"),
    kind_row!("Match", ChildBearing, recovery = false, boundary = false, children = [EXPR: Required], runtime ["negated"]),
    kind_row!("Substitution", ChildBearing, recovery = false, boundary = false, children = [EXPR: Required], static "substitution"),
    kind_row!("Transliteration", ChildBearing, recovery = false, boundary = false, children = [EXPR: Required], static "transliteration"),
    kind_row!("Package", ChildBearing, recovery = false, boundary = false, children = [BLOCK: Optional], static "package"),
    kind_row!("Use", Leaf, recovery = false, boundary = false, children = [], static "use"),
    kind_row!("No", Leaf, recovery = false, boundary = false, children = [], static "no"),
    kind_row!("PhaseBlock", ChildBearing, recovery = false, boundary = false, children = [BLOCK: Required], runtime ["phase"]),
    kind_row!("DataSection", Leaf, recovery = false, boundary = true, children = [], static "data_section"),
    kind_row!("Class", ChildBearing, recovery = false, boundary = false, children = [BODY: Required], static "class"),
    kind_row!("Format", Leaf, recovery = false, boundary = true, children = [], static "format"),
    kind_row!("Identifier", Leaf, recovery = false, boundary = false, children = [], static "identifier"),
    kind_row!("Error", ChildBearing, recovery = true, boundary = false, children = [PARTIAL: Optional], static "ERROR"),
    kind_row!("MissingExpression", Leaf, recovery = true, boundary = false, children = [], static "missing_expression"),
    kind_row!("MissingStatement", Leaf, recovery = true, boundary = false, children = [], static "missing_statement"),
    kind_row!("MissingIdentifier", Leaf, recovery = true, boundary = false, children = [], static "missing_identifier"),
    kind_row!("MissingBlock", Leaf, recovery = true, boundary = false, children = [], static "missing_block"),
    kind_row!("UnknownRest", Leaf, recovery = true, boundary = false, children = [], static "UNKNOWN_REST"),
];

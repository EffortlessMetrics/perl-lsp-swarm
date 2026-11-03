//! Focused evidence for the macro-generated [`FieldId`] catalog seam.
//!
//! `define_field_ids!` expands one `pub const` per canonical field name plus
//! the `ALL` inventory, `name()`, and `from_name()` lookup. The constants and
//! the lookup match arms are generated text, so the parity properties below
//! are the discriminating evidence for that seam: a typo in one invocation, a
//! duplicated canonical name, or a lookup arm that drifted from its constant
//! is red here.

use perl_ast::FieldId;
use std::collections::BTreeSet;

/// Every catalog entry must resolve through its own canonical name, in both
/// directions: `name()` returns exactly the string `from_name()` matches on.
#[test]
fn field_id_catalog_round_trips_every_entry() {
    assert!(!FieldId::ALL.is_empty(), "the field-id catalog must not be empty");
    for id in FieldId::ALL {
        let name = id.name();
        assert!(!name.is_empty(), "canonical field names must be non-empty");
        assert_eq!(
            FieldId::from_name(name),
            Some(*id),
            "{name}: from_name must resolve to the constant that names it"
        );
    }
}

/// Canonical names are the public compatibility inventory: a duplicate would
/// make `from_name` map one string onto two constants.
#[test]
fn field_id_catalog_names_are_unique() {
    let names: BTreeSet<&str> = FieldId::ALL.iter().map(|id| id.name()).collect();
    assert_eq!(
        names.len(),
        FieldId::ALL.len(),
        "duplicate canonical names in the field-id catalog"
    );
}

/// The lookup contract is exact and case-sensitive: near-miss spellings must
/// not resolve, so an unknown name stays a caller-visible error instead of
/// silently aliasing a different field.
#[test]
fn field_id_lookup_rejects_non_canonical_names() {
    assert_eq!(FieldId::from_name(""), None);
    assert_eq!(FieldId::from_name("Statements"), None);
    assert_eq!(FieldId::from_name("statements_extra"), None);
    assert_eq!(FieldId::from_name("statements "), None);
}

/// Pin the canonical spellings the external vocabulary depends on.
#[test]
fn field_id_catalog_pins_canonical_spellings() {
    let expected = [
        (FieldId::STATEMENTS, "statements"),
        (FieldId::EXPRESSION, "expression"),
        (FieldId::VARIABLE, "variable"),
        (FieldId::LEFT, "left"),
        (FieldId::RIGHT, "right"),
        (FieldId::CONDITION, "condition"),
        (FieldId::THEN_BRANCH, "then_branch"),
        (FieldId::ELSE_BRANCH, "else_branch"),
        (FieldId::OPERAND, "operand"),
        (FieldId::KEY, "key"),
        (FieldId::VALUE, "value"),
        (FieldId::BLOCK, "block"),
        (FieldId::BODY, "body"),
        (FieldId::INIT, "init"),
        (FieldId::UPDATE, "update"),
        (FieldId::TARGET, "target"),
        (FieldId::OBJECT, "object"),
        (FieldId::ARGS, "args"),
    ];
    for (id, name) in expected {
        assert_eq!(id.name(), name, "canonical spelling drifted for `{name}`");
    }
}

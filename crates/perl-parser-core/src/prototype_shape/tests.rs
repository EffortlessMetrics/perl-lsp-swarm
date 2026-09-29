//! Named falsifiers for the prototype-shape projector (#16810).

use super::{
    PrototypeCompleteness, PrototypeDefault, PrototypeRecovery, PrototypeReferent, PrototypeShape,
    PrototypeSlotKind, PrototypeSyntaxClass, raw_from_attribute,
};

fn project(raw: &str) -> PrototypeShape {
    PrototypeShape::project(raw)
}

fn kinds(shape: &PrototypeShape) -> Vec<String> {
    shape.slots().iter().map(|slot| slot.kind().tag()).collect()
}

fn optionality(shape: &PrototypeShape) -> Vec<bool> {
    shape.slots().iter().map(super::PrototypeSlot::is_optional).collect()
}

#[test]
fn a_scalar_prototype_is_one_required_scalar() {
    let shape = project("$");
    assert!(shape.is_exact(), "valid `$` must be exact");
    assert_eq!(kinds(&shape), ["scalar"]);
    assert_eq!(optionality(&shape), [false]);
    assert_eq!(shape.syntax_class(), PrototypeSyntaxClass::Ordinary);
}

#[test]
fn a_semicolon_makes_later_slots_optional_not_required() {
    let shape = project("$;$");
    assert!(shape.is_exact(), "`$;$` is a valid optional-scalar prototype");
    assert_eq!(kinds(&shape), ["scalar", "scalar"]);
    assert_eq!(optionality(&shape), [false, true]);
    assert_eq!(shape.first_optional_index(), Some(1));
    let boundary = match shape.optional_boundary() {
        Some(boundary) => boundary,
        None => panic!("`;` boundary missing for `$;$`"),
    };
    assert_eq!(&shape.raw()[boundary.start()..boundary.end()], ";");
}

#[test]
fn treating_all_slots_as_required_fails_the_optional_boundary() {
    let same_if_optionality_ignored = project("$$");
    let with_boundary = project("$;$");
    assert_ne!(
        same_if_optionality_ignored.semantic_digest(),
        with_boundary.semantic_digest(),
        "raw-string or required-only models cannot distinguish `$$` from `$;$`"
    );
}

#[test]
fn underscore_is_topic_default_not_a_plain_scalar() {
    let topic = project("_");
    let scalar = project("$");
    assert!(topic.is_exact());
    assert_eq!(kinds(&topic), ["topic-default-scalar"]);
    assert_eq!(topic.slots()[0].default(), PrototypeDefault::TopicVariable);
    assert_ne!(topic.semantic_digest(), scalar.semantic_digest(), "`_` must not collapse to `$`");
}

#[test]
fn topic_default_after_semicolon_is_optional() {
    let shape = project(";_");
    assert!(shape.is_exact(), "`(;_)` is the documented topic-default form");
    assert_eq!(kinds(&shape), ["topic-default-scalar"]);
    assert_eq!(optionality(&shape), [true]);
    assert_eq!(shape.slots()[0].default(), PrototypeDefault::TopicVariable);
}

#[test]
fn underscore_before_dollar_is_not_exact() {
    let shape = project("_$");
    assert!(!shape.is_exact(), "`_` is only last, or immediately before `;`, `@`, or `%`");
    assert!(matches!(
        shape.completeness(),
        PrototypeCompleteness::Recovered { reason: PrototypeRecovery::InvalidTopicDefaultPosition }
    ));
}

#[test]
fn underscore_immediately_before_slurpy_is_exact() {
    let array = project("_@");
    let hash = project("_%");
    assert!(array.is_exact(), "`_@` is admitted by perlsub");
    assert!(hash.is_exact(), "`_%` is admitted by perlsub");
    assert_eq!(kinds(&array), ["topic-default-scalar", "array-slurpy"]);
    assert_eq!(kinds(&hash), ["topic-default-scalar", "hash-slurpy"]);
}

#[test]
fn whitespace_is_formatting_not_a_slot() {
    let compact = project("$;$");
    let spaced = project("$ ; $");
    assert_eq!(kinds(&compact), kinds(&spaced));
    assert_eq!(optionality(&compact), optionality(&spaced));
    assert_eq!(compact.semantic_digest(), spaced.semantic_digest());
    assert_ne!(compact.raw(), spaced.raw(), "raw spelling must still differ");
}

#[test]
fn empty_prototype_is_nullary_and_exact() {
    let shape = project("");
    assert!(shape.is_exact());
    assert!(shape.slots().is_empty());
    assert_eq!(shape.syntax_class(), PrototypeSyntaxClass::Nullary);
}

#[test]
fn whitespace_only_prototype_is_still_nullary() {
    let shape = project("  ");
    assert!(shape.is_exact());
    assert!(shape.slots().is_empty());
    assert_eq!(shape.syntax_class(), PrototypeSyntaxClass::Nullary);
}

#[test]
fn leading_ampersand_is_block_taking() {
    let shape = project("&@");
    assert!(shape.is_exact());
    assert_eq!(shape.syntax_class(), PrototypeSyntaxClass::BlockTaking);
    assert_eq!(kinds(&shape), ["code", "array-slurpy"]);
}

#[test]
fn backslashed_ampersand_is_not_block_taking() {
    let shape = project(r"\&@");
    assert!(shape.is_exact());
    assert_eq!(shape.syntax_class(), PrototypeSyntaxClass::Ordinary);
    assert_eq!(kinds(&shape), ["ref:code", "array-slurpy"]);
}

#[test]
fn glob_plus_and_refs_stay_ordered() {
    let shape = project(r"*+\$\@\%");
    assert!(shape.is_exact(), "glob, plus, and refs are admitted");
    assert_eq!(
        kinds(&shape),
        ["glob", "scalar-or-reference", "ref:scalar", "ref:array", "ref:hash"]
    );
}

#[test]
fn grouped_reference_is_one_slot() {
    let shape = project(r"\[$@%&*]");
    assert!(shape.is_exact());
    assert_eq!(shape.slots().len(), 1);
    match shape.slots()[0].kind() {
        PrototypeSlotKind::GroupedReference(referents) => {
            assert_eq!(
                referents,
                &[
                    PrototypeReferent::Scalar,
                    PrototypeReferent::Array,
                    PrototypeReferent::Hash,
                    PrototypeReferent::Code,
                    PrototypeReferent::Glob
                ]
            );
        }
        other => panic!("expected grouped reference, got {other:?}"),
    }
}

#[test]
fn duplicate_semicolon_cannot_be_exact() {
    let shape = project("$;;$");
    assert!(!shape.is_exact());
    assert!(matches!(
        shape.completeness(),
        PrototypeCompleteness::Recovered { reason: PrototypeRecovery::DuplicateOptionalBoundary }
    ));
}

#[test]
fn invalid_characters_cannot_be_exact() {
    let shape = project("XYZ");
    assert!(!shape.is_exact());
    assert!(matches!(
        shape.completeness(),
        PrototypeCompleteness::Recovered { reason: PrototypeRecovery::InvalidCharacter }
    ));
}

#[test]
fn unclosed_group_cannot_be_exact() {
    let shape = project(r"\[$@");
    assert!(!shape.is_exact());
    assert!(matches!(
        shape.completeness(),
        PrototypeCompleteness::Recovered { reason: PrototypeRecovery::UnclosedGroup }
    ));
}

#[test]
fn empty_group_cannot_be_exact() {
    let shape = project(r"\[]");
    assert!(!shape.is_exact());
    assert!(matches!(
        shape.completeness(),
        PrototypeCompleteness::Recovered { reason: PrototypeRecovery::EmptyGroup }
    ));
}

#[test]
fn dangling_backslash_cannot_be_exact() {
    let shape = project(r"\");
    assert!(!shape.is_exact());
    assert!(matches!(
        shape.completeness(),
        PrototypeCompleteness::Recovered { reason: PrototypeRecovery::DanglingBackslash }
    ));
}

#[test]
fn recovered_shapes_keep_raw_spelling() {
    let shape = project("$x");
    assert_eq!(shape.raw(), "$x");
    assert!(!shape.is_exact());
}

#[test]
fn attribute_body_strips_the_prototype_wrapper_only() {
    assert_eq!(raw_from_attribute("prototype($$)"), Some("$$"));
    assert_eq!(raw_from_attribute("prototype( $ $ )"), Some(" $ $ "));
    assert_eq!(raw_from_attribute("lvalue"), None);
}

#[test]
fn digest_is_independent_of_hash_iteration() {
    let a = project(r"\[$@]");
    let b = project(r"\[$@]");
    assert_eq!(a.semantic_digest(), b.semantic_digest());
    assert!(a.semantic_digest().as_str().starts_with("prototype-shape.v1"));
}

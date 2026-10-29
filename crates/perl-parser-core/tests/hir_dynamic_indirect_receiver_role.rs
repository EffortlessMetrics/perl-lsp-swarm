//! A dynamic indirect constructor receiver (`new $class`) is a runtime
//! value, not a static class bareword (#15031).
//!
//! Before this claim, the flat HIR lowerer recorded the written receiver of
//! `new $class;` as `BarewordRole::IndirectObject` — the same role as the
//! static class name in `new Foo;` — so consumers could not distinguish a
//! dynamic constructor target from a static one without sniffing the name
//! for a sigil. The receiver now records `DynamicIndirectObject`, with its
//! exact source range and identity preserved (the fact is not dropped, and
//! the synthesized-topic guard in
//! `hir_synthesized_topic_not_a_bareword.rs` still owns the zero-width
//! fabrication cases).

use perl_parser_core::Parser;
use perl_parser_core::hir::{BarewordFact, BarewordRole, HirFile, lower_ast};

fn lower(source: &str) -> HirFile {
    let mut parser = Parser::new(source);
    lower_ast(&parser.parse_with_recovery().ast)
}

fn receiver_fact(source: &str, name: &str) -> Option<BarewordFact> {
    lower(source).bareword_table.facts.into_iter().find(|fact| fact.name == name)
}

#[test]
fn dynamic_indirect_receivers_record_dynamic_role_at_exact_ranges() {
    for (source, name, start, end) in [
        ("new $class;", "$class", 4usize, 10usize),
        ("my $o = new $class;", "$class", 12, 18),
        ("my $o = new $class(@args);", "$class", 12, 18),
        ("new $class::Foo;", "$class::Foo", 4, 15),
    ] {
        let fact = receiver_fact(source, name)
            .unwrap_or_else(|| panic!("{source:?} must record its written receiver {name:?}"));
        assert_eq!(
            fact.role,
            BarewordRole::DynamicIndirectObject,
            "{source:?} receiver must be distinguishable from a static class name"
        );
        assert_eq!(
            (fact.range.start, fact.range.end),
            (start, end),
            "{source:?} receiver range must be preserved exactly"
        );
    }
}

#[test]
fn static_indirect_receivers_keep_indirect_object_role() {
    for (source, name) in [("new Foo;", "Foo"), ("new Foo::Bar(1);", "Foo::Bar")] {
        let fact = receiver_fact(source, name)
            .unwrap_or_else(|| panic!("{source:?} must record its static receiver {name:?}"));
        assert_eq!(
            fact.role,
            BarewordRole::IndirectObject,
            "{source:?} static receiver must keep the existing role"
        );
    }
}

#[test]
fn dynamic_receiver_is_not_also_recorded_as_static() {
    let file = lower("new $class;");
    assert!(
        !file
            .bareword_table
            .facts
            .iter()
            .any(|fact| fact.name == "$class" && fact.role == BarewordRole::IndirectObject),
        "the dynamic receiver must not keep a static-role fact: {:?}",
        file.bareword_table.facts.iter().map(|f| (&f.name, f.role)).collect::<Vec<_>>()
    );
}

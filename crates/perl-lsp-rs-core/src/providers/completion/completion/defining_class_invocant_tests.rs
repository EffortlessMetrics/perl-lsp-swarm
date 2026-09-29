//! Discriminating proof for defining-class invocant method completion (#16861).
//!
//! These tests go through `classify_receiver` and `CompletionProvider::get_completions`.
//! Spelling of `$self` / `$this` / `$class` is not receiver evidence. Canonical
//! value-shape facts are.

use super::receiver::{ReceiverEvidence, classify_receiver, classify_text_pattern_receiver};
use super::*;
use perl_parser_core::Parser;
use perl_tdd_support::{must, must_some_with};
use perl_workspace::workspace_index::WorkspaceIndex;
use std::sync::Arc;
use url::Url;

fn ctx_for(prefix: &str, current_package: &str, source_position: usize) -> CompletionContext {
    CompletionContext {
        position: source_position,
        trigger_character: None,
        in_string: false,
        in_regex: false,
        in_comment: false,
        in_use_statement: false,
        current_package: current_package.to_string(),
        prefix: prefix.to_string(),
        prefix_start: source_position.saturating_sub(prefix.len()),
        cursor_scope_id: 0,
    }
}

fn arrow_ctx(source: &str, prefix: &str, current_package: &str) -> CompletionContext {
    let start = must_some_with(source.rfind(prefix), "source must contain the arrow prefix");
    ctx_for(prefix, current_package, start + prefix.len())
}

fn parse(source: &str) -> perl_parser_core::ast::Node {
    let mut parser = Parser::new(source);
    must(parser.parse())
}

fn animal_index() -> Result<Arc<WorkspaceIndex>, Box<dyn std::error::Error>> {
    let index = Arc::new(WorkspaceIndex::new());
    // Canonical initial-name fixture seeding (#11301 burndown): these files
    // are on-disk workspace members, not live documents.
    index.index_initial_file(
        Url::parse("file:///workspace/Animal.pm")?,
        "package Animal;\nsub name { }\nsub speak { }\n1;\n".to_string(),
    )?;
    index.index_initial_file(
        Url::parse("file:///workspace/Other.pm")?,
        "package Other;\nsub name { }\nsub fetch { }\n1;\n".to_string(),
    )?;
    index.index_initial_file(
        Url::parse("file:///workspace/Dog.pm")?,
        "package Dog;\nuse parent 'Animal';\nsub fetch { }\n1;\n".to_string(),
    )?;
    Ok(index)
}

fn completions_for(
    source: &str,
    prefix: &str,
    index: Arc<WorkspaceIndex>,
) -> Result<Vec<CompletionItem>, Box<dyn std::error::Error>> {
    let ast = parse(source);
    let provider = CompletionProvider::new_with_index_and_source(&ast, source, Some(index));
    let position = source.rfind(prefix).ok_or("missing arrow prefix")? + prefix.len();
    Ok(provider.get_completions(source, position))
}

fn has_label(completions: &[CompletionItem], label: &str) -> bool {
    completions.iter().any(|item| item.label == label)
}

fn exact_invocant_method(completions: &[CompletionItem], label: &str) -> bool {
    completions.iter().any(|item| {
        item.label == label
            && item.detail.as_deref().is_some_and(|detail| {
                detail.contains("receiver: type engine")
                    || detail.contains("receiver: self/this")
                    || detail.contains("receiver: source-backed object")
            })
    })
}

fn unknown_fallback_method(completions: &[CompletionItem], label: &str) -> bool {
    completions.iter().any(|item| {
        item.label == label
            && item.detail.as_deref().is_some_and(|detail| {
                detail.contains("receiver: unknown") || detail.contains("low confidence")
            })
    })
}

fn is_typed_defining_class_receiver(evidence: &ReceiverEvidence, package: &str) -> bool {
    matches!(evidence, ReceiverEvidence::SelfOrThis(pkg) if pkg == package)
}

#[test]
fn spelling_only_self_is_not_text_pattern_receiver() {
    let source = "package Animal;\n$self->";
    let ctx = ctx_for("$self->", "Animal", source.len());
    let ev = classify_text_pattern_receiver(&ctx, source);
    assert_eq!(
        ev,
        ReceiverEvidence::Unknown,
        "$self spelling plus current package must not create SelfOrThis"
    );
}

#[test]
fn spelling_only_self_matches_ordinary_unknown_lexical() {
    let source_self = "package Animal;\n$self->";
    let source_obj = "package Animal;\n$obj->";
    let self_ev =
        classify_receiver(&arrow_ctx(source_self, "$self->", "Animal"), source_self, None);
    let obj_ev = classify_receiver(&arrow_ctx(source_obj, "$obj->", "Animal"), source_obj, None);
    assert_eq!(self_ev, ReceiverEvidence::Unknown);
    assert_eq!(obj_ev, ReceiverEvidence::Unknown);
}

#[test]
fn list_declared_self_uses_canonical_receiver_facts() {
    let source = "package Animal;\nmethod speak {\n    my ($self) = @_;\n    $self->\n}\n";
    let ev = classify_receiver(&arrow_ctx(source, "$self->", "Animal"), source, None);
    assert!(
        is_typed_defining_class_receiver(&ev, "Animal"),
        "my ($self) = @_ should consume invocant facts, got {ev:?}"
    );
}

#[test]
fn shift_declared_self_uses_canonical_receiver_facts() {
    let source = "package Animal;\nmethod speak {\n    my $self = shift;\n    $self->\n}\n";
    let ev = classify_receiver(&arrow_ctx(source, "$self->", "Animal"), source, None);
    assert!(
        is_typed_defining_class_receiver(&ev, "Animal"),
        "my $self = shift should consume invocant facts, got {ev:?}"
    );
}

#[test]
fn signature_self_uses_canonical_receiver_facts() {
    let source = "package Animal;\nmethod speak($self) {\n    $self->\n}\n";
    let ev = classify_receiver(&arrow_ctx(source, "$self->", "Animal"), source, None);
    assert!(
        is_typed_defining_class_receiver(&ev, "Animal"),
        "signature $self should consume invocant facts, got {ev:?}"
    );
}

#[test]
fn signature_class_uses_canonical_receiver_facts() {
    let source = "package Animal;\nmethod new($class) {\n    $class->\n}\n";
    let ev = classify_receiver(&arrow_ctx(source, "$class->", "Animal"), source, None);
    assert!(
        is_typed_defining_class_receiver(&ev, "Animal"),
        "signature $class should consume existing class invocant facts, got {ev:?}"
    );
}

#[test]
fn reassigned_self_prefers_constructor_assignment() {
    let source = concat!(
        "package Animal;\n",
        "method speak {\n",
        "    my $self = shift;\n",
        "    $self = Other->new;\n",
        "    $self->\n",
        "}\n"
    );
    let ev = classify_receiver(&arrow_ctx(source, "$self->", "Animal"), source, None);
    assert_eq!(
        ev,
        ReceiverEvidence::ConstructorAssignment("Other".to_string()),
        "reassigned $self must not keep the defining-class receiver, got {ev:?}"
    );
}

#[test]
fn list_declared_self_completion_offers_defining_class_methods()
-> Result<(), Box<dyn std::error::Error>> {
    let source = "package Animal;\nmethod speak {\n    my ($self) = @_;\n    $self->\n}\n";
    let completions = completions_for(source, "$self->", animal_index()?)?;
    assert!(
        has_label(&completions, "speak") && exact_invocant_method(&completions, "name"),
        "proven $self-> should offer local speak and workspace Animal::name via canonical facts; got {:?}",
        completions.iter().map(|item| (&item.label, item.detail.as_deref())).collect::<Vec<_>>()
    );
    assert!(
        !has_label(&completions, "fetch"),
        "Other::fetch must not enter Animal $self-> results; got {:?}",
        completions.iter().map(|item| &item.label).collect::<Vec<_>>()
    );
    Ok(())
}

#[test]
fn class_invocant_completion_offers_defining_class_methods()
-> Result<(), Box<dyn std::error::Error>> {
    let source = "package Animal;\nmethod new {\n    my $class = shift;\n    $class->\n}\n";
    let completions = completions_for(source, "$class->", animal_index()?)?;
    assert!(
        exact_invocant_method(&completions, "name"),
        "proven $class-> should offer Animal methods via canonical facts; got {:?}",
        completions.iter().map(|item| (&item.label, item.detail.as_deref())).collect::<Vec<_>>()
    );
    assert!(
        !has_label(&completions, "fetch"),
        "Other::fetch must not enter Animal $class-> results"
    );
    Ok(())
}

#[test]
fn inherited_self_completion_offers_parent_methods() -> Result<(), Box<dyn std::error::Error>> {
    let source = "package Dog;\nuse parent 'Animal';\nmethod greet {\n    my $self = shift;\n    $self->\n}\n";
    let completions = completions_for(source, "$self->", animal_index()?)?;
    assert!(
        has_label(&completions, "speak") && has_label(&completions, "fetch"),
        "Dog $self-> should offer inherited Animal::speak and own fetch; got {:?}",
        completions.iter().map(|item| &item.label).collect::<Vec<_>>()
    );
    Ok(())
}

#[test]
fn spelling_only_self_does_not_get_exact_invocant_methods() -> Result<(), Box<dyn std::error::Error>>
{
    let source = "package Animal;\n$self->";
    let completions = completions_for(source, "$self->", animal_index()?)?;
    assert!(
        !exact_invocant_method(&completions, "speak"),
        "file-level $self spelling must not mint exact invocant methods; got {:?}",
        completions.iter().map(|item| (&item.label, item.detail.as_deref())).collect::<Vec<_>>()
    );
    let obj_source = "package Animal;\n$obj->";
    let obj_completions = completions_for(obj_source, "$obj->", animal_index()?)?;
    let self_unknown = unknown_fallback_method(&completions, "speak");
    let obj_unknown = unknown_fallback_method(&obj_completions, "speak");
    assert_eq!(
        self_unknown, obj_unknown,
        "$self spelling must follow the same unknown-receiver policy as $obj"
    );
    Ok(())
}

#[test]
fn ordinary_main_self_does_not_offer_animal_methods() -> Result<(), Box<dyn std::error::Error>> {
    let source = "sub helper {\n    my $self = $_[0];\n    $self->\n}\n";
    let completions = completions_for(source, "$self->", animal_index()?)?;
    assert!(
        !exact_invocant_method(&completions, "speak") && !has_label(&completions, "speak"),
        "main-package $self must not receive Animal methods; got {:?}",
        completions.iter().map(|item| &item.label).collect::<Vec<_>>()
    );
    Ok(())
}

#[test]
fn shadowed_inner_self_uses_nearest_invocant_package() {
    let source = concat!(
        "package Animal;\n",
        "method speak {\n",
        "    my $self = shift;\n",
        "    {\n",
        "        package Other;\n",
        "        my $self = shift;\n",
        "        $self->\n",
        "    }\n",
        "}\n"
    );
    let ev = classify_receiver(&arrow_ctx(source, "$self->", "Other"), source, None);
    let package = must_some_with(ev.package(), "shadowed invocant should still have a package");
    assert_eq!(package, "Other", "inner $self should follow the nearest package fact, got {ev:?}");
}

#[test]
fn list_declared_this_uses_canonical_receiver_facts() {
    let source = "package Animal;\nmethod speak {\n    my ($this) = @_;\n    $this->\n}\n";
    let ev = classify_receiver(&arrow_ctx(source, "$this->", "Animal"), source, None);
    assert!(
        is_typed_defining_class_receiver(&ev, "Animal"),
        "my ($this) = @_ should consume invocant facts, got {ev:?}"
    );
}

#[test]
fn earlier_package_self_does_not_take_later_package() {
    let source = concat!(
        "package Animal;\n",
        "method speak {\n",
        "    my ($self) = @_;\n",
        "    $self->\n",
        "}\n",
        "package Other;\n",
        "method fetch {\n",
        "    my ($self) = @_;\n",
        "    $self->\n",
        "}\n"
    );
    let animal_start = must_some_with(source.find("$self->"), "Animal $self-> site must exist");
    let other_start = must_some_with(source.rfind("$self->"), "Other $self-> site must exist");
    let animal_ev = classify_receiver(
        &ctx_for("$self->", "Animal", animal_start + "$self->".len()),
        source,
        None,
    );
    let other_ev = classify_receiver(
        &ctx_for("$self->", "Other", other_start + "$self->".len()),
        source,
        None,
    );
    assert!(
        is_typed_defining_class_receiver(&animal_ev, "Animal"),
        "earlier Animal $self-> must not leak Other, got {animal_ev:?}"
    );
    assert!(
        is_typed_defining_class_receiver(&other_ev, "Other"),
        "later Other $self-> must keep Other, got {other_ev:?}"
    );
}

#[test]
fn earlier_package_self_completion_does_not_offer_later_package_methods()
-> Result<(), Box<dyn std::error::Error>> {
    let source = concat!(
        "package Animal;\n",
        "method speak {\n",
        "    my ($self) = @_;\n",
        "    $self->\n",
        "}\n",
        "package Other;\n",
        "method fetch {\n",
        "    my ($self) = @_;\n",
        "    $self->\n",
        "}\n"
    );
    let animal_start = must_some_with(source.find("$self->"), "Animal $self-> site must exist");
    let ast = parse(source);
    let provider =
        CompletionProvider::new_with_index_and_source(&ast, source, Some(animal_index()?));
    let completions = provider.get_completions(source, animal_start + "$self->".len());
    assert!(
        has_label(&completions, "speak") && exact_invocant_method(&completions, "name"),
        "Animal $self-> should offer Animal methods via canonical facts; got {:?}",
        completions.iter().map(|item| (&item.label, item.detail.as_deref())).collect::<Vec<_>>()
    );
    assert!(
        !exact_invocant_method(&completions, "fetch"),
        "later Other::fetch must not enter as a canonical invocant method; got {:?}",
        completions.iter().map(|item| (&item.label, item.detail.as_deref())).collect::<Vec<_>>()
    );
    Ok(())
}

#[test]
fn file_level_self_after_package_block_is_unknown() {
    let source = concat!(
        "package Animal {\n",
        "    method speak { my ($self) = @_; }\n",
        "}\n",
        "$self->\n"
    );
    let ev = classify_receiver(&arrow_ctx(source, "$self->", "main"), source, None);
    assert_eq!(
        ev,
        ReceiverEvidence::Unknown,
        "file-level $self after a closed package block must not keep Animal, got {ev:?}"
    );
}

#[test]
fn file_level_class_after_signature_method_is_unknown() {
    let source = "package Animal;\nmethod new($class) { return 1; }\n$class->\n";
    let ev = classify_receiver(&arrow_ctx(source, "$class->", "Animal"), source, None);
    assert_eq!(
        ev,
        ReceiverEvidence::Unknown,
        "file-level $class after the method must not keep Animal, got {ev:?}"
    );
}

#[test]
fn numeric_self_assignment_is_not_defining_class_receiver() {
    let source = "package Animal;\nsub helper {\n    my $self = 42;\n    $self->\n}\n";
    let ev = classify_receiver(&arrow_ctx(source, "$self->", "Animal"), source, None);
    assert_eq!(
        ev,
        ReceiverEvidence::Unknown,
        "my $self = 42 must not become a defining-class invocant, got {ev:?}"
    );
}

#[test]
fn ordinary_package_sub_is_not_a_defining_class_method() {
    let source = "package Animal;\nsub helper {\n    my ($self) = @_;\n    $self->\n}\n";
    let ev = classify_receiver(&arrow_ctx(source, "$self->", "Animal"), source, None);
    assert_eq!(
        ev,
        ReceiverEvidence::Unknown,
        "ordinary package sub helper($self) must not become a defining-class invocant, got {ev:?}"
    );
}

#[test]
fn class_method_signature_is_admitted_defining_class_receiver() {
    let source = "class Animal {\n    method speak($self) {\n        $self->\n    }\n}\n";
    let ev = classify_receiver(&arrow_ctx(source, "$self->", "Animal"), source, None);
    assert_eq!(
        ev,
        ReceiverEvidence::SelfOrThis("Animal".to_string()),
        "core class method $self must be an admitted defining-class receiver, got {ev:?}"
    );
}

#[test]
fn scalar_reassignment_after_unpack_is_unknown() {
    let source = concat!(
        "package Animal;\n",
        "method speak {\n",
        "    my ($self) = @_;\n",
        "    $self = 42;\n",
        "    $self->\n",
        "}\n"
    );
    let ev = classify_receiver(&arrow_ctx(source, "$self->", "Animal"), source, None);
    assert_eq!(
        ev,
        ReceiverEvidence::Unknown,
        "my ($self)=@_; $self=42; $self-> must not keep Animal, got {ev:?}"
    );
}

#[test]
fn undef_reassignment_after_unpack_is_unknown() {
    let source = concat!(
        "package Animal;\n",
        "method speak {\n",
        "    my ($self) = @_;\n",
        "    $self = undef;\n",
        "    $self->\n",
        "}\n"
    );
    let ev = classify_receiver(&arrow_ctx(source, "$self->", "Animal"), source, None);
    assert_eq!(
        ev,
        ReceiverEvidence::Unknown,
        "undef reassignment must revoke the declared invocant, got {ev:?}"
    );
}

#[test]
fn inner_shadow_self_is_unknown() {
    let source = concat!(
        "package Animal;\n",
        "method speak {\n",
        "    my ($self) = @_;\n",
        "    {\n",
        "        my $self = 42;\n",
        "        $self->\n",
        "    }\n",
        "}\n"
    );
    let ev = classify_receiver(&arrow_ctx(source, "$self->", "Animal"), source, None);
    assert_eq!(
        ev,
        ReceiverEvidence::Unknown,
        "inner my $self = 42 must not keep the outer invocant, got {ev:?}"
    );
}

#[test]
fn declaration_after_cursor_is_unknown() {
    let source = concat!(
        "package Animal;\n",
        "method speak {\n",
        "    $self->\n",
        "    my ($self) = @_;\n",
        "}\n"
    );
    let start = must_some_with(source.find("$self->"), "cursor site must exist");
    let ev =
        classify_receiver(&ctx_for("$self->", "Animal", start + "$self->".len()), source, None);
    assert_eq!(
        ev,
        ReceiverEvidence::Unknown,
        "declaration after the cursor must not contribute, got {ev:?}"
    );
}

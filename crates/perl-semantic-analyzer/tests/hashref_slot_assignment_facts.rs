//! Negative controls for object-aware hashref-slot assignment.

use perl_semantic_analyzer::Parser;
use perl_semantic_analyzer::analysis::type_facts::{ShapeFact, TypeEvidence};
use perl_semantic_analyzer::analysis::type_inference::{PerlType, TypeInferenceEngine};

#[test]
fn ordinary_hashref_slot_assignment_remains_a_hash_shape() -> Result<(), String> {
    let code = "my $links = {}; my $tail = LinkedList::Node->new; $links->{child} = $tail;";
    let mut parser = Parser::new(code);
    let ast = parser.parse().map_err(|error| format!("parse failed: {error:?}"))?;
    let mut engine = TypeInferenceEngine::new();

    engine.infer(&ast).map_err(|error| format!("inference failed: {error:?}"))?;

    let links = engine.get_fact_at("links").ok_or_else(|| "missing links fact".to_string())?;
    let ShapeFact::Hash(shape) =
        links.shape.as_ref().ok_or_else(|| "missing links hash shape".to_string())?
    else {
        return Err("ordinary hashref assignment stopped producing a hash shape".to_string());
    };
    let child = shape
        .slots
        .get("child")
        .ok_or_else(|| "missing assigned hashref child slot".to_string())?;
    assert_eq!(child.ty, PerlType::Object("LinkedList::Node".to_string()));
    assert!(child.evidence.iter().any(|evidence| {
        matches!(
            evidence,
            TypeEvidence::HashRefSlot { base, key }
                if base == "$links" && key == "child"
        )
    }));
    Ok(())
}

pub(crate) fn generation_is_known(generation: &SourceGeneration) -> bool {
    matches!(generation, SourceGeneration::Known(value) if !value.trim().is_empty())
}

fn generation_is_well_formed(generation: &SourceGeneration) -> bool {
    !matches!(generation, SourceGeneration::Known(value) if value.trim().is_empty())
}

pub(crate) fn semantic_provenance_is_exact(provenance: SemanticProvenance) -> bool {
    matches!(
        provenance,
        SemanticProvenance::Known(
            Provenance::ExactAst
                | Provenance::DesugaredAst
                | Provenance::SemanticAnalyzer
                | Provenance::LiteralRequireImport
        )
    )
}

fn range_contains(envelope: &SemanticFactEnvelope, byte_offset: u32) -> bool {
    let anchor = &envelope.anchor;
    if anchor.start_byte == anchor.end_byte {
        byte_offset == anchor.start_byte
    } else {
        anchor.start_byte <= byte_offset && byte_offset < anchor.end_byte
    }
}

fn validate_envelope_structure(
    envelope: &SemanticFactEnvelope,
) -> Result<(), ProviderQueryContractError> {
    if envelope.anchor.start_byte > envelope.anchor.end_byte
        || matches!(&envelope.source_generation, SourceGeneration::Known(value) if value.trim().is_empty())
        || envelope.package.as_ref().is_some_and(|package| package.trim().is_empty())
        || envelope.producer == SemanticProducer::Unknown
    {
        return Err(ProviderQueryContractError::MalformedFact(envelope.fact_id));
    }

    let mut dependency_keys = BTreeSet::new();
    for dependency in envelope.invalidation_dependencies() {
        if dependency.dependency_key.trim().is_empty()
            || matches!(&dependency.generation, SourceGeneration::Known(value) if value.trim().is_empty())
            || !dependency_keys.insert(dependency.dependency_key.as_str())
        {
            return Err(ProviderQueryContractError::MalformedFact(envelope.fact_id));
        }
    }
    Ok(())
}

/// Whether two facts carry a canonical or explicit relation to the same target.
///
/// Package and scope equality intentionally do not establish identity. They may
/// bound a search, but two different entities in one package or lexical scope are
/// still different targets.
pub(crate) fn facts_are_related(left: &ProviderQueryFact, right: &ProviderQueryFact) -> bool {
    left.envelope.entity_id.is_some()
        && left.envelope.entity_id == right.envelope.entity_id
        || left
            .envelope
            .boundary
            .as_ref()
            .and_then(|boundary| boundary.boundary_id)
            == Some(right.envelope.fact_id)
        || right
            .envelope
            .boundary
            .as_ref()
            .and_then(|boundary| boundary.boundary_id)
            == Some(left.envelope.fact_id)
}

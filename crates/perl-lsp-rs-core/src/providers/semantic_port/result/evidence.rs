fn build_evidence(
    request: &ProviderQueryRequest,
    outcome: ProviderQueryOutcome,
    facts: &[ProviderQueryFact],
    completeness: Option<&ProviderCompletenessGrant>,
    mut input: ProviderQueryEvidenceInput,
    control_observation: ProviderQueryControlObservation,
) -> ProviderQueryEvidence {
    let mut producers: Vec<_> = facts
        .iter()
        .map(|fact| fact.envelope().producer)
        .collect();
    producers.retain(|producer| *producer != SemanticProducer::Unknown);
    producers.sort();
    producers.dedup();

    let provenance = summarize_provenance(facts, completeness);
    let confidence = summarize_confidence(facts, completeness);
    let freshness = summarize_freshness(facts, completeness);
    let primary_anchor = facts
        .iter()
        .find(|fact| fact.role().is_value())
        .or_else(|| facts.iter().find(|fact| fact.role().is_selector()))
        .map(|fact| fact.envelope().anchor);
    let boundary = facts
        .iter()
        .find_map(|fact| fact.envelope().boundary.clone())
        .or(input.boundary.take());
    let semantic_reason = summarize_reason(outcome, facts, input.semantic_reason);

    ProviderQueryEvidence {
        proof_class: proof_for_outcome(outcome),
        completeness: if completeness.is_some() {
            ProviderEvidenceCompleteness::Complete
        } else {
            ProviderEvidenceCompleteness::NotClaimed
        },
        completeness_authority: completeness.map(|grant| grant.authority().clone()),
        producers,
        provenance,
        confidence,
        freshness,
        document_generation: request.context.document_generation.clone(),
        workspace_generation: request.context.workspace_generation.clone(),
        primary_anchor,
        boundary,
        semantic_reason,
        traces: input.traces,
        limitations: input.limitations,
        terminal_state: input.terminal_state,
        control_observation,
        result_path: input.result_path,
    }
}

impl ProviderQueryResult {
    fn try_from_draft(
        request: &ProviderQueryRequest,
        control: &dyn ProviderQueryControl,
        draft: ProviderQueryResultDraft,
    ) -> Result<Self, ProviderQueryContractError> {
        if !request.is_well_formed() {
            return Err(ProviderQueryContractError::MalformedRequest);
        }
        let ProviderQueryResultDraft {
            outcome,
            mut facts,
            completeness,
            evidence: input,
        } = draft;
        facts.sort_by_key(|fact| fact.envelope().fact_id);
        reject_duplicate_fact_ids(&facts)?;
        validate_fact_subjects(request, &facts)?;
        validate_value_fact_kinds(request, &facts)?;
        validate_trace_surfaces(request, &input.traces)?;
        if completeness
            .as_ref()
            .is_some_and(|grant| !grant.matches(request))
        {
            return Err(ProviderQueryContractError::InvalidCompletenessGrant);
        }
        let control_observation = ProviderQueryControlObservation::capture(request, control);
        validate_terminal_claim(outcome, input.terminal_state, control_observation)?;

        let evidence = build_evidence(
            request,
            outcome,
            &facts,
            completeness.as_ref(),
            input,
            control_observation,
        );
        let result = Self {
            request: request.clone(),
            outcome,
            facts,
            evidence,
        };
        result.validate_internal(completeness.as_ref())?;
        Ok(result)
    }

    /// Original request bound to this result.
    #[must_use]
    pub const fn request(&self) -> &ProviderQueryRequest {
        &self.request
    }

    /// Query-level outcome.
    #[must_use]
    pub const fn outcome(&self) -> ProviderQueryOutcome {
        self.outcome
    }

    /// Canonical fact set supplying selection, values, and evidence.
    #[must_use]
    pub fn facts(&self) -> &[ProviderQueryFact] {
        &self.facts
    }

    /// Facts that selected the target at the request subject.
    pub fn selector_facts(&self) -> impl Iterator<Item = &SemanticFactEnvelope> {
        self.facts
            .iter()
            .filter(|fact| fact.role().is_selector())
            .map(ProviderQueryFact::envelope)
    }

    /// Facts returned to the provider.
    pub fn value_facts(&self) -> impl Iterator<Item = &SemanticFactEnvelope> {
        self.facts
            .iter()
            .filter(|fact| fact.role().is_value())
            .map(ProviderQueryFact::envelope)
    }

    /// Facts used only to support a qualified or no-value outcome.
    pub fn supporting_facts(&self) -> impl Iterator<Item = &SemanticFactEnvelope> {
        self.facts
            .iter()
            .filter(|fact| fact.role().is_supporting())
            .map(ProviderQueryFact::envelope)
    }

    /// Checked evidence derived from the same facts, request, and caller control.
    #[must_use]
    pub const fn evidence(&self) -> &ProviderQueryEvidence {
        &self.evidence
    }

    /// Whether this is an authoritative exact empty result.
    #[must_use]
    pub fn is_exact_empty(&self) -> bool {
        self.outcome == ProviderQueryOutcome::Exact && self.value_facts().next().is_none()
    }

    /// Revalidate this retained result against the intended request.
    pub fn validate_against(
        &self,
        request: &ProviderQueryRequest,
    ) -> Result<(), ProviderQueryContractError> {
        if &self.request != request {
            return Err(ProviderQueryContractError::RequestBindingMismatch);
        }
        let completeness_present =
            self.evidence.completeness == ProviderEvidenceCompleteness::Complete;
        self.validate_internal_presence(completeness_present)
    }

    fn validate_internal(
        &self,
        completeness: Option<&ProviderCompletenessGrant>,
    ) -> Result<(), ProviderQueryContractError> {
        self.validate_internal_presence(completeness.is_some())
    }

    fn validate_internal_presence(
        &self,
        completeness_present: bool,
    ) -> Result<(), ProviderQueryContractError> {
        validate_terminal_claim(
            self.outcome,
            self.evidence.terminal_state,
            self.evidence.control_observation,
        )?;

        let value_count = self
            .facts
            .iter()
            .filter(|fact| fact.role().is_value())
            .count();
        let supporting_count = self
            .facts
            .iter()
            .filter(|fact| fact.role().is_supporting())
            .count();
        let candidate_count = distinct_candidate_count(&self.facts);
        let any_stale = self
            .facts
            .iter()
            .any(|fact| fact.envelope().status() == SemanticFactStatus::Stale);
        let any_refused = self
            .facts
            .iter()
            .any(|fact| fact.envelope().status() == SemanticFactStatus::Refused);
        let has_dynamic_boundary = self
            .evidence
            .boundary
            .as_ref()
            .is_some_and(|boundary| is_dynamic_boundary(boundary.kind))
            || self.facts.iter().any(|fact| {
                fact.envelope()
                    .boundary
                    .as_ref()
                    .is_some_and(|boundary| is_dynamic_boundary(boundary.kind))
            });
        let has_refuse_boundary = self
            .evidence
            .boundary
            .as_ref()
            .is_some_and(|boundary| boundary.disposition == BoundaryDisposition::Refuse)
            || self.facts.iter().any(|fact| {
                fact.envelope()
                    .boundary
                    .as_ref()
                    .is_some_and(|boundary| {
                        boundary.disposition == BoundaryDisposition::Refuse
                    })
            });
        let all_current = self
            .facts
            .iter()
            .all(|fact| fact.is_generation_current(&self.request));
        let all_exact = self
            .facts
            .iter()
            .all(|fact| fact_is_exact_grade(fact, &self.request));

        match self.outcome {
            ProviderQueryOutcome::Exact => {
                if self.evidence.proof_class != ProviderProofClass::ExactRead
                    || self.evidence.terminal_state != ProviderQueryTerminalState::Completed
                    || self.evidence.result_path != ProviderResultPath::Primary
                    || !self.request.context.is_exact_ready()
                    || !self.evidence.limitations.is_empty()
                    || self.evidence.boundary.is_some()
                    || supporting_count != 0
                    || !all_exact
                    || !semantic_provenance_is_exact(self.evidence.provenance)
                    || self.evidence.confidence
                        != SemanticConfidence::Known(Confidence::High)
                    || self.evidence.freshness != SemanticFreshness::Fresh
                {
                    return invalid(self.outcome);
                }
                if value_count == 0 {
                    if !completeness_present
                        || self.evidence.completeness != ProviderEvidenceCompleteness::Complete
                        || self.evidence.completeness_authority.is_none()
                        || !self.evidence.producers.is_empty()
                    {
                        return Err(ProviderQueryContractError::MissingCompletenessGrant);
                    }
                } else if completeness_present
                    || self.evidence.completeness_authority.is_some()
                {
                    return Err(ProviderQueryContractError::UnexpectedCompletenessGrant);
                }
            }
            ProviderQueryOutcome::Degraded => {
                if completeness_present
                    || self.evidence.proof_class != ProviderProofClass::QualifiedRead
                    || self.evidence.terminal_state != ProviderQueryTerminalState::Completed
                    || self.evidence.result_path != ProviderResultPath::Primary
                    || value_count == 0
                    || !self.request.context.is_degraded_ready()
                    || !all_current
                    || any_stale
                    || any_refused
                    || (self
                        .facts
                        .iter()
                        .all(|fact| fact.envelope().status() == SemanticFactStatus::Exact)
                        && self.evidence.limitations.is_empty()
                        && self.evidence.boundary.is_none()
                        && self.request.context.readiness_state
                            != super::ProviderReadinessState::ReadyLimited)
                {
                    return invalid(self.outcome);
                }
            }
            ProviderQueryOutcome::Fallback => {
                if completeness_present
                    || self.evidence.proof_class != ProviderProofClass::FallbackOnly
                    || self.evidence.terminal_state != ProviderQueryTerminalState::Completed
                    || self.evidence.result_path != ProviderResultPath::Fallback
                    || value_count == 0
                    || !self.request.context.is_fallback_ready()
                    || !all_current
                    || any_stale
                    || any_refused
                    || !self
                        .evidence
                        .traces
                        .iter()
                        .any(|trace| trace.fallback_state == ProviderFallbackState::Fallback)
                {
                    return invalid(self.outcome);
                }
            }
            ProviderQueryOutcome::Refused => {
                require_no_values(value_count, self.outcome)?;
                if completeness_present
                    || self.evidence.proof_class != ProviderProofClass::RefusalOnly
                    || self.evidence.terminal_state != ProviderQueryTerminalState::Completed
                    || self.evidence.result_path != ProviderResultPath::Primary
                    || !(any_refused
                        || has_refuse_boundary
                        || self.evidence.semantic_reason
                            == SemanticReasonCode::UnsupportedEffect)
                {
                    return invalid(self.outcome);
                }
            }
            ProviderQueryOutcome::Stale => {
                require_no_values(value_count, self.outcome)?;
                if completeness_present
                    || self.evidence.proof_class != ProviderProofClass::RefusalOnly
                    || self.evidence.terminal_state != ProviderQueryTerminalState::Completed
                    || self.evidence.result_path != ProviderResultPath::Primary
                    || self.evidence.semantic_reason != SemanticReasonCode::StaleDependency
                    || !(any_stale
                        || self.request.context.readiness_state
                            == super::ProviderReadinessState::Stale)
                {
                    return invalid(self.outcome);
                }
            }
            ProviderQueryOutcome::Dynamic => {
                require_no_values(value_count, self.outcome)?;
                if completeness_present
                    || self.evidence.proof_class != ProviderProofClass::RefusalOnly
                    || self.evidence.terminal_state != ProviderQueryTerminalState::Completed
                    || self.evidence.result_path != ProviderResultPath::Primary
                    || self.evidence.semantic_reason != SemanticReasonCode::DynamicValue
                    || !has_dynamic_boundary
                {
                    return invalid(self.outcome);
                }
            }
            ProviderQueryOutcome::Ambiguous => {
                require_no_values(value_count, self.outcome)?;
                if completeness_present
                    || self.evidence.proof_class != ProviderProofClass::RefusalOnly
                    || self.evidence.terminal_state != ProviderQueryTerminalState::Completed
                    || self.evidence.result_path != ProviderResultPath::Primary
                    || candidate_count < 2
                {
                    return invalid(self.outcome);
                }
            }
            ProviderQueryOutcome::Unavailable => {
                require_no_values(value_count, self.outcome)?;
                if completeness_present
                    || self.evidence.proof_class != ProviderProofClass::RefusalOnly
                    || self.evidence.terminal_state != ProviderQueryTerminalState::Completed
                    || self.evidence.result_path != ProviderResultPath::Primary
                {
                    return invalid(self.outcome);
                }
            }
            ProviderQueryOutcome::Cancelled => {
                require_no_values(value_count, self.outcome)?;
                if completeness_present
                    || self.evidence.proof_class != ProviderProofClass::RefusalOnly
                    || self.evidence.terminal_state != ProviderQueryTerminalState::Cancelled
                {
                    return invalid(self.outcome);
                }
            }
            ProviderQueryOutcome::DeadlineExceeded => {
                require_no_values(value_count, self.outcome)?;
                if completeness_present
                    || self.evidence.proof_class != ProviderProofClass::RefusalOnly
                    || self.evidence.terminal_state
                        != ProviderQueryTerminalState::DeadlineExceeded
                {
                    return invalid(self.outcome);
                }
            }
            ProviderQueryOutcome::Error => {
                require_no_values(value_count, self.outcome)?;
                if completeness_present
                    || self.evidence.proof_class != ProviderProofClass::Unknown
                    || self.evidence.terminal_state != ProviderQueryTerminalState::Failed
                    || self.evidence.result_path != ProviderResultPath::Primary
                {
                    return invalid(self.outcome);
                }
            }
        }
        Ok(())
    }
}

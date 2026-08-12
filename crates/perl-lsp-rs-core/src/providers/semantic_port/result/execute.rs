/// Provider-facing semantic fact port. Implementations return an unchecked draft;
/// only [`execute_provider_query`] can produce the checked result consumed by policy.
pub trait ProviderSemanticPort {
    /// Query canonical semantic facts for one request.
    fn query(
        &self,
        request: &ProviderQueryRequest,
        control: &dyn ProviderQueryControl,
    ) -> Result<ProviderQueryResultDraft, ProviderQueryContractError>;
}

/// Execute one provider query and validate its draft against the original request
/// and the caller-owned live cancellation/deadline control.
pub fn execute_provider_query(
    port: &dyn ProviderSemanticPort,
    request: &ProviderQueryRequest,
    control: &dyn ProviderQueryControl,
) -> Result<ProviderQueryResult, ProviderQueryContractError> {
    let draft = port.query(request, control)?;
    ProviderQueryResult::try_from_draft(request, control, draft)
}

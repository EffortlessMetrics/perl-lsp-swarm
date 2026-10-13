//! Proof of the shared error contract when xtask declines the network backend.

fn main() {}

#[cfg(test)]
mod tests {
    use perl_lsp_rs_core::providers::{
        ai::budget::{AiResponseBudget, BudgetError, BudgetKind, BudgetViolation},
        inline_completion::BackendError,
    };
    use perl_parser_core::{ErrorCategory, ErrorClass};

    #[test]
    fn budget_refusal_retains_its_typed_metadata_and_resource_class() {
        let violation =
            BudgetViolation { kind: BudgetKind::CompletionBytes, limit: 17, observed_at_least: 18 };
        let error = BackendError::BudgetExceeded(violation);
        assert_eq!(error.error_class(), ErrorCategory::ResourceLimit);
        assert_eq!(error.to_string(), violation.to_string());
        assert!(matches!(error, BackendError::BudgetExceeded(value) if value == violation));
    }

    #[test]
    fn unavailable_network_backend_does_not_relax_response_budget_validation() {
        let result = AiResponseBudget::compiled_default().with_limit(BudgetKind::ResponseBytes, 0);
        assert!(matches!(
            result,
            Err(BudgetError::NotPositive { kind: BudgetKind::ResponseBytes })
        ));
    }
}

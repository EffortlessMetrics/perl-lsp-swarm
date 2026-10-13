//! AI dependency and error contracts selected by the existing routed xtask tests.

use std::{path::Path, process::Command};

#[test]
fn tooling_and_product_ai_features_are_resolved_by_cargo() -> Result<(), Box<dyn std::error::Error>>
{
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let python = if cfg!(windows) { "python" } else { "python3" };
    let output = Command::new(python)
        .arg(root.join("scripts/tests/test_xtask_ai_dependencies.py"))
        .current_dir(&root)
        .output()?;
    assert!(
        output.status.success(),
        "Cargo graph proof failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}

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

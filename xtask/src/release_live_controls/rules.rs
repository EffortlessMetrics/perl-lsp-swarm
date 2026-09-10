//! Validate every modeled rule before reporting it as observed.
use super::api_value as field;
use super::model::{RequiredContextRow, RulesetRule};
use serde_json::Value;

pub(super) fn parse(rule_type: &str, parameters: Option<&Value>) -> Result<RulesetRule, String> {
    let mut row = RulesetRule {
        rule_type: rule_type.into(),
        required_contexts: Vec::new(),
        required_approving_review_count: None,
        required_review_thread_resolution: None,
        dismiss_stale_reviews_on_push: None,
        require_code_owner_review: None,
        require_last_push_approval: None,
        strict_required_status_checks_policy: None,
    };
    match rule_type {
        "required_status_checks" => {
            let value = parameters.ok_or("status check rule omitted parameters")?;
            field::object(value, "status check parameters")?;
            row.strict_required_status_checks_policy =
                Some(field::boolean(value, "strict_required_status_checks_policy")?);
            reject_unknown_parameters(
                value,
                &["required_status_checks", "strict_required_status_checks_policy"],
            )?;
            for check in field::array(value, "required_status_checks")? {
                row.required_contexts.push(RequiredContextRow {
                    context: field::string(check, "context")?,
                    app_id: Some(field::id(check, "integration_id")?),
                });
            }
        }
        "pull_request" => {
            let value = parameters.ok_or("pull request rule omitted parameters")?;
            row.required_approving_review_count =
                Some(field::review_count(value, "required_approving_review_count", 10)?);
            row.required_review_thread_resolution =
                Some(field::boolean(value, "required_review_thread_resolution")?);
            row.dismiss_stale_reviews_on_push =
                Some(field::boolean(value, "dismiss_stale_reviews_on_push")?);
            row.require_code_owner_review =
                Some(field::boolean(value, "require_code_owner_review")?);
            row.require_last_push_approval =
                Some(field::boolean(value, "require_last_push_approval")?);
            reject_unknown_parameters(
                value,
                &[
                    "required_approving_review_count",
                    "required_review_thread_resolution",
                    "dismiss_stale_reviews_on_push",
                    "require_code_owner_review",
                    "require_last_push_approval",
                ],
            )?;
        }
        "creation"
        | "deletion"
        | "update"
        | "required_linear_history"
        | "required_signatures"
        | "non_fast_forward" => {
            if parameters
                .is_some_and(|value| value.as_object().is_none_or(|object| !object.is_empty()))
            {
                return Err("parameterless rule carries unsupported parameters".into());
            }
        }
        _ => return Err("rule type is outside this observer's modeled controls".into()),
    }
    Ok(row)
}

fn reject_unknown_parameters(value: &Value, known: &[&str]) -> Result<(), String> {
    if field::object(value, "rule parameters")?.keys().any(|key| !known.contains(&key.as_str())) {
        Err("rule carries unsupported optional parameters".into())
    } else {
        Ok(())
    }
}

//! Classic protection fields remain independent observations.
use super::api_value as field;
use super::model::{
    ClassicProtection, Observed, PullRequestReviewRule, RequiredContextRow, RequiredStatusChecks,
};
use serde_json::Value;

pub(super) fn parse(value: &Value) -> ClassicProtection {
    ClassicProtection {
        required_status_checks: nullable(value, "required_status_checks", status_checks),
        enforce_admins: field::observation(enabled(value, "enforce_admins")),
        required_pull_request_reviews: nullable(value, "required_pull_request_reviews", reviews),
        required_conversation_resolution: field::observation(enabled(
            value,
            "required_conversation_resolution",
        )),
        restrictions_present: match value.get("restrictions") {
            Some(Value::Null) => Observed::observed(false),
            Some(Value::Object(_)) => Observed::observed(true),
            _ => Observed::not_proven("restrictions must be explicitly null or an object"),
        },
    }
}

fn nullable<T>(
    value: &Value,
    key: &str,
    parse: impl FnOnce(&Value) -> Result<T, String>,
) -> Observed<T> {
    match value.get(key) {
        Some(Value::Null) => Observed::absent(format!("{key} is explicitly null")),
        Some(value) => field::observation(parse(value)),
        None => Observed::not_proven(format!("{key} was omitted")),
    }
}

fn enabled(value: &Value, key: &str) -> Result<bool, String> {
    field::boolean(value.get(key).ok_or_else(|| format!("{key} was omitted"))?, "enabled")
}

fn status_checks(value: &Value) -> Result<RequiredStatusChecks, String> {
    let strict = field::boolean(value, "strict")?;
    let rows = field::array(value, "checks")?;
    let contexts = rows
        .iter()
        .map(|row| {
            Ok(RequiredContextRow {
                context: field::string(row, "context")?,
                app_id: field::nullable_id(row, "app_id", true)?,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let legacy = field::array(value, "contexts")?
        .iter()
        .map(|value| value.as_str().ok_or("contexts contains a non-string"))
        .collect::<Result<std::collections::BTreeSet<_>, _>>()?;
    let checks =
        contexts.iter().map(|row| row.context.as_str()).collect::<std::collections::BTreeSet<_>>();
    if legacy != checks {
        return Err("checks and contexts disagree".into());
    }
    Ok(RequiredStatusChecks { strict, contexts })
}

fn reviews(value: &Value) -> Result<PullRequestReviewRule, String> {
    Ok(PullRequestReviewRule {
        required_approving_review_count: Some(field::review_count(
            value,
            "required_approving_review_count",
            6,
        )?),
        dismiss_stale_reviews: Some(field::boolean(value, "dismiss_stale_reviews")?),
        require_code_owner_reviews: Some(field::boolean(value, "require_code_owner_reviews")?),
        require_last_push_approval: Some(field::boolean(value, "require_last_push_approval")?),
    })
}

//! Environment collection keeps built-in rules, named deployment policies,
//! installed GitHub App rules and secret counts as separate observations.
use super::api_value as field;
use super::live::{ReadOnlyCommands, encode_path_segment, read_json, repo_path};
use super::model::{
    CustomDeploymentProtectionRule, DeploymentBranchPolicy, DeploymentProtectionApp,
    DeploymentReviewer, Environment, EnvironmentProtectionRule, NamedDeploymentPolicy,
    ObservationState, Observed,
};
use serde_json::Value;
use std::collections::BTreeSet;

pub fn collect_environments(
    commands: &dyn ReadOnlyCommands,
    owner: &str,
    name: &str,
) -> Observed<Vec<Environment>> {
    let path = format!("{}/environments", repo_path(owner, name));
    let items = match counted_pages(commands, &path, "environments") {
        Ok(items) => items,
        Err(detail) => return Observed::not_proven(detail),
    };
    let mut names = BTreeSet::new();
    let mut identities = BTreeSet::new();
    let mut environments = Vec::new();
    for item in items {
        let identity: Result<(u64, String, String), String> = (|| {
            let id = field::id(&item, "id")?;
            let node_id = field::string(&item, "node_id")?;
            let env_name = field::string(&item, "name")?;
            if !names.insert(env_name.to_lowercase()) || !identities.insert(id) {
                return Err("environment listing repeats an identity or name".into());
            }
            Ok((id, node_id, env_name))
        })();
        match identity {
            Ok((id, node_id, env_name)) => {
                environments.push(collect_one(commands, owner, name, id, node_id, env_name))
            }
            Err(detail) => return Observed::not_proven(detail),
        }
    }
    Observed::observed(environments)
}

fn counted_pages(
    commands: &dyn ReadOnlyCommands,
    base: &str,
    key: &str,
) -> Result<Vec<Value>, String> {
    let mut rows = Vec::new();
    let mut expected = None;
    for page in 1..=20 {
        let value = read_json(commands, &format!("{base}?per_page=100&page={page}"))?;
        let count = usize::try_from(field::unsigned(&value, "total_count")?)
            .map_err(|_| "total_count exceeds the platform count range".to_string())?;
        if expected.is_some_and(|old| old != count) {
            return Err(format!("{key} total_count changed between pages"));
        }
        expected = Some(count);
        let page_rows = field::array(&value, key)?;
        if page_rows.len() > 100 || rows.len().saturating_add(page_rows.len()) > count {
            return Err(format!("{key} page exceeds the declared count or requested page size"));
        }
        if page_rows.is_empty() && rows.len() < count {
            return Err(format!("{key} listing ended before total_count"));
        }
        rows.extend_from_slice(page_rows);
        if rows.len() == count {
            return Ok(rows);
        }
    }
    Err(format!("{key} listing exceeded the twenty-page observation bound"))
}

fn collect_one(
    commands: &dyn ReadOnlyCommands,
    owner: &str,
    name: &str,
    id: u64,
    node_id: String,
    env_name: String,
) -> Environment {
    let path =
        format!("{}/environments/{}", repo_path(owner, name), encode_path_segment(&env_name));
    let detail = read_json(commands, &path).and_then(|value| {
        if field::id(&value, "id")? != id
            || field::string(&value, "node_id")? != node_id
            || !field::string(&value, "name")?.eq_ignore_ascii_case(&env_name)
        {
            return Err("environment detail identity disagrees with the listing".into());
        }
        Ok(value)
    });
    let (protection_rules, deployment_branch_policy) = match detail {
        Ok(value) => {
            (field::observation(parse_builtin_rules(&value)), parse_deployment_policy(&value))
        }
        Err(detail) => (Observed::not_proven(detail.clone()), Observed::not_proven(detail)),
    };
    let deployment_branch_policies = match deployment_branch_policy.state {
        ObservationState::Absent => {
            Observed::absent("explicit null deployment policy allows all branches")
        }
        ObservationState::Observed => match deployment_branch_policy.value() {
            Some(policy) if policy.custom_branch_policies => {
                field::observation(collect_named_policies(commands, &path))
            }
            Some(_) => {
                Observed::absent("custom deployment branch policies are explicitly disabled")
            }
            None => Observed::not_proven("deployment policy has no observed value"),
        },
        ObservationState::NotProven => {
            Observed::not_proven("custom deployment-policy applicability was not established")
        }
    };
    let custom_deployment_protection_rules =
        field::observation(collect_custom_rules(commands, &path));
    let secret_count =
        field::observation(read_json(commands, &format!("{path}/secrets")).and_then(|value| {
            usize::try_from(field::unsigned(&value, "total_count")?)
                .map_err(|_| "secret total_count exceeds the platform count range".into())
        }));
    Environment {
        id,
        node_id,
        name: env_name,
        protection_rules,
        deployment_branch_policy,
        deployment_branch_policies,
        custom_deployment_protection_rules,
        secret_count,
    }
}

fn parse_builtin_rules(value: &Value) -> Result<Vec<EnvironmentProtectionRule>, String> {
    let rows = field::array(value, "protection_rules")?;
    let mut identities = BTreeSet::new();
    rows.iter()
        .map(|row| {
            let id = field::id(row, "id")?;
            let node_id = field::string(row, "node_id")?;
            if !identities.insert(id) {
                return Err("environment protection_rules repeats an identifier".into());
            }
            let rule_type = field::string(row, "type")?;
            let (wait_timer, reviewers, prevent_self_review) = match rule_type.as_str() {
                "wait_timer" => {
                    let timer = field::unsigned(row, "wait_timer")?;
                    if timer > 43_200 {
                        return Err("environment wait_timer exceeds thirty days".into());
                    }
                    (Some(timer), None, None)
                }
                "required_reviewers" => {
                    let reviewers = field::array(row, "reviewers")?;
                    if reviewers.len() > 6 {
                        return Err("environment reviewer count exceeds six".into());
                    }
                    let mut identities = BTreeSet::new();
                    let reviewers = reviewers
                        .iter()
                        .map(|entry| {
                            let reviewer_type = field::string(entry, "type")?;
                            if !matches!(reviewer_type.as_str(), "User" | "Team") {
                                return Err("unsupported environment reviewer type".into());
                            }
                            let reviewer = entry
                                .get("reviewer")
                                .ok_or("environment reviewer identity is missing")?;
                            let id = field::id(reviewer, "id")?;
                            let node_id = field::string(reviewer, "node_id")?;
                            if !identities.insert((reviewer_type.clone(), id)) {
                                return Err("environment reviewer identity is repeated".into());
                            }
                            Ok(DeploymentReviewer { reviewer_type, id, node_id })
                        })
                        .collect::<Result<Vec<_>, String>>()?;
                    (None, Some(reviewers), Some(field::boolean(row, "prevent_self_review")?))
                }
                "branch_policy" => (None, None, None),
                _ => return Err("unsupported environment protection rule type".into()),
            };
            // Known fields on the wrong variant cannot be silently discarded.
            for (key, expected) in [
                ("wait_timer", rule_type == "wait_timer"),
                ("reviewers", rule_type == "required_reviewers"),
                ("prevent_self_review", rule_type == "required_reviewers"),
            ] {
                if !expected && row.get(key).is_some() {
                    return Err(format!("{key} occurs on the wrong environment rule type"));
                }
            }
            Ok(EnvironmentProtectionRule {
                id,
                node_id,
                rule_type,
                wait_timer,
                reviewers,
                prevent_self_review,
            })
        })
        .collect()
}

fn parse_deployment_policy(value: &Value) -> Observed<DeploymentBranchPolicy> {
    match value.get("deployment_branch_policy") {
        Some(Value::Null) => Observed::absent("API explicitly allows all deployment branches"),
        Some(value) => field::observation((|| {
            let protected_branches = field::boolean(value, "protected_branches")?;
            let custom_branch_policies = field::boolean(value, "custom_branch_policies")?;
            if protected_branches == custom_branch_policies {
                return Err("deployment policy requires exactly one selection mode".into());
            }
            Ok(DeploymentBranchPolicy { protected_branches, custom_branch_policies })
        })()),
        None => Observed::not_proven("environment payload omitted deployment_branch_policy"),
    }
}

fn collect_named_policies(
    commands: &dyn ReadOnlyCommands,
    path: &str,
) -> Result<Vec<NamedDeploymentPolicy>, String> {
    let rows =
        counted_pages(commands, &format!("{path}/deployment-branch-policies"), "branch_policies")?;
    let mut identities = BTreeSet::new();
    rows.iter()
        .map(|row| {
            let id = field::id(row, "id")?;
            let node_id = field::string(row, "node_id")?;
            let name = field::string(row, "name")?;
            let policy_type = field::string(row, "type")?;
            if !matches!(policy_type.as_str(), "branch" | "tag") {
                return Err("deployment policy type is not branch or tag".into());
            }
            if !identities.insert(id) {
                return Err("deployment policies repeat an identifier".into());
            }
            Ok(NamedDeploymentPolicy { id, node_id, name, policy_type })
        })
        .collect()
}

fn collect_custom_rules(
    commands: &dyn ReadOnlyCommands,
    path: &str,
) -> Result<Vec<CustomDeploymentProtectionRule>, String> {
    // This GET lists installed rules, not available integrations. GitHub
    // documents no pagination parameters for this endpoint.
    let value = read_json(commands, &format!("{path}/deployment_protection_rules"))?;
    let count = usize::try_from(field::unsigned(&value, "total_count")?)
        .map_err(|_| "custom protection rule count overflowed")?;
    let rows = field::array(&value, "custom_deployment_protection_rules")?;
    if rows.len() != count {
        return Err("installed protection rules do not match total_count".into());
    }
    let mut identities = BTreeSet::new();
    rows.iter()
        .map(|row| {
            let id = field::id(row, "id")?;
            let node_id = field::string(row, "node_id")?;
            let enabled = field::boolean(row, "enabled")?;
            if !enabled {
                return Err("enabled protection-rule listing contains a disabled rule".into());
            }
            if !identities.insert(id) {
                return Err("installed protection rules repeat an identifier".into());
            }
            let app = row.get("app").ok_or("installed protection rule omitted app")?;
            let integration_url = field::string(app, "integration_url")?;
            if !integration_url.starts_with("https://")
                || integration_url.chars().any(char::is_whitespace)
            {
                return Err("integration_url must be an HTTPS URL".into());
            }
            let app = DeploymentProtectionApp {
                id: field::id(app, "id")?,
                node_id: field::string(app, "node_id")?,
                slug: field::string(app, "slug")?,
            };
            // integration_url is not fetched or copied into receipts: IDs and
            // slug identify the app without introducing an arbitrary URL read.
            Ok(CustomDeploymentProtectionRule { id, node_id, enabled, app })
        })
        .collect()
}

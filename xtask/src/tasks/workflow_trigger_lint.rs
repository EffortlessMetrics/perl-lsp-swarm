//! Enforce trigger/concurrency policy for required CI workflows.

use std::fs;
use std::path::{Path, PathBuf};

use clap::ValueEnum;
use color_eyre::eyre::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_yaml_ng::Value;

use crate::utils::project_root;

const REQUIRED_CANCEL_IN_PROGRESS: &str =
    "${{ github.event_name == 'pull_request' && github.event.action == 'synchronize' }}";

/// The policy file holds two tables that answer different questions.
///
/// `[[check]]` is this lint's governance list: the workflows whose trigger and
/// concurrency shape the contract is evaluated against. `[[checks]]` is the
/// status-context inventory, and it is the only place that records which
/// contexts the `main` ruleset actually requires.
///
/// Until #16164 this struct named only `check`. `toml::from_str` drops unknown
/// keys, so all 18 `[[checks]]` rows were discarded without a word and the
/// contract was enforced against whichever workflows happened to appear in a
/// four-entry list. Reading both is what lets `governance_violations` below
/// refuse a required context that no governance row covers.
#[derive(Debug, Clone, Deserialize)]
struct RequiredChecksPolicy {
    check: Vec<PolicyCheck>,
    #[serde(default)]
    checks: Vec<InventoryCheck>,
}

#[derive(Debug, Clone, Deserialize)]
struct PolicyCheck {
    name: String,
    workflow: String,
    required: bool,
    #[serde(default)]
    exemption: Vec<PolicyExemption>,
}

/// A `[[checks]]` row. Only the three fields this lint reasons about are
/// named; the inventory carries many more for other readers.
#[derive(Debug, Clone, Deserialize)]
struct InventoryCheck {
    name: String,
    /// `repository-job` or `external`, as the inventory row declares it.
    ///
    /// This is read rather than inferred. An earlier revision treated a
    /// missing `workflow` as an external producer, which is fail-open: a
    /// repository job whose row simply omits its workflow was then counted as
    /// governed without anything being evaluated (#16164 review). The
    /// inventory states the producer class explicitly on every row, and
    /// `codecov/patch` — the one genuinely external row — even carries a
    /// `workflow`, so absence of a workflow never established externality in
    /// the first place.
    #[serde(default)]
    producer: Option<String>,
    #[serde(default)]
    workflow: Option<String>,
    #[serde(default)]
    required: bool,
}

/// What this contract can say about one required context's producer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProducerClass {
    /// A job in a workflow of this repository: the trigger contract applies.
    RepositoryJob,
    /// Someone else's integration: out of this contract's reach, and counted
    /// separately rather than folded into the governed total.
    External,
    /// The row does not say. Not assumed either way.
    Unstated,
}

impl InventoryCheck {
    fn producer_class(&self) -> ProducerClass {
        match self.producer.as_deref() {
            Some("repository-job") => ProducerClass::RepositoryJob,
            Some("external") => ProducerClass::External,
            _ => ProducerClass::Unstated,
        }
    }
}

/// One contract clause a governance row knowingly does not satisfy.
///
/// An exemption is not a way to pass. It moves a divergence out of the silent
/// column and into a named one that carries a reason a reviewer can argue
/// with. `stale exemption` below is what keeps that honest: an exemption for a
/// clause the workflow now satisfies is itself a violation, so the list cannot
/// outlive the condition that justified it.
#[derive(Debug, Clone, Deserialize)]
struct PolicyExemption {
    clause: String,
    status: ExemptionStatus,
    reason: String,
    #[serde(default)]
    tracking: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
enum ExemptionStatus {
    /// Weighed and kept: the clause is the wrong requirement for this
    /// workflow, and the reason says why.
    Accepted,
    /// A real gap that has not been closed yet. `tracking` is mandatory, so a
    /// remainder cannot be recorded without somewhere to close it.
    Remainder,
}

#[derive(Debug, Clone, Serialize)]
struct RecordedExemption {
    clause: String,
    status: ExemptionStatus,
    reason: String,
    tracking: Option<String>,
    /// The contract message this exemption suppresses, kept verbatim so the
    /// receipt shows what is not being enforced rather than only its name.
    suppressed: String,
}

/// Every clause `evaluate_required_entry` can report, by the id an exemption
/// names. A clause id that is not on this list is rejected rather than
/// ignored, so a typo in the policy file cannot silently exempt nothing.
const CLAUSE_IDS: &[&str] = &[
    "workflow-exists",
    "pull-request-trigger",
    "merge-group-trigger",
    "push-targets-master",
    "no-path-filters",
    "event-aware-concurrency",
    "no-label-triggers",
];

#[derive(Debug, Clone, Serialize)]
struct WorkflowEvaluation {
    name: String,
    workflow: String,
    required: bool,
    ok: bool,
    violations: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    exemptions: Vec<RecordedExemption>,
}

/// What the contract covers, counted rather than assumed.
///
/// `docs/ci/workflow-trigger-policy.md` names the five ruleset-required
/// contexts and then states the clauses, and a reader joins the two. These
/// counts are what makes that join checkable instead of inferred.
#[derive(Debug, Clone, Serialize)]
struct GovernanceSummary {
    required_contexts: usize,
    governed: usize,
    /// Required contexts this contract does not reach because the inventory
    /// declares an external producer. Counted, not hidden: `governed` plus
    /// `external` plus `ungoverned` accounts for every required context.
    external: usize,
    ungoverned: Vec<String>,
    accepted_exemptions: usize,
    remainders: usize,
}

impl GovernanceSummary {
    /// The one-line human summary, split out of `println!` so the counts it
    /// states can be asserted.
    ///
    /// The JSON receipt carries `external` as its own field; the text line did
    /// not print it, so `1/5` read as four gaps when some of those four are out
    /// of this contract's reach by declaration rather than by omission (#16172
    /// review). Every required context now appears in exactly one of the three
    /// counts, and `governance_line_accounts_for_every_required_context` holds
    /// that.
    fn summary_line(&self) -> String {
        format!(
            "{}/{} ruleset-required contexts governed, {} produced outside this \
             repository, {} ungoverned, {} accepted exemptions, {} remainders",
            self.governed,
            self.required_contexts,
            self.external,
            self.ungoverned.len(),
            self.accepted_exemptions,
            self.remainders
        )
    }
}

#[derive(Debug, Clone, Serialize)]
struct WorkflowTriggerLintReceipt {
    schema_version: String,
    policy_path: Option<String>,
    fixture_path: Option<String>,
    overall_ok: bool,
    evaluations: Vec<WorkflowEvaluation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    governance: Option<GovernanceSummary>,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum WorkflowTriggerLintFormat {
    Text,
    Json,
}

pub fn run(
    policy_path: Option<PathBuf>,
    receipt_path: Option<PathBuf>,
    fixture_path: Option<PathBuf>,
    format: WorkflowTriggerLintFormat,
) -> Result<()> {
    let root = project_root()?;

    let (evaluations, governance, policy_display, fixture_display) = if let Some(fixture) =
        fixture_path
    {
        let evaluation = evaluate_fixture(&fixture)?;
        (vec![evaluation], None, None, Some(fixture.display().to_string()))
    } else {
        let policy = policy_path.unwrap_or_else(|| root.join(".ci/policies/required-checks.toml"));
        let (evaluations, governance) = evaluate_policy(&root, &policy)?;
        (evaluations, Some(governance), Some(policy.display().to_string()), None)
    };

    // An ungoverned required context is a lint failure in its own right. It is
    // not attached to any evaluation, because the whole defect is that no
    // evaluation exists for it.
    let overall_ok = evaluations.iter().all(|entry| entry.ok)
        && governance.as_ref().is_none_or(|summary| summary.ungoverned.is_empty());
    let receipt = WorkflowTriggerLintReceipt {
        schema_version: "1.1.0".to_string(),
        policy_path: policy_display,
        fixture_path: fixture_display,
        overall_ok,
        evaluations,
        governance,
    };

    if let Some(path) = receipt_path {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("creating receipt directory {}", parent.display()))?;
        }
        let payload = serde_json::to_string_pretty(&receipt)?;
        fs::write(&path, payload).with_context(|| format!("writing receipt {}", path.display()))?;
    }

    output(&receipt, format)?;

    if receipt.overall_ok { Ok(()) } else { bail!("workflow-trigger-lint found policy violations") }
}

fn evaluate_policy(
    root: &Path,
    policy_path: &Path,
) -> Result<(Vec<WorkflowEvaluation>, GovernanceSummary)> {
    let raw = fs::read_to_string(policy_path)
        .with_context(|| format!("reading policy file {}", policy_path.display()))?;
    let policy: RequiredChecksPolicy = toml::from_str(&raw)
        .with_context(|| format!("parsing policy file {}", policy_path.display()))?;

    let governance = governance_summary(&policy);

    let evaluations = policy
        .check
        .iter()
        .filter(|entry| entry.required)
        .map(|entry| evaluate_required_workflow(root, entry))
        .collect::<Result<Vec<_>>>()?;

    let accepted_exemptions = evaluations
        .iter()
        .flat_map(|entry| entry.exemptions.iter())
        .filter(|item| item.status == ExemptionStatus::Accepted)
        .count();
    let remainders = evaluations
        .iter()
        .flat_map(|entry| entry.exemptions.iter())
        .filter(|item| item.status == ExemptionStatus::Remainder)
        .count();

    Ok((evaluations, GovernanceSummary { accepted_exemptions, remainders, ..governance }))
}

/// Count the ruleset-required contexts the governance list covers.
///
/// A context is governed when some `[[check]]` row names its workflow *and*
/// carries `required = true`, because `evaluate_required_entry` returns no
/// violations at all for a row that does not. A row with `required = false`
/// therefore governs nothing, and counting it would restore exactly the
/// silence this check exists to remove.
fn governance_summary(policy: &RequiredChecksPolicy) -> GovernanceSummary {
    let governed_workflows: Vec<&str> = policy
        .check
        .iter()
        .filter(|entry| entry.required)
        .map(|entry| entry.workflow.as_str())
        .collect();

    let required_contexts: Vec<&InventoryCheck> =
        policy.checks.iter().filter(|entry| entry.required).collect();

    let mut ungoverned = Vec::new();
    let mut external = 0usize;

    for entry in &required_contexts {
        match entry.producer_class() {
            // Someone else's integration. The trigger contract governs this
            // repository's workflows, so there is no `[[check]]` row to demand.
            ProducerClass::External => external += 1,
            ProducerClass::RepositoryJob => match entry.workflow.as_deref() {
                Some(workflow) if governed_workflows.contains(&workflow) => {}
                Some(workflow) => ungoverned.push(format!(
                    "required context `{}` is produced by `{workflow}`, which no \
                     `[[check]]` row governs",
                    entry.name
                )),
                // A repository job that does not name its workflow cannot be
                // evaluated against the contract, and an unevaluated context is
                // not a governed one.
                None => ungoverned.push(format!(
                    "required context `{}` declares a repository job but names no \
                     workflow, so the trigger contract cannot be evaluated for it",
                    entry.name
                )),
            },
            // No producer class. Guessing one is how the fail-open path was
            // built; this says what is missing instead.
            ProducerClass::Unstated => ungoverned.push(format!(
                "required context `{}` names no producer, so whether the trigger \
                 contract governs it cannot be established",
                entry.name
            )),
        }
    }

    GovernanceSummary {
        required_contexts: required_contexts.len(),
        governed: required_contexts.len().saturating_sub(ungoverned.len() + external),
        external,
        ungoverned,
        accepted_exemptions: 0,
        remainders: 0,
    }
}

fn evaluate_fixture(fixture_path: &Path) -> Result<WorkflowEvaluation> {
    let workflow = read_workflow_yaml(fixture_path)?;
    Ok(evaluate_required_entry(
        "fixture",
        fixture_path.display().to_string(),
        true,
        fixture_path.exists(),
        Some(&workflow),
        &[],
    ))
}

fn evaluate_required_workflow(root: &Path, check: &PolicyCheck) -> Result<WorkflowEvaluation> {
    let workflow_path = root.join(&check.workflow);
    let workflow =
        if workflow_path.exists() { Some(read_workflow_yaml(&workflow_path)?) } else { None };

    Ok(evaluate_required_entry(
        &check.name,
        check.workflow.clone(),
        check.required,
        workflow_path.exists(),
        workflow.as_ref(),
        &check.exemption,
    ))
}

fn evaluate_required_entry(
    name: &str,
    workflow: String,
    required: bool,
    workflow_exists: bool,
    workflow_yaml: Option<&Value>,
    exemptions: &[PolicyExemption],
) -> WorkflowEvaluation {
    // Each finding carries the clause id an exemption names, so a divergence
    // and the record that accounts for it are matched on an identifier rather
    // than on message text.
    let mut findings: Vec<(&str, String)> = Vec::new();

    if required {
        if !workflow_exists {
            findings.push(("workflow-exists", "workflow file does not exist".to_string()));
        }

        if let Some(yaml) = workflow_yaml {
            if !has_trigger(yaml, "pull_request") {
                findings.push(("pull-request-trigger", "missing pull_request trigger".to_string()));
            }
            if !has_trigger(yaml, "merge_group") {
                findings.push(("merge-group-trigger", "missing merge_group trigger".to_string()));
            }
            if !push_targets_master(yaml) {
                findings.push((
                    "push-targets-master",
                    "push trigger must target master branch".to_string(),
                ));
            }
            if has_path_filters(yaml) {
                findings.push((
                    "no-path-filters",
                    "path filters are not allowed on required workflows".to_string(),
                ));
            }
            if !has_event_aware_concurrency(yaml) {
                findings.push((
                    "event-aware-concurrency",
                    format!(
                        "concurrency.cancel-in-progress must be `{REQUIRED_CANCEL_IN_PROGRESS}`"
                    ),
                ));
            }
            if pull_request_has_label_triggers(yaml) {
                findings.push((
                    "no-label-triggers",
                    "required CI workflows must not trigger on pull_request labeled/unlabeled"
                        .to_string(),
                ));
            }
        }
    }

    let (mut violations, recorded) = apply_exemptions(&findings, exemptions);

    // The policy file's own shape is part of the contract. A malformed
    // exemption is a violation of the row that carries it, never a silent
    // no-op, because a no-op here reads as "this clause passes".
    violations.extend(exemption_defects(&findings, exemptions));

    WorkflowEvaluation {
        name: name.to_string(),
        workflow,
        required,
        ok: violations.is_empty(),
        violations,
        exemptions: recorded,
    }
}

/// Split findings into the ones still failing and the ones a row accounts for.
fn apply_exemptions(
    findings: &[(&str, String)],
    exemptions: &[PolicyExemption],
) -> (Vec<String>, Vec<RecordedExemption>) {
    let mut violations = Vec::new();
    let mut recorded = Vec::new();

    for (clause, message) in findings {
        match exemptions.iter().find(|item| item.clause == *clause) {
            Some(exemption) => recorded.push(RecordedExemption {
                clause: exemption.clause.clone(),
                status: exemption.status,
                reason: exemption.reason.clone(),
                tracking: exemption.tracking.clone(),
                suppressed: message.clone(),
            }),
            None => violations.push(message.clone()),
        }
    }

    (violations, recorded)
}

/// Defects in the exemption records themselves.
///
/// The last of these is the one that matters. An exemption for a clause the
/// workflow now satisfies is a violation, so a divergence that gets fixed
/// forces its exemption to be deleted. Without that rule the list only ever
/// grows, and a policy file that only grows exemptions is a policy file that
/// stops meaning anything.
fn exemption_defects(findings: &[(&str, String)], exemptions: &[PolicyExemption]) -> Vec<String> {
    let mut defects = Vec::new();

    // Two exemptions for one clause let list order decide policy. `apply_exemptions`
    // records the first match, so an `accepted` entry placed above a `remainder`
    // silently swallows the remainder: it vanishes from the counts and from the
    // receipt, and the divergence reads as settled (#16164 review). A clause has one
    // disposition or the contract is not saying anything.
    for (index, exemption) in exemptions.iter().enumerate() {
        if exemptions[..index].iter().any(|earlier| earlier.clause == exemption.clause) {
            defects.push(format!(
                "clause `{}` carries more than one exemption; a clause has one \
                 disposition, and a second entry would be decided by list order",
                exemption.clause
            ));
        }
    }

    for exemption in exemptions {
        if !CLAUSE_IDS.contains(&exemption.clause.as_str()) {
            defects.push(format!(
                "exemption names unknown clause `{}`; expected one of {}",
                exemption.clause,
                CLAUSE_IDS.join(", ")
            ));
            continue;
        }

        if exemption.reason.trim().is_empty() {
            defects.push(format!("exemption for clause `{}` carries no reason", exemption.clause));
        }

        if exemption.status == ExemptionStatus::Remainder
            && exemption.tracking.as_ref().is_none_or(|item| item.trim().is_empty())
        {
            defects.push(format!(
                "exemption for clause `{}` is a remainder and must name a tracking issue",
                exemption.clause
            ));
        }

        if !findings.iter().any(|(clause, _)| *clause == exemption.clause) {
            defects.push(format!(
                "stale exemption for clause `{}`: the workflow satisfies it, so the \
                 exemption must be removed",
                exemption.clause
            ));
        }
    }

    defects
}

fn read_workflow_yaml(path: &Path) -> Result<Value> {
    let raw = fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let parsed = serde_yaml_ng::from_str(&raw)
        .with_context(|| format!("parsing workflow YAML {}", path.display()))?;
    Ok(parsed)
}

fn has_trigger(workflow: &Value, trigger_name: &str) -> bool {
    let Some(on) = get_on(workflow) else {
        return false;
    };

    match on {
        Value::String(value) => value == trigger_name,
        Value::Sequence(values) => {
            values.iter().any(|value| value.as_str().is_some_and(|item| item == trigger_name))
        }
        Value::Mapping(mapping) => {
            mapping.iter().any(|(key, _)| key.as_str().is_some_and(|item| item == trigger_name))
        }
        _ => false,
    }
}

fn push_targets_master(workflow: &Value) -> bool {
    let Some(on) = get_on(workflow) else {
        return false;
    };

    match on {
        Value::Mapping(mapping) => {
            let push = mapping.iter().find_map(|(key, value)| {
                if key.as_str().is_some_and(|name| name == "push") { Some(value) } else { None }
            });

            let Some(push) = push else {
                return false;
            };

            match push {
                Value::Mapping(push_mapping) => push_mapping.iter().any(|(key, value)| {
                    key.as_str().is_some_and(|name| name == "branches")
                        && branch_targets_master(value)
                }),
                _ => false,
            }
        }
        _ => false,
    }
}

fn branch_targets_master(branches: &Value) -> bool {
    match branches {
        Value::String(value) => value == "master" || value == "refs/heads/master",
        Value::Sequence(values) => values.iter().any(|value| {
            value.as_str().is_some_and(|item| item == "master" || item == "refs/heads/master")
        }),
        _ => false,
    }
}

fn has_path_filters(workflow: &Value) -> bool {
    let Some(on) = get_on(workflow) else {
        return false;
    };

    let Value::Mapping(mapping) = on else {
        return false;
    };

    if mapping
        .keys()
        .any(|key| key.as_str().is_some_and(|name| name == "paths" || name == "paths-ignore"))
    {
        return true;
    }

    mapping.iter().any(|(key, value)| {
        let Some(name) = key.as_str() else {
            return false;
        };

        if name != "pull_request" && name != "merge_group" && name != "push" {
            return false;
        }

        match value {
            Value::Mapping(event_map) => event_map.keys().any(|event_key| {
                event_key
                    .as_str()
                    .is_some_and(|event_name| event_name == "paths" || event_name == "paths-ignore")
            }),
            _ => false,
        }
    })
}

fn pull_request_has_label_triggers(workflow: &Value) -> bool {
    get_on(workflow)
        .and_then(Value::as_mapping)
        .and_then(|on| on.get(Value::String("pull_request".to_string())))
        .and_then(Value::as_mapping)
        .and_then(|pr| pr.get(Value::String("types".to_string())))
        .and_then(Value::as_sequence)
        .is_some_and(|types| {
            types
                .iter()
                .filter_map(Value::as_str)
                .any(|event| event == "labeled" || event == "unlabeled")
        })
}

fn has_event_aware_concurrency(workflow: &Value) -> bool {
    let Some(concurrency) = get_top_level_field(workflow, "concurrency") else {
        return false;
    };

    let Value::Mapping(map) = concurrency else {
        return false;
    };

    map.iter().any(|(key, value)| {
        key.as_str().is_some_and(|field| field == "cancel-in-progress")
            && value.as_str().is_some_and(|expr| expr.trim() == REQUIRED_CANCEL_IN_PROGRESS)
    })
}

fn output(receipt: &WorkflowTriggerLintReceipt, format: WorkflowTriggerLintFormat) -> Result<()> {
    match format {
        WorkflowTriggerLintFormat::Json => {
            println!("{}", serde_json::to_string_pretty(receipt)?);
        }
        WorkflowTriggerLintFormat::Text => {
            if receipt.overall_ok {
                println!("✓ workflow-trigger-lint passed");
            } else {
                println!("❌ workflow-trigger-lint found violations");
            }

            for evaluation in &receipt.evaluations {
                if evaluation.ok {
                    println!("  - {} ({}) ✓", evaluation.name, evaluation.workflow);
                } else {
                    println!("  - {} ({})", evaluation.name, evaluation.workflow);
                    for violation in &evaluation.violations {
                        println!("      * {}", violation);
                    }
                }
                // Printed for a passing row too. An exemption that is only
                // visible in a failing run is an exemption nobody re-reads.
                for exemption in &evaluation.exemptions {
                    let tracking = exemption
                        .tracking
                        .as_deref()
                        .map(|item| format!(" [{item}]"))
                        .unwrap_or_default();
                    println!(
                        "      ~ exempt {} ({:?}){}: {}",
                        exemption.clause, exemption.status, tracking, exemption.reason
                    );
                }
            }

            if let Some(governance) = &receipt.governance {
                println!("  governance: {}", governance.summary_line());
                for entry in &governance.ungoverned {
                    println!("      * {}", entry);
                }
            }
        }
    }
    Ok(())
}

fn get_on(workflow: &Value) -> Option<&Value> {
    get_top_level_field(workflow, "on")
}

fn get_top_level_field<'a>(workflow: &'a Value, field: &str) -> Option<&'a Value> {
    let Value::Mapping(mapping) = workflow else {
        return None;
    };

    mapping.iter().find_map(|(key, value)| {
        if key.as_str().is_some_and(|name| name == field) { Some(value) } else { None }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use color_eyre::eyre::ensure;

    fn load_fixture(name: &str) -> Result<Value> {
        let root = project_root()?;
        read_workflow_yaml(&root.join("xtask/tests/fixtures/workflows").join(name))
    }

    #[test]
    fn valid_required_fixture_passes() -> Result<()> {
        let fixture = load_fixture("valid-required.yml")?;
        let eval = evaluate_required_entry(
            "fixture",
            "fixture.yml".to_string(),
            true,
            true,
            Some(&fixture),
            &[],
        );
        assert!(eval.ok);
        Ok(())
    }

    #[test]
    fn labeled_pull_request_type_fixture_fails() -> Result<()> {
        let fixture = load_fixture("labeled-required.yml")?;
        let eval = evaluate_required_entry(
            "fixture",
            "fixture.yml".to_string(),
            true,
            true,
            Some(&fixture),
            &[],
        );
        assert!(!eval.ok);
        assert!(eval.violations.iter().any(|item| item.contains("labeled/unlabeled")));
        Ok(())
    }

    #[test]
    fn missing_merge_group_fixture_fails() -> Result<()> {
        let fixture = load_fixture("missing-merge-group.yml")?;
        let eval = evaluate_required_entry(
            "fixture",
            "fixture.yml".to_string(),
            true,
            true,
            Some(&fixture),
            &[],
        );
        assert!(!eval.ok);
        assert!(eval.violations.iter().any(|item| item.contains("merge_group")));
        Ok(())
    }

    fn exemption(clause: &str, status: ExemptionStatus) -> PolicyExemption {
        PolicyExemption {
            clause: clause.to_string(),
            status,
            reason: "a reason that is not empty".to_string(),
            tracking: Some("16164".to_string()),
        }
    }

    fn policy(check: Vec<PolicyCheck>, checks: Vec<InventoryCheck>) -> RequiredChecksPolicy {
        RequiredChecksPolicy { check, checks }
    }

    fn governance_row(workflow: &str, required: bool) -> PolicyCheck {
        PolicyCheck {
            name: workflow.to_string(),
            workflow: workflow.to_string(),
            required,
            exemption: Vec::new(),
        }
    }

    fn inventory_row(name: &str, workflow: Option<&str>, required: bool) -> InventoryCheck {
        InventoryCheck {
            name: name.to_string(),
            producer: Some("repository-job".to_string()),
            workflow: workflow.map(str::to_string),
            required,
        }
    }

    fn inventory_row_produced_by(
        name: &str,
        producer: Option<&str>,
        workflow: Option<&str>,
        required: bool,
    ) -> InventoryCheck {
        InventoryCheck {
            name: name.to_string(),
            producer: producer.map(str::to_string),
            workflow: workflow.map(str::to_string),
            required,
        }
    }

    /// The defect in #16164: `[[checks]]` was never deserialized, so a
    /// ruleset-required context could name a workflow the contract had never
    /// been evaluated against and nothing said a word.
    #[test]
    fn required_context_without_a_governance_row_is_reported() {
        let summary = governance_summary(&policy(
            vec![governance_row("ci.yml", true)],
            vec![
                inventory_row("Conflict marker check", Some("ci.yml"), true),
                inventory_row("ripr+ New Gap Gate", Some("ripr.yml"), true),
            ],
        ));

        assert_eq!(summary.required_contexts, 2);
        assert_eq!(summary.governed, 1);
        assert_eq!(summary.ungoverned.len(), 1, "{:?}", summary.ungoverned);
        assert!(
            summary.ungoverned[0].contains("ripr.yml"),
            "the ungoverned entry must name the workflow: {:?}",
            summary.ungoverned
        );
    }

    /// A `[[check]]` row with `required = false` produces no violations at
    /// all, so counting it as governance would reinstate the silence.
    #[test]
    fn a_non_required_governance_row_governs_nothing() {
        let summary = governance_summary(&policy(
            vec![governance_row("ripr.yml", false)],
            vec![inventory_row("ripr+ New Gap Gate", Some("ripr.yml"), true)],
        ));

        assert_eq!(summary.governed, 0);
        assert_eq!(summary.ungoverned.len(), 1, "{:?}", summary.ungoverned);
    }

    /// An external producer has no workflow in this repository, so demanding a
    /// governance row for it would be a requirement nobody could satisfy.
    #[test]
    fn an_external_producer_is_counted_out_of_scope_rather_than_governed() {
        // The trigger contract governs this repository's workflows. A row that
        // declares an external producer is outside it, so demanding a
        // `[[check]]` row for it would be a demand nobody could satisfy — but it
        // is not governed either, and the summary has to say which.
        let summary = governance_summary(&policy(
            Vec::new(),
            vec![inventory_row_produced_by(
                "codecov/patch",
                Some("external"),
                Some("codecov"),
                true,
            )],
        ));

        assert_eq!(summary.required_contexts, 1);
        assert_eq!(summary.external, 1);
        assert_eq!(summary.governed, 0, "an unreachable context is not an evaluated one");
        assert!(summary.ungoverned.is_empty(), "{:?}", summary.ungoverned);
    }

    /// The fail-open path #16164's review found: an earlier revision read a
    /// missing `workflow` as an external producer, so a repository job whose row
    /// omitted its workflow counted as governed with nothing evaluated.
    #[test]
    fn a_repository_job_with_no_workflow_is_reported_rather_than_assumed_external() {
        let summary = governance_summary(&policy(
            Vec::new(),
            vec![inventory_row_produced_by(
                "Some Required Job",
                Some("repository-job"),
                None,
                true,
            )],
        ));

        assert_eq!(summary.external, 0, "a missing workflow does not make a producer external");
        assert_eq!(summary.governed, 0);
        assert_eq!(summary.ungoverned.len(), 1, "{:?}", summary.ungoverned);
        assert!(
            summary.ungoverned[0].contains("names no workflow"),
            "the entry must say what is missing: {:?}",
            summary.ungoverned
        );
    }

    #[test]
    fn a_required_context_with_no_producer_is_reported() {
        let summary = governance_summary(&policy(
            vec![governance_row("ci.yml", true)],
            vec![inventory_row_produced_by("Some Required Job", None, Some("ci.yml"), true)],
        ));

        assert_eq!(summary.external, 0);
        assert_eq!(summary.governed, 0, "an unclassified producer is not a governed one");
        assert_eq!(summary.ungoverned.len(), 1, "{:?}", summary.ungoverned);
        assert!(
            summary.ungoverned[0].contains("names no producer"),
            "the entry must say what is missing: {:?}",
            summary.ungoverned
        );
    }

    #[test]
    fn an_exempted_clause_is_recorded_rather_than_reported() -> Result<()> {
        let fixture = load_fixture("missing-merge-group.yml")?;
        let eval = evaluate_required_entry(
            "fixture",
            "fixture.yml".to_string(),
            true,
            true,
            Some(&fixture),
            &[exemption("merge-group-trigger", ExemptionStatus::Accepted)],
        );

        assert!(
            !eval.violations.iter().any(|item| item.contains("merge_group")),
            "the exempted clause must leave the violation list: {:?}",
            eval.violations
        );
        assert_eq!(eval.exemptions.len(), 1);
        assert_eq!(eval.exemptions[0].clause, "merge-group-trigger");
        assert!(
            eval.exemptions[0].suppressed.contains("merge_group"),
            "the record must carry the message it suppresses: {:?}",
            eval.exemptions[0]
        );
        Ok(())
    }

    /// The rule that keeps an exemption list from being a one-way ratchet.
    #[test]
    fn an_exemption_for_a_clause_the_workflow_satisfies_is_a_violation() -> Result<()> {
        let fixture = load_fixture("valid-required.yml")?;
        let eval = evaluate_required_entry(
            "fixture",
            "fixture.yml".to_string(),
            true,
            true,
            Some(&fixture),
            &[exemption("merge-group-trigger", ExemptionStatus::Accepted)],
        );

        assert!(!eval.ok, "a stale exemption must fail the row");
        assert!(
            eval.violations.iter().any(|item| item.contains("stale exemption")),
            "{:?}",
            eval.violations
        );
        Ok(())
    }

    /// `apply_exemptions` records the first entry that matches a clause, so a
    /// second entry for the same clause is decided by list order: an `accepted`
    /// above a `remainder` swallows the remainder, which then appears in no
    /// count and no receipt (#16164 review).
    #[test]
    fn governance_line_accounts_for_every_required_context() -> Result<()> {
        // The JSON receipt has always carried `external`; the text line did not,
        // so a reader saw `governed/required` and read the difference as gaps
        // (#16172 review). The three counts must partition the required set, or
        // the line is arithmetic that does not add up.
        let summary = GovernanceSummary {
            required_contexts: 5,
            governed: 3,
            external: 1,
            ungoverned: vec!["some-context: names no workflow".to_string()],
            accepted_exemptions: 2,
            remainders: 1,
        };
        ensure!(
            summary.governed + summary.external + summary.ungoverned.len()
                == summary.required_contexts,
            "the fixture itself must partition the required set: \
             {} + {} + {} != {}",
            summary.governed,
            summary.external,
            summary.ungoverned.len(),
            summary.required_contexts
        );
        // The receipt must also reconcile with what the classifier actually
        // derives from a required set exercising every class at once: three
        // governed contexts, one external producer, one repository job that
        // names no workflow. A count that drifts from the classified outcomes
        // breaks the same partition the line prints (#16201).
        let classified = governance_summary(&policy(
            vec![governance_row("ci.yml", true)],
            vec![
                inventory_row("Governed One", Some("ci.yml"), true),
                inventory_row("Governed Two", Some("ci.yml"), true),
                inventory_row("Governed Three", Some("ci.yml"), true),
                inventory_row_produced_by("codecov/patch", Some("external"), Some("codecov"), true),
                inventory_row_produced_by("Stray Job", Some("repository-job"), None, true),
            ],
        ));
        ensure!(
            classified.governed == 3,
            "three repository rows name a governed workflow: got {}",
            classified.governed
        );
        ensure!(
            classified.external == 1,
            "one external producer is counted out of scope: got {}",
            classified.external
        );
        ensure!(
            classified.ungoverned.len() == 1,
            "one workflow-less job stays ungoverned: got {:?}",
            classified.ungoverned
        );
        ensure!(
            classified.governed + classified.external + classified.ungoverned.len()
                == classified.required_contexts,
            "the classified counts must partition the required set: \
             {} + {} + {} != {}",
            classified.governed,
            classified.external,
            classified.ungoverned.len(),
            classified.required_contexts
        );

        let line = summary.summary_line();
        ensure!(
            line.contains("3/5 ruleset-required contexts governed"),
            "the governed fraction must lead the line: {line}"
        );
        ensure!(
            line.contains("1 produced outside this repository"),
            "the out-of-scope count is the one the text used to drop: {line}"
        );
        ensure!(line.contains("1 ungoverned"), "the gap count must be printed: {line}");
        Ok(())
    }

    #[test]
    fn a_clause_carrying_two_exemptions_is_a_violation() -> Result<()> {
        let fixture = load_fixture("missing-merge-group.yml")?;
        let accepted = exemption("merge-group-trigger", ExemptionStatus::Accepted);
        let mut shadowed = exemption("merge-group-trigger", ExemptionStatus::Remainder);
        shadowed.tracking = Some("16164".to_string());
        let eval = evaluate_required_entry(
            "fixture",
            "fixture.yml".to_string(),
            true,
            true,
            Some(&fixture),
            &[accepted, shadowed],
        );

        assert!(
            eval.violations.iter().any(|item| item.contains("more than one exemption")),
            "a clause with two dispositions must not be decided by list order: {:?}",
            eval.violations
        );
        Ok(())
    }

    #[test]
    fn an_exemption_naming_an_unknown_clause_is_a_violation() -> Result<()> {
        let fixture = load_fixture("missing-merge-group.yml")?;
        let eval = evaluate_required_entry(
            "fixture",
            "fixture.yml".to_string(),
            true,
            true,
            Some(&fixture),
            &[exemption("merge_group", ExemptionStatus::Accepted)],
        );

        assert!(
            eval.violations.iter().any(|item| item.contains("unknown clause")),
            "an underscore typo must be refused, not silently exempt nothing: {:?}",
            eval.violations
        );
        // The real clause is still unaccounted for.
        assert!(eval.violations.iter().any(|item| item.contains("merge_group trigger")));
        Ok(())
    }

    #[test]
    fn a_remainder_without_a_tracking_issue_is_a_violation() -> Result<()> {
        let fixture = load_fixture("missing-merge-group.yml")?;
        let mut exempt = exemption("merge-group-trigger", ExemptionStatus::Remainder);
        exempt.tracking = None;
        let eval = evaluate_required_entry(
            "fixture",
            "fixture.yml".to_string(),
            true,
            true,
            Some(&fixture),
            &[exempt],
        );

        assert!(
            eval.violations.iter().any(|item| item.contains("tracking issue")),
            "{:?}",
            eval.violations
        );
        Ok(())
    }

    #[test]
    fn an_exemption_without_a_reason_is_a_violation() -> Result<()> {
        let fixture = load_fixture("missing-merge-group.yml")?;
        let mut exempt = exemption("merge-group-trigger", ExemptionStatus::Accepted);
        exempt.reason = "   ".to_string();
        let eval = evaluate_required_entry(
            "fixture",
            "fixture.yml".to_string(),
            true,
            true,
            Some(&fixture),
            &[exempt],
        );

        assert!(
            eval.violations.iter().any(|item| item.contains("no reason")),
            "{:?}",
            eval.violations
        );
        Ok(())
    }
}

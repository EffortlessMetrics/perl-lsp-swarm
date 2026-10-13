//! Required-context producer and subject binding contract (#15348).
//!
//! Every `[[checks]]` row marked `required = true` is a live ruleset status
//! context. A required-check name is presentation identity, not producer
//! authority: the ledger must bind each required row to its accepted app
//! integration (`ruleset_integration_id`), its single repository producer
//! (`workflow` + `job`), and the events on which that producer emits it. A
//! required row without that binding leaves merge authorization resting on an
//! ambiguous producer, which is exactly the gap #15348 records.
//!
//! Positive control: the checked-in ledger satisfies the contract and every
//! declared producer actually exists in its workflow. Negative controls:
//! fixtures reproducing the three recorded gap shapes (name-only row,
//! duplicate producer authority, producer without a job identity) are all
//! rejected.

use std::fs;
use std::path::{Component, Path};

use serde_yaml_ng::Value;
use toml::Value as TomlValue;

const POLICY_PATH: &str = ".ci/policies/required-checks.toml";

/// Structural ledger contract for the required-context inventory.
///
/// Returns one violation string per broken binding. Pure over the parsed
/// policy so the negative-control fixtures can exercise it without touching
/// repository files.
fn ledger_violations(policy: &TomlValue) -> Vec<String> {
    let mut violations = Vec::new();

    let Some(rows) = policy.get("checks").and_then(TomlValue::as_array) else {
        violations.push("policy must declare a `[[checks]]` inventory array".to_string());
        return violations;
    };

    // One accepted producer identity per required context name: a second row
    // capable of emitting the same name makes the producing job ambiguous.
    let mut seen_names: std::collections::BTreeMap<&str, usize> = Default::default();
    let mut seen_producers: std::collections::BTreeMap<(String, String), &str> = Default::default();

    for (index, row) in rows.iter().enumerate() {
        let name = row.get("name").and_then(TomlValue::as_str).unwrap_or("");
        if name.is_empty() {
            violations.push(format!("checks[{index}] must declare a `name`"));
            continue;
        }
        let required = row.get("required").and_then(TomlValue::as_bool) == Some(true);
        if !required {
            continue;
        }

        if let Some(previous) = seen_names.insert(name, index) {
            violations.push(format!(
                "required context `{name}` is declared by checks[{previous}] and \
                 checks[{index}]; a required context must have exactly one accepted \
                 producer row"
            ));
        }

        // A required status context must be ruleset-enforced. `enforcement =
        // "neither"` on a required row is the recorded #15348 gap shape: the
        // name is required live but the ledger claims no enforcement source.
        if row.get("enforcement").and_then(TomlValue::as_str) != Some("github-ruleset") {
            violations.push(format!(
                "required context `{name}` must declare `enforcement = \
                 \"github-ruleset\"`; a required row without live enforcement is \
                 name-only and cannot state its accepted producer"
            ));
        }

        // The strongest live binding GitHub supports: the app/integration
        // identity allowed to satisfy the row. Missing means any terminal
        // check run with the same display name is accepted.
        let binding = row.get("ruleset_integration_id").and_then(TomlValue::as_integer);
        match binding {
            Some(id) if id > 0 => {}
            Some(id) => {
                violations.push(format!(
                    "required context `{name}` carries non-positive \
                     `ruleset_integration_id` {id}"
                ));
            }
            None => violations.push(format!(
                "required context `{name}` must carry `ruleset_integration_id`; \
                 a name-only required row leaves the accepted app identity unstated"
            )),
        }

        // Repository producer identity: workflow path plus the exact job that
        // emits the context.
        let producer = row.get("producer").and_then(TomlValue::as_str);
        match producer {
            Some("repository-job") => {}
            Some(other) => {
                violations.push(format!(
                    "required context `{name}` has unsupported producer `{other}`; \
                     required rows must bind to a repository job"
                ));
                continue;
            }
            None => {
                violations.push(format!("required context `{name}` must declare a `producer`"));
                continue;
            }
        }

        let workflow = row.get("workflow").and_then(TomlValue::as_str);
        match workflow {
            Some(path) if valid_workflow_path(path) => {}
            Some(path) => {
                violations.push(format!(
                    "required context `{name}` workflow `{path}` is outside \
                     `.github/workflows/`"
                ));
            }
            None => {
                violations.push(format!("required context `{name}` must declare a `workflow` path"))
            }
        }

        let job = row.get("job").and_then(TomlValue::as_str);
        match job {
            Some(job) if !job.trim().is_empty() => {
                if let Some(path) = workflow {
                    let key = (path.to_string(), job.to_string());
                    if let Some(previous) = seen_producers.insert(key, name) {
                        violations.push(format!(
                            "required contexts `{previous}` and `{name}` both bind \
                             producer {path}#{job}; one job must not satisfy two \
                             required contexts"
                        ));
                    }
                }
            }
            Some(_) => {
                violations
                    .push(format!("required context `{name}` declares an empty `job` identity"));
            }
            None => violations
                .push(format!("required context `{name}` must declare the exact emitting `job`")),
        }

        // Events on which the producer emits the context: the subject/event
        // class the binding covers. A row reachable on no event cannot supply
        // the required context on any merge path.
        match row.get("events").and_then(TomlValue::as_array) {
            Some(events) if !events.is_empty() => {}
            _ => violations
                .push(format!("required context `{name}` must declare a non-empty `events` set")),
        }
    }

    violations
}

/// Producer existence: every required repository-job row must point at a
/// workflow file that declares the bound job id.
fn producer_existence_violations(root: &Path, policy: &TomlValue) -> Vec<String> {
    let mut violations = Vec::new();
    let Some(rows) = policy.get("checks").and_then(TomlValue::as_array) else {
        return violations;
    };

    for row in rows {
        if row.get("required").and_then(TomlValue::as_bool) != Some(true) {
            continue;
        }
        let name = row.get("name").and_then(TomlValue::as_str).unwrap_or("");
        let (Some(workflow), Some(job)) = (
            row.get("workflow").and_then(TomlValue::as_str),
            row.get("job").and_then(TomlValue::as_str),
        ) else {
            continue;
        };

        if !valid_workflow_path(workflow) {
            violations.push(format!(
                "required context `{name}` workflow `{workflow}` is not a safe workflow path"
            ));
            continue;
        }
        let path = root.join(workflow);
        let Ok(raw) = fs::read_to_string(&path) else {
            violations.push(format!(
                "required context `{name}` producer workflow `{workflow}` does not exist"
            ));
            continue;
        };
        let document: Value = match serde_yaml_ng::from_str(&raw) {
            Ok(document) => document,
            Err(error) => {
                violations.push(format!(
                    "required context `{name}` producer workflow `{workflow}` does \
                     not parse: {error}"
                ));
                continue;
            }
        };
        let declares_job = document
            .get("jobs")
            .and_then(Value::as_mapping)
            .is_some_and(|jobs| jobs.contains_key(Value::String(job.to_string())));
        if !declares_job {
            violations.push(format!(
                "required context `{name}` binds job `{job}`, but `{workflow}` does \
                 not declare it"
            ));
        }
    }

    violations
}

fn fixture(toml_text: &str) -> TomlValue {
    toml::from_str(toml_text).expect("fixture must parse as TOML")
}

fn valid_workflow_path(path: &str) -> bool {
    path.starts_with(".github/workflows/")
        && Path::new(path).components().all(|component| !matches!(component, Component::ParentDir))
}

const WELL_FORMED_ROW: &str = r#"
version = 2
[[checks]]
name = "Example Required Context"
producer = "repository-job"
workflow = ".github/workflows/example.yml"
job = "example-job"
workflow_result = "propagate"
events = ["pull_request", "merge_group"]
required = true
policy_role = "required"
applicability = "always-or-scoped-noop"
enforcement = "github-ruleset"
ruleset_integration_id = 15368
"#;

#[test]
fn well_formed_required_row_satisfies_the_binding_contract() {
    let policy = fixture(WELL_FORMED_ROW);
    assert!(
        ledger_violations(&policy).is_empty(),
        "a fully bound required row must not produce violations: {:?}",
        ledger_violations(&policy)
    );
}

#[test]
fn required_row_without_integration_binding_is_rejected() {
    // The exact #15348 gap shape: live-required name, no app binding.
    let policy = fixture(&WELL_FORMED_ROW.replace("ruleset_integration_id = 15368\n", ""));
    let violations = ledger_violations(&policy);
    assert!(
        violations.iter().any(|violation| violation.contains("Example Required Context")
            && violation.contains("ruleset_integration_id")),
        "a name-only required row must be rejected: {violations:?}"
    );
}

#[test]
fn required_row_without_ruleset_enforcement_is_rejected() {
    let policy = fixture(
        &WELL_FORMED_ROW.replace("enforcement = \"github-ruleset\"", "enforcement = \"neither\""),
    );
    let violations = ledger_violations(&policy);
    assert!(
        violations.iter().any(|violation| violation.contains("Example Required Context")
            && violation.contains("github-ruleset")),
        "an unenforced required row must be rejected: {violations:?}"
    );
}

#[test]
fn duplicate_required_context_is_rejected() {
    let policy = fixture(&format!(
        "{WELL_FORMED_ROW}\n[[checks]]\nname = \"Example Required Context\"\nproducer = \"repository-job\"\nworkflow = \".github/workflows/other.yml\"\njob = \"other-job\"\nevents = [\"pull_request\"]\nrequired = true\nenforcement = \"github-ruleset\"\nruleset_integration_id = 15368\n"
    ));
    let violations = ledger_violations(&policy);
    assert!(
        violations.iter().any(|violation| violation.contains("exactly one accepted producer row")),
        "two rows for one required context must be rejected: {violations:?}"
    );
}

#[test]
fn required_row_without_job_identity_is_rejected() {
    let policy = fixture(&WELL_FORMED_ROW.replace("job = \"example-job\"\n", ""));
    let violations = ledger_violations(&policy);
    assert!(
        violations.iter().any(|violation| violation.contains("Example Required Context")
            && violation.contains("`job`")),
        "a required row without an emitting job must be rejected: {violations:?}"
    );
}

#[test]
fn workflow_paths_with_parent_components_are_rejected_before_file_access() {
    let policy = fixture(&WELL_FORMED_ROW.replace(
        "workflow = \".github/workflows/example.yml\"",
        "workflow = \".github/workflows/../secrets.yml\"",
    ));
    let violations = ledger_violations(&policy);
    assert!(
        violations.iter().any(|violation| {
            violation.contains("Example Required Context")
                && (violation.contains("outside `.github/workflows/`")
                    || violation.contains("safe workflow path"))
        }),
        "a workflow path that escapes its allowed directory must be rejected: {violations:?}"
    );
    assert!(!valid_workflow_path(".github/workflows/../secrets.yml"));
    assert!(valid_workflow_path(".github/workflows/ci.yml"));
}

#[test]
fn required_contexts_carry_full_producer_and_subject_binding() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask lives directly under the repository root")
        .to_path_buf();
    let raw =
        fs::read_to_string(root.join(POLICY_PATH)).expect("required-checks policy must exist");
    let policy: TomlValue = toml::from_str(&raw).expect("policy must parse as TOML");

    let violations =
        [ledger_violations(&policy), producer_existence_violations(&root, &policy)].concat();
    assert!(
        violations.is_empty(),
        "every required status context must be bound to its accepted producer \
         and subject contract (see #15348): {violations:#?}"
    );
}

//! Regression contract for issue #13047: the provider-native publication skills and
//! the pull-request front door retain one ordered shift-left proof envelope.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const TEMPLATE_PATH: &str = ".github/PULL_REQUEST_TEMPLATE.md";
const WORKFLOW_PATH: &str = ".github/workflows/agent-flow-control-plane.yml";
const PUBLICATION_CHECK_COMMAND: &str =
    "cargo test -p xtask --test shift_left_publication_contract --locked";
const PROVIDER_SKILLS: &[(&str, &str)] = &[
    ("Codex", ".agents/skills/publish-pr/SKILL.md"),
    ("Claude", ".claude/skills/publish-pr/SKILL.md"),
];
const REQUIRED_WORKFLOW_PATHS: &[&str] = &[
    TEMPLATE_PATH,
    ".agents/skills/**",
    ".claude/skills/**",
    "xtask/tests/shift_left_publication_contract.rs",
];
const WORKFLOW_EVENTS: &[&str] = &["pull_request", "push"];
const REVIEW_INDEX_HEADINGS: &[&str] = &[
    "## Claim",
    "## Controlling issue",
    "## Governing contract",
    "## Changed production path",
    "## Proof",
    "## Test hardening",
    "## Simplification",
    "## Deviations",
    "## Claim Boundary",
    "## Non-goals",
    "## Risk and rollback",
    "## Review index",
];

fn project_root() -> Result<PathBuf, Box<dyn std::error::Error>> {
    Ok(PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("CARGO_MANIFEST_DIR has no parent")?
        .to_path_buf())
}

fn read(root: &Path, path: &str) -> Result<String, Box<dyn std::error::Error>> {
    Ok(fs::read_to_string(root.join(path))?)
}

fn workflow(root: &Path) -> Result<serde_yaml_ng::Value, Box<dyn std::error::Error>> {
    let content = read(root, WORKFLOW_PATH)?;
    Ok(serde_yaml_ng::from_str(&content)?)
}

fn contract_error(message: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn mapping_value<'a>(
    value: &'a serde_yaml_ng::Value,
    key: &str,
) -> Result<&'a serde_yaml_ng::Value, String> {
    value
        .as_mapping()
        .ok_or_else(|| format!("expected YAML mapping while looking for {key:?}"))?
        .get(serde_yaml_ng::Value::String(key.to_owned()))
        .ok_or_else(|| format!("missing YAML key {key:?}"))
}

fn mapping_value_mut<'a>(
    value: &'a mut serde_yaml_ng::Value,
    key: &str,
) -> Result<&'a mut serde_yaml_ng::Value, String> {
    value
        .as_mapping_mut()
        .ok_or_else(|| format!("expected YAML mapping while looking for {key:?}"))?
        .get_mut(serde_yaml_ng::Value::String(key.to_owned()))
        .ok_or_else(|| format!("missing YAML key {key:?}"))
}

fn workflow_paths(workflow: &serde_yaml_ng::Value, event: &str) -> Result<Vec<String>, String> {
    let event_value = mapping_value(mapping_value(workflow, "on")?, event)?;
    mapping_value(event_value, "paths")?
        .as_sequence()
        .ok_or_else(|| format!("workflow on.{event}.paths must be a YAML sequence"))?
        .iter()
        .map(|path| {
            path.as_str()
                .map(str::to_owned)
                .ok_or_else(|| format!("workflow on.{event}.paths entries must be strings"))
        })
        .collect()
}

fn validate_workflow(workflow: &serde_yaml_ng::Value) -> Result<(), String> {
    for &event in WORKFLOW_EVENTS {
        let paths = workflow_paths(workflow, event)?;
        for &required in REQUIRED_WORKFLOW_PATHS {
            if !paths.iter().any(|path| path == required) {
                return Err(format!("{WORKFLOW_PATH}: on.{event}.paths must watch {required:?}"));
            }
        }
    }

    let jobs = mapping_value(workflow, "jobs")?;
    let check = mapping_value(jobs, "check")?;
    let steps = mapping_value(check, "steps")?
        .as_sequence()
        .ok_or_else(|| format!("{WORKFLOW_PATH}: jobs.check.steps must be a YAML sequence"))?;
    let named_step = steps.iter().any(|step| {
        mapping_value(step, "name")
            .and_then(|name| name.as_str().ok_or_else(|| "name is not a string".to_owned()))
            .is_ok_and(|name| name == "Check shift-left publication contract")
            && mapping_value(step, "run")
                .and_then(|run| run.as_str().ok_or_else(|| "run is not a string".to_owned()))
                .is_ok_and(|run| run.trim() == PUBLICATION_CHECK_COMMAND)
    });
    if !named_step {
        return Err(format!(
            "{WORKFLOW_PATH}: jobs.check must execute the named publication-contract command"
        ));
    }

    let command_count = jobs
        .as_mapping()
        .ok_or_else(|| format!("{WORKFLOW_PATH}: jobs must be a YAML mapping"))?
        .values()
        .filter_map(|job| mapping_value(job, "steps").ok())
        .filter_map(serde_yaml_ng::Value::as_sequence)
        .flat_map(|steps| steps.iter())
        .filter_map(|step| mapping_value(step, "run").ok())
        .filter_map(serde_yaml_ng::Value::as_str)
        .filter(|run| run.trim() == PUBLICATION_CHECK_COMMAND)
        .count();
    if command_count != 1 {
        return Err(format!(
            "{WORKFLOW_PATH}: publication-contract command must occur exactly once, found {command_count}"
        ));
    }
    Ok(())
}

fn remove_workflow_path(
    workflow: &mut serde_yaml_ng::Value,
    event: &str,
    path: &str,
) -> Result<(), String> {
    let event_value = mapping_value_mut(mapping_value_mut(workflow, "on")?, event)?;
    let paths = mapping_value_mut(event_value, "paths")?
        .as_sequence_mut()
        .ok_or_else(|| format!("workflow on.{event}.paths must be a YAML sequence"))?;
    let original_len = paths.len();
    paths.retain(|entry| entry.as_str() != Some(path));
    if paths.len() == original_len {
        return Err(format!("workflow fixture path {path:?} was not present"));
    }
    Ok(())
}

fn replace_publication_command(workflow: &mut serde_yaml_ng::Value) -> Result<(), String> {
    let steps = mapping_value_mut(
        mapping_value_mut(mapping_value_mut(workflow, "jobs")?, "check")?,
        "steps",
    )?
    .as_sequence_mut()
    .ok_or_else(|| format!("{WORKFLOW_PATH}: jobs.check.steps must be a YAML sequence"))?;
    let step = steps
        .iter_mut()
        .find(|step| {
            mapping_value(step, "name")
                .and_then(|name| name.as_str().ok_or_else(|| "name is not a string".to_owned()))
                .is_ok_and(|name| name == "Check shift-left publication contract")
        })
        .ok_or_else(|| "publication-contract step fixture was not present".to_owned())?;
    *mapping_value_mut(step, "run")? = serde_yaml_ng::Value::String("true".to_owned());
    Ok(())
}

fn prose(text: &str) -> String {
    text.chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '_' {
                character.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn exact_heading_positions(document: &str, heading: &str) -> Vec<usize> {
    let mut offset = 0;
    let mut positions = Vec::new();
    for line in document.split_inclusive('\n') {
        let content = line.trim_end_matches(['\r', '\n']);
        if content == heading {
            positions.push(offset);
        }
        offset += line.len();
    }
    positions
}

fn validate_ordered_headings(
    document: &str,
    subject: &str,
    headings: &[&str],
) -> Result<(), String> {
    let mut previous = None;
    for &heading in headings {
        let positions = exact_heading_positions(document, heading);
        if positions.len() != 1 {
            return Err(format!(
                "{subject}: expected exactly one heading {heading:?}, found {}",
                positions.len()
            ));
        }
        let position = positions
            .first()
            .copied()
            .ok_or_else(|| format!("{subject}: missing heading {heading:?}"))?;
        if previous.is_some_and(|prior| position <= prior) {
            return Err(format!(
                "{subject}: heading {heading:?} is out of the shift-left review order"
            ));
        }
        previous = Some(position);
    }
    Ok(())
}

fn section<'a>(document: &'a str, heading: &str) -> Result<&'a str, String> {
    let positions = exact_heading_positions(document, heading);
    if positions.len() != 1 {
        return Err(format!(
            "template: expected exactly one section {heading:?}, found {}",
            positions.len()
        ));
    }
    let start = positions
        .first()
        .copied()
        .ok_or_else(|| format!("template: missing section {heading:?}"))?
        + heading.len();
    let tail = &document[start..];
    let content = tail.strip_prefix("\r\n").or_else(|| tail.strip_prefix('\n')).unwrap_or(tail);
    let end = content.find("\n## ").unwrap_or(content.len());
    Ok(&content[..end])
}

fn require_phrases(text: &str, subject: &str, phrases: &[&str]) -> Result<(), String> {
    let normalized = prose(text);
    for &phrase in phrases {
        let expected = prose(phrase);
        if !normalized.contains(&expected) {
            return Err(format!("{subject}: missing shift-left semantic marker {phrase:?}"));
        }
    }
    Ok(())
}

fn validate_template(template: &str) -> Result<(), String> {
    validate_ordered_headings(template, TEMPLATE_PATH, REVIEW_INDEX_HEADINGS)?;
    require_phrases(
        template,
        TEMPLATE_PATH,
        &[
            "a pr owns one coherent acceptance and rollback claim",
            "do not fabricate evidence",
            "i ran the cheapest discriminating proof first",
            "focused and affected proof covers the candidate s changed semantic subjects",
        ],
    )?;
    require_phrases(
        section(template, "## Changed production path")?,
        "Changed production path",
        &["real user protocol runtime route", "changed behavior"],
    )?;
    require_phrases(
        section(template, "## Proof")?,
        "Proof",
        &[
            "exact focused commands tests fixtures external oracle and observed results",
            "distinguish pass fail not run not_proven",
        ],
    )?;
    require_phrases(
        section(template, "## Test hardening")?,
        "Test hardening",
        &[
            "realistic wrong implementation",
            "negative stale failure recovery or opposite direction control",
        ],
    )?;
    require_phrases(
        section(template, "## Simplification")?,
        "Simplification",
        &[
            "duplicate authority scaffolding overbroad api repeated validation or dead compatibility",
        ],
    )?;
    require_phrases(
        section(template, "## Claim Boundary")?,
        "Claim Boundary",
        &["provably true", "explicitly out of scope"],
    )?;
    require_phrases(
        section(template, "## Non-goals")?,
        "Non-goals",
        &["unrun proof unsupported cases and remaining work"],
    )?;
    require_phrases(
        section(template, "## Risk and rollback")?,
        "Risk and rollback",
        &["failure modes compatibility support effects", "reverted or disabled"],
    )?;
    require_phrases(
        section(template, "## Review index")?,
        "Review index",
        &["governing contract key production seams proof generated artifacts and high risk files"],
    )?;
    Ok(())
}

fn validate_provider_skill(provider: &str, skill: &str) -> Result<(), String> {
    validate_ordered_headings(skill, provider, REVIEW_INDEX_HEADINGS)?;
    require_phrases(
        section(skill, "## PR review index")?,
        provider,
        &[
            "the order is load bearing",
            "trace the changed production path",
            "focused and affected proof",
            "pass fail not run not_proven",
            "realistic wrong implementation",
            "negative stale failure recovery or opposite direction controls",
            "simplify before publication",
            "bound the claim and non goals",
            "risk rollback and review locations",
        ],
    )?;
    if skill.contains("## What this establishes")
        || skill.contains("## What this does not establish")
    {
        return Err(format!(
            "{provider}: retired establishment headings diverge from the PR front door"
        ));
    }
    Ok(())
}

#[test]
fn publication_contract_is_current() -> Result<(), Box<dyn std::error::Error>> {
    let root = project_root()?;
    let workflow = workflow(&root)?;
    validate_workflow(&workflow).map_err(contract_error)?;

    let template = read(&root, TEMPLATE_PATH)?;
    validate_template(&template).map_err(contract_error)?;

    for &(provider, path) in PROVIDER_SKILLS {
        let skill = read(&root, path)?;
        validate_provider_skill(provider, &skill).map_err(contract_error)?;
    }
    Ok(())
}

#[test]
fn ratchet_rejects_workflow_trigger_and_command_mutations() -> Result<(), Box<dyn std::error::Error>>
{
    let root = project_root()?;
    let workflow = workflow(&root)?;
    validate_workflow(&workflow).map_err(contract_error)?;

    for &event in WORKFLOW_EVENTS {
        let mut missing_template_path = workflow.clone();
        remove_workflow_path(&mut missing_template_path, event, TEMPLATE_PATH)
            .map_err(contract_error)?;
        assert!(
            validate_workflow(&missing_template_path).is_err(),
            "removing the template trigger path from on.{event} must fail the contract"
        );

        let mut missing_codex_path = workflow.clone();
        remove_workflow_path(&mut missing_codex_path, event, ".agents/skills/**")
            .map_err(contract_error)?;
        assert!(
            validate_workflow(&missing_codex_path).is_err(),
            "removing the Codex skill trigger path from on.{event} must fail the contract"
        );

        let mut missing_claude_path = workflow.clone();
        remove_workflow_path(&mut missing_claude_path, event, ".claude/skills/**")
            .map_err(contract_error)?;
        assert!(
            validate_workflow(&missing_claude_path).is_err(),
            "removing the Claude skill trigger path from on.{event} must fail the contract"
        );
    }

    let mut missing_command = workflow;
    replace_publication_command(&mut missing_command).map_err(contract_error)?;
    assert!(
        validate_workflow(&missing_command).is_err(),
        "replacing the publication-contract command must fail the contract"
    );
    Ok(())
}

#[test]
fn ratchet_rejects_missing_or_reordered_template_sections() -> Result<(), Box<dyn std::error::Error>>
{
    let root = project_root()?;
    let template = read(&root, TEMPLATE_PATH)?;

    let missing_hardening = template.replacen("## Test hardening\n", "", 1);
    assert_ne!(missing_hardening, template, "missing-section mutation fixture must apply");
    assert!(
        validate_template(&missing_hardening).is_err(),
        "removing test hardening must fail the publication contract"
    );

    let reordered = template
        .replacen("## Proof", "## __publication_contract_swap__", 1)
        .replacen("## Review index", "## Proof", 1)
        .replacen("## __publication_contract_swap__", "## Review index", 1);
    assert_ne!(reordered, template, "reordering mutation fixture must apply");
    assert!(
        validate_template(&reordered).is_err(),
        "moving proof behind the review index must fail the publication contract"
    );
    Ok(())
}

#[test]
fn ratchet_rejects_weakened_proof_and_provider_drift() -> Result<(), Box<dyn std::error::Error>> {
    let root = project_root()?;
    let template = read(&root, TEMPLATE_PATH)?;
    let weakened =
        template.replacen("Distinguish pass/fail/not-run/NOT_PROVEN.", "List the result.", 1);
    assert_ne!(weakened, template, "proof mutation fixture must apply");
    assert!(
        validate_template(&weakened).is_err(),
        "collapsing typed proof states must fail the publication contract"
    );

    let codex = read(&root, ".agents/skills/publish-pr/SKILL.md")?;
    let drifted = codex.replacen("## Claim Boundary", "## What this establishes", 1);
    assert_ne!(drifted, codex, "provider drift mutation fixture must apply");
    assert!(
        validate_provider_skill("Codex", &drifted).is_err(),
        "restoring a retired provider-only boundary heading must fail"
    );

    // Genuine relocation: lift the review-index body out of its own section and
    // re-seat it above the heading. Every marker stays present in the document
    // but none remains inside the section that must carry it, which is exactly
    // the scoping law `validate_provider_skill` enforces. Derived from the
    // section rather than from hardcoded prose so the fixture cannot silently
    // stop matching when the guidance is reworded.
    let index_body = section(&codex, "## PR review index").map_err(contract_error)?.to_owned();
    let relocated = codex.replacen(&index_body, "", 1).replacen(
        "## PR review index",
        &format!("{}\n## PR review index", index_body.trim()),
        1,
    );
    assert_ne!(relocated, codex, "provider-marker relocation fixture must apply");
    assert!(
        relocated.contains("The order is load-bearing"),
        "relocation fixture must keep the markers in the document, only outside the section"
    );
    assert!(
        validate_provider_skill("Codex", &relocated).is_err(),
        "moving a provider marker outside the PR review-index section must fail"
    );
    Ok(())
}

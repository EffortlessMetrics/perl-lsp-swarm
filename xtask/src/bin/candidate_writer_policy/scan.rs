use crate::model::{Finding, FindingKind};
use serde_yaml_ng::{Mapping, Value};
use std::fs;
use std::path::{Path, PathBuf};

const WORKFLOW_DIR: &str = ".github/workflows";
const WRITE_SCOPES: &[&str] = &["actions", "contents", "pull-requests"];

pub(crate) fn project_root() -> Result<PathBuf, String> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| "xtask manifest directory has no repository parent".to_string())
}

pub(crate) fn scan_repository(root: &Path) -> Result<Vec<Finding>, String> {
    let workflows = root.join(WORKFLOW_DIR);
    let entries = fs::read_dir(&workflows)
        .map_err(|error| format!("reading {}: {error}", workflows.display()))?;
    let mut paths = Vec::new();
    for entry in entries {
        let path = entry
            .map_err(|error| format!("reading entry in {}: {error}", workflows.display()))?
            .path();
        if matches!(
            path.extension().and_then(|value| value.to_str()),
            Some("yml" | "yaml")
        ) {
            paths.push(path);
        }
    }
    paths.sort();

    let mut findings = Vec::new();
    for path in paths {
        let raw = fs::read_to_string(&path)
            .map_err(|error| format!("reading {}: {error}", path.display()))?;
        let workflow = serde_yaml_ng::from_str::<Value>(&raw)
            .map_err(|error| format!("parsing {}: {error}", path.display()))?;
        let name = path
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| format!("workflow filename is not valid UTF-8: {}", path.display()))?;
        findings.extend(scan_workflow(name, &workflow));
    }
    findings.sort();
    findings.dedup();
    Ok(findings)
}

pub(crate) fn scan_workflow(workflow_name: &str, workflow: &Value) -> Vec<Finding> {
    let triggers = trigger_names(workflow);
    if !triggers
        .iter()
        .any(|trigger| trigger == "pull_request" || trigger == "pull_request_target")
    {
        return Vec::new();
    }

    let top_permissions = workflow.get("permissions");
    let Some(jobs) = workflow.get("jobs").and_then(Value::as_mapping) else {
        return Vec::new();
    };

    let mut findings = Vec::new();
    for (job_name, job_value) in jobs {
        let Some(job_name) = job_name.as_str() else {
            continue;
        };
        let Some(job) = job_value.as_mapping() else {
            continue;
        };
        if job_is_statically_non_pr(job) || !job_has_write_authority(job, top_permissions) {
            continue;
        }

        if let Some(uses) = job
            .get(Value::String("uses".into()))
            .and_then(Value::as_str)
        {
            scan_reusable_writer(workflow_name, job_name, uses, &mut findings);
            continue;
        }

        let Some(steps) = job
            .get(Value::String("steps".into()))
            .and_then(Value::as_sequence)
        else {
            continue;
        };
        scan_writer_steps(workflow_name, job_name, steps, &mut findings);
    }
    findings
}

fn scan_reusable_writer(
    workflow_name: &str,
    job_name: &str,
    uses: &str,
    findings: &mut Vec<Finding>,
) {
    if uses.starts_with("./") {
        findings.push(Finding {
            workflow: workflow_name.into(),
            job: job_name.into(),
            kind: FindingKind::LocalReusableWriter,
            detail: format!(
                "write-capable PR job delegates to candidate-controlled reusable workflow `{uses}`"
            ),
        });
    } else if !is_immutable_reusable_workflow(uses) {
        findings.push(Finding {
            workflow: workflow_name.into(),
            job: job_name.into(),
            kind: FindingKind::MutableReusableWriter,
            detail: format!(
                "write-capable PR job delegates to mutable or non-workflow reference `{uses}`"
            ),
        });
    }
}

fn scan_writer_steps(
    workflow_name: &str,
    job_name: &str,
    steps: &[Value],
    findings: &mut Vec<Finding>,
) {
    findings.push(Finding {
        workflow: workflow_name.into(),
        job: job_name.into(),
        kind: FindingKind::CandidateDefinedWriter,
        detail: "write-capable PR job is defined by candidate-controlled workflow steps".into(),
    });

    for (index, step) in steps.iter().enumerate() {
        let Some(step) = step.as_mapping() else {
            continue;
        };
        if let Some(run) = step.get(Value::String("run".into())).and_then(Value::as_str)
            && run_self_modifies_writer(run)
        {
            findings.push(Finding {
                workflow: workflow_name.into(),
                job: job_name.into(),
                kind: FindingKind::SelfModifyingWriter,
                detail: format!(
                    "step {} deletes or rewrites workflow/control-plane files before pushing",
                    index + 1
                ),
            });
        }
    }
}

fn trigger_names(workflow: &Value) -> Vec<String> {
    let Some(value) = workflow.get("on") else {
        return Vec::new();
    };
    match value {
        Value::String(value) => vec![value.clone()],
        Value::Sequence(values) => values
            .iter()
            .filter_map(Value::as_str)
            .map(ToOwned::to_owned)
            .collect(),
        Value::Mapping(values) => values
            .keys()
            .filter_map(Value::as_str)
            .map(ToOwned::to_owned)
            .collect(),
        _ => Vec::new(),
    }
}

fn job_has_write_authority(job: &Mapping, top_permissions: Option<&Value>) -> bool {
    job.get(Value::String("permissions".into()))
        .or(top_permissions)
        .is_some_and(permission_set_can_write)
}

fn permission_set_can_write(value: &Value) -> bool {
    if value.as_str() == Some("write-all") {
        return true;
    }
    value.as_mapping().is_some_and(|permissions| {
        WRITE_SCOPES.iter().any(|scope| {
            permissions
                .get(Value::String((*scope).into()))
                .and_then(Value::as_str)
                == Some("write")
        })
    })
}

fn job_is_statically_non_pr(job: &Mapping) -> bool {
    job.get(Value::String("if".into()))
        .and_then(Value::as_str)
        .is_some_and(condition_excludes_pull_request)
}

fn condition_excludes_pull_request(condition: &str) -> bool {
    let Some(condition) = strip_outer_parentheses(condition) else {
        return false;
    };
    let Some(branches) = split_top_level(condition, "||") else {
        return false;
    };
    !branches.is_empty() && branches.iter().all(|branch| branch_has_trusted_event_anchor(branch))
}

fn branch_has_trusted_event_anchor(branch: &str) -> bool {
    let Some(branch) = strip_outer_parentheses(branch) else {
        return false;
    };
    let Some(or_branches) = split_top_level(branch, "||") else {
        return false;
    };
    if or_branches.len() > 1 {
        return or_branches.iter().all(|item| branch_has_trusted_event_anchor(item));
    }
    let Some(terms) = split_top_level(branch, "&&") else {
        return false;
    };
    !terms.is_empty() && terms.iter().any(|term| term_has_trusted_event_anchor(term))
}

fn term_has_trusted_event_anchor(term: &str) -> bool {
    if term_is_trusted_event_equality(term) {
        return true;
    }
    let Some(stripped) = strip_outer_parentheses(term) else {
        return false;
    };
    stripped != term && condition_excludes_pull_request(stripped)
}

fn term_is_trusted_event_equality(term: &str) -> bool {
    let Some(term) = strip_outer_parentheses(term) else {
        return false;
    };
    let normalized = term.chars().filter(|ch| !ch.is_whitespace()).collect::<String>();
    matches!(
        normalized.as_str(),
        "github.event_name=='schedule'"
            | "github.event_name==\"schedule\""
            | "github.event_name=='workflow_dispatch'"
            | "github.event_name==\"workflow_dispatch\""
            | "github.event_name=='push'"
            | "github.event_name==\"push\""
    )
}

fn split_top_level<'a>(condition: &'a str, operator: &str) -> Option<Vec<&'a str>> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut index = 0;
    let mut paren_depth = 0usize;
    let mut quote = None;

    while index < condition.len() {
        let ch = condition[index..].chars().next()?;
        let width = ch.len_utf8();
        if let Some(active_quote) = quote {
            if ch == active_quote {
                quote = None;
            }
            index += width;
            continue;
        }
        match ch {
            '\'' | '"' => quote = Some(ch),
            '(' => paren_depth += 1,
            ')' => paren_depth = paren_depth.checked_sub(1)?,
            _ => {}
        }
        if quote.is_none() && paren_depth == 0 && condition[index..].starts_with(operator) {
            let part = condition[start..index].trim();
            if part.is_empty() {
                return None;
            }
            parts.push(part);
            index += operator.len();
            start = index;
            continue;
        }
        index += width;
    }
    if quote.is_some() || paren_depth != 0 {
        return None;
    }
    let part = condition[start..].trim();
    if part.is_empty() {
        return None;
    }
    parts.push(part);
    Some(parts)
}

fn strip_outer_parentheses(mut expression: &str) -> Option<&str> {
    loop {
        expression = expression.trim();
        if expression.is_empty() {
            return None;
        }
        if !expression.starts_with('(')
            || !expression.ends_with(')')
            || !outer_parentheses_wrap_expression(expression)?
        {
            return Some(expression);
        }
        expression = &expression[1..expression.len() - 1];
    }
}

fn outer_parentheses_wrap_expression(expression: &str) -> Option<bool> {
    let mut depth = 0usize;
    let mut quote = None;
    for (index, ch) in expression.char_indices() {
        if let Some(active_quote) = quote {
            if ch == active_quote {
                quote = None;
            }
            continue;
        }
        match ch {
            '\'' | '"' => quote = Some(ch),
            '(' => depth += 1,
            ')' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(index + ch.len_utf8() == expression.len());
                }
            }
            _ => {}
        }
    }
    if quote.is_some() {
        return None;
    }
    Some(false)
}

fn is_immutable_reusable_workflow(uses: &str) -> bool {
    let Some((path, reference)) = uses.rsplit_once('@') else {
        return false;
    };
    path.contains("/.github/workflows/")
        && reference.len() == 40
        && reference.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn run_self_modifies_writer(run: &str) -> bool {
    let normalized = normalize_shell(run);
    let workflow_path = ".github/workflows/";
    (normalized.contains("git rm")
        || normalized.contains("rm -f")
        || normalized.contains("rm --force")
        || normalized.contains("sed -i")
        || normalized.contains("python") && normalized.contains("unlink"))
        && normalized.contains(workflow_path)
}

fn normalize_shell(run: &str) -> String {
    run.to_ascii_lowercase()
        .replace('\n', " ")
        .replace('\r', " ")
        .replace('\t', " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

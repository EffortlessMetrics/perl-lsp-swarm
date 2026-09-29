//! Contract tests for #2266 public-API/semver PR-time auto-selection.
//!
//! The nightly-only, label-gated compatibility rails must also select
//! themselves on pull requests whose diff touches a published facade surface,
//! while out-of-scope diffs settle as green scoped-noops rather than skipped
//! contexts.

use std::fs;
use std::path::{Path, PathBuf};

#[test]
fn ratchet_reader_rejects_missing_unreadable_empty_and_duplicate_lists()
-> Result<(), Box<dyn std::error::Error>> {
    let workflow = fs::read_to_string(project_root()?.join(".github/workflows/ci.yml"))?;
    let function = workflow
        .split("          def ratchet_crates():")
        .nth(1)
        .and_then(|rest| rest.split("          def derive_prefixes():").next())
        .ok_or("missing selector ratchet reader")?;
    let source = format!(
        "def ratchet_crates():{}",
        function
            .lines()
            .map(|line| { format!("\n{}", line.strip_prefix("          ").unwrap_or(line)) })
            .collect::<String>()
    );
    let harness = r#"
import contextlib, io, pathlib, sys, tempfile
exec(sys.argv[1])
with tempfile.TemporaryDirectory() as directory:
    root = pathlib.Path(directory)
    cases = [
        ('missing', None, 'cannot read'),
        ('directory', None, 'cannot read'),
        ('empty', '# only comments\n\n', 'lists no crates'),
        ('duplicate', 'perl-parser\n perl-parser # repeated\n', 'duplicate'),
        ('valid', '# facades\n perl-parser # parser\n\nperl-lsp\n', None),
    ]
    for name, content, diagnostic in cases:
        path = root / name
        if name == 'directory':
            path.mkdir()
        elif content is not None:
            path.write_text(content, encoding='utf-8')
        RATCHET_LIST = str(path)
        errors = io.StringIO()
        try:
            with contextlib.redirect_stderr(errors):
                result = ratchet_crates()
        except SystemExit as error:
            if diagnostic is None or error.code != 1 or diagnostic not in errors.getvalue():
                raise RuntimeError((name, error.code, errors.getvalue()))
        else:
            if diagnostic is not None or result != ('perl-parser', 'perl-lsp'):
                raise RuntimeError((name, result, 'unexpected acceptance'))
"#;
    let python = if cfg!(windows) { "python" } else { "python3" };
    let output = std::process::Command::new(python).args(["-c", harness, &source]).output()?;
    if !output.status.success() {
        return Err(format!(
            "selector reader proof failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    Ok(())
}

fn project_root() -> Result<PathBuf, Box<dyn std::error::Error>> {
    Ok(PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("CARGO_MANIFEST_DIR has no parent")?
        .to_path_buf())
}

fn read(root: &Path, rel: &str) -> Result<String, Box<dyn std::error::Error>> {
    Ok(fs::read_to_string(root.join(rel))?)
}

fn mapping_section<'a>(document: &'a str, key: &str, indent: usize) -> Option<&'a str> {
    let prefix = " ".repeat(indent);
    let anchor = format!("\n{prefix}{key}:");
    let start = document.find(&anchor)?;
    let body_start = start + anchor.len();
    let rest = &document[body_start..];
    let mut end = rest.len();
    for (offset, line) in rest.split('\n').enumerate() {
        if offset == 0 {
            continue;
        }
        let trimmed = line.trim_start();
        let line_indent = line.len() - trimmed.len();
        if !trimmed.is_empty() && !trimmed.starts_with('#') && line_indent <= indent {
            end = rest[..].split('\n').take(offset).map(|l| l.len() + 1).sum();
            break;
        }
    }
    Some(&document[start..body_start + end])
}

fn job_section<'a>(workflow: &'a str, job_id: &str) -> Option<&'a str> {
    mapping_section(workflow, job_id, 2)
}

fn event_section<'a>(workflow: &'a str, event: &str) -> Option<&'a str> {
    let triggers = mapping_section(workflow, "on", 0)?;
    mapping_section(triggers, event, 2)
}

fn pull_request_label_gate(section: &str) -> Option<&str> {
    let anchor = "contains(github.event.pull_request.labels.*.name, '";
    let job_indent = section.lines().find_map(|line| {
        let trimmed = line.trim_start();
        (!trimmed.is_empty() && !trimmed.starts_with('#')).then_some(line.len() - trimmed.len())
    })?;
    let field_indent = job_indent.saturating_add(2);
    let mut lines = section.lines();
    let if_line = lines.find(|line| {
        let trimmed = line.trim_start();
        line.len() - trimmed.len() == field_indent && trimmed.starts_with("if:")
    })?;
    let mut is_header = true;

    for line in std::iter::once(if_line).chain(lines) {
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();
        if !is_header && !trimmed.is_empty() && indent <= field_indent {
            break;
        }
        is_header = false;
        if trimmed.starts_with('#') {
            continue;
        }

        let expression = trimmed.strip_prefix("if:").map_or(trimmed, str::trim);
        let active_expression = expression.split_once('#').map_or(expression, |(active, _)| active);
        if let Some((_, rest)) = active_expression.split_once(anchor) {
            return Some(rest.split_once('\'')?.0);
        }
    }

    None
}

fn active_if_expression(section: &str) -> Option<String> {
    let job_indent = section.lines().find_map(|line| {
        let trimmed = line.trim_start();
        (!trimmed.is_empty() && !trimmed.starts_with('#')).then_some(line.len() - trimmed.len())
    })?;
    let field_indent = job_indent.saturating_add(2);
    let mut in_if = false;
    let mut parts = Vec::new();

    for line in section.lines() {
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();
        if !in_if {
            if indent == field_indent && trimmed.starts_with("if:") {
                in_if = true;
            } else {
                continue;
            }
        } else if !trimmed.is_empty() && indent <= field_indent {
            break;
        }

        if trimmed.starts_with('#') {
            continue;
        }
        let expression = trimmed.strip_prefix("if:").map_or(trimmed, str::trim);
        let active = expression.split_once('#').map_or(expression, |(value, _)| value).trim();
        if !active.is_empty() && active != "|" && active != ">" && active != ">-" {
            parts.push(active);
        }
    }

    in_if.then(|| parts.join(" ").split_whitespace().collect::<Vec<_>>().join(" "))
}

fn public_api_job_runs(
    section: &str,
    event: &str,
    dispatch_public_api: bool,
    action: Option<&str>,
    event_label: Option<&str>,
    labels: &[&str],
) -> bool {
    let Some(label) = pull_request_label_gate(section) else {
        return false;
    };
    let Some(expression) = active_if_expression(section) else {
        return false;
    };
    let expected = format!(
        "(github.event_name == 'workflow_dispatch' && inputs.run_public_api) || github.event_name == 'schedule' || (github.event_name == 'pull_request' && contains(github.event.pull_request.labels.*.name, '{label}') && (github.event.action != 'labeled' || github.event.label.name == '{label}'))"
    );
    if expression != expected {
        return false;
    }

    match event {
        // GitHub evaluates an omitted or false boolean dispatch input as
        // false, so an unspecified selector must not consume the runner.
        "workflow_dispatch" => dispatch_public_api,
        "schedule" => true,
        "pull_request" if labels.contains(&label) => match action {
            Some("opened" | "synchronize" | "reopened" | "ready_for_review") => true,
            Some("labeled") => event_label == Some(label),
            _ => false,
        },
        _ => false,
    }
}

fn pull_request_activity_types(workflow: &str) -> Option<Vec<&str>> {
    let pull_request = event_section(workflow, "pull_request")?;
    pull_request.lines().find_map(|line| {
        let trimmed = line.trim();
        if line.len() - line.trim_start().len() != 4 {
            return None;
        }
        let values = trimmed.strip_prefix("types:").map(str::trim)?;
        let values = values.strip_prefix('[').and_then(|values| values.strip_suffix(']'))?;
        Some(
            values.split(',').map(str::trim).map(|value| value.trim_matches(['\'', '"'])).collect(),
        )
    })
}

const PUBLIC_API_POLICY_INPUTS: [&str; 7] = [
    "justfile",
    "docs/ci/labels.md",
    ".github/ci-config.yml",
    "scripts/gh/ensure-labels.sh",
    "scripts/tests/test-public-api-ratchet.sh",
    "scripts/tests/test-public-api-label.sh",
    "xtask/tests/public_api_pr_gating_workflow.rs",
];

fn workflow_policy_covers_public_api_inputs(workflow: &str) -> bool {
    let Some(pull_request) = event_section(workflow, "pull_request") else {
        return false;
    };
    let Some(push) = event_section(workflow, "push") else {
        return false;
    };
    PUBLIC_API_POLICY_INPUTS.iter().all(|path| {
        let watched = format!("      - '{path}'");
        pull_request.matches(&watched).count() == 1 && push.matches(&watched).count() == 1
    }) && workflow.matches("name: Install just for executable recipe proofs").count() == 1
        && workflow.matches("uses: taiki-e/install-action@").count() == 1
        && workflow.matches("name: Public API trigger label contract").count() == 1
        && workflow
            .matches("run: cargo test -p xtask --test public_api_pr_gating_workflow --locked")
            .count()
            == 1
        && workflow.matches("name: Public API label reconciliation stub proof").count() == 1
        && workflow.matches("run: bash scripts/tests/test-public-api-label.sh").count() == 1
        && workflow.matches("name: Public API ratchet executable proof").count() == 1
        && workflow.matches("run: bash scripts/tests/test-public-api-ratchet.sh").count() == 1
}

fn permissions_are_contents_read(section: &str) -> bool {
    let mut active =
        section.lines().map(str::trim).filter(|line| !line.is_empty() && !line.starts_with('#'));
    active.next() == Some("permissions:") && active.eq(["contents: read"])
}

fn job_has_effective_read_only_permissions(workflow: &str, job_id: &str) -> bool {
    let Some(workflow_permissions) = mapping_section(workflow, "permissions", 0) else {
        return false;
    };
    if !permissions_are_contents_read(workflow_permissions) {
        return false;
    }
    let Some(job) = job_section(workflow, job_id) else {
        return false;
    };
    mapping_section(job, "permissions", 4).is_none_or(permissions_are_contents_read)
}

fn job_binds_tested_candidate_sha(section: &str) -> bool {
    let subject = "${{ github.event_name == 'pull_request' && github.event.pull_request.head.sha || github.sha }}";
    section.contains(&format!("ref: {subject}"))
        && section.contains(&format!("TESTED_SHA: {subject}"))
        && section.contains("test \"$(git rev-parse 'HEAD^{commit}')\" = \"$TESTED_SHA\"")
}

fn job_timeout_minutes(section: &str) -> Option<u64> {
    section
        .lines()
        .find_map(|line| line.trim().strip_prefix("timeout-minutes:")?.trim().parse().ok())
}

/// Restricted GitHub-expression evaluator for the public-api-pr event matrix.
///
/// The subset is comparison (`==` / `!=`), `&&`, `||`, parentheses, quoted
/// strings, and dotted identifiers. It exists so the live workflow and
/// constructed wrong jobs are judged by the same oracle, including the
/// #16815 case where a job-level `push` gate still settles as a scoped-noop
/// because every expensive step stays `api_scope == true`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ApiRatchetOutcome {
    Skip,
    ScopedNoop,
    FullCheck,
}

#[derive(Clone, Copy, Debug)]
struct ApiRatchetScenario {
    event: &'static str,
    git_ref: &'static str,
    run_ci: bool,
    is_latest: bool,
    api_scope: bool,
}

fn bool_word(value: bool) -> &'static str {
    if value { "true" } else { "false" }
}

fn unwrap_github_expr(raw: &str) -> &str {
    let trimmed = raw.trim();
    trimmed
        .strip_prefix("${{")
        .and_then(|inner| inner.strip_suffix("}}"))
        .map(str::trim)
        .unwrap_or(trimmed)
}

fn scenario_vars(scenario: &ApiRatchetScenario) -> std::collections::BTreeMap<String, String> {
    let mut vars = std::collections::BTreeMap::new();
    vars.insert("github.event_name".into(), scenario.event.into());
    vars.insert("github.ref".into(), scenario.git_ref.into());
    vars.insert("needs.draft-pr-check.outputs.run_ci".into(), bool_word(scenario.run_ci).into());
    vars.insert(
        "needs.draft-pr-check.outputs.api_scope".into(),
        bool_word(scenario.api_scope).into(),
    );
    vars.insert(
        "needs.preflight-latest-check.outputs.is_latest".into(),
        bool_word(scenario.is_latest).into(),
    );
    vars
}

struct ExprCursor<'a> {
    src: &'a str,
    i: usize,
}

impl<'a> ExprCursor<'a> {
    fn new(src: &'a str) -> Self {
        Self { src, i: 0 }
    }

    fn skip_ws(&mut self) {
        while let Some(rest) = self.src.get(self.i..) {
            let Some(ch) = rest.chars().next() else {
                return;
            };
            if !ch.is_whitespace() {
                return;
            }
            self.i += ch.len_utf8();
        }
    }

    fn starts_with(&self, token: &str) -> bool {
        self.src.get(self.i..).is_some_and(|rest| rest.starts_with(token))
    }

    fn take(&mut self, token: &str) -> bool {
        self.skip_ws();
        if self.starts_with(token) {
            self.i += token.len();
            true
        } else {
            false
        }
    }

    fn ident(&mut self) -> Option<&'a str> {
        self.skip_ws();
        let rest = self.src.get(self.i..)?;
        let len = rest
            .chars()
            .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '_' || *ch == '.' || *ch == '-')
            .map(char::len_utf8)
            .sum::<usize>();
        if len == 0 {
            return None;
        }
        let ident = rest.get(..len)?;
        if !ident.chars().next()?.is_ascii_alphabetic() && ident.chars().next()? != '_' {
            return None;
        }
        self.i += len;
        Some(ident)
    }

    fn string(&mut self) -> Result<Option<String>, String> {
        self.skip_ws();
        if !self.take("'") {
            return Ok(None);
        }
        let rest = self.src.get(self.i..).ok_or("truncated string")?;
        let end = rest.find('\'').ok_or("unterminated string")?;
        let value = rest.get(..end).ok_or("unterminated string")?.to_string();
        self.i += end + 1;
        Ok(Some(value))
    }
}

fn eval_github_expr(
    expr: &str,
    vars: &std::collections::BTreeMap<String, String>,
) -> Result<bool, String> {
    let unwrapped = unwrap_github_expr(expr);
    if unwrapped.is_empty() {
        return Err("empty GitHub expression".into());
    }
    let mut cursor = ExprCursor::new(unwrapped);
    let value = parse_or(&mut cursor, vars)?;
    cursor.skip_ws();
    if cursor.i != cursor.src.len() {
        return Err(format!("trailing expression tokens: {}", &cursor.src[cursor.i..]));
    }
    Ok(value)
}

fn parse_or(
    cursor: &mut ExprCursor<'_>,
    vars: &std::collections::BTreeMap<String, String>,
) -> Result<bool, String> {
    let mut value = parse_and(cursor, vars)?;
    while cursor.take("||") {
        let rhs = parse_and(cursor, vars)?;
        value = value || rhs;
    }
    Ok(value)
}

fn parse_and(
    cursor: &mut ExprCursor<'_>,
    vars: &std::collections::BTreeMap<String, String>,
) -> Result<bool, String> {
    let mut value = parse_cmp(cursor, vars)?;
    while cursor.take("&&") {
        let rhs = parse_cmp(cursor, vars)?;
        value = value && rhs;
    }
    Ok(value)
}

fn parse_cmp(
    cursor: &mut ExprCursor<'_>,
    vars: &std::collections::BTreeMap<String, String>,
) -> Result<bool, String> {
    let left = parse_primary_value(cursor, vars)?;
    cursor.skip_ws();
    if cursor.take("==") {
        let right = parse_primary_value(cursor, vars)?;
        return Ok(left == right);
    }
    if cursor.take("!=") {
        let right = parse_primary_value(cursor, vars)?;
        return Ok(left != right);
    }
    Ok(left == "true")
}

fn parse_primary_value(
    cursor: &mut ExprCursor<'_>,
    vars: &std::collections::BTreeMap<String, String>,
) -> Result<String, String> {
    cursor.skip_ws();
    if cursor.take("(") {
        let value = parse_or(cursor, vars)?;
        if !cursor.take(")") {
            return Err("missing closing parenthesis".into());
        }
        return Ok(bool_word(value).into());
    }
    if let Some(string) = cursor.string()? {
        return Ok(string);
    }
    let ident = cursor
        .ident()
        .ok_or_else(|| format!("expected identifier or string at {}", &cursor.src[cursor.i..]))?;
    match ident {
        "true" | "false" => Ok(ident.into()),
        _ => Ok(vars.get(ident).cloned().unwrap_or_default()),
    }
}

fn job_env_bindings(job: &str) -> std::collections::BTreeMap<String, String> {
    let Some(section) = mapping_section(job, "env", 4) else {
        return std::collections::BTreeMap::new();
    };
    let mut bindings = std::collections::BTreeMap::new();
    for line in section.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with("env:") {
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        if indent <= 4 {
            break;
        }
        let Some((key, value)) = trimmed.split_once(':') else {
            continue;
        };
        bindings.insert(format!("env.{}", key.trim()), value.trim().to_string());
    }
    bindings
}

struct WorkflowStep {
    name: String,
    if_expr: Option<String>,
    run: String,
    continue_on_error: bool,
}

fn parse_job_steps(job: &str) -> Vec<WorkflowStep> {
    let Some(steps) = mapping_section(job, "steps", 4) else {
        return Vec::new();
    };
    let mut parsed = Vec::new();
    let mut current: Option<WorkflowStep> = None;
    let mut in_run = false;
    let mut run_parts = Vec::new();

    let flush_run = |step: &mut WorkflowStep, run_parts: &mut Vec<String>| {
        if !run_parts.is_empty() {
            step.run = run_parts.join("\n");
            run_parts.clear();
        }
    };

    for line in steps.lines() {
        let indent = line.len() - line.trim_start().len();
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if in_run {
                run_parts.push(String::new());
            }
            continue;
        }
        if trimmed.starts_with("steps:") {
            continue;
        }
        if indent <= 4 && !trimmed.starts_with('#') {
            break;
        }
        if indent == 6 && trimmed.starts_with("- ") {
            if let Some(mut step) = current.take() {
                flush_run(&mut step, &mut run_parts);
                parsed.push(step);
            }
            in_run = false;
            let name = trimmed.strip_prefix("- name:").map(str::trim).unwrap_or("").to_string();
            current = Some(WorkflowStep {
                name,
                if_expr: None,
                run: String::new(),
                continue_on_error: false,
            });
            continue;
        }
        let Some(step) = current.as_mut() else {
            continue;
        };
        if indent == 8 && trimmed.starts_with("if:") {
            in_run = false;
            let expr = trimmed.trim_start_matches("if:").trim();
            if expr == "|" || expr == ">" || expr == ">-" {
                step.if_expr = Some(String::new());
            } else {
                step.if_expr = Some(unwrap_github_expr(expr).to_string());
            }
            continue;
        }
        if indent > 8
            && let Some(existing) = step.if_expr.as_mut()
        {
            let active = trimmed.split_once('#').map_or(trimmed, |(value, _)| value).trim();
            if !active.is_empty()
                && (existing.is_empty()
                    || existing.ends_with("&&")
                    || existing.ends_with("||")
                    || active.starts_with("&&")
                    || active.starts_with("||")
                    || active.starts_with('(')
                    || existing.ends_with('('))
            {
                if !existing.is_empty() {
                    existing.push(' ');
                }
                existing.push_str(active);
                continue;
            }
        }
        if indent == 8 && trimmed.starts_with("run:") {
            in_run = true;
            run_parts.clear();
            let rest = trimmed.trim_start_matches("run:").trim();
            if rest != "|" && rest != ">" && rest != ">-" && !rest.is_empty() {
                run_parts.push(rest.to_string());
                in_run = false;
            }
            continue;
        }
        if indent == 8 && trimmed.starts_with("continue-on-error:") {
            in_run = false;
            step.continue_on_error = trimmed.contains("true");
            continue;
        }
        if in_run && indent >= 10 {
            run_parts.push(line[10.min(line.len())..].to_string());
            continue;
        }
        if indent == 8 {
            in_run = false;
        }
    }
    if let Some(mut step) = current {
        flush_run(&mut step, &mut run_parts);
        parsed.push(step);
    }
    parsed
}

fn evaluate_env_vars(
    job: &str,
    scenario: &ApiRatchetScenario,
) -> Result<std::collections::BTreeMap<String, String>, String> {
    let mut vars = scenario_vars(scenario);
    for (key, raw) in job_env_bindings(job) {
        let value = if unwrap_github_expr(&raw) == raw {
            raw
        } else {
            bool_word(eval_github_expr(&raw, &vars)?).to_string()
        };
        vars.insert(key, value);
    }
    Ok(vars)
}

fn step_applies(
    step: &WorkflowStep,
    vars: &std::collections::BTreeMap<String, String>,
) -> Result<bool, String> {
    match step.if_expr.as_deref() {
        None => Ok(true),
        Some(expr) => eval_github_expr(expr, vars),
    }
}

fn api_ratchet_outcome(
    job: &str,
    scenario: &ApiRatchetScenario,
) -> Result<ApiRatchetOutcome, String> {
    if let Some(job_if) = active_if_expression(job) {
        let vars = evaluate_env_vars(job, scenario)?;
        if !eval_github_expr(&job_if, &vars)? {
            return Ok(ApiRatchetOutcome::Skip);
        }
    }
    let vars = evaluate_env_vars(job, scenario)?;
    let steps = parse_job_steps(job);
    let mut full = false;
    let mut noop = false;
    for step in &steps {
        if !step_applies(step, &vars)? {
            continue;
        }
        if step.continue_on_error {
            return Err(format!("step {} continues on error", step.name));
        }
        if step.run.lines().any(|line| line.trim() == "just public-api-check") {
            full = true;
        }
        if step.name.contains("Scoped no-op") || step.run.contains("scoped-noop") {
            noop = true;
        }
    }
    match (full, noop) {
        (true, false) => Ok(ApiRatchetOutcome::FullCheck),
        (false, true) => Ok(ApiRatchetOutcome::ScopedNoop),
        (false, false) => Err("job ran but neither ratchet nor scoped-noop applied".into()),
        (true, true) => Err("job ran both the ratchet and the scoped-noop".into()),
    }
}

fn expect_outcome(
    job: &str,
    scenario: ApiRatchetScenario,
    expected: ApiRatchetOutcome,
) -> Result<(), String> {
    let actual = api_ratchet_outcome(job, &scenario)?;
    if actual == expected {
        Ok(())
    } else {
        Err(format!(
            "event={} ref={} run_ci={} latest={} api_scope={} expected {expected:?} got {actual:?}",
            scenario.event,
            scenario.git_ref,
            scenario.run_ci,
            scenario.is_latest,
            scenario.api_scope
        ))
    }
}

#[test]
fn pull_request_label_gate_ignores_commented_examples() {
    let section = r#"
  public-api-check:
    # Legacy example: contains(github.event.pull_request.labels.*.name, 'ci:public-api')
    if: |
      github.event_name == 'pull_request' &&
      contains(github.event.pull_request.labels.*.name, 'ci:nonexistent')
    steps: []
"#;

    assert_eq!(pull_request_label_gate(section), Some("ci:nonexistent"));
}

#[test]
fn pull_request_public_api_gate_is_default_deny_with_named_bypasses() {
    let section = r#"
  public-api-check:
    if: |
      github.event_name == 'workflow_dispatch' ||
      github.event_name == 'schedule' ||
      (github.event_name == 'pull_request' &&
       contains(github.event.pull_request.labels.*.name, 'ci:public-api') &&
       (github.event.action != 'labeled' ||
        github.event.label.name == 'ci:public-api'))
    steps: []
"#;

    assert_eq!(pull_request_label_gate(section), Some("ci:public-api"));
    assert!(section.contains("github.event_name == 'workflow_dispatch'"));
    assert!(section.contains("github.event_name == 'schedule'"));
    assert!(section.contains("github.event_name == 'pull_request' &&"));

    let labels = ["ci:public-api"];
    assert!(labels.contains(&"ci:public-api"));
    assert!(!labels.contains(&"ci:not-public-api"));
    assert!(!labels.contains(&"ci:public-api-extra"));

    let wrong_label = section.replace("ci:public-api", "ci:not-public-api");
    assert_ne!(pull_request_label_gate(&wrong_label), Some("ci:public-api"));

    let commented_only = section
        .replace(
            "contains(github.event.pull_request.labels.*.name, 'ci:public-api')",
            "# contains(github.event.pull_request.labels.*.name, 'ci:public-api')",
        )
        .replace("github.event_name == 'pull_request' &&", "github.event_name == 'pull_request'");
    assert_eq!(
        pull_request_label_gate(&commented_only),
        None,
        "a commented label example must not satisfy the active PR gate"
    );

    let extra_pr_label = section.replace(
        "    steps: []",
        "       || (github.event_name == 'pull_request' &&\n       contains(github.event.pull_request.labels.*.name, 'ci:other'))\n    steps: []",
    );
    assert!(
        !public_api_job_runs(
            &extra_pr_label,
            "pull_request",
            false,
            Some("opened"),
            None,
            &["ci:other"],
        ),
        "an extra active PR label disjunct must not pass the canonical gate"
    );

    let extra_bypass = section.replace(
        "github.event_name == 'schedule' ||",
        "github.event_name == 'schedule' || github.event_name == 'push' ||",
    );
    assert!(
        !public_api_job_runs(&extra_bypass, "push", false, None, None, &["ci:public-api"]),
        "an extra active event bypass must not pass the canonical gate"
    );
}

#[test]
fn ci_yml_runs_both_compatibility_rails_on_pull_requests() -> Result<(), Box<dyn std::error::Error>>
{
    let root = project_root()?;
    let workflow = read(&root, ".github/workflows/ci.yml")?;

    let public_api = job_section(&workflow, "public-api-pr")
        .ok_or("ci.yml must define the public-api-pr rail (#2266)")?;
    let semver = job_section(&workflow, "semver-pr")
        .ok_or("ci.yml must define the advisory semver-pr rail (#2266)")?;

    for section in [public_api, semver] {
        assert!(
            section.contains("needs.draft-pr-check.outputs.api_scope"),
            "both rails must consume the draft-pr-check api_scope selector"
        );
        assert!(
            section.contains("github.event_name == 'pull_request'"),
            "both rails keep the pull_request scoped-noop path; schedule/manual coverage stays on ci-nightly.yml"
        );
        assert!(
            section.contains("needs.preflight-latest-check.outputs.is_latest == 'true'"),
            "rails must respect superseded-SHA skipping like the other merge-gate jobs"
        );
        assert!(
            section.contains(".ci/public-api-baselines"),
            "baseline-set edits are part of the API surface",
        );
    }

    assert!(
        public_api.contains("just public-api-check"),
        "public-api-pr must reuse the canonical baseline ratchet recipe"
    );
    assert!(
        !public_api.contains("continue-on-error"),
        "public-api-pr is the hard ratchet and must propagate breakage"
    );
    assert!(
        semver.contains("continue-on-error: true"),
        "semver-pr stays advisory until #2266's confidence window closes"
    );
    assert!(
        semver.contains("cargo-semver-checks --version 0.47.0 --locked"),
        "semver-pr must pin the same cargo-semver-checks version as the nightly lane"
    );
    assert!(
        public_api.contains("github.event_name == 'push'"),
        "public-api-pr must admit push so landed SHAs are ratcheted (#16815)"
    );
    assert!(
        !semver.contains("github.event_name == 'push'"),
        "semver-pr stays PR-only; main-push ratchet is public-api-pr's claim"
    );

    Ok(())
}

#[test]
fn scope_selection_is_job_level_and_never_label_gated() -> Result<(), Box<dyn std::error::Error>> {
    let root = project_root()?;
    let workflow = read(&root, ".github/workflows/ci.yml")?;

    // House rule (#2914, REQUIRED_STYLE_SELF_FILTERED): a workflow-level
    // `paths:` filter leaves statuses unreported; selection stays inside jobs.
    assert!(
        !workflow.contains("\n    paths:"),
        "ci.yml must not gain a workflow-level paths filter"
    );
    assert!(
        !workflow.contains("labels.*.name"),
        "ci.yml excludes the labeled event on purpose; labels must not gate its jobs"
    );

    // #14607: the crates the rails check directly are not restated in the
    // workflow; the selector reads the same single list the recipes read.
    for anchor in [
        ".ci/public-api-baselines/",
        "\"ratchet-crates.txt\"",
        "def ratchet_crates():",
        "for crate in ratchet_crates():",
    ] {
        assert!(
            workflow.contains(anchor),
            "scope selector must derive from the ratchet list: {anchor}"
        );
    }
    for stale_literal in ["\"crates/perl-parser/\",", "\"crates/perllsp/\","] {
        assert!(
            !workflow.contains(stale_literal),
            "scope selector must not restate the ratchet crate list: {stale_literal}"
        );
    }
    assert!(
        workflow.contains("api_scope=true") && workflow.contains("api_scope=false"),
        "draft-pr-check must emit both scope verdicts explicitly"
    );
    assert!(
        workflow.contains("/pulls/{pr_number}/files"),
        "scope must derive from the pull request's own file set, not the two-dot base diff"
    );
    assert!(
        workflow.contains("using changed-file fallback"),
        "API probing must fall back to the local diff scan instead of failing open"
    );
    assert!(
        workflow.contains("file=sys.stderr"),
        "API probing diagnostics must stay off stdout so api_scope remains a scalar output"
    );
    // Re-export closure (P2 on #12850): crates whose public items flow into a
    // facade baseline via re-export must select the rails too, and the
    // committed baselines are the derivation authority rather than a second
    // hand-written path list.
    assert!(
        workflow.contains("derive_prefixes"),
        "scope selector must derive re-export closure from the committed baselines"
    );

    Ok(())
}

#[test]
fn registry_records_the_new_advisory_contexts() -> Result<(), Box<dyn std::error::Error>> {
    let root = project_root()?;
    let policy = read(&root, ".ci/policies/required-checks.toml")?;

    for (context, job) in [
        ("Public API Surface (facade PR)", "public-api-pr"),
        ("Semver Compatibility (facade PR)", "semver-pr"),
    ] {
        let row_start = policy
            .find(&format!("name = \"{context}\""))
            .ok_or_else(|| format!("required-checks.toml must inventory {context}"))?;
        let row = &policy[row_start..];
        let row_end = row.find("[[checks]]").unwrap_or(row.len());
        let row = &row[..row_end];
        assert!(row.contains(&format!("job = \"{job}\"")), "{context} binds {job}");
        assert!(
            row.contains("required = false"),
            "{context} starts advisory; ruleset promotion is a separate owner act"
        );
        // Contract-model classification: both rails are preconditioned jobs
        // (event + preflight + run_ci), so applicability is "conditional" per
        // validate_gate_enforcement_contract.py; scoped-noop green settlement
        // happens at the step level inside each job.
        assert!(
            row.contains("applicability = \"conditional\""),
            "{context} follows the prerequisite-selected applicability convention"
        );
        if job == "public-api-pr" {
            assert!(
                row.contains("events = [\"pull_request\", \"push\"]"),
                "{context} must be reachable on pull_request and push (#16815)"
            );
            assert!(
                row.contains("#16815"),
                "{context} policy reason must name the main-push ratchet claim"
            );
        } else {
            assert!(
                row.contains("events = [\"pull_request\"]") && !row.contains("\"push\""),
                "{context} stays pull_request-only"
            );
        }
    }

    Ok(())
}

const PR_ONLY_JOB_IF: &str = r#"
  public-api-pr:
    if: github.event_name == 'pull_request' && needs.draft-pr-check.outputs.run_ci == 'true' && needs.preflight-latest-check.outputs.is_latest == 'true'
    steps:
      - name: Scoped no-op summary
        if: needs.draft-pr-check.outputs.api_scope != 'true'
        run: echo scoped-noop
      - name: Check public API surface
        if: needs.draft-pr-check.outputs.api_scope == 'true'
        run: just public-api-check
"#;

const PUSH_JOB_IF_STILL_SCOPE_GATED: &str = r#"
  public-api-pr:
    if: (github.event_name == 'pull_request' || github.event_name == 'push') && needs.draft-pr-check.outputs.run_ci == 'true' && needs.preflight-latest-check.outputs.is_latest == 'true'
    steps:
      - name: Scoped no-op summary
        if: needs.draft-pr-check.outputs.api_scope != 'true'
        run: echo scoped-noop
      - name: Check public API surface
        if: needs.draft-pr-check.outputs.api_scope == 'true'
        run: just public-api-check
"#;

const PUSH_OR_SCOPE_STEP_GATED: &str = r#"
  public-api-pr:
    if: (github.event_name == 'pull_request' || (github.event_name == 'push' && (github.ref == 'refs/heads/main' || github.ref == 'refs/heads/master'))) && needs.draft-pr-check.outputs.run_ci == 'true' && needs.preflight-latest-check.outputs.is_latest == 'true'
    env:
      RUN_API_RATCHET: ${{ github.event_name == 'push' || needs.draft-pr-check.outputs.api_scope == 'true' }}
    steps:
      - name: Scoped no-op summary
        if: env.RUN_API_RATCHET != 'true'
        run: echo scoped-noop
      - name: Check public API surface
        if: env.RUN_API_RATCHET == 'true'
        run: just public-api-check
"#;

const PUSH_OR_SCOPE_WITH_EXPRESSION_IN_RUN: &str = r#"
  public-api-pr:
    if: (github.event_name == 'pull_request' || (github.event_name == 'push' && (github.ref == 'refs/heads/main' || github.ref == 'refs/heads/master'))) && needs.draft-pr-check.outputs.run_ci == 'true' && needs.preflight-latest-check.outputs.is_latest == 'true'
    env:
      RUN_API_RATCHET: ${{ github.event_name == 'push' || needs.draft-pr-check.outputs.api_scope == 'true' }}
    steps:
      - name: Scoped no-op summary
        if: env.RUN_API_RATCHET != 'true'
        run: echo "- event: `${{ github.event_name }}`"
      - name: Check public API surface
        if: env.RUN_API_RATCHET == 'true'
        run: just public-api-check
"#;

fn reject_github_expressions_in_run_source(job: &str) -> Result<(), String> {
    for step in parse_job_steps(job) {
        if let Some(line) = step.run.lines().find(|line| line.contains("${{")) {
            return Err(format!(
                "step {} embeds a GitHub expression in run source; pass it through env: {line}",
                step.name
            ));
        }
    }
    Ok(())
}

fn matrix_ready_job(job: &str) -> Result<(), String> {
    reject_github_expressions_in_run_source(job)?;
    let pr_full = ApiRatchetScenario {
        event: "pull_request",
        git_ref: "refs/pull/1/merge",
        run_ci: true,
        is_latest: true,
        api_scope: true,
    };
    let pr_noop = ApiRatchetScenario {
        event: "pull_request",
        git_ref: "refs/pull/1/merge",
        run_ci: true,
        is_latest: true,
        api_scope: false,
    };
    let pr_draft = ApiRatchetScenario {
        event: "pull_request",
        git_ref: "refs/pull/1/merge",
        run_ci: false,
        is_latest: true,
        api_scope: true,
    };
    let main_push_out_of_scope = ApiRatchetScenario {
        event: "push",
        git_ref: "refs/heads/main",
        run_ci: true,
        is_latest: true,
        api_scope: false,
    };
    let main_push_superseded = ApiRatchetScenario {
        event: "push",
        git_ref: "refs/heads/main",
        run_ci: true,
        is_latest: false,
        api_scope: false,
    };
    let master_push = ApiRatchetScenario {
        event: "push",
        git_ref: "refs/heads/master",
        run_ci: true,
        is_latest: true,
        api_scope: false,
    };
    let merge_group = ApiRatchetScenario {
        event: "merge_group",
        git_ref: "refs/heads/gh-readonly-queue/main/pr-1",
        run_ci: true,
        is_latest: true,
        api_scope: false,
    };
    let dispatch = ApiRatchetScenario {
        event: "workflow_dispatch",
        git_ref: "refs/heads/main",
        run_ci: true,
        is_latest: true,
        api_scope: false,
    };
    let schedule = ApiRatchetScenario {
        event: "schedule",
        git_ref: "refs/heads/main",
        run_ci: true,
        is_latest: true,
        api_scope: false,
    };
    expect_outcome(job, pr_full, ApiRatchetOutcome::FullCheck)?;
    expect_outcome(job, pr_noop, ApiRatchetOutcome::ScopedNoop)?;
    expect_outcome(job, pr_draft, ApiRatchetOutcome::Skip)?;
    expect_outcome(job, main_push_out_of_scope, ApiRatchetOutcome::FullCheck)?;
    expect_outcome(job, main_push_superseded, ApiRatchetOutcome::Skip)?;
    expect_outcome(job, master_push, ApiRatchetOutcome::FullCheck)?;
    expect_outcome(job, merge_group, ApiRatchetOutcome::Skip)?;
    expect_outcome(job, dispatch, ApiRatchetOutcome::Skip)?;
    expect_outcome(job, schedule, ApiRatchetOutcome::Skip)?;
    Ok(())
}

#[test]
fn public_api_pr_event_matrix_rejects_pr_only_and_push_scoped_noop() -> Result<(), String> {
    let pr_only = matrix_ready_job(PR_ONLY_JOB_IF);
    if pr_only.is_ok() {
        return Err("a PR-only job if must fail the main-push full-check cell".into());
    }
    let still_scoped = matrix_ready_job(PUSH_JOB_IF_STILL_SCOPE_GATED);
    if still_scoped.is_ok() {
        return Err("admitting push at job if while leaving steps on api_scope must fail".into());
    }
    let interpolated_run = matrix_ready_job(PUSH_OR_SCOPE_WITH_EXPRESSION_IN_RUN);
    if interpolated_run.is_ok() {
        return Err("interpolating github.event_name into run source must fail".into());
    }
    matrix_ready_job(PUSH_OR_SCOPE_STEP_GATED)?;
    Ok(())
}

#[test]
fn public_api_pr_live_workflow_covers_the_event_matrix() -> Result<(), Box<dyn std::error::Error>> {
    let workflow = read(&project_root()?, ".github/workflows/ci.yml")?;
    let public_api = job_section(&workflow, "public-api-pr")
        .ok_or("ci.yml must define the public-api-pr rail")?;
    matrix_ready_job(public_api)?;
    if !job_binds_tested_candidate_sha(public_api) {
        return Err(
            "public-api-pr must checkout and verify the exact candidate SHA on the full-check path"
                .into(),
        );
    }
    for step in parse_job_steps(public_api) {
        if let Some(expr) = step.if_expr.as_deref()
            && !expr.contains("env.RUN_API_RATCHET")
        {
            return Err(format!(
                "step {} must key off RUN_API_RATCHET, not a private copy of api_scope: {expr}",
                step.name
            )
            .into());
        }
    }
    Ok(())
}

#[test]
fn public_api_pr_run_sources_pass_github_context_through_env()
-> Result<(), Box<dyn std::error::Error>> {
    let workflow = read(&project_root()?, ".github/workflows/ci.yml")?;
    let public_api = job_section(&workflow, "public-api-pr")
        .ok_or("ci.yml must define the public-api-pr rail")?;
    let mut saw_scoped_noop = false;
    let mut saw_identity = false;
    for step in parse_job_steps(public_api) {
        if step.name.contains("Scoped no-op") {
            saw_scoped_noop = true;
            if !step.run.contains("$EVENT_NAME") || !step.run.contains("$API_SCOPE") {
                return Err(
                    "scoped-noop summary must print EVENT_NAME and API_SCOPE from env".into()
                );
            }
        }
        if step.name.contains("Verify tested candidate identity") {
            saw_identity = true;
            if !step.run.contains("$EVENT_NAME") || !step.run.contains("$TESTED_SHA") {
                return Err("identity summary must print EVENT_NAME and TESTED_SHA from env".into());
            }
        }
    }
    if !saw_scoped_noop {
        return Err("public-api-pr must keep a scoped-noop summary step".into());
    }
    if !saw_identity {
        return Err("public-api-pr must keep an exact-head identity step".into());
    }
    if !public_api.contains("EVENT_NAME: ${{ github.event_name }}") {
        return Err("EVENT_NAME must bind github.event_name as env data".into());
    }
    if !public_api.contains("API_SCOPE: ${{ needs.draft-pr-check.outputs.api_scope }}") {
        return Err("API_SCOPE must bind draft-pr-check.api_scope as env data".into());
    }
    Ok(())
}

#[test]
fn draft_guard_keeps_api_scope_as_a_pr_diff_selector() -> Result<(), Box<dyn std::error::Error>> {
    let workflow = read(&project_root()?, ".github/workflows/ci.yml")?;
    let guard =
        job_section(&workflow, "draft-pr-check").ok_or("ci.yml must define draft-pr-check")?;
    let non_pr = guard
        .split(r#"if [ "${{ github.event_name }}" != "pull_request" ]; then"#)
        .nth(1)
        .and_then(|rest| rest.split("elif").next())
        .ok_or("draft-pr-check must keep an explicit non-PR branch")?;
    assert!(
        non_pr.contains("api_scope=false"),
        "non-PR events must not pretend a push has an API-scoped PR diff"
    );
    assert!(
        !non_pr.contains("api_scope=true"),
        "push must not overload api_scope; public-api-pr owns the main-push full check"
    );
    Ok(())
}

#[test]
fn nightly_label_gates_and_baseline_ratchet_survive_unchanged()
-> Result<(), Box<dyn std::error::Error>> {
    let root = project_root()?;
    let nightly = read(&root, ".github/workflows/ci-nightly.yml")?;
    assert!(
        nightly.contains("'ci:semver'") && nightly.contains("'ci:public-api'"),
        "manual label widening remains available on the nightly lane"
    );

    let justfile = read(&root, "justfile")?;
    assert!(
        justfile.contains("public-api-check:"),
        "canonical public API baseline recipe must stay registered in the justfile"
    );

    Ok(())
}

#[test]
fn nightly_public_api_label_is_governed_and_provisioned() -> Result<(), Box<dyn std::error::Error>>
{
    let root = project_root()?;
    let nightly = read(&root, ".github/workflows/ci-nightly.yml")?;
    let activities = pull_request_activity_types(&nightly)
        .ok_or("ci-nightly.yml must declare pull_request activity types")?;
    let expected_activities = ["opened", "synchronize", "reopened", "ready_for_review", "labeled"];
    if activities != expected_activities {
        return Err(format!(
            "public API label authorization must use the bounded activity set; got {activities:?}"
        )
        .into());
    }
    if !nightly.contains(
        "group: ci-nightly-${{ github.event.pull_request.number || github.ref }}",
    ) || !nightly.contains(
        "cancel-in-progress: ${{ github.event_name == 'pull_request' && github.event.action == 'synchronize' }}",
    ) {
        return Err(
            "nightly concurrency must bind one PR and cancel stale-head work only on synchronize"
                .into(),
        );
    }
    if !job_has_effective_read_only_permissions(&nightly, "public-api-check") {
        return Err("public-api-check must have effective contents-read-only permissions".into());
    }
    let write_override = nightly.replacen(
        "  public-api-check:\n",
        "  public-api-check:\n    permissions:\n      issues: write\n",
        1,
    );
    if job_has_effective_read_only_permissions(&write_override, "public-api-check") {
        return Err("a job-level write-permission override must fail the authority oracle".into());
    }
    let workflow_write =
        nightly.replacen("permissions:\n  contents: read", "permissions:\n  contents: write", 1);
    if job_has_effective_read_only_permissions(&workflow_write, "public-api-check") {
        return Err("a workflow-level write permission must fail the authority oracle".into());
    }
    let without_labeled = nightly.replace(", labeled", "");
    let without_labeled_activities = pull_request_activity_types(&without_labeled)
        .ok_or("labeled-removal fixture must retain a pull_request activity list")?;
    if without_labeled_activities.contains(&"labeled") {
        return Err("removing labeled activity must fail the dispatch contract".into());
    }
    for false_positive in ["unlabeled", "labeled-extra"] {
        let fixture = format!(
            "\nname: fixture\n\non:\n  pull_request:\n    types: [opened, {false_positive}]\n  schedule:\n"
        );
        let fixture_activities = pull_request_activity_types(&fixture)
            .ok_or("false-positive fixture must retain a pull_request activity list")?;
        if fixture_activities.contains(&"labeled") {
            return Err(format!(
                "{false_positive} must not satisfy the exact labeled activity contract"
            )
            .into());
        }
    }
    let missing_on_anchor = "\nname: fixture\n\n  pull_request:\n    types: [opened, synchronize, reopened, ready_for_review, labeled]\n  schedule:\n";
    if pull_request_activity_types(missing_on_anchor).is_some() {
        return Err("a pull_request-like mapping outside an on: block must not be accepted".into());
    }
    let misplaced_activities = "\nname: fixture\n\non:\n  pull_request:\n    branches: [main]\n  workflow_dispatch:\n    types: [opened, synchronize, reopened, ready_for_review, labeled]\n  schedule:\n";
    if pull_request_activity_types(misplaced_activities).is_some() {
        return Err("activity types from another event must not satisfy pull_request".into());
    }
    let public_api = job_section(&nightly, "public-api-check")
        .ok_or("ci-nightly.yml must define the public-api-check job")?;
    let label = pull_request_label_gate(public_api)
        .ok_or("public-api-check must expose its pull-request label gate")?;
    let timeout = job_timeout_minutes(public_api)
        .ok_or("public-api-check must declare a numeric timeout-minutes")?;
    if !public_api.contains("uses: actions/checkout@")
        || !public_api.contains("persist-credentials: false")
        || !job_binds_tested_candidate_sha(public_api)
    {
        return Err(
            "public-api-check must checkout and verify the exact candidate head without credentials"
                .into()
        );
    }
    let merge_sha_checkout =
        public_api.replace("github.event.pull_request.head.sha || github.sha", "github.sha");
    if job_binds_tested_candidate_sha(&merge_sha_checkout) {
        return Err("merge-SHA checkout must not satisfy exact candidate-head proof".into());
    }

    assert_eq!(label, "ci:public-api", "the public API lane owns one stable trigger label");
    assert!(
        public_api
            .contains("(github.event_name == 'workflow_dispatch' && inputs.run_public_api) ||")
    );
    assert!(public_api.contains("github.event_name == 'schedule' ||"));
    assert!(!public_api.contains("github.event_name == 'pull_request' ||"));
    assert!(!public_api.contains("github.event_name == 'push'"));
    assert!(public_api_job_runs(public_api, "workflow_dispatch", true, None, None, &[]));
    assert!(!public_api_job_runs(public_api, "workflow_dispatch", false, None, None, &[]));
    assert!(public_api_job_runs(public_api, "schedule", false, None, None, &[]));
    for action in ["opened", "synchronize", "reopened", "ready_for_review"] {
        assert!(public_api_job_runs(
            public_api,
            "pull_request",
            false,
            Some(action),
            None,
            &["ci:public-api"],
        ));
    }
    assert!(public_api_job_runs(
        public_api,
        "pull_request",
        false,
        Some("labeled"),
        Some("ci:public-api"),
        &["ci:public-api"],
    ));
    assert!(!public_api_job_runs(
        public_api,
        "pull_request",
        false,
        Some("labeled"),
        Some("ci:unrelated"),
        &["ci:public-api", "ci:unrelated"],
    ));
    assert!(!public_api_job_runs(
        public_api,
        "pull_request",
        false,
        Some("unlabeled"),
        Some("ci:unrelated"),
        &["ci:public-api"],
    ));
    assert!(!public_api_job_runs(
        public_api,
        "pull_request",
        false,
        Some("labeled"),
        None,
        &["ci:public-api"],
    ));
    assert!(!public_api_job_runs(public_api, "pull_request", false, Some("opened"), None, &[],));
    assert!(!public_api_job_runs(
        public_api,
        "pull_request",
        false,
        Some("opened"),
        None,
        &["ci:not-public-api"],
    ));
    assert!(!public_api_job_runs(
        public_api,
        "pull_request",
        false,
        Some("opened"),
        None,
        &["ci:public-api-extra"],
    ));
    assert!(!public_api_job_runs(public_api, "push", false, None, None, &["ci:public-api"],));

    let docs = read(&root, "docs/ci/labels.md")?;
    let governed_row = docs
        .lines()
        .find(|line| line.contains(&format!("`{label}`")))
        .ok_or_else(|| format!("{label} must be present in docs/ci/labels.md"))?;
    if timeout != 20
        || !governed_row.contains(&format!("{timeout}-minute"))
        || !governed_row.contains("fail-closed")
    {
        return Err(
            "the governed row must match the job's 20-minute cap and fail-closed intent".into()
        );
    }

    let config = read(&root, ".github/ci-config.yml")?;
    let metadata = config
        .split_once(&format!("  {label}:\n"))
        .and_then(|(_, rest)| rest.split_once("\n  # "))
        .map(|(value, _)| value)
        .ok_or_else(|| format!("{label} must have canonical metadata in .github/ci-config.yml"))?;
    let color = metadata
        .lines()
        .find_map(|line| line.trim().strip_prefix("color: '")?.strip_suffix('\''))
        .ok_or_else(|| format!("{label} must have a canonical color"))?;
    let description = metadata
        .lines()
        .find_map(|line| line.trim().strip_prefix("description: '")?.strip_suffix('\''))
        .ok_or_else(|| format!("{label} must have a canonical description"))?;

    let provisioning = read(&root, "scripts/gh/ensure-labels.sh")?;
    assert!(
        provisioning.contains("public_api_metadata()")
            && provisioning.contains(
                "CI_CONFIG_PATH=\"${CI_CONFIG_PATH:-${REPO_ROOT}/.github/ci-config.yml}\"",
            ),
        "provisioning must read the canonical ci-config metadata"
    );
    assert!(
        provisioning
            .lines()
            .any(|line| line.trim() == format!("ensure_reconciled \"{label}\" \"${{PUBLIC_API_COLOR}}\" \"${{PUBLIC_API_DESCRIPTION}}\"")),
        "provisioning must pass catalog-derived metadata to reconciliation"
    );
    assert!(
        !provisioning.contains(&format!("\"{color}\" \"{description}\"")),
        "provisioning must not duplicate the catalog metadata literals"
    );
    assert!(
        provisioning.contains("gh label edit \"$name\""),
        "existing public API labels must be reconciled, not merely skipped"
    );

    let policy = read(&root, ".github/workflows/workflow-policy.yml")?;
    if !workflow_policy_covers_public_api_inputs(&policy) {
        return Err(
            "workflow policy must cover and execute every public API authority input".into()
        );
    }
    for input in PUBLIC_API_POLICY_INPUTS {
        let without_input = policy.replace(&format!("      - '{input}'\n"), "");
        if workflow_policy_covers_public_api_inputs(&without_input) {
            return Err(
                format!("removing {input} coverage must fail the recurrence contract").into()
            );
        }
    }
    let watched = "      - 'justfile'\n";
    let push_section =
        event_section(&policy, "push").ok_or("Workflow Policy must retain a push mapping")?;
    let push_without_watched = push_section.replacen(watched, "", 1);
    let unbalanced = policy.replacen(push_section, &push_without_watched, 1).replacen(
        watched,
        &format!("{watched}{watched}"),
        1,
    );
    if workflow_policy_covers_public_api_inputs(&unbalanced) {
        return Err("duplicate pull_request coverage must not replace missing push coverage".into());
    }
    for executable_proof in [
        "      - name: Install just for executable recipe proofs\n",
        "        uses: taiki-e/install-action@",
        "      - name: Public API trigger label contract\n",
        "        run: cargo test -p xtask --test public_api_pr_gating_workflow --locked\n",
        "      - name: Public API label reconciliation stub proof\n",
        "        run: bash scripts/tests/test-public-api-label.sh\n",
        "      - name: Public API ratchet executable proof\n",
        "        run: bash scripts/tests/test-public-api-ratchet.sh\n",
    ] {
        let without_proof = policy.replace(executable_proof, "");
        if workflow_policy_covers_public_api_inputs(&without_proof) {
            return Err(
                "removing an executable proof step must fail the recurrence contract".into()
            );
        }
    }

    Ok(())
}

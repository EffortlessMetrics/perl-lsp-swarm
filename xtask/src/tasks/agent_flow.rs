//! Static provider-native skill topology checks for #5201.
//!
//! This module validates repository structure only. It does not inspect
//! GitHub, lifecycle labels, agent identity, or live issue/PR state.

use color_eyre::eyre::{Result, bail};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::utils::project_root;

const PROVIDER_SKILL_ROOTS: &[(&str, &str)] =
    &[("codex", ".agents/skills"), ("claude", ".claude/skills")];

const FORBIDDEN_SHARED_REVIEW_AUTHORITY: &str = "PR_REVIEW_STANDARD.md";
const METASYNTACTIC_PLACEHOLDERS: &[&str] = &["skill", "skill_name", "skill-name"];
const ROUTE_BEARING_LABELS: &[&str] = &[
    "entry flow",
    "entry route",
    "next flow",
    "next route",
    "return flow",
    "return route",
    "fallback flow",
    "fallback route",
];

const REVIEW_SKILL_MARKERS: &[(&str, &[&str])] = &[
    (
        "orchestrate-work",
        &[
            "## PR review orchestration",
            "review-tests",
            "review-candidate",
            "join evidence",
            "review-pr",
        ],
    ),
    (
        "finish-pr",
        &["orchestrate-work", "final-challenge", "review-pr", "verify-live-ci", "REVIEW_REQUIRED"],
    ),
    (
        "review-pr",
        &[
            "## Required review procedure",
            "production reachability",
            "proof discrimination",
            "REVIEW_CURRENT",
            "CHANGES_REQUIRED",
        ],
    ),
    (
        "verify-live-ci",
        &["REVIEW_REQUIRED", "REVIEW_CURRENT", "INTEGRATION_READY", "PR_IN_FLIGHT", "review-pr"],
    ),
    (
        "deliver-goal",
        &[
            "## Bounded related-PR review orchestration",
            "Substantive review result",
            "Integration posture",
            "review-pr",
        ],
    ),
    ("deliver-pr", &["finish-pr", "substantive review"]),
];

const CLAUDE_ROOT_MARKERS: &[&str] = &[
    ".claude/skills/",
    "## Claude-native PR review",
    "orchestrate-work",
    "review-pr",
    "REVIEW_CURRENT",
    "INTEGRATION_READY",
];

const CODEX_ROOT_MARKERS: &[&str] = &[
    ".agents/skills/",
    "## Codex-native PR review",
    "$orchestrate-work",
    "$review-pr",
    "REVIEW_CURRENT",
    "INTEGRATION_READY",
];

#[derive(Debug, Clone)]
pub struct CheckConfig {
    pub skill: Option<String>,
    pub format: String,
}

#[derive(Debug, Clone)]
pub struct ScenarioConfig {
    pub format: String,
}

#[derive(Debug, Serialize)]
struct CheckReport {
    schema: &'static str,
    result: &'static str,
    providers: BTreeMap<String, ProviderReport>,
    scenarios: ScenarioReport,
    errors: Vec<String>,
    advisories: Vec<String>,
}

#[derive(Debug, Serialize)]
struct ProviderReport {
    root: String,
    skill_count: usize,
    checked_skills: Vec<String>,
    route_count: usize,
    route_observations: Vec<RouteObservationReport>,
    metadata_chars: usize,
}

#[derive(Debug, Serialize)]
struct RouteObservationReport {
    source: String,
    path: String,
    line: usize,
    column_start: usize,
    column_end: usize,
    target: String,
    syntax: RouteSyntax,
    executable_edge: bool,
}

#[derive(Debug, Serialize)]
struct ScenarioReport {
    fixture_count: usize,
    checked_providers: Vec<String>,
    errors: Vec<String>,
}

#[derive(Debug, Serialize)]
struct ScenarioOutput {
    schema: &'static str,
    result: &'static str,
    fixture_count: usize,
    checked_providers: Vec<String>,
    errors: Vec<String>,
}

#[derive(Debug)]
struct Skill {
    name: String,
    path: PathBuf,
    text: String,
    route_targets: Vec<String>,
    route_observations: Vec<RouteObservation>,
    metadata_chars: usize,
}

#[derive(Debug, Clone, Copy, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
enum RouteSyntax {
    ExplicitSigil,
    ArrowTarget,
    ListTarget,
    BareTarget,
    ImperativeInvocation,
    LabeledTarget,
    ProseMention,
    CodeIdentifier,
    InlineCode,
    Placeholder,
}

impl RouteSyntax {
    const fn is_edge(self) -> bool {
        matches!(
            self,
            Self::ExplicitSigil
                | Self::ArrowTarget
                | Self::ListTarget
                | Self::BareTarget
                | Self::ImperativeInvocation
                | Self::LabeledTarget
        )
    }
}

#[derive(Debug, Clone, Eq, Ord, PartialEq, PartialOrd)]
struct RouteObservation {
    target: String,
    line: usize,
    column_start: usize,
    column_end: usize,
    syntax: RouteSyntax,
}

#[derive(Debug)]
struct ScenarioFixture {
    name: &'static str,
    required_skills: &'static [&'static str],
    required_edges: &'static [(&'static str, &'static str)],
}

const SCENARIO_FIXTURES: &[ScenarioFixture] = &[
    ScenarioFixture {
        name: "fresh_issue",
        required_skills: &["deliver-pr", "prepare-issue"],
        required_edges: &[("deliver-pr", "prepare-issue"), ("deliver-pr", "prepare-proof")],
    },
    ScenarioFixture {
        name: "existing_issue_no_proof",
        required_skills: &["deliver-pr", "prepare-proof"],
        required_edges: &[("deliver-pr", "prepare-proof")],
    },
    ScenarioFixture {
        name: "reviewed_proof_no_candidate",
        required_skills: &["deliver-pr", "build-candidate"],
        required_edges: &[("deliver-pr", "build-candidate")],
    },
    ScenarioFixture {
        name: "existing_coherent_pr",
        required_skills: &["deliver-pr", "finish-pr"],
        required_edges: &[("deliver-pr", "finish-pr")],
    },
    ScenarioFixture {
        name: "docs_no_proof",
        required_skills: &["deliver-pr", "prepare-proof"],
        required_edges: &[("deliver-pr", "prepare-proof")],
    },
    ScenarioFixture {
        name: "proof_authority_drift",
        required_skills: &["prepare-proof", "prepare-issue"],
        required_edges: &[("prepare-proof", "prepare-issue")],
    },
    ScenarioFixture {
        name: "candidate_scope_drift",
        required_skills: &["finish-pr", "build-candidate", "prepare-issue"],
        required_edges: &[("finish-pr", "prepare-issue")],
    },
    ScenarioFixture {
        name: "clean_formal_review",
        required_skills: &["finish-pr", "review-pr"],
        required_edges: &[("finish-pr", "review-pr")],
    },
    ScenarioFixture {
        name: "substantive_review_orchestration",
        required_skills: &[
            "finish-pr",
            "orchestrate-work",
            "review-pr",
            "review-tests",
            "review-candidate",
        ],
        required_edges: &[
            ("finish-pr", "orchestrate-work"),
            ("finish-pr", "review-pr"),
            ("orchestrate-work", "review-tests"),
            ("orchestrate-work", "review-candidate"),
            ("orchestrate-work", "review-pr"),
        ],
    },
    ScenarioFixture {
        name: "review_enters_live_integration",
        required_skills: &["review-pr", "verify-live-ci"],
        required_edges: &[("review-pr", "verify-live-ci")],
    },
    ScenarioFixture {
        name: "live_ci_requires_review",
        required_skills: &["verify-live-ci", "review-pr"],
        required_edges: &[("verify-live-ci", "review-pr")],
    },
    ScenarioFixture {
        name: "related_pr_native_review",
        required_skills: &[
            "deliver-goal",
            "deliver-pr",
            "orchestrate-work",
            "review-pr",
            "verify-live-ci",
        ],
        required_edges: &[
            ("deliver-goal", "deliver-pr"),
            ("deliver-goal", "orchestrate-work"),
            ("deliver-goal", "review-pr"),
            ("deliver-goal", "verify-live-ci"),
        ],
    },
    ScenarioFixture {
        name: "repair_changes_head",
        required_skills: &["finish-pr", "address-review-comments", "final-challenge", "review-pr"],
        required_edges: &[
            ("finish-pr", "address-review-comments"),
            ("finish-pr", "final-challenge"),
            ("finish-pr", "review-pr"),
        ],
    },
    ScenarioFixture {
        name: "post_publication_final_challenge",
        required_skills: &["finish-pr", "final-challenge"],
        required_edges: &[("finish-pr", "final-challenge")],
    },
    ScenarioFixture {
        name: "stale_candidate_claim_review",
        required_skills: &["finish-pr", "final-challenge", "review-pr"],
        required_edges: &[("finish-pr", "final-challenge"), ("finish-pr", "review-pr")],
    },
    ScenarioFixture {
        name: "merged_unreconciled_pr",
        required_skills: &["deliver-pr", "merge-reconcile"],
        required_edges: &[("deliver-pr", "merge-reconcile")],
    },
    ScenarioFixture {
        name: "multi_pr_in_flight",
        required_skills: &["deliver-goal", "deliver-pr"],
        required_edges: &[("deliver-goal", "deliver-pr")],
    },
    ScenarioFixture {
        name: "same_candidate_writer_collision",
        required_skills: &["orchestrate-work", "finish-pr", "build-candidate"],
        required_edges: &[("finish-pr", "build-candidate")],
    },
    ScenarioFixture {
        name: "actual_conflict",
        required_skills: &["finish-pr", "build-candidate"],
        required_edges: &[("finish-pr", "build-candidate")],
    },
    ScenarioFixture {
        name: "unchanged_main_movement",
        required_skills: &["deliver-pr", "finish-pr", "verify-live-ci"],
        required_edges: &[("finish-pr", "verify-live-ci")],
    },
];

pub fn run(config: CheckConfig) -> Result<()> {
    let root = project_root()?;
    let report = check_repository(&root, config.skill.as_deref())?;

    match config.format.as_str() {
        "human" => print_human(&report),
        "json" => println!("{}", serde_json::to_string_pretty(&report)?),
        other => bail!("unsupported agent-flow output format '{other}'; use human or json"),
    }

    if report.result == "FAIL" {
        bail!("agent-flow check failed")
    }
    Ok(())
}

pub fn run_scenarios(config: ScenarioConfig) -> Result<()> {
    let root = project_root()?;
    let report = check_repository(&root, None)?;
    let output = scenario_output(report);

    match config.format.as_str() {
        "human" => {
            println!("{}", output.result);
            println!(
                "scenarios: {} fixtures across {} providers",
                output.fixture_count,
                output.checked_providers.len()
            );
            for error in &output.errors {
                println!("ERROR: {error}");
            }
        }
        "json" => println!("{}", serde_json::to_string_pretty(&output)?),
        other => bail!("unsupported agent-flow output format '{other}'; use human or json"),
    }

    if output.result == "FAIL" {
        bail!("agent-flow scenarios failed")
    }
    Ok(())
}

fn scenario_output(report: CheckReport) -> ScenarioOutput {
    let scenarios = report.scenarios;
    let result = if scenarios.errors.is_empty() { "PASS" } else { "FAIL" };
    ScenarioOutput {
        schema: "agent-flow-scenarios.v1",
        result,
        fixture_count: scenarios.fixture_count,
        checked_providers: scenarios.checked_providers,
        errors: scenarios.errors,
    }
}

fn check_repository(root: &Path, selected_skill: Option<&str>) -> Result<CheckReport> {
    let mut providers = BTreeMap::new();
    let mut provider_skills = BTreeMap::new();
    let mut errors = Vec::new();
    let mut advisories = Vec::new();
    let mut selected_matches = 0;

    for (provider, relative_root) in PROVIDER_SKILL_ROOTS {
        let skill_root = root.join(relative_root);
        let skills = collect_skills(&skill_root, &mut errors)?;
        check_provider_operational_contract(root, provider, &skills, &mut errors)?;

        let known_names = skills.iter().map(|skill| skill.name.clone()).collect::<BTreeSet<_>>();
        let route_map = skills
            .iter()
            .map(|skill| {
                (skill.name.clone(), skill.route_targets.iter().cloned().collect::<BTreeSet<_>>())
            })
            .collect::<BTreeMap<_, _>>();
        provider_skills.insert((*provider).to_string(), (known_names.clone(), route_map));
        let mut route_count = 0;
        let mut route_reports = Vec::new();
        let mut metadata_chars = 0;
        let mut checked_skills = Vec::new();

        for skill in skills
            .iter()
            .filter(|skill| selected_skill.is_none_or(|selected| selected == skill.name))
        {
            selected_matches += 1;
            metadata_chars += skill.metadata_chars;
            checked_skills.push(skill.name.clone());
            let relative_path = skill
                .path
                .strip_prefix(root)
                .unwrap_or(&skill.path)
                .to_string_lossy()
                .replace('\\', "/");

            for observation in &skill.route_observations {
                let syntax = resolve_route_syntax(observation, &known_names);
                if syntax.is_edge() {
                    route_count += 1;
                    if !known_names.contains(&observation.target) {
                        errors.push(missing_route_target_message(
                            &skill.path,
                            &skill.name,
                            observation,
                        ));
                    }
                }
                route_reports.push(RouteObservationReport {
                    source: skill.name.clone(),
                    path: relative_path.clone(),
                    line: observation.line,
                    column_start: observation.column_start,
                    column_end: observation.column_end,
                    target: observation.target.clone(),
                    syntax,
                    executable_edge: syntax.is_edge(),
                });
            }
        }

        if selected_skill.is_none() && metadata_chars > 5_000 {
            advisories.push(format!(
                "{provider}: checked skill-name metadata is {metadata_chars} characters; keep root discovery metadata comfortably bounded"
            ));
        }

        providers.insert(
            (*provider).to_string(),
            ProviderReport {
                root: (*relative_root).to_string(),
                skill_count: checked_skills.len(),
                checked_skills,
                route_count,
                route_observations: route_reports,
                metadata_chars,
            },
        );
    }

    if let Some(selected_skill) = selected_skill
        && selected_matches == 0
    {
        errors.push(format!(
            "skill selector '{selected_skill}' did not match any provider-local skill"
        ));
    }

    let scenario_errors = check_scenarios(&provider_skills);
    errors.extend(scenario_errors.iter().cloned());
    let scenarios = ScenarioReport {
        fixture_count: SCENARIO_FIXTURES.len(),
        checked_providers: provider_skills.keys().cloned().collect(),
        errors: scenario_errors,
    };

    let result = if errors.is_empty() { "PASS" } else { "FAIL" };
    Ok(CheckReport {
        schema: "agent-flow-check.v2",
        result,
        providers,
        scenarios,
        errors,
        advisories,
    })
}

fn check_provider_operational_contract(
    root: &Path,
    provider: &str,
    skills: &[Skill],
    errors: &mut Vec<String>,
) -> Result<()> {
    let (root_path, root_markers) = match provider {
        "claude" => ("CLAUDE.md", CLAUDE_ROOT_MARKERS),
        "codex" => ("AGENTS.md", CODEX_ROOT_MARKERS),
        other => {
            errors.push(format!("unknown provider '{other}' in provider skill roots"));
            return Ok(());
        }
    };

    let provider_root = root.join(root_path);
    let root_text = fs::read_to_string(&provider_root)?;
    for marker in missing_markers(&root_text, root_markers) {
        errors.push(format!(
            "{}: provider-native review contract is missing marker '{}'",
            provider_root.display(),
            marker
        ));
    }
    if root_text.contains(FORBIDDEN_SHARED_REVIEW_AUTHORITY) {
        errors.push(format!(
            "{}: provider root still delegates review authority to '{}'",
            provider_root.display(),
            FORBIDDEN_SHARED_REVIEW_AUTHORITY
        ));
    }

    for (skill_name, markers) in REVIEW_SKILL_MARKERS {
        let Some(skill) = skills.iter().find(|skill| skill.name == *skill_name) else {
            errors.push(format!(
                "{provider}: provider-native review contract requires missing skill '{skill_name}'"
            ));
            continue;
        };

        for marker in missing_markers(&skill.text, markers) {
            errors.push(format!(
                "{}: provider-native review contract is missing marker '{}'",
                skill.path.display(),
                marker
            ));
        }
        if skill.text.contains(FORBIDDEN_SHARED_REVIEW_AUTHORITY) {
            errors.push(format!(
                "{}: skill still delegates review authority to '{}'",
                skill.path.display(),
                FORBIDDEN_SHARED_REVIEW_AUTHORITY
            ));
        }
    }

    Ok(())
}

fn missing_markers<'a>(text: &str, markers: &'a [&'a str]) -> Vec<&'a str> {
    markers.iter().copied().filter(|marker| !text.contains(marker)).collect()
}

fn check_scenarios(
    provider_skills: &BTreeMap<String, (BTreeSet<String>, BTreeMap<String, BTreeSet<String>>)>,
) -> Vec<String> {
    let mut errors = Vec::new();
    for (provider, (skill_names, route_map)) in provider_skills {
        for fixture in SCENARIO_FIXTURES {
            for required_skill in fixture.required_skills {
                if !skill_names.contains(*required_skill) {
                    errors.push(format!(
                        "{provider}: scenario '{}' requires missing skill '{}'",
                        fixture.name, required_skill
                    ));
                }
            }
            for (source, target) in fixture.required_edges {
                if !route_map.get(*source).is_some_and(|routes| routes.contains(*target)) {
                    errors.push(format!(
                        "{provider}: scenario '{}' has no route from '{}' to '{}'",
                        fixture.name, source, target
                    ));
                }
            }
        }
    }
    errors
}

fn collect_skills(skill_root: &Path, errors: &mut Vec<String>) -> Result<Vec<Skill>> {
    let mut skills = Vec::new();
    let mut entries = fs::read_dir(skill_root)?.collect::<std::result::Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let directory_name = entry.file_name().to_string_lossy().into_owned();
        let skill_path = entry.path().join("SKILL.md");
        if !skill_path.is_file() {
            errors.push(format!("{}: missing SKILL.md", entry.path().display()));
            continue;
        }
        let text = fs::read_to_string(&skill_path)?;
        let metadata_chars = frontmatter_metadata_chars(&text);
        match frontmatter_value(&text, "name") {
            Some(name) if name == directory_name => {}
            Some(name) => errors.push(format!(
                "{}: metadata name '{}' does not match directory '{}'",
                skill_path.display(),
                name,
                directory_name
            )),
            None => errors.push(format!("{}: missing frontmatter name", skill_path.display())),
        }
        let route_observations = route_observations(&text);
        let route_targets = edge_targets(&route_observations);
        skills.push(Skill {
            name: directory_name,
            path: skill_path,
            text,
            route_targets,
            route_observations,
            metadata_chars,
        });
    }
    skills.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(skills)
}

fn frontmatter_value(text: &str, key: &str) -> Option<String> {
    let mut lines = text.lines();
    if lines.next()?.trim() != "---" {
        return None;
    }
    let mut value = None;
    let mut closed = false;
    for line in lines {
        if line.trim() == "---" {
            closed = true;
            break;
        }
        if let Some((candidate_key, candidate_value)) = line.split_once(':')
            && candidate_key.trim() == key
        {
            value = Some(candidate_value.trim().trim_matches('"').to_string());
        }
    }
    closed.then_some(value).flatten()
}

fn frontmatter_metadata_chars(text: &str) -> usize {
    let mut lines = text.lines();
    if lines.next().map(str::trim) != Some("---") {
        return 0;
    }

    let mut metadata_chars = 0;
    for line in lines {
        if line.trim() == "---" {
            return metadata_chars;
        }
        metadata_chars += line.len() + 1;
    }
    0
}

fn route_targets(text: &str) -> Vec<String> {
    edge_targets(&route_observations(text))
}

fn edge_targets(observations: &[RouteObservation]) -> Vec<String> {
    observations
        .iter()
        .filter(|observation| observation.syntax.is_edge())
        .map(|observation| observation.target.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn resolve_route_syntax(
    observation: &RouteObservation,
    known_names: &BTreeSet<String>,
) -> RouteSyntax {
    match observation.syntax {
        RouteSyntax::InlineCode if known_names.contains(observation.target.as_str()) => {
            RouteSyntax::ProseMention
        }
        RouteSyntax::InlineCode => RouteSyntax::CodeIdentifier,
        syntax => syntax,
    }
}

fn route_observations(text: &str) -> Vec<RouteObservation> {
    let mut in_route_section = false;
    let mut observations = BTreeSet::new();
    for (line_index, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with("## ") {
            in_route_section = is_route_heading(trimmed);
            continue;
        }
        observations.extend(route_line_observations(line, line_index + 1, in_route_section));
    }
    observations.into_iter().collect()
}

fn is_route_heading(heading: &str) -> bool {
    let normalized = heading.trim_start_matches('#').trim().to_ascii_lowercase();
    normalized.contains("route")
        || normalized.contains("routing")
        || normalized.contains("valid exit")
        || normalized == "flow"
        || normalized == "loop"
        || normalized.contains("orchestration")
        || normalized.contains("outcome")
        || normalized == "procedure"
}

fn missing_route_target_message(
    path: &Path,
    source: &str,
    observation: &RouteObservation,
) -> String {
    format!(
        "{}:{}:{}: route from '{}' points to missing provider-local skill '{}' via {:?} (if this is prose or a code identifier, remove its route syntax; route references should use an explicit route form)",
        path.display(),
        observation.line,
        observation.column_start + 1,
        source,
        observation.target,
        observation.syntax
    )
}

fn route_tokens(line: &str, in_route_section: bool) -> Vec<String> {
    edge_targets(&route_line_observations(line, 1, in_route_section))
}

fn route_line_observations(
    line: &str,
    line_number: usize,
    in_route_section: bool,
) -> Vec<RouteObservation> {
    if !in_route_section {
        return Vec::new();
    }

    let mut observations = BTreeSet::new();
    scan_backticked_tokens(line, line_number, &mut observations);
    observations.into_iter().collect()
}

fn scan_backticked_tokens(
    line: &str,
    line_number: usize,
    observations: &mut BTreeSet<RouteObservation>,
) {
    let mut cursor = 0;
    // Text since the previous token's closing backtick. Arrow detection reads
    // only this segment: a `->` belongs to the token it points at, not to every
    // later token on the same line.
    let mut segment_start = 0;
    while let Some(relative_start) = line[cursor..].find('`') {
        let opening = cursor + relative_start;
        let content_start = opening + 1;
        let Some(relative_end) = line[content_start..].find('`') else {
            break;
        };
        let closing = content_start + relative_end;
        let token = &line[content_start..closing];

        if let Some(target) = token.strip_prefix('$') {
            if is_route_name(target) {
                observations.insert(RouteObservation {
                    target: target.to_string(),
                    line: line_number,
                    column_start: content_start + 1,
                    column_end: closing,
                    syntax: if METASYNTACTIC_PLACEHOLDERS.contains(&target) {
                        RouteSyntax::Placeholder
                    } else {
                        RouteSyntax::ExplicitSigil
                    },
                });
            }
            cursor = closing + 1;
            segment_start = cursor;
            continue;
        }

        if is_route_name(token) {
            let code_span = &line[opening..=closing];
            let segment = &line[segment_start..opening];
            let syntax = if METASYNTACTIC_PLACEHOLDERS.contains(&token) {
                RouteSyntax::Placeholder
            } else if segment.contains("->") || segment.contains('→') {
                RouteSyntax::ArrowTarget
            } else {
                classify_arrowless_code_span(line, code_span, opening)
            };
            observations.insert(RouteObservation {
                target: token.to_string(),
                line: line_number,
                column_start: content_start,
                column_end: closing,
                syntax,
            });
        }
        cursor = closing + 1;
        segment_start = cursor;
    }
}

fn classify_arrowless_code_span(line: &str, code_span: &str, opening: usize) -> RouteSyntax {
    let trimmed = line.trim();
    let candidate = strip_markdown_list_marker(trimmed);
    if candidate == code_span {
        return RouteSyntax::BareTarget;
    }
    if let Some(rest) = candidate.strip_prefix(code_span) {
        let rest = rest.trim_start();
        if rest.starts_with(':') || rest.starts_with('—') {
            return RouteSyntax::ListTarget;
        }
    }
    // A labeled route is defined by its label, not by its bullet: the bare and
    // list forms above carry no list requirement either, and
    // `has_route_label_prefix` already restricts the match to
    // `ROUTE_BEARING_LABELS`.
    //
    // Use `&line[..opening]` (the exact prefix before this token's opening
    // backtick) rather than the full line. When the same code span appears
    // more than once on a line, passing the full line to a `str::find`-based
    // helper always resolves the first occurrence's prefix context, silently
    // misclassifying every later occurrence. Scoping to `&line[..opening]`
    // makes each token's classification independent of its position in the
    // line — consistent with the arrow branch, which already uses `segment`
    // (the text since the previous closing backtick) for the same reason.
    let prefix_before_opening = &line[..opening];
    if has_route_label_prefix(prefix_before_opening) {
        return RouteSyntax::LabeledTarget;
    }
    if is_markdown_list_item(trimmed) && has_imperative_route_prefix(prefix_before_opening) {
        return RouteSyntax::ImperativeInvocation;
    }
    RouteSyntax::InlineCode
}

/// Check whether `prefix_before_opening` — everything in the source line
/// before the current token's opening backtick — ends with a route-bearing
/// label followed by a colon.
///
/// Accepts the raw `&line[..opening]` slice. The list marker (if any) and
/// trailing whitespace are stripped internally, so the caller does not need
/// to pre-process it.
///
/// Uses suffix matching (not equality) so that earlier content on the same
/// line — such as a preceding prose code span — does not prevent a later
/// labeled route from being recognised. The route labels in
/// `ROUTE_BEARING_LABELS` are specific enough that suffix matching is safe.
fn has_route_label_prefix(prefix_before_opening: &str) -> bool {
    // Strip the list marker and surrounding whitespace from the prefix.
    let candidate = strip_markdown_list_marker(prefix_before_opening.trim());
    let prefix = candidate.trim();
    let label = if let Some(without_colon) = prefix.strip_suffix(':') {
        strip_strong_emphasis(without_colon).trim()
    } else {
        let without_emphasis = strip_strong_emphasis(prefix);
        let Some(without_colon) = without_emphasis.strip_suffix(':') else {
            return false;
        };
        without_colon.trim()
    };
    let normalized = label.to_ascii_lowercase();
    ROUTE_BEARING_LABELS.iter().any(|&route_label| {
        // Suffix match: the route label appears at the end of a longer prefix.
        // Guard with a non-alphanumeric boundary so "xentry flow" doesn't
        // spuriously match "entry flow". An exact match yields an empty
        // `before`, which also passes the guard.
        if let Some(before) = normalized.strip_suffix(route_label) {
            !before.ends_with(|c: char| c.is_alphanumeric())
        } else {
            false
        }
    })
}

fn strip_strong_emphasis(text: &str) -> &str {
    text.strip_prefix("**").and_then(|inner| inner.strip_suffix("**")).unwrap_or(text)
}

/// Check whether `prefix_before_opening` — everything in the source line
/// before the current token's opening backtick — is exactly an imperative
/// route verb (after stripping the list marker and surrounding whitespace).
///
/// Accepts the raw `&line[..opening]` slice. The list marker and whitespace
/// are stripped internally.
fn has_imperative_route_prefix(prefix_before_opening: &str) -> bool {
    let candidate = strip_markdown_list_marker(prefix_before_opening.trim());
    let prefix = candidate.trim().to_ascii_lowercase();
    matches!(
        prefix.as_str(),
        "invoke"
            | "route to"
            | "continue with"
            | "proceed through"
            | "enter through"
            | "call"
            | "hand off to"
            | "return to"
    )
}

fn is_markdown_list_item(line: &str) -> bool {
    if ["- ", "* ", "+ "].iter().any(|marker| line.starts_with(marker)) {
        return true;
    }
    line.split_once(". ").is_some_and(|(prefix, _)| {
        !prefix.is_empty() && prefix.bytes().all(|byte| byte.is_ascii_digit())
    })
}

fn strip_markdown_list_marker(line: &str) -> &str {
    for marker in ["- ", "* ", "+ "] {
        if let Some(rest) = line.strip_prefix(marker) {
            return rest.trim_start();
        }
    }
    if let Some((prefix, rest)) = line.split_once(". ")
        && !prefix.is_empty()
        && prefix.bytes().all(|byte| byte.is_ascii_digit())
    {
        return rest.trim_start();
    }
    line
}

fn is_route_name(token: &str) -> bool {
    token.bytes().next().is_some_and(|byte| byte.is_ascii_lowercase())
        && token.bytes().all(is_route_name_byte)
}

const fn is_route_name_byte(byte: u8) -> bool {
    byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-' || byte == b'_'
}

fn print_human(report: &CheckReport) {
    println!("{}", report.result);
    for (provider, provider_report) in &report.providers {
        println!(
            "{provider}: {} skills, {} route references, {} metadata characters",
            provider_report.skill_count,
            provider_report.route_count,
            provider_report.metadata_chars
        );
    }
    println!(
        "scenarios: {} fixtures across {} providers",
        report.scenarios.fixture_count,
        report.scenarios.checked_providers.len()
    );
    for error in &report.errors {
        println!("ERROR: {error}");
    }
    for advisory in &report.advisories {
        println!("ADVISORY: {advisory}");
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};
    use std::path::Path;

    use super::{
        RouteObservation, RouteSyntax, SCENARIO_FIXTURES, check_scenarios, edge_targets,
        frontmatter_metadata_chars, frontmatter_value, missing_markers,
        missing_route_target_message, resolve_route_syntax, route_line_observations,
        route_observations, route_targets, route_tokens,
    };

    #[test]
    fn parses_frontmatter_name() {
        let text = "---\nname: prepare-issue\ndescription: test\n---\n";
        assert_eq!(frontmatter_value(text, "name").as_deref(), Some("prepare-issue"));
    }

    #[test]
    fn extracts_provider_route_targets_from_route_bearing_sections() {
        let text = "# Skill\n\n`not-a-route`\n\n## Procedure\n- `PLAN_READY` -> `$prepare-proof`\n- clean -> `deliver-pr`\n\n## Notes\n- `not-a-route`\n";
        assert_eq!(route_targets(text), vec!["deliver-pr", "prepare-proof"]);
    }

    #[test]
    fn ignores_provider_skill_mentions_outside_route_sections() {
        let text = "# Skill\n\nUse `$prepare-proof` for context.\n";
        assert!(route_targets(text).is_empty());
    }

    #[test]
    fn treats_routing_headings_as_route_bearing_sections() {
        let text = "## Entry routing\n- `READY` -> `$prepare-proof`\n";
        assert_eq!(route_targets(text), vec!["prepare-proof"]);
    }

    #[test]
    fn treats_flow_loop_and_orchestration_headings_as_route_bearing_sections() {
        let text = "## Loop\n- `$deliver-pr`\n\n## PR review orchestration\n- `review-pr`\n";
        assert_eq!(route_targets(text), vec!["deliver-pr", "review-pr"]);
    }

    #[test]
    fn preserves_arrow_list_bare_and_imperative_routes() {
        let text = "## Routes\n- ready -> `deliver-pr`\n- `review-pr`: submit review\n`verify-live-ci`\n2. Invoke `build-from-proof` where implementation is missing.\n";
        assert_eq!(
            route_targets(text),
            vec!["build-from-proof", "deliver-pr", "review-pr", "verify-live-ci"]
        );
        let syntaxes = route_observations(text)
            .into_iter()
            .filter(|observation| observation.syntax.is_edge())
            .map(|observation| observation.syntax)
            .collect::<BTreeSet<_>>();
        assert_eq!(
            syntaxes,
            BTreeSet::from([
                RouteSyntax::ArrowTarget,
                RouteSyntax::ListTarget,
                RouteSyntax::BareTarget,
                RouteSyntax::ImperativeInvocation,
            ])
        );
    }

    #[test]
    fn preserves_labeled_route_fields_as_edges() {
        let text = "## Routes\n- Entry flow: `deliver-pr`\n- **Next route:** `finish-pr`\n";
        let observations = route_observations(text);
        assert_eq!(observations.len(), 2, "both labeled route fields are observed");
        assert_eq!(edge_targets(&observations), vec!["deliver-pr", "finish-pr"]);
        assert!(
            observations.iter().all(|observation| observation.syntax == RouteSyntax::LabeledTarget),
            "both labeled route fields classify as LabeledTarget"
        );
    }

    #[test]
    fn labeled_route_typos_remain_load_bearing() {
        let observations = route_line_observations("- Entry flow: `delver-pr`", 1, true);
        assert_eq!(observations.len(), 1, "labeled route yields exactly one observation");
        assert_eq!(edge_targets(&observations), vec!["delver-pr"]);
        assert_eq!(
            resolve_route_syntax(&observations[0], &BTreeSet::new()),
            RouteSyntax::LabeledTarget,
            "a misspelled labeled route stays an edge so it fails closed"
        );
    }

    /// Regression test for #10201.
    ///
    /// When the same code span text appears more than once on a line and the
    /// first occurrence is prose, the later occurrence that carries a
    /// route-bearing label must still be classified as `LabeledTarget` and
    /// must therefore appear in the route edge set.
    ///
    /// The historical bug: `classify_arrowless_code_span` passed the whole
    /// line to the prefix helpers, which used `str::find` to locate the
    /// code span. `find` always returned the first occurrence's position, so
    /// every later occurrence was classified using the first occurrence's
    /// context — silently turning a real executable edge into `InlineCode`.
    #[test]
    fn prose_occurrence_before_labeled_route_does_not_shadow_it() {
        // "See `delver-pr`" is prose. "Entry flow: `delver-pr`" is a labeled
        // route reference to a non-existent skill — it must be an edge.
        let line = "- See `delver-pr` \u{2014} Entry flow: `delver-pr`";
        let observations = route_line_observations(line, 1, true);
        assert_eq!(observations.len(), 2, "both occurrences of `delver-pr` are observed");

        // Prose occurrence: prefix is "- See " — no route-bearing label.
        let prose = observations
            .iter()
            .find(|obs| {
                &line[obs.column_start..obs.column_end] == "delver-pr" && obs.column_start < 10
            })
            .expect("first (prose) occurrence is present");
        assert_eq!(
            prose.syntax,
            RouteSyntax::InlineCode,
            "prose occurrence has no route label and stays InlineCode"
        );

        // Labeled occurrence: prefix ends with "Entry flow:" — must be an edge.
        let labeled = observations
            .iter()
            .find(|obs| {
                &line[obs.column_start..obs.column_end] == "delver-pr" && obs.column_start > 10
            })
            .expect("second (labeled) occurrence is present");
        assert_eq!(
            resolve_route_syntax(labeled, &std::collections::BTreeSet::new()),
            RouteSyntax::LabeledTarget,
            "labeled occurrence is a route edge even when a prose occurrence precedes it"
        );
        assert_eq!(
            edge_targets(&observations),
            vec!["delver-pr"],
            "only the labeled occurrence contributes to the edge set"
        );
    }

    /// Verify that the word-boundary guard in `has_route_label_prefix` rejects
    /// a label whose text appears at the end of a longer word (e.g. "xentry
    /// flow" must not match "entry flow").
    #[test]
    fn word_boundary_guard_prevents_suffix_false_positive() {
        // "xentery flow:" — ends with "entry flow" in bytes but is not the
        // label "entry flow" because there is no word boundary before it.
        // (Using a realistic-looking scenario-prefix to avoid a trivially
        // empty-line test.)
        let observations = route_line_observations("- Reentry flow: `deliver-pr`", 1, true);
        assert_eq!(observations.len(), 1, "the single token is observed");
        assert!(
            edge_targets(&observations).is_empty(),
            "'Reentry flow' ends with 'entry flow' but is not a route-bearing label: no edge"
        );
        assert_eq!(
            observations[0].syntax,
            RouteSyntax::InlineCode,
            "a non-route label does not create a LabeledTarget"
        );
    }

    #[test]
    fn trailing_inline_code_after_an_arrow_route_is_not_an_edge() {
        let observations = route_line_observations(
            "- ready -> `deliver-pr` (compare `candidate-sha` first)",
            1,
            true,
        );
        assert_eq!(observations.len(), 2, "both backticked tokens are observed");
        assert_eq!(
            edge_targets(&observations),
            vec!["deliver-pr"],
            "only the arrow target is an edge; a later token must not inherit the arrow"
        );

        let trailing =
            observations.iter().find(|observation| observation.target == "candidate-sha");
        assert!(trailing.is_some(), "the trailing token is observed");
        let Some(trailing) = trailing else { return };
        assert_eq!(
            trailing.syntax,
            RouteSyntax::InlineCode,
            "a token after the arrow target stays inline code"
        );
    }

    #[test]
    fn every_target_in_an_arrow_chain_remains_an_edge() {
        // Scoping the arrow prefix must not break a chain: each *target* still
        // has an arrow in its own segment. The leading token is the chain's
        // source rather than a target, and carries no arrow before it, so it
        // stays inline code — unchanged by the scoping fix.
        let observations =
            route_line_observations("- `deliver-pr` -> `build-candidate` -> `finish-pr`", 1, true);
        assert_eq!(observations.len(), 3, "every chained token is observed");
        assert_eq!(
            edge_targets(&observations),
            vec!["build-candidate", "finish-pr"],
            "both arrow targets stay edges after the first link"
        );

        let head = observations.iter().find(|observation| observation.target == "deliver-pr");
        assert!(head.is_some(), "the chain source is observed");
        let Some(head) = head else { return };
        assert_eq!(
            head.syntax,
            RouteSyntax::InlineCode,
            "the chain source is not itself an arrow target"
        );
    }

    #[test]
    fn labeled_route_outside_a_list_is_still_an_edge() {
        let observations = route_line_observations("Entry flow: `deliver-pr`", 1, true);
        assert_eq!(observations.len(), 1, "the labeled route yields one observation");
        assert_eq!(
            edge_targets(&observations),
            vec!["deliver-pr"],
            "a labeled route written as a paragraph is still executable"
        );
        assert_eq!(
            observations[0].syntax,
            RouteSyntax::LabeledTarget,
            "the list marker is presentation, not route semantics"
        );
    }

    #[test]
    fn unrelated_labeled_code_remains_non_executable() {
        let observations = route_line_observations("- Cache key: `deliver-pr`", 1, true);
        assert_eq!(observations.len(), 1, "the labeled code yields one observation");
        assert!(
            edge_targets(&observations).is_empty(),
            "a label outside ROUTE_BEARING_LABELS creates no edge"
        );
        assert_eq!(
            observations[0].syntax,
            RouteSyntax::InlineCode,
            "an unrelated label leaves the token as inline code"
        );
    }

    #[test]
    fn existing_skill_name_in_prose_is_a_prose_mention() {
        let text =
            "## Procedure\nTake issue #123 through `deliver-pr` after the candidate is coherent.\n";
        assert!(route_targets(text).is_empty(), "prose creates no route target");
        let observations = route_observations(text);
        assert_eq!(observations.len(), 1, "the prose token is observed once");
        assert_eq!(observations[0].target, "deliver-pr");
        assert_eq!(observations[0].syntax, RouteSyntax::InlineCode);
        assert_eq!(
            resolve_route_syntax(&observations[0], &BTreeSet::from(["deliver-pr".to_string()])),
            RouteSyntax::ProseMention,
            "a real skill named in prose resolves to a prose mention"
        );
    }

    #[test]
    fn unknown_inline_code_is_a_code_identifier() {
        let text = "## Procedure\nCompare `candidate_sha` before selecting a route.\n";
        let observations = route_observations(text);
        assert_eq!(observations.len(), 1, "the inline code token is observed once");
        assert_eq!(observations[0].target, "candidate_sha");
        assert_eq!(observations[0].syntax, RouteSyntax::InlineCode);
        assert_eq!(
            resolve_route_syntax(&observations[0], &BTreeSet::from(["deliver-pr".to_string()])),
            RouteSyntax::CodeIdentifier,
            "a token naming no provider-local skill resolves to a code identifier"
        );
        assert!(
            !resolve_route_syntax(&observations[0], &BTreeSet::new()).is_edge(),
            "a code identifier is never an executable edge"
        );
    }

    #[test]
    fn backtick_formatting_cannot_mutate_prose_into_a_route() {
        let plain = "## Procedure\nTake issue #123 through deliver-pr after review.\n";
        let formatted = "## Procedure\nTake issue #123 through `deliver-pr` after review.\n";
        assert_eq!(
            route_targets(plain),
            route_targets(formatted),
            "backticks alone must not change the route set"
        );
        assert!(route_targets(formatted).is_empty(), "neither form creates a route");
    }

    #[test]
    fn explicit_sigil_remains_a_route_inside_prose() {
        assert_eq!(route_tokens("Invoke `$deliver-pr` after review.", true), vec!["deliver-pr"]);
    }

    #[test]
    fn unquoted_shell_or_prose_variables_are_not_routes() {
        assert!(route_tokens("Export $path before continuing.", true).is_empty());
        assert!(route_tokens("Read $status and report it.", true).is_empty());
    }

    #[test]
    fn near_miss_explicit_route_remains_load_bearing() {
        let observations = route_line_observations("- ready -> `delver-pr`", 1, true);
        assert_eq!(edge_targets(&observations), vec!["delver-pr"]);
        assert_eq!(
            resolve_route_syntax(&observations[0], &BTreeSet::new()),
            RouteSyntax::ArrowTarget
        );
    }

    #[test]
    fn route_observations_retain_source_line_and_indentation_range() {
        let line = "    - ready -> `deliver-pr`";
        let observations = route_line_observations(line, 9, true);
        assert_eq!(observations.len(), 1);
        let observation = &observations[0];
        assert_eq!(observation.line, 9);
        assert_eq!(observation.syntax, RouteSyntax::ArrowTarget);
        assert_eq!(&line[observation.column_start..observation.column_end], "deliver-pr");
    }

    #[test]
    fn ignores_uppercase_status_tokens() {
        assert_eq!(
            route_tokens("- `REVIEW_CURRENT` -> `$verify-live-ci`", true),
            vec!["verify-live-ci"]
        );
    }

    #[test]
    fn ignores_metasyntactic_placeholders_in_backticks() {
        assert_eq!(
            route_tokens("which `$skill` to consume. Do not ask agents.", true),
            Vec::<String>::new()
        );
        assert_eq!(route_tokens("- `$deliver-pr` then `$skill`", true), vec!["deliver-pr"]);
        assert!(
            route_observations("## Routes\n- `$skill`\n")
                .iter()
                .any(|observation| observation.syntax == RouteSyntax::Placeholder)
        );
    }

    #[test]
    fn missing_route_diagnostic_explains_source_syntax() {
        let observation = RouteObservation {
            target: "clear".into(),
            line: 12,
            column_start: 7,
            column_end: 12,
            syntax: RouteSyntax::ArrowTarget,
        };
        let message = missing_route_target_message(
            Path::new(".agents/skills/review-tests/SKILL.md"),
            "review-tests",
            &observation,
        );
        assert!(message.contains("SKILL.md:12:8"));
        assert!(message.contains("missing provider-local skill 'clear'"));
        assert!(message.contains("ArrowTarget"));
    }

    #[test]
    fn reports_missing_operational_markers() {
        assert_eq!(
            missing_markers("review-pr REVIEW_CURRENT", &["review-pr", "REVIEW_CURRENT"]),
            Vec::<&str>::new()
        );
        assert_eq!(
            missing_markers("review-pr", &["review-pr", "REVIEW_CURRENT"]),
            vec!["REVIEW_CURRENT"]
        );
    }

    #[test]
    fn rejects_unclosed_frontmatter() {
        let text = "---\nname: prepare-issue\ndescription: truncated\n";
        assert_eq!(frontmatter_value(text, "name"), None);
        assert_eq!(frontmatter_metadata_chars(text), 0);
    }

    #[test]
    fn ignores_delimiters_inside_document_body_for_metadata_size() {
        let text = "# Skill\n\n---\nnot frontmatter\n---\n";
        assert_eq!(frontmatter_metadata_chars(text), 0);
    }

    #[test]
    fn scenario_fixture_names_are_unique_and_cover_required_review_routes() {
        let names = SCENARIO_FIXTURES.iter().map(|fixture| fixture.name).collect::<BTreeSet<_>>();
        assert_eq!(names.len(), SCENARIO_FIXTURES.len());
        assert!(names.contains("fresh_issue"));
        assert!(names.contains("same_candidate_writer_collision"));
        assert!(names.contains("unchanged_main_movement"));
        assert!(names.contains("substantive_review_orchestration"));
        assert!(names.contains("review_enters_live_integration"));
        assert!(names.contains("live_ci_requires_review"));
        assert!(names.contains("related_pr_native_review"));
    }

    #[test]
    fn scenario_checker_reports_missing_static_route_contracts() {
        let mut providers = BTreeMap::new();
        providers.insert(
            "codex".to_string(),
            (
                [
                    "deliver-pr".to_string(),
                    "prepare-issue".to_string(),
                    "prepare-proof".to_string(),
                ]
                .into_iter()
                .collect(),
                [
                    ("deliver-pr".to_string(), ["prepare-issue".to_string()].into_iter().collect()),
                    (
                        "prepare-proof".to_string(),
                        ["prepare-issue".to_string()].into_iter().collect(),
                    ),
                ]
                .into_iter()
                .collect(),
            ),
        );

        let errors = check_scenarios(&providers);
        assert!(
            errors
                .iter()
                .any(|error| error.contains("no route from 'deliver-pr' to 'prepare-proof'"))
        );
    }

    #[test]
    fn scenario_output_ignores_unrelated_inventory_errors() {
        let output = super::scenario_output(super::CheckReport {
            schema: "agent-flow-check.v2",
            result: "FAIL",
            providers: BTreeMap::new(),
            scenarios: super::ScenarioReport {
                fixture_count: SCENARIO_FIXTURES.len(),
                checked_providers: vec!["codex".to_string()],
                errors: Vec::new(),
            },
            errors: vec!["codex: unrelated metadata error".to_string()],
            advisories: Vec::new(),
        });
        assert_eq!(output.result, "PASS");
        assert!(output.errors.is_empty());
    }
}

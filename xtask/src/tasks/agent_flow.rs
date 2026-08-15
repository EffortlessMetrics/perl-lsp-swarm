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
    metadata_chars: usize,
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
    metadata_chars: usize,
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
        let mut metadata_chars = 0;
        let mut checked_skills = Vec::new();

        for skill in skills
            .iter()
            .filter(|skill| selected_skill.is_none_or(|selected| selected == skill.name))
        {
            selected_matches += 1;
            metadata_chars += skill.metadata_chars;
            route_count += skill.route_targets.len();
            checked_skills.push(skill.name.clone());
            for target in &skill.route_targets {
                if !known_names.contains(target) {
                    errors.push(missing_route_target_message(&skill.path, &skill.name, target));
                }
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
        schema: "agent-flow-check.v1",
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
        let route_targets = route_targets(&text);
        skills.push(Skill {
            name: directory_name,
            path: skill_path,
            text,
            route_targets,
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
    let mut in_route_section = false;
    let mut targets = BTreeSet::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("## ") {
            in_route_section = is_route_heading(trimmed);
            continue;
        }
        targets.extend(route_tokens(trimmed, in_route_section));
    }
    targets.into_iter().collect()
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

fn missing_route_target_message(path: &Path, source: &str, target: &str) -> String {
    format!(
        "{}: route from '{}' points to missing provider-local skill '{}' (if this is prose or a code identifier, remove its backticks; route references should use an explicit route form)",
        path.display(),
        source,
        target
    )
}

fn route_tokens(line: &str, in_route_section: bool) -> Vec<String> {
    // Metasyntactic `$placeholders` that appear in prose inside route/orchestration
    // sections but are NOT actual skill route targets (#5930).
    const METASYNTACTIC_PLACEHOLDERS: &[&str] = &["skill", "skill_name", "skill-name"];

    let mut tokens = Vec::new();
    let chars = line.chars().collect::<Vec<_>>();
    let mut index = 0;
    while index < chars.len() {
        if in_route_section && chars[index] == '$' {
            let start = index + 1;
            let mut end = start;
            while end < chars.len()
                && (chars[end].is_ascii_lowercase()
                    || chars[end].is_ascii_digit()
                    || chars[end] == '-')
            {
                end += 1;
            }
            if end > start {
                let token: String = chars[start..end].iter().collect();
                // Skip metasyntactic placeholders that are prose, not route targets.
                if !METASYNTACTIC_PLACEHOLDERS.contains(&token.as_str()) {
                    tokens.push(token);
                }
            }
            index = end;
        } else if in_route_section && chars[index] == '`' {
            let start = index + 1;
            if let Some(relative_end) =
                chars[start..].iter().position(|character| *character == '`')
            {
                let end = start + relative_end;
                let token = chars[start..end].iter().collect::<String>();
                let token = token.strip_prefix('$').unwrap_or(&token);
                if token.chars().next().is_some_and(|character| character.is_ascii_lowercase())
                    && token.chars().all(|character| {
                        character.is_ascii_lowercase()
                            || character.is_ascii_digit()
                            || character == '-'
                    })
                    // Skip metasyntactic placeholders that are prose, not route
                    // targets — mirrors the bare-$ branch (#5930).
                    && !METASYNTACTIC_PLACEHOLDERS.contains(&token)
                {
                    tokens.push(token.to_owned());
                }
                index = end + 1;
            } else {
                index += 1;
            }
        } else {
            index += 1;
        }
    }
    tokens
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
        SCENARIO_FIXTURES, check_scenarios, frontmatter_metadata_chars, frontmatter_value,
        missing_markers, missing_route_target_message, route_targets, route_tokens,
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
    fn ignores_uppercase_status_tokens() {
        assert_eq!(
            route_tokens("- `REVIEW_CURRENT` -> `$verify-live-ci`", true),
            vec!["verify-live-ci"]
        );
    }

    #[test]
    fn ignores_metasyntactic_placeholders_in_backticks() {
        // A `$skill` placeholder in prose under a route-bearing heading must
        // not be treated as a route target — even inside backticks (#5930).
        assert_eq!(
            route_tokens("which `$skill` to consume. Do not ask agents.", true),
            Vec::<String>::new()
        );
        // The real skill names are still extracted.
        assert_eq!(route_tokens("- `$deliver-pr` then `$skill`", true), vec!["deliver-pr"]);
    }

    #[test]
    fn missing_route_diagnostic_explains_backticked_prose() {
        let message = missing_route_target_message(
            Path::new(".agents/skills/review-tests/SKILL.md"),
            "review-tests",
            "clear",
        );
        assert!(message.contains("missing provider-local skill 'clear'"));
        assert!(message.contains("if this is prose or a code identifier, remove its backticks"));
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
            schema: "agent-flow-check.v1",
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

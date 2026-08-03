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

#[derive(Debug, Clone)]
pub struct CheckConfig {
    pub skill: Option<String>,
    pub format: String,
}

#[derive(Debug, Serialize)]
struct CheckReport {
    schema: &'static str,
    result: &'static str,
    providers: BTreeMap<String, ProviderReport>,
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

#[derive(Debug)]
struct Skill {
    name: String,
    path: PathBuf,
    route_targets: Vec<String>,
    metadata_chars: usize,
}

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

fn check_repository(root: &Path, selected_skill: Option<&str>) -> Result<CheckReport> {
    let mut providers = BTreeMap::new();
    let mut errors = Vec::new();
    let mut advisories = Vec::new();
    let mut selected_matches = 0;

    for (provider, relative_root) in PROVIDER_SKILL_ROOTS {
        let skill_root = root.join(relative_root);
        let skills = collect_skills(&skill_root, &mut errors)?;
        let known_names = skills.iter().map(|skill| skill.name.clone()).collect::<BTreeSet<_>>();
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
                    errors.push(format!(
                        "{}: route from '{}' points to missing provider-local skill '{}'",
                        skill.path.display(),
                        skill.name,
                        target
                    ));
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

    let result = if errors.is_empty() { "PASS" } else { "FAIL" };
    Ok(CheckReport { schema: "agent-flow-check.v1", result, providers, errors, advisories })
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
        let metadata_chars = text
            .split_once("---")
            .and_then(|(_, remainder)| remainder.split_once("---"))
            .map_or(0, |(frontmatter, _)| frontmatter.len());
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
        || normalized.contains("valid exit")
        || normalized == "flow"
        || normalized.contains("orchestration")
        || normalized.contains("outcome")
        || normalized == "procedure"
}

fn route_tokens(line: &str, in_route_section: bool) -> Vec<String> {
    let mut tokens = Vec::new();
    let chars = line.chars().collect::<Vec<_>>();
    let mut index = 0;
    while index < chars.len() {
        if chars[index] == '$' {
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
                tokens.push(chars[start..end].iter().collect());
            }
            index = end;
        } else if in_route_section && chars[index] == '`' {
            let start = index + 1;
            if let Some(relative_end) =
                chars[start..].iter().position(|character| *character == '`')
            {
                let end = start + relative_end;
                let token = chars[start..end].iter().collect::<String>();
                if token.chars().next().is_some_and(|character| character.is_ascii_lowercase())
                    && token.chars().all(|character| {
                        character.is_ascii_lowercase()
                            || character.is_ascii_digit()
                            || character == '-'
                    })
                {
                    tokens.push(token);
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
    for error in &report.errors {
        println!("ERROR: {error}");
    }
    for advisory in &report.advisories {
        println!("ADVISORY: {advisory}");
    }
}

#[cfg(test)]
mod tests {
    use super::{frontmatter_value, route_targets, route_tokens};

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
    fn ignores_uppercase_status_tokens() {
        assert_eq!(
            route_tokens("- `REVIEW_CURRENT` -> `$verify-live-ci`", true),
            vec!["verify-live-ci"]
        );
    }

    #[test]
    fn rejects_unclosed_frontmatter() {
        let text = "---\nname: prepare-issue\ndescription: truncated\n";
        assert_eq!(frontmatter_value(text, "name"), None);
    }
}

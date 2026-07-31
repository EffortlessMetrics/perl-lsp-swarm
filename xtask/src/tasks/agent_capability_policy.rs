//! Enforce the retained Claude operation-profile capability boundary.
//!
//! Issue #3763 established an important control: a profile used for a
//! non-mutating review, audit, external-oracle, or CI-triage operation must not
//! receive direct mutation tools merely because its prose says not to write.
//!
//! The original gate encoded the historical lifecycle/persona catalogue as a
//! mandatory list. That made deleting the retired catalogue fail CI even when
//! the provider-native skill graph intentionally used no custom profiles.
//!
//! This check now governs only the small optional operation profiles whose
//! normal assignment is non-mutating. Their absence is valid: the warm root may
//! execute the same skill directly or use a provider-native built-in. When one
//! of these profiles is present, it must carry an explicit `tools:` allowlist
//! and exclude direct mutation or proxy-spawn tools.
//!
//! `Bash` remains available because these profiles inspect Git, tests, logs,
//! and repository-owned read-only instruments. This gate therefore proves a
//! bounded direct-tool surface; it does not claim shell-level sandbox isolation
//! or epistemic independence. Runtime permission inheritance and override
//! behavior remain capability-audit concerns.

use color_eyre::eyre::{Result, bail, eyre};
use serde_yaml_ng::Value as YamlValue;
use std::{
    fs,
    path::{Path, PathBuf},
};

/// Optional Claude operation profiles whose normal assignment is
/// non-mutating. Missing profiles are valid; present profiles are governed.
///
/// Keep this list aligned with any retained `.claude/agents/operation-*.md`
/// profiles that claim a fixed-candidate/read-only evidence boundary. Writer
/// profiles such as lane workers, feedback responders, and merge reconcilers
/// are intentionally excluded.
pub const OPTIONAL_NON_MUTATING_PROFILES: &[&str] = &[
    "operation-ci-triager",
    "operation-external-oracle",
    "operation-formal-reviewer",
    "operation-test-adversary",
];

/// Tool names that grant direct write, mutation, publication, or proxy-spawn
/// capability. A governed non-mutating profile allowlist must contain none of
/// these.
///
/// `Agent`/`Task` are excluded because a profile could proxy mutation through a
/// child. `Artifact` publishes a hosted artifact and is unnecessary for these
/// bounded evidence operations.
pub const FORBIDDEN_DIRECT_MUTATION_TOOLS: &[&str] = &[
    "Edit",
    "Write",
    "MultiEdit",
    "NotebookEdit",
    "Agent",
    "Task",
    "Artifact",
];

/// A single policy violation for one governed operation profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    pub agent: String,
    pub reason: String,
}

/// Validate every present optional non-mutating Claude operation profile under
/// `root`. `root` defaults to the repository root.
pub fn run(root: Option<PathBuf>) -> Result<()> {
    let root = match root {
        Some(root) => root,
        None => crate::utils::project_root()?,
    };
    let agents_dir = root.join(".claude/agents");
    let violations = audit_dir(&agents_dir)?;

    if violations.is_empty() {
        println!(
            "M4b capability boundary OK: {} optional non-mutating Claude operation profile(s) present; all exclude direct mutation/proxy tools ({})",
            present_profile_count(&agents_dir),
            agents_dir.display()
        );
        return Ok(());
    }

    let mut message = String::from(
        "M4b capability boundary FAILED: present non-mutating Claude operation profiles must exclude direct mutation/proxy tools \
         (Edit/Write/NotebookEdit/MultiEdit/Agent/Task/Artifact) and carry an explicit tools: allowlist. Offenders:\n",
    );
    for violation in &violations {
        message.push_str(&format!("  - {}: {}\n", violation.agent, violation.reason));
    }
    bail!(message);
}

/// Audit each governed profile that is actually present in `agents_dir`.
///
/// The optional catalogue may be absent or empty. This gate does not require a
/// named actor when the main thread or a provider-native built-in performs the
/// operation.
pub fn audit_dir(agents_dir: &Path) -> Result<Vec<Violation>> {
    if !agents_dir.exists() {
        return Ok(Vec::new());
    }
    if !agents_dir.is_dir() {
        bail!("{} exists but is not a directory", agents_dir.display());
    }

    let mut violations = Vec::new();
    for agent in OPTIONAL_NON_MUTATING_PROFILES {
        let path = agents_dir.join(format!("{agent}.md"));
        if !path.exists() {
            continue;
        }
        if !path.is_file() {
            violations.push(Violation {
                agent: (*agent).to_string(),
                reason: format!("expected {} to be a file", path.display()),
            });
            continue;
        }
        if let Err(reason) = enforce_read_only(&path)? {
            violations.push(Violation { agent: (*agent).to_string(), reason });
        }
    }
    Ok(violations)
}

fn present_profile_count(agents_dir: &Path) -> usize {
    OPTIONAL_NON_MUTATING_PROFILES
        .iter()
        .filter(|agent| agents_dir.join(format!("{agent}.md")).is_file())
        .count()
}

/// Check one governed profile definition for the direct-tool invariant.
///
/// The outer `Result` carries hard I/O or parse errors; the inner
/// `Result<(), String>` carries a policy violation reason (`Err`) or
/// compliance (`Ok`).
pub fn enforce_read_only(path: &Path) -> Result<std::result::Result<(), String>> {
    let frontmatter = load_frontmatter(path)?;

    let Some(tools_value) = frontmatter.get("tools") else {
        return Ok(Err(
            "no explicit tools: allowlist — inherits the parent tool surface (fail-open)"
                .to_string(),
        ));
    };

    let tools = parse_tool_list(tools_value)
        .map_err(|error| eyre!("{}: could not parse tools: allowlist: {error}", path.display()))?;

    if tools.is_empty() {
        return Ok(Err("tools: allowlist is empty".to_string()));
    }

    let forbidden: Vec<String> = tools
        .iter()
        .filter(|tool| {
            FORBIDDEN_DIRECT_MUTATION_TOOLS
                .iter()
                .any(|forbidden| forbidden.eq_ignore_ascii_case(tool))
        })
        .cloned()
        .collect();

    if forbidden.is_empty() {
        Ok(Ok(()))
    } else {
        Ok(Err(format!(
            "allowlist grants direct mutation/proxy tool(s): {}",
            forbidden.join(", ")
        )))
    }
}

/// Parse a frontmatter `tools:` value into individual tool names.
///
/// Accepts either an inline scalar (`tools: Read, Grep, Glob`) — which YAML
/// parses as a single comma-joined string — or a YAML sequence
/// (`tools: [Read, Grep]` or a block list).
fn parse_tool_list(value: &YamlValue) -> std::result::Result<Vec<String>, String> {
    match value {
        YamlValue::String(raw) => Ok(raw
            .split(',')
            .map(str::trim)
            .filter(|tool| !tool.is_empty())
            .map(str::to_string)
            .collect()),
        YamlValue::Sequence(items) => {
            let mut tools = Vec::with_capacity(items.len());
            for item in items {
                let tool = item
                    .as_str()
                    .ok_or_else(|| "tools: sequence entries must be strings".to_string())?;
                let tool = tool.trim();
                if !tool.is_empty() {
                    tools.push(tool.to_string());
                }
            }
            Ok(tools)
        }
        YamlValue::Null => Ok(Vec::new()),
        other => Err(format!("tools: must be a string or sequence, got {other:?}")),
    }
}

/// Load and parse the YAML frontmatter mapping from an agent definition file.
fn load_frontmatter(path: &Path) -> Result<serde_yaml_ng::Mapping> {
    let text = fs::read_to_string(path)
        .map_err(|error| eyre!("failed to read {}: {error}", path.display()))?;
    let normalized = text.replace("\r\n", "\n");
    let Some(rest) = normalized.strip_prefix("---\n") else {
        bail!("{} must start with YAML frontmatter", path.display());
    };
    let Some((frontmatter, _body)) = rest.split_once("\n---") else {
        bail!("{} must contain opening and closing frontmatter markers", path.display());
    };
    let data: YamlValue = serde_yaml_ng::from_str(frontmatter)
        .map_err(|error| eyre!("{} has invalid YAML frontmatter: {error}", path.display()))?;
    data.as_mapping()
        .cloned()
        .ok_or_else(|| eyre!("{} frontmatter must parse to a mapping", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const READ_ONLY_TOOLS: &str = "Read, Grep, Glob, Bash, WebSearch, WebFetch, TodoWrite";

    fn write_agent(dir: &Path, name: &str, tools_line: Option<&str>) -> Result<PathBuf> {
        fs::create_dir_all(dir)
            .map_err(|error| eyre!("failed to create {}: {error}", dir.display()))?;
        let path = dir.join(format!("{name}.md"));
        let mut frontmatter = format!("---\nname: {name}\ndescription: test operation profile\n");
        if let Some(tools) = tools_line {
            frontmatter.push_str(&format!("tools: {tools}\n"));
        }
        frontmatter.push_str("---\n\nprofile body\n");
        fs::write(&path, frontmatter)
            .map_err(|error| eyre!("failed to write {}: {error}", path.display()))?;
        Ok(path)
    }

    #[test]
    fn real_repository_optional_profiles_satisfy_boundary() -> Result<()> {
        run(None)
    }

    #[test]
    fn accepts_absent_optional_catalogue() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let agents_dir = tmp.path().join(".claude/agents");
        assert!(audit_dir(&agents_dir)?.is_empty());
        Ok(())
    }

    #[test]
    fn accepts_read_only_allowlist() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let path = write_agent(
            tmp.path(),
            "operation-formal-reviewer",
            Some(READ_ONLY_TOOLS),
        )?;
        assert_eq!(enforce_read_only(&path)?, Ok(()));
        Ok(())
    }

    #[test]
    fn rejects_write_tool_in_allowlist() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let path = write_agent(
            tmp.path(),
            "operation-formal-reviewer",
            Some("Read, Grep, Glob, Edit, Write"),
        )?;
        match enforce_read_only(&path)? {
            Ok(()) => bail!("a non-mutating profile with Edit/Write must be rejected"),
            Err(reason) => {
                assert!(reason.contains("Edit"), "reason should name Edit: {reason}");
                assert!(reason.contains("Write"), "reason should name Write: {reason}");
            }
        }
        Ok(())
    }

    #[test]
    fn rejects_agent_sub_spawn_tool() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let path = write_agent(
            tmp.path(),
            "operation-test-adversary",
            Some("Read, Grep, Glob, Agent"),
        )?;
        match enforce_read_only(&path)? {
            Ok(()) => bail!("a non-mutating profile that can spawn agents must be rejected"),
            Err(reason) => assert!(reason.contains("Agent"), "reason should name Agent: {reason}"),
        }
        Ok(())
    }

    #[test]
    fn rejects_missing_tools_allowlist_fail_closed() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let path = write_agent(tmp.path(), "operation-external-oracle", None)?;
        match enforce_read_only(&path)? {
            Ok(()) => bail!("a governed profile with no tools: allowlist must fail"),
            Err(reason) => assert!(reason.contains("no explicit"), "reason: {reason}"),
        }
        Ok(())
    }

    #[test]
    fn audit_dir_ignores_absent_profiles_and_unrelated_writers() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let agents_dir = tmp.path().join(".claude/agents");
        write_agent(
            &agents_dir,
            "operation-lane-worker",
            Some("Read, Grep, Glob, Edit, Write, Agent"),
        )?;
        assert!(audit_dir(&agents_dir)?.is_empty());
        Ok(())
    }

    #[test]
    fn audit_dir_flags_present_broken_governed_profile() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let agents_dir = tmp.path().join(".claude/agents");
        write_agent(
            &agents_dir,
            "operation-formal-reviewer",
            Some("Read, Grep, Write"),
        )?;
        let violations = audit_dir(&agents_dir)?;
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].agent, "operation-formal-reviewer");
        assert!(violations[0].reason.contains("Write"));
        Ok(())
    }

    #[test]
    fn run_rejects_broken_governed_profile() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let agents_dir = tmp.path().join(".claude/agents");
        write_agent(
            &agents_dir,
            "operation-ci-triager",
            Some("Read, Grep, Edit"),
        )?;
        match run(Some(tmp.path().to_path_buf())) {
            Ok(()) => bail!("run() must fail on a governed profile with a mutation tool"),
            Err(error) => assert!(
                error.to_string().contains("M4b capability boundary FAILED"),
                "error: {error}"
            ),
        }
        Ok(())
    }

    #[test]
    fn parses_inline_and_sequence_tool_lists() -> Result<()> {
        let inline = serde_yaml_ng::from_str::<YamlValue>("Read, Grep, Glob")?;
        assert_eq!(
            parse_tool_list(&inline).map_err(|error| eyre!(error))?,
            vec!["Read".to_string(), "Grep".to_string(), "Glob".to_string()]
        );
        let seq = serde_yaml_ng::from_str::<YamlValue>("[Read, Write]")?;
        assert_eq!(
            parse_tool_list(&seq).map_err(|error| eyre!(error))?,
            vec!["Read".to_string(), "Write".to_string()]
        );
        Ok(())
    }
}

//! Staged Changie render-surface parsing and path containment.

use serde::Deserialize;
use std::collections::BTreeSet;
use std::path::{Component, Path};

pub(super) const CONFIG_PATH: &str = ".changie.yaml";
pub(super) const AQUA_CONFIG_PATH: &str = "aqua.yaml";

#[derive(Debug, Deserialize)]
struct RawRenderConfig {
    #[serde(rename = "changesDir")]
    changes_dir: String,
    #[serde(rename = "unreleasedDir")]
    unreleased_dir: String,
    #[serde(default, rename = "headerPath")]
    header_path: Option<String>,
    #[serde(default, rename = "versionHeaderPath")]
    version_header_path: Option<String>,
    #[serde(default, rename = "versionFooterPath")]
    version_footer_path: Option<String>,
    #[serde(default)]
    projects: Vec<RawProject>,
}

#[derive(Debug, Deserialize)]
struct RawProject {
    key: String,
    changelog: String,
}

#[derive(Debug)]
pub(super) struct RenderSurface {
    changes_dir: String,
    unreleased_dir: String,
    projects: Vec<ProjectSurface>,
}

#[derive(Debug)]
struct ProjectSurface {
    key: String,
    changelog: String,
}

impl RenderSurface {
    pub(super) fn parse(config_text: &str) -> Result<Self, String> {
        let raw: RawRenderConfig = serde_yaml_ng::from_str(config_text)
            .map_err(|err| format!("failed to parse Changie render paths: {err}"))?;
        let changes_dir = normalize_repo_relative(&raw.changes_dir, "changesDir")?;
        let unreleased_leaf = normalize_repo_relative(&raw.unreleased_dir, "unreleasedDir")?;
        let unreleased_dir = format!("{changes_dir}/{unreleased_leaf}");

        for (label, path) in [
            ("headerPath", raw.header_path.as_deref()),
            ("versionHeaderPath", raw.version_header_path.as_deref()),
            ("versionFooterPath", raw.version_footer_path.as_deref()),
        ] {
            if let Some(path) = path {
                normalize_repo_relative(path, label)?;
            }
        }

        if raw.projects.is_empty() {
            return Err("Changie config must define at least one project".to_string());
        }

        let mut keys = BTreeSet::new();
        let mut projects = Vec::with_capacity(raw.projects.len());
        for project in raw.projects {
            let key = project.key.trim();
            if key.is_empty() {
                return Err("Changie project key cannot be empty".to_string());
            }
            if !keys.insert(key.to_string()) {
                return Err(format!("duplicate Changie project key `{key}`"));
            }
            projects.push(ProjectSurface {
                key: key.to_string(),
                changelog: normalize_repo_relative(
                    &project.changelog,
                    &format!("changelog path for project `{key}`"),
                )?,
            });
        }

        Ok(Self {
            changes_dir,
            unreleased_dir,
            projects,
        })
    }

    pub(super) fn is_input(&self, path: &str) -> bool {
        path == CONFIG_PATH
            || path == AQUA_CONFIG_PATH
            || is_within(path, &self.changes_dir)
            || self
                .projects
                .iter()
                .any(|project| project.changelog == path)
    }

    pub(super) fn is_fragment(&self, path: &str) -> bool {
        let lower = path.to_ascii_lowercase();
        is_within(path, &self.unreleased_dir)
            && (lower.ends_with(".yaml") || lower.ends_with(".yml"))
    }

    pub(super) fn project_keys(&self) -> Vec<String> {
        self.projects
            .iter()
            .map(|project| project.key.clone())
            .collect()
    }
}

pub(super) fn normalize_repo_relative(value: &str, label: &str) -> Result<String, String> {
    let portable = value.replace('\\', "/");
    let path = Path::new(&portable);
    if path.is_absolute() {
        return Err(format!(
            "{label} must be repository-relative, got `{value}`"
        ));
    }

    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => {
                let part = part
                    .to_str()
                    .ok_or_else(|| format!("{label} contains non-UTF-8 path data"))?;
                parts.push(part.to_string());
            }
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(format!(
                    "{label} must not escape the repository, got `{value}`"
                ));
            }
        }
    }
    if parts.is_empty() {
        return Err(format!("{label} must name a non-root path"));
    }
    Ok(parts.join("/"))
}

fn is_within(path: &str, directory: &str) -> bool {
    path == directory
        || path
            .strip_prefix(directory)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

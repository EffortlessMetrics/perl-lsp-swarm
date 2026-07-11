//! Advisory changelog-ledger checks for the Changie PR-time fragment flow.
//!
//! FOUNDATION / ADVISORY (issue #3768). This task validates that a PR carries an
//! explicit changelog *disposition* — a Changie fragment under
//! `.changes/unreleased/` OR an exemption-with-reason — and that any added
//! fragment is schema-valid and (when `changie` is on `PATH`) renders through
//! `changie batch --dry-run`. It **always exits 0**: it prints findings and
//! never blocks a PR. A follow-up PR flips enforcement on.
//!
//! It changes NO release execution: Cargo versions, publishing, and the
//! git-cliff generation path documented in `docs/CHANGELOG_WORKFLOW.md` are
//! untouched.

use color_eyre::eyre::{Result, eyre};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

const CHANGIE_CONFIG: &str = ".changie.yaml";
const POLICY_FILE: &str = "policy/changelog.toml";
const UNRELEASED_DIR: &str = ".changes/unreleased";
const SAMPLES_DIR: &str = ".changes/samples";
const PRODUCT_CHANGELOG: &str = "CHANGELOG.md";
const VSCODE_CHANGELOG: &str = "vscode-extension/CHANGELOG.md";

/// Parsed subset of `.changie.yaml` needed for validation.
#[derive(Debug, Deserialize)]
struct ChangieConfig {
    #[serde(default)]
    projects: Vec<ChangieProject>,
    #[serde(default)]
    components: Vec<String>,
    #[serde(default)]
    kinds: Vec<ChangieKind>,
    #[serde(default)]
    body: BodyConfig,
}

#[derive(Debug, Deserialize)]
struct ChangieProject {
    key: String,
}

#[derive(Debug, Deserialize)]
struct ChangieKind {
    label: String,
    #[serde(default)]
    key: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct BodyConfig {
    #[serde(default, rename = "minLength")]
    min_length: Option<u64>,
}

impl ChangieConfig {
    fn project_keys(&self) -> Vec<&str> {
        self.projects.iter().map(|p| p.key.as_str()).collect()
    }

    /// Kinds are matched case-insensitively against either the label or the key.
    fn has_kind(&self, kind: &str) -> bool {
        self.kinds.iter().any(|k| {
            k.label.eq_ignore_ascii_case(kind)
                || k.key.as_deref().is_some_and(|key| key.eq_ignore_ascii_case(kind))
        })
    }

    fn min_body(&self) -> usize {
        self.body.min_length.unwrap_or(0) as usize
    }
}

/// A parsed Changie fragment (the fields this check cares about).
#[derive(Debug, Deserialize)]
struct Fragment {
    #[serde(default)]
    project: String,
    #[serde(default)]
    component: String,
    #[serde(default)]
    kind: String,
    #[serde(default)]
    body: String,
    #[serde(default)]
    custom: BTreeMap<String, String>,
}

/// Validate one fragment against the config; return human-readable findings
/// (empty vec => valid).
fn validate_fragment(frag: &Fragment, cfg: &ChangieConfig) -> Vec<String> {
    let mut findings = Vec::new();

    let keys = cfg.project_keys();
    if frag.project.is_empty() {
        findings.push("missing `project` field".to_string());
    } else if !keys.iter().any(|k| *k == frag.project) {
        findings.push(format!(
            "unknown project `{}` (expected one of: {})",
            frag.project,
            keys.join(", ")
        ));
    }

    if frag.kind.is_empty() {
        findings.push("missing `kind` field".to_string());
    } else if !cfg.has_kind(&frag.kind) {
        findings.push(format!("unknown kind `{}`", frag.kind));
    }

    if !frag.component.is_empty() && !cfg.components.iter().any(|c| c == &frag.component) {
        findings.push(format!("unknown component `{}`", frag.component));
    }

    let min = cfg.min_body();
    if frag.body.trim().chars().count() < min {
        findings.push(format!("body too short (need >= {min} chars): {:?}", frag.body.trim()));
    }

    match frag.custom.get("PR") {
        None => findings.push("missing custom `PR` metadata".to_string()),
        Some(pr) => match pr.parse::<u64>() {
            Ok(n) if n >= 1 => {}
            _ => findings.push(format!("custom `PR` must be a positive integer, got {pr:?}")),
        },
    }

    if let Some(breaking) = frag.custom.get("Breaking")
        && breaking != "no"
        && breaking != "yes"
    {
        findings.push(format!("custom `Breaking` must be `no` or `yes`, got {breaking:?}"));
    }

    findings
}

/// A PR's changelog disposition.
#[derive(Debug, PartialEq, Eq)]
enum Disposition {
    /// One or more fragment files were added under `.changes/unreleased/`.
    Fragment(Vec<String>),
    /// An explicit exemption (PR-body marker or `.changes/exemptions/*` file).
    Exemption(String),
    /// A recognized release-prep PR (version bump + changelog batch).
    ReleasePrep,
    /// No explicit disposition found.
    Missing,
}

/// Recognized exemption categories (kept in sync with `policy/changelog.toml`).
const EXEMPTION_CATEGORIES: &[&str] = &[
    "none",
    "tests",
    "ci",
    "refactor",
    "generated-status",
    "docs-no-contract-change",
    "deps",
    "release-prep",
    "changelog-tooling",
];

/// Parse a PR-body exemption marker.
///
/// Accepts, case-insensitively, a line of the form:
/// `changelog-exempt: <category> — <reason>` or `changelog: none — <reason>`
/// (the separator may be `—`, `-`, or `:`).
fn parse_exemption_marker(pr_body: &str) -> Option<(String, String)> {
    for raw in pr_body.lines() {
        // Strip Markdown list/quote decoration and surrounding whitespace.
        let line = raw.trim().trim_start_matches(['>', '*', '-', ' ']).trim();
        let lower = line.to_ascii_lowercase();
        // `lower` has the same byte length as `line` (ASCII-only case folding),
        // so slicing by a suffix length stays on a char boundary.
        let tail = if let Some(t) = lower.strip_prefix("changelog-exempt:") {
            &line[line.len() - t.len()..]
        } else if let Some(t) = lower.strip_prefix("changelog:") {
            &line[line.len() - t.len()..]
        } else {
            continue;
        };
        let tail = tail.trim();
        let tail_lower = tail.to_ascii_lowercase();
        for cat in EXEMPTION_CATEGORIES {
            let Some(after) = tail_lower.strip_prefix(cat) else {
                continue;
            };
            // The category must be a whole token, not a prefix of a longer word.
            let next = after.chars().next();
            if matches!(next, None | Some(' ') | Some('-') | Some('—') | Some(':')) {
                let reason_start = tail.len() - after.len();
                let reason = tail[reason_start..]
                    .trim_start_matches([' ', '-', '—', ':'])
                    .trim()
                    .to_string();
                return Some(((*cat).to_string(), reason));
            }
        }
    }
    None
}

/// Files under `.changes/unreleased/` that are fragments (exclude `.gitkeep`).
fn fragment_files(changed: &[String]) -> Vec<String> {
    changed
        .iter()
        .filter(|f| {
            let norm = f.replace('\\', "/");
            norm.starts_with(&format!("{UNRELEASED_DIR}/")) && norm.ends_with(".yaml")
        })
        .cloned()
        .collect()
}

/// Heuristic: is this a release-prep PR (version bump + changelog batch)?
fn is_release_prep(changed: &[String]) -> bool {
    if changed.is_empty() {
        return false;
    }
    changed.iter().all(|f| {
        let n = f.replace('\\', "/");
        n == PRODUCT_CHANGELOG
            || n == VSCODE_CHANGELOG
            || n == "Cargo.toml"
            || n == "Cargo.lock"
            || n.ends_with("/Cargo.toml")
            || n == "features.toml"
            || n == "vscode-extension/package.json"
            || n == "vscode-extension/package-lock.json"
            || n.starts_with(".changes/")
    })
}

/// Best-effort mapping of a changed path to an exemption category (for hints).
fn exempt_category_for_path(path: &str) -> Option<&'static str> {
    let n = path.replace('\\', "/");
    if n == PRODUCT_CHANGELOG || n == VSCODE_CHANGELOG {
        return None; // handled by direct-edit detection
    }
    if n == ".changie.yaml"
        || n.starts_with(".changes/")
        || n == "cliff.toml"
        || n == "docs/CHANGELOG_WORKFLOW.md"
        || n == "policy/changelog.toml"
    {
        return Some("changelog-tooling");
    }
    if n == "docs/project/CURRENT_STATUS.md" || n.starts_with("docs/project/status/") {
        return Some("generated-status");
    }
    if n == "Cargo.lock" {
        return Some("deps");
    }
    if n.starts_with(".github/")
        || n.starts_with(".ci/")
        || n.starts_with("scripts/ci/")
        || n.starts_with("policy/")
    {
        return Some("ci");
    }
    if n.contains("/tests/") || n.ends_with("_test.rs") {
        return Some("tests");
    }
    if n.starts_with("docs/") || n.ends_with(".md") {
        return Some("docs-no-contract-change");
    }
    None
}

/// Detect the PR's disposition from its changed files and PR body.
fn detect_disposition(changed: &[String], pr_body: &str) -> Disposition {
    let frags = fragment_files(changed);
    if !frags.is_empty() {
        return Disposition::Fragment(frags);
    }
    if let Some((cat, reason)) = parse_exemption_marker(pr_body) {
        let reason = if reason.is_empty() { "(no reason given)".to_string() } else { reason };
        return Disposition::Exemption(format!("{cat}: {reason}"));
    }
    if changed.iter().any(|f| {
        let n = f.replace('\\', "/");
        n.starts_with(".changes/exemptions/") && n.ends_with(".md")
    }) {
        return Disposition::Exemption("file-based (.changes/exemptions/)".to_string());
    }
    if is_release_prep(changed) {
        return Disposition::ReleasePrep;
    }
    Disposition::Missing
}

/// Direct-edit warning: feature PRs should not hand-edit the generated
/// changelogs. Returns a warning message when applicable.
fn direct_changelog_edit_warning(changed: &[String], is_release_prep: bool) -> Option<String> {
    if is_release_prep {
        return None;
    }
    let edited: Vec<&str> = changed
        .iter()
        .map(|f| f.as_str())
        .filter(|f| {
            let n = f.replace('\\', "/");
            n == PRODUCT_CHANGELOG || n == VSCODE_CHANGELOG
        })
        .collect();
    if edited.is_empty() {
        None
    } else {
        Some(format!(
            "directly edits generated changelog(s): {}. Feature PRs should add a \
             Changie fragment (`changie new`) instead of hand-editing these files.",
            edited.join(", ")
        ))
    }
}

/// Is `changie` available on `PATH`?
fn changie_available() -> bool {
    Command::new("changie").arg("--version").output().map(|o| o.status.success()).unwrap_or(false)
}

/// Render a project's unreleased fragments via `changie batch --dry-run --keep`.
/// Returns rendered stdout, or None if changie is unavailable / the project has
/// no fragments. Never propagates a hard error (advisory).
fn render_project(root: &Path, project: &str) -> Option<String> {
    let out = Command::new("changie")
        .current_dir(root)
        .args(["batch", "v0.0.0-advisory-selftest", "--project", project, "--dry-run", "--keep"])
        .output()
        .ok()?;
    if out.status.success() {
        Some(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        None
    }
}

/// Read the changed-files list, either from an explicit file or via git diff.
fn read_changed_files(
    root: &Path,
    changed_files: Option<&Path>,
    base: &str,
) -> Result<Vec<String>> {
    if let Some(list) = changed_files {
        let content = std::fs::read_to_string(list)
            .map_err(|e| eyre!("failed to read changed-files list {}: {e}", list.display()))?;
        return Ok(content
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(str::to_string)
            .collect());
    }
    let out = Command::new("git")
        .current_dir(root)
        .args(["diff", "--name-only", &format!("{base}...HEAD")])
        .output()
        .map_err(|e| eyre!("failed to run git diff: {e}"))?;
    if !out.status.success() {
        // Advisory: an unknown base ref should not abort the check.
        eprintln!(
            "note: `git diff {base}...HEAD` failed ({}); no changed files resolved",
            String::from_utf8_lossy(&out.stderr).trim()
        );
        return Ok(Vec::new());
    }
    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect())
}

fn load_config(root: &Path) -> Result<ChangieConfig> {
    let path = root.join(CHANGIE_CONFIG);
    let content = std::fs::read_to_string(&path)
        .map_err(|e| eyre!("failed to read {}: {e}", path.display()))?;
    serde_yaml_ng::from_str(&content).map_err(|e| eyre!("failed to parse {}: {e}", path.display()))
}

/// Load a fragment from disk and validate it; append findings to `report`.
fn check_fragment_file(root: &Path, rel: &str, cfg: &ChangieConfig, report: &mut Report) {
    let path = root.join(rel);
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => {
            report.warn(format!("could not read fragment {rel}: {e}"));
            return;
        }
    };
    match serde_yaml_ng::from_str::<Fragment>(&content) {
        Ok(frag) => {
            let findings = validate_fragment(&frag, cfg);
            if findings.is_empty() {
                report.ok(format!("fragment {rel} is schema-valid"));
            } else {
                for f in findings {
                    report.warn(format!("{rel}: {f}"));
                }
            }
        }
        Err(e) => report.warn(format!("fragment {rel} does not parse as YAML: {e}")),
    }
}

/// Accumulates advisory findings for a single run.
#[derive(Default)]
struct Report {
    lines: Vec<String>,
}

impl Report {
    fn ok(&mut self, msg: impl Into<String>) {
        self.lines.push(format!("  OK   {}", msg.into()));
    }
    fn info(&mut self, msg: impl Into<String>) {
        self.lines.push(format!("  INFO {}", msg.into()));
    }
    fn warn(&mut self, msg: impl Into<String>) {
        self.lines.push(format!("  WARN {}", msg.into()));
    }
    fn emit(&self) {
        for l in &self.lines {
            println!("{l}");
        }
    }
}

/// Entry point for `cargo xtask changelog check`.
///
/// ADVISORY: always returns `Ok(())` when the workspace is well-formed; findings
/// are printed, not fatal.
pub fn check(
    base: Option<String>,
    changed_files: Option<PathBuf>,
    pr_body_file: Option<PathBuf>,
    self_test: bool,
    root: Option<PathBuf>,
) -> Result<()> {
    let root = match root {
        Some(r) => r,
        None => crate::utils::project_root()?,
    };

    println!("changelog check (ADVISORY — issue #3768; never blocks a PR)");

    let cfg = load_config(&root)?;
    // Surface the policy file / cutoff status for the reader.
    report_policy(&root);

    let mut report = Report::default();

    if self_test {
        run_self_test(&root, &cfg, &mut report);
        report.emit();
        return Ok(());
    }

    let base = base.unwrap_or_else(|| "origin/main".to_string());
    let changed = read_changed_files(&root, changed_files.as_deref(), &base)?;
    let pr_body = match pr_body_file {
        Some(p) => std::fs::read_to_string(&p).unwrap_or_default(),
        None => std::env::var("CHANGELOG_PR_BODY").unwrap_or_default(),
    };

    if changed.is_empty() {
        report.info("no changed files resolved (nothing to check)");
        report.emit();
        return Ok(());
    }

    let disposition = detect_disposition(&changed, &pr_body);
    match &disposition {
        Disposition::Fragment(frags) => {
            report.ok(format!("disposition: {} Changie fragment(s) added", frags.len()));
            for rel in frags {
                check_fragment_file(&root, rel, &cfg, &mut report);
            }
            // Render each project that has unreleased fragments.
            render_disposition_projects(&root, &mut report);
        }
        Disposition::Exemption(reason) => {
            report.ok(format!("disposition: exemption — {reason}"));
        }
        Disposition::ReleasePrep => {
            report.ok("disposition: recognized release-prep PR (version bump + changelog batch)");
        }
        Disposition::Missing => {
            report.warn(
                "no explicit changelog disposition found. Add a Changie fragment \
                 (`changie new`) OR a PR-body marker: `changelog-exempt: <category> — <reason>`.",
            );
            report_category_hint(&changed, &mut report);
        }
    }

    if let Some(warning) =
        direct_changelog_edit_warning(&changed, disposition == Disposition::ReleasePrep)
    {
        report.warn(warning);
    }

    report.emit();
    Ok(())
}

/// Print the policy file's enforcement mode and cutoff status.
fn report_policy(root: &Path) {
    let path = root.join(POLICY_FILE);
    match std::fs::read_to_string(&path) {
        Ok(content) => {
            let enforcement = toml::from_str::<toml::Value>(&content)
                .ok()
                .and_then(|v| v.get("enforcement").and_then(|e| e.as_str()).map(str::to_string))
                .unwrap_or_else(|| "advisory".to_string());
            let cutoff = toml::from_str::<toml::Value>(&content)
                .ok()
                .and_then(|v| {
                    v.get("fragment_enforcement_from").and_then(|e| e.as_str()).map(str::to_string)
                })
                .unwrap_or_default();
            println!("  policy: enforcement={enforcement}");
            if cutoff.starts_with("TODO") || cutoff.is_empty() {
                println!(
                    "  policy: cutoff not yet set ({cutoff}); enforcement boundary inactive \
                     until #3768 merges"
                );
            } else {
                println!("  policy: fragment_enforcement_from={cutoff}");
            }
        }
        Err(_) => println!("  policy: {POLICY_FILE} not found (advisory default)"),
    }
}

/// Render every project that has at least one unreleased fragment.
fn render_disposition_projects(root: &Path, report: &mut Report) {
    if !changie_available() {
        report.info("changie not on PATH; skipping render validation (advisory)");
        return;
    }
    let unreleased = root.join(UNRELEASED_DIR);
    let projects = fragment_projects_on_disk(&unreleased);
    if projects.is_empty() {
        report.info("no unreleased fragments on disk to render");
        return;
    }
    for project in projects {
        match render_project(root, &project) {
            Some(rendered) if !rendered.trim().is_empty() => {
                report.ok(format!("`changie batch --project {project} --dry-run` rendered OK"));
            }
            _ => report.info(format!("render for project `{project}` produced no output")),
        }
    }
}

/// Distinct project keys present in the on-disk unreleased fragments.
fn fragment_projects_on_disk(unreleased: &Path) -> Vec<String> {
    let mut projects = Vec::new();
    let Ok(entries) = std::fs::read_dir(unreleased) else {
        return projects;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("yaml") {
            continue;
        }
        if let Ok(content) = std::fs::read_to_string(&path)
            && let Ok(frag) = serde_yaml_ng::from_str::<Fragment>(&content)
            && !frag.project.is_empty()
            && !projects.contains(&frag.project)
        {
            projects.push(frag.project);
        }
    }
    projects
}

/// For a Missing disposition, hint the likely exemption category.
fn report_category_hint(changed: &[String], report: &mut Report) {
    let cats: Vec<&str> = changed.iter().filter_map(|f| exempt_category_for_path(f)).collect();
    if !cats.is_empty() && cats.len() == changed.len() {
        let mut uniq: Vec<&str> = cats.clone();
        uniq.sort_unstable();
        uniq.dedup();
        report.info(format!(
            "all changed files look like `{}` — an exemption likely applies",
            uniq.join(", ")
        ));
    }
}

/// `--self-test`: validate sample fragments and (if changie is present) render
/// them in a throwaway workspace to prove the config + render pipeline.
fn run_self_test(root: &Path, cfg: &ChangieConfig, report: &mut Report) {
    let samples = root.join(SAMPLES_DIR);
    let Ok(entries) = std::fs::read_dir(&samples) else {
        report.warn(format!("no samples directory at {}", samples.display()));
        return;
    };
    let mut sample_files: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("yaml"))
        .collect();
    sample_files.sort();

    if sample_files.is_empty() {
        report.warn("no sample fragments found");
        return;
    }

    let mut valid_by_project: BTreeMap<String, Vec<PathBuf>> = BTreeMap::new();
    for path in &sample_files {
        let rel = path.strip_prefix(root).unwrap_or(path).display().to_string();
        match std::fs::read_to_string(path) {
            Ok(content) => match serde_yaml_ng::from_str::<Fragment>(&content) {
                Ok(frag) => {
                    let findings = validate_fragment(&frag, cfg);
                    if findings.is_empty() {
                        report.ok(format!("sample {rel} is schema-valid"));
                        valid_by_project
                            .entry(frag.project.clone())
                            .or_default()
                            .push(path.clone());
                    } else {
                        for f in findings {
                            report.warn(format!("{rel}: {f}"));
                        }
                    }
                }
                Err(e) => report.warn(format!("sample {rel} does not parse: {e}")),
            },
            Err(e) => report.warn(format!("could not read sample {rel}: {e}")),
        }
    }

    if !changie_available() {
        report.info("changie not on PATH; skipping sample render (advisory)");
        return;
    }
    render_samples_in_tempdir(root, &valid_by_project, report);
}

/// Copy the config + samples into a temp workspace and render each project.
fn render_samples_in_tempdir(
    root: &Path,
    by_project: &BTreeMap<String, Vec<PathBuf>>,
    report: &mut Report,
) {
    let tmp = match tempfile::tempdir() {
        Ok(t) => t,
        Err(e) => {
            report.info(format!("could not create temp dir for render self-test: {e}"));
            return;
        }
    };
    let tmp_root = tmp.path();
    let setup = || -> std::io::Result<()> {
        std::fs::copy(root.join(CHANGIE_CONFIG), tmp_root.join(CHANGIE_CONFIG))?;
        let changes = tmp_root.join(".changes");
        std::fs::create_dir_all(changes.join("unreleased"))?;
        std::fs::copy(root.join(".changes/header.tpl.md"), changes.join("header.tpl.md"))?;
        Ok(())
    };
    if let Err(e) = setup() {
        report.info(format!("temp workspace setup failed: {e}"));
        return;
    }

    for (project, files) in by_project {
        if project.is_empty() {
            continue;
        }
        // Refresh the unreleased dir with just this project's samples.
        let unreleased = tmp_root.join(".changes/unreleased");
        let _ = clear_dir(&unreleased);
        for (i, src) in files.iter().enumerate() {
            let dest = unreleased.join(format!("{project}-sample-{i}.yaml"));
            if let Err(e) = std::fs::copy(src, &dest) {
                report.info(format!("could not stage sample for `{project}`: {e}"));
            }
        }
        match render_project(tmp_root, project) {
            Some(rendered) if rendered.contains("pull/") => {
                report
                    .ok(format!("sample render for `{project}` produced a PR-linked change line"));
            }
            Some(rendered) if !rendered.trim().is_empty() => {
                report.ok(format!("sample render for `{project}` produced output"));
            }
            _ => report.info(format!("sample render for `{project}` produced no output")),
        }
    }
}

fn clear_dir(dir: &Path) -> std::io::Result<()> {
    if !dir.exists() {
        return std::fs::create_dir_all(dir);
    }
    for entry in std::fs::read_dir(dir)?.flatten() {
        let _ = std::fs::remove_file(entry.path());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> ChangieConfig {
        let yaml = r#"
projects:
  - key: product
  - key: vscode
components:
  - Developer experience
  - Editor behavior
kinds:
  - label: Added
  - label: Fixed
body:
  minLength: 12
"#;
        serde_yaml_ng::from_str(yaml).expect("test config parses")
    }

    fn good_fragment() -> Fragment {
        let yaml = r#"
project: product
component: Developer experience
kind: Added
body: A sufficiently long changelog body line.
custom:
  PR: "3768"
  Breaking: "no"
"#;
        serde_yaml_ng::from_str(yaml).expect("fragment parses")
    }

    #[test]
    fn valid_fragment_has_no_findings() {
        let cfg = test_config();
        assert!(validate_fragment(&good_fragment(), &cfg).is_empty());
    }

    #[test]
    fn unknown_project_is_flagged() {
        let cfg = test_config();
        let mut frag = good_fragment();
        frag.project = "nope".to_string();
        let findings = validate_fragment(&frag, &cfg);
        assert!(findings.iter().any(|f| f.contains("unknown project")), "{findings:?}");
    }

    #[test]
    fn unknown_kind_is_flagged() {
        let cfg = test_config();
        let mut frag = good_fragment();
        frag.kind = "Frobnicated".to_string();
        let findings = validate_fragment(&frag, &cfg);
        assert!(findings.iter().any(|f| f.contains("unknown kind")), "{findings:?}");
    }

    #[test]
    fn short_body_is_flagged() {
        let cfg = test_config();
        let mut frag = good_fragment();
        frag.body = "short".to_string();
        let findings = validate_fragment(&frag, &cfg);
        assert!(findings.iter().any(|f| f.contains("body too short")), "{findings:?}");
    }

    #[test]
    fn missing_pr_is_flagged() {
        let cfg = test_config();
        let mut frag = good_fragment();
        frag.custom.remove("PR");
        let findings = validate_fragment(&frag, &cfg);
        assert!(findings.iter().any(|f| f.contains("PR")), "{findings:?}");
    }

    #[test]
    fn non_numeric_pr_is_flagged() {
        let cfg = test_config();
        let mut frag = good_fragment();
        frag.custom.insert("PR".to_string(), "abc".to_string());
        let findings = validate_fragment(&frag, &cfg);
        assert!(findings.iter().any(|f| f.contains("positive integer")), "{findings:?}");
    }

    #[test]
    fn invalid_breaking_is_flagged() {
        let cfg = test_config();
        let mut frag = good_fragment();
        frag.custom.insert("Breaking".to_string(), "maybe".to_string());
        let findings = validate_fragment(&frag, &cfg);
        assert!(findings.iter().any(|f| f.contains("Breaking")), "{findings:?}");
    }

    #[test]
    fn unknown_component_is_flagged() {
        let cfg = test_config();
        let mut frag = good_fragment();
        frag.component = "Nonexistent area".to_string();
        let findings = validate_fragment(&frag, &cfg);
        assert!(findings.iter().any(|f| f.contains("unknown component")), "{findings:?}");
    }

    #[test]
    fn kind_matches_case_insensitively() {
        let cfg = test_config();
        let mut frag = good_fragment();
        frag.kind = "added".to_string();
        assert!(validate_fragment(&frag, &cfg).is_empty());
    }

    #[test]
    fn fragment_files_filters_unreleased_yaml() {
        let changed = vec![
            ".changes/unreleased/product-1-Added-101010.yaml".to_string(),
            ".changes/unreleased/.gitkeep".to_string(),
            "src/lib.rs".to_string(),
            ".changes/samples/product-example.yaml".to_string(),
        ];
        let frags = fragment_files(&changed);
        assert_eq!(frags, vec![".changes/unreleased/product-1-Added-101010.yaml".to_string()]);
    }

    #[test]
    fn disposition_prefers_fragment() {
        let changed = vec![".changes/unreleased/product-1-Added-101010.yaml".to_string()];
        assert!(matches!(detect_disposition(&changed, ""), Disposition::Fragment(_)));
    }

    #[test]
    fn disposition_reads_pr_body_marker() {
        let changed = vec!["src/lib.rs".to_string()];
        let body = "This PR does X.\nchangelog-exempt: refactor — internal cleanup only\n";
        match detect_disposition(&changed, body) {
            Disposition::Exemption(reason) => {
                assert!(reason.starts_with("refactor:"), "{reason}");
                assert!(reason.contains("internal cleanup"), "{reason}");
            }
            other => panic!("expected exemption, got {other:?}"),
        }
    }

    #[test]
    fn disposition_recognizes_release_prep() {
        let changed = vec!["CHANGELOG.md".to_string(), "Cargo.toml".to_string()];
        assert_eq!(detect_disposition(&changed, ""), Disposition::ReleasePrep);
    }

    #[test]
    fn disposition_recognizes_exemption_file() {
        let changed = vec![".changes/exemptions/my-note.md".to_string()];
        assert!(matches!(detect_disposition(&changed, ""), Disposition::Exemption(_)));
    }

    #[test]
    fn disposition_missing_when_nothing_declared() {
        let changed = vec!["crates/perl-parser/src/lib.rs".to_string()];
        assert_eq!(detect_disposition(&changed, ""), Disposition::Missing);
    }

    #[test]
    fn none_marker_is_accepted() {
        let body = "changelog: none — trivial typo fix";
        let parsed = parse_exemption_marker(body);
        assert!(parsed.is_some());
        let (cat, reason) = parsed.expect("parsed");
        assert_eq!(cat, "none");
        assert!(reason.contains("typo"), "{reason}");
    }

    #[test]
    fn direct_edit_warns_for_feature_pr() {
        let changed = vec!["CHANGELOG.md".to_string(), "crates/perl-parser/src/lib.rs".to_string()];
        assert!(direct_changelog_edit_warning(&changed, false).is_some());
    }

    #[test]
    fn direct_edit_silent_for_release_prep() {
        let changed = vec!["CHANGELOG.md".to_string()];
        assert!(direct_changelog_edit_warning(&changed, true).is_none());
    }

    #[test]
    fn exempt_category_for_path_maps_known_paths() {
        assert_eq!(exempt_category_for_path(".github/workflows/x.yml"), Some("ci"));
        assert_eq!(exempt_category_for_path("crates/x/tests/a.rs"), Some("tests"));
        assert_eq!(exempt_category_for_path("Cargo.lock"), Some("deps"));
        assert_eq!(exempt_category_for_path(".changie.yaml"), Some("changelog-tooling"));
        assert_eq!(
            exempt_category_for_path("docs/project/status/lsp.md"),
            Some("generated-status")
        );
        assert_eq!(exempt_category_for_path("CHANGELOG.md"), None);
    }
}

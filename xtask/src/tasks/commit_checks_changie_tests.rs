use super::config::AQUA_CONFIG_PATH;
use super::*;
use color_eyre::eyre::{Context, Result, bail};
use std::fs;
use std::path::Path;
use std::process::Command;

const CONFIG: &str = "changesDir: .changes\nunreleasedDir: unreleased\nheaderPath: header.tpl.md\nprojects:\n  - key: product\n    changelog: CHANGELOG.md\ncomponents:\n  - Developer experience\nkinds:\n  - label: Fixed\nbody:\n  minLength: 12\n";
const AQUA_CONFIG: &str = "packages:\n  - name: miniscruff/changie@v1.25.0\n";

struct TempRepo {
    dir: tempfile::TempDir,
}

impl TempRepo {
    fn git(&self) -> Command {
        let mut command = Command::new("git");
        command
            .current_dir(self.root())
            .env("GIT_CONFIG_GLOBAL", self.root().join("missing-global-config"))
            .env("GIT_CONFIG_SYSTEM", self.root().join("missing-system-config"))
            .env("GIT_CONFIG_NOSYSTEM", "1");
        command
    }

    fn init() -> Result<Self> {
        let dir = tempfile::tempdir().context("failed to create temp repo")?;
        let repo = Self { dir };
        for args in [
            vec!["init", "--quiet"],
            vec!["config", "user.email", "test@example.com"],
            vec!["config", "user.name", "Test"],
        ] {
            let status =
                repo.git().args(&args).status().context("failed to configure temp git repo")?;
            if !status.success() {
                bail!("git command {args:?} failed");
            }
        }
        Ok(repo)
    }

    fn root(&self) -> &Path {
        self.dir.path()
    }

    fn write(&self, path: &str, content: &str) -> Result<()> {
        let destination = self.root().join(path);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(destination, content)?;
        Ok(())
    }

    fn add(&self, path: &str) -> Result<()> {
        let status = self.git().args(["add", path]).status().context("failed to stage fixture")?;
        if !status.success() {
            bail!("git add {path} failed");
        }
        Ok(())
    }

    fn commit(&self) -> Result<()> {
        let status = self
            .git()
            .args(["commit", "--quiet", "--no-verify", "-m", "baseline"])
            .status()
            .context("failed to commit fixture")?;
        if !status.success() {
            bail!("git commit failed");
        }
        Ok(())
    }

    fn stage_baseline(&self, config: &str) -> Result<()> {
        self.write(CONFIG_PATH, config)?;
        self.write(AQUA_CONFIG_PATH, AQUA_CONFIG)?;
        self.write(".changes/header.tpl.md", "# Changelog\n")?;
        self.write("CHANGELOG.md", "# Changelog\n")?;
        for path in [CONFIG_PATH, AQUA_CONFIG_PATH, ".changes/header.tpl.md", "CHANGELOG.md"] {
            self.add(path)?;
        }
        self.commit()
    }
}

fn fragment(body: &str) -> String {
    format!(
        "project: product\ncomponent: Developer experience\nkind: Fixed\nbody: \"{body}\"\ncustom:\n  PR: \"1\"\n  Breaking: \"no\"\n"
    )
}

#[test]
fn dry_render_materializes_the_captured_tree_not_unstaged_edits() -> Result<()> {
    let repo = TempRepo::init()?;
    repo.stage_baseline(CONFIG)?;
    let fragment_path = ".changes/unreleased/product-1-Fixed-000000.yaml";
    repo.write(fragment_path, &fragment("staged release note body"))?;
    repo.add(fragment_path)?;
    let tree_oid = staged::staged_tree_oid(repo.root())?;

    repo.write(CONFIG_PATH, "projects: []\n")?;
    repo.write(AQUA_CONFIG_PATH, "packages: []\n")?;
    repo.write(fragment_path, &fragment("unstaged replacement body"))?;

    let outcome = run_with_renderer(repo.root(), Some(&tree_oid), |workspace, projects| {
        assert_eq!(projects, &["product".to_string()]);
        let staged_config = fs::read_to_string(workspace.join(CONFIG_PATH))?;
        assert_eq!(staged_config, CONFIG);
        let staged_aqua = fs::read_to_string(workspace.join(AQUA_CONFIG_PATH))?;
        assert_eq!(staged_aqua, AQUA_CONFIG);
        let staged_fragment = fs::read_to_string(workspace.join(fragment_path))?;
        assert!(
            staged_fragment.contains("staged release note body"),
            "sandbox fragment must carry staged content: {staged_fragment}"
        );
        assert!(
            !staged_fragment.contains("unstaged replacement body"),
            "sandbox fragment must exclude unstaged content: {staged_fragment}"
        );
        Ok(RenderOutcome::Passed)
    })?;

    match outcome {
        CommitCheckOutcome::Pass(summary) => assert!(
            summary.contains("dry-render"),
            "pass summary must identify the dry-render: {summary}"
        ),
        CommitCheckOutcome::Flagged(report) => {
            bail!("expected captured staged inputs to pass: {report:?}");
        }
    }
    Ok(())
}

#[test]
fn changie_rejection_is_a_blocking_staged_input_finding() -> Result<()> {
    let repo = TempRepo::init()?;
    repo.stage_baseline(CONFIG)?;
    let fragment_path = ".changes/unreleased/product-1-Fixed-000000.yaml";
    repo.write(fragment_path, &fragment("valid release note body"))?;
    repo.add(fragment_path)?;

    let outcome = run_with_renderer(repo.root(), None, |_workspace, _projects| {
        Ok(RenderOutcome::Rejected(vec!["template execution failed".to_string()]))
    })?;
    match outcome {
        CommitCheckOutcome::Flagged(report) => {
            assert_eq!(report.posture, Posture::Blocked);
            assert!(
                report.result.contains("template execution failed"),
                "blocking report must preserve renderer failure: {}",
                report.result
            );
            assert!(
                report.fix.as_deref().is_some_and(|fix| fix.contains("cargo change")),
                "blocking report must provide the cargo change repair: {:?}",
                report.fix
            );
        }
        CommitCheckOutcome::Pass(summary) => {
            bail!("expected Changie rejection to block: {summary}");
        }
    }
    Ok(())
}

#[test]
fn configured_changes_and_unreleased_directories_drive_materialization() -> Result<()> {
    let repo = TempRepo::init()?;
    let config = CONFIG
        .replace("changesDir: .changes", "changesDir: notes")
        .replace("unreleasedDir: unreleased", "unreleasedDir: pending");
    repo.write(CONFIG_PATH, &config)?;
    repo.write(AQUA_CONFIG_PATH, AQUA_CONFIG)?;
    repo.write("notes/header.tpl.md", "# Changelog\n")?;
    repo.write("CHANGELOG.md", "# Changelog\n")?;
    for path in [CONFIG_PATH, AQUA_CONFIG_PATH, "notes/header.tpl.md", "CHANGELOG.md"] {
        repo.add(path)?;
    }
    repo.commit()?;

    let fragment_path = "notes/pending/product-1-Fixed-000000.yaml";
    repo.write(fragment_path, &fragment("configured directory body"))?;
    repo.add(fragment_path)?;

    let outcome = run_with_renderer(repo.root(), None, |workspace, _projects| {
        assert!(
            workspace.join(fragment_path).is_file(),
            "configured fragment must be materialized at {fragment_path}"
        );
        Ok(RenderOutcome::Passed)
    })?;
    match outcome {
        CommitCheckOutcome::Pass(_) => {}
        CommitCheckOutcome::Flagged(report) => {
            bail!("expected configured Changie directories to pass: {report:?}");
        }
    }
    Ok(())
}

#[test]
fn unsafe_config_paths_block_before_rendering() -> Result<()> {
    let repo = TempRepo::init()?;
    repo.stage_baseline(CONFIG)?;
    repo.write(CONFIG_PATH, &CONFIG.replace("changesDir: .changes", "changesDir: ../outside"))?;
    repo.add(CONFIG_PATH)?;

    let outcome = run_with_renderer(repo.root(), None, |_workspace, _projects| {
        bail!("renderer must not run for an unsafe staged config path")
    })?;
    match outcome {
        CommitCheckOutcome::Flagged(report) => {
            assert_eq!(report.posture, Posture::Blocked);
            assert!(
                report.result.contains("must not escape"),
                "unsafe path report must explain containment failure: {}",
                report.result
            );
        }
        CommitCheckOutcome::Pass(summary) => {
            bail!("expected unsafe Changie path to block: {summary}");
        }
    }
    Ok(())
}

use super::config::AQUA_CONFIG_PATH;
use super::*;
use color_eyre::eyre::{Context, ContextCompat, Result, bail};
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

    /// Stage `content` as a blob at `path` with an explicit git `mode`
    /// (e.g. `120000` symlink) via `hash-object -w` + `update-index
    /// --cacheinfo` — a type-change recorded purely in the index, with no
    /// working-tree symlink required.
    fn stage_blob_at(&self, mode: &str, content: &str, path: &str) -> Result<()> {
        use std::io::Write;
        use std::process::Stdio;

        let mut child = self
            .git()
            .args(["hash-object", "-w", "--stdin"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .context("failed to spawn `git hash-object -w --stdin`")?;
        child
            .stdin
            .take()
            .context("git hash-object stdin was not piped")?
            .write_all(content.as_bytes())
            .context("failed to write blob content to git hash-object")?;
        let output = child.wait_with_output().context("failed to wait for git hash-object")?;
        if !output.status.success() {
            bail!("git hash-object failed: {}", String::from_utf8_lossy(&output.stderr));
        }
        let blob = String::from_utf8(output.stdout)
            .context("git hash-object output was not UTF-8")?
            .trim()
            .to_string();
        let spec = format!("{mode},{blob},{path}");
        let status = self
            .git()
            .args(["update-index", "--add", "--cacheinfo", &spec])
            .status()
            .context("failed to run git update-index --cacheinfo")?;
        if !status.success() {
            bail!("git update-index --add --cacheinfo {spec} failed");
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

    /// Stage a deletion of an already-committed path (`git rm --cached`)
    /// without touching the working tree — the exact shape of a deletion
    /// entering the staged tree and its diff filter.
    fn remove_cached(&self, path: &str) -> Result<()> {
        let status = self
            .git()
            .args(["rm", "--cached", "--quiet", path])
            .status()
            .context("failed to unstage fixture path")?;
        if !status.success() {
            bail!("git rm --cached {path} failed");
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
        "project: product\ncomponent: Developer experience\nkind: Fixed\nbody: \"{body}\"\ntime: 2026-08-30T00:00:00Z\ncustom:\n  PR: \"1\"\n  Breaking: \"no\"\n"
    )
}

/// Issue #13484: a hand-authored fragment with an empty `time:` and the
/// HH:MM:SS orphaned as bare YAML (the #12549/#12648 signature) must block the
/// commit-tier gate with a finding naming the fragment and the defect — before
/// it can land and crash `changie batch` repo-wide at render time.
#[test]
fn empty_or_orphaned_time_blocks_the_staged_gate() -> Result<()> {
    for (label, fragment_text) in [
        (
            "empty time",
            "project: product\ncomponent: Developer experience\nkind: Fixed\nbody: \"valid release note body\"\ntime:\ncustom:\n  PR: \"1\"\n  Breaking: \"no\"\n",
        ),
        (
            "orphaned bare time",
            "project: product\ncomponent: Developer experience\nkind: Fixed\nbody: \"valid release note body\"\ntime:\n  13:55:13\ncustom:\n  PR: \"1\"\n  Breaking: \"no\"\n",
        ),
    ] {
        let repo = TempRepo::init()?;
        repo.stage_baseline(CONFIG)?;
        let fragment_path = ".changes/unreleased/product-1-Fixed-000000.yaml";
        repo.write(fragment_path, fragment_text)?;
        repo.add(fragment_path)?;

        let outcome = run_with_renderer(repo.root(), None, |_workspace, _projects| {
            bail!("renderer must not run while a malformed fragment is staged")
        })?;
        match outcome {
            CommitCheckOutcome::Flagged(report) => {
                assert_eq!(report.posture, Posture::Blocked, "{label}");
                assert!(
                    report.result.contains("`time:`"),
                    "{label}: blocking report must name the time defect: {}",
                    report.result
                );
                assert!(
                    report.result.contains(fragment_path),
                    "{label}: blocking report must name the fragment: {}",
                    report.result
                );
            }
            CommitCheckOutcome::Pass(summary) => {
                bail!("expected an empty/orphaned `time:` fragment to block: {label}: {summary}");
            }
        }
    }
    Ok(())
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
fn changie_only_staged_input_reports_its_path_in_the_summary() -> Result<()> {
    let repo = TempRepo::init()?;
    repo.stage_baseline(CONFIG)?;
    repo.write("CHANGELOG.md", "# Changelog\n\n## Pending\n")?;
    repo.add("CHANGELOG.md")?;

    let outcome = run_with_renderer(repo.root(), None, |workspace, projects| {
        assert_eq!(
            projects,
            &["product".to_string()],
            "Changie-only input must still render every configured project"
        );
        assert_eq!(
            fs::read_to_string(workspace.join("CHANGELOG.md"))?,
            "# Changelog\n\n## Pending\n",
            "the renderer sandbox must contain the staged changelog"
        );
        Ok(RenderOutcome::Passed)
    })?;

    match outcome {
        CommitCheckOutcome::Pass(summary) => {
            assert!(
                summary.contains("Changie dry-render passed"),
                "pass summary must identify the successful dry-render: {summary}"
            );
            assert!(
                summary.contains("staged inputs: CHANGELOG.md"),
                "pass summary must name the staged Changie input: {summary}"
            );
            assert!(
                !summary.contains("fragment"),
                "a changelog-only summary must not claim a fragment count: {summary}"
            );
        }
        CommitCheckOutcome::Flagged(report) => {
            bail!("expected a changelog-only Changie input to pass: {report:?}");
        }
    }
    Ok(())
}

#[test]
fn multiple_staged_changie_inputs_are_not_reduced_to_a_fragment_count() -> Result<()> {
    let repo = TempRepo::init()?;
    repo.stage_baseline(CONFIG)?;
    let first_fragment = ".changes/unreleased/product-1-Fixed-000000.yaml";
    let second_fragment = ".changes/unreleased/product-2-Fixed-000000.yaml";
    repo.write(first_fragment, &fragment("first release note body"))?;
    repo.write(second_fragment, &fragment("second release note body"))?;
    repo.write("CHANGELOG.md", "# Changelog\n\n## Pending\n")?;
    for path in [first_fragment, second_fragment, "CHANGELOG.md"] {
        repo.add(path)?;
    }

    let outcome = run_with_renderer(repo.root(), None, |workspace, projects| {
        assert_eq!(
            projects,
            &["product".to_string()],
            "all staged Changie inputs must still render every configured project"
        );
        for path in [first_fragment, second_fragment, "CHANGELOG.md"] {
            assert!(
                workspace.join(path).is_file(),
                "every staged Changie input must be materialized: {path}"
            );
        }
        Ok(RenderOutcome::Passed)
    })?;

    match outcome {
        CommitCheckOutcome::Pass(summary) => {
            assert!(
                summary.contains("staged inputs:")
                    && summary.contains(first_fragment)
                    && summary.contains(second_fragment)
                    && summary.contains("CHANGELOG.md"),
                "pass summary must identify all three staged Changie inputs, not only the two fragments: {summary}"
            );
            assert!(
                !summary.contains("2 staged Changie fragment"),
                "pass summary must not misrepresent the three-input render as a fragment count: {summary}"
            );
        }
        CommitCheckOutcome::Flagged(report) => {
            bail!("expected multiple staged Changie inputs to pass: {report:?}");
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

// ---------------------------------------------------------------------
// Issue #4092 gap 1 proofs: config removal is an explicit policy
// finding, never a clean pass and never an instrument failure, while
// ordinary fragment deletion beside a valid config stays a pass.
// ---------------------------------------------------------------------

/// Required proof 1: a candidate tree that deliberately deletes
/// `.changie.yaml` is a Changie policy finding (Blocked), and the check
/// never reaches a pass or a renderer invocation.
#[test]
fn deleting_only_the_changie_config_is_a_policy_finding_not_a_pass() -> Result<()> {
    let repo = TempRepo::init()?;
    repo.stage_baseline(CONFIG)?;
    repo.remove_cached(CONFIG_PATH)?;

    let outcome = run_with_renderer(repo.root(), None, |_workspace, _projects| {
        bail!("renderer must not run when the Changie config itself is absent")
    })?;
    match outcome {
        CommitCheckOutcome::Flagged(report) => {
            assert_eq!(
                report.posture,
                Posture::Blocked,
                "config removal must be a policy BLOCK, not {:#?}",
                report.posture
            );
            assert!(
                report.result.contains(CONFIG_PATH),
                "the policy finding must name the removed config: {}",
                report.result
            );
        }
        CommitCheckOutcome::Pass(summary) => {
            bail!(
                "deleting `.changie.yaml` must not silently pass while Changie is a mandatory \
                 authority: {summary}"
            );
        }
    }
    Ok(())
}

/// Required proof 2: deleting the config together with every unreleased
/// fragment is still a policy finding — "all fragments were also deleted"
/// is not authority to infer a decommissioning.
#[test]
fn deleting_config_and_all_fragments_still_blocks_instead_of_decommissioning() -> Result<()> {
    let repo = TempRepo::init()?;
    repo.stage_baseline(CONFIG)?;
    let fragment_path = ".changes/unreleased/product-1-Fixed-000000.yaml";
    repo.write(fragment_path, &fragment("committed fragment body"))?;
    repo.add(fragment_path)?;
    repo.commit()?;
    repo.remove_cached(CONFIG_PATH)?;
    repo.remove_cached(fragment_path)?;

    let outcome = run_with_renderer(repo.root(), None, |_workspace, _projects| {
        bail!("renderer must not run for a config-free candidate tree")
    })?;
    match outcome {
        CommitCheckOutcome::Flagged(report) => {
            assert_eq!(
                report.posture,
                Posture::Blocked,
                "config+fragment deletion must stay a policy BLOCK, not {:#?}",
                report.posture
            );
            assert!(
                report.result.contains(CONFIG_PATH),
                "the empty-ledger deletion must still be attributed to the missing config: {}",
                report.result
            );
        }
        CommitCheckOutcome::Pass(summary) => {
            bail!(
                "an all-deletions candidate must not be normalized into a clean decommission \
                 pass: {summary}"
            );
        }
    }
    Ok(())
}

/// Required proof 4: an ordinary fragment deletion while a valid config
/// remains is handled as a no-op for the release-note ledger and passes.
#[test]
fn ordinary_fragment_deletion_with_valid_config_passes() -> Result<()> {
    let repo = TempRepo::init()?;
    repo.stage_baseline(CONFIG)?;
    let fragment_path = ".changes/unreleased/product-1-Fixed-000000.yaml";
    repo.write(fragment_path, &fragment("committed fragment body"))?;
    repo.add(fragment_path)?;
    repo.commit()?;

    repo.remove_cached(fragment_path)?;

    let outcome = run_with_renderer(repo.root(), None, |_workspace, projects| {
        assert_eq!(
            projects,
            &["product".to_string()],
            "a fragment deletion must still render every configured project"
        );
        Ok(RenderOutcome::Passed)
    })?;
    match outcome {
        CommitCheckOutcome::Pass(summary) => assert!(
            summary.contains("dry-render"),
            "a fragment-only deletion must pass through the normal dry-render: {summary}"
        ),
        CommitCheckOutcome::Flagged(report) => {
            bail!("an ordinary fragment deletion beside a valid config must pass: {report:?}");
        }
    }
    Ok(())
}

/// Required proof 5 (gap 2, end-to-end): a regular-file -> symlink
/// type-change on a Changie input is part of the staged set and is
/// REJECTED by the owning check with its recorded `120000` mode — the
/// symlink blob (a link target that could point anywhere) is never read as
/// ordinary valid text and never materialized into the render sandbox.
/// Uses `aqua.yaml` (a non-fragment Changie input) so the dispatch reaches
/// the materialization loop's mode gate.
#[test]
fn type_changed_changie_input_blocks_before_materialization() -> Result<()> {
    let repo = TempRepo::init()?;
    repo.stage_baseline(CONFIG)?;

    // Regular file -> symlink recorded purely in the index: the blob
    // content is the link target, `../..`-relative to escape the repository
    // if any consumer ever treated it as text.
    repo.stage_blob_at("120000", "../../../outside/secrets", AQUA_CONFIG_PATH)?;

    let outcome = run_with_renderer(repo.root(), None, |_workspace, _projects| {
        bail!("renderer must not run for a type-changed Changie input")
    })?;
    match outcome {
        CommitCheckOutcome::Flagged(report) => {
            assert_eq!(
                report.posture,
                Posture::Blocked,
                "a type-changed Changie input must block, not {:#?}",
                report.posture
            );
            assert!(
                report.result.contains("unsupported staged mode 120000"),
                "the finding must name the recorded type-change mode: {}",
                report.result
            );
            assert!(
                report.affected.iter().any(|path| path == AQUA_CONFIG_PATH),
                "the finding must point at the type-changed input: {:?}",
                report.affected
            );
        }
        CommitCheckOutcome::Pass(summary) => {
            bail!("a symlinked Changie input must not be validated as text: {summary}");
        }
    }
    Ok(())
}

/// The other half of required proof 5: a type-changed FRAGMENT cannot
/// become clean either. Its symlink blob is link-target bytes, not
/// fragment YAML, so it must surface as a blocking content finding — never
/// silently skipped because the path "exists" in neither text shape.
#[test]
fn type_changed_fragment_cannot_become_clean() -> Result<()> {
    let repo = TempRepo::init()?;
    repo.stage_baseline(CONFIG)?;
    let fragment_path = ".changes/unreleased/product-1-Fixed-000000.yaml";
    repo.write(fragment_path, &fragment("soon type-changed body"))?;
    repo.add(fragment_path)?;
    repo.commit()?;

    repo.stage_blob_at("120000", "../../../outside/secrets", fragment_path)?;

    let outcome = run_with_renderer(repo.root(), None, |_workspace, _projects| {
        bail!("renderer must not run for a type-changed fragment")
    })?;
    match outcome {
        CommitCheckOutcome::Flagged(report) => {
            assert_eq!(
                report.posture,
                Posture::Blocked,
                "a type-changed fragment must block, not {:#?}",
                report.posture
            );
            assert!(
                report.result.contains(fragment_path),
                "the blocking finding must name the type-changed fragment: {}",
                report.result
            );
        }
        CommitCheckOutcome::Pass(summary) => {
            bail!("a type-changed fragment must not pass content validation: {summary}");
        }
    }
    Ok(())
}

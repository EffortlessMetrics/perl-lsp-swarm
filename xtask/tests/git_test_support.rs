//! Shared Git-backed test fixtures and the repository's hermetic fixture
//! contract (#13697).
//!
//! Git object, tree, diff, and receipt identities are only evidence when the
//! fixture controls every Git input that can change them. This module is the
//! narrowest reusable test seam for that control. It deliberately does not
//! become a production Git abstraction.
//!
//! Hermetic contract enforced here:
//!
//! - `GIT_CONFIG_NOSYSTEM=1`, `GIT_ATTR_NOSYSTEM=1`, and
//!   `GIT_CONFIG_GLOBAL`/`GIT_CONFIG_SYSTEM`
//!   point at caller-owned empty or pinned fixture files, so
//!   system and global configuration, aliases, hooks paths, templates,
//!   attributes, filters, signing, and object-format defaults cannot leak in;
//! - command-scoped `GIT_CONFIG_COUNT`/`GIT_CONFIG_KEY_*`/`GIT_CONFIG_VALUE_*`
//!   pairs and other ambient `GIT_*` variables are scrubbed;
//! - author/committer timestamps are pinned (`FIXTURE_TIMESTAMP`);
//! - locale and timezone are pinned (`LC_ALL=C`, `LANG=C`, `TZ=UTC`);
//! - interactive prompts are disabled (`GIT_TERMINAL_PROMPT=0`);
//! - repository-local configuration pins identity, `commit.gpgsign=false`,
//!   `core.autocrlf=false`, `core.fileMode=false`, and
//!   `init --object-format=sha1` so 40-character identities stay stable;
//! - failures are typed and carry argv, cwd, stderr, and stdout instead of
//!   silently falling back to the caller's Git environment.
//!
//! Fixture classification (issue #13697 inventory):
//!
//! - identity-pinning, migrated to [`HermeticGit`]: `xtask/tests/ci_subject.rs`
//!   and `xtask/tests/git_ancestry_cli.rs`;
//! - deliberately hostile (opt-in pins prove refusal paths):
//!   `xtask/tests/git_fixture_hermeticity.rs`;
//! - identity-insensitive, hermetic via the free [`git_cmd`] seam (they assert
//!   command behavior, never commit/tree identities): `check_file_policy.rs`
//!   and `non_rust_propose.rs`;
//! - identity-insensitive with their own local fixtures (not migrated; they
//!   already pin identity/signing or scope `GIT_CONFIG_*` where needed):
//!   `ci_pr_summary.rs`, `pr_title_check.rs`, `freshness_check.rs`,
//!   `worktree_cleanup_cli.rs`, `worktree_forensic_recovery.rs`,
//!   `xtask/src/tasks/commit_checks_changie_tests.rs`,
//!   `xtask/src/tasks/module_train_tests.rs`.

// The module is compiled separately into every integration-test target, and
// each target consumes a different subset of the shared contract; per-target
// dead-code analysis would otherwise warn on the unused subsets.
#![allow(dead_code)]

use anyhow::{Context, Result, bail};
use assert_cmd::Command as AssertCommand;
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command as StdCommand, Output},
};

/// Pinned author/committer timestamp shared by identity-pinning fixtures.
pub const FIXTURE_TIMESTAMP: &str = "2026-08-27T12:00:00Z";

/// Ambient environment variables honored by Git that must not reach a fixture.
const AMBIENT_GIT_ENV: &[&str] = &[
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_COMMON_DIR",
    "GIT_GRAFT_FILE",
    "GIT_NAMESPACE",
    "GIT_TEMPLATE_DIR",
    "GIT_DEFAULT_HASH",
    "GIT_DEFAULT_REF_FORMAT",
    "GIT_CONFIG",
    "GIT_AUTHOR_NAME",
    "GIT_AUTHOR_EMAIL",
    "GIT_AUTHOR_DATE",
    "GIT_COMMITTER_NAME",
    "GIT_COMMITTER_EMAIL",
    "GIT_COMMITTER_DATE",
    // Removing GIT_CONFIG_COUNT disables every GIT_CONFIG_KEY_n/VALUE_n pair.
    "GIT_CONFIG_COUNT",
    "GIT_CONFIG_PARAMETERS",
    "GIT_EXTERNAL_DIFF",
    "GIT_EDITOR",
    "GIT_SEQUENCE_EDITOR",
    "GIT_SSH",
    "GIT_SSH_COMMAND",
    "GIT_ASKPASS",
    "GIT_TERMINAL_PROMPT",
    "EDITOR",
    "VISUAL",
    "TZ",
    "LC_ALL",
    "LANG",
];

/// Renders a path as a Git configuration value. Git's INI parser treats
/// backslashes as escape sequences, so configuration values must use the
/// forward-slash form Git accepts on every platform.
pub fn config_path_value(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn scrub_ambient_git_env(cmd: &mut StdCommand) {
    for key in AMBIENT_GIT_ENV {
        cmd.env_remove(key);
    }
}

fn hermetic_env_pairs(global: &Path, system: &Path) -> Vec<(&'static str, String)> {
    vec![
        ("GIT_CONFIG_NOSYSTEM", "1".to_string()),
        ("GIT_ATTR_NOSYSTEM", "1".to_string()),
        ("GIT_CONFIG_GLOBAL", global.to_string_lossy().into_owned()),
        ("GIT_CONFIG_SYSTEM", system.to_string_lossy().into_owned()),
        ("GIT_AUTHOR_DATE", FIXTURE_TIMESTAMP.to_string()),
        ("GIT_COMMITTER_DATE", FIXTURE_TIMESTAMP.to_string()),
        ("GIT_TERMINAL_PROMPT", "0".to_string()),
        ("TZ", "UTC".to_string()),
        ("LC_ALL", "C".to_string()),
        ("LANG", "C".to_string()),
    ]
}

fn apply_hermetic_config(cmd: &mut StdCommand, global: &Path, system: &Path) {
    scrub_ambient_git_env(cmd);
    for (key, value) in hermetic_env_pairs(global, system) {
        cmd.env(key, value);
    }
}

fn fail_typed(args: &[&str], cwd: Option<&Path>, output: &Output) -> String {
    format!(
        "git {} failed in {}\nstatus: {:?}\nstderr:\n{}\nstdout:\n{}",
        args.join(" "),
        cwd.map(Path::display).unwrap_or(Path::new(".").display()),
        output.status.code(),
        String::from_utf8_lossy(&output.stderr).trim(),
        String::from_utf8_lossy(&output.stdout).trim(),
    )
}

/// A hermetic Git fixture harness owning pinned global/system configuration
/// files under a caller-owned base directory.
///
/// Every Git invocation built from this harness sees the same pinned inputs
/// regardless of the invoking machine's configuration.
pub struct HermeticGit {
    pinned_global: PathBuf,
    pinned_system: PathBuf,
}

impl HermeticGit {
    /// Creates pinned configuration files under `base` and returns the
    /// harness. `base` should live inside the caller's temporary directory.
    pub fn at(base: &Path) -> Result<Self> {
        Self::with_pins(base, &[])
    }

    /// Like [`HermeticGit::at`], but additionally pins `keys` into the pinned
    /// global configuration. This is the explicit opt-in for deliberately
    /// hostile tests; hostile values never arrive by inheritance.
    pub fn with_pins(base: &Path, pins: &[(&str, &str)]) -> Result<Self> {
        fs::create_dir_all(base).with_context(|| {
            format!("failed to create fixture pin directory {}", base.display())
        })?;
        let pinned_attributes = base.join("pinned-attributes");
        fs::write(&pinned_attributes, "")
            .with_context(|| format!("failed to write {}", pinned_attributes.display()))?;
        let pinned_system = base.join("pinned-system-config");
        fs::write(&pinned_system, "")
            .with_context(|| format!("failed to write {}", pinned_system.display()))?;
        let mut global = format!(
            "[user]\n\tname = Fixture User\n\temail = fixture@example.invalid\n\
             [commit]\n\tgpgsign = false\n\
             [tag]\n\tgpgsign = false\n\
             [init]\n\tdefaultBranch = main\n\tdefaultObjectFormat = sha1\n\
             [core]\n\tautocrlf = false\n\tfileMode = false\n\tsymlinks = false\n\tattributesFile = {}\n\
             [gc]\n\tauto = 0\n",
            config_path_value(&pinned_attributes),
        );
        for (key, value) in pins {
            let (section, name) =
                key.split_once('.').context("fixture pin key must look like section.name")?;
            global.push_str(&format!("\n[{section}]\n\t{name} = {value}\n"));
        }
        let pinned_global = base.join("pinned-global-config");
        fs::write(&pinned_global, global)
            .with_context(|| format!("failed to write {}", pinned_global.display()))?;
        Ok(Self { pinned_global, pinned_system })
    }

    /// Applies the hermetic environment to any spawned command, including
    /// non-git programs that read Git-backed fixtures. Later `env` calls on
    /// the same command would win, so apply this last.
    pub fn apply_env(&self, cmd: &mut StdCommand) {
        apply_hermetic_config(cmd, &self.pinned_global, &self.pinned_system);
    }

    /// [`HermeticGit::apply_env`] for an `assert_cmd::Command` child such as
    /// `cargo_bin_cmd!` binaries that read Git-backed fixtures.
    pub fn apply_env_to_assert(&self, cmd: &mut AssertCommand) {
        for key in AMBIENT_GIT_ENV {
            cmd.env_remove(key);
        }
        for (key, value) in hermetic_env_pairs(&self.pinned_global, &self.pinned_system) {
            cmd.env(key, value);
        }
    }

    /// Runs a Git command and returns its captured output.
    pub fn git_output(&self, repo: &Path, args: &[&str]) -> Result<Output> {
        let mut cmd = StdCommand::new("git");
        cmd.args(args).current_dir(repo);
        self.apply_env(&mut cmd);
        cmd.output().with_context(|| {
            format!("git {} failed to start in {}", args.join(" "), repo.display())
        })
    }

    /// Runs a Git command, returning trimmed stdout. Failures are typed and
    /// carry argv, cwd, stderr, and stdout.
    pub fn git(&self, repo: &Path, args: &[&str]) -> Result<String> {
        let output = self.git_output(repo, args)?;
        if !output.status.success() {
            bail!("{}", fail_typed(args, Some(repo), &output));
        }
        let stdout =
            String::from_utf8(output.stdout).context("git command returned non-UTF-8 output")?;
        Ok(stdout.trim().to_string())
    }

    /// Initializes `repo` as a hermetic 40-character SHA-1 repository on the
    /// `main` branch and pins repository-local configuration. Local pins
    /// outrank even hostile allowlisted global configuration.
    pub fn init_repo(&self, repo: &Path) -> Result<()> {
        fs::create_dir_all(repo)
            .with_context(|| format!("failed to create fixture repository {}", repo.display()))?;
        self.git(repo, &["init", "--initial-branch=main", "--object-format=sha1"])?;
        for (key, value) in [
            ("user.name", "Fixture User"),
            ("user.email", "fixture@example.invalid"),
            ("commit.gpgsign", "false"),
            ("core.autocrlf", "false"),
            ("core.fileMode", "false"),
        ] {
            self.git(repo, &["config", key, value])?;
        }
        Ok(())
    }
}

/// Initialize a minimal git repo in `dir` with one initial commit.
///
/// Hermetic by environment scrubbing (issue #13697). This seam is classified
/// identity-insensitive: consumers assert command behavior, not object IDs, so
/// it keeps the historical `master` branch name with graceful fallback for
/// older Git releases.
pub fn init_git_repo(dir: &Path) -> Result<()> {
    git_cmd(&["init", "-b", "master", "--object-format=sha1"], Some(dir))
        .or_else(|_| git_cmd(&["init", "-b", "master"], Some(dir)))
        .or_else(|_| git_cmd(&["init"], Some(dir)))?;
    git_cmd(&["config", "user.email", "test@test.com"], Some(dir))?;
    git_cmd(&["config", "user.name", "Test"], Some(dir))?;
    // Rename branch to master if needed (older git).
    let _ = git_cmd(&["checkout", "-b", "master"], Some(dir));
    Ok(())
}

/// Run a hermetic git command in `cwd`. Returns an error carrying argv, cwd,
/// and stderr if the command fails.
pub fn git_cmd(args: &[&str], cwd: Option<&Path>) -> Result<()> {
    let config_scope = tempfile::tempdir().context("failed to create Git config scope")?;
    let empty_config = config_scope.path().join("empty-config");
    fs::write(&empty_config, "").context("failed to create empty Git config")?;
    let mut cmd = StdCommand::new("git");
    cmd.args(args);
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    apply_hermetic_config(&mut cmd, &empty_config, &empty_config);
    let output = cmd.output().with_context(|| format!("git {} failed to start", args.join(" ")))?;
    if !output.status.success() {
        bail!("{}", fail_typed(args, cwd, &output));
    }
    Ok(())
}

/// Stage and commit a set of files in `repo`.
pub fn add_and_commit(repo: &Path, files: &[(&str, &str)], message: &str) -> Result<()> {
    for (name, content) in files {
        let path = repo.join(name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, content)?;
    }
    git_cmd(&["add", "."], Some(repo))?;
    git_cmd(&["commit", "-m", message], Some(repo))?;
    Ok(())
}

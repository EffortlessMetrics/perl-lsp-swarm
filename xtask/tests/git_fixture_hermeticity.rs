//! Hostile-configuration negative controls for the hermetic Git fixture
//! contract (#13697).
//!
//! Each control plants one Git configuration in a fixture-owned inheritance
//! path and then exercises both harness generations over
//! identical fixture content, identity, and pinned timestamps:
//!
//! - the legacy path (raw `git` inheriting only the planted configuration) must fail or
//!   drift — it establishes that the planted input can affect a fixture;
//! - the [`HermeticGit`] path must succeed and reproduce the raw-bytes object
//!   identity deterministically.
//! Local pins and environment isolation intentionally overlap. These controls
//! prove their combined contract, not that each redundant pin is individually
//! necessary. Bypassing environment application is the class-level falsifier:
//! hooks/filters can then alter the protected content despite the local pins.
//!
//! Issue control mapping:
//! 1. global `commit.gpgsign=true` -> [`hostile_global_signing_cannot_change_fixture_commits`]
//! 2. global `init.defaultObjectFormat=sha256` -> [`hostile_global_object_format_cannot_change_sha1_fixture_identities`]
//! 3. global hooks path mutating staged content -> [`hostile_global_hooks_path_cannot_mutate_staged_content`]
//! 4. global clean/smudge and line-ending configuration -> [`hostile_global_content_filters_cannot_change_the_pinned_tree`]
//!    and [`hostile_line_ending_configuration_cannot_change_the_pinned_tree`]
//! 5. deliberate hostile opt-in with typed refusal -> [`deliberate_hostile_opt_in_refuses_with_typed_failure`]
//! 6. command-scoped `GIT_CONFIG_COUNT` injection -> [`hostile_command_scoped_config_injection_is_scrubbed`]
//! 7. alternate config search paths, direct `GIT_CONFIG`, includes, and
//!    `safe.directory` -> [`ambient_config_search_paths_and_redirects_are_blocked`]

use anyhow::{Context, Result, bail, ensure};
use assert_cmd::Command as AssertCommand;
use std::fs;
use std::path::Path;
#[cfg(windows)]
use std::path::PathBuf;
use std::process::Command as StdCommand;

mod git_test_support;

use git_test_support::{FIXTURE_TIMESTAMP, HermeticGit, config_path_value, git_cmd_with_ambient};

/// Retain the planted global configuration, while isolating unrelated host
/// inputs. Unlike HermeticGit this does not override its signing, hooks,
/// filters, attributes, or object-format settings.
fn legacy_command(
    repo: &Path,
    args: &[&str],
    global: &Path,
) -> Result<(StdCommand, tempfile::TempDir)> {
    let scope = tempfile::tempdir()?;
    let empty_system = scope.path().join("system-config");
    fs::write(&empty_system, "")?;
    let mut cmd = StdCommand::new("git");
    cmd.args(args).current_dir(repo);
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().to_ascii_uppercase().starts_with("GIT_") {
            cmd.env_remove(key);
        }
    }
    for key in ["EDITOR", "VISUAL"] {
        cmd.env_remove(key);
    }
    cmd.env("GIT_CONFIG_GLOBAL", global)
        .env("GIT_CONFIG_SYSTEM", &empty_system)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_ATTR_NOSYSTEM", "1")
        .env("HOME", scope.path())
        .env("XDG_CONFIG_HOME", scope.path())
        .env("PROGRAMDATA", scope.path())
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("LC_ALL", "C")
        .env("LANG", "C")
        .env("TZ", "UTC")
        .env("GIT_AUTHOR_DATE", FIXTURE_TIMESTAMP)
        .env("GIT_COMMITTER_DATE", FIXTURE_TIMESTAMP);
    Ok((cmd, scope))
}

fn legacy_git(repo: &Path, args: &[&str], global: &Path) -> Result<String> {
    let (mut cmd, _scope) = legacy_command(repo, args, global)?;
    let output = cmd.output().with_context(|| format!("git {} failed to start", args.join(" ")))?;
    if !output.status.success() {
        bail!(
            "git {} failed in {}\nstderr:\n{}",
            args.join(" "),
            repo.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Initializes a legacy (non-hermetic) repository that pins identity locally
/// the way pre-#13697 fixtures did, but inherits only the planted configuration.
fn legacy_init(repo: &Path, global: &Path) -> Result<()> {
    fs::create_dir_all(repo)?;
    legacy_git(repo, &["init", "--initial-branch=main"], global)?;
    legacy_git(repo, &["config", "user.name", "Fixture User"], global)?;
    legacy_git(repo, &["config", "user.email", "fixture@example.invalid"], global)?;
    Ok(())
}

/// Stages the common legacy subject without committing it, so tests that
/// expect a commit refusal can prove the repository had content to commit.
fn stage_legacy_subject(repo: &Path, global: &Path, content: &str) -> Result<()> {
    fs::write(repo.join("tracked.txt"), content)?;
    legacy_git(repo, &["add", "tracked.txt"], global)?;
    Ok(())
}

/// Raw object bytes of a committed file, without config-driven clean filters.
fn committed_blob_id(hermetic: &HermeticGit, repo: &Path) -> Result<String> {
    hermetic.git(repo, &["rev-parse", "HEAD:tracked.txt"])
}

/// Raw-bytes hash of `content`, independent of any repository configuration.
fn raw_blob_id(hermetic: &HermeticGit, repo: &Path, content: &str) -> Result<String> {
    let file = repo.join("raw-oracle.txt");
    fs::write(&file, content)?;
    let id = hermetic.git(repo, &["hash-object", "--no-filters", "raw-oracle.txt"])?;
    let _ = fs::remove_file(file);
    Ok(id)
}

/// Builds a hostile global configuration file and returns its path.
fn hostile_global(dir: &Path, name: &str, body: &str) -> Result<std::path::PathBuf> {
    let path = dir.join(name);
    fs::write(&path, body)?;
    Ok(path)
}

/// The canonical fixture commit through the hermetic harness. Returns the
/// head commit and the raw-bytes blob identity of `tracked.txt`.
fn hermetic_commit(
    hermetic: &HermeticGit,
    repo: &Path,
    content: &str,
    hostile_global: &Path,
) -> Result<(String, String)> {
    let protected_git = |args: &[&str]| -> Result<()> {
        // Start with the same isolated legacy environment and planted global
        // input. The contract under test must replace it before execution.
        let (mut command, _scope) = legacy_command(repo, args, hostile_global)?;
        hermetic.apply_env(&mut command);
        let output = command.output()?;
        ensure!(
            output.status.success(),
            "protected git {} failed in {}: {}",
            args.join(" "),
            repo.display(),
            String::from_utf8_lossy(&output.stderr)
        );
        Ok(())
    };
    fs::create_dir_all(repo)?;
    protected_git(&["init", "--initial-branch=main", "--object-format=sha1"])?;
    hermetic.pin_repo_config(repo)?;
    let pinned = raw_blob_id(hermetic, repo, content)?;
    fs::write(repo.join("tracked.txt"), content)?;
    protected_git(&["add", "tracked.txt"])?;
    protected_git(&["commit", "-m", "hostile control subject"])?;
    let head = hermetic.git(repo, &["rev-parse", "HEAD"])?;
    Ok((head, pinned))
}

#[test]
fn free_git_command_blocks_home_and_xdg_attributes() -> Result<()> {
    for use_xdg in [false, true] {
        let tmp = tempfile::tempdir()?;
        let home = tmp.path().join("home");
        let xdg = tmp.path().join("xdg");
        let attributes_dir = if use_xdg { xdg.join("git") } else { home.join(".config/git") };
        fs::create_dir_all(&attributes_dir)?;
        fs::write(attributes_dir.join("attributes"), "tracked.txt text\n")?;
        let repo = tmp.path().join("repo");
        let hermetic = HermeticGit::at(&tmp.path().join("pins"))?;
        hermetic.init_repo(&repo)?;
        let content = "one\r\ntwo\r\n";
        let raw = raw_blob_id(&hermetic, &repo, content)?;
        fs::write(repo.join("tracked.txt"), content)?;
        let global = hostile_global(tmp.path(), "empty-global", "")?;

        // First prove that this search path really supplies an attribute and
        // changes staged bytes when the attributes pin is absent.
        let (mut legacy, _scope) = legacy_command(&repo, &["add", "tracked.txt"], &global)?;
        legacy.env("HOME", &home);
        if use_xdg {
            legacy.env("XDG_CONFIG_HOME", &xdg);
        } else {
            legacy.env_remove("XDG_CONFIG_HOME");
        }
        let output = legacy.output()?;
        ensure!(output.status.success(), "legacy staging failed: {:?}", output);
        let legacy_blob = hermetic.git(&repo, &["rev-parse", ":tracked.txt"])?;
        ensure!(legacy_blob != raw, "planted user attributes did not change staged bytes");

        let ambient = if use_xdg {
            vec![("HOME", home.as_path()), ("XDG_CONFIG_HOME", xdg.as_path())]
        } else {
            // An empty value selects Git's HOME fallback without touching the
            // parent process environment or depending on its XDG setting.
            vec![("HOME", home.as_path()), ("XDG_CONFIG_HOME", Path::new(""))]
        };
        let attributes = git_cmd_with_ambient(
            &["check-attr", "text", "--", "tracked.txt"],
            Some(&repo),
            &ambient,
        )?;
        ensure!(
            String::from_utf8(attributes.stdout)?.trim() == "tracked.txt: text: unspecified",
            "free command inherited the user attributes"
        );
        hermetic.git(&repo, &["update-index", "--force-remove", "tracked.txt"])?;
        git_cmd_with_ambient(&["add", "tracked.txt"], Some(&repo), &ambient)?;
        ensure!(
            hermetic.git(&repo, &["rev-parse", ":tracked.txt"])? == raw,
            "free command changed the raw staged content"
        );
    }
    Ok(())
}

/// The Windows Git executable derives the system attributes path from its
/// runtime prefix. Relocating only the executable lets this control plant a
/// system attributes file under a temporary prefix without changing the
/// machine-wide `etc/gitattributes`.
#[cfg(windows)]
#[test]
fn relocated_git_system_attributes_are_scrubbed() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let exec_path = StdCommand::new("git").arg("--exec-path").output()?;
    ensure!(exec_path.status.success(), "installed Git --exec-path probe failed");
    let installed_exec = String::from_utf8(exec_path.stdout)?.trim().replace('/', "\\");
    let installed_exec = Path::new(&installed_exec);
    let installed_git = installed_exec.join("git.exe");
    ensure!(installed_git.is_file(), "installed Git executable is missing");

    let relocated_exec = tmp.path().join("mingw64/libexec/git-core");
    fs::create_dir_all(&relocated_exec)?;
    fs::copy(&installed_git, relocated_exec.join("git.exe"))?;
    let system_attributes = tmp.path().join("etc/gitattributes");
    fs::create_dir_all(system_attributes.parent().context("system attributes parent")?)?;
    fs::write(&system_attributes, "tracked.txt text\n")?;

    let repo = tmp.path().join("repo");
    let hermetic = HermeticGit::at(&tmp.path().join("pins"))?;
    hermetic.init_repo(&repo)?;
    let content = "one\r\ntwo\r\n";
    let raw = raw_blob_id(&hermetic, &repo, content)?;
    fs::write(repo.join("tracked.txt"), content)?;

    let original_mingw_bin =
        installed_exec.parent().and_then(Path::parent).context("installed Git prefix")?.join("bin");
    let child_path = format!(
        "{};{};{}",
        relocated_exec.display(),
        original_mingw_bin.display(),
        std::env::var_os("SystemRoot")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\Windows"))
            .join("System32")
            .display(),
    );
    let empty_config = tmp.path().join("empty-config");
    fs::write(&empty_config, "")?;

    let command = |args: &[&str]| {
        let mut command = StdCommand::new(relocated_exec.join("git.exe"));
        command.args(args).current_dir(&repo);
        for (key, _) in std::env::vars_os() {
            if key.to_string_lossy().to_ascii_uppercase().starts_with("GIT_") {
                command.env_remove(key);
            }
        }
        for key in ["EDITOR", "VISUAL"] {
            command.env_remove(key);
        }
        command
            .env("PATH", &child_path)
            .env("HOME", tmp.path())
            .env("XDG_CONFIG_HOME", tmp.path())
            .env("GIT_CONFIG_GLOBAL", &empty_config)
            .env("GIT_CONFIG_SYSTEM", &empty_config)
            .env("GIT_CONFIG_NOSYSTEM", "0")
            .env("GIT_ATTR_NOSYSTEM", "0")
            .env("GIT_CONFIG_COUNT", "1")
            .env("GIT_CONFIG_KEY_0", "commit.gpgsign")
            .env("GIT_CONFIG_VALUE_0", "true")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("TZ", "UTC")
            .env("LC_ALL", "C")
            .env("LANG", "C");
        command
    };

    let mut control = command(&["check-attr", "text", "--", "tracked.txt"]);
    let control_attr = control.output()?;
    ensure!(
        control_attr.status.success()
            && String::from_utf8(control_attr.stdout)?.trim() == "tracked.txt: text: set",
        "relocated Git must observe the hostile system attribute: {}",
        String::from_utf8_lossy(&control_attr.stderr)
    );
    let control_add = command(&["add", "tracked.txt"]).output()?;
    ensure!(
        control_add.status.success(),
        "relocated Git control add failed: {}",
        String::from_utf8_lossy(&control_add.stderr)
    );
    ensure!(hermetic.git(&repo, &["rev-parse", ":tracked.txt"])? != raw);

    hermetic.git(&repo, &["update-index", "--force-remove", "tracked.txt"])?;
    let mut protected = command(&["check-attr", "text", "--", "tracked.txt"]);
    hermetic.apply_env(&mut protected);
    let protected_attr = protected.output()?;
    ensure!(
        protected_attr.status.success()
            && String::from_utf8(protected_attr.stdout)?.trim() == "tracked.txt: text: unspecified",
        "HermeticGit must scrub the relocated system attribute: {}",
        String::from_utf8_lossy(&protected_attr.stderr)
    );
    let mut protected_add = command(&["add", "tracked.txt"]);
    hermetic.apply_env(&mut protected_add);
    let protected_add_output = protected_add.output()?;
    ensure!(
        protected_add_output.status.success(),
        "relocated Git protected add failed: {}",
        String::from_utf8_lossy(&protected_add_output.stderr)
    );
    ensure!(hermetic.git(&repo, &["rev-parse", ":tracked.txt"])? == raw);
    Ok(())
}

#[test]
fn hostile_global_signing_cannot_change_fixture_commits() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let global = hostile_global(tmp.path(), "hostile-global", "[commit]\n\tgpgsign = true\n")?;
    let hermetic = HermeticGit::at(&tmp.path().join("pins"))?;

    let hermetic_repo = tmp.path().join("hermetic-repo");
    let (head, _) = hermetic_commit(&hermetic, &hermetic_repo, "content\n", &global)?;
    let commit_object = hermetic.git(&hermetic_repo, &["cat-file", "commit", "HEAD"])?;
    assert!(
        !commit_object.contains("gpgsig"),
        "hermetic fixture commit must stay unsigned under hostile global signing"
    );

    let legacy_repo = tmp.path().join("legacy-repo");
    legacy_init(&legacy_repo, &global)?;
    stage_legacy_subject(&legacy_repo, &global, "content\n")?;
    let legacy_outcome =
        legacy_git(&legacy_repo, &["commit", "-m", "hostile control subject"], &global);
    match legacy_outcome {
        Ok(_) => {
            let object = legacy_git(&legacy_repo, &["cat-file", "commit", "HEAD"], &global)?;
            assert!(
                object.contains("gpgsig"),
                "legacy harness must not silently produce the unsigned fixture commit \
                 under hostile global signing"
            );
            ensure!(
                legacy_git(&legacy_repo, &["rev-parse", "HEAD"], &global)? != head,
                "hostile global signing must not reproduce the pinned hermetic identity"
            );
        }
        Err(error) => {
            // Without a usable signing key the legacy harness fails outright:
            // the exact false failure class from #13110.
            ensure!(
                error.to_string().to_ascii_lowercase().contains("sign"),
                "legacy refusal must be attributable to signing, not an unrelated empty-repo or \
                 fixture failure: {error}"
            );
        }
    }
    Ok(())
}

#[test]
fn hostile_global_object_format_cannot_change_sha1_fixture_identities() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let global =
        hostile_global(tmp.path(), "hostile-global", "[init]\n\tdefaultObjectFormat = sha256\n")?;
    let hermetic = HermeticGit::at(&tmp.path().join("pins"))?;

    let hermetic_repo = tmp.path().join("hermetic-repo");
    let (head, _) = hermetic_commit(&hermetic, &hermetic_repo, "content\n", &global)?;
    assert_eq!(
        head.len(),
        40,
        "hermetic fixture must keep 40-character SHA-1 identities under hostile global object format"
    );
    let object = hermetic.git(&hermetic_repo, &["cat-file", "commit", "HEAD"])?;
    assert!(!object.contains("sha256"), "hermetic fixture commit must not be a SHA-256 object");

    // Probe whether this Git honors init.defaultObjectFormat at all; only on
    // capable Gits can the legacy harness demonstrate the drift the pin blocks.
    let probe = tempfile::tempdir()?;
    let probe_repo = probe.path().join("probe");
    let probe_repo_arg = probe_repo.to_string_lossy();
    let probe_initialized = hermetic
        .git(
            probe.path(),
            &[
                "-c",
                "init.defaultObjectFormat=sha256",
                "init",
                "--initial-branch=main",
                &probe_repo_arg,
            ],
        )
        .is_ok();
    let probe_supported = probe_initialized
        && hermetic
            .git(&probe_repo, &["rev-parse", "--show-object-format"])
            .is_ok_and(|format| format == "sha256");
    if probe_supported {
        let legacy_repo = tmp.path().join("legacy-repo");
        legacy_init(&legacy_repo, &global)?;
        ensure!(
            legacy_git(&legacy_repo, &["rev-parse", "--show-object-format"], &global)? == "sha256",
            "capable Git must honor the hostile SHA-256 precondition in the legacy repository"
        );
        stage_legacy_subject(&legacy_repo, &global, "content\n")?;
        legacy_git(&legacy_repo, &["commit", "-m", "hostile control subject"], &global)?;
        let legacy_head = legacy_git(&legacy_repo, &["rev-parse", "HEAD"], &global)?;
        assert_eq!(
            legacy_head.len(),
            64,
            "hostile global init.defaultObjectFormat must visibly drift a legacy fixture on \
             Gits that honor the key"
        );
    }
    Ok(())
}

#[test]
fn ambient_config_search_paths_and_redirects_are_blocked() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let hermetic = HermeticGit::at(&tmp.path().join("pins"))?;
    let repo = tmp.path().join("repo");
    hermetic.init_repo(&repo)?;

    let included = tmp.path().join("included-config");
    fs::write(&included, "[fixture]\n\tincluded = leaked\n")?;

    let fake_home = tmp.path().join("home");
    let fake_xdg = tmp.path().join("xdg");
    let fake_program_data = tmp.path().join("program-data");
    fs::create_dir_all(fake_xdg.join("git"))?;
    fs::create_dir_all(fake_program_data.join("Git"))?;
    fs::create_dir_all(&fake_home)?;

    let repo_match = format!("{}/", config_path_value(&repo));
    fs::write(
        fake_home.join(".gitconfig"),
        format!(
            "[includeIf \"gitdir:{repo_match}\"]\n\tpath = {}\n\
             [safe]\n\tdirectory = *\n[fixture]\n\thome = leaked\n",
            config_path_value(&included)
        ),
    )?;
    fs::write(fake_xdg.join("git/config"), "[fixture]\n\txdg = leaked\n")?;
    let hostile_system = fake_program_data.join("Git/config");
    fs::write(&hostile_system, "[fixture]\n\tsystem = leaked\n")?;
    let redirected = tmp.path().join("redirected-config");
    fs::write(&redirected, "[fixture]\n\tredirected = leaked\n")?;

    let mut cmd = StdCommand::new("git");
    cmd.args(["config", "--show-origin", "--get-regexp", "^(fixture\\.|safe\\.directory)"])
        .current_dir(&repo)
        .env("HOME", &fake_home)
        .env("XDG_CONFIG_HOME", &fake_xdg)
        .env("PROGRAMDATA", &fake_program_data)
        .env("GIT_CONFIG", &redirected)
        .env("GIT_CONFIG_GLOBAL", fake_home.join(".gitconfig"))
        .env("GIT_CONFIG_SYSTEM", &hostile_system)
        .env("GIT_ATTR_NOSYSTEM", "0");
    hermetic.apply_env(&mut cmd);

    let attr_nosystem = cmd
        .get_envs()
        .find(|(key, _)| *key == "GIT_ATTR_NOSYSTEM")
        .and_then(|(_, value)| value)
        .and_then(|value| value.to_str());
    ensure!(
        attr_nosystem == Some("1"),
        "every hermetic Git and child process must disable the system attributes plane"
    );
    let config_nosystem = cmd
        .get_envs()
        .find(|(key, _)| *key == "GIT_CONFIG_NOSYSTEM")
        .and_then(|(_, value)| value)
        .and_then(|value| value.to_str());
    ensure!(
        config_nosystem == Some("1"),
        "every hermetic Git and child process must disable the ordinary system config plane"
    );
    let direct_config = cmd.get_envs().find(|(key, _)| *key == "GIT_CONFIG");
    ensure!(
        direct_config.is_some_and(|(_, value)| value.is_none()),
        "direct GIT_CONFIG redirection must be removed before fixture-local pins are written"
    );
    let global_config =
        cmd.get_envs().find(|(key, _)| *key == "GIT_CONFIG_GLOBAL").and_then(|(_, value)| value);
    ensure!(
        global_config.is_some_and(|value| value != fake_home.join(".gitconfig").as_os_str()),
        "the fixture-owned global config must replace HOME/XDG config discovery"
    );
    let system_config =
        cmd.get_envs().find(|(key, _)| *key == "GIT_CONFIG_SYSTEM").and_then(|(_, value)| value);
    ensure!(
        system_config.is_some_and(|value| value != hostile_system.as_os_str()),
        "the fixture-owned system config must replace PROGRAMDATA/system config discovery"
    );
    let preserved_home =
        cmd.get_envs().find(|(key, _)| *key == "HOME").and_then(|(_, value)| value);
    ensure!(
        preserved_home == Some(fake_home.as_os_str()),
        "hermetic config routing must not erase unrelated child environment such as HOME"
    );

    let mut assert_child = AssertCommand::new("git");
    assert_child
        .env("HOME", &fake_home)
        .env("GIT_CONFIG", &redirected)
        .env("GIT_ATTR_NOSYSTEM", "0");
    hermetic.apply_env_to_assert(&mut assert_child);
    let child_attr_nosystem = assert_child
        .get_envs()
        .find(|(key, _)| *key == "GIT_ATTR_NOSYSTEM")
        .and_then(|(_, value)| value)
        .and_then(|value| value.to_str());
    ensure!(
        child_attr_nosystem == Some("1"),
        "assert_cmd children must receive the system-attributes pin"
    );
    let child_direct_config = assert_child.get_envs().find(|(key, _)| *key == "GIT_CONFIG");
    ensure!(
        child_direct_config.is_some_and(|(_, value)| value.is_none()),
        "assert_cmd children must scrub direct GIT_CONFIG redirection"
    );

    let output = cmd.output()?;
    ensure!(
        output.status.code() == Some(1) && output.stdout.is_empty(),
        "HOME/XDG/PROGRAMDATA/global/system/includeIf/safe.directory config leaked into the \
         hermetic repository:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    ensure!(
        hermetic.git(&repo, &["config", "--local", "--get", "user.name"])? == "Fixture User",
        "environment scrubbing must preserve intended repository-local fixture configuration"
    );
    ensure!(
        hermetic.git(&repo, &["config", "--local", "--get", "commit.gpgsign"])? == "false",
        "environment scrubbing must preserve the local unsigned-commit pin"
    );
    Ok(())
}

#[test]
fn alternate_global_destination_is_blocked() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let repo = tmp.path().join("repo");
    let poison = tmp.path().join("alternate-global");
    fs::write(&poison, "[fixture]\n\talternateGlobal = leaked\n")?;
    let hermetic = HermeticGit::at(&tmp.path().join("pins"))?;
    hermetic.init_repo(&repo)?;

    let (mut control, _scope) =
        legacy_command(&repo, &["config", "--get", "fixture.alternateGlobal"], &poison)?;
    let control_output = control.output()?;
    ensure!(
        control_output.status.success()
            && String::from_utf8(control_output.stdout)?.trim() == "leaked",
        "control must read the poisoned alternate global destination: {}",
        String::from_utf8_lossy(&control_output.stderr)
    );

    let (mut protected, _scope) =
        legacy_command(&repo, &["config", "--get", "fixture.alternateGlobal"], &poison)?;
    hermetic.apply_env(&mut protected);
    let protected_output = protected.output()?;
    ensure!(
        protected_output.status.code() == Some(1) && protected_output.stdout.is_empty(),
        "HermeticGit must block the alternate global destination: stdout={} stderr={}",
        String::from_utf8_lossy(&protected_output.stdout),
        String::from_utf8_lossy(&protected_output.stderr)
    );
    Ok(())
}

#[test]
fn hostile_global_hooks_path_cannot_mutate_staged_content() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let hooks = tmp.path().join("hostile-hooks");
    fs::create_dir_all(&hooks)?;
    let hook = hooks.join("pre-commit");
    fs::write(&hook, "#!/bin/sh\nprintf 'tampered\\n' >> tracked.txt\ngit add tracked.txt\n")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&hook, fs::Permissions::from_mode(0o755))?;
    }
    let global = hostile_global(
        tmp.path(),
        "hostile-global",
        &format!("[core]\n\thooksPath = {}\n", config_path_value(&hooks)),
    )?;
    let hermetic = HermeticGit::at(&tmp.path().join("pins"))?;

    let hermetic_repo = tmp.path().join("hermetic-repo");
    let (_head, pinned) = hermetic_commit(&hermetic, &hermetic_repo, "content\n", &global)?;
    ensure!(
        committed_blob_id(&hermetic, &hermetic_repo)? == pinned,
        "hostile global hooks path must not mutate the hermetic fixture's staged content"
    );

    // The legacy falsifier requires POSIX executable-bit semantics. The
    // hermetic assertion above remains cross-platform; Windows cannot provide
    // the same attributable hook-execution precondition.
    #[cfg(unix)]
    {
        let legacy_repo = tmp.path().join("legacy-repo");
        legacy_init(&legacy_repo, &global)?;
        fs::write(legacy_repo.join("tracked.txt"), "content\n")?;
        legacy_git(&legacy_repo, &["add", "tracked.txt"], &global)?;
        legacy_git(&legacy_repo, &["commit", "-m", "hostile control subject"], &global)?;
        let legacy_blob = legacy_git(&legacy_repo, &["rev-parse", "HEAD:tracked.txt"], &global)?;
        assert_ne!(
            legacy_blob, pinned,
            "legacy harness must show the hook-driven content drift the hermetic pin blocks"
        );
        ensure!(
            legacy_git(&legacy_repo, &["rev-parse", "HEAD"], &global)? != _head,
            "hook drift must not reproduce the pinned hermetic identity"
        );
    }
    Ok(())
}

#[test]
fn hostile_global_content_filters_cannot_change_the_pinned_tree() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let attributes = tmp.path().join("hostile-attributes");
    fs::write(&attributes, "*.txt\tfilter=hostile\n")?;
    let global = hostile_global(
        tmp.path(),
        "hostile-global",
        &format!(
            "[core]\n\tattributesFile = {}\n[filter \"hostile\"]\n\tclean = tr '[:lower:]' '[:upper:]'\n\tsmudge = cat\n",
            config_path_value(&attributes)
        ),
    )?;
    let hermetic = HermeticGit::at(&tmp.path().join("pins"))?;

    let hermetic_repo = tmp.path().join("hermetic-repo");
    let (_, pinned) = hermetic_commit(&hermetic, &hermetic_repo, "content\n", &global)?;
    ensure!(
        committed_blob_id(&hermetic, &hermetic_repo)? == pinned,
        "hostile global clean filter must not change the hermetic fixture's pinned blob"
    );

    // `tr` is the observable hostile filter and is a POSIX dependency. On
    // Unix the legacy branch must execute successfully and visibly drift;
    // other platforms exercise the cross-platform hermetic assertion above.
    #[cfg(unix)]
    {
        let legacy_repo = tmp.path().join("legacy-repo");
        legacy_init(&legacy_repo, &global)?;
        fs::write(legacy_repo.join("tracked.txt"), "content\n")?;
        legacy_git(&legacy_repo, &["add", "tracked.txt"], &global)?;
        legacy_git(&legacy_repo, &["commit", "-m", "hostile control subject"], &global)?;
        let legacy_blob = legacy_git(&legacy_repo, &["rev-parse", "HEAD:tracked.txt"], &global)?;
        assert_ne!(
            legacy_blob, pinned,
            "legacy harness must show the clean-filter drift the hermetic pin blocks"
        );
    }
    Ok(())
}

#[test]
fn hostile_line_ending_configuration_cannot_change_the_pinned_tree() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let global = hostile_global(tmp.path(), "hostile-global", "[core]\n\tautocrlf = true\n")?;
    let hermetic = HermeticGit::at(&tmp.path().join("pins"))?;
    let content = "line\r\n";

    let hermetic_repo = tmp.path().join("hermetic-repo");
    let (_, pinned) = hermetic_commit(&hermetic, &hermetic_repo, content, &global)?;
    ensure!(
        committed_blob_id(&hermetic, &hermetic_repo)? == pinned,
        "hostile global line-ending configuration must not change the hermetic fixture's \
         pinned blob"
    );

    let legacy_repo = tmp.path().join("legacy-repo");
    legacy_init(&legacy_repo, &global)?;
    fs::write(legacy_repo.join("tracked.txt"), content)?;
    legacy_git(&legacy_repo, &["add", "tracked.txt"], &global)?;
    legacy_git(&legacy_repo, &["commit", "-m", "hostile control subject"], &global)?;
    let legacy_blob = legacy_git(&legacy_repo, &["rev-parse", "HEAD:tracked.txt"], &global)?;
    assert_ne!(
        legacy_blob, pinned,
        "legacy harness must show the line-ending normalization the hermetic pin blocks"
    );
    Ok(())
}

#[test]
fn hostile_command_scoped_config_injection_is_scrubbed() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let hermetic = HermeticGit::at(&tmp.path().join("pins"))?;

    let hermetic_repo = tmp.path().join("hermetic-repo");
    hermetic.init_repo(&hermetic_repo)?;
    fs::write(hermetic_repo.join("tracked.txt"), "content\n")?;
    hermetic.git(&hermetic_repo, &["add", "tracked.txt"])?;
    let mut protected = StdCommand::new("git");
    protected
        .args(["commit", "-m", "hostile control subject"])
        .current_dir(&hermetic_repo)
        .env("GIT_CONFIG_COUNT", "1")
        .env("GIT_CONFIG_KEY_0", "commit.gpgsign")
        .env("GIT_CONFIG_VALUE_0", "true");
    hermetic.apply_env(&mut protected);
    let protected_output = protected.output()?;
    ensure!(
        protected_output.status.success(),
        "hermetic commit inherited signing injection: {}",
        String::from_utf8_lossy(&protected_output.stderr)
    );
    let head = hermetic.git(&hermetic_repo, &["rev-parse", "HEAD"])?;
    let commit_object = hermetic.git(&hermetic_repo, &["cat-file", "commit", "HEAD"])?;
    ensure!(
        !commit_object.contains("gpgsig"),
        "hermetic harness must scrub command-scoped commit.gpgsign injection"
    );

    // The same injection through the legacy harness must fail or sign.
    let legacy_repo = tmp.path().join("legacy-repo");
    let global = hostile_global(tmp.path(), "empty-global", "")?;
    legacy_init(&legacy_repo, &global)?;
    fs::write(legacy_repo.join("tracked.txt"), "content\n")?;
    legacy_git(&legacy_repo, &["add", "tracked.txt"], &global)?;
    let (mut injected, _scope) =
        legacy_command(&legacy_repo, &["commit", "-m", "hostile control subject"], &global)?;
    injected
        .env("GIT_CONFIG_COUNT", "1")
        .env("GIT_CONFIG_KEY_0", "commit.gpgsign")
        .env("GIT_CONFIG_VALUE_0", "true");
    let output = injected.output()?;
    if output.status.success() {
        let object = legacy_git(&legacy_repo, &["cat-file", "commit", "HEAD"], &global)?;
        ensure!(
            object.contains("gpgsig"),
            "legacy harness must not silently produce the unsigned commit under \
             command-scoped signing injection"
        );
        ensure!(
            legacy_git(&legacy_repo, &["rev-parse", "HEAD"], &global)? != head,
            "command-scoped signing injection must not reproduce the pinned identity"
        );
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).to_ascii_lowercase();
        ensure!(
            stderr.contains("sign"),
            "legacy refusal must be attributable to injected signing, not an unrelated failure: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}

#[test]
fn deliberate_hostile_opt_in_refuses_with_typed_failure() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let hooks = tmp.path().join("opt-in-hooks");
    fs::create_dir_all(&hooks)?;
    let hook = hooks.join("pre-commit");
    fs::write(&hook, "#!/bin/sh\necho 'refusing fixture commit' >&2\nexit 1\n")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&hook, fs::Permissions::from_mode(0o755))?;
    }

    let hermetic = HermeticGit::with_pins(
        &tmp.path().join("pins"),
        &[("core.hooksPath", &config_path_value(&hooks))],
    )?;
    let repo = tmp.path().join("opt-in-repo");
    hermetic.init_repo(&repo)?;

    let pinned_hooks = hermetic.git(&repo, &["config", "--global", "core.hooksPath"])?;
    ensure!(
        pinned_hooks == config_path_value(&hooks),
        "deliberate opt-in pin must be the only hostile value in the pinned global configuration"
    );

    fs::write(repo.join("tracked.txt"), "content\n")?;
    hermetic.git(&repo, &["add", "tracked.txt"])?;
    let refusal = hermetic.git(&repo, &["commit", "-m", "hostile opt-in subject"]);
    let error = match refusal {
        Ok(_) => bail!("opted-in hostile hook must refuse the fixture commit"),
        Err(error) => error.to_string(),
    };
    ensure!(error.contains("git commit -m"), "typed failure must carry the failing argv: {error}");
    ensure!(
        error.contains(&repo.display().to_string()),
        "typed failure must carry the working directory: {error}"
    );
    ensure!(
        error.contains("refusing fixture commit"),
        "typed failure must carry the hostile stderr: {error}"
    );
    Ok(())
}

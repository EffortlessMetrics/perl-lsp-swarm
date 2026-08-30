//! Hostile-configuration negative controls for the hermetic Git fixture
//! contract (#13697).
//!
//! Each control plants one ambient-breaking Git configuration on the invoking
//! machine's inheritance path and then exercises both harness generations over
//! identical fixture content, identity, and pinned timestamps:
//!
//! - the legacy path (raw `git` inheriting ambient configuration) must fail or
//!   drift — it is the falsifier for the corresponding pin, so removing a
//!   required pin re-opens the drift this file asserts against;
//! - the [`HermeticGit`] path must succeed and reproduce the raw-bytes object
//!   identity deterministically.
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
use std::process::Command as StdCommand;

mod git_test_support;

use git_test_support::{FIXTURE_TIMESTAMP, HermeticGit, config_path_value};

/// Runs a git command the way pre-#13697 fixtures did: ambient environment
/// inherited, only the fixture-global configuration swapped for `global`.
fn legacy_git(repo: &Path, args: &[&str], global: &Path) -> Result<String> {
    let mut cmd = StdCommand::new("git");
    cmd.args(args).current_dir(repo);
    cmd.env("GIT_CONFIG_GLOBAL", global)
        .env("GIT_AUTHOR_DATE", FIXTURE_TIMESTAMP)
        .env("GIT_COMMITTER_DATE", FIXTURE_TIMESTAMP);
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
/// the way pre-#13697 fixtures did, but inherits everything else.
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
fn hermetic_commit(hermetic: &HermeticGit, repo: &Path, content: &str) -> Result<(String, String)> {
    hermetic.init_repo(repo)?;
    let pinned = raw_blob_id(hermetic, repo, content)?;
    fs::write(repo.join("tracked.txt"), content)?;
    hermetic.git(repo, &["add", "tracked.txt"])?;
    hermetic.git(repo, &["commit", "-m", "hostile control subject"])?;
    let head = hermetic.git(repo, &["rev-parse", "HEAD"])?;
    Ok((head, pinned))
}

#[test]
fn hostile_global_signing_cannot_change_fixture_commits() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let global = hostile_global(tmp.path(), "hostile-global", "[commit]\n\tgpgsign = true\n")?;
    let hermetic = HermeticGit::at(&tmp.path().join("pins"))?;

    let hermetic_repo = tmp.path().join("hermetic-repo");
    let (head, _) = hermetic_commit(&hermetic, &hermetic_repo, "content\n")?;
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
    let (head, _) = hermetic_commit(&hermetic, &hermetic_repo, "content\n")?;
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
    let _global = hostile_global(
        tmp.path(),
        "hostile-global",
        &format!("[core]\n\thooksPath = {}\n", config_path_value(&hooks)),
    )?;
    let hermetic = HermeticGit::at(&tmp.path().join("pins"))?;

    let hermetic_repo = tmp.path().join("hermetic-repo");
    let (_head, pinned) = hermetic_commit(&hermetic, &hermetic_repo, "content\n")?;
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
        legacy_init(&legacy_repo, &_global)?;
        fs::write(legacy_repo.join("tracked.txt"), "content\n")?;
        legacy_git(&legacy_repo, &["add", "tracked.txt"], &_global)?;
        legacy_git(&legacy_repo, &["commit", "-m", "hostile control subject"], &_global)?;
        let legacy_blob = legacy_git(&legacy_repo, &["rev-parse", "HEAD:tracked.txt"], &_global)?;
        assert_ne!(
            legacy_blob, pinned,
            "legacy harness must show the hook-driven content drift the hermetic pin blocks"
        );
        ensure!(
            legacy_git(&legacy_repo, &["rev-parse", "HEAD"], &_global)? != _head,
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
    let _global = hostile_global(
        tmp.path(),
        "hostile-global",
        &format!(
            "[core]\n\tattributesFile = {}\n[filter \"hostile\"]\n\tclean = tr '[:lower:]' '[:upper:]'\n\tsmudge = cat\n",
            config_path_value(&attributes)
        ),
    )?;
    let hermetic = HermeticGit::at(&tmp.path().join("pins"))?;

    let hermetic_repo = tmp.path().join("hermetic-repo");
    let (_, pinned) = hermetic_commit(&hermetic, &hermetic_repo, "content\n")?;
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
        legacy_init(&legacy_repo, &_global)?;
        fs::write(legacy_repo.join("tracked.txt"), "content\n")?;
        legacy_git(&legacy_repo, &["add", "tracked.txt"], &_global)?;
        legacy_git(&legacy_repo, &["commit", "-m", "hostile control subject"], &_global)?;
        let legacy_blob = legacy_git(&legacy_repo, &["rev-parse", "HEAD:tracked.txt"], &_global)?;
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
    let (_, pinned) = hermetic_commit(&hermetic, &hermetic_repo, content)?;
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
    let (head, _) = hermetic_commit(&hermetic, &hermetic_repo, "content\n")?;
    let commit_object = hermetic.git(&hermetic_repo, &["cat-file", "commit", "HEAD"])?;
    assert!(
        !commit_object.contains("gpgsig"),
        "hermetic harness must scrub command-scoped commit.gpgsign injection"
    );

    // The same injection through the legacy harness must fail or sign.
    let legacy_repo = tmp.path().join("legacy-repo");
    let global = hostile_global(tmp.path(), "empty-global", "")?;
    legacy_init(&legacy_repo, &global)?;
    fs::write(legacy_repo.join("tracked.txt"), "content\n")?;
    legacy_git(&legacy_repo, &["add", "tracked.txt"], &global)?;
    let mut injected = StdCommand::new("git");
    injected
        .args(["commit", "-m", "hostile control subject"])
        .current_dir(&legacy_repo)
        .env("GIT_CONFIG_GLOBAL", &global)
        .env("GIT_AUTHOR_DATE", FIXTURE_TIMESTAMP)
        .env("GIT_COMMITTER_DATE", FIXTURE_TIMESTAMP)
        .env("GIT_CONFIG_COUNT", "1")
        .env("GIT_CONFIG_KEY_0", "commit.gpgsign")
        .env("GIT_CONFIG_VALUE_0", "true");
    let output = injected.output()?;
    if output.status.success() {
        let object = legacy_git(&legacy_repo, &["cat-file", "commit", "HEAD"], &global)?;
        assert!(
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

//! Exact, fail-closed external-command double (#15139).
//!
//! These unit fixtures pin the reusable contract. D1–D3 consume the same
//! types; domain admission/dispatch semantics stay in their owning tests.

#![allow(clippy::expect_used)]

#[path = "support/external_command.rs"]
mod external_command;

use external_command::{
    CommandResult, ExpectedExternalCommand, ExternalCommandFixture, ExternalCommandFixtureResult,
    JsonFieldContract, MismatchKind, ObservedExternalCommand,
};

fn observe_ok(
    fixture: &ExternalCommandFixture,
    program: &str,
    argv: &[&str],
) -> Result<CommandResult, String> {
    match fixture.observe(ObservedExternalCommand::new(
        program,
        argv.iter().map(|arg| (*arg).to_string()).collect(),
    )) {
        ExternalCommandFixtureResult::Matched(result) => Ok(result),
        ExternalCommandFixtureResult::Mismatch(mismatch) => Err(mismatch.to_string()),
    }
}

fn observe_mismatch(
    fixture: &ExternalCommandFixture,
    program: &str,
    argv: &[&str],
) -> Result<MismatchKind, String> {
    match fixture.observe(ObservedExternalCommand::new(
        program,
        argv.iter().map(|arg| (*arg).to_string()).collect(),
    )) {
        ExternalCommandFixtureResult::Mismatch(mismatch) => Ok(mismatch.kind),
        ExternalCommandFixtureResult::Matched(result) => {
            Err(format!("expected mismatch, matched with stdout {:?}", result.stdout))
        }
    }
}

fn gh_pr_view(fields: &str) -> ExpectedExternalCommand {
    ExpectedExternalCommand::exact(
        "gh",
        vec![
            "pr".into(),
            "view".into(),
            "7799".into(),
            "--repo".into(),
            "EffortlessMetrics/perl-lsp-swarm".into(),
            "--json".into(),
            fields.into(),
        ],
    )
    .json_fields_are_a_set()
    .json_contract(JsonFieldContract::exact_supported(
        "number,state,headRefName,headRefOid,isCrossRepository",
    ))
    .stdout(r#"{"number":7799,"state":"MERGED"}"#)
}

#[test]
fn dropping_a_required_argument_fails() -> Result<(), String> {
    let fixture = ExternalCommandFixture::new()
        .expect(ExpectedExternalCommand::exact("git", vec!["ls-remote".into(), "origin".into()]));
    assert_eq!(observe_mismatch(&fixture, "git", &["ls-remote"])?, MismatchKind::ArgumentMismatch);
    Ok(())
}

#[test]
fn changing_an_argument_value_with_the_same_prefix_fails() -> Result<(), String> {
    let fixture = ExternalCommandFixture::new().expect(ExpectedExternalCommand::exact(
        "git",
        vec!["ls-remote".into(), "origin".into(), "refs/heads/main".into()],
    ));
    assert_eq!(
        observe_mismatch(&fixture, "git", &["ls-remote", "origin", "refs/heads/other"])?,
        MismatchKind::ArgumentMismatch
    );
    Ok(())
}

#[test]
fn reordering_semantic_argv_fails() -> Result<(), String> {
    let fixture = ExternalCommandFixture::new().expect(ExpectedExternalCommand::exact(
        "git",
        vec!["remote".into(), "get-url".into(), "--push".into(), "origin".into()],
    ));
    assert_eq!(
        observe_mismatch(&fixture, "git", &["remote", "get-url", "origin", "--push"])?,
        MismatchKind::ArgumentMismatch
    );
    Ok(())
}

#[test]
fn an_unexpected_invocation_fails() -> Result<(), String> {
    let fixture = ExternalCommandFixture::new()
        .expect(ExpectedExternalCommand::exact("git", vec!["status".into()]));
    assert_eq!(observe_mismatch(&fixture, "git", &["diff"])?, MismatchKind::ArgumentMismatch);
    Ok(())
}

#[test]
fn too_many_calls_fail() -> Result<(), String> {
    let fixture = ExternalCommandFixture::new()
        .expect(ExpectedExternalCommand::exact("git", vec!["status".into()]).times(1));
    observe_ok(&fixture, "git", &["status"])?;
    assert_eq!(observe_mismatch(&fixture, "git", &["status"])?, MismatchKind::UnexpectedInvocation);
    Ok(())
}

#[test]
fn too_few_calls_fail_on_exhaustion() -> Result<(), String> {
    let fixture = ExternalCommandFixture::new()
        .expect(ExpectedExternalCommand::exact("git", vec!["status".into()]).times(2));
    observe_ok(&fixture, "git", &["status"])?;
    let unused = fixture.assert_exhausted().expect_err("unused expectation must fail");
    assert_eq!(unused.kind, MismatchKind::UnusedExpectation);
    Ok(())
}

#[test]
fn another_json_field_set_cannot_reuse_the_old_response() -> Result<(), String> {
    let fixture = ExternalCommandFixture::new()
        .expect(gh_pr_view("number,state,headRefName,headRefOid,isCrossRepository"));
    assert_eq!(
        observe_mismatch(
            &fixture,
            "gh",
            &[
                "pr",
                "view",
                "7799",
                "--repo",
                "EffortlessMetrics/perl-lsp-swarm",
                "--json",
                "number,state",
            ],
        )?,
        MismatchKind::JsonFieldSetMismatch
    );
    Ok(())
}

#[test]
fn historical_merged_json_field_is_unsupported() -> Result<(), String> {
    let fixture = ExternalCommandFixture::new()
        .expect(gh_pr_view("number,state,headRefName,headRefOid,isCrossRepository"));
    assert_eq!(
        observe_mismatch(
            &fixture,
            "gh",
            &[
                "pr",
                "view",
                "7799",
                "--repo",
                "EffortlessMetrics/perl-lsp-swarm",
                "--json",
                "number,state,merged,headRefName,headRefOid,isCrossRepository",
            ],
        )?,
        MismatchKind::UnsupportedJsonField
    );
    Ok(())
}

#[test]
fn fixture_payload_cannot_invent_unrequested_fields() -> Result<(), String> {
    let fixture = ExternalCommandFixture::new().expect(
        gh_pr_view("number,state,headRefName,headRefOid,isCrossRepository")
            .stdout(r#"{"number":7799,"state":"MERGED","merged":true}"#),
    );
    assert_eq!(
        observe_mismatch(
            &fixture,
            "gh",
            &[
                "pr",
                "view",
                "7799",
                "--repo",
                "EffortlessMetrics/perl-lsp-swarm",
                "--json",
                "number,state,headRefName,headRefOid,isCrossRepository",
            ],
        )?,
        MismatchKind::UnexpectedPayloadField
    );
    Ok(())
}

#[test]
fn prefix_only_executable_plus_subcommand_is_not_enough() -> Result<(), String> {
    let fixture = ExternalCommandFixture::new().expect(ExpectedExternalCommand::exact(
        "gh",
        vec![
            "pr".into(),
            "view".into(),
            "7799".into(),
            "--repo".into(),
            "EffortlessMetrics/perl-lsp-swarm".into(),
        ],
    ));
    assert_eq!(observe_mismatch(&fixture, "gh", &["pr", "view"])?, MismatchKind::ArgumentMismatch);
    Ok(())
}

#[test]
fn reordered_json_fields_are_accepted_when_declared_a_set() -> Result<(), String> {
    let fixture = ExternalCommandFixture::new()
        .expect(gh_pr_view("number,state,headRefName,headRefOid,isCrossRepository"));
    let result = observe_ok(
        &fixture,
        "gh",
        &[
            "pr",
            "view",
            "7799",
            "--repo",
            "EffortlessMetrics/perl-lsp-swarm",
            "--json",
            "isCrossRepository,headRefOid,headRefName,state,number",
        ],
    )?;
    assert_eq!(result.exit_code, 0);
    Ok(())
}

#[test]
fn selected_env_mismatch_fails_without_dumping_unrelated_env() -> Result<(), String> {
    let fixture = ExternalCommandFixture::new().expect(
        ExpectedExternalCommand::exact("gh", vec!["auth".into(), "token".into()])
            .env_binding("GH_REPO", "EffortlessMetrics/perl-lsp-swarm")
            .stdout("ok"),
    );
    let mut observed = ObservedExternalCommand::new("gh", vec!["auth".into(), "token".into()]);
    observed.env.insert("GH_REPO".into(), "other/repo".into());
    observed.env.insert("GH_TOKEN".into(), "planted-secret".into());
    observed.env.insert("LANG".into(), "C".into());
    match fixture.observe(observed) {
        ExternalCommandFixtureResult::Mismatch(mismatch) => {
            assert_eq!(mismatch.kind, MismatchKind::WrongEnv);
            let rendered = mismatch.to_string();
            assert!(rendered.contains("GH_REPO=other/repo"), "{rendered}");
            assert!(!rendered.contains("planted-secret"), "{rendered}");
            assert!(!rendered.contains("GH_TOKEN"), "{rendered}");
            assert!(!rendered.contains("LANG=C"), "{rendered}");
        }
        ExternalCommandFixtureResult::Matched(_) => {
            return Err("env mismatch must fail".into());
        }
    }
    Ok(())
}

#[test]
fn cwd_mismatch_fails() -> Result<(), String> {
    let fixture = ExternalCommandFixture::new()
        .expect(ExpectedExternalCommand::exact("git", vec!["status".into()]).cwd("/repo"));
    let mut observed = ObservedExternalCommand::new("git", vec!["status".into()]);
    observed.cwd = Some("/tmp".into());
    match fixture.observe(observed) {
        ExternalCommandFixtureResult::Mismatch(mismatch) => {
            assert_eq!(mismatch.kind, MismatchKind::WrongCwd);
        }
        ExternalCommandFixtureResult::Matched(_) => return Err("cwd mismatch must fail".into()),
    }
    Ok(())
}

#[test]
fn stale_subject_payload_is_a_distinct_mismatch_when_argv_names_another_head() -> Result<(), String>
{
    let fixture = ExternalCommandFixture::new().expect(ExpectedExternalCommand::exact(
        "gh",
        vec![
            "workflow".into(),
            "run".into(),
            "ci.yml".into(),
            "--ref".into(),
            "automation/ci-lane-history".into(),
            "-f".into(),
            "base_sha=aaa".into(),
            "-f".into(),
            "head_sha=bbb".into(),
        ],
    ));
    assert_eq!(
        observe_mismatch(
            &fixture,
            "gh",
            &[
                "workflow",
                "run",
                "ci.yml",
                "--ref",
                "automation/ci-lane-history",
                "-f",
                "base_sha=aaa",
                "-f",
                "head_sha=stale",
            ],
        )?,
        MismatchKind::ArgumentMismatch
    );
    Ok(())
}

#[test]
fn unused_fixture_cannot_report_success() {
    let fixture = ExternalCommandFixture::new()
        .expect(ExpectedExternalCommand::exact("git", vec!["status".into()]));
    let unused = fixture.assert_exhausted().expect_err("unused fixture must fail");
    assert_eq!(unused.kind, MismatchKind::UnusedExpectation);
}

#[test]
fn mismatch_output_is_bounded_and_ordered() -> Result<(), String> {
    let fixture = ExternalCommandFixture::new()
        .expect(ExpectedExternalCommand::exact("git", vec!["ls-remote".into(), "origin".into()]));
    let mismatch = match fixture
        .observe(ObservedExternalCommand::new("git", vec!["ls-remote".into(), "upstream".into()]))
    {
        ExternalCommandFixtureResult::Mismatch(mismatch) => mismatch,
        ExternalCommandFixtureResult::Matched(_) => return Err("must mismatch".into()),
    };
    let rendered = mismatch.to_string();
    assert!(rendered.contains("argument mismatch"), "{rendered}");
    assert!(rendered.contains("observed: git ls-remote upstream"), "{rendered}");
    assert!(rendered.contains("expected: git ls-remote origin"), "{rendered}");
    assert!(!rendered.contains("PATH="), "{rendered}");
    Ok(())
}

#[test]
fn migrated_adapters_do_not_restore_prefix_or_host_path_fallback() {
    let live = include_str!("branch_deletion_admission_live.rs");
    assert!(!live.contains("full.starts_with(prefix)"), "D1 must not restore prefix matching");
    assert!(!live.contains("filter(|(prefix, _)"), "D1 must not restore prefix lookup");

    let lane = include_str!("lane_history_ci_dispatch_contract.rs");
    assert!(
        !lane.contains("replace(\"gh pr view\""),
        "D2 must not use textual gh rewriting as hermeticity"
    );
    assert!(
        !lane.contains("split_paths(&existing_path)"),
        "D2 must not prepend a stub onto the host PATH"
    );

    let post = include_str!("post_merge_status_regen_tests.rs");
    assert!(
        !post.contains("replace(\"gh workflow run\""),
        "D2 post-merge must not rewrite gh as hermeticity"
    );
    assert!(
        !post.contains("split_paths(&existing_path)"),
        "D2 post-merge must not prepend a stub onto the host PATH"
    );
}

#[cfg(unix)]
mod hermetic {
    use super::*;
    use external_command::HermeticCommandEnv;
    use std::process::Command;

    #[test]
    fn unmatched_git_does_not_reach_a_path_sentinel() -> Result<(), Box<dyn std::error::Error>> {
        let fixture = ExternalCommandFixture::new().expect(
            ExpectedExternalCommand::exact(
                "git",
                vec!["rev-parse".into(), "--show-toplevel".into()],
            )
            .stdout("/repo\n"),
        );
        let env = HermeticCommandEnv::install("git", &fixture)?;
        let path = env.path_env()?;
        let output = Command::new("git")
            .args(["status"])
            .env("PATH", &path)
            .env("EXTERNAL_COMMAND_DOUBLE_DIR", &env.fixture_dir)
            .env("EXTERNAL_COMMAND_DOUBLE_SENTINEL", &env.sentinel_marker)
            .output()?;
        assert!(!output.status.success(), "unmatched git must fail the fixture");
        assert!(!env.sentinel_hit(), "live PATH fallback reached the sentinel");
        let mismatch = env.mismatch_text().ok_or("shim must record a mismatch")?;
        assert!(mismatch.contains("unexpected invocation"), "{mismatch}");
        Ok(())
    }

    #[test]
    fn matched_git_is_served_by_the_fixture() -> Result<(), Box<dyn std::error::Error>> {
        let fixture = ExternalCommandFixture::new().expect(
            ExpectedExternalCommand::exact(
                "git",
                vec!["rev-parse".into(), "--show-toplevel".into()],
            )
            .stdout("/repo\n"),
        );
        let env = HermeticCommandEnv::install("git", &fixture)?;
        let mut command = Command::new("git");
        command.args(["rev-parse", "--show-toplevel"]);
        env.apply(&mut command)?;
        let output = command.output()?;
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        assert_eq!(String::from_utf8_lossy(&output.stdout), "/repo\n");
        assert!(!env.sentinel_hit(), "matched git must not reach the sentinel");
        assert_eq!(env.observations()?.len(), 1);
        Ok(())
    }
}

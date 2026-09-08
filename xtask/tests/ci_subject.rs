//! Public immutable-subject and real ci-scope consumer proof (#8042).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use assert_cmd::cargo::cargo_bin_cmd;
use color_eyre::eyre::{Context, ContextCompat, Result, bail, ensure};
use serde_json::{Value, json};

const REPOSITORY: &str = "EffortlessMetrics/perl-lsp-swarm";

fn git(repo: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo)
        .env("GIT_AUTHOR_DATE", "2026-08-27T12:00:00Z")
        .env("GIT_COMMITTER_DATE", "2026-08-27T12:00:00Z")
        .output()
        .with_context(|| format!("git {} failed to start", args.join(" ")))?;
    ensure!(
        output.status.success(),
        "git {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr).trim()
    );
    Ok(String::from_utf8(output.stdout)?.trim().to_string())
}

fn commit(repo: &Path, path: &str, content: &str, message: &str) -> Result<String> {
    let file = repo.join(path);
    if let Some(parent) = file.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(file, content)?;
    git(repo, &["add", path])?;
    git(repo, &["commit", "-m", message])?;
    git(repo, &["rev-parse", "HEAD"])
}

fn init_fixture(root: &Path) -> Result<(String, String)> {
    git(root, &["init", "--initial-branch=main"])?;
    git(root, &["config", "user.email", "ci-subject@example.com"])?;
    git(root, &["config", "user.name", "CI Subject Fixture"])?;
    git(root, &["config", "commit.gpgsign", "false"])?;
    git(root, &["remote", "add", "origin", "git@github.com:EffortlessMetrics/perl-lsp-swarm.git"])?;
    fs::create_dir_all(root.join("crates/demo/src"))?;
    fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nmembers = [\"crates/demo\"]\nresolver = \"2\"\n",
    )?;
    fs::write(
        root.join("crates/demo/Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )?;
    fs::write(root.join("crates/demo/src/lib.rs"), "pub fn value() -> u8 { 1 }\n")?;
    git(root, &["add", "Cargo.toml", "crates/demo/Cargo.toml", "crates/demo/src/lib.rs"])?;
    git(root, &["commit", "-m", "fixture base"])?;
    let base = git(root, &["rev-parse", "HEAD"])?;
    git(root, &["checkout", "-b", "candidate"])?;
    let head = commit(
        root,
        "crates/demo/src/platform/windows.rs",
        "pub fn windows_value() -> u8 { 2 }\n",
        "fixture Windows Rust change",
    )?;
    Ok((base, head))
}

fn write_pr_event(path: &Path, base: &str, head: &str) -> Result<()> {
    let event = json!({
        "repository": {"full_name": REPOSITORY},
        "pull_request": {
            "base": {"sha": base, "repo": {"full_name": REPOSITORY}},
            "head": {"sha": head, "repo": {"full_name": REPOSITORY}}
        }
    });
    fs::write(path, serde_json::to_vec_pretty(&event)?)?;
    Ok(())
}

fn run_subject(repo: &Path, event: &Path, receipt: &Path) -> Result<()> {
    run_event_subject(repo, event, receipt, "pull_request", None)
}

fn run_event_subject(
    repo: &Path,
    event: &Path,
    receipt: &Path,
    event_name: &str,
    github_sha: Option<&str>,
) -> Result<()> {
    let output = run_event_subject_output(repo, event, receipt, event_name, github_sha)?;
    ensure!(
        output.status.success(),
        "ci-subject failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}

fn run_event_subject_output(
    repo: &Path,
    event: &Path,
    receipt: &Path,
    event_name: &str,
    github_sha: Option<&str>,
) -> Result<Output> {
    let mut command = cargo_bin_cmd!("xtask");
    command
        .args(["ci-subject", "--event-name", event_name, "--event-path"])
        .arg(event)
        .args(["--repository", REPOSITORY, "--receipt"])
        .arg(receipt)
        .arg("--root")
        .arg(repo);
    if let Some(github_sha) = github_sha {
        command.args(["--github-sha", github_sha]);
    }
    Ok(command.output()?)
}

#[test]
fn public_push_and_merge_group_adapters_preserve_direct_exact_pair_semantics() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let repo = tmp.path().join("repo");
    fs::create_dir_all(&repo)?;
    let (base, head) = init_fixture(&repo)?;
    let cases = [
        (
            "push",
            json!({
                "repository": {"full_name": REPOSITORY},
                "before": base,
                "after": head
            }),
            Some(head.as_str()),
        ),
        (
            "merge_group",
            json!({
                "repository": {"full_name": REPOSITORY},
                "merge_group": {"base_sha": base, "head_sha": head}
            }),
            None,
        ),
    ];

    for (event_name, event, github_sha) in cases {
        let event_path = tmp.path().join(format!("{event_name}.json"));
        let receipt_path = tmp.path().join(format!("{event_name}-subject.json"));
        fs::write(&event_path, serde_json::to_vec_pretty(&event)?)?;
        run_event_subject(&repo, &event_path, &receipt_path, event_name, github_sha)?;
        let receipt: Value = serde_json::from_slice(&fs::read(receipt_path)?)?;
        ensure!(receipt["base_sha"] == base);
        ensure!(receipt["head_sha"] == head);
        ensure!(receipt["diff_base_sha"] == base);
        ensure!(receipt["diff_mode"] == "direct");
        ensure!(receipt["changed_file_count"] == 1);
        ensure!(
            receipt["changed_input_digest"]
                == "36c8a973bc6b53f4abf35ed1b950f4f1f9d6695eba0fa4aee8d959983795d2c5"
        );
    }
    Ok(())
}

#[test]
fn captured_pr_subject_survives_base_branch_movement_and_drives_real_ci_scope() -> Result<()> {
    const EXPECTED_BASE_SHA: &str = "ada48a124469513961733ba4a2d2e06979d5f4d6";
    const EXPECTED_HEAD_SHA: &str = "5e45f8ae9f693add2055dc3f877e2c3a18abc288";
    const EXPECTED_BASE_TREE: &str = "a886ebee86252cc16c459dbe52830030ec354545";
    const EXPECTED_HEAD_TREE: &str = "c742cf5cbf9aa88f4f8ad298e306cd3e455d7238";
    const EXPECTED_SUBJECT_DIGEST: &str =
        "b5cf004cbbeabf075acef82521031a3a5d094a906dfe1b19e67bb503fcb0a75b";
    let tmp = tempfile::tempdir()?;
    let repo = tmp.path().join("repo");
    fs::create_dir_all(&repo)?;
    let (base, head) = init_fixture(&repo)?;
    ensure!(base == EXPECTED_BASE_SHA, "fixture base commit changed unexpectedly");
    ensure!(head == EXPECTED_HEAD_SHA, "fixture head commit changed unexpectedly");
    let event = tmp.path().join("event.json");
    let first_receipt = tmp.path().join("subject-first.json");
    let moved_receipt = tmp.path().join("subject-moved.json");
    write_pr_event(&event, &base, &head)?;

    run_subject(&repo, &event, &first_receipt)?;
    let first_bytes = fs::read(&first_receipt)?;

    git(&repo, &["checkout", "main"])?;
    let moved_main = commit(&repo, "README.md", "branch moved\n", "move main")?;
    ensure!(moved_main != base, "fixture main branch must move away from event base");
    git(&repo, &["checkout", "candidate"])?;

    run_subject(&repo, &event, &moved_receipt)?;
    let moved_bytes = fs::read(&moved_receipt)?;
    ensure!(first_bytes == moved_bytes, "semantic receipt bytes changed after branch movement");
    ensure!(moved_bytes.len() < 2048, "bounded subject receipt exceeded 2 KiB");

    let receipt: Value = serde_json::from_slice(&moved_bytes)?;
    ensure!(receipt["base_sha"] == base, "receipt lost captured PR base");
    ensure!(receipt["head_sha"] == head, "receipt lost captured PR head");
    ensure!(receipt["base_sha"] == EXPECTED_BASE_SHA, "receipt changed the canonical base");
    ensure!(receipt["head_sha"] == EXPECTED_HEAD_SHA, "receipt changed the canonical head");
    ensure!(receipt["base_tree"] == EXPECTED_BASE_TREE, "receipt changed the canonical base tree");
    ensure!(receipt["head_tree"] == EXPECTED_HEAD_TREE, "receipt changed the canonical head tree");
    ensure!(
        receipt["diff_base_tree"] == EXPECTED_BASE_TREE,
        "receipt changed the canonical diff tree"
    );
    ensure!(
        receipt["subject_digest"] == EXPECTED_SUBJECT_DIGEST,
        "subject digest mismatched the independent fixture oracle"
    );
    ensure!(receipt["changed_file_count"] == 1, "expected one changed input");
    ensure!(
        receipt["changed_input_digest"]
            == "36c8a973bc6b53f4abf35ed1b950f4f1f9d6695eba0fa4aee8d959983795d2c5",
        "changed-input digest must match the independently computed fixture oracle"
    );
    let expected_receipt = format!(
        "{{\n  \"schema_version\": \"ci-subject.v1\",\n  \"producer\": \"cargo-xtask-ci-subject\",\n  \"status\": \"RESOLVED\",\n  \"repository\": \"{REPOSITORY}\",\n  \"event_kind\": \"pull_request\",\n  \"resolution_source\": \"github_event\",\n  \"diff_mode\": \"merge_base\",\n  \"base_sha\": \"{base}\",\n  \"head_sha\": \"{head}\",\n  \"base_tree\": \"{}\",\n  \"head_tree\": \"{}\",\n  \"diff_base_sha\": \"{base}\",\n  \"diff_base_tree\": \"{}\",\n  \"changed_file_count\": 1,\n  \"changed_input_digest\": \"36c8a973bc6b53f4abf35ed1b950f4f1f9d6695eba0fa4aee8d959983795d2c5\",\n  \"subject_digest\": \"{}\",\n  \"error_code\": null\n}}",
        EXPECTED_BASE_TREE, EXPECTED_HEAD_TREE, EXPECTED_BASE_TREE, EXPECTED_SUBJECT_DIGEST,
    );
    ensure!(
        moved_bytes == expected_receipt.as_bytes(),
        "receipt bytes must match the independent canonical fixture"
    );

    let tampered_receipt = tmp.path().join("subject-tampered.json");
    let tampered =
        expected_receipt.replace("\"changed_file_count\": 1", "\"changed_file_count\": 2");
    ensure!(tampered != expected_receipt, "tamper control must change the receipt");
    fs::write(&tampered_receipt, tampered)?;
    let tampered_output = cargo_bin_cmd!("xtask")
        .args(["ci-scope", "--subject"])
        .arg(&tampered_receipt)
        .arg("--root")
        .arg(&repo)
        .args(["--format", "json"])
        .output()?;
    ensure!(!tampered_output.status.success(), "tampered receipt must be rejected");
    ensure!(
        String::from_utf8_lossy(&tampered_output.stderr).contains("subject digest mismatch"),
        "tampered receipt rejection must identify the invalid semantic digest"
    );

    let output = cargo_bin_cmd!("xtask")
        .args(["ci-scope", "--subject"])
        .arg(&moved_receipt)
        .arg("--root")
        .arg(&repo)
        .args(["--format", "json"])
        .output()?;
    ensure!(
        output.status.success(),
        "ci-scope subject consumer failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let scope: Value = serde_json::from_slice(&output.stdout)?;
    ensure!(scope["base"] == base, "ci-scope did not retain immutable base");
    ensure!(scope["head_sha"] == head, "ci-scope did not retain immutable head");
    ensure!(scope["diff_class"] == "code", "Rust change must remain code-relevant");
    ensure!(
        scope["direct_crates"]
            .as_array()
            .is_some_and(|crates| crates.iter().any(|entry| entry["name"] == "demo")),
        "real ci-scope consumer did not select the changed crate"
    );
    ensure!(scope["platform_overrides"]["windows_runner"] == true);
    ensure!(scope["platform_overrides"]["windows_test_crates"] == json!(["demo"]));
    let windows_test_crates = scope["platform_overrides"]["windows_test_crates"]
        .as_array()
        .context("ci-scope must emit a Windows crate projection")?;
    ensure!(!windows_test_crates.is_empty(), "Windows crate projection must not be empty");
    let crate_names = windows_test_crates
        .iter()
        .map(|crate_name| {
            crate_name.as_str().context("Windows crate projection must contain crate names")
        })
        .collect::<Result<Vec<_>>>()?;
    let package_args = crate_names
        .iter()
        .map(|crate_name| format!("-p {crate_name}"))
        .collect::<Vec<_>>()
        .join(" ");
    ensure!(
        package_args.split_whitespace().collect::<Vec<_>>()
            == crate_names.iter().flat_map(|crate_name| ["-p", *crate_name]).collect::<Vec<_>>(),
        "cache-key package arguments must be derived from the subject crate projection"
    );

    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .context("xtask manifest must have a repository parent")?
        .to_path_buf();
    let python = if cfg!(windows) { "python" } else { "python3" };
    let cache_key = Command::new(python)
        .arg(root.join("scripts/ci/scope_cache_key.py"))
        .args(["--require-non-empty", "--package-args", &package_args])
        .output()?;
    ensure!(
        cache_key.status.success(),
        "scope cache-key consumer failed: {}",
        String::from_utf8_lossy(&cache_key.stderr)
    );
    ensure!(
        String::from_utf8(cache_key.stdout)?.trim() == "2a97516c354b6884",
        "captured subject produced an unexpected Windows cache identity"
    );
    Ok(())
}

#[test]
fn producer_refuses_successful_receipt_when_checkout_head_differs() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let repo = tmp.path().join("repo");
    fs::create_dir_all(&repo)?;
    let (base, head) = init_fixture(&repo)?;
    let event = tmp.path().join("event.json");
    let receipt = tmp.path().join("failure.json");
    write_pr_event(&event, &base, &head)?;

    git(&repo, &["checkout", "main"])?;
    let output = run_event_subject_output(&repo, &event, &receipt, "pull_request", None)?;
    ensure!(!output.status.success(), "producer must reject a stale checkout HEAD");
    ensure!(
        !String::from_utf8(output.stdout)?.contains("ci subject: RESOLVED"),
        "producer must not announce a successful receipt on checkout mismatch"
    );
    let failure: Value = serde_json::from_slice(&fs::read(receipt)?)?;
    ensure!(failure["status"] == "NOT_PROVEN");
    ensure!(failure["error_code"] == "CHECKOUT_MISMATCH");
    Ok(())
}

#[test]
fn explicit_branch_name_fails_with_typed_not_proven_receipt() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let repo = tmp.path().join("repo");
    fs::create_dir_all(&repo)?;
    let (_, head) = init_fixture(&repo)?;
    let receipt = tmp.path().join("failure.json");
    let output = cargo_bin_cmd!("xtask")
        .args([
            "ci-subject",
            "--event-name",
            "explicit",
            "--repository",
            REPOSITORY,
            "--base-sha",
            "origin/main",
            "--head-sha",
            &head,
            "--receipt",
        ])
        .arg(&receipt)
        .arg("--root")
        .arg(&repo)
        .output()?;
    ensure!(!output.status.success(), "mutable branch input must fail");
    let failure: Value = serde_json::from_slice(&fs::read(receipt)?)?;
    ensure!(failure["status"] == "NOT_PROVEN");
    ensure!(failure["error_code"] == "MALFORMED_SHA");
    if output.stdout.windows("origin/main".len()).any(|window| window == b"origin/main") {
        bail!("failed resolver must not emit mutable input as successful output");
    }
    Ok(())
}

#[test]
fn ci_workflow_routes_scope_gate_contract_and_windows_cache_inputs_through_subject() -> Result<()> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .context("xtask manifest must have a repository parent")?
        .to_path_buf();
    let workflow = fs::read_to_string(root.join(".github/workflows/ci.yml"))?;
    ensure!(
        workflow.matches("ci-subject \\").count() >= 3,
        "repository-contract, platform scope, and PR smoke must resolve the shared subject"
    );
    ensure!(
        workflow.contains("ci-contract \\")
            && workflow.contains("--subject target/receipts/ci-subject.json \\")
    );
    ensure!(
        workflow.contains("ci-scope \\")
            && workflow.contains("--subject \"$RUNNER_TEMP/ci-subject.json\" \\")
    );
    ensure!(workflow.contains("ci-scope --subject target/receipts/ci-subject.json --format json"));
    ensure!(
        workflow
            .contains("gates --tier pr-fast --subject target/receipts/ci-subject.json --receipt")
    );
    ensure!(!workflow.contains("SCOPE_BASE:"), "mutable platform scope base must be removed");
    ensure!(
        !workflow.contains("ci-scope --base origin/main"),
        "candidate-bound ci-scope must not use mutable origin/main"
    );
    ensure!(
        !workflow.contains("gates --tier pr-fast --base origin/main"),
        "candidate-bound gate planning must not use mutable origin/main"
    );
    ensure!(
        !workflow.contains("&& inputs.head_sha ||"),
        "unvalidated dispatch head input must not reach checkout ref resolution"
    );
    ensure!(
        !workflow.contains("--base-sha \"${{ inputs.")
            && !workflow.contains("--head-sha \"${{ inputs."),
        "dispatch inputs must not be interpolated into shell scripts before SHA validation"
    );
    ensure!(
        workflow.matches("--base-sha \"$SUBJECT_BASE_SHA\"").count() >= 3
            && workflow.matches("--head-sha \"$SUBJECT_HEAD_SHA\"").count() >= 3,
        "subject resolver calls must consume quoted step-environment values"
    );
    ensure!(
        workflow.contains("windows_test_crates: ${{ steps.scope.outputs.windows_test_crates }}")
            && workflow.contains("needs.platform-overrides.outputs.windows_test_crates"),
        "exact-subject platform crate selection must still reach the Windows cache-key consumer"
    );
    Ok(())
}

//! Regression contracts for the CI hardening in issue #4657.

use std::fs;
use std::path::PathBuf;

fn project_root() -> Result<PathBuf, Box<dyn std::error::Error>> {
    Ok(PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("CARGO_MANIFEST_DIR has no parent")?
        .to_path_buf())
}

/// Extract one gate row from `.ci/gate-policy.yaml`.
///
/// Gates are two-space-indented `- name:` list entries under `gates:`; the
/// block runs until the next sibling entry. Returns `None` when the gate is
/// absent so callers can fail loudly rather than assert against an empty
/// string.
fn policy_gate_block<'a>(policy: &'a str, gate: &str) -> Option<&'a str> {
    let header = format!("\n  - name: {gate}\n");
    let start = policy.find(&header)? + 1;
    let rest = &policy[start..];
    let body_offset = rest.find('\n')? + 1;
    let end = rest[body_offset..]
        .match_indices('\n')
        .find(|(idx, _)| {
            let line = &rest[body_offset + idx + 1..];
            // Next sibling gate row: exactly two spaces of indent, `- name:`.
            line.starts_with("  - name:")
        })
        .map_or(rest.len(), |(idx, _)| body_offset + idx + 1);
    Some(&rest[..end])
}

/// Extract one top-level job block from a workflow file.
///
/// Jobs are keyed at two-space indentation under `jobs:`; the block runs until
/// the next key at that same indentation. Returns `None` when the job is absent
/// so callers can fail loudly rather than assert against an empty string.
fn job_block<'a>(workflow: &'a str, job: &str) -> Option<&'a str> {
    let header = format!("\n  {job}:\n");
    let start = workflow.find(&header)? + 1;
    let rest = &workflow[start..];
    let body_offset = rest.find('\n')? + 1;
    let end = rest[body_offset..]
        .match_indices('\n')
        .find(|(idx, _)| {
            let line = &rest[body_offset + idx + 1..];
            // Next top-level job: exactly two spaces of indent, then content.
            line.starts_with("  ") && !line.starts_with("   ") && !line.starts_with("  #")
        })
        .map_or(rest.len(), |(idx, _)| body_offset + idx + 1);
    Some(&rest[..end])
}

/// Extract one named step from a job block.
///
/// The Windows job contains several multiline `run: |` steps. Anchoring the
/// lookup to the step name prevents a contract from accidentally inspecting a
/// preceding helper step instead of the command it intends to guard.
fn step_block<'a>(job: &'a str, step_name: &str) -> Option<&'a str> {
    let header = format!("      - name: {step_name}\n");
    let start = job.find(&header)?;
    let rest = &job[start..];
    let end = rest.match_indices("\n      - ").next().map_or(rest.len(), |(idx, _)| idx);
    Some(&rest[..end])
}

/// Extract the script from a named step's inline or YAML-block `run` field.
fn step_run_script(step: &str) -> Option<String> {
    let mut in_block = false;
    let mut script = Vec::new();
    for line in step.lines() {
        let trimmed = line.trim();
        if in_block {
            script.push(trimmed);
        } else if let Some(command) = trimmed.strip_prefix("run: ") {
            if command == "|" {
                in_block = true;
            } else {
                return Some(command.to_string());
            }
        }
    }
    in_block.then(|| script.join("\n"))
}

/// #9594: the bit-rot guard must not pin the pull-request head SHA.
///
/// For a `pull_request` event the workflow definition comes from the base
/// branch. Pinning `head.sha` runs base's step list against the candidate's
/// tree, so a commit that adds a required step together with the file it needs
/// makes this required check fail on every older branch — on branch age rather
/// than on content. Checking out the event's own ref keeps the definition and
/// the tree one integration subject.
#[test]
fn compile_all_targets_checks_out_the_integration_subject() -> Result<(), Box<dyn std::error::Error>>
{
    let root = project_root()?;
    // Normalize to LF: `job_block` anchors on "\n  <job>:\n", which a CRLF
    // checkout would never match, turning a real contract check into a
    // confusing "job not found" error on Windows.
    let ci = fs::read_to_string(root.join(".github/workflows/ci.yml"))?.replace("\r\n", "\n");

    let job = job_block(&ci, "check-all-targets")
        .ok_or("ci.yml no longer defines a `check-all-targets` job")?;

    // Guard the extractor itself: a silently-empty block would make the
    // assertion below pass for the wrong reason.
    assert!(
        job.contains("name: Compile All Targets (bit-rot guard)")
            && job.contains("actions/checkout@"),
        "check-all-targets block was not extracted correctly; got:\n{job}"
    );

    assert!(
        !job.contains("pull_request.head.sha"),
        "the required `Compile All Targets (bit-rot guard)` job must not pin the PR head SHA \
         (#9594): the workflow definition comes from the base branch, so a head-pinned tree \
         fails on branch age rather than on content. Extracted job:\n{job}"
    );

    Ok(())
}

#[test]
fn ci_workflows_keep_issue_4657_hardening() -> Result<(), Box<dyn std::error::Error>> {
    let root = project_root()?;
    let required_checks = fs::read_to_string(root.join(".ci/policies/required-checks.toml"))?;
    let routed_rust = fs::read_to_string(root.join(".github/workflows/em-ci-routed-rust.yml"))?;
    let title_check = fs::read_to_string(root.join(".github/workflows/pr-title-check.yml"))?;
    let version_bump = fs::read_to_string(root.join(".github/workflows/version-bump.yml"))?;

    assert!(
        !required_checks.contains("parser-ratchet.yml"),
        "required-check policy must not reference the absent parser-ratchet workflow"
    );
    assert!(
        routed_rust.contains(
            "if: github.event.pull_request.draft != true || github.event_name != 'pull_request'"
        ),
        "Rust Small routing must skip draft pull requests while allowing ready_for_review"
    );
    assert!(
        routed_rust.contains("if [ \"$ROUTE_RESULT\" = \"skipped\" ]; then")
            && routed_rust.contains("required check is neutral/pass"),
        "Rust Small result aggregation must treat an intentionally skipped draft route as neutral"
    );
    assert!(
        title_check
            .contains("uses: actions/github-script@3a2844b7e9c422d3c10d287c895573f7108da1b3 # v9")
            && !title_check.contains("actions/github-script@v9"),
        "pull_request_target title validation must use an immutable github-script v9 ref"
    );
    assert!(
        version_bump.contains("GIT_CLIFF_VERSION: \"2.13.1\"")
            && version_bump.contains(
                "GIT_CLIFF_LINUX_AMD64_SHA256: \"9a1263f24e59a2f508c7b3d3283c9dea94a8bf697f96dbc18cc783cac6284546\""
            )
            && version_bump.contains("sha256sum -c -")
            && !version_bump.contains("releases/latest"),
        "version bump must use a fixed, checksum-verified git-cliff release"
    );

    Ok(())
}

/// #12693/#12752 residual: the bit-rot guard's budget envelope must stay
/// witnessed.
///
/// The required `Compile All Targets (bit-rot guard)` job runs the guarded
/// recipe `just check-all-targets`, which grew a third compile-only pass
/// (`cargo test --workspace --examples --no-run --locked`, #12650). That
/// changed the cost shape of the whole chain — justfile recipe →
/// `.ci/gate-policy.yaml` `compile_all_targets` row → `ci.yml` job watchdog —
/// while every prior contract in this file pinned only presence, name, and
/// checkout shape. A budget move therefore lands silently, exactly when the
/// envelope is least likely to have spare headroom. These exact-value pins do
/// not set budgets; they witness them, so any future change must confront and
/// consciously update this contract instead of drifting past review.
///
/// Enforcement lives on the required `Compile All Targets (bit-rot guard)`
/// lane itself (`ci.yml` executes this test as a required merge surface): no
/// gate row or other workflow ran this binary before #12863, so an unenforced
/// witness would have been indistinguishable from none.
#[test]
fn compile_all_targets_budget_envelope_stays_witnessed() -> Result<(), Box<dyn std::error::Error>> {
    let root = project_root()?;
    // Normalize to LF for the same `job_block` CRLF reason as above; both
    // reads feed new-line-anchored assertions.
    let ci = fs::read_to_string(root.join(".github/workflows/ci.yml"))?.replace("\r\n", "\n");
    let policy = fs::read_to_string(root.join(".ci/gate-policy.yaml"))?.replace("\r\n", "\n");

    let gate = policy_gate_block(&policy, "compile_all_targets")
        .ok_or(".ci/gate-policy.yaml no longer defines a `compile_all_targets` gate")?;
    // Guard the extractor itself: an empty or wrong slice would make the
    // field pins below pass (or fail) for the wrong reason.
    assert!(
        gate.contains("\n    budgets:\n"),
        "`compile_all_targets` block was not extracted correctly; got:\n{gate}"
    );

    // Budget numbers hold only for this recipe invocation; if the command
    // changes shape, its budget envelope must be re-derived, not inherited.
    assert!(
        gate.contains("\n    command: just check-all-targets\n"),
        "gate `compile_all_targets.command` must invoke `just check-all-targets`: \
         the timeout_seconds/max_duration_ms pins below describe exactly this \
         recipe chain. Extracted gate:\n{gate}"
    );
    // The runner-enforced hard timeout (three full-workspace compile passes).
    assert!(
        gate.contains("\n    timeout_seconds: 600\n"),
        "gate `compile_all_targets.timeout_seconds` drifted from 600: this is \
         the only budget the gate runner enforces, sized to the two workspace \
         checks plus the example-test pass. Raising it needs measured receipts \
         on #12693's chain; lowering it below the third pass reopens the \
         cancel-at-timeout family. Extracted gate:\n{gate}"
    );
    // The declared soft budget reviewers cite as the shared ceiling
    // (clippy_tests_kernel explicitly stays "below the compile_all_targets
    // ceiling").
    assert!(
        gate.contains("\n      max_duration_ms: 540000\n"),
        "gate `compile_all_targets.budgets.max_duration_ms` drifted from 540000: \
         other lanes derive their ceilings from this constant. Extracted \
         gate:\n{gate}"
    );

    // GitHub cancels the whole job at this ceiling regardless of inner gate
    // math; the four xtask merge-surface contract steps ride the same window,
    // so shrinking it resurrects the cancelled-contract-step family seen at
    // the previous 15m ceiling (#12650/#12712).
    let job = job_block(&ci, "check-all-targets")
        .ok_or("ci.yml no longer defines a `check-all-targets` job")?;
    assert!(
        job.contains("name: Compile All Targets (bit-rot guard)")
            && job.contains("\n    timeout-minutes: 25\n"),
        "job `check-all-targets.timeout-minutes` drifted from 25 (the wrapper \
         watchdog raised from 15 for the grown bit-rot guard); name stays \
         pinned because branch protection depends on it. Extracted job:\n{job}"
    );
    // The required job does not reach this recipe through the gate runner; it
    // has an independent `run:` step (#12863 P2). Pin that invocation too, or
    // a ci.yml-only edit could redirect the compile step while the policy row
    // — and every pin above — stays unchanged and green.
    assert!(
        job.contains("\n        run: just check-all-targets\n"),
        "job `check-all-targets` compile step drifted from `just \
         check-all-targets`: the gate-row budget pins above describe exactly \
         this invocation, so the required job must not silently execute a \
         different command under the same watchdog. Extracted job:\n{job}"
    );

    Ok(())
}

/// #14355: the Windows portability lane must admit integration targets.
///
/// The release-artifact smoke target is intentionally Unix-only at runtime,
/// but its crate-root cfg must still be compiled on Windows. The Windows lane
/// therefore retains its library execution and adds a narrow compile-only
/// command for this integration target.
#[test]
fn windows_platform_smoke_compiles_integration_targets_without_running_them()
-> Result<(), Box<dyn std::error::Error>> {
    let root = project_root()?;
    let ci = fs::read_to_string(root.join(".github/workflows/ci.yml"))?.replace("\r\n", "\n");
    let job = job_block(&ci, "windows-platform-smoke")
        .ok_or("ci.yml no longer defines a `windows-platform-smoke` job")?;
    let smoke_step = step_block(job, "Run Windows portability smoke")
        .ok_or("windows platform smoke has no named smoke step")?;
    assert!(
        !smoke_step.contains("scope_cache_key.py"),
        "Windows portability smoke contract must not inspect the preceding cache-key step"
    );
    let run = step_run_script(smoke_step).ok_or("windows platform smoke has no run command")?;

    assert!(
        run.contains("cargo test $WINDOWS_TEST_CRATES --locked --lib"),
        "Windows portability smoke must retain its library execution; command: {run}"
    );
    assert!(
        run.contains(
            "cargo test -p xtask --test release_artifact_size_smoke_script --locked --no-run"
        ),
        "Windows portability smoke must compile the Unix-only integration target without running it; command: {run}"
    );
    Ok(())
}

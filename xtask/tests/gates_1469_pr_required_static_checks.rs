//! RED TDD tests for issue #1469: Static checks required on PRs.
//!
//! These tests verify that:
//! - compile_all_targets is in pr_fast tier (moved from merge_gate)
//! - clippy_full is in pr_fast tier (moved from merge_gate)
//! - unit_routed_full gate exists in pr_fast tier (new gate)
//! - unit_routed_full uses --tests (not --lib) to catch integration test runtime failures
//! - No duplicate gate definitions across tiers
//! - GitHub Actions workflow matrix includes these gates on PRs

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use perl_tdd_support::{must, must_some};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct GatePolicyDoc {
    gates: Vec<PolicyGate>,
}

#[derive(Debug, Deserialize)]
struct PolicyGate {
    name: String,
    tier: String,
    #[serde(default = "default_true")]
    required: bool,
    #[serde(default)]
    command: String,
}

fn default_true() -> bool {
    true
}

fn project_root() -> PathBuf {
    let mut dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    dir.pop();
    dir
}

/// TEST: compile_all_targets is in pr_fast tier (red: currently in merge_gate)
#[test]
fn gate_compile_all_targets_moved_to_pr_fast() -> Result<(), Box<dyn std::error::Error>> {
    let root = project_root();
    let policy_path = root.join(".ci/gate-policy.yaml");
    let content = fs::read_to_string(policy_path)?;
    let parsed: GatePolicyDoc = serde_yaml_ng::from_str(&content)?;

    let gates: HashMap<_, _> =
        parsed.gates.into_iter().map(|gate| (gate.name.clone(), gate)).collect();

    let gate = must_some(gates.get("compile_all_targets"));

    assert_eq!(
        gate.tier, "pr_fast",
        "compile_all_targets must be in pr_fast tier (currently in merge_gate, needs move)"
    );
    assert!(gate.required, "compile_all_targets must be required on PRs");

    Ok(())
}

/// TEST: clippy_full is in pr_fast tier (red: currently in merge_gate)
#[test]
fn gate_clippy_full_moved_to_pr_fast() -> Result<(), Box<dyn std::error::Error>> {
    let root = project_root();
    let policy_path = root.join(".ci/gate-policy.yaml");
    let content = fs::read_to_string(policy_path)?;
    let parsed: GatePolicyDoc = serde_yaml_ng::from_str(&content)?;

    let gates: HashMap<_, _> =
        parsed.gates.into_iter().map(|gate| (gate.name.clone(), gate)).collect();

    let gate = must_some(gates.get("clippy_full"));

    assert_eq!(
        gate.tier, "pr_fast",
        "clippy_full must be in pr_fast tier (currently in merge_gate, needs move)"
    );
    assert!(gate.required, "clippy_full must be required on PRs");

    Ok(())
}

/// TEST: unit_routed_full gate exists in pr_fast tier (red: doesn't exist yet)
#[test]
fn gate_unit_routed_full_added_to_pr_fast() -> Result<(), Box<dyn std::error::Error>> {
    let root = project_root();
    let policy_path = root.join(".ci/gate-policy.yaml");
    let content = fs::read_to_string(policy_path)?;
    let parsed: GatePolicyDoc = serde_yaml_ng::from_str(&content)?;

    let gates: HashMap<_, _> =
        parsed.gates.into_iter().map(|gate| (gate.name.clone(), gate)).collect();

    let gate = must_some(gates.get("unit_routed_full"));

    assert_eq!(
        gate.tier, "pr_fast",
        "unit_routed_full must be in pr_fast tier for required PR test gate"
    );
    assert!(gate.required, "unit_routed_full must be required on PRs");

    Ok(())
}

/// TEST: unit_routed_full uses --tests (not --lib) to catch integration test runtime failures.
///
/// The motivating incident for #1469 was `all_kind_names_contains_every_variant` in
/// `crates/perl-ast/tests/` — a runtime assertion failure (not a compile error). That test
/// lives in the `tests/` integration-test directory and is NOT reachable by `--lib`.
/// Using `--tests` ensures the routed gate actually executes integration tests.
#[test]
fn gate_unit_routed_full_uses_tests_flag_not_lib() -> Result<(), Box<dyn std::error::Error>> {
    let root = project_root();
    let policy_path = root.join(".ci/gate-policy.yaml");
    let content = fs::read_to_string(policy_path)?;
    let parsed: GatePolicyDoc = serde_yaml_ng::from_str(&content)?;

    let gates: HashMap<_, _> =
        parsed.gates.into_iter().map(|gate| (gate.name.clone(), gate)).collect();

    let gate = must_some(gates.get("unit_routed_full"));

    assert!(
        gate.command.contains("--tests"),
        "unit_routed_full must use --tests (not --lib) to run integration tests from tests/ \
         directories; --lib only runs lib.rs inline tests and misses runtime assertion failures \
         like the 69≠70 variant-count check that motivated #1469. \
         Current command: {}",
        gate.command
    );
    assert!(
        !gate.command.contains("--lib"),
        "unit_routed_full must NOT use --lib (use --tests to run integration tests); \
         current command: {}",
        gate.command
    );

    Ok(())
}

/// TEST: All five gates are in pr_fast tier (compile_all_targets, clippy_full, unit_routed_full,
/// fmt, check_conflict_markers)
#[test]
fn pr_fast_tier_includes_all_required_static_checks() -> Result<(), Box<dyn std::error::Error>> {
    let root = project_root();
    let policy_path = root.join(".ci/gate-policy.yaml");
    let content = fs::read_to_string(policy_path)?;
    let parsed: GatePolicyDoc = serde_yaml_ng::from_str(&content)?;

    let gates: HashMap<_, _> =
        parsed.gates.into_iter().map(|gate| (gate.name.clone(), gate)).collect();

    for gate_name in
        ["fmt", "check_conflict_markers", "compile_all_targets", "clippy_full", "unit_routed_full"]
    {
        let gate = must_some(gates.get(gate_name));
        assert_eq!(gate.tier, "pr_fast", "{gate_name} must be in pr_fast tier (red TDD for #1469)");
        assert!(gate.required, "{gate_name} must be required on PRs");
    }

    Ok(())
}

/// TEST: No duplicate gate definitions (each gate name appears exactly once)
#[test]
fn no_duplicate_gate_definitions_across_tiers() -> Result<(), Box<dyn std::error::Error>> {
    let root = project_root();
    let policy_path = root.join(".ci/gate-policy.yaml");
    let content = fs::read_to_string(policy_path)?;
    let parsed: GatePolicyDoc = serde_yaml_ng::from_str(&content)?;

    // Count occurrences of each gate name
    let mut gate_counts = HashMap::new();
    for gate in parsed.gates {
        *gate_counts.entry(gate.name).or_insert(0) += 1;
    }

    // Check for duplicates (gates that appear more than once)
    for (gate_name, count) in gate_counts {
        assert_eq!(
            count, 1,
            "Gate '{gate_name}' appears {count} times (must appear exactly once to avoid tier collision)"
        );
    }

    Ok(())
}

/// TEST: compile_all_targets is NOT in merge_gate tier (it was moved to pr_fast)
#[test]
fn gate_compile_all_targets_not_in_merge_gate() -> Result<(), Box<dyn std::error::Error>> {
    let root = project_root();
    let policy_path = root.join(".ci/gate-policy.yaml");
    let content = fs::read_to_string(policy_path)?;
    let parsed: GatePolicyDoc = serde_yaml_ng::from_str(&content)?;

    let merge_gate_gates: Vec<_> =
        parsed.gates.iter().filter(|g| g.tier == "merge_gate").map(|g| g.name.clone()).collect();

    assert!(
        !merge_gate_gates.contains(&"compile_all_targets".to_string()),
        "compile_all_targets must NOT be in merge_gate tier (moved to pr_fast, red until builder implements)"
    );

    Ok(())
}

/// TEST: clippy_full is NOT in merge_gate tier (it was moved to pr_fast)
#[test]
fn gate_clippy_full_not_in_merge_gate() -> Result<(), Box<dyn std::error::Error>> {
    let root = project_root();
    let policy_path = root.join(".ci/gate-policy.yaml");
    let content = fs::read_to_string(policy_path)?;
    let parsed: GatePolicyDoc = serde_yaml_ng::from_str(&content)?;

    let merge_gate_gates: Vec<_> =
        parsed.gates.iter().filter(|g| g.tier == "merge_gate").map(|g| g.name.clone()).collect();

    assert!(
        !merge_gate_gates.contains(&"clippy_full".to_string()),
        "clippy_full must NOT be in merge_gate tier (moved to pr_fast, red until builder implements)"
    );

    Ok(())
}

/// TEST: the merge-gate-shards matrix must NOT duplicate compile_all_targets
/// (issue #15638). The dedicated required `check-all-targets` job owns
/// workspace-wide compilation; the shard copy ran the same ~19-minute command a
/// second time on the same tree and deterministically timed out against the
/// shard's shared budget while the standalone job passed on the identical
/// commit. This contract pins the de-duplication so the duplicate cannot
/// silently return.
#[test]
fn ci_workflow_includes_compile_all_targets_in_matrix() -> Result<(), Box<dyn std::error::Error>> {
    let root = project_root();
    let workflow = must(fs::read_to_string(root.join(".github/workflows/ci.yml")));

    // Check if compile_all_targets appears in the merge-gate-shards matrix section.
    // We search from the "merge-gate-shards:" label to the next top-level job definition
    // (lines starting with exactly two-space indent followed by a non-space char and ending
    // with ":"). A reliable delimiter is the "merge-gate:" job which immediately follows
    // the shards.
    let merge_gate_shards_start = must_some(workflow.find("merge-gate-shards:"));
    let rest = &workflow[merge_gate_shards_start..];

    // Use the "merge-gate:" aggregate job as the upper boundary of the shards section.
    // Fall back to searching the whole remaining file if not present.
    let next_job = rest.find("\nmerge-gate:").unwrap_or(rest.len());

    // The contract binds the shard matrix, not the prose around it: collect
    // only the `gates:` rows so an explanatory comment cannot pass or fail the
    // check for the wrong reason.
    let shard_gates: String = rest[..next_job]
        .lines()
        .filter_map(|line| line.trim().strip_prefix("gates: "))
        .collect::<Vec<_>>()
        .join(" ");

    assert!(
        !shard_gates.split_whitespace().any(|gate| gate == "compile_all_targets"),
        "merge-gate-shards matrix must not duplicate compile_all_targets (#15638): the \
         dedicated required `check-all-targets` job owns workspace-wide compilation, and \
         the shard copy deterministically times out against the shard's shared budget"
    );

    // The ownership side of the contract: the standalone required job must
    // still exist and run `just check-all-targets`, so removing the shard copy
    // never removes the coverage.
    let job_start = must_some(workflow.find("\n  check-all-targets:"));
    let job_rest = &workflow[job_start..];
    // A top-level job key sits at exactly two spaces followed by a YAML
    // identifier and a colon; job-body lines are indented at least four, and a
    // bare `  # comment` or blank line at job level is not a job boundary, so
    // match the key shape instead of any two-space line.
    let job_end = job_rest[1..]
        .match_indices('\n')
        .map(|(offset, _)| offset + 1)
        .find(|&offset| {
            let line = job_rest[1 + offset..].trim_end();
            line.starts_with("  ")
                && !line.starts_with("   ")
                && !line.trim_start().starts_with('#')
                && line[2..].ends_with(':')
                && line[2..]
                    .trim_end_matches(':')
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
        })
        .unwrap_or(job_rest.len());
    let job_section = &job_rest[..job_end];
    assert!(
        job_section.contains("just check-all-targets"),
        "the dedicated `check-all-targets` job must remain the compile_all_targets owner"
    );

    Ok(())
}

/// TEST: GitHub Actions workflow matrix includes clippy_full in merge-gate-shards
#[test]
fn ci_workflow_includes_clippy_full_in_matrix() -> Result<(), Box<dyn std::error::Error>> {
    let root = project_root();
    let workflow = must(fs::read_to_string(root.join(".github/workflows/ci.yml")));

    // Check if clippy_full appears in the merge-gate-shards matrix section.
    let merge_gate_shards_start = must_some(workflow.find("merge-gate-shards:"));
    let rest = &workflow[merge_gate_shards_start..];

    // Use the "merge-gate:" aggregate job as the upper boundary.
    let next_job = rest.find("\nmerge-gate:").unwrap_or(rest.len());
    let shards_section = &rest[..next_job];

    assert!(
        shards_section.contains("clippy_full"),
        "merge-gate-shards matrix must include clippy_full (already present, verify it's not lost)"
    );

    Ok(())
}

/// TEST: unit_routed_full runs on every PR via pr-smoke (--tier pr_fast)
///
/// The gate is in tier pr_fast, so it runs in the pr-smoke job with:
///   cargo xtask gates --tier pr-fast --subject target/receipts/ci-subject.json
/// This properly resolves {package_args} for the rust_scoped gate.
///
/// It does NOT appear in merge-gate-shards because that shard uses --gate <name>,
/// which doesn't resolve {package_args} and triggers the guard. Instead,
/// merge-gate runs MergeGate tier which includes pr_fast gates via plan_pr_fast_gates.
#[test]
fn ci_workflow_runs_unit_routed_full_in_pr_smoke() -> Result<(), Box<dyn std::error::Error>> {
    let root = project_root();
    let workflow = must(fs::read_to_string(root.join(".github/workflows/ci.yml")));
    let pr_smoke_job = workflow
        .split_once("  pr-smoke:")
        .and_then(|(_, remainder)| remainder.split_once("\n  merge-gate-shards:"))
        .map(|(job, _)| job)
        .ok_or("ci workflow must define a pr-smoke job")?;

    // Verify that pr-smoke job runs --tier pr-fast
    // which includes all pr_fast gates (including unit_routed_full)
    let has_pr_fast_tier = pr_smoke_job.contains("gates --tier pr-fast");
    let has_immutable_subject = pr_smoke_job.contains("--subject target/receipts/ci-subject.json");

    assert!(
        has_pr_fast_tier && has_immutable_subject,
        "pr-smoke job must run gates --tier pr-fast against the immutable CI subject \
         to properly resolve package_args for rust_scoped gates like unit_routed_full"
    );
    assert!(
        pr_smoke_job.contains("timeout-minutes: 75"),
        "pr-smoke job timeout must leave room for the inner watchdog and always-run receipt steps"
    );
    assert!(
        pr_smoke_job.contains("PR-fast timeout policy: GitHub job 75m, outer runner watchdog 60m"),
        "pr-smoke log message must document the active watchdog policy"
    );
    // The watchdog invocation is asserted by its durable parts rather than as one
    // literal line: the binary path spelling is incidental (it moved from
    // `./target/debug/xtask` to `"$CARGO_TARGET_DIR/debug/xtask"` in #4912), while the
    // signal, grace period, 3600s ceiling, tier, base, and --receipt are the contract.
    let parsed: serde_yaml_ng::Value = serde_yaml_ng::from_str(&workflow)?;
    let steps = parsed["jobs"]["pr-smoke"]["steps"]
        .as_sequence()
        .ok_or("pr-smoke must define workflow steps")?;
    let runners: Vec<_> = steps
        .iter()
        .filter(|step| step["name"].as_str() == Some("Run PR-fast via shared xtask gate runner"))
        .collect();
    assert_eq!(runners.len(), 1, "pr-smoke must have exactly one shared gate runner step");
    let script =
        runners[0]["run"].as_str().ok_or("the shared gate runner must have a run script")?;
    let watchdog_region = pr_fast_watchdog_region(script)?;

    for required in [
        "--signal=TERM",
        "--kill-after=60s",
        "3600s",
        "/debug/xtask",
        "gates --tier pr-fast",
        "--subject target/receipts/ci-subject.json",
        "--receipt",
    ] {
        assert!(
            watchdog_region.contains(required),
            "pr-smoke inner watchdog must leave enough room for unit_routed_full receipt \
             output; missing {required:?} in: {watchdog_region}"
        );
    }

    // Ensure the routed shard (which was a failed attempt to run unit_routed_full)
    // has been removed
    let has_routed_shard = workflow.contains("- name: routed");
    assert!(
        !has_routed_shard,
        "The routed shard must be removed — unit_routed_full is tier pr_fast \
         and runs correctly in pr-smoke, not in merge-gate-shards"
    );

    Ok(())
}

fn is_pr_fast_gate_command(line: &str) -> bool {
    let mut words = line.split_whitespace();
    let Some(binary) = words.next() else {
        return false;
    };
    let binary = if let Some(quoted) = binary.strip_prefix('"') {
        let Some(binary) = quoted.strip_suffix('"') else { return false };
        binary
    } else {
        binary
    };
    binary.ends_with("/debug/xtask")
        && binary.chars().all(|ch| ch.is_ascii_alphanumeric() || "/${}._-".contains(ch))
        && words.eq([
            "gates",
            "--tier",
            "pr-fast",
            "--subject",
            "target/receipts/ci-subject.json",
            "--receipt",
        ])
}

fn is_runner_message(line: &str) -> bool {
    let Some(message) = line.strip_prefix("echo \"").and_then(|text| text.strip_suffix('"')) else {
        return false;
    };
    !message.chars().any(|ch| matches!(ch, '"' | '`' | '\\'))
        && !message.replace("$status", "").contains('$')
}

/// Recognize the two supported runner shapes, rather than treating gate text in
/// comments, another command, or outside the timed heredoc as watchdog coverage.
/// This is a bounded workflow contract, not a general Bash parser.
fn pr_fast_watchdog_region(script: &str) -> Result<String, &'static str> {
    let lines: Vec<_> = script
        .lines()
        .filter(|line| !line.trim().is_empty() && !line.trim_start().starts_with('#'))
        .collect();
    let timed: Vec<_> = lines
        .iter()
        .enumerate()
        .filter(|(_, line)| line.trim_start().starts_with("timeout "))
        .collect();
    if timed.len() != 1 {
        return Err("pr-smoke must have one timeout watchdog around its gate runner");
    }
    let (start, line) = timed[0];
    if !lines[..start].iter().all(|line| line.trim() == "set +e" || is_runner_message(line.trim()))
    {
        return Err("unsupported commands before the timeout watchdog");
    }
    let command = line
        .trim()
        .strip_prefix("timeout --signal=TERM --kill-after=60s 3600s ")
        .ok_or("pr-smoke watchdog must retain TERM, 60s kill grace, and the 3600s ceiling")?;
    let end = if is_pr_fast_gate_command(command) {
        start
    } else {
        if command != "bash <<'PR_FAST'" {
            return Err("pr-smoke timeout must execute xtask or the PR_FAST Bash heredoc");
        }
        let end = lines[start + 1..]
            .iter()
            .position(|line| *line == "PR_FAST")
            .map(|offset| start + 1 + offset)
            .ok_or("the timed PR_FAST heredoc must have a standalone terminator")?;
        let body: Vec<_> = lines[start + 1..end].iter().map(|line| line.trim()).collect();
        let Some(tail_start) = body.len().checked_sub(4) else {
            return Err("the timed heredoc must run the gate and propagate its status");
        };
        if !is_pr_fast_gate_command(body[tail_start])
            || body[tail_start + 1..]
                != [
                    "full_status=$?",
                    "if [ \"$full_status\" -ne 0 ]; then exit \"$full_status\"; fi",
                    "exit \"$navigation_status\"",
                ]
        {
            return Err("the gate must execute inside PR_FAST and immediately propagate failure");
        }
        // Admit the reviewed navigation block as a whole. A token blacklist is
        // insufficient: Bash can spell an early exit as 'exit' or ex""it.
        // Intentional prelude changes need a paired contract-fixture update.
        let navigation: Vec<_> = PR_FAST_NAVIGATION_PRELUDE.lines().map(str::trim).collect();
        if body[..tail_start] != ["set -uo pipefail", "navigation_status=0"]
            && body[..tail_start] != navigation
        {
            return Err("the timed gate must follow a supported, reviewed navigation prelude");
        }
        end
    };
    let after: Vec<_> = lines[end + 1..].iter().map(|line| line.trim()).collect();
    let direct_status = after == ["status=$?", "exit \"$status\""];
    let logged_status = after.len() == 10
        && after[0..3] == ["status=$?", "case \"$status\" in", "124)"]
        && is_runner_message(after[3])
        && after[4..6] == [";;", "137|143)"]
        && is_runner_message(after[6])
        && after[7..] == [";;", "esac", "exit \"$status\""];
    if !direct_status && !logged_status {
        return Err("the step must capture the watchdog status immediately and exit with it");
    }
    Ok(lines[start..=end].join("\n"))
}

const PR_FAST_NAVIGATION_PRELUDE: &str = r#"set -uo pipefail
navigation_status=0
if [ "$PR_SMOKE_NAVIGATION_PROOF" = 'true' ]; then
  source_sha="$(git rev-parse --verify HEAD)"
  source_status=$?
  echo "NAVIGATION_PROOF source=$source_sha"
  for target in cross_file_goto_definition_tests navigation_regression_tests; do
    log="target/receipts/logs/navigation-${target}.log"
    target_status=1
    if ! printf 'NAVIGATION_PROOF source=%s target=%s\n' "$source_sha" "$target" | tee "$log"; then
      navigation_status=1
    fi
    if [ "$source_status" -eq 0 ]; then
      cargo test -p perl-lsp-rs --locked --test "$target" -- --test-threads=1 --color never 2>&1 | tee -a "$log"
      result=("${PIPESTATUS[@]}")
      target_status="${result[0]}"
      if [ "${result[1]}" -ne 0 ] || ! grep -Eq 'test result: ok\. [1-9][0-9]* passed; 0 failed;' "$log"; then
        target_status=1
      fi
    else
      echo 'NOT_PROVEN: current source identity prerequisite failed' | tee -a "$log"
    fi
    if ! printf 'NAVIGATION_PROOF source=%s target=%s exit=%s\n' "$source_sha" "$target" "$target_status" | tee -a "$log"; then
      navigation_status=1
    fi
    if [ "$target_status" -ne 0 ]; then navigation_status=1; fi
  done
fi"#;

const DIRECT_PR_FAST: &str = "timeout --signal=TERM --kill-after=60s 3600s ./target/debug/xtask gates --tier pr-fast --subject target/receipts/ci-subject.json --receipt\nstatus=$?\nexit \"$status\"\n";
const HEREDOC_PR_FAST: &str = "timeout --signal=TERM --kill-after=60s 3600s bash <<'PR_FAST'\nset -uo pipefail\nnavigation_status=0\n\"$CARGO_TARGET_DIR/debug/xtask\" gates --tier pr-fast --subject target/receipts/ci-subject.json --receipt\nfull_status=$?\nif [ \"$full_status\" -ne 0 ]; then exit \"$full_status\"; fi\nexit \"$navigation_status\"\nPR_FAST\nstatus=$?\nexit \"$status\"\n";

#[test]
fn pr_fast_watchdog_accepts_direct_and_heredoc_runners() {
    for script in [DIRECT_PR_FAST, HEREDOC_PR_FAST] {
        assert!(pr_fast_watchdog_region(script).is_ok(), "supported timed runner: {script}");
    }
}

#[test]
fn pr_fast_watchdog_rejects_untimed_or_status_losing_runners() {
    let cases = [
        ("missing watchdog", DIRECT_PR_FAST.replacen("timeout ", "", 1)),
        ("wrong ceiling", DIRECT_PR_FAST.replace("3600s", "60s")),
        ("wrong signal", DIRECT_PR_FAST.replace("--signal=TERM", "--signal=KILL")),
        ("wrong grace", DIRECT_PR_FAST.replace("--kill-after=60s", "--kill-after=1s")),
        (
            "unrelated timed command",
            DIRECT_PR_FAST.replace("3600s ./target", "3600s true\n./target"),
        ),
        ("printed gate", DIRECT_PR_FAST.replace("3600s ./target", "3600s echo ./target")),
        ("untimed nested shell", HEREDOC_PR_FAST.replace("3600s bash", "3600s true\nbash")),
        (
            "gate outside heredoc",
            HEREDOC_PR_FAST.replace("\n\"$CARGO_TARGET_DIR", "\nPR_FAST\n\"$CARGO_TARGET_DIR"),
        ),
        (
            "commented gate",
            HEREDOC_PR_FAST.replace("\n\"$CARGO_TARGET_DIR", "\n# \"$CARGO_TARGET_DIR"),
        ),
        (
            "nested heredoc data",
            HEREDOC_PR_FAST.replace("navigation_status=0", "navigation_status=0\ncat <<'DATA'"),
        ),
        (
            "early successful exit",
            HEREDOC_PR_FAST.replace("navigation_status=0", "navigation_status=0\nexit 0"),
        ),
        (
            "conditional gate",
            HEREDOC_PR_FAST.replace("navigation_status=0", "navigation_status=0\nif false; then"),
        ),
        ("printed outer runner", format!("cat <<'DATA'\n{HEREDOC_PR_FAST}")),
        (
            "quoted gate data",
            HEREDOC_PR_FAST.replace("navigation_status=0", "navigation_status=0\nprintf '\n"),
        ),
        (
            "inline early exit",
            HEREDOC_PR_FAST
                .replace("navigation_status=0", "navigation_status=0\necho ready;exit 0"),
        ),
        (
            "quoted early exit",
            HEREDOC_PR_FAST.replace("navigation_status=0", "navigation_status=0\n'exit' 0"),
        ),
        (
            "concatenated early exit",
            HEREDOC_PR_FAST.replace("navigation_status=0", "navigation_status=0\nex\"\"it 0"),
        ),
        (
            "escaped early exit",
            HEREDOC_PR_FAST.replace("navigation_status=0", "navigation_status=0\n\\exit 0"),
        ),
        (
            "unclosed binary quote",
            DIRECT_PR_FAST.replace("./target/debug/xtask", "\"./target/debug/xtask"),
        ),
        (
            "gate capture after command",
            HEREDOC_PR_FAST.replace("full_status=$?", "true\nfull_status=$?"),
        ),
        ("lost gate status", HEREDOC_PR_FAST.replace("full_status=$?", "full_status=0")),
        (
            "lost navigation status",
            HEREDOC_PR_FAST.replace("exit \"$navigation_status\"", "exit 0"),
        ),
        (
            "outer capture after command",
            HEREDOC_PR_FAST.replace("\nstatus=$?", "\ntrue\nstatus=$?"),
        ),
        ("lost outer status", HEREDOC_PR_FAST.replace("\nstatus=$?", "\nstatus=0")),
        ("successful step exit", HEREDOC_PR_FAST.replace("exit \"$status\"", "exit 0")),
    ];
    for (label, script) in cases {
        assert!(pr_fast_watchdog_region(&script).is_err(), "must refuse {label}: {script}");
    }
}

/// TEST: pr_fast tier has at least 5 gates (the static checks + others)
#[test]
fn pr_fast_tier_has_minimum_gate_count() -> Result<(), Box<dyn std::error::Error>> {
    let root = project_root();
    let policy_path = root.join(".ci/gate-policy.yaml");
    let content = fs::read_to_string(policy_path)?;
    let parsed: GatePolicyDoc = serde_yaml_ng::from_str(&content)?;

    let pr_fast_gates: Vec<_> =
        parsed.gates.iter().filter(|g| g.tier == "pr_fast").map(|g| g.name.clone()).collect();

    assert!(
        pr_fast_gates.len() >= 5,
        "pr_fast tier must include at least 5 gates (fmt, check_conflict_markers, compile_all_targets, clippy_full, unit_routed_full); currently has: {pr_fast_gates:?}"
    );

    Ok(())
}

/// TEST: GitHub Actions workflow matrix includes unit_parser_stack_full in merge-gate-shards (#5934)
///
/// The gate is declared `required: true` in `.ci/gate-policy.yaml` but was wired into
/// zero workflows, making the parser/lexer/parser-core lib surface unenforced in CI.
/// This test guards against that regression recurring: it fails if the shard is removed.
#[test]
fn ci_workflow_includes_unit_parser_stack_full_in_matrix() -> Result<(), Box<dyn std::error::Error>>
{
    let root = project_root();
    let workflow = must(fs::read_to_string(root.join(".github/workflows/ci.yml")));

    // Search only the merge-gate-shards section to avoid matching advisory lanes.
    let merge_gate_shards_start = must_some(workflow.find("merge-gate-shards:"));
    let rest = &workflow[merge_gate_shards_start..];
    let next_job = rest.find("\nmerge-gate:").unwrap_or(rest.len());
    let shards_section = &rest[..next_job];

    assert!(
        shards_section.contains("unit_parser_stack_full"),
        "merge-gate-shards matrix must include unit_parser_stack_full: the gate covers \
         perl-parser/perl-lexer/perl-parser-core lib tests and was declared required in \
         gate-policy.yaml but wired into zero workflows (#5934)"
    );
    assert!(
        shards_section.contains("parser_integration"),
        "merge-gate-shards matrix must include parser_integration: #6107's required bounded proof must be wired into CI"
    );

    Ok(())
}

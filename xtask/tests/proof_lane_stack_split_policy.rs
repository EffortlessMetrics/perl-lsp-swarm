//! Contract tests for the proof-lane stack split inventory.

use std::{collections::BTreeMap, error::Error, fs, path::PathBuf, process::Command};

type TestResult<T = ()> = Result<T, Box<dyn Error>>;

#[derive(Debug)]
struct ClassificationRow {
    path: String,
    slice: String,
    reason: String,
}

#[derive(Debug)]
struct SliceProofRow {
    slice: String,
    touched_surface: String,
    proof: String,
}

#[test]
fn split_plan_declares_scope_boundary_and_ordered_slices() -> TestResult {
    let plan = split_plan()?;

    for required in [
        "## Claim Boundary",
        "This lane owns repo-wide proof enforcement",
        "This lane does not own LSP 3.18 protocol behavior",
        "The current dirty stack must not merge as one PR",
        "## Landing Order",
        "## Quarantine Until Reclassified",
        "## Dirty File Classification",
        "## Coverage/RIPR Status Doc Split",
        "## Quality-Gate Helper Split",
        "## Xtask CLI Entry Split",
        "## Quality-Baseline Implementation Split",
        "## Quality-Gate Implementation Split",
        "## Split Mechanics",
        "## PR Handoff Contract",
        "## Slice Extraction Checklist",
    ] {
        assert!(plan.contains(required), "split plan missing `{required}`");
    }

    for slice in ["PR 0", "PR 1", "PR 2", "PR 3", "PR 4", "PR 5", "PR 6", "PR 7", "PR 8"] {
        assert!(plan.contains(&format!("| {slice} |")), "landing order must include {slice}");
    }

    assert!(
        !plan.contains("xtask/tests/quality_gate_cli_policy.rs"),
        "obsolete mega-test path must stay split into per-slice CLI policy tests"
    );

    Ok(())
}

#[test]
fn dirty_file_classification_assigns_key_surfaces_to_review_slices() -> TestResult {
    let plan = split_plan()?;
    let rows = classification_rows(&plan)?;
    let rows_by_path = rows_by_path(rows)?;

    for (path, expected_slice) in [
        ("docs/project/status/proof_lane_stack_split.md", "PR 0"),
        ("docs/project/CURRENT_STATUS.md", "PR 0"),
        (".github/workflows/ripr.yml", "PR 1 / PR 8"),
        ("docs/ci/ripr.md", "PR 1 / PR 8"),
        ("xtask/tests/ripr_new_gap_gate_workflow.rs", "PR 1"),
        ("codecov.yml", "PR 2"),
        (".ci/README-coverage.md", "PR 2 / PR 8"),
        ("docs/ci/codecov-rollout.md", "PR 2 / PR 8"),
        ("docs/how-to/COVERAGE.md", "PR 2 / PR 3 / PR 8"),
        ("xtask/tests/codecov_patch_gate_policy.rs", "PR 2"),
        ("docs/project/status/coverage_and_ripr_enforcement.md", "PR 0 / PR 5 / PR 6"),
        ("docs/project/status/index.md", "PR 0"),
        ("xtask/src/tasks/quality_baseline.rs", "PR 3 / PR 4"),
        ("xtask/tests/quality_gate_patch_coverage_cli_policy.rs", "PR 3"),
        ("xtask/tests/quality_gate_ripr_new_gap_cli_policy.rs", "PR 4"),
        ("policy/quality-gate-exceptions.toml", "PR 5"),
        ("xtask/tests/quality_gate_exception_policy.rs", "PR 5"),
        ("xtask/tests/quality_gate_final_enforce_cli_policy.rs", "PR 6"),
        (".github/PULL_REQUEST_TEMPLATE.md", "PR 7"),
        ("xtask/tests/quality_pr_summary_policy.rs", "PR 7"),
        (".github/workflows/ci-nightly.yml", "PR 8"),
        ("xtask/tests/quality_ci_wiring_policy.rs", "PR 8"),
        ("xtask/tests/proof_lane_stack_split_policy.rs", "PR 0"),
    ] {
        let row =
            rows_by_path.get(path).ok_or_else(|| format!("split plan must classify `{path}`"))?;
        assert_eq!(row.slice, expected_slice, "unexpected slice for `{path}`");
        assert!(
            !row.reason.trim().is_empty(),
            "classification for `{path}` must explain why the file belongs there"
        );
    }

    Ok(())
}

#[test]
fn dirty_file_classification_uses_only_known_slice_labels() -> TestResult {
    let plan = split_plan()?;
    let known_slices = known_landing_slices(&plan)?;

    for row in classification_rows(&plan)? {
        for slice in row.slice.split(" / ") {
            assert!(
                slice == "Quarantine" || known_slices.iter().any(|known| known == slice),
                "classification for `{}` uses unknown slice label `{slice}`",
                row.path
            );
        }
    }

    Ok(())
}

#[test]
fn landing_order_and_slice_extraction_checklist_cover_the_same_slices() -> TestResult {
    let plan = split_plan()?;
    let landing_slices = known_landing_slices(&plan)?;
    let checklist_slices = slice_extraction_checklist_slices(&plan)?;

    assert_eq!(
        checklist_slices, landing_slices,
        "slice extraction checklist must contain exactly the same slices as the landing order"
    );

    Ok(())
}

#[test]
fn xtask_touching_slices_include_xtask_fmt_check() -> TestResult {
    let plan = split_plan()?;

    for row in landing_order_proof_rows(&plan)?
        .into_iter()
        .chain(slice_extraction_checklist_proof_rows(&plan)?)
    {
        if row.touched_surface.contains("xtask/") || row.touched_surface.contains("xtask ") {
            assert!(
                row.proof.contains("rtk cargo fmt -p xtask --check"),
                "{} touches xtask files but does not include the xtask fmt check",
                row.slice
            );
        }
    }

    Ok(())
}

#[test]
fn every_slice_proof_includes_required_cleanup_checks() -> TestResult {
    let plan = split_plan()?;

    for row in landing_order_proof_rows(&plan)?
        .into_iter()
        .chain(slice_extraction_checklist_proof_rows(&plan)?)
    {
        for required in ["rtk git diff --check", "rtk bash scripts/storage-doctor"] {
            assert!(
                row.proof.contains(required),
                "{} proof commands must include `{required}`",
                row.slice
            );
        }
    }

    Ok(())
}

#[test]
fn dirty_worktree_paths_are_all_classified() -> TestResult {
    let plan = split_plan()?;
    let rows = classification_rows(&plan)?;
    let rows_by_path = rows_by_path(rows)?;

    for path in dirty_worktree_paths()? {
        assert!(
            classified_path_exists(&rows_by_path, &path),
            "dirty path `{path}` must be classified in the proof-lane split plan"
        );
    }

    Ok(())
}

#[test]
fn pr0_navigation_links_make_baseline_discoverable_without_policy_claims() -> TestResult {
    let root = project_root()?;
    let current_status = fs::read_to_string(root.join("docs/project/CURRENT_STATUS.md"))?;
    let status_index = fs::read_to_string(root.join("docs/project/status/index.md"))?;

    for required in [
        "| Coverage/RIPR enforcement baseline | [status/coverage_and_ripr_enforcement.md](status/coverage_and_ripr_enforcement.md) |",
        "| **Coverage/RIPR enforcement baseline** | See [status/coverage_and_ripr_enforcement.md](status/coverage_and_ripr_enforcement.md) | Human-owned baseline |",
    ] {
        assert!(
            current_status.contains(required),
            "CURRENT_STATUS.md must include PR0 baseline navigation row `{required}`"
        );
    }

    for required in [
        "| Coverage and RIPR enforcement baseline | [coverage_and_ripr_enforcement.md](coverage_and_ripr_enforcement.md) | Human | Coverage/ripr policy or baseline changes |",
    ] {
        assert!(
            status_index.contains(required),
            "status/index.md must include PR0 baseline navigation row `{required}`"
        );
    }

    for forbidden in
        ["quality-gate --mode enforce", "RIPR+ zero", "Codecov project coverage enforcement"]
    {
        assert!(
            !current_status.contains(forbidden) && !status_index.contains(forbidden),
            "PR0 navigation docs must not carry later enforcement claim `{forbidden}`"
        );
    }

    Ok(())
}

#[test]
fn coverage_ripr_status_doc_split_declares_baseline_exception_and_final_hunks() -> TestResult {
    let plan = split_plan()?;
    let status_split =
        section_block(&plan, "## Coverage/RIPR Status Doc Split", "## Quality-Gate Helper Split")?;

    for required in [
        "`docs/project/status/coverage_and_ripr_enforcement.md`",
        "PR 0",
        "measurement-only baseline commands",
        "generated receipt paths under `target/receipts/quality/*`",
        "PR 5",
        "Temporary burn-down exceptions",
        "`policy/quality-gate-exceptions.toml`",
        "`ripr-total-burndown`",
        "`project-coverage-burndown`",
        "`### Durable Policy Contract`",
        "PR 6",
        "final `quality-gate --mode enforce` contract",
        "RIPR+ zero",
        "new RIPR gaps zero",
        "Codecov project coverage enforcement after burn-down",
        "workspace coverage scope",
        "no active temporary exceptions",
        "no LSP 3.18 behavior",
        "no protocol extraction",
        "no Codecov project promotion",
    ] {
        assert!(
            status_split.contains(required),
            "coverage/ripr status doc split must include `{required}`"
        );
    }

    assert!(
        status_split.contains("it must remain measurement only")
            && status_split.contains("silently carrying temporary-exception")
            && status_split.contains("final-enforcement semantics"),
        "PR0 status-doc guidance must keep baseline hunks separate from later policy"
    );

    Ok(())
}

#[test]
fn quality_gate_helper_split_declares_function_level_ownership() -> TestResult {
    let plan = split_plan()?;
    let helper_split = section_block(&plan, "## Quality-Gate Helper Split", "## Split Mechanics")?;

    for required in [
        "`repo_root`, `current_head`, `next_action`, `next_actions_contain`",
        "`patch_quality_gate_command`, `patch_quality_gate_command_with_cli_patch`",
        "`new_ripr_quality_gate_command`, `write_ripr_plus_receipt`",
        "`write_actionable_ripr_plus_receipt`, `final_quality_gate_command`",
        "`write_workspace_coverage_receipt`, `write_project_gap_workspace_coverage_receipt`",
        "`write_final_codecov_config`, `write_advisory_project_codecov_config`",
    ] {
        assert!(
            helper_split.contains(required),
            "quality-gate helper split must assign `{required}`"
        );
    }

    assert!(
        helper_split.contains("| PR 3 | PR 4 / PR 6 / PR 7 |")
            && helper_split.contains("| PR 4 | PR 6 |")
            && helper_split.contains("| PR 6 | none |"),
        "quality-gate helper split must name first-owner and later-reuse slices"
    );
    assert!(
        helper_split.contains("Do not include final-enforce helper rows in")
            || plan.contains("Do not include final-enforce helper rows in"),
        "split mechanics must prevent PR3/PR4 from importing final-enforce fixtures"
    );

    Ok(())
}

#[test]
fn xtask_cli_entry_split_declares_mode_and_dispatch_ownership() -> TestResult {
    let plan = split_plan()?;
    let cli_split =
        section_block(&plan, "## Xtask CLI Entry Split", "## Quality-Gate Implementation Split")?;

    for required in [
        "`Commands::CoverageBaseline`, `Commands::QualityGate`",
        "`QualityGateCliMode::Advisory`, `QualityGateCliMode::EnforcePatchCoverage`",
        "`QualityGatePatchStatusSource`",
        "`Commands::CoverageBaseline` dispatch",
        "`Commands::QualityGate` dispatch",
        "`Commands::RiprPlus`, `Commands::RiprPlus` dispatch",
        "`QualityGateCliMode::EnforceNewRipr`",
        "`QualityGateCliMode::Enforce`",
        "Do not expose `--mode enforce` in PR 3 or PR 4",
        "Do not wire `ripr-plus`",
        "side effect of the patch coverage CLI slice",
    ] {
        assert!(cli_split.contains(required), "xtask CLI split must include `{required}`");
    }

    assert!(
        cli_split.contains("| PR 3 | PR 4 / PR 5 / PR 6 / PR 7 / PR 8 |")
            && cli_split.contains("| PR 4 | PR 6 / PR 7 / PR 8 |")
            && cli_split.contains("| PR 6 | PR 7 / final CI promotion |"),
        "xtask CLI split must name first-owner and later-reuse slices"
    );

    Ok(())
}

#[test]
fn quality_baseline_implementation_split_declares_measurement_ownership() -> TestResult {
    let plan = split_plan()?;
    let baseline_split = section_block(
        &plan,
        "## Quality-Baseline Implementation Split",
        "## Quality-Gate Implementation Split",
    )?;

    for required in [
        "`QUALITY_RECEIPT_SCHEMA_VERSION`, `LOCAL_COMMAND_PREFIX`, `write_or_check_receipt`",
        "`display_path`, `command_arg`, `git_head`",
        "`CoverageCounters`, `CoverageScope`, `CoverageFileRow`",
        "`CoverageBaselineReceipt`, `coverage_baseline`, `coverage_baseline_receipt`",
        "`parse_lcov`, `coverage_scope`, `required_coverage_roots`",
        "`coverage_files_below_target`, `flush_coverage_file`",
        "`CountRow`, `RiprSeamSample`, `RiprFileCluster`, `DeferredCountRow`",
        "`RiprPlusReceipt`, `ripr_plus`, `ripr_plus_receipt`",
        "`run_ripr_repo_seams`, `top_counts`, `ripr_count_field`",
        "`ripr_seam_sample`, `ripr_seam_sample_is_actionable`",
        "Do not include RIPR+ receipt code in PR 3",
        "PR 4 adds `ripr-plus`",
        "RIPR seam parsing",
    ] {
        assert!(
            baseline_split.contains(required),
            "quality-baseline split must include `{required}`"
        );
    }

    assert!(
        baseline_split.contains("| PR 3 | PR 4 / PR 6 / PR 8 |")
            && baseline_split.contains("| PR 4 | PR 6 / PR 8 |"),
        "quality-baseline split must name first-owner and later-reuse slices"
    );

    Ok(())
}

#[test]
fn quality_gate_implementation_split_declares_behavior_group_ownership() -> TestResult {
    let plan = split_plan()?;
    let implementation_split =
        section_block(&plan, "## Quality-Gate Implementation Split", "## Split Mechanics")?;

    for required in [
        "`QualityGateMode`, `PatchStatusSource`, `QualityGateConfig`",
        "`run`, `quality_gate_receipt`, `quality_gate_command_state`",
        "`CoverageGateState`, `CodecovStatusPolicy`, `CodecovCommentPolicy`",
        "`patch_coverage_policy_blockers`, `coverage_receipt_verify_command`",
        "`RiprGateState`, `RiprPrGateState`, `ReviewGuidanceState`",
        "`new_ripr_gap_action`, `ripr_pr_verify_command`, `ripr_review_verify_command`",
        "`QualityGateExceptionState`, `QualityGateException`, `QualityGateExceptionFile`",
        "`exception_state`, `exception_warnings`, `required_exception_warnings`",
        "`coverage_scope_value`, `unknown_coverage_scope`, `coverage_scope_blockers`",
        "`project_coverage_policy_blockers`, `project_policy_is_final`",
        "`render_quality_gate_markdown`, `render_pr_summary_guidance`",
        "`render_quality_gate_matrix`, `local_proof_commands`",
    ] {
        assert!(
            implementation_split.contains(required),
            "quality-gate implementation split must assign `{required}`"
        );
    }

    assert!(
        implementation_split.contains("| PR 3 | PR 4 / PR 5 / PR 6 / PR 7 |")
            && implementation_split.contains("| PR 4 | PR 6 / PR 7 |")
            && implementation_split.contains("| PR 5 | PR 6 / PR 7 |")
            && implementation_split.contains("| PR 6 | PR 7 |")
            && implementation_split.contains("| PR 7 | PR 8 summary wiring only |"),
        "quality-gate implementation split must name first-owner and later-reuse slices"
    );
    assert!(
        implementation_split
            .contains("PR 3 should introduce only the shared shell and patch coverage")
            && implementation_split.contains("PR 4 adds the RIPR row")
            && implementation_split.contains("PR 5 adds exception parsing")
            && implementation_split.contains("PR 6 adds final-enforce blockers")
            && implementation_split.contains("PR 7 adds markdown presentation"),
        "quality-gate implementation split must describe extraction order"
    );

    Ok(())
}

#[test]
fn slice_extraction_checklist_names_include_exclude_and_proof_for_each_slice() -> TestResult {
    let plan = split_plan()?;
    let checklist = section_block(&plan, "## Slice Extraction Checklist", "## Per-Slice Cleanup")?;

    for required in [
        "Each slice starts from a clean integration base",
        "Do not use `git stash`; it is shared across worktrees",
        "| Slice | Include | Exclude | Proof before handoff |",
        "| PR 0 | Split inventory docs",
        "| PR 1 | `.github/workflows/ripr.yml` ready-for-review/no-path-filter routing hunks",
        "`quality-gate --mode enforce-new-ripr`, blocking artifact checks",
        "| PR 2 | `codecov.yml` patch/project policy shape",
        "| PR 3 | `coverage-baseline`, `quality-gate --mode enforce-patch-coverage`",
        "| PR 4 | `ripr-plus`, `quality-gate --mode enforce-new-ripr`",
        "| PR 5 | `policy/quality-gate-exceptions.toml`",
        "| PR 6 | Final `quality-gate --mode enforce`",
        "| PR 7 | PR template quality-proof block",
        "| PR 8 | First blocking CI wiring for new RIPR gaps and patch coverage",
        "only files assigned to that slice are dirty",
        "If a dirty file is listed as",
        "`Quarantine`, leave it behind",
    ] {
        assert!(
            checklist.contains(required),
            "slice extraction checklist must include `{required}`"
        );
    }

    for command in [
        "rtk cargo test -p xtask --test proof_lane_stack_split_policy --profile agent --locked",
        "rtk cargo test -p xtask --test ripr_new_gap_gate_workflow --profile agent --locked",
        "rtk cargo test -p xtask --test codecov_patch_gate_policy --profile agent --locked",
        "rtk cargo test -p xtask --test quality_gate_patch_coverage_cli_policy --profile agent --locked",
        "rtk cargo test -p xtask --test quality_gate_ripr_new_gap_cli_policy --profile agent --locked",
        "rtk cargo test -p xtask --test quality_gate_exception_policy --profile agent --locked",
        "rtk cargo test -p xtask --test quality_gate_final_enforce_cli_policy --profile agent --locked",
        "rtk cargo test -p xtask --test quality_pr_summary_policy --profile agent --locked",
        "rtk cargo test -p xtask --test quality_ci_wiring_policy --profile agent --locked",
        "rtk git diff --check",
        "rtk bash scripts/storage-doctor",
    ] {
        assert!(
            checklist.contains(command),
            "slice extraction checklist must include proof command `{command}`"
        );
    }

    Ok(())
}

#[test]
fn pr_handoff_contract_requires_evidence_backed_quality_proof_fields() -> TestResult {
    let plan = split_plan()?;
    let handoff = section_block(&plan, "## PR Handoff Contract", "## Per-Slice Cleanup")?;

    for required in [
        "Every extracted slice must fill the PR template's `Quality Proof` block",
        "evidence, not intent",
        "Lane: check `coverage / proof / enforcement`",
        "Objective: one sentence matching the slice objective",
        "Claim boundary: state the proof surface this PR owns",
        "Non-goals: explicitly name no LSP 3.18 behavior",
        "no protocol extraction",
        "release work",
        "RIPR/coverage effect: state whether the slice is measurement-only",
        "CLI-contract-only",
        "presentation-only",
        "blocking",
        "Local proof commands and pass/fail results",
        "`quality-gate`",
        "receipt command",
        "Cleanup performed: report `rtk git status --short --branch`, `rtk git diff",
        "`rtk bash scripts/storage-doctor`",
        "What remains: name advisory burn-down debt",
        "`ripr-total-burndown`",
        "`project-coverage-burndown`",
        "must tell the next agent exactly what was proven",
        "command to run locally",
    ] {
        assert!(handoff.contains(required), "PR handoff contract must include `{required}`");
    }

    Ok(())
}

#[test]
fn quarantine_entries_are_not_mixed_into_proof_gate_slices() -> TestResult {
    let plan = split_plan()?;
    let rows = classification_rows(&plan)?;
    let rows_by_path = rows_by_path(rows)?;

    for path in [
        ".perl-lsp/goals/active.toml",
        "crates/perl-lsp-ux-tests/fixtures/editor_ux_fixture_matrix.json",
        "crates/perl-lsp-ux-tests/tests/ux_scenario_46_receiver_real_workspace_quality.rs",
        "docs/project/status/provider_confidence_matrix.md",
        "docs/project/status/real_perl_editor_trust_v1.md",
        "docs/project/status/receiver_facts.md",
        "docs/reference/STABILITY.md",
        "plans/real-perl-editor-trust/implementation-plan.md",
        "xtask/src/tasks/update_status/parser/metrics.rs",
        "xtask/src/tasks/worktree_allocator.rs",
        "xtask/tests/wave1_perl_module_collapse_tests.rs",
        "xtask/tests/wave_g1a_collapse_tests.rs",
    ] {
        let row = rows_by_path
            .get(path)
            .ok_or_else(|| format!("split plan must classify quarantined path `{path}`"))?;
        assert_eq!(row.slice, "Quarantine", "`{path}` must stay out of proof-gate PR slices");
    }

    Ok(())
}

#[test]
fn quarantine_list_and_classification_table_stay_in_sync() -> TestResult {
    let plan = split_plan()?;
    let quarantine = quarantine_paths(&plan)?;
    let rows = classification_rows(&plan)?;
    let rows_by_path = rows_by_path(rows)?;
    let landing_order =
        section_block(&plan, "## Landing Order", "## Quarantine Until Reclassified")?;

    for path in &quarantine {
        let row = rows_by_path.get(path).ok_or_else(|| {
            format!("quarantined path `{path}` must appear in classification table")
        })?;
        assert_eq!(row.slice, "Quarantine", "`{path}` must be classified as Quarantine");
        assert!(
            !landing_order.contains(path),
            "`{path}` must not appear in the PR0-PR8 landing-order primary files"
        );
    }

    for row in rows_by_path.values().filter(|row| row.slice == "Quarantine") {
        assert!(
            quarantine.contains(&row.path),
            "`{}` is classified as Quarantine but missing from the quarantine list",
            row.path
        );
    }

    Ok(())
}

fn section_block<'a>(
    content: &'a str,
    start_heading: &str,
    end_heading: &str,
) -> TestResult<&'a str> {
    let start =
        content.find(start_heading).ok_or_else(|| format!("missing section {start_heading}"))?;
    let section = &content[start..];
    let end = section
        .find(&format!("\n{end_heading}"))
        .ok_or_else(|| format!("section {start_heading} must end before {end_heading}"))?;
    Ok(&section[..end])
}

fn split_plan() -> TestResult<String> {
    Ok(fs::read_to_string(project_root()?.join("docs/project/status/proof_lane_stack_split.md"))?)
}

fn project_root() -> TestResult<PathBuf> {
    Ok(PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("CARGO_MANIFEST_DIR has no parent")?
        .to_path_buf())
}

fn known_landing_slices(plan: &str) -> TestResult<Vec<String>> {
    Ok(landing_order_proof_rows(plan)?.into_iter().map(|row| row.slice).collect())
}

fn landing_order_proof_rows(plan: &str) -> TestResult<Vec<SliceProofRow>> {
    let landing_order =
        section_block(plan, "## Landing Order", "## Quarantine Until Reclassified")?;
    table_slice_proof_rows(landing_order, 2, 5, "landing order")
}

fn slice_extraction_checklist_slices(plan: &str) -> TestResult<Vec<String>> {
    Ok(slice_extraction_checklist_proof_rows(plan)?.into_iter().map(|row| row.slice).collect())
}

fn slice_extraction_checklist_proof_rows(plan: &str) -> TestResult<Vec<SliceProofRow>> {
    let checklist = section_block(plan, "## Slice Extraction Checklist", "## Per-Slice Cleanup")?;
    table_slice_proof_rows(checklist, 1, 3, "slice extraction checklist")
}

fn table_slice_proof_rows(
    table: &str,
    touched_surface_column: usize,
    proof_column: usize,
    table_name: &str,
) -> TestResult<Vec<SliceProofRow>> {
    let mut slices = Vec::new();

    for line in table.lines().filter(|line| line.starts_with("| PR ")) {
        let columns: Vec<_> = line.trim_matches('|').split('|').map(str::trim).collect();
        if columns.len() <= proof_column || columns.len() <= touched_surface_column {
            return Err(format!("{table_name} row has too few columns: {line}").into());
        }
        slices.push(SliceProofRow {
            slice: columns[0].to_string(),
            touched_surface: columns[touched_surface_column].to_string(),
            proof: columns[proof_column].to_string(),
        });
    }

    if slices.is_empty() {
        return Err(format!("{table_name} must contain PR slice rows").into());
    }

    Ok(slices)
}

fn dirty_worktree_paths() -> TestResult<Vec<String>> {
    let output = Command::new("git")
        .args(["status", "--short", "--untracked-files=normal"])
        .current_dir(project_root()?)
        .output()?;

    if !output.status.success() {
        return Err(
            format!("git status failed: {}", String::from_utf8_lossy(&output.stderr)).into()
        );
    }

    let stdout = String::from_utf8(output.stdout)?;
    let mut paths = Vec::new();
    for line in stdout.lines().filter(|line| line.len() > 3) {
        let path = line[3..]
            .split_once(" -> ")
            .map_or(&line[3..], |(_, new_path)| new_path)
            .replace('\\', "/");
        paths.push(path);
    }
    paths.sort();
    paths.dedup();
    Ok(paths)
}

fn classified_path_exists(rows_by_path: &BTreeMap<String, ClassificationRow>, path: &str) -> bool {
    rows_by_path.contains_key(path)
        || path.ends_with('/')
            && rows_by_path.keys().any(|classified_path| classified_path.starts_with(path))
}

fn classification_rows(plan: &str) -> TestResult<Vec<ClassificationRow>> {
    let start = plan
        .find("## Dirty File Classification")
        .ok_or("split plan is missing dirty file classification")?;
    let section = &plan[start..];
    let end = section
        .find("\n## Coverage/RIPR Status Doc Split")
        .or_else(|| section.find("\n## Quality-Gate Helper Split"))
        .or_else(|| section.find("\n## Split Mechanics"))
        .ok_or("dirty file classification must end before the next section")?;
    let table = &section[..end];
    let mut rows = Vec::new();

    for line in table.lines().filter(|line| line.starts_with("| `")) {
        let columns: Vec<_> = line.trim_matches('|').split('|').map(str::trim).collect();
        if columns.len() != 3 {
            return Err(format!("classification row must have 3 columns: {line}").into());
        }
        rows.push(ClassificationRow {
            path: strip_backticks(columns[0]).to_string(),
            slice: columns[1].to_string(),
            reason: columns[2].to_string(),
        });
    }

    if rows.is_empty() {
        return Err("dirty file classification table must contain path rows".into());
    }

    Ok(rows)
}

fn quarantine_paths(plan: &str) -> TestResult<Vec<String>> {
    let section =
        section_block(plan, "## Quarantine Until Reclassified", "## Dirty File Classification")?;
    let mut paths = Vec::new();

    for line in section.lines().filter(|line| line.starts_with("- `")) {
        let path = line.trim_start_matches("- `").trim_end_matches('`').to_string();
        paths.push(path);
    }

    if paths.is_empty() {
        return Err("quarantine list must contain path bullets".into());
    }

    Ok(paths)
}

fn rows_by_path(rows: Vec<ClassificationRow>) -> TestResult<BTreeMap<String, ClassificationRow>> {
    let mut rows_by_path = BTreeMap::new();
    for row in rows {
        if rows_by_path.insert(row.path.clone(), row).is_some() {
            return Err("dirty file classification table must not contain duplicate paths".into());
        }
    }
    Ok(rows_by_path)
}

fn strip_backticks(value: &str) -> &str {
    value.trim().trim_start_matches('`').trim_end_matches('`')
}

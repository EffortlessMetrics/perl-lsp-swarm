mod coverage;
mod io;
mod model;
mod proof_packs;
mod route;
mod summary;

use color_eyre::eyre::Result;
use std::path::PathBuf;

use io::{git_changed_files, normalize_changed_files, write_receipt, write_text};
use route::route_receipt;
use summary::render_summary;

#[derive(Debug, Clone)]
pub struct CiRouteArgs {
    pub base: String,
    pub head: String,
    pub receipt: PathBuf,
    pub summary: PathBuf,
    pub changed_files: Vec<String>,
}

pub fn run(args: CiRouteArgs) -> Result<()> {
    let changed_files = if args.changed_files.is_empty() {
        git_changed_files(&args.base, &args.head)?
    } else {
        normalize_changed_files(args.changed_files)
    };
    let receipt = route_receipt(&args.base, &args.head, changed_files)?;
    write_receipt(&args.receipt, &receipt)?;
    let markdown = render_summary(&args.receipt, &args.summary, &receipt);
    write_text(&args.summary, &markdown)?;
    println!(
        "ci route receipt OK: {} changed files, {} proof packs, receipt {} summary {}",
        receipt.changed_files.len(),
        receipt.required_proof_packs.len(),
        args.receipt.display(),
        args.summary.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::coverage::{
        NON_LCOV_COVERAGE_SKIP_REASON, NON_SOURCE_LCOV_COVERAGE_SKIP_REASON,
        coverage_pack_manifest, coverage_proof_pack_receipts, coverage_proof_pack_selection,
        parse_coverage_pack_manifest,
    };
    use super::model::CiRouteReceipt;
    use super::*;
    use color_eyre::eyre::{bail, eyre};
    use serde_json::Value;
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    #[test]
    fn ci_route_receipt_maps_supported_editor_smoke_to_focused_pack() -> Result<()> {
        let receipt = route_receipt(
            "origin/main",
            "HEAD",
            vec!["xtask/src/tasks/supported_editor_inline_smoke.rs".to_string()],
        )?;

        assert_eq!(receipt.changed_surfaces, vec!["xtask-supported-editor-inline-smoke"]);
        assert!(proof_pack_ids(&receipt).contains(&"xtask-supported-editor-inline-smoke"));
        assert!(
            receipt
                .coverage_pack_selector
                .iter()
                .any(|pack| pack == "patch-coverage-xtask-supported-editor-inline-smoke")
        );
        assert!(receipt.coverage_proof_packs.iter().any(|pack| {
            pack.id == "patch-coverage-xtask-supported-editor-inline-smoke"
                && pack
                    .commands
                    .iter()
                    .any(|command| command.contains("supported_editor_inline_smoke"))
                && pack
                    .coverage_filters
                    .iter()
                    .any(|filter| filter == "supported_editor_inline_smoke")
        }));
        assert_eq!(
            receipt.skipped_by_policy.get("full-ux-regression").map(String::as_str),
            Some("supported-editor smoke receipt change")
        );
        Ok(())
    }

    #[test]
    fn ci_route_receipt_maps_semantic_inline_receipts_to_dashboard_pack() -> Result<()> {
        let receipt = route_receipt(
            "origin/main",
            "HEAD",
            vec!["xtask/src/tasks/semantic_inline_receipts.rs".to_string()],
        )?;

        assert_eq!(receipt.changed_surfaces, vec!["xtask-semantic-inline-receipts"]);
        assert!(proof_pack_ids(&receipt).contains(&"xtask-semantic-inline-receipts"));
        assert!(
            receipt
                .coverage_pack_selector
                .iter()
                .any(|pack| pack == "patch-coverage-xtask-semantic-inline")
        );
        assert!(receipt.coverage_proof_packs.iter().any(|pack| {
            pack.id == "patch-coverage-xtask-semantic-inline"
                && pack.commands.iter().any(|command| command.contains("semantic_inline_receipts"))
        }));
        Ok(())
    }

    #[test]
    fn ci_route_receipt_skips_coverage_for_docs_only_changes() -> Result<()> {
        let receipt = route_receipt(
            "origin/main",
            "HEAD",
            vec!["docs/development/INLINE_COMPLETION_ROADMAP.md".to_string()],
        )?;

        assert_eq!(receipt.changed_surfaces, vec!["docs"]);
        assert!(proof_pack_ids(&receipt).contains(&"docs-focused"));
        assert!(receipt.coverage_pack_selector.is_empty());
        assert!(receipt.coverage_proof_packs.is_empty());
        assert_eq!(
            receipt.skipped_by_policy.get("codecov-patch-95").map(String::as_str),
            Some("docs-only change")
        );
        Ok(())
    }

    #[test]
    fn route_receipt_maps_ci_route_files_to_focused_non_lcov_route_pack() -> Result<()> {
        let receipt =
            route_receipt("origin/main", "HEAD", vec!["xtask/src/tasks/ci_route.rs".to_string()])?;

        assert_eq!(receipt.changed_surfaces, vec!["ci-routing"]);
        assert!(proof_pack_ids(&receipt).contains(&"ci-route-receipt"));
        assert!(receipt.coverage_pack_selector.is_empty());
        assert!(receipt.coverage_proof_packs.is_empty());
        assert_eq!(
            receipt.skipped_by_policy.get("patch-coverage-ci-route").map(String::as_str),
            Some(NON_LCOV_COVERAGE_SKIP_REASON)
        );
        Ok(())
    }

    #[test]
    fn route_receipt_maps_codecov_router_script_to_focused_non_lcov_route_pack() -> Result<()> {
        let receipt = route_receipt(
            "origin/main",
            "HEAD",
            vec!["scripts/ci/route-codecov-packs.py".to_string()],
        )?;

        assert_eq!(receipt.changed_surfaces, vec!["ci-routing"]);
        assert!(proof_pack_ids(&receipt).contains(&"ci-route-receipt"));
        assert!(receipt.required_proof_packs.iter().any(|pack| {
            pack.id == "ci-route-receipt"
                && pack.commands.iter().any(|command| {
                    command == "python -m unittest scripts/ci/test_route_codecov_packs.py"
                })
        }));
        assert!(receipt.coverage_pack_selector.is_empty());
        assert!(receipt.coverage_proof_packs.is_empty());
        assert_eq!(
            receipt.skipped_by_policy.get("patch-coverage-ci-route").map(String::as_str),
            Some(NON_LCOV_COVERAGE_SKIP_REASON)
        );
        Ok(())
    }

    #[test]
    fn ci_route_receipt_maps_ci_policy_tests_to_focused_non_lcov_policy_pack() -> Result<()> {
        let receipt = route_receipt(
            "origin/main",
            "HEAD",
            vec!["xtask/tests/quality_ci_wiring_policy.rs".to_string()],
        )?;

        assert_eq!(receipt.changed_surfaces, vec!["ci-policy"]);
        assert!(proof_pack_ids(&receipt).contains(&"ci-policy-focused"));
        assert!(receipt.coverage_pack_selector.is_empty());
        assert!(receipt.coverage_proof_packs.is_empty());
        assert_eq!(
            receipt.skipped_by_policy.get("patch-coverage-ci-policy").map(String::as_str),
            Some(NON_LCOV_COVERAGE_SKIP_REASON)
        );
        Ok(())
    }

    #[test]
    fn ci_route_receipt_maps_ci_classifier_script_to_focused_policy_pack() -> Result<()> {
        let receipt =
            route_receipt("origin/main", "HEAD", vec!["scripts/ci/ci_classify.py".to_string()])?;

        assert_eq!(receipt.changed_surfaces, vec!["ci-policy"]);
        assert!(proof_pack_ids(&receipt).contains(&"ci-policy-focused"));
        assert!(receipt.required_proof_packs.iter().any(|pack| {
            pack.id == "ci-policy-focused"
                && pack
                    .commands
                    .iter()
                    .any(|command| command == "python -m unittest scripts/ci/test_ci_classify.py")
        }));
        assert!(receipt.coverage_pack_selector.is_empty());
        assert!(receipt.coverage_proof_packs.is_empty());
        assert_eq!(
            receipt.skipped_by_policy.get("patch-coverage-ci-policy").map(String::as_str),
            Some(NON_LCOV_COVERAGE_SKIP_REASON)
        );
        Ok(())
    }

    #[test]
    fn ci_route_receipt_maps_ci_actuals_script_to_focused_non_lcov_actuals_pack() -> Result<()> {
        let receipt = route_receipt(
            "origin/main",
            "HEAD",
            vec!["scripts/ci/emit_ci_actuals.py".to_string()],
        )?;

        assert_eq!(receipt.changed_surfaces, vec!["ci-actuals"]);
        assert!(proof_pack_ids(&receipt).contains(&"ci-actuals-focused"));
        assert!(receipt.required_proof_packs.iter().any(|pack| {
            pack.id == "ci-actuals-focused"
                && pack.commands.iter().any(|command| {
                    command == "python -m unittest scripts/ci/test_emit_ci_actuals.py"
                })
        }));
        assert!(receipt.coverage_pack_selector.is_empty());
        assert!(receipt.coverage_proof_packs.is_empty());
        assert_eq!(
            receipt.skipped_by_policy.get("patch-coverage-ci-actuals").map(String::as_str),
            Some(NON_LCOV_COVERAGE_SKIP_REASON)
        );
        Ok(())
    }

    #[test]
    fn ci_route_receipt_maps_ripr_summary_script_to_focused_non_lcov_summary_pack() -> Result<()> {
        let receipt =
            route_receipt("origin/main", "HEAD", vec!["scripts/ci/ripr_summary.py".to_string()])?;

        assert_eq!(receipt.changed_surfaces, vec!["ripr-summary"]);
        assert!(proof_pack_ids(&receipt).contains(&"ripr-summary-focused"));
        assert!(receipt.required_proof_packs.iter().any(|pack| {
            pack.id == "ripr-summary-focused"
                && pack
                    .commands
                    .iter()
                    .any(|command| command == "python -m unittest scripts/ci/test_ripr_summary.py")
        }));
        assert!(receipt.coverage_pack_selector.is_empty());
        assert!(receipt.coverage_proof_packs.is_empty());
        assert_eq!(
            receipt.skipped_by_policy.get("patch-coverage-ripr-summary").map(String::as_str),
            Some(NON_LCOV_COVERAGE_SKIP_REASON)
        );
        Ok(())
    }

    #[test]
    fn ci_route_receipt_maps_completion_provider_to_focused_pack() -> Result<()> {
        let receipt = route_receipt(
            "origin/main",
            "HEAD",
            vec![
                "crates/perl-lsp-rs-core/src/providers/completion/completion/import_map/used_modules.rs"
                    .to_string(),
            ],
        )?;

        assert_eq!(receipt.changed_surfaces, vec!["completion-core"]);
        assert!(proof_pack_ids(&receipt).contains(&"completion-core"));
        assert!(
            receipt
                .coverage_pack_selector
                .iter()
                .any(|pack| pack == "patch-coverage-completion-core")
        );
        assert!(receipt.coverage_proof_packs.iter().any(|pack| {
            pack.id == "patch-coverage-completion-core"
                && pack.commands.iter().any(|command| command.contains("completion::completion"))
                && pack.coverage_filters.iter().any(|filter| filter == "completion::completion")
        }));
        Ok(())
    }

    #[test]
    fn ci_route_coverage_proof_pack_receipts_materializes_each_selected_pack() -> Result<()> {
        let selector = vec![
            "patch-coverage-xtask-semantic-inline".to_string(),
            "patch-coverage-xtask-supported-editor-inline-smoke".to_string(),
        ];
        let packs = coverage_proof_pack_receipts(&selector)?;

        let pack_ids: Vec<&str> = packs.iter().map(|pack| pack.id.as_str()).collect();
        assert_eq!(
            pack_ids,
            vec![
                "patch-coverage-xtask-semantic-inline",
                "patch-coverage-xtask-supported-editor-inline-smoke"
            ]
        );

        let semantic_pack = packs.first().ok_or_else(|| eyre!("missing semantic coverage pack"))?;
        assert_eq!(
            semantic_pack.files,
            vec![
                "xtask/src/tasks/semantic_inline_receipts.rs",
                "xtask/src/tasks/semantic_inline_next_edit.rs",
                "xtask/tests/semantic_inline_receipts_cli.rs",
                "xtask/tests/semantic_inline_next_edit_cli.rs",
            ]
        );
        assert_eq!(
            semantic_pack.coverage_filters,
            vec!["semantic_inline_receipts", "semantic_inline_next_edit"]
        );

        let supported_editor_pack =
            packs.get(1).ok_or_else(|| eyre!("missing supported-editor coverage pack"))?;
        assert_eq!(
            supported_editor_pack.files,
            vec![
                "xtask/src/tasks/supported_editor_inline_smoke.rs",
                "xtask/tests/supported_editor_inline_smoke_cli.rs",
            ]
        );
        assert_eq!(
            supported_editor_pack.coverage_filters,
            vec!["supported_editor_inline_smoke", "semantic_inline_receipts"]
        );
        assert!(supported_editor_pack.commands.iter().any(|command| {
            command
                == "cargo test -p xtask --test supported_editor_inline_smoke_cli --profile agent --locked -- --nocapture"
        }));
        Ok(())
    }

    #[test]
    fn ci_route_coverage_proof_pack_receipts_reports_unknown_selector() -> Result<()> {
        let selector = vec!["patch-coverage-missing-pack".to_string()];
        let Err(error) = coverage_proof_pack_receipts(&selector) else {
            bail!("unknown coverage selector should fail");
        };
        assert_eq!(
            error.to_string(),
            "coverage pack `patch-coverage-missing-pack` is missing from .ci/coverage-packs.toml"
        );
        Ok(())
    }

    #[test]
    fn ci_route_coverage_pack_manifest_rejects_empty_pack_id() -> Result<()> {
        let Err(error) = parse_coverage_pack_manifest(
            r#"
                [[pack]]
                id = " "
                files = ["xtask/src/tasks/ci_route.rs"]
                commands = ["cargo test -p xtask ci_route"]
                coverage_filters = ["ci_route"]
            "#,
        ) else {
            bail!("empty coverage pack id should fail");
        };
        assert_eq!(error.to_string(), "coverage pack id must not be empty");
        Ok(())
    }

    #[test]
    fn ci_route_coverage_pack_manifest_rejects_empty_file_list() -> Result<()> {
        let Err(error) = parse_coverage_pack_manifest(
            r#"
                [[pack]]
                id = "patch-coverage-ci-route"
                files = []
                commands = ["cargo test -p xtask ci_route"]
                coverage_filters = ["ci_route"]
            "#,
        ) else {
            bail!("coverage pack without files should fail");
        };
        assert_eq!(
            error.to_string(),
            "coverage pack `patch-coverage-ci-route` must list at least one file"
        );
        Ok(())
    }

    #[test]
    fn ci_route_coverage_pack_manifest_rejects_empty_command_list() -> Result<()> {
        let Err(error) = parse_coverage_pack_manifest(
            r#"
                [[pack]]
                id = "patch-coverage-ci-route"
                files = ["xtask/src/tasks/ci_route.rs"]
                commands = []
                coverage_filters = ["ci_route"]
            "#,
        ) else {
            bail!("coverage pack without commands should fail");
        };
        assert_eq!(
            error.to_string(),
            "coverage pack `patch-coverage-ci-route` must list at least one command"
        );
        Ok(())
    }

    #[test]
    fn ci_route_coverage_pack_manifest_rejects_empty_coverage_filter_list() -> Result<()> {
        let Err(error) = parse_coverage_pack_manifest(
            r#"
                [[pack]]
                id = "patch-coverage-ci-route"
                files = ["xtask/src/tasks/ci_route.rs"]
                commands = ["cargo test -p xtask ci_route"]
                coverage_filters = []
            "#,
        ) else {
            bail!("coverage pack without filters should fail");
        };
        assert_eq!(
            error.to_string(),
            "coverage pack `patch-coverage-ci-route` must list at least one coverage filter"
        );
        Ok(())
    }

    #[test]
    fn ci_route_coverage_pack_manifest_rejects_duplicate_pack_id() -> Result<()> {
        let Err(error) = parse_coverage_pack_manifest(
            r#"
                [[pack]]
                id = "patch-coverage-ci-route"
                files = ["xtask/src/tasks/ci_route.rs"]
                commands = ["cargo test -p xtask ci_route"]
                coverage_filters = ["ci_route"]

                [[pack]]
                id = "patch-coverage-ci-route"
                files = ["xtask/tests/ci_route_cli.rs"]
                commands = ["cargo test -p xtask --test ci_route_cli"]
                coverage_filters = ["ci_route_cli"]
            "#,
        ) else {
            bail!("duplicate coverage pack id should fail");
        };
        assert_eq!(error.to_string(), "duplicate coverage pack id `patch-coverage-ci-route`");
        Ok(())
    }

    #[test]
    fn ci_route_coverage_pack_manifest_lists_every_route_selector() -> Result<()> {
        let manifest = coverage_pack_manifest()?;
        let manifest_ids: Vec<&str> = manifest.pack.iter().map(|pack| pack.id.as_str()).collect();

        assert_eq!(
            manifest_ids,
            vec![
                "patch-coverage-xtask-supported-editor-inline-smoke",
                "patch-coverage-xtask-semantic-inline",
                "patch-coverage-inline-core",
                "patch-coverage-completion-core",
                "patch-coverage-ux-scenario",
                "patch-coverage-ci-policy",
                "patch-coverage-ci-route",
                "patch-coverage-ci-actuals",
                "patch-coverage-ripr-summary",
                "patch-coverage-rust-focused",
            ]
        );
        let route_selectors = [
            "patch-coverage-xtask-semantic-inline",
            "patch-coverage-xtask-supported-editor-inline-smoke",
            "patch-coverage-inline-core",
            "patch-coverage-completion-core",
            "patch-coverage-ux-scenario",
            "patch-coverage-ci-policy",
            "patch-coverage-ci-route",
            "patch-coverage-ci-actuals",
            "patch-coverage-ripr-summary",
            "patch-coverage-rust-focused",
        ];
        let changed_files = vec![
            "xtask/src/tasks/semantic_inline_receipts.rs".to_string(),
            "xtask/src/tasks/supported_editor_inline_smoke.rs".to_string(),
            "crates/perl-lsp-rs-core/src/providers/inline_completion/engine.rs".to_string(),
            "crates/perl-lsp-rs-core/src/providers/completion/completion/import_map/used_modules.rs"
                .to_string(),
            "crates/perl-parser/src/lib.rs".to_string(),
        ];
        let (selected, skipped, proof_packs) = coverage_proof_pack_selection(
            &route_selectors.iter().map(|selector| (*selector).to_string()).collect::<Vec<_>>(),
            &changed_files,
        )?;
        assert_eq!(
            selected,
            vec![
                "patch-coverage-xtask-semantic-inline",
                "patch-coverage-xtask-supported-editor-inline-smoke",
                "patch-coverage-inline-core",
                "patch-coverage-completion-core",
                "patch-coverage-rust-focused",
            ]
        );
        assert_eq!(
            skipped.get("patch-coverage-ci-policy").map(String::as_str),
            Some(NON_LCOV_COVERAGE_SKIP_REASON)
        );
        assert_eq!(
            skipped.get("patch-coverage-ci-route").map(String::as_str),
            Some(NON_LCOV_COVERAGE_SKIP_REASON)
        );
        assert_eq!(
            skipped.get("patch-coverage-ci-actuals").map(String::as_str),
            Some(NON_LCOV_COVERAGE_SKIP_REASON)
        );
        assert_eq!(
            skipped.get("patch-coverage-ripr-summary").map(String::as_str),
            Some(NON_LCOV_COVERAGE_SKIP_REASON)
        );
        assert_eq!(
            skipped.get("patch-coverage-ux-scenario").map(String::as_str),
            Some(NON_SOURCE_LCOV_COVERAGE_SKIP_REASON)
        );
        let inline_core_pack = proof_packs
            .iter()
            .find(|pack| pack.id == "patch-coverage-inline-core")
            .ok_or_else(|| eyre!("missing inline core coverage pack"))?;
        assert!(
            inline_core_pack
                .files
                .iter()
                .any(|file| { file == "crates/perl-lsp-rs-core/src/providers/inline_completion/" })
        );
        assert!(
            inline_core_pack
                .commands
                .iter()
                .any(|command| { command.contains("inline-completion-quality") })
        );
        let completion_core_pack = proof_packs
            .iter()
            .find(|pack| pack.id == "patch-coverage-completion-core")
            .ok_or_else(|| eyre!("missing completion core coverage pack"))?;
        assert!(
            completion_core_pack
                .files
                .iter()
                .any(|file| { file == "crates/perl-lsp-rs-core/src/providers/completion/" })
        );
        assert!(
            completion_core_pack
                .commands
                .iter()
                .any(|command| { command.contains("completion::completion") })
        );
        Ok(())
    }

    #[test]
    fn ci_route_receipt_skips_lcov_pack_when_only_matching_test_file_changed() -> Result<()> {
        let receipt = route_receipt(
            "origin/main",
            "HEAD",
            vec!["xtask/tests/semantic_inline_receipts_cli.rs".to_string()],
        )?;

        assert_eq!(receipt.changed_surfaces, vec!["xtask-semantic-inline-receipts"]);
        assert!(proof_pack_ids(&receipt).contains(&"xtask-semantic-inline-receipts"));
        assert!(receipt.coverage_pack_selector.is_empty());
        assert!(receipt.coverage_proof_packs.is_empty());
        assert_eq!(
            receipt
                .skipped_by_policy
                .get("patch-coverage-xtask-semantic-inline")
                .map(String::as_str),
            Some(NON_SOURCE_LCOV_COVERAGE_SKIP_REASON)
        );
        Ok(())
    }

    #[test]
    fn ci_route_summary_reports_docs_only_without_coverage_packs() -> Result<()> {
        let receipt =
            route_receipt("origin/main", "HEAD", vec!["docs/release notes.md".to_string()])?;
        let summary = render_summary(
            Path::new("target/receipts/ci route.json"),
            Path::new("target/receipts/ci route.md"),
            &receipt,
        );

        assert!(summary.contains("## Coverage Proof Packs"));
        assert!(summary.contains("- none"));
        assert!(summary.contains("`docs/release notes.md`"));
        assert!(summary.contains("`codecov-patch-95`: docs-only change"));
        assert!(
            summary.contains(
                "rtk cargo xtask ci route --base origin/main --head HEAD --receipt 'target/receipts/ci route.json' --summary 'target/receipts/ci route.md' --changed-file 'docs/release notes.md'"
            )
        );
        Ok(())
    }

    #[test]
    fn ci_route_command_writes_receipt_from_explicit_changed_files() -> Result<()> {
        let temp = TempDir::new()?;
        let receipt_path = temp.path().join("ci-route.json");
        let summary_path = temp.path().join("ci-route.md");

        run(CiRouteArgs {
            base: "origin/main".to_string(),
            head: "HEAD".to_string(),
            receipt: receipt_path.clone(),
            summary: summary_path.clone(),
            changed_files: vec!["xtask\\src\\tasks\\supported_editor_inline_smoke.rs".to_string()],
        })?;

        let value: Value = serde_json::from_str(&fs::read_to_string(receipt_path)?)?;
        assert_eq!(
            value
                .get("schema_version")
                .and_then(Value::as_str)
                .ok_or_else(|| eyre!("missing schema_version"))?,
            "ci-route.v1"
        );
        assert_eq!(
            value
                .pointer("/changed_files/0")
                .and_then(Value::as_str)
                .ok_or_else(|| eyre!("missing changed file"))?,
            "xtask/src/tasks/supported_editor_inline_smoke.rs"
        );
        assert_eq!(
            value
                .pointer("/coverage_pack_selector/0")
                .and_then(Value::as_str)
                .ok_or_else(|| eyre!("missing coverage pack"))?,
            "patch-coverage-xtask-supported-editor-inline-smoke"
        );
        let summary = fs::read_to_string(summary_path)?;
        assert!(summary.contains("# CI Route Proof Packet"));
        assert!(summary.contains("patch-coverage-xtask-supported-editor-inline-smoke"));
        assert!(summary.contains("supported_editor_inline_smoke"));
        assert!(summary.contains("rtk cargo xtask ci route --base origin/main --head HEAD"));
        assert!(
            summary.contains("--changed-file xtask/src/tasks/supported_editor_inline_smoke.rs")
        );
        Ok(())
    }

    fn proof_pack_ids(receipt: &CiRouteReceipt) -> Vec<&str> {
        receipt.required_proof_packs.iter().map(|pack| pack.id.as_str()).collect()
    }
}

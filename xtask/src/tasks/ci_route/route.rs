use color_eyre::eyre::Result;
use std::collections::{BTreeMap, BTreeSet};

use super::coverage::coverage_proof_pack_selection;
use super::model::{CiRouteReceipt, ProofPackReceipt};
use super::proof_packs::{
    CI_ACTUALS_PACK, CI_POLICY_PACK, CI_ROUTE_PACK, COMPLETION_CORE_PACK, DOCS_PACK,
    GENERAL_RUST_PACK, INLINE_CORE_PACK, PREFLIGHT_PACK, ProofPack, RIPR_SUMMARY_PACK,
    UX_SCENARIO_PACK, XTASK_SEMANTIC_INLINE_PACK, XTASK_SUPPORTED_EDITOR_INLINE_PACK,
};

pub(super) fn route_receipt(
    base: &str,
    head: &str,
    changed_files: Vec<String>,
) -> Result<CiRouteReceipt> {
    let mut route = RouteBuilder::default();
    route.add_pack(PREFLIGHT_PACK);

    if changed_files.is_empty() {
        route.add_surface("no_changes");
    }

    let docs_only =
        !changed_files.is_empty() && changed_files.iter().all(|file| is_docs_file(file));
    if docs_only {
        route.add_surface("docs");
        route.add_pack(DOCS_PACK);
        route.skip("full-ux-regression", "docs-only change");
        route.skip("release-smoke", "no release surface changed");
        route.skip("docker", "no docker or release workflow changed");
        route.skip("codecov-patch-95", "docs-only change");
    } else {
        for file in &changed_files {
            route_file(file, &mut route);
        }
        route.skip("release-smoke", "no release surface changed");
        route.skip("docker", "no docker or release workflow changed");
    }

    let requested_coverage_pack_selector: Vec<String> =
        route.coverage_pack_selector.iter().cloned().collect();
    let (coverage_pack_selector, skipped_coverage_packs, coverage_proof_packs) =
        coverage_proof_pack_selection(&requested_coverage_pack_selector, &changed_files)?;
    for (pack, reason) in skipped_coverage_packs {
        route.skip(pack, reason);
    }
    let estimated_lem = route.estimated_lem(coverage_pack_selector.len());

    Ok(CiRouteReceipt {
        schema_version: "ci-route.v1",
        provider_action: "changed_file_proof_pack_route",
        claim_boundary: "CI-enforced changed-file proof routing; selected coverage pack commands feed Codecov / Patch 95 on pull requests",
        base: base.to_string(),
        head: head.to_string(),
        changed_files,
        changed_surfaces: route.surfaces.into_iter().collect(),
        required_proof_packs: route
            .proof_packs
            .into_values()
            .map(|pack| ProofPackReceipt {
                id: pack.id.to_string(),
                commands: pack.commands.iter().map(|command| (*command).to_string()).collect(),
            })
            .collect(),
        skipped_by_policy: route.skipped_by_policy,
        coverage_pack_selector,
        coverage_proof_packs,
        estimated_lem,
    })
}

fn route_file(file: &str, route: &mut RouteBuilder) {
    if file == "xtask/src/tasks/supported_editor_inline_smoke.rs"
        || file == "xtask/tests/supported_editor_inline_smoke_cli.rs"
    {
        route.add_surface("xtask-supported-editor-inline-smoke");
        route.add_pack(XTASK_SUPPORTED_EDITOR_INLINE_PACK);
        route.add_coverage_pack("patch-coverage-xtask-supported-editor-inline-smoke");
        route.skip("full-ux-regression", "supported-editor smoke receipt change");
        return;
    }

    if file == "xtask/src/tasks/semantic_inline_receipts.rs"
        || file == "xtask/src/tasks/semantic_inline_next_edit.rs"
        || file == "xtask/tests/semantic_inline_receipts_cli.rs"
        || file == "xtask/tests/semantic_inline_next_edit_cli.rs"
    {
        route.add_surface("xtask-semantic-inline-receipts");
        route.add_pack(XTASK_SEMANTIC_INLINE_PACK);
        route.add_coverage_pack("patch-coverage-xtask-semantic-inline");
        route.skip("full-ux-regression", "semantic receipt/dashboard change");
        return;
    }

    if file.starts_with("crates/perl-lsp-rs-core/src/providers/inline_completion/") {
        route.add_surface("inline-core");
        route.add_pack(INLINE_CORE_PACK);
        route.add_coverage_pack("patch-coverage-inline-core");
        return;
    }

    if file.starts_with("crates/perl-lsp-rs-core/src/providers/completion/") {
        route.add_surface("completion-core");
        route.add_pack(COMPLETION_CORE_PACK);
        route.add_coverage_pack("patch-coverage-completion-core");
        return;
    }

    if file.starts_with("crates/perl-lsp-ux-tests/tests/ux_scenario_") {
        route.add_surface("ux-scenario");
        route.add_pack(UX_SCENARIO_PACK);
        route.add_coverage_pack("patch-coverage-ux-scenario");
        return;
    }

    if file.starts_with(".github/workflows/")
        || file.starts_with(".ci/")
        || file.starts_with("policy/")
        || file == "scripts/ci/ci_classify.py"
        || file == "scripts/ci/test_ci_classify.py"
        || matches!(
            file,
            "xtask/tests/codecov_patch_gate_policy.rs"
                | "xtask/tests/quality_ci_wiring_policy.rs"
                | "xtask/tests/quality_gate_patch_coverage_cli_policy.rs"
        )
    {
        route.add_surface("ci-policy");
        route.add_pack(CI_POLICY_PACK);
        route.add_coverage_pack("patch-coverage-ci-policy");
        return;
    }

    if file == "scripts/ci/route-codecov-packs.py"
        || file == "scripts/ci/test_route_codecov_packs.py"
        || file == "xtask/src/tasks/ci_route.rs"
        || file.starts_with("xtask/src/tasks/ci_route/")
        || file == "xtask/tests/ci_route_cli.rs"
    {
        route.add_surface("ci-routing");
        route.add_pack(CI_ROUTE_PACK);
        route.add_coverage_pack("patch-coverage-ci-route");
        return;
    }

    if file == "scripts/ci/emit_ci_actuals.py" || file == "scripts/ci/test_emit_ci_actuals.py" {
        route.add_surface("ci-actuals");
        route.add_pack(CI_ACTUALS_PACK);
        route.add_coverage_pack("patch-coverage-ci-actuals");
        return;
    }

    if file == "scripts/ci/ripr_summary.py" || file == "scripts/ci/test_ripr_summary.py" {
        route.add_surface("ripr-summary");
        route.add_pack(RIPR_SUMMARY_PACK);
        route.add_coverage_pack("patch-coverage-ripr-summary");
        return;
    }

    if is_docs_file(file) {
        route.add_surface("docs");
        route.add_pack(DOCS_PACK);
        return;
    }

    if file.ends_with(".rs") {
        route.add_surface("rust");
        route.add_pack(GENERAL_RUST_PACK);
        route.add_coverage_pack("patch-coverage-rust-focused");
    } else {
        route.add_surface("misc");
    }
}

fn is_docs_file(file: &str) -> bool {
    file.starts_with("docs/")
        || file == "README.md"
        || file == "CHANGELOG.md"
        || file.ends_with(".md")
}

#[derive(Default)]
struct RouteBuilder {
    surfaces: BTreeSet<String>,
    proof_packs: BTreeMap<&'static str, ProofPack>,
    skipped_by_policy: BTreeMap<String, String>,
    coverage_pack_selector: BTreeSet<String>,
}

impl RouteBuilder {
    fn add_surface(&mut self, surface: impl Into<String>) {
        self.surfaces.insert(surface.into());
    }

    fn add_pack(&mut self, pack: ProofPack) {
        self.proof_packs.insert(pack.id, pack);
    }

    fn skip(&mut self, pack: impl Into<String>, reason: impl Into<String>) {
        self.skipped_by_policy.entry(pack.into()).or_insert_with(|| reason.into());
    }

    fn add_coverage_pack(&mut self, pack: impl Into<String>) {
        self.coverage_pack_selector.insert(pack.into());
    }

    fn estimated_lem(&self, coverage_pack_count: usize) -> u64 {
        let pack_cost = u64::try_from(self.proof_packs.len()).unwrap_or(u64::MAX);
        let coverage_cost = u64::try_from(coverage_pack_count).unwrap_or(u64::MAX);
        2 + pack_cost.saturating_mul(3) + coverage_cost.saturating_mul(4)
    }
}

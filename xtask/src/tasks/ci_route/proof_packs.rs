#[derive(Debug, Clone)]
pub(super) struct ProofPack {
    pub(super) id: &'static str,
    pub(super) commands: &'static [&'static str],
}

pub(super) const PREFLIGHT_PACK: ProofPack = ProofPack {
    id: "preflight",
    commands: &[
        "cargo xtask pr title-check --no-gh",
        "cargo fmt -p xtask -- --check",
        "git diff --check",
    ],
};

pub(super) const DOCS_PACK: ProofPack = ProofPack {
    id: "docs-focused",
    commands: &["cargo xtask check-devex-docs", "cargo xtask doc-claims"],
};

pub(super) const XTASK_SEMANTIC_INLINE_PACK: ProofPack = ProofPack {
    id: "xtask-semantic-inline-receipts",
    commands: &[
        "cargo test -p xtask --bin xtask --profile agent --locked semantic_inline_receipts -- --nocapture",
        "cargo test -p xtask --test semantic_inline_receipts_cli --profile agent --locked -- --nocapture",
    ],
};

pub(super) const XTASK_SUPPORTED_EDITOR_INLINE_PACK: ProofPack = ProofPack {
    id: "xtask-supported-editor-inline-smoke",
    commands: &[
        "cargo test -p xtask --bin xtask --profile agent --locked supported_editor_inline_smoke -- --nocapture",
        "cargo test -p xtask --test supported_editor_inline_smoke_cli --profile agent --locked -- --nocapture",
        "cargo test -p xtask --bin xtask --profile agent --locked semantic_inline_receipts -- --nocapture",
    ],
};

pub(super) const INLINE_CORE_PACK: ProofPack = ProofPack {
    id: "inline-core",
    commands: &[
        "cargo test -p perl-lsp-rs-core --lib --profile agent --locked inline_completion -- --nocapture",
        "cargo run -p xtask --profile agent --locked -- inline-completion-quality --receipt target/receipts/inline-completion-quality.json",
    ],
};

pub(super) const COMPLETION_CORE_PACK: ProofPack = ProofPack {
    id: "completion-core",
    commands: &[
        "cargo test -p perl-lsp-rs-core --lib --profile agent --locked completion::completion -- --nocapture",
    ],
};

pub(super) const UX_SCENARIO_PACK: ProofPack = ProofPack {
    id: "ux-scenario-focused",
    commands: &[
        "cargo test -p perl-lsp-ux-tests --profile agent --locked -- --nocapture",
        "python -m json.tool crates/perl-lsp-ux-tests/fixtures/editor_ux_fixture_matrix.json",
    ],
};

pub(super) const CI_POLICY_PACK: ProofPack = ProofPack {
    id: "ci-policy-focused",
    commands: &[
        "python -m unittest scripts/ci/test_ci_classify.py",
        "cargo xtask workflow-trigger-lint --policy .ci/policies/required-checks.toml --receipt target/receipts/workflow-trigger-lint.json",
        "cargo test -p xtask --test quality_ci_wiring_policy --profile agent --locked -- --nocapture",
    ],
};

pub(super) const CI_ROUTE_PACK: ProofPack = ProofPack {
    id: "ci-route-receipt",
    commands: &[
        "python -m unittest scripts/ci/test_route_codecov_packs.py",
        "cargo test -p xtask --bin xtask --profile agent --locked ci_route -- --nocapture",
        "cargo test -p xtask --test ci_route_cli --profile agent --locked -- --nocapture",
        "cargo run -p xtask --profile agent --locked -- ci route --base origin/main --head HEAD --receipt target/receipts/ci-route.json",
    ],
};

pub(super) const CI_ACTUALS_PACK: ProofPack = ProofPack {
    id: "ci-actuals-focused",
    commands: &["python -m unittest scripts/ci/test_emit_ci_actuals.py"],
};

pub(super) const RIPR_SUMMARY_PACK: ProofPack = ProofPack {
    id: "ripr-summary-focused",
    commands: &["python -m unittest scripts/ci/test_ripr_summary.py"],
};

pub(super) const GENERAL_RUST_PACK: ProofPack = ProofPack {
    id: "rust-focused",
    commands: &["cargo check --workspace --all-targets --profile agent --locked"],
};

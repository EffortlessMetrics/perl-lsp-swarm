Warning: truncated output (original token count: 76666)
Total output lines: 7983

//! Xtask automation for perl-lsp
//!
//! This binary provides custom automation tasks for building, testing,
//! and maintaining the perl-lsp workspace.

// Task-runner binary — println!/eprintln! are intentional diagnostic output.
#![allow(clippy::print_stderr, clippy::print_stdout)]

use clap::{CommandFactory, Parser, Subcommand, ValueEnum};
use cli::srp::{SrpCommand, SrpMicrocratesArgs, UnwiredScanArgs};
use color_eyre::eyre::{Result, bail, eyre};
use std::collections::BTreeMap;
use std::path::PathBuf;

mod allocation_tracker;
mod cli;
mod tasks;
#[cfg(test)]
mod test_support;
mod types;
mod utils;
#[cfg(feature = "legacy")]
use tasks::corpus;
use tasks::dead_code::{DeadCodeConfig, DeadCodeMode};
use tasks::dependency_hygiene::{DependencyHygieneConfig, DependencyHygieneMode};
use tasks::emacs_train_specs::{LeafSpecDisposition, SpecsOutputFormat};
use tasks::gate_policy::GatePolicyProfile;
use tasks::gates::{GateTier, OutputFormat as GatesOutputFormat};
use tasks::issue_controllers::IssueControllersCommand;
use tasks::issue_plan::IssuePlanOutputFormat;
use tasks::methodology_gate::MethodologyOutputFormat;
use tasks::targeted_checks::CheckMode;
use tasks::unwired_scan::UnwiredScanConfig;
use tasks::ux_scorecard::UxScorecardFormat;
use tasks::workflow_trigger_lint::WorkflowTriggerLintFormat;
use tasks::worktree_allocator::AgentWorktreeCommand;
use tasks::{
    activation, active_goal_manifest, agent_capability_policy, agent_flow,
    agent_implementation_packet, agent_lease, agent_receipt, agent_review_packet,
    aggregate_receipts, badges, bench, benchmarks, build, build_timing, bump_version, change_set,
    check, check_agent_context, check_lint_policy, check_tautology, check_test_wiring,
    check_toolchain, check_version_sync, ci, ci_audit_workflows, ci_cache_inventory, ci_contract,
    ci_doctor, ci_explain, ci_hygiene, ci_measure, ci_metrics, ci_policy, ci_pr_summary, ci_route,
    ci_scope, clean, clippy_cost_measure, code_action_generation_ledger, command_evidence, compare,
    compat_inventory, compiler_lexical_cutline, compiler_performance_receipt,
    compiler_upstream_status, completion_candidates, corpus_audit, count_ratchet, cpan_corpus,
    critic_rule_proof, dead_code, dead_code_api_ledger, debt_report, dependency_hygiene, dev,
    devex_docs, devex_doctor, devex_plan, doc, doc_claims, e2e_validate, edge_cases,
    emacs_train_context, emacs_train_packet, emacs_train_specs, features, finalize_check,
    fix_forward, fmt, forbid_fatal_constructs, forensics, gate_receipts, gates, generated_files,
    github, github_preflight, github_review, goals, hardening, hook_checks, ignored_tests,
    incremental_proof, inject_sha_assets, inline_completion_quality, inline_completion_smoke,
    install_surface_check, integration_proof, intent_diff_gate, issue_controllers, issue_plan,
    kwalitee_namespace_inventory, layer_check, lsp_318_claims, lsp_318_matrix, lsp_ux_smoke,
    memory_trends, merge_ready, methodology_gate, metrics, module_train, module_train_live,
    native_critic, native_format, native_neovim_train, native_product_surface, native_tooling,
    oneliner_capability_matrix, oracle_fixture_manifest, oracle_receipt_schema, oracle_runner,
    parse_rust, parser_corpus_sweep, parser_matrix, parser_ratchet, perl_core_harness,
    perl_corpus_train, perl_kwalitee, populate_book, pre_push_plan, prep_crates_io_launch,
    product_health_rail_contract, product_health_status, protocol_type_substrate_matrix,
    provider_confidence_matrix, provider_promotion_ledger, publication_facts, publish,
    publish_closure, publish_manifest_check, publish_receipts, quality_baseline, quality_gate,
    queue_health, queue_snapshot, quickorm_api_matrix, receipts, release, release_artifact_check,
    release_candidate_artifacts, release_evidence, release_notes, release_trust_invariants,
    release_turnkey, repo_hygiene, repository_topology, ripr_evidence, rust_small_proof, seam_diff,
    semantic_inline_next_edit, semantic_inline_receipts, semantic_scorecard,
    semantic_shadow_compare, semantic_token_classes, session_receipt, shadow_parity,
    srp_microcrates, standalone_diagnostics, supported_editor_inline_smoke, swarm_agent_roster,
    swarm_summary, sync_release_docs, targeted_checks, test, test_lsp, train_edge_contract,
    unwired_scan, update_homebrew, update_status, ux_regression_receipt, ux_scorecard,
    validate_workspace_exclusions, workflow_authority_inventory, workflow_policy_lint,
    workflow_trigger_lint, workspace_symbol_classes, worktree_allocator, worktrees,
    writer_admission,
};
#[cfg(feature = "parser-tasks")]
use tasks::{bindings, compare_parsers, highlight};
use types::TestSuite;
#[cfg(any(feature = "legacy", feature = "parser-tasks"))]
use types::*;

#[derive(Parser)]
#[command(name = "xtask")]
#[command(about = "Custom tasks for perl-lsp")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Print all available top-level xtask commands.
    #[command(name = "list-commands")]
    List,

    /// Run lean CI suite (format, clippy, tests) for constrained environments.
    /// Use `ci doctor` to check local/CI parity without running the full suite.
    Ci {
        /// Optional sub-command; omit to run the full CI suite.
        #[command(subcommand)]
        command: Option<CiSubcommand>,
    },

    /// Prove one exact parent-head -> child-head stack increment (#11229 S1).
    #[command(name = "ci-stack")]
    StackIncrement {
        /// Sub-command selecting subject assembly, plan selection, artifact
        /// validation, or advisory explanation.
        #[command(subcommand)]
        command: tasks::ci_stack_increment::StackIncrementCommand,
    },

    /// Run format and clippy checks only (no tests)
    CheckOnly,

    /// Verify every workspace member has package-local agent context
    /// (CLAUDE.md), an exemption, or a tracked context-debt entry
    /// (`.ci/policies/agent-context-policy.toml`).
    CheckAgentContext,

    /// Verify the governed Clippy lint policy ledger and workspace inheritance.
    CheckLintPolicy,

    /// Verify local Rust toolchain meets the pinned MSRV in rust-toolchain.toml.
    CheckToolchain {
        /// Show a warning when rustc satisfies the minimum MSRV but differs
        /// from the exact pinned channel string.
        #[arg(long)]
        doctor: bool,
    },

    /// Verify DevEx docs match the toolchain and command surface.
    CheckDevexDocs,

    /// Verify first-mile product surfaces stay native-only (no legacy bridge /
    /// external-tool-required framing).
    CheckNativeProductSurface {
        /// Also fail on bare external-tool names (`perltidy`, `perlcritic`,
        /// `Perl::LanguageServer`, ...) that appear on a first-mile `.md`
        /// surface without a native-first qualifier on the same line.
        #[arg(long)]
        strict: bool,
    },

    /// Validate Real Perl Editor Trust provider/support claim tables.
    CheckProviderConfidenceMatrix,

    /// Validate Real Perl Editor Trust support claim map.
    CheckSupportClaims,

    /// RETIRED: performs no validation and emits a retirement receipt.
    /// The active goal manifest it validated no longer exists. Always exits 0.
    CheckActiveGoalManifest,

    /// Validate machine-readable Real Perl Editor Trust provider promotion ledger.
    CheckProviderPromotionLedger,

    /// Validate the code-action provider-generation disposition ledger and its
    /// parity corpus against current source (#9188).
    CheckCodeActionGenerationLedger,

    /// Validate the `perl_parser::dead_code` API disposition ledger against
    /// current module source, the public-API baseline, its compatibility
    /// corpus, the consumer inventory and the generated projection (#9777).
    CheckDeadCodeApiLedger {
        /// Regenerate the Markdown projection from the ledger before checking.
        #[arg(long)]
        write: bool,
    },

    /// Validate declared differential real-Perl oracle fixtures.
    CheckOracleFixtureManifest,

    /// Generate, validate, and list the versioned activation inventory
    /// (`activation_inventory.v1`, #9204): a deterministic classified catalog
    /// of product, preview, compatibility-shim, test-api, lab, oracle,
    /// benchmark, and gate surfaces derived from existing authorities plus a
    /// narrow, typed, owner/expiry-bound override ledger. Does not implement
    /// activation checking (#9205).
    Activation {
        /// Operation to run against the activation inventory.
        #[command(subcommand)]
        command: tasks::activation::ActivationSubcommand,
    },

    /// List, validate, and explain the compiler lexical cut-line cases
    /// manifest (`compiler_lexical_cutline_cases.v1`, #12156).
    CompilerLexicalCutline {
        /// Operation to run against the manifest.
        #[command(subcommand)]
        command: tasks::compiler_lexical_cutline::CompilerLexicalCutlineSubcommand,
    },

    /// Check, explain, and project the standalone diagnostic reason/action
    /// registry (`standalone_diagnostics.v1`, #11493).
    StandaloneDiagnostics {
        /// Operation to run against the registry.
        #[command(subcommand)]
        command: tasks::standalone_diagnostics::StandaloneDiagnosticsSubcommand,
    },

    /// Validate the versioned critic rule-proof manifest, live fixture
    /// propositions, and generated status (`critic_rule_proof.v1`, #6973).
    CriticRuleProof {
        /// Operation to run against the rule-proof manifest.
        #[command(subcommand)]
        command: tasks::critic_rule_proof::CriticRuleProofSubcommand,
    },

    /// Validate the versioned release trust-invariant registry and generated
    /// Markdown projection (`release_trust_invariants.v1`, #9392). Does
    /// not consume live candidate receipts.
    #[command(name = "release-trust-invariants")]
    ReleaseTrustInvariants {
        /// Operation to run against the trust-invariant registry.
        #[command(subcommand)]
        command: tasks::release_trust_invariants::ReleaseTrustInvariantsSubcommand,
    },

    /// Validate differential real-Perl oracle receipt schema.
    CheckOracleReceiptSchema,

    /// Validate the compiler performance receipt schema, its vocabularies, and every committed fixture.
    CheckCompilerPerformanceReceipt,

    /// Validate the shared typed train edge and claim-profile contract
    /// (train_edge_contract.v1), its programme-neutral fixtures, and the
    /// declared adaptations of the landed programme train manifests.
    CheckTrainEdgeContract,

    /// Validate the structural release-candidate security contract (no audits).
    CandidateSecurityContract {
        /// Path to the contract JSON document.
        #[arg(long)]
        contract: PathBuf,
    },

    /// Validate the stable native Neovim implementation train manifest
    /// (native_neovim_train.v1, #11392): the closed schema, graph shift-left
    /// rejection law with named diagnostics, the shuffled determinism
    /// control, and every discriminating invalid fixture.
    #[command(name = "check-native-neovim-train")]
    CheckNativeNeovimTrain,

    /// Stable perl-corpus authority train (perl_corpus_train.v1, #10980):
    /// validate the checked manifest, render deterministic reviewer
    /// projections, or explain one static node packet. Offline and
    /// read-only; derives no frontier and observes no GitHub state.
    #[command(name = "perl-corpus-train")]
    PerlCorpusTrain {
        #[command(subcommand)]
        command: PerlCorpusTrainCommand,
    },

    /// Validate the dependency-neutral product-health rail/adapter registry contract.
    #[command(name = "check-product-health-rail-contract")]
    CheckProductHealthRailContract,

    /// Deterministic generic assembly of independent product-health rails
    /// (`product_health_status.v1`, #12360).  Read-only offline
    /// `build`/`check`/`show`/`diff` over one checked
    /// `product_health_rail_registry.v1` (#12359) and its repository-local
    /// source packets.  Fails closed: a rail without real evidence keeps a
    /// typed state, never synthetic green; no support/release/publication
    /// authority is granted.
    #[command(name = "product-health")]
    ProductHealth {
        #[command(subcommand)]
        command: tasks::product_health_status::ProductHealthCommand,
    },

    /// Validate the shared bounded builder-packet contract
    /// (agent_implementation_packet.v1, #10872): the closed schema, the
    /// programme-neutral fixtures, the fail-closed negative controls, the
    /// canonical-semantics control, and the deterministic golden
    /// projections. `--update-golden` rewrites the golden vectors.
    #[command(name = "check-agent-implementation-packet")]
    CheckAgentImplementationPacket {
        /// Rewrite the golden projection vectors (explicit writer action;
        /// never live packet state).
        #[arg(long)]
        update_golden: bool,
    },

    /// Render one projection of a caller-supplied packet document to stdout
    /// (agent_implementation_packet.v1, #10872). Fails closed when the
    /// document violates the contract. Packet instances are runtime-local
    /// outputs: this command never writes repository files.
    #[command(name = "render-agent-packet")]
    RenderAgentImplementationPacket {
        /// Projection to render.
        #[arg(long, value_enum, default_value = "markdown")]
        format: agent_implementation_packet::PacketProjection,

        /// Path to the caller-supplied packet document.
        input: std::path::PathBuf,
    },

    /// Validate specialized Vim/vim-lsp driver observations
    /// (vim_lsp_specialized_driver.v1, #11380) against the compiled action
    /// vocabulary: barrier/timeout/generation/result semantics, boundedness,
    /// and the pinned Vim + vim-lsp + perllsp subject. The file carries one
    /// JSON observation per line and fails closed on any violation.
    #[command(name = "check-vim-lsp-specialized-observations")]
    CheckVimLspSpecializedObservations {
        /// Path to the JSONL observations file emitted by the specialized
        /// adapter or the fake backend.
        #[arg(long)]
        file: PathBuf,
    },

    /// Editor-compat actual-host execution (#10944): launch, drive, bound,
    /// and clean one exact editor client subject through the shared hermetic
    /// Rust host runner. Every exact input is digest-verified before launch;
    /// an unavailable host is a typed error, never a skip.
    #[command(name = "editor-compat")]
    EditorCompat {
        #[command(subcommand)]
        command: EditorCompatCommand,
    },

    /// Provision and verify the content-bound Vim + vim-lsp host test
    /// instrument (vim_vim_lsp_host_toolchain.v1, #11372): the pinned Vim
    /// release bytes plus the #11369-pinned vim-lsp subject, acquired,
    /// digest-verified, cached under exact-identity keys, revalidated on
    /// every use, and handed off as ephemeral roles. Test-instrument
    /// identity only: no support verdict, journey, or receipt.
    #[command(name = "vim-host-toolchain")]
    VimHostToolchain {
        #[command(subcommand)]
        command: VimHostToolchainCommand,
    },

    /// Validate the shared adversarial review-packet, review-finding, and
    /// advisory closure-projection contracts (#10881): the closed schemas,
    /// the programme-neutral fixtures, the fail-closed negative controls,
    /// the canonical-semantics control, and the deterministic golden
    /// projections. `--update-golden` rewrites the golden vectors.
    #[command(name = "check-agent-review-packet")]
    CheckAgentReviewPacket {
        /// Rewrite the golden projection vectors (explicit writer action;
        /// never live review state).
        #[arg(long)]
        update_golden: bool,
    },

    /// Render one projection of a caller-supplied review document (packet,
    /// finding, or closure projection) to stdout (#10881). Fails closed when
    /// the document violates the contract. Document instances are
    /// runtime-local outputs: this command never writes repository files.
    #[command(name = "render-review-packet")]
    RenderAgentReviewPacket {
        /// Projection to render.
        #[arg(long, value_enum, default_value = "markdown")]
        format: agent_review_packet::ReviewProjection,

        /// Path to the caller-supplied review document.
        input: std::path::PathBuf,
    },

    /// Run differential oracle comparison (PackageSubTable vertical slice).
    ///
    /// Loads fixtures from the manifest, runs the PackageSubTable extractor
    /// against both the Rust HIR and real Perl, and emits comparison receipts
    /// to target/receipts/oracle/. Requires `perl` on PATH.
    #[command(name = "check-oracle-compare")]
    CheckOracleCompare,

    /// Validate semantic-token class promotion registry.
    CheckSemanticTokenClasses,

    /// Validate selected LSP 3.18 claim-boundary guardrails.
    #[command(name = "check-lsp-318-claims")]
    CheckLsp318Claims,

    /// Generate or check the selected LSP 3.18 conformance matrix.
    #[command(name = "generate-lsp-318-matrix")]
    GenerateLsp318Matrix {
        /// Check that the checked-in matrix matches generated content.
        #[arg(long)]
        check: bool,
    },

    /// Generate or check the DBIx::QuickORM API return matrix.
    ///
    /// The matrix is a projection of the reviewed registry in
    /// `perl-semantic-facts`; edit the registry, not the generated document.
    #[command(name = "generate-quickorm-api-matrix")]
    GenerateQuickormApiMatrix {
        /// Check that the checked-in matrix matches generated content.
        #[arg(long)]
        check: bool,
    },

    /// Generate or check the Perl command-line analysis capability matrix.
    ///
    /// Fails when a declared capability row claims support without fixture
    /// evidence in the command-line conformance corpus.
    #[command(name = "oneliner-capability-matrix")]
    OnelinerCapabilityMatrix {
        /// Check that the checked-in matrix matches generated content.
        #[arg(long)]
        check: bool,
    },

    /// Validate `policy/repository-topology.toml` and project it to a human table.
    #[command(name = "repo-topology")]
    RepoTopology {
        /// Validate only, and require the checked-in projection to be current.
        #[arg(long)]
        check: bool,
    },

    /// Reconcile `policy/tree-sitter-compat-inventory.toml` against the real
    /// `perl-tree-sitter-compat` surface and project the inventory (#8880).
    #[command(name = "compat-inventory")]
    CompatInventory {
        /// Validate only, and require the checked-in projection to be current.
        #[arg(long)]
        check: bool,
    },

    /// Reconcile `policy/kwalitee-namespace-inventory.toml` against every live
    /// `perl-kwalitee` / `perl_kwalitee` reference and report unresolved active
    /// counts by migration target (#8752).
    #[command(name = "kwalitee-inventory")]
    KwaliteeInventory {
        /// Reconcile references and add a confirmation line on success.
        #[arg(long)]
        check: bool,
        /// Print entry skeletons with current line hashes instead of checking.
        #[arg(long, conflicts_with = "check")]
        scaffold: bool,
        /// Evaluate a different repository tree (for hermetic tests).
        #[arg(long)]
        root: Option<PathBuf>,
    },

    /// Reconcile `policy/completion-candidate-producers.toml` against the live
    /// `textDocument/completion` candidate producers and hold the finalizer
    /// route closed (#10949).
    #[command(name = "completion-candidates")]
    CompletionCandidates {
        /// Operation to run against the inventory.
        #[command(subcommand)]
        command: completion_candidates::CompletionCandidatesSubcommand,
    },

    /// Generate or check the protocol-type substrate and migration-denominator
    /// matrix (#11802).
    #[command(name = "generate-protocol-type-substrate-matrix")]
    GenerateProtocolTypeSubstrateMatrix {
        /// Check that the checked-in matrix and receipt match generated content.
        #[arg(long)]
        check: bool,
    },

    /// Validate workspace-symbol class promotion registry.
    CheckWorkspaceSymbolClasses,

    /// RETIRED: receipt-only compatibility surface for the former tracked
    /// work selector. Every subcommand selects no work, mutates nothing, and
    /// exits 0 with a retirement receipt. Current GitHub issues, PRs,
    /// reviews, and checks own live work selection.
    Goals {
        #[command(subcommand)]
        command: GoalsCommand,
    },

    /// Offline current-tree status and safe parallel frontier over the
    /// stable `module_train.v1` train graph (#11626 C02, data from #11625).
    ///
    /// Reads only the checked-in manifest and local git facts. Performs no
    /// network or GitHub access, no scheduling, and no mutation.
    ModuleTrain {
        #[command(subcommand)]
        command: ModuleTrainCommand,
    },

    /// Capture a GitHub PR queue snapshot for disconnected maintainership.
    Queue {
        #[command(subcommand)]
        command: QueueCommand,
    },

    /// Emit a machine-produced session-start receipt capturing checkout
    /// identity (repo/branch/SHA relative to `origin/main`) and an advisory
    /// staleness liveness check (M5 phase 4, #3777). READ-ONLY except for
    /// `git fetch origin main` and writing the receipt JSON to `--out`;
    /// never mutates a branch, worktree, PR, or ledger. Always exits 0 --
    /// staleness is a WARNING, not a build gate (build-lease enforcement is
    /// M5 phase 3, a separate deliverable).
    #[command(name = "session-receipt")]
    SessionReceipt {
        /// Emit machine-readable JSON to stdout (also always written to `--out`).
        #[arg(long)]
        json: bool,

        /// Stamp an explicit program id into the receipt. Portfolio state does
        /// not auto-select a repository-global program.
        #[arg(long)]
        program: Option<String>,

        /// Optional lane label to stamp into the receipt. No auto-detection --
        /// lane is a runtime work-item selection, not inherent checkout state.
        #[arg(long)]
        lane: Option<String>,

        /// Output path for the receipt JSON (default: `target/receipts/session-start.json`).
        #[arg(long)]
        out: Option<PathBuf>,

        /// Commits-behind-`origin/main` threshold that triggers the advisory
        /// staleness WARNING.
        #[arg(long, default_value_t = session_receipt::DEFAULT_WARN_THRESHOLD)]
        warn_threshold: u32,
    },

    /// PR-related local tooling (title check, etc.)
    Pr {
        #[command(subcommand)]
        command: PrSubcommand,
    },

    /// Verify landing and content-survival proof without evaluating semantic completion.
    ///
    /// Implements the landing-proof layer of CLOSE_PROOF_POLICY.md: proves
    /// ancestry through the shared `xtask::git_ancestry` authority and emits a
    /// structured `landing_proof.v1` receipt. Landing ancestry never
    /// authorizes an issue close; `semantic_completion` is always
    /// `not_evaluated`.
    ///
    /// Exit 0 = landing proof passes, exit 2 = commit is provably not
    /// reachable, exit 1 = error or not-proven (git failed, bad input, or a
    /// shallow/partial checkout that cannot decide ancestry).
    #[command(name = "landing-proof")]
    PrCloseProof {
        /// Commit SHA to verify.
        #[arg(long)]
        commit: String,
        /// Canonical main ref (e.g. origin/main).
        #[arg(long, default_value = "origin/main")]
        canonical_main: String,
        /// Optional distinctive string to grep in canonical-main (Rule 3 substance check).
        #[arg(long)]
        substance_grep: Option<String>,
        /// Output format: `human` (default) or `json`.
        #[arg(long, default_value = "human")]
        format: String,
    },

    /// PR reconciliation ledger commands.
    #[command(name = "pr-ledger")]
    PrLedger {
        #[command(subcommand)]
        command: PrLedgerCommand,
    },

    /// Check target-only development commits before a release sync.
    #[command(name = "sync-divergence")]
    SyncDivergence {
        #[command(subcommand)]
        command: SyncDivergenceCommand,
    },

    /// Issue Research / Plan Review Desk tooling (report-only audit, etc.).
    #[command(name = "issue-plan")]
    IssuePlan {
        #[command(subcommand)]
        command: IssuePlanSubcommand,
    },

    /// Editor-integration train tooling (#7979/#8706). Deterministic,
    /// offline, fail-closed projections over checked train contracts.
    #[command(name = "integration")]
    Integration {
        #[command(subcommand)]
        command: IntegrationCommand,
    },

    /// Issue-controller train tooling: independent static validation of the
    /// stable `issue_controller_train.v1` manifest and its checked human
    /// projection (#11765). Deterministic and offline only.
    #[command(name = "issue-controllers")]
    IssueControllers {
        #[command(subcommand)]
        command: IssueControllersCommand,
    },

    /// Writer admission — read-only pre-admission diagnostic (#3957 W1).
    /// Reports a PASS/BLOCK/NOT_PROVEN verdict with per-check reasons.
    /// Never mutates git state, the filesystem, or GitHub.
    #[command(name = "writer-admission")]
    WriterAdmission {
        /// Target branch being admitted (defaults to the current branch).
        #[arg(long)]
        branch: Option<String>,

        /// Canonical base ref (e.g. origin/main).
        #[arg(long, default_value = "origin/main")]
        base: String,

        /// Worktree/checkout path to inspect (defaults to the CWD).
        #[arg(long)]
        worktree: Option<PathBuf>,

        /// Expected SHA for the canonical base. Omit to skip the
        /// base-ref-mismatch comparison.
        #[arg(long)]
        expected_base_sha: Option<String>,

        /// GitHub repo (owner/name) for the advisory candidate-presence
        /// lookup: an open PR is surfaced as continuation evidence, never
        /// as proof of a live writer or a collision.
        #[arg(long)]
        repo: Option<String>,

        /// JSON fixture (offline / deterministic tests) instead of live
        /// git/gh.
        #[arg(long)]
        fixture: Option<PathBuf>,

        /// Emit JSON instead of human-readable text.
        #[arg(long)]
        json: bool,

        /// Disk-floor GB threshold (matches clean-worktrees.sh FLOOR_GB).
        #[arg(long, default_value_t = 200.0)]
        floor_gb: f64,

        /// Disk-floor percentage threshold (matches clean-worktrees.sh
        /// FLOOR_PCT).
        #[arg(long, default_value_t = 5.0)]
        floor_pct: f64,

        /// Large-staged-change-set threshold (synthetic mass-staged
        /// additions guard).
        #[arg(long, default_value_t = 1000)]
        large_staged_threshold: u32,
    },

    /// Build project with various configurations
    Build {
        /// Build in release mode
        #[arg(long)]
        release: bool,

        /// Build with specific features
        #[arg(long, value_delimiter = ',')]
        features: Option<Vec<String>>,

        /// Build only C scanner
        #[arg(long)]
        c_scanner: bool,

        /// Build only Rust scanner
        #[arg(long)]
        rust_scanner: bool,
    },

    /// Run tests with various configurations
    Test {
        /// Run tests in release mode
        #[arg(long)]
        release: bool,

        /// Run specific test suite
        #[arg(long, value_enum)]
        suite: Option<TestSuite>,

        /// Run tests with specific features
        #[arg(long, value_delimiter = ',')]
        features: Option<Vec<String>>,

        /// Run tests with verbose output
        #[arg(long)]
        verbose: bool,

        /// Run tests with coverage
        #[arg(long)]
        coverage: bool,
    },

    /// Run local smoke checks against explicit binaries.
    Smoke {
        #[command(subcommand)]
        command: SmokeCommand,
    },

    /// Verify inline completion over stdio against a built binary.
    #[command(name = "inline-completion-smoke")]
    InlineCompletionSmoke {
        /// Path to the perl-lsp binary to execute.
        #[arg(long)]
        binary: PathBuf,
    },

    /// Emit a deterministic inline-completion quality receipt.
    #[command(name = "inline-completion-quality")]
    InlineCompletionQuality {
        /// Receipt JSON path to write.
        #[arg(long, default_value = "target/receipts/inline-completion-quality.json")]
        receipt: PathBuf,
    },

    /// Emit a semantic inline-completion UX receipt dashboard.
    #[command(name = "semantic-inline-receipts")]
    SemanticInlineReceipts {
        /// Receipt JSON path to write.
        #[arg(long, default_value = "target/receipts/semantic-inline-receipts.json")]
        receipt: PathBuf,
        /// Optional deterministic quality receipt to summarize when present.
        #[arg(long, default_value = "target/receipts/inline-completion-quality.json")]
        quality_receipt: PathBuf,
        /// Optional next-edit scaffold receipt to validate and summarize when present.
        #[arg(long, default_value = "target/receipts/semantic-inline-next-edit.json")]
        next_edit_receipt: PathBuf,
    },

    /// Emit a semantic inline-completion next-edit scaffold receipt.
    #[command(name = "semantic-inline-next-edit")]
    SemanticInlineNextEdit {
        /// Receipt JSON path to write.
        #[arg(long, default_value = "target/receipts/semantic-inline-next-edit.json")]
        receipt: PathBuf,
    },

    /// Emit a supported-editor inline-completion smoke receipt bundle.
    #[command(name = "supported-editor-inline-smoke")]
    SupportedEditorInlineSmoke {
        /// Receipt JSON path to write.
        #[arg(long, default_value = "target/receipts/supported-editor-inline-smoke.json")]
        receipt: PathBuf,
    },

    /// Run release UX smoke fixtures over stdio and optionally write receipts.
    #[command(name = "lsp-ux-smoke")]
    LspUxSmoke {
        /// Fixture root containing manifest.json and scenario directories.
        #[arg(long, default_value = "testdata/ux/release_smoke")]
        fixture: PathBuf,
        /// Write JSON and Markdown receipts under target/receipts/ux.
        #[arg(long)]
        receipt: bool,
        /// Existing perl-lsp binary to run instead of building target/agent/perl-lsp.
        #[arg(long)]
        binary: Option<PathBuf>,
        /// Do not auto-build perl-lsp when --binary is omitted.
        #[arg(long)]
        no_build: bool,
    },

    /// Deprecated compatibility delegate for the Python badge endpoint owner.
    #[command(hide = true)]
    Badges {
        /// Check committed endpoints for drift without updating badges/.
        #[arg(long)]
        check: bool,
    },

    /// Generate or check a coverage baseline receipt for the quality lane.
    #[command(name = "coverage-baseline")]
    CoverageBaseline {
        /// LCOV input path.
        #[arg(long, default_value = "target/lcov.info")]
        lcov: PathBuf,
        /// Coverage receipt JSON path.
        #[arg(long, default_value = "target/receipts/quality/coverage-baseline.json")]
        receipt: PathBuf,
        /// Codecov configuration path.
        #[arg(long, default_value = "codecov.yml")]
        codecov: PathBuf,
        /// Patch coverage percentage from Codecov for this PR.
        #[arg(long)]
        patch_coverage: Option<f64>,
        /// Compute patch coverage from executable lines changed since this git base.
        #[arg(long)]
        patch_base: Option<String>,
        /// Coverage scope recorded in the receipt.
        #[arg(long)]
        scope: Option<String>,
        /// Validate the existing receipt instead of rewriting it.
        #[arg(long)]
        check: bool,
    },

    /// Evaluate coverage and RIPR proof receipts for local and CI gates.
    #[command(name = "quality-gate")]
    QualityGate {
        /// Gate mode to evaluate.
        #[arg(long, value_enum)]
        mode: tasks::quality_gate::QualityGateMode,
        /// Temporary quality exception policy path.
        #[arg(long, default_value = "policy/quality-gate-exceptions.toml")]
        exception_policy: PathBuf,
        /// Repo-wide RIPR+ receipt JSON path.
        #[arg(long, default_value = "target/receipts/quality/ripr-plus.json")]
        ripr_receipt: PathBuf,
        /// Diff-scoped RIPR PR evidence JSON path.
        #[arg(long, default_value = "target/ripr/pr/repo-exposure.json")]
        ripr_pr_receipt: PathBuf,
        /// RIPR review-guidance receipt JSON path.
        #[arg(long, default_value = "target/ripr/review/comments.json")]
        review_receipt: PathBuf,
        /// Coverage receipt JSON path.
        #[arg(long, default_value = "target/receipts/quality/coverage-baseline.json")]
        coverage_receipt: PathBuf,
        /// Codecov configuration path.
        #[arg(long, default_value = "codecov.yml")]
        codecov: PathBuf,
        /// Patch coverage percentage from Codecov for this PR.
        #[arg(long)]
        patch_coverage: Option<f64>,
        /// Base revision used for diff-scoped RIPR receipt commands.
        #[arg(long, default_value = "origin/HEAD")]
        ripr_base: String,
        /// Head revision used for diff-scoped RIPR receipt commands.
        #[arg(long, default_value = "HEAD")]
        ripr_head: String,
        /// Quality-gate JSON receipt path.
        #[arg(long, default_value = "target/receipts/quality/quality-gate.json")]
        receipt: PathBuf,
        /// Quality-gate Markdown summary path.
        #[arg(long, default_value = "target/receipts/quality/quality-gate.md")]
        summary: PathBuf,
        /// Validate existing quality-gate outputs instead of rewriting them.
        #[arg(long)]
        check: bool,
    },

    /// Produce diff-scoped RIPR PR evidence artifacts.
    RiprPr {
        /// Root passed to RIPR. Defaults to the repository root.
        #[arg(long, default_value = ".")]
        root: String,
        /// Base revision for the PR diff.
        #[arg(long, default_value = "origin/main")]
        base: String,
        /// Head revision for the PR diff.
        #[arg(long, default_value = "HEAD")]
        head: String,
        /// Original PR head SHA when the evaluated revision is a merge ref.
        #[arg(long)]
        pr_head: Option<String>,
        /// Validate existing target/ripr/pr artifacts instead of regenerating.
        #[arg(long)]
        check: bool,
    },

    /// Emit a repo-wide RIPR+ baseline receipt for the quality lane.
    RiprPlus {
        /// Root passed to RIPR. Defaults to the repository root.
        #[arg(long, default_value = ".")]
        root: String,
        /// Receipt JSON path.
        #[arg(long, default_value = "target/receipts/quality/ripr-plus.json")]
        receipt: PathBuf,
        /// RIPR suppression policy path.
        #[arg(long, default_value = "policy/ripr-suppressions.toml")]
        suppressions: PathBuf,
        /// Validate the existing receipt instead of rewriting it.
        #[arg(long)]
        check: bool,
    },

    /// Produce diff-scoped RIPR review guidance artifacts without posting comments.
    RiprReviewComments {
        /// Root passed to RIPR. Defaults to the repository root.
        #[arg(long, default_value = ".")]
        root: String,
        /// Base revision for the PR diff.
        #[arg(long, default_value = "origin/main")]
        base: String,
        /// Head revision for the PR diff.
        #[arg(long, default_value = "HEAD")]
        head: String,
        /// Original PR head SHA when the evaluated revision is a merge ref.
        #[arg(long)]
        pr_head: Option<String>,
        /// Bound RIPR review guidance generation; timeout writes an advisory error artifact.
        #[arg(long)]
        timeout_seconds: Option<u64>,
        /// Validate existing target/ripr/review artifacts instead of regenerating.
        #[arg(long)]
        check: bool,
    },

    /// Generate the stable PR evidence summary from machine-readable artifacts.
    RiprPrSummary {
        /// Validate the generated summary instead of rewriting it.
        #[arg(long)]
        check: bool,
    },

    /// Render non-blocking GitHub warning annotations from comments[] guidance only.
    RiprAnnotations {
        /// Review guidance JSON path.
        #[arg(long, default_value = "target/ripr/review/comments.json")]
        comments: String,
        /// Output path for rendered annotation commands.
        #[arg(long, default_value = "target/ripr/review/annotations.txt")]
        out: String,
        /// Validate existing annotation output instead of regenerating.
        #[arg(long)]
        check: bool,
    },

    /// Emit mutation-routing evidence from PR evidence and labels.
    ImpactedEvidence {
        /// PR evidence JSON input.
        #[arg(long, default_value = "target/ripr/pr/repo-exposure.json")]
        pr_evidence: String,
        /// Repeatable PR label input.
        #[arg(long = "label")]
        labels: Vec<String>,
        /// Comma, semicolon, or newline separated PR labels.
        #[arg(long)]
        labels_csv: Option<String>,
        /// Validate existing impacted evidence instead of regenerating.
        #[arg(long)]
        check: bool,
    },

    /// Run benchmarks
    Bench {
        /// Run specific benchmark
        #[arg(long)]
        name: Option<String>,

        /// Save benchmark results
        #[arg(long)]
        save: bool,

        /// Output file for results
        #[arg(long)]
        output: Option<PathBuf>,
    },

    /// Run C vs Rust benchmark comparison
    Compare {
        /// Run only C implementation benchmarks
        #[arg(long)]
        c_only: bool,

        /// Run only Rust implementation benchmarks
        #[arg(long)]
        rust_only: bool,

        /// Run scanner comparison only
        #[arg(long)]
        scanner_only: bool,

        /// Validate existing results only
        #[arg(long)]
        validate_only: bool,

        /// Output directory for results
        #[arg(long, default_value = "benchmark_results")]
        output_dir: PathBuf,

        /// Check performance gates
        #[arg(long)]
        check_gates: bool,

        /// Generate detailed report
        #[arg(long)]
        report: bool,
    },

    /// Run the benchmark script wrapper (`benchmarks/scripts/run-benchmarks.sh`).
    BenchRun {
        /// Write benchmark results to a JSON file.
        #[arg(long)]
        output: Option<PathBuf>,

        /// Run quick smoke benchmarks with reduced sample size.
        #[arg(long)]
        quick: bool,

        /// Restrict benchmarks to a specific category.
        #[arg(long)]
        category: Option<String>,
    },

    /// Compare benchmark output receipts (`benchmarks/scripts/compare.sh`).
    BenchCompare {
        /// Enable strict mode (exit non-zero on regression).
        #[arg(long)]
        fail_on_regression: bool,
    },

    /// Format benchmark JSON via `benchmarks/scripts/format-results.py`.
    BenchFormat {
        /// Emit a receipt summary for CI.
        #[arg(long)]
        receipt: bool,

        /// Emit markdown summary.
        #[arg(long)]
        markdown: bool,
    },

    /// Extract and normalize Criterion benchmark outputs (`target/criterion/.../estimates.json`).
    BenchExtract {
        /// Root path that contains `target/criterion`.
        #[arg(long)]
        base_path: Option<PathBuf>,

        /// Output JSON path.
        #[arg(long)]
        output: Option<PathBuf>,
    },

    /// Run benchmark alert checks (`benchmarks/scripts/alert.py`).
    BenchAlert {
        /// Output markdown alerts.
        #[arg(long)]
        format: Option<String>,

        /// Run checks and fail on warning conditions.
        #[arg(long)]
        check: bool,
    },

    /// Run the local benchmark alert regression test suite.
    BenchAlertTest,

    /// Generate Homebrew formula and VS Code asset map from checksums JSON.
    InjectShaAssets {
        /// Version tag used by release artifacts (e.g. v0.8.3).
        #[arg(long)]
        version: String,

        /// GitHub organization owning the release repository.
        #[arg(long)]
        owner: String,

        /// GitHub repository name for releases.
        #[arg(long)]
        repo: String,

        /// Artifact prefix for release filenames.
        #[arg(long)]
        prefix: String,

        /// Path to checksums JSON from cargo-dist.
        #[arg(long)]
        checksums: PathBuf,

        /// Optional output path for generated Homebrew formula.
        #[arg(long)]
        brew_out: Option<PathBuf>,

        /// Optional output path for generated VS Code extension asset map.
        #[arg(long)]
        asset_map_out: Option<PathBuf>,
    },

    /// Generate Homebrew formula from a release SHA256SUMS file.
    UpdateHomebrew {
        /// Release version tag used by release artifacts (e.g. v0.8.3).
        #[arg(long)]
        version: String,

        /// GitHub organization owning the release repository.
        #[arg(long, default_value = "EffortlessMetrics")]
        owner: String,

        /// GitHub repository name for releases.
        #[arg(long, default_value = "perl-lsp")]
        repo: String,

        /// Artifact prefix for release filenames.
        #[arg(long, default_value = "perllsp")]
        prefix: String,

        /// Output path for generated Homebrew formula.
        #[arg(long, default_value = "Formula/perllsp.rb")]
        output: PathBuf,
    },

    /// Generate documentation
    Doc {
        /// Open docs in browser
        #[arg(long)]
        open: bool,

        /// Build docs for all features
        #[arg(long)]
        all_features: bool,
    },

    /// Run code quality checks
    Check {
        /// Run clippy
        #[arg(long)]
        clippy: bool,

        /// Run formatting check
        #[arg(long)]
        fmt: bool,

        /// Run all checks
        #[arg(long)]
        all: bool,
    },

    /// Format code
    Fmt {
        /// Check formatting without making changes
        #[arg(long)]
        check: bool,

        /// Format only the staged Rust diff and re-stage it.
        ///
        /// The apply half of the `rustfmt_staged` commit gate: that check
        /// blocks a commit whose staged Rust would be reformatted, and this
        /// fixes exactly those files instead of the whole workspace. Files
        /// that are staged *and* separately modified in the worktree are left
        /// untouched, so formatting never sweeps unstaged work into a commit.
        ///
        /// Cannot be combined with --check or --package.
        #[arg(long, conflicts_with_all = ["check", "package"])]
        staged: bool,

        /// Restrict formatting to one or more package names.
        ///
        /// Accepts repeated flags (`--package xtask --package perl-parser`) or
        /// a comma-delimited list (`--package xtask,perl-parser`).
        #[arg(long, short = 'p', value_delimiter = ',')]
        package: Option<Vec<String>>,
    },

    /// Run corpus tests
    #[cfg(feature = "legacy")]
    Corpus {
        /// Path to corpus directory
        #[arg(long, default_value = "tree-sitter-perl/test/corpus")]
        path: PathBuf,

        /// Run with specific scanner
        #[arg(long, value_enum)]
        scanner: Option<ScannerType>,

        /// Run diagnostic analysis on first failing test
        #[arg(long)]
        diagnose: bool,

        /// Test current parser behavior with simple expressions
        #[arg(long)]
        test: bool,
    },

    /// Run highlight tests
    #[cfg(feature = "parser-tasks")]
    Highlight {
        /// Path to highlight test directory
        #[arg(long, default_value = "c/test/highlight")]
        path: PathBuf,

        /// Run with specific scanner
        #[arg(long, value_enum)]
        scanner: Option<ScannerType>,
    },

    /// Clean build artifacts
    Clean {
        /// Clean all artifacts including target
        #[arg(long)]
        all: bool,
    },

    /// Detect dead code, unused dependencies, and unused imports
    ///
    /// Combines cargo-machete/cargo-udeps with clippy dead_code lints.
    /// Supports check (against baseline), baseline generation, and JSON report modes.
    DeadCode {
        /// Mode: check (default), baseline, or report
        #[arg(value_enum, default_value = "check")]
        mode: DeadCodeMode,

        /// Strict mode: fail on any regression above baseline
        #[arg(long)]
        strict: bool,
    },

    /// Dependency hygiene: identify unused Cargo dependencies (authority: #9364).
    ///
    /// Uses cargo-machete as the V1 primary instrument. Produces typed
    /// item-level findings with outcome vocabulary:
    /// SUCCESS | POLICY_FINDING | NOT_PROVEN | NOT_APPLICABLE.
    ///
    /// Never installs tools as a side effect. cargo-udeps is removed from the
    /// active hygiene path; see issue #9364 for re-introduction criteria.
    #[command(name = "dependency-hygiene")]
    DependencyHygiene {
        /// Mode: check (default) fails closed on any finding; report writes JSON
        /// and exits 0.
        #[arg(value_enum, default_value = "check")]
        mode: DependencyHygieneMode,
    },

    /// Run a developer environment smoke check.
    DevexDoctor,

    /// Developer experience helpers.
    Devex {
        #[command(subcommand)]
        command: DevexCommand,
    },

    /// Validate the static provider-native agent-flow topology.
    #[command(name = "agent-flow")]
    AgentFlow {
        #[command(subcommand)]
        command: AgentFlowCommand,
    },

    /// Plan bounded serial pre-push proof from the shared change set.
    ///
    /// PLANNING ONLY: emits a deterministic proof plan, including the change-set
    /// digest, selected and deferred steps, and posture. It runs none of the
    /// planned Cargo, workflow, or RIPR commands and changes no hook behavior.
    /// `--base auto` delegates base resolution to the shared change-set resolver.
    PrePushPlan {
        /// Git base ref used by the shared change-set resolver.
        #[arg(long, default_value = "auto")]
        base: String,
        /// Commit-ish head consumed by the shared change-set resolver.
        #[arg(long, default_value = "HEAD")]
        head: String,
        /// Output format: human or json.
        #[arg(long, default_value = "human")]
        format: String,
    },

    /// Audit CI workflows for PR-safety and spend-risk controls.
    CiAuditWorkflows,

    /// Derive the active CI cache inventory + `ci_cache_receipt.v1` (#9177).
    ///
    /// Diagnostic: classifies reachability, save authority, and byte
    /// provenance for every `Swatinem/rust-cache`/`actions/cache` step
    /// already in source. Changes no cache behavior and grants no save
    /// authority.
    CiCacheInventory {
        /// Diff the derived inventory against the checked-in manifest and
        /// fail on drift, instead of (re)writing it.
        #[arg(long)]
        check: bool,

        /// Write the receipt JSON to this path instead of stdout.
        /// Rejected alongside `--check`, which never writes a receipt.
        #[arg(long)]
        receipt: Option<PathBuf>,

        /// Checked-in manifest path (defaults to `ci_cache_inventory::MANIFEST`).
        #[arg(long)]
        manifest: Option<PathBuf>,

        /// Schema version this producer must emit and validate. Only
        /// `v1` is supported; anything else fails loudly instead of
        /// emitting a shape the caller does not parse.
        #[arg(long, default_value = "v1")]
        api_version: String,
    },

    /// Classify credential derivation kinds in `.github/workflows/*.yml` (#14867).
    ///
    /// Advisory inventory of the credential column. Does not change workflow
    /// behavior and is not a merge gate.
    WorkflowAuthorityInventory {
        /// Write JSON to this path instead of stdout.
        #[arg(long)]
        receipt: Option<PathBuf>,
    },

    /// Lint GitHub workflow security policy invariants.
    WorkflowPolicyLint {
        /// Evaluate this repository instead of the compile-time project root.
        #[arg(long, conflicts_with = "fixture")]
        root: Option<PathBuf>,

        /// Write a JSON receipt artifact for CI consumption.
        #[arg(long)]
        receipt: Option<PathBuf>,

        /// Lint a single workflow fixture instead of repository workflows.
        #[arg(long)]
        fixture: Option<PathBuf>,

        /// Also validate that every workflow has a `[[lane]]` entry in
        /// policy/ci-lane-whitelist.toml. Advisory (warning-level) until the
        /// whitelist has stabilized — see docs/ci/perl-lsp-rollout-plan.md PR 11.
        #[arg(long, conflicts_with = "fixture")]
        check_lane_whitelist: bool,
    },

    /// Measure CI lane runtimes and emit timing artifacts.
    CiMeasure,

    /// Time workspace Clippy by target-kind scope under controlled cache
    /// states and write a receipt (#11736 decision-1 cost instrument).
    ClippyCostMeasure {
        /// Receipt output path (relative paths resolve against the project root).
        #[arg(long, default_value = "target/receipts/clippy-cost-measurement.json")]
        receipt: PathBuf,

        /// Comma-separated scopes to measure: lib,all-targets
        #[arg(long, value_delimiter = ',', default_value = "lib,all-targets")]
        scopes: Vec<clippy_cost_measure::ClippyScope>,

        /// Comma-separated cache states to measure: warm,members-cold
        #[arg(long, value_delimiter = ',', default_value = "warm,members-cold")]
        states: Vec<clippy_cost_measure::ClippyCacheState>,

        /// Per-pass watchdog in seconds; a killed pass fails the measurement loudly.
        #[arg(long, default_value_t = 2400)]
        timeout_secs: u64,
    },

    /// Analyze GitHub Actions costs over a recent period.
    CiCostMonitor {
        /// Number of days to analyze.
        #[arg(long, default_value_t = 30)]
        days: u64,

        /// Emit machine-readable output.
        #[arg(long)]
        json: bool,
    },

    /// Measure CI baseline from recent workflow runs.
    CiBaseline {
        /// Branch to analyze. When empty, the repository's default branch is
        /// derived from `gh repo view --json defaultBranchRef` instead of
        /// assuming a hard-coded name (which silently returned zero rows on
        /// the `main` branch).
        #[arg(short, long, default_value = "")]
        branch: String,

        /// Number of days to analyze.
        #[arg(short, long, default_value_t = 30)]
        days: u64,

        /// Max runs to fetch.
        #[arg(short, long, default_value_t = 200)]
        limit: usize,

        /// Output directory for ci_baseline artifacts.
        ///
        /// Defaults to `target/metrics` so the consumer in
        /// `metrics::release_health::read_ci_baseline` finds the file at the
        /// canonical contract path. Override to a different directory to keep
        /// historical or per-branch baselines side by side.
        #[arg(short, long, default_value = metrics::release_health::CI_BASELINE_OUTPUT_DIR)]
        output: PathBuf,
    },

    /// Compute the CI scope — changed crates, reverse-dep closure, and architectural wideners.
    ///
    /// Emits a JSON (or text) payload listing changed files, mapped crates, the
    /// reverse-dependency closure, architectural wideners applied, and the
    /// selected CI lanes with reasons. Deterministic given the same diff and
    /// `cargo metadata` output.
    ///
    /// Example: `cargo xtask ci-scope --base auto --format json`
    CiScope {
        /// Base git reference to diff against (default: auto-detect).
        #[arg(long, default_value = "auto")]
        base: String,

        /// Immutable CI subject receipt. When supplied, the exact receipt
        /// identity replaces mutable base/HEAD discovery.
        #[arg(long)]
        subject: Option<PathBuf>,

        /// Repository root override for hermetic fixtures.
        #[arg(long)]
        root: Option<PathBuf>,

        /// Output format: `json` or `text` (default: json).
        #[arg(long, default_value = "json")]
        format: String,
    },

    /// Resolve one immutable GitHub-event subject and bounded input receipt (#8042).
    CiSubject {
        /// Event kind (`pull_request`, `push`, `merge_group`,
        /// `workflow_dispatch`, or `explicit`). Defaults to
        /// `GITHUB_EVENT_NAME`, then `explicit`.
        #[arg(long)]
        event_name: Option<String>,
        /// GitHub event JSON. Defaults to `GITHUB_EVENT_PATH`.
        #[arg(long)]
        event_path: Option<PathBuf>,
        /// Expected owner/name. Defaults to `GITHUB_REPOSITORY`.
        #[arg(long)]
        repository: Option<String>,
        /// Exact GitHub workflow SHA. Defaults to `GITHUB_SHA`.
        #[arg(long)]
        github_sha: Option<String>,
        /// Exact base SHA for explicit/workflow-dispatch subjects.
        #[arg(long)]
        base_sha: Option<String>,
        /// Exact head SHA for explicit/workflow-dispatch subjects.
        #[arg(long)]
        head_sha: Option<String>,
        /// Bounded semantic receipt path.
        #[arg(long)]
        receipt: PathBuf,
        /// Repository root override for hermetic fixtures.
        #[arg(long)]
        root: Option<PathBuf>,
    },

    /// Run the thin exact-head repository contract advisory (issue #3987).
    CiContract {
        /// Base git ref or full SHA for the evaluated range.
        #[arg(long, default_value = "origin/main")]
        base: String,
        /// Head git ref or full SHA for the evaluated range.
        #[arg(long, default_value = "HEAD")]
        head: String,
        /// Immutable CI subject receipt. When supplied, its exact identity
        /// and changed-input digest replace independent event resolution.
        #[arg(long)]
        subject: Option<PathBuf>,
        /// JSON receipt output path.
        #[arg(long, default_value = "target/receipts/ci-contract.json")]
        receipt: PathBuf,
        /// Markdown summary output path.
        #[arg(long, default_value = "target/receipts/ci-contract.md")]
        summary: PathBuf,
    },

    /// Capture typed evidence for one command or a small serial proof set.
    CommandEvidence {
        #[command(subcommand)]
        command: CommandEvidenceCommand,
    },

    /// Construct one bounded synthetic integration proof from an existing
    /// trigger packet and selected command evidence.
    #[command(name = "integration-proof")]
    IntegrationProof {
        /// JSON input containing the #4588 trigger packet and selected proof commands.
        #[arg(long)]
        spec: PathBuf,
        /// JSON receipt output path.
        #[arg(long, default_value = "target/receipts/integration-proof.json")]
        receipt: PathBuf,
    },

    /// Run exact-head Taplo and typos checks for changed repository files.
    ///
    /// The command composes the shared change-set resolver and invokes both
    /// tools through the pinned Aqua inventory. Missing tooling is reported as
    /// NOT_PROVEN and exits non-zero; it never becomes a silent pass.
    RepoHygiene {
        /// Base git ref or full SHA for the evaluated range.
        #[arg(long, default_value = "origin/main")]
        base: String,
        /// Head git ref or full SHA for the evaluated range.
        #[arg(long, default_value = "HEAD")]
        head: String,
        /// JSON receipt output path.
        #[arg(long, default_value = "target/receipts/repo-hygiene.json")]
        receipt: PathBuf,
        /// Markdown summary output path.
        #[arg(long, default_value = "target/receipts/repo-hygiene.md")]
        summary: PathBuf,
    },

    /// Resolve a change set (base/head SHAs + changed paths) via the single
    /// #3985 `change_set::resolve_change_set` base-resolver + diff — the
    /// runtime-neutral interface `hooks/pre-push` consumes (#3985 Slice 3A)
    /// so the hook never needs its own shell base-resolution algorithm.
    ///
    /// `--base auto` (the default) walks the main-first candidate chain
    /// (`origin/main`, `main`, `HEAD~1`) and never falls back to
    /// `origin/master` (issue #3985: that ref does not exist on this
    /// remote). An explicit `--base` must resolve on its own — an
    /// unresolvable explicit base is a loud, non-zero-exit error, never a
    /// silent substitution or an empty-changed-paths "success".
    ///
    /// `--format json` (default) emits the bounded contract
    /// `{base_sha, head_sha, changed_paths}`. `--format paths` emits one
    /// changed path per line and nothing else — the lean, `jq`-free shape
    /// `hooks/pre-push` parses.
    ///
    /// Example: `cargo xtask change-set --base auto --head HEAD --format paths`
    ChangeSet {
        /// Base git ref to diff against. `"auto"` (default) triggers
        /// main-first candidate resolution; any other value is treated as
        /// an explicit base that must resolve on its own.
        #[arg(long, default_value = "auto")]
        base: String,

        /// Head git ref/SHA to diff to.
        #[arg(long, default_value = "HEAD")]
        head: String,

        /// Output format: `json` (default, bounded contract) or `paths`
        /// (one changed path per line, no SHAs). Any other value is a
        /// loud error, never a silent fallback to `json`.
        #[arg(long, default_value = "json")]
        format: String,

        /// Repository root to resolve the change set against. Defaults to
        /// the perl-lsp workspace root. Override for testing against a
        /// fixture repository.
        #[arg(long)]
        root: Option<PathBuf>,
    },

    /// Shadow-parity measurement: compare the pre-push shell selector's
    /// doc-only/single-crate taxonomy against `ci_scope::classify_files`'s
    /// Rust taxonomy across a fixed corpus of 11 representative
    /// changed-path scenarios (#3985 Slice 3B).
    ///
    /// MEASUREMENT ONLY — selects, skips, and routes nothing. `hooks/pre-push`
    /// and `ci_scope.rs` are untouched; this command only reports where the
    /// two selectors agree or differ, and in which direction, to feed the
    /// maintainer's pending coverage decision (see #3985 comments).
    ///
    /// Example: `cargo xtask change-set-parity --format markdown`
    ChangeSetParity {
        /// Output format: `text` (default, human-readable), `markdown` (the
        /// committed-report table shape), or `json`.
        #[arg(long, default_value = "text")]
        format: String,
    },

    /// Report which "seams" (changed files, plus a coarse changed-crate
    /// set) a push changed between a recorded review-epoch marker SHA and
    /// current HEAD — advisory, read-only slice 1 of issue #3986. Composes
    /// `change_set::resolve_change_set_with_mode` with
    /// `DiffMode::DirectTwoDot` (see `cargo xtask change-set`
    /// above); does not reimplement git diff or base resolution.
    ///
    /// This is a reporter, not a gate: it changes no bot trigger, no
    /// required check, no branch-protection rule, and nothing about what
    /// merges. See `.claude/reference/review-convergence.md` § Review-epoch
    /// markers for the `review-epoch: examined <full-sha>` PR-comment
    /// convention this reporter is meant to consume.
    ///
    /// `--base` must resolve on its own (an invalid/nonexistent base SHA is
    /// a loud, non-zero-exit error, never a silently-empty "no seams
    /// changed" report).
    ///
    /// Example: `carg…46666 tokens truncated…yclePolicy => ci_policy::check_memory_lifecycle(),
        Commands::CheckMemoryRetainedOwnerDrift { base, report_only } => {
            ci_policy::check_memory_retained_owner_drift(ci_policy::RetainedOwnerDriftConfig {
                base,
                report_only,
            })
        }
        Commands::MemoryTrends { command } => match command {
            MemoryTrendsCommand::Render { input_dir, history_dirs, baseline, output } => {
                memory_trends::render(memory_trends::MemoryTrendsConfig {
                    input_dir,
                    history_dirs,
                    baseline,
                    output,
                })
            }
        },
        Commands::NativeFormat { command } => match command {
            NativeFormatCommand::Check { fixtures, receipt_dir } => {
                native_format::check(native_format::NativeFormatCheckConfig {
                    fixtures,
                    receipt_dir,
                })
            }
            NativeFormatCommand::Corpus { roots, receipt, summary } => {
                native_format::corpus(native_format::NativeFormatCorpusConfig {
                    roots,
                    receipt,
                    summary,
                })
            }
            NativeFormatCommand::PerltidyCompat { profile, receipt, summary } => {
                native_format::perltidy_compat(native_format::NativeFormatPerltidyCompatConfig {
                    profile,
                    receipt,
                    summary,
                })
            }
            NativeFormatCommand::Config { workspace_root, receipt, summary } => {
                native_format::config(native_format::NativeFormatConfigReceiptConfig {
                    workspace_root,
                    receipt,
                    summary,
                })
            }
        },
        Commands::NativeCritic { command } => match command {
            NativeCriticCommand::Check {
                roots,
                profile,
                severity,
                include,
                exclude,
                receipt,
                summary,
            } => native_critic::check(native_critic::NativeCriticCheckConfig {
                roots,
                profile,
                severity,
                include,
                exclude,
                receipt,
                summary,
            }),
        },
        Commands::NativeTooling { command } => match command {
            NativeToolingCommand::Status {
                format_fixtures,
                format_receipt,
                format_corpus_receipt,
                format_perltidy_compat_receipt,
                format_config_receipt,
                critic_perlcritic_compat_receipt,
                critic_check_receipt,
                critic_false_positive_receipt,
                receipt,
                markdown,
            } => native_tooling::status(native_tooling::NativeToolingStatusConfig {
                format_fixtures,
                format_receipt,
                format_corpus_receipt,
                format_perltidy_compat_receipt,
                format_config_receipt,
                critic_perlcritic_compat_receipt,
                critic_check_receipt,
                critic_false_positive_receipt,
                receipt,
                markdown,
            }),
            NativeToolingCommand::PerlcriticCompat { profile, receipt, summary } => {
                native_tooling::perlcritic_compat(native_tooling::PerlcriticCompatConfig {
                    profile,
                    receipt,
                    summary,
                })
            }
            NativeToolingCommand::CheckDefaults { root } => {
                native_tooling::check_defaults(native_tooling::NativeToolingDefaultsConfig { root })
            }
            NativeToolingCommand::Readiness { status_receipt, receipt, markdown } => {
                native_tooling::readiness(native_tooling::NativeToolingReadinessConfig {
                    status_receipt,
                    receipt,
                    markdown,
                })
            }
        },
        Commands::PerlKwalitee { command } => match command {
            PerlKwaliteeCommand::Check { profile, dist, strict, repo_root } => {
                perl_kwalitee::check(profile, dist, strict, repo_root)
            }
            PerlKwaliteeCommand::Report { profile, dist, json, markdown, repo_root } => {
                // Default receipt paths anchor to the tree being evaluated:
                // the override root when given, else the live workspace root.
                let anchor = match &repo_root {
                    Some(r) => r.clone(),
                    None => utils::project_root()?,
                };
                let json = json.unwrap_or_else(|| perl_kwalitee::default_json_path(&anchor));
                let markdown =
                    markdown.unwrap_or_else(|| perl_kwalitee::default_markdown_path(&anchor));
                perl_kwalitee::report(profile, dist, json, markdown, repo_root)
            }
            PerlKwaliteeCommand::Explain { indicator } => perl_kwalitee::explain(&indicator),
        },
        Commands::SecurityHardening => hardening::security_hardening(),
        Commands::PerformanceHardening => hardening::performance_hardening(),
        Commands::ProductionGatesValidation => hardening::production_gates_validation(),
        Commands::ForensicsHarvest { pr } => forensics::run_harvest(&pr),
        Commands::ForensicsTemporal { pr } => forensics::run_temporal(&pr),
        Commands::ForensicsTelemetryQuick { pr } => forensics::run_telemetry_quick(&pr),
        Commands::ForensicsTelemetryFull { pr } => forensics::run_telemetry_full(&pr),
        Commands::ForensicsDossier { pr } => forensics::run_dossier(&pr),
        Commands::ForensicsRender { pr, format } => forensics::run_render(&pr, &format),
        Commands::VerifyPublicationFacts { args } => publication_facts::run(args),
        Commands::GhLabels => github::run_labels(),
        Commands::GhTriage { limit } => github::run_issues_needing_triage(limit),
        Commands::GhBackfillPrefixedLabels { apply } => github::run_backfill_prefixed_labels(apply),
        Commands::GhCandidate { command } => match command {
            GhGithubCommand::Candidate { pr, expected_head, fixture, json } => {
                github::run_candidate(pr, expected_head, fixture, json)
            }
        },
        Commands::GhReviewConvergence { pr, json } => {
            github_review::run_review_convergence(pr, json)
        }
        Commands::GhPreflight { pr, json } => github_preflight::run_preflight(pr, json),
        Commands::CorpusAudit { corpus_path, output, check, fresh } => {
            corpus_audit::run(corpus_audit::AuditConfig {
                corpus_path,
                output_path: output,
                timeout: std::time::Duration::from_secs(30),
                fresh,
                check,
            })
        }
        Commands::CorpusAuditParseOne { path } => corpus_audit::run_parse_one(path),
        Commands::ParserMatrix { report, output } => parser_matrix::run_with_paths(report, output),
        #[cfg(feature = "parser-tasks")]
        Commands::CompareThree { verbose, format } => {
            compare_parsers::run_three_way(verbose, format.as_str())
        }
        Commands::TestLsp { create_only, test, cleanup } => {
            test_lsp::run(create_only, test, cleanup)
        }
        Commands::BumpVersion { version } => bump_version::run(version),
        Commands::PublishCrates { yes, dry_run } => publish::publish_crates(yes, dry_run),
        Commands::PublishRelease { version, dry_run, git_ref } => {
            publish::publish_release(version, dry_run, git_ref)
        }
        Commands::HookCheck => hook_checks::run_hook_check(),
        Commands::HookRegistryCheck => hook_checks::run_hook_registry_check(),
        Commands::HookTests => hook_checks::run_hook_tests(),
        Commands::ForbidFatalConstructs { args } => forbid_fatal_constructs::run(args),
        Commands::CiHygiene { command, args } => ci_hygiene::run(command, args),
        Commands::PublishVscode { yes, token } => publish::publish_vscode(yes, token),
        Commands::PublishClosure { crate_name } => publish_closure::run(crate_name),
        Commands::PublishedCrateCount => count_ratchet::run(),
        Commands::PublishManifestCheck => publish_manifest_check::run(),
        Commands::SmokeTestRelease { version } => publish::smoke_test_release(version),
        Commands::PublishReceipts { date } => publish_receipts::run(date),
        Commands::ParserCorpusSweep {
            roots,
            manifest,
            output,
            baseline,
            enforce,
            verbose,
            receipt,
            profile,
        } => parser_corpus_sweep::run(build_parser_corpus_sweep_config(
            roots, manifest, output, baseline, enforce, verbose, receipt, profile,
        )),
        Commands::TreeSitterIncrementalProof { profile, output } => {
            incremental_proof::run(profile, output)
        }
        Commands::PerlCoreHarness { command } => match command {
            PerlCoreHarnessCommand::Prepare { perl_ref, output_dir } => {
                perl_core_harness::prepare(perl_core_harness::PrepareConfig {
                    perl_ref,
                    output_dir,
                })
            }
            PerlCoreHarnessCommand::Discover { perl_tree, host_perl, runner, profile, output } => {
                perl_core_harness::discover(perl_core_harness::DiscoverConfig {
                    perl_tree,
                    host_perl,
                    runner,
                    profile,
                    output,
                })
            }
            PerlCoreHarnessCommand::SeriesManifest {
                discovery,
                output,
                series_id,
                profile,
                perl_requested_ref,
                perl_resolved_ref,
                preparation_receipt_id,
                preparation_receipt_digest,
                compiler_subject_identity,
                invocation_identity,
                capability_identity,
                environment_identity,
                replaces_series_id,
                change_reason,
                check,
            } => perl_core_harness::series_manifest(perl_core_harness::SeriesManifestConfig {
                discovery,
                output,
                series_id,
                profile,
                perl_requested_ref,
                perl_resolved_ref,
                preparation_receipt_id,
                preparation_receipt_digest,
                compiler_subject_identity,
                invocation_identity,
                capability_identity,
                environment_identity,
                replaces_series_id,
                change_reason,
                check,
            }),
            PerlCoreHarnessCommand::Boundaries {
                registry,
                baselines,
                bundles,
                output,
                check,
                report,
                historical,
            } => perl_core_harness::boundaries(perl_core_harness::BoundaryRegistryConfig {
                registry,
                baselines,
                bundles,
                output,
                check: check || !report,
                report,
                historical,
            }),
            PerlCoreHarnessCommand::Triage {
                bundle,
                output,
                history,
                write_history,
                check_history,
            } => perl_core_harness::triage(perl_core_harness::TriageConfig {
                bundle,
                output,
                history,
                write_history,
                check_history,
            }),
            PerlCoreHarnessCommand::CurrentAuthority {
                index,
                lineages,
                repository_root,
                landed_sha,
            } => perl_core_harness::validate_current_authority(
                perl_core_harness::CurrentAuthorityConfig {
                    index,
                    lineages,
                    repository_root,
                    landed_sha,
                },
            )
            .map(|_| ()),
            PerlCoreHarnessCommand::Run {
                mode,
                perl_tree,
                host_perl,
                runner,
                profile,
                tests,
                output,
                runner_binary,
                no_diagnostic_probes,
            } => perl_core_harness::run_mode(perl_core_harness::RunConfig {
                perl_tree,
                host_perl,
                runner,
                mode,
                profile,
                tests,
                output,
                runner_binary,
                diagnostic_probes: !no_diagnostic_probes,
            }),
            PerlCoreHarnessCommand::Report => perl_core_harness::report(),
            PerlCoreHarnessCommand::Baseline {
                mode,
                profile,
                report,
                baseline,
                series,
                previous_baseline,
                boundary_retirements,
                compiler_subject_identity,
                invocation_identity,
                capability_identity,
                environment_identity,
                accepted_transition_id,
                evidence_bundle,
                check: _,
                accept,
            } => perl_core_harness::baseline(perl_core_harness::BaselineConfig {
                mode,
                profile,
                report,
                baseline,
                accept,
                series,
                previous_baseline,
                boundary_retirements,
                compiler_subject_identity,
                invocation_identity,
                capability_identity,
                environment_identity,
                accepted_transition_id,
                evidence_bundle,
            }),
            PerlCoreHarnessCommand::Smoke {
                perl_tree,
                host_perl,
                runner,
                profile,
                modes,
                output_dir,
                runner_binary,
                perl_ref,
            } => perl_core_harness::smoke(perl_core_harness::SmokeConfig {
                perl_tree,
                host_perl,
                runner,
                profile,
                modes,
                output_dir,
                runner_binary,
                perl_ref,
            }),
        },
        Commands::ParserRatchet { command } => match command {
            ParserRatchetCommand::Run { profile, base, head, receipt, force_selected } => {
                parser_ratchet::run(parser_ratchet::ParserRatchetRunConfig {
                    profile,
                    base,
                    head,
                    receipt,
                    force_selected,
                })
            }
        },
        Commands::CpanCorpus { command } => {
            let mut config = cpan_corpus::CpanCorpusConfig::default();
            match command {
                CpanCorpusCommand::FetchList { top_n, output } => {
                    config.top_n = top_n;
                    if let Some(out) = output {
                        config.dist_list = out;
                    }
                    cpan_corpus::fetch_list(&config)
                }
                CpanCorpusCommand::Install {
                    dist_list,
                    install_dir,
                    verbose,
                    reset,
                    time_budget_minutes,
                } => {
                    if let Some(dl) = dist_list {
                        config.dist_list = dl;
                    }
                    config.force_reset = reset;
                    if let Some(id) = install_dir {
                        config.install_dir = id;
                    }
                    config.verbose = verbose;
                    config.time_budget = match time_budget_minutes {
                        None => None,
                        Some(mins) => {
                            let secs = mins.checked_mul(60).ok_or_else(|| {
                                color_eyre::eyre::eyre!(
                                    "--time-budget-minutes {mins} overflows the budget clock"
                                )
                            })?;
                            Some(std::time::Duration::from_secs(secs))
                        }
                    };
                    cpan_corpus::install(&config)
                }
                CpanCorpusCommand::Sweep { output, enforce, verbose, install_dir } => {
                    if let Some(id) = install_dir {
                        config.install_dir = id;
                    }
                    config.verbose = verbose;
                    cpan_corpus::sweep(&config, output, enforce)
                }
                CpanCorpusCommand::Ratchet { verbose, install_dir } => {
                    if let Some(id) = install_dir {
                        config.install_dir = id;
                    }
                    config.verbose = verbose;
                    cpan_corpus::ratchet(&config)
                }
            }
        }
        Commands::Receipts { tests_only, docs_only, output_dir, test_threads } => {
            receipts::run(receipts::ReceiptsConfig {
                tests_only,
                docs_only,
                output_dir,
                test_threads,
            })
        }
        Commands::AggregateReceipts { check, inputs, output, allow_noop } => {
            aggregate_receipts::run(aggregate_receipts::AggregateReceiptsConfig {
                check,
                inputs,
                output,
                allow_noop,
            })
        }
        Commands::FinalizeCheck { receipt, allow_noop, fail_on_advisory } => {
            finalize_check::run(finalize_check::FinalizeCheckConfig {
                receipt,
                allow_noop,
                fail_on_advisory,
            })
        }
        Commands::MergeReady { command } => match command {
            MergeReadyCommand::Evaluate { snapshot, output } => {
                merge_ready::evaluate_snapshot_file(&snapshot, output.as_deref())
            }
            MergeReadyCommand::Emit { pr, receipt, snapshot } => {
                merge_ready::emit(pr, receipt, snapshot)
            }
            MergeReadyCommand::Verify { pr, fixture } => merge_ready::verify(pr, fixture),
        },
        Commands::IgnoredTests { update, check, check_issue_refs, verbose } => {
            ignored_tests::run(update, check, check_issue_refs, verbose)
        }
        Commands::DebtReport { check, json, summary, expired, ledger } => {
            debt_report::run(debt_report::DebtReportConfig {
                check,
                json,
                summary,
                expired,
                ledger,
            })
        }
        Commands::DocClaims => doc_claims::run(),
        Commands::InstallSurfaceCheck => install_surface_check::run(),
        Commands::IntentDiffGate { pr, fixture, receipt } => {
            intent_diff_gate::run(intent_diff_gate::IntentDiffGateConfig { pr, fixture, receipt })
        }
        Commands::Features { command } => match command {
            FeaturesCommand::SyncDocs => features::sync_docs(),
            FeaturesCommand::Verify => features::verify(),
            FeaturesCommand::Invariants => features::invariants(),
            FeaturesCommand::Report => features::report(),
            FeaturesCommand::RegenVendored => features::regen_vendored(),
        },
        Commands::Agent { command } => match command {
            AgentCommand::Lease { command } => match command {
                AgentLeaseCommand::Acquire { task, out } => agent_lease::acquire(&task, &out),
                AgentLeaseCommand::Verify { lease, current } => {
                    agent_lease::verify(&lease, &current)
                }
            },
            AgentCommand::Ledgers { command } => match command {
                AgentLedgersCommand::Validate { dir, format, expected_schema } => {
                    let fmt = match format.as_str() {
                        "json" => tasks::agent_ledgers::ValidateFormat::Json,
                        "human" => tasks::agent_ledgers::ValidateFormat::Human,
                        other => color_eyre::eyre::bail!(
                            "unknown --format `{other}`; expected `human` or `json`"
                        ),
                    };
                    tasks::agent_ledgers::validate(tasks::agent_ledgers::ValidateConfig {
                        ledger_dir: dir,
                        format: fmt,
                        expected_schema,
                    })
                }
            },
            AgentCommand::Receipt { command } => match command {
                AgentReceiptCommand::Validate { receipt } => agent_receipt::validate(&receipt),
            },
            AgentCommand::Worktree { command } => worktree_allocator::run(command),
        },
        Commands::FixForward { command } => match command {
            FixForwardCommand::Classify { receipt, output } => {
                fix_forward::classify(receipt, output)
            }
            FixForwardCommand::ListPlaybooks => fix_forward::list_playbooks(),
        },
        Commands::UpdateStatus { write, check, only } => update_status::run(write, check, only),
        Commands::Srp { command } => match command {
            SrpCommand::Microcrates(args) => srp_microcrates::run(args.output),
            SrpCommand::LayerCheck => layer_check::run(),
            SrpCommand::UnwiredScan(args) => unwired_scan::run(UnwiredScanConfig {
                lsp_crate: args.lsp_crate,
                json: args.json,
                check: args.check,
            }),
            SrpCommand::CheckTestWiring => check_test_wiring::run(),
        },
        Commands::SrpMicrocrates { args } => srp_microcrates::run(args.output),
        Commands::LayerCheck => layer_check::run(),
        Commands::UnwiredScan { args } => unwired_scan::run(UnwiredScanConfig {
            lsp_crate: args.lsp_crate,
            json: args.json,
            check: args.check,
        }),
        Commands::CheckTestWiring => check_test_wiring::run(),
        Commands::CompilerProfile { command } => {
            let root = utils::project_root()?;
            match command {
                CompilerProfileCommand::List => {
                    tasks::compiler_profile::list(&root).map_err(|error| eyre!(error.to_string()))
                }
                CompilerProfileCommand::Check { path } => {
                    tasks::compiler_profile::check(&path).map_err(|error| eyre!(error.to_string()))
                }
            }
        }
        Commands::Compiler { command } => match command {
            CompilerUpstreamCommand::Upstream { command } => match command {
                CompilerUpstreamStatusGroup::Status { command } => {
                    compiler_upstream_status::run(command)
                }
            },
        },
        Commands::Metrics { command } => match command {
            MetricsCommand::ParserStats { input, json } => metrics::parser_stats::run(input, json),
            MetricsCommand::ParserAccuracy {
                json,
                check,
                export_status_receipts,
                manifest,
                output,
                cadence,
            } => metrics::parser_accuracy::run(
                json,
                check,
                export_status_receipts,
                manifest,
                output,
                &cadence,
            ),
            MetricsCommand::HirCoverage { json, output, write_status, check } => {
                metrics::hir_coverage::run(json, output, write_status, check)
            }
            MetricsCommand::LspStats { json, receipt_dir, output } => {
                metrics::lsp_stats::run_with_receipt_dir(
                    json,
                    receipt_dir.as_deref(),
                    output.as_deref(),
                )
            }
            MetricsCommand::WorkspaceStats => metrics::workspace_stats::run(),
            MetricsCommand::DiagnosticsStats => metrics::diagnostics_stats::run(),
            MetricsCommand::Memory {
                workload_json,
                plateau_json,
                scenario,
                receipt,
                commit,
                event,
                markdown,
            } => {
                let scenario = match scenario {
                    Some(scenario) => scenario,
                    None => metrics::memory::infer_scenario(&workload_json)
                        .map_err(|error| eyre!(error.to_string()))?,
                };
                metrics::memory::run(metrics::memory::MemoryMetricsConfig {
                    scenario,
                    workload_json,
                    plateau_json,
                    receipt,
                    commit,
                    event,
                    markdown,
                })
            }
            MetricsCommand::ReleaseHealth { days, json } => {
                metrics::release_health::run(days, json)
            }
            MetricsCommand::RatchetCheck { subsystem, current, record } => {
                let root = utils::project_root()?;
                metrics::ratchet::run_ratchet_check(&root, &subsystem, current, record)
            }
            MetricsCommand::PromoteBaseline { subsystem, delta_pct } => {
                let root = utils::project_root()?;
                metrics::ratchet::run_promote_baseline(&root, &subsystem, delta_pct)
            }
            MetricsCommand::SweepStats { input } => metrics::sweep_stats::run(input),
        },
        Commands::UxScorecard { format, input, output, status_md, ratchet_check } => {
            let format = match format {
                UxScorecardOutputFormat::Human => UxScorecardFormat::Human,
                UxScorecardOutputFormat::Json => UxScorecardFormat::Json,
            };
            ux_scorecard::run(format, input, output, status_md, ratchet_check)
        }
        Commands::SemanticScorecard { manifest, output, status_md, check } => {
            semantic_scorecard::run(manifest, output, status_md, check)
        }
        Commands::RustSmallProof { receipt, verify_receipt } => {
            rust_small_proof::run(receipt, verify_receipt)
        }
        Commands::SemanticShadowCompare { output, status_md, check } => {
            semantic_shadow_compare::run(output, status_md, check)
        }
        Commands::UxRegressionReceipt { input, receipt, sha, exit_status_file } => {
            ux_regression_receipt::run(ux_regression_receipt::UxRegressionReceiptConfig {
                input,
                receipt,
                sha,
                exit_status_file,
            })
        }
        Commands::ValidateMemoryProfiler => compare::validate_memory_profiling(),
        Commands::E2eValidate { workspace_size, report, skip_workspace, skip_bench, verbose } => {
            e2e_validate::run(e2e_validate::E2eConfig {
                workspace_size,
                report_path: report,
                skip_workspace,
                skip_bench,
                verbose,
            })
        }
        Commands::Gates {
            tier,
            gate_policy,
            gate,
            base,
            subject,
            list,
            explain_denominator,
            explain_disposition,
            format,
            receipt,
            receipt_path,
            diff,
            fail_fast,
            parallel,
            verbose,
            staged,
            route_plan,
        } => gates::run(gates::GateRunnerConfig {
            tier,
            gate_policy: Some(gate_policy),
            gate_filter: gate,
            base_ref: base,
            subject,
            output_format: format,
            emit_receipt: receipt,
            receipt_path,
            diff_baseline: diff,
            list_only: list,
            explain_denominator,
            explain_disposition,
            fail_fast,
            parallel,
            verbose,
            staged,
            route_plan_path: route_plan,
        }),
        Commands::Precommit { format, receipt } => gates::run(gates::GateRunnerConfig {
            tier: GateTier::Commit,
            output_format: format,
            emit_receipt: receipt,
            staged: true,
            ..gates::GateRunnerConfig::default()
        }),
        Commands::LspSmokeAtomic { receipt } => tasks::lsp_smoke_atomic::run_cli(&receipt),
        Commands::GatePolicy { command } => match command {
            GatePolicyCommand::Check => match tasks::gate_policy::check() {
                Ok(()) => Ok(()),
                Err(error) => {
                    eprintln!("gate-policy: instrument failure: {error}");
                    std::process::exit(2);
                }
            },
            GatePolicyCommand::Effective { profile } => tasks::gate_policy::effective(profile),
        },
        Commands::Changelog { command } => match command {
            ChangelogCommand::Check { base, changed_files, pr_body_file, self_test, root } => {
                // Three-outcome contract (see xtask/src/tasks/changelog.rs docs):
                //   Ok(PolicySatisfied | AdvisoryFinding) => exit 0.
                //   Ok(BlockingViolation)                 => exit 1 (only reachable
                //     once policy/changelog.toml's `blocking_enforced_from` is set
                //     and reached).
                //   Err(instrument/config failure)         => exit 2, distinct from
                //     both — never a silent pass, never a policy verdict.
                match tasks::changelog::check(base, changed_files, pr_body_file, self_test, root) {
                    Ok(
                        tasks::changelog::CheckOutcome::PolicySatisfied
                        | tasks::changelog::CheckOutcome::AdvisoryFinding,
                    ) => Ok(()),
                    Ok(tasks::changelog::CheckOutcome::BlockingViolation) => {
                        eprintln!("changelog check: blocking policy violation");
                        std::process::exit(1);
                    }
                    Err(e) => {
                        eprintln!("changelog check: instrument failure: {e}");
                        std::process::exit(2);
                    }
                }
            }
        },
        Commands::Workflows { command } => match command {
            WorkflowsCommand::Check { base, self_test, receipt, root } => {
                // Same three-outcome contract as `Commands::Changelog` above
                // (see xtask/src/tasks/workflows.rs docs):
                //   Ok(PolicySatisfied | AdvisoryFinding) => exit 0.
                //   Ok(BlockingViolation)                 => exit 1 (unreachable
                //     until policy/workflow-contracts.toml's clocks are armed).
                //   Err(instrument/config failure)         => exit 2.
                match tasks::workflows::check(base, self_test, receipt, root) {
                    Ok(
                        tasks::workflows::CheckOutcome::PolicySatisfied
                        | tasks::workflows::CheckOutcome::AdvisoryFinding,
                    ) => Ok(()),
                    Ok(tasks::workflows::CheckOutcome::BlockingViolation) => {
                        eprintln!("workflows check: blocking policy violation");
                        std::process::exit(1);
                    }
                    Err(e) => {
                        eprintln!("workflows check: instrument failure: {e}");
                        std::process::exit(2);
                    }
                }
            }
        },
        Commands::GateReceipts { command } => match command {
            GateReceiptsCommand::List { format } => {
                gate_receipts::list(convert_gate_receipts_format(format))
                    .map_err(|error| eyre!(error.to_string()))
            }
            GateReceiptsCommand::Validate { path, format } => {
                gate_receipts::validate(&path, convert_gate_receipts_format(format))
                    .map_err(|error| eyre!(error.to_string()))
            }
            GateReceiptsCommand::ValidateAll { dir, format } => {
                gate_receipts::validate_all(&dir, convert_gate_receipts_format(format))
                    .map_err(|error| eyre!(error.to_string()))
            }
        },
        Commands::MethodologyGate { fixture, pr, receipt, dry_run, enforce, format } => {
            methodology_gate::run(methodology_gate::MethodologyGateConfig {
                fixture,
                pr,
                receipt,
                dry_run,
                enforce,
                format,
            })
        }
        Commands::IssuePlan { command } => match command {
            IssuePlanSubcommand::Audit { fixture, repo, labels, receipt, dry_run, format } => {
                issue_plan::audit(issue_plan::AuditConfig {
                    fixture,
                    repo,
                    labels,
                    receipt,
                    dry_run,
                    format,
                })
            }
        },
        Commands::IssueControllers { command } => issue_controllers::run(command),
        Commands::WriterAdmission {
            branch,
            base,
            worktree,
            expected_base_sha,
            repo,
            fixture,
            json,
            floor_gb,
            floor_pct,
            large_staged_threshold,
        } => writer_admission::run(writer_admission::AdmissionConfig {
            branch,
            base,
            worktree,
            expected_base_sha,
            repo,
            fixture,
            json,
            floor_gb,
            floor_pct,
            large_staged_threshold,
        }),
        Commands::TargetedChecks { base, mode } => targeted_checks::run(base, mode),
        Commands::ResolvePackageName { crate_dir } => {
            // Use the current working directory as workspace root so this subcommand
            // works correctly both in the main workspace and in test synthetic workspaces.
            let root = std::env::current_dir()
                .map_err(|e| eyre!("Failed to get current working directory: {e}"))?;
            let name = tasks::targeted_checks::resolve_single_package_name(&root, &crate_dir)?;
            println!("{name}");
            Ok(())
        }
        Commands::CheckNamingConsistency { root } => {
            tasks::check_naming_consistency::run_default(root)
        }
        Commands::WorktreeCleanup { root, force } => worktrees::cleanup(root, force),
        Commands::WorktreeRecovery { command } => match command {
            WorktreeRecoveryCommand::Plan { repository, candidate, json } => {
                let plan = xtask::worktree_forensic_recovery::inspect(&repository, &candidate)?;
                let format = if json {
                    xtask::worktree_forensic_recovery::OutputFormat::Json
                } else {
                    xtask::worktree_forensic_recovery::OutputFormat::Human
                };
                print!("{}", xtask::worktree_forensic_recovery::render(&plan, format)?);
                let code = xtask::worktree_forensic_recovery::exit_code(&plan);
                if code != 0 {
                    std::process::exit(code);
                }
                Ok(())
            }
        },
        Commands::ValidateSwarmAgentRoster { root } => swarm_agent_roster::run(root),
        Commands::CheckAgentCapabilities { root } => agent_capability_policy::run(root),
        Commands::SwarmSummary { ops_dir, since, limit, format } => {
            swarm_summary::run(swarm_summary::SwarmSummaryConfig { ops_dir, since, limit, format })
        }
        Commands::PopulateBook => populate_book::run(),
        Commands::ValidateWorkspaceExclusions => validate_workspace_exclusions::run(),
        Commands::BuildTimingReceipt { clean, incremental, tests, output, baseline } => {
            build_timing::run_receipt(clean, incremental, tests, output, baseline)
        }
        Commands::CompareBuildTiming { baseline, current } => {
            build_timing::run_compare(baseline, current)
        }
        Commands::GeneratedFiles { command } => match command {
            GeneratedFilesCommand::List { fixture } => generated_files::list(fixture),
            GeneratedFilesCommand::Check {
                receipt,
                fixture,
                generator_receipt,
                allow_manual_edits,
            } => generated_files::check(receipt, fixture, generator_receipt, allow_manual_edits),
        },
        Commands::NoPanic { command } => match command {
            NoPanicCommand::Debt { command } => match command {
                NoPanicDebtCommand::Inventory { root, json, markdown } => {
                    let root = match root {
                        Some(path) => path,
                        None => utils::project_root()?,
                    };
                    let summary = xtask::no_panic_debt::run_inventory(&root, json, markdown)?;
                    println!("{summary}");
                    Ok(())
                }
                NoPanicDebtCommand::Check {
                    root,
                    artifact,
                    baseline,
                    clippy_observation,
                    owner_state,
                } => {
                    let root = match root {
                        Some(path) => path,
                        None => utils::project_root()?,
                    };
                    let result = xtask::no_panic_debt::run_check(
                        &root,
                        artifact,
                        baseline,
                        clippy_observation,
                        owner_state,
                    )?;
                    println!("{}", xtask::no_panic_debt::format_check_result(&result));
                    if result.ok {
                        Ok(())
                    } else {
                        Err(eyre!(
                            "test_panic_family_debt.v1 check failed with {} finding(s)",
                            result.findings.len()
                        ))
                    }
                }
                NoPanicDebtCommand::Report { root, json } => {
                    let root = match root {
                        Some(path) => path,
                        None => utils::project_root()?,
                    };
                    print!("{}", xtask::no_panic_debt::run_report(&root, json)?);
                    Ok(())
                }
            },
        },
        Commands::NonRust { command } => match command {
            NonRustCommand::ExactTree {
                base_sha,
                subject_sha,
                pr_head_sha,
                receipt,
                event_name,
                repository,
            } => {
                let root = utils::project_root()?;
                tasks::file_policy::non_rust_exact_tree(
                    &root,
                    &base_sha,
                    &subject_sha,
                    pr_head_sha.as_deref(),
                    &receipt,
                    event_name.as_deref(),
                    repository.as_deref(),
                )
            }
            NonRustCommand::Inventory { check } => {
                let root = utils::project_root()?;
                if check {
                    tasks::file_policy::non_rust_inventory_check(&root)
                } else {
                    tasks::file_policy::non_rust_inventory(&root)
                }
            }
            NonRustCommand::Check { mode, json, allowlist, root: root_override } => {
                use tasks::file_policy::{CheckFilePolicyConfig, CheckFilePolicyMode};
                let root = utils::project_root()?;
                let mode = match mode {
                    CheckFilePolicyCliMode::Advisory => CheckFilePolicyMode::Advisory,
                    CheckFilePolicyCliMode::BlockingAllowlist => {
                        CheckFilePolicyMode::BlockingAllowlist
                    }
                    CheckFilePolicyCliMode::BlockingStrict => CheckFilePolicyMode::BlockingStrict,
                };
                tasks::file_policy::check_file_policy(
                    &root,
                    CheckFilePolicyConfig {
                        mode,
                        json_output: json,
                        allowlist_path: allowlist,
                        root_override,
                    },
                )
            }
            NonRustCommand::Propose { output_dir, group_by, root: root_override } => {
                use tasks::file_policy::{ProposeConfig, ProposeGroupBy};
                let root = utils::project_root()?;
                let group_by = match group_by {
                    ProposeGroupByArg::Directory => ProposeGroupBy::Directory,
                    ProposeGroupByArg::Extension => ProposeGroupBy::Extension,
                };
                tasks::file_policy::non_rust_propose(
                    &root,
                    ProposeConfig { output_dir, group_by, root_override },
                )
            }
            NonRustCommand::ValidatePolicy { allowlist, debt } => {
                use tasks::file_policy::ValidateNonRustPolicyConfig;
                tasks::file_policy::validate_non_rust_policy(ValidateNonRustPolicyConfig {
                    allowlist_path: allowlist,
                    debt_path: debt,
                })
            }
            NonRustCommand::MigrationCandidates { format, output, limit, root: root_override } => {
                use tasks::file_policy::{MigrationCandidateFormat, MigrationCandidatesConfig};
                let root = utils::project_root()?;
                let format = match format {
                    MigrationCandidateFormatArg::Markdown => MigrationCandidateFormat::Markdown,
                    MigrationCandidateFormatArg::Json => MigrationCandidateFormat::Json,
                };
                tasks::file_policy::non_rust_migration_candidates(
                    &root,
                    MigrationCandidatesConfig { format, output, limit, root_override },
                )
            }
        },
        Commands::Policy { command } => match command {
            PolicyCommand::Cadence { as_of, json, markdown } => {
                let root = utils::project_root()?;
                tasks::policy_cadence::run(
                    &root,
                    tasks::policy_cadence::CadenceArgs { as_of, json, markdown },
                )
            }
            PolicyCommand::Transition { base, json, markdown } => {
                let root = utils::project_root()?;
                tasks::policy_cadence::transition::run(
                    &root,
                    tasks::policy_cadence::transition::TransitionArgs { base, json, markdown },
                )
            }
        },
        Commands::CheckFilePolicy { mode, json, allowlist, root: root_override } => {
            use tasks::file_policy::{CheckFilePolicyConfig, CheckFilePolicyMode};
            let root = utils::project_root()?;
            let mode = match mode {
                CheckFilePolicyCliMode::Advisory => CheckFilePolicyMode::Advisory,
                CheckFilePolicyCliMode::BlockingAllowlist => CheckFilePolicyMode::BlockingAllowlist,
                CheckFilePolicyCliMode::BlockingStrict => CheckFilePolicyMode::BlockingStrict,
            };
            tasks::file_policy::check_file_policy(
                &root,
                CheckFilePolicyConfig {
                    mode,
                    json_output: json,
                    allowlist_path: allowlist,
                    root_override,
                },
            )
        }
        Commands::CheckGenerated { mode, json } => {
            let root = utils::project_root()?;
            tasks::generated_policy::run(&root, mode, json)
        }
        Commands::FreshnessCheck {
            base,
            mode,
            json,
            no_fetch,
            allow_historical,
            reason,
            binaries,
        } => {
            use tasks::freshness_check::{FreshnessCheckConfig, FreshnessMode};
            let mode = match mode {
                FreshnessCheckMode::Warn => FreshnessMode::Warn,
                FreshnessCheckMode::Block => FreshnessMode::Block,
            };
            tasks::freshness_check::run(FreshnessCheckConfig {
                base,
                mode,
                json_output: json,
                no_fetch,
                allow_historical,
                reason,
                check_binaries: binaries,
            })
        }
        Commands::GenerateSemanticSnapshot { fixture_dir, output, check } => {
            tasks::generate_semantic_snapshot::run(
                tasks::generate_semantic_snapshot::GenerateSemanticSnapshotArgs {
                    fixture_dir,
                    output,
                    check,
                },
            )
        }
    }
}

fn print_top_level_commands() {
    let mut command_names = Cli::command()
        .get_subcommands()
        .map(|subcommand| subcommand.get_name().to_string())
        .collect::<Vec<_>>();
    command_names.sort_unstable();

    for command_name in command_names {
        println!("{command_name}");
    }
}

fn parse_toolchain_map(values: &[String]) -> Result<BTreeMap<String, String>> {
    let mut toolchains = BTreeMap::new();
    for value in values {
        let Some((name, version)) = value.split_once('=') else {
            bail!("toolchain must be name=version, got {value:?}");
        };
        if name.is_empty() || version.is_empty() {
            bail!("toolchain must be name=version, got {value:?}");
        }
        if toolchains.insert(name.to_string(), version.to_string()).is_some() {
            bail!("duplicate toolchain {name}");
        }
    }
    if toolchains.is_empty() {
        bail!("at least one --toolchain name=version is required");
    }
    Ok(toolchains)
}

fn parse_optional_rfc3339(value: Option<String>) -> Result<Option<chrono::DateTime<chrono::Utc>>> {
    match value {
        None => Ok(None),
        Some(text) => {
            let parsed = chrono::DateTime::parse_from_rfc3339(&text)
                .map_err(|error| eyre!("--now must be RFC3339: {error}"))?;
            Ok(Some(parsed.with_timezone(&chrono::Utc)))
        }
    }
}

/// Validates a `--profile` value before it flows into
/// `parser_corpus_sweep::receipt_path_for_profile`, which interpolates the
/// value verbatim into `target/receipts/<profile>-corpus-sweep.json`
/// (xtask/src/tasks/parser_corpus_sweep.rs). Without this guard, a value
/// containing `..` or a path separator (`--profile "../foo"`,
/// `--profile "foo/bar"`) would let the receipt escape `target/receipts/`
/// or create an unexpected subdirectory (#3929 review finding). Real
/// profile names are short slugs (`"system"`, `"cpan"`, `"cpan-common"` —
/// see `default_corpus_profile` and the existing receipt-path tests below),
/// so an ASCII alphanumeric/`-`/`_` allowlist covers every legitimate case
/// with no breaking change.
fn profile_slug_parser(value: &str) -> Result<String, String> {
    let is_valid_slug = !value.is_empty()
        && value.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if is_valid_slug {
        Ok(value.to_string())
    } else {
        Err(format!(
            "invalid --profile value {value:?}: must be a non-empty slug of ASCII letters, \
             digits, `-`, or `_` (no `/`, `\\`, `..`, or other path characters)"
        ))
    }
}

/// Build the `SweepConfig` for the `parser-corpus-sweep` command, resolving
/// `roots` to concrete corpus directories and threading `profile` through to
/// `SweepConfig.corpus_profile` (used for report/receipt naming).
//
// Each parameter mirrors a ParserCorpusSweep CLI field one-to-one, so
// reshaping into a struct would just re-create the same argument list at
// the call site without clarifying anything; the lint is suppressed rather
// than fixed for that reason (AGENTS.md code-quality bar).
#[allow(clippy::too_many_arguments)]
fn build_parser_corpus_sweep_config(
    roots: Option<Vec<PathBuf>>,
    manifest: Option<PathBuf>,
    output: Option<PathBuf>,
    baseline: Option<PathBuf>,
    enforce: bool,
    verbose: bool,
    receipt: bool,
    profile: Option<String>,
) -> parser_corpus_sweep::SweepConfig {
    let base_roots = roots.unwrap_or_else(parser_corpus_sweep::default_base_roots);
    let corpus_roots = parser_corpus_sweep::resolve_corpus_roots(&base_roots);
    parser_corpus_sweep::SweepConfig {
        corpus_profile: profile,
        base_roots,
        corpus_roots,
        manifest_path: manifest,
        manifest_perl5lib: Vec::new(),
        output_path: output,
        baseline_path: baseline,
        enforce,
        verbose,
        receipt,
    }
}

fn convert_gate_receipts_format(format: GateReceiptsFormat) -> gate_receipts::OutputFormat {
    match format {
        GateReceiptsFormat::Human => gate_receipts::OutputFormat::Human,
        GateReceiptsFormat::Json => gate_receipts::OutputFormat::Json,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type TestResult<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

    fn parse_completion_candidates(
        args: &[&str],
    ) -> TestResult<completion_candidates::CompletionCandidatesSubcommand> {
        match Cli::try_parse_from(args)?.command {
            Commands::CompletionCandidates { command } => Ok(command),
            _ => Err(std::io::Error::other("expected completion-candidates command").into()),
        }
    }

    /// The inventory's four verbs are its whole interface (#10949). A wrong
    /// clap name, a missing `#[command(subcommand)]`, or an argument declared
    /// as a flag instead of a positional all compile cleanly and break the CLI,
    /// so each shape is parsed here and each wrong shape is refused.
    #[test]
    fn completion_candidates_cli_shapes_parse() -> TestResult {
        use completion_candidates::CompletionCandidatesSubcommand as Sub;

        assert!(matches!(
            parse_completion_candidates(&["xtask", "completion-candidates", "check"])?,
            Sub::Check
        ));
        assert!(matches!(
            parse_completion_candidates(&["xtask", "completion-candidates", "list"])?,
            Sub::List
        ));

        let explain = parse_completion_candidates(&[
            "xtask",
            "completion-candidates",
            "explain",
            "some::producer::id",
        ])?;
        match explain {
            Sub::Explain { producer_id } => assert_eq!(producer_id, "some::producer::id"),
            other => {
                return Err(
                    std::io::Error::other(format!("expected explain, got {other:?}")).into()
                );
            }
        }

        assert!(matches!(
            parse_completion_candidates(&["xtask", "completion-candidates", "graph"])?,
            Sub::Graph { stdout: false }
        ));
        assert!(matches!(
            parse_completion_candidates(&["xtask", "completion-candidates", "graph", "--stdout"])?,
            Sub::Graph { stdout: true }
        ));

        // A verb is required, `explain` needs its producer id, and the
        // subcommand name is `completion-candidates` rather than the Rust
        // identifier — each is a regression clap would otherwise accept.
        assert!(Cli::try_parse_from(["xtask", "completion-candidates"]).is_err());
        assert!(Cli::try_parse_from(["xtask", "completion-candidates", "explain"]).is_err());
        assert!(Cli::try_parse_from(["xtask", "completion_candidates", "check"]).is_err());
        assert!(Cli::try_parse_from(["xtask", "completion-candidates", "chekc"]).is_err());
        Ok(())
    }

    #[test]
    fn candidate_security_contract_command_requires_and_preserves_path() -> TestResult {
        match Cli::try_parse_from([
            "xtask",
            "candidate-security-contract",
            "--contract",
            "candidate.json",
        ])?
        .command
        {
            Commands::CandidateSecurityContract { contract }
                if contract == PathBuf::from("candidate.json") => {}
            _ => return Err(std::io::Error::other("contract path was not preserved").into()),
        }
        if Cli::try_parse_from(["xtask", "candidate-security-contract"]).is_ok() {
            return Err(std::io::Error::other("contract path must be required").into());
        }
        Ok(())
    }

    fn parse_devex_command(args: &[&str]) -> TestResult<DevexCommand> {
        match Cli::try_parse_from(args)?.command {
            Commands::Devex { command } => Ok(command),
            _ => Err(std::io::Error::other("expected devex command").into()),
        }
    }

    #[test]
    fn devex_commands_default_to_auto_base() -> TestResult {
        let cases = [
            (["xtask", "devex", "plan"].as_slice(), "plan"),
            (["xtask", "devex", "receipt"].as_slice(), "receipt"),
            (["xtask", "devex", "cockpit"].as_slice(), "cockpit"),
            (["xtask", "devex", "pr-body"].as_slice(), "pr-body"),
        ];

        for (args, name) in cases {
            let base = match parse_devex_command(args)? {
                DevexCommand::Plan { base }
                | DevexCommand::Receipt { base, .. }
                | DevexCommand::Cockpit { base, .. }
                | DevexCommand::PrBody { base, .. } => base,
            };
            assert_eq!(base, "auto", "{name} should auto-detect the diff base by default");
        }

        Ok(())
    }

    #[test]
    fn devex_plan_respects_explicit_base() -> TestResult {
        match parse_devex_command(&["xtask", "devex", "plan", "--base", "HEAD~1"])? {
            DevexCommand::Plan { base } => assert_eq!(base, "HEAD~1"),
            _ => return Err(std::io::Error::other("expected devex plan command").into()),
        }

        Ok(())
    }

    #[test]
    fn parser_corpus_sweep_accepts_profile_flag() -> TestResult {
        match Cli::try_parse_from(["xtask", "parser-corpus-sweep", "--profile", "cpan"])?.command {
            Commands::ParserCorpusSweep { profile, .. } => {
                assert_eq!(profile.as_deref(), Some("cpan"));
            }
            _ => return Err(std::io::Error::other("expected parser-corpus-sweep command").into()),
        }

        Ok(())
    }

    #[test]
    fn parser_corpus_sweep_profile_defaults_to_none() -> TestResult {
        match Cli::try_parse_from(["xtask", "parser-corpus-sweep"])?.command {
            Commands::ParserCorpusSweep { profile, .. } => {
                assert_eq!(profile, None);
            }
            _ => return Err(std::io::Error::other("expected parser-corpus-sweep command").into()),
        }

        Ok(())
    }

    #[test]
    fn parser_corpus_sweep_profile_rejects_path_traversal_and_separators() -> TestResult {
        // #3929 review finding: --profile flows verbatim into
        // target/receipts/<profile>-corpus-sweep.json, so a value containing
        // `..` or a path separator must be rejected before it ever reaches
        // that interpolation, not just documented as trusted input.
        for bad_profile in ["../foo", "foo/bar", "foo\\bar", "..", ""] {
            let result =
                Cli::try_parse_from(["xtask", "parser-corpus-sweep", "--profile", bad_profile]);
            assert!(
                result.is_err(),
                "--profile {bad_profile:?} must be rejected by profile_slug_parser"
            );
        }

        Ok(())
    }

    #[test]
    fn parser_corpus_sweep_profile_accepts_known_slugs() -> TestResult {
        // Real callers use short slugs (see default_corpus_profile and
        // receipt_path_for_profile's own tests in parser_corpus_sweep.rs);
        // confirm the allowlist doesn't regress any of them.
        for good_profile in ["system", "cpan", "cpan-common", "profile_1"] {
            match Cli::try_parse_from(["xtask", "parser-corpus-sweep", "--profile", good_profile])?
                .command
            {
                Commands::ParserCorpusSweep { profile, .. } => {
                    assert_eq!(profile.as_deref(), Some(good_profile));
                }
                _ => {
                    return Err(
                        std::io::Error::other("expected parser-corpus-sweep command").into()
                    );
                }
            }
        }

        Ok(())
    }

    #[test]
    fn parser_corpus_sweep_threads_profile_into_sweep_config() -> TestResult {
        let config = build_parser_corpus_sweep_config(
            Some(Vec::new()),
            None,
            None,
            None,
            false,
            false,
            false,
            Some("cpan".to_string()),
        );

        assert_eq!(
            config.corpus_profile.as_deref(),
            Some("cpan"),
            "--profile should flow through to SweepConfig.corpus_profile"
        );

        Ok(())
    }

    #[test]
    fn perl_core_harness_dispatch_fails_closed_for_future_subcommands() -> TestResult {
        let cases = [
            (
                PerlCoreHarnessCommand::Run {
                    mode: perl_core_harness::HarnessMode::Execute,
                    perl_tree: PathBuf::from("unused"),
                    host_perl: None,
                    runner: perl_core_harness::HarnessRunner::Test,
                    profile: perl_core_harness::HarnessProfile::Base,
                    tests: Vec::new(),
                    output: None,
                    runner_binary: None,
                    no_diagnostic_probes: false,
                },
                "requires one or more explicit --test",
            ),
            (PerlCoreHarnessCommand::Report, "report is not implemented"),
        ];

        for (command, expected) in cases {
            let err = run_cli(Cli { command: Commands::PerlCoreHarness { command } })
                .err()
                .ok_or_else(|| std::io::Error::other("perl-core-harness command should fail"))?;

            assert!(err.to_string().contains(expected), "expected {expected:?}, got {err:?}");
        }

        Ok(())
    }

    #[test]
    fn perl_core_harness_dispatch_reports_missing_discovery_tree() -> TestResult {
        let temp = tempfile::tempdir()?;
        let missing_tree = temp.path().join("missing-perl-tree");

        let err = run_cli(Cli {
            command: Commands::PerlCoreHarness {
                command: PerlCoreHarnessCommand::Discover {
                    perl_tree: missing_tree,
                    host_perl: None,
                    runner: perl_core_harness::HarnessRunner::Test,
                    profile: perl_core_harness::HarnessProfile::Base,
                    output: None,
                },
            },
        })
        .err()
        .ok_or_else(|| std::io::Error::other("discover should fail for a missing tree"))?;

        assert!(
            err.to_string().contains("prepared Perl tree does not exist or is not a directory"),
            "missing-tree error should be explicit, got {err:?}"
        );

        Ok(())
    }
}

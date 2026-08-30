//! Maintained ledger rows for the initialize-operation denominator (#10040).
//!
//! Every row cites a current-`main` file and entry function. Stale citations,
//! under- or over-declared blocking exposure, unowned reachable work, and phase
//! claims that contradict where the operation actually runs are all caught by
//! [`super::ledger_errors`] against the derived census.
//!
//! Declared exposure is not transcribed from documentation. `find_perl_interpreter`
//! carries a doc comment claiming a subprocess is spawned per cache entry, but no
//! `Command` appears anywhere on that path; the rows below record what the source
//! does, and the checker refuses any row that drifts from it in either direction.

use super::census::Exposure;
use super::{ExecutionPoint, InitOperationRow, MigrationWave, PhaseDisposition, Trigger};

// ---------------------------------------------------------------------------
// Cited source files (current main)
// ---------------------------------------------------------------------------

const F_CAPABILITIES: &str = "crates/perl-lsp-rs/src/runtime/lifecycle/capabilities.rs";
const F_LIFECYCLE_DISPATCH: &str = "crates/perl-lsp-rs/src/runtime/dispatch/lifecycle.rs";
const F_PREFLIGHT: &str = "crates/perl-lsp-rs/src/runtime/dispatch/preflight.rs";
const F_LIFECYCLE_WORKSPACE: &str = "crates/perl-lsp-rs/src/runtime/lifecycle/workspace.rs";
const F_TOOLS: &str = "crates/perl-lsp-rs/src/runtime/lifecycle/tools.rs";
const F_RUNTIME: &str = "crates/perl-lsp-rs/src/runtime/mod.rs";
const F_RUNTIME_WORKSPACE: &str = "crates/perl-lsp-rs/src/runtime/workspace.rs";
const F_TIMING: &str = "crates/perl-lsp-rs/src/runtime/timing.rs";
const F_CORE_CAPABILITIES: &str = "crates/perl-lsp-rs-core/src/protocol/capabilities.rs";

// ---------------------------------------------------------------------------
// Authorities joined rather than duplicated
// ---------------------------------------------------------------------------

/// #9662 final-surface authority.
const OWNER_SURFACE: &str = "#9662 EffectiveLspSurface";
/// #6736 configuration precedence authority.
const OWNER_CONFIG: &str = "#6736 configuration authority";
/// #7419/#7420 environment authority.
const OWNER_ENVIRONMENT: &str = "#7419/#7420 ProjectEnvironmentSnapshot";
/// #7209 external-tool role authority.
const OWNER_TOOL_ROLES: &str = "#7209 external-tool roles";
/// #8509 `.perltidyrc` parse/mapping authority.
const OWNER_PERLTIDY_CONFIG: &str = "#8509 perltidy configuration mapping";
/// #10024 application task owner.
const OWNER_TASKS: &str = "#10024 application task owner";
/// #7708 initialize ordering authority.
const OWNER_ORDERING: &str = "#7708 initialize ordering";

/// The maintained ledger.
pub fn ledger_rows() -> Vec<InitOperationRow> {
    vec![
        // -------------------------------------------------------------------
        // Entry points. These are umbrella rows: their closure spans the whole
        // initialize path, so they never account for coverage. They still
        // account for exposure written directly in their own bodies.
        // -------------------------------------------------------------------
        InitOperationRow {
            operation_id: "init.request.capability_normalization",
            file: F_CAPABILITIES,
            function: "handle_initialize",
            proposition: "the client's declared capabilities, roots and initialization options are \
                          normalized once, and the static InitializeResult is committed exactly \
                          once per session",
            side_effects: &[
                "writes client_capabilities",
                "writes workspace_folders and root_path",
                "queues pending_startup_log for JetBrains-family clients",
                "commits the InitializeResult",
            ],
            declared_exposure: &[Exposure::Filesystem, Exposure::PathLookup, Exposure::EnvRead],
            triggers: &[Trigger::Initialize],
            exactly_once: true,
            current_point: ExecutionPoint::BeforeResponse,
            phase: PhaseDisposition::ProtocolRequiredBeforeResponse,
            migration_wave: MigrationWave::None,
            affects_static_initialize_result: false,
            static_surface_join: "",
            affects_dynamic_registration_plan: true,
            affects_negotiation: true,
            affects_initial_native_semantics: false,
            current_owner: OWNER_SURFACE,
            target_owner: OWNER_SURFACE,
            proof_family: "initialize_capability_normalization",
            memoization: "guarded by initialize_requested compare_exchange",
            call_site_argument: "",
            owns_exposure: false,
        },
        InitOperationRow {
            operation_id: "init.deferred.complete_initialization",
            file: F_LIFECYCLE_DISPATCH,
            function: "complete_initialization",
            proposition: "post-response bootstrap runs exactly once, whether reached from the \
                          initialized notification or the compatibility trigger",
            side_effects: &[
                "emits queued window/logMessage",
                "sends client/registerCapability for watchers and inline completion",
                "starts workspace indexing",
                "sends perl-lsp/index-ready",
                "emits window/logMessage while the index is still building",
            ],
            declared_exposure: &[Exposure::Filesystem, Exposure::ProcessSpawn, Exposure::EnvRead],
            triggers: &[Trigger::Initialized, Trigger::AutoInitializeCompat],
            exactly_once: true,
            current_point: ExecutionPoint::AfterResponse,
            phase: PhaseDisposition::DeferToPostInitializeEnvironment,
            migration_wave: MigrationWave::None,
            affects_static_initialize_result: false,
            static_surface_join: "",
            affects_dynamic_registration_plan: true,
            affects_negotiation: false,
            affects_initial_native_semantics: false,
            current_owner: OWNER_ORDERING,
            target_owner: OWNER_ORDERING,
            proof_family: "initialize_deferred_bootstrap",
            memoization: "guarded by an initialized compare_exchange",
            call_site_argument: "",
            owns_exposure: false,
        },
        InitOperationRow {
            operation_id: "init.compat.auto_initialize_dispatch",
            file: F_LIFECYCLE_DISPATCH,
            function: "auto_initialize_for_compat",
            proposition: "a client that never sends initialized still reaches the deferred \
                          bootstrap exactly once",
            side_effects: &["delegates to complete_initialization"],
            declared_exposure: &[Exposure::Filesystem, Exposure::ProcessSpawn, Exposure::EnvRead],
            triggers: &[Trigger::AutoInitializeCompat],
            exactly_once: true,
            current_point: ExecutionPoint::AfterResponse,
            phase: PhaseDisposition::DeferToPostInitializeEnvironment,
            migration_wave: MigrationWave::None,
            affects_static_initialize_result: false,
            static_surface_join: "",
            affects_dynamic_registration_plan: true,
            affects_negotiation: false,
            affects_initial_native_semantics: false,
            current_owner: OWNER_ORDERING,
            target_owner: OWNER_ORDERING,
            proof_family: "initialize_compat_trigger",
            memoization: "shares complete_initialization's guard",
            call_site_argument: "",
            owns_exposure: false,
        },
        InitOperationRow {
            operation_id: "init.compat.auto_initialize_preflight",
            file: F_PREFLIGHT,
            function: "auto_initialize_for_compat",
            proposition: "the first non-lifecycle request after initialize triggers compatibility \
                          bootstrap and the matching configuration pull",
            side_effects: &["requests workspace configuration for folders"],
            declared_exposure: &[Exposure::Filesystem, Exposure::ProcessSpawn, Exposure::EnvRead],
            triggers: &[Trigger::AutoInitializeCompat],
            exactly_once: false,
            current_point: ExecutionPoint::AfterResponse,
            phase: PhaseDisposition::DeferToPostInitializeEnvironment,
            migration_wave: MigrationWave::None,
            affects_static_initialize_result: false,
            static_surface_join: "",
            affects_dynamic_registration_plan: false,
            affects_negotiation: false,
            affects_initial_native_semantics: false,
            current_owner: OWNER_ORDERING,
            target_owner: OWNER_ORDERING,
            proof_family: "initialize_compat_trigger",
            memoization: "",
            call_site_argument: "",
            owns_exposure: false,
        },
        // -------------------------------------------------------------------
        // Operations. These account for coverage and must be pairwise
        // non-nested.
        // -------------------------------------------------------------------
        InitOperationRow {
            operation_id: "init.response.static_capability_construction",
            file: F_CORE_CAPABILITIES,
            function: "capabilities_json",
            proposition: "the static server capability surface is constructed from build flags \
                          alone, with no ambient process, tool or interpreter state as input",
            side_effects: &[],
            declared_exposure: &[],
            triggers: &[Trigger::Initialize],
            exactly_once: true,
            current_point: ExecutionPoint::BeforeResponse,
            phase: PhaseDisposition::ProtocolRequiredBeforeResponse,
            migration_wave: MigrationWave::None,
            // The one row that genuinely shapes the static InitializeResult. It
            // exercises the ambient-state rule positively: the join is admitted
            // only because this operation reaches no PATH, process or network
            // work, which is exactly #9662's purity requirement.
            affects_static_initialize_result: true,
            static_surface_join: "#9662 SurfaceInputs (BuildFlags census profile)",
            affects_dynamic_registration_plan: false,
            affects_negotiation: true,
            affects_initial_native_semantics: false,
            current_owner: OWNER_SURFACE,
            target_owner: OWNER_SURFACE,
            proof_family: "initialize_static_surface_is_pure",
            memoization: "",
            call_site_argument: "",
            owns_exposure: false,
        },
        InitOperationRow {
            operation_id: "init.config.perltidyrc_profile_import",
            file: F_LIFECYCLE_WORKSPACE,
            function: "set_root_uri",
            proposition: "a workspace-root .perltidyrc contributes its admitted native options as \
                          a base configuration layer beneath .perl-lsp.toml",
            side_effects: &[
                "writes root_path",
                "writes discovered_perltidy_profile",
                "applies native perltidy options to config",
            ],
            declared_exposure: &[Exposure::Filesystem, Exposure::EnvRead],
            triggers: &[Trigger::Initialize],
            exactly_once: false,
            current_point: ExecutionPoint::BeforeResponse,
            // This shapes initial native formatter semantics, so it belongs
            // before the response. It reads a file but never selects an
            // executable, which is exactly why it must stay distinct from
            // perltidy executable discovery.
            phase: PhaseDisposition::LocalProcessFreeConfigBeforeResponse,
            migration_wave: MigrationWave::None,
            affects_static_initialize_result: false,
            static_surface_join: "",
            affects_dynamic_registration_plan: false,
            affects_negotiation: false,
            affects_initial_native_semantics: true,
            current_owner: OWNER_PERLTIDY_CONFIG,
            target_owner: OWNER_PERLTIDY_CONFIG,
            proof_family: "initialize_perltidyrc_native_import",
            memoization: "",
            call_site_argument: "",
            owns_exposure: true,
        },
        InitOperationRow {
            operation_id: "init.config.project_config_load",
            file: F_LIFECYCLE_WORKSPACE,
            function: "load_and_apply_project_config",
            proposition: "each workspace folder's .perl-lsp.toml is layered over initialization \
                          options, with first-folder-wins for server-global sections",
            side_effects: &[
                "writes project_config and effective_workspace_config",
                "emits window/showMessage Warning on parse failure",
                "emits a multi-root conflict notice",
            ],
            declared_exposure: &[Exposure::Filesystem],
            triggers: &[Trigger::Initialize, Trigger::Reconfiguration],
            exactly_once: false,
            current_point: ExecutionPoint::BeforeResponse,
            phase: PhaseDisposition::LocalProcessFreeConfigBeforeResponse,
            migration_wave: MigrationWave::None,
            affects_static_initialize_result: false,
            static_surface_join: "",
            affects_dynamic_registration_plan: false,
            affects_negotiation: false,
            affects_initial_native_semantics: true,
            current_owner: OWNER_CONFIG,
            target_owner: OWNER_CONFIG,
            proof_family: "initialize_project_config_precedence",
            memoization: "",
            call_site_argument: "",
            owns_exposure: true,
        },
        InitOperationRow {
            operation_id: "init.environment.perl_interpreter_discovery",
            file: F_LIFECYCLE_WORKSPACE,
            function: "check_perl_interpreter",
            proposition: "an interpreter is located from configuration, toolchain managers, PATH \
                          or OS fallbacks, and its absence is reported once per session",
            side_effects: &[
                "emits window/logMessage Info on fallback discovery",
                "emits window/showMessage Error once per process when absent",
            ],
            declared_exposure: &[Exposure::Filesystem, Exposure::PathLookup, Exposure::EnvRead],
            triggers: &[Trigger::Initialize],
            exactly_once: false,
            current_point: ExecutionPoint::BeforeResponse,
            // Environment evidence, not negotiation authority. It runs before
            // the response today purely by position, which is what E02 moves.
            phase: PhaseDisposition::DeferToPostInitializeEnvironment,
            migration_wave: MigrationWave::E02,
            affects_static_initialize_result: false,
            static_surface_join: "",
            affects_dynamic_registration_plan: false,
            affects_negotiation: false,
            affects_initial_native_semantics: false,
            current_owner: OWNER_ENVIRONMENT,
            target_owner: OWNER_ENVIRONMENT,
            proof_family: "initialize_interpreter_discovery",
            memoization: "calls the uncached find_perl_interpreter even though a cached variant \
                          exists; the not-found warning is gated by a module-static Once, so it \
                          fires once per process, not once per session as its doc comment says",
            call_site_argument: "",
            owns_exposure: true,
        },
        // The two `detect_tool` call sites at capabilities.rs:746-747 share one
        // mechanism but carry different target dispositions, so they are two
        // rows. #10040's role rulings separate them explicitly: perlcritic
        // executable discovery leaves the product lifecycle, while perltidy
        // discovery survives as post-init/lazy availability for a #7209
        // authorized explicit adapter. Collapsing them would invite E02 to
        // delete the perltidy probe outright instead of deferring it.
        InitOperationRow {
            operation_id: "init.tools.perltidy_executable_detection",
            file: F_TOOLS,
            function: "detect_tool",
            proposition: "perltidy's presence on PATH is observed as advisory availability for an \
                          explicitly authorized external adapter, never as automatic selection",
            side_effects: &[],
            declared_exposure: &[Exposure::PathLookup],
            triggers: &[Trigger::Initialize],
            exactly_once: false,
            current_point: ExecutionPoint::BeforeResponse,
            // `runtime_flags(self, _has_perltidy: bool)` ignores its argument,
            // so this result gates nothing that is advertised today. It remains
            // advisory availability rather than product lifecycle work.
            phase: PhaseDisposition::LazyOnFirstUse,
            migration_wave: MigrationWave::E02,
            affects_static_initialize_result: false,
            static_surface_join: "",
            affects_dynamic_registration_plan: false,
            affects_negotiation: false,
            affects_initial_native_semantics: false,
            current_owner: OWNER_TOOL_ROLES,
            target_owner: OWNER_TOOL_ROLES,
            proof_family: "initialize_tool_detection_is_not_capability_authority",
            memoization: "result is discarded; recomputed on every initialize",
            call_site_argument: "perltidy",
            owns_exposure: true,
        },
        InitOperationRow {
            operation_id: "init.tools.perlcritic_executable_detection",
            file: F_TOOLS,
            function: "detect_tool",
            proposition: "perlcritic's presence on PATH is observed with no current semantic \
                          consumer beyond tracing at this call site",
            side_effects: &[],
            declared_exposure: &[Exposure::PathLookup],
            triggers: &[Trigger::Initialize],
            exactly_once: false,
            current_point: ExecutionPoint::BeforeResponse,
            // #7209 forbids Perl::Critic product-runtime selection outright, so
            // unlike perltidy this probe has no post-init role to defer to.
            phase: PhaseDisposition::RemoveFromProductLifecycle,
            migration_wave: MigrationWave::E02,
            affects_static_initialize_result: false,
            static_surface_join: "",
            affects_dynamic_registration_plan: false,
            affects_negotiation: false,
            affects_initial_native_semantics: false,
            current_owner: OWNER_TOOL_ROLES,
            target_owner: OWNER_TOOL_ROLES,
            proof_family: "initialize_tool_detection_is_not_capability_authority",
            memoization: "result is discarded; recomputed on every initialize",
            // Both tool rows cite the same shared `detect_tool`, so without a
            // distinguishing argument deleting this probe would leave the row
            // valid against the sibling's call site.
            call_site_argument: "perlcritic",
            // The sibling perltidy row already accounts for this shared
            // mechanism's closure; both owning it would be redundant.
            owns_exposure: false,
        },
        InitOperationRow {
            operation_id: "init.backend.ai_refresh",
            file: F_RUNTIME,
            function: "refresh_ai_backend",
            proposition: "an optional AI completion backend is constructed when the activation \
                          authority permits it and a key resolves",
            side_effects: &["writes ai_inline_backend"],
            declared_exposure: &[Exposure::EnvRead],
            triggers: &[Trigger::Initialize, Trigger::Reconfiguration],
            exactly_once: false,
            current_point: ExecutionPoint::BeforeResponse,
            phase: PhaseDisposition::LazyOnFirstUse,
            migration_wave: MigrationWave::E03,
            affects_static_initialize_result: false,
            static_surface_join: "",
            affects_dynamic_registration_plan: false,
            affects_negotiation: false,
            affects_initial_native_semantics: false,
            current_owner: "runtime provider readiness",
            target_owner: OWNER_TASKS,
            proof_family: "initialize_optional_backend_is_runtime_readiness",
            memoization: "",
            call_site_argument: "",
            owns_exposure: true,
        },
        InitOperationRow {
            operation_id: "init.workspace.indexing_start",
            file: F_RUNTIME_WORKSPACE,
            function: "start_workspace_indexing",
            proposition: "workspace-wide Perl file discovery and symbol indexing begin as \
                          background work after the response",
            side_effects: &["spawns background indexing workers", "publishes progress tokens"],
            // No env read: the chain that appeared to reach one ran through a
            // mis-resolved method edge, refused once method calls were required
            // to match call syntax and stay in the calling file.
            declared_exposure: &[Exposure::Filesystem, Exposure::ProcessSpawn],
            triggers: &[Trigger::Initialized, Trigger::AutoInitializeCompat],
            exactly_once: false,
            current_point: ExecutionPoint::AfterResponse,
            phase: PhaseDisposition::DeferToPostInitializeEnvironment,
            migration_wave: MigrationWave::None,
            affects_static_initialize_result: false,
            static_surface_join: "",
            affects_dynamic_registration_plan: false,
            affects_negotiation: false,
            affects_initial_native_semantics: false,
            current_owner: OWNER_TASKS,
            target_owner: OWNER_TASKS,
            proof_family: "initialize_workspace_indexing_is_background_work",
            memoization: "git discovery results are cached per root",
            call_site_argument: "",
            owns_exposure: true,
        },
        InitOperationRow {
            operation_id: "init.instrumentation.timing_mode",
            file: F_TIMING,
            function: "mode",
            proposition: "startup timing instrumentation resolves its mode from the environment \
                          the first time any timed span runs",
            side_effects: &[],
            declared_exposure: &[Exposure::EnvRead],
            // Not tied to a lifecycle message: the `OnceLock` is filled by
            // whichever timed span happens to run first, which may be an
            // initialize span or a later request entirely.
            triggers: &[Trigger::FirstUse],
            exactly_once: false,
            current_point: ExecutionPoint::OnDemand,
            phase: PhaseDisposition::ExistingExternalOwnerNoMove,
            migration_wave: MigrationWave::None,
            affects_static_initialize_result: false,
            static_surface_join: "",
            affects_dynamic_registration_plan: false,
            affects_negotiation: false,
            affects_initial_native_semantics: false,
            current_owner: "#8077 startup measurement",
            target_owner: "#8077 startup measurement",
            proof_family: "initialize_instrumentation_has_no_semantic_effect",
            memoization: "resolved once into a static",
            // Shared leaf utility, as above.
            call_site_argument: "",
            owns_exposure: false,
        },
        InitOperationRow {
            operation_id: "init.instrumentation.timing_file_sink",
            file: F_TIMING,
            function: "file_writer",
            // The sink is a separate operation from the mode selection above:
            // it touches the filesystem, and only on the `TimingMode::File`
            // branch. Folding it into the mode row would let one proposition
            // stand for two different exposures.
            proposition: "the file timing sink is created and opened the first time a span is \
                          written under `TimingMode::File`",
            side_effects: &[],
            declared_exposure: &[Exposure::Filesystem],
            triggers: &[Trigger::FirstUse],
            exactly_once: false,
            current_point: ExecutionPoint::OnDemand,
            phase: PhaseDisposition::ExistingExternalOwnerNoMove,
            migration_wave: MigrationWave::None,
            affects_static_initialize_result: false,
            static_surface_join: "",
            affects_dynamic_registration_plan: false,
            affects_negotiation: false,
            affects_initial_native_semantics: false,
            current_owner: "#8077 startup measurement",
            target_owner: "#8077 startup measurement",
            proof_family: "initialize_instrumentation_has_no_semantic_effect",
            memoization: "the handle is opened once into a static",
            call_site_argument: "",
            owns_exposure: false,
        },
    ]
}

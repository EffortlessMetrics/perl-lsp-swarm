//! Falsifiers for the initialize-operation phase and owner ledger (#10040).
//!
//! A ledger that only ever passes proves nothing. Each test here builds a
//! deliberately wrong ledger, or a synthetic codebase containing the mistake the
//! controlling issue names as a negative control, and asserts the checker
//! rejects it. The final tests confirm the real ledger passes against real
//! source and that generated output is deterministic.

use xtask::init_environment::census::{Census, Exposure};
use xtask::init_environment::{
    ExecutionPoint, InitOperationRow, MigrationWave, PhaseDisposition, Trigger, census,
    ledger_errors, ledger_errors_with_roots, ledger_rows, render_json,
};

// ---------------------------------------------------------------------------
// Synthetic fixtures
// ---------------------------------------------------------------------------

const SYNTHETIC_ROOT_FILE: &str = "crates/perl-lsp-rs/src/entry.rs";
const SYNTHETIC_HELPER_FILE: &str = "crates/perl-lsp-rs/src/helper.rs";

fn synthetic_roots() -> Vec<(&'static str, &'static str)> {
    vec![(SYNTHETIC_ROOT_FILE, "handle_initialize")]
}

/// A codebase whose process spawn sits three hops below the entry point, inside
/// a helper whose name says nothing about processes.
fn indirect_process_sources() -> Vec<(String, String)> {
    vec![
        (
            SYNTHETIC_ROOT_FILE.to_string(),
            r#"
            impl Server {
                pub fn handle_initialize(&self) {
                    self.normalize_capabilities();
                }
                fn normalize_capabilities(&self) {
                    self.resolve_profile();
                }
                fn resolve_profile(&self) {
                    read_profile_metadata();
                }
            }
            "#
            .to_string(),
        ),
        (
            SYNTHETIC_HELPER_FILE.to_string(),
            r#"
            pub fn read_profile_metadata() {
                let _ = std::process::Command::new("perl").arg("--version").output();
            }
            "#
            .to_string(),
        ),
    ]
}

fn baseline_row() -> InitOperationRow {
    InitOperationRow {
        operation_id: "synthetic.entry",
        file: SYNTHETIC_ROOT_FILE,
        function: "handle_initialize",
        proposition: "a synthetic entry point used to exercise the checker",
        side_effects: &[],
        declared_exposure: &[Exposure::ProcessSpawn],
        triggers: &[Trigger::Initialize],
        exactly_once: true,
        current_point: ExecutionPoint::BeforeResponse,
        phase: PhaseDisposition::ProtocolRequiredBeforeResponse,
        migration_wave: MigrationWave::None,
        affects_static_initialize_result: false,
        static_surface_join: "",
        affects_dynamic_registration_plan: false,
        affects_negotiation: false,
        affects_initial_native_semantics: false,
        current_owner: "synthetic owner",
        target_owner: "synthetic owner",
        proof_family: "synthetic",
        memoization: "",
        call_site_argument: "",
        owns_exposure: true,
    }
}

fn assert_reports(errors: &[String], needle: &str) {
    assert!(
        errors.iter().any(|error| error.contains(needle)),
        "expected a finding containing {needle:?}, got: {errors:#?}"
    );
}

// ---------------------------------------------------------------------------
// Census precision
// ---------------------------------------------------------------------------

#[test]
fn indirect_process_helper_is_attributed_through_three_hops() {
    let census = Census::from_sources(&indirect_process_sources());
    let root =
        census.resolve(SYNTHETIC_ROOT_FILE, "handle_initialize").expect("synthetic root resolves");

    let exposures = census.transitive_exposures(root, census::MAX_DEPTH);
    let witness = exposures
        .get(&Exposure::ProcessSpawn)
        .expect("a process spawn three hops down must still be attributed");

    assert!(
        witness.chain.len() >= 4,
        "witness should name every hop to the helper, got {:?}",
        witness.chain
    );
    assert!(
        witness.render().contains("read_profile_metadata"),
        "witness must name the helper that carries the exposure, got {}",
        witness.render()
    );
}

#[test]
fn a_checker_that_saw_only_direct_calls_would_miss_this() {
    // Guard the guard: the entry function must carry no process work of its own,
    // so the previous test can only pass by following the call graph.
    let census = Census::from_sources(&indirect_process_sources());
    let root =
        census.resolve(SYNTHETIC_ROOT_FILE, "handle_initialize").expect("synthetic root resolves");

    assert!(
        census.direct_exposures(root).is_empty(),
        "the synthetic entry point must not spawn anything directly"
    );
}

#[test]
fn same_name_functions_in_different_crates_are_not_conflated() {
    // `handle_initialize` names both the LSP handler and an unrelated DAP
    // handler. Resolving by bare name would attribute the DAP network work to
    // the LSP initialize path.
    let sources = vec![
        (
            SYNTHETIC_ROOT_FILE.to_string(),
            "impl Server { pub fn handle_initialize(&self) {} }".to_string(),
        ),
        (
            "crates/perl-dap/src/process.rs".to_string(),
            r#"
            impl Adapter {
                pub fn handle_initialize(&self) {
                    let _ = std::net::TcpStream::connect("127.0.0.1:0");
                }
            }
            "#
            .to_string(),
        ),
    ];
    let census = Census::from_sources(&sources);
    let lsp = census
        .resolve(SYNTHETIC_ROOT_FILE, "handle_initialize")
        .expect("the LSP handler resolves by exact file");

    assert!(
        !census.transitive_exposures(lsp, census::MAX_DEPTH).contains_key(&Exposure::Network),
        "DAP network work must not be attributed to the LSP initialize path"
    );
}

#[test]
fn cfg_test_helpers_are_excluded_from_the_census() {
    let sources = vec![(
        SYNTHETIC_ROOT_FILE.to_string(),
        r#"
        impl Server { pub fn handle_initialize(&self) {} }

        #[cfg(test)]
        mod tests {
            pub fn spawn_a_thing() {
                let _ = std::process::Command::new("perl").output();
            }
        }
        "#
        .to_string(),
    )];
    let census = Census::from_sources(&sources);

    assert!(
        census.resolve(SYNTHETIC_ROOT_FILE, "spawn_a_thing").is_none(),
        "a #[cfg(test)] helper must not enter the production census"
    );
}

#[test]
fn a_generic_method_name_does_not_resolve_outside_the_calling_file() {
    // `handle_initialize` calls `params.get("capabilities")` on a serde_json
    // Value. That receiver type is not indexed, so a crate-wide fallback would
    // resolve `get` to whichever single `get` method the crate defines and
    // manufacture a path into unrelated per-request work.
    let sources = vec![
        (
            SYNTHETIC_ROOT_FILE.to_string(),
            r#"
            impl Server {
                pub fn handle_initialize(&self, params: &Value) {
                    let _ = params.get("capabilities");
                }
            }
            "#
            .to_string(),
        ),
        (
            "crates/perl-lsp-rs/src/unrelated.rs".to_string(),
            r#"
            impl RequestContext {
                pub fn get(&self) -> Option<u8> {
                    let _ = std::env::var("PERL_LSP_SOMETHING");
                    None
                }
            }
            "#
            .to_string(),
        ),
        // A second definition, as in real source where `get` is everywhere.
        // Without it the name would be globally unique and resolve on that
        // branch, so the fixture would not exercise the locality rule at all.
        (
            "crates/perl-lsp-rs/src/other.rs".to_string(),
            "impl Cache { pub fn get(&self) -> Option<u8> { None } }".to_string(),
        ),
    ];
    let census = Census::from_sources(&sources);
    let root =
        census.resolve(SYNTHETIC_ROOT_FILE, "handle_initialize").expect("synthetic root resolves");

    assert!(
        census.transitive_exposures(root, census::MAX_DEPTH).is_empty(),
        "a same-crate `get` in another file must not be reached from an unrelated receiver"
    );
}

#[test]
fn colliding_names_are_not_claimed_to_be_dropped_edges() {
    // The collision set is a raw definition count. Resolution happens per call
    // site, so a colliding name is often still traversed; reporting it as
    // "edges not traversed" would overstate the blind spot.
    let sources = vec![(
        SYNTHETIC_ROOT_FILE.to_string(),
        r#"
        impl Server {
            pub fn handle_initialize(&self) { probe(); }
        }
        pub fn probe() { let _ = which::which("perltidy"); }
        impl Other { pub fn probe(&self) {} }
        "#
        .to_string(),
    )];
    let census = Census::from_sources(&sources);
    let root =
        census.resolve(SYNTHETIC_ROOT_FILE, "handle_initialize").expect("synthetic root resolves");

    assert!(census.colliding_names().contains("probe"), "`probe` collides by name");
    assert!(
        census.transitive_exposures(root, census::MAX_DEPTH).contains_key(&Exposure::PathLookup),
        "a colliding name still resolves per call site and must be traversed"
    );
}

// ---------------------------------------------------------------------------
// Fail-closed admission
// ---------------------------------------------------------------------------

#[test]
fn an_unparsable_source_is_reported_rather_than_silently_dropped() {
    // A source the census cannot read shrinks the denominator. Skipping it
    // quietly would let the ledger pass on a partial view of the tree.
    let mut sources = indirect_process_sources();
    sources.push((
        "crates/perl-lsp-rs/src/broken.rs".to_string(),
        "pub fn unreadable( { this is not rust".to_string(),
    ));
    let census = Census::from_sources(&sources);

    assert_eq!(census.unparsable_sources().len(), 1, "the unreadable file must be recorded");

    let errors = ledger_errors_with_roots(&[baseline_row()], &census, &synthetic_roots());
    assert_reports(&errors, "instrument failure");
    assert_reports(&errors, "crates/perl-lsp-rs/src/broken.rs");
}

#[test]
fn an_ambiguous_citation_is_rejected() {
    // One file can hold a `&self` method and a free function of the same name,
    // as `command_exists` does. Binding a row to whichever came first would let
    // it derive exposure from the wrong definition.
    let sources = vec![(
        SYNTHETIC_ROOT_FILE.to_string(),
        r#"
        impl Server { pub fn handle_initialize(&self) { probe(); } }
        impl Server { pub fn probe(&self) {} }
        pub fn probe() { let _ = std::process::Command::new("perl").output(); }
        "#
        .to_string(),
    )];
    let census = Census::from_sources(&sources);

    assert_eq!(census.citation_arity(SYNTHETIC_ROOT_FILE, "probe"), 2);
    assert!(
        census.resolve(SYNTHETIC_ROOT_FILE, "probe").is_none(),
        "an ambiguous citation must not bind arbitrarily"
    );

    let row =
        InitOperationRow { operation_id: "synthetic.probe", function: "probe", ..baseline_row() };
    let errors = ledger_errors_with_roots(&[row], &census, &synthetic_roots());
    assert_reports(&errors, "ambiguous citation");
}

#[test]
fn an_empty_ledger_is_rejected_structurally() {
    let census = Census::from_sources(&indirect_process_sources());
    let errors = ledger_errors_with_roots(&[], &census, &synthetic_roots());
    assert_reports(&errors, "the ledger is empty");
}

#[test]
fn a_file_declared_only_under_cfg_test_is_excluded() {
    // `#[cfg(test)] mod helpers;` pulls in an external file that parses as an
    // ordinary module. Its names must not suppress or redirect production edges.
    let sources = vec![
        (
            SYNTHETIC_ROOT_FILE.to_string(),
            r#"
            #[cfg(test)]
            mod helpers;

            impl Server { pub fn handle_initialize(&self) {} }
            "#
            .to_string(),
        ),
        (
            // `entry.rs` declaring `mod helpers;` owns `entry/helpers.rs`, not
            // a sibling `src/helpers.rs`.
            "crates/perl-lsp-rs/src/entry/helpers.rs".to_string(),
            r#"
            pub fn spawn_for_tests() {
                let _ = std::process::Command::new("perl").output();
            }
            "#
            .to_string(),
        ),
    ];
    let census = Census::from_sources(&sources);

    assert!(
        census.resolve("crates/perl-lsp-rs/src/entry/helpers.rs", "spawn_for_tests").is_none(),
        "a file declared only under #[cfg(test)] must not enter the production census"
    );
}

#[test]
fn a_method_call_does_not_resolve_to_a_free_function() {
    // Method syntax can only reach a method. Falling back to a free function of
    // the same name would let `x.probe()` inherit unrelated blocking work.
    let sources = vec![(
        SYNTHETIC_ROOT_FILE.to_string(),
        r#"
        impl Server {
            pub fn handle_initialize(&self, other: &Other) {
                other.probe();
            }
        }
        pub fn probe() { let _ = std::process::Command::new("perl").output(); }
        "#
        .to_string(),
    )];
    let census = Census::from_sources(&sources);
    let root =
        census.resolve(SYNTHETIC_ROOT_FILE, "handle_initialize").expect("synthetic root resolves");

    assert!(
        census.transitive_exposures(root, census::MAX_DEPTH).is_empty(),
        "a method call must not resolve to a free function of the same name"
    );
}

#[test]
fn a_cfg_test_method_inside_an_ordinary_impl_is_excluded() {
    let sources = vec![(
        SYNTHETIC_ROOT_FILE.to_string(),
        r#"
        impl Server {
            pub fn handle_initialize(&self) {}

            #[cfg(test)]
            fn test_only_spawn(&self) {
                let _ = std::process::Command::new("perl").output();
            }
        }
        "#
        .to_string(),
    )];
    let census = Census::from_sources(&sources);

    assert!(
        census.resolve(SYNTHETIC_ROOT_FILE, "test_only_spawn").is_none(),
        "a #[cfg(test)] method must not enter the production census"
    );
}

#[test]
fn a_test_module_declaration_excludes_only_its_own_file() {
    // Resolving `#[cfg(test)] mod tests;` to a bare name would exclude every
    // `tests.rs` in every scanned crate and silently drop production code.
    let sources = vec![
        ("crates/perl-lsp-rs/src/owner.rs".to_string(), "#[cfg(test)]\nmod tests;\n".to_string()),
        (
            "crates/perl-lsp-rs/src/owner/tests.rs".to_string(),
            "pub fn helper_in_test_module() {}".to_string(),
        ),
        (
            // Same basename, different owner: must survive.
            "crates/perl-dap/src/unrelated/tests.rs".to_string(),
            "pub fn unrelated_production_fn() {}".to_string(),
        ),
    ];
    let census = Census::from_sources(&sources);

    assert!(
        census.resolve("crates/perl-lsp-rs/src/owner/tests.rs", "helper_in_test_module").is_none(),
        "the declared test module must be excluded"
    );
    assert!(
        census
            .resolve("crates/perl-dap/src/unrelated/tests.rs", "unrelated_production_fn")
            .is_some(),
        "an unrelated file sharing the basename must not be excluded"
    );
}

// ---------------------------------------------------------------------------
// Call-site identity
// ---------------------------------------------------------------------------

/// Two rows citing one shared helper, distinguished only by call-site argument.
fn shared_helper_sources() -> Vec<(String, String)> {
    vec![
        (
            SYNTHETIC_ROOT_FILE.to_string(),
            r#"
            impl Server {
                pub fn handle_initialize(&self) {
                    self.detect_tool("perltidy");
                    self.detect_tool("perlcritic");
                }
            }
            "#
            .to_string(),
        ),
        (
            SYNTHETIC_HELPER_FILE.to_string(),
            r#"
            impl Server {
                pub fn detect_tool(&self, name: &str) -> bool {
                    which::which(name).is_ok()
                }
            }
            "#
            .to_string(),
        ),
    ]
}

#[test]
fn a_row_naming_a_call_site_that_does_not_exist_is_rejected() {
    let census = Census::from_sources(&shared_helper_sources());
    let row = InitOperationRow {
        operation_id: "synthetic.tool",
        file: SYNTHETIC_HELPER_FILE,
        function: "detect_tool",
        declared_exposure: &[Exposure::PathLookup],
        // No source calls detect_tool("pls").
        call_site_argument: "pls",
        ..baseline_row()
    };

    let errors = ledger_errors_with_roots(&[row], &census, &synthetic_roots());
    assert_reports(&errors, "no initialize-reachable source makes that call");
}

#[test]
fn rows_sharing_a_helper_are_independently_falsifiable() {
    // Both rows cite `detect_tool`. Each must stand on its own call site, so
    // removing one operation retires exactly one row.
    let census = Census::from_sources(&shared_helper_sources());
    let perltidy = InitOperationRow {
        operation_id: "synthetic.tool.perltidy",
        file: SYNTHETIC_HELPER_FILE,
        function: "detect_tool",
        declared_exposure: &[Exposure::PathLookup],
        call_site_argument: "perltidy",
        ..baseline_row()
    };
    let perlcritic = InitOperationRow {
        operation_id: "synthetic.tool.perlcritic",
        file: SYNTHETIC_HELPER_FILE,
        function: "detect_tool",
        declared_exposure: &[Exposure::PathLookup],
        call_site_argument: "perlcritic",
        owns_exposure: false,
        ..baseline_row()
    };

    let errors = ledger_errors_with_roots(
        &[perltidy.clone(), perlcritic.clone()],
        &census,
        &synthetic_roots(),
    );
    assert!(
        !errors.iter().any(|error| error.contains("makes that call")),
        "both call sites exist, so neither row is stale: {errors:#?}"
    );

    // Now delete only the perlcritic call.
    let mut reduced = shared_helper_sources();
    reduced[0].1 = reduced[0].1.replace("self.detect_tool(\"perlcritic\");", "");
    let reduced_census = Census::from_sources(&reduced);

    let errors =
        ledger_errors_with_roots(&[perltidy, perlcritic], &reduced_census, &synthetic_roots());
    assert_reports(&errors, "synthetic.tool.perlcritic");
    assert!(
        !errors.iter().any(|error| error.contains("synthetic.tool.perltidy")),
        "the surviving operation's row must not be reported: {errors:#?}"
    );
}

// ---------------------------------------------------------------------------
// Side effects are derived, not trusted
// ---------------------------------------------------------------------------

#[test]
fn a_side_effect_naming_an_unsent_method_is_rejected() {
    let mut sources = indirect_process_sources();
    sources.push((
        "crates/perl-lsp-rs/src/notify.rs".to_string(),
        r#"
        pub fn send(server: &Server) {
            server.notify("perl-lsp/index-ready", ());
        }
        "#
        .to_string(),
    ));
    let census = Census::from_sources(&sources);

    let mut row = baseline_row();
    row.side_effects = &["sends perl/workspaceReady"];

    let errors = ledger_errors_with_roots(&[row], &census, &synthetic_roots());
    assert_reports(&errors, "no scanned source sends `perl/workspaceReady`");
}

#[test]
fn a_side_effect_naming_a_real_method_is_accepted() {
    let mut sources = indirect_process_sources();
    sources.push((
        "crates/perl-lsp-rs/src/notify.rs".to_string(),
        r#"
        pub fn send(server: &Server) {
            server.notify("perl-lsp/index-ready", ());
        }
        "#
        .to_string(),
    ));
    let census = Census::from_sources(&sources);

    let mut row = baseline_row();
    row.side_effects = &["sends perl-lsp/index-ready"];

    let errors = ledger_errors_with_roots(&[row], &census, &synthetic_roots());
    assert!(
        !errors.iter().any(|error| error.contains("no scanned source sends")),
        "a method that is genuinely sent must not be reported: {errors:#?}"
    );
}

// ---------------------------------------------------------------------------
// Coverage: unregistered initialize work
// ---------------------------------------------------------------------------

#[test]
fn a_helper_spawning_a_process_without_a_row_is_rejected() {
    let census = Census::from_sources(&indirect_process_sources());

    // A ledger that accounts for nothing: the row exists but does not own its
    // closure, so the helper carrying the spawn belongs to no row.
    let mut row = baseline_row();
    row.owns_exposure = false;
    row.declared_exposure = &[];

    let errors = ledger_errors_with_roots(&[row], &census, &synthetic_roots());
    assert_reports(&errors, "unregistered initialize work");
    assert_reports(&errors, "read_profile_metadata");
}

#[test]
fn an_owning_row_accounts_for_the_helper_beneath_it() {
    let census = Census::from_sources(&indirect_process_sources());
    let errors = ledger_errors_with_roots(&[baseline_row()], &census, &synthetic_roots());

    assert!(
        !errors.iter().any(|error| error.contains("unregistered initialize work")),
        "an owning row must account for work in its closure, got: {errors:#?}"
    );
}

#[test]
fn a_ledger_with_no_owning_row_is_called_vacuous() {
    let census = Census::from_sources(&indirect_process_sources());
    let mut row = baseline_row();
    row.owns_exposure = false;
    row.declared_exposure = &[];

    let errors = ledger_errors_with_roots(&[row], &census, &synthetic_roots());
    assert_reports(&errors, "coverage checking would be vacuous");
}

#[test]
fn nested_owning_rows_are_rejected() {
    // Without this rule a single broad row would absorb every operation beneath
    // it and coverage would go green having discriminated nothing.
    let census = Census::from_sources(&indirect_process_sources());
    let outer = baseline_row();
    let inner = InitOperationRow {
        operation_id: "synthetic.inner",
        file: SYNTHETIC_HELPER_FILE,
        function: "read_profile_metadata",
        declared_exposure: &[Exposure::ProcessSpawn],
        ..baseline_row()
    };

    let errors = ledger_errors_with_roots(&[outer, inner], &census, &synthetic_roots());
    assert_reports(&errors, "are nested");
}

// ---------------------------------------------------------------------------
// Exposure declaration must match derived source, in both directions
// ---------------------------------------------------------------------------

#[test]
fn under_declared_exposure_is_rejected() {
    let census = Census::from_sources(&indirect_process_sources());
    let mut row = baseline_row();
    row.declared_exposure = &[];

    let errors = ledger_errors_with_roots(&[row], &census, &synthetic_roots());
    assert_reports(&errors, "under-declares exposure");
}

#[test]
fn over_declared_exposure_is_rejected() {
    // This is the stale-prose case. `find_perl_interpreter` carries a doc
    // comment about spawning a subprocess that the code does not do; a row
    // copied from that prose must not survive.
    let census = Census::from_sources(&indirect_process_sources());
    let mut row = baseline_row();
    row.declared_exposure = &[Exposure::ProcessSpawn, Exposure::Network];

    let errors = ledger_errors_with_roots(&[row], &census, &synthetic_roots());
    assert_reports(&errors, "over-declares exposure `network`");
}

#[test]
fn a_process_free_claim_over_process_work_is_rejected() {
    let census = Census::from_sources(&indirect_process_sources());
    let mut row = baseline_row();
    row.phase = PhaseDisposition::LocalProcessFreeConfigBeforeResponse;

    let errors = ledger_errors_with_roots(&[row], &census, &synthetic_roots());
    assert_reports(&errors, "claims process-free configuration but reaches");
}

// ---------------------------------------------------------------------------
// Static-surface authority
// ---------------------------------------------------------------------------

#[test]
fn a_static_surface_claim_without_a_join_is_rejected() {
    let census = Census::from_sources(&indirect_process_sources());
    let mut row = baseline_row();
    row.affects_static_initialize_result = true;
    row.static_surface_join = "";

    let errors = ledger_errors_with_roots(&[row], &census, &synthetic_roots());
    assert_reports(&errors, "without a #9662 final-surface join");
}

#[test]
fn ambient_state_cannot_become_static_surface_authority() {
    // EffectiveLspSurface is pure: it does not probe tools or read PATH. A row
    // that reaches ambient state and also claims a static-surface effect is
    // asserting exactly the edge #9662 removed.
    let census = Census::from_sources(&indirect_process_sources());
    let mut row = baseline_row();
    row.affects_static_initialize_result = true;
    row.static_surface_join = "serverCapabilities.documentFormattingProvider";

    let errors = ledger_errors_with_roots(&[row], &census, &synthetic_roots());
    assert_reports(&errors, "depends on ambient state");
}

#[test]
fn a_join_without_a_static_surface_claim_is_rejected() {
    let census = Census::from_sources(&indirect_process_sources());
    let mut row = baseline_row();
    row.static_surface_join = "serverCapabilities.documentFormattingProvider";

    let errors = ledger_errors_with_roots(&[row], &census, &synthetic_roots());
    assert_reports(&errors, "names a final-surface join but claims no static");
}

// ---------------------------------------------------------------------------
// Phase discipline
// ---------------------------------------------------------------------------

#[test]
fn a_deferred_row_still_running_before_the_response_needs_a_wave() {
    let census = Census::from_sources(&indirect_process_sources());
    let mut row = baseline_row();
    row.phase = PhaseDisposition::DeferToPostInitializeEnvironment;
    row.current_point = ExecutionPoint::BeforeResponse;
    row.migration_wave = MigrationWave::None;

    let errors = ledger_errors_with_roots(&[row], &census, &synthetic_roots());
    assert_reports(&errors, "with no migration wave");
}

#[test]
fn a_before_response_claim_over_deferred_work_is_rejected() {
    let census = Census::from_sources(&indirect_process_sources());
    let mut row = baseline_row();
    row.phase = PhaseDisposition::ProtocolRequiredBeforeResponse;
    row.current_point = ExecutionPoint::AfterResponse;

    let errors = ledger_errors_with_roots(&[row], &census, &synthetic_roots());
    assert_reports(&errors, "but currently runs");
}

#[test]
fn a_terminal_disposition_claiming_a_cutover_is_rejected() {
    let census = Census::from_sources(&indirect_process_sources());
    let mut row = baseline_row();
    row.phase = PhaseDisposition::ExistingExternalOwnerNoMove;
    row.migration_wave = MigrationWave::E02;

    let errors = ledger_errors_with_roots(&[row], &census, &synthetic_roots());
    assert_reports(&errors, "terminal disposition");
}

// ---------------------------------------------------------------------------
// Structural and citation discipline
// ---------------------------------------------------------------------------

#[test]
fn a_stale_citation_is_rejected() {
    let census = Census::from_sources(&indirect_process_sources());
    let mut row = baseline_row();
    row.function = "function_that_was_renamed_away";

    let errors = ledger_errors_with_roots(&[row], &census, &synthetic_roots());
    assert_reports(&errors, "stale citation");
}

#[test]
fn a_citation_naming_the_wrong_file_is_rejected() {
    let census = Census::from_sources(&indirect_process_sources());
    let mut row = baseline_row();
    row.file = SYNTHETIC_HELPER_FILE;

    let errors = ledger_errors_with_roots(&[row], &census, &synthetic_roots());
    assert_reports(&errors, "stale citation");
}

#[test]
fn duplicate_operation_ids_are_rejected() {
    let census = Census::from_sources(&indirect_process_sources());
    let errors =
        ledger_errors_with_roots(&[baseline_row(), baseline_row()], &census, &synthetic_roots());
    assert_reports(&errors, "duplicate operation_id");
}

#[test]
fn a_row_without_a_trigger_is_rejected() {
    let census = Census::from_sources(&indirect_process_sources());
    let mut row = baseline_row();
    row.triggers = &[];

    let errors = ledger_errors_with_roots(&[row], &census, &synthetic_roots());
    assert_reports(&errors, "declares no trigger");
}

#[test]
fn a_missing_census_root_is_rejected() {
    let census = Census::from_sources(&indirect_process_sources());
    let errors = ledger_errors_with_roots(
        &[baseline_row()],
        &census,
        &[("crates/perl-lsp-rs/src/gone.rs", "vanished_entry_point")],
    );
    assert_reports(&errors, "denominator cannot be established");
}

// ---------------------------------------------------------------------------
// Positive controls against real source
// ---------------------------------------------------------------------------

#[test]
fn the_maintained_ledger_matches_current_source() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let census = Census::from_workspace(&root).expect("census builds from the workspace");
    let rows = ledger_rows();

    assert!(!rows.is_empty(), "the ledger must not be empty");

    let errors = ledger_errors(&rows, &census);
    assert!(errors.is_empty(), "the maintained ledger disagrees with current source: {errors:#?}");
}

#[test]
fn every_row_has_exactly_one_phase_and_a_named_owner() {
    for row in ledger_rows() {
        assert!(!row.operation_id.is_empty(), "row identity must be stable and non-empty");
        assert!(
            !row.phase.label().is_empty(),
            "row {} must carry a phase disposition",
            row.operation_id
        );
        assert!(
            !row.current_owner.is_empty() && !row.target_owner.is_empty(),
            "row {} must name a current and target owner",
            row.operation_id
        );
    }
}

#[test]
fn rendered_output_is_deterministic() {
    let rows = ledger_rows();
    assert_eq!(render_json(&rows), render_json(&rows), "a second render must not differ");
}

#[test]
fn rendered_output_is_row_order_independent() {
    let mut reversed = ledger_rows();
    reversed.reverse();
    assert_eq!(
        render_json(&ledger_rows()),
        render_json(&reversed),
        "input ordering must not change rendered bytes"
    );
}

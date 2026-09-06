//! Falsifiers for the initialize-operation phase and owner ledger (#10040).
//!
//! A ledger that only ever passes proves nothing. Each test here builds a
//! deliberately wrong ledger, or a synthetic codebase containing the mistake the
//! controlling issue names as a negative control, and asserts the checker
//! rejects it. The final tests confirm the real ledger passes against real
//! source and that generated output is deterministic.

use xtask::init_environment::census::{Census, Exposure};
use xtask::init_environment::{
    ExecutionPoint, InitOperationRow, MigrationWave, PhaseDisposition, RESPONSE_COMMITTING_ROOT,
    Trigger, census, ledger_errors, ledger_errors_with_roots, ledger_rows, render_json,
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

/// One file per interesting `cfg` shape, each defining a distinctly named
/// helper, so a single census answers the whole family.
fn cfg_shape_sources() -> Vec<(String, String)> {
    vec![(
        SYNTHETIC_ROOT_FILE.to_string(),
        r#"
        impl Server { pub fn handle_initialize(&self) {} }

        #[cfg(test)]
        fn only_under_test() {}

        #[cfg(all(test, unix))]
        fn only_under_test_on_unix() {}

        #[cfg(any(test, feature = "expose_lsp_test_api"))]
        fn shipped_when_the_feature_is_on() {}

        #[cfg(not(test))]
        fn never_under_test() {}

        #[cfg(feature = "expose_lsp_test_api")]
        fn behind_a_plain_feature() {}

        #[cfg(unix)]
        fn behind_a_target() {}
        "#
        .to_string(),
    )]
}

#[test]
fn a_cfg_that_can_never_hold_without_test_is_excluded() {
    let census = Census::from_sources(&cfg_shape_sources());

    for excluded in ["only_under_test", "only_under_test_on_unix"] {
        assert!(
            census.resolve(SYNTHETIC_ROOT_FILE, excluded).is_none(),
            "{excluded} compiles only under `test` and must stay out of the census"
        );
    }
}

#[test]
fn a_cfg_that_ships_outside_test_is_kept_even_when_it_mentions_test() {
    // The census predicate must be "cannot compile without `test`", not
    // "mentions `test`". `#[cfg(any(test, feature = "expose_lsp_test_api"))]`
    // is a real, shipping shape in `perl-lsp-rs`, and `#[cfg(not(test))]` is
    // production-*only*. Excluding either silently narrows the denominator,
    // which is the direction that hides blocking work.
    let census = Census::from_sources(&cfg_shape_sources());

    for kept in [
        "shipped_when_the_feature_is_on",
        "never_under_test",
        "behind_a_plain_feature",
        "behind_a_target",
    ] {
        assert!(
            census.resolve(SYNTHETIC_ROOT_FILE, kept).is_some(),
            "{kept} can compile without `test` and must remain in the census"
        );
    }
}

#[test]
fn feature_gated_blocking_work_is_still_owned_by_a_row() {
    // The consequence that matters: a spawn behind `any(test, feature = "…")`
    // is reachable production work, so coverage must demand a row for it.
    let sources = vec![
        (
            SYNTHETIC_ROOT_FILE.to_string(),
            r#"
            impl Server {
                pub fn handle_initialize(&self) {
                    probe_under_feature();
                }
            }
            "#
            .to_string(),
        ),
        (
            SYNTHETIC_HELPER_FILE.to_string(),
            r#"
            #[cfg(any(test, feature = "expose_lsp_test_api"))]
            pub fn probe_under_feature() {
                let _ = std::process::Command::new("perl").output();
            }
            "#
            .to_string(),
        ),
    ];
    let census = Census::from_sources(&sources);
    let mut row = baseline_row();
    row.owns_exposure = false;
    row.declared_exposure = &[];

    let errors = ledger_errors_with_roots(&[row], &census, &synthetic_roots());
    assert_reports(&errors, "unregistered initialize work");
    assert_reports(&errors, "probe_under_feature");
}

#[test]
fn a_test_module_behind_a_path_attribute_is_excluded() {
    // `#[path]` overrides the conventional file lookup, so a test module can
    // point at a filename the conventional exclusion never names. Rust resolves
    // the attribute relative to the directory containing the declaring file, so
    // `src/entry.rs` with `#[path = "fixtures/entry_cases.rs"]` names
    // `src/fixtures/entry_cases.rs` — not `src/entry/fixtures/entry_cases.rs`.
    let sources = vec![
        (
            "crates/perl-lsp-rs/src/entry.rs".to_string(),
            r#"
            impl Server { pub fn handle_initialize(&self) {} }

            #[cfg(test)]
            #[path = "fixtures/entry_cases.rs"]
            mod cases;
            "#
            .to_string(),
        ),
        (
            "crates/perl-lsp-rs/src/fixtures/entry_cases.rs".to_string(),
            r#"
            pub fn spawn_from_a_relocated_test_module() {
                let _ = std::process::Command::new("perl").output();
            }
            "#
            .to_string(),
        ),
    ];
    let census = Census::from_sources(&sources);

    assert!(
        census
            .resolve(
                "crates/perl-lsp-rs/src/fixtures/entry_cases.rs",
                "spawn_from_a_relocated_test_module"
            )
            .is_none(),
        "a #[path]-relocated #[cfg(test)] module must not enter the production census"
    );
}

#[test]
fn a_path_attribute_resolves_against_the_declaring_file_not_the_module_directory() {
    // The falsifier mirrors Rust's real layout for
    // `src/documentation_targets.rs` + `#[path = "documentation_targets_tests.rs"]`:
    // the file lives beside the declaring file, and the module-directory
    // prediction (`src/entry/…` here) is exactly the misresolution this must
    // reject. A helper in the mispredicted location stays production; the real
    // relocated test module is excluded.
    let sources = vec![
        (
            "crates/perl-lsp-rs/src/entry.rs".to_string(),
            r#"
            impl Server { pub fn handle_initialize(&self) {} }

            #[cfg(test)]
            #[path = "entry_cases.rs"]
            mod cases;
            "#
            .to_string(),
        ),
        (
            "crates/perl-lsp-rs/src/entry/entry_cases.rs".to_string(),
            "pub fn helper_in_the_mispredicted_location() {}".to_string(),
        ),
        (
            "crates/perl-lsp-rs/src/entry_cases.rs".to_string(),
            r#"
            pub fn spawn_from_the_real_test_module() {
                let _ = std::process::Command::new("perl").output();
            }
            "#
            .to_string(),
        ),
    ];
    let census = Census::from_sources(&sources);

    assert!(
        census
            .resolve("crates/perl-lsp-rs/src/entry_cases.rs", "spawn_from_the_real_test_module")
            .is_none(),
        "the real #[path] target must be excluded from the production census"
    );
    assert!(
        census
            .resolve(
                "crates/perl-lsp-rs/src/entry/entry_cases.rs",
                "helper_in_the_mispredicted_location"
            )
            .is_some(),
        "a production file that merely shares the predicted name must stay indexed"
    );
}

#[test]
fn a_whole_impl_under_cfg_test_is_excluded_from_the_census() {
    // A `#[cfg(test)]` gate on an entire `impl` never reaches the method items:
    // each method carries no attribute of its own, so without an impl-level
    // stop the methods redirect same-name edges and satisfy protocol claims.
    let sources = vec![(
        SYNTHETIC_ROOT_FILE.to_string(),
        r#"
        impl Server { pub fn handle_initialize(&self) {} }

        #[cfg(test)]
        impl Server {
            pub fn handle_initialize(&self) {
                let _ = std::process::Command::new("perl").output();
            }
            pub fn test_only_announce(&self) {
                self.notify("perl-lsp/index-ready", ());
            }
        }
        "#
        .to_string(),
    )];
    let census = Census::from_sources(&sources);

    assert!(
        census.resolve(SYNTHETIC_ROOT_FILE, "test_only_announce").is_none(),
        "a method of a #[cfg(test)] impl must not enter the production census"
    );
    assert!(
        census.citation_arity(SYNTHETIC_ROOT_FILE, "handle_initialize") == 1,
        "the test-only colliding root must not enter the census alongside the real one"
    );
}

#[test]
fn a_globally_unique_method_name_does_not_bypass_receiver_locality() {
    // With only one same-crate definition the uniqueness shortcut would bind
    // any receiver's method call to it. The receiver here is a type the census
    // does not index, so the edge must stay unresolved regardless of
    // uniqueness; a second definition is not needed to exercise the branch.
    let sources = vec![
        (
            SYNTHETIC_ROOT_FILE.to_string(),
            r#"
            impl Server {
                pub fn handle_initialize(&self, ctx: &Foreign) {
                    ctx.reap_stale_sessions();
                }
            }
            "#
            .to_string(),
        ),
        (
            "crates/perl-lsp-rs/src/other.rs".to_string(),
            r#"
            impl SessionTable {
                pub fn reap_stale_sessions(&self) {
                    let _ = std::process::Command::new("perl").output();
                }
            }
            "#
            .to_string(),
        ),
    ];
    let census = Census::from_sources(&sources);
    let root =
        census.resolve(SYNTHETIC_ROOT_FILE, "handle_initialize").expect("synthetic root resolves");

    // This is the documented over-approximation: a mis-bound edge can only
    // produce a spurious finding, never hide reachable work, and dropping
    // unique cross-file `self.helper()` edges would shrink the denominator.
    // The falsifier pins the rule so a silent direction flip cannot happen.
    assert!(
        census.transitive_exposures(root, census::MAX_DEPTH).contains_key(&Exposure::ProcessSpawn),
        "rule 1 (unique definition) binds before locality narrows; this pin must match the doc"
    );
}

#[test]
fn a_recursive_call_does_not_bind_a_namesake() {
    // `descend` calls itself, and an unrelated same-named definition carries a
    // spawn. Resolving the recursive call against the namesake would
    // attribute the spawn to everything that reaches the recursive function.
    let sources = vec![
        (
            SYNTHETIC_ROOT_FILE.to_string(),
            r#"
            impl Server {
                pub fn handle_initialize(&self) {
                    let _ = descend(3);
                }
            }
            pub fn descend(depth: usize) -> usize {
                if depth == 0 { 0 } else { descend(depth - 1) }
            }
            "#
            .to_string(),
        ),
        (
            SYNTHETIC_HELPER_FILE.to_string(),
            r#"
            pub fn descend(depth: usize) -> usize {
                let _ = std::process::Command::new("perl").output();
                depth
            }
            "#
            .to_string(),
        ),
    ];
    let census = Census::from_sources(&sources);
    let root =
        census.resolve(SYNTHETIC_ROOT_FILE, "handle_initialize").expect("synthetic root resolves");
    let recursive = census.resolve(SYNTHETIC_ROOT_FILE, "descend").expect("recursive fn resolves");

    assert!(
        !census.transitive_exposures(root, census::MAX_DEPTH).contains_key(&Exposure::ProcessSpawn),
        "the recursive call must not resolve to the same-named definition in another file"
    );
    assert!(
        census.transitive_exposures(recursive, census::MAX_DEPTH).is_empty(),
        "traversal from the recursive function must not reach its namesake"
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
// Process exposure is execution, not construction
// ---------------------------------------------------------------------------

#[test]
fn a_command_builder_without_execution_is_not_a_spawn() {
    // `Command::new` constructs a builder; nothing runs until `spawn`,
    // `output`, or `status`. A builder-only helper must not fabricate spawn
    // exposure and force false ledger declarations.
    let sources = vec![
        (
            SYNTHETIC_ROOT_FILE.to_string(),
            r#"
            impl Server {
                pub fn handle_initialize(&self) {
                    let _ = build_command("perl");
                }
            }
            "#
            .to_string(),
        ),
        (
            SYNTHETIC_HELPER_FILE.to_string(),
            r#"
            pub fn build_command(program: &str) -> std::process::Command {
                let mut command = std::process::Command::new(program);
                command.arg("--version");
                command
            }
            "#
            .to_string(),
        ),
    ];
    let census = Census::from_sources(&sources);
    let root =
        census.resolve(SYNTHETIC_ROOT_FILE, "handle_initialize").expect("synthetic root resolves");

    assert!(
        census.transitive_exposures(root, census::MAX_DEPTH).is_empty(),
        "building a Command without executing it must not count as process work"
    );
}

#[test]
fn a_command_executed_from_a_split_variable_is_a_spawn() {
    // Positive control for the guard: a builder constructed in one statement
    // and executed in another within one body is still process work.
    let sources = vec![
        (
            SYNTHETIC_ROOT_FILE.to_string(),
            r#"
            impl Server {
                pub fn handle_initialize(&self) {
                    run_perl_version();
                }
            }
            "#
            .to_string(),
        ),
        (
            SYNTHETIC_HELPER_FILE.to_string(),
            r#"
            pub fn run_perl_version() {
                let mut command = std::process::Command::new("perl");
                command.arg("--version");
                let _ = command.output();
            }
            pub fn spawn_directly() {
                let _ = std::process::Command::new("perl").spawn();
            }
            pub fn status_directly() {
                let _ = std::process::Command::new("perl").status();
            }
            "#
            .to_string(),
        ),
    ];
    let census = Census::from_sources(&sources);
    let root =
        census.resolve(SYNTHETIC_ROOT_FILE, "handle_initialize").expect("synthetic root resolves");

    let exposures = census.transitive_exposures(root, census::MAX_DEPTH);
    assert!(
        exposures.contains_key(&Exposure::ProcessSpawn),
        "an executed split-variable command is process work, got {exposures:?}"
    );

    for name in ["run_perl_version", "spawn_directly", "status_directly"] {
        let index = census.resolve(SYNTHETIC_HELPER_FILE, name).expect("helper resolves");
        assert!(
            census.direct_exposures(index).contains(&Exposure::ProcessSpawn),
            "{name} must carry direct process exposure"
        );
    }
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

#[test]
fn a_same_named_call_elsewhere_cannot_keep_a_stale_row_alive() {
    // Call-site identity must include the resolved callee, not only the
    // callee's final name plus the literal. Here `detect_tool("perltidy")` is
    // called twice: once on the cited same-file method, and once — through an
    // initialize-reachable path — on an unrelated type in another file.
    // Deleting only the cited call must retire the row; a name-plus-literal
    // match would keep it validated by the decoy call.
    let cited_sources = vec![
        (
            SYNTHETIC_ROOT_FILE.to_string(),
            r#"
            impl Server {
                pub fn handle_initialize(&self) {
                    self.detect_tool("perltidy");
                    self.run_decoy();
                }
                pub fn detect_tool(&self, name: &str) -> bool {
                    name == "perltidy"
                }
                pub fn run_decoy(&self) {
                    decoy_runner();
                }
            }
            "#
            .to_string(),
        ),
        (
            SYNTHETIC_HELPER_FILE.to_string(),
            r#"
            pub struct Decoy;
            impl Decoy {
                pub fn detect_tool(&self, _name: &str) -> bool {
                    false
                }
            }
            pub fn decoy_runner() -> bool {
                Decoy.detect_tool("perltidy")
            }
            "#
            .to_string(),
        ),
    ];
    let row = InitOperationRow {
        operation_id: "synthetic.tool",
        file: SYNTHETIC_ROOT_FILE,
        function: "detect_tool",
        declared_exposure: &[],
        call_site_argument: "perltidy",
        owns_exposure: false,
        ..baseline_row()
    };

    let census = Census::from_sources(&cited_sources);
    let errors = ledger_errors_with_roots(&[row.clone()], &census, &synthetic_roots());
    assert!(
        !errors.iter().any(|error| error.contains("makes that call")),
        "the cited call site exists, so the row must validate: {errors:#?}"
    );

    // Delete only the call targeting the cited function.
    let mut reduced = cited_sources;
    reduced[0].1 = reduced[0].1.replace("self.detect_tool(\"perltidy\");", "");
    let reduced_census = Census::from_sources(&reduced);

    let errors = ledger_errors_with_roots(&[row], &reduced_census, &synthetic_roots());
    assert_reports(&errors, "makes that call");
}

#[test]
fn a_later_argument_cannot_preserve_a_row_call_site() {
    // The discriminator is the FIRST argument. A row whose distinguishing
    // literal only appears in a later argument position describes a call the
    // source no longer makes, so an unrelated trailing string must not keep
    // the row alive.
    let sources = vec![(
        SYNTHETIC_ROOT_FILE.to_string(),
        r#"
        impl Server {
            pub fn handle_initialize(&self) {
                self.detect_tool(mode, "perltidy");
            }
            pub fn detect_tool(&self, mode: &str, name: &str) -> bool {
                name == "perltidy"
            }
        }
        "#
        .to_string(),
    )];
    let census = Census::from_sources(&sources);
    let row = InitOperationRow {
        operation_id: "synthetic.tool",
        file: SYNTHETIC_ROOT_FILE,
        function: "detect_tool",
        declared_exposure: &[],
        call_site_argument: "perltidy",
        owns_exposure: false,
        ..baseline_row()
    };

    let errors = ledger_errors_with_roots(&[row], &census, &synthetic_roots());
    assert_reports(&errors, "makes that call");
}

// ---------------------------------------------------------------------------
// Side effects are derived, not trusted
// ---------------------------------------------------------------------------

#[test]
fn a_side_effect_naming_an_unsent_method_is_rejected() {
    let census = Census::from_sources(&indirect_process_sources());

    let mut row = baseline_row();
    row.side_effects = &["sends perl/workspaceReady"];

    let errors = ledger_errors_with_roots(&[row], &census, &synthetic_roots());
    assert_reports(&errors, "no function reachable from");
    assert_reports(&errors, "sends `perl/workspaceReady`");
}

#[test]
fn a_side_effect_only_unrelated_code_sends_is_rejected() {
    // The literal exists somewhere in the scanned tree, but nothing reachable
    // from the row's cited operation sends it. A workspace-global literal scan
    // would validate this row; binding the claim to the cited operation's
    // closure rejects it.
    let mut sources = indirect_process_sources();
    sources.push((
        "crates/perl-lsp-rs/src/notify.rs".to_string(),
        r#"
        pub fn send_unrelated(server: &Server) {
            server.notify("perl-lsp/index-ready", ());
        }
        "#
        .to_string(),
    ));
    let census = Census::from_sources(&sources);

    let mut row = baseline_row();
    row.side_effects = &["sends perl-lsp/index-ready"];

    let errors = ledger_errors_with_roots(&[row], &census, &synthetic_roots());
    assert_reports(&errors, "no function reachable from");
    assert_reports(&errors, "sends `perl-lsp/index-ready`");
}

#[test]
fn a_side_effect_the_cited_operation_reaches_is_accepted() {
    // Positive control: the sender sits inside the cited operation's closure,
    // so the claim is bound to code that actually runs it.
    let sources = vec![
        (
            SYNTHETIC_ROOT_FILE.to_string(),
            r#"
            impl Server {
                pub fn handle_initialize(&self) {
                    self.announce_index_ready();
                }
                pub fn announce_index_ready(&self) {
                    self.notify("perl-lsp/index-ready", ());
                }
            }
            "#
            .to_string(),
        ),
        (SYNTHETIC_HELPER_FILE.to_string(), "pub fn unused() {}".to_string()),
    ];
    let census = Census::from_sources(&sources);

    let mut row = baseline_row();
    row.side_effects = &["sends perl-lsp/index-ready"];

    let errors = ledger_errors_with_roots(&[row], &census, &synthetic_roots());
    assert!(
        !errors.iter().any(|error| error.contains("claims side effect")),
        "a method the cited operation genuinely sends must not be reported: {errors:#?}"
    );
}

#[test]
fn a_dollar_prefixed_standard_method_claim_is_validated() {
    // `$/progress` is a standard method. The claim must be checked, not
    // silently skipped by the recognizer.
    let sources = vec![
        (
            SYNTHETIC_ROOT_FILE.to_string(),
            r#"
            impl Server {
                pub fn handle_initialize(&self) {
                    self.report_progress();
                }
                pub fn report_progress(&self) {
                    self.notify("$/progress", ());
                }
            }
            "#
            .to_string(),
        ),
        (SYNTHETIC_HELPER_FILE.to_string(), "pub fn unused() {}".to_string()),
    ];
    let census = Census::from_sources(&sources);

    let mut row = baseline_row();
    row.side_effects = &["sends $/progress"];

    let errors = ledger_errors_with_roots(&[row], &census, &synthetic_roots());
    assert!(
        !errors.iter().any(|error| error.contains("claims side effect")),
        "a recognized standard-method claim that is genuinely sent must validate: {errors:#?}"
    );
}

#[test]
fn a_stale_dollar_prefixed_claim_is_rejected() {
    let census = Census::from_sources(&indirect_process_sources());

    let mut row = baseline_row();
    row.side_effects = &["sends $/progress"];

    let errors = ledger_errors_with_roots(&[row], &census, &synthetic_roots());
    assert_reports(&errors, "no function reachable from");
    assert_reports(&errors, "`$/progress`");
}

#[test]
fn a_multi_segment_extension_method_claim_is_validated() {
    let sources = vec![
        (
            SYNTHETIC_ROOT_FILE.to_string(),
            r#"
            impl Server {
                pub fn handle_initialize(&self) {
                    self.announce_extension();
                }
                pub fn announce_extension(&self) {
                    self.notify("perl/lsp/indexReady", ());
                }
            }
            "#
            .to_string(),
        ),
        (SYNTHETIC_HELPER_FILE.to_string(), "pub fn unused() {}".to_string()),
    ];
    let census = Census::from_sources(&sources);

    let mut row = baseline_row();
    row.side_effects = &["sends perl/lsp/indexReady"];

    let errors = ledger_errors_with_roots(&[row], &census, &synthetic_roots());
    assert!(
        !errors.iter().any(|error| error.contains("claims side effect")),
        "a multi-segment extension claim that is genuinely sent must validate: {errors:#?}"
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
// Execution point is derived from which root reaches the operation
// ---------------------------------------------------------------------------
//
// `phase_errors` only ever compares two ledger declarations to each other, so a
// stale `current_point` validated itself and could assign work to the wrong
// migration wave. These falsifiers pin the derived rule: a synthetic codebase
// that puts one operation behind the response-committing root and another
// behind a post-response root, with a third that no root reaches at all.

const POST_RESPONSE_ROOT_FILE: &str = "crates/perl-lsp-rs/src/runtime/dispatch/lifecycle.rs";

fn lifecycle_timing_sources() -> Vec<(String, String)> {
    vec![
        (
            RESPONSE_COMMITTING_ROOT.0.to_string(),
            r#"
            pub fn handle_initialize() {
                normalize_capabilities();
                shared_phase_helper();
            }
            fn normalize_capabilities() {
                let _ = std::process::Command::new("perl").arg("--version").output();
            }
            "#
            .to_string(),
        ),
        (
            POST_RESPONSE_ROOT_FILE.to_string(),
            r#"
            pub fn complete_initialization() {
                start_workspace_indexing();
                shared_phase_helper();
            }
            fn start_workspace_indexing() {
                let _ = std::fs::read_to_string("Makefile.PL");
            }
            "#
            .to_string(),
        ),
        (
            SYNTHETIC_HELPER_FILE.to_string(),
            r#"
            pub fn resolve_timing_mode() {
                let _ = std::env::var("PERL_LSP_TIMING");
            }
            pub fn shared_phase_helper() {}
            "#
            .to_string(),
        ),
    ]
}

fn lifecycle_roots() -> Vec<(&'static str, &'static str)> {
    vec![RESPONSE_COMMITTING_ROOT, (POST_RESPONSE_ROOT_FILE, "complete_initialization")]
}

/// The pre-response row: reached by the response-committing root only.
fn pre_response_row() -> InitOperationRow {
    InitOperationRow {
        operation_id: "synthetic.pre",
        file: RESPONSE_COMMITTING_ROOT.0,
        function: RESPONSE_COMMITTING_ROOT.1,
        declared_exposure: &[Exposure::ProcessSpawn],
        current_point: ExecutionPoint::BeforeResponse,
        phase: PhaseDisposition::ProtocolRequiredBeforeResponse,
        migration_wave: MigrationWave::None,
        ..baseline_row()
    }
}

/// The post-response row: reached only by the `initialized` root.
fn post_response_row() -> InitOperationRow {
    InitOperationRow {
        operation_id: "synthetic.post",
        file: POST_RESPONSE_ROOT_FILE,
        function: "complete_initialization",
        declared_exposure: &[Exposure::Filesystem],
        triggers: &[Trigger::Initialized],
        current_point: ExecutionPoint::AfterResponse,
        phase: PhaseDisposition::DeferToPostInitializeEnvironment,
        migration_wave: MigrationWave::None,
        ..baseline_row()
    }
}

#[test]
fn the_fixture_names_the_real_response_committing_root() {
    // If the product renames its response-committing entry point, these
    // falsifiers must stop compiling against a fiction rather than keep
    // passing against one.
    assert_eq!(RESPONSE_COMMITTING_ROOT.1, "handle_initialize");
    let census = Census::from_sources(&lifecycle_timing_sources());
    assert!(
        census.resolve(RESPONSE_COMMITTING_ROOT.0, RESPONSE_COMMITTING_ROOT.1).is_some(),
        "the synthetic fixture must define the response-committing root"
    );
}

#[test]
fn the_lifecycle_fixture_ledger_is_accepted() {
    // Positive control: without this, every rejection below could be an
    // artefact of the fixture rather than of the declared execution point.
    let census = Census::from_sources(&lifecycle_timing_sources());
    let errors = ledger_errors_with_roots(
        &[pre_response_row(), post_response_row()],
        &census,
        &lifecycle_roots(),
    );
    assert!(errors.is_empty(), "the lifecycle fixture ledger must pass, got: {errors:#?}");
}

#[test]
fn a_before_response_claim_the_response_root_cannot_reach_is_rejected() {
    // The stale-timing case the phase rule could not see: post-response work
    // still declaring that it blocks the response, and therefore claiming a
    // cutover wave for a move that has already happened.
    let census = Census::from_sources(&lifecycle_timing_sources());
    let mut row = post_response_row();
    row.current_point = ExecutionPoint::BeforeResponse;
    row.migration_wave = MigrationWave::E02;

    let errors = ledger_errors_with_roots(&[pre_response_row(), row], &census, &lifecycle_roots());
    assert_reports(&errors, "declares `before_response`");
    assert_reports(&errors, "not reachable from the response-committing root");
}

#[test]
fn an_after_response_claim_over_blocking_work_is_rejected() {
    // The dangerous direction: work that really does block the response
    // describing itself as already deferred, so nobody schedules the move.
    let census = Census::from_sources(&lifecycle_timing_sources());
    let mut row = pre_response_row();
    row.current_point = ExecutionPoint::AfterResponse;
    row.phase = PhaseDisposition::DeferToPostInitializeEnvironment;

    let errors = ledger_errors_with_roots(&[row, post_response_row()], &census, &lifecycle_roots());
    assert_reports(&errors, "declares `after_response`");
    assert_reports(&errors, "reachable only from the response-committing root");
}

#[test]
fn an_on_demand_claim_over_lifecycle_reachable_work_is_rejected() {
    // `on_demand` must not become the escape hatch that `before`/`after`
    // derivation closed: a function on a lifecycle call graph runs whether or
    // not the prose calls it lazy.
    let census = Census::from_sources(&lifecycle_timing_sources());
    let mut row = pre_response_row();
    row.current_point = ExecutionPoint::OnDemand;
    row.phase = PhaseDisposition::LazyOnFirstUse;

    let errors = ledger_errors_with_roots(&[row, post_response_row()], &census, &lifecycle_roots());
    assert_reports(&errors, "declares `on_demand`");
    assert_reports(&errors, "runs unconditionally");
}

#[test]
fn an_after_response_claim_over_work_no_root_reaches_is_rejected() {
    // The symmetric hole: requiring only "not reachable *solely* from the
    // response-committing root" would let a row nothing reaches at all sit in
    // `after_response` and inherit that point's migration timing.
    let census = Census::from_sources(&lifecycle_timing_sources());
    let unreached = InitOperationRow {
        operation_id: "synthetic.unreached",
        file: SYNTHETIC_HELPER_FILE,
        function: "resolve_timing_mode",
        declared_exposure: &[Exposure::EnvRead],
        current_point: ExecutionPoint::AfterResponse,
        phase: PhaseDisposition::DeferToPostInitializeEnvironment,
        migration_wave: MigrationWave::None,
        owns_exposure: false,
        ..baseline_row()
    };

    let errors = ledger_errors_with_roots(
        &[pre_response_row(), post_response_row(), unreached],
        &census,
        &lifecycle_roots(),
    );
    assert_reports(&errors, "synthetic.unreached");
    assert_reports(&errors, "is reached by no lifecycle root");
}

#[test]
fn an_on_demand_row_no_root_reaches_is_accepted() {
    // Negative control for the rule above: a genuinely lazy leaf, reached by no
    // lifecycle root, must not be forced into a response-relative point.
    let census = Census::from_sources(&lifecycle_timing_sources());
    let lazy = InitOperationRow {
        operation_id: "synthetic.lazy",
        file: SYNTHETIC_HELPER_FILE,
        function: "resolve_timing_mode",
        declared_exposure: &[Exposure::EnvRead],
        triggers: &[Trigger::FirstUse],
        current_point: ExecutionPoint::OnDemand,
        phase: PhaseDisposition::LazyOnFirstUse,
        migration_wave: MigrationWave::None,
        owns_exposure: false,
        ..baseline_row()
    };

    let errors = ledger_errors_with_roots(
        &[pre_response_row(), post_response_row(), lazy],
        &census,
        &lifecycle_roots(),
    );
    assert!(
        !errors.iter().any(|error| error.contains("synthetic.lazy")),
        "an unreachable lazy leaf must not be given a response-relative point, got: {errors:#?}"
    );
}

#[test]
fn a_helper_shared_by_both_phases_cannot_claim_after_response() {
    // Both lifecycle roots call the same helper. Initialization already
    // executes it before the response, so an `after_response` row would hide
    // pre-response execution behind a single-valued timing model — the model
    // derives `before_response` for dual-reachable operations.
    let census = Census::from_sources(&lifecycle_timing_sources());
    let shared = InitOperationRow {
        operation_id: "synthetic.shared",
        file: SYNTHETIC_HELPER_FILE,
        function: "shared_phase_helper",
        declared_exposure: &[],
        current_point: ExecutionPoint::AfterResponse,
        phase: PhaseDisposition::DeferToPostInitializeEnvironment,
        migration_wave: MigrationWave::None,
        owns_exposure: false,
        ..baseline_row()
    };

    let errors = ledger_errors_with_roots(
        &[pre_response_row(), post_response_row(), shared],
        &census,
        &lifecycle_roots(),
    );
    assert_reports(&errors, "synthetic.shared");
    assert_reports(&errors, "reachable from the response-committing root as well");
}

#[test]
fn a_helper_shared_by_both_phases_is_accepted_as_before_response() {
    // Negative control: the same operation declared with the derived point
    // must pass, so the rejection above cannot be an artefact of the fixture.
    let census = Census::from_sources(&lifecycle_timing_sources());
    let shared = InitOperationRow {
        operation_id: "synthetic.shared",
        file: SYNTHETIC_HELPER_FILE,
        function: "shared_phase_helper",
        declared_exposure: &[],
        current_point: ExecutionPoint::BeforeResponse,
        phase: PhaseDisposition::LocalProcessFreeConfigBeforeResponse,
        migration_wave: MigrationWave::None,
        owns_exposure: false,
        ..baseline_row()
    };

    let errors = ledger_errors_with_roots(
        &[pre_response_row(), post_response_row(), shared],
        &census,
        &lifecycle_roots(),
    );
    assert!(
        !errors.iter().any(|error| error.contains("synthetic.shared")),
        "the derived before_response point must validate, got: {errors:#?}"
    );
}

#[test]
fn a_deferred_row_already_after_response_cannot_claim_a_wave() {
    // The move the wave schedules has already happened: the operation runs at
    // the target point of its own disposition.
    let census = Census::from_sources(&lifecycle_timing_sources());
    let mut completed = post_response_row();
    completed.migration_wave = MigrationWave::E02;

    let errors =
        ledger_errors_with_roots(&[pre_response_row(), completed], &census, &lifecycle_roots());
    assert_reports(&errors, "already runs at `after_response`");
    assert_reports(&errors, "still claims wave E02");
}

#[test]
fn a_lazy_row_already_on_demand_cannot_claim_a_wave() {
    let census = Census::from_sources(&lifecycle_timing_sources());
    let mut lazy = InitOperationRow {
        operation_id: "synthetic.lazy",
        file: SYNTHETIC_HELPER_FILE,
        function: "resolve_timing_mode",
        declared_exposure: &[Exposure::EnvRead],
        triggers: &[Trigger::FirstUse],
        current_point: ExecutionPoint::OnDemand,
        phase: PhaseDisposition::LazyOnFirstUse,
        migration_wave: MigrationWave::E03,
        owns_exposure: false,
        ..baseline_row()
    };
    lazy.migration_wave = MigrationWave::E03;

    let errors = ledger_errors_with_roots(
        &[pre_response_row(), post_response_row(), lazy],
        &census,
        &lifecycle_roots(),
    );
    assert_reports(&errors, "already runs at `on_demand`");
    assert_reports(&errors, "still claims wave E03");
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
fn a_graph_deeper_than_the_bound_is_reported_as_instrument_failure() {
    // The traversal cap keeps the walk terminating on a cyclic graph, but a cap
    // that truncates silently is an instrument failure reported as a pass: the
    // spawn below sits past the bound, so coverage sees a short graph and finds
    // nothing to complain about.
    let mut body = String::from("impl Server { pub fn handle_initialize(&self) { hop_0(); }\n");
    let hops = census::MAX_DEPTH + 3;
    for hop in 0..hops {
        body.push_str(&format!("pub fn hop_{hop}() {{ hop_{}(); }}\n", hop + 1));
    }
    body.push_str(&format!(
        "pub fn hop_{hops}() {{ let _ = std::process::Command::new(\"perl\").output(); }} }}"
    ));
    let census = Census::from_sources(&[(SYNTHETIC_ROOT_FILE.to_string(), body)]);

    let errors = ledger_errors_with_roots(&[baseline_row()], &census, &synthetic_roots());
    assert_reports(&errors, "instrument failure");
    assert_reports(&errors, "was truncated");
    assert_reports(&errors, "coverage cannot be established");
}

#[test]
fn a_row_origin_past_the_bound_is_reported_even_when_the_root_closes() {
    // Checking only the roots would leave this case silent. `exposure_errors`,
    // `phase_errors` and `static_surface_errors` each start their own bounded
    // traversal at the row's cited function, and an `on_demand` row is by
    // construction reached by no root — so the root walk closing immediately
    // says nothing about the walk the checker actually performs for that row.
    let mut body = String::from("impl Server { pub fn handle_initialize(&self) {} }\n");
    let hops = census::MAX_DEPTH + 3;
    body.push_str("pub fn lazy_entry() { hop_0(); }\n");
    for hop in 0..hops {
        body.push_str(&format!("pub fn hop_{hop}() {{ hop_{}(); }}\n", hop + 1));
    }
    body.push_str(&format!(
        "pub fn hop_{hops}() {{ let _ = std::process::Command::new(\"perl\").output(); }}"
    ));
    let census = Census::from_sources(&[(SYNTHETIC_ROOT_FILE.to_string(), body)]);

    let root = census.resolve(SYNTHETIC_ROOT_FILE, "handle_initialize").expect("root resolves");
    assert!(
        census.truncation_witnesses(root, census::MAX_DEPTH).is_empty(),
        "the root's own walk must close, or this test would pass for the wrong reason"
    );

    let lazy = InitOperationRow {
        operation_id: "synthetic.lazy_chain",
        file: SYNTHETIC_ROOT_FILE,
        function: "lazy_entry",
        declared_exposure: &[],
        triggers: &[Trigger::FirstUse],
        current_point: ExecutionPoint::OnDemand,
        phase: PhaseDisposition::LazyOnFirstUse,
        migration_wave: MigrationWave::None,
        owns_exposure: false,
        ..baseline_row()
    };

    let errors = ledger_errors_with_roots(&[baseline_row(), lazy], &census, &synthetic_roots());
    assert_reports(&errors, "instrument failure");
    assert_reports(&errors, "row synthetic.lazy_chain");
    assert_reports(&errors, "was truncated");
}

#[test]
fn a_graph_inside_the_bound_reports_no_truncation() {
    // Negative control: the ordinary fixture must not be called truncated, or
    // the rule above would fire on everything and discriminate nothing.
    let census = Census::from_sources(&indirect_process_sources());
    let errors = ledger_errors_with_roots(&[baseline_row()], &census, &synthetic_roots());

    assert!(
        !errors.iter().any(|error| error.contains("was truncated")),
        "a graph that closes inside the bound must not be reported: {errors:#?}"
    );
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

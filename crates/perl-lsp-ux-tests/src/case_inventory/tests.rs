//! Deterministic fixture proof for `ux_case_inventory.v1` discovery.
//!
//! Every test here injects command output through [`UxDiscoveryCommands`]; none
//! of them compile anything or touch the filesystem. The negative controls
//! mirror the ten wrong implementations named by `#9890` one-for-one.

use super::*;
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

type TestResult = Result<(), Box<dyn std::error::Error>>;

const WORKSPACE_ROOT: &str = "/work space/perl-lsp";

// ── Fixture command source ───────────────────────────────────────────────

#[derive(Default)]
struct FixtureCommands {
    cargo_stdout: String,
    listings: BTreeMap<String, String>,
    digests: BTreeMap<String, String>,
    missing: BTreeSet<String>,
    failing_lists: BTreeSet<String>,
    failing_digests: BTreeSet<String>,
    /// Executables whose digest changes on every read, modelling a concurrent
    /// build replacing the binary mid-discovery.
    mutating_digests: BTreeSet<String>,
    digest_reads: RefCell<BTreeMap<String, usize>>,
}

impl FixtureCommands {
    fn new(cargo_stdout: impl Into<String>) -> Self {
        Self { cargo_stdout: cargo_stdout.into(), ..Self::default() }
    }

    fn listing(mut self, executable: &str, output: impl Into<String>) -> Self {
        self.listings.insert(executable.to_string(), output.into());
        self.digests.insert(executable.to_string(), sha256_hex(executable.as_bytes()));
        self
    }

    fn missing(mut self, executable: &str) -> Self {
        self.missing.insert(executable.to_string());
        self
    }

    fn failing_list(mut self, executable: &str) -> Self {
        self.failing_lists.insert(executable.to_string());
        self
    }

    fn failing_digest(mut self, executable: &str) -> Self {
        self.failing_digests.insert(executable.to_string());
        self
    }

    fn mutating_digest(mut self, executable: &str) -> Self {
        self.mutating_digests.insert(executable.to_string());
        self
    }
}

impl UxDiscoveryCommands for FixtureCommands {
    fn compile_test_targets(&self, _argv: &[String]) -> Result<String, UxDiscoveryFailure> {
        Ok(self.cargo_stdout.clone())
    }

    fn list_cases(
        &self,
        target_identity: &str,
        executable: &Path,
        argv: &[String],
    ) -> Result<String, UxDiscoveryFailure> {
        let key = executable.to_string_lossy().into_owned();
        if self.failing_lists.contains(&key) {
            return Err(UxDiscoveryFailure::ListCommandFailed {
                target: target_identity.to_string(),
                argv: argv.to_vec(),
                status: Some(101),
                detail: "fixture list failure".to_string(),
            });
        }
        self.listings.get(&key).cloned().ok_or_else(|| UxDiscoveryFailure::InstrumentFailure {
            reason: format!("fixture has no listing for `{key}`"),
        })
    }

    fn executable_digest(
        &self,
        target_identity: &str,
        executable: &Path,
    ) -> Result<String, UxDiscoveryFailure> {
        let key = executable.to_string_lossy().into_owned();
        if self.failing_digests.contains(&key) {
            return Err(UxDiscoveryFailure::DigestUnavailable {
                target: target_identity.to_string(),
                reason: "fixture digest failure".to_string(),
            });
        }
        if self.mutating_digests.contains(&key) {
            let mut reads = self.digest_reads.borrow_mut();
            let count = reads.entry(key.clone()).or_insert(0);
            *count += 1;
            return Ok(sha256_hex(format!("{key}#{count}").as_bytes()));
        }
        self.digests.get(&key).cloned().ok_or_else(|| UxDiscoveryFailure::InstrumentFailure {
            reason: format!("fixture has no digest for `{key}`"),
        })
    }

    fn executable_exists(&self, executable: &Path) -> bool {
        !self.missing.contains(&executable.to_string_lossy().into_owned())
    }
}

// ── Fixture builders ─────────────────────────────────────────────────────

fn artifact_line(
    package: &str,
    kind: &str,
    target: &str,
    executable: &str,
    features: &[&str],
) -> String {
    let features =
        features.iter().map(|feature| format!("\"{feature}\"")).collect::<Vec<_>>().join(",");
    format!(
        r#"{{"reason":"compiler-artifact","package_id":"path+file:///work space/perl-lsp/crates/{package}#0.1.0","target":{{"kind":["{kind}"],"crate_types":["bin"],"name":"{target}","src_path":"/work space/perl-lsp/crates/{package}/tests/{target}.rs"}},"profile":{{"opt_level":"0","debuginfo":2,"debug_assertions":true,"overflow_checks":true,"test":true}},"features":[{features}],"filenames":["{executable}"],"executable":"{executable}","fresh":false}}"#
    )
}

fn lib_artifact_line(package: &str, target: &str, executable: &str, features: &[&str]) -> String {
    let features =
        features.iter().map(|feature| format!("\"{feature}\"")).collect::<Vec<_>>().join(",");
    format!(
        r#"{{"reason":"compiler-artifact","package_id":"path+file:///work space/perl-lsp/crates/{package}#0.1.0","target":{{"kind":["lib"],"crate_types":["lib"],"name":"{target}","src_path":"/work space/perl-lsp/crates/{package}/src/lib.rs"}},"profile":{{"test":true}},"features":[{features}],"filenames":["{executable}"],"executable":"{executable}","fresh":false}}"#
    )
}

fn exe(name: &str) -> String {
    format!("{WORKSPACE_ROOT}/target/debug/deps/{name}.exe")
}

fn terse(cases: &[&str]) -> String {
    let mut out = String::new();
    for case in cases {
        out.push_str(case);
        out.push_str(": test\n");
    }
    out.push('\n');
    out.push_str(&format!("{} tests, 0 benchmarks\n", cases.len()));
    out
}

fn request(tier: UxCiTier) -> UxDiscoveryRequest {
    let mut request = UxDiscoveryRequest::new(tier, PathBuf::from(WORKSPACE_ROOT));
    request.repository_sha = Some("e78f2a2".to_string());
    request.repository_dirty_state = UxDirtyState::Clean;
    request.cargo_lock_digest = Some(sha256_hex(b"Cargo.lock"));
    request.package_manifest_digest = Some(sha256_hex(b"Cargo.toml"));
    request.rust_toolchain = "rustc 1.95.0 (deadbeef 2026-01-01)".to_string();
    request.host_target = "x86_64-unknown-linux-gnu".to_string();
    request
}

/// The representative healthy subject: one lib target and two integration
/// targets that share a numeric scenario prefix and a test name.
fn healthy_fixture() -> FixtureCommands {
    let lib = exe("perl_lsp_ux_tests-1111");
    let first = exe("ux_scenario_18_diagnostics_after_edit-2222");
    let second = exe("ux_scenario_18_goto_declaration-3333");
    let stdout = [
        lib_artifact_line("perl-lsp-ux-tests", "perl_lsp_ux_tests", &lib, &[]),
        artifact_line(
            "perl-lsp-ux-tests",
            "test",
            "ux_scenario_18_diagnostics_after_edit",
            &first,
            &[],
        ),
        artifact_line("perl-lsp-ux-tests", "test", "ux_scenario_18_goto_declaration", &second, &[]),
    ]
    .join("\n");

    FixtureCommands::new(stdout)
        .listing(&lib, terse(&["recorder::tests::receipt_round_trips", "taxonomy::tests::route"]))
        .listing(&first, terse(&["opens_document", "reports_diagnostics"]))
        .listing(&second, terse(&["opens_document", "resolves_declaration"]))
}

// ── Identity ─────────────────────────────────────────────────────────────

#[test]
fn case_id_round_trips_every_component() -> TestResult {
    let id = UxCaseId::new("perl-lsp-ux-tests", "test", "ux_scenario_01", "module::nested::case");
    let components = id.components().ok_or("case id should round-trip")?;
    assert_eq!(components[0], "perl-lsp-ux-tests");
    assert_eq!(components[1], "test");
    assert_eq!(components[2], "ux_scenario_01");
    assert_eq!(components[3], "module::nested::case");
    Ok(())
}

#[test]
fn case_id_separates_components_that_contain_colons() -> TestResult {
    // Without escaping, `a::b` + `c` and `a` + `b::c` collapse to one string.
    let left = UxCaseId::new("pkg", "test", "a::b", "c");
    let right = UxCaseId::new("pkg", "test", "a", "b::c");
    assert_ne!(left, right, "colon-bearing components must stay distinct");
    assert_eq!(left.components().ok_or("left round-trip")?[2], "a::b");
    assert_eq!(right.components().ok_or("right round-trip")?[3], "b::c");
    Ok(())
}

#[test]
fn case_id_escapes_are_themselves_escaped() -> TestResult {
    let id = UxCaseId::new("pkg", "test", "target", "literal%3Aname");
    assert_eq!(id.components().ok_or("round-trip")?[3], "literal%3Aname");
    Ok(())
}

// ── Profile contract ─────────────────────────────────────────────────────

#[test]
fn each_profile_declares_its_own_feature_population() {
    assert_eq!(profile_features(UxCiTier::Pr), &[] as &[&str]);
    assert_eq!(profile_features(UxCiTier::Nightly), &["integration-test"]);
    // Spelled out independently: widening nightly must not widen release.
    assert_eq!(profile_features(UxCiTier::Release), &["integration-test"]);
}

#[test]
fn unknown_profile_names_are_rejected() {
    let failure = parse_profile("staging").expect_err("unknown profile must fail closed");
    assert_eq!(failure.kind(), "unknown_profile");
}

#[test]
fn compile_argv_binds_the_selected_features() {
    let pr = compile_argv(UxCiTier::Pr);
    assert!(!pr.contains(&"--features".to_string()), "pr selects default features: {pr:?}");
    let nightly = compile_argv(UxCiTier::Nightly);
    assert!(
        nightly
            .windows(2)
            .any(|pair| pair == ["--features".to_string(), "integration-test".to_string()])
    );
}

// ── Cargo artifact parsing ───────────────────────────────────────────────

#[test]
fn discovers_library_unit_and_integration_targets_together() -> TestResult {
    let inventory = discover_cases(&healthy_fixture(), &request(UxCiTier::Pr))?;

    assert_eq!(inventory.schema, UX_CASE_INVENTORY_SCHEMA);
    assert_eq!(inventory.totals.target_count, 3);
    assert_eq!(inventory.totals.case_count, 6);

    let kinds: Vec<&UxTargetKind> =
        inventory.targets.iter().map(|target| &target.target_kind).collect();
    assert!(kinds.contains(&&UxTargetKind::Lib), "lib unit cases stay in the denominator");
    assert_eq!(
        kinds.iter().filter(|kind| **kind == &UxTargetKind::Test).count(),
        2,
        "both integration targets are present"
    );
    Ok(())
}

#[test]
fn nested_module_qualified_names_are_retained_verbatim() -> TestResult {
    let inventory = discover_cases(&healthy_fixture(), &request(UxCiTier::Pr))?;
    let names: BTreeSet<&str> = inventory
        .targets
        .iter()
        .flat_map(|target| target.cases.iter())
        .map(|case| case.test_name.as_str())
        .collect();
    assert!(names.contains("recorder::tests::receipt_round_trips"));
    assert!(names.contains("taxonomy::tests::route"));
    Ok(())
}

#[test]
fn ignored_cases_stay_in_the_denominator_and_the_gap_is_declared() -> TestResult {
    // libtest's terse listing prints `#[ignore]`d cases exactly like the rest.
    let executable = exe("ux_scenario_06_large_file-4444");
    let stdout =
        artifact_line("perl-lsp-ux-tests", "test", "ux_scenario_06_large_file", &executable, &[]);
    let commands = FixtureCommands::new(stdout)
        .listing(&executable, terse(&["fast_case", "ignored_slow_case"]));

    let inventory = discover_cases(&commands, &request(UxCiTier::Pr))?;
    assert_eq!(inventory.totals.case_count, 2, "ignored cases are part of the population");
    assert!(
        inventory.limitations.contains(&UxInventoryLimitation::IgnoreStateNotObservable),
        "the inventory must declare that it cannot see ignore state: {:?}",
        inventory.limitations
    );
    Ok(())
}

#[test]
fn duplicate_cargo_artifact_messages_describe_one_target() -> TestResult {
    let executable = exe("ux_scenario_01_simple_file-5555");
    let line =
        artifact_line("perl-lsp-ux-tests", "test", "ux_scenario_01_simple_file", &executable, &[]);
    let commands = FixtureCommands::new(format!("{line}\n{line}"))
        .listing(&executable, terse(&["opens_a_file"]));

    let inventory = discover_cases(&commands, &request(UxCiTier::Pr))?;
    assert_eq!(inventory.totals.target_count, 1);
    assert_eq!(inventory.totals.case_count, 1);
    Ok(())
}

#[test]
fn non_test_artifacts_and_non_json_progress_lines_are_ignored() -> TestResult {
    let executable = exe("ux_scenario_01_simple_file-6666");
    let stdout = [
        "   Compiling perl-lsp-ux-tests v0.1.0".to_string(),
        r#"{"reason":"build-script-executed","package_id":"path+file:///x/crates/perl-lsp-ux-tests#0.1.0"}"#.to_string(),
        r#"{"reason":"compiler-artifact","package_id":"path+file:///x/crates/perl-lsp-ux-tests#0.1.0","target":{"kind":["lib"],"name":"perl_lsp_ux_tests","src_path":"/x/src/lib.rs"},"profile":{"test":false},"features":[],"filenames":["/x/libperl_lsp_ux_tests.rlib"],"executable":null}"#.to_string(),
        artifact_line("perl-lsp-ux-tests", "test", "ux_scenario_01_simple_file", &executable, &[]),
    ]
    .join("\n");
    let commands = FixtureCommands::new(stdout).listing(&executable, terse(&["opens_a_file"]));

    let inventory = discover_cases(&commands, &request(UxCiTier::Pr))?;
    assert_eq!(inventory.totals.target_count, 1, "only test-profile executables count");
    Ok(())
}

#[test]
fn package_ids_parse_from_every_cargo_spelling() -> TestResult {
    let directory_named =
        parse_package_id("path+file:///x/crates/perl-lsp-ux-tests#0.1.0").ok_or("parse")?;
    assert_eq!(directory_named.name, "perl-lsp-ux-tests");
    assert_eq!(directory_named.version.as_deref(), Some("0.1.0"));
    assert_eq!(directory_named.source, UxPackageSource::WorkspacePath);

    let explicitly_named =
        parse_package_id("path+file:///x/crates/renamed-dir#perl-lsp-ux-tests@0.1.0")
            .ok_or("parse")?;
    assert_eq!(explicitly_named.name, "perl-lsp-ux-tests");
    assert_eq!(explicitly_named.version.as_deref(), Some("0.1.0"));

    let legacy = parse_package_id("perl-lsp-ux-tests 0.1.0 (path+file:///x)").ok_or("parse")?;
    assert_eq!(legacy.name, "perl-lsp-ux-tests");
    assert_eq!(legacy.version.as_deref(), Some("0.1.0"));
    assert_eq!(legacy.source, UxPackageSource::WorkspacePath);

    let registry =
        parse_package_id("registry+https://github.com/rust-lang/crates.io-index#serde@1.0.0")
            .ok_or("parse")?;
    assert_eq!(registry.source, UxPackageSource::Registry);
    Ok(())
}

// ── libtest list parsing ─────────────────────────────────────────────────

#[test]
fn list_output_accepts_tests_and_benchmarks() -> TestResult {
    let listed = parse_libtest_list(
        "t",
        "alpha: test\nbeta::nested: test\ngamma: benchmark\n\n2 tests, 1 benchmark\n",
    )?;
    assert_eq!(listed.len(), 3);
    assert_eq!(listed[2].kind, UxCaseKind::Benchmark);
    Ok(())
}

#[test]
fn list_output_accepts_the_singular_summary_spelling() -> TestResult {
    let listed = parse_libtest_list("t", "alpha: test\n\n1 test, 0 benchmarks\n")?;
    assert_eq!(listed.len(), 1);
    Ok(())
}

#[test]
fn zero_case_targets_are_recorded_rather_than_hidden() -> TestResult {
    let empty = exe("ux_scenario_06_large_file-7777");
    let populated = exe("ux_scenario_01_simple_file-8888");
    let stdout = [
        artifact_line("perl-lsp-ux-tests", "test", "ux_scenario_06_large_file", &empty, &[]),
        artifact_line("perl-lsp-ux-tests", "test", "ux_scenario_01_simple_file", &populated, &[]),
    ]
    .join("\n");
    let commands = FixtureCommands::new(stdout)
        .listing(&empty, "\n0 tests, 0 benchmarks\n")
        .listing(&populated, terse(&["opens_a_file"]));

    let inventory = discover_cases(&commands, &request(UxCiTier::Pr))?;
    assert_eq!(inventory.totals.target_count, 2);
    assert_eq!(inventory.totals.zero_case_target_count, 1);
    assert_eq!(inventory.zero_case_targets.len(), 1);
    assert!(
        inventory.limitations.contains(&UxInventoryLimitation::ZeroCaseTargetPresent),
        "a zero-case target must be a declared limitation, not silence"
    );
    Ok(())
}

// ── Negative controls (#9890 list, one test per control) ─────────────────

#[test]
fn control_01_case_population_comes_from_executables_not_source_files() -> TestResult {
    // The discovery algorithm has no filesystem access at all: the only case
    // names it can produce are the ones an executable listed. A target whose
    // source file suggests many scenarios but whose executable lists one case
    // yields exactly one case.
    let executable = exe("ux_scenario_14_inc_conformance-9999");
    let stdout = artifact_line(
        "perl-lsp-ux-tests",
        "test",
        "ux_scenario_14_inc_conformance",
        &executable,
        &[],
    );
    let commands =
        FixtureCommands::new(stdout).listing(&executable, terse(&["only_case_in_the_binary"]));

    let inventory = discover_cases(&commands, &request(UxCiTier::Pr))?;
    assert_eq!(inventory.totals.case_count, 1);
    assert_eq!(inventory.targets[0].cases[0].test_name, "only_case_in_the_binary");
    Ok(())
}

#[test]
fn control_02_numeric_scenario_prefix_is_display_only() -> TestResult {
    let inventory = discover_cases(&healthy_fixture(), &request(UxCiTier::Pr))?;

    let prefixed: Vec<&UxTargetInventory> = inventory
        .targets
        .iter()
        .filter(|target| target.target_name.starts_with("ux_scenario_18_"))
        .collect();
    assert_eq!(prefixed.len(), 2, "two targets share prefix 18");

    for target in &prefixed {
        for case in &target.cases {
            assert_eq!(
                case.display.scenario_prefix.as_deref(),
                Some("18"),
                "the colliding prefix is still exposed as display metadata"
            );
            assert!(
                !case.case_id.as_str().starts_with("18"),
                "identity must not be derived from the prefix: {}",
                case.case_id
            );
        }
    }

    let ids: BTreeSet<&str> = inventory
        .targets
        .iter()
        .flat_map(|target| target.cases.iter())
        .map(|case| case.case_id.as_str())
        .collect();
    assert_eq!(ids.len(), inventory.totals.case_count, "prefix collision must not merge cases");
    Ok(())
}

#[test]
fn control_03_same_test_name_in_two_targets_stays_two_cases() -> TestResult {
    let inventory = discover_cases(&healthy_fixture(), &request(UxCiTier::Pr))?;

    let shared: Vec<&UxCase> = inventory
        .targets
        .iter()
        .flat_map(|target| target.cases.iter())
        .filter(|case| case.test_name == "opens_document")
        .collect();
    assert_eq!(shared.len(), 2, "the same name exists in two targets");
    assert_ne!(shared[0].case_id, shared[1].case_id, "and keeps two identities");
    Ok(())
}

#[test]
fn control_04_missing_executable_is_not_an_empty_target() -> TestResult {
    let executable = exe("ux_scenario_01_simple_file-aaaa");
    let stdout =
        artifact_line("perl-lsp-ux-tests", "test", "ux_scenario_01_simple_file", &executable, &[]);
    let commands = FixtureCommands::new(stdout)
        .listing(&executable, terse(&["opens_a_file"]))
        .missing(&executable);

    let failure =
        discover_cases(&commands, &request(UxCiTier::Pr)).expect_err("missing binary must fail");
    assert_eq!(failure.kind(), "test_artifact_missing");
    Ok(())
}

#[test]
fn control_05_malformed_list_output_is_not_zero_tests() -> TestResult {
    let executable = exe("ux_scenario_01_simple_file-bbbb");
    let stdout =
        artifact_line("perl-lsp-ux-tests", "test", "ux_scenario_01_simple_file", &executable, &[]);

    // A changed/foreign list format.
    let commands = FixtureCommands::new(stdout.clone())
        .listing(&executable, "opens_a_file (line 12)\n\n1 tests, 0 benchmarks\n");
    let failure = discover_cases(&commands, &request(UxCiTier::Pr))
        .expect_err("unknown line shape must fail closed");
    assert_eq!(failure.kind(), "malformed_list_output");

    // A truncated capture with no summary at all.
    let commands =
        FixtureCommands::new(stdout.clone()).listing(&executable, "opens_a_file: test\n");
    let failure = discover_cases(&commands, &request(UxCiTier::Pr))
        .expect_err("a missing summary must fail closed");
    assert_eq!(failure.kind(), "missing_list_summary");

    // A summary that disagrees with the listing.
    let commands = FixtureCommands::new(stdout)
        .listing(&executable, "opens_a_file: test\n\n4 tests, 0 benchmarks\n");
    let failure = discover_cases(&commands, &request(UxCiTier::Pr))
        .expect_err("a count mismatch must fail closed");
    assert_eq!(failure.kind(), "list_count_mismatch");
    Ok(())
}

#[test]
fn control_06_feature_movement_moves_inventory_identity() -> TestResult {
    let executable = exe("ux_scenario_06_large_file-cccc");
    let pr_stdout =
        artifact_line("perl-lsp-ux-tests", "test", "ux_scenario_06_large_file", &executable, &[]);
    let nightly_stdout = artifact_line(
        "perl-lsp-ux-tests",
        "test",
        "ux_scenario_06_large_file",
        &executable,
        &["integration-test"],
    );
    let listing = terse(&["opens_a_file"]);

    let pr = discover_cases(
        &FixtureCommands::new(pr_stdout).listing(&executable, listing.clone()),
        &request(UxCiTier::Pr),
    )?;
    let nightly = discover_cases(
        &FixtureCommands::new(nightly_stdout).listing(&executable, listing),
        &request(UxCiTier::Nightly),
    )?;

    assert_eq!(pr.totals.case_count, nightly.totals.case_count, "same cases, different subject");
    assert_ne!(
        pr.subject.subject_digest, nightly.subject.subject_digest,
        "a feature-population change must move the subject identity"
    );
    assert_ne!(
        pr.inventory_digest, nightly.inventory_digest,
        "a feature-population change must move the inventory identity"
    );
    Ok(())
}

#[test]
fn control_07_stale_wrong_profile_executable_is_rejected() -> TestResult {
    let executable = exe("ux_scenario_06_large_file-dddd");
    // Cargo reports an artifact built with `integration-test` while the `pr`
    // profile selected the default feature population.
    let stdout = artifact_line(
        "perl-lsp-ux-tests",
        "test",
        "ux_scenario_06_large_file",
        &executable,
        &["integration-test"],
    );
    let commands = FixtureCommands::new(stdout).listing(&executable, terse(&["opens_a_file"]));

    let failure = discover_cases(&commands, &request(UxCiTier::Pr))
        .expect_err("a wrong-profile artifact must fail closed");
    assert_eq!(failure.kind(), "wrong_profile_artifact");
    Ok(())
}

#[test]
fn control_08_shuffled_cargo_and_case_order_produces_identical_bytes() -> TestResult {
    let lib = exe("perl_lsp_ux_tests-1111");
    let first = exe("ux_scenario_18_diagnostics_after_edit-2222");
    let second = exe("ux_scenario_18_goto_declaration-3333");

    let forward = healthy_fixture();
    let reversed = FixtureCommands::new(
        [
            artifact_line(
                "perl-lsp-ux-tests",
                "test",
                "ux_scenario_18_goto_declaration",
                &second,
                &[],
            ),
            artifact_line(
                "perl-lsp-ux-tests",
                "test",
                "ux_scenario_18_diagnostics_after_edit",
                &first,
                &[],
            ),
            lib_artifact_line("perl-lsp-ux-tests", "perl_lsp_ux_tests", &lib, &[]),
        ]
        .join("\n"),
    )
    .listing(&lib, terse(&["taxonomy::tests::route", "recorder::tests::receipt_round_trips"]))
    .listing(&first, terse(&["reports_diagnostics", "opens_document"]))
    .listing(&second, terse(&["resolves_declaration", "opens_document"]));

    let left = discover_cases(&forward, &request(UxCiTier::Pr))?;
    let right = discover_cases(&reversed, &request(UxCiTier::Pr))?;

    assert_eq!(left.durable_bytes()?, right.durable_bytes()?, "order must not change the bytes");
    assert_eq!(left.inventory_digest, right.inventory_digest);
    Ok(())
}

#[test]
fn control_09_other_packages_never_enter_the_denominator() -> TestResult {
    let ours = exe("ux_scenario_01_simple_file-eeee");
    let theirs = exe("lsp_code_actions-ffff");
    let stdout = [
        artifact_line("perl-lsp-ux-tests", "test", "ux_scenario_01_simple_file", &ours, &[]),
        artifact_line("perl-lsp-rs", "test", "lsp_code_actions", &theirs, &[]),
    ]
    .join("\n");
    let commands = FixtureCommands::new(stdout)
        .listing(&ours, terse(&["opens_a_file"]))
        .listing(&theirs, terse(&["unrelated_case_a", "unrelated_case_b"]));

    let inventory = discover_cases(&commands, &request(UxCiTier::Pr))?;
    assert_eq!(inventory.totals.target_count, 1);
    assert_eq!(inventory.totals.case_count, 1);
    assert!(
        inventory
            .targets
            .iter()
            .all(|target| target.target_identity.starts_with("perl-lsp-ux-tests::")),
        "only the UX package may form the denominator"
    );
    Ok(())
}

#[test]
fn control_10_a_different_runners_output_cannot_pass_as_a_listing() -> TestResult {
    let executable = exe("ux_scenario_01_simple_file-0001");
    let stdout =
        artifact_line("perl-lsp-ux-tests", "test", "ux_scenario_01_simple_file", &executable, &[]);
    // `cargo nextest list` prints a target header plus indented case names and
    // no libtest summary. Silently accepting it would substitute one runner's
    // denominator for another's.
    let nextest = "  ux_scenario_01_simple_file:\n    opens_a_file\n";
    let commands = FixtureCommands::new(stdout).listing(&executable, nextest);

    let failure = discover_cases(&commands, &request(UxCiTier::Pr))
        .expect_err("foreign runner output must fail closed");
    assert_eq!(failure.kind(), "malformed_list_output");
    Ok(())
}

// ── Remaining failure semantics ──────────────────────────────────────────

#[test]
fn cargo_reporting_no_test_artifacts_fails_closed() -> TestResult {
    let commands = FixtureCommands::new("   Finished test profile\n");
    let failure = discover_cases(&commands, &request(UxCiTier::Pr))
        .expect_err("an empty artifact set must fail closed");
    assert_eq!(failure.kind(), "no_test_artifacts");
    Ok(())
}

#[test]
fn conflicting_executables_for_one_target_fail_closed() -> TestResult {
    let first = exe("ux_scenario_01_simple_file-0002");
    let second = exe("ux_scenario_01_simple_file-0003");
    let stdout = [
        artifact_line("perl-lsp-ux-tests", "test", "ux_scenario_01_simple_file", &first, &[]),
        artifact_line("perl-lsp-ux-tests", "test", "ux_scenario_01_simple_file", &second, &[]),
    ]
    .join("\n");
    let commands = FixtureCommands::new(stdout).listing(&first, terse(&["opens_a_file"]));

    let failure = discover_cases(&commands, &request(UxCiTier::Pr))
        .expect_err("two executables for one target must fail closed");
    assert_eq!(failure.kind(), "duplicate_artifact");
    Ok(())
}

#[test]
fn a_malformed_artifact_message_fails_closed() -> TestResult {
    let stdout = r#"{"reason":"compiler-artifact","package_id":"path+file:///x/crates/perl-lsp-ux-tests#0.1.0","target":{"kind":["test"],"src_path":"/x/tests/a.rs"},"profile":{"test":true},"features":[],"executable":"/x/a"}"#;
    let failure = parse_cargo_test_artifacts(stdout, UX_INVENTORY_PACKAGE)
        .expect_err("an artifact with no target name must fail closed");
    assert_eq!(failure.kind(), "malformed_cargo_message");
    Ok(())
}

#[test]
fn a_failing_list_invocation_is_not_an_empty_target() -> TestResult {
    let executable = exe("ux_scenario_01_simple_file-0004");
    let stdout =
        artifact_line("perl-lsp-ux-tests", "test", "ux_scenario_01_simple_file", &executable, &[]);
    let commands = FixtureCommands::new(stdout)
        .listing(&executable, terse(&["opens_a_file"]))
        .failing_list(&executable);

    let failure = discover_cases(&commands, &request(UxCiTier::Pr))
        .expect_err("a failed list invocation must fail closed");
    assert_eq!(failure.kind(), "list_command_failed");
    Ok(())
}

#[test]
fn an_undigestable_executable_fails_closed() -> TestResult {
    let executable = exe("ux_scenario_01_simple_file-0005");
    let stdout =
        artifact_line("perl-lsp-ux-tests", "test", "ux_scenario_01_simple_file", &executable, &[]);
    let commands = FixtureCommands::new(stdout)
        .listing(&executable, terse(&["opens_a_file"]))
        .failing_digest(&executable);

    let failure = discover_cases(&commands, &request(UxCiTier::Pr))
        .expect_err("an undigestable executable must fail closed");
    assert_eq!(failure.kind(), "digest_unavailable");
    Ok(())
}

#[test]
fn a_target_that_lists_one_name_twice_fails_closed() -> TestResult {
    let executable = exe("ux_scenario_01_simple_file-0006");
    let stdout =
        artifact_line("perl-lsp-ux-tests", "test", "ux_scenario_01_simple_file", &executable, &[]);
    let commands = FixtureCommands::new(stdout)
        .listing(&executable, "opens_a_file: test\nopens_a_file: test\n\n2 tests, 0 benchmarks\n");

    let failure = discover_cases(&commands, &request(UxCiTier::Pr))
        .expect_err("a colliding case id must fail closed");
    assert_eq!(failure.kind(), "duplicate_case_id");
    Ok(())
}

// ── Durable projection ───────────────────────────────────────────────────

#[test]
fn paths_with_spaces_and_executable_suffixes_normalize_to_a_workspace_role() -> TestResult {
    let inventory = discover_cases(&healthy_fixture(), &request(UxCiTier::Pr))?;
    for target in &inventory.targets {
        assert_eq!(target.executable.role, UxExecutableRole::WorkspaceTarget);
        let relative = target
            .executable
            .workspace_relative_path
            .as_deref()
            .ok_or("a workspace executable must expose a relative path")?;
        assert!(relative.starts_with("target/debug/deps/"), "unexpected role path: {relative}");
        assert!(!relative.contains("/work space"), "no absolute path may leak: {relative}");
        assert!(target.executable.file_name.ends_with(".exe"));
        assert!(target.executable.digest.starts_with("sha256:"));
    }
    Ok(())
}

#[test]
fn absolute_paths_and_timestamps_stay_out_of_the_durable_projection() -> TestResult {
    let mut with_local = request(UxCiTier::Pr);
    with_local.include_local_execution = true;
    with_local.generated_at = Some("2026-08-29T20:00:00Z".to_string());

    let local = discover_cases(&healthy_fixture(), &with_local)?;
    let durable_only = discover_cases(&healthy_fixture(), &request(UxCiTier::Pr))?;

    let local_execution =
        local.local_execution.as_ref().ok_or("the local section was requested")?;
    assert_eq!(local_execution.generated_at.as_deref(), Some("2026-08-29T20:00:00Z"));
    assert!(!local_execution.target_executables.is_empty());

    assert_eq!(
        local.durable_bytes()?,
        durable_only.durable_bytes()?,
        "machine-local detail must not move the durable projection"
    );
    assert_eq!(local.inventory_digest, durable_only.inventory_digest);

    let serialized = serde_json::to_string(&local.durable_projection()?)?;
    assert!(!serialized.contains("2026-08-29T20:00:00Z"), "no timestamp in the durable projection");
    assert!(!serialized.contains(WORKSPACE_ROOT), "no absolute checkout path either");
    // The Cargo package id embeds the absolute checkout path in whatever
    // encoding Cargo chose, so the locator itself must be absent rather than
    // merely unrecognizable.
    assert!(!serialized.contains("path+file:"), "no Cargo locator in the durable projection");
    assert!(!serialized.contains("file://"), "no file URL in the durable projection");
    assert_eq!(local.subject.package_source, UxPackageSource::WorkspacePath);
    assert_eq!(local.subject.package_version.as_deref(), Some("0.1.0"));
    assert_eq!(
        local_execution.package_id.as_deref(),
        Some("path+file:///work space/perl-lsp/crates/perl-lsp-ux-tests#0.1.0"),
        "the raw locator is retained, but only in the machine-local section"
    );
    Ok(())
}

#[test]
fn the_recorded_digest_verifies_and_a_tampered_row_breaks_it() -> TestResult {
    let mut inventory = discover_cases(&healthy_fixture(), &request(UxCiTier::Pr))?;
    inventory.verify_digest()?;

    // Shrinking the denominator without re-deriving the digest must be visible.
    inventory.targets.truncate(1);
    let failure = inventory.verify_digest().expect_err("a tampered inventory must not verify");
    assert_eq!(failure.kind(), "instrument_failure");
    Ok(())
}

#[test]
fn totals_are_derived_from_the_rows_rather_than_asserted() -> TestResult {
    let inventory = discover_cases(&healthy_fixture(), &request(UxCiTier::Pr))?;
    let summed: usize = inventory.targets.iter().map(|target| target.cases.len()).sum();
    assert_eq!(inventory.totals.case_count, summed);
    assert_eq!(inventory.totals.target_count, inventory.targets.len());
    for target in &inventory.targets {
        assert_eq!(
            inventory.totals.cases_per_target.get(&target.target_identity),
            Some(&target.cases.len())
        );
    }
    Ok(())
}

#[test]
fn canonical_replay_records_the_exact_commands() -> TestResult {
    let inventory = discover_cases(&healthy_fixture(), &request(UxCiTier::Pr))?;
    assert_eq!(inventory.canonical_replay.compile_argv, compile_argv(UxCiTier::Pr));
    // Not `--format terse`: terse omits the `N tests, M benchmarks` summary
    // that `missing_list_summary` depends on.
    assert_eq!(inventory.canonical_replay.list_argv_suffix, vec!["--list"]);
    assert!(
        !LIST_ARGV_SUFFIX.contains(&"terse"),
        "the terse list format drops the completeness cross-check"
    );
    for target in &inventory.targets {
        assert!(target.list_argv.ends_with(&["--list".to_string()]));
        assert_eq!(
            target.list_argv.first().map(String::as_str),
            target.executable.workspace_relative_path.as_deref(),
            "replay must name the executable by its durable role"
        );
    }
    Ok(())
}

#[test]
fn the_inventory_survives_a_json_round_trip() -> TestResult {
    let inventory = discover_cases(&healthy_fixture(), &request(UxCiTier::Pr))?;
    let encoded = serde_json::to_string(&inventory)?;
    let decoded: UxCaseInventory = serde_json::from_str(&encoded)?;
    assert_eq!(decoded, inventory);
    decoded.verify_digest()?;
    Ok(())
}

// ── Guards added after review of #13878 ──────────────────────────────────

#[test]
fn a_malformed_feature_population_cannot_become_an_exact_empty_one() -> TestResult {
    let executable = exe("ux_scenario_01_simple_file-1001");
    // The `pr` profile selects the empty feature set, so an artifact whose
    // features are unknown would otherwise be accepted as an exact match.
    let missing = format!(
        r#"{{"reason":"compiler-artifact","package_id":"path+file:///x/crates/perl-lsp-ux-tests#0.1.0","target":{{"kind":["test"],"name":"ux_scenario_01_simple_file","src_path":"/x/tests/a.rs"}},"profile":{{"test":true}},"executable":"{executable}"}}"#
    );
    let failure = parse_cargo_test_artifacts(&missing, UX_INVENTORY_PACKAGE)
        .expect_err("a missing features array must fail closed");
    assert_eq!(failure.kind(), "malformed_cargo_message");

    let non_array = missing.replace(r#""executable""#, r#""features":{},"executable""#);
    let failure = parse_cargo_test_artifacts(&non_array, UX_INVENTORY_PACKAGE)
        .expect_err("a non-array features value must fail closed");
    assert_eq!(failure.kind(), "malformed_cargo_message");

    let non_string = missing.replace(r#""executable""#, r#""features":["ok",7],"executable""#);
    let failure = parse_cargo_test_artifacts(&non_string, UX_INVENTORY_PACKAGE)
        .expect_err("a non-string feature entry must fail closed");
    assert_eq!(failure.kind(), "malformed_cargo_message");
    Ok(())
}

#[test]
fn a_summary_that_contradicts_the_case_kinds_fails_closed() -> TestResult {
    // Combined totals agree (1 == 1); only a per-kind comparison catches this.
    let failure = parse_libtest_list("t", "foo: benchmark\n\n1 test, 0 benchmarks\n")
        .expect_err("a contradictory kind count must fail closed");
    assert_eq!(failure.kind(), "list_count_mismatch");

    let failure = parse_libtest_list("t", "foo: test\n\n0 tests, 1 benchmark\n")
        .expect_err("the opposite contradiction must also fail closed");
    assert_eq!(failure.kind(), "list_count_mismatch");

    // The honest pairing still parses.
    let listed = parse_libtest_list("t", "foo: benchmark\n\n0 tests, 1 benchmark\n")?;
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].kind, UxCaseKind::Benchmark);
    Ok(())
}

#[test]
fn an_executable_replaced_between_digest_and_listing_fails_closed() -> TestResult {
    let executable = exe("ux_scenario_01_simple_file-1002");
    let stdout =
        artifact_line("perl-lsp-ux-tests", "test", "ux_scenario_01_simple_file", &executable, &[]);
    let commands = FixtureCommands::new(stdout)
        .listing(&executable, terse(&["opens_a_file"]))
        .mutating_digest(&executable);

    let failure = discover_cases(&commands, &request(UxCiTier::Pr))
        .expect_err("a binary swapped mid-discovery must fail closed");
    assert_eq!(failure.kind(), "executable_changed_during_discovery");
    Ok(())
}

#[test]
fn an_external_cargo_target_dir_keeps_a_runnable_replay() -> TestResult {
    // `.cargo/config.local.toml.example` supports a target dir outside the
    // checkout; the durable replay must still locate the executable.
    let target_root = "/var/cache/cargo-target";
    let executable = format!("{target_root}/debug/deps/ux_scenario_01_simple_file-1003.exe");
    let stdout =
        artifact_line("perl-lsp-ux-tests", "test", "ux_scenario_01_simple_file", &executable, &[]);
    let commands = FixtureCommands::new(stdout).listing(&executable, terse(&["opens_a_file"]));

    let mut req = request(UxCiTier::Pr);
    req.cargo_target_root = Some(PathBuf::from(target_root));
    let inventory = discover_cases(&commands, &req)?;

    let target = inventory.targets.first().ok_or("one target")?;
    assert_eq!(target.executable.role, UxExecutableRole::CargoTargetDir);
    assert_eq!(
        target.executable.target_dir_relative_path.as_deref(),
        Some("debug/deps/ux_scenario_01_simple_file-1003.exe")
    );
    assert_eq!(
        target.list_argv.first().map(String::as_str),
        Some("${CARGO_TARGET_DIR}/debug/deps/ux_scenario_01_simple_file-1003.exe"),
        "the replay must remain runnable via $CARGO_TARGET_DIR"
    );
    assert!(
        !inventory.limitations.contains(&UxInventoryLimitation::ReplayNotSelfContained),
        "a target-dir-relative replay is self-contained"
    );
    let serialized = serde_json::to_string(&inventory.durable_projection()?)?;
    assert!(!serialized.contains(target_root), "no absolute target root in the projection");
    Ok(())
}

#[test]
fn an_executable_under_neither_root_declares_an_unrunnable_replay() -> TestResult {
    let executable = "/somewhere/else/ux_scenario_01_simple_file-1004.exe".to_string();
    let stdout =
        artifact_line("perl-lsp-ux-tests", "test", "ux_scenario_01_simple_file", &executable, &[]);
    let commands = FixtureCommands::new(stdout).listing(&executable, terse(&["opens_a_file"]));

    let inventory = discover_cases(&commands, &request(UxCiTier::Pr))?;
    let target = inventory.targets.first().ok_or("one target")?;
    assert_eq!(target.executable.role, UxExecutableRole::OutsideWorkspace);
    assert!(
        inventory.limitations.contains(&UxInventoryLimitation::ReplayNotSelfContained),
        "an unlocatable executable must declare that its replay is not self-contained"
    );
    Ok(())
}

// ── Guards added after the second review round of #13878 ─────────────────

#[test]
fn an_unparseable_cargo_json_object_cannot_silently_drop_a_target() -> TestResult {
    // A truncated `compiler-artifact` line would otherwise vanish, shrinking
    // the denominator by exactly the target it described.
    let truncated = r#"{"reason":"compiler-artifact","package_id":"path+file:///x/crates/perl-lsp-ux-tests#0.1.0","target":{"kind":["test"]"#;
    let failure = parse_cargo_test_artifacts(truncated, UX_INVENTORY_PACKAGE)
        .expect_err("a broken JSON object must fail closed");
    assert_eq!(failure.kind(), "malformed_cargo_message");

    // Human progress output is still tolerated: it is not a message.
    let executable = exe("ux_scenario_01_simple_file-2001");
    let stdout = format!(
        "   Compiling perl-lsp-ux-tests v0.1.0\n{}",
        artifact_line("perl-lsp-ux-tests", "test", "ux_scenario_01_simple_file", &executable, &[])
    );
    assert_eq!(parse_cargo_test_artifacts(&stdout, UX_INVENTORY_PACKAGE)?.len(), 1);
    Ok(())
}

#[test]
fn duplicate_artifacts_for_one_executable_must_agree_in_full() -> TestResult {
    let executable = exe("ux_scenario_01_simple_file-2002");
    let first =
        artifact_line("perl-lsp-ux-tests", "test", "ux_scenario_01_simple_file", &executable, &[]);
    let second = artifact_line(
        "perl-lsp-ux-tests",
        "test",
        "ux_scenario_01_simple_file",
        &executable,
        &["integration-test"],
    );

    // Identical repeats remain one fact.
    assert_eq!(
        parse_cargo_test_artifacts(&format!("{first}\n{first}"), UX_INVENTORY_PACKAGE)?.len(),
        1
    );

    // Contradictory repeats must not resolve by arrival order.
    let failure = parse_cargo_test_artifacts(&format!("{first}\n{second}"), UX_INVENTORY_PACKAGE)
        .expect_err("contradictory messages must fail closed");
    assert_eq!(failure.kind(), "contradictory_artifact");
    let reversed = parse_cargo_test_artifacts(&format!("{second}\n{first}"), UX_INVENTORY_PACKAGE)
        .expect_err("and in the other order too");
    assert_eq!(reversed.kind(), "contradictory_artifact");
    Ok(())
}

#[test]
fn a_malformed_case_id_is_rejected_on_deserialization() -> TestResult {
    // Too few components.
    assert!(serde_json::from_str::<UxCaseId>(r#""pkg::test::target""#).is_err());
    // An escape that decodes to nothing valid.
    assert!(serde_json::from_str::<UxCaseId>(r#""pkg::test::target::a%ZZb""#).is_err());
    // A well-formed identity still round-trips.
    let id = UxCaseId::new("perl-lsp-ux-tests", "test", "t", "module::case");
    let decoded: UxCaseId = serde_json::from_str(&serde_json::to_string(&id)?)?;
    assert_eq!(decoded, id);
    Ok(())
}

#[test]
fn the_implicit_package_id_form_derives_its_name_from_the_directory() -> TestResult {
    // Pins the documented constraint: Cargo emits this form only when package
    // name and directory name agree, and the parser cannot detect a mismatch.
    // The package filter in `parse_cargo_test_artifacts` is what keeps a
    // wrongly-named target out of the denominator.
    let mismatched = parse_package_id("path+file:///x/crates/renamed-dir#0.1.0").ok_or("parse")?;
    assert_eq!(mismatched.name, "renamed-dir");

    let executable = exe("ux_scenario_01_simple_file-2003");
    let stdout = format!(
        r#"{{"reason":"compiler-artifact","package_id":"path+file:///x/crates/renamed-dir#0.1.0","target":{{"kind":["test"],"name":"ux_scenario_01_simple_file","src_path":"/x/tests/a.rs"}},"profile":{{"test":true}},"features":[],"executable":"{executable}"}}"#
    );
    assert!(
        parse_cargo_test_artifacts(&stdout, UX_INVENTORY_PACKAGE)?.is_empty(),
        "a directory-derived name that is not the UX package must be filtered out"
    );
    Ok(())
}

#[test]
fn unknown_subject_facts_are_declared_rather_than_passed_off_as_known() -> TestResult {
    let executable = exe("ux_scenario_01_simple_file-3001");
    let stdout =
        artifact_line("perl-lsp-ux-tests", "test", "ux_scenario_01_simple_file", &executable, &[]);
    let commands = FixtureCommands::new(stdout).listing(&executable, terse(&["opens_a_file"]));

    // A request where every optional probe failed — the shape `xtask` produces
    // when git, rustc, or the manifest reads are unavailable.
    let bare = UxDiscoveryRequest::new(UxCiTier::Pr, PathBuf::from(WORKSPACE_ROOT));
    let inventory = discover_cases(&commands, &bare)?;

    for expected in [
        UxInventoryLimitation::RepositoryShaUnknown,
        UxInventoryLimitation::RepositoryDirtyStateUnknown,
        UxInventoryLimitation::RustToolchainUnknown,
        UxInventoryLimitation::HostTargetUnknown,
        UxInventoryLimitation::CargoLockDigestUnknown,
        UxInventoryLimitation::PackageManifestDigestUnknown,
    ] {
        assert!(
            inventory.limitations.contains(&expected),
            "{expected:?} must be declared; limitations were {:?}",
            inventory.limitations
        );
    }

    // A fully-probed subject declares none of them.
    let complete = discover_cases(&commands, &request(UxCiTier::Pr))?;
    for absent in [
        UxInventoryLimitation::RustToolchainUnknown,
        UxInventoryLimitation::HostTargetUnknown,
        UxInventoryLimitation::CargoLockDigestUnknown,
        UxInventoryLimitation::PackageManifestDigestUnknown,
    ] {
        assert!(!complete.limitations.contains(&absent), "{absent:?} must not be declared");
    }
    Ok(())
}

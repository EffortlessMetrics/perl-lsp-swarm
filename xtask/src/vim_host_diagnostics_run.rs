//! #10946 bootstrap/diagnostics scenario for the hermetic Vim + vim-lsp host
//! runner.
//!
//! This module is the first execution consumer of the #10944/#12545
//! substrate: it proves the four behavior-bearing cells — exact bootstrap,
//! #7762 native root selection (with wrong-root discrimination), the
//! diagnostics lifecycle (governed defect appears through the client's own
//! diagnostics state, a real buffer edit clears it, currentness is barriered
//! rather than slept), and baseline cleanup — through the pinned actual Vim +
//! vim-lsp + perllsp subject.
//!
//! Ownership split (consumed, never duplicated):
//!
//! - `vim_host_run::vim_host_runner` (#10944) owns hermetic launch,
//!   supervision, process ledgers, cleanup comparison, wire mining, and
//!   receipt composition. This module owns only what the scenario adds: the
//!   governed fixture variants, the journey's event contract, the four-cell
//!   judgment, and the scenario receipt.
//! - `vim_lsp_cell_catalog` (#11374) owns cell registration; this module
//!   cites catalog cell ids in its receipt journey but never edits a catalog.
//! - `#7762` (via `.ci/editor-clients/vim-vim-lsp-activation-root.v1.json`)
//!   owns root selection; the fixture only arranges markers, and the driver
//!   observes native resolution. The adapter never forces a filetype.
//! - The expectation oracle lives here in Rust — the governed defect line,
//!   its fix, the expected and decoy root identities — never derived from the
//!   responses under test (#10938 law), and never embedded in Vimscript.
//!
//! Fail-closed laws:
//!
//! - the diagnostics oracle requires the client's own state (the classified
//!   `lsp#get_buffer_diagnostics_counts()` surface) AND the client's own wire
//!   record (a `publishDiagnostics` batch for the governed file carrying an
//!   error-severity parser-code diagnostic); either alone is insufficient —
//!   a server-log-only or counts-only claim cannot pass;
//! - an unrelated diagnostic (no parser-family code) never satisfies the
//!   governed-defect cell;
//! - post-edit currentness requires a publishDiagnostics batch ordered after
//!   the `textDocument/didChange` notification plus the cleared client state;
//!   a reused pre-edit batch cannot satisfy it;
//! - the root cell requires the observed root to equal the Rust-side
//!   expected governed root and to differ from the outer/same-named decoy; a
//!   server answering from the wrong root fails the journey even when
//!   diagnostics appear;
//! - negative fixture variants (`defect_absent`, `wrong_root_decoy`) are
//!   expected to fail with typed reasons; a pass on a negative variant is a
//!   contract violation of the oracle, never a green run.

use anyhow::{Context, Result, bail, ensure};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::editor_client_compat::{CapabilityBasis, CleanupResult, JourneyCell, ObservationResult};
use crate::vim_host_run::vim_host_runner;
use crate::vim_host_run::{BoundHostPlan, VimHostRunInputs, bind_host_run_plan};
use vim_host_runner::{
    DriverEventKind, HermeticVimLayout, ProcessObservation, VimHostRunPlan, WireEvidence,
    build_vim_command_with_extras, run_owned_process, validate_receipt_binding,
};

pub const DIAGNOSTICS_JOURNEY_SELECTOR: &str = "vim_vim_lsp_bootstrap_diagnostics.v1";
pub const DIAGNOSTICS_FIXTURE_ID: &str = "vim_vim_lsp_bootstrap_diagnostics_v1";

/// The governed fixture's stable layout, relative to the materialized fixture
/// root. These are the expectation constants: they are authored here, not
/// derived from any observed run output.
pub const OPENED_FILE_REL: &str = "workspace/project/main.pl";
pub const EXPECTED_ROOT_REL: &str = "workspace/project";
pub const DECOY_ROOT_REL: &str = "workspace";
pub const DECOY_SAME_NAME_FILE_REL: &str = "workspace/main.pl";
/// The governed defect: the trailing semicolon is missing on this line.
pub const DEFECT_LINE: usize = 5;
pub const DEFECT_LINE_TEXT: &str = "my $value = My::Widget::answer()";
pub const FIXED_LINE_TEXT: &str = "my $value = My::Widget::answer();";
/// The file-name token of the governed document inside the client's wire
/// record (publishDiagnostics `uri` tail).
pub const GOVERNED_FILE_TOKEN: &str = "main.pl";

/// One scenario fixture variant. `Canonical` must pass; the two negative
/// variants must fail with their typed reason (the red-first controls).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticsFixtureVariant {
    Canonical,
    DefectAbsent,
    WrongRootDecoy,
}

impl DiagnosticsFixtureVariant {
    pub fn from_id(id: &str) -> Result<Self> {
        match id {
            "canonical" => Ok(Self::Canonical),
            "defect_absent" => Ok(Self::DefectAbsent),
            "wrong_root_decoy" => Ok(Self::WrongRootDecoy),
            other => bail!(
                "unknown diagnostics fixture variant {other}: known variants are canonical, \
                 defect_absent, wrong_root_decoy"
            ),
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Self::Canonical => "canonical",
            Self::DefectAbsent => "defect_absent",
            Self::WrongRootDecoy => "wrong_root_decoy",
        }
    }

    /// The typed driver-failure reason this variant must produce; `None` for
    /// the canonical variant, which must pass.
    pub fn expected_negative_reason(self) -> Option<&'static str> {
        match self {
            Self::Canonical => None,
            Self::DefectAbsent => Some("defect_state_absent"),
            Self::WrongRootDecoy => Some("root_mismatch"),
        }
    }
}

/// The materialized governed fixture for one variant.
pub struct DiagnosticsFixture {
    pub root: PathBuf,
    pub variant: DiagnosticsFixtureVariant,
}

/// Materialize the #10946 governed fixture under `root`:
///
/// ```text
/// workspace/                      <- outer decoy root (no marker, canonical)
///   main.pl                       <- same-named decoy file (clean)
///   .perl-lsp.toml                <- marker ONLY in the wrong_root_decoy variant
///   project/                      <- the governed #7762 root
///     .perl-lsp.toml              <- marker (canonical + defect_absent)
///     main.pl                     <- the governed source (defect on DEFECT_LINE)
///     lib/My/Widget.pm            <- the definition target
/// ```
///
/// The `defect_absent` variant ships the already-fixed line: the governed
/// diagnostic must never appear and the journey must fail. The
/// `wrong_root_decoy` variant moves the marker to the outer workspace so
/// native nearest-parent-marker resolution selects the decoy root: a server
/// answering from the wrong root must fail the journey even though the file,
/// the defect, and the client are all otherwise canonical.
pub fn materialize_diagnostics_fixture(
    root: &Path,
    variant: DiagnosticsFixtureVariant,
) -> Result<DiagnosticsFixture> {
    ensure!(root.is_absolute(), "fixture root must be absolute");
    let workspace = root.join("workspace");
    let project = workspace.join("project");
    let lib = project.join("lib/My");
    fs::create_dir_all(&lib).with_context(|| format!("creating {}", lib.display()))?;
    let governed_line = match variant {
        DiagnosticsFixtureVariant::Canonical | DiagnosticsFixtureVariant::WrongRootDecoy => {
            format!("{DEFECT_LINE_TEXT}\n")
        }
        DiagnosticsFixtureVariant::DefectAbsent => format!("{FIXED_LINE_TEXT}\n"),
    };
    let main_pl = format!(
        "use strict;\nuse warnings;\nuse lib 'lib';\nuse My::Widget;\n{governed_line}print \
         \"$value\\n\";\n"
    );
    fs::write(project.join("main.pl"), main_pl)?;
    fs::write(
        lib.join("Widget.pm"),
        "package My::Widget;\nuse strict;\nuse warnings;\nsub answer { 42 }\n1;\n",
    )?;
    // The same-named decoy at the outer root: clean, harmless, and never the
    // governed document. Its presence is what makes `workspace` a real
    // same-name outer project rather than an arbitrary parent directory.
    fs::write(
        workspace.join("main.pl"),
        "use strict;\nuse warnings;\nprint \"outer decoy\\n\";\n",
    )?;
    let marker = "# vim/vim-lsp #10946 governed activation marker\n";
    match variant {
        DiagnosticsFixtureVariant::WrongRootDecoy => {
            // Marker ONLY at the decoy root: native resolution selects
            // `workspace`, and the journey must reject it.
            fs::write(workspace.join(".perl-lsp.toml"), marker)?;
        }
        DiagnosticsFixtureVariant::Canonical | DiagnosticsFixtureVariant::DefectAbsent => {
            fs::write(project.join(".perl-lsp.toml"), marker)?;
        }
    }
    Ok(DiagnosticsFixture { root: root.to_path_buf(), variant })
}

/// The scenario's environment contract beyond the substrate's: the
/// Rust-authored expectation delivered to the driver (never re-derived in
/// Vimscript).
pub fn journey_env() -> Vec<(std::ffi::OsString, std::ffi::OsString)> {
    [
        ("PERLLSP_VIM_HOST_OPENED_FILE_REL", OPENED_FILE_REL.to_string()),
        ("PERLLSP_VIM_HOST_EXPECTED_ROOT_REL", EXPECTED_ROOT_REL.to_string()),
        ("PERLLSP_VIM_HOST_DECOY_ROOT_REL", DECOY_ROOT_REL.to_string()),
        ("PERLLSP_VIM_HOST_DEFECT_LINE", DEFECT_LINE.to_string()),
        ("PERLLSP_VIM_HOST_FIX_LINE", FIXED_LINE_TEXT.to_string()),
    ]
    .into_iter()
    .map(|(key, value)| (std::ffi::OsString::from(key), std::ffi::OsString::from(value)))
    .collect()
}

/// The typed outcome of one diagnostics host run.
pub struct DiagnosticsRunOutcome {
    pub receipt_path: PathBuf,
    pub result: ObservationResult,
    pub process_cleanup: CleanupResult,
    pub driver_complete: bool,
    /// The typed driver-failure reason when the driver failed; the negative
    /// variants' expected reason lands here.
    pub driver_failure_reason: Option<String>,
}

/// Execute one #10946 bootstrap/diagnostics host run against the exact pinned
/// subject and write its canonical receipt. `variant` selects the fixture;
/// only `canonical` may pass.
pub fn host_diagnostics_run(
    repo_root: &Path,
    run: &VimHostRunInputs,
    variant: DiagnosticsFixtureVariant,
) -> Result<DiagnosticsRunOutcome> {
    crate::vim_host_run::ensure_fresh_output_root(&run.out_root)?;
    fs::create_dir_all(&run.out_root)
        .with_context(|| format!("creating output root {}", run.out_root.display()))?;

    let driver = repo_root.join("scripts/test/vim-host-diagnostics-driver.vim");
    let fixture = materialize_diagnostics_fixture(&run.out_root.join("fixture"), variant)?;
    let BoundHostPlan { plan, server_name, root_markers } = bind_host_run_plan(
        repo_root,
        run,
        &driver,
        &fixture.root,
        DIAGNOSTICS_JOURNEY_SELECTOR,
        DIAGNOSTICS_FIXTURE_ID,
    )?;
    let layout = HermeticVimLayout::prepare(&run.out_root.join("hermetic"))?;
    let mut command =
        build_vim_command_with_extras(&plan, &layout, &server_name, &root_markers, &journey_env())?;
    let mut observation = run_owned_process(&mut command, &plan, &layout)?;

    let client_log_bytes = fs::read(layout.client_log()).unwrap_or_default();
    let wire = vim_host_runner::extract_wire_evidence(&client_log_bytes);
    observation
        .artifacts
        .extend(vim_host_runner::retain_wire_evidence_artifacts(&plan, &layout, &wire)?);

    let judgment = evaluate_diagnostics_observation(&plan, &observation, &wire, variant);

    let snapshot = layout.capability_snapshot();
    let snapshot_sha256 =
        if snapshot.is_file() { Some(vim_host_runner::file_sha256(&snapshot)?) } else { None };
    let capabilities = vim_host_runner::capabilities_from_wire_evidence(&wire, snapshot_sha256)?;
    let diagnostics = vim_host_runner::diagnostics_from_wire_evidence(&wire);

    let mut limitations = vec![
        "headless silent-ex Vim (-es): GUI-only client surfaces are not exercised by this harness"
            .to_string(),
        format!(
            "fixture variant {}: the governed fixture arranges #7762 markers and the governed \
             defect; expectation constants are Rust-authored, never derived from run output",
            variant.id()
        ),
        "completion/navigation/rename/formatting/unicode/deep-sync cells are separate leaves and \
         are not claimed here"
            .to_string(),
    ];
    if variant.expected_negative_reason().is_some() {
        limitations.push(format!(
            "negative control variant: the run must fail with the typed reason {}; a pass would \
             be an oracle violation",
            variant.expected_negative_reason().unwrap_or("unknown")
        ));
    }
    if let Some(reason) = &judgment.driver_failure_reason {
        limitations.push(format!("driver failed: {reason}"));
    }
    if judgment.wrong_initialize_root {
        limitations.push(
            "the initialize request's rootUri disagreed with the driver-observed root; the run \
             cannot claim a consistent root identity"
                .to_string(),
        );
    }
    if observation.cleanup != CleanupResult::Pass {
        limitations.push(format!(
            "process cleanup {} ({})",
            match observation.cleanup {
                CleanupResult::Pass => "pass",
                CleanupResult::Fail => "fail",
                CleanupResult::NotProven => "not_proven",
            },
            observation.cleanup_detail
        ));
    }
    if plan.identity.platform.os == "windows" {
        limitations.push(
            "windows is a local probe platform for this harness; the maintained CI host row is \
             linux (vim availability and process probes are best-effort on windows)"
                .to_string(),
        );
    }

    let receipt = vim_host_runner::build_receipt(
        &plan,
        &observation,
        capabilities,
        diagnostics,
        diagnostics_journey(&observation, &wire, &judgment),
        judgment.result,
        judgment.failure_class,
        limitations,
        format!(
            "#10946 {DIAGNOSTICS_JOURNEY_SELECTOR}: bootstrap, native root selection, \
             diagnostics lifecycle, and baseline cleanup for the exact pinned subject only"
        ),
    );
    let receipt_path = run.out_root.join("receipt.json");
    fs::write(&receipt_path, serde_json::to_vec_pretty(&receipt)?)
        .with_context(|| format!("writing receipt {}", receipt_path.display()))?;
    validate_receipt_binding(&receipt, &plan)
        .context("the emitted receipt failed its own freshness binding")?;
    Ok(DiagnosticsRunOutcome {
        receipt_path,
        result: judgment.result,
        process_cleanup: observation.cleanup,
        driver_complete: observation.driver_complete,
        driver_failure_reason: judgment.driver_failure_reason,
    })
}

// ---------------------------------------------------------------------------
// Judgment
// ---------------------------------------------------------------------------

/// The four-cell judgment over one observed diagnostics run.
pub struct DiagnosticsJudgment {
    pub result: ObservationResult,
    pub failure_class: Option<crate::editor_client_compat::FailureClass>,
    pub driver_failure_reason: Option<String>,
    /// The initialize request's rootUri tail disagreed with the expected
    /// governed root (typed inconsistency; cannot pass).
    pub wrong_initialize_root: bool,
    /// Per-cell results for the receipt journey, keyed by catalog cell id.
    pub cells: BTreeMap<String, ObservationResult>,
}

/// The #10946 catalog cell ids this journey evidences. The catalog owns
/// registration; this scenario only cites.
pub const CELL_BOOTSTRAP: &str = "vim.vim_lsp.core.bootstrap";
pub const CELL_ROOT: &str = "vim.vim_lsp.core.root";
pub const CELL_DIAGNOSTICS: &str = "vim.vim_lsp.core.diagnostics";
pub const CELL_CURRENTNESS: &str = "vim.vim_lsp.currentness.post_edit";
pub const CELL_BASELINE_CLEANUP: &str = "vim.vim_lsp.lifecycle.baseline_cleanup";

/// Judge one observed run against the scenario's Rust-authored expectations.
///
/// Positive path (all four cells must pass): registration bound to the
/// planned candidate digest, wire attach identity, native root equal to the
/// governed root and distinct from the decoy, the governed defect visible in
/// the client's own state AND wire record, a post-didChange wire batch with
/// the discriminator gone plus cleared client state, and the orderly
/// process boundary.
#[allow(clippy::too_many_lines)]
pub fn evaluate_diagnostics_observation(
    plan: &VimHostRunPlan,
    observation: &ProcessObservation,
    wire: &WireEvidence,
    variant: DiagnosticsFixtureVariant,
) -> DiagnosticsJudgment {
    let mut cells = BTreeMap::new();
    let events = &observation.events;
    let detail = |kind: DriverEventKind, key: &str| -> Option<&str> {
        events
            .iter()
            .find(|event| event.kind == kind)
            .and_then(|event| event.details.get(key))
            .map(String::as_str)
    };

    // --- bootstrap cell: exact candidate registration + attach identity.
    let registration_digest_match =
        detail(DriverEventKind::RegistrationSelected, "candidate_sha256")
            == Some(plan.identity.candidate_artifact_sha256.as_str());
    let attach_identity_observed = wire.saw_initialize && wire.saw_initialized;
    let bootstrap_observed =
        events.iter().any(|event| event.kind == DriverEventKind::ServerInitialized)
            && events.iter().any(|event| event.kind == DriverEventKind::InitializeObserved);
    let bootstrap_ok = registration_digest_match
        && attach_identity_observed
        && bootstrap_observed
        && observation.passed_process_boundary();
    cells.insert(
        CELL_BOOTSTRAP.to_string(),
        cell_result(
            bootstrap_observed,
            bootstrap_ok,
            "bootstrap barriers or attach identity incomplete",
        ),
    );

    // --- root cell: observed root equals the governed root, decoy recorded
    // and distinct, and the initialize rootUri agrees.
    let root_event = events.iter().find(|event| event.kind == DriverEventKind::RootSelected);
    let observed_root = detail(DriverEventKind::RootSelected, "observed_root");
    let expected_reported = detail(DriverEventKind::RootSelected, "expected_root");
    let decoy_reported = detail(DriverEventKind::RootSelected, "decoy_root");
    let root_observed = root_event.is_some();
    let initialize_root_ok = wire
        .initialize_request
        .as_ref()
        .and_then(|request| request.get("params"))
        .and_then(|params| params.get("rootUri"))
        .and_then(|uri| uri.as_str())
        .is_some_and(|uri| uri_ends_with_segment(uri, EXPECTED_ROOT_REL));
    let wrong_initialize_root = root_observed && !initialize_root_ok;
    let root_ok = root_observed
        && observed_root == Some(EXPECTED_ROOT_REL)
        && expected_reported == Some(EXPECTED_ROOT_REL)
        && decoy_reported == Some(DECOY_ROOT_REL)
        && initialize_root_ok;
    cells.insert(
        CELL_ROOT.to_string(),
        cell_result(
            root_observed,
            root_ok,
            "root did not resolve to the governed root or the decoy identity is missing",
        ),
    );

    // --- diagnostics cell: the governed defect through the client's own
    // state AND the client's own wire record; an unrelated diagnostic (no
    // parser-family code) never satisfies it.
    let defect_event =
        events.iter().find(|event| event.kind == DriverEventKind::DefectStateObserved);
    let defect_client_errors = detail(DriverEventKind::DefectStateObserved, "errors")
        .and_then(|value| value.parse::<u32>().ok());
    let defect_wire_batch = governed_defect_batch(wire);
    let diagnostics_observed = defect_event.is_some();
    let diagnostics_ok = diagnostics_observed
        && defect_client_errors.is_some_and(|count| count >= 1)
        && defect_wire_batch;
    cells.insert(
        CELL_DIAGNOSTICS.to_string(),
        cell_result(
            diagnostics_observed,
            diagnostics_ok,
            "the governed defect was not proven through client state and client wire record",
        ),
    );

    // --- currentness cell: a real edit, a wire batch ordered after the
    // didChange, the discriminator gone from that batch, and the cleared
    // client state. A reused pre-edit batch cannot satisfy it.
    let fix_observed = events.iter().any(|event| event.kind == DriverEventKind::DefectFixApplied);
    let current_event =
        events.iter().find(|event| event.kind == DriverEventKind::CurrentStateObserved);
    let current_client_errors = detail(DriverEventKind::CurrentStateObserved, "errors")
        .and_then(|value| value.parse::<u32>().ok());
    let post_edit_batch = post_edit_cleared_batch(wire);
    let currentness_observed = fix_observed && current_event.is_some();
    let currentness_ok =
        currentness_observed && current_client_errors == Some(0) && post_edit_batch;
    cells.insert(
        CELL_CURRENTNESS.to_string(),
        cell_result(
            currentness_observed,
            currentness_ok,
            "post-edit current state was not proven after a wire-ordered didChange",
        ),
    );

    // --- baseline cleanup cell: the supervisor's deterministic process
    // boundary.
    let cleanup_observed = observation.status_code.is_some();
    let cleanup_ok = observation.passed_process_boundary();
    cells.insert(
        CELL_BASELINE_CLEANUP.to_string(),
        cell_result(cleanup_observed, cleanup_ok, "process cleanup was not proven"),
    );

    let driver_failed_event =
        events.iter().find(|event| event.kind == DriverEventKind::DriverFailed);
    let driver_failure_reason =
        driver_failed_event.and_then(|event| event.details.get("reason")).cloned();
    let leaked = observation.cleanup == CleanupResult::Fail;
    let four_cells_ok = bootstrap_ok && root_ok && diagnostics_ok && currentness_ok && cleanup_ok;
    let result = if observation.passed_process_boundary() && four_cells_ok {
        // A negative variant that reaches a pass is an oracle violation: it
        // cannot happen honestly, and reporting it as a pass would hide it.
        if variant.expected_negative_reason().is_some() {
            ObservationResult::Fail
        } else {
            ObservationResult::Pass
        }
    } else if driver_failed_event.is_some()
        || observation.timed_out
        || leaked
        || observation.status_code.is_some_and(|code| code != 0)
    {
        ObservationResult::Fail
    } else {
        ObservationResult::NotProven
    };
    let failure_class = if result == ObservationResult::Pass {
        None
    } else if leaked {
        Some(crate::editor_client_compat::FailureClass::Cleanup)
    } else if observation.timed_out {
        Some(crate::editor_client_compat::FailureClass::Instrument)
    } else if driver_failed_event.is_some() || observation.status_code.is_some_and(|code| code != 0)
    {
        Some(crate::editor_client_compat::FailureClass::HostClient)
    } else {
        Some(crate::editor_client_compat::FailureClass::Instrument)
    };
    DiagnosticsJudgment {
        result,
        failure_class,
        driver_failure_reason,
        wrong_initialize_root,
        cells,
    }
}

fn cell_result(observed: bool, ok: bool, limitation: &str) -> ObservationResult {
    if ok {
        ObservationResult::Pass
    } else if observed {
        ObservationResult::Fail
    } else {
        let _ = limitation;
        ObservationResult::NotProven
    }
}

/// Whether at least one publishDiagnostics batch for the governed file
/// carries an error-severity parser-code diagnostic: the client's own record
/// of the governed defect. Counts alone (any error, any file) never satisfy
/// this — the discriminator is the parser-family code on the governed file.
pub fn governed_defect_batch(wire: &WireEvidence) -> bool {
    wire.publish_diagnostics_batches.iter().any(|batch| {
        batch.uri_file == GOVERNED_FILE_TOKEN
            && batch.error_severity_count >= 1
            && batch.parser_code_count >= 1
    })
}

/// Whether at least one publishDiagnostics batch for the governed file is
/// ordered after the didChange notification AND carries no parser-code
/// diagnostic: the current post-edit generation, not a reused pre-edit one.
pub fn post_edit_cleared_batch(wire: &WireEvidence) -> bool {
    let Some(did_change_line) = wire.did_change_line else { return false };
    wire.publish_diagnostics_batches.iter().any(|batch| {
        batch.line_index > did_change_line
            && batch.uri_file == GOVERNED_FILE_TOKEN
            && batch.parser_code_count == 0
            && batch.error_severity_count == 0
    })
}

/// Whether a `file://` URI ends with the expected relative directory
/// segment, on every host's path spelling.
fn uri_ends_with_segment(uri: &str, segment: &str) -> bool {
    let normalized = uri.replace('\\', "/");
    normalized.trim_end_matches('/').ends_with(&format!("/{segment}"))
        || normalized.trim_end_matches('/') == format!("file:///{segment}")
        || normalized.ends_with(segment)
}

// ---------------------------------------------------------------------------
// Receipt journey
// ---------------------------------------------------------------------------

/// Compose the receipt journey: the lifecycle barrier cells (the #10944
/// surface) plus the five catalog cells this scenario evidences.
pub fn diagnostics_journey(
    observation: &ProcessObservation,
    wire: &WireEvidence,
    judgment: &DiagnosticsJudgment,
) -> Vec<JourneyCell> {
    let mut cells = crate::vim_host_run::outcome_journey(observation, wire);
    for (kind, id) in [
        (DriverEventKind::DefectStateObserved, "defect_state_observed"),
        (DriverEventKind::DefectFixApplied, "defect_fix_applied"),
        (DriverEventKind::CurrentStateObserved, "current_state_observed"),
    ] {
        let observed = observation.events.iter().any(|event| event.kind == kind);
        cells.push(JourneyCell {
            id: id.to_string(),
            capability_basis: CapabilityBasis::NotApplicable,
            observed,
            result: if observed { ObservationResult::Pass } else { ObservationResult::NotProven },
            evidence: vec!["vim/driver-events.jsonl".to_string()],
            limitation: if observed {
                None
            } else {
                Some("diagnostics lifecycle barrier never emitted".to_string())
            },
        });
    }
    let catalog_limitations: BTreeMap<&str, &str> = BTreeMap::from([
        (
            CELL_ROOT,
            "native #7762 root selection for the governed project only; the same-named outer \
             decoy is proven distinct",
        ),
        (
            CELL_DIAGNOSTICS,
            "governed defect visibility through the client's own diagnostics state and wire \
             record only",
        ),
        (
            CELL_CURRENTNESS,
            "post-edit currentness after a wire-ordered didChange for this exact subject",
        ),
    ]);
    for (cell_id, result) in &judgment.cells {
        cells.push(JourneyCell {
            id: cell_id.clone(),
            capability_basis: CapabilityBasis::NotApplicable,
            observed: *result != ObservationResult::NotProven || cell_id == CELL_ROOT,
            result: *result,
            evidence: catalog_evidence(cell_id),
            limitation: if *result == ObservationResult::Pass {
                catalog_limitations.get(cell_id.as_str()).map(|text| text.to_string())
            } else {
                Some(format!("{cell_id} was not proven for this exact subject"))
            },
        });
    }
    cells
}

fn catalog_evidence(cell_id: &str) -> Vec<String> {
    match cell_id {
        CELL_BOOTSTRAP => vec![
            "vim/driver-events.jsonl".to_string(),
            "vim/initialize-request.json".to_string(),
            "vim/process-ledger.json".to_string(),
        ],
        CELL_ROOT => {
            vec!["vim/driver-events.jsonl".to_string(), "vim/initialize-request.json".to_string()]
        }
        CELL_DIAGNOSTICS | CELL_CURRENTNESS => {
            vec!["vim/driver-events.jsonl".to_string(), "vim/vim-lsp-client.log".to_string()]
        }
        _ => vec!["vim/process-ledger.json".to_string()],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vim_host_runner::PublishDiagnosticsBatch;

    #[test]
    fn fixture_variants_parse_and_carry_typed_negative_reasons() {
        assert_eq!(
            DiagnosticsFixtureVariant::from_id("canonical").unwrap(),
            DiagnosticsFixtureVariant::Canonical
        );
        assert!(DiagnosticsFixtureVariant::from_id("other").is_err());
        assert_eq!(
            DiagnosticsFixtureVariant::DefectAbsent.expected_negative_reason(),
            Some("defect_state_absent")
        );
        assert_eq!(
            DiagnosticsFixtureVariant::WrongRootDecoy.expected_negative_reason(),
            Some("root_mismatch")
        );
        assert_eq!(DiagnosticsFixtureVariant::Canonical.expected_negative_reason(), None);
    }

    #[test]
    fn governed_wire_batches_discriminate_defect_and_currentness() {
        let wire = WireEvidence {
            publish_diagnostics_batches: vec![
                PublishDiagnosticsBatch {
                    line_index: 3,
                    uri_file: "main.pl".to_string(),
                    diagnostics_count: 1,
                    error_severity_count: 1,
                    parser_code_count: 1,
                },
                PublishDiagnosticsBatch {
                    line_index: 9,
                    uri_file: "main.pl".to_string(),
                    diagnostics_count: 0,
                    error_severity_count: 0,
                    parser_code_count: 0,
                },
            ],
            did_change_line: Some(7),
            ..WireEvidence::default()
        };
        assert!(governed_defect_batch(&wire));
        assert!(post_edit_cleared_batch(&wire));
        // An unrelated diagnostic (no parser code) never satisfies the defect.
        let unrelated = WireEvidence {
            publish_diagnostics_batches: vec![PublishDiagnosticsBatch {
                line_index: 3,
                uri_file: "main.pl".to_string(),
                diagnostics_count: 1,
                error_severity_count: 1,
                parser_code_count: 0,
            }],
            ..WireEvidence::default()
        };
        assert!(!governed_defect_batch(&unrelated));
        // A pre-edit batch cannot satisfy currentness.
        assert!(!post_edit_cleared_batch(&unrelated));
    }
}

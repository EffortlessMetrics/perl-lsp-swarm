//! #11403 expanded-activation scenario for the hermetic Vim + vim-lsp host
//! runner.
//!
//! This module is the expanded-activation execution consumer of the
//! #10944/#12545 substrate and the #12589/#12660 scenario pattern: it proves,
//! through the pinned actual Vim + vim-lsp + perllsp subject, the finite
//! #7762 activation-root denominator executed row by row and receipted into
//! the #11388 activation cell catalog.
//!
//! Ownership split (consumed, never duplicated):
//!
//! - `vim_host_run::vim_host_runner` (#10944) owns hermetic launch,
//!   supervision, process ledgers, cleanup comparison, wire mining, and
//!   receipt composition. This module owns only what the scenario adds: the
//!   denominator execution fixture, the journey's environment contract, the
//!   90-cell (18 denominator rows x 5 aspects) judgment, and the scenario
//!   receipt.
//! - `vim_lsp_cell_catalog::activation` (#11388) owns the finite #7762
//!   denominator mirror and cell registration; this scenario cites catalog
//!   cell ids in its receipt journey but never edits a catalog. The receipt's
//!   activation cells are proven equal to the registered catalog ids by a
//!   committed test, so a local invention cannot masquerade as a cell.
//! - `#7762` (via `.ci/editor-clients/vim-vim-lsp-activation-root.v1.json`)
//!   owns the filetype/root denominator bytes. The fixture arranges
//!   content-discriminating subjects (XPM/TADS controls, shebang scripts,
//!   extension-only CGI/FCGI, template/POD/XS adjacency files); the driver
//!   observes native resolution and never forces a filetype — the
//!   `preset_filetype_claimed` variant exists precisely to prove that a
//!   forced filetype cannot be relabeled native.
//! - The expectation oracle lives here in Rust — governed defect lines,
//!   negative-control expectations, override boundaries — never derived from
//!   the responses under test (#10938 law), and never embedded in Vimscript.
//!
//! Fail-closed laws beyond the substrate's:
//!
//! - a native disposition requires the retained pre-override `&filetype`
//!   observation bound to its denominator row (`native_vim`, `preset=0`); a
//!   pre-forced filetype never satisfies a native cell;
//! - negative-control rows (`Image.pm` -> xpm, `game.t` -> tads) must resolve
//!   to their distinct artifact expectations; a blanket rule that steals them
//!   into Perl fails the run with the typed `adjacent_language_stolen`
//!   reason (proven by the `blanket_override_steal` red control);
//! - an attachment observation exists only where the row genuinely activates
//!   (native Perl or the bounded reviewed override); any other activated row
//!   is contamination and fails; an unactivated false subject keeps its
//!   `client_not_exposed` preservation receipt;
//! - a semantic cell affirms only for rows whose #7762 expectation claims
//!   Perl support, and only through the two-source discriminator: the
//!   client's own settled error state AND a parser-coded publishDiagnostics
//!   batch mined for that row's exact document tail;
//! - overrides apply only to rows carrying the `manual_override` boundary,
//!   only after the native observation was retained, only as one narrow
//!   exact-buffer rule shaped like a reviewed user equivalent, and they keep
//!   their `not_authorized_by_extension_alone` limitation;
//! - between-rows reset evidence (real didClose buffer close) is load-bearing
//!   for every row of the canonical shape, so activation rows cannot
//!   contaminate each other silently;
//! - negative variants are expected to fail with typed reasons; a pass on a
//!   negative variant is an oracle violation, never a green run.

use anyhow::{Context as _, Result, bail, ensure};
use serde::Serialize;
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use crate::editor_client_compat::{CapabilityBasis, CleanupResult, JourneyCell, ObservationResult};
use crate::vim_host_run::vim_host_runner;
use crate::vim_host_run::{BoundHostPlan, VimHostRunInputs, bind_host_run_plan};
use crate::vim_lsp_cell_catalog::activation::{ACTIVATION_ASPECTS, ACTIVATION_DENOMINATOR};
use vim_host_runner::{
    DriverEvent, DriverEventKind, HermeticVimLayout, ProcessObservation, WireEvidence,
    build_vim_command_with_extras, run_owned_process, validate_receipt_binding,
};

pub const ACTIVATION_JOURNEY_SELECTOR: &str = "vim_vim_lsp_expanded_activation.v1";
pub const ACTIVATION_FIXTURE_ID: &str = "vim_vim_lsp_expanded_activation_v1";

/// The governed fixture layout, relative to the materialized fixture root.
/// Authored here, never derived from run output.
pub const GOVERNED_ROOT_REL: &str = "workspace/project";
pub const DECOY_ROOT_REL: &str = "workspace";
/// The same-named clean decoy at the outer root: real outer project material,
/// never the governed document.
pub const DECOY_SAME_NAME_FILE_REL: &str = "workspace/main.pl";

/// The governed root marker for this fixture. `.perl-lsp.toml` is first on
/// the #7762 authority list, so every row resolves to the project root.
pub const ROOT_MARKER: &str = ".perl-lsp.toml";

/// Cell-ID prefix mirror of [`crate::vim_lsp_cell_catalog::activation`] (the
/// catalog owns registration; this scenario cites). The committed test below
/// proves the citation set equals the registered catalog ids exactly.
pub const ACTIVATION_CELL_PREFIX: &str = "vim.vim_lsp.activation.";

// ---------------------------------------------------------------------------
// Rust-authored fixture expectations
// ---------------------------------------------------------------------------

/// Every claimed row embeds the #10946-classified governed defect on line 4:
/// the trailing semicolon is missing, which the pinned perllsp reports as an
/// error-severity parser-coded diagnostic on that document alone.
pub const DEFECT_LINE_TEXT: &str = "my $value = My::Widget::answer()";

const CLAIMED_SOURCE_HEAD: [&str; 3] =
    ["use strict;", "use warnings;", "use lib 'lib'; use My::Widget;"];
const CLAIMED_SOURCE_TAIL: [&str; 1] = ["print \"$value\\n\";"];

/// Negative-control discriminators whose bytes the runtime heuristics classify
/// away from Perl: an X PixMap header inside `.pm`, TADS source inside `.t`.
const XPM_SOURCE_LINES: [&str; 7] = [
    "/* XPM */",
    "static char * image_xpm[] = {",
    "\"16 16 2 1\",",
    "\"a c #FFFFFF\",",
    "\"b c #000000\",",
    "\"aaaaaaaaaaaaaaaa\",",
    "};",
];
const TADS_SOURCE_LINES: [&str; 4] =
    ["% classes", "% main = args", "class hello: main(args)", "hello.show();"];

/// Extension-only CGI/FCGI subjects: deliberately WITHOUT any shebang so the
/// native observation is honestly undetected and the row exercises exactly
/// its admitted bounded-override route. A shebang here would silently convert
/// the rows into native activations and hide the override receipt.
const CGI_PLAIN_LINES: [&str; 3] =
    ["use strict;", "print \"Content-Type: text/plain\\n\";", "print \"ok\\n\";"];

const CPANFILE_LINES: [&str; 2] = ["requires \"Plack\";", "requires \"Try::Tiny\";"];

/// Shebang deployment subjects (admitted `observe` rows with declared native
/// detection): plain executable scripts whose first line selects Perl natively.
const SHEBANG_TOOL_LINES: [&str; 4] =
    ["#!/usr/bin/perl", "use strict;", "print \"tool ok\\n\";", "__END__"];

const POD_LINES: [&str; 5] =
    ["=pod", "=head1 notes", "plain documentation body", "=cut", "print \"pod body\\n\";"];
const XS_LINES: [&str; 4] = [
    "#include \"EXTERN.h\"",
    "#include \"perl.h\"",
    "MODULE = Native PACKAGE = Native",
    "void hello()",
];
const TEMPLATE_EP_LINES: [&str; 3] = ["<html>", "<body><%= content %></body>", "</html>"];
const TEMPLATE_TT_LINES: [&str; 3] = ["[% block content %]", "hello", "[% end %]"];
const MASON_LINES: [&str; 3] = ["<%method title>", "component title", "</%method>"];

fn lines_to_string(lines: &[&str]) -> String {
    let mut body = String::new();
    for line in lines {
        body.push_str(line);
        body.push('\n');
    }
    body
}

/// A claimed row's document: head + governed defect + tail.
fn claimed_source() -> String {
    let mut body = String::new();
    for line in CLAIMED_SOURCE_HEAD {
        body.push_str(line);
        body.push('\n');
    }
    body.push_str(DEFECT_LINE_TEXT);
    body.push('\n');
    for line in CLAIMED_SOURCE_TAIL {
        body.push_str(line);
        body.push('\n');
    }
    body
}

/// The deterministic bytes of one denominator row within the governed project.
///
/// Content choice is expectation-driven, authored from the landed artifact
/// fields consumed by this leaf: negative-control rows carry their
/// disambiguating language bytes; extension-only override rows omit every
/// native steer so the bounded-override route stays honestly exercised;
/// shebang rows steer natively through the interpreter line.
pub fn row_fixture_bytes(path: &str) -> Option<String> {
    match path {
        "sample.pl" | "legacy.PL" | "Sample.pm" | "sample.t" | "app.psgi" => Some(claimed_source()),
        "Image.pm" => Some(lines_to_string(&XPM_SOURCE_LINES)),
        "game.t" => Some(lines_to_string(&TADS_SOURCE_LINES)),
        "app.cgi" | "app.fcgi" => Some(lines_to_string(&CGI_PLAIN_LINES)),
        "cpanfile" => Some(lines_to_string(&CPANFILE_LINES)),
        "bin/tool" | "script/tool" => Some(lines_to_string(&SHEBANG_TOOL_LINES)),
        "notes.pod" => Some(lines_to_string(&POD_LINES)),
        "Native.xs" => Some(lines_to_string(&XS_LINES)),
        "view.ep" => Some(lines_to_string(&TEMPLATE_EP_LINES)),
        "view.tt" | "view.tt2" => Some(lines_to_string(&TEMPLATE_TT_LINES)),
        "view.mason" => Some(lines_to_string(&MASON_LINES)),
        _ => None,
    }
}

/// The publishDiagnostics `uri` tail the wire miner sees for one row.
fn row_uri_tail(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

// ---------------------------------------------------------------------------
// Fixture variants
// ---------------------------------------------------------------------------

/// One scenario fixture/journey variant. `Canonical` must pass; the two
/// negative controls must fail with their typed reasons (red-first controls).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivationFixtureVariant {
    Canonical,
    /// The harness pre-forces `filetype=perl` after the open, exactly the way
    /// a dishonest harness would fake native activation: the whole synthetic
    /// claim voids with the typed reason.
    PresetFiletypeClaimed,
    /// The harness installs the forbidden broad `*.t -> perl` rule before the
    /// pass: it must steal the TADS control and be caught.
    BlanketOverrideSteal,
}

impl ActivationFixtureVariant {
    pub fn from_id(id: &str) -> Result<Self> {
        match id {
            "canonical" => Ok(Self::Canonical),
            "preset_filetype_claimed" => Ok(Self::PresetFiletypeClaimed),
            "blanket_override_steal" => Ok(Self::BlanketOverrideSteal),
            other => bail!(
                "unknown activation fixture variant {other}: known variants are canonical, \
                 preset_filetype_claimed, blanket_override_steal"
            ),
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Self::Canonical => "canonical",
            Self::PresetFiletypeClaimed => "preset_filetype_claimed",
            Self::BlanketOverrideSteal => "blanket_override_steal",
        }
    }

    /// The typed driver-failure reason this variant must produce; `None` for
    /// the canonical variant, which must pass.
    pub fn expected_negative_reason(self) -> Option<&'static str> {
        match self {
            Self::Canonical => None,
            Self::PresetFiletypeClaimed => Some("pre_forced_filetype_not_native"),
            Self::BlanketOverrideSteal => Some("adjacent_language_stolen"),
        }
    }
}

// ---------------------------------------------------------------------------
// Fixture materialization
// ---------------------------------------------------------------------------

/// The materialized expanded-activation fixture for one variant.
pub struct ActivationFixture {
    pub root: PathBuf,
    pub variant: ActivationFixtureVariant,
}

/// Materialize the #11403 denominator fixture under `root`:
///
/// ```text
/// workspace/                      <- outer decoy root
///   main.pl                       <- clean same-named decoy document
///   project/                      <- the governed #7762 root
///     .perl-lsp.toml              <- marker (governed root)
///     sample.pl legacy.PL Sample.pm Image.pm sample.t game.t app.psgi
///     app.cgi app.fcgi cpanfile notes.pod Native.xs view.ep view.tt
///     view.tt2 view.mason          <- the 18 denominator documents
///     bin/tool script/tool         <- shebang denominator documents
///     lib/My/Widget.pm             <- definition target for the defect line
/// ```
///
/// All denominator bytes come from [`row_fixture_bytes`], keyed by the exact
/// artifact paths. The marker sits only in the governed project; root
/// discrimination itself stays owned by the #12589 bootstrap journey.
pub fn materialize_activation_fixture(
    root: &Path,
    variant: ActivationFixtureVariant,
) -> Result<ActivationFixture> {
    ensure!(root.is_absolute(), "fixture root must be absolute");
    let workspace = root.join("workspace");
    let project = workspace.join("project");
    let lib = project.join("lib/My");
    fs::create_dir_all(&lib).with_context(|| format!("creating {}", lib.display()))?;
    for directory in ["bin", "script"] {
        let path = project.join(directory);
        fs::create_dir_all(&path).with_context(|| format!("creating {}", path.display()))?;
    }

    for row in ACTIVATION_DENOMINATOR {
        let target = project.join(row.path);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
        }
        let bytes = row_fixture_bytes(row.path).with_context(|| {
            format!("denominator row {} has no authored fixture bytes", row.path)
        })?;
        fs::write(&target, bytes).with_context(|| format!("writing {}", target.display()))?;
    }

    fs::write(
        lib.join("Widget.pm"),
        "package My::Widget;\nuse strict;\nuse warnings;\nsub answer { 42 }\n1;\n",
    )?;
    fs::write(
        workspace.join("main.pl"),
        "use strict;\nuse warnings;\nprint \"outer decoy\\n\";\n",
    )?;
    fs::write(project.join(ROOT_MARKER), "# vim/vim-lsp #11403 governed activation-root marker\n")?;

    Ok(ActivationFixture { root: root.to_path_buf(), variant })
}

/// The payload delivered to the driver: one entry per denominator row, in
/// artifact order, carrying exactly what the execution needs. Authored from
/// the landed catalog mirror (`ACTIVATION_DENOMINATOR`) — never re-derived.
#[derive(Debug, Clone, Serialize)]
struct ActivationRowPayload<'a> {
    row: &'a str,
    path: &'a str,
    expect: &'a str,
    negative_control: bool,
    manual_override: &'a str,
    claimed: bool,
}

fn activation_rows_payload_json() -> Result<OsString> {
    let rows: Vec<ActivationRowPayload<'_>> = ACTIVATION_DENOMINATOR
        .iter()
        .map(|row| ActivationRowPayload {
            row: row.slug,
            path: row.path,
            expect: row.expect,
            negative_control: row.negative_control,
            manual_override: row.manual_override.unwrap_or(""),
            // Only the one #7762 expectation claiming Perl support carries a
            // semantic obligation.
            claimed: row.expect == "perl",
        })
        .collect();
    Ok(OsString::from(serde_json::to_string(&rows)?))
}

/// The scenario's environment contract beyond the substrate's.
pub fn journey_env(variant: ActivationFixtureVariant) -> Result<Vec<(OsString, OsString)>> {
    let mut env: Vec<(OsString, OsString)> = vec![
        (OsString::from("PERLLSP_VIM_HOST_EXPECTED_ROOT_REL"), OsString::from(GOVERNED_ROOT_REL)),
        (OsString::from("PERLLSP_VIM_HOST_DECOY_ROOT_REL"), OsString::from(DECOY_ROOT_REL)),
        (OsString::from("PERLLSP_VIM_HOST_ACTIVATION_PHASE"), OsString::from(variant.id())),
    ];
    env.push((
        OsString::from("PERLLSP_VIM_HOST_ACTIVATION_ROWS_JSON"),
        activation_rows_payload_json()?,
    ));
    Ok(env)
}

// ---------------------------------------------------------------------------
// Execution
// ---------------------------------------------------------------------------

/// The typed outcome of one expanded-activation host run.
pub struct ActivationRunOutcome {
    pub receipt_path: PathBuf,
    pub result: ObservationResult,
    pub process_cleanup: CleanupResult,
    pub driver_complete: bool,
    /// The typed driver-failure reason when the driver failed; the negative
    /// controls' expected reasons land here.
    pub driver_failure_reason: Option<String>,
}

/// Execute one #11403 expanded-activation host run against the exact pinned
/// subject and write its canonical receipt. `variant` selects the fixture/
/// journey shape; only `canonical` may pass.
pub fn host_activation_run(
    repo_root: &Path,
    run: &VimHostRunInputs,
    variant: ActivationFixtureVariant,
) -> Result<ActivationRunOutcome> {
    crate::vim_host_run::ensure_fresh_output_root(&run.out_root)?;
    fs::create_dir_all(&run.out_root)
        .with_context(|| format!("creating output root {}", run.out_root.display()))?;

    let driver = repo_root.join("scripts/test/vim-host-activation-driver.vim");
    let fixture = materialize_activation_fixture(&run.out_root.join("fixture"), variant)?;
    let BoundHostPlan { plan, server_name, root_markers } = bind_host_run_plan(
        repo_root,
        run,
        &driver,
        &fixture.root,
        ACTIVATION_JOURNEY_SELECTOR,
        ACTIVATION_FIXTURE_ID,
    )?;
    let layout = HermeticVimLayout::prepare(&run.out_root.join("hermetic"))?;
    let env = journey_env(variant)?;
    let mut command =
        build_vim_command_with_extras(&plan, &layout, &server_name, &root_markers, &env)?;
    let mut observation = run_owned_process(&mut command, &plan, &layout)?;

    let client_log_bytes = fs::read(layout.client_log()).unwrap_or_default();
    let wire = vim_host_runner::extract_wire_evidence(&client_log_bytes);
    observation
        .artifacts
        .extend(vim_host_runner::retain_wire_evidence_artifacts(&plan, &layout, &wire)?);

    let judgment = evaluate_activation_observation(&observation, &wire, variant);

    let snapshot = layout.capability_snapshot();
    let snapshot_sha256 =
        if snapshot.is_file() { Some(vim_host_runner::file_sha256(&snapshot)?) } else { None };
    let capabilities = vim_host_runner::capabilities_from_wire_evidence(&wire, snapshot_sha256)?;
    let diagnostics = vim_host_runner::diagnostics_from_wire_evidence(&wire);

    let mut limitations = vec![
        "headless silent-ex Vim (-es): GUI-only client surfaces are not exercised by this harness"
            .to_string(),
        "fixture bytes and expectations are Rust-authored from the landed #7762 artifact and the \
         #11388 activation catalog; they are never derived from run output"
            .to_string(),
        "activation receipts never imply semantic support: filetype, override, and attachment \
         dispositions stay separate propositions (activation_is_not_semantic_support)"
            .to_string(),
        "semantic dispositions exist only for the rows whose #7762 expectation claims Perl \
         support, and only where a real operation was observed through the governed root"
            .to_string(),
        format!(
            "fixture variant {}: the governed fixture arranges the #7762 denominator bytes",
            variant.id()
        ),
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
        activation_journey(&observation, &wire, &judgment),
        judgment.result,
        judgment.failure_class,
        limitations,
        format!(
            "#11403 {ACTIVATION_JOURNEY_SELECTOR}: finite #7762 expanded-activation denominator \
             executed per row through the exact subject — native filetype, bounded override, \
             vim-lsp attachment/languageId/root, semantic eligibility where claimed, and \
             ambiguity preservation — receipted into #11388 activation cells only"
        ),
    );
    let receipt_path = run.out_root.join("receipt.json");
    fs::write(&receipt_path, serde_json::to_vec_pretty(&receipt)?)
        .with_context(|| format!("writing receipt {}", receipt_path.display()))?;
    validate_receipt_binding(&receipt, &plan)
        .context("the emitted receipt failed its own freshness binding")?;
    Ok(ActivationRunOutcome {
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

/// Judge one observed run against the scenario's Rust-authored expectations.
///
/// The positive shape requires, for every denominator row: the retained
/// native observation matching the row's artifact expectation (exact match
/// for expectation-bearing rows, a completed observation for `observe` rows);
/// genuine activation (attachment carrying the Perl language surface) exactly
/// where the row genuinely activates natively or through its bounded
/// override, nowhere else; the two-source semantic proof for the claimed rows
/// and nothing else; the between-rows reset; and
/// negative-control/adjacency preservation everywhere.
#[allow(clippy::too_many_lines)]
pub fn evaluate_activation_observation(
    observation: &ProcessObservation,
    wire: &WireEvidence,
    variant: ActivationFixtureVariant,
) -> ActivationJudgment {
    let natives_index =
        index_events_by_row(&observation.events, DriverEventKind::ActivationNativeObserved);
    let overrides_index =
        index_events_by_row(&observation.events, DriverEventKind::ActivationOverrideApplied);
    let attachments_index =
        index_events_by_row(&observation.events, DriverEventKind::ActivationAttachmentObserved);
    let semantics_index =
        index_events_by_row(&observation.events, DriverEventKind::ActivationSemanticObserved);
    let resets_index =
        index_events_by_row(&observation.events, DriverEventKind::ActivationRowReset);

    let mut cells: BTreeMap<String, ObservationResult> = BTreeMap::new();

    let mut adjacency_intact = true;

    for row in ACTIVATION_DENOMINATOR {
        let slug = row.slug;
        let offset = row_offset(slug);
        let native_event = natives_index.get(&offset).copied();
        let override_event = overrides_index.get(&offset).copied();
        let attachment_event = attachments_index.get(&offset).copied();
        let semantic_event = semantics_index.get(&offset).copied();
        let reset_event = resets_index.get(&offset).copied();

        // --- native_filetype ------------------------------------------------
        let native_result = match native_event {
            None => ObservationResult::NotProven,
            Some(event) => {
                let observed = event_detail(event, "observed_filetype");
                if event_detail(event, "detection") != "native_vim"
                    || event_detail(event, "preset") != "0"
                {
                    // A forged state can never satisfy a native proposition.
                    adjacency_intact = false;
                    ObservationResult::Fail
                } else if row.expect == "observe" {
                    // Observation-only row: any directly observed terminal
                    // disposition (`unset` included) completes the cell.
                    ObservationResult::Pass
                } else if observed == row.expect {
                    ObservationResult::Pass
                } else {
                    // Includes the blanket-steal signature: perl observed on a
                    // distinct-language expectation row.
                    if observed == "perl" {
                        adjacency_intact = false;
                    }
                    ObservationResult::Fail
                }
            }
        };
        put_cell(&mut cells, slug, "native_filetype", native_result);

        // --- override ---------------------------------------------------------
        let override_result = if row.manual_override.is_some() {
            match override_event {
                Some(event)
                    if event_detail(event, "rule") == "narrow_exact_buffer_setf_perl"
                        && event_detail(event, "boundary")
                            == "not_authorized_by_extension_alone"
                        && event_detail(event, "filetype_after") == "perl" =>
                {
                    ObservationResult::Pass
                }
                Some(_) => ObservationResult::Fail,
                None => ObservationResult::NotProven,
            }
        } else if override_event.is_some() {
            // No override authorized for this row; any exercised override is
            // an unauthorized pathway.
            adjacency_intact = false;
            ObservationResult::Fail
        } else {
            ObservationResult::Unsupported
        };
        put_cell(&mut cells, slug, "override", override_result);
        let overridden = override_result == ObservationResult::Pass;

        // --- attachment --------------------------------------------------------
        let observed_natively_perl = native_event.is_some_and(|event| {
            event_detail(event, "observed_filetype") == "perl"
                && event_detail(event, "detection") == "native_vim"
                && event_detail(event, "preset") == "0"
        }) && !row.negative_control;
        let activates = observed_natively_perl || overridden;
        let attachment_result = if activates {
            match attachment_event {
                Some(event) if event_detail(event, "attached") == "1" => ObservationResult::Pass,
                Some(_) => ObservationResult::Fail,
                None => ObservationResult::NotProven,
            }
        } else if attachment_event.is_some() {
            // An activation arrived on a row with no legitimate route.
            adjacency_intact = false;
            ObservationResult::Fail
        } else {
            // The LSP client never exposed itself on this subject.
            ObservationResult::Unsupported
        };
        put_cell(&mut cells, slug, "attachment", attachment_result);

        // --- semantic_result ---------------------------------------------------
        let claimed = row.expect == "perl";
        let semantic_result = if !claimed {
            // activation_is_not_semantic_support: the row claims no Perl
            // semantic support, so no semantic disposition may exist here.
            ObservationResult::Unsupported
        } else if attachment_result != ObservationResult::Pass {
            ObservationResult::NotProven
        } else {
            let state_proof = semantic_event
                .and_then(|event| event_detail(event, "errors").parse::<u32>().ok())
                .is_some_and(|errors| errors >= 1);
            let wire_proof = row_semantic_wire_proof(wire, row.path);
            if state_proof && wire_proof {
                ObservationResult::Pass
            } else {
                // A claimed row that attached but produced no two-source real
                // operation is exactly the falsifier the issue forbids:
                // filetype/attachment filling a semantic cell. Fail typed.
                ObservationResult::Fail
            }
        };
        put_cell(&mut cells, slug, "semantic_result", semantic_result);

        // --- ambiguity_preserved -----------------------------------------------
        let ambiguity_result = if row.negative_control {
            // Distinct-language control: preserved exactly when its artifact
            // expectation was honored and nothing activated it.
            if native_result == ObservationResult::Pass
                && attachment_result == ObservationResult::Unsupported
            {
                ObservationResult::Pass
            } else {
                adjacency_intact = false;
                ObservationResult::Fail
            }
        } else if row.semantic_support == Some("independent") {
            // Adjacent-language subject: preservation means no Perl takeover
            // and no activation ever arrived.
            let stolen = native_event
                .is_some_and(|event| event_detail(event, "observed_filetype") == "perl")
                || attachment_event.is_some();
            if stolen {
                adjacency_intact = false;
                ObservationResult::Fail
            } else {
                ObservationResult::Pass
            }
        } else {
            // Authorized-deployment ambiguous names and bounded-override rows:
            // preserved while every activation came from a declared route
            // (native observation or the reviewed narrow override).
            let rogue =
                matches!(override_result, ObservationResult::Fail | ObservationResult::NotProven)
                    || matches!(attachment_result, ObservationResult::Fail);
            if rogue {
                adjacency_intact = false;
                ObservationResult::Fail
            } else {
                ObservationResult::Pass
            }
        };
        put_cell(&mut cells, slug, "ambiguity_preserved", ambiguity_result);

        // --- canonical cleanup load-bearing -------------------------------------
        if variant_requires_full_pass(variant)
            && reset_event.is_none()
            && native_result != ObservationResult::NotProven
        {
            // Missing between-rows evidence poisons the run instead of
            // silently passing.
            adjacency_intact = false;
            put_cell(&mut cells, slug, "ambiguity_preserved", ObservationResult::Fail);
        }
    }

    // Bootstrap/root substrate facts this journey also binds.
    let initialize_root_ok = wire
        .initialize_request
        .as_ref()
        .and_then(|request| request.get("params"))
        .and_then(|params| params.get("rootUri"))
        .and_then(|uri| uri.as_str())
        .is_some_and(|uri| uri_ends_with_segment(uri, GOVERNED_ROOT_REL));
    let bootstrap_observed =
        observation.events.iter().any(|event| event.kind == DriverEventKind::ServerInitialized);

    let driver_failed_event =
        observation.events.iter().find(|event| event.kind == DriverEventKind::DriverFailed);
    let driver_failure_reason =
        driver_failed_event.and_then(|event| event.details.get("reason")).cloned();

    let all_cells_sound = cells
        .values()
        .all(|result| matches!(result, ObservationResult::Pass | ObservationResult::Unsupported));
    let some_cell_passed = cells.values().any(|result| *result == ObservationResult::Pass);

    let passed_process_boundary =
        observation.passed_process_boundary() && bootstrap_observed && initialize_root_ok;

    let result = if passed_process_boundary
        && all_cells_sound
        && some_cell_passed
        && adjacency_intact
        && variant_requires_full_pass(variant)
        && driver_failure_reason.is_none()
    {
        ObservationResult::Pass
    } else if driver_failed_event.is_some()
        || observation.timed_out
        || observation.cleanup == CleanupResult::Fail
        || observation.status_code.is_some_and(|code| code != 0)
        || !passed_process_boundary
        || !adjacency_intact
        || cells.values().any(|result| *result == ObservationResult::Fail)
    {
        ObservationResult::Fail
    } else {
        ObservationResult::NotProven
    };

    let failure_class = if result == ObservationResult::Pass {
        None
    } else if observation.cleanup == CleanupResult::Fail {
        Some(crate::editor_client_compat::FailureClass::Cleanup)
    } else if observation.timed_out {
        Some(crate::editor_client_compat::FailureClass::Instrument)
    } else if !bootstrap_observed || !initialize_root_ok {
        Some(crate::editor_client_compat::FailureClass::HostClient)
    } else {
        Some(crate::editor_client_compat::FailureClass::Instrument)
    };

    ActivationJudgment {
        result,
        failure_class,
        driver_failure_reason,
        wrong_initialize_root: !initialize_root_ok,
        cells,
    }
}

/// Whether the variant's positive shape is required to fully pass. Only the
/// canonical variant carries that obligation; the red controls must fail.
fn variant_requires_full_pass(variant: ActivationFixtureVariant) -> bool {
    variant == ActivationFixtureVariant::Canonical
}

fn put_cell(
    cells: &mut BTreeMap<String, ObservationResult>,
    slug: &str,
    aspect: &str,
    result: ObservationResult,
) {
    cells.insert(activation_cell_id(slug, aspect), result);
}

/// The six-part judgment over one observed activation run.
pub struct ActivationJudgment {
    pub result: ObservationResult,
    pub failure_class: Option<crate::editor_client_compat::FailureClass>,
    pub driver_failure_reason: Option<String>,
    /// The initialize request's rootUri disagreed with the governed root.
    pub wrong_initialize_root: bool,
    /// Per-cell results for the receipt journey, keyed by catalog cell id
    /// (exactly the 18 rows x 5 aspects of the #11388 activation catalog).
    pub cells: BTreeMap<String, ObservationResult>,
}

/// The receipt journey cell id of one denominator row/aspect pair.
pub fn activation_cell_id(slug: &str, aspect: &str) -> String {
    format!("{ACTIVATION_CELL_PREFIX}{slug}_{aspect}")
}

fn row_offset(slug: &str) -> usize {
    ACTIVATION_DENOMINATOR.iter().position(|row| row.slug == slug).unwrap_or(usize::MAX)
}

/// One detail value of a driver event, or the empty string when absent.
fn event_detail<'a>(event: &'a DriverEvent, key: &str) -> &'a str {
    event.details.get(key).map(String::as_str).unwrap_or("")
}

/// Index repeating activation events by their `row_index` detail.
fn index_events_by_row(
    events: &[DriverEvent],
    kind: DriverEventKind,
) -> BTreeMap<usize, &DriverEvent> {
    let mut index = BTreeMap::new();
    for event in events.iter().filter(|event| event.kind == kind) {
        let raw = event.details.get("row_index").map(String::as_str).unwrap_or("");
        if let Ok(row_index) = raw.parse::<usize>() {
            index.insert(row_index, event);
        }
    }
    index
}

/// Two-source wire leg for a claimed row: at least one parser-coded,
/// error-severity publishDiagnostics batch for this row's own document tail.
fn row_semantic_wire_proof(wire: &WireEvidence, path: &str) -> bool {
    let tail = row_uri_tail(path);
    wire.publish_diagnostics_batches.iter().any(|batch| {
        batch.uri_file == tail && batch.error_severity_count >= 1 && batch.parser_code_count >= 1
    })
}

/// Whether a `file://` URI ends with the expected relative directory segment,
/// on every host's path spelling.
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
/// surface) plus exactly the 90 #11388 activation cells this scenario
/// evidences.
#[allow(clippy::too_many_lines)]
pub fn activation_journey(
    observation: &ProcessObservation,
    wire: &WireEvidence,
    judgment: &ActivationJudgment,
) -> Vec<JourneyCell> {
    let mut cells = crate::vim_host_run::outcome_journey(observation, wire);
    for row in ACTIVATION_DENOMINATOR {
        for &aspect in ACTIVATION_ASPECTS {
            let cell_id = activation_cell_id(row.slug, aspect);
            let result =
                judgment.cells.get(&cell_id).copied().unwrap_or(ObservationResult::NotProven);
            cells.push(JourneyCell {
                id: cell_id.clone(),
                capability_basis: CapabilityBasis::NotApplicable,
                observed: result == ObservationResult::Pass,
                result,
                evidence: activation_cell_evidence(aspect),
                limitation: activation_cell_limitation(row.slug, aspect, result),
            });
        }
    }
    cells
}

fn activation_cell_evidence(aspect: &str) -> Vec<String> {
    match aspect {
        "attachment" | "semantic_result" => vec![
            "vim/driver-events.jsonl".to_string(),
            "vim/vim-lsp-client.log".to_string(),
            "vim/initialize-request.json".to_string(),
        ],
        "override" | "ambiguity_preserved" => {
            vec!["vim/driver-events.jsonl".to_string(), "vim/vim-lsp-client.log".to_string()]
        }
        _ => vec!["vim/driver-events.jsonl".to_string()],
    }
}

fn activation_cell_limitation(
    slug: &str,
    aspect: &str,
    result: ObservationResult,
) -> Option<String> {
    match result {
        ObservationResult::Pass => Some(match aspect {
            "native_filetype" => {
                "native detection retained before any override; never a semantic-support claim"
                    .to_string()
            }
            "override" => "bounded override through one narrow reviewed user-equivalent rule; the \
                 extension alone stays unauthorized (not_authorized_by_extension_alone)"
                .to_string(),
            "attachment" => {
                "activation_only: vim-lsp attachment and languageId identity, never semantic \
                 support (activation_is_not_semantic_support)"
                    .to_string()
            }
            "semantic_result" => {
                "two-source semantic discriminator on the row's own document through the \
                 governed root"
                    .to_string()
            }
            _ => "ambiguity/adjacent-language identity stayed intact for this row".to_string(),
        }),
        ObservationResult::Unsupported => Some(match aspect {
            "override" => {
                format!("row {slug}: the #7762 row authorizes no override; none was applied")
            }
            "attachment" => {
                format!(
                    "row {slug}: the LSP client never activated on this subject \
                     (client_not_exposed; the adjacent language stays protected)"
                )
            }
            "semantic_result" => {
                format!(
                    "row {slug}: activation_is_not_semantic_support — the #7762 row claims no \
                     Perl semantic support, so no semantic disposition exists here"
                )
            }
            _ => format!("row {slug}: nothing was exposed to observe"),
        }),
        _ => Some(format!("{slug}_{aspect} was not proven for this exact subject")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vim_host_runner::{DRIVER_SCHEMA_VERSION, PublishDiagnosticsBatch};

    use crate::editor_client_compat::CleanupResult as Cleanup;

    fn event(kind: DriverEventKind, sequence: u64, details: &[(&str, &str)]) -> DriverEvent {
        DriverEvent {
            schema_version: DRIVER_SCHEMA_VERSION.to_string(),
            sequence,
            kind,
            details: details
                .iter()
                .map(|(key, value)| (key.to_string(), value.to_string()))
                .collect(),
        }
    }

    fn cell(judgment: &ActivationJudgment, slug: &str, aspect: &str) -> ObservationResult {
        judgment
            .cells
            .get(&activation_cell_id(slug, aspect))
            .copied()
            .unwrap_or(ObservationResult::NotProven)
    }

    /// The complete honest canonical stream: the row-0 substrate singletons
    /// first (in their rank order), then every denominator row's repeating
    /// activation observations in artifact order, then shutdown.
    fn canonical_stream() -> Vec<DriverEvent> {
        let mut sequence = 0_u64;
        let mut stream = vec![
            event(
                DriverEventKind::HostStarted,
                {
                    sequence += 1;
                    sequence
                },
                &[],
            ),
            event(
                DriverEventKind::ClientLoaded,
                {
                    sequence += 1;
                    sequence
                },
                &[],
            ),
            event(
                DriverEventKind::RegistrationSelected,
                {
                    sequence += 1;
                    sequence
                },
                &[("cmd", "perllsp--stdio"), ("candidate_sha256", "a".repeat(64).as_str())],
            ),
            event(
                DriverEventKind::FixtureOpened,
                {
                    sequence += 1;
                    sequence
                },
                &[],
            ),
            event(
                DriverEventKind::ServerInitialized,
                {
                    sequence += 1;
                    sequence
                },
                &[],
            ),
            event(
                DriverEventKind::BufferEnabled,
                {
                    sequence += 1;
                    sequence
                },
                &[("filetype", "perl"), ("detection", "native_vim")],
            ),
            event(
                DriverEventKind::InitializeObserved,
                {
                    sequence += 1;
                    sequence
                },
                &[],
            ),
            event(
                DriverEventKind::RootSelected,
                {
                    sequence += 1;
                    sequence
                },
                &[
                    ("root_source", "activation_root_marker"),
                    ("expected_root", GOVERNED_ROOT_REL),
                    ("observed_root", GOVERNED_ROOT_REL),
                    ("decoy_root", DECOY_ROOT_REL),
                ],
            ),
            event(
                DriverEventKind::DiagnosticsObserved,
                {
                    sequence += 1;
                    sequence
                },
                &[("mode", "push"), ("evidence", "client_log")],
            ),
        ];

        // The landed artifact's own expectations decide the observations.
        for (index, row) in ACTIVATION_DENOMINATOR.iter().enumerate() {
            let slug = row.slug;
            let observed = match row.path {
                "sample.pl" | "legacy.PL" | "Sample.pm" | "sample.t" | "app.psgi" | "bin/tool"
                | "script/tool" => "perl",
                "Image.pm" => "xpm",
                "game.t" => "tads",
                "notes.pod" => "pod",
                "Native.xs" => "xs",
                "view.mason" => "mason",
                _ => "unset",
            };
            stream.push(event(
                DriverEventKind::ActivationNativeObserved,
                {
                    sequence += 1;
                    sequence
                },
                &[
                    ("row_index", index.to_string().as_str()),
                    ("row", slug),
                    ("observed_filetype", observed),
                    ("detection", "native_vim"),
                    ("preset", "0"),
                ],
            ));

            if observed == "perl" && !row.negative_control && row.expect == "perl" {
                stream.push(event(
                    DriverEventKind::ActivationAttachmentObserved,
                    {
                        sequence += 1;
                        sequence
                    },
                    &[
                        ("row_index", index.to_string().as_str()),
                        ("row", slug),
                        ("language_id", "perl"),
                        ("attached", "1"),
                    ],
                ));
                stream.push(event(
                    DriverEventKind::ActivationSemanticObserved,
                    {
                        sequence += 1;
                        sequence
                    },
                    &[
                        ("row_index", index.to_string().as_str()),
                        ("row", slug),
                        ("state_source", "client_state"),
                        ("errors", "1"),
                    ],
                ));
            } else if row.manual_override.is_some() {
                stream.push(event(
                    DriverEventKind::ActivationOverrideApplied,
                    {
                        sequence += 1;
                        sequence
                    },
                    &[
                        ("row_index", index.to_string().as_str()),
                        ("row", slug),
                        ("rule", "narrow_exact_buffer_setf_perl"),
                        ("boundary", "not_authorized_by_extension_alone"),
                        ("filetype_after", "perl"),
                    ],
                ));
                stream.push(event(
                    DriverEventKind::ActivationAttachmentObserved,
                    {
                        sequence += 1;
                        sequence
                    },
                    &[
                        ("row_index", index.to_string().as_str()),
                        ("row", slug),
                        ("language_id", "perl"),
                        ("attached", "1"),
                    ],
                ));
            }

            stream.push(event(
                DriverEventKind::ActivationRowReset,
                {
                    sequence += 1;
                    sequence
                },
                &[
                    ("row_index", index.to_string().as_str()),
                    ("row", slug),
                    ("reset", "buffer_close"),
                ],
            ));
        }

        stream.push(event(
            DriverEventKind::ShutdownStarted,
            {
                sequence += 1;
                sequence
            },
            &[],
        ));
        stream.push(event(
            DriverEventKind::ShutdownCompleted,
            {
                sequence += 1;
                sequence
            },
            &[],
        ));
        stream
    }

    fn canonical_wire() -> WireEvidence {
        let mut batches = Vec::new();
        let mut line = 10_usize;
        for row in ACTIVATION_DENOMINATOR.iter().filter(|row| row.expect == "perl") {
            batches.push(PublishDiagnosticsBatch {
                line_index: line,
                uri_file: row_uri_tail(row.path).to_string(),
                diagnostics_count: 1,
                error_severity_count: 1,
                parser_code_count: 1,
            });
            line += 5;
        }
        WireEvidence {
            initialize_request: Some(serde_json::json!({
                "params": {"rootUri": format!("file:///{GOVERNED_ROOT_REL}")}
            })),
            publish_diagnostics_batches: batches,
            ..WireEvidence::default()
        }
    }

    fn observation(events: Vec<DriverEvent>, status: i32, cleanup: Cleanup) -> ProcessObservation {
        ProcessObservation {
            status_code: Some(status),
            timed_out: false,
            kill_requested: false,
            cleanup,
            cleanup_detail: String::new(),
            events,
            driver_complete: true,
            artifacts: Vec::new(),
        }
    }

    #[test]
    fn variants_parse_and_carry_typed_negative_reasons() {
        assert_eq!(
            ActivationFixtureVariant::from_id("canonical").expect("canonical"),
            ActivationFixtureVariant::Canonical
        );
        assert_eq!(
            ActivationFixtureVariant::from_id("preset_filetype_claimed")
                .expect("preset")
                .expected_negative_reason(),
            Some("pre_forced_filetype_not_native")
        );
        assert_eq!(
            ActivationFixtureVariant::from_id("blanket_override_steal")
                .expect("blanket")
                .expected_negative_reason(),
            Some("adjacent_language_stolen")
        );
        assert!(ActivationFixtureVariant::from_id("other").is_err());
    }

    #[test]
    fn every_denominator_row_has_authored_fixture_bytes() {
        for row in ACTIVATION_DENOMINATOR {
            let body = row_fixture_bytes(row.path).expect("authored bytes");
            assert!(body.ends_with('\n'), "row {} bytes end with newline", row.path);
            if row.expect == "perl" {
                assert!(
                    body.contains(DEFECT_LINE_TEXT),
                    "claimed row {} embeds the governed defect",
                    row.path
                );
            }
        }
    }

    #[test]
    fn cited_cells_are_exactly_the_registered_catalog_cells() {
        let judgment = evaluate_activation_observation(
            &observation(Vec::new(), 0, Cleanup::Pass),
            &WireEvidence::default(),
            ActivationFixtureVariant::Canonical,
        );
        let mut cited: Vec<String> = judgment.cells.keys().cloned().collect();
        cited.sort();

        let mut registered: Vec<String> = crate::vim_lsp_cell_catalog::registry()
            .iter()
            .flat_map(|catalog| catalog.cells.iter().map(|entry| entry.cell_id.clone()))
            .filter(|cell_id| cell_id.starts_with(ACTIVATION_CELL_PREFIX))
            .collect();
        registered.sort();

        assert_eq!(
            cited.len(),
            ACTIVATION_DENOMINATOR.len() * ACTIVATION_ASPECTS.len(),
            "the journey judges exactly rows x aspects"
        );
        assert_eq!(cited, registered, "receipt citations must equal #11388 registration");
    }

    #[test]
    fn honest_canonical_stream_passes_with_bounded_dispositions() {
        let judgment = evaluate_activation_observation(
            &observation(canonical_stream(), 0, Cleanup::Pass),
            &canonical_wire(),
            ActivationFixtureVariant::Canonical,
        );
        assert_eq!(judgment.result, ObservationResult::Pass, "cells: {:?}", judgment.cells);
        for row in ACTIVATION_DENOMINATOR {
            if row.expect == "perl" {
                for aspect in ["native_filetype", "attachment", "semantic_result"] {
                    assert_eq!(
                        cell(&judgment, row.slug, aspect),
                        ObservationResult::Pass,
                        "{}/{aspect}",
                        row.slug
                    );
                }
            }
            if row.negative_control {
                assert_eq!(cell(&judgment, row.slug, "native_filetype"), ObservationResult::Pass);
                assert_eq!(
                    cell(&judgment, row.slug, "ambiguity_preserved"),
                    ObservationResult::Pass
                );
            }
            if row.manual_override.is_some() {
                assert_eq!(cell(&judgment, row.slug, "override"), ObservationResult::Pass);
            } else {
                assert_eq!(cell(&judgment, row.slug, "override"), ObservationResult::Unsupported);
            }
            if row.semantic_support == Some("independent") {
                assert_eq!(cell(&judgment, row.slug, "attachment"), ObservationResult::Unsupported);
                assert_eq!(
                    cell(&judgment, row.slug, "ambiguity_preserved"),
                    ObservationResult::Pass
                );
            }
        }
    }

    #[test]
    fn blanket_theft_fails_typed_on_the_stolen_control() {
        let stolen_offset = ACTIVATION_DENOMINATOR
            .iter()
            .position(|row| row.slug == "t_tads")
            .expect("denominator row");
        let mut events = canonical_stream();

        // Replace t_tads' native observation with a stolen Perl state.
        let position = events
            .iter()
            .position(|item| {
                item.kind == DriverEventKind::ActivationNativeObserved
                    && item.details.get("row_index").map(String::as_str)
                        == Some(stolen_offset.to_string().as_str())
            })
            .expect("stolen control native event");
        events[position] = event(
            DriverEventKind::ActivationNativeObserved,
            events[position].sequence,
            &[
                ("row_index", stolen_offset.to_string().as_str()),
                ("row", "t_tads"),
                ("observed_filetype", "perl"),
                ("detection", "native_vim"),
                ("preset", "0"),
            ],
        );
        // The driver's typed refusal rides last.
        let last_sequence = events.last().map(|item| item.sequence).unwrap_or_default();
        events.push(event(
            DriverEventKind::DriverFailed,
            last_sequence + 1,
            &[("reason", "adjacent_language_stolen")],
        ));

        let judgment = evaluate_activation_observation(
            &observation(events, 2, Cleanup::Pass),
            &canonical_wire(),
            ActivationFixtureVariant::BlanketOverrideSteal,
        );
        assert_eq!(judgment.result, ObservationResult::Fail);
        assert_eq!(judgment.driver_failure_reason.as_deref(), Some("adjacent_language_stolen"));
        assert_eq!(cell(&judgment, "t_tads", "native_filetype"), ObservationResult::Fail);
        assert_eq!(cell(&judgment, "t_tads", "ambiguity_preserved"), ObservationResult::Fail);
    }

    #[test]
    fn claimed_attachment_without_real_operation_fails() {
        // The falsifier the issue names directly: correct filetype and
        // attachment must not fill a semantic cell without a real operation.
        let mut events = canonical_stream();
        events.retain(|item| item.kind != DriverEventKind::ActivationSemanticObserved);

        let judgment = evaluate_activation_observation(
            &observation(events, 0, Cleanup::Pass),
            &canonical_wire(),
            ActivationFixtureVariant::Canonical,
        );
        assert_eq!(judgment.result, ObservationResult::Fail);
        assert_eq!(cell(&judgment, "pl", "semantic_result"), ObservationResult::Fail);
        assert_eq!(cell(&judgment, "pl", "attachment"), ObservationResult::Pass);
    }

    #[test]
    fn pre_forced_filetype_never_counts_as_native() {
        let mut events: Vec<DriverEvent> = Vec::new();
        let mut sequence = 0_u64;
        events.push(event(
            DriverEventKind::HostStarted,
            {
                sequence += 1;
                sequence
            },
            &[],
        ));
        events.push(event(
            DriverEventKind::FixtureOpened,
            {
                sequence += 1;
                sequence
            },
            &[("bootstrap_row", "pl")],
        ));
        events.push(event(
            DriverEventKind::ActivationNativeObserved,
            {
                sequence += 1;
                sequence
            },
            &[
                ("row_index", "0"),
                ("row", "pl"),
                ("observed_filetype", "perl"),
                ("detection", "pre_forced"),
                ("preset", "1"),
            ],
        ));
        events.push(event(
            DriverEventKind::ShutdownStarted,
            {
                sequence += 1;
                sequence
            },
            &[],
        ));
        events.push(event(
            DriverEventKind::ShutdownCompleted,
            {
                sequence += 1;
                sequence
            },
            &[],
        ));
        events.push(event(
            DriverEventKind::DriverFailed,
            {
                sequence += 1;
                sequence
            },
            &[("reason", "pre_forced_filetype_not_native")],
        ));

        let judgment = evaluate_activation_observation(
            &observation(events, 2, Cleanup::Pass),
            &WireEvidence::default(),
            ActivationFixtureVariant::PresetFiletypeClaimed,
        );
        assert_eq!(judgment.result, ObservationResult::Fail);
        assert_eq!(
            judgment.driver_failure_reason.as_deref(),
            Some("pre_forced_filetype_not_native")
        );
        assert_eq!(cell(&judgment, "pl", "native_filetype"), ObservationResult::Fail);
    }

    #[test]
    fn materialized_fixture_carries_every_denominator_document() {
        let scratch =
            std::env::temp_dir().join(format!("plsw-11403-fixture-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&scratch);
        let fixture_root = scratch.join("out");
        let fixture =
            materialize_activation_fixture(&fixture_root, ActivationFixtureVariant::Canonical)
                .expect("fixture materializes");
        for row in ACTIVATION_DENOMINATOR {
            let path = fixture.root.join("workspace/project").join(row.path);
            assert!(path.is_file(), "{} exists", path.display());
        }
        assert!(fixture.root.join(DECOY_SAME_NAME_FILE_REL).is_file());
        let _ = std::fs::remove_dir_all(&scratch);
    }
}

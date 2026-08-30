//! #11396 save-format scenario for the hermetic Vim + vim-lsp host runner.
//!
//! This module is the format-on-save execution consumer of the #10944/#12545
//! substrate and the #12589/#12660 scenario pattern: it proves, through the
//! pinned actual Vim + vim-lsp + perllsp subject, the #11384 save cells —
//! route, invocation cardinality, applied formatting, legitimate no-change,
//! disabled/refused, failure, and stale-result rejection — using only the
//! routes the exact subject genuinely supports.
//!
//! Route classification (source-backed, re-proven on every run's own wire):
//!
//! - **the pinned client ships no built-in format-on-save surface.** Its own
//!   `initialize` request advertises `textDocument.synchronization.willSave:
//!   false` and `willSaveWaitUntil: false` (mined from the run's wire; a
//!   client that someday offers them is a different route row), and the pinned
//!   checkout registers no save autocmd of its own.
//! - **selected route: the documented repository autocmd delegating to the
//!   canonical client action.** `BufWritePre *.pl` → `execute('LspDocument
//!   FormatSync')`, bounded by `g:lsp_format_sync_timeout` — exactly the
//!   pattern the pinned client documents (`doc/vim-lsp.txt`,
//!   `:LspDocumentFormatSync`: "Useful when running |:autocmd| commands such
//!   as formatting before save"; `README.md`'s format-on-save example).
//! - **rejected alternatives:** `willSaveWaitUntil` (not exposed by this
//!   client), the async `:LspDocumentFormat` as a save owner (cannot settle
//!   before the write; the documented save pattern is the sync command), and
//!   manual commands (the pinned #11380 `manual_comparator` control — a
//!   negative subject only, never a scenario owner).
//!
//! Disposition semantics consumed (perllsp, verified in-tree):
//!
//! - applied: the native formatter returns edits; the client applies them to
//!   the buffer inside `BufWritePre`, so both buffer and file bytes settle to
//!   the Rust-authored canonical text on the ordinary write.
//! - legitimate no-change: already-canonical source still triggers exactly
//!   one request (route executed — proven by the request-count delta, never
//!   assumed), the server returns `"result":[]`, and the bytes stay exact.
//! - disabled: the owner is removed (`owner_count = 0`); the ordinary save
//!   emits zero formatting requests inside a bounded absence window and the
//!   non-canonical bytes survive — distinct from no-change through the
//!   authored byte oracle.
//! - refused: `[formatting] engine = "off"` (restart-required project
//!   config) makes the server return `"result":[]` for non-canonical source —
//!   a wire shape it shares with no-change, so the refusal is discriminated
//!   by the non-canonical bytes a legitimate no-change can never carry.
//! - failure: `[formatting] engine = "external-perltidy"` with a missing
//!   profile is a real engine failure — the server answers a JSON-RPC error
//!   the client surfaces through its format-error path, no edits apply, and
//!   the non-canonical bytes survive.
//! - stale/cancelled: a save-format sync timeout shorter than the round trip
//!   releases the result after the write already happened; the late response
//!   can never apply (the client's wait already aborted), which the journey
//!   proves through post-write settlement observation plus a bounded
//!   bytes-held window on a large authored document whose formatting round
//!   trip is orders of magnitude beyond the 1ms timeout.
//!
//! Ownership split (consumed, never duplicated):
//!
//! - `vim_host_run::vim_host_runner` (#10944) owns hermetic launch,
//!   supervision, process ledgers, cleanup comparison, generic wire mining,
//!   and receipt composition. This module owns the save fixture variants, the
//!   scenario-local save wire mining (direction-aware request/response/error
//!   counts), the seven-cell judgment, and the scenario receipt.
//! - `vim_lsp_cell_catalog` (#11384) owns cell registration; this module
//!   cites catalog cell ids in its receipt journey but never edits a catalog.
//! - The expectation oracle lives here in Rust — the canonical and
//!   non-canonical texts, the bulk stale document, the TOML generations, and
//!   every expected digest — never derived from the responses under test
//!   (#10938 law), and never embedded in Vimscript beyond delivery through
//!   the environment channel.
//!
//! Fail-closed laws beyond the substrate's:
//!
//! - every save-triggered settlement requires the client's own wire record
//!   (outgoing `textDocument/formatting` request count delta and the settled
//!   response kind), never a log echo alone;
//! - the applied and no-change cells bind exact buffer AND file bytes through
//!   sha256 identities computed over Rust-authored texts;
//! - a no-change claim without its request (falsifier 5) cannot pass: the
//!   request delta must be exactly one and the response must have settled;
//! - a refusal flattened into no-change (falsifier 6) cannot pass: no-change
//!   requires canonical bytes, the refused leg requires non-canonical bytes,
//!   and both bind the same authored digests;
//! - the failure cell admits no pass: its receipt result is the honest `fail`
//!   disposition with `observed = true`, and the run-level pass requires that
//!   failure to have been recorded distinctly (#11384 law);
//! - a stale result that applies (falsifier 7) cannot pass: the bytes must
//!   hold non-canonical through the bounded window while the late response
//!   settles on the wire;
//! - negative fixture variants (`manual_comparator_only`, `duplicate_owner`,
//!   `wrong_root_decoy`) are expected to fail with typed reasons; a pass on a
//!   negative variant is an oracle violation, never a green run.

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

pub const SAVE_FORMAT_JOURNEY_SELECTOR: &str = "vim_vim_lsp_save_format.v1";
pub const SAVE_FORMAT_FIXTURE_ID: &str = "vim_vim_lsp_save_format_v1";

// ---------------------------------------------------------------------------
// Rust-authored fixture expectations
// ---------------------------------------------------------------------------

/// The governed fixture's stable layout, relative to the materialized fixture
/// root. Authored here, never derived from run output.
pub const GOVERNED_ROOT_REL: &str = "workspace/project";
pub const DECOY_ROOT_REL: &str = "workspace";
pub const OPENED_FILE_REL: &str = "workspace/project/main.pl";
pub const DECOY_FILE_REL: &str = "workspace/main.pl";
pub const BULK_FILE_REL: &str = "workspace/project/bulk.pl";
/// The governed root marker (the #7762 authority-list marker the earlier
/// journeys established).
pub const ROOT_MARKER: &str = "cpanfile";

/// Wire file-name tokens (publishDiagnostics `uri` tails).
pub const MAIN_TOKEN: &str = "main.pl";
pub const BULK_TOKEN: &str = "bulk.pl";

/// The deliberately non-canonical governed source: valid, diagnostics-clean
/// Perl whose formatting the native formatter deterministically rewrites
/// (spacing around operators, spaces after keywords and commas, block
/// indentation, braces on their own lines). Authored from the formatter's own
/// published layout laws, never from a run.
pub const NON_CANONICAL_LINES: [&str; 10] = [
    "use strict;",
    "use warnings;",
    "my $seed=3;",
    "my $value=compute($seed);",
    "sub compute{",
    "my($input)=@_;",
    "if($input==1){return $input+1;}",
    "return $input*2;",
    "}",
    "print \"$value\\n\";",
];

/// The canonical generation: the exact post-format bytes the ordinary save
/// must settle to in both the buffer and the file. Authored against the
/// formatter's deterministic layout laws *under the client-delivered LSP
/// options*: the hermetic host (`-Nu NONE`, no vimrc) runs Vim's defaults —
/// `noexpandtab` — so vim-lsp's format request carries `insertSpaces: false`
/// and the native formatter indents re-blocked statements with tabs. The
/// sub header keeps its attached brace (only re-blocked `if` headers are
/// split onto their own line); the expanded `if` body is one tab deep.
pub const CANONICAL_LINES: [&str; 12] = [
    "use strict;",
    "use warnings;",
    "my $seed = 3;",
    "my $value = compute($seed);",
    "sub compute{",
    "my ($input) = @_;",
    "if ($input == 1) {",
    "\treturn $input + 1;",
    "}",
    "return $input * 2;",
    "}",
    "print \"$value\\n\";",
];

/// The decoy same-named file at the outer root carries the canonical bytes so
/// the wrong-root negative stays diagnostics-relevant; it is never opened by
/// the canonical journey.
pub const DECOY_LINES: [&str; 3] = ["use strict;", "use warnings;", "print \"outer decoy\\n\";"];

/// Blocks in the authored bulk stale document. Sized so the formatting round
/// trip (request → native format of every block → response → client dispatch)
/// is orders of magnitude beyond the 1ms stale timeout, making the
/// timeout-before-response outcome deterministic rather than a race.
pub const BULK_BLOCKS: usize = 400;

/// The bounded sync timeout for save-triggered formatting in every ordinary
/// leg. Generous against a cold server, still parent-bounded.
pub const SAVE_SYNC_TIMEOUT_MS: u64 = 30_000;
/// The stale-leg sync timeout: shorter than any possible formatting round
/// trip, so the sync wait aborts before the result settles.
pub const STALE_SYNC_TIMEOUT_MS: u64 = 1;
/// The bounded bytes-held observation window after the late result settles.
pub const STALE_WINDOW_MS: u64 = 5000;

/// The project-config generations, authored here and delivered to the driver
/// through the environment (never embedded in Vimscript).
pub const TOML_OFF_TEXT: &str = "[formatting]\nengine = \"off\"\n";
pub const TOML_EXTERNAL_TEXT: &str = "[formatting]\nengine = \"external-perltidy\"\nperltidy_profile = \"missing_save_format_profile.perltidyrc\"\n";

/// The #11384 catalog cell ids this journey evidences. The catalog owns
/// registration; this scenario only cites.
pub const CELL_ROUTE: &str = "vim.vim_lsp.save.route";
pub const CELL_CARDINALITY: &str = "vim.vim_lsp.save.invocation_cardinality";
pub const CELL_APPLIED: &str = "vim.vim_lsp.save.format_applied";
pub const CELL_NO_CHANGE: &str = "vim.vim_lsp.save.format_no_change";
pub const CELL_DISABLED: &str = "vim.vim_lsp.save.disabled_or_refused";
pub const CELL_FAILURE: &str = "vim.vim_lsp.save.failure";
pub const CELL_STALE: &str = "vim.vim_lsp.save.stale_result_rejected";

// ---------------------------------------------------------------------------
// Fixture variants
// ---------------------------------------------------------------------------

/// One scenario fixture variant. `Canonical` must pass; the three negative
/// variants must fail with their typed reason (the red-first controls).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveFormatFixtureVariant {
    Canonical,
    /// No save owner is ever configured; a manual `:LspDocumentFormatSync`
    /// comparator produces the canonical bytes, but the load-bearing trigger
    /// is absent and the journey must reject it.
    ManualComparatorOnly,
    /// Two identical save owners are armed: one save issues two formatting
    /// requests and the cardinality law must reject it.
    DuplicateOwner,
    /// The #7762 marker moves to the outer workspace: native resolution
    /// selects the decoy root and the journey must reject it.
    WrongRootDecoy,
}

impl SaveFormatFixtureVariant {
    pub fn from_id(id: &str) -> Result<Self> {
        match id {
            "canonical" => Ok(Self::Canonical),
            "manual_comparator_only" => Ok(Self::ManualComparatorOnly),
            "duplicate_owner" => Ok(Self::DuplicateOwner),
            "wrong_root_decoy" => Ok(Self::WrongRootDecoy),
            other => bail!(
                "unknown save-format fixture variant {other}: known variants are canonical, \
                 manual_comparator_only, duplicate_owner, wrong_root_decoy"
            ),
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Self::Canonical => "canonical",
            Self::ManualComparatorOnly => "manual_comparator_only",
            Self::DuplicateOwner => "duplicate_owner",
            Self::WrongRootDecoy => "wrong_root_decoy",
        }
    }

    /// The typed driver-failure reason this variant must produce; `None` for
    /// the canonical variant, which must pass.
    pub fn expected_negative_reason(self) -> Option<&'static str> {
        match self {
            Self::Canonical => None,
            Self::ManualComparatorOnly => Some("save_trigger_absent"),
            Self::DuplicateOwner => Some("duplicate_invocation_observed"),
            Self::WrongRootDecoy => Some("root_mismatch"),
        }
    }
}

/// The materialized governed fixture for one variant.
pub struct SaveFormatFixture {
    pub root: PathBuf,
    pub variant: SaveFormatFixtureVariant,
}

/// Materialize the #11396 governed fixture under `root`:
///
/// ```text
/// workspace/                      <- outer decoy root (no marker, canonical)
///   main.pl                       <- same-named decoy file (canonical bytes)
///   cpanfile                      <- marker ONLY in the wrong_root_decoy variant
///   project/                      <- the governed #7762 root
///     cpanfile                    <- the governed root marker (all but decoy)
///     main.pl                     <- the governed source (non-canonical G1)
///     bulk.pl                     <- the large stale-leg document (non-canonical)
/// ```
///
/// No `.perl-lsp.toml` exists initially: the refused/failure config
/// generations are created during the journey through external mutations and
/// server restarts, exactly like the #11390 config lifecycle. The fixture
/// digest recorded in the run plan pins exactly this initial state.
pub fn materialize_save_format_fixture(
    root: &Path,
    variant: SaveFormatFixtureVariant,
) -> Result<SaveFormatFixture> {
    ensure!(root.is_absolute(), "fixture root must be absolute");
    let workspace = root.join("workspace");
    let project = workspace.join("project");
    fs::create_dir_all(&project).with_context(|| format!("creating {}", project.display()))?;
    write_lines(&project.join("main.pl"), &NON_CANONICAL_LINES)?;
    fs::write(project.join("bulk.pl"), bulk_non_canonical_text())?;
    write_lines(&workspace.join("main.pl"), &DECOY_LINES)?;
    let marker = "# vim/vim-lsp #11396 governed root marker (cpanfile per #7762)\n";
    match variant {
        SaveFormatFixtureVariant::WrongRootDecoy => {
            fs::write(workspace.join(ROOT_MARKER), marker)?;
        }
        _ => {
            fs::write(project.join(ROOT_MARKER), marker)?;
        }
    }
    Ok(SaveFormatFixture { root: root.to_path_buf(), variant })
}

fn write_lines(path: &Path, lines: &[&str]) -> Result<()> {
    let mut text = String::new();
    for line in lines {
        text.push_str(line);
        text.push('\n');
    }
    fs::write(path, text).with_context(|| format!("writing {}", path.display()))
}

/// The full source texts delivered to the driver (the byte oracle: authored
/// here, applied verbatim by the driver for the disabled-leg re-mutation).
pub fn canonical_source_text() -> String {
    let mut text = CANONICAL_LINES.join("\n");
    text.push('\n');
    text
}

pub fn non_canonical_source_text() -> String {
    let mut text = NON_CANONICAL_LINES.join("\n");
    text.push('\n');
    text
}

/// The large non-canonical stale-leg document. Only its parse validity and
/// round-trip cost matter: its formatted form is never applied and never
/// authored.
pub fn bulk_non_canonical_text() -> String {
    let mut lines = Vec::with_capacity(BULK_BLOCKS * 2 + 3);
    lines.push("use strict;".to_string());
    lines.push("use warnings;".to_string());
    for index in 0..BULK_BLOCKS {
        lines.push(format!("my $v{index}={};", index + 1));
        lines.push(format!("if($v{index}=={index}){{my $w{index}=$v{index}+1;}}"));
    }
    lines.push("print \"bulk done\\n\";".to_string());
    let mut text = lines.join("\n");
    text.push('\n');
    text
}

/// The sha256 identity Vim's `sha256()` computes over the same bytes, in the
/// substrate's `sha256:<hex>` form.
pub fn text_sha256(text: &str) -> Result<String> {
    vim_host_runner::bytes_sha256(text.as_bytes())
}

/// The scenario's environment contract beyond the substrate's: the
/// Rust-authored expectations delivered to the driver (never re-derived in
/// Vimscript). The byte identities are raw 64-char hex — the form Vim's own
/// `sha256()` returns — while the driver's settlement events carry the
/// substrate's `sha256:<hex>` identity form.
pub fn save_format_env(
    variant: SaveFormatFixtureVariant,
) -> Vec<(std::ffi::OsString, std::ffi::OsString)> {
    let raw_hex = |text: &str| {
        text_sha256(text)
            .map(|identity| identity.trim_start_matches("sha256:").to_string())
            .unwrap_or_default()
    };
    let canonical_sha = raw_hex(&canonical_source_text());
    let non_canonical_sha = raw_hex(&non_canonical_source_text());
    let bulk_sha = raw_hex(&bulk_non_canonical_text());
    let pairs = [
        ("PERLLSP_VIM_HOST_SAVE_VARIANT", variant.id().to_string()),
        ("PERLLSP_VIM_HOST_OPENED_FILE_REL", OPENED_FILE_REL.to_string()),
        ("PERLLSP_VIM_HOST_EXPECTED_ROOT_REL", GOVERNED_ROOT_REL.to_string()),
        ("PERLLSP_VIM_HOST_DECOY_ROOT_REL", DECOY_ROOT_REL.to_string()),
        ("PERLLSP_VIM_HOST_DECOY_FILE_REL", DECOY_FILE_REL.to_string()),
        ("PERLLSP_VIM_HOST_BULK_FILE_REL", BULK_FILE_REL.to_string()),
        ("PERLLSP_VIM_HOST_CANONICAL_SHA256", canonical_sha),
        ("PERLLSP_VIM_HOST_NON_CANONICAL_SHA256", non_canonical_sha),
        ("PERLLSP_VIM_HOST_BULK_SHA256", bulk_sha),
        ("PERLLSP_VIM_HOST_NON_CANONICAL_TEXT", non_canonical_source_text()),
        ("PERLLSP_VIM_HOST_SAVE_SYNC_TIMEOUT_MS", SAVE_SYNC_TIMEOUT_MS.to_string()),
        ("PERLLSP_VIM_HOST_STALE_SYNC_TIMEOUT_MS", STALE_SYNC_TIMEOUT_MS.to_string()),
        ("PERLLSP_VIM_HOST_STALE_WINDOW_MS", STALE_WINDOW_MS.to_string()),
        ("PERLLSP_VIM_HOST_TOML_OFF_TEXT", TOML_OFF_TEXT.to_string()),
        ("PERLLSP_VIM_HOST_TOML_EXTERNAL_TEXT", TOML_EXTERNAL_TEXT.to_string()),
    ];
    pairs
        .into_iter()
        .map(|(key, value)| (std::ffi::OsString::from(key), std::ffi::OsString::from(value)))
        .collect()
}

// ---------------------------------------------------------------------------
// Scenario-local save wire mining
// ---------------------------------------------------------------------------

/// The save facts mined from the vim-lsp client log. Direction-aware (the
/// same law the #11390 mining landed): outgoing `--->` lines are requests,
/// incoming `<---` lines carry the settled response together with the echoed
/// request, so a method can appear on both its send line and its response
/// echo and only direction separates them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SaveWire {
    /// Ordered line indexes of every outgoing `textDocument/formatting`
    /// request.
    pub request_lines: Vec<usize>,
    /// Ordered line indexes of every settled formatting response (the
    /// incoming envelope whose echoed request is a formatting request).
    pub response_lines: Vec<usize>,
    /// Settled responses that carry a JSON-RPC error object.
    pub error_response_lines: Vec<usize>,
    /// Settled responses whose result is an empty edit list.
    pub empty_response_lines: Vec<usize>,
    /// Settled responses whose result carries at least one edit.
    pub edits_response_lines: Vec<usize>,
    /// Ordered line indexes of every outgoing `$/cancelRequest`.
    pub cancel_request_lines: Vec<usize>,
}

impl SaveWire {
    pub fn request_count(&self) -> usize {
        self.request_lines.len()
    }

    pub fn response_count(&self) -> usize {
        self.response_lines.len()
    }
}

/// Whether the client's own initialize-request capabilities offer a
/// `textDocument.synchronization` boolean (for example `willSaveWaitUntil`).
pub fn client_sync_capability_offers(
    client_capabilities: &Option<serde_json::Value>,
    field: &str,
) -> Option<bool> {
    client_capabilities
        .as_ref()
        .and_then(|capabilities| capabilities.get("textDocument"))
        .and_then(|text_document| text_document.get("synchronization"))
        .and_then(|synchronization| synchronization.get(field))
        .and_then(serde_json::Value::as_bool)
}

/// Extract the save wire facts from the vim-lsp client log bytes. Each log
/// line carries its JSON payload inside an envelope array whose first element
/// is the direction marker (`--->` client-to-server, `<---`
/// server-to-client) followed by the lsp id, server name, and payload. The
/// incoming payload is `{response, request}` where `request` echoes the
/// original request.
pub fn extract_save_wire(log: &[u8]) -> SaveWire {
    let text = String::from_utf8_lossy(log);
    let mut wire = SaveWire::default();
    for (index, line) in text.lines().enumerate() {
        let Some(value) = first_json_value(line) else { continue };
        let serde_json::Value::Array(items) = &value else { continue };
        let Some(serde_json::Value::String(direction)) = items.first() else { continue };
        let Some(payload) = items.get(3) else { continue };
        if direction == "--->" {
            if let Some(method) = payload.get("method").and_then(serde_json::Value::as_str) {
                match method {
                    "textDocument/formatting" => wire.request_lines.push(index),
                    "$/cancelRequest" => wire.cancel_request_lines.push(index),
                    _ => {}
                }
            }
        } else if direction == "<---" {
            let request_method = payload
                .get("request")
                .and_then(|request| request.get("method"))
                .and_then(serde_json::Value::as_str);
            if request_method == Some("textDocument/formatting") {
                wire.response_lines.push(index);
                let response = payload.get("response");
                if response.is_some_and(|response| response.get("error").is_some()) {
                    wire.error_response_lines.push(index);
                } else {
                    match response.and_then(|response| response.get("result")) {
                        Some(serde_json::Value::Array(edits)) if edits.is_empty() => {
                            wire.empty_response_lines.push(index);
                        }
                        Some(serde_json::Value::Array(_)) => wire.edits_response_lines.push(index),
                        _ => {}
                    }
                }
            }
        }
    }
    wire
}

fn first_json_value(line: &str) -> Option<serde_json::Value> {
    for (index, byte) in line.bytes().enumerate() {
        if (byte == b'[' || byte == b'{')
            && let Ok(value) = serde_json::from_str::<serde_json::Value>(&line[index..])
        {
            return Some(value);
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Run
// ---------------------------------------------------------------------------

/// The typed outcome of one save-format host run.
pub struct SaveFormatRunOutcome {
    pub receipt_path: PathBuf,
    pub result: ObservationResult,
    pub process_cleanup: CleanupResult,
    pub driver_complete: bool,
    /// The typed driver-failure reason when the driver failed; the negative
    /// variants' expected reason lands here.
    pub driver_failure_reason: Option<String>,
}

/// Execute one #11396 save-format host run against the exact pinned subject
/// and write its canonical receipt. `variant` selects the fixture; only
/// `canonical` may pass.
pub fn host_save_format_run(
    repo_root: &Path,
    run: &VimHostRunInputs,
    variant: SaveFormatFixtureVariant,
) -> Result<SaveFormatRunOutcome> {
    crate::vim_host_run::ensure_fresh_output_root(&run.out_root)?;
    fs::create_dir_all(&run.out_root)
        .with_context(|| format!("creating output root {}", run.out_root.display()))?;

    let driver = repo_root.join("scripts/test/vim-host-save-format-driver.vim");
    let fixture = materialize_save_format_fixture(&run.out_root.join("fixture"), variant)?;
    let BoundHostPlan { plan, server_name, root_markers } = bind_host_run_plan(
        repo_root,
        run,
        &driver,
        &fixture.root,
        SAVE_FORMAT_JOURNEY_SELECTOR,
        SAVE_FORMAT_FIXTURE_ID,
    )?;
    let layout = HermeticVimLayout::prepare(&run.out_root.join("hermetic"))?;
    let mut command = build_vim_command_with_extras(
        &plan,
        &layout,
        &server_name,
        &root_markers,
        &save_format_env(variant),
    )?;
    let mut observation = run_owned_process(&mut command, &plan, &layout)?;

    let client_log_bytes = fs::read(layout.client_log()).unwrap_or_default();
    let wire = vim_host_runner::extract_wire_evidence(&client_log_bytes);
    let save_wire = extract_save_wire(&client_log_bytes);
    observation
        .artifacts
        .extend(vim_host_runner::retain_wire_evidence_artifacts(&plan, &layout, &wire)?);

    let judgment =
        evaluate_save_format_observation(&plan, &observation, &wire, &save_wire, variant);

    let snapshot = layout.capability_snapshot();
    let snapshot_sha256 =
        if snapshot.is_file() { Some(vim_host_runner::file_sha256(&snapshot)?) } else { None };
    let capabilities = vim_host_runner::capabilities_from_wire_evidence(&wire, snapshot_sha256)?;
    let diagnostics = vim_host_runner::diagnostics_from_wire_evidence(&wire);

    let mut limitations = save_format_limitations(&observation, &judgment, &save_wire, variant);
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
        save_format_journey(&observation, &judgment, &wire),
        judgment.result,
        judgment.failure_class,
        limitations,
        format!(
            "#11396 {SAVE_FORMAT_JOURNEY_SELECTOR}: ordinary-save formatting ownership, exact \
             applied/no-change state, and distinct non-pass dispositions for the exact pinned \
             subject only"
        ),
    );
    let receipt_path = run.out_root.join("receipt.json");
    fs::write(&receipt_path, serde_json::to_vec_pretty(&receipt)?)
        .with_context(|| format!("writing receipt {}", receipt_path.display()))?;
    validate_receipt_binding(&receipt, &plan)
        .context("the emitted receipt failed its own save-format binding")?;
    Ok(SaveFormatRunOutcome {
        receipt_path,
        result: judgment.result,
        process_cleanup: observation.cleanup,
        driver_complete: observation.driver_complete,
        driver_failure_reason: judgment.driver_failure_reason,
    })
}

fn save_format_limitations(
    observation: &ProcessObservation,
    judgment: &SaveFormatJudgment,
    save_wire: &SaveWire,
    variant: SaveFormatFixtureVariant,
) -> Vec<String> {
    let mut limitations = vec![
        "headless silent-ex Vim (-es): GUI-only client surfaces are not exercised by this harness"
            .to_string(),
        format!(
            "fixture variant {}: canonical/no-change/refused/failure bytes and every expectation \
             are Rust-authored, never derived from run output; the fixture digest pins the \
             initial state and every later mutation is a typed journey event",
            variant.id()
        ),
        "route classification: this client offers no willSave/willSaveWaitUntil save-format \
         surface, so the selected owner is the documented BufWritePre autocmd delegating to the \
         canonical sync format action; a client exposing those surfaces is a different route row"
            .to_string(),
        "the refused disposition shares the empty-result wire shape with legitimate no-change; \
         the authored byte oracle (non-canonical source for refusal, canonical source for \
         no-change) is what keeps the dispositions distinct"
            .to_string(),
        "the stale hold is a bounded absence observation on a large authored document: the 1ms \
         sync timeout is deterministic only against a round trip that cannot complete within it"
            .to_string(),
        "the failure leg's engine failure is exercised through the external engine with a \
         missing profile (an error the server reports for any unavailable engine invocation); \
         no native-engine failure input is claimed"
            .to_string(),
        "explicit-format, recovery, activation, and maintained/public replay cells are separate \
         leaves and are not claimed here"
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
    limitations.push(format!(
        "wire save facts observed: {} formatting requests, {} settled responses ({} error, {} \
         empty, {} edits), {} cancel requests",
        save_wire.request_count(),
        save_wire.response_count(),
        save_wire.error_response_lines.len(),
        save_wire.empty_response_lines.len(),
        save_wire.edits_response_lines.len(),
        save_wire.cancel_request_lines.len(),
    ));
    limitations
}

// ---------------------------------------------------------------------------
// Judgment
// ---------------------------------------------------------------------------

/// The seven-cell judgment over one observed save-format run.
pub struct SaveFormatJudgment {
    pub result: ObservationResult,
    pub failure_class: Option<crate::editor_client_compat::FailureClass>,
    pub driver_failure_reason: Option<String>,
    /// Route facts recorded for the receipt, read from the client's own
    /// initialize request and the run's own wire.
    pub client_will_save_wait_until: Option<bool>,
    pub server_formatting_advertised: Option<bool>,
    /// Per-cell proven results (the run-level pass requires all seven); the
    /// receipt composes the failure cell's honest `fail` disposition on top.
    pub cells: BTreeMap<String, ObservationResult>,
}

fn detail<'a>(
    events: &'a [vim_host_runner::DriverEvent],
    kind: DriverEventKind,
    key: &str,
) -> Option<&'a str> {
    events
        .iter()
        .find(|event| event.kind == kind)
        .and_then(|event| event.details.get(key))
        .map(String::as_str)
}

fn indexed_events<'a>(
    events: &'a [vim_host_runner::DriverEvent],
    kind: DriverEventKind,
    index_key: &str,
) -> Vec<&'a vim_host_runner::DriverEvent> {
    let mut found: Vec<&vim_host_runner::DriverEvent> =
        events.iter().filter(|event| event.kind == kind).collect();
    found.sort_by_key(|event| {
        event.details.get(index_key).and_then(|value| value.parse::<u32>().ok()).unwrap_or(u32::MAX)
    });
    found
}

fn cell_result(observed: bool, ok: bool) -> ObservationResult {
    if ok {
        ObservationResult::Pass
    } else if observed {
        ObservationResult::Fail
    } else {
        ObservationResult::NotProven
    }
}

/// Whether a `file://` URI ends with the expected relative directory segment,
/// on every host's path spelling (mirrors the #10946 helper).
fn uri_ends_with_segment(uri: &str, segment: &str) -> bool {
    let normalized = uri.replace('\\', "/");
    normalized.trim_end_matches('/').ends_with(&format!("/{segment}"))
        || normalized.trim_end_matches('/') == format!("file:///{segment}")
        || normalized.ends_with(segment)
}

fn settlement(
    events: &[vim_host_runner::DriverEvent],
    save_index: usize,
) -> Option<&vim_host_runner::DriverEvent> {
    indexed_events(events, DriverEventKind::SaveSettlementObserved, "save_index")
        .into_iter()
        .find(|event| event.details.get("save_index") == Some(&save_index.to_string()))
}

fn settlement_detail<'a>(
    events: &'a [vim_host_runner::DriverEvent],
    save_index: usize,
    key: &str,
) -> Option<&'a str> {
    settlement(events, save_index).and_then(|event| event.details.get(key)).map(String::as_str)
}

/// Judge one observed run against the scenario's Rust-authored expectations.
///
/// Positive path (all seven cells must be proven): registration bound to the
/// planned candidate digest, native root equal to the governed root with the
/// decoy distinct, the server advertising document formatting, the client
/// offering no native save-format surface, exactly one armed save owner for
/// every save-triggered settlement, one save → one admitted invocation, exact
/// canonical bytes for applied and no-change, distinct non-pass dispositions,
/// and a stale result that provably never applies.
#[allow(clippy::too_many_lines)]
pub fn evaluate_save_format_observation(
    plan: &VimHostRunPlan,
    observation: &ProcessObservation,
    wire: &WireEvidence,
    save_wire: &SaveWire,
    variant: SaveFormatFixtureVariant,
) -> SaveFormatJudgment {
    let mut cells = BTreeMap::new();
    let events = &observation.events;
    let canonical_sha = text_sha256(&canonical_source_text()).unwrap_or_default();
    let non_canonical_sha = text_sha256(&non_canonical_source_text()).unwrap_or_default();

    // --- substrate prerequisites: registration, attach, root.
    let registration_digest_match =
        detail(events, DriverEventKind::RegistrationSelected, "candidate_sha256")
            == Some(plan.identity.candidate_artifact_sha256.as_str());
    let attach_identity_observed = wire.saw_initialize && wire.saw_initialized;
    let root_event = events.iter().find(|event| event.kind == DriverEventKind::RootSelected);
    let observed_root = detail(events, DriverEventKind::RootSelected, "observed_root");
    let decoy_reported = detail(events, DriverEventKind::RootSelected, "decoy_root");
    let root_observed = root_event.is_some();
    let initialize_root_ok = wire
        .initialize_request
        .as_ref()
        .and_then(|request| request.get("params"))
        .and_then(|params| params.get("rootUri"))
        .and_then(|uri| uri.as_str())
        .is_some_and(|uri| uri_ends_with_segment(uri, GOVERNED_ROOT_REL));
    let root_ok = root_observed
        && observed_root == Some(GOVERNED_ROOT_REL)
        && decoy_reported == Some(DECOY_ROOT_REL)
        && initialize_root_ok;

    // --- route facts from the client's own offered capabilities and the
    // server's advertised surface (observed by the driver from the client's
    // own capability state).
    let client_will_save_wait_until =
        client_sync_capability_offers(&wire.client_capabilities, "willSaveWaitUntil");
    let server_formatting_advertised =
        detail(events, DriverEventKind::InitializeObserved, "document_formatting_advertised")
            .map(|value| value == "1");

    let owners = indexed_events(events, DriverEventKind::SaveOwnerConfigured, "owner_index");
    let settlements = indexed_events(events, DriverEventKind::SaveSettlementObserved, "save_index");
    let holds = indexed_events(events, DriverEventKind::StaleResultHoldObserved, "hold_index");
    let owner_count_at = |event: &vim_host_runner::DriverEvent| -> u32 {
        event
            .details
            .get("owner_count")
            .and_then(|value| value.parse::<u32>().ok())
            .unwrap_or(u32::MAX)
    };

    // --- route cell: the documented autocmd owner, armed exactly once for
    // every save-triggered settlement, on a client with no native save-format
    // surface and a server that advertises document formatting.
    let single_owner_armed = owners.iter().any(|event| {
        event.details.get("route") == Some(&"bufwritepre_autocmd".to_string())
            && owner_count_at(event) == 1
    });
    let save_triggered = settlements
        .iter()
        .any(|event| event.details.get("trigger") == Some(&"bufwritepre_save".to_string()));
    let route_observed = attach_identity_observed && root_observed && !owners.is_empty();
    let route_ok = route_observed
        && registration_digest_match
        && root_ok
        && client_will_save_wait_until == Some(false)
        && server_formatting_advertised == Some(true)
        && single_owner_armed
        && save_triggered;
    cells.insert(CELL_ROUTE.to_string(), cell_result(route_observed, route_ok));

    // --- cardinality cell: every save-triggered settlement with an armed
    // owner admits exactly one new formatting invocation; the ownerless
    // disabled settlement admits zero. Any other count (the duplicate-owner
    // control) fails here.
    let mut cardinality_observed = false;
    let mut cardinality_ok = true;
    for settlement_event in &settlements {
        let trigger = settlement_event.details.get("trigger").map(String::as_str);
        let owner_count = owner_count_at(settlement_event);
        let before = settlement_event
            .details
            .get("requests_before")
            .and_then(|value| value.parse::<u32>().ok())
            .unwrap_or(u32::MAX);
        let after = settlement_event
            .details
            .get("requests_after")
            .and_then(|value| value.parse::<u32>().ok())
            .unwrap_or(u32::MAX);
        let delta = after.saturating_sub(before);
        cardinality_observed = true;
        let expected = if trigger == Some("bufwritepre_save") && owner_count >= 1 { 1 } else { 0 };
        if delta != expected {
            cardinality_ok = false;
        }
    }
    // The wire's own request count must corroborate the last settlement's
    // post-state: no unaccounted requests beyond what settlements reported.
    let last_after = settlements
        .last()
        .and_then(|event| event.details.get("requests_after"))
        .and_then(|value| value.parse::<usize>().ok());
    if last_after.is_some_and(|after| save_wire.request_count() < after) {
        cardinality_ok = false;
    }
    cells.insert(CELL_CARDINALITY.to_string(), cell_result(cardinality_observed, cardinality_ok));

    // --- applied cell (save 1): the ordinary save settles both the buffer
    // and the file to the exact canonical bytes with one edits response.
    let applied_state = settlement_detail(events, 1, "buffer_sha256")
        == Some(canonical_sha.as_str())
        && settlement_detail(events, 1, "file_sha256") == Some(canonical_sha.as_str());
    let applied_trigger = settlement_detail(events, 1, "trigger") == Some("bufwritepre_save");
    let applied_response = settlement_detail(events, 1, "response_kind") == Some("edits");
    let applied_owner = settlement(events, 1).map(owner_count_at) == Some(1);
    let applied_label = settlement_detail(events, 1, "disposition") == Some("applied");
    let applied_observed = settlement(events, 1).is_some();
    let applied_ok = applied_state
        && applied_trigger
        && applied_response
        && applied_owner
        && applied_label
        && cardinality_ok;
    cells.insert(CELL_APPLIED.to_string(), cell_result(applied_observed, applied_ok));

    // --- no-change cell (saves 2 and 4): the route executed (one request,
    // settled response), the bytes stayed exactly canonical, and no hidden
    // failure flattened itself into no-change (the response settled without
    // error). Save 4 doubles as the post-timeout route-health recovery.
    let mut no_change_observed = false;
    let mut no_change_ok = true;
    for save_index in [2usize, 4] {
        let state = settlement_detail(events, save_index, "buffer_sha256")
            == Some(canonical_sha.as_str())
            && settlement_detail(events, save_index, "file_sha256") == Some(canonical_sha.as_str());
        let trigger = settlement_detail(events, save_index, "trigger") == Some("bufwritepre_save");
        let response = settlement_detail(events, save_index, "response_kind") == Some("empty");
        let delta_one = settlement(events, save_index).is_some_and(|event| {
            let before = event.details.get("requests_before").and_then(|v| v.parse::<u32>().ok());
            let after = event.details.get("requests_after").and_then(|v| v.parse::<u32>().ok());
            matches!((before, after), (Some(b), Some(a)) if a == b + 1)
        });
        if settlement(events, save_index).is_some() {
            no_change_observed = true;
        }
        let label_ok = settlement_detail(events, save_index, "disposition") == Some("no_change");
        if !(state && trigger && response && delta_one && label_ok) {
            no_change_ok = false;
        }
    }
    cells.insert(CELL_NO_CHANGE.to_string(), cell_result(no_change_observed, no_change_ok));

    // --- disabled/refused cell: two distinct non-pass dispositions. The
    // disabled settlement (save 5) runs an ordinary save with no armed owner:
    // zero new requests inside a bounded absence window and the
    // non-canonical bytes survive. The refused settlement (save 6) runs with
    // the engine-off config generation: one request, an empty result, and
    // the non-canonical bytes survive — wire-identical to no-change and
    // distinct only through the authored byte oracle.
    let disabled_state = settlement_detail(events, 5, "buffer_sha256")
        == Some(non_canonical_sha.as_str())
        && settlement_detail(events, 5, "file_sha256") == Some(non_canonical_sha.as_str());
    let disabled_trigger = settlement_detail(events, 5, "trigger") == Some("bufwritepre_save");
    let disabled_owner_absent =
        settlement(events, 5).is_some_and(|event| owner_count_at(event) == 0);
    let disabled_zero_requests = settlement(events, 5).is_some_and(|event| {
        matches!(
            (
                event.details.get("requests_before").and_then(|v| v.parse::<u32>().ok()),
                event.details.get("requests_after").and_then(|v| v.parse::<u32>().ok()),
            ),
            (Some(before), Some(after)) if before == after
        )
    });
    let disabled_response_absent = settlement_detail(events, 5, "response_kind") == Some("absent");
    let refused_state = settlement_detail(events, 6, "buffer_sha256")
        == Some(non_canonical_sha.as_str())
        && settlement_detail(events, 6, "file_sha256") == Some(non_canonical_sha.as_str());
    let refused_trigger = settlement_detail(events, 6, "trigger") == Some("bufwritepre_save");
    let refused_one_request = settlement(events, 6).is_some_and(|event| {
        matches!(
            (
                event.details.get("requests_before").and_then(|v| v.parse::<u32>().ok()),
                event.details.get("requests_after").and_then(|v| v.parse::<u32>().ok()),
            ),
            (Some(before), Some(after)) if after == before + 1
        )
    });
    let refused_empty = settlement_detail(events, 6, "response_kind") == Some("empty");
    let disabled_refused_observed =
        settlement(events, 5).is_some() || settlement(events, 6).is_some();
    let disabled_label = settlement_detail(events, 5, "disposition") == Some("disabled");
    let refused_label = settlement_detail(events, 6, "disposition") == Some("refused");
    let disabled_refused_ok = disabled_state
        && disabled_trigger
        && disabled_owner_absent
        && disabled_zero_requests
        && disabled_response_absent
        && disabled_label
        && refused_state
        && refused_trigger
        && refused_one_request
        && refused_empty
        && refused_label;
    cells.insert(
        CELL_DISABLED.to_string(),
        cell_result(disabled_refused_observed, disabled_refused_ok),
    );

    // --- failure cell: the engine failure is honestly recorded — one
    // request, an ERROR response (never a flattened empty), the
    // non-canonical bytes survive, and the receipt carries the honest `fail`
    // disposition (#11384 law: this cell never passes).
    let failure_state = settlement_detail(events, 7, "buffer_sha256")
        == Some(non_canonical_sha.as_str())
        && settlement_detail(events, 7, "file_sha256") == Some(non_canonical_sha.as_str());
    let failure_trigger = settlement_detail(events, 7, "trigger") == Some("bufwritepre_save");
    let failure_error = settlement_detail(events, 7, "response_kind") == Some("error");
    let failure_label = settlement_detail(events, 7, "disposition") == Some("failure");
    let failure_observed = settlement(events, 7).is_some();
    let failure_ok = failure_state && failure_trigger && failure_error && failure_label;
    cells.insert(CELL_FAILURE.to_string(), cell_result(failure_observed, failure_ok));

    // --- stale-result cell: the timed-out save (save 3) proves the late
    // result was released and never applied — the bytes held non-canonical
    // through the bounded window while the response settled afterwards — and
    // the follow-up save (folded into the no-change cell's save 4) proves the
    // route still settles after the stale rejection.
    let stale_state = settlement_detail(events, 3, "buffer_sha256")
        .is_some_and(|sha| sha == text_sha256(&bulk_non_canonical_text()).unwrap_or_default())
        && settlement_detail(events, 3, "file_sha256")
            .is_some_and(|sha| sha == text_sha256(&bulk_non_canonical_text()).unwrap_or_default());
    let stale_trigger = settlement_detail(events, 3, "trigger") == Some("bufwritepre_save");
    let stale_label = settlement_detail(events, 3, "disposition") == Some("stale_rejected");
    let stale_response = settlement_detail(events, 3, "response_kind") == Some("edits")
        || settlement_detail(events, 3, "response_kind") == Some("error");
    let stale_holds_honest = holds.iter().all(|event| {
        event.details.get("bytes_held") == Some(&"1".to_string())
            && event.details.get("late_response_rejected") == Some(&"1".to_string())
            && event
                .details
                .get("window_ms")
                .and_then(|value| value.parse::<u64>().ok())
                .is_some_and(|window| window >= vim_host_runner::MIN_STALE_WINDOW_MS)
    });
    let stale_observed = settlement(events, 3).is_some() || !holds.is_empty();
    let stale_ok = stale_state
        && stale_trigger
        && stale_label
        && stale_response
        && !holds.is_empty()
        && stale_holds_honest;
    cells.insert(CELL_STALE.to_string(), cell_result(stale_observed, stale_ok));

    let driver_failed_event =
        events.iter().find(|event| event.kind == DriverEventKind::DriverFailed);
    let driver_failure_reason =
        driver_failed_event.and_then(|event| event.details.get("reason")).cloned();
    let leaked = observation.cleanup == CleanupResult::Fail;
    let seven_cells_ok = cells.values().all(|result| *result == ObservationResult::Pass);
    let result = if observation.passed_process_boundary() && seven_cells_ok {
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
    SaveFormatJudgment {
        result,
        failure_class,
        driver_failure_reason,
        client_will_save_wait_until,
        server_formatting_advertised,
        cells,
    }
}

// ---------------------------------------------------------------------------
// Receipt journey
// ---------------------------------------------------------------------------

/// Compose the receipt journey: the lifecycle barrier cells (the #10944
/// surface) plus the seven #11384 catalog cells this scenario evidences. The
/// generic receipt admits no fail cell in a passing run, so the failure
/// cell's generic result is the proven honest-record claim (pass with the
/// observation attached) while its limitation carries the #11384 family
/// vocabulary token `observed_disposition=fail` — the row itself never
/// carries pass, and the CI binding checks that token.
pub fn save_format_journey(
    observation: &ProcessObservation,
    judgment: &SaveFormatJudgment,
    wire: &WireEvidence,
) -> Vec<JourneyCell> {
    let mut cells = crate::vim_host_run::outcome_journey(observation, wire);
    let catalog_limitations: BTreeMap<&str, &str> = BTreeMap::from([
        (
            CELL_ROUTE,
            "route shape only: the documented BufWritePre autocmd delegating to the canonical \
             sync format action; this client offers no willSave/willSaveWaitUntil surface and \
             route classification is never an automatic semantic pass",
        ),
        (
            CELL_CARDINALITY,
            "one ordinary save with one armed owner admits exactly one formatting invocation, \
             observed on the run's own wire; the ownerless disabled settlement admits zero",
        ),
        (
            CELL_APPLIED,
            "the ordinary save settles both the buffer and the file to the exact Rust-authored \
             canonical bytes; a manual or test-side edit can never satisfy it",
        ),
        (
            CELL_NO_CHANGE,
            "legitimate no-change requires the route to have executed (exactly one request and \
             a settled error-free response) over already-canonical source",
        ),
        (
            CELL_DISABLED,
            "disabled (ownerless save, zero requests) and refused (engine-off empty result over \
             non-canonical source) stay distinct from no-change through the authored byte oracle",
        ),
        (
            CELL_FAILURE,
            "observed_disposition=fail; the engine failure was honestly recorded (one error \
             response, no edits, bytes retained); the #11384 family row for this cell never \
             carries pass — this generic journey cell records the proven honest-record claim",
        ),
        (
            CELL_STALE,
            "the timed-out save-format result is released after the write and provably never \
             applies; the bytes hold non-canonical through the bounded observation window",
        ),
    ]);
    for (cell_id, result) in &judgment.cells {
        let observed = *result != ObservationResult::NotProven;
        cells.push(JourneyCell {
            id: cell_id.clone(),
            capability_basis: CapabilityBasis::NotApplicable,
            observed,
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
        CELL_ROUTE => vec![
            "vim/driver-events.jsonl".to_string(),
            "vim/initialize-request.json".to_string(),
            "vim/client-capabilities.json".to_string(),
        ],
        CELL_STALE => vec![
            "vim/driver-events.jsonl".to_string(),
            "vim/vim-lsp-client.log".to_string(),
            "vim/process-ledger.json".to_string(),
        ],
        _ => vec!["vim/driver-events.jsonl".to_string(), "vim/vim-lsp-client.log".to_string()],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_variants_parse_and_carry_typed_negative_reasons() {
        assert!(matches!(
            SaveFormatFixtureVariant::from_id("canonical"),
            Ok(SaveFormatFixtureVariant::Canonical)
        ));
        assert!(SaveFormatFixtureVariant::from_id("other").is_err());
        assert_eq!(
            SaveFormatFixtureVariant::ManualComparatorOnly.expected_negative_reason(),
            Some("save_trigger_absent")
        );
        assert_eq!(
            SaveFormatFixtureVariant::DuplicateOwner.expected_negative_reason(),
            Some("duplicate_invocation_observed")
        );
        assert_eq!(
            SaveFormatFixtureVariant::WrongRootDecoy.expected_negative_reason(),
            Some("root_mismatch")
        );
        assert_eq!(SaveFormatFixtureVariant::Canonical.expected_negative_reason(), None);
    }

    #[test]
    fn canonical_text_is_the_formatter_shape_of_the_non_canonical_text() {
        let non_canonical = non_canonical_source_text();
        let canonical = canonical_source_text();
        assert_ne!(non_canonical, canonical);
        // Same statements, different layout: the canonical generation expands
        // the single-line if-block (tab-indented under the hermetic host's
        // insertSpaces=false options) and normalizes operator and keyword
        // spacing, while the attached sub brace stays attached. The texts are
        // shadowing-free so no strict-warning discriminator rides along.
        assert!(canonical.contains(
            "sub compute{\nmy ($input) = @_;\nif ($input == 1) {\n\treturn $input + 1;\n}"
        ));
        assert!(canonical.contains("my $seed = 3;"));
        assert!(canonical.contains("my $value = compute($seed);"));
        assert!(non_canonical.contains("sub compute{"));
        assert!(non_canonical.contains("if($input==1){return $input+1;}"));
        assert!(non_canonical.contains("my $seed=3;"));
    }

    #[test]
    fn bulk_stale_document_is_large_and_stays_non_canonical() {
        let bulk = bulk_non_canonical_text();
        assert!(bulk.lines().count() >= BULK_BLOCKS * 2);
        assert!(bulk.contains("if($v0==0){my $w0=$v0+1;}"));
        assert!(!bulk.contains("if ($v0 == 0)"));
    }

    #[test]
    fn save_wire_mines_directions_without_double_counting_responses() {
        let log = concat!(
            "08/25/2026 10:00:00:[\"--->\",3,\"perl\",{\"method\":\"textDocument/formatting\",\"id\":7,\"params\":{}}]\n",
            "08/25/2026 10:00:00:[\"<---\",3,\"perl\",{\"response\":{\"id\":7,\"result\":[{\"range\":{}}]},\"request\":{\"method\":\"textDocument/formatting\",\"id\":7}}]\n",
            "08/25/2026 10:00:01:[\"--->\",3,\"perl\",{\"method\":\"textDocument/didSave\",\"params\":{}}]\n",
            "08/25/2026 10:00:02:[\"<---\",3,\"perl\",{\"response\":{\"id\":8,\"result\":[]},\"request\":{\"method\":\"textDocument/formatting\",\"id\":8}}]\n",
            "08/25/2026 10:00:03:[\"<---\",3,\"perl\",{\"response\":{\"id\":9,\"error\":{\"code\":-32603}},\"request\":{\"method\":\"textDocument/formatting\",\"id\":9}}]\n"
        );
        let wire = extract_save_wire(log.as_bytes());
        assert_eq!(wire.request_count(), 1);
        assert_eq!(wire.response_count(), 3);
        assert_eq!(wire.edits_response_lines, vec![1]);
        assert_eq!(wire.empty_response_lines, vec![3]);
        assert_eq!(wire.error_response_lines, vec![4]);
        assert!(wire.cancel_request_lines.is_empty());
    }

    #[test]
    fn client_sync_capability_reads_the_client_own_offer() {
        let capabilities = serde_json::json!({
            "textDocument": {"synchronization": {"willSave": false, "willSaveWaitUntil": false}}
        });
        assert_eq!(
            client_sync_capability_offers(&Some(capabilities.clone()), "willSaveWaitUntil"),
            Some(false)
        );
        assert_eq!(client_sync_capability_offers(&Some(serde_json::json!({})), "willSave"), None);
        assert_eq!(client_sync_capability_offers(&None, "willSaveWaitUntil"), None);
    }
}

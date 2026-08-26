//! #11390 freshness-generations scenario for the hermetic Vim + vim-lsp host
//! runner.
//!
//! This module is the freshness execution consumer of the #10944/#12545
//! substrate and the #12589 scenario pattern: it proves, through the pinned
//! actual Vim + vim-lsp + perllsp subject, the #11381 freshness cells —
//! route, external source, project config, client settings, stale generation
//! rejection, and provider ownership — using only the routes the exact
//! subject genuinely supports.
//!
//! Route classification (source-backed, re-proven on every run's own wire):
//!
//! - **client watcher: not exposed.** The pinned vim-lsp's initialize request
//!   carries no `workspace.didChangeWatchedFiles` capability in any form and
//!   the client never sends the notification (the pinned checkout has no
//!   watcher). The judgment reads the client's own offered capabilities from
//!   the mined initialize request — a client that someday exposes the watcher
//!   surface changes this row honestly instead of silently.
//! - **server-owned watcher: unreachable for this subject.** perllsp reacts to
//!   client-pushed `workspace/didChangeWatchedFiles` only; without a client
//!   push there is no watcher route.
//! - **explicit reload: supported** for open documents, through the client's
//!   real didClose+didOpen path (`bwipeout!` + `edit`).
//! - **client settings push: supported** through the #11369-classified stable
//!   public surface `lsp#update_workspace_config`, which emits
//!   `workspace/didChangeConfiguration`. The semantic result materializes on
//!   the next document open: the client offers no
//!   `workspace.diagnostics.refreshSupport`, so perllsp cannot spontaneously
//!   re-push diagnostics for open documents after a configuration change.
//! - **project config: restart required.** `.perl-lsp.toml` is loaded once at
//!   initialize (and on workspace-folder changes); no watcher exists, so a new
//!   project-config generation reaches the server only through a server
//!   restart. The client channel's `includePaths` (the registration's only
//!   workspace field) overrides TOML includePaths per-field, so the governed
//!   TOML discriminator is a field the client channel never carries: the
//!   `[critic] exclude` list (native critic, default engine, no external
//!   Perl::Critic needed; the excluded policy is the distinct identity
//!   `native.common.stale_dollar_at`, which no built-in diagnostic twins).
//!
//! Ownership split (consumed, never duplicated):
//!
//! - `vim_host_run::vim_host_runner` (#10944) owns hermetic launch,
//!   supervision, process ledgers, cleanup comparison, generic wire mining,
//!   and receipt composition. This module owns the freshness fixture
//!   variants, the scenario-local freshness wire mining (warning-severity
//!   batches, `workspace/didChangeConfiguration` positions, didOpen/didClose
//!   ordering, initialize restart counting), the six-cell judgment, and the
//!   scenario receipt.
//! - `vim_lsp_cell_catalog` (#11381) owns cell registration; this module
//!   cites catalog cell ids in its receipt journey but never edits a catalog.
//! - `#7762` owns root selection (via the activation-root manifest). The
//!   governed root marker for this fixture is `cpanfile` — on the manifest's
//!   authority list — so the `.perl-lsp.toml` lifecycle (create/malformed/
//!   repair) never disturbs root selection.
//! - The expectation oracle lives here in Rust — the source generations, the
//!   defect and clean texts, the settings include-path channels, and the TOML
//!   variants — never derived from the responses under test (#10938 law),
//!   and never embedded in Vimscript.
//!
//! Fail-closed laws beyond the substrate's:
//!
//! - every semantic cell requires the client's own state (the classified
//!   `lsp#get_buffer_diagnostics_counts()` surface) AND the client's own wire
//!   record (mined publishDiagnostics batches for the governed file tokens);
//!   a log-only or counts-only claim cannot pass;
//! - an external source mutation never satisfies a cell by itself: only the
//!   materialization the exact route supports (client reopen, settings push
//!   plus reopen, server restart) can converge the state, and every
//!   spontaneous-refresh claim is red-controlled by the `live_reload_claimed`
//!   negative variant;
//! - an old generation never satisfies currency: the released-old-generation
//!   bytes stay invisible to the client state until an explicit
//!   materialization, and the judgment requires the defect generation to
//!   stand until its own reload;
//! - the decoy same-named file at the wrong root can never supply the
//!   changed result: its mutation is proven invisible to the governed
//!   buffer's state and wire record;
//! - a settings effect is attributable only to the configuration push: the
//!   same client reopen without the push (the in-journey control) must leave
//!   the discriminator present;
//! - negative fixture variants (`wrong_root_decoy`, `live_reload_claimed`,
//!   `ambient_path_only`) are expected to fail with typed reasons; a pass on
//!   a negative variant is an oracle violation, never a green run.

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

pub const FRESHNESS_JOURNEY_SELECTOR: &str = "vim_vim_lsp_freshness_generations.v1";
pub const FRESHNESS_FIXTURE_ID: &str = "vim_vim_lsp_freshness_generations_v1";

// ---------------------------------------------------------------------------
// Rust-authored fixture expectations
// ---------------------------------------------------------------------------

/// The governed fixture's stable layout, relative to the materialized fixture
/// root. Authored here, never derived from run output.
pub const GOVERNED_ROOT_REL: &str = "workspace/project";
pub const DECOY_ROOT_REL: &str = "workspace";
pub const OPENED_FILE_REL: &str = "workspace/project/main.pl";
pub const DECOY_FILE_REL: &str = "workspace/main.pl";
pub const SETTINGS_FILE_REL: &str = "workspace/project/settings.pl";
pub const CONFIG_FILE_REL: &str = "workspace/project/config.pl";
/// The governed root marker. `cpanfile` is on the #7762 authority list, which
/// frees `.perl-lsp.toml` for the config lifecycle without moving the root.
pub const ROOT_MARKER: &str = "cpanfile";

/// Wire file-name tokens of the governed documents (publishDiagnostics `uri`
/// tails) — the only tokens the judgment accepts evidence from.
pub const MAIN_TOKEN: &str = "main.pl";
pub const SETTINGS_TOKEN: &str = "settings.pl";
pub const CONFIG_TOKEN: &str = "config.pl";

/// G1: the governed clean source generation. Line 4 is the mutation target.
pub const CLEAN_LINE_TEXT: &str = "my $value = My::Widget::answer();";
/// G2: the governed defective generation (the #10946 governed defect: the
/// trailing semicolon is missing), produced by an external in-place mutation.
pub const DEFECT_LINE_TEXT: &str = "my $value = My::Widget::answer()";
pub const MUTATION_LINE: usize = 4;

pub const CLEAN_SOURCE_LINES: [&str; 5] =
    ["use strict;", "use warnings;", "use My::Widget;", CLEAN_LINE_TEXT, "print \"$value\\n\";"];

/// The decoy same-named file at the outer root: clean until the decoy
/// mutation writes the defective generation into it.
pub const DECOY_CLEAN_LINES: [&str; 3] =
    ["use strict;", "use warnings;", "print \"outer decoy\\n\";"];

/// The client-settings governed file: `My::Vendor::Extra` lives under
/// `vendor/`, which is reachable only through the settings channel.
pub const SETTINGS_SOURCE_LINES: [&str; 5] = [
    "use strict;",
    "use warnings;",
    "use My::Vendor::Extra;",
    "my $extra = My::Vendor::Extra::label();",
    "print \"$extra\\n\";",
];

/// The project-config governed file: a block `eval` immediately followed by
/// an `if ($@)` condition — the classic pattern the *distinct* native critic
/// identity `native.common.stale_dollar_at` flags (no built-in diagnostic
/// twins it: PL407 deliberately treats the immediate eval-to-condition read
/// as valid flow), so the TOML `[critic] exclude` discriminator moves exactly
/// one warning and no core lint can mask or accompany it.
pub const CONFIG_SOURCE_LINES: [&str; 6] = [
    "use strict;",
    "use warnings;",
    "sub handled { return 1 }",
    "eval { handled(); 1; };",
    "if ($@) { print \"caught\\n\"; }",
    "print \"done\\n\";",
];

pub const CRITIC_EXCLUDED_POLICY: &str = "native.common.stale_dollar_at";

/// The project-config generations, authored here and delivered to the driver
/// through the environment (never embedded in Vimscript).
pub const TOML_EXCLUDE_TEXT: &str = "[critic]\nengine = \"native\"\nexclude = \
     [\"native.common.stale_dollar_at\"]\n";
pub const TOML_MALFORMED_TEXT: &str = "[critic\nengine = \"native\"\nexclude = [\"native.io";

/// The settings channels: the canonical push admits the workspace-contained
/// relative `vendor` path; the ambient negative variant carries an absolute
/// path the server must reject (#4998 law: unadmitted paths never resolve).
pub const CANONICAL_SETTINGS_PATHS: &str = "lib,vendor";

/// The bounded absence-observation window for stale-generation holds.
pub const STALE_WINDOW_MS: u64 = 5000;

/// The #11381 catalog cell ids this journey evidences. The catalog owns
/// registration; this scenario only cites.
pub const CELL_ROUTE: &str = "vim.vim_lsp.freshness.route";
pub const CELL_EXTERNAL_SOURCE: &str = "vim.vim_lsp.freshness.external_source";
pub const CELL_PROJECT_CONFIG: &str = "vim.vim_lsp.freshness.project_config";
pub const CELL_CLIENT_SETTINGS: &str = "vim.vim_lsp.freshness.client_settings";
pub const CELL_STALE_GENERATION: &str = "vim.vim_lsp.freshness.stale_generation_rejected";
pub const CELL_PROVIDER_OWNERSHIP: &str = "vim.vim_lsp.freshness.provider_ownership";

// ---------------------------------------------------------------------------
// Fixture variants
// ---------------------------------------------------------------------------

/// One scenario fixture variant. `Canonical` must pass; the three negative
/// variants must fail with their typed reason (the red-first controls).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FreshnessFixtureVariant {
    Canonical,
    /// The #7762 marker moves to the outer workspace: native resolution
    /// selects the decoy root and the journey must reject it.
    WrongRootDecoy,
    /// The fixture is canonical, but the journey claims the client refreshes
    /// open-document semantics spontaneously after the external mutation: the
    /// claim must fail (no watcher route exists for this subject).
    LiveReloadClaimed,
    /// The settings push carries an absolute include path the server must
    /// reject: the warning discriminator must stay and the journey must fail.
    AmbientPathOnly,
}

impl FreshnessFixtureVariant {
    pub fn from_id(id: &str) -> Result<Self> {
        match id {
            "canonical" => Ok(Self::Canonical),
            "wrong_root_decoy" => Ok(Self::WrongRootDecoy),
            "live_reload_claimed" => Ok(Self::LiveReloadClaimed),
            "ambient_path_only" => Ok(Self::AmbientPathOnly),
            other => bail!(
                "unknown freshness fixture variant {other}: known variants are canonical, \
                 wrong_root_decoy, live_reload_claimed, ambient_path_only"
            ),
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Self::Canonical => "canonical",
            Self::WrongRootDecoy => "wrong_root_decoy",
            Self::LiveReloadClaimed => "live_reload_claimed",
            Self::AmbientPathOnly => "ambient_path_only",
        }
    }

    /// The typed driver-failure reason this variant must produce; `None` for
    /// the canonical variant, which must pass.
    pub fn expected_negative_reason(self) -> Option<&'static str> {
        match self {
            Self::Canonical => None,
            Self::WrongRootDecoy => Some("root_mismatch"),
            Self::LiveReloadClaimed => Some("live_freshness_absent"),
            Self::AmbientPathOnly => Some("settings_effect_absent"),
        }
    }
}

/// The materialized governed fixture for one variant.
pub struct FreshnessFixture {
    pub root: PathBuf,
    pub variant: FreshnessFixtureVariant,
}

/// Materialize the #11390 governed fixture under `root`:
///
/// ```text
/// workspace/                      <- outer decoy root (no marker, canonical)
///   main.pl                       <- same-named decoy file (clean)
///   cpanfile                      <- marker ONLY in the wrong_root_decoy variant
///   project/                      <- the governed #7762 root
///     cpanfile                    <- the governed root marker (all but decoy)
///     main.pl                     <- the governed source (clean G1)
///     settings.pl                 <- the client-settings governed file
///     config.pl                   <- the project-config governed file
///     lib/My/Widget.pm            <- resolvable through the registration channel
///     vendor/My/Vendor/Extra.pm   <- resolvable only via the settings channel
/// ```
///
/// No `.perl-lsp.toml` exists initially: the project-config generation is
/// created during the journey (create/malformed/repair through external
/// mutations and server restarts). The fixture digest recorded in the run plan
/// pins exactly this initial state; every later mutation is a typed journey
/// event, never silent fixture drift.
pub fn materialize_freshness_fixture(
    root: &Path,
    variant: FreshnessFixtureVariant,
) -> Result<FreshnessFixture> {
    ensure!(root.is_absolute(), "fixture root must be absolute");
    let workspace = root.join("workspace");
    let project = workspace.join("project");
    for directory in [project.join("lib/My"), project.join("vendor/My/Vendor")] {
        fs::create_dir_all(&directory)
            .with_context(|| format!("creating {}", directory.display()))?;
    }
    write_lines(&project.join("main.pl"), &CLEAN_SOURCE_LINES)?;
    write_lines(&project.join("settings.pl"), &SETTINGS_SOURCE_LINES)?;
    write_lines(&project.join("config.pl"), &CONFIG_SOURCE_LINES)?;
    fs::write(
        project.join("lib/My/Widget.pm"),
        "package My::Widget;\nuse strict;\nuse warnings;\nsub answer { 42 }\n1;\n",
    )?;
    fs::write(
        project.join("vendor/My/Vendor/Extra.pm"),
        "package My::Vendor::Extra;\nuse strict;\nuse warnings;\nsub label { 'vendor' }\n1;\n",
    )?;
    write_lines(&workspace.join("main.pl"), &DECOY_CLEAN_LINES)?;
    let marker = "# vim/vim-lsp #11390 governed root marker (cpanfile per #7762)\n";
    match variant {
        FreshnessFixtureVariant::WrongRootDecoy => {
            // Marker ONLY at the decoy root: native resolution selects
            // `workspace`, and the journey must reject it.
            fs::write(workspace.join(ROOT_MARKER), marker)?;
        }
        _ => {
            fs::write(project.join(ROOT_MARKER), marker)?;
        }
    }
    Ok(FreshnessFixture { root: root.to_path_buf(), variant })
}

fn write_lines(path: &Path, lines: &[&str]) -> Result<()> {
    let mut text = String::new();
    for line in lines {
        text.push_str(line);
        text.push('\n');
    }
    fs::write(path, text).with_context(|| format!("writing {}", path.display()))
}

/// The full G1/G2 source texts delivered to the driver (the mutation oracle:
/// authored here, applied verbatim by the adapter).
pub fn clean_source_text() -> String {
    CLEAN_SOURCE_LINES.join("\n")
}

pub fn defect_source_text() -> String {
    let mut lines: Vec<String> = CLEAN_SOURCE_LINES.iter().map(ToString::to_string).collect();
    lines[MUTATION_LINE - 1] = DEFECT_LINE_TEXT.to_string();
    lines.join("\n")
}

fn decoy_defect_text() -> String {
    // The decoy mutation writes the same defective generation into the
    // same-named decoy: maximally confusable with the governed file.
    defect_source_text()
}

/// The settings channel delivered to the driver. The ambient negative variant
/// carries the absolute vendor path — a real, resolvable directory admitted by
/// no authority (#4998): the server must reject it and the discriminator must
/// stay.
pub fn settings_include_paths(fixture_root: &Path, variant: FreshnessFixtureVariant) -> String {
    match variant {
        FreshnessFixtureVariant::AmbientPathOnly => {
            let vendor = fixture_root.join("workspace/project/vendor");
            format!("lib,{}", vendor.to_string_lossy().replace('\\', "/"))
        }
        _ => CANONICAL_SETTINGS_PATHS.to_string(),
    }
}

/// The scenario's environment contract beyond the substrate's: the
/// Rust-authored expectations delivered to the driver (never re-derived in
/// Vimscript).
pub fn freshness_env(
    fixture_root: &Path,
    variant: FreshnessFixtureVariant,
) -> Vec<(std::ffi::OsString, std::ffi::OsString)> {
    let pairs = [
        ("PERLLSP_VIM_HOST_FRESHNESS_VARIANT", variant.id().to_string()),
        ("PERLLSP_VIM_HOST_OPENED_FILE_REL", OPENED_FILE_REL.to_string()),
        ("PERLLSP_VIM_HOST_EXPECTED_ROOT_REL", GOVERNED_ROOT_REL.to_string()),
        ("PERLLSP_VIM_HOST_DECOY_ROOT_REL", DECOY_ROOT_REL.to_string()),
        ("PERLLSP_VIM_HOST_DECOY_FILE_REL", DECOY_FILE_REL.to_string()),
        ("PERLLSP_VIM_HOST_SETTINGS_FILE_REL", SETTINGS_FILE_REL.to_string()),
        ("PERLLSP_VIM_HOST_CONFIG_FILE_REL", CONFIG_FILE_REL.to_string()),
        ("PERLLSP_VIM_HOST_MUTATION_LINE", MUTATION_LINE.to_string()),
        ("PERLLSP_VIM_HOST_CLEAN_SOURCE_TEXT", clean_source_text()),
        ("PERLLSP_VIM_HOST_DEFECT_SOURCE_TEXT", defect_source_text()),
        ("PERLLSP_VIM_HOST_DECOY_DEFECT_TEXT", decoy_defect_text()),
        ("PERLLSP_VIM_HOST_STALE_WINDOW_MS", STALE_WINDOW_MS.to_string()),
        ("PERLLSP_VIM_HOST_SETTINGS_INCLUDE_PATHS", settings_include_paths(fixture_root, variant)),
        ("PERLLSP_VIM_HOST_TOML_EXCLUDE_TEXT", TOML_EXCLUDE_TEXT.to_string()),
        ("PERLLSP_VIM_HOST_TOML_MALFORMED_TEXT", TOML_MALFORMED_TEXT.to_string()),
    ];
    pairs
        .into_iter()
        .map(|(key, value)| (std::ffi::OsString::from(key), std::ffi::OsString::from(value)))
        .collect()
}

// ---------------------------------------------------------------------------
// Scenario-local freshness wire mining
// ---------------------------------------------------------------------------

/// One mined `textDocument/publishDiagnostics` batch with the
/// freshness-relevant discriminators: warning-severity count, the PL701
/// module-resolution code count, and the governed native-critic policy code
/// count (the substrate's generic mining carries only error-severity and
/// parser-family counts).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FreshnessBatch {
    pub line_index: usize,
    pub uri_file: String,
    pub error_severity_count: usize,
    pub warning_severity_count: usize,
    pub pl701_count: usize,
    pub critic_policy_count: usize,
}

impl FreshnessBatch {
    pub fn is_clean(&self) -> bool {
        self.error_severity_count == 0 && self.warning_severity_count == 0
    }
}

/// The freshness facts mined from the vim-lsp client log: ordered
/// didOpen/didClose positions per governed file token (generation ordering),
/// `workspace/didChangeConfiguration` positions (settings push ordering), and
/// the initialize-request count (restart / process-generation evidence).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FreshnessWire {
    pub batches: Vec<FreshnessBatch>,
    /// Ordered (line, token) pairs for every `textDocument/didOpen`.
    pub did_open_lines: Vec<(usize, String)>,
    /// Ordered (line, token) pairs for every `textDocument/didClose`.
    pub did_close_lines: Vec<(usize, String)>,
    /// Ordered line indexes of every `workspace/didChangeConfiguration`.
    pub did_change_configuration_lines: Vec<usize>,
    /// How many `initialize` requests the client logged (one per server
    /// process generation).
    pub initialize_count: usize,
}

impl FreshnessWire {
    /// Ordered line indexes of didOpen events for one file token.
    pub fn opens_of(&self, token: &str) -> Vec<usize> {
        self.did_open_lines
            .iter()
            .filter(|(_, file)| file == token)
            .map(|(line, _)| *line)
            .collect()
    }

    /// Ordered line indexes of didClose events for one file token.
    pub fn closes_of(&self, token: &str) -> Vec<usize> {
        self.did_close_lines
            .iter()
            .filter(|(_, file)| file == token)
            .map(|(line, _)| *line)
            .collect()
    }

    /// The batches for one file token, in wire order.
    pub fn batches_of(&self, token: &str) -> Vec<&FreshnessBatch> {
        self.batches.iter().filter(|batch| batch.uri_file == token).collect()
    }

    /// The settled wire generation for `token` inside the window bounded by
    /// two didOpen events of that document (`start` exclusive, `end`
    /// exclusive; `usize::MAX` for the final window): the last batch in the
    /// window, which is what the client's current state corresponds to.
    pub fn latest_batch_between(
        &self,
        token: &str,
        start: usize,
        end: usize,
    ) -> Option<&FreshnessBatch> {
        self.batches_of(token)
            .iter()
            .copied()
            .rfind(|batch| batch.line_index > start && batch.line_index < end)
    }

    /// The settled wire generation after the document's `open_index`-th
    /// didOpen. The window ends at the earlier of the document's next didOpen
    /// or its first didClose after this open: the server clears a closed
    /// document's diagnostics with one final empty publishDiagnostics, and
    /// that clearing artifact is not a state claim about the generation — the
    /// settled generation is the last batch before the document's close.
    pub fn settled_batch_after_open(
        &self,
        token: &str,
        open_index: usize,
    ) -> Option<&FreshnessBatch> {
        let opens = self.opens_of(token);
        let start = *opens.get(open_index)?;
        let next_open = opens.get(open_index + 1).copied().unwrap_or(usize::MAX);
        let next_close =
            self.closes_of(token).into_iter().find(|close| *close > start).unwrap_or(usize::MAX);
        self.latest_batch_between(token, start, next_open.min(next_close))
    }
}

/// Extract the freshness wire facts from the vim-lsp client log bytes. Each
/// vim-lsp log line carries its JSON payload inside an envelope array whose
/// first element is the direction marker (`--->` client-to-server, `<---`
/// server-to-client) followed by the payload — and response lines embed the
/// original request, so a method can appear on both its send line and its
/// response echo. Client-originated lifecycle facts (initialize restarts,
/// didOpen/didClose ordering, configuration pushes) are counted from outgoing
/// send lines only; server pushes (publishDiagnostics) are mined from
/// incoming lines.
pub fn extract_freshness_wire(log: &[u8]) -> FreshnessWire {
    let text = String::from_utf8_lossy(log);
    let mut wire = FreshnessWire::default();
    for (index, line) in text.lines().enumerate() {
        let Some(value) = first_json_value(line) else { continue };
        if let serde_json::Value::Array(items) = &value
            && let Some(serde_json::Value::String(direction)) = items.first()
            && (direction == "--->" || direction == "<---")
        {
            if let Some(payload) = items.get(3) {
                walk_freshness_value(payload, index, direction == "--->", &mut wire);
            }
            continue;
        }
        // Unenveloped payloads (or other envelope shapes) are walked without
        // direction knowledge, matching the substrate's tolerant mining.
        walk_freshness_value(&value, index, true, &mut wire);
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

fn walk_freshness_value(
    value: &serde_json::Value,
    line_index: usize,
    outgoing: bool,
    wire: &mut FreshnessWire,
) {
    match value {
        serde_json::Value::Object(map) => {
            if let Some(serde_json::Value::String(method)) = map.get("method") {
                match method.as_str() {
                    "initialize" if outgoing => wire.initialize_count += 1,
                    "textDocument/didOpen" if outgoing => {
                        if let Some(token) = did_open_token(map) {
                            wire.did_open_lines.push((line_index, token));
                        }
                    }
                    "textDocument/didClose" if outgoing => {
                        if let Some(token) = did_open_token(map) {
                            wire.did_close_lines.push((line_index, token));
                        }
                    }
                    "workspace/didChangeConfiguration" if outgoing => {
                        wire.did_change_configuration_lines.push(line_index);
                    }
                    "textDocument/publishDiagnostics" => {
                        if let Some(batch) = mine_freshness_batch(map, line_index) {
                            wire.batches.push(batch);
                        }
                    }
                    _ => {}
                }
            }
            for child in map.values() {
                walk_freshness_value(child, line_index, outgoing, wire);
            }
        }
        serde_json::Value::Array(items) => {
            for child in items {
                walk_freshness_value(child, line_index, outgoing, wire);
            }
        }
        _ => {}
    }
}

fn did_open_token(map: &serde_json::Map<String, serde_json::Value>) -> Option<String> {
    let uri = map.get("params")?.get("textDocument")?.get("uri")?.as_str()?;
    let token = uri.rsplit('/').next().unwrap_or("").to_string();
    if token.is_empty() || token.contains('\\') { None } else { Some(token) }
}

fn mine_freshness_batch(
    map: &serde_json::Map<String, serde_json::Value>,
    line_index: usize,
) -> Option<FreshnessBatch> {
    let params = map.get("params")?;
    let uri = params.get("uri")?.as_str()?;
    let uri_file = uri.rsplit('/').next().unwrap_or("").to_string();
    if uri_file.is_empty() || uri_file.contains('\\') {
        return None;
    }
    let diagnostics = params.get("diagnostics")?.as_array()?;
    let mut batch = FreshnessBatch {
        line_index,
        uri_file,
        error_severity_count: 0,
        warning_severity_count: 0,
        pl701_count: 0,
        critic_policy_count: 0,
    };
    for diagnostic in diagnostics {
        match diagnostic.get("severity").and_then(serde_json::Value::as_i64) {
            Some(1) => batch.error_severity_count += 1,
            Some(2) => batch.warning_severity_count += 1,
            _ => {}
        }
        let code = diagnostic.get("code").and_then(serde_json::Value::as_str).unwrap_or("");
        if code == "PL701" {
            batch.pl701_count += 1;
        }
        if code == CRITIC_EXCLUDED_POLICY {
            batch.critic_policy_count += 1;
        }
    }
    Some(batch)
}

// ---------------------------------------------------------------------------
// Run
// ---------------------------------------------------------------------------

/// The typed outcome of one freshness host run.
pub struct FreshnessRunOutcome {
    pub receipt_path: PathBuf,
    pub result: ObservationResult,
    pub process_cleanup: CleanupResult,
    pub driver_complete: bool,
    /// The typed driver-failure reason when the driver failed; the negative
    /// variants' expected reason lands here.
    pub driver_failure_reason: Option<String>,
}

/// Execute one #11390 freshness-generations host run against the exact pinned
/// subject and write its canonical receipt. `variant` selects the fixture;
/// only `canonical` may pass.
pub fn host_freshness_run(
    repo_root: &Path,
    run: &VimHostRunInputs,
    variant: FreshnessFixtureVariant,
) -> Result<FreshnessRunOutcome> {
    crate::vim_host_run::ensure_fresh_output_root(&run.out_root)?;
    fs::create_dir_all(&run.out_root)
        .with_context(|| format!("creating output root {}", run.out_root.display()))?;

    let driver = repo_root.join("scripts/test/vim-host-freshness-driver.vim");
    let fixture = materialize_freshness_fixture(&run.out_root.join("fixture"), variant)?;
    let BoundHostPlan { plan, server_name, root_markers } = bind_host_run_plan(
        repo_root,
        run,
        &driver,
        &fixture.root,
        FRESHNESS_JOURNEY_SELECTOR,
        FRESHNESS_FIXTURE_ID,
    )?;
    let layout = HermeticVimLayout::prepare(&run.out_root.join("hermetic"))?;
    let mut command = build_vim_command_with_extras(
        &plan,
        &layout,
        &server_name,
        &root_markers,
        &freshness_env(&fixture.root, variant),
    )?;
    let mut observation = run_owned_process(&mut command, &plan, &layout)?;

    let client_log_bytes = fs::read(layout.client_log()).unwrap_or_default();
    let wire = vim_host_runner::extract_wire_evidence(&client_log_bytes);
    let freshness_wire = extract_freshness_wire(&client_log_bytes);
    observation
        .artifacts
        .extend(vim_host_runner::retain_wire_evidence_artifacts(&plan, &layout, &wire)?);

    let judgment =
        evaluate_freshness_observation(&plan, &observation, &wire, &freshness_wire, variant);

    let snapshot = layout.capability_snapshot();
    let snapshot_sha256 =
        if snapshot.is_file() { Some(vim_host_runner::file_sha256(&snapshot)?) } else { None };
    let capabilities = vim_host_runner::capabilities_from_wire_evidence(&wire, snapshot_sha256)?;
    let diagnostics = vim_host_runner::diagnostics_from_wire_evidence(&wire);

    let mut limitations = freshness_limitations(&observation, &judgment, &freshness_wire, variant);
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
        freshness_journey(&observation, &judgment, &wire),
        judgment.result,
        judgment.failure_class,
        limitations,
        format!(
            "#11390 {FRESHNESS_JOURNEY_SELECTOR}: external source, project config, client \
             settings, stale generation rejection, and provider ownership for the exact pinned \
             subject only"
        ),
    );
    let receipt_path = run.out_root.join("receipt.json");
    fs::write(&receipt_path, serde_json::to_vec_pretty(&receipt)?)
        .with_context(|| format!("writing receipt {}", receipt_path.display()))?;
    validate_receipt_binding(&receipt, &plan)
        .context("the emitted receipt failed its own freshness binding")?;
    Ok(FreshnessRunOutcome {
        receipt_path,
        result: judgment.result,
        process_cleanup: observation.cleanup,
        driver_complete: observation.driver_complete,
        driver_failure_reason: judgment.driver_failure_reason,
    })
}

fn freshness_limitations(
    observation: &ProcessObservation,
    judgment: &FreshnessJudgment,
    freshness_wire: &FreshnessWire,
    variant: FreshnessFixtureVariant,
) -> Vec<String> {
    let mut limitations = vec![
        "headless silent-ex Vim (-es): GUI-only client surfaces are not exercised by this harness"
            .to_string(),
        format!(
            "fixture variant {}: source/config generations and all expectations are Rust-authored, \
             never derived from run output; the fixture digest pins the initial state and every \
             later mutation is a typed journey event",
            variant.id()
        ),
        "route classification: this client exposes no workspace.didChangeWatchedFiles surface and \
         offers no workspace.diagnostics.refreshSupport, so open-document semantics materialize \
         only through explicit client materialization (reload/reopen, settings push plus reopen, \
         server restart); a client that exposes those surfaces is a different route row"
            .to_string(),
        "stale-generation holds are bounded absence observations: the window proves no spontaneous \
         republish occurred within it, not that none can ever occur"
            .to_string(),
        "the project-config route is restart-required for this subject: .perl-lsp.toml is \
         initialize-loaded only and no watcher reaches it; the client channel's includePaths \
         override is why the governed TOML discriminator is the [critic] exclude list"
            .to_string(),
        "the client-settings effect materializes on the next document open, not on a spontaneous \
         re-push: the client advertises no diagnostic refresh support"
            .to_string(),
        "save/recovery/reopen activation and maintained/public replay cells are separate leaves \
         and are not claimed here"
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
    limitations.push(format!(
        "wire process generations observed: {} initialize requests, {} didOpen events, {} \
         workspace/didChangeConfiguration notifications",
        freshness_wire.initialize_count,
        freshness_wire.did_open_lines.len(),
        freshness_wire.did_change_configuration_lines.len(),
    ));
    limitations
}

// ---------------------------------------------------------------------------
// Judgment
// ---------------------------------------------------------------------------

/// The six-cell judgment over one observed freshness run.
pub struct FreshnessJudgment {
    pub result: ObservationResult,
    pub failure_class: Option<crate::editor_client_compat::FailureClass>,
    pub driver_failure_reason: Option<String>,
    /// The initialize request's rootUri tail disagreed with the expected
    /// governed root (typed inconsistency; cannot pass).
    pub wrong_initialize_root: bool,
    /// Route facts recorded for the receipt: the client's own offered
    /// capabilities, read from the mined initialize request.
    pub client_watcher_exposed: bool,
    pub diagnostic_refresh_supported: bool,
    /// Per-cell results for the receipt journey, keyed by catalog cell id.
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

fn indexed_events(
    events: &[vim_host_runner::DriverEvent],
    kind: DriverEventKind,
) -> Vec<&vim_host_runner::DriverEvent> {
    let mut found: Vec<&vim_host_runner::DriverEvent> =
        events.iter().filter(|event| event.kind == kind).collect();
    found.sort_by_key(|event| {
        event
            .details
            .get(match kind {
                DriverEventKind::ExternalMutationApplied => "mutation_index",
                DriverEventKind::StaleGenerationHeld => "hold_index",
                DriverEventKind::ClientMaterializationApplied => "materialization_index",
                DriverEventKind::GenerationCurrentObserved => "generation_index",
                _ => "sequence",
            })
            .and_then(|value| value.parse::<u32>().ok())
            .unwrap_or(u32::MAX)
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

/// Whether the client's initialize-request capabilities object carries a
/// nested field (for example `workspace.didChangeWatchedFiles` or
/// `workspace.diagnostics.refreshSupport`).
fn client_capability_offers(
    client_capabilities: &Option<serde_json::Value>,
    section: &str,
    field: &str,
) -> bool {
    client_capabilities
        .as_ref()
        .and_then(|capabilities| capabilities.get(section))
        .and_then(|section| section.get(field))
        .is_some()
}

/// Whether a `file://` URI ends with the expected relative directory segment,
/// on every host's path spelling (mirrors the #10946 helper).
fn uri_ends_with_segment(uri: &str, segment: &str) -> bool {
    let normalized = uri.replace('\\', "/");
    normalized.trim_end_matches('/').ends_with(&format!("/{segment}"))
        || normalized.trim_end_matches('/') == format!("file:///{segment}")
        || normalized.ends_with(segment)
}

/// Judge one observed run against the scenario's Rust-authored expectations.
///
/// Positive path (all six cells must pass): registration bound to the planned
/// candidate digest and attach identity, native root equal to the governed
/// root and distinct from the decoy, the client's own capabilities carrying
/// no watcher surface, every generation converging only through the route the
/// subject supports, the old and decoy generations never repopulating state,
/// the settings effect attributable to the push alone, the project-config
/// generations following restarts exactly, and the orderly process boundary.
#[allow(clippy::too_many_lines)]
pub fn evaluate_freshness_observation(
    plan: &VimHostRunPlan,
    observation: &ProcessObservation,
    wire: &WireEvidence,
    freshness_wire: &FreshnessWire,
    variant: FreshnessFixtureVariant,
) -> FreshnessJudgment {
    let mut cells = BTreeMap::new();
    let events = &observation.events;

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
    let wrong_initialize_root = root_observed && !initialize_root_ok;
    let root_ok = root_observed
        && observed_root == Some(GOVERNED_ROOT_REL)
        && decoy_reported == Some(DECOY_ROOT_REL)
        && initialize_root_ok;

    // --- route facts from the client's own offered capabilities.
    let client_watcher_exposed =
        client_capability_offers(&wire.client_capabilities, "workspace", "didChangeWatchedFiles");
    let diagnostic_refresh_supported =
        client_capability_offers(&wire.client_capabilities, "workspace", "diagnostics");

    let mutations = indexed_events(events, DriverEventKind::ExternalMutationApplied);
    let holds = indexed_events(events, DriverEventKind::StaleGenerationHeld);
    let materializations = indexed_events(events, DriverEventKind::ClientMaterializationApplied);
    let generations = indexed_events(events, DriverEventKind::GenerationCurrentObserved);

    let generation_token = |index: usize| -> Option<&str> {
        generations.get(index).and_then(|event| event.details.get("generation")).map(String::as_str)
    };
    let generation_counts = |index: usize| -> Option<(u32, u32)> {
        generations.get(index).and_then(|event| {
            let errors = event.details.get("errors")?.parse::<u32>().ok()?;
            let warnings = event.details.get("warnings")?.parse::<u32>().ok()?;
            Some((errors, warnings))
        })
    };

    // --- route cell: honest classification of the routes this subject
    // genuinely supports, from the client's own capabilities and the observed
    // materialization modes — never from a registration or log token.
    let holds_observed = !holds.is_empty();
    let reload_route_observed = materializations.iter().any(|event| {
        event.details.get("materialization") == Some(&"client_close_reopen".to_string())
    });
    let settings_route_observed = materializations
        .iter()
        .any(|event| event.details.get("materialization") == Some(&"settings_push".to_string()));
    let restart_route_observed = materializations
        .iter()
        .any(|event| event.details.get("materialization") == Some(&"server_restart".to_string()));
    let route_observed = attach_identity_observed && root_observed;
    let route_ok = route_observed
        && !client_watcher_exposed
        && holds_observed
        && reload_route_observed
        && settings_route_observed
        && restart_route_observed;
    cells.insert(CELL_ROUTE.to_string(), cell_result(route_observed, route_ok));

    // --- external source cell: G1 clean, G2 defect through explicit reload,
    // G3 restoration through explicit reload, and the decoy mutation
    // invisible to the governed buffer. Every claim rests on the client's own
    // wire batches for the governed token, ordered against the didOpen
    // boundaries of each reload.
    let g1_clean_state =
        generation_counts(0) == Some((0, 0)) && generation_token(0) == Some("g1_clean");
    let g2_defect_state = generation_counts(1).is_some_and(|(errors, _)| errors >= 1)
        && generation_token(1) == Some("g2_defect");
    let g3_clean_state =
        generation_counts(2) == Some((0, 0)) && generation_token(2) == Some("g3_old_clean");
    let g1_wire = freshness_wire
        .settled_batch_after_open(MAIN_TOKEN, 0)
        .is_some_and(FreshnessBatch::is_clean);
    let g2_wire = freshness_wire
        .settled_batch_after_open(MAIN_TOKEN, 1)
        .is_some_and(|batch| batch.error_severity_count >= 1 && batch.pl701_count == 0);
    let g3_wire = freshness_wire
        .settled_batch_after_open(MAIN_TOKEN, 2)
        .is_some_and(FreshnessBatch::is_clean);
    let decoy_control_wire = freshness_wire
        .settled_batch_after_open(MAIN_TOKEN, 3)
        .is_some_and(FreshnessBatch::is_clean);
    let decoy_mutation_observed =
        mutations.iter().any(|event| event.details.get("target") == Some(&"decoy".to_string()));
    let external_source_observed = g1_clean_state || g2_defect_state || g3_clean_state;
    let external_source_ok = g1_clean_state
        && g2_defect_state
        && g3_clean_state
        && g1_wire
        && g2_wire
        && g3_wire
        && decoy_mutation_observed
        && decoy_control_wire;
    cells.insert(
        CELL_EXTERNAL_SOURCE.to_string(),
        cell_result(external_source_observed, external_source_ok),
    );

    // --- client settings cell: the PL701 discriminator is present through
    // the client's own state and wire, an identical client reopen without the
    // push leaves it present, the push is on the wire between the control and
    // effect reopens, and the effect reopen clears it in both surfaces.
    let settings_opens = freshness_wire.opens_of(SETTINGS_TOKEN);
    let settings_baseline_state = generation_counts(4).is_some_and(|(_, warnings)| warnings >= 1)
        && generation_token(4) == Some("settings_pl701_present");
    let settings_control_state = generation_counts(5).is_some_and(|(_, warnings)| warnings >= 1)
        && generation_token(5) == Some("settings_control_present");
    let settings_effect_state = generation_counts(6) == Some((0, 0))
        && generation_token(6) == Some("settings_push_cleared");
    let settings_baseline_wire = freshness_wire
        .settled_batch_after_open(SETTINGS_TOKEN, 0)
        .is_some_and(|batch| batch.warning_severity_count >= 1 && batch.pl701_count >= 1);
    let settings_control_wire = freshness_wire
        .settled_batch_after_open(SETTINGS_TOKEN, 1)
        .is_some_and(|batch| batch.warning_severity_count >= 1);
    let settings_push_between = {
        match (settings_opens.get(1), settings_opens.get(2)) {
            (Some(control), Some(effect)) => freshness_wire
                .did_change_configuration_lines
                .iter()
                .any(|line| line > control && line < effect),
            _ => false,
        }
    };
    let settings_effect_wire = freshness_wire
        .settled_batch_after_open(SETTINGS_TOKEN, 2)
        .is_some_and(FreshnessBatch::is_clean);
    let settings_push_observed = settings_baseline_state || settings_control_state;
    let settings_ok = settings_baseline_state
        && settings_control_state
        && settings_effect_state
        && settings_baseline_wire
        && settings_control_wire
        && settings_push_between
        && settings_effect_wire;
    cells
        .insert(CELL_CLIENT_SETTINGS.to_string(), cell_result(settings_push_observed, settings_ok));

    // --- project config cell: the critic discriminator follows the TOML
    // generations exactly across restarts — present at the baseline, absent
    // after the exclude config's restart, present again after the malformed
    // config's restart (the server rejects it honestly), absent after the
    // repair's restart — with no live effect between mutation and restart.
    let config_baseline_state = generation_counts(7).is_some_and(|(_, warnings)| warnings >= 1)
        && generation_token(7) == Some("config_critic_present");
    let config_excluded_state = generation_counts(8) == Some((0, 0))
        && generation_token(8) == Some("config_exclude_active");
    let config_malformed_state = generation_counts(9).is_some_and(|(_, warnings)| warnings >= 1)
        && generation_token(9) == Some("config_malformed_rejected");
    let config_repaired_state = generation_counts(10) == Some((0, 0))
        && generation_token(10) == Some("config_exclude_repaired");
    let config_wire_state_at = |open_index: usize, expect_warning: bool| -> bool {
        freshness_wire.settled_batch_after_open(CONFIG_TOKEN, open_index).is_some_and(|batch| {
            if expect_warning {
                batch.warning_severity_count >= 1 && batch.critic_policy_count >= 1
            } else {
                batch.critic_policy_count == 0 && batch.warning_severity_count == 0
            }
        })
    };
    let config_baseline_wire = config_wire_state_at(0, true);
    let config_excluded_wire = config_wire_state_at(1, false);
    let config_malformed_wire = config_wire_state_at(2, true);
    let config_repaired_wire = config_wire_state_at(3, false);
    let config_mutations_observed = mutations
        .iter()
        .filter(|event| event.details.get("target") == Some(&"project_config".to_string()))
        .count()
        >= 3;
    let config_holds_observed = holds.iter().any(|event| {
        event.details.get("held_generation").is_some_and(|value| value.starts_with("toml_"))
    });
    let restarts_observed = materializations
        .iter()
        .filter(|event| event.details.get("materialization") == Some(&"server_restart".to_string()))
        .count()
        >= 3
        && freshness_wire.initialize_count >= 4;
    let config_observed = config_baseline_state || config_excluded_state;
    let config_ok = config_baseline_state
        && config_excluded_state
        && config_malformed_state
        && config_repaired_state
        && config_baseline_wire
        && config_excluded_wire
        && config_malformed_wire
        && config_repaired_wire
        && config_mutations_observed
        && config_holds_observed
        && restarts_observed;
    cells.insert(CELL_PROJECT_CONFIG.to_string(), cell_result(config_observed, config_ok));

    // --- stale generation cell: the external mutations never repopulate the
    // client state on their own (bounded hold windows with the wire push
    // count unmoved), and the released old generation stays invisible until
    // its own explicit materialization.
    let source_holds = holds
        .iter()
        .filter(|event| {
            event
                .details
                .get("held_generation")
                .is_some_and(|value| value == "g2_defect" || value == "g3_old_clean")
        })
        .count();
    let held_windows_honest = holds.iter().all(|event| {
        event.details.get("state_held") == Some(&"1".to_string())
            && event
                .details
                .get("window_ms")
                .and_then(|value| value.parse::<u64>().ok())
                .is_some_and(|window| window >= vim_host_runner::MIN_STALE_WINDOW_MS)
    });
    // The wire side of stale rejection: the settled generation of the G2
    // window is still the defective one (the released old generation never
    // settled in as current before its own materialization), and the settled
    // generation after the decoy mutation is still the clean one (the decoy
    // never settled in either). The per-didOpen leading empty publish and the
    // on-close clearing publish are transport artifacts, not state claims:
    // settled semantics (last batch before the document's close) is the
    // honest oracle.
    let old_generation_never_settled = freshness_wire
        .settled_batch_after_open(MAIN_TOKEN, 1)
        .is_some_and(|batch| batch.error_severity_count >= 1)
        && freshness_wire
            .settled_batch_after_open(MAIN_TOKEN, 3)
            .is_some_and(FreshnessBatch::is_clean);
    let stale_observed = source_holds >= 2;
    let stale_ok = stale_observed && held_windows_honest && old_generation_never_settled;
    cells.insert(CELL_STALE_GENERATION.to_string(), cell_result(stale_observed, stale_ok));

    // --- provider ownership cell: the exact registered candidate digest, the
    // governed root identity with the decoy distinct, the decoy mutation
    // never supplying the governed result, and all judged batches carrying
    // the governed file tokens in this client's own log.
    let decoy_never_supplied = decoy_mutation_observed && decoy_control_wire;
    let provider_observed = attach_identity_observed && root_observed;
    let provider_ok = provider_observed
        && registration_digest_match
        && root_ok
        && decoy_never_supplied
        && external_source_ok;
    cells.insert(CELL_PROVIDER_OWNERSHIP.to_string(), cell_result(provider_observed, provider_ok));

    let driver_failed_event =
        events.iter().find(|event| event.kind == DriverEventKind::DriverFailed);
    let driver_failure_reason =
        driver_failed_event.and_then(|event| event.details.get("reason")).cloned();
    let leaked = observation.cleanup == CleanupResult::Fail;
    let six_cells_ok = cells.values().all(|result| *result == ObservationResult::Pass);
    let result = if observation.passed_process_boundary() && six_cells_ok {
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
    FreshnessJudgment {
        result,
        failure_class,
        driver_failure_reason,
        wrong_initialize_root,
        client_watcher_exposed,
        diagnostic_refresh_supported,
        cells,
    }
}

// ---------------------------------------------------------------------------
// Receipt journey
// ---------------------------------------------------------------------------

/// Compose the receipt journey: the lifecycle barrier cells (the #10944
/// surface, judged against the run's real mined wire — the teardown-deferred
/// shutdown cell needs the client's own exit trace) plus the six #11381
/// catalog cells this scenario evidences.
pub fn freshness_journey(
    observation: &ProcessObservation,
    judgment: &FreshnessJudgment,
    wire: &WireEvidence,
) -> Vec<JourneyCell> {
    let mut cells = crate::vim_host_run::outcome_journey(observation, wire);
    let catalog_limitations: BTreeMap<&str, &str> = BTreeMap::from([
        (
            CELL_ROUTE,
            "route shape only: explicit reload (open documents), client settings push, and \
             server restart; no client watcher surface is exposed by this subject and route \
             classification is never an automatic semantic pass",
        ),
        (
            CELL_EXTERNAL_SOURCE,
            "external source generations converge only through the explicit client reload route; \
             the same-named decoy at the wrong root is proven invisible",
        ),
        (
            CELL_PROJECT_CONFIG,
            "restart_required: the .perl-lsp.toml generations (create/malformed/repair) reach the \
             server only through server restarts; malformed is honestly rejected with the prior \
             semantics restored",
        ),
        (
            CELL_CLIENT_SETTINGS,
            "the settings push reaches the server live (workspace/didChangeConfiguration) and the \
             semantic result materializes on the next document open; the identical reopen without \
             the push is the in-journey control",
        ),
        (
            CELL_STALE_GENERATION,
            "held and released generations never repopulate client state within the bounded \
             observation windows; only an explicit materialization accepts a generation",
        ),
        (
            CELL_PROVIDER_OWNERSHIP,
            "the exact registered candidate digest, the governed root identity, and this client's \
             own wire record own every observed result",
        ),
    ]);
    for (cell_id, result) in &judgment.cells {
        cells.push(JourneyCell {
            id: cell_id.clone(),
            capability_basis: CapabilityBasis::NotApplicable,
            observed: *result != ObservationResult::NotProven,
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
        CELL_EXTERNAL_SOURCE | CELL_STALE_GENERATION => {
            vec!["vim/driver-events.jsonl".to_string(), "vim/vim-lsp-client.log".to_string()]
        }
        CELL_PROJECT_CONFIG | CELL_CLIENT_SETTINGS => {
            vec!["vim/driver-events.jsonl".to_string(), "vim/vim-lsp-client.log".to_string()]
        }
        _ => vec![
            "vim/driver-events.jsonl".to_string(),
            "vim/initialize-request.json".to_string(),
            "vim/process-ledger.json".to_string(),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_variants_parse_and_carry_typed_negative_reasons() {
        assert!(matches!(
            FreshnessFixtureVariant::from_id("canonical"),
            Ok(FreshnessFixtureVariant::Canonical)
        ));
        assert!(FreshnessFixtureVariant::from_id("other").is_err());
        assert_eq!(
            FreshnessFixtureVariant::WrongRootDecoy.expected_negative_reason(),
            Some("root_mismatch")
        );
        assert_eq!(
            FreshnessFixtureVariant::LiveReloadClaimed.expected_negative_reason(),
            Some("live_freshness_absent")
        );
        assert_eq!(
            FreshnessFixtureVariant::AmbientPathOnly.expected_negative_reason(),
            Some("settings_effect_absent")
        );
        assert_eq!(FreshnessFixtureVariant::Canonical.expected_negative_reason(), None);
    }

    #[test]
    fn defect_text_changes_exactly_the_mutation_line() {
        let clean = clean_source_text();
        let defect = defect_source_text();
        assert_ne!(clean, defect);
        let clean_lines: Vec<&str> = clean.lines().collect();
        let defect_lines: Vec<&str> = defect.lines().collect();
        assert_eq!(clean_lines.len(), defect_lines.len());
        for index in 0..clean_lines.len() {
            if index + 1 == MUTATION_LINE {
                assert_eq!(defect_lines[index], DEFECT_LINE_TEXT);
                assert_ne!(defect_lines[index], clean_lines[index]);
            } else {
                assert_eq!(defect_lines[index], clean_lines[index]);
            }
        }
    }

    #[test]
    fn freshness_wire_mines_generations_pushes_and_warning_batches() {
        let log = concat!(
            "{\"method\":\"initialize\",\"params\":{}}\n",
            "{\"method\":\"textDocument/didOpen\",\"params\":{\"textDocument\":{\"uri\":\"file:///w/project/main.pl\"}}}\n",
            "{\"method\":\"textDocument/publishDiagnostics\",\"params\":{\"uri\":\"file:///w/project/main.pl\",\"diagnostics\":[{\"severity\":2,\"code\":\"PL701\"}]}}\n",
            "{\"method\":\"workspace/didChangeConfiguration\",\"params\":{}}\n",
            "{\"method\":\"textDocument/didClose\",\"params\":{\"textDocument\":{\"uri\":\"file:///w/project/main.pl\"}}}\n",
            "{\"method\":\"textDocument/didOpen\",\"params\":{\"textDocument\":{\"uri\":\"file:///w/project/settings.pl\"}}}\n",
            "{\"method\":\"textDocument/publishDiagnostics\",\"params\":{\"uri\":\"file:///w/project/settings.pl\",\"diagnostics\":[{\"severity\":2,\"code\":\"native.common.stale_dollar_at\"}]}}\n",
            "{\"method\":\"initialize\",\"params\":{}}\n"
        );
        let wire = extract_freshness_wire(log.as_bytes());
        assert_eq!(wire.initialize_count, 2);
        assert_eq!(wire.did_open_lines.len(), 2);
        assert_eq!(wire.did_close_lines.len(), 1);
        assert_eq!(wire.did_change_configuration_lines, vec![3]);
        assert_eq!(wire.opens_of("main.pl"), vec![1]);
        assert_eq!(wire.opens_of("settings.pl"), vec![5]);
        let main_batches = wire.batches_of("main.pl");
        assert_eq!(main_batches.len(), 1);
        assert_eq!(main_batches[0].pl701_count, 1);
        assert_eq!(main_batches[0].warning_severity_count, 1);
        let settings_batches = wire.batches_of("settings.pl");
        assert_eq!(settings_batches[0].critic_policy_count, 1);
    }

    #[test]
    fn settings_channels_stay_relative_except_in_the_ambient_negative() {
        let root = Path::new("/tmp/fixture");
        assert_eq!(settings_include_paths(root, FreshnessFixtureVariant::Canonical), "lib,vendor");
        let ambient = settings_include_paths(root, FreshnessFixtureVariant::AmbientPathOnly);
        assert_eq!(ambient, "lib,/tmp/fixture/workspace/project/vendor");
    }
}

//! Exact, source-backed Emacs stock-discovery observations (#13610).
//!
//! This module records what exact released and upstream-source Eglot/lsp-mode
//! revisions register for Perl. It is lower-tier than an actual Emacs host
//! receipt and cannot promote support. The checked subject manifest owns
//! package/source subjects; this projection owns only one exhaustive source
//! observation at an exact repository commit and tree.

use crate::editor_client_compat::ClientSourceState;
use crate::emacs_subject_manifest::SubjectClientKind;
use anyhow::{Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const SCHEMA_VERSION: &str = "emacs_stock_discovery.v1";
pub const CLAIM_CEILING: &str = "source_registration_observation";

const EMACS_REPOSITORY: &str = "https://github.com/emacs-mirror/emacs";
const LSP_MODE_REPOSITORY: &str = "https://github.com/emacs-lsp/lsp-mode";

/// Audited exact identity for one named observation. Validation compares the
/// rendered row against these values, so a replaced-but-shape-valid commit,
/// tree, or blob fails closed instead of publishing a fabricated exact
/// observation. The checked constructors and this table agree by construction;
/// any one-sided edit fails `validate()` in the contract tests.
#[derive(Debug, Clone, PartialEq, Eq)]
struct AuditedObservation {
    observation_id: &'static str,
    /// Canonical checked-manifest subject the row joins to, when one exists.
    subject_id: Option<&'static str>,
    commit: &'static str,
    tree_sha1: &'static str,
    /// `(path, blob)` for every observed file, in stable path order.
    files: &'static [(&'static str, &'static str)],
}

const AUDITED_OBSERVATIONS: &[AuditedObservation] = &[
    AuditedObservation {
        observation_id: "eglot_released_1_24_stock_registry",
        subject_id: Some("released_eglot_gnu_elpa_1_24"),
        commit: "0d67e76b94e1f0af9fe364aed8aa5db1c494c206",
        tree_sha1: "fa588e3cafbd43e97b3ac9cd1a0bc727430c4731",
        files: &[("lisp/progmodes/eglot.el", "f701a38ab8bd9ad984c58320907cb8a93396ec69")],
    },
    AuditedObservation {
        observation_id: "eglot_source_f4f249a2_stock_registry",
        subject_id: None,
        commit: "f4f249a2249a7047ba41a659b8fcdcd7e1caf4e0",
        tree_sha1: "ffd5ed14f7cc689e22163527e47d6ae0d0acbea0",
        files: &[("lisp/progmodes/eglot.el", "f2a9e36989cd90500e66900efe5138e6dee56668")],
    },
    AuditedObservation {
        observation_id: "lsp_mode_released_10_0_0_clients",
        subject_id: Some("released_lsp_mode_melpa_stable_10_0_0"),
        commit: "913a6c07f163205cb568bc68d7dfe677dbc358ab",
        tree_sha1: "0941b1b96aee881e5256bf2fce9ad75391c1abb4",
        files: &[
            ("clients/lsp-perl.el", "28569f7ecf22a5b02762976ef338693c655679ad"),
            ("clients/lsp-perlnavigator.el", "51cbc768c960c40433d61299cc837b94f7830423"),
            ("clients/lsp-pls.el", "f5437fbf739dd661d0850ede25ad7ef73dbf81d4"),
            ("lsp-mode.el", "ee16b0ca0c999eb9ba6db25a46ec3c1a19330619"),
        ],
    },
    AuditedObservation {
        observation_id: "lsp_mode_source_e15b8205_clients",
        subject_id: None,
        commit: "e15b8205cbd0369df40b412909eb3ed3264e96a2",
        tree_sha1: "51274cfa292c0b5ae0a70edb9c0c61b153b5f916",
        files: &[
            ("clients/lsp-perl.el", "28569f7ecf22a5b02762976ef338693c655679ad"),
            ("clients/lsp-perlnavigator.el", "51cbc768c960c40433d61299cc837b94f7830423"),
            ("clients/lsp-pls.el", "f5437fbf739dd661d0850ede25ad7ef73dbf81d4"),
            ("lsp-mode.el", "9575875cd4c7ef49ab0bd8e5473e44f73c4a0c7d"),
        ],
    },
];

fn audited_observation(observation_id: &str) -> Result<&'static AuditedObservation> {
    AUDITED_OBSERVATIONS
        .iter()
        .find(|row| row.observation_id == observation_id)
        .ok_or_else(|| anyhow::anyhow!("unaudited observation_id {observation_id}"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RegistrationSurface {
    EglotServerPrograms,
    LspModeClientModules,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservedSourceFile {
    pub path: String,
    pub git_blob_sha1: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegistrationEntry {
    pub entry_id: String,
    pub major_modes: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub activation_language: Option<String>,
    pub command_shape: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StockDiscoveryObservation {
    pub observation_id: String,
    /// Canonical checked-manifest subject this row joins to, when the checked
    /// manifest owns this exact revision. Upstream-source rows observed at
    /// commits no manifest row pins stay `None` rather than manufacturing an
    /// alias unknown to `SubjectManifest::row_for` and the host registry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject_id: Option<String>,
    pub client_kind: SubjectClientKind,
    pub client_version: String,
    pub source_state: ClientSourceState,
    pub repository: String,
    pub commit: String,
    pub tree_sha1: String,
    pub registration_surface: RegistrationSurface,
    pub search_scope: Vec<String>,
    pub observed_files: Vec<ObservedSourceFile>,
    pub observation_complete: bool,
    pub manual_registration_injected: bool,
    pub perllsp_present: bool,
    pub entries: Vec<RegistrationEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StockDiscoveryBaseline {
    pub schema_version: String,
    pub claim_ceiling: String,
    pub observations: Vec<StockDiscoveryObservation>,
}

impl StockDiscoveryBaseline {
    pub fn validate(&self) -> Result<()> {
        ensure!(self.schema_version == SCHEMA_VERSION, "schema_version must be {SCHEMA_VERSION}");
        ensure!(self.claim_ceiling == CLAIM_CEILING, "claim_ceiling must be {CLAIM_CEILING}");
        ensure!(self.observations.len() == 4, "baseline must contain four exact observations");

        let mut ids = BTreeSet::new();
        for (index, observation) in self.observations.iter().enumerate() {
            observation.validate()?;
            ensure!(
                ids.insert(observation.observation_id.as_str()),
                "duplicate observation_id {}",
                observation.observation_id
            );
            ensure!(
                dimension_rank(observation)? == index,
                "observations must use stable released/source order for Eglot then lsp-mode"
            );
        }
        Ok(())
    }
}

impl StockDiscoveryObservation {
    fn validate(&self) -> Result<()> {
        ensure!(is_subject_token(&self.observation_id), "observation_id must be a stable token");
        ensure!(!self.client_version.trim().is_empty(), "client_version must be present");
        ensure!(
            self.repository.starts_with("https://github.com/"),
            "repository must be an exact GitHub URL"
        );
        ensure!(is_lower_hex(&self.commit, 40), "commit must be an exact 40-hex revision");
        ensure!(is_lower_hex(&self.tree_sha1, 40), "tree_sha1 must be an exact 40-hex Git tree");
        let audited = audited_observation(&self.observation_id)?;
        ensure!(
            self.commit == audited.commit,
            "commit {} does not match the audited revision for {}",
            self.commit,
            self.observation_id
        );
        ensure!(
            self.tree_sha1 == audited.tree_sha1,
            "tree_sha1 does not match the audited tree for {}",
            self.observation_id
        );
        ensure!(
            self.subject_id.as_deref() == audited.subject_id,
            "subject_id must bind the canonical checked-manifest subject for {}",
            self.observation_id
        );
        ensure!(
            self.observation_complete,
            "absence cannot be inferred from an incomplete observation"
        );
        ensure!(
            !self.manual_registration_injected,
            "manual registration cannot satisfy stock discovery"
        );
        validate_sorted_tokens(&self.search_scope, "search_scope")?;
        validate_source_files(&self.observed_files)?;
        bind_observed_files_to_audited_identity(&self.observed_files, audited)?;

        ensure!(
            !self.entries.is_empty(),
            "complete observation must retain the existing Perl entry set"
        );
        let mut entry_ids = BTreeSet::new();
        for entry in &self.entries {
            entry.validate()?;
            ensure!(
                entry_ids.insert(entry.entry_id.as_str()),
                "duplicate registration entry {}",
                entry.entry_id
            );
        }

        let observed_perllsp = self.entries.iter().any(RegistrationEntry::is_perllsp);
        ensure!(
            self.perllsp_present == observed_perllsp,
            "perllsp_present must be derived from the exact observed entry set"
        );

        match self.client_kind {
            SubjectClientKind::ExternalEglot => self.validate_eglot(),
            SubjectClientKind::LspMode => self.validate_lsp_mode(),
            SubjectClientKind::BundledEglot => {
                bail!("this baseline records external released/source subjects only")
            }
        }
    }

    fn validate_eglot(&self) -> Result<()> {
        ensure!(
            self.repository == EMACS_REPOSITORY,
            "Eglot rows must bind the audited Emacs mirror"
        );
        ensure!(
            self.registration_surface == RegistrationSurface::EglotServerPrograms,
            "Eglot rows must observe eglot-server-programs"
        );
        ensure!(
            strings_equal(&self.search_scope, &["lisp/progmodes/eglot.el:eglot-server-programs"]),
            "Eglot observation must cover the complete stock registry"
        );
        ensure!(
            self.entries.len() == 1,
            "current exact Eglot subjects must retain one Perl contact"
        );
        let Some(entry) = self.entries.first() else {
            bail!("Eglot Perl contact is missing");
        };
        ensure!(
            entry.entry_id == "perl_language_server",
            "Eglot baseline must retain the legacy Perl contact"
        );
        ensure!(
            strings_equal(&entry.major_modes, &["perl-mode", "cperl-mode"]),
            "Eglot Perl modes changed"
        );
        ensure!(
            entry.activation_language.is_none(),
            "current Eglot source does not declare an explicit Perl language id here"
        );
        ensure!(
            entry.server_id.is_none() && entry.priority.is_none(),
            "Eglot contact is not an lsp-mode client row"
        );
        ensure!(
            strings_equal(
                &entry.command_shape,
                &["perl", "-MPerl::LanguageServer", "-e", "Perl::LanguageServer::run",]
            ),
            "Eglot legacy Perl command changed"
        );
        Ok(())
    }

    fn validate_lsp_mode(&self) -> Result<()> {
        ensure!(
            self.repository == LSP_MODE_REPOSITORY,
            "lsp-mode rows must bind the audited upstream repository"
        );
        ensure!(
            self.registration_surface == RegistrationSurface::LspModeClientModules,
            "lsp-mode rows must observe client modules"
        );
        ensure!(
            strings_equal(&self.search_scope, &["clients/", "lsp-mode.el"]),
            "lsp-mode absence must bind the full client directory and package root"
        );
        ensure!(
            self.entries.len() == 3,
            "current exact lsp-mode subjects must retain three Perl clients"
        );
        ensure!(
            self.entries.iter().map(|entry| entry.entry_id.as_str()).eq([
                "perlnavigator",
                "pls",
                "perl_language_server"
            ]),
            "lsp-mode Perl client set/order changed"
        );
        ensure!(
            self.entries.iter().map(|entry| entry.priority).eq([Some(0), Some(-1), Some(-2)]),
            "lsp-mode Perl priority order changed"
        );
        ensure!(
            self.entries.iter().map(|entry| entry.activation_language.as_deref()).eq([
                Some("perl"),
                Some("perl"),
                None
            ]),
            "lsp-mode activation-vs-major-mode language identity changed"
        );

        let navigator = self.entry("perlnavigator")?;
        ensure!(
            navigator.major_modes.is_empty(),
            "Perl Navigator uses language activation, not a major-mode list"
        );
        ensure!(
            navigator.server_id.as_deref() == Some("perlnavigator"),
            "Perl Navigator server id changed"
        );
        ensure!(
            strings_equal(
                &navigator.command_shape,
                &["managed_or_configured:perlnavigator", "--stdio"]
            ),
            "Perl Navigator command shape changed"
        );

        let pls = self.entry("pls")?;
        ensure!(pls.major_modes.is_empty(), "PLS uses language activation, not a major-mode list");
        ensure!(pls.server_id.as_deref() == Some("pls"), "PLS server id changed");
        ensure!(
            strings_equal(
                &pls.command_shape,
                &["configured:lsp-pls-executable", "configured:lsp-pls-arguments..."]
            ),
            "PLS command shape changed"
        );

        let legacy = self.entry("perl_language_server")?;
        ensure!(
            strings_equal(&legacy.major_modes, &["perl-mode", "cperl-mode"]),
            "Perl::LanguageServer major modes changed"
        );
        ensure!(
            legacy.server_id.as_deref() == Some("perl-language-server"),
            "Perl::LanguageServer server id changed"
        );
        ensure!(
            strings_equal(
                &legacy.command_shape,
                &[
                    "configured:lsp-perl-language-server-path",
                    "-MPerl::LanguageServer",
                    "-e",
                    "Perl::LanguageServer::run",
                    "--",
                    "--port {port} --version {client-version}",
                ]
            ),
            "Perl::LanguageServer command shape changed"
        );
        Ok(())
    }

    fn entry(&self, entry_id: &str) -> Result<&RegistrationEntry> {
        self.entries
            .iter()
            .find(|entry| entry.entry_id == entry_id)
            .ok_or_else(|| anyhow::anyhow!("missing registration entry {entry_id}"))
    }
}

impl RegistrationEntry {
    fn validate(&self) -> Result<()> {
        ensure!(is_subject_token(&self.entry_id), "entry_id must be a stable token");
        ensure!(
            !self.major_modes.is_empty() || self.activation_language.is_some(),
            "entry must retain major modes or a language activation selector"
        );
        ensure!(
            self.command_shape.first().is_some_and(|program| !program.is_empty()),
            "registration command must have a program shape"
        );
        if let Some(server_id) = &self.server_id {
            ensure!(is_wire_identifier(server_id), "server_id must be a stable wire identifier");
        }
        Ok(())
    }

    fn is_perllsp(&self) -> bool {
        self.entry_id == "perllsp"
            || self.server_id.as_deref() == Some("perllsp")
            || self.command_shape.first().is_some_and(|program| program == "perllsp")
    }
}

pub fn checked_baseline() -> StockDiscoveryBaseline {
    StockDiscoveryBaseline {
        schema_version: SCHEMA_VERSION.to_string(),
        claim_ceiling: CLAIM_CEILING.to_string(),
        observations: vec![
            eglot_observation(
                "eglot_released_1_24_stock_registry",
                Some("released_eglot_gnu_elpa_1_24"),
                "1.24",
                ClientSourceState::Released,
                "0d67e76b94e1f0af9fe364aed8aa5db1c494c206",
                "fa588e3cafbd43e97b3ac9cd1a0bc727430c4731",
                "f701a38ab8bd9ad984c58320907cb8a93396ec69",
            ),
            eglot_observation(
                "eglot_source_f4f249a2_stock_registry",
                None,
                "1.24",
                ClientSourceState::UpstreamSource,
                "f4f249a2249a7047ba41a659b8fcdcd7e1caf4e0",
                "ffd5ed14f7cc689e22163527e47d6ae0d0acbea0",
                "f2a9e36989cd90500e66900efe5138e6dee56668",
            ),
            lsp_mode_observation(
                "lsp_mode_released_10_0_0_clients",
                Some("released_lsp_mode_melpa_stable_10_0_0"),
                "10.0.0",
                ClientSourceState::Released,
                "913a6c07f163205cb568bc68d7dfe677dbc358ab",
                "0941b1b96aee881e5256bf2fce9ad75391c1abb4",
                "ee16b0ca0c999eb9ba6db25a46ec3c1a19330619",
            ),
            lsp_mode_observation(
                "lsp_mode_source_e15b8205_clients",
                None,
                "10.0.1",
                ClientSourceState::UpstreamSource,
                "e15b8205cbd0369df40b412909eb3ed3264e96a2",
                "51274cfa292c0b5ae0a70edb9c0c61b153b5f916",
                "9575875cd4c7ef49ab0bd8e5473e44f73c4a0c7d",
            ),
        ],
    }
}

pub fn render_checked_json() -> Result<String> {
    let baseline = checked_baseline();
    baseline.validate()?;
    let mut rendered = serde_json::to_string_pretty(&baseline)?;
    rendered.push('\n');
    Ok(rendered)
}

fn eglot_observation(
    observation_id: &str,
    subject_id: Option<&str>,
    client_version: &str,
    source_state: ClientSourceState,
    commit: &str,
    tree_sha1: &str,
    blob_sha1: &str,
) -> StockDiscoveryObservation {
    StockDiscoveryObservation {
        observation_id: observation_id.to_string(),
        subject_id: subject_id.map(str::to_string),
        client_kind: SubjectClientKind::ExternalEglot,
        client_version: client_version.to_string(),
        source_state,
        repository: EMACS_REPOSITORY.to_string(),
        commit: commit.to_string(),
        tree_sha1: tree_sha1.to_string(),
        registration_surface: RegistrationSurface::EglotServerPrograms,
        search_scope: vec!["lisp/progmodes/eglot.el:eglot-server-programs".to_string()],
        observed_files: vec![ObservedSourceFile {
            path: "lisp/progmodes/eglot.el".to_string(),
            git_blob_sha1: blob_sha1.to_string(),
        }],
        observation_complete: true,
        manual_registration_injected: false,
        perllsp_present: false,
        entries: vec![RegistrationEntry {
            entry_id: "perl_language_server".to_string(),
            major_modes: vec!["perl-mode".to_string(), "cperl-mode".to_string()],
            activation_language: None,
            command_shape: vec![
                "perl".to_string(),
                "-MPerl::LanguageServer".to_string(),
                "-e".to_string(),
                "Perl::LanguageServer::run".to_string(),
            ],
            server_id: None,
            priority: None,
        }],
    }
}

fn lsp_mode_observation(
    observation_id: &str,
    subject_id: Option<&str>,
    client_version: &str,
    source_state: ClientSourceState,
    commit: &str,
    tree_sha1: &str,
    root_blob_sha1: &str,
) -> StockDiscoveryObservation {
    StockDiscoveryObservation {
        observation_id: observation_id.to_string(),
        subject_id: subject_id.map(str::to_string),
        client_kind: SubjectClientKind::LspMode,
        client_version: client_version.to_string(),
        source_state,
        repository: LSP_MODE_REPOSITORY.to_string(),
        commit: commit.to_string(),
        tree_sha1: tree_sha1.to_string(),
        registration_surface: RegistrationSurface::LspModeClientModules,
        search_scope: vec!["clients/".to_string(), "lsp-mode.el".to_string()],
        observed_files: vec![
            ObservedSourceFile {
                path: "clients/lsp-perl.el".to_string(),
                git_blob_sha1: "28569f7ecf22a5b02762976ef338693c655679ad".to_string(),
            },
            ObservedSourceFile {
                path: "clients/lsp-perlnavigator.el".to_string(),
                git_blob_sha1: "51cbc768c960c40433d61299cc837b94f7830423".to_string(),
            },
            ObservedSourceFile {
                path: "clients/lsp-pls.el".to_string(),
                git_blob_sha1: "f5437fbf739dd661d0850ede25ad7ef73dbf81d4".to_string(),
            },
            ObservedSourceFile {
                path: "lsp-mode.el".to_string(),
                git_blob_sha1: root_blob_sha1.to_string(),
            },
        ],
        observation_complete: true,
        manual_registration_injected: false,
        perllsp_present: false,
        entries: vec![
            RegistrationEntry {
                entry_id: "perlnavigator".to_string(),
                major_modes: Vec::new(),
                activation_language: Some("perl".to_string()),
                command_shape: vec![
                    "managed_or_configured:perlnavigator".to_string(),
                    "--stdio".to_string(),
                ],
                server_id: Some("perlnavigator".to_string()),
                priority: Some(0),
            },
            RegistrationEntry {
                entry_id: "pls".to_string(),
                major_modes: Vec::new(),
                activation_language: Some("perl".to_string()),
                command_shape: vec![
                    "configured:lsp-pls-executable".to_string(),
                    "configured:lsp-pls-arguments...".to_string(),
                ],
                server_id: Some("pls".to_string()),
                priority: Some(-1),
            },
            RegistrationEntry {
                entry_id: "perl_language_server".to_string(),
                major_modes: vec!["perl-mode".to_string(), "cperl-mode".to_string()],
                activation_language: None,
                command_shape: vec![
                    "configured:lsp-perl-language-server-path".to_string(),
                    "-MPerl::LanguageServer".to_string(),
                    "-e".to_string(),
                    "Perl::LanguageServer::run".to_string(),
                    "--".to_string(),
                    "--port {port} --version {client-version}".to_string(),
                ],
                server_id: Some("perl-language-server".to_string()),
                priority: Some(-2),
            },
        ],
    }
}

fn validate_sorted_tokens(values: &[String], field: &str) -> Result<()> {
    ensure!(!values.is_empty(), "{field} must not be empty");
    ensure!(
        values.windows(2).all(|pair| pair[0] < pair[1]),
        "{field} must be unique and deterministically sorted"
    );
    Ok(())
}

/// Bind every observed file's path and blob to the audited inventory, so a
/// swapped-but-shape-valid blob cannot pass as an exact source observation.
fn bind_observed_files_to_audited_identity(
    files: &[ObservedSourceFile],
    audited: &AuditedObservation,
) -> Result<()> {
    ensure!(
        files.len() == audited.files.len(),
        "observed file set must match the audited inventory for {}",
        audited.observation_id
    );
    for file in files {
        let audited_blob =
            audited.files.iter().find(|(path, _)| *path == file.path).map(|(_, blob)| *blob);
        ensure!(
            audited_blob == Some(file.git_blob_sha1.as_str()),
            "observed blob for {} does not match the audited identity for {}",
            file.path,
            audited.observation_id
        );
    }
    Ok(())
}

fn validate_source_files(files: &[ObservedSourceFile]) -> Result<()> {
    ensure!(!files.is_empty(), "at least one exact observed source file is required");
    let mut paths = BTreeSet::new();
    let mut previous_path: Option<&str> = None;
    for file in files {
        ensure!(
            !file.path.starts_with('/') && !file.path.contains(".."),
            "source paths must be bounded and relative"
        );
        ensure!(is_lower_hex(&file.git_blob_sha1, 40), "source blob must be exact 40-hex SHA-1");
        ensure!(paths.insert(file.path.as_str()), "duplicate observed source path {}", file.path);
        if let Some(previous) = previous_path {
            ensure!(
                previous < file.path.as_str(),
                "observed source files must use stable path order"
            );
        }
        previous_path = Some(file.path.as_str());
    }
    Ok(())
}

fn dimension_rank(observation: &StockDiscoveryObservation) -> Result<usize> {
    match (observation.client_kind, observation.source_state) {
        (SubjectClientKind::ExternalEglot, ClientSourceState::Released) => Ok(0),
        (SubjectClientKind::ExternalEglot, ClientSourceState::UpstreamSource) => Ok(1),
        (SubjectClientKind::LspMode, ClientSourceState::Released) => Ok(2),
        (SubjectClientKind::LspMode, ClientSourceState::UpstreamSource) => Ok(3),
        _ => bail!("baseline accepts only external Eglot/lsp-mode released/source rows"),
    }
}

fn strings_equal(actual: &[String], expected: &[&str]) -> bool {
    actual.iter().map(String::as_str).eq(expected.iter().copied())
}

fn is_lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn is_subject_token(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

fn is_wire_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_' || byte == b'-'
        })
}

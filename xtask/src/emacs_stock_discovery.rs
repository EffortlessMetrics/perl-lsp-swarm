//! Exact, source-backed Emacs stock-discovery observations (#13610).
//!
//! This module records what exact released and upstream-source Eglot/lsp-mode
//! subjects register for Perl. It is lower-tier than an actual Emacs host
//! receipt and cannot promote support. The checked subject manifest owns
//! package/source identity; this projection owns only the observed upstream
//! registration/client entries at one exact source revision.

use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const SCHEMA_VERSION: &str = "emacs_stock_discovery.v1";
pub const CLAIM_CEILING: &str = "source_registration_observation";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClientFamily {
    Eglot,
    LspMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceState {
    Released,
    UpstreamSource,
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
    pub modes: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language_id: Option<String>,
    pub command: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StockDiscoveryObservation {
    pub subject_id: String,
    pub client_family: ClientFamily,
    pub client_version: String,
    pub source_state: SourceState,
    pub repository: String,
    pub commit: String,
    pub registration_surface: RegistrationSurface,
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
        let mut dimensions = BTreeSet::new();
        let mut previous_key: Option<(ClientFamily, SourceState)> = None;
        for observation in &self.observations {
            observation.validate()?;
            ensure!(ids.insert(observation.subject_id.as_str()), "duplicate subject_id {}", observation.subject_id);
            ensure!(
                dimensions.insert((observation.client_family, observation.source_state)),
                "duplicate client/source-state observation"
            );
            let key = (observation.client_family, observation.source_state);
            if let Some(previous) = previous_key {
                ensure!(previous < key, "observations must use stable client/source-state order");
            }
            previous_key = Some(key);
        }

        ensure!(
            dimensions
                == BTreeSet::from([
                    (ClientFamily::Eglot, SourceState::Released),
                    (ClientFamily::Eglot, SourceState::UpstreamSource),
                    (ClientFamily::LspMode, SourceState::Released),
                    (ClientFamily::LspMode, SourceState::UpstreamSource),
                ]),
            "baseline must keep released/source rows independent for both clients"
        );
        Ok(())
    }
}

impl StockDiscoveryObservation {
    fn validate(&self) -> Result<()> {
        ensure!(is_token(&self.subject_id), "subject_id must be a stable token");
        ensure!(!self.client_version.trim().is_empty(), "client_version must be present");
        ensure!(self.repository.starts_with("https://github.com/"), "repository must be an exact GitHub URL");
        ensure!(is_lower_hex(&self.commit, 40), "commit must be an exact 40-hex revision");
        ensure!(self.observation_complete, "absence cannot be inferred from an incomplete observation");
        ensure!(!self.manual_registration_injected, "manual registration cannot satisfy stock discovery");
        ensure!(!self.observed_files.is_empty(), "at least one exact source file is required");

        let mut paths = BTreeSet::new();
        let mut previous_path: Option<&str> = None;
        for file in &self.observed_files {
            ensure!(!file.path.starts_with('/') && !file.path.contains(".."), "source paths must be bounded and relative");
            ensure!(is_lower_hex(&file.git_blob_sha1, 40), "source blob must be exact 40-hex SHA-1");
            ensure!(paths.insert(file.path.as_str()), "duplicate observed source path {}", file.path);
            if let Some(previous) = previous_path {
                ensure!(previous < file.path.as_str(), "observed source files must use stable path order");
            }
            previous_path = Some(file.path.as_str());
        }

        ensure!(!self.entries.is_empty(), "complete observation must retain the existing Perl entry set");
        let mut entry_ids = BTreeSet::new();
        for entry in &self.entries {
            entry.validate()?;
            ensure!(entry_ids.insert(entry.entry_id.as_str()), "duplicate registration entry {}", entry.entry_id);
        }

        let observed_perllsp = self.entries.iter().any(|entry| {
            entry.server_id.as_deref() == Some("perllsp")
                || entry.command.first().is_some_and(|program| program == "perllsp")
        });
        ensure!(
            self.perllsp_present == observed_perllsp,
            "perllsp_present must be derived from the exact observed entry set"
        );

        match self.client_family {
            ClientFamily::Eglot => self.validate_eglot(),
            ClientFamily::LspMode => self.validate_lsp_mode(),
        }
    }

    fn validate_eglot(&self) -> Result<()> {
        ensure!(
            self.registration_surface == RegistrationSurface::EglotServerPrograms,
            "Eglot rows must observe eglot-server-programs"
        );
        ensure!(self.entries.len() == 1, "current exact Eglot subjects must retain one Perl contact");
        let entry = &self.entries[0];
        ensure!(entry.entry_id == "perl_language_server", "Eglot baseline must retain the legacy Perl contact");
        ensure!(entry.modes == ["perl-mode", "cperl-mode"], "Eglot Perl modes changed");
        ensure!(entry.language_id.is_none(), "current Eglot source does not declare an explicit Perl language id here");
        ensure!(entry.server_id.is_none() && entry.priority.is_none(), "Eglot contact is not an lsp-mode client row");
        ensure!(
            entry.command
                == [
                    "perl",
                    "-MPerl::LanguageServer",
                    "-e",
                    "Perl::LanguageServer::run",
                ],
            "Eglot legacy Perl command changed"
        );
        Ok(())
    }

    fn validate_lsp_mode(&self) -> Result<()> {
        ensure!(
            self.registration_surface == RegistrationSurface::LspModeClientModules,
            "lsp-mode rows must observe client modules"
        );
        ensure!(self.entries.len() == 3, "current exact lsp-mode subjects must retain three Perl clients");
        let priorities = self.entries.iter().map(|entry| entry.priority).collect::<Vec<_>>();
        ensure!(priorities == [Some(0), Some(-1), Some(-2)], "lsp-mode Perl priority order changed");
        let ids = self.entries.iter().map(|entry| entry.entry_id.as_str()).collect::<Vec<_>>();
        ensure!(
            ids == ["perlnavigator", "pls", "perl_language_server"],
            "lsp-mode Perl client set/order changed"
        );
        ensure!(
            self.entries.iter().all(|entry| entry.language_id.as_deref() == Some("perl")),
            "lsp-mode Perl clients must retain the Perl activation language"
        );
        Ok(())
    }
}

impl RegistrationEntry {
    fn validate(&self) -> Result<()> {
        ensure!(is_token(&self.entry_id), "entry_id must be a stable token");
        ensure!(!self.modes.is_empty(), "registration entry must retain its modes");
        ensure!(self.command.first().is_some_and(|program| !program.is_empty()), "registration command must have a program");
        if let Some(server_id) = &self.server_id {
            ensure!(is_token(server_id), "server_id must be a stable token");
        }
        Ok(())
    }
}

pub fn checked_baseline() -> StockDiscoveryBaseline {
    StockDiscoveryBaseline {
        schema_version: SCHEMA_VERSION.to_string(),
        claim_ceiling: CLAIM_CEILING.to_string(),
        observations: vec![
            eglot_observation(
                "released_eglot_gnu_elpa_1_24",
                "1.24",
                SourceState::Released,
                "0d67e76b94e1f0af9fe364aed8aa5db1c494c206",
                "f701a38ab8bd9ad984c58320907cb8a93396ec69",
            ),
            eglot_observation(
                "source_eglot_emacs_f4f249a2",
                "1.24",
                SourceState::UpstreamSource,
                "f4f249a2249a7047ba41a659b8fcdcd7e1caf4e0",
                "f2a9e36989cd90500e66900efe5138e6dee56668",
            ),
            lsp_mode_observation(
                "released_lsp_mode_10_0_0",
                "10.0.0",
                SourceState::Released,
                "913a6c07f163205cb568bc68d7dfe677dbc358ab",
                "ee16b0ca0c999eb9ba6db25a46ec3c1a19330619",
            ),
            lsp_mode_observation(
                "source_lsp_mode_e15b8205",
                "10.0.1",
                SourceState::UpstreamSource,
                "e15b8205cbd0369df40b412909eb3ed3264e96a2",
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
    subject_id: &str,
    client_version: &str,
    source_state: SourceState,
    commit: &str,
    blob: &str,
) -> StockDiscoveryObservation {
    StockDiscoveryObservation {
        subject_id: subject_id.to_string(),
        client_family: ClientFamily::Eglot,
        client_version: client_version.to_string(),
        source_state,
        repository: "https://github.com/emacs-mirror/emacs".to_string(),
        commit: commit.to_string(),
        registration_surface: RegistrationSurface::EglotServerPrograms,
        observed_files: vec![ObservedSourceFile {
            path: "lisp/progmodes/eglot.el".to_string(),
            git_blob_sha1: blob.to_string(),
        }],
        observation_complete: true,
        manual_registration_injected: false,
        perllsp_present: false,
        entries: vec![RegistrationEntry {
            entry_id: "perl_language_server".to_string(),
            modes: vec!["perl-mode".to_string(), "cperl-mode".to_string()],
            language_id: None,
            command: vec![
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
    subject_id: &str,
    client_version: &str,
    source_state: SourceState,
    commit: &str,
    root_blob: &str,
) -> StockDiscoveryObservation {
    StockDiscoveryObservation {
        subject_id: subject_id.to_string(),
        client_family: ClientFamily::LspMode,
        client_version: client_version.to_string(),
        source_state,
        repository: "https://github.com/emacs-lsp/lsp-mode".to_string(),
        commit: commit.to_string(),
        registration_surface: RegistrationSurface::LspModeClientModules,
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
                git_blob_sha1: root_blob.to_string(),
            },
        ],
        observation_complete: true,
        manual_registration_injected: false,
        perllsp_present: false,
        entries: vec![
            RegistrationEntry {
                entry_id: "perlnavigator".to_string(),
                modes: vec!["perl".to_string()],
                language_id: Some("perl".to_string()),
                command: vec!["perlnavigator".to_string(), "--stdio".to_string()],
                server_id: Some("perlnavigator".to_string()),
                priority: Some(0),
            },
            RegistrationEntry {
                entry_id: "pls".to_string(),
                modes: vec!["perl".to_string()],
                language_id: Some("perl".to_string()),
                command: vec!["pls".to_string()],
                server_id: Some("pls".to_string()),
                priority: Some(-1),
            },
            RegistrationEntry {
                entry_id: "perl_language_server".to_string(),
                modes: vec!["perl-mode".to_string(), "cperl-mode".to_string()],
                language_id: Some("perl".to_string()),
                command: vec![
                    "perl".to_string(),
                    "-MPerl::LanguageServer".to_string(),
                    "-e".to_string(),
                    "Perl::LanguageServer::run".to_string(),
                ],
                server_id: Some("perl-language-server".to_string()),
                priority: Some(-2),
            },
        ],
    }
}

fn is_lower_hex(value: &str, length: usize) -> bool {
    value.len() == length && value.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn is_token(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

//! Actual-host execution surface for the hermetic Emacs runner (#7778).
//!
//! The runner substrate itself lives in `xtask/tests/support/emacs_host_runner.rs`
//! (landed with the runner core, pull #8024) and is included here unchanged so
//! the binary command and the contract tests share one implementation instead of
//! forking a second supervisor.  This module owns what the substrate's residual
//! needed: the exact client-subject registry, run-plan construction over the
//! checked tree, fixture materialization, and the first checked consumer of
//! `build_emacs_command`/`HermeticLayout`/`run_owned_process`.
//!
//! The generic process-tree cleanup boundary (owned-process-tree semantics,
//! descendant verification, truncation metadata) is owned by #8734's runner
//! substrate. This module consumes those fail-closed semantics without
//! claiming Emacs, Eglot, lsp-mode, diagnostic, root, or install support.

use anyhow::{Context, Result, bail, ensure};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use walkdir::WalkDir;

#[path = "../tests/support/emacs_host_runner.rs"]
pub mod emacs_host_runner;

use emacs_host_runner::{
    EmacsClientKind, EmacsHostPaths, EmacsHostRunPlan, HermeticLayout, ProcessObservation,
    build_emacs_command, build_receipt, file_sha256, run_owned_process,
};
use xtask::editor_client_compat::{
    CANONICAL_EXPECTATION_SET_ID, CapabilityBasis, CapabilityIdentity, CleanupResult,
    ClientSourceState, DiagnosticMode, DiagnosticsIdentity, EvidenceStage, FailureClass,
    JourneyCell, ObservationResult, PlatformIdentity, PositionEncodingBasis, RegistrationState,
    WorkspaceFixtureIdentity, canonical_expectation_set_digest, fixture_digest,
};

const REPOSITORY: &str = "EffortlessMetrics/perl-lsp-swarm";
const DEFAULT_TIMEOUT_MS: u64 = 180_000;

/// One exact client subject of the Emacs host runner registry.
///
/// A subject is an immutable identity: a new client release or a different
/// host build is a new variant, never a silent edit of an existing row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmacsClientSubject {
    /// Bundled Eglot inside exact Emacs 29.4 (slice 1).
    BundledEglotEmacs294,
    /// Bundled Eglot inside exact Emacs 30.1 (slice 1).
    BundledEglotEmacs301,
    /// Standalone Eglot released as GNU ELPA 1.23 (slice 2).
    ReleasedEglotGnuElpa123,
    /// Released GNU ELPA Eglot 1.24, manifest-bound (#11745).
    ReleasedEglotGnuElpa124,
    /// Upstream-source Eglot pinned to the emacs.git commit
    /// `c1ad9d27207aff96a22d49ae4c6cab35a2619927` (#11745).
    SourceEglotEmacsC1ad9d27,
    /// Released lsp-mode 10.0.0 from MELPA Stable, manifest-bound (#11746).
    ReleasedLspModeMelpaStable1000,
    /// Upstream-source lsp-mode pinned to the emacs-lsp/lsp-mode commit
    /// `6bfc593d7b1bc0dd656f09ffce52cc085ebced05` (#11746).
    SourceLspModeGithub6bfc593,
}

impl EmacsClientSubject {
    /// Parse a CLI subject id. Unknown ids are typed errors listing the
    /// registry, never a fallback to whatever matches loosely.
    pub fn from_id(id: &str) -> Result<Self> {
        match id {
            "bundled_eglot_emacs_29_4" => Ok(Self::BundledEglotEmacs294),
            "bundled_eglot_emacs_30_1" => Ok(Self::BundledEglotEmacs301),
            "released_eglot_gnu_elpa_1_23" => Ok(Self::ReleasedEglotGnuElpa123),
            "released_eglot_gnu_elpa_1_24" => Ok(Self::ReleasedEglotGnuElpa124),
            "source_eglot_emacs_c1ad9d27" => Ok(Self::SourceEglotEmacsC1ad9d27),
            "released_lsp_mode_melpa_stable_10_0_0" => Ok(Self::ReleasedLspModeMelpaStable1000),
            "source_lsp_mode_github_6bfc593" => Ok(Self::SourceLspModeGithub6bfc593),
            _ => bail!(
                "unknown client subject {id}: known subjects are {}",
                Self::known_ids().join(", ")
            ),
        }
    }

    /// Every exact client subject the execution surface can run today.
    pub fn known_ids() -> &'static [&'static str] {
        &[
            "bundled_eglot_emacs_29_4",
            "bundled_eglot_emacs_30_1",
            "released_eglot_gnu_elpa_1_23",
            "released_eglot_gnu_elpa_1_24",
            "source_eglot_emacs_c1ad9d27",
            "released_lsp_mode_melpa_stable_10_0_0",
            "source_lsp_mode_github_6bfc593",
        ]
    }

    pub fn id(self) -> &'static str {
        match self {
            Self::BundledEglotEmacs294 => "bundled_eglot_emacs_29_4",
            Self::BundledEglotEmacs301 => "bundled_eglot_emacs_30_1",
            Self::ReleasedEglotGnuElpa123 => "released_eglot_gnu_elpa_1_23",
            Self::ReleasedEglotGnuElpa124 => "released_eglot_gnu_elpa_1_24",
            Self::SourceEglotEmacsC1ad9d27 => "source_eglot_emacs_c1ad9d27",
            Self::ReleasedLspModeMelpaStable1000 => "released_lsp_mode_melpa_stable_10_0_0",
            Self::SourceLspModeGithub6bfc593 => "source_lsp_mode_github_6bfc593",
        }
    }

    /// The host-build token the subject requires in the probed
    /// `emacs --version` line. A host without this token is a different
    /// subject, not this run.
    pub fn pinned_emacs_version_token(self) -> &'static str {
        match self {
            Self::BundledEglotEmacs294 => "29.4",
            Self::BundledEglotEmacs301
            | Self::ReleasedEglotGnuElpa123
            | Self::ReleasedEglotGnuElpa124
            | Self::SourceEglotEmacsC1ad9d27
            | Self::ReleasedLspModeMelpaStable1000
            | Self::SourceLspModeGithub6bfc593 => "30.1",
        }
    }

    /// The subject pins the exact host build before anything is launched.
    pub fn ensure_pinned_host_version(self, emacs_version: &str) -> Result<()> {
        let token = self.pinned_emacs_version_token();
        ensure!(
            emacs_version.contains(token),
            "Emacs host {emacs_version} does not match the pinned subject {} ({token})",
            self.id()
        );
        Ok(())
    }

    /// The pinned client identity for the subject.  Bundled rows draw every
    /// identity string from the checked subject manifest (#11744): the
    /// manifest is the single declared authority for subject rows, and this
    /// registry only dispatches runner mechanics.  Public so the contract
    /// tests can pin the registry row without a host.
    pub fn client_identity(
        self,
        manifest: &crate::emacs_subject_manifest::SubjectManifest,
        source_sha256: String,
        package_sha256: Option<String>,
    ) -> Result<emacs_host_runner::ClientSubject> {
        match self {
            Self::BundledEglotEmacs294 | Self::BundledEglotEmacs301 => {
                let row = manifest.row_for(self.id()).with_context(|| {
                    format!(
                        "bundled subject {} must be a row of the checked subject manifest",
                        self.id()
                    )
                })?;
                // One bundled identity constructor: the manifest module
                // owns the row-to-subject mapping so the resolver and the
                // registry cannot drift apart.
                Ok(crate::emacs_subject_manifest::runner_client_subject(row, source_sha256, None))
            }
            // The manifest-bound external rows (#11745/#11746) draw every
            // identity field from their checked rows the same way.
            Self::ReleasedEglotGnuElpa124
            | Self::SourceEglotEmacsC1ad9d27
            | Self::ReleasedLspModeMelpaStable1000
            | Self::SourceLspModeGithub6bfc593 => {
                let row = manifest.row_for(self.id()).with_context(|| {
                    format!(
                        "external subject {} must be a row of the checked subject manifest",
                        self.id()
                    )
                })?;
                Ok(crate::emacs_subject_manifest::runner_client_subject(
                    row,
                    source_sha256,
                    package_sha256,
                ))
            }
            // The slice-2 released row predates the subject manifest and
            // keeps its explicit-input identity; the digest-bound package
            // rows (#11745) extend the manifest without changing these
            // landed mechanics.
            Self::ReleasedEglotGnuElpa123 => Ok(emacs_host_runner::ClientSubject {
                client_id: self.id().to_string(),
                kind: EmacsClientKind::ExternalEglot,
                version: "1.23".to_string(),
                source_state: ClientSourceState::Released,
                source_ref: "gnu-elpa-eglot-1.23".to_string(),
                source_sha256,
                package_sha256,
            }),
        }
    }

    /// Checked-in adapter path relative to the repository root. The
    /// external Eglot rows share the external-Eglot adapter: the released
    /// rows arrive with their declared package input and the pinned
    /// upstream-source row rides the same adapter package-free (#8776), so
    /// one adapter services both external source states without a copied
    /// journey. The #11746 lsp-mode rows have no adapter yet: lsp-mode
    /// journey mechanics belong to the lsp-mode lanes, so plan construction
    /// for these subjects fails closed on the missing adapter until that
    /// lane lands one, while subject materialization through the manifest
    /// resolver is complete without it.
    pub fn adapter_relative_path(self) -> &'static str {
        match self {
            Self::BundledEglotEmacs294 | Self::BundledEglotEmacs301 => {
                "scripts/test/emacs-clients/eglot-bundled.el"
            }
            Self::ReleasedEglotGnuElpa123
            | Self::ReleasedEglotGnuElpa124
            | Self::SourceEglotEmacsC1ad9d27 => "scripts/test/emacs-clients/eglot-released.el",
            Self::ReleasedLspModeMelpaStable1000 | Self::SourceLspModeGithub6bfc593 => {
                "scripts/test/emacs-clients/lsp-mode.el"
            }
        }
    }

    /// Checked-in configuration path relative to the repository root.
    pub fn configuration_relative_path(self) -> &'static str {
        match self {
            Self::BundledEglotEmacs294 | Self::BundledEglotEmacs301 => {
                "scripts/test/emacs-clients/eglot-bundled-config.el"
            }
            Self::ReleasedEglotGnuElpa123
            | Self::ReleasedEglotGnuElpa124
            | Self::SourceEglotEmacsC1ad9d27 => {
                "scripts/test/emacs-clients/eglot-released-config.el"
            }
            Self::ReleasedLspModeMelpaStable1000 | Self::SourceLspModeGithub6bfc593 => {
                "scripts/test/emacs-clients/lsp-mode-config.el"
            }
        }
    }

    /// Journey selector token bound into the run plan and receipt.
    pub fn journey_selector(self) -> &'static str {
        match self {
            Self::BundledEglotEmacs294 | Self::BundledEglotEmacs301 => "bundled_eglot_lifecycle.v1",
            Self::ReleasedEglotGnuElpa123 | Self::ReleasedEglotGnuElpa124 => {
                "released_eglot_lifecycle.v1"
            }
            Self::SourceEglotEmacsC1ad9d27 => "source_eglot_lifecycle.v1",
            Self::ReleasedLspModeMelpaStable1000 => "released_lsp_mode_lifecycle.v1",
            Self::SourceLspModeGithub6bfc593 => "source_lsp_mode_lifecycle.v1",
        }
    }

    /// Fixture identity token for the materialized journey fixture.
    pub fn fixture_id(self) -> &'static str {
        match self {
            Self::BundledEglotEmacs294 | Self::BundledEglotEmacs301 => "bundled_eglot_lifecycle_v1",
            Self::ReleasedEglotGnuElpa123 | Self::ReleasedEglotGnuElpa124 => {
                "released_eglot_lifecycle_v1"
            }
            Self::SourceEglotEmacsC1ad9d27 => "source_eglot_lifecycle_v1",
            Self::ReleasedLspModeMelpaStable1000 => "released_lsp_mode_lifecycle_v1",
            Self::SourceLspModeGithub6bfc593 => "source_lsp_mode_lifecycle_v1",
        }
    }

    /// Released subjects carry an exact package identity; bundled subjects
    /// cannot (`validate_client_subject` rejects that combination).
    pub fn requires_client_package(self) -> bool {
        match self {
            Self::BundledEglotEmacs294
            | Self::BundledEglotEmacs301
            | Self::SourceEglotEmacsC1ad9d27
            | Self::SourceLspModeGithub6bfc593 => false,
            Self::ReleasedEglotGnuElpa123
            | Self::ReleasedEglotGnuElpa124
            | Self::ReleasedLspModeMelpaStable1000 => true,
        }
    }

    /// A released subject resolves its client source only through the
    /// declared package inputs, never by searching the host installation.
    pub fn resolves_client_source_from_installation(self) -> bool {
        match self {
            Self::BundledEglotEmacs294 | Self::BundledEglotEmacs301 => true,
            Self::ReleasedEglotGnuElpa123
            | Self::ReleasedEglotGnuElpa124
            | Self::SourceEglotEmacsC1ad9d27
            | Self::ReleasedLspModeMelpaStable1000
            | Self::SourceLspModeGithub6bfc593 => false,
        }
    }

    /// Whether run-plan construction routes this subject through the
    /// checked subject manifest resolver (#11744). Every bundled row and
    /// the manifest-bound external rows (#11745/#11746) do; the slice-2
    /// released row predates the manifest and keeps its landed
    /// explicit-input mechanics until it is superseded.
    pub fn resolves_through_subject_manifest(self) -> bool {
        match self {
            Self::BundledEglotEmacs294
            | Self::BundledEglotEmacs301
            | Self::ReleasedEglotGnuElpa124
            | Self::SourceEglotEmacsC1ad9d27
            | Self::ReleasedLspModeMelpaStable1000
            | Self::SourceLspModeGithub6bfc593 => true,
            Self::ReleasedEglotGnuElpa123 => false,
        }
    }

    /// Whether an actual host run is supported by the current driver
    /// adapters. The external Eglot adapter services the pinned
    /// upstream-source subject package-free (#8776): the declared package
    /// input reaches the adapter exactly for released subjects (the plan
    /// builder enforces that shape before launch), and its absence selects
    /// the upstream-source identity emission on the same shared journey,
    /// so the launch table no longer refuses that row.
    pub fn launches_with_current_driver(self) -> bool {
        match self {
            Self::BundledEglotEmacs294
            | Self::BundledEglotEmacs301
            | Self::ReleasedEglotGnuElpa123
            | Self::ReleasedEglotGnuElpa124
            | Self::SourceEglotEmacsC1ad9d27 => true,
            // No lsp-mode adapter exists yet; plan construction already
            // fails closed on the missing adapter file, and the host-run
            // boundary refuses the launch with the same typed reason.
            Self::ReleasedLspModeMelpaStable1000 | Self::SourceLspModeGithub6bfc593 => false,
        }
    }
}

/// Inputs for one client-subject host run.  Every path must be absolute and
/// exact; the plan builder verifies digests before the host is launched.
pub struct EmacsHostRunInputs {
    pub emacs_executable: PathBuf,
    pub candidate_executable: PathBuf,
    pub client_source: PathBuf,
    pub client_package: Option<PathBuf>,
    pub out_root: PathBuf,
    pub timeout_ms: u64,
}

/// Materialize the bounded journey fixture under `root` and return its
/// digest-identity root.  The fixture is intentionally small: these slices
/// prove the client lifecycle, and semantic expectations stay with the
/// canonical expectation set rather than a journey-local oracle.
pub fn materialize_client_subject_fixture(root: &Path) -> Result<PathBuf> {
    ensure!(root.is_absolute(), "fixture root must be absolute");
    let lib = root.join("lib/My");
    let script = root.join("script");
    fs::create_dir_all(&lib).with_context(|| format!("creating {}", lib.display()))?;
    fs::create_dir_all(&script).with_context(|| format!("creating {}", script.display()))?;
    fs::write(
        lib.join("Thing.pm"),
        "package My::Thing;\nuse strict;\nuse warnings;\nsub sentinel { \"CLIENT_SUBJECT_LIFECYCLE\" }\n1;\n",
    )?;
    fs::write(
        script.join("probe.pl"),
        "use strict;\nuse warnings;\nuse lib '../lib';\nuse My::Thing;\nprint My::Thing::sentinel(), \"\\n\";\n",
    )?;
    Ok(root.to_path_buf())
}

/// The library forms one exact Emacs build can ship for its bundled Eglot.
/// Installed builds commonly load `eglot.elc` while shipping `eglot.el`
/// and/or `eglot.el.gz`; the digest binds whichever form is present, and the
/// bundled-ness proof is the installation-root containment, not the file
/// extension. Preference order is deterministic: `.el`, then `.elc`, then
/// `.el.gz`.
const BUNDLED_LIBRARY_FORMS: [&str; 3] = ["eglot.el", "eglot.elc", "eglot.el.gz"];

/// Typed failure of resolving the bundled client library inside one exact
/// Emacs installation. The producer stays typed (#11744 review): consumers
/// must never reclassify a formatted error string to recover the kind,
/// because a message reword would silently change the rejection's type.
#[derive(Debug)]
pub enum BundledClientResolutionError {
    /// The exact Emacs executable could not be canonicalized into an
    /// installation root. An instrument failure, never a subject verdict.
    ExecutableUnresolvable { path: PathBuf, source: std::io::Error },
    /// No library of any declared form exists inside the installation.
    NoLibrary { installation: PathBuf },
    /// Two libraries of the same form exist: an identity defect of the
    /// build, never a silent choice.
    Ambiguous { form: &'static str, candidates: usize, installation: PathBuf },
}

impl fmt::Display for BundledClientResolutionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ExecutableUnresolvable { path, source } => write!(
                formatter,
                "resolving the exact Emacs executable {}: {source}",
                path.display()
            ),
            Self::NoLibrary { installation } => write!(
                formatter,
                "no bundled Eglot library {:?} found inside the exact Emacs installation {}",
                BUNDLED_LIBRARY_FORMS,
                installation.display()
            ),
            Self::Ambiguous { form, candidates, installation } => write!(
                formatter,
                "ambiguous bundled {form} identity: {candidates} candidate libraries inside {}",
                installation.display()
            ),
        }
    }
}

impl std::error::Error for BundledClientResolutionError {}

/// Resolve the bundled Eglot library inside the exact Emacs installation.
///
/// The executable path is canonicalized first so a symlinked `emacs` (for
/// example `/usr/bin/emacs`) cannot point the search at a foreign tree.
/// Two libraries of the *same* form inside one build is an identity defect
/// and a typed error; different forms of one library are normal shipping
/// and resolved by the fixed preference order.
pub fn resolve_bundled_client_source(
    emacs_executable: &Path,
) -> std::result::Result<PathBuf, BundledClientResolutionError> {
    let canonical = fs::canonicalize(emacs_executable).map_err(|source| {
        BundledClientResolutionError::ExecutableUnresolvable {
            path: emacs_executable.to_path_buf(),
            source,
        }
    })?;
    let Some(bin) = canonical.parent() else {
        return Err(BundledClientResolutionError::ExecutableUnresolvable {
            path: emacs_executable.to_path_buf(),
            source: std::io::Error::other("Emacs executable has no parent directory"),
        });
    };
    let Some(root) = bin.parent() else {
        return Err(BundledClientResolutionError::ExecutableUnresolvable {
            path: emacs_executable.to_path_buf(),
            source: std::io::Error::other("Emacs executable has no installation root"),
        });
    };
    for form in BUNDLED_LIBRARY_FORMS {
        let mut matches: Vec<PathBuf> = WalkDir::new(root)
            .max_depth(7)
            .into_iter()
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.file_type().is_file())
            .filter(|entry| entry.file_name().to_str() == Some(form))
            .map(|entry| entry.into_path())
            .collect();
        matches.sort();
        match matches.len() {
            0 => continue,
            1 => return Ok(matches.remove(0)),
            count => {
                return Err(BundledClientResolutionError::Ambiguous {
                    form,
                    candidates: count,
                    installation: root.to_path_buf(),
                });
            }
        }
    }
    Err(BundledClientResolutionError::NoLibrary { installation: root.to_path_buf() })
}

fn bounded_diagnostic(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    let first = text.lines().find(|line| !line.trim().is_empty()).unwrap_or("").trim();
    first.chars().take(400).collect()
}

/// A reused output directory silently concatenates driver event streams
/// (the driver appends and restarts its sequence), so a retry into the same
/// directory would either fail parsing or misattribute stale artifacts. The
/// runner refuses instead of cleaning: nothing here owns destructive
/// deletion of a caller-supplied path. The stale-receipt law is owned by
/// `crate::editor_host`.
pub fn ensure_fresh_output_root(out_root: &Path) -> Result<()> {
    crate::editor_host::FreshReceiptTarget::refuse_existing(out_root, "output root")
}

/// Extract a standalone 40-hex commit-like token from a version line, if it
/// carries one. Used to bind a candidate's self-reported build revision to
/// the repository commit before the receipt claims that provenance. A longer
/// or shorter hex run is not a commit identity.
pub fn extract_commit_like_token(line: &str) -> Option<String> {
    let bytes = line.as_bytes();
    let mut start = 0;
    while start < bytes.len() {
        if bytes[start].is_ascii_hexdigit() {
            let mut end = start;
            while end < bytes.len() && bytes[end].is_ascii_hexdigit() {
                end += 1;
            }
            if end - start == 40 {
                return Some(line[start..end].to_ascii_lowercase());
            }
            start = end;
        } else {
            start += 1;
        }
    }
    None
}

fn first_output_line(command: &mut Command, label: &str) -> Result<String> {
    let output = command
        .stdin(Stdio::null())
        .output()
        .with_context(|| format!("running {label} identity probe"))?;
    ensure!(
        output.status.success(),
        "{label} identity probe failed with status {}: {}",
        output.status,
        bounded_diagnostic(&output.stderr)
    );
    let text = String::from_utf8_lossy(&output.stdout);
    let line = text.lines().next().unwrap_or_default().trim().to_string();
    ensure!(!line.is_empty(), "{label} identity probe produced no version line");
    Ok(line)
}

/// Current commit identity for the candidate run plan.  Computed from the
/// repository, never from ambient state.
fn candidate_commit_identity(repo_root: &Path) -> Result<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .arg("rev-parse")
        .arg("HEAD")
        .stdin(Stdio::null())
        .output()
        .context("running git rev-parse for the candidate identity")?;
    ensure!(
        output.status.success(),
        "git rev-parse failed for the candidate identity: {}",
        bounded_diagnostic(&output.stderr)
    );
    let sha = String::from_utf8_lossy(&output.stdout).trim().to_string();
    ensure!(
        sha.len() == 40 && sha.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "git rev-parse produced a malformed commit identity"
    );
    Ok(sha.to_lowercase())
}

/// Build the complete run plan for one client subject over the checked
/// tree.  Bundled subjects are digest-validated through the subject
/// manifest resolver (#11744) before the plan exists: a modified, ambient,
/// or cross-generation client file is a typed rejection, never a receipt
/// labeled as the checked subject over unchecked bytes.  Validation
/// (digest verification of every exact input) happens inside
/// `build_emacs_command`, so a returned plan has already proven its file
/// identities.
pub fn build_client_subject_run_plan(
    repo_root: &Path,
    subject: EmacsClientSubject,
    run: &EmacsHostRunInputs,
    commit: &str,
    candidate_version: &str,
    emacs_version: &str,
    subject_manifest: &crate::emacs_subject_manifest::SubjectManifest,
) -> Result<(EmacsHostRunPlan, HermeticLayout)> {
    let package_sha256 = match (&run.client_package, subject.requires_client_package()) {
        (Some(package), true) => Some(file_sha256(package)?),
        (None, true) => bail!(
            "subject {} requires an exact client package file (released packages bind package \
             identity); pass --client-package",
            subject.id()
        ),
        (Some(_), false) => bail!(
            "subject {} cannot carry a separate package identity (bundled source state)",
            subject.id()
        ),
        (None, false) => None,
    };
    // Bundled rows and the manifest-bound external rows (#11745) resolve
    // through the subject manifest resolver so the plan binds exactly the
    // audited library bytes and the exact Emacs build; the resolver's
    // bounded cache entry is written under the run's fresh output root, and
    // a released subject additionally validates its exact package archive
    // bytes before the plan exists. The slice-2 released row predates the
    // subject manifest and keeps its explicit-input identity until it is
    // superseded.
    let (client, emacs_build_sha256) = if subject.resolves_through_subject_manifest() {
        let resolved = crate::emacs_subject_manifest::resolve(
            subject_manifest,
            subject.id(),
            &crate::emacs_subject_manifest::ResolveRequest {
                emacs_executable: &run.emacs_executable,
                client_source: Some(&run.client_source),
                client_package: run.client_package.as_deref(),
                cache_root: &run.out_root.join("subject-input-cache"),
                probed_emacs_version: Some(emacs_version),
            },
        )
        .map_err(|failure| {
            anyhow::anyhow!("subject {} failed manifest resolution: {failure}", subject.id())
        })?;
        (resolved.client, resolved.emacs_build_sha256)
    } else {
        (
            subject.client_identity(
                subject_manifest,
                file_sha256(&run.client_source)?,
                package_sha256,
            )?,
            file_sha256(&run.emacs_executable)?,
        )
    };
    let driver = repo_root.join("scripts/test/emacs-host-driver.el");
    let adapter = repo_root.join(subject.adapter_relative_path());
    let configuration = repo_root.join(subject.configuration_relative_path());
    let fixture_root = materialize_client_subject_fixture(&run.out_root.join("fixture"))?;
    let layout = HermeticLayout::prepare(&run.out_root.join("hermetic"))?;
    let plan = EmacsHostRunPlan {
        identity: emacs_host_runner::EmacsHostRunIdentity {
            schema_version: emacs_host_runner::RUN_PLAN_SCHEMA_VERSION.to_string(),
            stage: EvidenceStage::ExactSourceLocal,
            repository: REPOSITORY.to_string(),
            candidate_sha: commit.to_string(),
            emacs_version: emacs_version.to_string(),
            emacs_build_sha256,
            client,
            driver_sha256: file_sha256(&driver)?,
            adapter_sha256: file_sha256(&adapter)?,
            configuration_sha256: file_sha256(&configuration)?,
            candidate_version: candidate_version.to_string(),
            candidate_build_revision: commit.to_string(),
            candidate_artifact_sha256: file_sha256(&run.candidate_executable)?,
            fixture: WorkspaceFixtureIdentity {
                id: subject.fixture_id().to_string(),
                digest: fixture_digest(&fixture_root)?,
                expectation_set_id: CANONICAL_EXPECTATION_SET_ID.to_string(),
                expectation_set_digest: canonical_expectation_set_digest()?,
            },
            journey_selector: subject.journey_selector().to_string(),
            platform: current_platform()?,
            registration_state: RegistrationState::ManualClientRegistration,
            timeout_ms: if run.timeout_ms == 0 { DEFAULT_TIMEOUT_MS } else { run.timeout_ms },
        },
        paths: EmacsHostPaths {
            emacs_executable: run.emacs_executable.clone(),
            client_source: run.client_source.clone(),
            client_package: run.client_package.clone(),
            driver,
            adapter,
            configuration,
            candidate_executable: run.candidate_executable.clone(),
            fixture_root,
            artifact_root: layout.artifact_directory.clone(),
        },
    };
    Ok((plan, layout))
}

fn current_platform() -> Result<PlatformIdentity> {
    // Plan validation independently rejects unsafe identity tokens, so an
    // inherited OS_VERSION carrying a path-like value fails closed there.
    let os_version = std::env::var("OS_VERSION")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "unreported".to_string());
    Ok(PlatformIdentity {
        os: std::env::consts::OS.to_string(),
        os_version,
        arch: std::env::consts::ARCH.to_string(),
    })
}

/// The typed outcome of one host run.  The receipt is written for every run
/// that reached the process stage, including failed ones; absence of a
/// receipt means the run never launched.
pub struct HostRunOutcome {
    pub receipt_path: PathBuf,
    pub result: ObservationResult,
    pub process_cleanup: CleanupResult,
    pub driver_complete: bool,
}

/// CLI entry: validate the subject id, resolve the client source when the
/// subject allows installation resolution, and execute the run.
pub fn host_run_from_cli(
    repo_root: &Path,
    subject: &str,
    emacs_executable: PathBuf,
    candidate_executable: PathBuf,
    client_source: Option<PathBuf>,
    client_package: Option<PathBuf>,
    out_root: PathBuf,
    timeout_ms: u64,
) -> Result<HostRunOutcome> {
    let subject = EmacsClientSubject::from_id(subject)?;
    // Exact inputs are checked before any installation walk or launch: an
    // unavailable host or candidate is a typed error here, never a skip and
    // never a search through an unrelated directory tree.
    for (label, path) in
        [("Emacs executable", &emacs_executable), ("candidate executable", &candidate_executable)]
    {
        ensure!(path.is_absolute(), "{label} must be an absolute path: {}", path.display());
        ensure!(path.is_file(), "{label} is not a file: {}", path.display());
    }
    ensure!(out_root.is_absolute(), "output root must be an absolute path: {}", out_root.display());
    if let Some(package) = &client_package {
        ensure!(
            package.is_absolute() && package.is_file(),
            "client package must be an absolute file path: {}",
            package.display()
        );
    }
    let client_source = match client_source {
        Some(path) => {
            ensure!(
                path.is_absolute() && path.is_file(),
                "client source must be an absolute file path: {}",
                path.display()
            );
            path
        }
        None => {
            ensure!(
                subject.resolves_client_source_from_installation(),
                "subject {} requires an explicit --client-source (the exact eglot.el from the \
                 released package); it is never searched from the host installation",
                subject.id()
            );
            resolve_bundled_client_source(&emacs_executable)?
        }
    };
    host_run(
        repo_root,
        subject,
        &EmacsHostRunInputs {
            emacs_executable,
            candidate_executable,
            client_source,
            client_package,
            out_root,
            timeout_ms,
        },
    )
}

/// Execute one client-subject actual-host run and write its receipt.
///
/// Missing or unusable inputs are typed errors before launch: an unavailable
/// host is never reported as a green or skipped run.
pub fn host_run(
    repo_root: &Path,
    subject: EmacsClientSubject,
    run: &EmacsHostRunInputs,
) -> Result<HostRunOutcome> {
    // A subject whose materialization is complete but whose driver adapter
    // does not exist yet is refused here, before any launch step: the
    // lsp-mode rows have no adapter, so their launch would die inside the
    // driver instead of refusing at the boundary. The refusal names the
    // boundary; the lsp-mode adapter arrives with its journey lane.
    ensure!(
        subject.launches_with_current_driver(),
        "subject {} has no driver adapter yet: materialization through the subject manifest is          complete, but an actual host run is unsupported until its journey lane lands an          adapter",
        subject.id()
    );
    ensure_fresh_output_root(&run.out_root)?;
    let commit = candidate_commit_identity(repo_root)?;
    let candidate_version =
        first_output_line(Command::new(&run.candidate_executable).arg("--version"), "candidate")?;
    let emacs_version =
        first_output_line(Command::new(&run.emacs_executable).arg("--version"), "Emacs")?;
    subject.ensure_pinned_host_version(&emacs_version)?;
    // When the candidate's own version line carries a build revision, it
    // must agree with the repository commit the run plan is about to claim;
    // otherwise the receipt would assert a provenance it never observed.
    if let Some(reported) = extract_commit_like_token(&candidate_version) {
        ensure!(
            reported == commit,
            "candidate reports build revision {reported} but the repository is at {commit}"
        );
    }
    fs::create_dir_all(&run.out_root)
        .with_context(|| format!("creating output root {}", run.out_root.display()))?;
    // The checked subject manifest (#11744) is the declared identity
    // authority for bundled rows; a missing or invalid manifest fails the
    // run closed rather than falling back to embedded identity strings.
    let subject_manifest = crate::emacs_subject_manifest::SubjectManifest::load(repo_root)?;
    let (plan, layout) = build_client_subject_run_plan(
        repo_root,
        subject,
        run,
        &commit,
        &candidate_version,
        &emacs_version,
        &subject_manifest,
    )?;

    let mut command = build_emacs_command(&plan, &layout)?;
    let observation = run_owned_process(&mut command, &plan, &layout)?;
    let outcome = evaluate_observation(&plan, &observation)?;

    let snapshot = layout.capability_snapshot();
    let capabilities = if snapshot.is_file() {
        CapabilityIdentity {
            initialize_snapshot_sha256: file_sha256(&snapshot)?,
            position_encodings_offered: Vec::new(),
            position_encoding_basis: PositionEncodingBasis::NotProven,
            position_encoding_selected: None,
        }
    } else {
        CapabilityIdentity {
            // Hash of zero bytes: the snapshot is absent, and the
            // limitation below says so.  It never stands in for content.
            initialize_snapshot_sha256: file_sha256_of_empty()?,
            position_encodings_offered: Vec::new(),
            position_encoding_basis: PositionEncodingBasis::NotProven,
            position_encoding_selected: None,
        }
    };
    let mut limitations = vec![
        "substrate lifecycle proof only: client support verdicts belong to #7126/#7721/#7727"
            .to_string(),
        "process-tree cleanup is independently observed by the shared runner; this receipt copies \
         that disposition and does not re-judge it"
            .to_string(),
    ];
    if !snapshot.is_file() {
        limitations.push(
            "initialize capability snapshot absent; its hash is the empty digest".to_string(),
        );
    }
    if !outcome.runtime_digest_match {
        limitations.push(
            "the adapter's runtime client-identity attestation did not match the run plan"
                .to_string(),
        );
    }
    if let Some(observed) = &outcome.runtime_version_mismatch {
        limitations.push(observed.clone());
    }
    if extract_commit_like_token(&plan.identity.candidate_version).is_none() {
        limitations.push(
            "candidate version line carries no build revision; candidate_build_revision is bound \
             to the repository commit and the executable digest only"
                .to_string(),
        );
    }
    let receipt = build_receipt(
        &plan,
        &observation,
        capabilities,
        DiagnosticsIdentity {
            advertised_mode: DiagnosticMode::NotProven,
            observed_messages: Vec::new(),
        },
        outcome_journey(&observation),
        outcome.result,
        outcome.failure_class,
        limitations,
        format!(
            "#7778 {}: actual-host substrate proof, no support claim",
            subject.journey_selector()
        ),
    );
    // Fresh-receipt law (#10894): the receipt is reserved by this run's
    // identity composite, refuses any pre-existing file, and its write refuses
    // to overwrite — a stale prior receipt can never satisfy this run.
    let receipt_path = run.out_root.join("receipt.json");
    let subject_digest = crate::editor_host::sha256_bytes(
        format!(
            "{}\n{}\n{}\n",
            plan.identity.candidate_sha,
            plan.identity.candidate_artifact_sha256,
            plan.identity.driver_sha256
        )
        .as_bytes(),
    )?;
    let receipt_target =
        crate::editor_host::FreshReceiptTarget::reserve(receipt_path.clone(), subject_digest)?;
    receipt_target.write(&serde_json::to_vec_pretty(&receipt)?)?;
    Ok(HostRunOutcome {
        receipt_path,
        result: outcome.result,
        process_cleanup: observation.cleanup,
        driver_complete: observation.driver_complete,
    })
}

pub struct OutcomeJudgment {
    pub result: ObservationResult,
    pub failure_class: Option<FailureClass>,
    pub runtime_digest_match: bool,
    pub runtime_version_mismatch: Option<String>,
}

/// Runtime version-evidence judgment for external client subjects
/// (#8776 review repair): `version` is a required identity field for the
/// released and upstream-source Eglot states, so the version header the
/// adapter read from the loaded file must equal the registry pin byte for
/// byte, and absent evidence is a mismatch — never a silent pass — because
/// the external adapters fail their run on an unreadable header before
/// this judgment is reached. Bundled subjects keep their looser law:
/// installed builds ship compiled/compressed forms whose header can be
/// unreadable, so their identity stays digest-authoritative.
///
/// Public so the adapter contract tests pin the judgment without a host.
pub fn external_version_evidence_mismatch(
    kind: emacs_host_runner::EmacsClientKind,
    source_state: ClientSourceState,
    observed: Option<&str>,
    planned: &str,
) -> Option<String> {
    let external_eglot_state = kind == emacs_host_runner::EmacsClientKind::ExternalEglot
        && matches!(source_state, ClientSourceState::Released | ClientSourceState::UpstreamSource);
    if !external_eglot_state {
        return None;
    }
    match observed {
        Some(observed) if observed != planned => Some(format!(
            "runtime client version {observed} does not match the pinned subject version {planned}"
        )),
        Some(_) => None,
        None => Some("external Eglot client_loaded event carried no version evidence".to_string()),
    }
}

/// Cross-check the adapter's runtime identity attestation (the loaded client
/// library digest, and — for external Eglot subjects, where the version is a
/// required identity field — the observed version header) against the run
/// plan, then judge the run. Public so the contract suite can pin the
/// cleanup-facet law against the Vim evaluator's identical semantics.
pub fn evaluate_observation(
    plan: &EmacsHostRunPlan,
    observation: &ProcessObservation,
) -> Result<OutcomeJudgment> {
    let planned_digest = plan
        .identity
        .client
        .source_sha256
        .strip_prefix("sha256:")
        .unwrap_or(&plan.identity.client.source_sha256)
        .to_string();
    let client_loaded = observation
        .events
        .iter()
        .find(|event| event.kind == emacs_host_runner::DriverEventKind::ClientLoaded);
    let observed_digest =
        client_loaded.and_then(|event| event.details.get("source_sha256")).cloned();
    let runtime_digest_match = match observed_digest {
        Some(observed) => observed == planned_digest,
        None => false,
    };
    let runtime_version_mismatch = external_version_evidence_mismatch(
        plan.identity.client.kind,
        plan.identity.client.source_state,
        client_loaded.and_then(|event| event.details.get("version")).map(String::as_str),
        &plan.identity.client.version,
    );
    let driver_failed = observation
        .events
        .iter()
        .any(|event| event.kind == emacs_host_runner::DriverEventKind::DriverFailed);
    // Even an orderly exit-0 run that leaked the candidate is a failure, not
    // a not-proven — same law as the Vim evaluator (#10894 cleanup facet).
    let leaked = observation.cleanup == CleanupResult::Fail;
    let result = if observation.passed_process_boundary()
        && runtime_digest_match
        && runtime_version_mismatch.is_none()
    {
        ObservationResult::Pass
    } else if driver_failed
        || observation.timed_out
        || leaked
        || observation.status_code.is_some_and(|code| code != 0)
    {
        ObservationResult::Fail
    } else {
        ObservationResult::NotProven
    };
    let failure_class = if driver_failed {
        Some(FailureClass::HostClient)
    } else if observation.cleanup != CleanupResult::Pass {
        // An observed leak (Fail) and an unobserved shutdown (NotProven) are
        // both cleanup-classified; only Fail demotes the overall verdict.
        Some(FailureClass::Cleanup)
    } else if !runtime_digest_match || runtime_version_mismatch.is_some() {
        Some(FailureClass::Environment)
    } else {
        None
    };
    Ok(OutcomeJudgment { result, failure_class, runtime_digest_match, runtime_version_mismatch })
}

fn outcome_journey(observation: &ProcessObservation) -> Vec<JourneyCell> {
    use emacs_host_runner::DriverEventKind;
    let mut cells = Vec::new();
    for (id, kind) in [
        ("client_loaded", DriverEventKind::ClientLoaded),
        ("registration_selected", DriverEventKind::RegistrationSelected),
        ("initialize_observed", DriverEventKind::InitializeObserved),
        ("workspace_ready", DriverEventKind::WorkspaceReady),
        ("buffer_opened", DriverEventKind::BufferOpened),
        ("shutdown_completed", DriverEventKind::ShutdownCompleted),
    ] {
        let observed = observation.events.iter().any(|event| event.kind == kind);
        cells.push(JourneyCell {
            id: id.to_string(),
            capability_basis: CapabilityBasis::NotApplicable,
            observed,
            result: if observed { ObservationResult::Pass } else { ObservationResult::NotProven },
            evidence: vec!["emacs/driver-events.jsonl".to_string()],
            limitation: if observed {
                None
            } else {
                Some("lifecycle barrier never emitted".to_string())
            },
        });
    }
    cells.push(JourneyCell {
        id: "process_boundary".to_string(),
        capability_basis: CapabilityBasis::NotApplicable,
        observed: observation.status_code.is_some(),
        result: if observation.passed_process_boundary() {
            ObservationResult::Pass
        } else if observation.timed_out {
            ObservationResult::Fail
        } else {
            ObservationResult::NotProven
        },
        evidence: vec!["emacs/process-ledger.json".to_string()],
        limitation: Some(
            "cleanup pass requires a driver-complete status-0 host exit and an independently \
             observed empty candidate process set; timeout/force cleanup reaps this-run \
             candidate survivors by pid (never image-wide); an unusable probe is not_proven"
                .to_string(),
        ),
    });
    cells
}

fn file_sha256_of_empty() -> Result<String> {
    let empty = tempfile::NamedTempFile::new()?;
    file_sha256(empty.path())
}

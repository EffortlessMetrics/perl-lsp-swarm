//! Adapter contract tests for the checked Eglot host adapter (#8776, ADP_E
//! consuming the landed #8755 subject denominator).
//!
//! One external Eglot adapter services the released and the pinned
//! upstream-source subject states without a copied journey; this target
//! discriminates that adapter slice against the issue's first falsifiers:
//!
//! 1. wrong subject — the adapter surface resolves its subjects through the
//!    landed `SUBJECT_DENOMINATOR`, never a re-derived subject set
//!    (`adapter_subjects_resolve_through_the_landed_denominator`);
//! 2. same visible Eglot version but wrong loaded file — released bytes
//!    offered as the source subject's input are refused before any plan
//!    exists: identity is the audited digest, never the shared `1.24`
//!    header (`same_version_wrong_file_is_refused_before_the_plan`);
//! 3. copied orchestration — the shared journey (registration, connect,
//!    candidate binding, capability capture, shutdown, evidence exports)
//!    exists exactly once in the adapter; a per-state copy of the journey
//!    fails the counts (`the_external_adapter_keeps_one_shared_journey`);
//! 4. package-free binding — the source run plan reaches the adapter
//!    boundary with no package identity while the released plan still binds
//!    its declared archive
//!    (`source_and_released_plans_reach_the_adapter_boundary`);
//! 5. typed refusals preserved — the lsp-mode rows keep their launch
//!    refusals, and a package input offered to the source subject is
//!    refused as an identity conflict
//!    (`unadapted_lanes_and_foreign_identities_still_refuse`);
//! 6. mutation — an adapter that stopped emitting the loaded-file identity
//!    evidence (literal-byte digest, version header, resolution-equality
//!    proof) or the observed-program candidate binding cannot pass
//!    structurally (`identity_evidence_and_candidate_binding_are_pinned`).
//!
//! No Eglot support verdict, capability normalization, receipt, or
//! host-observation semantics is claimed here: those stay with #7779/#8819,
//! #11361, and #11360. Actual host runs execute on provisioned hosts through
//! `cargo xtask integration emacs host-run`; an unavailable host is never a
//! skipped green.

// Plain #[test] functions assert through the standard panic-on-failure
// idiom; these tests are proof, not production paths. `expect` (not
// `allow`) so Clippy flags the suppression once the idiom moves on.
#![expect(clippy::expect_used, clippy::panic)]

use anyhow::{Context, Result, ensure};
use sha2::{Digest as ShaDigest, Sha256};
use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

use xtask::editor_client_compat::ClientSourceState;
use xtask::emacs_host_run::{self, EmacsClientSubject, build_client_subject_run_plan};
use xtask::emacs_subject_fan_in::SUBJECT_DENOMINATOR;
use xtask::emacs_subject_manifest::{
    ExternalPackageIdentity, MaterializationMethod, ResolveFailure, ResolveRequest,
    SourceTreeIdentity, SubjectClientKind, SubjectManifest, SubjectRejection, SubjectRow, resolve,
};

const ADAPTER: &str = "scripts/test/emacs-clients/eglot-released.el";
const BUNDLED_ADAPTER: &str = "scripts/test/emacs-clients/eglot-bundled.el";
const SOURCE_ID: &str = "source_eglot_emacs_c1ad9d27";
const RELEASED_ID: &str = "released_eglot_gnu_elpa_1_24";
const LEGACY_RELEASED_ID: &str = "released_eglot_gnu_elpa_1_23";

/// Fixture bytes standing in for the audited files. Both carry the SAME
/// `1.24` version header on purpose: the released/source version
/// resemblance is exactly what the adapter slice must refuse to treat as
/// identity.
const RELEASED_EGLOT_BYTES: &[u8] =
    b";; eglot.el --- released GNU ELPA 1.24 client (adapter fixture)\n;; Version: 1.24\n";
const SOURCE_EGLOT_BYTES: &[u8] =
    b";; eglot.el --- emacs.git c1ad9d27 source client (adapter fixture)\n;; Version: 1.24\n";
const RELEASED_ARCHIVE_BYTES: &[u8] = b"adapter-fixture GNU ELPA archive bytes for eglot 1.24";

const RELEASED_SOURCE_COMMIT: &str = "0d67e76b94e1f0af9fe364aed8aa5db1c494c206";
const SOURCE_TREE_COMMIT: &str = "c1ad9d27207aff96a22d49ae4c6cab35a2619927";
const SOURCE_TREE_SHA1: &str = "dc5475f03a6462846d36ade5a68a2e90a2578087";

fn workspace_root() -> Result<PathBuf> {
    Ok(Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .context("xtask must live directly under the workspace root")?
        .to_path_buf())
}

fn read_checked(relative: &str) -> Result<String> {
    let path = workspace_root()?.join(relative);
    fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))
}

fn sha256_of(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let mut identity = String::from("sha256:");
    for byte in hasher.finalize() {
        identity.push_str(&format!("{byte:02x}"));
    }
    identity
}

fn released_row() -> SubjectRow {
    SubjectRow {
        subject_id: RELEASED_ID.to_string(),
        client_kind: SubjectClientKind::ExternalEglot,
        source_state: ClientSourceState::Released,
        emacs_release_tag: RELEASED_SOURCE_COMMIT.to_string(),
        emacs_version_token: "30.1".to_string(),
        client_version_hint: "1.24".to_string(),
        client_source_relative_path: "eglot.el".to_string(),
        client_source_sha256: sha256_of(RELEASED_EGLOT_BYTES),
        materialization: MaterializationMethod::ExplicitInput,
        client_library_forms: vec!["eglot.el".to_string()],
        external_package: Some(ExternalPackageIdentity {
            archive_url: "https://elpa.gnu.org/packages/eglot-fixture.tar".to_string(),
            archive_sha256: sha256_of(RELEASED_ARCHIVE_BYTES),
            attested_source_commit: RELEASED_SOURCE_COMMIT.to_string(),
            package_requires: vec!["emacs 26.3".to_string(), "jsonrpc 1.0.29".to_string()],
            minimum_emacs: "26.3".to_string(),
            checksum_disposition: "gnu_elpa_archive_sha256_at_audit_time".to_string(),
        }),
        source_tree: None,
        digest_audit: xtask::emacs_subject_manifest::DigestAudit {
            gnu_tarball_url: "https://elpa.gnu.org/packages/eglot-fixture.tar".to_string(),
            gnu_tarball_sha256: sha256_of(RELEASED_ARCHIVE_BYTES),
            observed_client_version_header: "1.24".to_string(),
        },
    }
}

fn source_row() -> SubjectRow {
    SubjectRow {
        subject_id: SOURCE_ID.to_string(),
        client_kind: SubjectClientKind::ExternalEglot,
        source_state: ClientSourceState::UpstreamSource,
        emacs_release_tag: SOURCE_TREE_COMMIT.to_string(),
        emacs_version_token: "30.1".to_string(),
        client_version_hint: "1.24".to_string(),
        client_source_relative_path: "lisp/progmodes/eglot.el".to_string(),
        client_source_sha256: sha256_of(SOURCE_EGLOT_BYTES),
        materialization: MaterializationMethod::ExplicitInput,
        client_library_forms: vec!["eglot.el".to_string()],
        external_package: None,
        source_tree: Some(SourceTreeIdentity {
            source_repo_url: "https://github.com/emacs-mirror/emacs".to_string(),
            commit: SOURCE_TREE_COMMIT.to_string(),
            tree_sha1: SOURCE_TREE_SHA1.to_string(),
        }),
        digest_audit: xtask::emacs_subject_manifest::DigestAudit {
            gnu_tarball_url:
                "https://raw.githubusercontent.com/emacs-mirror/emacs/fixture/lisp/progmodes/eglot.el"
                    .to_string(),
            gnu_tarball_sha256: sha256_of(SOURCE_EGLOT_BYTES),
            observed_client_version_header: "1.24".to_string(),
        },
    }
}

fn fixture_manifest() -> SubjectManifest {
    SubjectManifest {
        schema_version: xtask::emacs_subject_manifest::MANIFEST_SCHEMA_VERSION.to_string(),
        subjects: vec![released_row(), source_row()],
    }
}

/// A fake exact Emacs executable: `<root>/bin/emacs`. External subjects
/// never search the installation tree, so no library tree is needed.
fn fixture_emacs(root: &Path) -> Result<PathBuf> {
    let bin = root.join("bin");
    fs::create_dir_all(&bin)?;
    let emacs = bin.join("emacs");
    fs::write(&emacs, b"fake exact emacs 30.1 executable")?;
    Ok(emacs)
}

/// Materialize the exact external input directory a caller extracted from
/// the declared source: the client file is `<dir>/eglot.el`, the released
/// archive `<dir>/eglot-1.24.tar`.
fn fixture_input_dir(root: &Path, client_bytes: &[u8], archive: Option<&[u8]>) -> Result<PathBuf> {
    let dir = root.join("materialized/eglot-exact-input");
    fs::create_dir_all(&dir)?;
    fs::write(dir.join("eglot.el"), client_bytes)?;
    if let Some(bytes) = archive {
        fs::write(dir.join("eglot-1.24.tar"), bytes)?;
    }
    Ok(dir)
}

fn default_request<'a>(
    emacs_executable: &'a Path,
    client_source: Option<&'a Path>,
    client_package: Option<&'a Path>,
    cache_root: &'a Path,
) -> ResolveRequest<'a> {
    ResolveRequest {
        emacs_executable,
        client_source,
        client_package,
        cache_root,
        probed_emacs_version: Some("GNU Emacs 30.1 (fixture)"),
    }
}

fn rejection_of(failure: &ResolveFailure) -> &SubjectRejection {
    match failure {
        ResolveFailure::Rejected(rejection) => rejection,
        ResolveFailure::Instrument(message) => {
            panic!("expected a typed rejection, got an instrument failure: {message}")
        }
    }
}

/// Resolve one external fixture subject and hand its host-run inputs to the
/// real shared plan builder over the checked tree, returning the plan with
/// the builder's own hermetic layout.
fn fixture_run_plan(
    tree: &Path,
    subject: EmacsClientSubject,
    client_bytes: &[u8],
    archive: Option<&[u8]>,
) -> Result<(
    emacs_host_run::emacs_host_runner::EmacsHostRunPlan,
    emacs_host_run::emacs_host_runner::HermeticLayout,
)> {
    let emacs = fixture_emacs(tree)?;
    let input = fixture_input_dir(tree, client_bytes, archive)?;
    let candidate_name = if cfg!(windows) { "perllsp.exe" } else { "perllsp" };
    let candidate = tree.join(candidate_name);
    fs::write(&candidate, b"fake exact perllsp candidate bytes")?;
    let manifest = fixture_manifest();
    let resolved = resolve(
        &manifest,
        subject.id(),
        &default_request(
            &emacs,
            Some(&input.join("eglot.el")),
            archive.map(|_| input.join("eglot-1.24.tar")).as_deref(),
            &tree.join("cache"),
        ),
    )
    .map_err(|failure| anyhow::anyhow!("fixture resolution failed: {failure}"))?;
    let run = resolved.host_run_inputs(&candidate, &tree.join("out"), 0);
    let (plan, layout) = build_client_subject_run_plan(
        &workspace_root()?,
        subject,
        &run,
        &"0".repeat(40),
        "perllsp fake",
        "GNU Emacs 30.1 (fixture)",
        &manifest,
    )?;
    Ok((plan, layout))
}

// ---------------------------------------------------------------------------
// 1. Wrong subject: the adapter surface consumes the landed denominator
// ---------------------------------------------------------------------------

/// The adapter's subject set comes from the landed subject denominator
/// (#8755): every Eglot slot of the denominator dispatches through the
/// registry, binds the external adapter (or the bundled one, for the
/// bundled generations), and the six denominator identities stay distinct
/// — the adapter lane re-derives no subject set of its own.
#[test]
fn adapter_subjects_resolve_through_the_landed_denominator() -> Result<()> {
    let eglot_slots: Vec<_> = SUBJECT_DENOMINATOR
        .iter()
        .filter(|slot| slot.client_kind == SubjectClientKind::ExternalEglot)
        .collect();
    ensure!(eglot_slots.len() == 2, "the denominator binds exactly two external Eglot slots");

    for slot in SUBJECT_DENOMINATOR {
        // Adapter binding is constrained for the Eglot family only: the
        // lsp-mode slots' adapter surface belongs to the lsp-mode lane.
        let is_eglot = matches!(
            slot.client_kind,
            SubjectClientKind::BundledEglot | SubjectClientKind::ExternalEglot
        );
        let subject = EmacsClientSubject::from_id(slot.subject_id)
            .with_context(|| format!("denominator slot {} must dispatch", slot.subject_id))?;
        if is_eglot {
            let adapter = subject.adapter_relative_path();
            ensure!(
                adapter == ADAPTER || adapter == BUNDLED_ADAPTER,
                "an Eglot denominator slot must bind an Eglot adapter: {} binds {adapter}",
                slot.subject_id
            );
            let checked = workspace_root()?.join(adapter);
            ensure!(checked.is_file(), "the bound adapter must be a checked-in file: {adapter}");
            let configuration = subject.configuration_relative_path();
            ensure!(
                workspace_root()?.join(configuration).is_file(),
                "the bound configuration must be a checked-in file: {configuration}"
            );
        }
        ensure!(
            subject.pinned_emacs_version_token() == slot.emacs_version_token,
            "registry token must agree with the denominator slot for {}",
            slot.subject_id
        );
    }

    // Six-identity distinctness carries through the registry the adapter
    // dispatches over: every denominator id is distinct and dispatches.
    let ids: BTreeSet<&str> = SUBJECT_DENOMINATOR.iter().map(|slot| slot.subject_id).collect();
    ensure!(ids.len() == SUBJECT_DENOMINATOR.len(), "denominator ids must be distinct");
    for id in &ids {
        ensure!(EmacsClientSubject::from_id(id).is_ok(), "denominator id {id} must dispatch");
    }

    // The registry's external Eglot surface covers the two denominator
    // slots plus the legacy released row that predates the manifest; the
    // source row and both manifest-bound released rows route through the
    // subject manifest resolver.
    for id in [RELEASED_ID, SOURCE_ID, LEGACY_RELEASED_ID] {
        let subject = EmacsClientSubject::from_id(id)?;
        ensure!(
            subject.adapter_relative_path() == ADAPTER,
            "external Eglot subject {id} must share the external adapter"
        );
    }
    ensure!(EmacsClientSubject::from_id(RELEASED_ID)?.resolves_through_subject_manifest());
    ensure!(EmacsClientSubject::from_id(SOURCE_ID)?.resolves_through_subject_manifest());
    ensure!(!EmacsClientSubject::from_id(LEGACY_RELEASED_ID)?.resolves_through_subject_manifest());
    Ok(())
}

// ---------------------------------------------------------------------------
// 2. Same visible version, wrong loaded file
// ---------------------------------------------------------------------------

/// The released audited bytes carry the same `1.24` header as the source
/// subject's audited bytes. Offered as the source subject's input, they are
/// a typed identity mismatch through the joint resolution — before any
/// plan, adapter, or launch step exists — and no run output state appears.
#[test]
fn same_version_wrong_file_is_refused_before_the_plan() {
    let root = tempfile::tempdir().expect("tempdir");
    let tree = root.path().join("tree");
    let emacs = fixture_emacs(&tree).expect("fixture emacs");
    let input = fixture_input_dir(&tree, RELEASED_EGLOT_BYTES, None).expect("input dir");
    let client_path = input.join("eglot.el");
    let cache = tree.join("cache");
    let failure = resolve(
        &fixture_manifest(),
        SOURCE_ID,
        &default_request(&emacs, Some(&client_path), None, &cache),
    )
    .expect_err("released bytes must not satisfy the source subject despite the shared header");
    match rejection_of(&failure) {
        SubjectRejection::IdentityMismatch { subject_id, reason } => {
            assert_eq!(subject_id, SOURCE_ID);
            assert!(
                reason.contains("does not match the pinned subject digest"),
                "the reason must name the digest identity, not the version: {reason}"
            );
        }
        other => panic!("expected IdentityMismatch, got {other:?}"),
    }
    assert!(!cache.exists(), "a rejected resolution must write no cache state");

    // The same refusal reaches the run-plan boundary: no plan, and no run
    // output directory is created behind the rejection.
    let candidate_name = if cfg!(windows) { "perllsp.exe" } else { "perllsp" };
    let candidate = tree.join(candidate_name);
    fs::write(&candidate, b"fake exact perllsp candidate bytes").expect("candidate file");
    let out_root = tree.join("out");
    let run = emacs_host_run::EmacsHostRunInputs {
        emacs_executable: emacs,
        candidate_executable: candidate,
        client_source: client_path,
        client_package: None,
        out_root: out_root.clone(),
        timeout_ms: 0,
    };
    let error = build_client_subject_run_plan(
        &workspace_root().expect("workspace root"),
        EmacsClientSubject::from_id(SOURCE_ID).expect("registry row"),
        &run,
        &"0".repeat(40),
        "perllsp fake",
        "GNU Emacs 30.1 (fixture)",
        &fixture_manifest(),
    )
    .expect_err("wrong bytes must not produce a source-subject plan");
    assert!(
        error.to_string().contains("failed manifest resolution"),
        "the typed rejection must reach the plan boundary: {error}"
    );
    assert!(!out_root.exists(), "a rejected plan must leave no run output state behind");
}

// ---------------------------------------------------------------------------
// 3. Copied orchestration
// ---------------------------------------------------------------------------

/// The two external source states share one journey: registration,
/// connect, candidate binding, capability capture, shutdown, and evidence
/// export each appear exactly once. A copied per-state journey — the
/// failure mode this adapter exists to avoid — breaks the counts.
#[test]
fn the_external_adapter_keeps_one_shared_journey() -> Result<()> {
    let adapter = read_checked(ADAPTER)?;
    assert_eq!(
        adapter.matches("(defun perl-lsp-test-client-run").count(),
        1,
        "exactly one entrypoint"
    );
    assert_eq!(adapter.matches("(eglot--connect").count(), 1, "exactly one connect");
    assert_eq!(
        adapter.matches("\"registration_selected\"").count(),
        1,
        "exactly one registration barrier"
    );
    assert_eq!(
        adapter.matches("\"initialize_observed\"").count(),
        1,
        "exactly one initialize barrier"
    );
    assert_eq!(adapter.matches("\"workspace_ready\"").count(), 1, "exactly one workspace barrier");
    assert_eq!(adapter.matches("\"shutdown_started\"").count(), 1, "exactly one shutdown start");
    assert_eq!(adapter.matches("(eglot-shutdown server").count(), 1, "exactly one shutdown call");
    assert_eq!(
        adapter.matches("(secure-hash 'sha256").count(),
        1,
        "exactly one digest site: both states read the declared file through the same facts \
         function"
    );
    assert_eq!(
        adapter.matches("(process-command").count(),
        1,
        "exactly one observed-program binding"
    );
    // The identity preamble is the only place the two states diverge: the
    // conditional package requirement and the two client_loaded identity
    // emissions.
    assert_eq!(
        adapter.matches("\"client_loaded\"").count(),
        1,
        "one client_loaded emission site (conditional payload)"
    );
    assert_eq!(
        adapter.matches("(source_state . \"released\")").count(),
        1,
        "exactly one released identity payload"
    );
    assert_eq!(
        adapter.matches("(source_state . \"upstream_source\")").count(),
        1,
        "exactly one upstream-source identity payload"
    );
    // The package requirement itself must be conditional: an unconditional
    // demand would refuse every package-free source run again (the exact
    // pre-adapter failure this slice removes).
    assert!(
        adapter.contains("(when package-file"),
        "the declared-package requirement must be guarded by the input's presence"
    );
    // And the branch order must bind the identities honestly: the released
    // payload is the package-present branch, the upstream-source payload
    // the package-free one.
    let conditional = adapter
        .find("(if package-file")
        .context("the client_loaded payload must be selected by the declared package input")?;
    let released_payload = adapter
        .find("(source_state . \"released\")")
        .context("the released identity payload must exist")?;
    let source_payload = adapter
        .find("(source_state . \"upstream_source\")")
        .context("the upstream-source identity payload must exist")?;
    assert!(
        conditional < released_payload && released_payload < source_payload,
        "released is the package-present branch and upstream_source the package-free branch"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// 4. Package-free binding at the adapter boundary
// ---------------------------------------------------------------------------

/// Both external states round-trip through the real shared plan builder and
/// the built host command: the source plan reaches the adapter with no
/// package identity anywhere (no path, no digest, no environment binding),
/// while the released plan still binds its declared archive. The launch
/// table reflects the earned adapter surface: the source row launches, the
/// lsp-mode rows still refuse.
#[test]
fn source_and_released_plans_reach_the_adapter_boundary() -> Result<()> {
    let root = tempfile::tempdir()?;
    let source_tree = root.path().join("source");
    let (source_plan, source_layout) = fixture_run_plan(
        &source_tree,
        EmacsClientSubject::from_id(SOURCE_ID)?,
        SOURCE_EGLOT_BYTES,
        None,
    )?;
    ensure!(source_plan.identity.client.client_id == SOURCE_ID);
    ensure!(
        source_plan.identity.client.source_state == ClientSourceState::UpstreamSource,
        "the source plan binds the upstream-source state"
    );
    ensure!(
        source_plan.identity.client.package_sha256.is_none(),
        "a source plan carries no package identity"
    );
    ensure!(source_plan.paths.client_package.is_none());
    ensure!(
        source_plan.identity.journey_selector == "source_eglot_lifecycle.v1",
        "the source journey selector must stay distinct from the released one"
    );

    let released_tree = root.path().join("released");
    let (released_plan, released_layout) = fixture_run_plan(
        &released_tree,
        EmacsClientSubject::from_id(RELEASED_ID)?,
        RELEASED_EGLOT_BYTES,
        Some(RELEASED_ARCHIVE_BYTES),
    )?;
    ensure!(
        released_plan.identity.client.package_sha256.is_some(),
        "the released plan keeps its exact package identity"
    );
    ensure!(released_plan.identity.journey_selector == "released_eglot_lifecycle.v1");

    // The built host command discriminates the two states exactly by the
    // declared package input: same driver, same adapter, same
    // configuration; the package environment binding is the discriminator
    // the adapter reads.
    let source_command =
        emacs_host_run::emacs_host_runner::build_emacs_command(&source_plan, &source_layout)?;
    let released_command =
        emacs_host_run::emacs_host_runner::build_emacs_command(&released_plan, &released_layout)?;

    for (label, command) in [("source", &source_command), ("released", &released_command)] {
        let argv: Vec<String> =
            command.get_args().map(|argument| argument.to_string_lossy().into_owned()).collect();
        ensure!(
            argv.iter().any(|argument| argument.ends_with("emacs-host-driver.el")),
            "the {label} run must load the shared driver"
        );
        ensure!(
            argv.iter().any(|argument| argument.ends_with("eglot-released.el")),
            "the {label} run must load the external Eglot adapter"
        );
    }

    let environment = |command: &std::process::Command| {
        command
            .get_envs()
            .filter_map(|(name, value)| value.map(|value| (name.to_owned(), value.to_owned())))
            .collect::<std::collections::BTreeMap<_, _>>()
    };
    let source_environment = environment(&source_command);
    let released_environment = environment(&released_command);
    for (label, variables) in [("source", &source_environment), ("released", &released_environment)]
    {
        let configuration = variables
            .get(OsStr::new("PERL_LSP_EMACS_CONFIGURATION"))
            .with_context(|| format!("the {label} run must bind the checked configuration"))?;
        ensure!(
            configuration.to_string_lossy().ends_with("eglot-released-config.el"),
            "the {label} run must bind the shared external configuration"
        );
    }
    ensure!(
        !source_environment.contains_key(OsStr::new("PERL_LSP_EMACS_CLIENT_PACKAGE")),
        "the source run must reach the adapter package-free: no package environment binding"
    );
    ensure!(
        source_environment.contains_key(OsStr::new("PERL_LSP_EMACS_CLIENT_SOURCE")),
        "the declared client source still reaches the source adapter"
    );
    ensure!(
        released_environment.contains_key(OsStr::new("PERL_LSP_EMACS_CLIENT_PACKAGE")),
        "the released run keeps its declared package binding"
    );

    // The earned launch surface: the source row launches with the current
    // driver; the lsp-mode rows still refuse.
    ensure!(EmacsClientSubject::from_id(SOURCE_ID)?.launches_with_current_driver());
    ensure!(EmacsClientSubject::from_id(RELEASED_ID)?.launches_with_current_driver());
    Ok(())
}

// ---------------------------------------------------------------------------
// 5. Typed refusals preserved
// ---------------------------------------------------------------------------

/// Earning the source launch widens no other lane's boundary: the lsp-mode
/// rows keep their typed launch refusals, and a package input offered to
/// the source subject is refused as an identity conflict before the plan
/// exists — released and source states stay non-interchangeable at the
/// adapter boundary.
#[test]
fn unadapted_lanes_and_foreign_identities_still_refuse() {
    for id in ["released_lsp_mode_melpa_stable_10_0_0", "source_lsp_mode_github_6bfc593"] {
        let subject = EmacsClientSubject::from_id(id).expect("registry row");
        assert!(
            !subject.launches_with_current_driver(),
            "the lsp-mode row {id} must keep its typed launch refusal"
        );
        let run = emacs_host_run::EmacsHostRunInputs {
            emacs_executable: PathBuf::from("/nonexistent/emacs"),
            candidate_executable: PathBuf::from("/nonexistent/perllsp"),
            client_source: PathBuf::from("/nonexistent/client.el"),
            client_package: None,
            out_root: PathBuf::from("/nonexistent/out"),
            timeout_ms: 0,
        };
        let error = emacs_host_run::host_run(Path::new("/nonexistent/repo"), subject, &run)
            .err()
            .expect("an unadapted subject must refuse before any launch step");
        assert!(
            error.to_string().contains("no driver adapter yet"),
            "the refusal must name the missing-adapter boundary for {id}: {error}"
        );
    }

    // The source subject never accepts a package identity: the run-plan
    // builder refuses the foreign input before any resolution or launch.
    let root = tempfile::tempdir().expect("tempdir");
    let tree = root.path().join("tree");
    let _emacs = fixture_emacs(&tree).expect("fixture emacs");
    let input = fixture_input_dir(&tree, SOURCE_EGLOT_BYTES, Some(RELEASED_ARCHIVE_BYTES))
        .expect("input dir");
    let candidate_name = if cfg!(windows) { "perllsp.exe" } else { "perllsp" };
    let candidate = tree.join(candidate_name);
    fs::write(&candidate, b"fake exact perllsp candidate bytes").expect("candidate file");
    let run = emacs_host_run::EmacsHostRunInputs {
        emacs_executable: tree.join("bin/emacs"),
        candidate_executable: candidate,
        client_source: input.join("eglot.el"),
        client_package: Some(input.join("eglot-1.24.tar")),
        out_root: tree.join("out"),
        timeout_ms: 0,
    };
    let error = build_client_subject_run_plan(
        &workspace_root().expect("workspace root"),
        EmacsClientSubject::from_id(SOURCE_ID).expect("registry row"),
        &run,
        &"0".repeat(40),
        "perllsp fake",
        "GNU Emacs 30.1 (fixture)",
        &fixture_manifest(),
    )
    .expect_err("a package input must not produce a source-subject plan");
    assert!(
        error.to_string().contains("cannot carry a separate package identity"),
        "the refusal must name the state conflict: {error}"
    );
}

// ---------------------------------------------------------------------------
// 6. Mutation: identity evidence and candidate binding stay pinned
// ---------------------------------------------------------------------------

/// The runtime identity evidence the judge cross-checks — the literal-byte
/// digest and the version header of the loaded file, the load-path
/// ownership order, the resolution-equality proof, and the observed-program
/// candidate binding — is pinned structurally so an adapter that stopped
/// emitting any of it cannot pass while the runs still go green.
#[test]
fn identity_evidence_and_candidate_binding_are_pinned() -> Result<()> {
    let adapter = read_checked(ADAPTER)?;
    assert!(
        adapter.contains("(insert-file-contents-literally"),
        "digests must be computed over raw bytes"
    );
    assert!(
        adapter.contains("(lm-version"),
        "the loaded file's own version header must be observed"
    );
    assert!(
        adapter.contains("carries no version header"),
        "an unreadable version header must fail an external run"
    );
    let load_path_index = adapter
        .find("(add-to-list 'load-path (file-name-directory library))")
        .context("the adapter must push the declared file's directory onto load-path")?;
    let require_index =
        adapter.find("(require 'eglot)").context("the adapter must require eglot exactly once")?;
    ensure!(
        load_path_index < require_index,
        "the eglot require must come after the declared directory owns load-path"
    );
    ensure!(
        adapter.matches("(require 'eglot)").count() == 1,
        "eglot must be required exactly once so no earlier load can satisfy it"
    );
    assert!(
        adapter.contains("(string-equal (file-truename resolved)\n                            (file-truename library))"),
        "the resolved library must be proven to be the declared file"
    );
    assert!(
        adapter.contains("(process-command"),
        "the observed program of the live server process is the candidate binding"
    );
    assert!(
        adapter.contains("non-candidate server program"),
        "an observed program that differs from the candidate must fail the run"
    );
    // The hermetic registration law carries to both states: the manual
    // candidate row is the whole table, so no ambient entry can answer.
    assert!(adapter.contains("(setq eglot-server-programs"));
    for forbidden in [
        "package-initialize",
        "package-archives",
        "package-install",
        "package-refresh-contents",
        "use-package",
        "add-to-list 'eglot-server-programs",
        "add-to-list #'eglot-server-programs",
    ] {
        assert!(
            !adapter.contains(forbidden),
            "the hermetic external adapter must never touch ambient package state: {forbidden}"
        );
    }
    // The readability guard precedes the file read (review repair): the
    // facts function guards `file-readable-p` before its literal insert,
    // and the entrypoint computes facts only after the package guard — an
    // eager `let*' binding would raise an Emacs-generic read error before
    // the declared typed error could run.
    let facts_defun = adapter
        .find("(defun perl-lsp-test-released-library-facts")
        .context("the facts function must exist")?;
    let facts_region = &adapter[facts_defun..];
    let facts_guard = facts_region
        .find("(unless (file-readable-p library)")
        .context("the facts function must guard the library's readability")?;
    let facts_insert = facts_region
        .find("(insert-file-contents-literally library)")
        .context("the facts function must read the library literally")?;
    assert!(facts_guard < facts_insert, "the readability guard must run before the file is read");
    let package_guard =
        adapter.find("(when package-file").context("the package requirement guard must exist")?;
    let facts_computation = adapter
        .find("(let ((facts (perl-lsp-test-released-library-facts library)))")
        .context("facts must be computed through an explicit guarded binding")?;
    assert!(
        package_guard < facts_computation,
        "facts must be computed after the package guard, never eagerly in the entrypoint's \
         binding list"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// 7. Runtime version-evidence judgment (review repair on the P2 finding)
// ---------------------------------------------------------------------------

/// The runtime version cross-check covers both external Eglot states: a
/// stale audited version hint (planned version disagreeing with the
/// digest-pinned bytes' own header) must surface as a limitation instead of
/// writing a Pass receipt with the wrong planned version, and absent
/// version evidence is a mismatch, never a silent pass. Bundled subjects
/// keep their digest-authoritative law: their installed forms can carry an
/// unreadable header, so no version judgment applies there.
#[test]
fn runtime_version_evidence_is_enforced_for_both_external_states() {
    use emacs_host_run::emacs_host_runner::EmacsClientKind;
    use xtask::emacs_host_run::external_version_evidence_mismatch;

    // Upstream-source: the state this slice made launchable.
    let mismatch = external_version_evidence_mismatch(
        EmacsClientKind::ExternalEglot,
        ClientSourceState::UpstreamSource,
        Some("1.25"),
        "1.24",
    )
    .expect("a stale audited hint must surface as runtime evidence mismatch");
    assert!(
        mismatch.contains("runtime client version 1.25")
            && mismatch.contains("pinned subject version 1.24"),
        "the mismatch must name both versions: {mismatch}"
    );
    assert_eq!(
        external_version_evidence_mismatch(
            EmacsClientKind::ExternalEglot,
            ClientSourceState::UpstreamSource,
            Some("1.24"),
            "1.24",
        ),
        None,
        "matching evidence passes"
    );
    let absent = external_version_evidence_mismatch(
        EmacsClientKind::ExternalEglot,
        ClientSourceState::UpstreamSource,
        None,
        "1.24",
    )
    .expect("absent version evidence must be a mismatch for external states");
    assert!(
        absent.contains("no version evidence"),
        "the mismatch must name the missing evidence: {absent}"
    );

    // Released: the pre-existing law, unchanged in strength.
    assert!(
        external_version_evidence_mismatch(
            EmacsClientKind::ExternalEglot,
            ClientSourceState::Released,
            Some("1.23"),
            "1.24",
        )
        .is_some()
    );
    assert_eq!(
        external_version_evidence_mismatch(
            EmacsClientKind::ExternalEglot,
            ClientSourceState::Released,
            Some("1.24"),
            "1.24",
        ),
        None
    );

    // Bundled: digest-authoritative, no version judgment — even an
    // unreadable ("version_unavailable") or disagreeing header does not
    // fail a bundled run through this seam.
    assert_eq!(
        external_version_evidence_mismatch(
            EmacsClientKind::BundledEglot,
            ClientSourceState::Bundled,
            Some("version_unavailable"),
            "1.17.30",
        ),
        None
    );
    assert_eq!(
        external_version_evidence_mismatch(
            EmacsClientKind::BundledEglot,
            ClientSourceState::Bundled,
            None,
            "1.17.30",
        ),
        None
    );
}

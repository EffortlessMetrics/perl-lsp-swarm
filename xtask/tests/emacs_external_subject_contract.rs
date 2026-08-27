//! Discriminating contract tests for the external Emacs client-subject rows
//! of #11745 (released GNU ELPA Eglot and pinned upstream-source Eglot).
//!
//! The first seven tests are the issue's first falsifiers, in the order the
//! body lists them:
//!
//! 1. released subject omits archive/package identity -> reject;
//! 2. source subject uses a floating ref -> reject;
//! 3. source tree with a matching version header is labeled released ->
//!    reject;
//! 4. package/archive digest changes while the version stays equal ->
//!    reject;
//! 5. dependency/Emacs compatibility drift is ignored -> reject;
//! 6. ambient ELPA/native-comp copy satisfies the exact subject -> reject;
//! 7. intended manifest identity is reported as observed runtime identity
//!    -> reject.
//!
//! The remaining tests pin the positive contract: both subjects materialize
//! through the #11744 resolver binding their exact identities, released and
//! source states stay non-interchangeable, cache keys bind the archive
//! digest rather than the version alone, the checked manifest pins the
//! audited digests, the registry agrees with the manifest, and both rows
//! round-trip through the real shared run-plan boundary. No semantic
//! support, journey, or upstream-acceptance claim is proven here.

// Plain #[test] functions assert through the standard panic-on-failure
// idiom; these tests are proof, not production paths. `expect` (not
// `allow`) so Clippy flags the suppression once the idiom moves on.
#![expect(clippy::expect_used, clippy::panic)]

use anyhow::{Context, Result, ensure};
use sha2::{Digest as ShaDigest, Sha256};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use xtask::editor_client_compat::ClientSourceState;
use xtask::emacs_host_run::{self, EmacsClientSubject, build_client_subject_run_plan};
use xtask::emacs_subject_manifest::{
    CACHE_ENTRY_FILE, ExternalPackageIdentity, MaterializationMethod, ResolveFailure,
    ResolveRequest, SourceTreeIdentity, SubjectClientKind, SubjectManifest, SubjectRejection,
    SubjectRow, resolve,
};

fn workspace_root() -> Result<PathBuf> {
    Ok(Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .context("xtask must live directly under the workspace root")?
        .to_path_buf())
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

/// Fixture bytes standing in for the audited released archive's `eglot.el`
/// and the audited upstream-tree `eglot.el`. Both carry the SAME version
/// header on purpose: the source/release version resemblance is exactly
/// what the resolver must refuse to treat as identity.
const RELEASED_EGLOT_BYTES: &[u8] =
    b";; eglot.el --- released GNU ELPA 1.24 client (fixture)\n;; Version: 1.24\n";
const SOURCE_EGLOT_BYTES: &[u8] =
    b";; eglot.el --- upstream source client (fixture)\n;; Version: 1.24\n";
const RELEASED_ARCHIVE_BYTES: &[u8] = b"fixture GNU ELPA archive bytes for eglot 1.24";
const REPACKED_ARCHIVE_BYTES: &[u8] =
    b"repacked archive bytes that still claim eglot 1.24 (different bytes)";

const RELEASED_SOURCE_COMMIT: &str = "0d67e76b94e1f0af9fe364aed8aa5db1c494c206";
const SOURCE_TREE_COMMIT: &str = "c1ad9d27207aff96a22d49ae4c6cab35a2619927";
const SOURCE_TREE_SHA1: &str = "dc5475f03a6462846d36ade5a68a2e90a2578087";

fn fixture_external_package(
    archive_sha256: String,
    minimum_emacs: &str,
) -> ExternalPackageIdentity {
    ExternalPackageIdentity {
        archive_url: "https://elpa.gnu.org/packages/eglot-fixture.tar".to_string(),
        archive_sha256,
        attested_source_commit: RELEASED_SOURCE_COMMIT.to_string(),
        package_requires: vec![
            format!("emacs {minimum_emacs}"),
            "eldoc 1.16.0".to_string(),
            "jsonrpc 1.0.29".to_string(),
        ],
        minimum_emacs: minimum_emacs.to_string(),
        checksum_disposition: "gnu_elpa_archive_sha256_at_audit_time".to_string(),
    }
}

fn released_row(
    subject_id: &str,
    client_sha256: String,
    archive_sha256: String,
    minimum_emacs: &str,
) -> SubjectRow {
    SubjectRow {
        subject_id: subject_id.to_string(),
        client_kind: SubjectClientKind::ExternalEglot,
        source_state: ClientSourceState::Released,
        emacs_release_tag: RELEASED_SOURCE_COMMIT.to_string(),
        emacs_version_token: "30.1".to_string(),
        client_version_hint: "1.24".to_string(),
        client_source_relative_path: "eglot.el".to_string(),
        client_source_sha256: client_sha256,
        materialization: MaterializationMethod::ExplicitInput,
        client_library_forms: vec!["eglot.el".to_string()],
        external_package: Some(fixture_external_package(archive_sha256.clone(), minimum_emacs)),
        source_tree: None,
        digest_audit: xtask::emacs_subject_manifest::DigestAudit {
            gnu_tarball_url: "https://elpa.gnu.org/packages/eglot-fixture.tar".to_string(),
            gnu_tarball_sha256: archive_sha256,
            observed_client_version_header: "1.24".to_string(),
        },
    }
}

fn source_row(subject_id: &str, client_sha256: String) -> SubjectRow {
    SubjectRow {
        subject_id: subject_id.to_string(),
        client_kind: SubjectClientKind::ExternalEglot,
        source_state: ClientSourceState::UpstreamSource,
        emacs_release_tag: SOURCE_TREE_COMMIT.to_string(),
        emacs_version_token: "30.1".to_string(),
        client_version_hint: "1.24".to_string(),
        client_source_relative_path: "lisp/progmodes/eglot.el".to_string(),
        client_source_sha256: client_sha256.clone(),
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
            gnu_tarball_sha256: client_sha256,
            observed_client_version_header: "1.24".to_string(),
        },
    }
}

fn fixture_manifest() -> SubjectManifest {
    SubjectManifest {
        schema_version: xtask::emacs_subject_manifest::MANIFEST_SCHEMA_VERSION.to_string(),
        subjects: vec![
            released_row(
                "released_eglot_gnu_elpa_1_24",
                sha256_of(RELEASED_EGLOT_BYTES),
                sha256_of(RELEASED_ARCHIVE_BYTES),
                "26.3",
            ),
            source_row("source_eglot_emacs_c1ad9d27", sha256_of(SOURCE_EGLOT_BYTES)),
        ],
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

/// Materialize a bounded external input directory holding the exact client
/// file (and optionally the exact archive) a caller extracted from the
/// declared source. Returns the directory path; the client file is always
/// `<dir>/eglot.el` and the archive `<dir>/eglot-1.24.tar`.
fn fixture_input_dir(
    root: &Path,
    client_bytes: &[u8],
    archive_bytes: Option<&[u8]>,
) -> Result<PathBuf> {
    let dir = root.join("materialized/eglot-exact-input");
    fs::create_dir_all(&dir)?;
    fs::write(dir.join("eglot.el"), client_bytes)?;
    if let Some(bytes) = archive_bytes {
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

// ---------------------------------------------------------------------------
// First falsifiers (issue order)
// ---------------------------------------------------------------------------

/// Falsifier 1: a released subject without its exact package/archive
/// identity is unavailable, and a released row that declares none is an
/// invalid manifest — a version string alone is not a released package.
#[test]
fn released_subject_without_package_identity_is_rejected() {
    let root = tempfile::tempdir().expect("tempdir");
    let emacs = fixture_emacs(root.path()).expect("fixture emacs");
    let client = root.path().join("input/eglot.el");
    fs::create_dir_all(client.parent().expect("parent")).expect("input dir");
    fs::write(&client, RELEASED_EGLOT_BYTES).expect("client file");
    let manifest = fixture_manifest();
    let failure = resolve(
        &manifest,
        "released_eglot_gnu_elpa_1_24",
        &default_request(&emacs, Some(&client), None, &root.path().join("cache")),
    )
    .expect_err("a released subject requires the exact package archive input");
    match rejection_of(&failure) {
        SubjectRejection::UnavailableSubject { subject_id, reason } => {
            assert_eq!(subject_id, "released_eglot_gnu_elpa_1_24");
            assert!(
                reason.contains("package archive"),
                "the reason must name the missing package/archive identity: {reason}"
            );
        }
        other => panic!("expected UnavailableSubject, got {other:?}"),
    }

    let mut no_package = fixture_manifest();
    no_package.subjects[0].external_package = None;
    let rejection =
        no_package.validate().expect_err("a released row must declare package identity");
    match &rejection {
        SubjectRejection::InvalidManifest { reason } => assert!(
            reason.contains("package/archive identity"),
            "the reason must name the released-identity rule: {reason}"
        ),
        other => panic!("expected InvalidManifest, got {other:?}"),
    }
}

/// Falsifier 2: floating refs and mutable aliases never pin source bytes.
#[test]
fn floating_source_refs_are_rejected() {
    for floating in ["main", "HEAD", "trunk", "latest"] {
        let mut manifest = fixture_manifest();
        manifest.subjects[1].emacs_release_tag = floating.to_string();
        let rejection =
            manifest.validate().expect_err("a floating source ref must fail validation");
        match &rejection {
            SubjectRejection::InvalidManifest { reason } => assert!(
                reason.contains("floating") || reason.contains("commit pin"),
                "the reason must name the ref rule: {reason}"
            ),
            other => panic!("expected InvalidManifest, got {other:?}"),
        }
    }
    // A mutable alias that reaches the tree-block rules must also refuse:
    // a tag disagreeing with the pinned commit is not an exact tree.
    let mut aliased = fixture_manifest();
    aliased.subjects[1].emacs_release_tag = SOURCE_TREE_COMMIT.to_string();
    aliased.subjects[1].source_tree = Some(SourceTreeIdentity {
        source_repo_url: "https://github.com/emacs-mirror/emacs".to_string(),
        commit: "0f4aa7b3d4b3b7f2f83a2b7a694af4a3d2c1b0a9".to_string(),
        tree_sha1: SOURCE_TREE_SHA1.to_string(),
    });
    let rejection = aliased.validate().expect_err("tag/tree disagreement must fail validation");
    match &rejection {
        SubjectRejection::InvalidManifest { reason } => assert!(
            reason.contains("commit pin"),
            "the reason must name the tag/tree coherence rule: {reason}"
        ),
        other => panic!("expected InvalidManifest, got {other:?}"),
    }
}

/// Falsifier 3: a source tree whose version header matches the released
/// version is still not the released package, and vice versa — the bytes
/// bind the subject, not the header resemblance.
#[test]
fn source_tree_with_release_version_header_is_not_the_released_subject() {
    let root = tempfile::tempdir().expect("tempdir");
    let emacs = fixture_emacs(root.path()).expect("fixture emacs");
    let input = fixture_input_dir(root.path(), SOURCE_EGLOT_BYTES, Some(RELEASED_ARCHIVE_BYTES))
        .expect("input dir");
    let client = input.join("eglot.el");
    let archive = input.join("eglot-1.24.tar");
    let manifest = fixture_manifest();
    let failure = resolve(
        &manifest,
        "released_eglot_gnu_elpa_1_24",
        &default_request(&emacs, Some(&client), Some(&archive), &root.path().join("cache")),
    )
    .expect_err("source-tree bytes must not satisfy the released subject");
    match rejection_of(&failure) {
        SubjectRejection::IdentityMismatch { subject_id, reason } => {
            assert_eq!(subject_id, "released_eglot_gnu_elpa_1_24");
            assert!(
                reason.contains(&sha256_of(SOURCE_EGLOT_BYTES))
                    && reason.contains(&sha256_of(RELEASED_EGLOT_BYTES)),
                "the reason must name the observed and pinned digests: {reason}"
            );
        }
        other => panic!("expected IdentityMismatch, got {other:?}"),
    }

    let released_input =
        fixture_input_dir(root.path(), RELEASED_EGLOT_BYTES, None).expect("released input");
    let released_client = released_input.join("eglot.el");
    let failure = resolve(
        &manifest,
        "source_eglot_emacs_c1ad9d27",
        &default_request(&emacs, Some(&released_client), None, &root.path().join("cache-2")),
    )
    .expect_err("released bytes must not satisfy the source subject");
    match rejection_of(&failure) {
        SubjectRejection::IdentityMismatch { subject_id, .. } => {
            assert_eq!(subject_id, "source_eglot_emacs_c1ad9d27");
        }
        other => panic!("expected IdentityMismatch, got {other:?}"),
    }

    // Labeling a source row as released without package identity is an
    // invalid manifest, exactly the source-as-release labeling the issue
    // forbids.
    let mut mislabeled = fixture_manifest();
    mislabeled.subjects[1].source_state = ClientSourceState::Released;
    mislabeled.subjects[1].external_package = None;
    let rejection = mislabeled.validate().expect_err("a mislabeled source row must not validate");
    match &rejection {
        SubjectRejection::InvalidManifest { reason } => assert!(
            reason.contains("package/archive identity"),
            "the reason must name the released-identity rule: {reason}"
        ),
        other => panic!("expected InvalidManifest, got {other:?}"),
    }
}

/// Falsifier 4: the same version over different package bytes is a
/// different subject; the archive digest is validated before anything is
/// loadable.
#[test]
fn same_version_different_archive_bytes_is_rejected() {
    let root = tempfile::tempdir().expect("tempdir");
    let emacs = fixture_emacs(root.path()).expect("fixture emacs");
    let input = fixture_input_dir(root.path(), RELEASED_EGLOT_BYTES, Some(REPACKED_ARCHIVE_BYTES))
        .expect("input dir");
    let client = input.join("eglot.el");
    let archive = input.join("eglot-1.24.tar");
    let manifest = fixture_manifest();
    let failure = resolve(
        &manifest,
        "released_eglot_gnu_elpa_1_24",
        &default_request(&emacs, Some(&client), Some(&archive), &root.path().join("cache")),
    )
    .expect_err("repacked archive bytes must not satisfy the released subject");
    match rejection_of(&failure) {
        SubjectRejection::IdentityMismatch { subject_id, reason } => {
            assert_eq!(subject_id, "released_eglot_gnu_elpa_1_24");
            assert!(
                reason.contains(&sha256_of(REPACKED_ARCHIVE_BYTES))
                    && reason.contains(&sha256_of(RELEASED_ARCHIVE_BYTES)),
                "the reason must name the observed and declared archive digests: {reason}"
            );
        }
        other => panic!("expected IdentityMismatch, got {other:?}"),
    }
    assert!(
        !root.path().join("cache").exists(),
        "a rejected resolution must not write cache state"
    );
}

/// Falsifier 5: dependency and Emacs-compatibility drift is refused at
/// validation and resolution, never silently compatible.
#[test]
fn emacs_compatibility_drift_is_rejected() {
    // A row whose pinned host token sits below its declared minimum Emacs
    // is incompatible with its own dependency floor.
    let mut drifted = fixture_manifest();
    drifted.subjects[0].emacs_version_token = "26.1".to_string();
    drifted.subjects[0].external_package =
        Some(fixture_external_package(sha256_of(RELEASED_ARCHIVE_BYTES), "26.3"));
    let rejection = drifted.validate().expect_err("host-token drift must fail validation");
    match &rejection {
        SubjectRejection::InvalidManifest { reason } => assert!(
            reason.contains("below the declared minimum Emacs"),
            "the reason must name the compatibility floor: {reason}"
        ),
        other => panic!("expected InvalidManifest, got {other:?}"),
    }

    // The declared minimum must equal the emacs dependency entry.
    let mut mismatched_requires = fixture_manifest();
    mismatched_requires.subjects[0].external_package = Some(ExternalPackageIdentity {
        archive_url: "https://elpa.gnu.org/packages/eglot-fixture.tar".to_string(),
        archive_sha256: sha256_of(RELEASED_ARCHIVE_BYTES),
        attested_source_commit: RELEASED_SOURCE_COMMIT.to_string(),
        package_requires: vec!["emacs 27.1".to_string(), "eldoc 1.16.0".to_string()],
        minimum_emacs: "26.3".to_string(),
        checksum_disposition: "gnu_elpa_archive_sha256_at_audit_time".to_string(),
    });
    let rejection =
        mismatched_requires.validate().expect_err("requires/minimum drift must fail validation");
    match &rejection {
        SubjectRejection::InvalidManifest { reason } => assert!(
            reason.contains("minimum_emacs"),
            "the reason must name the requires/minimum coherence rule: {reason}"
        ),
        other => panic!("expected InvalidManifest, got {other:?}"),
    }

    // A probed host without the pinned token stays an explicit
    // incompatibility for external subjects too.
    let root = tempfile::tempdir().expect("tempdir");
    let emacs = fixture_emacs(root.path()).expect("fixture emacs");
    let input = fixture_input_dir(root.path(), RELEASED_EGLOT_BYTES, Some(RELEASED_ARCHIVE_BYTES))
        .expect("input dir");
    let request = ResolveRequest {
        emacs_executable: &emacs,
        client_source: Some(&input.join("eglot.el")),
        client_package: Some(&input.join("eglot-1.24.tar")),
        cache_root: &root.path().join("cache"),
        probed_emacs_version: Some("GNU Emacs 29.4 (fixture)"),
    };
    let failure = resolve(&fixture_manifest(), "released_eglot_gnu_elpa_1_24", &request)
        .expect_err("a host without the pinned token is incompatible");
    match rejection_of(&failure) {
        SubjectRejection::IncompatibleSubject { subject_id, reason } => {
            assert_eq!(subject_id, "released_eglot_gnu_elpa_1_24");
            assert!(
                reason.contains("29.4") && reason.contains("30.1"),
                "the reason must name observed and pinned tokens: {reason}"
            );
        }
        other => panic!("expected IncompatibleSubject, got {other:?}"),
    }
}

/// Falsifier 6: an ambient ELPA-layout copy — even with byte-identical
/// content and the correct archive — cannot satisfy an exact subject.
#[test]
fn ambient_elpa_copy_cannot_satisfy_external_subjects() {
    let root = tempfile::tempdir().expect("tempdir");
    let emacs = fixture_emacs(root.path()).expect("fixture emacs");
    let ambient = root.path().join("home/.emacs.d/elpa/eglot-1.24/eglot.el");
    fs::create_dir_all(ambient.parent().expect("parent")).expect("elpa dir");
    fs::write(&ambient, RELEASED_EGLOT_BYTES).expect("ambient copy");
    let archive = root.path().join("input/eglot-1.24.tar");
    fs::create_dir_all(archive.parent().expect("parent")).expect("input dir");
    fs::write(&archive, RELEASED_ARCHIVE_BYTES).expect("archive");
    let manifest = fixture_manifest();
    let failure = resolve(
        &manifest,
        "released_eglot_gnu_elpa_1_24",
        &default_request(&emacs, Some(&ambient), Some(&archive), &root.path().join("cache")),
    )
    .expect_err("an ambient ELPA copy must not satisfy the released subject");
    match rejection_of(&failure) {
        SubjectRejection::AmbientStateRejected { subject_id, reason } => {
            assert_eq!(subject_id, "released_eglot_gnu_elpa_1_24");
            assert!(
                reason.contains("elpa"),
                "the reason must name the package-layout marker: {reason}"
            );
        }
        other => panic!("expected AmbientStateRejected, got {other:?}"),
    }

    let ambient_source = root.path().join("home/.emacs.d/elpa/eglot-1.24/eglot-source.el");
    fs::write(&ambient_source, SOURCE_EGLOT_BYTES).expect("ambient source copy");
    let failure = resolve(
        &manifest,
        "source_eglot_emacs_c1ad9d27",
        &default_request(&emacs, Some(&ambient_source), None, &root.path().join("cache-2")),
    )
    .expect_err("an ambient ELPA copy must not satisfy the source subject");
    match rejection_of(&failure) {
        SubjectRejection::AmbientStateRejected { subject_id, reason } => {
            assert_eq!(subject_id, "source_eglot_emacs_c1ad9d27");
            assert!(reason.contains("elpa"), "the reason must name the marker: {reason}");
        }
        other => panic!("expected AmbientStateRejected, got {other:?}"),
    }
}

/// Falsifier 7: cache records carry intended input only; a doctored record
/// claiming a runtime-load observation fails closed.
#[test]
fn manifest_identity_is_never_runtime_identity() {
    let root = tempfile::tempdir().expect("tempdir");
    let emacs = fixture_emacs(root.path()).expect("fixture emacs");
    let input = fixture_input_dir(root.path(), RELEASED_EGLOT_BYTES, Some(RELEASED_ARCHIVE_BYTES))
        .expect("input dir");
    let manifest = fixture_manifest();
    let resolved = resolve(
        &manifest,
        "released_eglot_gnu_elpa_1_24",
        &default_request(
            &emacs,
            Some(&input.join("eglot.el")),
            Some(&input.join("eglot-1.24.tar")),
            &root.path().join("cache"),
        ),
    )
    .expect("the released subject resolves");
    let record_path = resolved.cache_entry.join(CACHE_ENTRY_FILE);
    let record = fs::read_to_string(&record_path).expect("cache record");
    assert!(
        record.contains("intended input"),
        "the record must carry the intended-input boundary: {record}"
    );
    assert!(
        !record.contains("loaded") && !record.contains("runtime_pass"),
        "the record vocabulary must not claim any runtime load: {record}"
    );

    let doctored: serde_json::Value = serde_json::from_str(&record).expect("record json");
    let mut object = doctored.as_object().expect("record object").clone();
    object.insert(
        "runtime_loaded_sha256".to_string(),
        serde_json::json!(
            "sha256:0000000000000000000000000000000000000000000000000000000000000000"
        ),
    );
    fs::write(&record_path, serde_json::to_vec_pretty(&object).expect("serialize"))
        .expect("doctored record");
    let failure = resolve(
        &manifest,
        "released_eglot_gnu_elpa_1_24",
        &default_request(
            &emacs,
            Some(&input.join("eglot.el")),
            Some(&input.join("eglot-1.24.tar")),
            &root.path().join("cache"),
        ),
    )
    .expect_err("a doctored record claiming runtime observation must fail closed");
    match &failure {
        ResolveFailure::Instrument(message) => assert!(
            message.contains("runtime_loaded_sha256") || message.contains("refuses to parse"),
            "the doctored record must fail as a parse refusal naming the rejected claim: {message}"
        ),
        ResolveFailure::Rejected(rejection) => {
            panic!("expected an instrument failure, got a rejection: {rejection}")
        }
    }
}

// ---------------------------------------------------------------------------
// Positive contract
// ---------------------------------------------------------------------------

/// The released subject resolves binding the exact archive and loaded-file
/// identities, the exact host build, and a deterministic cache entry.
#[test]
fn released_subject_binds_exact_archive_and_file_identity() -> Result<()> {
    let root = tempfile::tempdir()?;
    let emacs = fixture_emacs(root.path())?;
    let input = fixture_input_dir(root.path(), RELEASED_EGLOT_BYTES, Some(RELEASED_ARCHIVE_BYTES))?;
    let client_path = input.join("eglot.el");
    let archive_path = input.join("eglot-1.24.tar");
    let manifest = fixture_manifest();
    let cache = root.path().join("cache");
    let resolved = resolve(
        &manifest,
        "released_eglot_gnu_elpa_1_24",
        &default_request(&emacs, Some(&client_path), Some(&archive_path), &cache),
    )?;
    ensure!(!resolved.reused_cache, "a fresh cache must materialize");
    ensure!(
        resolved.client.client_id == "released_eglot_gnu_elpa_1_24"
            && resolved.client.kind
                == emacs_host_run::emacs_host_runner::EmacsClientKind::ExternalEglot
            && resolved.client.source_state == ClientSourceState::Released
    );
    ensure!(
        resolved.client.source_sha256 == sha256_of(RELEASED_EGLOT_BYTES),
        "the loaded-file digest must equal the declared digest"
    );
    ensure!(
        resolved.client.package_sha256.as_deref()
            == Some(sha256_of(RELEASED_ARCHIVE_BYTES).as_str()),
        "the validated archive digest is the package identity"
    );
    ensure!(
        resolved.client.source_ref == RELEASED_SOURCE_COMMIT,
        "the released subject's source ref is the archive-attested commit"
    );
    ensure!(
        resolved.emacs_build_sha256 == sha256_of(b"fake exact emacs 30.1 executable"),
        "the exact host build digest is bound at resolution"
    );
    ensure!(resolved.client_package.as_deref() == Some(archive_path.as_path()));
    let again = resolve(
        &manifest,
        "released_eglot_gnu_elpa_1_24",
        &default_request(&emacs, Some(&client_path), Some(&archive_path), &cache),
    )?;
    ensure!(again.reused_cache, "the same identity must reuse the entry");
    ensure!(again.cache_key == resolved.cache_key);
    Ok(())
}

/// The source subject materializes from the exact tree file under its
/// immutable commit pin, with no package identity anywhere.
#[test]
fn source_subject_materializes_from_the_exact_tree() -> Result<()> {
    let root = tempfile::tempdir()?;
    let emacs = fixture_emacs(root.path())?;
    let input = fixture_input_dir(root.path(), SOURCE_EGLOT_BYTES, None)?;
    let client_path = input.join("eglot.el");
    let manifest = fixture_manifest();
    let cache = root.path().join("cache");
    let resolved = resolve(
        &manifest,
        "source_eglot_emacs_c1ad9d27",
        &default_request(&emacs, Some(&client_path), None, &cache),
    )?;
    ensure!(
        resolved.client.source_state == ClientSourceState::UpstreamSource
            && resolved.client.source_ref == SOURCE_TREE_COMMIT
            && resolved.client.source_sha256 == sha256_of(SOURCE_EGLOT_BYTES)
    );
    ensure!(
        resolved.client.package_sha256.is_none(),
        "an upstream-source subject carries no package identity"
    );
    ensure!(resolved.client_package.is_none());
    let again = resolve(
        &manifest,
        "source_eglot_emacs_c1ad9d27",
        &default_request(&emacs, Some(&client_path), None, &cache),
    )?;
    ensure!(again.reused_cache && again.cache_key == resolved.cache_key);
    Ok(())
}

/// Released and source subjects never share an identity: their cache keys
/// differ, package input cannot satisfy a source subject, and omitting the
/// explicit client input is a typed unavailable disposition.
#[test]
fn released_and_source_subjects_are_not_interchangeable() -> Result<()> {
    let root = tempfile::tempdir()?;
    let emacs = fixture_emacs(root.path())?;
    let released_input =
        fixture_input_dir(root.path(), RELEASED_EGLOT_BYTES, Some(RELEASED_ARCHIVE_BYTES))?;
    let source_input = fixture_input_dir(&root.path().join("src"), SOURCE_EGLOT_BYTES, None)?;
    let manifest = fixture_manifest();
    let released = resolve(
        &manifest,
        "released_eglot_gnu_elpa_1_24",
        &default_request(
            &emacs,
            Some(&released_input.join("eglot.el")),
            Some(&released_input.join("eglot-1.24.tar")),
            &root.path().join("cache-released"),
        ),
    )?;
    let source = resolve(
        &manifest,
        "source_eglot_emacs_c1ad9d27",
        &default_request(
            &emacs,
            Some(&source_input.join("eglot.el")),
            None,
            &root.path().join("cache-source"),
        ),
    )?;
    ensure!(
        released.cache_key != source.cache_key,
        "released and source subjects must never share a cache key"
    );

    // Package input offered to a source subject is the source-as-release
    // labeling the issue forbids.
    let failure = resolve(
        &manifest,
        "source_eglot_emacs_c1ad9d27",
        &default_request(
            &emacs,
            Some(&source_input.join("eglot.el")),
            Some(&released_input.join("eglot-1.24.tar")),
            &root.path().join("cache-mix"),
        ),
    )
    .expect_err("package input cannot satisfy a source subject");
    match rejection_of(&failure) {
        SubjectRejection::IdentityMismatch { subject_id, reason } => {
            assert_eq!(subject_id, "source_eglot_emacs_c1ad9d27");
            assert!(
                reason.contains("non-interchangeable"),
                "the reason must name the non-interchangeability rule: {reason}"
            );
        }
        other => panic!("expected IdentityMismatch, got {other:?}"),
    }

    // Omitting the explicit client input is a typed unavailable
    // disposition, never an installation search.
    let failure = resolve(
        &manifest,
        "source_eglot_emacs_c1ad9d27",
        &default_request(&emacs, None, None, &root.path().join("cache-none")),
    )
    .expect_err("an external subject without an explicit input is unavailable");
    match rejection_of(&failure) {
        SubjectRejection::UnavailableSubject { subject_id, reason } => {
            assert_eq!(subject_id, "source_eglot_emacs_c1ad9d27");
            assert!(
                reason.contains("explicit exact client input"),
                "the reason must name the explicit-input rule: {reason}"
            );
        }
        other => panic!("expected UnavailableSubject, got {other:?}"),
    }
    Ok(())
}

/// Cache keys bind the archive digest, not the version alone: when a
/// re-audited row declares different archive bytes under the same version
/// string and loaded-file digest, the old cache entry is stale and the two
/// identities resolve to different keys.
#[test]
fn cache_keys_bind_the_archive_digest_not_the_version_alone() -> Result<()> {
    let root = tempfile::tempdir()?;
    let emacs = fixture_emacs(root.path())?;
    let original_input =
        fixture_input_dir(root.path(), RELEASED_EGLOT_BYTES, Some(RELEASED_ARCHIVE_BYTES))?;
    let repacked_root = root.path().join("repacked");
    let repacked_input =
        fixture_input_dir(&repacked_root, RELEASED_EGLOT_BYTES, Some(REPACKED_ARCHIVE_BYTES))?;
    let subject_id = "released_eglot_gnu_elpa_1_24";

    let original_manifest = fixture_manifest();
    let original = resolve(
        &original_manifest,
        subject_id,
        &default_request(
            &emacs,
            Some(&original_input.join("eglot.el")),
            Some(&original_input.join("eglot-1.24.tar")),
            &root.path().join("cache-shared"),
        ),
    )?;

    // The re-audited row: same version, same loaded-file digest, different
    // declared archive bytes (a repack of the same release).
    let mut repacked_manifest = fixture_manifest();
    repacked_manifest.subjects[0].external_package =
        Some(fixture_external_package(sha256_of(REPACKED_ARCHIVE_BYTES), "26.3"));
    repacked_manifest.subjects[0].digest_audit.gnu_tarball_sha256 =
        sha256_of(REPACKED_ARCHIVE_BYTES);
    repacked_manifest.validate().expect("the re-audited row must itself be a valid manifest");

    let failure = resolve(
        &repacked_manifest,
        subject_id,
        &default_request(
            &emacs,
            Some(&repacked_input.join("eglot.el")),
            Some(&repacked_input.join("eglot-1.24.tar")),
            &root.path().join("cache-shared"),
        ),
    )
    .expect_err("the immutable cache must refuse the re-audited identity under the old entry");
    match rejection_of(&failure) {
        SubjectRejection::StaleCacheEntry { subject_id, reason, .. } => {
            assert_eq!(subject_id, "released_eglot_gnu_elpa_1_24");
            assert!(
                reason.contains("different"),
                "the reason must name the identity difference: {reason}"
            );
        }
        other => panic!("expected StaleCacheEntry, got {other:?}"),
    }

    let repacked = resolve(
        &repacked_manifest,
        subject_id,
        &default_request(
            &emacs,
            Some(&repacked_input.join("eglot.el")),
            Some(&repacked_input.join("eglot-1.24.tar")),
            &root.path().join("cache-fresh"),
        ),
    )?;
    ensure!(
        original.client.version == repacked.client.version
            && original.client.source_sha256 == repacked.client.source_sha256,
        "the two rows share the version string and loaded-file digest"
    );
    ensure!(
        original.cache_key != repacked.cache_key,
        "the archive digest must be part of the cache identity"
    );
    ensure!(
        original.client.package_sha256 != repacked.client.package_sha256,
        "the two resolutions bind different package identities"
    );
    Ok(())
}

/// The checked manifest pins the implementation-time audited external
/// Eglot identities (#11745). Digests are audit facts from the official
/// GNU ELPA archive and the emacs.git source tree, re-verified at
/// implementation time.
#[test]
fn checked_manifest_pins_the_audited_external_eglot_subjects() -> Result<()> {
    let manifest = SubjectManifest::load(&workspace_root()?)?;
    manifest.validate().expect("the checked manifest with external rows must validate");
    let released = manifest.row_for("released_eglot_gnu_elpa_1_24")?;
    ensure!(
        released.client_kind == SubjectClientKind::ExternalEglot
            && released.source_state == ClientSourceState::Released
            && released.materialization == MaterializationMethod::ExplicitInput
    );
    ensure!(
        released.client_source_sha256
            == "sha256:14f16148ec7c76642be39e302d048bb06c06550e6fd9095a6cdd8e186dec1a47",
        "the released loaded-file digest is the audited archive's eglot.el digest"
    );
    let package = released
        .external_package
        .as_ref()
        .context("the released row must declare its package identity")?;
    ensure!(
        package.archive_url == "https://elpa.gnu.org/packages/eglot-1.24.tar"
            && package.archive_sha256
                == "sha256:ec91a464e63315bb7e150cafe8c73c9707b43f76ab6cbe7b09908ec1fb03213a",
        "the released row pins the exact GNU ELPA 1.24 archive bytes"
    );
    ensure!(
        package.attested_source_commit == "0d67e76b94e1f0af9fe364aed8aa5db1c494c206"
            && released.emacs_release_tag == "0d67e76b94e1f0af9fe364aed8aa5db1c494c206",
        "the released row's ref is the archive-attested source commit"
    );
    ensure!(
        package.minimum_emacs == "26.3",
        "the released row pins the audited minimum Emacs floor"
    );
    // Pin the complete audited dependency list, not a count plus one entry
    // (same review finding as the lsp-mode rows): a silently swapped
    // dependency must fail this assertion.
    let audited_requires = [
        "emacs 26.3",
        "eldoc 1.16.0",
        "external-completion 0.1",
        "flymake 1.4.5",
        "jsonrpc 1.0.29",
        "project 0.11.2",
        "seq 2.23",
        "xref 1.7.0",
    ];
    ensure!(
        package.package_requires.iter().map(String::as_str).collect::<Vec<_>>() == audited_requires,
        "the released row must pin the exact audited dependency list in audit order: {:?}",
        package.package_requires
    );
    ensure!(released.source_tree.is_none());

    let source = manifest.row_for("source_eglot_emacs_c1ad9d27")?;
    ensure!(
        source.client_kind == SubjectClientKind::ExternalEglot
            && source.source_state == ClientSourceState::UpstreamSource
            && source.materialization == MaterializationMethod::ExplicitInput
    );
    ensure!(
        source.client_source_sha256
            == "sha256:c16bf52a9a03f7e1f84f6f2668271a08418064b740a398c29b0b8c4ae50fbf9e",
        "the source loaded-file digest is the audited tree file digest"
    );
    let tree =
        source.source_tree.as_ref().context("the source row must declare its tree identity")?;
    ensure!(
        tree.commit == "c1ad9d27207aff96a22d49ae4c6cab35a2619927"
            && source.emacs_release_tag == tree.commit
            && tree.tree_sha1 == "dc5475f03a6462846d36ade5a68a2e90a2578087",
        "the source row binds one immutable commit/tree pin"
    );
    ensure!(source.external_package.is_none());
    ensure!(
        source.client_source_sha256 != released.client_source_sha256,
        "released and source subjects bind different audited bytes"
    );
    ensure!(
        source.client_version_hint == released.client_version_hint,
        "both audited files carry the same version header — the resemblance the resolver \
         refuses to treat as identity"
    );
    Ok(())
}

/// The runner registry and the checked manifest agree on the external
/// Eglot rows: both ids dispatch, tokens agree, manifest-bound external
/// rows cover each other exactly, and the legacy slice-2 row is the single
/// documented non-manifest exception.
#[test]
fn external_registry_rows_agree_with_the_manifest() -> Result<()> {
    for id in ["released_eglot_gnu_elpa_1_24", "source_eglot_emacs_c1ad9d27"] {
        let subject = EmacsClientSubject::from_id(id)?;
        ensure!(EmacsClientSubject::known_ids().contains(&id));
        ensure!(subject.resolves_through_subject_manifest());
        let manifest = SubjectManifest::load(&workspace_root()?)?;
        let row = manifest.row_for(id)?;
        ensure!(
            subject.pinned_emacs_version_token() == row.emacs_version_token,
            "registry token and manifest token disagree for {id}"
        );
    }
    ensure!(
        EmacsClientSubject::from_id("released_eglot_gnu_elpa_1_24")?.requires_client_package(),
        "the released subject carries an exact package identity"
    );
    ensure!(
        !EmacsClientSubject::from_id("source_eglot_emacs_c1ad9d27")?.requires_client_package(),
        "the source subject carries no package identity"
    );

    let manifest = SubjectManifest::load(&workspace_root()?)?;
    let registry_external: BTreeSet<String> = EmacsClientSubject::known_ids()
        .iter()
        .filter(|id| {
            EmacsClientSubject::from_id(id).is_ok_and(|subject| {
                subject.resolves_through_subject_manifest()
                    && !subject.resolves_client_source_from_installation()
            })
        })
        .map(|id| id.to_string())
        .collect();
    let manifest_external: BTreeSet<String> = manifest
        .subjects
        .iter()
        .filter(|row| row.client_kind != SubjectClientKind::BundledEglot)
        .map(|row| row.subject_id.clone())
        .collect();
    ensure!(
        registry_external == manifest_external,
        "manifest-bound external registry rows and manifest external rows must cover each other \
         exactly: registry {registry_external:?} vs manifest {manifest_external:?}"
    );
    ensure!(
        !EmacsClientSubject::from_id("released_eglot_gnu_elpa_1_23")?
            .resolves_through_subject_manifest(),
        "the slice-2 released row keeps its landed explicit-input mechanics until superseded"
    );
    Ok(())
}

/// Both external subjects round-trip through the real shared run-plan
/// boundary: the resolver's inputs feed the landed plan builder, the plan
/// validates over digest-verified files, and the plan binds the resolved
/// identities — released with its package digest, source without one. This
/// is a materialization round trip only: no journey is run and no support
/// is claimed.
#[test]
fn external_subjects_round_trip_through_the_run_plan_boundary() -> Result<()> {
    let root = tempfile::tempdir()?;
    let candidate_name = if cfg!(windows) { "perllsp.exe" } else { "perllsp" };
    let released_id = "released_eglot_gnu_elpa_1_24";
    let source_id = "source_eglot_emacs_c1ad9d27";
    for (subject_id, client_bytes, archive_bytes) in [
        (released_id, RELEASED_EGLOT_BYTES, Some(RELEASED_ARCHIVE_BYTES)),
        (source_id, SOURCE_EGLOT_BYTES, None),
    ] {
        let subject = EmacsClientSubject::from_id(subject_id)?;
        let tree = root.path().join(subject_id);
        let emacs = fixture_emacs(&tree)?;
        let input = fixture_input_dir(&tree, client_bytes, archive_bytes)?;
        let candidate = tree.join(candidate_name);
        fs::write(&candidate, b"fake exact perllsp candidate bytes")?;

        let mut manifest = fixture_manifest();
        manifest.subjects.retain(|row| row.subject_id == subject_id);
        let resolved = resolve(
            &manifest,
            subject_id,
            &default_request(
                &emacs,
                Some(&input.join("eglot.el")),
                archive_bytes.map(|_| input.join("eglot-1.24.tar")).as_deref(),
                &tree.join("cache"),
            ),
        )?;
        let run = resolved.host_run_inputs(&candidate, &tree.join("out"), 0);
        let (plan, _layout) = build_client_subject_run_plan(
            &workspace_root()?,
            subject,
            &run,
            &"0".repeat(40),
            "perllsp fake",
            "GNU Emacs 30.1 (fixture)",
            &manifest,
        )?;
        plan.validate()?;
        ensure!(plan.identity.client.client_id == subject_id);
        ensure!(
            plan.identity.client.source_sha256 == sha256_of(client_bytes),
            "the plan binds the resolved loaded-file digest"
        );
        ensure!(
            plan.identity.client.package_sha256.is_some() == archive_bytes.is_some(),
            "the plan binds the package identity exactly when the subject declares one"
        );
        ensure!(
            plan.identity.emacs_build_sha256 == resolved.emacs_build_sha256,
            "the plan binds the same executable digest the resolver bound"
        );
    }
    Ok(())
}

/// The run-plan boundary itself refuses repacked archive bytes: the typed
/// rejection reaches the plan builder and leaves no run output state
/// behind.
#[test]
fn run_plan_boundary_rejects_a_repacked_archive() -> Result<()> {
    let root = tempfile::tempdir()?;
    let tree = root.path().join("tree");
    let emacs = fixture_emacs(&tree)?;
    let input = fixture_input_dir(&tree, RELEASED_EGLOT_BYTES, Some(REPACKED_ARCHIVE_BYTES))?;
    let candidate_name = if cfg!(windows) { "perllsp.exe" } else { "perllsp" };
    let candidate = tree.join(candidate_name);
    fs::write(&candidate, b"fake exact perllsp candidate bytes")?;
    let out_root = tree.join("out");
    let run = emacs_host_run::EmacsHostRunInputs {
        emacs_executable: emacs,
        candidate_executable: candidate,
        client_source: input.join("eglot.el"),
        client_package: Some(input.join("eglot-1.24.tar")),
        out_root: out_root.clone(),
        timeout_ms: 0,
    };
    let error = build_client_subject_run_plan(
        &workspace_root()?,
        EmacsClientSubject::from_id("released_eglot_gnu_elpa_1_24")?,
        &run,
        &"0".repeat(40),
        "perllsp fake",
        "GNU Emacs 30.1 (fixture)",
        &fixture_manifest(),
    )
    .err()
    .context("repacked archive bytes must not produce a released-subject plan")?;
    assert!(
        error.to_string().contains("identity mismatch"),
        "the typed rejection must reach the plan boundary: {error}"
    );
    ensure!(
        !out_root.exists(),
        "a rejected resolution must leave no run output state behind: {}",
        out_root.display()
    );
    Ok(())
}

/// A change in the declared external identity that leaves the client bytes
/// and commit untouched — a corrected tree object id, a dependency entry —
/// still makes the old cache entry stale: every declared external fact
/// keys the cache, not just the archive bytes (review finding on the
/// initial candidate).
#[test]
fn declared_external_identity_changes_make_the_cache_entry_stale() -> Result<()> {
    let root = tempfile::tempdir()?;
    let emacs = fixture_emacs(root.path())?;
    let input = fixture_input_dir(root.path(), SOURCE_EGLOT_BYTES, None)?;
    let client_path = input.join("eglot.el");
    let manifest = fixture_manifest();
    let cache = root.path().join("cache");
    resolve(
        &manifest,
        "source_eglot_emacs_c1ad9d27",
        &default_request(&emacs, Some(&client_path), None, &cache),
    )?;

    // The re-audited row corrects the tree object id; the commit and file
    // digest stay identical, so only the declared external identity moved.
    let mut corrected_tree = fixture_manifest();
    corrected_tree.subjects[1].source_tree = Some(SourceTreeIdentity {
        source_repo_url: "https://github.com/emacs-mirror/emacs".to_string(),
        commit: SOURCE_TREE_COMMIT.to_string(),
        tree_sha1: "aa55d5f03a6462846d36ade5a68a2e90a2578087".to_string(),
    });
    corrected_tree.validate().expect("the corrected tree row must itself be valid");
    let failure = resolve(
        &corrected_tree,
        "source_eglot_emacs_c1ad9d27",
        &default_request(&emacs, Some(&client_path), None, &cache),
    )
    .expect_err("a changed declared external identity must refuse the old entry");
    match rejection_of(&failure) {
        SubjectRejection::StaleCacheEntry { subject_id, reason, .. } => {
            assert_eq!(subject_id, "source_eglot_emacs_c1ad9d27");
            assert!(
                reason.contains("different declared external identity"),
                "the reason must name the external-identity difference: {reason}"
            );
        }
        other => panic!("expected StaleCacheEntry, got {other:?}"),
    }
    Ok(())
}

/// A source-subject launch proceeds past the host-run adapter boundary
/// (#8776's external adapter services the subject package-free): the run
/// no longer refuses with the missing-adapter reason and instead fails on
/// the next typed boundary — the candidate commit identity of the
/// nonexistent repository — never skipping past exact-input validation.
#[test]
fn source_subject_launch_proceeds_past_the_adapter_boundary() {
    let subject = EmacsClientSubject::from_id("source_eglot_emacs_c1ad9d27").expect("registry row");
    let run = emacs_host_run::EmacsHostRunInputs {
        emacs_executable: PathBuf::from("/nonexistent/emacs"),
        candidate_executable: PathBuf::from("/nonexistent/perllsp"),
        client_source: PathBuf::from("/nonexistent/eglot.el"),
        client_package: None,
        out_root: PathBuf::from("/nonexistent/out"),
        timeout_ms: 0,
    };
    let error = emacs_host_run::host_run(Path::new("/nonexistent/repo"), subject, &run)
        .err()
        .expect("the nonexistent exact inputs must still fail the run");
    assert!(
        !error.to_string().contains("no driver adapter yet"),
        "the source subject no longer refuses at the adapter boundary: {error}"
    );
    assert!(
        error.to_string().contains("candidate identity"),
        "the failure must come from the next typed boundary (commit identity probe): {error}"
    );
}
